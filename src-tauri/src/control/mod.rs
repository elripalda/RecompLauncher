//! The local control API: lets a script or an AI tool (through MCP) see which games are installed
//! and running, and start one with Play. That's all it does. It's off until the player turns it
//! on in Settings, and turning it off stops it at once.
//!
//! - It listens on 127.0.0.1 only, on a random port, with a random bearer token. Both go in
//!   `<data dir>/control-api/endpoint.json`, which only this user can read (see [`private`]); the
//!   token is never logged. Where that file can't be made private, the API doesn't start.
//! - Requests from web pages are refused, and sizes, connections and waits are bounded ([`http`]).
//! - What it can do is the [`Launcher`] trait: status and Play. There is no general command
//!   dispatch behind it, so neither HTTP nor MCP can reach anything else.
//!
//! See docs/control-api.md.

pub mod http;
pub mod mcp;
pub mod private;
mod server;

use http::ApiError;
use serde_json::{json, Value};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{watch, Semaphore};

pub const API_VERSION: &str = "1";
pub const ENDPOINT_SCHEMA: &str = "recomplauncher-control-api/1";
pub const MAX_CONNECTIONS: usize = 8;
/// Longest a game id can be. Real ones are far shorter.
pub const MAX_ID_LEN: usize = 200;

/// What Play answers with, in time.
pub type PlayResult = Pin<Box<dyn Future<Output = Result<Value, ApiError>> + Send>>;

/// Everything the API can do in the launcher.
pub trait Launcher: Send + Sync + 'static {
    /// The installed games and which are running. Read only.
    fn status(&self) -> Value;
    /// Starts an installed game that's ready to play, as its desktop shortcut does, but never
    /// runs a setup or an installer. `id` has passed [`valid_id`]. `off` is this API's switch:
    /// the game's actual start goes through [`Off::launch`], so nothing starts once it's off.
    /// Cancel safe waits through [`Off::while_on`]. Once save mutation begins, finish it before
    /// returning: shutdown keeps this future alive so its locks and start claim stay held.
    fn play(&self, id: String, off: Off) -> PlayResult;
}

/// One running API's off switch, and the gate every game it starts goes through. Each start of
/// the API has its own, so a Play still getting ready when it went off doesn't start a game later,
/// even once it's on again.
///
/// Turning it off and starting a game take the same lock: once [`Off::turn_off`] returns, no game
/// can start through [`Off::launch`], even from a Play stuck in work that can't be interrupted
/// (unpacking a game, say). Only the start itself is held under it, never the work before.
#[derive(Clone, Default)]
pub struct Off {
    gate: Arc<Mutex<bool>>,
    changed: Arc<tokio::sync::Notify>,
}

impl Off {
    pub fn is_off(&self) -> bool {
        *self.lock()
    }

    /// Waits for a game being started right now (only the start, a moment), then refuses any more.
    pub fn turn_off(&self) {
        *self.lock() = true;
        self.changed.notify_waiters();
    }

    /// Only for waits that can safely be dropped. Cloud sync must finish instead: its blocking
    /// restore keeps changing saves even if the async future waiting on it is dropped.
    pub async fn while_on<F: Future>(&self, safe: F) -> Option<F::Output> {
        let changed = self.changed.notified();
        tokio::pin!(changed);
        // Register before checking the gate, including when shutdown races this first poll.
        changed.as_mut().enable();
        if self.is_off() {
            return None;
        }
        tokio::select! {
            biased;
            _ = changed => None,
            value = safe => Some(value),
        }
    }

    /// `start` (the game's actual start), unless it's off: then None, and nothing is run.
    pub fn launch<T>(&self, start: impl FnOnce() -> T) -> Option<T> {
        let off = self.lock();
        if *off {
            return None;
        }
        Some(start())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, bool> {
        // A start that panicked leaves the switch as it was.
        self.gate.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub fn control_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("control-api")
}

pub fn endpoint_file(data_dir: &Path) -> PathBuf {
    control_dir(data_dir).join("endpoint.json")
}

/// A game id worth looking up: not empty, not huge, no control characters. Whether it's an
/// installed game is the launcher's to say.
pub fn valid_id(id: &str) -> Result<(), ApiError> {
    if id.trim().is_empty() || id.len() > MAX_ID_LEN || id.chars().any(char::is_control) {
        return Err(ApiError::new(400, "invalid_id", "`id` must be the id of an installed game (see status)."));
    }
    Ok(())
}

/// Shared by every connection of one running API.
pub(crate) struct Ctx {
    pub port: u16,
    token: String,
    pub launcher: Arc<dyn Launcher>,
    pub app_version: String,
    pub conns: Arc<Semaphore>,
    /// Set when the API is turned off: requests still in the air are refused.
    pub off: Off,
}

impl Ctx {
    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn stopped(&self) -> bool {
        self.off.is_off()
    }

    pub async fn status(&self) -> Result<Value, ApiError> {
        let launcher = self.launcher.clone();
        // It reads the Library and catalog from disk: kept off the server's own threads.
        let status = tokio::task::spawn_blocking(move || launcher.status()).await;
        let mut v = status.map_err(|_| ApiError::new(500, "internal", "Reading the status stopped unexpectedly."))?;
        v["ok"] = json!(true);
        v["api_version"] = json!(API_VERSION);
        Ok(v)
    }

    pub async fn play(&self, id: &str) -> Result<Value, ApiError> {
        valid_id(id)?;
        if self.stopped() {
            return Err(ApiError::new(503, "stopped", "The control API was turned off."));
        }
        self.launcher.play(id.to_string(), self.off.clone()).await
    }
}

/// A running API. Dropping it doesn't stop it: call [`Server::stop`].
pub struct Server {
    pub port: u16,
    pub endpoint: PathBuf,
    ctx: Arc<Ctx>,
    stop: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

/// Starts the API: listens, writes the endpoint file, then serves. Must run inside the async
/// runtime. Err: it isn't running, and no endpoint file was left.
pub async fn start(data_dir: &Path, app_version: &str, launcher: Arc<dyn Launcher>) -> Result<Server, String> {
    let dir = control_dir(data_dir);
    std::fs::create_dir_all(data_dir).map_err(|e| format!("can't make {}: {e}", data_dir.display()))?;
    private::private_dir(&dir).map_err(|e| format!("can't make {} private to this user: {e}", dir.display()))?;
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await.map_err(|e| format!("can't listen on 127.0.0.1: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let token = new_token()?;
    let endpoint = endpoint_file(data_dir);
    let info = json!({
        "schema": ENDPOINT_SCHEMA,
        "api_version": API_VERSION,
        "url": format!("http://127.0.0.1:{port}"),
        "mcp_url": format!("http://127.0.0.1:{port}/mcp"),
        "port": port,
        "token": token,
        "pid": std::process::id(),
        "app_version": app_version,
    });
    let bytes = serde_json::to_vec_pretty(&info).unwrap_or_default();
    private::write_private(&endpoint, &bytes).map_err(|e| format!("can't write {} privately: {e}", endpoint.display()))?;
    let ctx = Arc::new(Ctx {
        port,
        token,
        launcher,
        app_version: app_version.to_string(),
        conns: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
        off: Off::default(),
    });
    let (stop, stopping) = watch::channel(false);
    let task = tokio::spawn(server::serve(ctx.clone(), listener, stopping));
    Ok(Server { port, endpoint, ctx, stop, task })
}

impl Server {
    /// Stops taking connections, refuses requests in the air, and removes the endpoint file
    /// (only if it's still this launcher's). Returns at once; [`Server::stop`] also waits for the
    /// server to end.
    pub fn halt(&self) {
        self.ctx.off.turn_off();
        let _ = self.stop.send(true);
        remove_endpoint_if_mine(&self.endpoint);
    }

    /// [`Server::halt`], then waits (briefly) for the server to end: the port closed, and every
    /// request ended. Safe preparation waits (a shader download) are cancelled; cloud sync keeps
    /// its locks and start claim until any blocking save restore finishes. Unfinished work is
    /// handed back, still tracked, as [`Ending`]. It can't start its game any more
    /// (see [`Off`]). A game that's running keeps running.
    pub async fn stop(self) -> Result<(), Ending> {
        self.stop_within(STOP_WAIT).await
    }

    pub async fn stop_within(self, wait: Duration) -> Result<(), Ending> {
        self.halt();
        Ending(self.task).wait(wait).await
    }
}

/// How long turning the API off waits for its requests to end.
pub const STOP_WAIT: Duration = Duration::from_secs(5);

/// A stopped API whose requests hadn't all ended yet: none of them can start a game, but one is
/// still finishing what it was doing.
pub struct Ending(tokio::task::JoinHandle<()>);

impl Ending {
    pub fn ended(&self) -> bool {
        self.0.is_finished()
    }

    /// Something to ask later whether it ended (`is_finished`), for whoever can't hold this.
    pub fn tracker(&self) -> tokio::task::AbortHandle {
        self.0.abort_handle()
    }

    /// Waits up to `wait` for it to end. Err: still going, and still tracked.
    pub async fn wait(mut self, wait: Duration) -> Result<(), Ending> {
        match tokio::time::timeout(wait, &mut self.0).await {
            Ok(_) => Ok(()),
            Err(_) => Err(self),
        }
    }
}

fn remove_endpoint_if_mine(endpoint: &Path) {
    let read = std::fs::read(endpoint).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    let pid = read.and_then(|v| v.get("pid").and_then(Value::as_u64));
    if pid == Some(std::process::id() as u64) {
        let _ = std::fs::remove_file(endpoint);
    }
}

fn new_token() -> Result<String, String> {
    let mut b = [0u8; 32];
    getrandom::getrandom(&mut b).map_err(|e| format!("no randomness for the token: {e}"))?;
    Ok(hex::encode(b))
}

#[cfg(test)]
pub(crate) mod tests;

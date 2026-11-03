//! The local control API: lets scripts and agents (through MCP) run the launcher's own commands
//! in the running app, with no window clicking. It's off unless asked for (see [`wanted`]).
//!
//! - Listens on 127.0.0.1 only, on a random port, with a random bearer token. Both go in
//!   `<data dir>/control-api/endpoint.json` (mode 0600, folder 0700); the token is never logged.
//!   Only on Unix (macOS, Linux): on Windows the file can't yet be limited to this user (portable
//!   data folders may be shared), so the API refuses to start there ([`platform_supported`]).
//! - Requests from web pages are refused (Host must be the loopback address, no Origin or
//!   Sec-Fetch-* headers, JSON bodies only), sizes, connections and waits are bounded.
//! - Commands run through Tauri's own IPC pipeline in this process ([`bridge`]), so the API covers
//!   exactly the commands registered with `generate_handler!`, with their real state.
//! - `POST /mcp` speaks MCP (JSON-RPC); `tools/control-api/recomplauncher_mcp.py` carries it
//!   over stdio for MCP clients.
//!
//! See docs/control-api.md.

pub mod bridge;
pub mod http;
pub mod jobs;
pub mod mcp;
pub mod registry;
mod server;

use http::ApiError;
use jobs::{EventLog, JobState, JobTable};
use registry::Registry;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Listener, Manager, Runtime};
use tokio::sync::{Notify, Semaphore};

pub const API_VERSION: &str = "1";
pub const ENDPOINT_SCHEMA: &str = "recomplauncher-control-api/1";
/// Command-line switch that turns the API on (also handed to a running launcher).
pub const ARG: &str = "--control-api";
/// Environment variable: 1/on turns the API on, 0/off keeps it off even when enabled.
pub const ENV: &str = "RECOMPLAUNCHER_CONTROL_API";
/// Environment variable: a fixed port instead of a random one.
pub const ENV_PORT: &str = "RECOMPLAUNCHER_CONTROL_PORT";
/// Told to the window after a command that changes something succeeds, so it reads the state again.
pub const CHANGED_EVENT: &str = "control-api-changed";
pub const MAX_CONNECTIONS: usize = 16;
pub const DEFAULT_WAIT_MS: u64 = 30_000;
pub const MAX_WAIT_MS: u64 = 600_000;
pub const MAX_EVENT_WAIT_MS: u64 = 30_000;
/// The app's command that changes some settings atomically (merge patch under its own lock).
pub const SETTINGS_PATCH: &str = "patch_settings";

pub struct Options<R: Runtime> {
    /// The launcher's data folder (the endpoint file goes in `control-api/` there).
    pub data_dir: PathBuf,
    pub registry: &'static Registry,
    /// The window whose webview commands are invoked through.
    pub label: &'static str,
    pub app_version: &'static str,
    /// 0: any free port.
    pub port: u16,
    pub show: fn(&AppHandle<R>),
    pub hide: fn(&AppHandle<R>),
    pub log: fn(String),
}

pub fn control_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("control-api")
}

pub fn endpoint_file(data_dir: &Path) -> PathBuf {
    control_dir(data_dir).join("endpoint.json")
}

/// What the environment variable's value says: Some(false) keeps the API off whatever else asks
/// for it, Some(true) turns it on, None leaves it to the switch and the `enabled` file.
pub fn env_choice(value: Option<&str>) -> Option<bool> {
    match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("0" | "off" | "false" | "no") => Some(false),
        Some("1" | "on" | "true" | "yes") => Some(true),
        _ => None,
    }
}

/// The environment variable's value in this process.
pub fn env_value() -> Option<String> {
    std::env::var(ENV).ok()
}

/// Whether the API should run: the environment variable says so either way, else the
/// `--control-api` switch or an `enabled` file in the control-api folder turns it on.
pub fn wanted(data_dir: &Path, args: &[String]) -> bool {
    wanted_with(env_value().as_deref(), data_dir, args)
}

/// [`wanted`] with the environment variable's value given.
pub fn wanted_with(env: Option<&str>, data_dir: &Path, args: &[String]) -> bool {
    match env_choice(env) {
        Some(on) => on,
        None => args.iter().any(|a| a == ARG) || control_dir(data_dir).join("enabled").is_file(),
    }
}

/// Whether this platform can keep the endpoint file, and so the token, readable by this user only.
/// Unix: yes (0600 file in a 0700 folder). Windows: not implemented (no private ACL is set), and a
/// portable data folder may be on a shared drive, so the API stays off rather than leak the token.
pub fn platform_supported() -> Result<(), String> {
    if cfg!(unix) {
        Ok(())
    } else {
        Err("the control API isn't available on this platform: its endpoint file (with the token) can't be made private to this user here".into())
    }
}

pub fn port_from_env() -> u16 {
    std::env::var(ENV_PORT).ok().and_then(|p| p.trim().parse().ok()).unwrap_or(0)
}

/// The running API, kept in the app's state.
pub struct Ctl<R: Runtime> {
    pub app: AppHandle<R>,
    pub opts: Options<R>,
    pub port: u16,
    token: String,
    endpoint: PathBuf,
    pub started_ms: u64,
    events: Mutex<EventLog>,
    event_ping: Notify,
    jobs: Mutex<JobTable>,
    job_ping: Notify,
    conns: Arc<Semaphore>,
    reconciling: AtomicBool,
    closed: AtomicBool,
    pub auth_failures: AtomicU64,
}

static START: Mutex<()> = Mutex::new(());

/// Starts the API when asked for at run time (the `--control-api` switch handed to the launcher
/// already running, say), unless the environment variable keeps it off: `env` is its value
/// ([`env_value`]). Ok(None): kept off.
pub fn start_requested<R: Runtime>(app: &AppHandle<R>, opts: Options<R>, env: Option<&str>) -> Result<Option<Arc<Ctl<R>>>, String> {
    if env_choice(env) == Some(false) {
        (opts.log)(format!("Control API asked for, but {ENV}={} keeps it off.", env.unwrap_or_default().trim()));
        return Ok(None);
    }
    start(app, opts).map(Some)
}

/// Starts the API (once; a second call returns the one running).
pub fn start<R: Runtime>(app: &AppHandle<R>, opts: Options<R>) -> Result<Arc<Ctl<R>>, String> {
    platform_supported()?;
    let _one = START.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(c) = app.try_state::<Arc<Ctl<R>>>() {
        return Ok(c.inner().clone());
    }
    let dir = control_dir(&opts.data_dir);
    private_dir(&dir).map_err(|e| format!("can't prepare {}: {e}", dir.display()))?;
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, opts.port)).map_err(|e| format!("can't listen on 127.0.0.1:{}: {e}", opts.port))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let token = new_token()?;
    let endpoint = endpoint_file(&opts.data_dir);
    let ctl = Arc::new(Ctl {
        app: app.clone(),
        port,
        token,
        endpoint: endpoint.clone(),
        started_ms: jobs::now_ms(),
        events: Mutex::default(),
        event_ping: Notify::new(),
        jobs: Mutex::default(),
        job_ping: Notify::new(),
        conns: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
        reconciling: AtomicBool::new(false),
        closed: AtomicBool::new(false),
        auth_failures: AtomicU64::new(0),
        opts,
    });
    let info = json!({
        "schema": ENDPOINT_SCHEMA,
        "api_version": API_VERSION,
        "url": format!("http://127.0.0.1:{port}"),
        "mcp_url": format!("http://127.0.0.1:{port}/mcp"),
        "port": port,
        "token": ctl.token,
        "pid": std::process::id(),
        "app_version": ctl.opts.app_version,
        "identifier": app.config().identifier,
        "data_dir": ctl.opts.data_dir,
        "started_ms": ctl.started_ms,
    });
    write_private(&endpoint, &serde_json::to_vec_pretty(&info).unwrap_or_default()).map_err(|e| format!("can't write {}: {e}", endpoint.display()))?;
    for name in ctl.opts.registry.listened_events() {
        let c = ctl.clone();
        app.listen_any(name, move |e| c.on_event(name, e.payload()));
    }
    app.manage(ctl.clone());
    let c = ctl.clone();
    tauri::async_runtime::spawn(async move {
        match tokio::net::TcpListener::from_std(listener) {
            Ok(l) => server::serve(c, l).await,
            Err(e) => (c.opts.log)(format!("Control API couldn't listen: {e}")),
        }
    });
    (ctl.opts.log)(format!("Control API on http://127.0.0.1:{port}; endpoint file {}", endpoint.display()));
    Ok(ctl)
}

/// At exit: the endpoint file goes (only if it's still this process's).
pub fn stop<R: Runtime>(app: &AppHandle<R>) {
    if let Some(c) = app.try_state::<Arc<Ctl<R>>>() {
        c.closed.store(true, Ordering::Relaxed);
        let mine = std::fs::read(&c.endpoint).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|v| v.get("pid").and_then(Value::as_u64)) == Some(std::process::id() as u64);
        if mine {
            let _ = std::fs::remove_file(&c.endpoint);
        }
    }
}

fn new_token() -> Result<String, String> {
    let mut b = [0u8; 32];
    getrandom::getrandom(&mut b).map_err(|e| format!("no randomness for the token: {e}"))?;
    Ok(hex::encode(b))
}

/// A folder only this user can enter (and not a link to somewhere else).
fn private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let m = std::fs::symlink_metadata(dir)?;
    if !m.is_dir() || m.file_type().is_symlink() {
        return Err(std::io::Error::other("not a plain folder"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Written to a new file only this user can read, then moved into place. Unix only: elsewhere
/// it refuses rather than write the token where others may read it.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if !cfg!(unix) {
        return Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "no private file permissions on this platform"));
    }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let mut f = o.open(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&tmp, path)
}

/// How a finished (or not) job answers a call: HTTP status and body.
pub fn invoke_reply(job: &Value) -> (u16, Value) {
    match job.get("state").and_then(Value::as_str).unwrap_or("") {
        "succeeded" => (200, json!({"ok": true, "job_id": job["id"], "state": "succeeded", "result": job.get("result").cloned().unwrap_or(Value::Null), "job": job})),
        "failed" | "cancelled" => {
            let err = job.get("error").cloned().unwrap_or_else(|| json!({"code": "cancelled", "message": "The job was cancelled."}));
            let status = match err.get("code").and_then(Value::as_str).unwrap_or("") {
                "invalid_args" => 400,
                "unknown_command" => 404,
                "forbidden" => 403,
                "unavailable" => 503,
                "cancelled" => 409,
                _ => 422,
            };
            (status, json!({"ok": false, "job_id": job["id"], "state": job["state"], "error": err, "job": job}))
        }
        _ => (202, json!({"ok": true, "job_id": job["id"], "state": job["state"], "finished": false, "job": job, "hint": "Still going: GET /v1/jobs/{id}?wait_ms=..., events at /v1/events."})),
    }
}

impl<R: Runtime> Ctl<R> {
    pub fn token_matches(&self, t: &str) -> bool {
        http::same(t.as_bytes(), self.token.as_bytes())
    }

    pub fn closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }

    pub(crate) fn token(&self) -> &str {
        &self.token
    }

    pub(crate) fn conns(&self) -> Arc<Semaphore> {
        self.conns.clone()
    }

    fn on_event(&self, name: &str, payload: &str) {
        let rec = self.events.lock().unwrap().push(name, payload);
        let (changed, ended) = self.jobs.lock().unwrap().on_event_ended(&rec);
        self.event_ping.notify_waiters();
        if changed {
            self.job_ping.notify_waiters();
        }
        // Background work started through the API ended with its closing event (a setup's done,
        // error or cancelled, an install's): what it changed is only in place now, so the window
        // is told again, as it was when the command answered.
        for (job, command, state) in ended {
            if !self.opts.registry.note(&command).read_only {
                let _ = self.app.emit(CHANGED_EVENT, json!({"command": command, "job": job, "state": state.as_str()}));
            }
        }
    }

    /// Runs a command through the bridge, outside the job list (for the API's own reads).
    pub async fn call_internal(&self, name: &str, args: Value) -> Result<Value, Value> {
        let (app, label, n) = (self.app.clone(), self.opts.label, name.to_string());
        let started = tauri::async_runtime::spawn_blocking(move || bridge::dispatch(&app, label, &n, args)).await;
        match started {
            Ok(Ok(rx)) => match tokio::time::timeout(Duration::from_millis(DEFAULT_WAIT_MS), rx).await {
                Ok(Ok(a)) => a.map_err(|e| bridge::classify(name, e)),
                Ok(Err(_)) => Err(json!({"code": "no_answer", "message": "The command ended without answering.", "command": name})),
                Err(_) => Err(json!({"code": "timeout", "message": "The command didn't answer in time.", "command": name})),
            },
            Ok(Err(e)) => Err(e.body()),
            Err(_) => Err(json!({"code": "internal", "message": "The command stopped unexpectedly.", "command": name})),
        }
    }

    /// Calls a registered command as a job. `wait_ms` 0: answer at once with the job; otherwise
    /// wait up to that long for it to finish.
    pub async fn invoke(self: &Arc<Self>, name: &str, args: Value, wait_ms: u64, validate: bool) -> Result<Value, ApiError> {
        let reg = self.opts.registry;
        let spec = reg.find(name).ok_or_else(|| ApiError::new(404, "unknown_command", format!("There's no command `{name}`.")).with(json!({"hint": "List them with GET /v1/commands or the rlapi_list_commands tool."})))?;
        let args = registry::normalize_args(spec, args, validate).map_err(|(m, d)| ApiError::new(400, "invalid_args", m).with(d))?;
        let note = reg.note(spec.name);
        // Events from here on may be this job's (see JobTable::returned).
        let since = self.events.lock().unwrap().last_seq();
        let id = self.jobs.lock().unwrap().create(spec.name, &args, note.track, note.cancel, since).map_err(|_| ApiError::new(429, "too_many_jobs", format!("{} calls are already unfinished; wait for some to end.", jobs::MAX_ACTIVE_JOBS)))?;
        let me = self.clone();
        let (job, cmd, read_only) = (id.clone(), spec.name, note.read_only);
        let background = note.track.map(|t| t.completes_on_event && !t.active_in.is_empty()).unwrap_or(false);
        tauri::async_runtime::spawn(async move {
            let (app, label) = (me.app.clone(), me.opts.label);
            let started = tauri::async_runtime::spawn_blocking(move || bridge::dispatch(&app, label, cmd, args)).await;
            let answer = match started {
                Ok(Ok(rx)) => match rx.await {
                    Ok(a) => a.map_err(|e| bridge::classify(cmd, e)),
                    Err(_) => Err(json!({"code": "no_answer", "message": "The command ended without answering.", "command": cmd})),
                },
                Ok(Err(e)) => Err(e.body()),
                Err(_) => Err(json!({"code": "internal", "message": "The command stopped unexpectedly.", "command": cmd})),
            };
            let ok = answer.is_ok();
            let deferred = {
                // Under the job table's lock, so no event falls between the ones already kept
                // (replayed here) and the ones on_event brings from now on, and a cancel asked
                // for meanwhile (cancel_job, same lock) is either seen here or sent by cancel_job.
                let mut table = me.jobs.lock().unwrap();
                let backlog = match note.track {
                    Some(t) if !t.run_field.is_empty() => {
                        let log = me.events.lock().unwrap();
                        let upto = log.last_seq();
                        (log.after(since, &[t.event.to_string()], jobs::MAX_EVENTS).0, upto)
                    }
                    _ => (vec![], 0),
                };
                table.returned(&job, answer, &backlog.0, backlog.1)
            };
            me.job_ping.notify_waiters();
            // A cancel asked for before the command answered: sent now that the work has started,
            // bound to the run it answered with.
            if let Some((c, args)) = deferred {
                let _ = me.send_cancel(&job, c, args).await;
            }
            if ok && !read_only {
                let _ = me.app.emit(CHANGED_EVENT, json!({"command": cmd}));
            }
            if ok && background {
                me.reconcile_soon();
            }
        });
        if wait_ms > 0 {
            self.wait_job(&id, wait_ms.min(MAX_WAIT_MS)).await;
        }
        self.job(&id).ok_or_else(|| ApiError::new(500, "internal", "The job went missing."))
    }

    pub fn job(&self, id: &str) -> Option<Value> {
        self.jobs.lock().unwrap().get(id).map(|j| j.to_json())
    }

    pub fn jobs(&self) -> Value {
        let t = self.jobs.lock().unwrap();
        json!({"ok": true, "active": t.active(), "jobs": t.list()})
    }

    fn job_finished(&self, id: &str) -> bool {
        self.jobs.lock().unwrap().get(id).map(|j| j.state.finished()).unwrap_or(true)
    }

    pub async fn wait_job(&self, id: &str, wait_ms: u64) {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(wait_ms);
        loop {
            let ping = self.job_ping.notified();
            tokio::pin!(ping);
            ping.as_mut().enable();
            if self.job_finished(id) {
                return;
            }
            if tokio::time::timeout_at(deadline, ping).await.is_err() {
                return;
            }
        }
    }

    /// Cancels a job through the app's own cancel command for it. Before the job's command has
    /// answered, its work may not have started yet (the command can be waiting for a lock): a
    /// cancel sent then could find nothing to stop, and the work would start after it. So the
    /// cancel is kept on the job (`cancelling`, answer `pending: true`) and sent the moment the
    /// command answers, bound to the run it names (see JobTable::returned). Once it has answered,
    /// the cancel is sent now. Either way the job ends with the work's own closing event (or at
    /// once for cancels that have none).
    pub async fn cancel_job(&self, id: &str) -> Result<Value, ApiError> {
        let (c, args) = {
            let mut t = self.jobs.lock().unwrap();
            let j = t.get_mut(id).ok_or_else(|| ApiError::new(404, "job_not_found", format!("No job `{id}` (finished jobs are kept for a while, then dropped).")))?;
            if j.state.finished() {
                return Err(ApiError::new(409, "already_finished", "The job has already finished.").with(j.to_json()));
            }
            let Some(c) = j.cancel else {
                return Err(ApiError::new(409, "not_cancellable", format!("`{}` has no cancel in the launcher; it ends by itself.", j.command)).with(json!({"cancellable": self.opts.registry.cancels.iter().map(|(n, _)| *n).collect::<Vec<_>>()})));
            };
            if j.cancel_requested {
                // Already asked for (and waiting for the answer, or sent).
                return Ok(json!({"ok": true, "pending": !j.returned, "job": j.to_json()}));
            }
            j.cancel_requested = true;
            j.state = JobState::Cancelling;
            j.updated_ms = jobs::now_ms();
            if !j.returned {
                j.note = Some(format!("Cancel asked before `{}` answered: it's sent through `{}` as soon as it does (its work may not have started yet).", j.command, c.command));
                let v = json!({"ok": true, "pending": true, "job": j.to_json()});
                drop(t);
                self.job_ping.notify_waiters();
                return Ok(v);
            }
            match jobs::cancel_args(&c, j) {
                Ok(args) => (c, args),
                Err(why) => {
                    j.cancel_failed(why.clone());
                    return Err(ApiError::new(409, "cancel_failed", why).with(j.to_json()));
                }
            }
        };
        self.send_cancel(id, c, args).await?;
        Ok(json!({"ok": true, "pending": false, "job": self.job(id)}))
    }

    /// Sends a job's cancel through the app's command. Taken: the job is `cancelling` until the
    /// work's closing event (or `cancelled` at once when the cancel has none). Refused: the job
    /// carries on as it was.
    async fn send_cancel(&self, id: &str, c: registry::Cancel, args: Value) -> Result<(), ApiError> {
        let r = self.call_internal(c.command, args).await;
        let out = {
            let mut t = self.jobs.lock().unwrap();
            match r {
                Ok(_) => {
                    t.mark(id, if c.immediate { JobState::Cancelled } else { JobState::Cancelling }, Some(format!("Cancel asked through `{}`.", c.command)));
                    Ok(())
                }
                Err(e) => {
                    if let Some(j) = t.get_mut(id) {
                        j.cancel_failed(format!("`{}` didn't take the cancel.", c.command));
                    }
                    Err(ApiError::new(409, "cancel_failed", "The launcher didn't take the cancel.").with(e))
                }
            }
        };
        self.job_ping.notify_waiters();
        out
    }

    /// Background work the app may finish without a closing event: checked against get_state.
    fn reconcile_soon(self: &Arc<Self>) {
        if self.reconciling.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(2)).await;
                let todo = me.jobs.lock().unwrap().to_reconcile(3000);
                let waiting = me.jobs.lock().unwrap().list().iter().any(|j| j["finished"] == false && j["completes_on_event"] == true);
                if todo.is_empty() {
                    if !waiting || me.closed() {
                        me.reconciling.store(false, Ordering::SeqCst);
                        return;
                    }
                    continue;
                }
                let Ok(state) = me.call_internal("get_state", json!({})).await else { continue };
                let mut changed = false;
                for (id, field, key) in todo {
                    let live = state.get(field).and_then(Value::as_array).map(|a| a.iter().any(|v| v.as_str() == Some(key.as_str()))).unwrap_or(true);
                    if !live {
                        me.jobs.lock().unwrap().mark(&id, JobState::Succeeded, Some(format!("Finished without a closing event: no longer in get_state.{field}.")));
                        changed = true;
                    }
                }
                if changed {
                    me.job_ping.notify_waiters();
                }
            }
        });
    }

    /// Events after `after` (waiting up to `wait_ms` for one if there are none yet).
    pub async fn events(&self, after: u64, names: &[String], limit: usize, wait_ms: u64) -> Value {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(wait_ms.min(MAX_EVENT_WAIT_MS));
        loop {
            let ping = self.event_ping.notified();
            tokio::pin!(ping);
            ping.as_mut().enable();
            let (list, oldest, last) = {
                let log = self.events.lock().unwrap();
                let (l, o) = log.after(after, names, limit.clamp(1, 500));
                (l, o, log.last_seq())
            };
            if !list.is_empty() || tokio::time::Instant::now() >= deadline {
                let next = list.last().map(|e| e.seq).unwrap_or(last.max(after));
                return json!({
                    "ok": true,
                    "events": list.iter().map(|e| e.to_json()).collect::<Vec<_>>(),
                    "next_after": next,
                    "last_seq": last,
                    "oldest_kept": oldest,
                    "missed_some": after > 0 && after + 1 < oldest,
                    "names": self.opts.registry.listened_events(),
                });
            }
            if tokio::time::timeout_at(deadline, ping).await.is_err() {
                continue;
            }
        }
    }

    pub fn window(&self) -> Value {
        match self.app.get_webview_window(self.opts.label) {
            Some(w) => json!({
                "label": self.opts.label,
                "visible": w.is_visible().ok(),
                "focused": w.is_focused().ok(),
                "minimized": w.is_minimized().ok(),
                "fullscreen": w.is_fullscreen().ok(),
                "url": w.url().ok().map(|u| u.to_string()),
            }),
            None => json!({"label": self.opts.label, "present": false}),
        }
    }

    pub async fn status(&self) -> Value {
        let reg = self.opts.registry;
        let mut v = json!({
            "ok": true,
            "api_version": API_VERSION,
            "app": {
                "name": self.app.package_info().name,
                "version": self.opts.app_version,
                "identifier": self.app.config().identifier,
                "pid": std::process::id(),
                "data_dir": self.opts.data_dir,
            },
            "api": {
                "url": format!("http://127.0.0.1:{}", self.port),
                "started_ms": self.started_ms,
                "uptime_s": jobs::now_ms().saturating_sub(self.started_ms) / 1000,
                "commands": reg.commands.len(),
                "events": reg.listened_events(),
                "jobs_active": self.jobs.lock().unwrap().active(),
                "auth_failures": self.auth_failures.load(Ordering::Relaxed),
            },
            "window": self.window(),
        });
        if reg.find("get_state").is_some() {
            match self.call_internal("get_state", json!({})).await {
                Ok(st) => {
                    v["launcher"] = json!({
                        "running": st.get("running").cloned().unwrap_or(Value::Null),
                        "jobs": st.get("jobs").cloned().unwrap_or(Value::Null),
                        "missing": st.get("missing").cloned().unwrap_or(Value::Null),
                        "installed": st.get("library").and_then(Value::as_object).map(|o| o.keys().cloned().collect::<Vec<_>>()),
                        "setup_done": st.pointer("/settings/setup_done").cloned().unwrap_or(Value::Null),
                    })
                }
                Err(e) => v["launcher"] = json!({"error": e}),
            }
        }
        v
    }

    /// show / hide / focus / status / quit.
    pub fn app_action(&self, action: &str) -> Result<Value, ApiError> {
        let w = self.app.get_webview_window(self.opts.label);
        match action {
            "status" => {}
            "show" => (self.opts.show)(&self.app),
            "hide" => (self.opts.hide)(&self.app),
            "focus" => {
                if let Some(w) = &w {
                    let _ = w.set_focus();
                }
            }
            "quit" => {
                let app = self.app.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(300)).await;
                    app.exit(0);
                });
                return Ok(json!({"ok": true, "action": "quit", "note": "The launcher closes in a moment; this API goes with it."}));
            }
            other => return Err(ApiError::new(400, "invalid_args", format!("Unknown action `{other}`.")).with(json!({"actions": ["show", "hide", "focus", "status", "quit"]}))),
        }
        Ok(json!({"ok": true, "action": action, "window": self.window()}))
    }

    /// Changes some settings: `patch` merged into the current ones (RFC 7386: objects merge,
    /// null removes) by the app's `patch_settings` command, which reads, merges and saves under
    /// the same lock as every other settings change in the app, so concurrent patches (and the
    /// app's own changes) aren't lost.
    pub async fn update_settings(self: &Arc<Self>, patch: Value, wait_ms: u64) -> Result<Value, ApiError> {
        if self.opts.registry.find(SETTINGS_PATCH).is_none() {
            return Err(ApiError::new(501, "unsupported", format!("This app has no `{SETTINGS_PATCH}` command.")));
        }
        if !patch.is_object() {
            return Err(ApiError::new(400, "invalid_args", "`patch` must be an object of settings to change."));
        }
        self.invoke(SETTINGS_PATCH, json!({"patch": patch}), wait_ms.max(1), true).await
    }
}

pub fn merge_patch(target: &mut Value, patch: &Value) {
    match patch {
        Value::Object(p) => {
            if !target.is_object() {
                *target = json!({});
            }
            let t = target.as_object_mut().unwrap();
            for (k, v) in p {
                if v.is_null() {
                    t.remove(k);
                } else {
                    merge_patch(t.entry(k.clone()).or_insert(Value::Null), v);
                }
            }
        }
        other => *target = other.clone(),
    }
}

// The end-to-end tests start the API, which runs only where the endpoint file can be private.
#[cfg(all(test, unix))]
mod tests;

#[cfg(test)]
mod platform_tests {
    #[test]
    fn only_platforms_with_private_files_run_it() {
        assert_eq!(super::platform_supported().is_ok(), cfg!(unix));
        if !cfg!(unix) {
            let f = std::env::temp_dir().join("rl-control-platform-test.json");
            assert!(super::write_private(&f, b"{}").is_err(), "no token file without private permissions");
        }
    }
}

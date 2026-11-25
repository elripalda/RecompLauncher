//! Test control: an MCP server inside the launcher, so an AI agent (or a script) can drive a build
//! the way a tester would before it ships: look at the window, press things, go to pages, start
//! installs and games through the window's own commands, press controller buttons, and read logs.
//!
//! It's off unless the launcher is started with RECOMPLAUNCHER_TEST_CONTROL set to a token the
//! tester picks (16 characters or more). Players never set it, so for them nothing listens and
//! nothing is added to the window. When it's on:
//!
//! - it listens on 127.0.0.1 only (RECOMPLAUNCHER_TEST_CONTROL_PORT, else 47811);
//! - every request needs `Authorization: Bearer <token>`, and requests from web pages are refused
//!   (see http.rs);
//! - the window shows a "Test control on" mark, and the trail says it started.
//!
//! The window's side is bridge.js, put in the page once it's ready. See docs/TEST-CONTROL.md.

mod http;
mod mcp;
mod page;

use crate::trail;
use std::net::Ipv4Addr;
use std::sync::Arc;
use tauri::{AppHandle, Listener};

pub const ENV_TOKEN: &str = "RECOMPLAUNCHER_TEST_CONTROL";
pub const ENV_PORT: &str = "RECOMPLAUNCHER_TEST_CONTROL_PORT";
pub const DEFAULT_PORT: u16 = 47811;
/// Long enough that it can't be guessed by trying.
const MIN_TOKEN: usize = 16;
/// Connections handled at once; more are told to try again.
const MAX_CONNS: usize = 8;

#[derive(Debug, PartialEq)]
pub struct Wanted {
    pub token: String,
    pub port: u16,
}

/// What the two variables ask for: None when test control is off (the variable isn't set, or is
/// empty), an error when it's set but can't be used as it is.
pub fn wanted(token: Option<String>, port: Option<String>) -> Result<Option<Wanted>, String> {
    let token = token.map(|t| t.trim().to_string()).unwrap_or_default();
    if token.is_empty() {
        return Ok(None);
    }
    if token.chars().count() < MIN_TOKEN {
        return Err(format!("{ENV_TOKEN} must be at least {MIN_TOKEN} characters"));
    }
    let printable = token.chars().all(|c| c.is_ascii_graphic());
    if !printable {
        return Err(format!("{ENV_TOKEN} may only use letters, digits and symbols (no spaces)"));
    }
    let port = match port.map(|p| p.trim().to_string()).filter(|p| !p.is_empty()) {
        None => DEFAULT_PORT,
        Some(p) => match p.parse::<u16>() {
            Ok(n) if n >= 1024 => n,
            _ => return Err(format!("{ENV_PORT} must be a port number from 1024 to 65535")),
        },
    };
    Ok(Some(Wanted { token, port }))
}

/// What a request's handling needs.
pub(crate) struct Ctx {
    pub app: AppHandle,
    pub token: String,
    pub port: u16,
    pub conns: Arc<tokio::sync::Semaphore>,
}

/// Starts test control when the launcher was asked for it. Called once, from setup, for the
/// launcher with a window (not for a game's shortcut start).
pub fn start(app: &AppHandle) {
    let asked = wanted(std::env::var(ENV_TOKEN).ok(), std::env::var(ENV_PORT).ok());
    let w = match asked {
        Ok(Some(w)) => w,
        Ok(None) => return,
        Err(why) => {
            trail::note(format!("Test control: not started ({why})"));
            return;
        }
    };
    app.listen_any(page::REPLY_EVENT, |ev| page::reply(ev.payload()));
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let listener = match tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, w.port)).await {
            Ok(l) => l,
            Err(e) => {
                trail::note(format!("Test control: couldn't listen on 127.0.0.1:{} ({e})", w.port));
                return;
            }
        };
        trail::note(format!("Test control: on, MCP at http://127.0.0.1:{}/mcp", w.port));
        let ctx = Arc::new(Ctx { app: app.clone(), token: w.token, port: w.port, conns: Arc::new(tokio::sync::Semaphore::new(MAX_CONNS)) });
        // The mark on the window (and the console and event records) from the start, not only
        // from the first call.
        tauri::async_runtime::spawn(async move {
            crate::wait_for_ui().await;
            let _ = page::run(&app, "ping", &serde_json::json!({}), None, std::time::Duration::from_secs(10)).await;
        });
        serve(ctx, listener).await;
    });
}

async fn serve(ctx: Arc<Ctx>, listener: tokio::net::TcpListener) {
    loop {
        let Ok((sock, peer)) = listener.accept().await else {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            continue;
        };
        if !peer.ip().is_loopback() {
            continue;
        }
        let Ok(permit) = ctx.conns.clone().try_acquire_owned() else {
            let busy = http::ApiError::new(503, "busy", "Too many connections at once; try again.");
            let _ = sock.try_write(&http::response_bytes(503, Some(&busy.to_json())));
            continue;
        };
        let c = ctx.clone();
        tauri::async_runtime::spawn(async move {
            let _permit = permit;
            handle(c, sock).await;
        });
    }
}

async fn handle(ctx: Arc<Ctx>, mut sock: tokio::net::TcpStream) {
    let (port, token) = (ctx.port, ctx.token.clone());
    let read = http::read_request(&mut sock, move |r| http::guard(r, port, &token)).await;
    let req = match read {
        Ok(r) => r,
        Err(e) => {
            http::write_response(&mut sock, e.status, Some(&e.to_json())).await;
            http::drain(&mut sock).await;
            return;
        }
    };
    let (status, body) = route(&ctx, &req).await;
    http::write_response(&mut sock, status, body.as_ref()).await;
}

async fn route(ctx: &Ctx, req: &http::Request) -> (u16, Option<serde_json::Value>) {
    match (req.method.as_str(), req.path.as_str()) {
        ("POST", "/mcp") => {
            let msg: serde_json::Value = match serde_json::from_slice(&req.body) {
                Ok(v) => v,
                Err(e) => return (200, Some(mcp::parse_error(&e.to_string()))),
            };
            match mcp::handle(ctx, msg).await {
                Some(v) => (200, Some(v)),
                None => (202, None),
            }
        }
        // No server-sent stream: MCP lets a server answer GET with 405.
        (_, "/mcp") => (405, Some(http::ApiError::new(405, "method_not_allowed", "Send MCP messages with POST.").to_json())),
        _ => (404, Some(http::ApiError::new(404, "not_found", "The only path is POST /mcp.").to_json())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(x: &str) -> Option<String> {
        Some(x.to_string())
    }

    #[test]
    fn off_when_the_variable_is_missing_or_empty() {
        assert_eq!(wanted(None, None), Ok(None));
        assert_eq!(wanted(s("  "), s("5000")), Ok(None));
    }

    #[test]
    fn a_long_token_turns_it_on_at_the_usual_port() {
        let w = wanted(s("abcdefghijklmnop"), None).unwrap().unwrap();
        assert_eq!(w, Wanted { token: "abcdefghijklmnop".into(), port: DEFAULT_PORT });
    }

    #[test]
    fn a_short_token_is_refused() {
        assert!(wanted(s("short"), None).is_err());
    }

    #[test]
    fn a_token_with_spaces_is_refused() {
        assert!(wanted(s("abcdefgh ijklmnopq"), None).is_err());
    }

    #[test]
    fn the_port_can_be_chosen() {
        assert_eq!(wanted(s("abcdefghijklmnop"), s("50123")).unwrap().unwrap().port, 50123);
    }

    #[test]
    fn a_bad_port_is_refused() {
        assert!(wanted(s("abcdefghijklmnop"), s("80")).is_err());
        assert!(wanted(s("abcdefghijklmnop"), s("seventy")).is_err());
        assert!(wanted(s("abcdefghijklmnop"), s("70000")).is_err());
    }
}

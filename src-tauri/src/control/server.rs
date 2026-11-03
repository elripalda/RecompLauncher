//! Connections and routes. Every request passes [`http::guard`] before anything is done with it.

use super::http::{self, ApiError, Request};
use super::{invoke_reply, mcp, registry, Ctl, DEFAULT_WAIT_MS};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::Runtime;

pub async fn serve<R: Runtime>(ctl: Arc<Ctl<R>>, listener: tokio::net::TcpListener) {
    loop {
        let (sock, peer) = match listener.accept().await {
            Ok(x) => x,
            Err(_) => {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                continue;
            }
        };
        if ctl.closed() {
            break;
        }
        if !peer.ip().is_loopback() {
            continue;
        }
        match ctl.conns().try_acquire_owned() {
            Ok(permit) => {
                let c = ctl.clone();
                tauri::async_runtime::spawn(async move {
                    let _permit = permit;
                    handle(c, sock).await;
                });
            }
            // Too many at once: a short refusal, without waiting on this one.
            Err(_) => {
                let _ = sock.try_write(&http::response_bytes(503, Some(&ApiError::new(503, "busy", "Too many connections at once; try again.").to_json())));
            }
        }
    }
}

async fn handle<R: Runtime>(ctl: Arc<Ctl<R>>, mut sock: tokio::net::TcpStream) {
    let (port, c2) = (ctl.port, ctl.clone());
    let req = match http::read_request(&mut sock, move |r| http::guard(r, port, c2.token())).await {
        Ok(r) => r,
        Err(e) => {
            if e.status == 401 {
                ctl.auth_failures.fetch_add(1, Ordering::Relaxed);
            }
            http::write_response(&mut sock, e.status, Some(&e.to_json())).await;
            http::drain(&mut sock).await;
            return;
        }
    };
    let (status, body) = match route(&ctl, &req).await {
        Ok(x) => x,
        Err(e) => (e.status, Some(e.to_json())),
    };
    http::write_response(&mut sock, status, body.as_ref()).await;
}

fn body_json(req: &Request) -> Result<Value, ApiError> {
    if req.body.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_slice(&req.body).map_err(|e| ApiError::new(400, "invalid_json", format!("The body isn't valid JSON: {e}")))
}

fn num(req: &Request, name: &str, default: u64) -> Result<u64, ApiError> {
    match req.param(name) {
        None | Some("") => Ok(default),
        Some(v) => v.parse().map_err(|_| ApiError::new(400, "invalid_args", format!("`{name}` must be a whole number."))),
    }
}

const ROUTES: &[&str] = &[
    "GET /v1/status",
    "GET /v1/commands?q=&module=",
    "GET /v1/commands/{name}",
    "POST /v1/invoke {command, args, async?, timeout_ms?, validate?}",
    "GET /v1/jobs",
    "GET /v1/jobs/{id}?wait_ms=",
    "POST /v1/jobs/{id}/cancel",
    "GET /v1/events?after=&names=a,b&limit=&wait_ms=",
    "POST /v1/app {action: show|hide|focus|status|quit}",
    "POST /v1/settings {patch, timeout_ms?}",
    "GET /v1/limitations",
    "POST /mcp (MCP JSON-RPC; ?tools=meta lists only the rlapi_* tools)",
];

pub fn list_commands(reg: &registry::Registry, q: &str, module: &str) -> Value {
    let q = q.to_ascii_lowercase();
    let list: Vec<Value> = reg
        .commands
        .iter()
        .filter(|c| module.is_empty() || c.module == module)
        .filter(|c| q.is_empty() || c.name.contains(q.as_str()) || c.doc.to_ascii_lowercase().contains(q.as_str()))
        .map(|c| reg.summary(c))
        .collect();
    json!({"ok": true, "count": list.len(), "total": reg.commands.len(), "commands": list})
}

async fn route<R: Runtime>(ctl: &Arc<Ctl<R>>, req: &Request) -> Result<(u16, Option<Value>), ApiError> {
    let reg = ctl.opts.registry;
    let segs: Vec<&str> = req.path.trim_matches('/').split('/').collect();
    let ok = |v: Value| Ok((200, Some(v)));
    match (req.method.as_str(), segs.as_slice()) {
        ("GET", ["v1", "status"]) => ok(ctl.status().await),
        ("GET", ["v1", "commands"]) => ok(list_commands(reg, req.param("q").unwrap_or(""), req.param("module").unwrap_or(""))),
        ("GET", ["v1", "commands", name]) => match reg.find(name) {
            Some(c) => ok(json!({"ok": true, "command": reg.describe(c)})),
            None => Err(ApiError::new(404, "unknown_command", format!("There's no command `{name}`."))),
        },
        ("POST", ["v1", "invoke"]) => {
            let b = body_json(req)?;
            let name = b.get("command").and_then(Value::as_str).ok_or_else(|| ApiError::new(400, "invalid_args", "`command` is required."))?;
            let wait = if b.get("async").and_then(Value::as_bool).unwrap_or(false) { 0 } else { b.get("timeout_ms").and_then(Value::as_u64).unwrap_or(DEFAULT_WAIT_MS).max(1) };
            let validate = b.get("validate").and_then(Value::as_bool).unwrap_or(true);
            let job = ctl.invoke(name, b.get("args").cloned().unwrap_or(Value::Null), wait, validate).await?;
            let (s, v) = invoke_reply(&job);
            Ok((s, Some(v)))
        }
        ("GET", ["v1", "jobs"]) => ok(ctl.jobs()),
        ("GET", ["v1", "jobs", id]) => {
            let wait = num(req, "wait_ms", 0)?;
            if wait > 0 {
                ctl.wait_job(id, wait.min(super::MAX_WAIT_MS)).await;
            }
            match ctl.job(id) {
                Some(j) => ok(json!({"ok": true, "job": j})),
                None => Err(ApiError::new(404, "job_not_found", format!("No job `{id}`."))),
            }
        }
        // 202 while the cancel waits for the command to answer (it's sent then), 200 once sent.
        ("POST", ["v1", "jobs", id, "cancel"]) => {
            let v = ctl.cancel_job(id).await?;
            Ok((if v["pending"] == true { 202 } else { 200 }, Some(v)))
        }
        ("GET", ["v1", "events"]) => {
            let names: Vec<String> = req.param("names").unwrap_or("").split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect();
            ok(ctl.events(num(req, "after", 0)?, &names, num(req, "limit", 200)? as usize, num(req, "wait_ms", 0)?).await)
        }
        ("POST", ["v1", "app"]) => {
            let b = body_json(req)?;
            ok(ctl.app_action(b.get("action").and_then(Value::as_str).unwrap_or("status"))?)
        }
        ("POST", ["v1", "settings"]) => {
            let b = body_json(req)?;
            let job = ctl.update_settings(b.get("patch").cloned().unwrap_or(Value::Null), b.get("timeout_ms").and_then(Value::as_u64).unwrap_or(DEFAULT_WAIT_MS)).await?;
            let (s, v) = invoke_reply(&job);
            Ok((s, Some(v)))
        }
        ("GET", ["v1", "limitations"]) => ok(json!({"ok": true, "limitations": registry::limitations()})),
        ("POST", ["mcp"]) => {
            let meta_only = req.param("tools") == Some("meta");
            let msg: Value = match serde_json::from_slice(&req.body) {
                Ok(v) => v,
                Err(e) => return Ok((200, Some(mcp::parse_error(&e.to_string())))),
            };
            match mcp::handle(ctl, msg, meta_only).await {
                Some(v) => ok(v),
                None => Ok((202, None)),
            }
        }
        (_, ["v1", "status" | "commands" | "invoke" | "jobs" | "events" | "app" | "settings" | "limitations", ..]) | (_, ["mcp"]) => Err(ApiError::new(405, "method_not_allowed", "Wrong method for this path.").with(json!({"routes": ROUTES}))),
        _ => Err(ApiError::new(404, "not_found", "No such path.").with(json!({"routes": ROUTES}))),
    }
}

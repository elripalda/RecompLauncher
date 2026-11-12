//! Connections and routes. Every request passes [`http::guard`] before anything is done with it,
//! and the routes below are the whole API: status, Play, and MCP with the same two tools.

use super::http::{self, ApiError, Request};
use super::{mcp, Ctx};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::watch;

pub const ROUTES: &[&str] = &["GET /v1/status", "POST /v1/play {\"id\": \"...\"}", "POST /mcp (MCP JSON-RPC: launcher_status, play_game)"];

pub(crate) async fn serve(ctx: Arc<Ctx>, listener: tokio::net::TcpListener, mut stopping: watch::Receiver<bool>) {
    // Every connection's task, so turning the API off can end them all.
    let mut conns = tokio::task::JoinSet::new();
    loop {
        let accepted = tokio::select! {
            _ = stopping.changed() => break,
            // Ended ones are let go as they end.
            Some(_) = conns.join_next(), if !conns.is_empty() => continue,
            a = listener.accept() => a,
        };
        let Ok((sock, peer)) = accepted else {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            continue;
        };
        if !peer.ip().is_loopback() {
            continue;
        }
        let Ok(permit) = ctx.conns.clone().try_acquire_owned() else {
            // Too many at once: a short refusal, without waiting on this one.
            let busy = ApiError::new(503, "busy", "Too many connections at once; try again.");
            let _ = sock.try_write(&http::response_bytes(503, Some(&busy.to_json())));
            continue;
        };
        let c = ctx.clone();
        let ending = stopping.clone();
        conns.spawn(async move {
            let _permit = permit;
            handle(c, sock, ending).await;
        });
    }
    // Nothing listens on the port any more.
    drop(listener);
    // Don't abort dispatched Plays: dropping their futures can release a start claim and cloud
    // lock while spawn_blocking still restores saves. Safe waits observe Off; mutation finishes
    // with its guards held. The server stays tracked as Ending until every request has ended.
    while conns.join_next().await.is_some() {}
}

async fn handle(ctx: Arc<Ctx>, mut sock: tokio::net::TcpStream, mut stopping: watch::Receiver<bool>) {
    let (port, c2) = (ctx.port, ctx.clone());
    let read = tokio::select! {
        biased;
        _ = stopping.wait_for(|off| *off) => return,
        read = http::read_request(&mut sock, move |r| http::guard(r, port, c2.token())) => read,
    };
    let req = match read {
        Ok(r) => r,
        Err(e) => {
            http::write_response(&mut sock, e.status, Some(&e.to_json())).await;
            http::drain(&mut sock).await;
            return;
        }
    };
    let (status, body) = match route(&ctx, &req).await {
        Ok(x) => x,
        Err(e) => (e.status, Some(e.to_json())),
    };
    http::write_response(&mut sock, status, body.as_ref()).await;
}

fn body_json(req: &Request) -> Result<Value, ApiError> {
    serde_json::from_slice(&req.body).map_err(|e| ApiError::new(400, "invalid_json", format!("The body isn't valid JSON: {e}")))
}

/// Play's body: exactly `{"id": "<game id>"}`. Anything else is refused rather than ignored, so a
/// caller can't believe it changed something it didn't.
pub fn play_id(body: &Value) -> Result<String, ApiError> {
    let Some(obj) = body.as_object() else {
        return Err(ApiError::new(400, "invalid_args", "The body must be {\"id\": \"<game id>\"}."));
    };
    if let Some(extra) = obj.keys().find(|k| k.as_str() != "id") {
        return Err(ApiError::new(400, "invalid_args", format!("Play takes only `id` (not `{extra}`).")));
    }
    let Some(id) = obj.get("id").and_then(Value::as_str) else {
        return Err(ApiError::new(400, "invalid_args", "`id` is required: the id of an installed game (see status)."));
    };
    Ok(id.to_string())
}

async fn route(ctx: &Arc<Ctx>, req: &Request) -> Result<(u16, Option<Value>), ApiError> {
    if ctx.stopped() {
        return Err(ApiError::new(503, "stopped", "The control API was turned off."));
    }
    let routes = || json!({"routes": ROUTES});
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/v1/status") => Ok((200, Some(ctx.status().await?))),
        ("POST", "/v1/play") => {
            let id = play_id(&body_json(req)?)?;
            Ok((200, Some(ctx.play(&id).await?)))
        }
        ("POST", "/mcp") => {
            let msg: Value = match serde_json::from_slice(&req.body) {
                Ok(v) => v,
                Err(e) => return Ok((200, Some(mcp::parse_error(&e.to_string())))),
            };
            match mcp::handle(ctx, msg).await {
                Some(v) => Ok((200, Some(v))),
                None => Ok((202, None)),
            }
        }
        (_, "/v1/status" | "/v1/play" | "/mcp") => {
            let mut e = ApiError::new(405, "method_not_allowed", "Wrong method for this path.").to_json();
            e["error"]["details"] = routes();
            Ok((405, Some(e)))
        }
        _ => {
            let mut e = ApiError::new(404, "not_found", "No such path. This API only has status and Play.").to_json();
            e["error"]["details"] = routes();
            Ok((404, Some(e)))
        }
    }
}

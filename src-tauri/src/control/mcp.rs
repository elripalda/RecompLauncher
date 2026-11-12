//! MCP (Model Context Protocol) over JSON-RPC 2.0, served at `POST /mcp`: `initialize`, `ping`,
//! `tools/list` and `tools/call`, with two tools: `launcher_status` and `play_game`. They call the
//! same [`Ctx`] methods as the HTTP routes, so MCP can't do anything HTTP can't. The stdio adapter
//! (tools/control-api/recomplauncher_mcp.py) passes each line it reads to this endpoint.

use super::server::play_id;
use super::Ctx;
use serde_json::{json, Value};

pub const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
pub const DEFAULT_PROTOCOL: &str = "2025-06-18";
pub const STATUS_TOOL: &str = "launcher_status";
pub const PLAY_TOOL: &str = "play_game";

const INSTRUCTIONS: &str = "Sees and starts games in the running RecompLauncher. launcher_status lists the installed games (with their ids) and which are running. play_game starts one that's ready to play, by id. It never installs or sets anything up: a game that needs that answers needs_setup, and the player does it in the launcher's window. Nothing else is available: installing, removing and settings stay in the window.";

fn rpc_ok(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn rpc_err(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

pub fn parse_error(detail: &str) -> Value {
    rpc_err(&Value::Null, -32700, &format!("Parse error: {detail}"))
}

/// The tools, with schemas that allow nothing beyond what they use.
pub fn tools() -> Value {
    let status_schema = json!({"type": "object", "properties": {}, "additionalProperties": false});
    let play_schema = json!({
        "type": "object",
        "properties": {"id": {"type": "string", "minLength": 1, "maxLength": super::MAX_ID_LEN, "description": "An installed game's id, from launcher_status"}},
        "required": ["id"],
        "additionalProperties": false,
    });
    json!([
        {"name": STATUS_TOOL, "title": "Launcher status", "description": "The installed games (id, name, version, whether each is running or its folder is missing, and the games inside recomps that carry several) and the ids of the games running now.", "inputSchema": status_schema, "annotations": {"readOnlyHint": true}},
        {"name": PLAY_TOOL, "title": "Play a game", "description": "Starts an installed game that's ready to play, by id, as its desktop shortcut would (with its shader and cloud save). It never runs a setup or an installer: a game that needs one is refused with needs_setup. When its recomp is already playing another of its games, it's refused with busy. Other running games keep running.", "inputSchema": play_schema, "annotations": {"readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false}},
    ])
}

/// One message. None: nothing to answer (a notification, or a response from the client).
pub(crate) async fn handle(ctx: &Ctx, msg: Value) -> Option<Value> {
    // Batches were dropped from MCP: one call per request keeps every call through one path.
    if msg.is_array() {
        return Some(rpc_err(&Value::Null, -32600, "Invalid Request: send one message at a time"));
    }
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str);
    let is_rpc = msg.get("jsonrpc").and_then(Value::as_str) == Some("2.0");
    let Some(method) = method.filter(|_| is_rpc) else {
        // A response from the client (to nothing asked), or junk.
        if msg.get("result").is_some() || msg.get("error").is_some() {
            return None;
        }
        return Some(rpc_err(&id.unwrap_or(Value::Null), -32600, "Invalid Request"));
    };
    // Notifications (initialized, cancelled...): nothing to answer.
    let id = id?;
    let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));
    let answer = match method {
        "initialize" => {
            let want = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(DEFAULT_PROTOCOL);
            let version = if PROTOCOL_VERSIONS.contains(&want) { want } else { DEFAULT_PROTOCOL };
            rpc_ok(&id, json!({
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "recomplauncher", "title": "RecompLauncher", "version": ctx.app_version},
                "instructions": INSTRUCTIONS,
            }))
        }
        "ping" => rpc_ok(&id, json!({})),
        "tools/list" => rpc_ok(&id, json!({"tools": tools()})),
        "tools/call" => call(ctx, &id, &params).await,
        _ => rpc_err(&id, -32601, &format!("Method not found: {method}")),
    };
    Some(answer)
}

fn tool_result(structured: Value, is_error: bool) -> Value {
    let text = serde_json::to_string(&structured).unwrap_or_default();
    json!({"content": [{"type": "text", "text": text}], "structuredContent": structured, "isError": is_error})
}

async fn call(ctx: &Ctx, id: &Value, params: &Value) -> Value {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return rpc_err(id, -32602, "Invalid params: `name` is required");
    };
    let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
    let answer = match name {
        STATUS_TOOL => status(ctx, &args).await,
        PLAY_TOOL => play(ctx, &args).await,
        // Only the two tools exist: anything else is an unknown tool, not a command to look up.
        _ => return rpc_err(id, -32602, &format!("Unknown tool: {name}")),
    };
    let result = match answer {
        Ok(v) => tool_result(v, false),
        Err(e) => tool_result(e.to_json(), true),
    };
    rpc_ok(id, result)
}

async fn status(ctx: &Ctx, args: &Value) -> Result<Value, super::http::ApiError> {
    let empty = args.as_object().is_some_and(|o| o.is_empty());
    if !empty {
        return Err(super::http::ApiError::new(400, "invalid_args", "launcher_status takes no arguments."));
    }
    ctx.status().await
}

async fn play(ctx: &Ctx, args: &Value) -> Result<Value, super::http::ApiError> {
    let id = play_id(args)?;
    ctx.play(&id).await
}

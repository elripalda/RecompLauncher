//! MCP (Model Context Protocol) over JSON-RPC 2.0, served at `POST /mcp`: `initialize`,
//! `ping`, `tools/list` and `tools/call`. Each registered command is a tool named `rl_<command>`
//! with its parameters as the input schema; the `rlapi_*` tools cover discovery, jobs, events,
//! the window and settings. The stdio adapter (tools/control-api/recomplauncher_mcp.py) passes
//! each line it reads to this endpoint and prints the answer.

use super::http::ApiError;
use super::{invoke_reply, registry, server, Ctl, DEFAULT_WAIT_MS};
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::Runtime;

pub const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
pub const DEFAULT_PROTOCOL: &str = "2025-06-18";
pub const COMMAND_PREFIX: &str = "rl_";

const INSTRUCTIONS: &str = "Controls the running RecompLauncher app. Tools named rl_<command> call the launcher's own backend commands with live state (the same ones its window uses); rlapi_* tools list and describe commands, follow jobs and events, show/hide/quit the window and change settings. Start with rlapi_status, then rlapi_list_commands. Long work (install, setup_copy) returns a job: follow it with rlapi_job (wait_ms) or rlapi_events, cancel with rlapi_cancel_job. File pickers aren't available: every command takes paths directly (see rlapi_limitations).";

fn rpc_ok(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn rpc_err(id: &Value, code: i64, message: &str, data: Option<Value>) -> Value {
    let mut e = json!({"code": code, "message": message});
    if let Some(d) = data {
        e["data"] = d;
    }
    json!({"jsonrpc": "2.0", "id": id, "error": e})
}

pub fn parse_error(detail: &str) -> Value {
    rpc_err(&Value::Null, -32700, "Parse error", Some(json!({"detail": detail})))
}

/// One message (or a batch). None: nothing to answer (notifications).
pub async fn handle<R: Runtime>(ctl: &Arc<Ctl<R>>, msg: Value, meta_only: bool) -> Option<Value> {
    match msg {
        Value::Array(list) => {
            if list.is_empty() {
                return Some(rpc_err(&Value::Null, -32600, "Invalid Request", Some(json!({"detail": "empty batch"}))));
            }
            let mut out = vec![];
            for m in list {
                if let Some(r) = handle_one(ctl, m, meta_only).await {
                    out.push(r);
                }
            }
            (!out.is_empty()).then_some(Value::Array(out))
        }
        m => handle_one(ctl, m, meta_only).await,
    }
}

async fn handle_one<R: Runtime>(ctl: &Arc<Ctl<R>>, msg: Value, meta_only: bool) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str);
    if msg.get("jsonrpc").and_then(Value::as_str) != Some("2.0") || method.is_none() {
        // A response from the client (to nothing we asked) or junk.
        if msg.get("result").is_some() || msg.get("error").is_some() {
            return None;
        }
        return Some(rpc_err(&id.unwrap_or(Value::Null), -32600, "Invalid Request", None));
    }
    let method = method.unwrap();
    let Some(id) = id else {
        // Notifications (initialized, cancelled...): nothing to answer.
        return None;
    };
    let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));
    Some(match method {
        "initialize" => {
            let want = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(DEFAULT_PROTOCOL);
            let version = if PROTOCOL_VERSIONS.contains(&want) { want } else { DEFAULT_PROTOCOL };
            rpc_ok(&id, json!({
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "recomplauncher", "title": "RecompLauncher", "version": ctl.opts.app_version},
                "instructions": INSTRUCTIONS,
            }))
        }
        "ping" => rpc_ok(&id, json!({})),
        "tools/list" => rpc_ok(&id, json!({"tools": tools(ctl.opts.registry, meta_only)})),
        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return Some(rpc_err(&id, -32602, "Invalid params", Some(json!({"detail": "`name` is required"}))));
            };
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            match call(ctl, name, args).await {
                Some(r) => rpc_ok(&id, r),
                None => rpc_err(&id, -32602, &format!("Unknown tool: {name}"), Some(json!({"hint": "tools/list"}))),
            }
        }
        _ => rpc_err(&id, -32601, "Method not found", Some(json!({"method": method}))),
    })
}

fn obj(props: Value, required: &[&str]) -> Value {
    let mut v = json!({"type": "object", "properties": props, "additionalProperties": false});
    if !required.is_empty() {
        v["required"] = json!(required);
    }
    v
}

fn meta_tools() -> Vec<Value> {
    let ro = json!({"readOnlyHint": true});
    vec![
        json!({"name": "rlapi_status", "title": "Launcher status", "description": "The app (version, pid, data folder), the API, the main window and the launcher's running games, background jobs and installed ids.", "inputSchema": obj(json!({}), &[]), "annotations": ro}),
        json!({"name": "rlapi_list_commands", "title": "List commands", "description": "Every backend command the API can call (each is also a tool rl_<name>), with a one-line summary and its parameters (name? = optional). Filter by text or source module.", "inputSchema": obj(json!({"query": {"type": "string", "description": "Text in the name or documentation"}, "module": {"type": "string", "description": "Source module, e.g. lib, adopt, cloud, plugins"}}), &[]), "annotations": ro}),
        json!({"name": "rlapi_describe_command", "title": "Describe a command", "description": "One command in full: documentation, Rust signature, parameter schemas, what it returns, how it reports progress and how it's cancelled.", "inputSchema": obj(json!({"name": {"type": "string", "description": "Command name (with or without the rl_ prefix)"}}), &["name"]), "annotations": ro}),
        json!({"name": "rlapi_invoke", "title": "Invoke a command", "description": "Calls a backend command by name. async: true answers at once with a job; otherwise waits up to timeout_ms (default 30000) and answers with the result, or with the job if it's still going. validate: false skips the API's own argument checks (Tauri's still apply).", "inputSchema": obj(json!({"command": {"type": "string"}, "args": {"type": "object", "description": "The command's arguments (camelCase or snake_case keys)"}, "async": {"type": "boolean"}, "timeout_ms": {"type": "integer", "minimum": 1, "maximum": super::MAX_WAIT_MS}, "validate": {"type": "boolean"}}), &["command"])}),
        json!({"name": "rlapi_jobs", "title": "List jobs", "description": "Calls made through the API (newest first) with their state: running, cancelling, succeeded, failed or cancelled.", "inputSchema": obj(json!({}), &[]), "annotations": ro}),
        json!({"name": "rlapi_job", "title": "Job status", "description": "One job: state, result or error, latest progress and recent events. wait_ms waits for it to finish (up to that long).", "inputSchema": obj(json!({"id": {"type": "string"}, "wait_ms": {"type": "integer", "minimum": 0, "maximum": super::MAX_WAIT_MS}}), &["id"]), "annotations": ro}),
        json!({"name": "rlapi_cancel_job", "title": "Cancel a job", "description": "Cancels a job through the launcher's own cancel for it (install -> cancel, setup_copy -> setup_cancel for the job's own run, account_start -> account_cancel). Asked before the command has answered, the cancel waits and is sent as soon as it does (pending: true, state cancelling); the job then ends with the work's closing event. Other commands can't be cancelled and say so.", "inputSchema": obj(json!({"id": {"type": "string"}}), &["id"])}),
        json!({"name": "rlapi_events", "title": "Events", "description": "The launcher's events (install/setup/move/mod progress, game started/stopped, toasts...) after sequence number `after`, optionally only some names; wait_ms waits for the next one (max 30000). Pass next_after back as after.", "inputSchema": obj(json!({"after": {"type": "integer", "minimum": 0}, "names": {"type": "array", "items": {"type": "string"}}, "limit": {"type": "integer", "minimum": 1, "maximum": 500}, "wait_ms": {"type": "integer", "minimum": 0, "maximum": super::MAX_EVENT_WAIT_MS}}), &[]), "annotations": ro}),
        json!({"name": "rlapi_app", "title": "Window and app", "description": "show, hide or focus the main window, read its status, or quit the launcher.", "inputSchema": obj(json!({"action": {"type": "string", "enum": ["status", "show", "hide", "focus", "quit"]}}), &["action"])}),
        json!({"name": "rlapi_update_settings", "title": "Change settings", "description": "Changes only the given settings: `patch` merged into the current settings (objects merge, null takes a setting back to its default) and saved by the launcher's patch_settings in one step, so changes made at the same time aren't lost. Answers with the settings as saved. Paths must be absolute.", "inputSchema": obj(json!({"patch": {"type": "object"}, "timeout_ms": {"type": "integer", "minimum": 1}}), &["patch"])}),
        json!({"name": "rlapi_limitations", "title": "Limitations", "description": "What the API doesn't do (native file pickers and confirm boxes, window chrome) and which commands to use instead.", "inputSchema": obj(json!({}), &[]), "annotations": ro}),
    ]
}

pub fn tools(reg: &registry::Registry, meta_only: bool) -> Vec<Value> {
    let mut out = meta_tools();
    if meta_only {
        return out;
    }
    for (c, schema) in reg.commands.iter().zip(reg.input_schemas().iter()) {
        let n = reg.note(c.name);
        let mut d = if c.doc.is_empty() { format!("The launcher's `{}` command.", c.name) } else { c.doc.to_string() };
        if d.chars().count() > 1200 {
            d = d.chars().take(1197).collect::<String>() + "...";
        }
        d.push_str(&format!("\n\nBackend command `{}` ({}.rs), returns `{}`.", c.path, c.module, c.returns));
        if let Some(t) = n.track {
            d.push_str(&if t.completes_on_event {
                format!(" Runs in the background: answers with a job that ends with its `{}` event (follow with rlapi_job / rlapi_events).", t.event)
            } else {
                format!(" Progress: `{}` events.", t.event)
            });
        }
        if let Some(k) = n.cancel {
            d.push_str(&format!(" Cancel with rlapi_cancel_job (uses `{}`).", k.command));
        }
        if !n.remark.is_empty() {
            d.push(' ');
            d.push_str(n.remark);
        }
        let mut t = json!({"name": format!("{COMMAND_PREFIX}{}", c.name), "title": c.name, "description": d, "inputSchema": schema});
        if n.read_only {
            t["annotations"] = json!({"readOnlyHint": true});
        }
        out.push(t);
    }
    out
}

fn tool_result(structured: Value, is_error: bool) -> Value {
    let text = serde_json::to_string(&structured).unwrap_or_default();
    json!({"content": [{"type": "text", "text": text}], "structuredContent": structured, "isError": is_error})
}

fn from_api(e: ApiError) -> Value {
    tool_result(e.to_json(), true)
}

fn from_job(r: Result<Value, ApiError>) -> Value {
    match r {
        Ok(job) => {
            let (status, mut v) = invoke_reply(&job);
            if status == 202 {
                v["hint"] = Value::String(format!("Still going. Follow it with rlapi_job {{\"id\": \"{}\", \"wait_ms\": 30000}} or rlapi_events.", job["id"].as_str().unwrap_or("")));
            }
            tool_result(v, status >= 400)
        }
        Err(e) => from_api(e),
    }
}

fn u(args: &Value, k: &str) -> Option<u64> {
    args.get(k).and_then(Value::as_u64)
}

/// A tool call: None when there's no such tool.
async fn call<R: Runtime>(ctl: &Arc<Ctl<R>>, name: &str, args: Value) -> Option<Value> {
    let reg = ctl.opts.registry;
    if let Some(cmd) = name.strip_prefix(COMMAND_PREFIX) {
        reg.find(cmd)?;
        return Some(from_job(ctl.invoke(cmd, args, DEFAULT_WAIT_MS, true).await));
    }
    let s = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    Some(match name {
        "rlapi_status" => tool_result(ctl.status().await, false),
        "rlapi_list_commands" => tool_result(server::list_commands(reg, &s("query"), &s("module")), false),
        "rlapi_describe_command" => {
            let n = s("name");
            let n = n.strip_prefix(COMMAND_PREFIX).unwrap_or(&n);
            match reg.find(n) {
                Some(c) => tool_result(json!({"ok": true, "command": reg.describe(c)}), false),
                None => from_api(ApiError::new(404, "unknown_command", format!("There's no command `{n}`."))),
            }
        }
        "rlapi_invoke" => {
            let cmd = s("command");
            let wait = if args.get("async").and_then(Value::as_bool).unwrap_or(false) { 0 } else { u(&args, "timeout_ms").unwrap_or(DEFAULT_WAIT_MS).max(1) };
            let validate = args.get("validate").and_then(Value::as_bool).unwrap_or(true);
            from_job(ctl.invoke(&cmd, args.get("args").cloned().unwrap_or(Value::Null), wait, validate).await)
        }
        "rlapi_jobs" => tool_result(ctl.jobs(), false),
        "rlapi_job" => {
            let id = s("id");
            if let Some(w) = u(&args, "wait_ms").filter(|w| *w > 0) {
                ctl.wait_job(&id, w.min(super::MAX_WAIT_MS)).await;
            }
            match ctl.job(&id) {
                Some(j) => tool_result(json!({"ok": true, "job": j}), false),
                None => from_api(ApiError::new(404, "job_not_found", format!("No job `{id}`."))),
            }
        }
        "rlapi_cancel_job" => match ctl.cancel_job(&s("id")).await {
            Ok(v) => tool_result(v, false),
            Err(e) => from_api(e),
        },
        "rlapi_events" => {
            let names: Vec<String> = args.get("names").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect()).unwrap_or_default();
            tool_result(ctl.events(u(&args, "after").unwrap_or(0), &names, u(&args, "limit").unwrap_or(200) as usize, u(&args, "wait_ms").unwrap_or(0)).await, false)
        }
        "rlapi_app" => match ctl.app_action(&s("action")) {
            Ok(v) => tool_result(v, false),
            Err(e) => from_api(e),
        },
        "rlapi_update_settings" => from_job(ctl.update_settings(args.get("patch").cloned().unwrap_or(Value::Null), u(&args, "timeout_ms").unwrap_or(DEFAULT_WAIT_MS)).await),
        "rlapi_limitations" => tool_result(json!({"ok": true, "limitations": registry::limitations()}), false),
        _ => return None,
    })
}

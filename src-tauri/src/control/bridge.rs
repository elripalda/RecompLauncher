//! Calls a registered command the way the window does: an `InvokeRequest` handed to the main
//! webview's `on_message`, inside this process. That runs Tauri's own pipeline: the app's
//! `generate_handler!` dispatch, the capability/ACL check for a local origin, and the injection
//! of `State`, `AppHandle` and the window, so there's no second copy of any command here.
//!
//! The invoke key comes from `AppHandle::invoke_key()`, Tauri's public accessor for backend code
//! with its own invoke system. It never leaves the process: nothing here sends it anywhere.

use super::http::ApiError;
use serde_json::{json, Value};
use tauri::ipc::{CallbackFn, InvokeBody, InvokeError, InvokeResponse, InvokeResponseBody};
use tauri::webview::InvokeRequest;
use tauri::{AppHandle, Manager, Runtime};
use tokio::sync::oneshot;

/// What a command answered: its value, or the error it gave (as Tauri passes it to the page).
pub type Answer = Result<Value, Value>;

/// The address the page itself is at, used when the webview can't say (Tauri treats it as local).
fn page_url() -> tauri::Url {
    #[cfg(any(windows, target_os = "android"))]
    let u = "http://tauri.localhost/";
    #[cfg(not(any(windows, target_os = "android")))]
    let u = "tauri://localhost/";
    u.parse().expect("static url")
}

/// Starts one command. Sync commands run on the calling thread before this returns (so call it
/// off the main thread); async ones are spawned by Tauri. The answer arrives on the receiver.
pub fn dispatch<R: Runtime>(app: &AppHandle<R>, label: &str, cmd: &str, args: Value) -> Result<oneshot::Receiver<Answer>, ApiError> {
    let w = app.get_webview_window(label).ok_or_else(|| ApiError::new(503, "unavailable", format!("The `{label}` window isn't there, so commands can't run.")))?;
    let url = w.url().unwrap_or_else(|_| page_url());
    let (tx, rx) = oneshot::channel();
    let req = InvokeRequest {
        cmd: cmd.to_string(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url,
        body: InvokeBody::Json(args),
        headers: Default::default(),
        invoke_key: app.invoke_key().to_string(),
    };
    w.on_message(
        req,
        Box::new(move |_wv, _cmd, resp, _ok, _err| {
            let _ = tx.send(answer(resp));
        }),
    );
    Ok(rx)
}

fn answer(resp: InvokeResponse) -> Answer {
    match resp {
        InvokeResponse::Ok(InvokeResponseBody::Json(s)) => Ok(serde_json::from_str(&s).unwrap_or(Value::String(s))),
        InvokeResponse::Ok(InvokeResponseBody::Raw(b)) => {
            use base64::Engine;
            Ok(json!({"encoding": "base64", "data": base64::engine::general_purpose::STANDARD.encode(b)}))
        }
        InvokeResponse::Err(InvokeError(v)) => Err(v),
    }
}

/// A command's error as the API reports it, with a code saying what kind it is.
pub fn classify(cmd: &str, err: Value) -> Value {
    let msg = match &err {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let code = if msg.starts_with("invalid args") || msg.contains("missing required key") {
        "invalid_args"
    } else if msg.starts_with("Command ") && msg.ends_with(" not found") {
        "unknown_command"
    } else if msg.contains("not allowed") && (msg.contains("ACL") || msg.contains("capabilit")) {
        "forbidden"
    } else if msg == "Cancelled" {
        "cancelled"
    } else {
        "command_failed"
    };
    let mut v = json!({"code": code, "message": msg, "command": cmd});
    if !err.is_string() {
        v["raw"] = err;
    }
    v
}

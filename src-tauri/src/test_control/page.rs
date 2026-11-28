//! Asking the launcher's window: each call runs bridge.js's `run` in the page, and the page answers
//! on the rl-test-reply event with the call's id.

use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tokio::sync::oneshot;

pub const REPLY_EVENT: &str = "rl-test-reply";
const BRIDGE: &str = include_str!("bridge.js");

struct Reply {
    ok: bool,
    value: Value,
}

static PENDING: Mutex<Option<HashMap<String, oneshot::Sender<Reply>>>> = Mutex::new(None);
static NEXT: AtomicU64 = AtomicU64::new(1);

/// An id no page script can guess, so only the bridge's own answer counts.
fn new_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(NEXT.fetch_add(1, Ordering::Relaxed));
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    h.write_u128(n);
    format!("{:016x}{:016x}", h.finish(), std::collections::hash_map::RandomState::new().build_hasher().finish())
}

fn forget(id: &str) {
    if let Some(m) = PENDING.lock().unwrap().as_mut() {
        m.remove(id);
    }
}

/// The scripts for one call: the bridge (it keeps itself to one copy) and the call. ui_eval's
/// `code` is run as the body of an async function (so it can `await` and `return`). It goes in as
/// script text, not through eval(), which the window's content security policy forbids. Its call
/// is a script of its own, after one that says it's coming: code that doesn't parse then never
/// runs, and the bridge answers the call with the browser's syntax error.
pub fn scripts_for(id: &str, op: &str, args: &Value, code: Option<&str>) -> Vec<String> {
    let id = Value::from(id).to_string();
    let op = Value::from(op).to_string();
    let Some(c) = code else {
        return vec![format!("{BRIDGE}\n;window.__rlTest.run({id}, {op}, {args}, null);")];
    };
    vec![format!("{BRIDGE}\n;window.__rlTest.expectEval({id});"), format!("window.__rlTest.run({id}, {op}, {args}, async (args) => {{\n{c}\n}});")]
}

/// Runs one step in the window and waits for its answer. Err is what to tell the caller.
pub async fn run(app: &AppHandle, op: &str, args: &Value, code: Option<&str>, wait: Duration) -> Result<Value, String> {
    let Some(w) = app.get_webview_window("main") else {
        return Err("The launcher's window isn't there (it may be starting again).".into());
    };
    let id = new_id();
    let (tx, rx) = oneshot::channel();
    PENDING.lock().unwrap().get_or_insert_with(HashMap::new).insert(id.clone(), tx);
    for script in scripts_for(&id, op, args, code) {
        if let Err(e) = w.eval(script) {
            forget(&id);
            return Err(format!("Couldn't run that in the window: {e}"));
        }
    }
    let answer = tokio::time::timeout(wait, rx).await;
    forget(&id);
    match answer {
        Ok(Ok(r)) if r.ok => Ok(r.value),
        Ok(Ok(r)) => Err(r.value.as_str().map(String::from).unwrap_or_else(|| r.value.to_string())),
        Ok(Err(_)) => Err("The window's answer was lost.".into()),
        Err(_) if code.is_some() => Err(format!("The code didn't finish in {} s (a promise that never settles?).", wait.as_secs())),
        Err(_) => Err(format!("The window didn't answer in {} s (it may be busy or reloading).", wait.as_secs())),
    }
}

/// The page's answer to a call (the rl-test-reply event's payload).
pub fn reply(payload: &str) {
    let Ok(v) = serde_json::from_str::<Value>(payload) else { return };
    let Some(id) = v.get("id").and_then(Value::as_str) else { return };
    let tx = PENDING.lock().unwrap().as_mut().and_then(|m| m.remove(id));
    let Some(tx) = tx else { return };
    let ok = v.get("ok").and_then(Value::as_bool).unwrap_or(false);
    let value = v.get("value").cloned().unwrap_or(Value::Null);
    let _ = tx.send(Reply { ok, value });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ids_differ() {
        assert_ne!(new_id(), new_id());
    }

    #[test]
    fn a_step_names_its_id_op_and_args() {
        let s = scripts_for("abc", "click", &json!({"text": "Play \"now\""}), None);
        assert_eq!(s.len(), 1);
        assert!(s[0].ends_with(r#"window.__rlTest.run("abc", "click", {"text":"Play \"now\""}, null);"#));
    }

    #[test]
    fn eval_code_is_its_own_script_after_the_bridge() {
        let s = scripts_for("abc", "eval", &json!({}), Some("return 1 + 1;"));
        assert_eq!(s.len(), 2);
        assert!(s[0].ends_with(r#"window.__rlTest.expectEval("abc");"#));
        assert_eq!(s[1], "window.__rlTest.run(\"abc\", \"eval\", {}, async (args) => {\nreturn 1 + 1;\n});");
    }

    #[tokio::test]
    async fn an_answer_reaches_the_call_waiting_for_it() {
        let (tx, rx) = oneshot::channel();
        PENDING.lock().unwrap().get_or_insert_with(HashMap::new).insert("t1".into(), tx);
        reply(r#"{"id":"t1","ok":true,"value":{"view":"home"}}"#);
        let r = rx.await.unwrap();
        assert!(r.ok);
        assert_eq!(r.value, json!({"view": "home"}));
    }

    #[test]
    fn an_answer_with_an_unknown_id_is_ignored() {
        reply(r#"{"id":"nobody","ok":true,"value":1}"#);
        reply("not json");
    }
}

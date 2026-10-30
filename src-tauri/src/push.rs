//! New builds the moment they're out. The website names a push channel in launcher/version.json
//! ("push": an https address that streams server-sent events, an ntfy.sh topic's /sse for one),
//! and the launcher keeps listening to it while it runs. A message there only says "look now":
//!
//! - "launcher" (or anything not understood): the launcher reads version.json again and, if
//!   there's a newer RecompLauncher, says so the same way as its own checks.
//! - "game <id>" (or JSON {"type":"game","id":...}): a recomp put out a release; launchers that
//!   have it installed look at its releases again and show the update.
//! - "catalog": the list changed; the window reloads it.
//!
//! Nothing in a message is trusted beyond that: what to install and its checksum still come from
//! recomplauncher.com. Looks are spread over a few seconds so every launcher doesn't ask at once,
//! and there's at most one of each kind a minute, whatever arrives. "Look for updates when the
//! launcher opens" off in Settings turns the listening off too.

use crate::App;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Debug, PartialEq)]
enum Push {
    Launcher,
    Game(String),
    Catalog,
}

/// What one event's data asks for. Keep-alives and the channel's own notices ask for nothing.
fn read(data: &str) -> Option<Push> {
    let data = data.trim();
    if data.is_empty() {
        return None;
    }
    // ntfy wraps what was published: {"event":"message","message":"game zelda64recomp"}.
    let (text, v) = match serde_json::from_str::<Value>(data) {
        Ok(v) if v.is_object() => {
            let ev = v.get("event").and_then(Value::as_str).unwrap_or("message");
            if ev != "message" {
                return None;
            }
            match v.get("message").and_then(Value::as_str) {
                Some(m) => (m.trim().to_string(), serde_json::from_str::<Value>(m).ok().filter(Value::is_object)),
                None => (String::new(), Some(v)),
            }
        }
        _ => (data.to_string(), None),
    };
    if let Some(v) = v {
        let kind = v.get("type").and_then(Value::as_str).unwrap_or("");
        let id = v.get("id").and_then(Value::as_str).unwrap_or("").trim();
        return Some(match kind {
            "game" if !id.is_empty() => Push::Game(id.to_string()),
            "catalog" => Push::Catalog,
            _ => Push::Launcher,
        });
    }
    let mut w = text.split_whitespace();
    Some(match (w.next().unwrap_or("").to_ascii_lowercase().as_str(), w.next()) {
        ("game", Some(id)) => Push::Game(id.to_string()),
        ("catalog", _) => Push::Catalog,
        _ => Push::Launcher,
    })
}

/// The channel version.json names, if any (https only).
async fn channel(app: &AppHandle) -> Option<String> {
    let st = app.state::<App>();
    let url = format!("{}/launcher/version.json", crate::catalog::base());
    let v: Value = st.http.get(url).timeout(Duration::from_secs(15)).send().await.ok()?.json().await.ok()?;
    v.get("push").and_then(Value::as_str).map(str::trim).filter(|u| u.starts_with("https://")).map(String::from)
}

fn on(app: &AppHandle) -> bool {
    app.state::<App>().lib.lock().unwrap().settings.check_updates
}

/// A few seconds, different on every PC, so a push doesn't bring every launcher at once.
fn spread() -> Duration {
    Duration::from_millis(500 + (crate::usage::rand_byte() as u64) * 80)
}

async fn act(app: AppHandle, p: Push) {
    tokio::time::sleep(spread()).await;
    match p {
        Push::Launcher => {
            let st = app.state::<App>();
            if let Ok(Some(v)) = crate::launcher_update(st).await {
                let ver = v.get("version").and_then(Value::as_str).unwrap_or("").to_string();
                let hidden = app.get_webview_window("main").map(|w| !w.is_visible().unwrap_or(true)).unwrap_or(true);
                if hidden {
                    crate::ingame::notify(&app, &format!("RecompLauncher {ver} is out"), "Open RecompLauncher to update.");
                }
                let _ = app.emit("launcher-update", v);
            }
        }
        Push::Game(id) => {
            let st = app.state::<App>();
            let g = st.lib.lock().unwrap().installed.get(&id).cloned();
            let Some(g) = g.filter(|g| !g.repo.is_empty() && !g.local) else { return };
            // Its releases looked at again now, past the usual ten-minute cache.
            let _ = crate::catalog::releases(&st.http, &st.dir, &g.repo, true).await;
            let _ = app.emit("push", json!({"type": "game", "id": id}));
        }
        Push::Catalog => {
            let _ = app.emit("push", json!({"type": "catalog"}));
        }
    }
}

/// Listens for as long as the launcher runs, connecting again after a drop (sooner after a
/// connection that lasted, later after one that failed straight away).
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut wait = 5u64;
        let mut last: HashMap<String, Instant> = HashMap::new();
        loop {
            if !on(&app) {
                tokio::time::sleep(Duration::from_secs(300)).await;
                continue;
            }
            let Some(url) = channel(&app).await else {
                tokio::time::sleep(Duration::from_secs(1800)).await;
                continue;
            };
            let began = Instant::now();
            let st = app.state::<App>();
            let resp = st.http.get(&url).header("Accept", "text/event-stream").send().await;
            if let Ok(mut r) = resp.and_then(|r| r.error_for_status()) {
                let mut buf = String::new();
                // A channel that's gone quiet for longer than its keep-alives is dropped and made again.
                while let Ok(Ok(Some(chunk))) = tokio::time::timeout(Duration::from_secs(150), r.chunk()).await {
                    buf.push_str(&String::from_utf8_lossy(&chunk));
                    while let Some(i) = buf.find('\n') {
                        let line = buf[..i].trim_end_matches('\r').to_string();
                        buf.drain(..=i);
                        let Some(data) = line.strip_prefix("data:") else { continue };
                        let Some(p) = read(data) else { continue };
                        let key = match &p {
                            Push::Launcher => "launcher".to_string(),
                            Push::Game(id) => format!("game {id}"),
                            Push::Catalog => "catalog".to_string(),
                        };
                        if last.get(&key).map(|t| t.elapsed() < Duration::from_secs(60)).unwrap_or(false) {
                            continue;
                        }
                        last.insert(key, Instant::now());
                        tauri::async_runtime::spawn(act(app.clone(), p));
                    }
                    if buf.len() > 65_536 {
                        buf.clear();
                    }
                    if !on(&app) {
                        break;
                    }
                }
            }
            wait = if began.elapsed() > Duration::from_secs(120) { 5 } else { (wait * 2).min(600) };
            tokio::time::sleep(Duration::from_secs(wait) + spread()).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_pushes() {
        assert_eq!(read(r#"{"id":"x","time":1,"event":"keepalive","topic":"t"}"#), None);
        assert_eq!(read(r#"{"id":"x","time":1,"event":"open","topic":"t"}"#), None);
        assert_eq!(read(r#"{"event":"message","message":"launcher"}"#), Some(Push::Launcher));
        assert_eq!(read(r#"{"event":"message","message":"game zelda64recomp"}"#), Some(Push::Game("zelda64recomp".into())));
        assert_eq!(read(r#"{"event":"message","message":"{\"type\":\"catalog\"}"}"#), Some(Push::Catalog));
        assert_eq!(read(r#"{"type":"game","id":"botw-native"}"#), Some(Push::Game("botw-native".into())));
        assert_eq!(read("launcher"), Some(Push::Launcher));
        assert_eq!(read(""), None);
    }
}

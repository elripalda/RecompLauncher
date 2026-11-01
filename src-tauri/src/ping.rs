//! Anonymous usage stats for recomplauncher.com's dashboard: the launcher opening and closing, a
//! heartbeat while it's open, and games starting and stopping. Each ping carries a random ID made
//! once on this PC (never tied to the account), the launcher's version, the system, Desktop or Big
//! Picture mode, whether someone is signed in, and the recomp being played. No names, no paths.
//! Settings > General > "Share anonymous usage stats" turns it all off. Every error is dropped.
//! The heartbeat is every half hour at the most often (the website can ask for less often, never
//! more), and nothing is sent while the site is resting after trouble.

use crate::App;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Manager};

/// Seconds between heartbeats: half an hour, or longer if the website asks.
const DEFAULT_NEXT: u64 = 1800;
const MAX_NEXT: u64 = 86_400;

fn url() -> String {
    format!("{}/api/launcher-ping.php", crate::catalog::base())
}

/// Desktop or Big Picture, as the window last said.
static MODE: Mutex<String> = Mutex::new(String::new());
/// Seconds to the next heartbeat, as the website last asked.
static NEXT: AtomicU64 = AtomicU64::new(DEFAULT_NEXT);
static OPENED: AtomicBool = AtomicBool::new(false);
/// Games being played now: id -> (session, start).
static PLAYING: Mutex<Option<HashMap<String, (String, std::time::Instant)>>> = Mutex::new(None);

fn on(st: &App) -> bool {
    let s = &st.lib.lock().unwrap().settings;
    s.data_asked && s.share_stats
}

fn uuid() -> String {
    let mut b = [0u8; 16];
    for x in b.iter_mut() {
        *x = crate::usage::rand_byte();
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h = hex::encode(b);
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// The random ID for these stats: made once, kept in the data folder.
pub fn install(dir: &Path) -> String {
    let f = dir.join("stats.id");
    if let Ok(s) = std::fs::read_to_string(&f) {
        let s = s.trim().to_string();
        if s.len() == 36 && s.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
            return s;
        }
    }
    let id = uuid();
    let _ = std::fs::write(&f, &id);
    id
}

fn os_version() -> String {
    static V: OnceLock<String> = OnceLock::new();
    V.get_or_init(|| {
        #[cfg(windows)]
        {
            // 10.0.22631
            crate::win::os_version()
        }
        #[cfg(not(windows))]
        {
            std::fs::read_to_string("/proc/sys/kernel/osrelease").map(|s| s.trim().to_string()).unwrap_or_default()
        }
    })
    .clone()
}

fn mode() -> String {
    let m = MODE.lock().unwrap().clone();
    if m == "tv" {
        m
    } else {
        "desk".into()
    }
}

/// The recomp as the catalog names it, with the version installed. Local builds aren't sent.
fn game(st: &App, id: &str) -> Option<Value> {
    let lib = st.lib.lock().unwrap();
    let g = lib.installed.get(id)?;
    if g.local {
        return None;
    }
    Some(json!({"id": id, "name": g.name, "version": g.tag}))
}

fn body(st: &App, event: &str) -> Value {
    json!({
        "install": install(&st.dir),
        "event": event,
        "version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "os_version": os_version(),
        "mode": mode(),
        "signed_in": crate::cloud::signed_in(&st.dir),
    })
}

async fn post(app: &AppHandle, b: Value) {
    let st = app.state::<App>();
    if !on(&st) {
        return;
    }
    let u = url();
    let r = crate::net::send(st.http.post(&u).json(&b).timeout(crate::net::QUICK), &u, false).await;
    if let Ok(r) = r {
        if let Ok(v) = r.json::<Value>().await {
            if let Some(n) = v.get("next").and_then(Value::as_u64) {
                NEXT.store(n.clamp(DEFAULT_NEXT, MAX_NEXT), Ordering::Relaxed);
            }
        }
    }
}

fn send(app: &AppHandle, b: Value) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { post(&app, b).await });
}

/// What's being played, for a heartbeat: the game that started last.
fn now_playing(st: &App, b: &mut Value) {
    let p = PLAYING.lock().unwrap();
    let Some(m) = p.as_ref() else { return };
    if let Some((id, (session, _))) = m.iter().max_by_key(|(_, (_, t))| *t) {
        if let Some(g) = game(st, id) {
            b["game"] = g;
            b["session"] = json!(session);
        }
    }
}

/// The window is up: "open", then a heartbeat on the website's schedule while it runs.
#[tauri::command]
pub fn usage_open(app: AppHandle, mode: String) {
    *MODE.lock().unwrap() = mode;
    if OPENED.swap(true, Ordering::SeqCst) {
        return;
    }
    let st = app.state::<App>();
    send(&app, body(&st, "open"));
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(crate::net::spread(NEXT.load(Ordering::Relaxed), 0.1)).await;
            let st = app2.state::<App>();
            let mut b = body(&st, "heartbeat");
            now_playing(&st, &mut b);
            post(&app2, b).await;
        }
    });
}

/// Desktop or Big Picture mode, when it changes.
#[tauri::command]
pub fn usage_mode(mode: String) {
    *MODE.lock().unwrap() = mode;
}

pub fn game_start(app: &AppHandle, id: &str) {
    let st = app.state::<App>();
    let Some(g) = game(&st, id) else { return };
    let session = uuid();
    PLAYING.lock().unwrap().get_or_insert_with(HashMap::new).insert(id.to_string(), (session.clone(), std::time::Instant::now()));
    let mut b = body(&st, "game_start");
    b["game"] = g;
    b["session"] = json!(session);
    send(app, b);
}

pub fn game_stop(app: &AppHandle, id: &str, secs: u64) {
    let st = app.state::<App>();
    let Some((session, _)) = PLAYING.lock().unwrap().as_mut().and_then(|m| m.remove(id)) else { return };
    let Some(g) = game(&st, id) else { return };
    let mut b = body(&st, "game_stop");
    b["game"] = g;
    b["session"] = json!(session);
    b["seconds"] = json!(secs);
    send(app, b);
}

/// Quitting: "close", given two seconds at most.
pub fn close(app: &AppHandle) {
    if !OPENED.load(Ordering::SeqCst) {
        return;
    }
    let st = app.state::<App>();
    if !on(&st) {
        return;
    }
    let b = body(&st, "close");
    let app2 = app.clone();
    let _ = tauri::async_runtime::block_on(async move { tokio::time::timeout(std::time::Duration::from_secs(2), post(&app2, b)).await });
}

#[cfg(test)]
mod tests {
    #[test]
    fn ids_are_uuids() {
        let a = super::uuid();
        let b = super::uuid();
        assert_eq!(a.len(), 36);
        assert_eq!(a.as_bytes()[14], b'4');
        assert!(a.split('-').map(str::len).eq([8, 4, 4, 4, 12]));
        assert_ne!(a, b);
    }
}

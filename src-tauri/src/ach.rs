//! Recomp Achievements: the launcher's side. Only for accounts the website lets in (test accounts
//! for now): everything here stays off, and no game gets a channel, until ach.php says yes.
//!
//! A game started from here gets RL_SESSION, the name of a pipe (Windows) or socket (Linux, Mac)
//! made for that one start. A recomp with the achievements library connects to it and both
//! sides send one JSON object per line (format 1). The same channel is meant for the
//! online hub later, so it knows nothing about achievements beyond `handle`.
//!
//! Xbox 360 recomps on ReXGlue without the library: their own unlock file is watched instead
//! (achievements/<TITLE ID>.toml under the game's user data folder).
//!
//! Every unlock is saved here first (achievements/store.json in the app's data folder), shown in
//! a toast over the game, and sent to the player's account when it can be.

use crate::{library::now, App, Error};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

type Res<T> = Result<T, Error>;

const API: &str = "https://recompstore.com/api/ach.php";
/// How often a running game's session checks in with the website.
const CHECKIN_SECS: u64 = 180;
/// Unlocks this soon after a save loads are its old unlocks coming back, not new ones.
const AFTER_LOAD_SECS: u64 = 10;
/// Lines kept in a session's channel log (dev tools).
const LOG_LINES: usize = 400;
/// A line longer than this from a game is dropped (a whole list is the longest thing sent).
const MAX_LINE: usize = 1 << 20;

// ---------- who may use it ----------

/// 0 not known yet, 1 this account is let in, 2 it isn't (or nobody's signed in).
static ACCESS: AtomicU8 = AtomicU8::new(0);

pub fn allowed() -> bool {
    ACCESS.load(Ordering::Relaxed) == 1
}

fn access_file(dir: &Path) -> PathBuf {
    dir.join("achievements").join("access.json")
}

/// At start: what the website said last time, until it's asked again.
pub fn init(dir: &Path) {
    let ok = std::fs::read(access_file(dir)).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|v| v.get("ok").and_then(Value::as_bool)).unwrap_or(false);
    ACCESS.store(if ok && crate::cloud::token(dir).is_some() { 1 } else { 2 }, Ordering::Relaxed);
}

fn set_access(app: &AppHandle, dir: &Path, ok: bool) {
    let before = ACCESS.swap(if ok { 1 } else { 2 }, Ordering::Relaxed);
    let _ = std::fs::create_dir_all(dir.join("achievements"));
    let _ = std::fs::write(access_file(dir), json!({"ok": ok, "at": now()}).to_string());
    if before != ACCESS.load(Ordering::Relaxed) {
        let _ = app.emit("ach-access", json!({"allowed": ok}));
    }
}

/// Asks the website whether this account is a test account (a 403 means it isn't). Offline, the
/// last answer stands.
pub async fn check_access(app: &AppHandle) -> bool {
    let dir = app.state::<App>().dir.clone();
    if crate::cloud::token(&dir).is_none() {
        set_access(app, &dir, false);
        return false;
    }
    match call(app, "GET", "access", &[], None).await {
        Ok((200, v)) => set_access(app, &dir, v.get("ok").and_then(Value::as_bool).unwrap_or(false)),
        Ok((401 | 403, _)) => set_access(app, &dir, false),
        _ => {}
    }
    allowed()
}

// ---------- the website ----------

/// One call to ach.php with the account's token. The status comes back with the answer, so a
/// caller can tell "not a test account" (403) and "no list" (404) from a failure.
async fn call(app: &AppHandle, method: &str, a: &str, query: &[(&str, &str)], body: Option<Value>) -> Res<(u16, Value)> {
    let st = app.state::<App>();
    let token = crate::cloud::token(&st.dir).ok_or_else(|| Error::Msg("Not signed in.".into()))?;
    let mut q: Vec<(&str, &str)> = vec![("a", a)];
    q.extend_from_slice(query);
    let req = if method == "GET" { st.http.get(API) } else { st.http.post(API).json(&body.unwrap_or(Value::Null)) };
    let r = req.query(&q).header("X-RL-Token", token).timeout(Duration::from_secs(15)).send().await.map_err(|e| Error::Msg(format!("ach.php: {e}")))?;
    let code = r.status().as_u16();
    let v: Value = r.json().await.unwrap_or(Value::Null);
    if code == 403 {
        set_access(app, &st.dir, false);
    }
    Ok((code, v))
}

// ---------- what's kept on this PC ----------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Unlock {
    pub game: String,
    pub id: u32,
    /// memory, native or event.
    pub source: String,
    pub at: u64,
    pub session: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<u64>,
    pub casual: bool,
    /// Why it's Casual: "casual:<reason>" (the game said), "voids:<keys>" (a setting that voids
    /// unlocks was on), "earlier" (it had it before this session), "save_loaded" (right after a
    /// save loaded). Empty when nothing stopped it from being Ranked.
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub earlier: bool,
    #[serde(default)]
    pub synced: bool,
    /// What the website decided, once it has it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ranked: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Session {
    pub id: String,
    pub game: String,
    pub version: String,
    pub started: u64,
    #[serde(default)]
    pub checkins: Vec<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub casual: Option<String>,
    #[serde(default)]
    pub save_loaded: Vec<u64>,
    /// The website's id for it, once session-start went through.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    /// Check-ins the website already has (the rest go with the next call).
    #[serde(default)]
    pub sent_checkins: usize,
    /// The game sent its native achievements' names after session-start went: sent again with them.
    #[serde(default)]
    pub native_pending: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Store {
    #[serde(default)]
    unlocks: Vec<Unlock>,
    #[serde(default)]
    sessions: BTreeMap<String, Session>,
    /// Measured progress: game -> achievement -> (value, target).
    #[serde(default)]
    progress: BTreeMap<String, BTreeMap<u32, (u64, u64)>>,
    /// Rules a game said it can't check: game -> achievement -> why.
    #[serde(default)]
    broken: BTreeMap<String, BTreeMap<u32, String>>,
    /// Where each game's ReXGlue unlock file turned up (with %APPDATA% and the like for the
    /// player's own folders), for the catalog.
    #[serde(default)]
    found: BTreeMap<String, String>,
    /// The account's unlocks as the website has them (a=mine), per game.
    #[serde(default)]
    mine: BTreeMap<String, Value>,
    #[serde(default)]
    sound: Option<bool>,
    /// The game's own achievements as its runtime named them (native_list): id, title,
    /// description, gamerscore.
    #[serde(default)]
    native: BTreeMap<String, Vec<Value>>,
}

static STORE: Mutex<Option<Store>> = Mutex::new(None);
static DIR: OnceLock<PathBuf> = OnceLock::new();

fn store_file(dir: &Path) -> PathBuf {
    dir.join("achievements").join("store.json")
}

/// Works on the store (loaded the first time), and writes it back when `f` says it changed it.
fn with_store<T>(dir: &Path, f: impl FnOnce(&mut Store) -> (T, bool)) -> T {
    let _ = DIR.set(dir.to_path_buf());
    let mut g = STORE.lock().unwrap();
    if g.is_none() {
        *g = Some(std::fs::read(store_file(dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default());
    }
    let s = g.as_mut().unwrap();
    let (out, changed) = f(s);
    if changed {
        let file = store_file(dir);
        let _ = std::fs::create_dir_all(file.parent().unwrap());
        let tmp = file.with_extension("json.tmp");
        if std::fs::write(&tmp, serde_json::to_vec_pretty(s).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(&tmp, &file);
        }
    }
    out
}

// ---------- achievement lists ----------

/// A dev folder's list (achievements/dev-lists/<game>.json) wins over the website's, so a list
/// can be tried before it's on the site. Then the last copy from the website.
pub fn list_for(dir: &Path, game: &str) -> Option<Value> {
    let safe: String = game.chars().filter(|c| c.is_ascii_alphanumeric() || "-_.".contains(*c)).collect();
    if safe.is_empty() {
        return None;
    }
    let read = |p: PathBuf| std::fs::read(p).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).filter(|v| v.get("achievements").map(Value::is_array).unwrap_or(false) || v.get("source").and_then(Value::as_str) == Some("native"));
    read(dir.join("achievements").join("dev-lists").join(format!("{safe}.json"))).or_else(|| read(dir.join("achievements").join("lists").join(format!("{safe}.json"))))
}

/// The website's list for a game, kept for offline play. A 404 (no achievements for it) clears
/// the old copy.
async fn fetch_list(app: &AppHandle, game: &str) -> Option<Value> {
    let dir = app.state::<App>().dir.clone();
    let safe: String = game.chars().filter(|c| c.is_ascii_alphanumeric() || "-_.".contains(*c)).collect();
    let file = dir.join("achievements").join("lists").join(format!("{safe}.json"));
    match call(app, "GET", "list", &[("game", game)], None).await {
        Ok((200, v)) => {
            let list = v.get("list").cloned().filter(|l| l.is_object())?;
            let _ = std::fs::create_dir_all(file.parent().unwrap());
            let _ = std::fs::write(&file, serde_json::to_vec_pretty(&list).unwrap_or_default());
            Some(list)
        }
        Ok((404, _)) => {
            let _ = std::fs::remove_file(&file);
            None
        }
        _ => None,
    }
}

fn entries(list: &Value) -> Vec<Value> {
    list.get("achievements").and_then(Value::as_array).cloned().unwrap_or_default()
}

/// The list as the player sees it: a native list without titles takes the names the game's runtime
/// sent (native_list), and with no list at all those names are the list.
pub fn effective(dir: &Path, game: &str, list: Option<Value>) -> Option<Value> {
    let names = with_store(dir, |d| (d.native.get(game).cloned().unwrap_or_default(), false));
    if names.is_empty() {
        return list;
    }
    let mut l = list.unwrap_or_else(|| json!({"format": 1, "game": game, "source": "native", "console": "x360"}));
    let have = entries(&l);
    let filled: Vec<Value> = if have.is_empty() {
        names.iter().map(|n| { let mut e = n.clone(); if e.get("points").is_none() { e["points"] = json!(points_of(n)); } e }).collect()
    } else {
        have.into_iter()
            .map(|mut e| {
                let nid = num(&e, "native").or_else(|| num(&e, "id"));
                if let Some(n) = names.iter().find(|n| num(n, "id") == nid) {
                    for k in ["title", "description", "gamerscore"] {
                        if e.get(k).map(|v| v.is_null() || v.as_str() == Some("")).unwrap_or(true) {
                            if let Some(v) = n.get(k) {
                                e[k] = v.clone();
                            }
                        }
                    }
                    if e.get("points").is_none() {
                        e["points"] = json!(points_of(n));
                    }
                }
                e
            })
            .collect()
    };
    l["achievements"] = json!(filled);
    Some(l)
}

/// Every achievement in the game unlocked: by the list's ids, or a native list's "total".
fn all_unlocked(list: Option<&Value>, have: &std::collections::HashSet<u32>) -> bool {
    let ids: Vec<u32> = list.map(entries).unwrap_or_default().iter().filter_map(|a| num(a, "id")).map(|x| x as u32).collect();
    if !ids.is_empty() {
        return ids.iter().all(|i| have.contains(i));
    }
    list.and_then(|l| num(l, "total")).map(|t| t > 0 && have.len() as u64 >= t).unwrap_or(false)
}

fn num(v: &Value, k: &str) -> Option<u64> {
    v.get(k).and_then(|x| x.as_u64().or_else(|| x.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64)).or_else(|| x.as_str().and_then(|s| s.trim().parse().ok())))
}

fn entry_of(list: Option<&Value>, id: u32) -> Option<Value> {
    list.map(entries).unwrap_or_default().into_iter().find(|a| num(a, "id") == Some(id as u64))
}

/// Points on the 5/10/25/50 scale. A native list's gamerscore is brought onto it.
pub fn points_of(a: &Value) -> u64 {
    if let Some(p) = num(a, "points") {
        return p;
    }
    num(a, "gamerscore").map(gamerscore_points).unwrap_or(0)
}

pub fn gamerscore_points(g: u64) -> u64 {
    match g {
        0 => 0,
        1..=10 => 5,
        11..=25 => 10,
        26..=50 => 25,
        _ => 50,
    }
}

/// Whether the installed version is one the list's rules were checked on ("versions"): ">=1.4.0",
/// "1.4.x", "1.4.0, 1.4.2", ">=1.2 <2.0" (spaces join, commas and "||" are either-or). Nothing
/// said means any version.
pub fn version_ok(spec: &str, tag: &str) -> bool {
    let spec = spec.trim();
    if spec.is_empty() || spec == "*" {
        return true;
    }
    let have = crate::boot::ver_key(tag);
    spec.split(|c| c == ',' || c == '|').map(str::trim).filter(|x| !x.is_empty()).any(|alt| {
        alt.split_whitespace().all(|one| {
            let (op, v) = [">=", "<=", ">", "<", "="].iter().find_map(|o| one.strip_prefix(o).map(|r| (*o, r.trim()))).unwrap_or(("=", one));
            if v.contains(['x', 'X', '*']) {
                let want: Vec<&str> = v.trim_start_matches(['v', 'V']).split('.').collect();
                let parts: Vec<String> = tag.trim().trim_start_matches(['v', 'V']).split(['.', '-']).map(str::to_string).collect();
                return want.iter().enumerate().all(|(i, w)| matches!(*w, "x" | "X" | "*") || parts.get(i).map(|p| p == w).unwrap_or(false));
            }
            let want = crate::boot::ver_key(v);
            match op {
                ">=" => have >= want,
                "<=" => have <= want,
                ">" => have > want,
                "<" => have < want,
                _ => have[..3] == want[..3],
            }
        })
    })
}

// ---------- a game's session ----------

struct Live {
    session: String,
    name: String,
    version: String,
    /// Why the list's rules aren't sent this time (a version they weren't checked on).
    paused: Option<String>,
    tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    connected: bool,
    hello: Option<Value>,
    log: VecDeque<(u64, &'static str, String)>,
    peeks: HashMap<u64, tokio::sync::oneshot::Sender<String>>,
    list: Option<Value>,
    voided: Vec<String>,
    casual: Option<String>,
    save_loaded: Option<Instant>,
    stop: Vec<tokio::sync::oneshot::Sender<()>>,
    /// Unlocks from a save that just loaded, shown together in one toast.
    burst: Vec<Value>,
    #[cfg(unix)]
    socket: Option<PathBuf>,
}

static LIVE: Mutex<Option<HashMap<String, Live>>> = Mutex::new(None);
static SEQ: AtomicU64 = AtomicU64::new(1);

fn live<T>(f: impl FnOnce(&mut HashMap<String, Live>) -> T) -> T {
    let mut g = LIVE.lock().unwrap();
    f(g.get_or_insert_with(HashMap::new))
}

fn new_session_id() -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let mix = t ^ ((std::process::id() as u128) << 64) ^ (SEQ.fetch_add(1, Ordering::Relaxed) as u128).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    format!("{:016x}", (mix as u64) ^ ((mix >> 64) as u64))
}

fn log_line(game: &str, dir: &'static str, line: &str) {
    live(|m| {
        if let Some(l) = m.get_mut(game) {
            l.log.push_back((now(), dir, line.chars().take(4000).collect()));
            while l.log.len() > LOG_LINES {
                l.log.pop_front();
            }
        }
    });
}

/// Sends one message to the game, if it's connected.
fn send(app: &AppHandle, game: &str, msg: Value) {
    let line = msg.to_string();
    let sent = live(|m| m.get(game).and_then(|l| l.tx.as_ref()).map(|tx| tx.send(line.clone()).is_ok()).unwrap_or(false));
    if sent {
        log_line(game, ">", &line);
        let _ = app.emit("ach-log", json!({"game": game}));
    }
}

/// Setting keys from the list's "voids" that are on for this game: unlocks while they are can't
/// be Ranked. A key is a setting's id in its profile, or a key in one of its settings files.
fn voided(prof: Option<&crate::profile::Profile>, install: &Path, keys: &[String]) -> Vec<String> {
    let Some(p) = prof else { return vec![] };
    let on = |v: &str| { let v = v.trim().trim_matches('"').to_ascii_lowercase(); !matches!(v.as_str(), "" | "0" | "false" | "off" | "no" | "null" | "disabled") };
    keys.iter()
        .filter(|k| {
            if let Some(s) = p.settings.iter().find(|s| &s.id == *k) {
                return p.own_value(s, install).map(|v| on(&v)).unwrap_or(false);
            }
            p.config.iter().any(|c| p.config_file(&c.id, install).and_then(|f| f.get(k)).map(|v| on(&v)).unwrap_or(false))
        })
        .cloned()
        .collect()
}

pub struct Start<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub version: &'a str,
    pub install: &'a Path,
    pub origin: &'a str,
    pub prof: Option<&'a crate::profile::Profile>,
    pub catalog_folder: Option<String>,
}

/// Before a game starts: its session and channel. Returns the channel's name for RL_SESSION, or
/// None when achievements are off for this account (the game starts as it always did).
pub fn start(app: &AppHandle, s: Start) -> Option<String> {
    let st = app.state::<App>();
    if !allowed() || crate::cloud::token(&st.dir).is_none() {
        return None;
    }
    end_now(app, s.id);
    let session = new_session_id();
    let started = now();
    let list = list_for(&st.dir, s.id);
    let voids: Vec<String> = list.as_ref().and_then(|l| l.get("voids")).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
    let voided = voided(s.prof, s.install, &voids);
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (name, listener) = match open_channel(&session) {
        Ok(x) => x,
        Err(e) => {
            crate::trail::note(format!("Achievements {}: no channel ({e})", s.id));
            return None;
        }
    };
    with_store(&st.dir, |d| {
        d.sessions.insert(session.clone(), Session { id: session.clone(), game: s.id.into(), version: s.version.into(), started, checkins: vec![started], casual: (!voided.is_empty()).then(|| format!("voids:{}", voided.join(","))), ..Default::default() });
        // Old sessions with nothing left to send.
        let unsent: std::collections::HashSet<String> = d.unlocks.iter().filter(|u| !u.synced).map(|u| u.session.clone()).collect();
        let mut ended: Vec<(u64, String)> = d.sessions.values().filter(|x| x.ended.is_some() && !unsent.contains(&x.id) && x.server.is_some()).map(|x| (x.started, x.id.clone())).collect();
        ended.sort();
        if ended.len() > 200 {
            for (_, id) in ended.iter().take(ended.len() - 200) {
                d.sessions.remove(id);
            }
        }
        ((), true)
    });
    live(|m| {
        m.insert(s.id.to_string(), Live {
            session: session.clone(), name: s.name.into(), version: s.version.into(), paused: None, tx: None, connected: false, hello: None, log: VecDeque::new(), peeks: HashMap::new(), list: list.clone(), voided: voided.clone(),
            casual: None, save_loaded: None, stop: vec![stop_tx], burst: vec![],
            #[cfg(unix)]
            socket: Some(PathBuf::from(&name)),
        })
    });
    log_line(s.id, "*", &format!("session {session} on {name}{}", if voided.is_empty() { String::new() } else { format!(" (Casual: {} on)", voided.join(", ")) }));
    crate::trail::note(format!("Achievements {}: session started", s.id));
    let (a, game) = (app.clone(), s.id.to_string());
    tauri::async_runtime::spawn(serve(a, game, listener, stop_rx));
    // The website hears about it now, and every few minutes while it runs.
    let (a, game, sid) = (app.clone(), s.id.to_string(), session.clone());
    let (tick_tx, mut tick_rx) = tokio::sync::oneshot::channel::<()>();
    live(|m| { if let Some(l) = m.get_mut(&game) { l.stop.push(tick_tx); } });
    tauri::async_runtime::spawn(async move {
        sync(&a).await;
        loop {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(CHECKIN_SECS)) => {}
                _ = &mut tick_rx => break,
            }
            let dir = a.state::<App>().dir.clone();
            with_store(&dir, |d| { if let Some(x) = d.sessions.get_mut(&sid) { x.checkins.push(now()); } ((), true) });
            checkin(&a, &sid, json!({})).await;
        }
    });
    // A ReXGlue recomp (Xbox 360) without the library: its own unlock file.
    if s.origin == "x360" {
        let (rx_tx, rx_rx) = tokio::sync::oneshot::channel::<()>();
        live(|m| { if let Some(l) = m.get_mut(s.id) { l.stop.push(rx_tx); } });
        let w = RexWatch { game: s.id.into(), install: s.install.to_path_buf(), started, catalog_folder: s.catalog_folder.clone() };
        tauri::async_runtime::spawn(rex_watch(app.clone(), w, rx_rx));
    }
    // A fresh list for next time (and this time, if it arrives before the game says hello).
    let (a, game) = (app.clone(), s.id.to_string());
    tauri::async_runtime::spawn(async move {
        if let Some(l) = fetch_list(&a, &game).await {
            if list_for(&a.state::<App>().dir, &game).is_some() {
                live(|m| { if let Some(x) = m.get_mut(&game) { if x.list.is_none() { x.list = Some(l); } } });
            }
        }
    });
    Some(name)
}

/// The game didn't start after all: its session goes without a trace on the website.
pub fn abort(app: &AppHandle, game: &str) {
    let sid = live(|m| m.get(game).map(|l| l.session.clone()));
    end_now(app, game);
    if let Some(sid) = sid {
        with_store(&app.state::<App>().dir, |d| { d.sessions.remove(&sid); ((), true) });
    }
}

fn end_now(app: &AppHandle, game: &str) -> Option<String> {
    let l = live(|m| m.remove(game))?;
    for s in l.stop {
        let _ = s.send(());
    }
    #[cfg(unix)]
    if let Some(p) = &l.socket {
        let _ = std::fs::remove_file(p);
    }
    with_store(&app.state::<App>().dir, |d| {
        if let Some(x) = d.sessions.get_mut(&l.session) {
            x.ended = Some(now());
            x.checkins.push(now());
        }
        ((), true)
    });
    Some(l.session)
}

/// The game closed: its channel goes, a last check-in, and what's left is sent.
pub fn end(app: &AppHandle, game: &str) {
    let Some(sid) = end_now(app, game) else { return };
    crate::trail::note(format!("Achievements {game}: session ended"));
    let a = app.clone();
    tauri::async_runtime::spawn(async move {
        checkin(&a, &sid, json!({})).await;
        sync(&a).await;
    });
}

// ---------- the channel ----------

#[cfg(windows)]
type Listener = tokio::net::windows::named_pipe::NamedPipeServer;
#[cfg(unix)]
type Listener = tokio::net::UnixListener;

#[cfg(windows)]
fn open_channel(session: &str) -> std::io::Result<(String, Listener)> {
    let name = format!(r"\\.\pipe\RecompLauncher-{session}");
    let server = tokio::net::windows::named_pipe::ServerOptions::new().first_pipe_instance(true).max_instances(2).create(&name)?;
    Ok((name, server))
}

#[cfg(unix)]
fn open_channel(session: &str) -> std::io::Result<(String, Listener)> {
    // Short: a socket's path has to fit in about 100 characters (Mac's temp folder is long).
    let dir = std::env::temp_dir().join("RecompLauncher");
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    let path = dir.join(format!("{}.sock", &session[..12.min(session.len())]));
    let _ = std::fs::remove_file(&path);
    let l = tokio::net::UnixListener::bind(&path)?;
    Ok((path.to_string_lossy().into_owned(), l))
}

/// Waits for the game to connect (it may never), then reads its lines until it goes. A game that
/// reconnects is taken again.
async fn serve(app: AppHandle, game: String, listener: Listener, mut stop: tokio::sync::oneshot::Receiver<()>) {
    #[cfg(windows)]
    {
        let mut server = listener;
        loop {
            tokio::select! {
                r = server.connect() => { if r.is_err() { break; } }
                _ = &mut stop => return,
            }
            let pipe = server;
            // A new instance for a reconnect, before this one is read.
            let sid = live(|m| m.get(&game).map(|l| l.session.clone()));
            let Some(sid) = sid else { return };
            let next = tokio::net::windows::named_pipe::ServerOptions::new().max_instances(2).create(format!(r"\\.\pipe\RecompLauncher-{sid}"));
            if !talk(&app, &game, pipe, &mut stop).await {
                return;
            }
            match next {
                Ok(n) => server = n,
                Err(_) => return,
            }
        }
    }
    #[cfg(unix)]
    {
        loop {
            let conn = tokio::select! {
                r = listener.accept() => match r { Ok((c, _)) => c, Err(_) => return },
                _ = &mut stop => return,
            };
            if !talk(&app, &game, conn, &mut stop).await {
                return;
            }
        }
    }
}

/// One connection: lines both ways. False when the session ended while it was open.
async fn talk<C>(app: &AppHandle, game: &str, conn: C, stop: &mut tokio::sync::oneshot::Receiver<()>) -> bool
where
    C: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + 'static,
{
    use tokio::io::AsyncWriteExt;
    let (rd, mut wr) = tokio::io::split(conn);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    live(|m| { if let Some(l) = m.get_mut(game) { l.tx = Some(tx); l.connected = true; } });
    log_line(game, "*", "connected");
    let _ = app.emit("ach-log", json!({"game": game}));
    let writer = tauri::async_runtime::spawn(async move {
        while let Some(line) = rx.recv().await {
            if wr.write_all(line.as_bytes()).await.is_err() || wr.write_all(b"\n").await.is_err() || wr.flush().await.is_err() {
                break;
            }
        }
    });
    let mut reader = tokio::io::BufReader::new(rd);
    let mut buf = Vec::new();
    let ended = loop {
        buf.clear();
        let r = tokio::select! {
            r = read_line(&mut reader, &mut buf) => r,
            _ = &mut *stop => break true,
        };
        match r {
            Ok(0) | Err(_) => break false,
            Ok(_) => {
                let line = String::from_utf8_lossy(&buf).trim().to_string();
                if line.is_empty() {
                    continue;
                }
                log_line(game, "<", &line);
                let _ = app.emit("ach-log", json!({"game": game}));
                match serde_json::from_str::<Value>(&line) {
                    Ok(v) if v.is_object() => handle(app, game, &v),
                    _ => log_line(game, "!", "not a JSON object"),
                }
            }
        }
    };
    live(|m| { if let Some(l) = m.get_mut(game) { l.tx = None; l.connected = false; } });
    writer.abort();
    log_line(game, "*", "disconnected");
    let _ = app.emit("ach-log", json!({"game": game}));
    !ended
}

/// One line, up to MAX_LINE bytes (a longer one is read through and dropped).
async fn read_line<R: tokio::io::AsyncBufRead + Unpin>(r: &mut R, out: &mut Vec<u8>) -> std::io::Result<usize> {
    use tokio::io::AsyncBufReadExt;
    let (mut total, mut over) = (0usize, false);
    loop {
        let (done, used) = {
            let avail = r.fill_buf().await?;
            if avail.is_empty() {
                if over {
                    out.clear();
                }
                return Ok(total);
            }
            let (part, done, used) = match avail.iter().position(|b| *b == b'\n') {
                Some(i) => (&avail[..i], true, i + 1),
                None => (avail, false, avail.len()),
            };
            if !over && out.len() + part.len() <= MAX_LINE {
                out.extend_from_slice(part);
            } else {
                over = true;
            }
            (done, used)
        };
        r.consume(used);
        total += used;
        if done {
            if over {
                out.clear();
            }
            return Ok(total);
        }
    }
}

/// A message from the game ("From the recomp", format 1).
fn handle(app: &AppHandle, game: &str, m: &Value) {
    let dir = app.state::<App>().dir.clone();
    let t = m.get("t").and_then(Value::as_str).unwrap_or("");
    let id = num(m, "id").map(|x| x as u32);
    match t {
        "hello" => {
            let list = live(|x| x.get_mut(game).map(|l| { l.hello = Some(m.clone()); l.list.clone() })).flatten().or_else(|| list_for(&dir, game));
            if let Some(g) = m.get("game").and_then(Value::as_str).filter(|g| !g.is_empty() && *g != game) {
                log_line(game, "!", &format!("the game says it's \"{g}\""));
            }
            // The launcher shows the toast; the runtime keeps its own quiet.
            send(app, game, json!({"t": "quiet"}));
            let version = live(|x| x.get(game).map(|l| l.version.clone())).unwrap_or_default();
            let checked = list.as_ref().and_then(|l| l.get("versions")).and_then(Value::as_str).unwrap_or("").to_string();
            let list = list.filter(|_| {
                let ok = version_ok(&checked, &version);
                if !ok {
                    let why = format!("its rules were checked on {checked}, and this is {version}");
                    log_line(game, "!", &format!("list paused: {why}"));
                    live(|x| { if let Some(l) = x.get_mut(game) { l.paused = Some(why); } });
                }
                ok
            });
            if let Some(list) = list {
                let rules: Vec<Value> = entries(&list).into_iter().filter_map(|a| { let tr = a.get("trigger").and_then(Value::as_str).filter(|s| !s.trim().is_empty())?; Some(json!({"id": num(&a, "id")?, "trigger": tr})) }).collect();
                if !rules.is_empty() {
                    let done = unlocked_ids(&dir, game);
                    send(app, game, json!({"t": "list", "achievements": rules, "unlocked": done}));
                }
            }
            let _ = app.emit("ach-changed", json!({"game": game}));
        }
        "list_ok" => {}
        "native_list" => {
            let names: Vec<Value> = m.get("achievements").and_then(Value::as_array).cloned().unwrap_or_default().into_iter()
                .filter_map(|a| {
                    let id = num(&a, "id")?;
                    let st = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or("").chars().take(200).collect::<String>();
                    Some(json!({"id": id, "title": st("title"), "description": st("description"), "gamerscore": num(&a, "gamerscore").unwrap_or(0)}))
                })
                .take(1000)
                .collect();
            if !names.is_empty() {
                let sid = live(|x| x.get(game).map(|l| l.session.clone()));
                with_store(&dir, |d| {
                    d.native.insert(game.into(), names);
                    if let Some(x) = sid.as_ref().and_then(|s| d.sessions.get_mut(s)) {
                        x.native_pending = true;
                    }
                    ((), true)
                });
                let a = app.clone();
                tauri::async_runtime::spawn(async move { sync(&a).await; });
                let _ = app.emit("ach-changed", json!({"game": game}));
            }
        }
        "unlock" => {
            if let Some(id) = id {
                record(app, game, id, "memory", num(m, "frame"), false, None);
            }
        }
        "progress" => {
            if let (Some(id), Some(v), Some(tg)) = (id, num(m, "value"), num(m, "target")) {
                with_store(&dir, |d| { d.progress.entry(game.into()).or_default().insert(id, (v, tg)); ((), true) });
                let _ = app.emit("ach-changed", json!({"game": game}));
            }
        }
        "native" => {
            if let Some(nid) = id {
                // A native list can give its achievements its own ids: "native" says which is which.
                let list = live(|x| x.get(game).and_then(|l| l.list.clone())).or_else(|| list_for(&dir, game));
                let mapped = list.as_ref().map(entries).unwrap_or_default().iter().find(|a| num(a, "native") == Some(nid as u64)).and_then(|a| num(a, "id")).map(|x| x as u32).unwrap_or(nid);
                let earlier = m.get("earlier").and_then(Value::as_bool).unwrap_or(false);
                record(app, game, mapped, "native", num(m, "frame"), earlier, None);
            }
        }
        "event" => {
            let name = m.get("name").and_then(Value::as_str).unwrap_or("");
            let list = live(|x| x.get(game).and_then(|l| l.list.clone())).or_else(|| list_for(&dir, game));
            match list.as_ref().map(entries).unwrap_or_default().iter().find(|a| a.get("event").and_then(Value::as_str) == Some(name)).and_then(|a| num(a, "id")) {
                Some(aid) => record(app, game, aid as u32, "event", num(m, "frame"), false, None),
                None => log_line(game, "!", &format!("no achievement for event \"{name}\"")),
            }
        }
        "save_loaded" => {
            let sid = live(|x| x.get_mut(game).map(|l| { l.save_loaded = Some(Instant::now()); l.session.clone() }));
            if let Some(sid) = sid {
                with_store(&dir, |d| { if let Some(x) = d.sessions.get_mut(&sid) { x.save_loaded.push(now()); x.checkins.push(now()); } ((), true) });
                let a = app.clone();
                tauri::async_runtime::spawn(async move { checkin(&a, &sid, json!({"save_loaded": true})).await; });
            }
        }
        "casual" => {
            let reason = m.get("reason").and_then(Value::as_str).unwrap_or("casual").chars().take(40).collect::<String>();
            let sid = live(|x| x.get_mut(game).map(|l| { l.casual = Some(reason.clone()); l.session.clone() }));
            if let Some(sid) = sid {
                with_store(&dir, |d| { if let Some(x) = d.sessions.get_mut(&sid) { x.casual = Some(reason.clone()); x.checkins.push(now()); } ((), true) });
                let a = app.clone();
                tauri::async_runtime::spawn(async move { checkin(&a, &sid, json!({"casual": reason})).await; });
            }
        }
        "broken" => {
            if let Some(id) = id {
                let why = m.get("why").and_then(Value::as_str).unwrap_or("it reads memory this game doesn't have").chars().take(200).collect::<String>();
                with_store(&dir, |d| { d.broken.entry(game.into()).or_default().insert(id, why); ((), true) });
                let _ = app.emit("ach-changed", json!({"game": game}));
            }
        }
        "mem" => {
            if let Some(seq) = num(m, "seq") {
                let hex = m.get("hex").and_then(Value::as_str).unwrap_or("").to_string();
                if let Some(tx) = live(|x| x.get_mut(game).and_then(|l| l.peeks.remove(&seq))) {
                    let _ = tx.send(hex);
                }
            }
        }
        _ => log_line(game, "!", &format!("unknown message \"{t}\"")),
    }
}

fn unlocked_ids(dir: &Path, game: &str) -> Vec<u32> {
    with_store(dir, |d| {
        let mut v: Vec<u32> = d.unlocks.iter().filter(|u| u.game == game).map(|u| u.id).collect();
        v.sort();
        v.dedup();
        (v, false)
    })
}

/// An unlock: kept here first, then the toast, then the website.
fn record(app: &AppHandle, game: &str, id: u32, source: &str, frame: Option<u64>, earlier: bool, why: Option<&str>) {
    let dir = app.state::<App>().dir.clone();
    let Some((session, casual, voided, after_load, list, name)) = live(|m| m.get(game).map(|l| (l.session.clone(), l.casual.clone(), l.voided.clone(), l.save_loaded.map(|t| t.elapsed().as_secs() < AFTER_LOAD_SECS).unwrap_or(false), l.list.clone(), l.name.clone()))) else { return };
    let list = effective(&dir, game, list.or_else(|| list_for(&dir, game)));
    let reason = if earlier {
        "earlier".to_string()
    } else if let Some(c) = casual {
        format!("casual:{c}")
    } else if !voided.is_empty() {
        format!("voids:{}", voided.join(","))
    } else if after_load || why == Some("save_loaded") {
        "save_loaded".to_string()
    } else {
        why.unwrap_or("").to_string()
    };
    let u = Unlock { game: game.into(), id, source: source.into(), at: now(), session, frame, casual: !reason.is_empty(), reason: reason.clone(), earlier, ..Default::default() };
    let (fresh, all_done) = with_store(&dir, |d| {
        if d.unlocks.iter().any(|x| x.game == game && x.id == id) {
            return ((false, false), false);
        }
        d.unlocks.push(u.clone());
        let have: std::collections::HashSet<u32> = d.unlocks.iter().filter(|x| x.game == game).map(|x| x.id).collect();
        ((true, all_unlocked(list.as_ref(), &have)), true)
    });
    if !fresh {
        log_line(game, "*", &format!("{id} was already unlocked"));
        return;
    }
    crate::trail::note(format!("Achievements {game}: {id} unlocked ({source}{})", if reason.is_empty() { String::new() } else { format!(", Casual: {reason}") }));
    let _ = app.emit("ach-changed", json!({"game": game}));
    if !earlier {
        let e = entry_of(list.as_ref(), id);
        let title = e.as_ref().and_then(|a| a.get("title").and_then(Value::as_str)).map(str::to_string).unwrap_or_else(|| format!("Achievement {id}"));
        let card = json!({"title": title, "points": e.as_ref().map(points_of).unwrap_or(0), "game": name, "casual": !reason.is_empty()});
        if reason == "save_loaded" {
            // A save's old unlocks come in a rush: one toast for all of them.
            let first = live(|m| m.get_mut(game).map(|l| { l.burst.push(card.clone()); l.burst.len() == 1 })).unwrap_or(false);
            if first {
                let (a, g) = (app.clone(), game.to_string());
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    let cards = live(|m| m.get_mut(&g).map(|l| std::mem::take(&mut l.burst))).unwrap_or_default();
                    match cards.len() {
                        0 => {}
                        1 => toast(&a, cards[0].clone()),
                        n => toast(&a, json!({"title": format!("{n} achievements from your save"), "points": cards.iter().filter_map(|c| c.get("points").and_then(Value::as_u64)).sum::<u64>(), "game": cards[0].get("game"), "casual": true, "many": n})),
                    }
                });
            }
        } else {
            toast(app, card);
        }
        if all_done {
            toast(app, json!({"title": name, "mastered": true, "points": list.as_ref().map(entries).unwrap_or_default().iter().map(points_of).sum::<u64>()}));
        }
    }
    let a = app.clone();
    tauri::async_runtime::spawn(async move { sync(&a).await; });
}

// ---------- sending to the account ----------

static SYNCING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn checkin(app: &AppHandle, sid: &str, extra: Value) {
    let dir = app.state::<App>().dir.clone();
    let server = with_store(&dir, |d| (d.sessions.get(sid).and_then(|s| s.server.clone()), false));
    // Not on the website yet: the times wait here and go with session-start.
    let Some(server) = server else { return };
    let mut body = json!({"session": server});
    if let (Some(b), Some(e)) = (body.as_object_mut(), extra.as_object()) {
        for (k, v) in e {
            b.insert(k.clone(), v.clone());
        }
    }
    if let Ok((200..=299, _)) = call(app, "POST", "checkin", &[], Some(body)).await {
        with_store(&dir, |d| { if let Some(x) = d.sessions.get_mut(sid) { x.sent_checkins = x.checkins.len(); } ((), true) });
    }
}

/// Everything not on the website yet: sessions it hasn't heard of (with their start and check-in
/// times, so it can judge them), then their unlocks. One at a time; offline, it all waits.
pub async fn sync(app: &AppHandle) {
    let _one = SYNCING.lock().await;
    let dir = app.state::<App>().dir.clone();
    if !allowed() || crate::cloud::token(&dir).is_none() {
        return;
    }
    let (starts, sends) = with_store(&dir, |d| {
        let unsent: std::collections::HashSet<String> = d.unlocks.iter().filter(|u| !u.synced).map(|u| u.session.clone()).collect();
        let starts: Vec<Session> = d.sessions.values().filter(|s| (s.server.is_none() && (s.ended.is_none() || unsent.contains(&s.id))) || s.native_pending).cloned().collect();
        ((starts, unsent), false)
    });
    for s in starts {
        let mut body = json!({"game": s.game, "version": s.version, "started": s.started, "checkins": s.checkins, "local": s.id});
        if let Some(c) = &s.casual {
            body["casual"] = json!(c);
        }
        if !s.save_loaded.is_empty() {
            body["save_loaded"] = json!(s.save_loaded);
        }
        if let Some(e) = s.ended {
            body["ended"] = json!(e);
        }
        if let Some(f) = with_store(&dir, |d| (d.found.get(&s.game).cloned(), false)) {
            body["unlock_folder"] = json!(f);
        }
        // The game's own names for its achievements, as its runtime sent them. The same "local"
        // again gets the same session back, so this is safe to send a second time.
        if let Some(n) = with_store(&dir, |d| (d.native.get(&s.game).cloned(), false)) {
            body["native"] = json!(n);
        }
        match call(app, "POST", "session-start", &[], Some(body)).await {
            Ok((200..=299, v)) => {
                let server = v.get("session").map(|x| x.as_str().map(str::to_string).unwrap_or_else(|| x.to_string())).filter(|x| !x.is_empty() && x != "null");
                if let Some(server) = server {
                    with_store(&dir, |d| { if let Some(x) = d.sessions.get_mut(&s.id) { x.server = Some(server); x.sent_checkins = x.checkins.len(); x.native_pending = false; } ((), true) });
                }
            }
            Ok(_) | Err(_) => return,
        }
    }
    for sid in sends {
        let (server, list): (Option<String>, Vec<Unlock>) = with_store(&dir, |d| ((d.sessions.get(&sid).and_then(|s| s.server.clone()), d.unlocks.iter().filter(|u| u.session == sid && !u.synced).cloned().collect()), false));
        let Some(server) = server else { continue };
        if list.is_empty() {
            continue;
        }
        let game = list[0].game.clone();
        let names = effective(&dir, &game, list_for(&dir, &game));
        let items: Vec<Value> = list
            .iter()
            .map(|u| {
                let mut o = json!({"id": u.id, "source": u.source, "at": u.at});
                if u.casual {
                    o["casual"] = json!(true);
                    o["reason"] = json!(u.reason);
                }
                if u.earlier {
                    o["earlier"] = json!(true);
                }
                if let Some(f) = u.frame {
                    o["frame"] = json!(f);
                }
                if u.source == "native" {
                    if let Some(e) = entry_of(names.as_ref(), u.id) {
                        for k in ["title", "description", "gamerscore"] {
                            if let Some(v) = e.get(k) {
                                o[k] = v.clone();
                            }
                        }
                    }
                }
                o
            })
            .collect();
        match call(app, "POST", "unlocks", &[], Some(json!({"session": server, "unlocks": items}))).await {
            Ok((200..=299, v)) => {
                let results = v.get("results").and_then(Value::as_array).cloned().unwrap_or_default();
                with_store(&dir, |d| {
                    for u in d.unlocks.iter_mut().filter(|u| u.session == sid && list.iter().any(|x| x.id == u.id && x.game == u.game)) {
                        u.synced = true;
                        if let Some(r) = results.iter().find(|r| num(r, "id") == Some(u.id as u64)) {
                            u.ranked = r.get("ranked").and_then(Value::as_bool);
                            u.server_reason = r.get("reason").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
                        }
                    }
                    ((), true)
                });
                let _ = app.emit("ach-changed", json!({"game": game}));
            }
            _ => return,
        }
    }
}

// ---------- ReXGlue's unlock file ----------

struct RexWatch {
    game: String,
    install: PathBuf,
    started: u64,
    catalog_folder: Option<String>,
}

/// `[unlocked.<id>]` sections and their `filetime`, from ReXGlue's achievements/<TITLE ID>.toml.
pub fn parse_unlock_file(text: &str) -> BTreeMap<u32, u64> {
    let mut out = BTreeMap::new();
    let mut cur: Option<u32> = None;
    for l in text.lines() {
        let l = l.trim();
        if let Some(sec) = l.strip_prefix('[').and_then(|x| x.strip_suffix(']')) {
            cur = sec.trim().strip_prefix("unlocked.").and_then(|x| {
                let x = x.trim().trim_matches('"');
                x.strip_prefix("0x").or_else(|| x.strip_prefix("0X")).map(|h| u32::from_str_radix(h, 16).ok()).unwrap_or_else(|| x.parse().ok())
            });
            if let Some(id) = cur {
                out.entry(id).or_insert(0);
            }
        } else if let (Some(id), Some(v)) = (cur, l.strip_prefix("filetime").map(|r| r.trim_start()).and_then(|r| r.strip_prefix('='))) {
            if let Ok(ft) = v.trim().parse::<u64>() {
                out.insert(id, filetime_unix(ft));
            }
        }
    }
    out
}

/// A Windows FILETIME (100 ns since 1601) as Unix seconds.
pub fn filetime_unix(ft: u64) -> u64 {
    (ft / 10_000_000).saturating_sub(11_644_473_600)
}

fn title_file(p: &Path) -> bool {
    p.file_name().and_then(|n| n.to_str()).map(|n| n.len() == 13 && n.to_ascii_lowercase().ends_with(".toml") && n[..8].chars().all(|c| c.is_ascii_hexdigit())).unwrap_or(false)
}

/// achievements/<TITLE ID>.toml files in `dir` (its own achievements folder).
fn unlock_files_in(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir.join("achievements")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| title_file(p)).collect()
}

/// Folders a ReXGlue game may keep its user data in: the game's own folder (a few levels down),
/// what the catalog says, and the usual places in the player's folders (two levels down).
fn rex_candidates(w: &RexWatch) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = vec![];
    if let Some(f) = w.catalog_folder.as_deref().map(|f| expand(f, &w.install)) {
        out.extend(unlock_files_in(&f));
        // A folder that's the achievements folder itself.
        out.extend(std::fs::read_dir(&f).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| title_file(p)));
    }
    let mut stack = vec![(w.install.clone(), 0usize)];
    while let Some((d, depth)) = stack.pop() {
        out.extend(unlock_files_in(&d));
        if depth < 3 {
            stack.extend(std::fs::read_dir(&d).into_iter().flatten().flatten().filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false)).map(|e| (e.path(), depth + 1)).take(200));
        }
    }
    let mut roots: Vec<PathBuf> = vec![];
    #[cfg(windows)]
    for k in ["APPDATA", "LOCALAPPDATA"] {
        if let Some(v) = std::env::var_os(k) {
            roots.push(PathBuf::from(v));
        }
    }
    if let Some(h) = dirs::home_dir() {
        roots.push(h.join("Saved Games"));
        roots.push(h.join("AppData").join("LocalLow"));
        roots.push(h.join(".local").join("share"));
        roots.push(h.join(".config"));
    }
    if let Some(d) = dirs::document_dir() {
        roots.push(d);
    }
    for r in roots {
        for e in std::fs::read_dir(&r).into_iter().flatten().flatten().filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false)).take(800) {
            out.extend(unlock_files_in(&e.path()));
            for e2 in std::fs::read_dir(e.path()).into_iter().flatten().flatten().filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false)).take(60) {
                out.extend(unlock_files_in(&e2.path()));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// A folder from the catalog: {game} is the game's folder; %APPDATA%, %LOCALAPPDATA%,
/// %USERPROFILE% and %DOCUMENTS% the player's own.
fn expand(f: &str, install: &Path) -> PathBuf {
    let mut s = f.replace("{game}", &install.to_string_lossy());
    let pairs: Vec<(&str, Option<PathBuf>)> = vec![
        ("%APPDATA%", std::env::var_os("APPDATA").map(PathBuf::from).or_else(dirs::config_dir)),
        ("%LOCALAPPDATA%", std::env::var_os("LOCALAPPDATA").map(PathBuf::from).or_else(dirs::data_local_dir)),
        ("%USERPROFILE%", dirs::home_dir()),
        ("%DOCUMENTS%", dirs::document_dir()),
    ];
    for (k, v) in pairs {
        if let Some(v) = v {
            s = s.replace(k, &v.to_string_lossy());
        }
    }
    PathBuf::from(s)
}

/// The other way: a folder written for the catalog, without the player's name in it.
fn unexpand(p: &Path, install: &Path) -> String {
    let s = p.to_string_lossy().to_string();
    let pairs: Vec<(Option<PathBuf>, &str)> = vec![
        (Some(install.to_path_buf()), "{game}"),
        (std::env::var_os("LOCALAPPDATA").map(PathBuf::from), "%LOCALAPPDATA%"),
        (std::env::var_os("APPDATA").map(PathBuf::from), "%APPDATA%"),
        (dirs::document_dir(), "%DOCUMENTS%"),
        (dirs::home_dir(), "%USERPROFILE%"),
    ];
    for (k, v) in pairs {
        if let Some(k) = k.filter(|k| !k.as_os_str().is_empty()) {
            if let Ok(rest) = p.strip_prefix(&k) {
                return if rest.as_os_str().is_empty() { v.to_string() } else { format!("{v}{}{}", std::path::MAIN_SEPARATOR, rest.display()) };
            }
        }
    }
    s
}

fn newest(files: Vec<PathBuf>) -> Option<PathBuf> {
    files.into_iter().max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
}

async fn rex_watch(app: AppHandle, w: RexWatch, mut stop: tokio::sync::oneshot::Receiver<()>) {
    let dir = app.state::<App>().dir.clone();
    let remembered = with_store(&dir, |d| (d.found.get(&w.game).cloned(), false));
    let look = |w: &RexWatch| -> Option<PathBuf> {
        if let Some(f) = remembered.as_deref() {
            let p = expand(f, &w.install);
            if let Some(x) = newest(unlock_files_in(&p)) {
                return Some(x);
            }
        }
        newest(rex_candidates(w))
    };
    // What's there when the session starts: earlier unlocks.
    let mut file = look(&w);
    let mut known: BTreeMap<u32, u64> = BTreeMap::new();
    let mut first_sight = true;
    let mut since_look = Instant::now();
    let begun = Instant::now();
    let mut recent: VecDeque<Instant> = VecDeque::new();
    loop {
        if let Some(f) = &file {
            if let Ok(text) = tokio::fs::read_to_string(f).await {
                let now_entries = parse_unlock_file(&text);
                if first_sight {
                    first_sight = false;
                    let folder = f.parent().and_then(|p| p.parent()).map(|p| unexpand(p, &w.install)).unwrap_or_default();
                    log_line(&w.game, "*", &format!("ReXGlue unlock file: {}", unexpand(f, &w.install)));
                    let changed = with_store(&dir, |d| { let c = d.found.get(&w.game) != Some(&folder); d.found.insert(w.game.clone(), folder.clone()); (c, c) });
                    if changed {
                        crate::trail::note(format!("Achievements {}: ReXGlue unlock file in {folder}", w.game));
                    }
                    // Already there: from before this session.
                    for (id, at) in &now_entries {
                        if *at == 0 || *at + 60 < w.started || begun.elapsed().as_secs() < 3 {
                            record(&app, &w.game, *id, "native", None, true, None);
                            known.insert(*id, *at);
                        }
                    }
                }
                let new: Vec<(u32, u64)> = now_entries.iter().filter(|(id, _)| !known.contains_key(id)).map(|(a, b)| (*a, *b)).collect();
                if !new.is_empty() {
                    let t = Instant::now();
                    for _ in &new {
                        recent.push_back(t);
                    }
                    while recent.front().map(|x| x.elapsed().as_secs() > 5).unwrap_or(false) {
                        recent.pop_front();
                    }
                    // Several at once early on: a loaded save sending its unlocks again.
                    let burst = begun.elapsed().as_secs() < 90 && recent.len() >= 2;
                    for (id, at) in new {
                        known.insert(id, at);
                        let old = at != 0 && at + 60 < w.started;
                        record(&app, &w.game, id, "native", None, old, if burst { Some("save_loaded") } else { None });
                    }
                }
            }
        } else if since_look.elapsed().as_secs() >= if begun.elapsed().as_secs() < 120 { 10 } else { 30 } {
            // Not there yet: ReXGlue makes it with the first unlock.
            since_look = Instant::now();
            let dir2 = w.install.clone();
            let w2 = RexWatch { game: w.game.clone(), install: dir2, started: w.started, catalog_folder: w.catalog_folder.clone() };
            file = tokio::task::spawn_blocking(move || newest(rex_candidates(&w2))).await.ok().flatten();
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(2)) => {}
            _ = &mut stop => return,
        }
    }
}

// ---------- the toast ----------

static TOASTS: Mutex<Vec<Value>> = Mutex::new(Vec::new());

fn sound_on(dir: &Path) -> bool {
    with_store(dir, |d| (d.sound.unwrap_or(true), false))
}

/// Shows a toast in the corner over the game (queued in the toast's own window).
pub fn toast(app: &AppHandle, mut card: Value) {
    card["sound"] = json!(sound_on(&app.state::<App>().dir));
    TOASTS.lock().unwrap().push(card);
    let a = app.clone();
    let _ = app.run_on_main_thread(move || {
        let w = match a.get_webview_window("toast") {
            Some(w) => w,
            None => match build_toast(&a) {
                Ok(w) => w,
                Err(e) => {
                    crate::trail::note(format!("Achievements: the toast window didn't open ({e})"));
                    return;
                }
            },
        };
        place_toast(&a, &w);
        let _ = w.set_ignore_cursor_events(true);
        let _ = w.show();
        let _ = a.emit_to("toast", "ach-toast", json!({}));
    });
}

fn build_toast(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    let w = tauri::WebviewWindowBuilder::new(app, "toast", tauri::WebviewUrl::App("toast.html".into()))
        .title("RecompLauncher achievements")
        .transparent(true)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(false)
        .visible(false)
        .focused(false)
        .focusable(false)
        .inner_size(TOAST_W, TOAST_H)
        .build()?;
    crate::rest(&w, true);
    Ok(w)
}

const TOAST_W: f64 = 540.0;
const TOAST_H: f64 = 200.0;

/// Top right of the screen the game is on (the one with the mouse, else the main one).
fn place_toast(app: &AppHandle, w: &tauri::WebviewWindow) {
    let mons = app.available_monitors().unwrap_or_default();
    let cursor = app.get_webview_window("main").and_then(|m| m.cursor_position().ok());
    let mon = cursor
        .and_then(|c| mons.iter().find(|m| { let (p, s) = (m.position(), m.size()); c.x >= p.x as f64 && c.y >= p.y as f64 && c.x < (p.x + s.width as i32) as f64 && c.y < (p.y + s.height as i32) as f64 }).cloned())
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(m) = mon else { return };
    let k = m.scale_factor();
    let (p, s) = (m.position(), m.size());
    let (ww, wh) = ((TOAST_W * k) as i32, (TOAST_H * k) as i32);
    let _ = w.set_size(tauri::PhysicalSize::new(ww as u32, wh as u32));
    let _ = w.set_position(tauri::PhysicalPosition::new(p.x + s.width as i32 - ww, p.y));
}

#[tauri::command]
pub fn ach_toast_take() -> Vec<Value> {
    std::mem::take(&mut *TOASTS.lock().unwrap())
}

#[tauri::command]
pub fn ach_toast_done(app: AppHandle) {
    if TOASTS.lock().unwrap().is_empty() {
        if let Some(w) = app.get_webview_window("toast") {
            let _ = w.hide();
        }
    }
}

// ---------- the game page ----------

#[tauri::command]
pub async fn ach_access(app: AppHandle, fresh: Option<bool>) -> Res<Value> {
    if fresh.unwrap_or(false) || ACCESS.load(Ordering::Relaxed) == 0 {
        check_access(&app).await;
    }
    Ok(json!({"allowed": allowed()}))
}

/// Everything the Achievements tab shows for a game. The website's list and the account's
/// unlocks are asked for in the background (ach-changed when they arrive).
#[tauri::command]
pub async fn ach_game(app: AppHandle, id: String, fresh: Option<bool>) -> Res<Value> {
    if !allowed() {
        return Ok(json!({"allowed": false}));
    }
    let dir = app.state::<App>().dir.clone();
    if fresh.unwrap_or(false) {
        let (a, g) = (app.clone(), id.clone());
        tauri::async_runtime::spawn(async move {
            let before = list_for(&a.state::<App>().dir, &g);
            let after = fetch_list(&a, &g).await;
            let mine = call(&a, "GET", "mine", &[("game", g.as_str())], None).await;
            let mut changed = after.is_some() && before != after;
            if let Ok((200, v)) = mine {
                changed |= with_store(&a.state::<App>().dir, |d| { let c = d.mine.get(&g) != Some(&v); d.mine.insert(g.clone(), v); (c, c) });
            }
            if changed {
                let _ = a.emit("ach-changed", json!({"game": g}));
            }
        });
    }
    let list = effective(&dir, &id, list_for(&dir, &id));
    let (unlocks, progress, broken, mine, found, sound) = with_store(&dir, |d| {
        (
            (
                d.unlocks.iter().filter(|u| u.game == id).cloned().collect::<Vec<_>>(),
                d.progress.get(&id).cloned().unwrap_or_default(),
                d.broken.get(&id).cloned().unwrap_or_default(),
                d.mine.get(&id).cloned(),
                d.found.get(&id).cloned(),
                d.sound.unwrap_or(true),
            ),
            false,
        )
    });
    let (live_now, connected, hello, session, paused) = live(|m| m.get(&id).map(|l| (true, l.connected, l.hello.clone(), Some(l.session.clone()), l.paused.clone()))).unwrap_or((false, false, None, None, None));
    // The website's unlocks for this account (from another PC, say) count too.
    let mut ids: Vec<u32> = unlocks.iter().map(|u| u.id).collect();
    if let Some(m) = mine.as_ref().and_then(|m| m.get("unlocks")).and_then(Value::as_array) {
        ids.extend(m.iter().filter_map(|u| num(u, "id").or_else(|| u.as_u64())).map(|x| x as u32));
    }
    ids.sort();
    ids.dedup();
    let all = list.as_ref().map(entries).unwrap_or_default();
    let total: u64 = all.iter().map(points_of).sum();
    let got: u64 = all.iter().filter(|a| num(a, "id").map(|i| ids.contains(&(i as u32))).unwrap_or(false)).map(points_of).sum();
    let progress: BTreeMap<String, Value> = progress.into_iter().map(|(k, (v, t))| (k.to_string(), json!({"value": v, "target": t}))).collect();
    let broken: BTreeMap<String, String> = broken.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    Ok(json!({
        "allowed": true, "list": list, "unlocks": unlocks, "unlocked": ids, "mine": mine, "progress": progress, "broken": broken,
        "points": got, "total": total, "count": if all.is_empty() { list.as_ref().and_then(|l| num(l, "total")).unwrap_or(0) as usize } else { all.len() }, "mastered": all_unlocked(list.as_ref(), &ids.iter().copied().collect()),
        "live": live_now, "connected": connected, "hello": hello, "session": session, "found": found, "sound": sound, "paused": paused,
    }))
}

#[tauri::command]
pub fn ach_sound(app: AppHandle, on: bool) {
    with_store(&app.state::<App>().dir, |d| { d.sound = Some(on); ((), true) });
}

// ---------- dev tools ----------

#[tauri::command]
pub fn ach_log(id: String) -> Vec<Value> {
    if !allowed() {
        return vec![];
    }
    live(|m| m.get(&id).map(|l| l.log.iter().map(|(at, d, s)| json!({"at": at, "dir": d, "line": s})).collect()).unwrap_or_default())
}

/// Reads up to 256 bytes of the running game's memory through its channel (it answers "mem").
async fn peek(app: &AppHandle, game: &str, address: u32, size: u32) -> Res<Vec<u8>> {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    let ok = live(|m| m.get_mut(game).filter(|l| l.connected).map(|l| { l.peeks.insert(seq, tx); }).is_some());
    if !ok {
        return Err(Error::Msg("The game isn't connected.".into()));
    }
    send(app, game, json!({"t": "peek", "address": address, "size": size.clamp(1, 256), "seq": seq}));
    let hex = tokio::time::timeout(Duration::from_secs(2), rx).await.map_err(|_| Error::Msg("The game didn't answer.".into()))?.map_err(|_| Error::Msg("The game didn't answer.".into()))?;
    live(|m| { if let Some(l) = m.get_mut(game) { l.peeks.remove(&seq); } });
    Ok((0..hex.len() / 2).filter_map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()).collect())
}

#[tauri::command]
pub async fn ach_peek(app: AppHandle, id: String, address: u32, size: u32) -> Res<String> {
    if !allowed() {
        return Err(Error::Msg("Not available.".into()));
    }
    let b = peek(&app, &id, address, size).await?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// Whether a rule is true right now, read from the running game's memory.
#[tauri::command]
pub async fn ach_rule(app: AppHandle, id: String, rule: String) -> Res<Value> {
    if !allowed() {
        return Err(Error::Msg("Not available.".into()));
    }
    let r = rule::parse(&rule).map_err(Error::Msg)?;
    let mut cache: HashMap<u32, Vec<u8>> = HashMap::new();
    // Each address read once, in 16-byte lines.
    let needs = rule::addresses(&r);
    for (addr, n) in needs {
        let line = addr & !15;
        let end = addr + n;
        let mut at = line;
        while at < end {
            if let std::collections::hash_map::Entry::Vacant(e) = cache.entry(at) {
                e.insert(peek(&app, &id, at, 16).await?);
            }
            at += 16;
        }
    }
    // Pointers (AddAddress) need what they point at, read as they're followed.
    let mut tries = 0;
    loop {
        let res = rule::eval(&r, &|addr, n| {
            let mut out = Vec::new();
            for i in 0..n {
                let a = addr.wrapping_add(i);
                out.push(*cache.get(&(a & !15))?.get((a & 15) as usize)?);
            }
            Some(out)
        });
        match res {
            Ok(v) => return Ok(v),
            Err(rule::Need(addr)) if tries < 8 => {
                tries += 1;
                let line = addr & !15;
                let b = peek(&app, &id, line, 32).await?;
                cache.insert(line, b.get(..16).map(<[u8]>::to_vec).unwrap_or_default());
                if b.len() > 16 {
                    cache.insert(line + 16, b[16..].to_vec());
                }
            }
            Err(rule::Need(addr)) => return Err(Error::Msg(format!("Couldn't read 0x{addr:x}."))),
        }
    }
}

/// Dev tools: this game's unlocks gone from this PC (the website keeps its own).
#[tauri::command]
pub fn ach_reset(app: AppHandle, id: String) {
    if !allowed() {
        return;
    }
    with_store(&app.state::<App>().dir, |d| {
        d.unlocks.retain(|u| u.game != id);
        d.progress.remove(&id);
        d.broken.remove(&id);
        d.mine.remove(&id);
        ((), true)
    });
    crate::trail::note(format!("Achievements {id}: unlocks reset on this PC"));
    let _ = app.emit("ach-changed", json!({"game": id}));
    // And on the account (test accounts only).
    let a = app.clone();
    tauri::async_runtime::spawn(async move {
        let r = call(&a, "POST", "reset", &[], Some(json!({"game": id}))).await;
        let n = match &r { Ok((200..=299, v)) => num(v, "cleared").unwrap_or(0).to_string(), _ => "none (offline?)".into() };
        crate::trail::note(format!("Achievements {id}: reset on the account, {n} cleared"));
        let _ = a.emit("ach-changed", json!({"game": id}));
    });
}

/// Dev tools: the toast, to see it over a game.
#[tauri::command]
pub fn ach_test_toast(app: AppHandle, id: String, mastered: Option<bool>) {
    if !allowed() {
        return;
    }
    let dir = app.state::<App>().dir.clone();
    let list = effective(&dir, &id, list_for(&dir, &id));
    let first = list.as_ref().map(entries).unwrap_or_default().into_iter().next();
    if mastered.unwrap_or(false) {
        toast(&app, json!({"title": id, "mastered": true, "points": list.as_ref().map(entries).unwrap_or_default().iter().map(points_of).sum::<u64>()}));
    } else {
        toast(&app, json!({"title": first.as_ref().and_then(|a| a.get("title").and_then(Value::as_str)).unwrap_or("Feather Light"), "points": first.as_ref().map(points_of).unwrap_or(10), "game": id}));
    }
}

/// The memory-rule tester: rcheevos' condition format, read once from the game's memory.
pub mod rule {
    use serde_json::{json, Value};

    #[derive(Debug, Clone, PartialEq)]
    pub enum Op {
        Mem { size: Size, addr: u32, prefix: Option<char> },
        Num(f64),
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum Size {
        B8,
        B16,
        B24,
        B32,
        Lo4,
        Hi4,
        Bit(u8),
        BitCount,
        B16Be,
        B24Be,
        B32Be,
    }

    #[derive(Debug, Clone)]
    pub struct Cond {
        pub flag: Option<char>,
        pub left: Op,
        pub op: Option<String>,
        pub right: Option<Op>,
        pub hits: u32,
        pub text: String,
    }

    pub struct Need(pub u32);

    /// Groups: the core, then the alternatives ("S" between them). Conditions are joined by "_".
    pub fn parse(s: &str) -> Result<Vec<Vec<Cond>>, String> {
        let s = s.trim();
        if s.is_empty() {
            return Err("Paste a rule first.".into());
        }
        let mut groups = vec![];
        for (gi, g) in split_groups(s).into_iter().enumerate() {
            if g.is_empty() && gi == 0 {
                groups.push(vec![]);
                continue;
            }
            let mut v = vec![];
            for c in g.split('_') {
                v.push(cond(c)?);
            }
            groups.push(v);
        }
        Ok(groups)
    }

    /// "S" splits groups, but not inside an address (0xS12 is a bit read).
    fn split_groups(s: &str) -> Vec<String> {
        let mut out = vec![String::new()];
        let b: Vec<char> = s.chars().collect();
        for i in 0..b.len() {
            let c = b[i];
            let in_addr = i >= 2 && b[i - 1] == 'x' && b[i - 2] == '0';
            if c == 'S' && !in_addr {
                out.push(String::new());
            } else {
                out.last_mut().unwrap().push(c);
            }
        }
        out
    }

    fn cond(c: &str) -> Result<Cond, String> {
        let text = c.to_string();
        let mut rest = c.trim();
        let mut flag = None;
        if rest.len() > 2 && rest.as_bytes()[1] == b':' && rest.as_bytes()[0].is_ascii_alphabetic() {
            flag = Some(rest.as_bytes()[0] as char);
            rest = &rest[2..];
        }
        let mut hits = 0;
        if rest.ends_with('.') {
            if let Some(i) = rest[..rest.len() - 1].rfind('.') {
                hits = rest[i + 1..rest.len() - 1].parse().map_err(|_| format!("Hit count in \"{c}\" isn't a number."))?;
                rest = &rest[..i];
            }
        }
        let (left, after) = operand(rest).map_err(|e| format!("{e} in \"{c}\""))?;
        let after = after.trim();
        if after.is_empty() {
            return Ok(Cond { flag, left, op: None, right: None, hits, text });
        }
        let ops = ["!=", "<=", ">=", "=", "<", ">", "*", "/", "&", "^", "+", "-", "%"];
        let op = ops.iter().find(|o| after.starts_with(**o)).ok_or_else(|| format!("No comparison in \"{c}\"."))?;
        let (right, tail) = operand(&after[op.len()..]).map_err(|e| format!("{e} in \"{c}\""))?;
        if !tail.trim().is_empty() {
            return Err(format!("Extra text \"{}\" in \"{c}\".", tail.trim()));
        }
        Ok(Cond { flag, left, op: Some(op.to_string()), right: Some(right), hits, text })
    }

    fn operand(s: &str) -> Result<(Op, &str), String> {
        let s = s.trim_start();
        let mut prefix = None;
        let mut t = s;
        if let Some(c) = t.chars().next().filter(|c| matches!(c, 'd' | 'p' | 'b' | '~')) {
            if t[1..].starts_with("0x") || t[1..].starts_with("0X") {
                prefix = Some(c);
                t = &t[1..];
            }
        }
        if t.starts_with("0x") || t.starts_with("0X") {
            let r = &t[2..];
            let (size, r) = match r.chars().next() {
                Some('H' | 'h') => (Size::B8, &r[1..]),
                Some(' ') => (Size::B16, &r[1..]),
                Some('W' | 'w') => (Size::B24, &r[1..]),
                Some('X' | 'x') => (Size::B32, &r[1..]),
                Some('L' | 'l') => (Size::Lo4, &r[1..]),
                Some('U' | 'u') => (Size::Hi4, &r[1..]),
                Some(c @ ('M'..='T')) => (Size::Bit(c as u8 - b'M'), &r[1..]),
                Some('K' | 'k') => (Size::BitCount, &r[1..]),
                Some('I' | 'i') => (Size::B16Be, &r[1..]),
                Some('J' | 'j') => (Size::B24Be, &r[1..]),
                Some('G' | 'g') => (Size::B32Be, &r[1..]),
                Some(c) if c.is_ascii_hexdigit() => (Size::B16, r),
                _ => return Err("an address size the tester doesn't read".into()),
            };
            let n = r.chars().take_while(|c| c.is_ascii_hexdigit()).count();
            if n == 0 {
                return Err("an address with no number".into());
            }
            let addr = u32::from_str_radix(&r[..n], 16).map_err(|_| "an address too big")?;
            return Ok((Op::Mem { size, addr, prefix }, &r[n..]));
        }
        if let Some(r) = t.strip_prefix('h').or_else(|| t.strip_prefix('H')) {
            let n = r.chars().take_while(|c| c.is_ascii_hexdigit()).count();
            let v = u64::from_str_radix(&r[..n], 16).map_err(|_| "a hex value that doesn't read")?;
            return Ok((Op::Num(v as f64), &r[n..]));
        }
        if let Some(r) = t.strip_prefix('f').or_else(|| t.strip_prefix('F')) {
            let n = r.chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-').count();
            let v: f64 = r[..n].parse().map_err(|_| "a float that doesn't read")?;
            return Ok((Op::Num(v), &r[n..]));
        }
        let n = t.chars().enumerate().take_while(|(i, c)| c.is_ascii_digit() || (*i == 0 && *c == '-')).count();
        if n == 0 {
            return Err("something the tester can't read".into());
        }
        let v: f64 = t[..n].parse().map_err(|_| "a number that doesn't read")?;
        Ok((Op::Num(v), &t[n..]))
    }

    fn bytes(s: Size) -> u32 {
        match s {
            Size::B8 | Size::Lo4 | Size::Hi4 | Size::Bit(_) | Size::BitCount => 1,
            Size::B16 | Size::B16Be => 2,
            Size::B24 | Size::B24Be => 3,
            Size::B32 | Size::B32Be => 4,
        }
    }

    /// Every address a rule reads straight away (pointers are followed later).
    pub fn addresses(r: &[Vec<Cond>]) -> Vec<(u32, u32)> {
        let mut v = vec![];
        for c in r.iter().flatten() {
            for o in [Some(&c.left), c.right.as_ref()].into_iter().flatten() {
                if let Op::Mem { size, addr, .. } = o {
                    v.push((*addr, bytes(*size)));
                }
            }
        }
        v.sort();
        v.dedup();
        v
    }

    fn value(o: &Op, offset: u32, read: &dyn Fn(u32, u32) -> Option<Vec<u8>>) -> Result<f64, Need> {
        match o {
            Op::Num(n) => Ok(*n),
            Op::Mem { size, addr, prefix } => {
                let a = addr.wrapping_add(offset);
                let b = read(a, bytes(*size)).ok_or(Need(a))?;
                let le = |n: usize| b.iter().take(n).enumerate().fold(0u64, |acc, (i, x)| acc | (*x as u64) << (8 * i));
                let be = |n: usize| b.iter().take(n).fold(0u64, |acc, x| (acc << 8) | *x as u64);
                let mut v = match size {
                    Size::B8 => le(1),
                    Size::B16 => le(2),
                    Size::B24 => le(3),
                    Size::B32 => le(4),
                    Size::Lo4 => le(1) & 0xF,
                    Size::Hi4 => le(1) >> 4,
                    Size::Bit(n) => (le(1) >> n) & 1,
                    Size::BitCount => le(1).count_ones() as u64,
                    Size::B16Be => be(2),
                    Size::B24Be => be(3),
                    Size::B32Be => be(4),
                };
                match prefix {
                    Some('b') => {
                        // Binary-coded decimal: each nibble a digit.
                        let mut out = 0u64;
                        let mut mul = 1u64;
                        let mut x = v;
                        while x > 0 {
                            out += (x & 0xF) * mul;
                            mul *= 10;
                            x >>= 4;
                        }
                        v = out;
                    }
                    Some('~') => v = !v & ((1u64 << (8 * bytes(*size))) - 1),
                    _ => {}
                }
                Ok(v as f64)
            }
        }
    }

    fn cmp(a: f64, op: &str, b: f64) -> bool {
        match op {
            "=" => a == b,
            "!=" => a != b,
            "<" => a < b,
            "<=" => a <= b,
            ">" => a > b,
            ">=" => a >= b,
            _ => false,
        }
    }

    fn arith(a: f64, op: &str, b: f64) -> f64 {
        match op {
            "*" => a * b,
            "/" if b != 0.0 => (a / b).trunc(),
            "&" => ((a as u64) & (b as u64)) as f64,
            "^" => ((a as u64) ^ (b as u64)) as f64,
            "+" => a + b,
            "-" => a - b,
            "%" if b != 0.0 => (a as u64 % b as u64) as f64,
            _ => a,
        }
    }

    /// The rule's truth now: each condition, each group, and the whole (the core, and one of the
    /// alternatives when there are any). Hit counts, delta and prior reads can't be judged from
    /// one read, and are noted.
    pub fn eval(r: &[Vec<Cond>], read: &dyn Fn(u32, u32) -> Option<Vec<u8>>) -> Result<Value, Need> {
        let mut groups = vec![];
        let mut notes: Vec<String> = vec![];
        let mut truth = vec![];
        for g in r {
            let (mut add, mut offset) = (0f64, 0u32);
            let mut chain: Option<(bool, char)> = None;
            let mut parts = vec![];
            let mut ok = true;
            for c in g {
                if let Op::Mem { prefix: Some('d' | 'p'), .. } = c.left {
                    notes.push("Delta and prior reads use the current value here.".into());
                }
                if c.hits > 0 {
                    notes.push("Hit counts are left out: the tester reads once.".into());
                }
                let l = value(&c.left, offset, read)? + add;
                let rv = match &c.right { Some(o) => value(o, offset, read)?, None => 0.0 };
                match c.flag {
                    Some('A') => {
                        add = c.op.as_deref().map(|o| arith(l, o, rv)).unwrap_or(l);
                        offset = 0;
                        parts.push(json!({"text": c.text, "value": add, "kind": "add"}));
                        continue;
                    }
                    Some('B') => {
                        add = -c.op.as_deref().map(|o| arith(l, o, rv)).unwrap_or(l);
                        offset = 0;
                        parts.push(json!({"text": c.text, "value": add, "kind": "sub"}));
                        continue;
                    }
                    Some('I') => {
                        offset = c.op.as_deref().map(|o| arith(l, o, rv)).unwrap_or(l) as u32;
                        add = 0.0;
                        parts.push(json!({"text": c.text, "value": offset, "kind": "pointer"}));
                        continue;
                    }
                    _ => {}
                }
                let mut t = c.op.as_deref().map(|o| cmp(l, o, rv)).unwrap_or(l != 0.0);
                add = 0.0;
                offset = 0;
                if let Some((prev, how)) = chain.take() {
                    t = if how == 'N' { prev && t } else { prev || t };
                }
                match c.flag {
                    Some(f @ ('N' | 'O')) => {
                        chain = Some((t, f));
                        parts.push(json!({"text": c.text, "true": t, "kind": "chain"}));
                        continue;
                    }
                    Some('R') => {
                        parts.push(json!({"text": c.text, "true": t, "kind": "reset"}));
                        if t {
                            notes.push("A reset condition is true right now.".into());
                        }
                        continue;
                    }
                    Some('P') => {
                        parts.push(json!({"text": c.text, "true": t, "kind": "pause"}));
                        if t {
                            ok = false;
                        }
                        continue;
                    }
                    Some(f) if !matches!(f, 'M' | 'Q' | 'T' | 'G') => notes.push(format!("The {f}: flag is read as a plain condition.")),
                    _ => {}
                }
                parts.push(json!({"text": c.text, "true": t, "left": l, "right": rv}));
                ok &= t;
            }
            truth.push(ok);
            groups.push(json!({"true": ok, "parts": parts}));
        }
        let core = truth.first().copied().unwrap_or(true);
        let alts = &truth[truth.len().min(1)..];
        let whole = core && (alts.is_empty() || alts.iter().any(|x| *x));
        notes.sort();
        notes.dedup();
        Ok(json!({"true": whole, "groups": groups, "notes": notes}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rexglue_unlock_files_are_read() {
        let t = "[unlocked.3]\nfiletime = 133756128000000000\n\n[unlocked.12]\nfiletime = 133756200000000000\n[other]\nfiletime = 5\n";
        let m = parse_unlock_file(t);
        assert_eq!(m.len(), 2);
        assert_eq!(m[&3], filetime_unix(133756128000000000));
        assert_eq!(filetime_unix(116444736000000000), 0);
        assert!(m[&12] > m[&3]);
        assert!(title_file(Path::new("x/achievements/4D5307E6.toml")) && !title_file(Path::new("x/achievements/settings.toml")));
    }

    #[test]
    fn lists_pause_on_versions_they_werent_checked_on() {
        assert!(version_ok("", "v9"));
        assert!(version_ok(">=1.4.0", "v1.4.2") && !version_ok(">=1.4.0", "1.3.9"));
        assert!(version_ok("1.4.x", "v1.4.7") && !version_ok("1.4.x", "1.5.0"));
        assert!(version_ok("1.4.0, 1.4.2", "1.4.2") && !version_ok("1.4.0, 1.4.2", "1.4.1"));
        assert!(version_ok(">=1.2 <2.0", "1.9") && !version_ok(">=1.2 <2.0", "2.0.0"));
        assert!(version_ok("1.0.0 || 2.x", "2.3"));
    }

    #[test]
    fn mastered_counts_a_native_lists_total() {
        let have: std::collections::HashSet<u32> = [1, 2, 3].into_iter().collect();
        assert!(all_unlocked(Some(&json!({"source": "native", "total": 3})), &have));
        assert!(!all_unlocked(Some(&json!({"source": "native", "total": 4})), &have));
        assert!(all_unlocked(Some(&json!({"achievements": [{"id": 1}, {"id": 3}]})), &have));
        assert!(!all_unlocked(Some(&json!({"achievements": [{"id": 1}, {"id": 9}]})), &have));
        assert!(!all_unlocked(None, &have));
    }

    #[test]
    fn gamerscore_comes_onto_the_points_scale() {
        assert_eq!(points_of(&json!({"points": 25})), 25);
        assert_eq!(points_of(&json!({"gamerscore": 10})), 5);
        assert_eq!(points_of(&json!({"gamerscore": 20})), 10);
        assert_eq!(points_of(&json!({"gamerscore": 50})), 25);
        assert_eq!(points_of(&json!({"gamerscore": 100})), 50);
    }

    #[test]
    fn rules_are_tested_against_memory() {
        let mem: Vec<u8> = (0..64u32).map(|i| i as u8).collect();
        let read = |a: u32, n: u32| -> Option<Vec<u8>> { (a + n <= 64).then(|| mem[a as usize..(a + n) as usize].to_vec()) };
        let t = |s: &str| rule::eval(&rule::parse(s).unwrap(), &read).ok().unwrap()["true"].as_bool().unwrap();
        assert!(t("0xH0019=25"));
        assert!(!t("0xH0019=2"));
        assert!(t("0x0002=h0302"));
        assert!(t("0xX0004=h07060504"));
        assert!(t("0xI0002=h0203"));
        assert!(t("0xH0010=16_0xH0011>16"));
        assert!(!t("0xH0010=16_0xH0011<16"));
        // Alternatives: the core and one of them.
        assert!(t("0xH0001=1S0xH0002=9S0xH0003=3"));
        assert!(!t("0xH0001=1S0xH0002=9S0xH0003=9"));
        // AddSource: 5 + 6 = 11.
        assert!(t("A:0xH0005_0xH0006=11"));
        // A pause that's true stops it.
        assert!(!t("0xH0001=1_P:0xH0002=2"));
        // Bits: 0x05 is 0101.
        assert!(t("0xM0005=1_0xN0005=0_0xO0005=1"));
        assert!(t("0xL0013=3_0xU0013=1"));
        assert!(rule::parse("0xZ12=1").is_err());
        assert!(rule::parse("").is_err());
    }

    #[test]
    fn pointers_are_followed() {
        let mut mem = vec![0u8; 64];
        mem[0] = 0x20;
        mem[0x24] = 7;
        let read = |a: u32, n: u32| -> Option<Vec<u8>> { (a + n <= 64).then(|| mem[a as usize..(a + n) as usize].to_vec()) };
        let r = rule::parse("I:0xH0000_0xH0004=7").unwrap();
        assert_eq!(rule::eval(&r, &read).ok().unwrap()["true"], json!(true));
    }

    #[cfg(unix)]
    #[test]
    fn the_channel_takes_a_game_and_reads_its_lines() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let sid = new_session_id();
            assert_eq!(sid.len(), 16);
            let (name, listener) = open_channel(&sid).unwrap();
            assert!(name.len() < 100);
            let client = tokio::spawn({
                let name = name.clone();
                async move {
                    use tokio::io::AsyncWriteExt;
                    let mut c = tokio::net::UnixStream::connect(&name).await.unwrap();
                    let long = format!("{{\"t\":\"x\",\"pad\":\"{}\"}}\n", "a".repeat(MAX_LINE + 10));
                    c.write_all(b"{\"t\":\"hello\",\"sdk\":1}\n").await.unwrap();
                    c.write_all(long.as_bytes()).await.unwrap();
                    c.write_all(b"{\"t\":\"unlock\",\"id\":3,\"frame\":9}\n").await.unwrap();
                }
            });
            let (conn, _) = listener.accept().await.unwrap();
            let mut r = tokio::io::BufReader::new(conn);
            let mut b = Vec::new();
            read_line(&mut r, &mut b).await.unwrap();
            assert_eq!(serde_json::from_slice::<Value>(&b).unwrap()["t"], "hello");
            b.clear();
            read_line(&mut r, &mut b).await.unwrap();
            assert!(b.is_empty(), "a line that's too long is dropped");
            b.clear();
            read_line(&mut r, &mut b).await.unwrap();
            assert_eq!(serde_json::from_slice::<Value>(&b).unwrap()["id"], 3);
            client.await.unwrap();
            let _ = std::fs::remove_file(&name);
        });
    }

    #[test]
    fn folders_are_written_without_the_players_name() {
        let install = Path::new("/games/x");
        assert_eq!(unexpand(Path::new("/games/x/user"), install), format!("{{game}}{}user", std::path::MAIN_SEPARATOR));
        assert_eq!(expand("{game}/user", install), PathBuf::from("/games/x/user"));
    }
}

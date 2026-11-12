//! The player's RecompStore account, and cloud saves.
//!
//! Signing in: the launcher asks recomplauncher.com for a short code, opens the browser on the
//! page that approves it (or shows the code and a QR code in Big Picture mode), and waits. Once the
//! player says yes on the website, the launcher gets a token for this PC. The website keeps a list
//! of PCs, so one can be signed out from there. No password ever passes through the launcher.
//!
//! Cloud saves: after each session a game's new save copy (saves.rs) goes up to the account, which
//! keeps the newest five per game, up to 150 MB in all. Before a game starts, a newer copy from
//! another PC is put in place first; if both changed, the player picks. docs/LAUNCHER-API.md has the
//! website's side.

use crate::library::Installed;
use crate::saves::{self, Spot};
use crate::{catalog, App, Error};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};

type Res<T> = Result<T, Error>;

fn api() -> String {
    format!("{}/api/launcher.php", catalog::base())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct User {
    #[serde(default)]
    pub uid: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub avatar: String,
    /// The developer page this account runs, when the website says it's a verified developer:
    /// {slug, name, verified}. Settings > Developers only shows for them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev: Option<serde_json::Value>,
}

/// account.json in the app's data folder. Windows keeps the token sealed (secret.rs); elsewhere
/// it stays in the owner-only file. Older Windows accounts migrate on their first read.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Account {
    #[serde(default)]
    token: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    sealed: String,
    user: User,
}

fn file(dir: &Path) -> PathBuf {
    dir.join("account.json")
}

// Account checks and background refreshes can run together. Keep a reader from seeing a
// truncated write, and keep simultaneous refreshes from overwriting parts of each other's JSON.
static ACCOUNT_FILE: Mutex<()> = Mutex::new(());

fn load(dir: &Path) -> Option<Account> {
    let _guard = ACCOUNT_FILE.lock().unwrap();
    let mut a = std::fs::read(file(dir)).ok().and_then(|b| serde_json::from_slice::<Account>(&b).ok())?;
    if !a.sealed.is_empty() {
        a.token = crate::secret::open(&a.sealed)?;
    } else if cfg!(windows) && !a.token.is_empty() {
        // Only Windows has a credential store. A plain token is already the saved format on
        // Linux and macOS, so reading it must not rewrite the account every time.
        store_locked(dir, Some(&a));
    }
    Some(a).filter(|a| !a.token.is_empty())
}

/// This PC's sign-in token, when signed in.
pub fn token(dir: &Path) -> Option<String> {
    load(dir).map(|a| a.token)
}

fn store(dir: &Path, a: Option<&Account>) {
    let _guard = ACCOUNT_FILE.lock().unwrap();
    store_locked(dir, a);
}

/// The caller holds ACCOUNT_FILE, including during migration from load().
fn store_locked(dir: &Path, a: Option<&Account>) {
    match a {
        Some(a) => {
            let mut keep = a.clone();
            if let Some(s) = crate::secret::seal(&a.token) {
                keep.sealed = s;
                keep.token = String::new();
            } else {
                keep.sealed = String::new();
            }
            // Replace only after a complete private copy exists. A stopped or failed write must
            // leave the previous sign-in intact; ACCOUNT_FILE also protects this staging path.
            let temp = dir.join("account.json.tmp");
            let saved = crate::secret::write_private(&temp, &serde_json::to_vec_pretty(&keep).unwrap_or_default())
                .and_then(|_| std::fs::rename(&temp, file(dir)));
            if let Err(e) = saved {
                let _ = std::fs::remove_file(&temp);
                crate::trail::note(format!("Account: couldn't save sign-in ({e})"));
            }
        }
        None => {
            let _ = std::fs::remove_file(file(dir));
            let _ = std::fs::remove_file(dir.join("account.json.tmp"));
            crate::secret::forget();
        }
    }
}

pub fn signed_in(dir: &Path) -> bool {
    load(dir).is_some()
}

/// The website said this PC's token is no good any more (signed out from the website).
fn forget(app: &AppHandle) {
    let st = app.state::<App>();
    store(&st.dir, None);
    let _ = app.emit("account", json!({"signedIn": false}));
}

/// Cloud saves paused on the site (it answered 423), or not reachable for them (403, a block in
/// front of the site): no more uploads until the launcher starts again, and saves stay on this PC.
static PAUSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub(crate) fn paused() -> bool {
    PAUSED.load(std::sync::atomic::Ordering::Relaxed)
}
/// What the site says while cloud saves are paused, when it doesn't say it itself.
const PAUSED_TEXT: &str = "Cloud saves are paused for now. Your saves on this PC are safe.";
/// Larger than this, a copy isn't sent (the site and what's in front of it refuse bigger ones).
const MAX_UPLOAD: usize = 95 * 1024 * 1024;

/// The address a save going up or coming down is gated by (net.rs rests it on its own, so one
/// cut-off upload never marks the whole site Offline).
fn save_url(action: &str) -> String {
    format!("{}?a={action}", api())
}

/// The account's cloud saves are full (the site answered 413): no copies go up on their own until
/// a saves list shows room for the one refused, or an hour passes. Said once, not on every try.
/// (When it said so, the size of the copy it refused.)
static FULL: Mutex<Option<(std::time::Instant, u64)>> = Mutex::new(None);
static FULL_TOLD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const FULL_TEXT: &str = "Your cloud saves are full (150 MB). Remove old cloud copies to make room; your saves stay on this PC.";
const FULL_REST: std::time::Duration = std::time::Duration::from_secs(3600);

fn full_now() -> bool {
    let mut f = FULL.lock().unwrap();
    match *f {
        Some((at, _)) if at.elapsed() < FULL_REST => true,
        Some(_) => {
            *f = None;
            false
        }
        None => false,
    }
}

fn set_full(size: u64) {
    *FULL.lock().unwrap() = Some((std::time::Instant::now(), size));
}

/// A saves list says how much room is left: enough for the copy that was refused, and uploads
/// start again.
fn room_now(used: u64, quota: u64) {
    let mut f = FULL.lock().unwrap();
    if let Some((_, size)) = *f {
        if quota > 0 && quota.saturating_sub(used) >= size {
            *f = None;
            FULL_TOLD.store(false, std::sync::atomic::Ordering::Relaxed);
            crate::trail::note("Cloud saves: there's room again, uploads go on");
        }
    }
}

fn is_full(e: &Error) -> bool {
    e.to_string().contains("cloud saves are full")
}

/// A copy that didn't go up whole (cut off, too slow, "didn't arrive whole"): tried again after 1,
/// 5, 15 and then 60 minutes, so at most a few tries an hour, whatever else asks for it.
static TRIES: Mutex<Option<std::collections::HashMap<String, (u32, std::time::Instant)>>> = Mutex::new(None);
const UPLOAD_WAITS: [u64; 4] = [60, 300, 900, 3600];

fn copy_key(id: &str, name: &str) -> String {
    format!("{id}/{name}")
}

/// Seconds until this copy may be tried again (None: now).
fn copy_wait_left(id: &str, name: &str) -> Option<u64> {
    let t = TRIES.lock().unwrap();
    let (_, next) = t.as_ref()?.get(&copy_key(id, name))?;
    let left = next.saturating_duration_since(std::time::Instant::now()).as_secs();
    (left > 0).then_some(left)
}

/// The soonest any copy waiting this way may be tried again.
fn next_copy_try() -> Option<u64> {
    let t = TRIES.lock().unwrap();
    let now = std::time::Instant::now();
    t.as_ref()?.values().map(|(_, next)| next.saturating_duration_since(now).as_secs()).min()
}

fn copy_failed(id: &str, name: &str, why: &str) -> u64 {
    let mut t = TRIES.lock().unwrap();
    let e = t.get_or_insert_with(Default::default).entry(copy_key(id, name)).or_insert((0, std::time::Instant::now()));
    e.0 = e.0.saturating_add(1);
    let secs = UPLOAD_WAITS[(e.0 as usize - 1).min(UPLOAD_WAITS.len() - 1)];
    e.1 = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    crate::trail::note(format!("Cloud saves: upload of {id} failed ({why}), next try in {secs}s"));
    secs
}

fn copy_went_up(id: &str, name: &str) {
    if let Some(t) = TRIES.lock().unwrap().as_mut() {
        t.remove(&copy_key(id, name));
    }
}

async fn answer(app: &AppHandle, r: reqwest::Response) -> Res<Value> {
    let code = r.status().as_u16();
    let action = r.url().query_pairs().find(|(k, _)| k == "a").map(|(_, v)| v.to_string()).unwrap_or_default();
    let cloud = matches!(action.as_str(), "saves" | "save-get" | "save-put" | "save-delete");
    let v: Value = r.json().await.unwrap_or(Value::Null);
    let said = v.get("error").and_then(Value::as_str).map(str::trim).filter(|w| !w.is_empty() && w.len() <= 300).map(String::from);
    if code == 423 {
        PAUSED.store(true, std::sync::atomic::Ordering::Relaxed);
        return Err(Error::Msg(said.unwrap_or_else(|| PAUSED_TEXT.into())));
    }
    // Cloud saves blocked before the site (Cloudflare): not a sign-in problem.
    if code == 403 && cloud {
        PAUSED.store(true, std::sync::atomic::Ordering::Relaxed);
        return Err(Error::Msg(said.unwrap_or_else(|| "Cloud saves aren't available right now. Your saves on this PC are safe.".into())));
    }
    if code == 401 {
        forget(app);
        return Err(Error::Msg("You were signed out. Sign in again to use cloud saves.".into()));
    }
    // The site having trouble (blocked, busy, down): the same as no connection, so it's quiet.
    // Unless it says why itself (cloud saves paused for a while, say): then that's what shows,
    // still as the site being unavailable, so saves waiting to go up are tried again later.
    if crate::net::trouble_status(code) {
        if let Some(why) = said {
            return Err(Error::Net(why));
        }
        return Err(Error::Net("Can't reach recomplauncher.com right now.".into()));
    }
    if code >= 400 || v.get("ok") == Some(&Value::Bool(false)) {
        let why = v.get("error").and_then(Value::as_str).map(String::from).unwrap_or_else(|| format!("The website didn't answer properly ({code}). Try again in a moment."));
        return Err(Error::Msg(why));
    }
    Ok(v)
}

fn net(e: reqwest::Error) -> Error {
    Error::Net(if e.is_timeout() || e.is_connect() { "Can't reach recomplauncher.com right now.".into() } else { e.to_string() })
}

async fn get(app: &AppHandle, q: &[(&str, &str)]) -> Res<Value> {
    let st = app.state::<App>();
    let a = load(&st.dir).ok_or_else(|| Error::Msg("Sign in first.".into()))?;
    let url = api();
    let r = crate::net::send(st.http.get(&url).query(q).bearer_auth(&a.token).header("X-RL-Token", a.token.as_str()).timeout(crate::net::QUICK), &url, true).await?;
    answer(app, r).await
}

async fn post(app: &AppHandle, a_: &str, body: Value, token: bool) -> Res<Value> {
    let st = app.state::<App>();
    let url = api();
    let mut req = st.http.post(&url).query(&[("a", a_)]).json(&body).timeout(crate::net::QUICK);
    if token {
        let a = load(&st.dir).ok_or_else(|| Error::Msg("Sign in first.".into()))?;
        req = req.bearer_auth(&a.token).header("X-RL-Token", a.token.as_str());
    }
    answer(app, crate::net::send(req, &url, true).await?).await
}

// ---------- the saved list, kept with the account ----------

/// The account's saved PC games (the website's library): {saved: [{id, at}]}. Installed games stay
/// per PC; only the saved ("Not installed") list goes with the account.
#[tauri::command]
pub async fn library_pull(app: AppHandle) -> Res<Value> {
    get(&app, &[("a", "library")]).await
}

/// Saves or unsaves games on the account: one (`game`), or up to 500 at once for the first merge.
#[tauri::command]
pub async fn library_push(app: AppHandle, games: Vec<String>, on: bool) -> Res<Value> {
    let games: Vec<String> = games.into_iter().filter(|g| !g.is_empty() && g.len() <= 200).take(500).collect();
    if games.is_empty() {
        return Ok(json!({}));
    }
    let body = if games.len() == 1 { json!({ "game": games[0], "on": on }) } else { json!({ "games": games, "on": on }) };
    post(&app, "library-save", body, true).await
}

// ---------- signing in ----------

/// The sign-in that's waiting for the player to approve it on the website.
static LINK: Mutex<Option<String>> = Mutex::new(None);

#[derive(Serialize)]
pub struct LinkStart {
    code: String,
    url: String,
    qr: String,
    interval: u64,
    expires: u64,
}

fn qr_svg(text: &str) -> String {
    qrcode::QrCode::new(text.as_bytes())
        .map(|q| {
            q.render::<qrcode::render::svg::Color>()
                .min_dimensions(220, 220)
                .dark_color(qrcode::render::svg::Color("#0B1220"))
                .light_color(qrcode::render::svg::Color("#F4F8FC"))
                .quiet_zone(true)
                .build()
        })
        .unwrap_or_default()
}

/// A code for this PC. `create`: the website shows "make an account" first.
#[tauri::command]
pub async fn account_start(app: AppHandle, create: bool) -> Res<LinkStart> {
    let v = post(&app, "link-start", json!({"device": saves::device_name(), "os": std::env::consts::OS, "version": env!("CARGO_PKG_VERSION")}), false).await?;
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let (code, poll) = (s("code"), s("poll"));
    if code.is_empty() || poll.is_empty() {
        return Err(Error::Msg("The website didn't answer properly. Try again in a moment.".into()));
    }
    *LINK.lock().unwrap() = Some(poll);
    let mut url = s("url");
    if create {
        url.push_str(if url.contains('?') { "&new=1" } else { "?new=1" });
    }
    Ok(LinkStart {
        qr: qr_svg(&url),
        code,
        url,
        interval: v.get("interval").and_then(Value::as_u64).unwrap_or(3).max(2),
        expires: v.get("expires").and_then(Value::as_u64).unwrap_or(600),
    })
}

/// Has the player approved it yet? "pending", "ok", "expired" or "denied".
#[tauri::command]
pub async fn account_poll(app: AppHandle) -> Res<Value> {
    let Some(poll) = LINK.lock().unwrap().clone() else { return Ok(json!({"status": "expired"})) };
    let v = post(&app, "link-poll", json!({"poll": poll}), false).await?;
    let status = v.get("status").and_then(Value::as_str).unwrap_or("pending").to_string();
    if status == "ok" {
        let token = v.get("token").and_then(Value::as_str).unwrap_or("").to_string();
        let user: User = serde_json::from_value(v.get("account").cloned().unwrap_or(Value::Null)).unwrap_or_default();
        if token.is_empty() {
            return Err(Error::Msg("The website didn't answer properly. Try again in a moment.".into()));
        }
        let st = app.state::<App>();
        store(&st.dir, Some(&Account { token, user: user.clone(), ..Default::default() }));
        *LINK.lock().unwrap() = None;
        let _ = app.emit("account", json!({"signedIn": true, "user": user}));
        // Anything that was waiting to go up.
        let app2 = app.clone();
        tauri::async_runtime::spawn(async move { upload_pending(&app2).await });
        return Ok(json!({"status": "ok", "user": user}));
    }
    if status != "pending" {
        *LINK.lock().unwrap() = None;
    }
    Ok(json!({"status": status}))
}

#[tauri::command(async)]
pub fn account_cancel() {
    *LINK.lock().unwrap() = None;
}

/// Who's signed in, and how much of their cloud space is used (asked of the website when it can be).
#[tauri::command]
pub async fn account_state(app: AppHandle, fresh: bool) -> Res<Value> {
    let st = app.state::<App>();
    let Some(a) = load(&st.dir) else { return Ok(json!({"signedIn": false})) };
    if !fresh {
        return Ok(json!({"signedIn": true, "user": a.user}));
    }
    // No answer: the account shows as offline, and the site is asked again later (see net.rs).
    let r = get(&app, &[("a", "me")]).await;
    let why = match &r {
        Err(e) => e.to_string(),
        Ok(_) => String::new(),
    };
    match r {
        Ok(v) => {
            let user: User = serde_json::from_value(v.get("account").cloned().unwrap_or(Value::Null)).unwrap_or(a.user.clone());
            store(&st.dir, Some(&Account { token: a.token, user: user.clone(), ..Default::default() }));
            Ok(json!({"signedIn": true, "user": user, "used": v.pointer("/saves/used"), "quota": v.pointer("/saves/quota")}))
        }
        Err(Error::Net(_)) => Ok(json!({"signedIn": true, "user": a.user, "offline": true, "why": why})),
        Err(e) if !signed_in(&st.dir) => Err(e),
        Err(_) => Ok(json!({"signedIn": true, "user": a.user, "offline": true, "why": why})),
    }
}

#[tauri::command]
pub async fn account_signout(app: AppHandle) -> Res<()> {
    let _ = post(&app, "signout", json!({}), true).await;
    forget(&app);
    Ok(())
}

// ---------- a game's saves ----------

fn game(st: &App, id: &str) -> Res<Installed> {
    let g = st.lib.lock().unwrap().installed.get(id).cloned();
    // One game of several with saves of its own: its copies are its own, on the recomp's install.
    g.or_else(|| own_saves(st, id).map(|p| p.g)).ok_or_else(|| Error::Msg("It isn't installed.".into()))
}

/// One game of a recomp that carries several, when it declares its own saves.
fn own_saves(st: &App, id: &str) -> Option<crate::GamePick> {
    crate::game_of(st, id).filter(|p| !p.game.saves.is_empty())
}

/// Where this game's saves are, best first.
pub fn spots_of(st: &App, id: &str, g: &Installed) -> Vec<Spot> {
    if let Some(p) = own_saves(st, id) {
        // Only the game's own: the recomp's other places are the other games'.
        let mut g = g.clone();
        g.save_place = Default::default();
        g.more_save_places.clear();
        g.found_saves.clear();
        return saves::spots(&p.game.saves, &[], &g);
    }
    let cat = crate::cached_catalog(st);
    let entry = catalog::app(&cat, id).cloned().unwrap_or(Value::Null);
    let dev = crate::profile::find(&g.path, &g.repo, &entry).map(|p| p.saves).unwrap_or_default();
    let from_cat: Vec<String> = entry.get("saves").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    saves::spots(&dev, &from_cat, g)
}

/// One game's saves at the same time: two backups of one game never overlap.
static BUSY: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Waits its turn for the saves, but not forever: something else that's stuck can't keep a
/// backup spinning.
async fn busy() -> Res<tokio::sync::MutexGuard<'static, ()>> {
    tokio::time::timeout(std::time::Duration::from_secs(90), BUSY.lock())
        .await
        .map_err(|_| Error::Msg("Another save copy is still being made. Try again in a minute.".into()))
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Res<T> {
    tokio::task::spawn_blocking(f).await.map_err(|e| Error::Msg(e.to_string()))
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CloudCopy {
    pub v: String,
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub made: u64,
    #[serde(default)]
    pub device: String,
    #[serde(default)]
    pub played: u64,
}

async fn cloud_list(app: &AppHandle, id: &str) -> Res<(Vec<CloudCopy>, Value)> {
    let v = get(app, &[("a", "saves"), ("game", id)]).await?;
    let list: Vec<CloudCopy> = serde_json::from_value(v.get("versions").cloned().unwrap_or(json!([]))).unwrap_or_default();
    let mut list = list;
    list.sort_by(|a, b| b.made.cmp(&a.made));
    if let (Some(used), Some(quota)) = (v.get("used").and_then(Value::as_u64), v.get("quota").and_then(Value::as_u64)) {
        room_now(used, quota);
    }
    Ok((list, json!({"used": v.get("used"), "quota": v.get("quota")})))
}

const SYNC_CONFLICT: &str = "Both save copies changed. Choose one in Saves → Sync Now.";

/// Puts one local copy up if the cloud hasn't changed since the last sync.
async fn upload(app: &AppHandle, id: &str, name: &str) -> Res<()> {
    let st = app.state::<App>();
    let mut state = saves::state(&st.dir, id);
    let copy = saves::manifest(&saves::dir_of(&st.dir, id).join(name))
        .ok_or_else(|| Error::Msg("That copy is gone.".into()))?;
    let (list, _) = cloud_list(app, id).await?;
    let Some(top) = list.first() else { return upload_copy(app, id, name, 0).await };
    if top.hash == copy.hash {
        state.synced_hash = copy.hash;
        if state.pending.as_deref() == Some(name) {
            state.pending = None;
        }
        saves::set_state(&st.dir, id, &state);
        return Ok(());
    }
    if top.hash != state.synced_hash {
        return Err(Error::Msg(SYNC_CONFLICT.into()));
    }
    upload_copy(app, id, name, top.made.saturating_add(1)).await
}

async fn upload_copy(app: &AppHandle, id: &str, name: &str, earliest: u64) -> Res<()> {
    let st = app.state::<App>();
    let path = saves::dir_of(&st.dir, id).join(name);
    let man = saves::manifest(&path).ok_or_else(|| Error::Msg("That copy is gone.".into()))?;
    if paused() {
        return Err(Error::Msg(PAUSED_TEXT.into()));
    }
    // On its own (not a button): full, or this copy is waiting its turn after a failed try.
    if !crate::net::forced() {
        if full_now() {
            return Err(Error::Msg(FULL_TEXT.into()));
        }
        if let Some(left) = copy_wait_left(id, name) {
            return Err(Error::Net(format!("This save goes up in {left}s (the last try didn't get through).")));
        }
    }
    let body = tokio::fs::read(&path).await?;
    // Too big to go up: it stays on this PC, and isn't tried again (the next copy is).
    if body.len() > MAX_UPLOAD {
        let mut s = saves::state(&st.dir, id);
        if s.pending.as_deref() == Some(name) {
            s.pending = None;
            saves::set_state(&st.dir, id, &s);
        }
        crate::trail::note(format!("Cloud saves: {id}'s copy is {} MB, too big to send", body.len() / (1024 * 1024)));
        return Err(Error::Msg("This save is too big for cloud saves (over 95 MB). It's kept on this PC.".into()));
    }
    let a = load(&st.dir).ok_or_else(|| Error::Msg("Sign in first.".into()))?;
    let (made, played) = (man.made.max(earliest).to_string(), man.played.to_string());
    let size = body.len() as u64;
    let url = api();
    let req = st
        .http
        .put(&url)
        .query(&[("a", "save-put"), ("game", id), ("hash", &man.hash), ("made", &made), ("played", &played), ("device", &man.device)])
        .bearer_auth(&a.token).header("X-RL-Token", a.token.as_str())
        .header("Content-Type", "application/zip")
        .body(body)
        .timeout(std::time::Duration::from_secs(300));
    // The site resting for saves: not a try of this copy, so its own clock isn't moved.
    if !crate::net::forced() && crate::net::resting(&save_url("save-put"), true) {
        return Err(Error::Net("Can't reach recomplauncher.com right now.".into()));
    }
    let r = match crate::net::send(req, &save_url("save-put"), true).await {
        Ok(r) => r,
        Err(e) => {
            copy_failed(id, name, if matches!(e, Error::Net(_)) { "no answer, cut off or too slow" } else { "error" });
            return Err(e);
        }
    };
    let code = r.status().as_u16();
    if let Err(e) = answer(app, r).await {
        if code == 413 || is_full(&e) {
            set_full(size);
            crate::trail::note(format!("Cloud saves: upload of {id} failed (cloud saves full), no uploads until there's room"));
            return Err(Error::Msg(FULL_TEXT.into()));
        }
        // Cut off on the way (the site says it didn't arrive whole), or the site busy (52x):
        // tried again later, on this copy's own clock.
        if code == 400 || matches!(e, Error::Net(_)) {
            copy_failed(id, name, &format!("{code}"));
        }
        return Err(e);
    }
    copy_went_up(id, name);
    let mut s = saves::state(&st.dir, id);
    s.synced_hash = man.hash;
    // The copy waiting is this one or an older one: what's in the cloud now is newer, so it
    // doesn't go up after it.
    if s.pending.as_deref().map(|p| p == name || copy_time(p) <= copy_time(name)).unwrap_or(false) {
        s.pending = None;
    }
    saves::set_state(&st.dir, id, &s);
    Ok(())
}

/// When a local copy was made, from its name ("<time>.zip").
fn copy_time(name: &str) -> u64 {
    name.strip_suffix(".zip").and_then(|n| n.parse().ok()).unwrap_or(0)
}

/// Select a retry only after earlier backups/restores finish, and keep them excluded until the
/// caller finishes uploading it. A restore may have cancelled the queue while this was waiting.
async fn pending_copy(dir: &Path, id: &str, library: &Mutex<crate::library::Library>) -> (tokio::sync::MutexGuard<'static, ()>, Option<String>) {
    let guard = BUSY.lock().await;
    // The game may have been removed or had cloud saves turned off while this retry waited.
    // One game of several keeps its copies under its full id, with the recomp's cloud setting.
    let enabled = {
        let l = library.lock().unwrap();
        let g = l.installed.get(id).or_else(|| crate::games::split(id).and_then(|(r, _)| l.installed.get(r)));
        g.map(|g| g.cloud == Some(true)).unwrap_or(false)
    };
    let pending = if enabled { saves::state(dir, id).pending } else { None };
    (guard, pending)
}

/// Copies that didn't get up before (offline, or not signed in yet), one after another. Any that
/// still don't make it are tried again once the site can be asked again (see net.rs).
pub async fn upload_pending(app: &AppHandle) {
    let st = app.state::<App>();
    let forced = crate::net::forced();
    if !signed_in(&st.dir) || (!forced && (crate::net::resting(&save_url("save-put"), false) || full_now())) || paused() {
        return;
    }
    let ids: Vec<String> = st.lib.lock().unwrap().installed.values().filter(|g| g.cloud == Some(true)).map(|g| g.id.clone()).collect();
    let mut no_answer = false;
    for id in ids {
        let (_busy, pending) = pending_copy(&st.dir, &id, &st.lib).await;
        if let Some(name) = pending {
            // Waiting its turn after a failed try: the others still go, and it's tried on its clock.
            if !forced && copy_wait_left(&id, &name).is_some() {
                no_answer = true;
                continue;
            }
            match upload(app, &id, &name).await {
                Ok(()) => {}
                // Paused on the site: the rest wait for the next start.
                Err(_) if paused() => break,
                // Full: none of them go until there's room.
                Err(e) if is_full(&e) => break,
                // The site isn't answering for saves at all: the rest wait, and go when it does.
                Err(Error::Net(_)) if crate::net::resting(&save_url("save-put"), false) => {
                    no_answer = true;
                    break;
                }
                // This copy didn't get through (cut off, too slow, not whole): it's tried again on
                // its own clock, and the others still go.
                Err(_) if copy_wait_left(&id, &name).is_some() => {
                    no_answer = true;
                    continue;
                }
                // Refused for this one (the copy is gone, say): it waits for the next launch.
                Err(_) => continue,
            }
        }
    }
    if no_answer {
        retry_pending_later(app);
    } else {
        RETRIES.store(0, std::sync::atomic::Ordering::SeqCst);
    }
}

static RETRY_PLANNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static RETRIES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Pending copies tried again once the site can be asked again, later each time (30 seconds,
/// doubling up to an hour, plus a random part); only after the site didn't answer.
fn retry_pending_later(app: &AppHandle) {
    if RETRY_PLANNED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let n = RETRIES.fetch_add(1, std::sync::atomic::Ordering::SeqCst).min(7);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // The site's own rest, or the soonest a copy that failed may be tried again.
        let wait = crate::net::wait_left(&save_url("save-put")).max(next_copy_try().unwrap_or((30u64 << n).min(3600)));
        crate::trail::note(format!("Cloud saves: waiting uploads tried again in {wait}s"));
        tokio::time::sleep(crate::net::spread(wait, 0.3)).await;
        RETRY_PLANNED.store(false, std::sync::atomic::Ordering::SeqCst);
        if paused() {
            return;
        }
        Box::pin(upload_pending(&app)).await;
    });
}

/// After a session (and from "Back up now"): a new copy if anything changed, sent up when cloud
/// saves are on. Returns the new copy, if there is one.
pub async fn after_session(app: &AppHandle, id: &str) -> Res<Option<saves::Copy>> {
    let _busy = busy().await?;
    let st = app.state::<App>();
    let g = game(&st, id)?;
    let spots = spots_of(&st, id, &g);
    let (dir, id2, g2) = (st.dir.clone(), id.to_string(), g.clone());
    let copy = blocking(move || saves::backup(&dir, &id2, &spots, &g2)).await??;
    if g.cloud == Some(true) && signed_in(&st.dir) {
        let s = saves::state(&st.dir, id);
        let latest = saves::list(&st.dir, id).into_iter().next();
        if let Some(l) = latest.filter(|l| l.hash != s.synced_hash) {
            if let Err(e) = upload(app, id, &l.name).await {
                let too_big = e.to_string().starts_with("This save is too big");
                if !too_big {
                    let mut s = saves::state(&st.dir, id);
                    s.pending = Some(l.name.clone());
                    saves::set_state(&st.dir, id, &s);
                }
                if matches!(e, Error::Net(_)) && !paused() {
                    retry_pending_later(app);
                }
                // Paused: said once each time the launcher runs, not after every game.
                static TOLD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
                if e.to_string() == SYNC_CONFLICT {
                    let _ = app.emit("toast", format!("{}: {SYNC_CONFLICT}", g.name));
                } else if too_big {
                    let _ = app.emit("toast", format!("{}: {e}", g.name));
                } else if is_full(&e) {
                    if !FULL_TOLD.swap(true, std::sync::atomic::Ordering::Relaxed) {
                        let _ = app.emit("toast", e.to_string());
                    }
                } else if matches!(e, Error::Net(_)) && e.to_string().starts_with("This save goes up in") {
                    // Waiting its turn after a failed try: nothing new to say.
                } else if !paused() || !TOLD.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    let _ = app.emit("toast", format!("Your save for {} is backed up on this PC. It goes to the cloud next time: {e}", g.name));
                }
            }
        }
    }
    Ok(copy)
}

/// Gets a cloud copy and puts it in place (what's there now is kept as a local copy first).
async fn take_cloud(app: &AppHandle, id: &str, c: &CloudCopy) -> Res<usize> {
    let st = app.state::<App>();
    let a = load(&st.dir).ok_or_else(|| Error::Msg("Sign in first.".into()))?;
    let url = api();
    let req = st
        .http
        .get(&url)
        .query(&[("a", "save-get"), ("game", id), ("v", &c.v)])
        .bearer_auth(&a.token).header("X-RL-Token", a.token.as_str())
        .timeout(std::time::Duration::from_secs(300));
    let r = crate::net::send(req, &save_url("save-get"), true).await?;
    if r.status().as_u16() != 200 {
        answer(app, r).await?;
        return Err(Error::Msg("That cloud save couldn't be downloaded.".into()));
    }
    let bytes = r.bytes().await.map_err(net)?;
    let d = saves::dir_of(&st.dir, id);
    tokio::fs::create_dir_all(&d).await?;
    let tmp = d.join(format!("cloud-{}.download", c.v.chars().filter(|x| x.is_ascii_alphanumeric()).collect::<String>()));
    tokio::fs::write(&tmp, &bytes).await?;
    let g = game(&st, id)?;
    let spots = spots_of(&st, id, &g);
    let (dir, id2, t2) = (st.dir.clone(), id.to_string(), tmp.clone());
    let n = blocking(move || saves::restore(&dir, &id2, &t2, &spots, &g)).await;
    let _ = tokio::fs::remove_file(&tmp).await;
    let n = n??;
    let mut s = saves::state(&st.dir, id);
    s.synced_hash = c.hash.clone();
    s.last_hash = c.hash.clone();
    saves::set_state(&st.dir, id, &s);
    Ok(n)
}

/// Whether the cloud copy can be put in place without replacing locally changed saves.
fn should_restore(local: &str, synced: &str, cloud: &str, empty: bool) -> bool {
    if cloud == local {
        return false;
    }
    let local_changed = !local.is_empty() && local != synced;
    let cloud_changed = cloud != synced;
    // A reset/reinstall can remove the saves while the last synced hash is still remembered.
    // The cloud copy is needed then even if nobody has played on another PC since that sync.
    empty || (cloud_changed && !local_changed)
}

fn reserve_sync(st: &App, id: &str) -> Res<crate::Starting> {
    let install = crate::recomp_of(st, id);
    let mut starting = crate::STARTING.lock().unwrap();
    if starting.contains(&install) || st.running.lock().unwrap().contains_key(&install)
        || st.pids.lock().unwrap().contains_key(&install) {
        return Err(Error::Msg("Close the game first.".into()));
    }
    starting.push(install.clone());
    Ok(crate::Starting(install))
}

/// Upload the checked live save, even if a different save is still in the backup history.
async fn upload_current(app: &AppHandle, id: &str, g: Installed, spots: Vec<Spot>, hash: String, after: u64) -> Res<()> {
    let dir = app.state::<App>().dir.clone();
    let id2 = id.to_string();
    let copy = blocking(move || {
        saves::backup(&dir, &id2, &spots, &g)?;
        saves::list(&dir, &id2).into_iter().next().filter(|c| c.hash == hash)
            .ok_or_else(|| Error::Msg("The saves changed. Try syncing again.".into()))
    }).await??;
    // A player can choose a save that was backed up before the current cloud copy.
    upload_copy(app, id, &copy.name, after.saturating_add(1)).await
}

/// Before Play: what to do about the cloud.
/// "play": go ahead. "restored": a newer save from another PC was put in place. "conflict": both
/// changed, the player picks (`local` and `cloud` say what each is). "offline": couldn't check.
#[tauri::command]
pub async fn saves_check(app: AppHandle, id: String, manual: Option<bool>) -> Res<Value> {
    if manual == Some(true) {
        return crate::net::now(check_saves(app, id, true)).await;
    }
    check_saves(app, id, false).await
}

async fn check_saves(app: AppHandle, id: String, manual: bool) -> Res<Value> {
    let st = app.state::<App>();
    let g = game(&st, &id)?;
    if g.cloud != Some(true) || !signed_in(&st.dir) {
        if manual {
            return Err(Error::Msg("Turn on cloud saves for this game first.".into()));
        }
        return Ok(json!({"action": "play"}));
    }
    let _busy = busy().await?;
    let _reserved = reserve_sync(&st, &id)?;
    let (list, _) = match cloud_list(&app, &id).await {
        Ok(x) => x,
        Err(Error::Net(_)) => return Ok(json!({"action": "offline"})),
        Err(e) => return Err(e),
    };
    let top = list.first();
    if !manual && top.is_none() {
        return Ok(json!({"action": "play"}));
    }
    let spots = spots_of(&st, &id, &g);
    let (g2, spots2) = (g.clone(), spots.clone());
    let (entries, infos, local) = blocking(move || saves::current(&spots2, &g2)).await?;
    if entries.len() != infos.len() {
        return Err(Error::Msg("Some save files couldn't be read. Try syncing again.".into()));
    }
    let s = saves::state(&st.dir, &id);
    let Some(top) = top else {
        if local.is_empty() {
            return Ok(json!({"action": "play"}));
        }
        upload_current(&app, &id, g, spots, local, 0).await?;
        return Ok(json!({"action": "uploaded"}));
    };
    if top.hash == local {
        if s.synced_hash != local || s.pending.is_some() {
            let mut s = s;
            s.synced_hash = local;
            s.pending = None;
            saves::set_state(&st.dir, &id, &s);
        }
        return Ok(json!({"action": "play"}));
    }
    if should_restore(&local, &s.synced_hash, &top.hash, entries.is_empty()) {
        let n = take_cloud(&app, &id, top).await?;
        return Ok(json!({"action": "restored", "files": n, "cloud": top}));
    }
    if top.hash == s.synced_hash {
        if manual {
            upload_current(&app, &id, g, spots, local, top.made).await?;
            return Ok(json!({"action": "uploaded"}));
        }
        // After Play, the local changes go up at the end of the session.
        return Ok(json!({"action": "play"}));
    }
    let newest = entries.iter().filter_map(|e| std::fs::metadata(&e.abs).ok()?.modified().ok()).max();
    let when = newest.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    Ok(json!({"action": "conflict", "local": {"when": when, "device": saves::device_name(), "played": g.played_secs, "hash": local}, "cloud": top}))
}

/// A shortcut or tray launch has no conflict prompt, so let the player resolve it in the launcher.
pub async fn sync_quiet(app: &AppHandle, id: &str) -> Res<()> {
    let st = app.state::<App>();
    let save_id = if own_saves(&st, id).is_some() { id.to_string() } else { crate::recomp_of(&st, id) };
    let result = saves_check(app.clone(), save_id, None).await?;
    if result["action"] == "conflict" {
        return Err(Error::Msg(SYNC_CONFLICT.into()));
    }
    Ok(())
}

/// The player's pick after a conflict: keep this PC's save (it goes up as the newest), or the
/// cloud's (this PC's is kept as a local copy first).
#[tauri::command]
pub async fn saves_pick(app: AppHandle, id: String, keep: String, local_hash: String, cloud_version: String) -> Res<()> {
    let _busy = busy().await?;
    let st = app.state::<App>();
    let g = game(&st, &id)?;
    if g.cloud != Some(true) || !signed_in(&st.dir) {
        return Err(Error::Msg("Turn on cloud saves for this game first.".into()));
    }
    let _reserved = reserve_sync(&st, &id)?;
    crate::net::now(async {
        let changed = || Error::Msg("The saves changed. Try syncing again.".into());
        let (list, _) = cloud_list(&app, &id).await?;
        let top = list.first().filter(|c| c.v == cloud_version).ok_or_else(changed)?;
        let spots = spots_of(&st, &id, &g);
        let (g2, spots2) = (g.clone(), spots.clone());
        let (entries, infos, hash) = blocking(move || saves::current(&spots2, &g2)).await?;
        if hash != local_hash || entries.len() != infos.len() {
            return Err(changed());
        }
        match keep.as_str() {
            "cloud" => take_cloud(&app, &id, top).await.map(|_| ()),
            "local" => upload_current(&app, &id, g, spots, hash, top.made).await,
            _ => Err(Error::Msg("Choose this PC or the cloud.".into())),
        }
    }).await
}

/// Turns cloud saves on or off for a game. On: what's on this PC goes up now (unless the cloud
/// already has something newer, which the next Play sorts out).
#[tauri::command]
pub async fn saves_cloud(app: AppHandle, id: String, on: bool) -> Res<Installed> {
    let st = app.state::<App>();
    let g = {
        let mut l = st.lib.lock().unwrap();
        let g = l.installed.get_mut(&id).ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
        g.cloud = Some(on);
        let g = g.clone();
        let _ = l.save(&st.dir);
        g
    };
    if on && signed_in(&st.dir) {
        let app2 = app.clone();
        let id2 = id.clone();
        tauri::async_runtime::spawn(async move {
            let has_cloud = cloud_list(&app2, &id2).await.map(|(l, _)| !l.is_empty()).unwrap_or(true);
            if !has_cloud {
                let _ = after_session(&app2, &id2).await;
                let _ = app2.emit("saves", json!({"id": id2}));
            }
        });
    }
    Ok(g)
}

#[derive(Serialize)]
pub struct SpotInfo {
    template: String,
    path: String,
    from: &'static str,
    exists: bool,
}

/// Everything the Saves tab shows.
#[tauri::command]
pub async fn saves_info(app: AppHandle, id: String) -> Res<Value> {
    let st = app.state::<App>();
    let g = game(&st, &id)?;
    let spots: Vec<SpotInfo> = spots_of(&st, &id, &g)
        .into_iter()
        .map(|s| {
            let p = saves::resolve_for(&s.template, &g);
            let shown = if s.template.contains('*') { p.parent().map(Path::to_path_buf).unwrap_or(p.clone()) } else { p.clone() };
            SpotInfo { exists: shown.exists(), path: p.to_string_lossy().to_string(), template: s.template, from: s.from }
        })
        .collect();
    let local = saves::list(&st.dir, &id);
    let state = saves::state(&st.dir, &id);
    let mut cloud = Value::Null;
    let mut space = Value::Null;
    let mut offline = false;
    if g.cloud == Some(true) && signed_in(&st.dir) {
        match cloud_list(&app, &id).await {
            Ok((l, sp)) => {
                cloud = serde_json::to_value(l).unwrap_or(Value::Null);
                space = sp;
            }
            Err(Error::Net(_)) => offline = true,
            Err(_) => {}
        }
    }
    // Copies on this PC that haven't gone up yet: the newest ones, back to the one that's in the cloud.
    let waiting = match state.pending.as_deref() {
        Some(p) => local.iter().take_while(|c| c.hash != state.synced_hash).filter(|c| c.when >= copy_time(p)).count().max(1),
        None => 0,
    };
    Ok(json!({
        "spots": spots, "local": local, "cloud": cloud, "space": space, "offline": offline,
        "cloudOn": g.cloud, "signedIn": signed_in(&st.dir), "synced": state.synced_hash, "pending": state.pending.is_some(),
        "waiting": waiting, "status": STATUS_PAGE,
        // Seconds until a copy that didn't get through is tried again, and whether the account's
        // cloud saves are full (no uploads until there's room).
        "retryIn": state.pending.as_deref().and_then(|p| copy_wait_left(&id, p)), "full": full_now(),
    }))
}

/// Where players see whether the site is having trouble on our side.
const STATUS_PAGE: &str = "https://recomplauncher.com/status";

fn upload_candidate(dir: &Path, id: &str, running: bool) -> Res<Option<saves::Copy>> {
    let s = saves::state(dir, id);
    let latest = saves::list(dir, id).into_iter().next();
    // The newest local archive can still hold the save from before a restore. During play we
    // cannot replace it with a current backup, so ask the player to close the game first.
    if running && latest.as_ref().map(|l| l.hash != s.last_hash).unwrap_or(false) {
        return Err(Error::Msg("Close the game first to back up and upload its restored save.".into()));
    }
    Ok(latest.filter(|l| l.hash != s.synced_hash))
}

/// Upload Now, from a game's Saves tab: the current save copied (when the game isn't running)
/// and sent up straight away, and any copies still waiting with it. Asked now, even while the
/// launcher is leaving the website alone after trouble. Then what the tab shows.
#[tauri::command]
pub async fn saves_upload_now(app: AppHandle, id: String) -> Res<Value> {
    let st = app.state::<App>();
    let g = game(&st, &id)?;
    if g.cloud != Some(true) || !signed_in(&st.dir) {
        return Err(Error::Msg("Turn on cloud saves for this game first.".into()));
    }
    let running = st.running.lock().unwrap().contains_key(&id);
    let r: Res<()> = crate::net::now(async {
        {
            let _busy = busy().await?;
            // A fresh copy of what's there now (a running game's saves wait until it closes).
            if !running {
                let spots = spots_of(&st, &id, &g);
                let (dir, id2, g2) = (st.dir.clone(), id.clone(), g.clone());
                blocking(move || saves::backup(&dir, &id2, &spots, &g2)).await??;
            }
            if let Some(l) = upload_candidate(&st.dir, &id, running)? {
                if let Err(e) = upload(&app, &id, &l.name).await {
                    // It waits, and goes when the site answers again (once: no more tries now).
                    if !e.to_string().starts_with("This save is too big") {
                        let mut s = saves::state(&st.dir, &id);
                        s.pending = Some(l.name.clone());
                        saves::set_state(&st.dir, &id, &s);
                    }
                    return Err(e);
                }
            }
        }
        // It went up: the other games' waiting copies go too.
        upload_pending(&app).await;
        Ok(())
    })
    .await;
    crate::trail::note(format!("Cloud saves: Upload Now for {id}: {}", match &r { Ok(()) => "done".to_string(), Err(e) => e.to_string() }));
    if matches!(r, Err(Error::Net(_))) && !paused() {
        retry_pending_later(&app);
    }
    match r {
        Ok(()) => crate::net::now(saves_info(app.clone(), id)).await,
        Err(Error::Net(_)) => Err(Error::Msg(format!("The website still can't be reached. Your saves are safe on this PC and go up when it's back. See {STATUS_PAGE} for whether it's on our side."))),
        Err(e) => Err(e),
    }
}

/// Check Again (a game's Saves tab, or Retry on the Account screen): the website asked straight
/// away, and any copies waiting go up if it answers. `id`: the game whose tab asked, if any.
#[tauri::command]
pub async fn cloud_retry(app: AppHandle, id: Option<String>) -> Res<Value> {
    let st = app.state::<App>();
    if !signed_in(&st.dir) {
        return Err(Error::Msg("Sign in first.".into()));
    }
    let r = crate::net::now(async {
        let v = get(&app, &[("a", "me")]).await;
        if let Err(Error::Net(e)) = v {
            return Err(Error::Net(e));
        }
        upload_pending(&app).await;
        Ok(())
    })
    .await;
    match r {
        Ok(()) => match id {
            Some(id) => crate::net::now(saves_info(app.clone(), id)).await,
            None => Ok(json!({"ok": true})),
        },
        Err(_) => Err(Error::Msg(format!("The website still can't be reached. Your saves are safe on this PC and go up when it's back. See {STATUS_PAGE} for whether it's on our side."))),
    }
}

/// The Saves page: every installed game's copies on this PC, without asking the website.
#[tauri::command]
pub async fn saves_overview(app: AppHandle) -> Res<Value> {
    tokio::task::spawn_blocking(move || overview(&app)).await.map_err(|e| Error::Msg(e.to_string()))
}

fn overview(app: &AppHandle) -> Value {
    let st = app.state::<App>();
    // Copies made here while the player's folder was away go to it now that it's back.
    saves::gather(&st.dir);
    let games: Vec<(String, Installed)> = st.lib.lock().unwrap().installed.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    let signed = signed_in(&st.dir);
    let mut out = Vec::new();
    for (id, g) in games {
        let local = saves::list(&st.dir, &id);
        let state = saves::state(&st.dir, &id);
        let found = spots_of(&st, &id, &g).iter().any(|s| {
            let p = saves::resolve_for(&s.template, &g);
            let shown = if s.template.contains('*') { p.parent().map(Path::to_path_buf).unwrap_or(p.clone()) } else { p };
            shown.exists()
        });
        let size: u64 = local.iter().map(|c| c.size).sum();
        out.push(json!({
            "id": id, "copies": local.len(), "size": size, "newest": local.first(),
            "cloudOn": g.cloud, "synced": state.synced_hash, "pending": state.pending.is_some(), "found": found,
        }));
    }
    let chosen = saves::chosen_root();
    json!({ "signedIn": signed, "games": out, "root": saves::root(&st.dir), "chosen": chosen, "reachable": chosen.as_ref().map(|p| p.is_dir()).unwrap_or(true) })
}

#[tauri::command]
pub async fn saves_backup(app: AppHandle, id: String) -> Res<Option<saves::Copy>> {
    if app.state::<App>().running.lock().unwrap().contains_key(&id) {
        return Err(Error::Msg("Close the game first.".into()));
    }
    after_session(&app, &id).await
}

/// Checks a catalog title's other installed recomps, or copies the one the player just approved.
#[tauri::command]
pub async fn saves_share(app: AppHandle, id: String, selection: Option<Value>) -> Res<Option<Value>> {
    let _busy = busy().await?;
    blocking(move || {
        let st = app.state::<App>();
        let cat = crate::cached_catalog(&st);
        let changed = || Error::Msg("The saves changed or one of the versions is busy. Try again.".into());
        let unavailable = || if selection.is_some() { Err(changed()) } else { Ok(None) };
        let title = |id: &str| catalog::app(&cat, id).and_then(|a| a.get("title")).and_then(Value::as_str).filter(|s| !s.is_empty());
        let Some(group) = title(&id) else { return unavailable() };
        let (target, peers) = {
            let lib = st.lib.lock().unwrap();
            let Some(target) = lib.installed.get(&id).cloned() else { return unavailable() };
            let peers: Vec<_> = lib.installed.values().filter(|g| g.id != id && title(&g.id) == Some(group)).cloned().collect();
            (target, peers)
        };
        let available = |id: &str| !crate::setup_busy(id)
            && !st.jobs.lock().unwrap().contains_key(id) && !st.moving.lock().unwrap().contains(id)
            && !st.running.lock().unwrap().contains_key(id) && !st.pids.lock().unwrap().contains_key(id);
        let offer = || {
            if !available(&id) || crate::STARTING.lock().unwrap().contains(&id) {
                return None;
            }
            let mut candidates: Vec<_> = peers.iter().filter_map(|g| {
                if !available(&g.id) || crate::STARTING.lock().unwrap().contains(&g.id) {
                    return None;
                }
                let spots = spots_of(&st, &g.id, g);
                let [spot] = spots.as_slice() else { return None };
                saves::share_file(spot, g, None).map(|file| (g, file))
            }).collect();
            candidates.sort_unstable_by(|(a, af), (b, bf)| bf.modified.cmp(&af.modified).then_with(|| a.id.cmp(&b.id)));
            let spots = spots_of(&st, &id, &target);
            let [spot] = spots.as_slice() else { return None };
            let (g, source, destination) = candidates.into_iter().find_map(|(g, source)| {
                let destination = saves::share_file(spot, &target, Some(&source))?;
                Some((g, source, destination))
            })?;
            saves::newer_save(&source, &destination).then(|| (g, source, destination, spot.clone()))
        };
        let Some((g, source, destination, spot)) = offer() else {
            return unavailable();
        };
        let token = json!({ "id": g.id, "source": source, "destination": destination });
        if let Some(selection) = selection {
            if token != selection {
                return Err(changed());
            }
            let _reserved = {
                let mut starting = crate::STARTING.lock().unwrap();
                if [&id, &g.id].into_iter().any(|i| starting.contains(i) || !available(i)) {
                    return Err(changed());
                }
                starting.extend([id.clone(), g.id.clone()]);
                (crate::Starting(id.clone()), crate::Starting(g.id.clone()))
            };
            saves::share_save(&st.dir, &id, &spot, &target, &source, &destination)?;
            return Ok(None);
        }
        Ok(Some(json!({
            "selection": token, "name": g.name, "sourceWhen": source.when(), "destinationWhen": destination.when()
        })))
    }).await?
}

/// Puts a copy back: one of this PC's (`name`) or a cloud one (`v`).
#[tauri::command]
pub async fn saves_restore(app: AppHandle, id: String, name: Option<String>, v: Option<String>) -> Res<usize> {
    let st = app.state::<App>();
    if st.running.lock().unwrap().contains_key(&id) {
        return Err(Error::Msg("Close the game first.".into()));
    }
    let _busy = BUSY.lock().await;
    if let Some(v) = v {
        let (list, _) = cloud_list(&app, &id).await?;
        let c = list.into_iter().find(|c| c.v == v).ok_or_else(|| Error::Msg("That cloud save is gone.".into()))?;
        return take_cloud(&app, &id, &c).await;
    }
    let name = name.unwrap_or_default();
    if name.contains('/') || name.contains('\\') || !name.ends_with(".zip") {
        return Err(Error::Msg("That isn't one of its save copies.".into()));
    }
    let g = game(&st, &id)?;
    let spots = spots_of(&st, &id, &g);
    let (dir, id2) = (st.dir.clone(), id.clone());
    let file = saves::dir_of(&st.dir, &id).join(&name);
    blocking(move || saves::restore(&dir, &id2, &file, &spots, &g)).await?
}

/// Forgets where the launcher noticed the game saving (it looks again next time).
#[tauri::command(async)]
pub fn saves_forget_found(st: State<'_, App>, id: String) -> Res<Installed> {
    let mut l = st.lib.lock().unwrap();
    let g = l.installed.get_mut(&id).ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    g.found_saves.clear();
    let g = g.clone();
    let _ = l.save(&st.dir);
    Ok(g)
}

/// The player's own answer to where the game saves (a folder they picked), or None to go back to
/// what the launcher works out.
#[tauri::command(async)]
pub fn saves_set_place(st: State<'_, App>, id: String, path: Option<String>) -> Res<Installed> {
    let mut l = st.lib.lock().unwrap();
    let g = l.installed.get_mut(&id).ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    g.save_place = match path.filter(|p| !p.trim().is_empty()) {
        Some(p) => {
            let p = PathBuf::from(p);
            if !p.is_dir() {
                return Err(Error::Msg("That folder couldn't be found.".into()));
            }
            Some(saves::templatize_for(&p, g))
        }
        None => None,
    };
    g.more_save_places.clear();
    let g = g.clone();
    let _ = l.save(&st.dir);
    Ok(g)
}

/// One more folder where the game saves (a launcher with several games in it, each saving in its
/// own folder): the ones it has now stay, and backups and cloud saves cover all of them.
#[tauri::command(async)]
pub fn saves_add_place(st: State<'_, App>, id: String, path: String) -> Res<Installed> {
    let g = game(&st, &id)?;
    let p = PathBuf::from(&path);
    if !p.is_dir() {
        return Err(Error::Msg("That folder couldn't be found.".into()));
    }
    let now: Vec<String> = spots_of(&st, &id, &g).into_iter().filter(|s| s.from != "game").map(|s| s.template).collect();
    let list = saves::add_place(&now, &p, &g).map_err(Error::Msg)?;
    let mut l = st.lib.lock().unwrap();
    let g = l.installed.get_mut(&id).ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    g.save_place = list.first().cloned();
    g.more_save_places = list.get(1..).map(<[String]>::to_vec).unwrap_or_default();
    let g = g.clone();
    let _ = l.save(&st.dir);
    Ok(g)
}

/// One of the player's save folders taken off the list (the last one: back to the usual place).
#[tauri::command(async)]
pub fn saves_remove_place(st: State<'_, App>, id: String, template: String) -> Res<Installed> {
    let mut l = st.lib.lock().unwrap();
    let g = l.installed.get_mut(&id).ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let mut list: Vec<String> = g.save_place.iter().chain(g.more_save_places.iter()).filter(|t| t.trim() != template.trim()).cloned().collect();
    list.dedup();
    g.save_place = list.first().cloned();
    g.more_save_places = list.get(1..).map(<[String]>::to_vec).unwrap_or_default();
    let g = g.clone();
    let _ = l.save(&st.dir);
    Ok(g)
}

#[tauri::command]
pub async fn saves_open(app: AppHandle, id: String) -> Res<()> {
    let st = app.state::<App>();
    let g = game(&st, &id)?;
    let p = spots_of(&st, &id, &g)
        .iter()
        .map(|s| saves::resolve_for(&s.template, &g))
        .map(|p| if p.is_dir() { p } else { p.parent().map(Path::to_path_buf).unwrap_or(p) })
        .find(|p| p.is_dir())
        .ok_or_else(|| Error::Msg("Its save folder isn't there yet. Play once first.".into()))?;
    crate::open_thing(&app, &p.to_string_lossy(), false)
}

/// Where the launcher keeps this game's save copies (the backups folder the player picked, or the
/// launcher's own saves/<id>): not where the game itself saves.
#[tauri::command]
pub async fn saves_open_backups(app: AppHandle, id: String) -> Res<()> {
    let st = app.state::<App>();
    let d = saves::dir_of(&st.dir, &id);
    std::fs::create_dir_all(&d).map_err(|e| Error::Msg(format!("Couldn't open the backups folder: {e}")))?;
    crate::open_thing(&app, &d.to_string_lossy(), false)
}

// ---------- (0.9.1) games sent from the website, and betas ----------

/// Games sent from recomplauncher.com ("Get it on my PC") that this launcher hasn't handled yet.
#[tauri::command]
pub async fn inbox(app: AppHandle) -> Res<Value> {
    if !signed_in(&app.state::<App>().dir) {
        return Ok(json!({"items": []}));
    }
    let v = get(&app, &[("a", "inbox")]).await?;
    Ok(json!({"items": v.get("items").cloned().unwrap_or(json!([]))}))
}

/// One of them done with: "installing", "installed" (it already was) or "no" (can't run here).
#[tauri::command]
pub async fn inbox_done(app: AppHandle, id: i64, result: String) -> Res<()> {
    post(&app, "inbox-done", json!({"id": id, "result": result}), true).await?;
    Ok(())
}

/// How often a signed-in launcher looks for games sent from the website.
const INBOX_EVERY: u64 = 15 * 60;

/// When the launcher opens, then every fifteen minutes or so while signed in: anything sent from
/// the website goes to the window (which installs it), with a notification when the window is out
/// of sight. Not asked while the site is resting after trouble.
pub fn watch_inbox(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut told: std::collections::HashSet<i64> = Default::default();
        tokio::time::sleep(crate::net::spread(8, 1.0)).await;
        loop {
            if signed_in(&app.state::<App>().dir) && !crate::net::resting(&api(), false) {
                if let Ok(v) = inbox(app.clone()).await {
                    let items: Vec<Value> = v.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
                    let fresh: Vec<Value> = items.into_iter().filter(|i| i.get("id").and_then(Value::as_i64).map(|id| told.insert(id)).unwrap_or(false)).collect();
                    if !fresh.is_empty() {
                        let hidden = app.get_webview_window("main").map(|w| !w.is_visible().unwrap_or(true) || w.is_minimized().unwrap_or(false)).unwrap_or(true);
                        if hidden {
                            for i in &fresh {
                                let name = i.get("name").and_then(Value::as_str).unwrap_or("A game");
                                crate::ingame::notify(&app, &format!("{name} is installing"), "Sent from recomplauncher.com.");
                            }
                        }
                        let _ = app.emit("inbox", json!({"items": fresh}));
                    }
                }
            }
            tokio::time::sleep(crate::net::spread(INBOX_EVERY, 0.2)).await;
        }
    });
}

/// The betas this account can see (open, request, and invite ones it's in), with its status in each.
#[tauri::command]
pub async fn betas(app: AppHandle) -> Res<Value> {
    if !signed_in(&app.state::<App>().dir) {
        return Ok(json!({"betas": []}));
    }
    let v = get(&app, &[("a", "betas")]).await?;
    Ok(json!({"betas": v.get("betas").cloned().unwrap_or(json!([]))}))
}

#[tauri::command]
pub async fn beta_join(app: AppHandle, id: i64) -> Res<Value> {
    post(&app, "beta-join", json!({"id": id}), true).await
}

#[tauri::command]
pub async fn beta_leave(app: AppHandle, id: i64) -> Res<Value> {
    post(&app, "beta-leave", json!({"id": id}), true).await
}

#[cfg(all(test, not(windows)))]
mod account_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn a_copy_that_does_not_get_through_waits_longer_each_time() {
        let (id, name) = ("retry-test", "1700000000.zip");
        assert_eq!(copy_wait_left(id, name), None);
        let waits: Vec<u64> = (0..5).map(|_| copy_failed(id, name, "cut off")).collect();
        assert_eq!(waits, vec![60, 300, 900, 3600, 3600], "a few tries an hour at most");
        assert!(copy_wait_left(id, name).unwrap() > 3500);
        assert!(next_copy_try().unwrap() > 0);
        copy_went_up(id, name);
        assert_eq!(copy_wait_left(id, name), None);
    }

    #[test]
    fn a_full_cloud_stops_uploads_until_there_is_room() {
        const MB: u64 = 1024 * 1024;
        set_full(37 * MB);
        assert!(full_now());
        assert!(is_full(&Error::Msg(FULL_TEXT.into())));
        assert!(is_full(&Error::Msg("Your cloud saves are full (150 MB).".into())));
        // Not enough room for the copy it refused: still full.
        room_now(140 * MB, 150 * MB);
        assert!(full_now());
        room_now(100 * MB, 150 * MB);
        assert!(!full_now());
    }

    struct AccountDir(PathBuf);
    impl AccountDir {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!("rl-account-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for AccountDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn reading_a_plain_token_does_not_rewrite_the_account() {
        let dir = AccountDir::new();
        let saved = br#"{"token":"test-token","user":{"uid":"42","username":"Player","avatar":""}}"#;
        crate::secret::write_private(&file(&dir.0), saved).unwrap();
        assert_eq!(token(&dir.0).as_deref(), Some("test-token"));
        assert!(signed_in(&dir.0));
        assert_eq!(std::fs::read(file(&dir.0)).unwrap(), saved);
    }

    #[test]
    fn a_failed_account_replacement_keeps_the_previous_sign_in() {
        let dir = AccountDir::new();
        let mut account = Account { token: "previous-token".into(), ..Default::default() };
        store(&dir.0, Some(&account));
        // A blocked staging path simulates a failed write without relying on user permissions.
        std::fs::create_dir(dir.0.join("account.json.tmp")).unwrap();
        account.token = "new-token".into();
        store(&dir.0, Some(&account));
        assert_eq!(token(&dir.0).as_deref(), Some("previous-token"));
    }

    #[test]
    fn concurrent_account_refreshes_do_not_lose_the_sign_in() {
        let dir = AccountDir::new();
        let account = Account { token: "test-token".into(), user: User { uid: "42".into(), username: "Player".into(), ..Default::default() }, ..Default::default() };
        store(&dir.0, Some(&account));
        let start = std::sync::Barrier::new(8);
        let missing = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for worker in 0..8 {
                let (path, account, start, missing) = (&dir.0, &account, &start, &missing);
                scope.spawn(move || {
                    start.wait();
                    for turn in 0..100 {
                        if worker % 2 == 0 {
                            let mut refreshed = account.clone();
                            refreshed.user.avatar = "a".repeat(4096 + worker * 31 + turn);
                            store(path, Some(&refreshed));
                        }
                        if token(path).as_deref() != Some("test-token") {
                            missing.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                });
            }
        });
        assert_eq!(missing.load(Ordering::Relaxed), 0, "an account read observed an incomplete refresh");
        assert_eq!(token(&dir.0).as_deref(), Some("test-token"), "the next launch must still be signed in");
    }
}

#[cfg(test)]
mod sync_tests {
    use super::should_restore;

    #[test]
    fn missing_local_saves_restore_even_when_the_cloud_has_not_changed() {
        // The game was reset/reinstalled, but the launcher still remembers its last sync.
        assert!(should_restore("", "last-save", "last-save", true));
    }

    #[test]
    fn automatic_restore_preserves_existing_sync_and_conflict_rules() {
        for (local, synced, cloud, empty, restore) in [
            ("same", "same", "same", false, false),
            ("same", "previous", "same", false, false),
            ("previous", "previous", "new-cloud", false, true),
            ("new-local", "previous", "previous", false, false),
            ("new-local", "previous", "new-cloud", false, false),
            ("", "", "new-cloud", true, true),
            ("", "previous", "new-cloud", true, true),
            // Files whose checksums could not be read are not the same as missing files.
            ("", "previous", "previous", false, false),
        ] {
            assert_eq!(should_restore(local, synced, cloud, empty), restore, "local={local}, synced={synced}, cloud={cloud}, empty={empty}");
        }
    }

    #[tokio::test]
    async fn a_waiting_retry_does_not_keep_the_archive_superseded_by_a_restore() {
        use super::*;
        let dir = std::env::temp_dir().join(format!("rl-save-retry-{}", std::process::id()));
        let app = dir.join("app");
        let inst = dir.join("game");
        std::fs::create_dir_all(inst.join("save")).unwrap();
        let path = inst.join("save/slot.sav");
        let game = Installed { id: "g".into(), path: inst, cloud: Some(true), ..Default::default() };
        let mut library = crate::library::Library::default();
        library.installed.insert("g".into(), game.clone());
        let library = Mutex::new(library);
        let spots = vec![Spot { template: "{install}/save".into(), from: "developer" }];
        std::fs::write(&path, "chosen save").unwrap();
        let chosen = saves::backup(&app, "g", &spots, &game).unwrap().unwrap();
        std::fs::write(&path, "superseded save").unwrap();
        let queued = saves::backup(&app, "g", &spots, &game).unwrap().unwrap();
        saves::set_state(&app, "g", &saves::State { pending: Some(queued.name.clone()), ..Default::default() });

        // Restore owns BUSY. Poll the retry once so it really waits before completing restore.
        let restoring = BUSY.lock().await;
        let retry = pending_copy(&app, "g", &library);
        tokio::pin!(retry);
        assert!(futures_util::poll!(&mut retry).is_pending());
        saves::restore(&app, "g", &saves::dir_of(&app, "g").join(&chosen.name), &spots, &game).unwrap();
        drop(restoring);

        let (_upload, pending) = retry.await;
        assert!(pending.is_none(), "read the queue after restore completes, not before waiting for it");
        assert_eq!(std::fs::read_to_string(path).unwrap(), "chosen save");
        assert!(saves::dir_of(&app, "g").join(queued.name).is_file());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn a_waiting_retry_respects_cloud_saves_being_turned_off() {
        use super::*;
        let dir = std::env::temp_dir().join(format!("rl-save-retry-disabled-{}", std::process::id()));
        let mut library = crate::library::Library::default();
        library.installed.insert("g".into(), Installed { cloud: Some(true), ..Default::default() });
        let library = Mutex::new(library);
        saves::set_state(&dir, "g", &saves::State { pending: Some("123.zip".into()), ..Default::default() });
        let busy = BUSY.lock().await;
        let retry = pending_copy(&dir, "g", &library);
        tokio::pin!(retry);
        assert!(futures_util::poll!(&mut retry).is_pending());
        library.lock().unwrap().installed.get_mut("g").unwrap().cloud = Some(false);
        drop(busy);
        let (upload, pending) = retry.await;
        assert!(pending.is_none(), "a disabled game must not upload after waiting for another save operation");
        assert_eq!(saves::state(&dir, "g").pending.as_deref(), Some("123.zip"), "keep the copy queued if cloud saves are enabled later");
        drop(upload);
        library.lock().unwrap().installed.get_mut("g").unwrap().cloud = Some(true);
        let (_upload, pending) = pending_copy(&dir, "g", &library).await;
        assert_eq!(pending.as_deref(), Some("123.zip"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn uploading_during_play_cannot_send_the_pre_restore_archive() {
        use super::*;
        let dir = std::env::temp_dir().join(format!("rl-save-upload-running-{}", std::process::id()));
        let app = dir.join("app");
        let inst = dir.join("game");
        std::fs::create_dir_all(inst.join("save")).unwrap();
        let path = inst.join("save/slot.sav");
        let game = Installed { id: "g".into(), path: inst, ..Default::default() };
        let spots = vec![Spot { template: "{install}/save".into(), from: "developer" }];
        std::fs::write(&path, "chosen save").unwrap();
        let chosen = saves::backup(&app, "g", &spots, &game).unwrap().unwrap();
        std::fs::write(&path, "superseded save").unwrap();
        let previous = saves::backup(&app, "g", &spots, &game).unwrap().unwrap();
        assert_eq!(upload_candidate(&app, "g", true).unwrap().unwrap().hash, previous.hash);
        saves::restore(&app, "g", &saves::dir_of(&app, "g").join(&chosen.name), &spots, &game).unwrap();
        assert!(matches!(upload_candidate(&app, "g", true), Err(Error::Msg(_))), "ask the player to close the game rather than upload the superseded save");
        // Closing the game makes a current backup; Upload Now can then send the chosen save.
        saves::backup(&app, "g", &spots, &game).unwrap().unwrap();
        assert_eq!(upload_candidate(&app, "g", false).unwrap().unwrap().hash, chosen.hash);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

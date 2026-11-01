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

/// account.json in the app's data folder. The token is kept sealed (secret.rs): `sealed` holds it
/// and `token` stays empty. A file from before that still has it plain, and is sealed on first read.
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

fn load(dir: &Path) -> Option<Account> {
    let mut a = std::fs::read(file(dir)).ok().and_then(|b| serde_json::from_slice::<Account>(&b).ok())?;
    if !a.sealed.is_empty() {
        a.token = crate::secret::open(&a.sealed)?;
    } else if !a.token.is_empty() {
        // Kept plain by an older launcher: sealed now.
        store(dir, Some(&a));
    }
    Some(a).filter(|a| !a.token.is_empty())
}

/// This PC's sign-in token, when signed in.
pub fn token(dir: &Path) -> Option<String> {
    load(dir).map(|a| a.token)
}

fn store(dir: &Path, a: Option<&Account>) {
    match a {
        Some(a) => {
            let mut keep = a.clone();
            if let Some(s) = crate::secret::seal(&a.token) {
                keep.sealed = s;
                keep.token = String::new();
            } else {
                keep.sealed = String::new();
            }
            let _ = crate::secret::write_private(&file(dir), &serde_json::to_vec_pretty(&keep).unwrap_or_default());
        }
        None => {
            let _ = std::fs::remove_file(file(dir));
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

async fn answer(app: &AppHandle, r: reqwest::Response) -> Res<Value> {
    let code = r.status().as_u16();
    let v: Value = r.json().await.unwrap_or(Value::Null);
    if code == 401 {
        forget(app);
        return Err(Error::Msg("You were signed out. Sign in again to use cloud saves.".into()));
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
    let r = st.http.get(api()).query(q).bearer_auth(&a.token).header("X-RL-Token", a.token.as_str()).timeout(std::time::Duration::from_secs(15)).send().await.map_err(net)?;
    answer(app, r).await
}

async fn post(app: &AppHandle, a_: &str, body: Value, token: bool) -> Res<Value> {
    let st = app.state::<App>();
    let mut req = st.http.post(api()).query(&[("a", a_)]).json(&body).timeout(std::time::Duration::from_secs(20));
    if token {
        let a = load(&st.dir).ok_or_else(|| Error::Msg("Sign in first.".into()))?;
        req = req.bearer_auth(&a.token).header("X-RL-Token", a.token.as_str());
    }
    answer(app, req.send().await.map_err(net)?).await
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
    // A slow or briefly blocked answer gets one more try before the account shows as offline.
    let mut r = get(&app, &[("a", "me")]).await;
    if matches!(r, Err(Error::Net(_)) | Err(Error::Msg(_))) && signed_in(&st.dir) {
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        r = get(&app, &[("a", "me")]).await;
    }
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
    st.lib.lock().unwrap().installed.get(id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))
}

/// Where this game's saves are, best first.
pub fn spots_of(st: &App, id: &str, g: &Installed) -> Vec<Spot> {
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
    Ok((list, json!({"used": v.get("used"), "quota": v.get("quota")})))
}

/// Puts one local copy up.
async fn upload(app: &AppHandle, id: &str, name: &str) -> Res<()> {
    let st = app.state::<App>();
    let path = saves::dir_of(&st.dir, id).join(name);
    let man = saves::manifest(&path).ok_or_else(|| Error::Msg("That copy is gone.".into()))?;
    let body = tokio::fs::read(&path).await?;
    let a = load(&st.dir).ok_or_else(|| Error::Msg("Sign in first.".into()))?;
    let (made, played) = (man.made.to_string(), man.played.to_string());
    let r = st
        .http
        .put(api())
        .query(&[("a", "save-put"), ("game", id), ("hash", &man.hash), ("made", &made), ("played", &played), ("device", &man.device)])
        .bearer_auth(&a.token).header("X-RL-Token", a.token.as_str())
        .header("Content-Type", "application/zip")
        .body(body)
        .timeout(std::time::Duration::from_secs(300))
        .send()
        .await
        .map_err(net)?;
    answer(app, r).await?;
    let mut s = saves::state(&st.dir, id);
    s.synced_hash = man.hash;
    if s.pending.as_deref() == Some(name) {
        s.pending = None;
    }
    saves::set_state(&st.dir, id, &s);
    Ok(())
}

/// Copies that didn't get up before (offline, or not signed in yet).
pub async fn upload_pending(app: &AppHandle) {
    let st = app.state::<App>();
    if !signed_in(&st.dir) {
        return;
    }
    let ids: Vec<String> = st.lib.lock().unwrap().installed.values().filter(|g| g.cloud == Some(true)).map(|g| g.id.clone()).collect();
    for id in ids {
        let s = saves::state(&st.dir, &id);
        if let Some(name) = s.pending {
            let _ = upload(app, &id, &name).await;
        }
    }
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
                let mut s = saves::state(&st.dir, id);
                s.pending = Some(l.name.clone());
                saves::set_state(&st.dir, id, &s);
                let _ = app.emit("toast", format!("Your save for {} is backed up on this PC. It goes to the cloud next time: {e}", g.name));
            }
        }
    }
    Ok(copy)
}

/// Gets a cloud copy and puts it in place (what's there now is kept as a local copy first).
async fn take_cloud(app: &AppHandle, id: &str, c: &CloudCopy) -> Res<usize> {
    let st = app.state::<App>();
    let a = load(&st.dir).ok_or_else(|| Error::Msg("Sign in first.".into()))?;
    let r = st
        .http
        .get(api())
        .query(&[("a", "save-get"), ("game", id), ("v", &c.v)])
        .bearer_auth(&a.token).header("X-RL-Token", a.token.as_str())
        .timeout(std::time::Duration::from_secs(300))
        .send()
        .await
        .map_err(net)?;
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

/// Before Play: what to do about the cloud.
/// "play": go ahead. "restored": a newer save from another PC was put in place. "conflict": both
/// changed, the player picks (`local` and `cloud` say what each is). "offline": couldn't check.
#[tauri::command]
pub async fn saves_check(app: AppHandle, id: String) -> Res<Value> {
    let st = app.state::<App>();
    let g = game(&st, &id)?;
    if g.cloud != Some(true) || !signed_in(&st.dir) {
        return Ok(json!({"action": "play"}));
    }
    let _busy = BUSY.lock().await;
    let (list, _) = match cloud_list(&app, &id).await {
        Ok(x) => x,
        Err(Error::Net(_)) => return Ok(json!({"action": "offline"})),
        Err(e) => return Err(e),
    };
    let Some(top) = list.first().cloned() else { return Ok(json!({"action": "play"})) };
    let spots = spots_of(&st, &id, &g);
    let g2 = g.clone();
    let (entries, _, local) = blocking(move || saves::current(&spots, &g2)).await?;
    let s = saves::state(&st.dir, &id);
    if top.hash == local {
        if s.synced_hash != local {
            let mut s = s;
            s.synced_hash = local;
            saves::set_state(&st.dir, &id, &s);
        }
        return Ok(json!({"action": "play"}));
    }
    let local_changed = !local.is_empty() && local != s.synced_hash;
    let cloud_changed = top.hash != s.synced_hash;
    if !cloud_changed {
        // Only this PC has played since: it goes up after this session.
        return Ok(json!({"action": "play"}));
    }
    if !local_changed || entries.is_empty() {
        let n = take_cloud(&app, &id, &top).await?;
        return Ok(json!({"action": "restored", "files": n, "cloud": top}));
    }
    let newest = entries.iter().filter_map(|e| std::fs::metadata(&e.abs).ok()?.modified().ok()).max();
    let when = newest.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    Ok(json!({"action": "conflict", "local": {"when": when, "device": saves::device_name(), "played": g.played_secs}, "cloud": top}))
}

/// Before a start with no window to ask in (a shortcut, the tray): a newer cloud save is put in
/// place; if both changed, this PC's is played and goes up afterwards as the newest (the cloud keeps
/// the other one in its history).
pub async fn sync_quiet(app: &AppHandle, id: &str) {
    let _ = saves_check(app.clone(), id.to_string()).await;
}

/// The player's pick after a conflict: keep this PC's save (it goes up as the newest), or the
/// cloud's (this PC's is kept as a local copy first).
#[tauri::command]
pub async fn saves_pick(app: AppHandle, id: String, keep: String) -> Res<()> {
    let st = app.state::<App>();
    if keep == "cloud" {
        let _busy = BUSY.lock().await;
        let (list, _) = cloud_list(&app, &id).await?;
        let top = list.first().cloned().ok_or_else(|| Error::Msg("There's no cloud save for it.".into()))?;
        take_cloud(&app, &id, &top).await?;
        return Ok(());
    }
    // This PC's save: a fresh copy, sent up as the newest.
    after_session(&app, &id).await?;
    let latest = saves::list(&st.dir, &id).into_iter().next().ok_or_else(|| Error::Msg("There's no save on this PC.".into()))?;
    let _busy = BUSY.lock().await;
    upload(&app, &id, &latest.name).await
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
                let st = app2.state::<App>();
                if let Some(l) = saves::list(&st.dir, &id2).into_iter().next() {
                    let _busy = BUSY.lock().await;
                    let _ = upload(&app2, &id2, &l.name).await;
                }
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
            let p = saves::resolve(&s.template, &g.path);
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
    Ok(json!({
        "spots": spots, "local": local, "cloud": cloud, "space": space, "offline": offline,
        "cloudOn": g.cloud, "signedIn": signed_in(&st.dir), "synced": state.synced_hash, "pending": state.pending.is_some(),
    }))
}

/// The Saves page: every installed game's copies on this PC, without asking the website.
#[tauri::command]
pub async fn saves_overview(app: AppHandle) -> Res<Value> {
    tokio::task::spawn_blocking(move || overview(&app)).await.map_err(|e| Error::Msg(e.to_string()))
}

fn overview(app: &AppHandle) -> Value {
    let st = app.state::<App>();
    let games: Vec<(String, Installed)> = st.lib.lock().unwrap().installed.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    let signed = signed_in(&st.dir);
    let mut out = Vec::new();
    for (id, g) in games {
        let local = saves::list(&st.dir, &id);
        let state = saves::state(&st.dir, &id);
        let found = spots_of(&st, &id, &g).iter().any(|s| {
            let p = saves::resolve(&s.template, &g.path);
            let shown = if s.template.contains('*') { p.parent().map(Path::to_path_buf).unwrap_or(p.clone()) } else { p };
            shown.exists()
        });
        let size: u64 = local.iter().map(|c| c.size).sum();
        out.push(json!({
            "id": id, "copies": local.len(), "size": size, "newest": local.first(),
            "cloudOn": g.cloud, "synced": state.synced_hash, "pending": state.pending.is_some(), "found": found,
        }));
    }
    json!({ "signedIn": signed, "games": out })
}

#[tauri::command]
pub async fn saves_backup(app: AppHandle, id: String) -> Res<Option<saves::Copy>> {
    if app.state::<App>().running.lock().unwrap().contains_key(&id) {
        return Err(Error::Msg("Close the game first.".into()));
    }
    after_session(&app, &id).await
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
            Some(saves::templatize(&p, &g.path))
        }
        None => None,
    };
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
        .map(|s| saves::resolve(&s.template, &g.path))
        .map(|p| if p.is_dir() { p } else { p.parent().map(Path::to_path_buf).unwrap_or(p) })
        .find(|p| p.is_dir())
        .ok_or_else(|| Error::Msg("Its save folder isn't there yet. Play once first.".into()))?;
    crate::open_thing(&app, &p.to_string_lossy(), false)
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

/// Every minute and a half while signed in: anything sent from the website goes to the window
/// (which installs it), with a notification when the window is out of sight.
pub fn watch_inbox(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut told: std::collections::HashSet<i64> = Default::default();
        tokio::time::sleep(std::time::Duration::from_secs(8)).await;
        loop {
            if signed_in(&app.state::<App>().dir) {
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
            tokio::time::sleep(std::time::Duration::from_secs(90)).await;
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

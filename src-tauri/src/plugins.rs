//! Plugins: add-ons for the launcher, installed from a .rlplugin file (a zip with a plugin.json).
//!
//! A plugin's page runs walled off: in a sandboxed frame with its own (empty) origin, served from
//! rlplug:// with a policy that lets it load only its own files and connect nowhere. It can't call
//! the launcher's commands (those need a key only the launcher's own page has). Everything it does
//! goes through the launcher's page as a request, which checks the plugin's permissions and asks
//! the player before anything is downloaded or installed, and the first time it reaches each site.

use crate::library::{now, Installed};
use crate::{App, Error, Res};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};

/// What a plugin may ask for. Keeping its own data is always allowed.
pub const PERMISSIONS: &[&str] = &["library.read", "library.add", "network", "install", "notify"];
const MAX_UNPACKED: u64 = 64 << 20;
const MAX_FILES: usize = 4000;
const MAX_STORE: usize = 1 << 20;
const MAX_FETCH: usize = 32 << 20;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Page {
    #[serde(default)]
    pub title: String,
    #[serde(default = "index")]
    pub entry: String,
    /// An icon for the sidebar (an .svg or .png in the plugin).
    #[serde(default)]
    pub icon: String,
}
fn index() -> String {
    "index.html".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MenuItem {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub homepage: String,
    #[serde(default)]
    pub page: Option<Page>,
    /// Items in a game's right-click menu. Picking one opens the plugin's page with it.
    #[serde(default)]
    pub menu: Vec<MenuItem>,
    #[serde(default)]
    pub permissions: Vec<String>,
    /// The sites it connects to (with "network" or "install"): host names, `*.example.com` for
    /// all of a site's subdomains.
    #[serde(default)]
    pub sites: Vec<String>,
    #[serde(default)]
    pub min_launcher: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Entry {
    #[serde(default)]
    enabled: bool,
    /// Sites the player said to always allow.
    #[serde(default)]
    sites: Vec<String>,
    #[serde(default)]
    installed_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Saved {
    /// Every plugin off at once (the kill switch).
    #[serde(default)]
    all_off: bool,
    #[serde(default)]
    plugins: BTreeMap<String, Entry>,
}

/// Sites allowed once, until the launcher closes: (plugin, site).
static ONCE: Mutex<Option<HashSet<(String, String)>>> = Mutex::new(None);

fn root(dir: &Path) -> PathBuf {
    dir.join("plugins")
}
fn saved_file(dir: &Path) -> PathBuf {
    dir.join("plugins.json")
}
fn data_file(dir: &Path, id: &str) -> PathBuf {
    dir.join("plugin-data").join(format!("{id}.json"))
}
fn load(dir: &Path) -> Saved {
    std::fs::read(saved_file(dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}
fn store(dir: &Path, s: &Saved) {
    let _ = std::fs::write(saved_file(dir), serde_json::to_vec_pretty(s).unwrap_or_default());
}

/// An installed plugin's manifest.
pub fn manifest(dir: &Path, id: &str) -> Option<Manifest> {
    if !good_id(id) {
        return None;
    }
    std::fs::read(root(dir).join(id).join("plugin.json")).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

fn good_id(id: &str) -> bool {
    let b = id.as_bytes();
    (3..=40).contains(&b.len()) && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-') && b[0] != b'-' && b[b.len() - 1] != b'-'
}

fn good_site(s: &str) -> bool {
    let h = s.strip_prefix("*.").unwrap_or(s);
    h.contains('.') && h.len() <= 253 && h.split('.').all(|l| !l.is_empty() && l.len() <= 63 && l.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-'))
}

/// A path inside the plugin, never out of it.
fn good_rel(p: &str) -> bool {
    !p.is_empty() && !p.starts_with('/') && !p.contains('\\') && !p.contains(':') && p.split('/').all(|s| !s.is_empty() && s != "." && s != "..")
}

/// Checks a manifest: what the launcher shows and enforces has to make sense.
pub fn check(m: &mut Manifest) -> Result<(), String> {
    if !good_id(&m.id) {
        return Err("Its id has to be 3 to 40 lowercase letters, numbers and dashes.".into());
    }
    m.name = m.name.trim().chars().take(40).collect();
    m.version = m.version.trim().chars().take(24).collect();
    m.author = m.author.trim().chars().take(60).collect();
    m.description = m.description.trim().chars().take(400).collect();
    if m.name.is_empty() || m.version.is_empty() {
        return Err("It needs a name and a version.".into());
    }
    if let Some(bad) = m.permissions.iter().find(|p| !PERMISSIONS.contains(&p.as_str())) {
        return Err(format!("It asks for a permission the launcher doesn't have: {bad}."));
    }
    m.permissions.sort();
    m.permissions.dedup();
    for s in m.sites.iter_mut() {
        *s = s.trim().to_lowercase();
    }
    m.sites.sort();
    m.sites.dedup();
    if let Some(bad) = m.sites.iter().find(|s| !good_site(s)) {
        return Err(format!("\"{bad}\" isn't a site name. List host names only, like example.com."));
    }
    let net = m.permissions.iter().any(|p| p == "network" || p == "install");
    if net && m.sites.is_empty() {
        return Err("It wants to connect to the internet but doesn't list the sites.".into());
    }
    if !net {
        m.sites.clear();
    }
    for f in [Some(&m.icon), m.page.as_ref().map(|p| &p.entry), m.page.as_ref().map(|p| &p.icon)].into_iter().flatten() {
        if !f.is_empty() && !good_rel(f) {
            return Err(format!("\"{f}\" isn't a file inside the plugin."));
        }
    }
    m.menu.retain(|i| !i.id.is_empty() && !i.label.trim().is_empty());
    m.menu.truncate(8);
    for i in m.menu.iter_mut() {
        i.label = i.label.trim().chars().take(40).collect();
    }
    if !m.menu.is_empty() && m.page.is_none() {
        return Err("Its menu items need a page to open.".into());
    }
    Ok(())
}

/// Reads a .rlplugin file: its manifest and where its files start inside it.
fn open_package(file: &Path) -> Result<(zip::ZipArchive<std::fs::File>, Manifest, String), String> {
    let f = std::fs::File::open(file).map_err(|e| format!("Couldn't open it: {e}"))?;
    let mut z = zip::ZipArchive::new(f).map_err(|_| "That isn't a plugin file (it isn't a zip).".to_string())?;
    if z.len() > MAX_FILES {
        return Err("It has too many files.".into());
    }
    let mut total = 0u64;
    let mut base: Option<String> = None;
    for i in 0..z.len() {
        let e = z.by_index_raw(i).map_err(|e| e.to_string())?;
        total += e.size();
        if e.enclosed_name().is_none() {
            return Err("It has a file that points outside its folder.".into());
        }
        let n = e.name().to_string();
        if n.rsplit('/').next() == Some("plugin.json") {
            let b = n[..n.len() - "plugin.json".len()].to_string();
            if base.as_ref().map(|x| b.len() < x.len()).unwrap_or(true) {
                base = Some(b);
            }
        }
    }
    if total > MAX_UNPACKED {
        return Err("It's too big for a plugin (64 MB at most).".into());
    }
    let base = base.ok_or("There's no plugin.json in it.")?;
    let mut m: Manifest = {
        let mut e = z.by_name(&format!("{base}plugin.json")).map_err(|e| e.to_string())?;
        let mut s = String::new();
        std::io::Read::read_to_string(&mut e, &mut s).map_err(|_| "Its plugin.json isn't text.".to_string())?;
        serde_json::from_str(&s).map_err(|e| format!("Its plugin.json can't be read: {e}"))?
    };
    check(&mut m)?;
    Ok((z, m, base))
}

/// What a plugin file is and asks for, shown before it's installed.
#[tauri::command(async)]
pub fn plugin_inspect(st: State<'_, App>, path: String) -> Res<Value> {
    let (_, m, _) = open_package(Path::new(&path)).map_err(Error::Msg)?;
    let had = manifest(&st.dir, &m.id).map(|o| o.version);
    Ok(json!({"manifest": m, "installed": had}))
}

/// Installs (or updates) a plugin from its file. It starts off: the player turns it on.
#[tauri::command(async)]
pub fn plugin_install(st: State<'_, App>, path: String) -> Res<Manifest> {
    let (mut z, m, base) = open_package(Path::new(&path)).map_err(Error::Msg)?;
    let plugins = root(&st.dir);
    let target = plugins.join(&m.id);
    let staging = plugins.join(format!(".{}.new", m.id));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    for i in 0..z.len() {
        let mut e = z.by_index(i).map_err(|e| Error::Msg(e.to_string()))?;
        let Some(rel) = e.enclosed_name() else { continue };
        let Ok(rel) = rel.strip_prefix(&base) else { continue };
        if rel.as_os_str().is_empty() {
            continue;
        }
        let out = staging.join(rel);
        if e.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p)?;
        }
        let mut f = std::fs::File::create(&out)?;
        std::io::copy(&mut (&mut e).take(MAX_UNPACKED), &mut f)?;
    }
    // Its manifest as checked.
    std::fs::write(staging.join("plugin.json"), serde_json::to_vec_pretty(&m).unwrap_or_default())?;
    let _ = std::fs::remove_dir_all(&target);
    std::fs::rename(&staging, &target)?;
    let mut s = load(&st.dir);
    let e = s.plugins.entry(m.id.clone()).or_default();
    if e.installed_at == 0 {
        e.installed_at = now();
    }
    // Sites it no longer lists aren't allowed anymore.
    e.sites.retain(|x| m.sites.iter().any(|y| site_matches(y, x)));
    store(&st.dir, &s);
    Ok(m)
}


#[tauri::command(async)]
pub fn plugin_list(st: State<'_, App>) -> Res<Value> {
    let s = load(&st.dir);
    let mut out = vec![];
    for (id, e) in &s.plugins {
        if let Some(m) = manifest(&st.dir, id) {
            out.push(json!({"manifest": m, "enabled": e.enabled, "sites": e.sites}));
        }
    }
    Ok(json!({"all_off": s.all_off, "plugins": out}))
}

#[tauri::command(async)]
pub fn plugin_enable(st: State<'_, App>, id: String, on: bool) -> Res<()> {
    let mut s = load(&st.dir);
    let e = s.plugins.get_mut(&id).ok_or_else(|| Error::Msg("That plugin isn't installed.".into()))?;
    e.enabled = on;
    store(&st.dir, &s);
    Ok(())
}

#[tauri::command(async)]
pub fn plugin_all_off(st: State<'_, App>, off: bool) -> Res<()> {
    let mut s = load(&st.dir);
    s.all_off = off;
    store(&st.dir, &s);
    Ok(())
}

/// Takes a plugin away with its files, its data and the sites it was allowed.
#[tauri::command(async)]
pub fn plugin_uninstall(st: State<'_, App>, id: String) -> Res<()> {
    if !good_id(&id) {
        return Err(Error::Msg("That plugin isn't installed.".into()));
    }
    let mut s = load(&st.dir);
    s.plugins.remove(&id);
    store(&st.dir, &s);
    let _ = std::fs::remove_dir_all(root(&st.dir).join(&id));
    let _ = std::fs::remove_file(data_file(&st.dir, &id));
    if let Some(o) = ONCE.lock().unwrap().as_mut() {
        o.retain(|(p, _)| p != &id);
    }
    Ok(())
}

/// The plugin, when it's on and may do `perm` ("" for anything every plugin may do).
fn allowed(dir: &Path, id: &str, perm: &str) -> Res<Manifest> {
    let s = load(dir);
    let on = !s.all_off && s.plugins.get(id).map(|e| e.enabled).unwrap_or(false);
    let m = manifest(dir, id).filter(|_| on).ok_or_else(|| Error::Msg("That plugin is off.".into()))?;
    if !perm.is_empty() && !m.permissions.iter().any(|p| p == perm) {
        return Err(Error::Msg(format!("{} didn't ask for permission to do that.", m.name)));
    }
    Ok(m)
}

fn site_matches(listed: &str, host: &str) -> bool {
    match listed.strip_prefix("*.") {
        Some(base) => host == base || host.ends_with(&format!(".{base}")),
        None => host == listed,
    }
}

/// A plugin's address checked: https, to a site it lists, that the player allowed. Err
/// `ASK_SITE|host` when the player hasn't been asked about that site yet.
fn check_url(dir: &Path, m: &Manifest, url: &str) -> Res<reqwest::Url> {
    let u = reqwest::Url::parse(url).map_err(|_| Error::Msg(format!("{} asked for an address that isn't one.", m.name)))?;
    let host = u.host_str().unwrap_or("").to_lowercase();
    if u.scheme() != "https" || !u.username().is_empty() || u.password().is_some() || u.port().is_some() {
        return Err(Error::Msg(format!("{} can only connect to sites over https.", m.name)));
    }
    if !m.sites.iter().any(|s| site_matches(s, &host)) {
        return Err(Error::Msg(format!("{} didn't list {host} as a site it connects to.", m.name)));
    }
    let always = load(dir).plugins.get(&m.id).map(|e| e.sites.iter().any(|s| s == &host)).unwrap_or(false);
    let once = ONCE.lock().unwrap().as_ref().map(|o| o.contains(&(m.id.clone(), host.clone()))).unwrap_or(false);
    if !always && !once {
        return Err(Error::Msg(format!("ASK_SITE|{host}")));
    }
    Ok(u)
}

/// The player's answer about a site: "once" (until the launcher closes) or "always".
#[tauri::command(async)]
pub fn plugin_allow_site(st: State<'_, App>, id: String, host: String, always: bool) -> Res<()> {
    let m = allowed(&st.dir, &id, "")?;
    let host = host.to_lowercase();
    if !m.sites.iter().any(|s| site_matches(s, &host)) {
        return Err(Error::Msg("It doesn't list that site.".into()));
    }
    if always {
        let mut s = load(&st.dir);
        if let Some(e) = s.plugins.get_mut(&id) {
            if !e.sites.contains(&host) {
                e.sites.push(host);
            }
        }
        store(&st.dir, &s);
    } else {
        ONCE.lock().unwrap().get_or_insert_with(HashSet::new).insert((id, host));
    }
    Ok(())
}

/// Stops always allowing a site.
#[tauri::command(async)]
pub fn plugin_forget_site(st: State<'_, App>, id: String, host: String) -> Res<()> {
    let mut s = load(&st.dir);
    if let Some(e) = s.plugins.get_mut(&id) {
        e.sites.retain(|x| x != &host);
    }
    store(&st.dir, &s);
    if let Some(o) = ONCE.lock().unwrap().as_mut() {
        o.remove(&(id, host));
    }
    Ok(())
}

/// A client for a plugin's requests: the headers it set on every request, and no following of
/// redirects by itself: `send` follows them, step by step, only to sites the plugin lists. The
/// launcher's own client and sign-in are never used.
fn client_for(headers: &BTreeMap<String, String>) -> Res<reqwest::Client> {
    use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
    let mut h = HeaderMap::new();
    for (k, v) in headers {
        // Where a request comes from is the launcher's to say. A plugin's own cookies (a sign-in to
        // a site it lists) go along; the launcher has none of its own here.
        if ["host", "origin", "referer", "content-length"].contains(&k.to_lowercase().as_str()) {
            continue;
        }
        if let (Ok(n), Ok(v)) = (HeaderName::from_bytes(k.as_bytes()), HeaderValue::from_str(v)) {
            h.insert(n, v);
        }
    }
    reqwest::Client::builder()
        .user_agent(concat!("RecompLauncher/", env!("CARGO_PKG_VERSION")))
        .default_headers(h)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| Error::Msg(e.to_string()))
}

/// Where a redirect may go: https, a site the plugin lists, and one the player allowed, or a
/// subdomain of one they allowed (a site's own download servers).
fn check_redirect(dir: &Path, m: &Manifest, u: reqwest::Url) -> Res<reqwest::Url> {
    let host = u.host_str().unwrap_or("").to_lowercase();
    if u.scheme() != "https" || !m.sites.iter().any(|s| site_matches(s, &host)) {
        return Err(Error::Msg(format!("The site sent {} somewhere it didn't list ({host}), so the launcher stopped there.", m.name)));
    }
    let allowed_hosts: Vec<String> = load(dir).plugins.get(&m.id).map(|e| e.sites.clone()).unwrap_or_default().into_iter().chain(ONCE.lock().unwrap().as_ref().map(|o| o.iter().filter(|(p, _)| p == &m.id).map(|(_, h)| h.clone()).collect::<Vec<_>>()).unwrap_or_default()).collect();
    if allowed_hosts.iter().any(|a| &host == a || host.ends_with(&format!(".{a}"))) {
        return Ok(u);
    }
    Err(Error::Msg(format!("ASK_SITE|{host}")))
}

/// Sends a plugin's request, following redirects by hand (up to 10) so each step is checked.
/// The answer and the address it came from.
async fn send(dir: &Path, m: &Manifest, client: &reqwest::Client, mut method: reqwest::Method, mut u: reqwest::Url, mut body: Option<String>) -> Res<(reqwest::Response, reqwest::Url)> {
    for _ in 0..10 {
        let mut rq = client.request(method.clone(), u.clone()).timeout(std::time::Duration::from_secs(60));
        if let Some(b) = &body {
            rq = rq.body(b.clone());
        }
        let r = rq.send().await.map_err(|e| Error::Msg(format!("Couldn't reach the site: {e}")))?;
        if !r.status().is_redirection() {
            return Ok((r, u));
        }
        let Some(next) = r.headers().get(reqwest::header::LOCATION).and_then(|v| v.to_str().ok()).and_then(|l| u.join(l).ok()) else { return Ok((r, u)) };
        u = check_redirect(dir, m, next)?;
        if matches!(r.status().as_u16(), 301..=303) {
            method = reqwest::Method::GET;
            body = None;
        }
    }
    Err(Error::Msg("The site kept sending the request elsewhere.".into()))
}

/// A request a plugin makes to a site it lists (the launcher makes it for the plugin). The answer
/// as text, or as base64 when `binary`.
#[tauri::command]
pub async fn plugin_fetch(st: State<'_, App>, id: String, url: String, method: Option<String>, headers: Option<BTreeMap<String, String>>, body: Option<String>, binary: Option<bool>) -> Res<Value> {
    let m = allowed(&st.dir, &id, "network")?;
    let u = check_url(&st.dir, &m, &url)?;
    let method = match method.as_deref().unwrap_or("GET").to_uppercase().as_str() {
        "GET" => reqwest::Method::GET,
        "POST" => reqwest::Method::POST,
        "PUT" => reqwest::Method::PUT,
        "DELETE" => reqwest::Method::DELETE,
        "HEAD" => reqwest::Method::HEAD,
        _ => return Err(Error::Msg("That kind of request isn't allowed.".into())),
    };
    let client = client_for(&headers.unwrap_or_default())?;
    let (r, _) = send(&st.dir, &m, &client, method, u, body).await?;
    let status = r.status().as_u16();
    let hs: BTreeMap<String, String> = r.headers().iter().filter_map(|(k, v)| v.to_str().ok().map(|v| (k.as_str().to_string(), v.to_string()))).collect();
    let bytes = r.bytes().await.map_err(|e| Error::Msg(e.to_string()))?;
    if bytes.len() > MAX_FETCH {
        return Err(Error::Msg("The answer was too big (32 MB at most).".into()));
    }
    let body = if binary.unwrap_or(false) {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    } else {
        String::from_utf8_lossy(&bytes).to_string()
    };
    Ok(json!({"status": status, "headers": hs, "body": body}))
}

/// What a plugin keeps for itself (up to 1 MB).
#[tauri::command(async)]
pub fn plugin_store_get(st: State<'_, App>, id: String) -> Res<Value> {
    allowed(&st.dir, &id, "")?;
    Ok(std::fs::read(data_file(&st.dir, &id)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_else(|| json!({})))
}

#[tauri::command(async)]
pub fn plugin_store_set(st: State<'_, App>, id: String, key: String, value: Value) -> Res<()> {
    allowed(&st.dir, &id, "")?;
    let f = data_file(&st.dir, &id);
    let mut all: serde_json::Map<String, Value> = std::fs::read(&f).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    if value.is_null() {
        all.remove(&key);
    } else {
        all.insert(key.chars().take(200).collect(), value);
    }
    let b = serde_json::to_vec(&all).unwrap_or_default();
    if b.len() > MAX_STORE {
        return Err(Error::Msg("A plugin can keep 1 MB at most.".into()));
    }
    std::fs::create_dir_all(f.parent().unwrap())?;
    std::fs::write(f, b)?;
    Ok(())
}

/// The Library as a plugin sees it (with "library.read").
#[tauri::command(async)]
pub fn plugin_library(st: State<'_, App>, id: String) -> Res<Value> {
    allowed(&st.dir, &id, "library.read")?;
    let l = st.lib.lock().unwrap();
    let games: Vec<Value> = l
        .installed
        .values()
        .map(|g| json!({"id": g.id, "name": g.name, "version": g.version, "folder": g.path, "repo": g.repo, "played_secs": g.played_secs, "last_played": g.last_played}))
        .collect();
    Ok(json!(games))
}

/// A folder the player confirmed, added to the Library for a plugin (with "library.add").
#[tauri::command(async)]
pub fn plugin_add_folder(st: State<'_, App>, id: String, folder: String, name: Option<String>) -> Res<Installed> {
    allowed(&st.dir, &id, "library.add")?;
    let mut g = crate::devtools::add_folder(&st, folder, None, "own")?;
    if let Some(n) = name.map(|n| n.trim().chars().take(80).collect::<String>()).filter(|n| !n.is_empty()) {
        if let Some(x) = st.lib.lock().unwrap().installed.get_mut(&g.id) {
            x.name = n.clone();
        }
        g.name = n;
        st.save();
    }
    Ok(g)
}

/// Downloads a zip (or .7z) a plugin points at and installs what's in it as a game in the Library
/// (with "install"). The player said yes to this exact download first.
#[tauri::command]
pub async fn plugin_install_zip(app: AppHandle, st: State<'_, App>, id: String, url: String, name: String, headers: Option<BTreeMap<String, String>>) -> Res<Installed> {
    let m = allowed(&st.dir, &id, "install")?;
    let u = check_url(&st.dir, &m, &url)?;
    let name: String = name.trim().chars().take(80).collect();
    if name.is_empty() {
        return Err(Error::Msg("It needs a name for the game.".into()));
    }
    let file_name = u.path_segments().and_then(|s| s.last()).filter(|s| !s.is_empty()).unwrap_or("download.zip").to_string();
    let low = file_name.to_lowercase();
    if !(low.ends_with(".zip") || low.ends_with(".7z")) {
        return Err(Error::Msg("Plugins can install .zip and .7z files only.".into()));
    }
    let job = format!("plugin-{}", crate::library::folder_name(&name).to_lowercase());
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let dl_dir = st.dir.join("cache").join("plugin-downloads");
    // Where the file really is, after the site's redirects (each one checked).
    let client = client_for(&headers.unwrap_or_default())?;
    let (_, at) = send(&st.dir, &m, &client, reqwest::Method::GET, u, None).await?;
    let asset = crate::assets::Asset { name: file_name, size: 0, digest: None, browser_download_url: at.to_string() };
    let report = {
        let app = app.clone();
        move |p: crate::install::Progress| {
            let _ = app.emit("plugin-progress", &p);
        }
    };
    let got = crate::install::download(&client, &asset, &dl_dir, &job, cancel, &report).await?;
    let lib_dir = st.lib.lock().unwrap().settings.library_dir.clone();
    let mut target = lib_dir.join(crate::library::folder_name(&name));
    let mut n = 2;
    while target.exists() {
        target = lib_dir.join(format!("{}-{n}", crate::library::folder_name(&name)));
        n += 1;
    }
    let staging = st.dir.join("cache").join("plugin-unpack");
    let file = got.file.clone();
    let t2 = target.clone();
    tokio::task::spawn_blocking(move || crate::install::unpack(&file, &t2, &staging)).await.map_err(|e| Error::Msg(e.to_string()))??;
    let _ = std::fs::remove_file(&got.file);
    let mut g = crate::devtools::add_folder(&st, target.to_string_lossy().to_string(), None, "own")?;
    if let Some(x) = st.lib.lock().unwrap().installed.get_mut(&g.id) {
        x.name = name.clone();
    }
    g.name = name;
    st.save();
    Ok(g)
}

// ---------- serving a plugin's files ----------

/// The SDK every plugin page loads (/_sdk/rl.js): requests to the launcher's page.
pub const SDK: &str = r#"(() => {
  let n = 0;
  const wait = new Map(), ons = {};
  addEventListener('message', (e) => {
    if (e.source !== parent) return;
    const d = e.data || {};
    if (d.rl !== 1) return;
    if (d.n != null && wait.has(d.n)) { const [ok, no] = wait.get(d.n); wait.delete(d.n); d.ok ? ok(d.value) : no(new Error(d.error)); }
    else if (d.event) (ons[d.event] || []).forEach((f) => { try { f(d.data); } catch (err) { console.error(err); } });
  });
  const call = (method, args) => new Promise((ok, no) => { const k = ++n; wait.set(k, [ok, no]); parent.postMessage({ rl: 1, n: k, call: method, args: args || {} }, '*'); });
  window.rl = Object.freeze({
    library: Object.freeze({ list: () => call('library.list'), addFolder: (folder, name) => call('library.addFolder', { folder, name }) }),
    net: Object.freeze({ fetch: (url, o) => call('net.fetch', Object.assign({ url }, o || {})) }),
    install: Object.freeze({ zip: (url, name, o) => call('install.zip', { url, name, headers: (o && o.headers) || null }) }),
    storage: Object.freeze({ get: (key) => call('storage.get', { key }), set: (key, value) => call('storage.set', { key, value }), all: () => call('storage.all') }),
    notify: (title, body) => call('notify', { title, body }),
    toast: (text) => call('toast', { text }),
    openGame: (id) => call('game.open', { id }),
    on: (event, f) => { (ons[event] = ons[event] || []).push(f); },
    info: () => call('info'),
  });
  call('ready').catch(() => {});
})();
"#;

/// What a plugin page may load and do: its own files, and nothing on the internet.
pub const PAGE_POLICY: &str = "default-src 'none'; script-src rlplug: http://rlplug.localhost 'unsafe-inline'; style-src rlplug: http://rlplug.localhost 'unsafe-inline'; img-src rlplug: http://rlplug.localhost data: blob:; font-src rlplug: http://rlplug.localhost data:; media-src rlplug: http://rlplug.localhost data: blob:; connect-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'none'; object-src 'none'";

fn mime(p: &Path) -> &'static str {
    match p.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref() {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("mp3") => "audio/mpeg",
        Some("ogg") => "audio/ogg",
        Some("wav") => "audio/wav",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        _ => "application/octet-stream",
    }
}

/// rlplug://localhost/<plugin>/<file>: a file of a plugin that's on.
pub fn serve(dir: &Path, path: &str) -> tauri::http::Response<Vec<u8>> {
    let bad = || tauri::http::Response::builder().status(404).body(Vec::new()).unwrap();
    let path = percent_decode(path.trim_start_matches('/'));
    if path == "_sdk/rl.js" {
        return tauri::http::Response::builder().header("Content-Type", "text/javascript; charset=utf-8").header("Cache-Control", "no-store").body(SDK.as_bytes().to_vec()).unwrap();
    }
    let Some((id, rel)) = path.split_once('/') else { return bad() };
    if !good_rel(rel) || allowed(dir, id, "").is_err() {
        return bad();
    }
    let base = root(dir).join(id);
    let file = base.join(rel);
    let (Ok(b), Ok(f)) = (base.canonicalize(), file.canonicalize()) else { return bad() };
    if !f.starts_with(&b) || !f.is_file() {
        return bad();
    }
    let Ok(bytes) = std::fs::read(&f) else { return bad() };
    tauri::http::Response::builder()
        .header("Content-Type", mime(&f))
        .header("Content-Security-Policy", PAGE_POLICY)
        .header("X-Content-Type-Options", "nosniff")
        .header("Cache-Control", "no-store")
        .body(bytes)
        .unwrap_or_else(|_| bad())
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m() -> Manifest {
        serde_json::from_value(json!({"id": "example-tools", "name": "Example Tools", "version": "1.0.0", "permissions": ["network"], "sites": ["Example.com", "*.cdn.example.com"], "page": {"entry": "index.html"}})).unwrap()
    }

    #[test]
    fn manifests_are_checked() {
        let mut a = m();
        assert!(check(&mut a).is_ok());
        assert_eq!(a.sites, vec!["*.cdn.example.com", "example.com"]);
        let mut b = m();
        b.id = "Bad Id".into();
        assert!(check(&mut b).is_err());
        let mut c = m();
        c.permissions.push("files.everything".into());
        assert!(check(&mut c).is_err());
        let mut d = m();
        d.sites = vec!["https://example.com/x".into()];
        assert!(check(&mut d).is_err());
        let mut e = m();
        e.sites.clear();
        assert!(check(&mut e).is_err());
        let mut f = m();
        f.page = Some(Page { entry: "../../secret.html".into(), ..Default::default() });
        assert!(check(&mut f).is_err());
    }

    #[test]
    fn sites_match_only_what_was_listed() {
        assert!(site_matches("example.com", "example.com"));
        assert!(!site_matches("example.com", "evil-example.com"));
        assert!(!site_matches("example.com", "a.example.com"));
        assert!(site_matches("*.example.com", "a.example.com"));
        assert!(site_matches("*.example.com", "example.com"));
        assert!(!site_matches("*.example.com", "example.com.evil.net"));
    }

    #[test]
    fn redirects_stay_on_the_sites_a_plugin_lists() {
        let d = std::env::temp_dir().join(format!("rl-plug-r-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let mut man = m();
        check(&mut man).unwrap();
        let mut s = Saved::default();
        s.plugins.insert("example-tools".into(), Entry { enabled: true, sites: vec!["example.com".into()], installed_at: 1 });
        store(&d, &s);
        let u = |x: &str| reqwest::Url::parse(x).unwrap();
        assert!(check_redirect(&d, &man, u("https://example.com/b")).is_ok());
        // The site's own servers (a subdomain of one the player allowed, and listed).
        assert!(check_redirect(&d, &man, u("https://dl1.cdn.example.com/f.zip")).is_ok());
        // Somewhere it didn't list, or not https.
        assert!(check_redirect(&d, &man, u("https://evil.net/f.zip")).unwrap_err().to_string().contains("didn't list"));
        assert!(check_redirect(&d, &man, u("http://example.com/f.zip")).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn plugin_files_stay_inside_the_plugin() {
        assert!(good_rel("index.html"));
        assert!(good_rel("img/icon.svg"));
        assert!(!good_rel("../x"));
        assert!(!good_rel("a/../../x"));
        assert!(!good_rel("/etc/passwd"));
        assert!(!good_rel("C:/x"));
        assert!(!good_rel("a\\b"));
        assert_eq!(percent_decode("my-plugin/a%20b.html"), "my-plugin/a b.html");
    }

    #[test]
    fn a_package_installs_and_is_served_only_when_on() {
        let d = std::env::temp_dir().join(format!("rl-plug-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let pkg = d.join("x.rlplugin");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&pkg).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            z.start_file("example-tools/plugin.json", o).unwrap();
            std::io::Write::write_all(&mut z, serde_json::to_string(&m()).unwrap().as_bytes()).unwrap();
            z.start_file("example-tools/index.html", o).unwrap();
            std::io::Write::write_all(&mut z, b"<h1>hi</h1>").unwrap();
            z.finish().unwrap();
        }
        let (_, man, base) = open_package(&pkg).unwrap();
        assert_eq!(man.id, "example-tools");
        assert_eq!(base, "example-tools/");
        // Installed by hand the way plugin_install does it, then served.
        let target = root(&d).join("example-tools");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("plugin.json"), serde_json::to_vec(&man).unwrap()).unwrap();
        std::fs::write(target.join("index.html"), b"<h1>hi</h1>").unwrap();
        let mut s = Saved::default();
        s.plugins.insert("example-tools".into(), Entry::default());
        store(&d, &s);
        assert_eq!(serve(&d, "/example-tools/index.html").status(), 404);
        s.plugins.get_mut("example-tools").unwrap().enabled = true;
        store(&d, &s);
        let r = serve(&d, "/example-tools/index.html");
        assert_eq!(r.status(), 200);
        assert!(r.headers()["Content-Security-Policy"].to_str().unwrap().contains("connect-src 'none'"));
        assert_eq!(serve(&d, "/example-tools/../plugins.json").status(), 404);
        s.all_off = true;
        store(&d, &s);
        assert_eq!(serve(&d, "/example-tools/index.html").status(), 404);
        let _ = std::fs::remove_dir_all(&d);
    }
}

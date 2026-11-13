//! Recomps the player already has: added to Steam as non-Steam games, installed by another launcher,
//! or in a folder they point at. Each one is matched to its recomp in the catalog and added to the
//! Library where it is (or moved into the Recomp Library), and from then on it updates like any other.

use crate::{catalog, install, library::Installed, shortcuts, App, Error};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use tauri::State;

type Res<T> = Result<T, Error>;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Found {
    /// The recomp in the catalog it matched, or empty when it couldn't tell.
    pub id: String,
    /// What it's called where it was found.
    pub name: String,
    /// Its folder.
    pub path: PathBuf,
    /// The program, relative to `path`.
    pub exe: Option<PathBuf>,
    /// The release it's on, when that's known (another launcher may write it down); empty means "not known".
    pub tag: String,
    /// steam, launcher (another launcher's library) or folder.
    pub from: String,
    /// Already in the Library (the same folder, or the same recomp).
    pub have: bool,
    /// Its project's link, when the place it came from names one.
    #[serde(default)]
    pub link: String,
    /// The match is certain: its project is the recomp's.
    #[serde(default)]
    pub sure: bool,
    /// Only in another launcher's list, not on this PC.
    #[serde(default)]
    pub missing: bool,
    /// Hidden in the launcher it came from.
    #[serde(default)]
    pub hidden: bool,
    /// When it isn't sure: the closest recomps by name, best first.
    #[serde(default)]
    pub guesses: Vec<String>,
}

// ---------- matching a name to a recomp ----------

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase()
}

/// Program and folder names any game can have.
fn generic(n: &str) -> bool {
    matches!(n, "launcher" | "game" | "games" | "play" | "start" | "setup" | "main" | "client" | "release" | "windows" | "win64" | "portable" | "default" | "runner" | "loader" | "program" | "application" | "bin" | "build")
        || n.starts_with("launcher") && n[8..].chars().all(|c| c.is_ascii_digit())
}

/// The catalog recomp whose project is `repo` (owner/name, or a GitHub or GitLab link).
pub fn repo_match(cat: &Value, repo: &str) -> Option<String> {
    let apps = cat.get("apps")?.as_array()?;
    let s = |a: &Value, k: &str| a.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    if repo.trim().is_empty() {
        return None;
    }
    let r = repo.trim().trim_end_matches('/').trim_end_matches(".git").to_lowercase();
    let r = match r.split_once("gitlab.com/") {
        Some((_, p)) => format!("gitlab:{p}"),
        None => r.rsplit("github.com/").next().unwrap_or(&r).to_string(),
    };
    apps.iter().find(|a| s(a, "repo").to_lowercase() == r).map(|a| s(a, "id"))
}

/// Recomps whose names are close to `hints` (a program's name, a folder's name, a Steam name),
/// best first. A program called Launcher.exe or Game.exe says nothing about which game it is:
/// those words never match on their own (another launcher's Link's Awakening isn't the OpenGOAL
/// Launcher).
fn scored(cat: &Value, hints: &[&str]) -> Vec<(usize, String)> {
    let Some(apps) = cat.get("apps").and_then(Value::as_array) else { return vec![] };
    let s = |a: &Value, k: &str| a.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let hints: Vec<String> = hints.iter().map(|h| norm(h)).filter(|h| h.len() >= 4 && !generic(h)).collect();
    let mut out = vec![];
    for a in apps {
        let repo_name = s(a, "repo").rsplit(['/', ':']).next().unwrap_or("").to_string();
        let keys: Vec<String> = [s(a, "id"), s(a, "name"), repo_name].iter().map(|k| norm(k)).filter(|k| k.len() >= 4 && !generic(k)).collect();
        let mut score = 0;
        for h in &hints {
            for k in &keys {
                let sc = if h == k {
                    100
                } else if (h.starts_with(k.as_str()) || k.starts_with(h.as_str())) && h.len().min(k.len()) >= 6 {
                    50 + h.len().min(k.len())
                } else {
                    0
                };
                score = score.max(sc);
            }
        }
        if score > 0 {
            out.push((score, s(a, "id")));
        }
    }
    // Stable: equally close ones keep the catalog's order.
    out.sort_by(|x, y| y.0.cmp(&x.0));
    out
}

/// The catalog recomp that `hints` point to. A repo that matches wins outright; otherwise the
/// closest name, and nothing when two are as close.
pub fn guess(cat: &Value, repo: &str, hints: &[&str]) -> Option<String> {
    if let Some(id) = repo_match(cat, repo) {
        return Some(id);
    }
    match scored(cat, hints).as_slice() {
        [(a, _), (b, _), ..] if a == b => None,
        [(_, id), ..] => Some(id.clone()),
        [] => None,
    }
}

/// Up to `n` recomps to offer when it isn't sure which one a game is, best first.
pub fn guesses(cat: &Value, hints: &[&str], n: usize) -> Vec<String> {
    scored(cat, hints).into_iter().map(|(_, id)| id).take(n).collect()
}

// ---------- where they are ----------

/// The non-Steam games in Steam: (name, program, start folder), for every Steam account on this PC.
fn steam_games() -> Vec<(String, PathBuf, PathBuf)> {
    let Some(steam) = shortcuts::steam_dir() else { return vec![] };
    let me = shortcuts::launcher_exe();
    let mut out = vec![];
    for user in shortcuts::steam_users(&steam) {
        let Ok(b) = std::fs::read(user.join("config/shortcuts.vdf")) else { continue };
        let Some(top) = shortcuts::parse_vdf(&b) else { continue };
        for (_, list) in &top {
            let shortcuts::Node::Map(items) = list else { continue };
            for (_, it) in items {
                let name = it.str("AppName").to_string();
                let exe = PathBuf::from(it.str("Exe").trim().trim_matches('"'));
                let start = PathBuf::from(it.str("StartDir").trim().trim_matches('"'));
                // Ours (they start RecompLauncher) and anything that isn't a program on this PC.
                if exe.as_os_str().is_empty() || exe == me || !exe.is_file() {
                    continue;
                }
                if !out.iter().any(|(_, e, _): &(String, PathBuf, PathBuf)| e == &exe) {
                    out.push((name, exe, start));
                }
            }
        }
    }
    out
}

/// Folders another launcher keeps its library in (apps.json beside Apps), in the usual places: where
/// its installer puts it, and folders people unzip the portable version into.
pub fn library_roots() -> Vec<PathBuf> {
    let mut look: Vec<PathBuf> = vec![];
    for var in ["LOCALAPPDATA", "APPDATA"] {
        if let Ok(v) = std::env::var(var) {
            look.push(PathBuf::from(&v).join("QuiverLauncher"));
            look.push(PathBuf::from(&v).join("Quiver Launcher"));
        }
    }
    if let Some(home) = dirs::home_dir() {
        for d in ["Downloads", "Desktop", "Documents", "Games", ""] {
            let base = if d.is_empty() { home.clone() } else { home.join(d) };
            look.push(base.clone());
            // One level down, where a portable copy was unzipped.
            for e in std::fs::read_dir(&base).into_iter().flatten().flatten() {
                let n = e.file_name().to_string_lossy().to_lowercase();
                if n.contains("quiver") && e.path().is_dir() {
                    look.push(e.path());
                    for e2 in std::fs::read_dir(e.path()).into_iter().flatten().flatten() {
                        if e2.path().is_dir() {
                            look.push(e2.path());
                        }
                    }
                }
            }
        }
    }
    // Linux and Mac: beside the app, or in the system's own data folder when that isn't writable.
    if let Ok(v) = std::env::var("XDG_DATA_HOME") {
        look.push(PathBuf::from(v).join("QuiverLauncher"));
    }
    if let Some(home) = dirs::home_dir() {
        look.push(home.join(".local/share/QuiverLauncher"));
        look.push(home.join("Library/Application Support/QuiverLauncher"));
    }
    if let Ok(v) = std::env::var("APPIMAGE") {
        if let Some(d) = Path::new(&v).parent() {
            look.push(d.to_path_buf());
        }
    }
    for drive in ["C:\\", "D:\\", "E:\\"] {
        for d in ["QuiverLauncher", "Quiver Launcher", "Quiver", "Games\\QuiverLauncher"] {
            look.push(PathBuf::from(drive).join(d));
        }
    }
    let mut out: Vec<PathBuf> = vec![];
    for p in look {
        if is_library(&p) && !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

pub fn is_library(dir: &Path) -> bool {
    let Ok(b) = std::fs::read(dir.join("apps.json")) else { return false };
    serde_json::from_slice::<Value>(&b).ok().and_then(|v| v.get("apps").map(Value::is_array)).unwrap_or(false)
        && (dir.join("settings.json").is_file() || dir.join("Apps").is_dir())
}

/// One project in another launcher's list.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LibEntry {
    pub name: String,
    /// Its project as that launcher wrote it (owner/name or a link); empty for a game added by hand.
    pub repo: String,
    /// Where its folder is, or would be.
    pub path: PathBuf,
    pub installed: bool,
    pub tag: String,
    /// The program, relative to `path`.
    pub exe: Option<PathBuf>,
    pub hidden: bool,
}

/// A project's name or link in one comparable form.
fn mark_key(s: &str) -> String {
    let s = s.trim().trim_end_matches('/').trim_end_matches(".git").to_lowercase();
    let s = s.trim_start_matches("https://").trim_start_matches("http://").trim_start_matches("www.");
    s.trim_start_matches("github.com/").to_string()
}

/// What another launcher marked hidden: flags on its entries, and the list it keeps beside them
/// (JSON or one per line).
fn hidden_marks(root: &Path) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    let Ok(text) = std::fs::read_to_string(root.join("apps.json.lock")) else { return out };
    fn walk(v: &Value, out: &mut std::collections::HashSet<String>) {
        match v {
            Value::String(s) => {
                out.insert(mark_key(s));
            }
            Value::Array(l) => l.iter().for_each(|x| walk(x, out)),
            Value::Object(m) => m.iter().for_each(|(k, x)| {
                out.insert(mark_key(k));
                walk(x, out);
            }),
            _ => {}
        }
    }
    match serde_json::from_str::<Value>(&text) {
        Ok(v) => walk(&v, &mut out),
        Err(_) => text.lines().map(str::trim).filter(|l| !l.is_empty()).for_each(|l| {
            out.insert(mark_key(l));
        }),
    }
    out.remove("");
    out
}

/// Everything in another launcher's list (apps.json beside Apps), on this PC or not.
pub fn library_entries(root: &Path) -> Vec<LibEntry> {
    let read = |f: &str| std::fs::read(root.join(f)).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).unwrap_or(Value::Null);
    let apps = read("apps.json");
    let settings = read("settings.json");
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
    let flag = |v: &Value, k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
    let apps_dir = {
        let p = s(&settings, "AppsPath");
        if p.is_empty() { root.join("Apps") } else { PathBuf::from(p) }
    };
    let marks = hidden_marks(root);
    let mut out = vec![];
    for a in apps.get("apps").and_then(Value::as_array).cloned().unwrap_or_default() {
        let install = s(&a, "installPath");
        let folder = s(&a, "folderName");
        let path = if !install.is_empty() {
            PathBuf::from(install)
        } else if !folder.is_empty() {
            apps_dir.join(&folder)
        } else {
            continue;
        };
        let installed = path.is_dir();
        let tag = if installed { std::fs::read_to_string(path.join("version.txt")).map(|t| t.trim().to_string()).unwrap_or_default() } else { String::new() };
        let exe = if installed {
            std::fs::read_to_string(path.join("selected_executable.txt"))
                .ok()
                .map(|t| PathBuf::from(t.trim()))
                .filter(|p| p.is_file())
                .and_then(|p| p.strip_prefix(&path).ok().map(Path::to_path_buf))
        } else {
            None
        };
        let (name, repo) = (s(&a, "name"), s(&a, "repository"));
        let hidden = flag(&a, "hidden") || flag(&a, "isHidden") || [&name, &repo, &folder, &s(&a, "id")].iter().any(|k| !k.is_empty() && marks.contains(&mark_key(k)));
        out.push(LibEntry { name, repo, path, installed, tag, exe, hidden });
    }
    out
}

/// The installed apps in another launcher's list: (name, repo, folder, tag, program).
pub fn installed_entries(root: &Path) -> Vec<(String, String, PathBuf, String, Option<PathBuf>)> {
    library_entries(root).into_iter().filter(|e| e.installed).map(|e| (e.name, e.repo, e.path, e.tag, e.exe)).collect()
}

/// A project as a link the launcher can add from: owner/name is on GitHub.
fn project_link(repo: &str) -> String {
    let r = repo.trim().trim_end_matches('/').trim_end_matches(".git");
    if r.is_empty() {
        String::new()
    } else if r.starts_with("http://") || r.starts_with("https://") {
        r.to_string()
    } else if let Some(p) = r.strip_prefix("gitlab:") {
        format!("https://gitlab.com/{p}")
    } else if r.matches('/').count() == 1 && !r.contains(' ') {
        format!("https://github.com/{r}")
    } else {
        String::new()
    }
}

/// Another launcher's whole list, matched: sure when its project is a recomp's, otherwise with
/// the closest recomps to choose from. Projects only in the list come along so they can be added
/// from their link; ones added by hand that aren't on this PC have nothing to bring.
fn from_library(cat: &Value, root: &Path, os: &str) -> Vec<Found> {
    let mut out = vec![];
    for e in library_entries(root) {
        let link = project_link(&e.repo);
        if !e.installed && link.is_empty() {
            continue;
        }
        let folder = e.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let stem = e.exe.as_ref().and_then(|x| x.file_stem()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let hints = [e.name.as_str(), folder.as_str(), stem.as_str()];
        let sure = repo_match(cat, &e.repo);
        let id = sure.clone().or_else(|| guess(cat, "", &hints)).unwrap_or_default();
        let near = if sure.is_some() { vec![] } else { guesses(cat, &hints, 3) };
        let exe = if e.installed { e.exe.or_else(|| install::find_exe(&e.path, os, &[&e.name])) } else { None };
        out.push(Found {
            id,
            name: e.name,
            path: e.path,
            exe,
            tag: e.tag,
            from: "launcher".into(),
            have: false,
            link,
            sure: sure.is_some(),
            missing: !e.installed,
            hidden: e.hidden,
            guesses: near,
        });
    }
    out
}

/// A program and the folder that holds the game: the start folder when the program is inside it,
/// otherwise the program's own folder.
fn split(exe: &Path, start: Option<&Path>) -> (PathBuf, Option<PathBuf>) {
    let dir = start.filter(|s| !s.as_os_str().is_empty() && exe.starts_with(s)).map(Path::to_path_buf).unwrap_or_else(|| exe.parent().unwrap_or(Path::new(".")).to_path_buf());
    let rel = exe.strip_prefix(&dir).ok().map(Path::to_path_buf);
    (dir, rel)
}

fn mark(found: &mut [Found], have: &[Installed]) {
    for f in found.iter_mut() {
        f.have = have.iter().any(|g| g.path == f.path || (!f.id.is_empty() && g.id == f.id && !g.local));
    }
}

fn names(cat: &Value, id: &str) -> Vec<String> {
    let a = catalog::app(cat, id).cloned().unwrap_or(Value::Null);
    let s = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    vec![s("name"), s("repo").rsplit(['/', ':']).next().unwrap_or("").to_string(), s("game")]
}

/// Every recomp found on this PC that matches one in the catalog (and those Steam or another
/// launcher has that it couldn't place, with an empty id, so the player can say which it is).
/// `every` (Settings → Bring Your Games) also keeps another launcher's projects that aren't on this
/// PC and the ones in its list that don't look like recomps: the player decides about those.
fn scan(cat: &Value, every: bool) -> Vec<Found> {
    let os = crate::assets::os_now();
    let mut out: Vec<Found> = vec![];
    for root in library_roots() {
        for f in from_library(cat, &root, os) {
            let looks = f.name.to_lowercase().contains("recomp") || f.link.to_lowercase().contains("recomp");
            if !every && (f.missing || f.id.is_empty() && !looks) {
                continue;
            }
            if out.iter().any(|o| o.path == f.path) {
                continue;
            }
            out.push(f);
        }
    }
    for (name, exe, start) in steam_games() {
        let (path, rel) = split(&exe, Some(&start));
        if out.iter().any(|f| f.path == path) {
            continue;
        }
        let folder = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let stem = exe.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let repo = read_repo(&path);
        let id = guess(cat, &repo, &[&name, &stem, &folder]).unwrap_or_default();
        let looks = [&name, &stem, &folder].iter().any(|s| s.to_lowercase().contains("recomp"));
        if id.is_empty() && !looks {
            continue;
        }
        let sure = repo_match(cat, &repo).is_some();
        let near = if sure { vec![] } else { guesses(cat, &[&name, &stem, &folder], 3) };
        out.push(Found { id, name, path, exe: rel, tag: String::new(), from: "steam".into(), have: false, link: project_link(&repo), sure, guesses: near, ..Default::default() });
    }
    out
}

#[tauri::command(async)]
pub async fn adopt_scan(st: State<'_, App>) -> Res<Vec<Found>> {
    let cat = catalog::catalog(&st.http, &st.dir, false).await?.data;
    let mut found = tokio::task::spawn_blocking(move || scan(&cat, false)).await.map_err(|e| Error::Msg(e.to_string()))?;
    let have: Vec<Installed> = st.lib.lock().unwrap().installed.values().cloned().collect();
    mark(&mut found, &have);
    Ok(found)
}

/// Settings → Bring Your Games: everything in other launchers' lists and in Steam, sure matches
/// and the rest, for the player to bring over.
#[tauri::command(async)]
pub async fn migrate_scan(st: State<'_, App>) -> Res<Vec<Found>> {
    let cat = catalog::catalog(&st.http, &st.dir, false).await?.data;
    let mut found = tokio::task::spawn_blocking(move || scan(&cat, true)).await.map_err(|e| Error::Msg(e.to_string()))?;
    let have: Vec<Installed> = st.lib.lock().unwrap().installed.values().cloned().collect();
    mark(&mut found, &have);
    Ok(found)
}

/// Settings → Bring Your Games, a folder the player picked: another launcher's whole list when it's
/// that launcher's folder, otherwise what's in the folder.
#[tauri::command(async)]
pub async fn migrate_pick(st: State<'_, App>, path: String) -> Res<Vec<Found>> {
    let p = PathBuf::from(&path);
    let root = if is_library(&p) { Some(p.clone()) } else { p.parent().filter(|d| is_library(d)).map(Path::to_path_buf) };
    let Some(root) = root else { return adopt_pick(st, path).await };
    let cat = catalog::catalog(&st.http, &st.dir, false).await?.data;
    let os = crate::assets::os_now();
    let mut found = tokio::task::spawn_blocking(move || from_library(&cat, &root, os)).await.map_err(|e| Error::Msg(e.to_string()))?;
    let have: Vec<Installed> = st.lib.lock().unwrap().installed.values().cloned().collect();
    mark(&mut found, &have);
    Ok(found)
}

/// A developer's recomplauncher.json names its repo.
fn read_repo(dir: &Path) -> String {
    crate::profile::release_profile(dir).map(|p| p.profile.repo)
        .or_else(|| std::fs::read(dir.join("recomplauncher.json")).ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .and_then(|v| v.get("repo").and_then(Value::as_str).map(String::from)))
        .unwrap_or_default()
}

const SHARED: &str = "That folder holds more than the game, like Downloads or a whole drive. Put the game in a folder of its own, then pick that folder.";

/// A folder or a program the player picked: what's in it. Another launcher's folder gives the recomps
/// of its list that are on this PC.
#[tauri::command(async)]
pub async fn adopt_pick(st: State<'_, App>, path: String) -> Res<Vec<Found>> {
    let cat = catalog::catalog(&st.http, &st.dir, false).await?.data;
    let p = PathBuf::from(&path);
    let os = crate::assets::os_now();
    let mut out = vec![];
    // Games in the Library waiting for the program their own installer built (it put the game in a
    // folder the launcher didn't know): found in the folder picked, they're connected to it.
    let look = if p.is_dir() { p.clone() } else { p.parent().map(Path::to_path_buf).unwrap_or_default() };
    for (id, folder, exe) in crate::own_setup::attach_built(&st, &look) {
        let name = st.lib.lock().unwrap().installed.get(&id).map(|g| g.name.clone()).unwrap_or_default();
        out.push(Found { id, name, path: folder, exe: exe.file_name().map(PathBuf::from), tag: String::new(), from: "linked".into(), have: true, ..Default::default() });
    }
    let other = if is_library(&p) { Some(p.clone()) } else { p.parent().filter(|d| is_library(d)).map(Path::to_path_buf) };
    if let Some(root) = other {
        out.extend(from_library(&cat, &root, os).into_iter().filter(|f| !f.missing));
    } else if p.is_file() {
        let (dir, rel) = split(&p, None);
        if crate::library::shared_folder(&dir, &st.lib.lock().unwrap().settings.all_libraries()) {
            return Err(Error::Msg(SHARED.into()));
        }
        let folder = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let stem = p.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let id = guess(&cat, &read_repo(&dir), &[&stem, &folder]).unwrap_or_default();
        out.push(Found { id, name: folder, path: dir, exe: rel, tag: String::new(), from: "folder".into(), have: false, ..Default::default() });
    } else if p.is_dir() {
        if crate::library::shared_folder(&p, &st.lib.lock().unwrap().settings.all_libraries()) {
            return Err(Error::Msg(SHARED.into()));
        }
        let folder = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        // A folder of games (like "Games"): no program of its own at the top and not a recomp
        // itself, so each folder inside that is a recomp the catalog knows is offered.
        let top_program = std::fs::read_dir(&p).into_iter().flatten().flatten().any(|e| install::is_program(&e.path(), os));
        if !top_program && guess(&cat, &read_repo(&p), &[&folder]).is_none() {
            let mut many = vec![];
            for e in std::fs::read_dir(&p).into_iter().flatten().flatten().take(300) {
                let sd = e.path();
                if !sd.is_dir() || crate::library::shared_folder(&sd, &st.lib.lock().unwrap().settings.all_libraries()) {
                    continue;
                }
                let name = sd.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let Some(exe) = install::find_exe(&sd, os, &[&name]) else { continue };
                let stem = exe.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                // Ones it can't name come too (id empty): the player says which game, or adds it anyway.
                let id = guess(&cat, &read_repo(&sd), &[&name, &stem]).unwrap_or_default();
                let rel = exe.strip_prefix(&sd).map(Path::to_path_buf).unwrap_or(exe);
                many.push(Found { id, name, path: sd, exe: Some(rel), tag: String::new(), from: "folder".into(), have: false, ..Default::default() });
            }
            if !many.is_empty() {
                let have: Vec<Installed> = st.lib.lock().unwrap().installed.values().cloned().collect();
                mark(&mut many, &have);
                return Ok(many);
            }
        }
        let exe = install::find_exe(&p, os, &[&folder]);
        let stem = exe.as_ref().and_then(|e| e.file_stem()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let id = guess(&cat, &read_repo(&p), &[&folder, &stem]).unwrap_or_default();
        if exe.is_none() {
            if !out.is_empty() {
                return Ok(out);
            }
            return Err(Error::Msg("There's no program in that folder.".into()));
        }
        if !out.iter().any(|f| f.path == p) {
            out.push(Found { id, name: folder, path: p, exe, tag: String::new(), from: "folder".into(), have: false, ..Default::default() });
        }
    } else {
        return Err(Error::Msg("That isn't there.".into()));
    }
    let have: Vec<Installed> = st.lib.lock().unwrap().installed.values().cloned().collect();
    let linked: Vec<PathBuf> = out.iter().filter(|f| f.from == "linked").map(|f| f.path.clone()).collect();
    mark(&mut out, &have);
    // Connected just now: shown as in the Library, once.
    out.retain(|f| f.from == "linked" || !linked.contains(&f.path));
    Ok(out)
}

/// "I Already Have It" on a game's page: is the folder the player picked this recomp? The program
/// its profile starts is in it (or in the one folder inside it), or, with no profile, a program
/// named like the recomp is. The folder that holds the game, its program, and whether it fits.
pub fn check_folder(dir: &Path, entry: &Value, names: &[String], os: &str) -> (PathBuf, Option<PathBuf>, bool) {
    let mut roots = vec![dir.to_path_buf()];
    // The game one level down (a zip unpacked into a folder of its own): the folder inside.
    let inside: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| !p.file_name().map(|n| n.to_string_lossy().starts_with('.')).unwrap_or(true)).collect();
    if inside.len() == 1 && inside[0].is_dir() {
        roots.insert(0, inside[0].clone());
    }
    let repo = entry.get("repo").and_then(Value::as_str).unwrap_or("");
    let n: Vec<&str> = names.iter().map(String::as_str).filter(|x| !x.is_empty()).collect();
    for root in &roots {
        if let Some(start) = crate::profile::find(root, repo, entry).and_then(|p| p.start) {
            if let Some(prog) = start.program.iter().map(|p| p.trim_start_matches("{install}/").to_string()).find(|p| root.join(p).is_file()) {
                return (root.clone(), Some(PathBuf::from(prog)), true);
            }
            continue;
        }
        // On Linux a Windows build counts too: it plays through Proton.
        let found = install::find_exe(root, os, &n).or_else(|| (os == "linux").then(|| install::find_exe(root, "windows", &n)).flatten());
        if let Some(exe) = found {
            // Words that are in many recomps' names ("Recompiled", "Port", "Edition") don't count.
            let want: Vec<String> = n.iter().flat_map(|x| install::tokens(x)).filter(|t| t.len() > 2 && t != "the" && !install::TAIL_WORDS.contains(&t.as_str())).collect();
            let stem = exe.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let squashed: String = stem.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_lowercase();
            let named = install::tokens(&stem).iter().any(|t| want.contains(t)) || want.iter().any(|w| w.len() >= 4 && squashed.contains(w.as_str()));
            let rel = exe.strip_prefix(root).map(Path::to_path_buf).unwrap_or(exe);
            if named {
                return (root.clone(), Some(rel), true);
            }
        }
    }
    // Not it, as far as can be told: its best program, for Use It Anyway.
    let root = roots[0].clone();
    let exe = install::find_exe(&root, os, &n).or_else(|| (os == "linux").then(|| install::find_exe(&root, "windows", &n)).flatten()).map(|e| e.strip_prefix(&root).map(Path::to_path_buf).unwrap_or(e));
    (root, exe, false)
}

/// The command for it: the folder, its program, whether it's this recomp, and the release it's on
/// (not known for a folder: the next update offers itself).
#[tauri::command(async)]
pub async fn adopt_check(st: State<'_, App>, id: String, path: String) -> Res<Value> {
    let cat = catalog::catalog(&st.http, &st.dir, false).await?.data;
    let entry = catalog::app(&cat, &id).cloned().ok_or_else(|| Error::Msg("This recomp isn't listed anymore.".into()))?;
    let dir = PathBuf::from(&path);
    if !dir.is_dir() {
        return Err(Error::Msg("That folder couldn't be found.".into()));
    }
    if crate::library::shared_folder(&dir, &st.lib.lock().unwrap().settings.all_libraries()) {
        return Err(Error::Msg(SHARED.into()));
    }
    if st.lib.lock().unwrap().installed.get(&id).map(|g| !g.local).unwrap_or(false) {
        return Err(Error::Msg("It's already in your Library.".into()));
    }
    let names = names(&cat, &id);
    let os = crate::assets::os_now();
    let (root, exe, fits) = tokio::task::spawn_blocking(move || check_folder(&dir, &entry, &names, os)).await.map_err(|e| Error::Msg(e.to_string()))?;
    // Already another game's folder in the Library (or inside one, or holding one): two games in
    // one folder would update and uninstall each other.
    let taken = st.lib.lock().unwrap().installed.values().find(|g| !g.path.as_os_str().is_empty() && (g.path == root || root.starts_with(&g.path) || g.path.starts_with(&root))).map(|g| g.name.clone());
    if let Some(name) = taken {
        return Ok(serde_json::json!({ "path": root, "taken": name }));
    }
    let proton = os == "linux" && exe.as_ref().map(|e| crate::proton::is_windows_program(e)).unwrap_or(false);
    Ok(serde_json::json!({ "path": root, "exe": exe, "fits": fits, "tag": "", "proton": proton }))
}

#[derive(Debug, Deserialize)]
pub struct Adopt {
    pub id: String,
    pub path: PathBuf,
    pub exe: Option<PathBuf>,
    #[serde(default)]
    pub tag: String,
    /// A Windows build on Linux (from a game's page, I Already Have It): it plays through Proton.
    #[serde(default)]
    pub proton: bool,
}

/// Add them to the Library where they are. -> the ones added. (Moving them is move_game, after.)
#[tauri::command(async)]
pub async fn adopt_add(st: State<'_, App>, items: Vec<Adopt>) -> Res<Vec<Installed>> {
    let cat = catalog::catalog(&st.http, &st.dir, false).await?.data;
    let os = crate::assets::os_now();
    let mut added = vec![];
    // Ones the launcher doesn't know: added as the player's own games (they play, but don't update).
    let mut unknown = vec![];
    {
        let mut l = st.lib.lock().unwrap();
        let libs = l.settings.all_libraries();
        for it in items {
            let Some(entry) = catalog::app(&cat, &it.id) else {
                if !crate::library::shared_folder(&it.path, &libs) {
                    unknown.push(it);
                }
                continue;
            };
            if l.installed.get(&it.id).map(|g| !g.local).unwrap_or(false) || !it.path.is_dir() || crate::library::shared_folder(&it.path, &libs) {
                continue;
            }
            let s = |k: &str| entry.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            let n = names(&cat, &it.id);
            let exe = it.exe.filter(|e| it.path.join(e).is_file()).or_else(|| install::find_exe(&it.path, os, &n.iter().map(String::as_str).collect::<Vec<_>>()));
            let g = Installed {
                id: it.id.clone(),
                name: s("name"),
                repo: s("repo"),
                tag: it.tag.trim().to_string(),
                version: s("version"),
                path: it.path.clone(),
                exe,
                installed_at: crate::library::now(),
                look_amount: 50,
                // Played somewhere else before: its setup, if it has one, already ran.
                setup_checked: true,
                exe_rules: install::EXE_RULES,
                proton: it.proton,
                ..Default::default()
            };
            l.put(&it.id, g.clone());
            added.push(g);
        }
    }
    st.save();
    for it in unknown {
        let path = match &it.exe {
            Some(e) if it.path.join(e).is_file() => it.path.join(e),
            _ => it.path.clone(),
        };
        if let Ok(g) = crate::devtools::add_any(st.clone(), path.to_string_lossy().to_string()) {
            added.push(g);
        }
    }
    Ok(added)
}

/// Change the catalog association without installing or touching the game's folder.
/// An empty project means the player maintains this installation themselves.
#[tauri::command(async)]
pub fn set_project(st: State<'_, App>, id: String, project: String) -> Res<Installed> {
    change_project(&st, &id, &project, &crate::cached_catalog(&st))
}

/// A listing the catalog replaced ("replaced": {old id: new id}), moved to its new listing when
/// the player presses Move to It: the install, play time, saves (and their backups), the copy of
/// the game they chose and their settings go with it, and links to the old id still open it.
/// The same project under a new id keeps its files and version. Another project's release:
/// `reinstall` is true, and the window installs it into the same folder (an update, saves backed
/// up first) so nothing is lost if that fails.
#[tauri::command(async)]
pub fn move_listing(st: State<'_, App>, from: String, to: String) -> Res<Value> {
    move_listing_in(&st, &crate::cached_catalog(&st), &from, &to)
}

pub(crate) fn move_listing_in(st: &App, cat: &Value, from: &str, to: &str) -> Res<Value> {
    let (from, to) = (from.to_string(), to.to_string());
    let replaced = cat.get("replaced").and_then(|r| r.get(&from)).and_then(Value::as_str);
    if replaced != Some(to.as_str()) {
        return Err(Error::Msg("That listing wasn't replaced. Refresh the list and try again.".into()));
    }
    let g = st.lib.lock().unwrap().installed.get(&from).cloned().ok_or_else(|| Error::Msg("It isn't in your Library.".into()))?;
    let entry = catalog::app(cat, &to).cloned().ok_or_else(|| Error::Msg("The new listing isn't in the catalog yet. Refresh it and try again.".into()))?;
    let same = !g.repo.is_empty() && crate::str_of(&entry, "repo").eq_ignore_ascii_case(&g.repo);
    let mut next = change_project(st, &from, &to, cat)?;
    next.name = crate::str_of(&entry, "name").to_string();
    if same {
        // The same release under a new name: what's installed is still exactly what it was.
        let k = &g;
        next.tag = k.tag.clone();
        next.version = k.version.clone();
        next.asset = k.asset.clone();
        next.sha256 = k.sha256.clone();
        next.size = k.size;
        next.verified = k.verified;
        next.files = k.files.clone();
        next.setup = k.setup.clone();
        next.setup_done = k.setup_done;
        next.cloud = k.cloud;
    }
    {
        let mut l = st.lib.lock().unwrap();
        if let Some(x) = l.installed.get_mut(&to) {
            *x = next.clone();
        }
        l.save(&st.dir)?;
    }
    crate::trail::note(format!("Moved {from} to its new listing {to}{}", if same { "" } else { " (its new release installs next)" }));
    Ok(serde_json::json!({ "id": to, "reinstall": !same, "game": next }))
}

fn associated_game(g: &Installed, id: &str, entry: Option<&Value>) -> Installed {
    let mut next = g.clone();
    next.id = id.into();
    next.repo = entry.map(|e| crate::str_of(e, "repo").to_string()).unwrap_or_default();
    next.local = entry.is_none();
    next.tag = if next.local { "own".into() } else { String::new() };
    next.version.clear();
    next.asset.clear();
    next.sha256.clear();
    next.size = 0;
    next.verified = false;
    next.pinned = false;
    next.auto_update = false;
    next.exe_rules = install::EXE_RULES;
    // These describe files supplied by the previous project's release. Treat the files as
    // the player's own until an explicit installation supplies a new manifest.
    next.files.clear();
    next.setup = None;
    next.setup_done = false;
    next.setup_checked = true;
    // Remote saves belong to the old project. Keep local backups, and ask before using the
    // new project's cloud saves instead of silently uploading to a different namespace.
    next.cloud = None;
    next
}

pub(crate) fn change_project(st: &App, old: &str, project: &str, cat: &Value) -> Res<Installed> {
    let entry = if project.is_empty() {
        None
    } else {
        Some(catalog::app(cat, project).filter(|e| !crate::str_of(e, "repo").is_empty())
            .ok_or_else(|| Error::Msg("That project isn't in the catalog. Refresh it and try again.".into()))?)
    };
    let mut l = st.lib.lock().unwrap();
    let g = l.installed.get(old).cloned().ok_or_else(|| Error::Msg("It isn't in your Library.".into()))?;
    if (entry.is_some() && old == project && !g.local) || (entry.is_none() && g.local && g.tag == "own") {
        return Ok(g);
    }
    let new = if entry.is_some() {
        project.to_string()
    } else {
        let base = format!("own-{}", crate::library::folder_name(old));
        let mut id = base.clone();
        let mut n = 2;
        while l.installed.contains_key(&id) {
            id = format!("{base}-{n}");
            n += 1;
        }
        id
    };
    if new != old && l.installed.contains_key(&new) {
        return Err(Error::Msg("That project is already in your Library. Its installation won't be replaced.".into()));
    }
    if [old, new.as_str()].iter().any(|id| st.running.lock().unwrap().contains_key(*id)
        || st.jobs.lock().unwrap().contains_key(*id) || st.moving.lock().unwrap().contains(*id)
        || crate::STARTING.lock().unwrap().iter().any(|starting| starting == *id)
        || crate::setup_busy(id) || crate::mods_busy(id)) {
        return Err(Error::Msg("Stop the game and wait for its downloads, setup and file operations before changing its project.".into()));
    }
    if crate::saves::chosen_root().is_some_and(|p| !p.is_dir()) {
        return Err(Error::Msg("Connect your save backup drive before changing the project.".into()));
    }
    let next = associated_game(&g, &new, entry);
    // Library ids also key backups and looks. Rename them before committing the library;
    // reject collisions, and roll back if moving data or saving the library fails.
    let mut dirs = vec![
        (crate::saves::dir_of(&st.dir, old), crate::saves::dir_of(&st.dir, &new)),
        (st.dir.join("saves").join(old), st.dir.join("saves").join(&new)),
        (st.dir.join("looks").join(old), st.dir.join("looks").join(&new)),
    ];
    dirs.dedup();
    dirs.retain(|(from, to)| from != to && from.exists());
    if dirs.iter().any(|(_, to)| to.exists()) {
        return Err(Error::Msg("The selected project already has backup or shader data. Nothing was changed.".into()));
    }
    let mut moved = Vec::new();
    for (from, to) in &dirs {
        if let Err(err) = std::fs::rename(from, to) {
            for (from, to) in moved.iter().rev() { let _ = std::fs::rename(to, from); }
            return Err(err.into());
        }
        moved.push((from, to));
    }
    let before = l.clone();
    l.installed.remove(old);
    l.installed.insert(new.clone(), next.clone());
    l.project_aliases.remove(&new);
    for target in l.project_aliases.values_mut() {
        if target == old { *target = new.clone(); }
    }
    if new != old { l.project_aliases.insert(old.to_string(), new.clone()); }
    if let Some(mods) = l.mods.remove(old) { l.mods.insert(new.clone(), mods); }
    if let Some(art) = l.arts.remove(old) { l.arts.insert(new.clone(), art); }
    l.seen_versions.remove(old);
    crate::rename_id(&mut l.settings.ui, old, &new);
    if let Err(err) = l.save(&st.dir) {
        *l = before;
        for (from, to) in moved.iter().rev() { let _ = std::fs::rename(to, from); }
        return Err(err.into());
    }
    Ok(next)
}

#[cfg(test)]
mod tests {

    #[test]
    fn appimage_repository_comes_from_the_embedded_profile() {
        use crate::appimage::tests::{image, TempDir};
        let dir = TempDir::new();
        std::fs::write(dir.0.join("game.AppImage"),
            image(Some(br#"{"format":1,"repo":"developer/game"}"#), backhand::compression::Compressor::Gzip)).unwrap();
        std::fs::write(dir.0.join("recomplauncher.json"), r#"{"format":1,"repo":"stale/sidecar"}"#).unwrap();
        assert_eq!(read_repo(&dir.0), "developer/game");
    }

    use super::*;
    use serde_json::json;

    #[test]
    fn a_folder_the_player_already_has_is_checked() {
        let d = std::env::temp_dir().join(format!("rl-adopt-check-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let os = crate::assets::os_now();
        let prog = if os == "windows" { "Zelda64Recompiled.exe" } else { "Zelda64Recompiled" };
        let names = vec!["Zelda 64: Recompiled".to_string(), "Zelda64Recomp".to_string(), "Majora's Mask".to_string()];
        let entry = json!({ "id": "z64", "repo": "Zelda64Recomp/Zelda64Recomp" });
        // Its program right there, and one folder down.
        let right = d.join("right");
        std::fs::create_dir_all(&right).unwrap();
        let mut body = b"#!/bin/sh\n".to_vec();
        body.resize(4096, b' ');
        std::fs::write(right.join(prog), &body).unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(right.join(prog), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let (root, exe, fits) = check_folder(&right, &entry, &names, os);
        assert!(fits && root == right && exe == Some(PathBuf::from(prog)));
        let outer = d.join("outer");
        std::fs::create_dir_all(&outer).unwrap();
        std::fs::rename(&right, outer.join("Zelda64Recomp-v1.2")).unwrap();
        let (root, _, fits) = check_folder(&outer, &entry, &names, os);
        assert!(fits && root == outer.join("Zelda64Recomp-v1.2"));
        // Another game's folder: not it, and its program is still offered for Use It Anyway.
        let wrong = d.join("wrong");
        std::fs::create_dir_all(&wrong).unwrap();
        let other = if os == "windows" { "SonicUnleashed.exe" } else { "SonicUnleashed" };
        std::fs::write(wrong.join(other), &body).unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(wrong.join(other), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let (_, exe, fits) = check_folder(&wrong, &entry, &names, os);
        assert!(!fits);
        assert_eq!(exe, Some(PathBuf::from(other)));
        // Another recomp's program with a word every recomp has ("Recompiled"): not it.
        let banjo = d.join("banjo");
        std::fs::create_dir_all(&banjo).unwrap();
        let bprog = if os == "windows" { "BanjoRecompiled.exe" } else { "BanjoRecompiled" };
        std::fs::write(banjo.join(bprog), &body).unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(banjo.join(bprog), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        assert!(!check_folder(&banjo, &entry, &names, os).2);
        // With a profile: its start program decides.
        let prof = d.join("prof");
        std::fs::create_dir_all(prof.join("bin")).unwrap();
        std::fs::write(prof.join("bin/game.exe"), b"MZ").unwrap();
        let entry2 = json!({ "id": "x", "boot": { "start": { "program": ["bin/game.exe"] } } });
        let has_start = crate::profile::find(&prof, "", &entry2).and_then(|p| p.start).is_some();
        if has_start {
            let (_, exe, fits) = check_folder(&prof, &entry2, &["Other".to_string()], os);
            assert!(fits && exe == Some(PathBuf::from("bin/game.exe")));
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    fn cat() -> Value {
        json!({"apps": [
            {"id": "zelda64recomp", "name": "Zelda 64: Recompiled", "repo": "Zelda64Recomp/Zelda64Recomp"},
            {"id": "banjorecomp", "name": "Banjo: Recompiled", "repo": "BanjoRecomp/BanjoRecomp"},
            {"id": "unleashedrecomp", "name": "Unleashed Recompiled", "repo": "hedge-dev/UnleashedRecomp"},
            {"id": "mk64recomp", "name": "Mario Kart 64 Recomp", "repo": "mk/MK64"},
        ]})
    }

    #[test]
    fn a_generic_program_name_matches_nothing() {
        let c = json!({"apps": [{"id": "launcher", "name": "OpenGOAL Launcher", "repo": "open-goal/launcher"}, {"id": "zelda64recomp", "name": "Zelda 64: Recompiled", "repo": "Zelda64Recomp/Zelda64Recomp"}]});
        assert_eq!(guess(&c, "", &["TheLegendOfZeldaLinksAwakeningDX-HDUpdated-Recreation", "Launcher"]), None);
        assert_eq!(guess(&c, "", &["OpenGOAL Launcher"]), Some("launcher".into()));
        assert_eq!(guess(&c, "open-goal/launcher", &["Launcher"]), Some("launcher".into()));
    }

    #[test]
    fn matches_by_repo_then_name() {
        let c = cat();
        assert_eq!(guess(&c, "https://github.com/hedge-dev/UnleashedRecomp", &[]).as_deref(), Some("unleashedrecomp"));
        assert_eq!(guess(&c, "", &["Zelda64Recompiled", "Zelda64Recomp"]).as_deref(), Some("zelda64recomp"));
        assert_eq!(guess(&c, "", &["UnleashedRecompiled"]).as_deref(), Some("unleashedrecomp"));
        assert_eq!(guess(&c, "", &["Banjo: Recompiled"]).as_deref(), Some("banjorecomp"));
        assert_eq!(guess(&c, "", &["Photoshop", "Adobe"]), None);
        assert_eq!(guess(&c, "", &["MK64"]).as_deref(), Some("mk64recomp"));
        assert_eq!(guess(&c, "", &["Recomp"]), None);
    }

    /// A catalog with neutral names for the tests below.
    fn plain_cat() -> Value {
        json!({"apps": [
            {"id": "skyracer", "name": "Sky Racer Recompiled", "repo": "dev-a/SkyRacerRecomp"},
            {"id": "skyracerdx", "name": "Sky Racer DX", "repo": "dev-b/SkyRacerDX"},
            {"id": "cavequest", "name": "Cave Quest Recomp", "repo": "gitlab:group/cave-quest"},
        ]})
    }

    /// Another launcher's folder with three projects: one installed, one only in its list, one
    /// added by hand and not installed.
    fn other_library(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rl-lib-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let app = d.join("Apps").join("MyRacer");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join("SkyRacer.exe"), b"MZ").unwrap();
        std::fs::write(app.join("version.txt"), "v2.0\n").unwrap();
        std::fs::write(d.join("settings.json"), "{}").unwrap();
        std::fs::write(d.join("apps.json"), r#"{"apps":[
            {"name":"My Racer","repository":"dev-a/SkyRacerRecomp","folderName":"MyRacer"},
            {"name":"Some Port","repository":"someone/some-port","folderName":"SomePort"},
            {"name":"By Hand","repository":"","folderName":"ByHand"}
        ]}"#).unwrap();
        d
    }

    #[test]
    fn another_launchers_whole_list_is_read_installed_or_not() {
        let d = other_library("all");
        let all = library_entries(&d);
        assert_eq!(all.len(), 3);
        assert!(all[0].installed && !all[1].installed && !all[2].installed);
        assert_eq!(all[0].tag, "v2.0");
        assert!(all.iter().all(|e| !e.hidden));
        // The older reader still gives just the installed ones.
        assert_eq!(installed_entries(&d).len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn games_hidden_in_another_launcher_stay_hidden() {
        let d = other_library("hid");
        std::fs::write(d.join("apps.json.lock"), "someone/some-port\n").unwrap();
        let all = library_entries(&d);
        assert!(!all[0].hidden && all[1].hidden);
        // The same list as JSON, by name or by link.
        std::fs::write(d.join("apps.json.lock"), r#"{"hidden":["My Racer","https://github.com/someone/some-port"]}"#).unwrap();
        let all = library_entries(&d);
        assert!(all[0].hidden && all[1].hidden && !all[2].hidden);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn bringing_games_over_is_sure_only_when_the_project_matches() {
        let d = other_library("sure");
        let found = from_library(&plain_cat(), &d, "windows");
        // The one added by hand and not installed has nothing to bring.
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].id, "skyracer");
        assert!(found[0].sure && !found[0].missing && found[0].guesses.is_empty());
        assert_eq!(found[0].exe.as_deref(), Some(Path::new("SkyRacer.exe")));
        assert_eq!(found[0].from, "launcher");
        // Only in the list, and not a recomp the launcher knows: it comes with its link to add from.
        assert!(found[1].missing && !found[1].sure && found[1].id.is_empty());
        assert_eq!(found[1].link, "https://github.com/someone/some-port");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn close_names_are_offered_best_first() {
        let c = plain_cat();
        // The exact name first, then the close one.
        assert_eq!(guesses(&c, &["Sky Racer DX"], 3), vec!["skyracerdx".to_string(), "skyracer".to_string()]);
        assert_eq!(guesses(&c, &["Sky Racer DX"], 1), vec!["skyracerdx".to_string()]);
        // An exact name wins outright; one both are as close to is offered both ways, and guess
        // won't pick for the player.
        assert_eq!(guess(&c, "", &["SkyRacer"]).as_deref(), Some("skyracer"));
        assert_eq!(guesses(&c, &["Sky Rac"], 3).len(), 2);
        assert_eq!(guess(&c, "", &["Sky Rac"]), None);
        assert!(guesses(&c, &["Launcher"], 3).is_empty());
        assert_eq!(repo_match(&c, "https://gitlab.com/group/cave-quest/"), Some("cavequest".into()));
        assert_eq!(repo_match(&c, "https://github.com/dev-a/SkyRacerRecomp.git"), Some("skyracer".into()));
        assert_eq!(repo_match(&c, ""), None);
    }

    #[test]
    fn a_project_becomes_a_link_to_add_from() {
        assert_eq!(project_link("owner/name"), "https://github.com/owner/name");
        assert_eq!(project_link("https://gitlab.com/g/sub/p.git"), "https://gitlab.com/g/sub/p");
        assert_eq!(project_link("gitlab:g/p"), "https://gitlab.com/g/p");
        assert_eq!(project_link(""), "");
        assert_eq!(project_link("not a project"), "");
    }

    #[test]
    fn reads_another_launchers_library() {
        let d = std::env::temp_dir().join(format!("rl-other-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let app = d.join("Apps").join("Zelda64Recomp");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join("Zelda64Recompiled.exe"), b"MZ").unwrap();
        std::fs::write(app.join("version.txt"), "v1.2.2\n").unwrap();
        std::fs::write(app.join("selected_executable.txt"), app.join("Zelda64Recompiled.exe").to_string_lossy().as_bytes()).unwrap();
        std::fs::write(d.join("settings.json"), "{}").unwrap();
        std::fs::write(d.join("apps.json"), r#"{"apps":[{"name":"Zelda 64","repository":"Zelda64Recomp/Zelda64Recomp","folderName":"Zelda64Recomp"},{"name":"Not installed","repository":"a/b","folderName":"Gone"}]}"#).unwrap();
        assert!(is_library(&d));
        let apps = installed_entries(&d);
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].3, "v1.2.2");
        assert_eq!(apps[0].4.as_deref(), Some(Path::new("Zelda64Recompiled.exe")));
        assert_eq!(guess(&cat(), &apps[0].1, &[]).as_deref(), Some("zelda64recomp"));
        let _ = std::fs::remove_dir_all(&d);
    }
}

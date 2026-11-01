//! Recomps the player already has: added to Steam as non-Steam games, installed by Quiver Launcher,
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
    /// The release it's on, when that's known (Quiver writes it down); empty means "not known".
    pub tag: String,
    /// steam, quiver or folder.
    pub from: String,
    /// Already in the Library (the same folder, or the same recomp).
    pub have: bool,
}

// ---------- matching a name to a recomp ----------

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase()
}

/// The catalog recomp that `hints` (a repo, a program's name, a folder's name, a Steam name) point to.
/// A repo that matches wins outright; otherwise the closest name, and nothing when two are as close.
pub fn guess(cat: &Value, repo: &str, hints: &[&str]) -> Option<String> {
    let apps = cat.get("apps")?.as_array()?;
    let s = |a: &Value, k: &str| a.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    if !repo.is_empty() {
        let r = repo.trim().trim_end_matches(".git").trim_end_matches('/').to_lowercase();
        let r = r.rsplit("github.com/").next().unwrap_or(&r).to_string();
        if let Some(a) = apps.iter().find(|a| s(a, "repo").to_lowercase() == r) {
            return Some(s(a, "id"));
        }
    }
    let hints: Vec<String> = hints.iter().map(|h| norm(h)).filter(|h| h.len() >= 4).collect();
    let mut best: Option<(usize, String)> = None;
    let mut tie = false;
    for a in apps {
        let repo_name = s(a, "repo").rsplit('/').next().unwrap_or("").to_string();
        let keys: Vec<String> = [s(a, "id"), s(a, "name"), repo_name].iter().map(|k| norm(k)).filter(|k| k.len() >= 4).collect();
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
        if score == 0 {
            continue;
        }
        match &best {
            Some((b, _)) if score < *b => {}
            Some((b, _)) if score == *b => tie = true,
            _ => {
                best = Some((score, s(a, "id")));
                tie = false;
            }
        }
    }
    if tie {
        return None;
    }
    best.map(|(_, id)| id)
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

/// Folders Quiver Launcher keeps its library in (apps.json beside Apps), in the usual places: where
/// its installer puts it, and folders people unzip the portable version into.
pub fn quiver_roots() -> Vec<PathBuf> {
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
            // One level down: Downloads\QuiverLauncher-win-Portable, Desktop\Quiver, ...
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
    for drive in ["C:\\", "D:\\", "E:\\"] {
        for d in ["QuiverLauncher", "Quiver Launcher", "Quiver", "Games\\QuiverLauncher"] {
            look.push(PathBuf::from(drive).join(d));
        }
    }
    let mut out: Vec<PathBuf> = vec![];
    for p in look {
        if is_quiver(&p) && !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

pub fn is_quiver(dir: &Path) -> bool {
    let Ok(b) = std::fs::read(dir.join("apps.json")) else { return false };
    serde_json::from_slice::<Value>(&b).ok().and_then(|v| v.get("apps").map(Value::is_array)).unwrap_or(false)
        && (dir.join("settings.json").is_file() || dir.join("Apps").is_dir())
}

/// The apps in one Quiver library that are installed: (name, repo, folder, tag, program).
pub fn quiver_apps(root: &Path) -> Vec<(String, String, PathBuf, String, Option<PathBuf>)> {
    let read = |f: &str| std::fs::read(root.join(f)).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).unwrap_or(Value::Null);
    let apps = read("apps.json");
    let settings = read("settings.json");
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
    let apps_dir = {
        let p = s(&settings, "AppsPath");
        if p.is_empty() { root.join("Apps") } else { PathBuf::from(p) }
    };
    let mut out = vec![];
    for a in apps.get("apps").and_then(Value::as_array).cloned().unwrap_or_default() {
        let install = s(&a, "installPath");
        let folder = s(&a, "folderName");
        let path = if !install.is_empty() {
            PathBuf::from(install)
        } else if !folder.is_empty() {
            apps_dir.join(folder)
        } else {
            continue;
        };
        if !path.is_dir() {
            continue;
        }
        let tag = std::fs::read_to_string(path.join("version.txt")).map(|t| t.trim().to_string()).unwrap_or_default();
        let exe = std::fs::read_to_string(path.join("selected_executable.txt"))
            .ok()
            .map(|t| PathBuf::from(t.trim()))
            .filter(|p| p.is_file())
            .and_then(|p| p.strip_prefix(&path).ok().map(Path::to_path_buf));
        out.push((s(&a, "name"), s(&a, "repository"), path, tag, exe));
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
    vec![s("name"), s("repo").rsplit('/').next().unwrap_or("").to_string(), s("game")]
}

/// Every recomp found on this PC that matches one in the catalog (and those Steam or Quiver have
/// that it couldn't place, with an empty id, so the player can say which it is).
#[tauri::command(async)]
pub async fn adopt_scan(st: State<'_, App>) -> Res<Vec<Found>> {
    let cat = catalog::catalog(&st.http, &st.dir, false).await?.data;
    let os = crate::assets::os_now();
    let found = tokio::task::spawn_blocking({
        let cat = cat.clone();
        move || {
            let mut out: Vec<Found> = vec![];
            for root in quiver_roots() {
                for (name, repo, path, tag, exe) in quiver_apps(&root) {
                    let folder = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    let stem = exe.as_ref().and_then(|e| e.file_stem()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    let id = guess(&cat, &repo, &[&name, &folder, &stem]).unwrap_or_default();
                    // Quiver installs anything from GitHub; only recomps (or ones it can't place but
                    // that look like one) come along.
                    if id.is_empty() && !(name.to_lowercase().contains("recomp") || repo.to_lowercase().contains("recomp")) {
                        continue;
                    }
                    let exe = exe.or_else(|| install::find_exe(&path, os, &[&name]));
                    out.push(Found { id, name, path, exe, tag, from: "quiver".into(), have: false });
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
                let id = guess(&cat, &repo, &[&name, &stem, &folder]).unwrap_or_default();
                let looks = [&name, &stem, &folder].iter().any(|s| s.to_lowercase().contains("recomp"));
                if id.is_empty() && !looks {
                    continue;
                }
                out.push(Found { id, name, path, exe: rel, tag: String::new(), from: "steam".into(), have: false });
            }
            out
        }
    })
    .await
    .map_err(|e| Error::Msg(e.to_string()))?;
    let mut found = found;
    let have: Vec<Installed> = st.lib.lock().unwrap().installed.values().cloned().collect();
    mark(&mut found, &have);
    Ok(found)
}

/// A developer's recomplauncher.json names its repo.
fn read_repo(dir: &Path) -> String {
    std::fs::read(dir.join("recomplauncher.json")).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|v| v.get("repo").and_then(Value::as_str).map(String::from)).unwrap_or_default()
}

const SHARED: &str = "That folder holds more than the game, like Downloads or a whole drive. Put the game in a folder of its own, then pick that folder.";

/// A folder or a program the player picked: what's in it. A Quiver library gives all of its recomps.
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
        out.push(Found { id, name, path: folder, exe: exe.file_name().map(PathBuf::from), tag: String::new(), from: "linked".into(), have: true });
    }
    let quiver = if is_quiver(&p) { Some(p.clone()) } else { p.parent().filter(|d| is_quiver(d)).map(Path::to_path_buf) };
    if let Some(root) = quiver {
        for (name, repo, path, tag, exe) in quiver_apps(&root) {
            let folder = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let id = guess(&cat, &repo, &[&name, &folder]).unwrap_or_default();
            let exe = exe.or_else(|| install::find_exe(&path, os, &[&name]));
            out.push(Found { id, name, path, exe, tag, from: "quiver".into(), have: false });
        }
    } else if p.is_file() {
        let (dir, rel) = split(&p, None);
        if crate::library::shared_folder(&dir, &st.lib.lock().unwrap().settings.all_libraries()) {
            return Err(Error::Msg(SHARED.into()));
        }
        let folder = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let stem = p.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let id = guess(&cat, &read_repo(&dir), &[&stem, &folder]).unwrap_or_default();
        out.push(Found { id, name: folder, path: dir, exe: rel, tag: String::new(), from: "folder".into(), have: false });
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
                many.push(Found { id, name, path: sd, exe: Some(rel), tag: String::new(), from: "folder".into(), have: false });
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
            out.push(Found { id, name: folder, path: p, exe, tag: String::new(), from: "folder".into(), have: false });
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

#[derive(Debug, Deserialize)]
pub struct Adopt {
    pub id: String,
    pub path: PathBuf,
    pub exe: Option<PathBuf>,
    #[serde(default)]
    pub tag: String,
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
                ..Default::default()
            };
            l.installed.insert(it.id.clone(), g.clone());
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cat() -> Value {
        json!({"apps": [
            {"id": "zelda64recomp", "name": "Zelda 64: Recompiled", "repo": "Zelda64Recomp/Zelda64Recomp"},
            {"id": "banjorecomp", "name": "Banjo: Recompiled", "repo": "BanjoRecomp/BanjoRecomp"},
            {"id": "unleashedrecomp", "name": "Unleashed Recompiled", "repo": "hedge-dev/UnleashedRecomp"},
            {"id": "mk64recomp", "name": "Mario Kart 64 Recomp", "repo": "mk/MK64"},
        ]})
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

    #[test]
    fn reads_a_quiver_library() {
        let d = std::env::temp_dir().join(format!("rl-quiver-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let app = d.join("Apps").join("Zelda64Recomp");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join("Zelda64Recompiled.exe"), b"MZ").unwrap();
        std::fs::write(app.join("version.txt"), "v1.2.2\n").unwrap();
        std::fs::write(app.join("selected_executable.txt"), app.join("Zelda64Recompiled.exe").to_string_lossy().as_bytes()).unwrap();
        std::fs::write(d.join("settings.json"), "{}").unwrap();
        std::fs::write(d.join("apps.json"), r#"{"apps":[{"name":"Zelda 64","repository":"Zelda64Recomp/Zelda64Recomp","folderName":"Zelda64Recomp"},{"name":"Not installed","repository":"a/b","folderName":"Gone"}]}"#).unwrap();
        assert!(is_quiver(&d));
        let apps = quiver_apps(&d);
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].3, "v1.2.2");
        assert_eq!(apps[0].4.as_deref(), Some(Path::new("Zelda64Recompiled.exe")));
        assert_eq!(guess(&cat(), &apps[0].1, &[]).as_deref(), Some("zelda64recomp"));
        let _ = std::fs::remove_dir_all(&d);
    }
}

//! Recomps that only ship a Windows setup program (the catalog's "installer": true). Install
//! downloads it into the launcher's own staging folder, not the library; Set Up runs it when the
//! player presses Run Installer (through ShellExecute, so Windows can ask for administrator
//! rights), suggests a folder in the library, then finds the game where it went and adds it.
//! Updates download the new setup and run it again over the same folder.

use crate::library::Installed;
use crate::{adopt, catalog, install, App, Error, Res};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::State;

/// Set while the player is waiting on a game's installer: Cancel stops the wait.
static WAITING: Mutex<Vec<(String, std::sync::Arc<AtomicBool>)>> = Mutex::new(Vec::new());

/// The catalog says this recomp only comes as an installer.
pub fn is_installer(entry: &Value) -> bool {
    entry.get("installer").and_then(Value::as_bool).unwrap_or(false)
}

/// The catalog has held or flagged this recomp's release: its installer never runs.
pub fn held(entry: &Value) -> bool {
    ["held", "flagged"].iter().any(|k| entry.get(*k).and_then(Value::as_bool).unwrap_or(false))
}

/// Where a game's downloaded setup waits for Set Up.
pub fn staging(app_dir: &Path, id: &str) -> PathBuf {
    app_dir.join("installers").join(crate::library::folder_name(id))
}

/// Puts the downloaded file's setup in `dir`: an archive (.zip, .7z, .rar) is unpacked and the
/// setup inside picked. The setup's path.
pub fn stage(file: &Path, dir: &Path) -> Result<PathBuf, Error> {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir)?;
    let low = file.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    if low.ends_with(".exe") || low.ends_with(".msi") {
        let to = dir.join(file.file_name().unwrap_or_default());
        if std::fs::rename(file, &to).is_err() {
            std::fs::copy(file, &to)?;
        }
        return Ok(to);
    }
    let tmp = dir.with_extension("unpacking");
    let r = install::unpack(file, dir, &tmp);
    let _ = std::fs::remove_dir_all(&tmp);
    r?;
    pick_setup(dir).ok_or_else(|| Error::Msg("The download has no setup program in it.".into()))
}

/// The setup among the files: one named like a setup first, then the biggest program.
fn pick_setup(dir: &Path) -> Option<PathBuf> {
    let mut found: Vec<(PathBuf, u64)> = vec![];
    let mut level = vec![dir.to_path_buf()];
    for _ in 0..3 {
        let mut next = vec![];
        for d in &level {
            for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    next.push(p);
                } else if p.extension().map(|x| x.eq_ignore_ascii_case("exe") || x.eq_ignore_ascii_case("msi")).unwrap_or(false) {
                    found.push((p.clone(), e.metadata().map(|m| m.len()).unwrap_or(0)));
                }
            }
        }
        level = next;
    }
    found.iter().filter(|(p, _)| install::looks_like_setup(p)).max_by_key(|(_, n)| *n).or_else(|| found.iter().max_by_key(|(_, n)| *n)).map(|(p, _)| p.clone())
}

/// Which kind of installer it is, from the names Inno Setup and NSIS leave in their programs:
/// "inno", "nsis", or "" for any other.
pub fn kind(exe: &Path) -> &'static str {
    use std::io::Read;
    let mut buf = vec![];
    if let Ok(f) = std::fs::File::open(exe) {
        let _ = f.take(24 << 20).read_to_end(&mut buf);
    }
    kind_of(&buf)
}

fn kind_of(bytes: &[u8]) -> &'static str {
    let has = |n: &[u8]| bytes.windows(n.len()).any(|w| w == n);
    if has(b"Inno Setup") {
        "inno"
    } else if has(b"Nullsoft") {
        "nsis"
    } else {
        ""
    }
}

/// The options that suggest an install folder: Inno's /DIR="…", NSIS's /D=… (last, no quotes).
pub fn dir_args(kind: &str, dir: &Path) -> String {
    match kind {
        "inno" => format!("/DIR=\"{}\"", dir.display()),
        "nsis" => format!("/D={}", dir.display()),
        _ => String::new(),
    }
}

/// The game's names, for spotting its program and its entry among Windows' apps.
fn names(g: &Installed, entry: &Value) -> Vec<String> {
    let mut n = vec![g.name.clone()];
    for k in ["name", "game"] {
        if let Some(s) = entry.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()) {
            n.push(s.to_string());
        }
    }
    if let Some(r) = g.repo.rsplit(['/', ':']).next().filter(|r| !r.is_empty()) {
        n.push(r.to_string());
    }
    n.dedup();
    n
}

/// Runs a game's waiting installer and waits for it to close, then looks for the game: in the
/// folder it suggested, then where Windows says it went. Only ever from Set Up's Run Installer.
#[tauri::command]
pub async fn installer_run(st: State<'_, App>, id: String) -> Res<Value> {
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't in your Library.".into()))?;
    let setup = PathBuf::from(&g.installer_setup);
    if g.installer_setup.is_empty() || !setup.is_file() {
        return Err(Error::Msg("Its installer isn't downloaded anymore. Press Update or Install to get it again.".into()));
    }
    if !setup.starts_with(staging(&st.dir, &id)) {
        return Err(Error::Msg("That setup isn't the one the launcher downloaded for this game.".into()));
    }
    let cat = crate::cached_catalog(&st);
    let entry = catalog::app(&cat, &id).cloned().unwrap_or(Value::Null);
    if !is_installer(&entry) || held(&entry) {
        return Err(Error::Msg("This release is on hold, so its installer won't run. Try again later.".into()));
    }
    let k = kind(&setup);
    let suggest = g.path.clone();
    let args = dir_args(k, &suggest);
    crate::trail::note(format!("Installer {id}: running {} ({})", setup.file_name().unwrap_or_default().to_string_lossy(), if k.is_empty() { "other" } else { k }));
    let started = std::time::SystemTime::now();
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    WAITING.lock().unwrap().push((id.clone(), stop.clone()));
    let s2 = setup.clone();
    let closed = tokio::task::spawn_blocking(move || run_and_wait(&s2, &args, &stop)).await.map_err(|e| Error::Msg(e.to_string()));
    WAITING.lock().unwrap().retain(|(i, _)| i != &id);
    let closed = closed??;
    let Some(code) = closed else {
        crate::trail::note(format!("Installer {id}: stopped waiting"));
        return Ok(json!({ "stopped": true }));
    };
    crate::trail::note(format!("Installer {id}: closed ({})", code.map(|c| c.to_string()).unwrap_or_else(|| "?".into())));
    let n = names(&g, &entry);
    let mut found = find(&suggest, &g.path, &entry, &n);
    // Not in any folder named for it: where its fresh shortcut or its entry in Settings > Apps
    // points, so a game installed somewhere else doesn't keep asking to be set up.
    if found.is_empty() {
        let nr: Vec<&str> = n.iter().map(String::as_str).collect();
        if let Some((exe, _)) = crate::own_setup::installed_elsewhere(&nr, started, &g.path, None) {
            if let (Some(root), Some(file)) = (exe.parent(), exe.file_name()) {
                crate::trail::note(format!("Installer {id}: found it through its shortcut or its Windows entry"));
                found.push((root.to_path_buf(), PathBuf::from(file)));
            }
        }
    }
    if let [(root, exe)] = found.as_slice() {
        let g = finish(&st, &id, root.clone(), exe.clone())?;
        return Ok(json!({ "done": true, "game": g }));
    }
    Ok(json!({ "done": false, "code": code, "found": found.iter().map(|(p, e)| json!({"path": p, "exe": e})).collect::<Vec<_>>() }))
}

#[cfg(windows)]
fn run_and_wait(setup: &Path, args: &str, stop: &AtomicBool) -> Result<Option<Option<i32>>, Error> {
    let dir = setup.parent().unwrap_or(Path::new("."));
    let p = crate::win::shell_open(setup, args, dir).map_err(|e| {
        if e.raw_os_error() == Some(1223) {
            Error::Msg("Windows' prompt was declined, so the installer didn't run.".into())
        } else {
            Error::Msg(format!("Windows didn't start the installer: {e}"))
        }
    })?;
    Ok(p.wait_or(stop))
}

#[cfg(not(windows))]
fn run_and_wait(_setup: &Path, _args: &str, _stop: &AtomicBool) -> Result<Option<Option<i32>>, Error> {
    Err(Error::Msg("Installers only run on Windows.".into()))
}

/// Stops waiting for a game's installer (the installer's own window stays up).
#[tauri::command(async)]
pub fn installer_cancel(id: String) {
    for (i, s) in WAITING.lock().unwrap().iter() {
        if *i == id {
            s.store(true, Ordering::Relaxed);
        }
    }
}

/// Where the game might be after its installer: (folder, its program) for each place it fits.
fn find(suggest: &Path, current: &Path, entry: &Value, names: &[String]) -> Vec<(PathBuf, PathBuf)> {
    let nr: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut places = vec![suggest.to_path_buf()];
    if current != suggest {
        places.push(current.to_path_buf());
    }
    places.extend(crate::own_setup::app_folders(&nr));
    let mut out: Vec<(PathBuf, PathBuf)> = vec![];
    for d in places.iter().filter(|d| d.is_dir()) {
        let (root, exe, fits) = adopt::check_folder(d, entry, names, "windows");
        if let (Some(e), true) = (exe, fits) {
            if !out.iter().any(|(r, _)| crate::emus::same(r, &root)) {
                out.push((root, e));
            }
        }
    }
    out
}

/// The folder the player says the installer used: the game's folder and program there, or the
/// best guess when it doesn't look like this game.
#[tauri::command]
pub async fn installer_check(st: State<'_, App>, id: String, path: String) -> Res<Value> {
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't in your Library.".into()))?;
    let dir = PathBuf::from(&path);
    if !dir.is_dir() {
        return Err(Error::Msg("That folder couldn't be found.".into()));
    }
    let cat = crate::cached_catalog(&st);
    let entry = catalog::app(&cat, &id).cloned().unwrap_or(Value::Null);
    let n = names(&g, &entry);
    let (root, exe, fits) = tokio::task::spawn_blocking(move || adopt::check_folder(&dir, &entry, &n, "windows")).await.map_err(|e| Error::Msg(e.to_string()))?;
    let names: Vec<String> = names(&g, &catalog::app(&cat, &id).cloned().unwrap_or(Value::Null));
    let exes: Vec<String> = programs(&root, &names.iter().map(String::as_str).collect::<Vec<_>>(), exe.as_deref());
    Ok(json!({ "path": root, "exe": exe, "fits": fits, "exes": exes }))
}

/// The programs in a folder (two levels deep), relative to it, for picking the game by hand: the
/// best guess first, then the biggest. Crash handlers, installers, runtime installers and tools
/// are left out, so the list never opens on crashpad_handler.exe; when nothing else is there, all
/// of them are shown.
fn programs(root: &Path, names: &[&str], guess: Option<&Path>) -> Vec<String> {
    let mut all: Vec<(String, u64, bool)> = vec![];
    let mut level = vec![root.to_path_buf()];
    for _ in 0..2 {
        let mut next = vec![];
        for d in &level {
            for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    next.push(p);
                } else if p.extension().map(|x| x.eq_ignore_ascii_case("exe")).unwrap_or(false) {
                    let rel = p.strip_prefix(root).map(Path::to_path_buf).unwrap_or_default();
                    let n = rel.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
                    if n.starts_with("unins") {
                        continue;
                    }
                    let helper = install::never_the_game(&rel, names);
                    all.push((rel.to_string_lossy().to_string(), e.metadata().map(|m| m.len()).unwrap_or(0), helper));
                }
            }
        }
        level = next;
    }
    if all.iter().any(|(_, _, h)| !h) {
        all.retain(|(_, _, h)| !h);
    }
    let guess = guess.map(|g| g.to_string_lossy().to_string());
    all.sort_by(|a, b| (Some(&b.0) == guess.as_ref()).cmp(&(Some(&a.0) == guess.as_ref())).then(b.1.cmp(&a.1)).then(a.0.cmp(&b.0)));
    all.into_iter().map(|(p, _, _)| p).take(40).collect()
}

/// Done: the game where the installer put it, at the release it installed, with its setup marked
/// done (Play instead of Set Up) and the downloaded setup gone.
#[tauri::command]
pub async fn installer_done(st: State<'_, App>, id: String, path: String, exe: String) -> Res<Installed> {
    let root = PathBuf::from(&path);
    let exe = PathBuf::from(&exe);
    if !root.is_dir() || exe.as_os_str().is_empty() || exe.is_absolute() || !root.join(&exe).is_file() {
        return Err(Error::Msg("Pick the game's program in that folder.".into()));
    }
    finish(&st, &id, root, exe)
}

fn finish(st: &App, id: &str, root: PathBuf, exe: PathBuf) -> Res<Installed> {
    let g = {
        let mut l = st.lib.lock().unwrap();
        let taken = l.installed.values().find(|o| o.id != id && !o.path.as_os_str().is_empty() && (o.path == root || root.starts_with(&o.path) || o.path.starts_with(&root))).map(|o| o.name.clone());
        if let Some(name) = taken {
            return Err(Error::Msg(format!("That's {name}'s folder. Pick the folder this game installed to.")));
        }
        let Some(g) = l.installed.get_mut(id) else { return Err(Error::Msg("It isn't in your Library.".into())) };
        if g.path != root {
            crate::trail::note(format!("Installer {id}: the game is in another folder now"));
        }
        g.path = root;
        g.exe = Some(exe);
        g.exe_rules = u8::MAX;
        g.setup = None;
        g.setup_done = true;
        g.setup_checked = true;
        g.installer = true;
        if !g.installer_tag.is_empty() {
            if g.tag != g.installer_tag {
                g.prev_tag = if g.tag.is_empty() { String::new() } else { g.tag.clone() };
            }
            g.tag = std::mem::take(&mut g.installer_tag);
        }
        g.installer_setup = String::new();
        g.updated_at = crate::library::now();
        let g = g.clone();
        // Setup Required is done for it: the button is Play.
        let ui = l.settings.ui.as_object_mut();
        if let Some(ui) = ui {
            let ok = ui.entry("setup_ok").or_insert_with(|| json!({}));
            if let Some(o) = ok.as_object_mut() {
                o.insert(id.to_string(), json!(true));
            }
        } else {
            l.settings.ui = json!({ "setup_ok": { id: true } });
        }
        g
    };
    st.save();
    let _ = std::fs::remove_dir_all(staging(&st.dir, id));
    crate::trail::note(format!("Installer {id}: added at {}", g.tag));
    Ok(g)
}

/// A game an installer put in place: the uninstaller in its folder (unins000.exe, uninstall.exe,
/// Uninstall*.exe), or the one Windows lists for that folder.
pub fn uninstaller(dir: &Path) -> Option<(PathBuf, String)> {
    let mut list: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| {
        let n = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        n.ends_with(".exe") && (n.starts_with("unins") || n.starts_with("uninstall"))
    }).collect();
    list.sort();
    if let Some(p) = list.into_iter().next() {
        return Some((p, String::new()));
    }
    crate::own_setup::uninstaller_for(dir).map(|cmd| {
        let (prog, args) = split(&cmd);
        (PathBuf::from(prog), args)
    })
}

fn split(cmd: &str) -> (String, String) {
    let c = cmd.trim();
    if let Some(rest) = c.strip_prefix('"') {
        if let Some((p, a)) = rest.split_once('"') {
            return (p.to_string(), a.trim().to_string());
        }
    }
    match c.find(".exe") {
        Some(i) => (c[..i + 4].to_string(), c[i + 4..].trim().to_string()),
        None => (c.to_string(), String::new()),
    }
}

/// Runs a game's uninstaller the way double-clicking it does, and waits for it to close.
pub fn run_uninstaller(dir: &Path) -> bool {
    let Some((prog, args)) = uninstaller(dir) else { return false };
    #[cfg(windows)]
    {
        let cwd = prog.parent().filter(|d| d.is_dir()).map(Path::to_path_buf).unwrap_or_else(std::env::temp_dir);
        match crate::win::shell_open(&prog, &args, &cwd) {
            Ok(p) => {
                let never = AtomicBool::new(false);
                let _ = p.wait_or(&never);
                true
            }
            Err(_) => false,
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (prog, args);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_program_list_opens_on_the_game_not_a_helper() {
        let d = std::env::temp_dir().join(format!("rl-inst-progs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for (f, n) in [("crashpad_handler.exe", 3_000_000), ("NX2navi.exe", 2_000_000), ("unins000.exe", 1_000), ("tools/helper.exe", 9_000_000), ("vc_redist.x64.exe", 20_000_000)] {
            std::fs::create_dir_all(d.join(f).parent().unwrap()).unwrap();
            std::fs::write(d.join(f), vec![0u8; n]).unwrap();
        }
        assert_eq!(programs(&d, &["NX2navi"], None), vec!["NX2navi.exe".to_string()]);
        // The best guess first, even when it's smaller.
        std::fs::write(d.join("Big.exe"), vec![0u8; 5_000_000]).unwrap();
        assert_eq!(programs(&d, &["NX2navi"], Some(Path::new("NX2navi.exe")))[0], "NX2navi.exe");
        assert_eq!(programs(&d, &["NX2navi"], None)[0], "Big.exe");
        // Only helpers there: all of them, so the player can still pick.
        let e = d.join("only");
        std::fs::create_dir_all(&e).unwrap();
        std::fs::write(e.join("crashpad_handler.exe"), b"x").unwrap();
        assert_eq!(programs(&e, &["NX2navi"], None), vec!["crashpad_handler.exe".to_string()]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn tells_installers_apart_and_suggests_the_folder() {
        assert_eq!(kind_of(b"MZ....Inno Setup Setup Data (6.2.0)"), "inno");
        assert_eq!(kind_of(b"MZ....Nullsoft Install System v3.09"), "nsis");
        assert_eq!(kind_of(b"MZ....WiX Toolset"), "");
        let d = Path::new("C:\\Recomp Library\\Crash Course 2");
        assert_eq!(dir_args("inno", d), "/DIR=\"C:\\Recomp Library\\Crash Course 2\"");
        assert_eq!(dir_args("nsis", d), "/D=C:\\Recomp Library\\Crash Course 2");
        assert_eq!(dir_args("", d), "");
        assert_eq!(split("\"C:\\Games\\X\\unins000.exe\" /SILENT"), ("C:\\Games\\X\\unins000.exe".into(), "/SILENT".into()));
        assert_eq!(split("C:\\Games\\X\\uninstall.exe"), ("C:\\Games\\X\\uninstall.exe".into(), String::new()));
    }

    #[test]
    fn stages_the_setup_and_picks_it_from_an_archive() {
        let d = std::env::temp_dir().join(format!("rl-inst-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("dl")).unwrap();
        let f = d.join("dl").join("CC2installer.exe");
        std::fs::write(&f, b"MZ setup").unwrap();
        let s = stage(&f, &d.join("stage")).unwrap();
        assert_eq!(s, d.join("stage").join("CC2installer.exe"));
        assert!(s.is_file());
        // In a folder: the setup-named one wins over a bigger helper.
        std::fs::create_dir_all(d.join("x/sub")).unwrap();
        std::fs::write(d.join("x/sub/TagForce-Setup.exe"), b"MZ s").unwrap();
        std::fs::write(d.join("x/sub/helper.exe"), vec![0u8; 4096]).unwrap();
        let p = pick_setup(&d.join("x")).unwrap();
        assert!(p.ends_with("TagForce-Setup.exe"));
        let _ = std::fs::remove_dir_all(&d);
    }
}

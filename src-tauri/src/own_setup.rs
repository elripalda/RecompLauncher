//! Recomps whose release isn't the game yet: an installer (MaddenNFL08-Recompiled-Setup.exe) that
//! builds or puts the game somewhere else, or a first-time setup beside the game
//! (Tenkaichi3Decomp-setup.exe) that unpacks the game's files from the player's copy. The first
//! Play runs it. When it closes, the launcher looks for the game: in its own folder, then (for an
//! installer) where Windows says the installer put it, by its entry in Settings > Apps and the
//! shortcuts it made.

use crate::install;
use crate::library::Installed;
use crate::{App, Error, Res};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;
use tauri::{AppHandle, Emitter, Manager};

/// Games whose setup is open now.
static OPEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn names(g: &Installed) -> Vec<String> {
    let mut n = vec![g.name.clone()];
    if let Some(r) = g.repo.rsplit('/').next().filter(|r| !r.is_empty()) {
        n.push(r.to_string());
    }
    n
}

/// The setup to run before this Play, if one still has to. Installs from before the launcher
/// knew about setups are looked at once here.
pub fn pending(st: &App, id: &str, g: &Installed) -> Option<PathBuf> {
    if g.local {
        return None;
    }
    let mut g = g.clone();
    if !g.setup_checked || g.exe_rules < install::EXE_RULES {
        let os = crate::assets::os_now();
        let n = names(&g);
        let nr: Vec<&str> = n.iter().map(String::as_str).collect();
        // The program, picked again by this version's rules: a game's own launcher goes ahead of
        // its engine (CTR Nitro-Fueled, BotW Native), and "Crash" in a game's name doesn't read as
        // a crash handler. One outside its folder, or one the player picked, stays.
        let outside = g.exe.as_ref().map(|e| e.is_absolute()).unwrap_or(false);
        let boot = crate::boot::of(&st.dir, id);
        if !outside && g.exe_rules != u8::MAX {
            if let Some(e) = boot.as_ref().and_then(|b| b.start_in(&g.path)).or_else(|| install::find_exe(&g.path, os, &nr)) {
                // Launch options were for the program it started before (a path to the game's files
                // for its engine, say): its launcher doesn't take them.
                if g.exe.as_ref().map(|old| *old != e).unwrap_or(false) {
                    g.args = String::new();
                }
                g.exe = Some(e);
            } else if !g.exe.as_ref().map(|e| g.path.join(e).is_file() && !install::looks_like_setup(e)).unwrap_or(false) {
                g.exe = None;
            }
        }
        let inside = g.exe.clone().filter(|e| !e.is_absolute());
        let setup = match &boot {
            Some(b) if !b.is_installer() => b.setup_in(&g.path),
            _ => install::find_setup(&g.path, os, inside.as_deref()),
        };
        if !g.setup_checked {
            // Played for a while already: its setup ran then. A game that only ever closed right
            // away probably needed it.
            g.setup_done = setup.is_some() && g.played_secs >= 120 && inside.is_some();
        } else if setup != g.setup {
            g.setup_done = false;
        }
        g.setup = setup;
        g.setup_checked = true;
        if g.exe_rules != u8::MAX {
            g.exe_rules = install::EXE_RULES;
        }
        {
            let mut l = st.lib.lock().unwrap();
            let Some(r) = l.installed.get_mut(id) else { return None };
            r.exe = g.exe.clone();
            r.args = g.args.clone();
            r.setup = g.setup.clone();
            r.setup_done = g.setup_done;
            r.setup_checked = true;
            r.exe_rules = g.exe_rules;
        }
        st.save();
    }
    if g.setup_done {
        return None;
    }
    g.setup.as_ref().map(|s| g.path.join(s)).filter(|p| p.is_file())
}

/// Starts a program the way double-clicking it would: an installer that asks for administrator
/// rights gets Windows' prompt instead of failing.
pub fn spawn(p: &Path) -> std::io::Result<std::process::Child> {
    let dir = p.parent().map(Path::to_path_buf).unwrap_or_default();
    match std::process::Command::new(p).current_dir(&dir).spawn() {
        #[cfg(windows)]
        Err(e) if e.raw_os_error() == Some(740) => {
            use std::os::windows::process::CommandExt;
            let q = |s: &Path| s.to_string_lossy().replace('\'', "''");
            let script = format!("$p = Start-Process -FilePath '{}' -WorkingDirectory '{}' -Verb RunAs -Wait -PassThru; exit $p.ExitCode", q(p), q(&dir));
            std::process::Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", &script]).creation_flags(0x0800_0000).spawn()
        }
        r => r,
    }
}

fn tell(app: &AppHandle, id: &str, name: &str, state: &str) {
    let _ = app.emit("first-setup", json!({"id": id, "name": name, "state": state}));
}

/// Runs the setup and, once it closes, finds the game.
pub fn run(app: &AppHandle, id: &str, g: &Installed, setup: &Path) -> Res<()> {
    {
        let mut o = OPEN.lock().unwrap();
        if o.iter().any(|x| x == id) {
            return Err(Error::Msg(format!("{}'s setup is still open. Finish it, then press Play.", g.name)));
        }
        o.push(id.to_string());
    }
    let since = SystemTime::now();
    let shown = setup.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let mut child = match spawn(setup) {
        Ok(c) => c,
        Err(e) => {
            OPEN.lock().unwrap().retain(|x| x != id);
            return Err(if crate::is_blocked(&e) { crate::defender_msg(e, &shown) } else { Error::Msg(format!("Couldn't start {shown}: {e}")) });
        }
    };
    tell(app, id, &g.name, "open");
    let from_bp = crate::BIG_PICTURE.load(std::sync::atomic::Ordering::Relaxed);
    if from_bp {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.minimize();
        }
    }
    let (app, id, name) = (app.clone(), id.to_string(), g.name.clone());
    std::thread::spawn(move || {
        let code = child.wait().ok().and_then(|s| s.code());
        let state = finish(&app, &id, code, since);
        OPEN.lock().unwrap().retain(|x| x != &id);
        if from_bp {
            crate::show_main(&app);
        }
        tell(&app, &id, &name, state);
    });
    Ok(())
}

/// After the setup: "done" (the game is there to play), "lost" (it finished, but the game wasn't
/// found: the player picks its program) or "cancelled" (it didn't finish; the next Play opens it
/// again).
fn finish(app: &AppHandle, id: &str, code: Option<i32>, since: SystemTime) -> &'static str {
    let st = app.state::<App>();
    let Some(mut g) = st.lib.lock().unwrap().installed.get(id).cloned() else { return "cancelled" };
    let os = crate::assets::os_now();
    let n = names(&g);
    let nr: Vec<&str> = n.iter().map(String::as_str).collect();
    let usable = |g: &Installed| g.exe.as_ref().map(|e| g.path.join(e).is_file() && !install::looks_like_setup(e)).unwrap_or(false);
    let installer = g.exe.as_ref().map(|e| e.is_absolute()).unwrap_or(true);
    if !usable(&g) {
        g.exe = install::find_exe(&g.path, os, &nr);
    }
    // An installer: where it put the game. Looked at again after an update too, in case it moved.
    if installer && (!usable(&g) || g.exe.as_ref().map(|e| e.is_absolute()).unwrap_or(false)) {
        let then = crate::boot::of(&st.dir, id).and_then(|b| b.then_start());
        if let Some((exe, args)) = installed_elsewhere(&nr, since, &g.path, then.as_deref()) {
            g.exe = Some(exe);
            if g.args.trim().is_empty() {
                g.args = args;
            }
        }
    }
    let found = usable(&g);
    let state = if found { "done" } else if code == Some(0) { "lost" } else { "cancelled" };
    {
        let mut l = st.lib.lock().unwrap();
        if let Some(r) = l.installed.get_mut(id) {
            r.exe = g.exe.clone();
            r.args = g.args.clone();
            r.setup_done = state != "cancelled";
        }
    }
    st.save();
    state
}

/// Name words that say nothing about which game it is.
const GENERIC: [&str; 18] = ["recomp", "recompiled", "decomp", "decompiled", "port", "the", "launcher", "online", "edition", "native", "windows", "game", "x64", "x86", "bit", "version", "beta", "alpha"];

fn words(s: &str) -> Vec<String> {
    install::name_tokens(s).into_iter().filter(|t| (t.len() > 2 || (t.len() == 2 && t.chars().all(|c| c.is_ascii_digit()))) && !GENERIC.contains(&t.as_str())).collect()
}

/// Whether an app's or shortcut's name is this game's: every word of it is in the game's names,
/// and enough of them ("Madden NFL 08" for Madden NFL 08 Recompiled, not Madden NFL 25).
fn is_ours(want: &[String], name: &str) -> bool {
    let have = words(name);
    if have.is_empty() || have.iter().any(|h| h.starts_with("unins")) {
        return false;
    }
    let hit = |h: &String| want.iter().any(|w| w == h || (h.len() > 3 && w.starts_with(h.as_str())) || (w.len() > 3 && h.starts_with(w.as_str())));
    let hits = have.iter().filter(|h| hit(h)).count();
    hits == have.len() && hits >= want.len().min(2)
}

fn want_of(names: &[&str]) -> Vec<String> {
    let mut w: Vec<String> = names.iter().flat_map(|n| words(n)).collect();
    w.sort();
    w.dedup();
    w
}

/// Where an installer put the game: (program, its options). Its fresh shortcuts first, then its
/// entry in Settings > Apps, then older shortcuts.
#[cfg(windows)]
fn installed_elsewhere(names: &[&str], since: SystemTime, ours: &Path, then: Option<&Path>) -> Option<(PathBuf, String)> {
    use std::os::windows::process::CommandExt;
    const PS: &str = r#"$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$o = @()
foreach ($k in 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*', 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*', 'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*') {
  Get-ItemProperty $k | Where-Object { $_.DisplayName } | ForEach-Object { $o += [pscustomobject]@{ k = 'app'; n = [string]$_.DisplayName; p = [string]$_.InstallLocation; i = [string]$_.DisplayIcon; u = [string]$_.UninstallString } }
}
$w = New-Object -ComObject WScript.Shell
foreach ($d in [Environment]::GetFolderPath('Desktop'), [Environment]::GetFolderPath('CommonDesktopDirectory'), [Environment]::GetFolderPath('Programs'), [Environment]::GetFolderPath('CommonPrograms')) {
  if (-not $d) { continue }
  Get-ChildItem -LiteralPath $d -Filter *.lnk -Recurse -Depth 2 | ForEach-Object {
    $s = $w.CreateShortcut($_.FullName)
    $o += [pscustomobject]@{ k = 'lnk'; n = $_.BaseName; p = [string]$s.TargetPath; a = [string]$s.Arguments; t = [int64](($_.LastWriteTimeUtc - [datetime]'1970-01-01').TotalSeconds) }
  }
}
ConvertTo-Json -InputObject @($o) -Compress"#;
    let file = std::env::temp_dir().join("recomplauncher-find-install.ps1");
    // A BOM so Windows PowerShell reads the script as UTF-8.
    std::fs::write(&file, [&[0xEF, 0xBB, 0xBF][..], PS.as_bytes()].concat()).ok()?;
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&file)
        .creation_flags(0x0800_0000)
        .output()
        .ok()?;
    let _ = std::fs::remove_file(&file);
    let text = String::from_utf8_lossy(&out.stdout);
    let list: Vec<Value> = serde_json::from_str(text.trim().trim_start_matches('\u{feff}')).ok()?;
    pick(&list, names, since, ours, then)
}

#[cfg(not(windows))]
fn installed_elsewhere(_names: &[&str], _since: SystemTime, _ours: &Path, _then: Option<&Path>) -> Option<(PathBuf, String)> {
    None
}

/// The game among Windows' apps and shortcuts (see installed_elsewhere).
fn pick(list: &[Value], names: &[&str], since: SystemTime, ours: &Path, then: Option<&Path>) -> Option<(PathBuf, String)> {
    let want = want_of(names);
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
    let me = std::env::current_exe().ok();
    let since = since.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0) - 30;
    // A shortcut's program: a program or launch script, not an uninstaller, not ours, not the
    // installer the launcher just ran.
    let good = |p: &Path| {
        let low = p.to_string_lossy().to_lowercase();
        p.is_file() && (low.ends_with(".exe") || low.ends_with(".bat") || low.ends_with(".cmd")) && !p.starts_with(ours) && Some(p) != me.as_deref() && !install::looks_like_setup(p) && !p.file_stem().map(|n| n.to_string_lossy().to_lowercase().starts_with("unins")).unwrap_or(false)
    };
    let lnk = |fresh: bool| {
        list.iter()
            .filter(|v| s(v, "k") == "lnk" && is_ours(&want, &s(v, "n")))
            .filter(|v| (v.get("t").and_then(Value::as_i64).unwrap_or(0) >= since) == fresh)
            .map(|v| (PathBuf::from(s(v, "p")), s(v, "a")))
            .find(|(p, _)| good(p))
    };
    // The boot profile says where the game starts in the folder the installer used: that first.
    if let Some(t) = then {
        for v in list.iter().filter(|v| s(v, "k") == "app" && is_ours(&want, &s(v, "n"))) {
            let p = PathBuf::from(s(v, "p")).join(t);
            if !s(v, "p").is_empty() && p.is_file() && !p.starts_with(ours) {
                return Some((p, String::new()));
            }
        }
        for v in list.iter().filter(|v| s(v, "k") == "lnk" && is_ours(&want, &s(v, "n"))) {
            let target = PathBuf::from(s(v, "p"));
            for up in target.ancestors().skip(1).take(4) {
                let p = up.join(t);
                if p.is_file() && !p.starts_with(ours) {
                    return Some((p, String::new()));
                }
            }
        }
    }
    if let Some(x) = lnk(true) {
        return Some(x);
    }
    for v in list.iter().filter(|v| s(v, "k") == "app" && is_ours(&want, &s(v, "n"))) {
        let from_path = |raw: String| {
            let raw = raw.trim().to_string();
            let p = if let Some(r) = raw.strip_prefix('"') { r.split('"').next().unwrap_or("").to_string() } else { raw.split(',').next().unwrap_or("").to_string() };
            let p = PathBuf::from(p.trim());
            if p.is_file() { p.parent().map(Path::to_path_buf) } else { None }
        };
        let dir = Some(PathBuf::from(s(v, "p"))).filter(|d| !d.as_os_str().is_empty() && d.is_dir()).or_else(|| from_path(s(v, "i"))).or_else(|| from_path(s(v, "u")));
        let Some(dir) = dir else { continue };
        if dir.starts_with(ours) {
            continue;
        }
        if let Some(rel) = install::find_exe(&dir, "windows", names) {
            let p = dir.join(rel);
            if good(&p) {
                return Some((p, String::new()));
            }
        }
    }
    lnk(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_its_own_apps() {
        let want = want_of(&["Madden NFL 08 Recompiled", "MaddenNFL08-Recompiled"]);
        assert!(is_ours(&want, "Madden NFL 08 Recompiled"));
        assert!(is_ours(&want, "Madden NFL 08"));
        assert!(!is_ours(&want, "Madden NFL 25"));
        assert!(!is_ours(&want, "Uninstall Madden NFL 08"));
        assert!(!is_ours(&want, "Recompiled"));
        let want = want_of(&["SMS Launcher", "sms-launcher"]);
        assert!(is_ours(&want, "SMS Launcher"));
        assert!(!is_ours(&want, "SMS Backup"));
    }

    #[test]
    fn finds_where_it_went() {
        let d = std::env::temp_dir().join(format!("rl-own-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let game = d.join("Madden08");
        std::fs::create_dir_all(game.join("dist")).unwrap();
        std::fs::write(game.join("dist/madden08.exe"), vec![1u8; 4000]).unwrap();
        std::fs::write(game.join("dist/Launch Madden NFL 08.bat"), b"@echo off").unwrap();
        std::fs::write(game.join("unins000.exe"), b"x").unwrap();
        let lib = d.join("lib/maddennfl08-recompiled");
        std::fs::create_dir_all(&lib).unwrap();
        let names = ["Madden NFL 08 Recompiled", "MaddenNFL08-Recompiled"];
        let unins = game.join("unins000.exe").to_string_lossy().to_string();
        let list = vec![
            json!({"k": "app", "n": "Madden NFL 25", "p": d.to_string_lossy()}),
            json!({"k": "app", "n": "Madden NFL 08 Recompiled", "p": "", "i": "", "u": format!("\"{unins}\" /S")}),
            json!({"k": "lnk", "n": "Uninstall Madden NFL 08", "p": unins, "a": "", "t": 0}),
        ];
        let (exe, _) = pick(&list, &names, SystemTime::now(), &lib, None).unwrap();
        assert_eq!(exe, game.join("dist/Launch Madden NFL 08.bat"));
        // Its fresh desktop shortcut wins, options and all.
        let mut list = list;
        let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();
        list.push(json!({"k": "lnk", "n": "Madden NFL 08", "p": game.join("dist/madden08.exe").to_string_lossy(), "a": "--render_target_path_d3d12=rov", "t": now}));
        let (exe, args) = pick(&list, &names, SystemTime::now(), &lib, None).unwrap();
        assert_eq!(exe, game.join("dist/madden08.exe"));
        assert_eq!(args, "--render_target_path_d3d12=rov");
        let _ = std::fs::remove_dir_all(&d);
    }
}

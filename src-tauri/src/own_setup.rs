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
        // A game whose installer builds it (its profile names the program it makes): a program in
        // the download that isn't that one is a tool, not the game, so the setup runs first.
        let mut redo = false;
        if let Some(want) = boot.as_ref().and_then(|b| b.then_start()).and_then(|t| t.file_name().map(|n| n.to_string_lossy().to_lowercase())) {
            let wrong = |e: &PathBuf| e.file_name().map(|n| n.to_string_lossy().to_lowercase() != want).unwrap_or(true);
            if g.exe.as_ref().map(|e| wrong(e) || !g.path.join(e).is_file()).unwrap_or(false) || (g.exe.is_none() && g.setup_done) {
                // Left on a tool from the download (an older launcher's guess): the game it built,
                // if it can be found, else its setup again (which picks up where it got to).
                let since = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(g.installed_at);
                let near: Vec<PathBuf> = [Some(g.path.clone()), g.path.parent().map(Path::to_path_buf)].into_iter().flatten().collect();
                match find_built(&want, since, &near) {
                    Some(p) => g.exe = Some(p),
                    None => {
                        g.exe = None;
                        redo = true;
                    }
                }
            }
        }
        let inside = g.exe.clone().filter(|e| !e.is_absolute());
        let setup = match &boot {
            Some(b) if !b.is_installer() => b.setup_in(&g.path),
            Some(b) => b.setup_near(&g.path).or_else(|| install::find_setup(&g.path, os, inside.as_deref())),
            None => install::find_setup(&g.path, os, inside.as_deref()),
        };
        if redo {
            g.setup_done = false;
        } else if !g.setup_checked {
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
    // Set up, but the game it builds wasn't found then (the setup was still going, say): looked
    // for again, and when it's nowhere, its setup opens again to finish.
    if g.setup_done && g.exe.is_none() {
        if let Some(want) = crate::boot::of(&st.dir, id).and_then(|b| b.then_name()) {
            let since = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(g.installed_at);
            let mut near: Vec<PathBuf> = [Some(g.path.clone()), g.path.parent().map(Path::to_path_buf)].into_iter().flatten().collect();
            near.extend(player_places(st));
            let found = find_built(&want, since, &near);
            {
                let mut l = st.lib.lock().unwrap();
                if let Some(r) = l.installed.get_mut(id) {
                    match &found {
                        Some(p) => r.exe = Some(p.clone()),
                        None => r.setup_done = false,
                    }
                }
            }
            st.save();
            if found.is_some() {
                return None;
            }
            g.setup_done = false;
        }
    }
    // Its setup ran, but what it makes isn't there (closed early, or the disc wasn't picked): it
    // hasn't finished, so it opens again instead of the game starting only to quit.
    if g.setup_done && g.setup.is_some() && crate::boot::of(&st.dir, id).and_then(|b| b.not_made(&g.path)).is_some() {
        if let Some(r) = st.lib.lock().unwrap().installed.get_mut(id) {
            r.setup_done = false;
        }
        st.save();
        g.setup_done = false;
    }
    if g.setup_done {
        return None;
    }
    g.setup.as_ref().map(|s| g.path.join(s)).filter(|p| p.is_file())
}

/// Starts a program the way double-clicking it would: an installer that asks for administrator
/// rights gets Windows' prompt instead of failing.
pub fn spawn(p: &Path) -> std::io::Result<std::process::Child> {
    spawn_with(p, None)
}

/// `copy`: the player's copy of the game, handed to the setup as if dropped on it.
pub fn spawn_with(p: &Path, copy: Option<&Path>) -> std::io::Result<std::process::Child> {
    let dir = p.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut c = std::process::Command::new(p);
    crate::sysenv::clean_std(&mut c);
    if let Some(x) = copy {
        c.arg(x);
    }
    match c.current_dir(&dir).spawn() {
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
pub fn run(app: &AppHandle, id: &str, g: &Installed, setup: &Path, copy: Option<&Path>) -> Res<()> {
    {
        let mut o = OPEN.lock().unwrap();
        if o.iter().any(|x| x == id) {
            return Err(Error::Msg(format!("{}'s setup is still open. Finish it, then press Play.", g.name)));
        }
        o.push(id.to_string());
    }
    let since = SystemTime::now();
    let shown = setup.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let mut child = match spawn_with(setup, copy) {
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
    let folder = g.path.clone();
    std::thread::spawn(move || {
        let code = child.wait().ok().and_then(|s| s.code());
        // A setup that's only a starter (TotK-Setup.exe opens its Python window and quits): the
        // setup isn't over until every program from the download it started has closed too.
        let places = wait_for_rest(&folder);
        let state = finish(&app, &id, code, since, &places);
        OPEN.lock().unwrap().retain(|x| x != &id);
        if from_bp {
            crate::show_main(&app);
        }
        tell(&app, &id, &name, state);
    });
    Ok(())
}

/// Waits while any program from `dir` (the game's download) is still running: what a setup that
/// only starts another program goes on doing after its own program has quit. Windows only.
/// Returns the folders those programs were given on their command lines (TotK's setup starts its
/// builder with `--dest C:\Games\TOTK`): where the player told it to put the game.
fn wait_for_rest(dir: &Path) -> Vec<String> {
    let mut said: Vec<String> = Vec::new();
    #[cfg(windows)]
    {
        // A moment for what it started to show up.
        std::thread::sleep(std::time::Duration::from_secs(2));
        loop {
            let (n, lines) = running_from(dir);
            for l in lines {
                for a in paths_in(&l) {
                    if !said.contains(&a) {
                        said.push(a);
                    }
                }
            }
            if n == 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(4));
        }
    }
    #[cfg(not(windows))]
    let _ = dir;
    said
}

/// The programs running from inside `dir`: how many, and their command lines.
#[cfg(windows)]
fn running_from(dir: &Path) -> (usize, Vec<String>) {
    use std::os::windows::process::CommandExt;
    let d = dir.to_string_lossy().trim_end_matches('\\').replace('\'', "''");
    let script = format!("[Console]::OutputEncoding = [Text.Encoding]::UTF8; $d = '{d}\\'; $p = @(Get-CimInstance Win32_Process | Where-Object {{ $_.ExecutablePath -and $_.ExecutablePath.StartsWith($d, [StringComparison]::OrdinalIgnoreCase) }}); $p.Count; $p | ForEach-Object {{ [string]$_.CommandLine }}");
    let Ok(o) = std::process::Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", &script]).creation_flags(0x0800_0000).output() else { return (0, vec![]) };
    let text = String::from_utf8_lossy(&o.stdout).to_string();
    let mut lines = text.lines();
    let n = lines.next().and_then(|l| l.trim().parse().ok()).unwrap_or(0);
    (n, lines.map(str::to_string).collect())
}

/// The Windows paths in a command line (`C:\x`, quoted or not, also after `--dest=`).
fn paths_in(line: &str) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in line.chars() {
        match c {
            '"' => quoted = !quoted,
            ' ' | '\t' if !quoted => {
                if !cur.is_empty() {
                    args.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() {
        args.push(cur);
    }
    args.into_iter()
        .skip(1)
        .map(|a| a.rsplit_once('=').map(|(_, v)| v.to_string()).unwrap_or(a))
        .filter(|a| {
            let b = a.as_bytes();
            b.len() > 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
        })
        .collect()
}

/// Games in the Library waiting for the program their installer builds (TotK's totk.exe), found in
/// `dir` (a folder the player picked) a few folders down: from now on Play starts it from there.
/// Returns (game, folder, program) for each one found.
pub fn attach_built(st: &App, dir: &Path) -> Vec<(String, PathBuf, PathBuf)> {
    let waiting: Vec<(String, String)> = {
        let l = st.lib.lock().unwrap();
        l.installed
            .values()
            .filter(|g| !g.local && !g.exe.as_ref().map(|e| g.path.join(e).is_file()).unwrap_or(false))
            .filter_map(|g| crate::boot::of(&st.dir, &g.id).and_then(|b| b.then_name()).map(|w| (g.id.clone(), w)))
            .collect()
    };
    let mut out = vec![];
    for (id, want) in waiting {
        let mut stack = vec![(dir.to_path_buf(), 0usize)];
        let mut hit: Option<PathBuf> = None;
        while let Some((d, depth)) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let Ok(t) = e.file_type() else { continue };
                let n = e.file_name().to_string_lossy().to_lowercase();
                if t.is_dir() && depth < 3 && !n.starts_with('.') {
                    stack.push((e.path(), depth + 1));
                } else if t.is_file() && n == want {
                    hit = Some(e.path());
                }
            }
            if hit.is_some() {
                break;
            }
        }
        if let Some(p) = hit {
            {
                let mut l = st.lib.lock().unwrap();
                if let Some(r) = l.installed.get_mut(&id) {
                    r.exe = Some(p.clone());
                    r.setup_done = true;
                }
            }
            st.save();
            let folder = p.parent().map(Path::to_path_buf).unwrap_or_default();
            out.push((id, folder, p));
        }
    }
    out
}

/// Where the player keeps games: their Recomp Library folders and the ROM folders they added.
fn player_places(st: &App) -> Vec<PathBuf> {
    let l = st.lib.lock().unwrap();
    let s = &l.settings;
    let mut v = vec![s.library_dir.clone()];
    v.extend(s.library_dirs.iter().cloned());
    v.extend(s.rom_dirs.iter().cloned());
    v.into_iter().filter(|p| !p.as_os_str().is_empty()).collect()
}

/// After the setup: "done" (the game is there to play), "lost" (it finished, but the game wasn't
/// found: the player picks its program) or "cancelled" (it didn't finish; the next Play opens it
/// again).
fn finish(app: &AppHandle, id: &str, code: Option<i32>, since: SystemTime, said: &[String]) -> &'static str {
    let st = app.state::<App>();
    let Some(mut g) = st.lib.lock().unwrap().installed.get(id).cloned() else { return "cancelled" };
    let os = crate::assets::os_now();
    let n = names(&g);
    let nr: Vec<&str> = n.iter().map(String::as_str).collect();
    let want = crate::boot::of(&st.dir, id).and_then(|b| b.then_start()).and_then(|t| t.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    let usable = |g: &Installed| {
        g.exe.as_ref().map(|e| g.path.join(e).is_file() && !install::looks_like_setup(e)).unwrap_or(false)
            && want.as_ref().map(|w| g.exe.as_ref().and_then(|e| e.file_name()).map(|n| n.to_string_lossy().to_lowercase() == *w).unwrap_or(false)).unwrap_or(true)
    };
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
    // An installer that builds the game in a folder the player picked, with no shortcut or entry in
    // Windows' apps: the program it names is looked for where it may have gone.
    if !usable(&g) {
        if let Some(w) = want.as_deref() {
            // First where the setup was told to put it, then the game's folder, the player's
            // library and ROM folders.
            let mut near: Vec<PathBuf> = Vec::new();
            for a in said {
                let p = PathBuf::from(a);
                if p.is_dir() {
                    near.push(p);
                } else if let Some(d) = p.parent().filter(|d| d.is_dir()) {
                    near.push(d.to_path_buf());
                }
            }
            near.extend([Some(g.path.clone()), g.path.parent().map(Path::to_path_buf)].into_iter().flatten());
            near.extend(player_places(&st));
            if let Some(p) = find_built(w, since, &near) {
                g.exe = Some(p);
            }
        }
    }
    // A program that isn't the one the game's profile names is a tool from the download, never
    // the game: rather than start it, the player is asked to pick the game.
    if want.is_some() && !usable(&g) {
        g.exe = None;
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

/// The program a game's installer builds (its profile's `then_start`, like `totk.exe`), made since
/// `since`, wherever the player told the installer to put it: looked for under the game's folder,
/// its library, the player's usual folders and the top of each drive. Time- and size-limited.
pub fn find_built(file: &str, since: SystemTime, near: &[PathBuf]) -> Option<PathBuf> {
    let want = file.to_lowercase();
    let since = since.checked_sub(std::time::Duration::from_secs(120)).unwrap_or(since);
    let until = std::time::Instant::now() + std::time::Duration::from_secs(12);
    let mut roots: Vec<(PathBuf, usize)> = near.iter().map(|p| (p.clone(), 4)).collect();
    for q in crate::files::quick() {
        roots.push((q.path, 3));
    }
    let drives = crate::files::drives();
    // Program Files, which the drive-wide look skips: a couple of folders down.
    for d in &drives {
        for pf in ["Program Files", "Program Files (x86)"] {
            roots.push((d.path.join(pf), 2));
        }
    }
    for d in drives {
        roots.push((d.path, 3));
    }
    let skip = ["windows", "program files", "program files (x86)", "programdata", "$recycle.bin", "system volume information", "appdata", "node_modules", ".git"];
    let mut seen = std::collections::HashSet::new();
    let mut looked = 0usize;
    let mut best: Option<(SystemTime, PathBuf)> = None;
    for (root, depth) in roots {
        let mut stack = vec![(root, 0usize)];
        while let Some((dir, d)) = stack.pop() {
            if std::time::Instant::now() > until || looked > 400_000 {
                return best.map(|b| b.1);
            }
            let key = dir.to_string_lossy().to_lowercase();
            if !seen.insert(key) {
                continue;
            }
            for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                looked += 1;
                let name = e.file_name().to_string_lossy().to_lowercase();
                let Ok(ft) = e.file_type() else { continue };
                if ft.is_dir() {
                    if d < depth && !skip.contains(&name.as_str()) && !name.starts_with('.') {
                        stack.push((e.path(), d + 1));
                    }
                } else if name == want {
                    let made = e.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
                    if made >= since && best.as_ref().map(|b| made > b.0).unwrap_or(true) {
                        best = Some((made, e.path()));
                    }
                }
            }
        }
    }
    best.map(|b| b.1)
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

/// The uninstaller Windows lists for a game an installer put in `dir` (its install folder holds
/// the game's program): the command to run, as Windows keeps it.
#[cfg(windows)]
pub fn uninstaller_for(dir: &Path) -> Option<String> {
    use std::os::windows::process::CommandExt;
    const PS: &str = r#"$o = @()
foreach ($k in 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*', 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*', 'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*') {
  Get-ItemProperty $k -ErrorAction SilentlyContinue | Where-Object { $_.UninstallString } | ForEach-Object { $o += [pscustomobject]@{ p = [string]$_.InstallLocation; i = [string]$_.DisplayIcon; u = [string]$_.UninstallString } }
}
ConvertTo-Json -InputObject @($o) -Compress"#;
    let file = std::env::temp_dir().join("recomplauncher-find-uninstall.ps1");
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
    pick_uninstaller(&list, dir)
}

#[cfg(not(windows))]
pub fn uninstaller_for(_dir: &Path) -> Option<String> {
    None
}

/// The entry whose install folder (or icon, or uninstaller) is the game's folder.
fn pick_uninstaller(list: &[Value], dir: &Path) -> Option<String> {
    let norm = |s: &str| s.trim().trim_matches('"').split(',').next().unwrap_or("").replace('/', "\\").trim_end_matches('\\').to_lowercase();
    let want = norm(&dir.to_string_lossy());
    if want.len() < 4 {
        return None;
    }
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    list.iter()
        .find(|v| {
            let loc = norm(&s(v, "p"));
            let in_dir = |f: &str| {
                let f = norm(f);
                f.rsplit_once('\\').map(|(d, _)| d == want).unwrap_or(false)
            };
            (!loc.is_empty() && loc == want) || in_dir(&s(v, "i")) || in_dir(&s(v, "u"))
        })
        .map(|v| s(v, "u"))
        .filter(|u| !u.trim().is_empty())
}

/// Starts a game's own uninstaller (what Windows lists for it); the player goes through its window.
pub fn run_uninstaller(cmd: &str) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // As Windows keeps it: a quoted program with its options, or MsiExec /X{...}.
        std::process::Command::new("cmd").raw_arg(format!("/C \"{cmd}\"")).creation_flags(0x0800_0000).spawn()?;
    }
    #[cfg(not(windows))]
    let _ = cmd;
    Ok(())
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

#[cfg(test)]
mod place_tests {
    use super::*;

    #[test]
    fn finds_the_folder_a_setup_was_told() {
        let l = r#""C:\Downloads\TotK-Setup\python\python.exe" -m installer --base "D:\Dumps\TotK base.xci" --update D:\Dumps\upd.nsp --lang en --dest "E:\My Games\TOTK" --flag=F:\x"#;
        assert_eq!(paths_in(l), vec!["D:\\Dumps\\TotK base.xci", "D:\\Dumps\\upd.nsp", "E:\\My Games\\TOTK", "F:\\x"]);
        assert!(paths_in("pythonw.exe -m installer.gui").is_empty());
    }
}

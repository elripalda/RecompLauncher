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
    if let Some(r) = g.repo.rsplit(['/', ':']).next().filter(|r| !r.is_empty()) {
        n.push(r.to_string());
    }
    n
}

/// The setup that has to run before the game starts, if one does: asked by shortcuts and the
/// game's page. It leaves the once-only reasons (an update, the game asking) for Play.
pub fn pending(st: &App, id: &str, g: &Installed) -> Option<PathBuf> {
    pending_with(st, id, g, false)
}

/// The setup to run before this Play, if one still has to. A once-only reason is marked as
/// answered here, so a setup closed before it finished isn't asked for again: the version that
/// played before plays.
pub fn pending_for_play(st: &App, id: &str, g: &Installed) -> Option<PathBuf> {
    pending_with(st, id, g, true)
}

/// Played this long with its program, a game's program is the right one.
const PLAYED_A_WHILE: u64 = 120;

/// Whether the program a game has stays when the launcher can't pick one again itself: one that
/// can start and isn't a setup or tool. An older version's guess between programs that don't name
/// the game doesn't, unless the game has played a while with it: the player is asked once instead.
fn keeps_old_program(usable: bool, unclear: bool, played_secs: u64) -> bool {
    let unproven_guess = unclear && played_secs < PLAYED_A_WHILE;
    usable && !unproven_guess
}

/// Installs from before the launcher knew about setups are looked at once here.
fn pending_with(st: &App, id: &str, g: &Installed, play: bool) -> Option<PathBuf> {
    if g.local {
        return None;
    }
    let mut g = g.clone();
    // How the catalog's profile starts it changed since the last Play (a fixed start program, say):
    // the program is picked again by it. The player's own pick, saves and settings stay.
    let hash = {
        let cat = crate::cached_catalog(st);
        crate::catalog::app(&cat, id).map(crate::boot::start_hash_of).unwrap_or_default()
    };
    let new_profile = !hash.is_empty() && hash != g.boot_hash;
    if new_profile {
        if !g.boot_hash.is_empty() && g.exe_rules != u8::MAX {
            crate::trail::note(format!("Play {id}: its profile changed, its program is picked again"));
            g.exe_rules = 0;
        }
        g.boot_hash = hash.clone();
        if let Some(r) = st.lib.lock().unwrap().installed.get_mut(id) {
            r.boot_hash = hash;
        }
        st.save();
    }
    // Left on the release's own installer as if it were the game (picked by hand when it was the
    // only program there): it goes back to being the setup, which finds the game it installs.
    let own_installer = g.exe.as_ref().map(|e| !e.is_absolute() && e.file_name().map(|n| n.to_string_lossy().eq_ignore_ascii_case(&g.asset)).unwrap_or(false) && install::looks_like_setup(e)).unwrap_or(false);
    if own_installer {
        g.exe = None;
        g.exe_rules = 0;
        g.setup_done = false;
    }
    // Left on something that can't start (a disc image or an archive picked by mistake): picked
    // again, or asked.
    if g.exe.as_ref().map(|e| crate::not_a_program(e).is_some()).unwrap_or(false) {
        crate::trail::note(format!("Play {id}: its program was set to a file that can't start, picked again"));
        g.exe = None;
        g.exe_rules = 0;
        g.setup_checked = false;
    }
    if !g.setup_checked || g.exe_rules < install::EXE_RULES {
        let os = crate::assets::os_now();
        let n = names(&g);
        let nr: Vec<&str> = n.iter().map(String::as_str).collect();
        // The program, picked again by this version's rules: a game's own launcher goes ahead of
        // its engine (CTR Nitro-Fueled, BotW Native), and "Crash" in a game's name doesn't read as
        // a crash handler. One outside its folder, or one the player picked, stays.
        let outside = g.exe.as_ref().map(|e| e.is_absolute()).unwrap_or(false);
        let boot = crate::boot::of_build(&st.dir, id, crate::boot::windows_build(g.proton));
        if !outside && g.exe_rules != u8::MAX {
            if let Some(e) = boot.as_ref().and_then(|b| b.start_in(&g.path)).or_else(|| install::find_game_exe(&g.path, os, &nr)) {
                // Launch options were for the program it started before (a path to the game's files
                // for its engine, say): its launcher doesn't take them.
                if g.exe.as_ref().map(|old| *old != e).unwrap_or(false) {
                    g.args = String::new();
                }
                g.exe = Some(e);
            } else {
                let usable = g.exe.as_ref().map(|e| g.path.join(e).is_file() && !install::looks_like_setup(e) && !install::looks_like_tool(e, &nr)).unwrap_or(false);
                let unclear = install::unclear_game_exe(&g.path, os, &nr);
                if !keeps_old_program(usable, unclear, g.played_secs) {
                    g.exe = None;
                }
            }
        }
        // A game whose installer builds it (its profile names the program it makes): a program in
        // the download that isn't that one is a tool, not the game, so the setup runs first.
        let mut redo = false;
        if let Some(want) = boot.as_ref().and_then(|b| b.then_start()).and_then(|t| t.file_name().map(|n| n.to_string_lossy().to_lowercase())) {
            let wrong = |e: &PathBuf| e.file_name().map(|n| !crate::boot::name_fits(&want, &n.to_string_lossy())).unwrap_or(true);
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
    // From here on, why the setup runs now is worked out for this Play only and never written down:
    // only a setup that finishes changes the record (`finish`). A setup closed before it finished
    // leaves the game as it was, and the once-only reasons below (an update, the game asking) have
    // their own marks, so a version that played before plays again on the next Play.
    //
    // Set up, but the game it builds wasn't found then (the setup was still going, say): looked
    // for again (and kept when found), and when it's nowhere, its setup opens again to finish.
    if g.setup_done && g.exe.is_none() {
        if let Some(want) = crate::boot::of_build(&st.dir, id, crate::boot::windows_build(g.proton)).and_then(|b| b.then_name()) {
            let since = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(g.installed_at);
            let mut near: Vec<PathBuf> = [Some(g.path.clone()), g.path.parent().map(Path::to_path_buf)].into_iter().flatten().collect();
            near.extend(player_places(st));
            if let Some(p) = find_built(&want, since, &near) {
                if let Some(r) = st.lib.lock().unwrap().installed.get_mut(id) {
                    r.exe = Some(p);
                }
                st.save();
                return None;
            }
            g.setup_done = false;
        }
    }
    // Its setup ran, but what it makes isn't there (closed early, or the disc wasn't picked): it
    // hasn't finished, so it opens again instead of the game starting only to quit.
    if g.setup_done && g.setup.is_some() && crate::boot::of_build(&st.dir, id, crate::boot::windows_build(g.proton)).and_then(|b| b.not_made(&g.path)).is_some() {
        g.setup_done = false;
    }
    // Updated before the launcher knew to run an installer again (the game it built is older than
    // the update): its setup runs once for this version, so the new version gets built.
    if g.setup_done && g.setup.is_some() && crate::boot::of_build(&st.dir, id, crate::boot::windows_build(g.proton)).map(|b| b.is_installer()).unwrap_or(false) {
        let mark = st.dir.join("rebuilt").join(format!("{id}@{}", g.tag.replace(['/', '\\', ':'], "_")));
        let built = g.exe.as_ref().filter(|e| !e.is_absolute()).map(|e| g.path.join(e)).and_then(|p| std::fs::metadata(p).ok()).and_then(|m| m.modified().ok());
        let updated = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(g.updated_at);
        let old_build = built.map(|b| b + std::time::Duration::from_secs(120) < updated).unwrap_or(false);
        if old_build && !mark.exists() && g.updated_at > g.installed_at {
            if play {
                let _ = std::fs::create_dir_all(mark.parent().unwrap());
                let _ = std::fs::write(&mark, b"");
                crate::trail::note(format!("Play {id}: its build is older than the {} update, the setup runs once to build it", g.tag));
            }
            g.setup_done = false;
        }
    }
    // The game said on its last start that its setup hasn't run ("run …Setup.exe first"): it
    // runs now, once for what the game said.
    if g.setup_done && asked_for_setup(st, id, &g, play) {
        crate::trail::note(format!("Play {id}: the game asked for its setup last time, so it runs first"));
        g.setup_done = false;
    }
    if g.setup_done {
        return None;
    }
    g.setup.as_ref().map(|s| g.path.join(s)).filter(|p| p.is_file())
}

/// The game's own last words name its setup program (HopperSetup.exe), and that's news: each
/// time the game prints it is answered once (`answer`: Play marks it), so a setup that ran, or
/// one closed before it finished, isn't asked for again by an old message.
fn asked_for_setup(st: &App, id: &str, g: &Installed, answer: bool) -> bool {
    let Some(name) = g.setup.as_ref().and_then(|s| s.file_name()).map(|n| n.to_string_lossy().to_lowercase()) else { return false };
    let out = crate::game_output_path(&st.dir, id);
    let Some(stamp) = std::fs::metadata(&out).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map(|d| d.as_millis().to_string()) else { return false };
    let mark = out.with_extension("setup-asked");
    if std::fs::read_to_string(&mark).map(|m| m == stamp).unwrap_or(false) {
        return false;
    }
    let Ok(text) = std::fs::read_to_string(&out) else { return false };
    let lines: Vec<&str> = text.lines().collect();
    let named = lines.iter().rev().take(80).any(|l| l.to_lowercase().contains(&name));
    if named && answer {
        let _ = std::fs::write(&mark, &stamp);
    }
    named
}

/// A setup that's running: started as usual, or with administrator rights after Windows' prompt.
pub enum Started {
    Child(std::process::Child),
    #[cfg(windows)]
    Admin(crate::win::Elevated),
}

impl Started {
    /// Waits for it to close: its exit code.
    pub fn wait(self) -> Option<i32> {
        match self {
            Started::Child(mut c) => c.wait().ok().and_then(|s| s.code()),
            #[cfg(windows)]
            Started::Admin(e) => e.wait(),
        }
    }
}

/// Starts a program the way double-clicking it would: an installer that asks for administrator
/// rights gets Windows' prompt instead of failing.
pub fn spawn(p: &Path) -> std::io::Result<Started> {
    spawn_with(p, None)
}

/// `copy`: the player's copy of the game, handed to the setup as if dropped on it.
pub fn spawn_with(p: &Path, copy: Option<&Path>) -> std::io::Result<Started> {
    let dir = p.parent().map(Path::to_path_buf).unwrap_or_default();
    // On Linux every game plays from a plain folder: an AppImage is unpacked, never run (Play
    // unpacks a game's own before its setup gets here).
    #[cfg(target_os = "linux")]
    if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("appimage")) {
        return Err(std::io::Error::other("it's an AppImage, and on Linux the launcher plays games only from a plain folder. Install the game again from its page"));
    }
    // A Windows setup on Linux runs through Proton (or Wine), like a Windows game does.
    // A Windows Installer package goes through Windows' installer (msiexec /i).
    let msi = crate::install::is_msi(p);
    #[cfg(not(windows))]
    let mut c = if crate::proton::is_windows_program(p) {
        match (crate::proton::runner(), crate::proton::data_dir()) {
            (Some(r), Some(data)) if msi => {
                let mut c = crate::proton::std_command(&r, &data, Path::new("msiexec"));
                c.arg("/i").arg(p);
                c
            }
            (Some(r), Some(data)) => crate::proton::std_command(&r, &data, p),
            _ => return Err(std::io::Error::other("it's a Windows program, and running it on Linux needs Proton or Wine (Steam's Proton is enough)")),
        }
    } else {
        std::process::Command::new(p)
    };
    #[cfg(windows)]
    let mut c = if msi {
        let mut c = std::process::Command::new("msiexec");
        c.arg("/i").arg(p);
        c
    } else {
        std::process::Command::new(p)
    };
    crate::sysenv::clean_std(&mut c);
    if let Some(x) = copy {
        c.arg(x);
    }
    match c.current_dir(&dir).spawn() {
        Ok(child) => Ok(Started::Child(child)),
        #[cfg(windows)]
        Err(e) if e.raw_os_error() == Some(740) => {
            let args = copy.map(|x| format!("\"{}\"", x.display())).unwrap_or_default();
            crate::win::run_as_admin(p, &args, &dir).map(Started::Admin)
        }
        Err(e) => Err(e),
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
    let child = match spawn_with(setup, copy) {
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
        let code = child.wait();
        // A setup that's only a starter (TotK-Setup.exe opens its Python window and quits): the
        // setup isn't over until every program from the download it started has closed too.
        let places = wait_for_rest(&folder);
        let (state, candidates) = finish(&app, &id, code, since, &places);
        OPEN.lock().unwrap().retain(|x| x != &id);
        if from_bp {
            crate::show_main(&app);
        }
        let _ = app.emit("first-setup", json!({"id": id, "name": name, "state": state, "candidates": candidates}));
    });
    Ok(())
}

/// Waits while any program from `dir` (the game's download) is still running: what a setup that
/// only starts another program goes on doing after its own program has quit. Windows only.
/// Returns the folders those programs were given on their command lines (TotK's setup starts its
/// builder with `--dest C:\Games\TOTK`): where the player told it to put the game.
fn wait_for_rest(dir: &Path) -> Vec<String> {
    #[cfg_attr(not(windows), allow(unused_mut))]
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
    let d = dir.to_string_lossy().trim_end_matches('\\').to_lowercase() + "\\";
    let mine = std::process::id();
    let mut lines = vec![];
    for (pid, _) in crate::win::processes() {
        if pid == mine || pid == 0 {
            continue;
        }
        let inside = crate::win::process_file(pid).map(|f| f.to_string_lossy().to_lowercase().starts_with(&d)).unwrap_or(false);
        if inside {
            lines.push(crate::win::command_line(pid).unwrap_or_default());
        }
    }
    (lines.len(), lines)
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
            .filter_map(|g| crate::boot::of_build(&st.dir, &g.id, crate::boot::windows_build(g.proton)).and_then(|b| b.then_name()).map(|w| (g.id.clone(), w)))
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
                } else if t.is_file() && crate::boot::name_fits(&want, &n) {
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
/// Returns how it went ("done", "confirm", "lost" or "cancelled") and, for "confirm", the programs
/// the player can pick from (the launcher's own pick first).
fn finish(app: &AppHandle, id: &str, code: Option<i32>, since: SystemTime, said: &[String]) -> (&'static str, Vec<String>) {
    let st = app.state::<App>();
    let Some(mut g) = st.lib.lock().unwrap().installed.get(id).cloned() else { return ("cancelled", vec![]) };
    let os = crate::assets::os_now();
    let n = names(&g);
    let nr: Vec<&str> = n.iter().map(String::as_str).collect();
    let want = crate::boot::of_build(&st.dir, id, crate::boot::windows_build(g.proton)).and_then(|b| b.then_start()).and_then(|t| t.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    let usable = |g: &Installed| {
        g.exe.as_ref().map(|e| g.path.join(e).is_file() && !install::looks_like_setup(e)).unwrap_or(false)
            && want.as_ref().map(|w| g.exe.as_ref().and_then(|e| e.file_name()).map(|n| crate::boot::name_fits(w, &n.to_string_lossy())).unwrap_or(false)).unwrap_or(true)
    };
    let installer = g.exe.as_ref().map(|e| e.is_absolute()).unwrap_or(true);
    if !usable(&g) {
        g.exe = install::find_exe(&g.path, os, &nr);
    }
    // An installer: where it put the game. Looked at again after an update too, in case it moved.
    if installer && (!usable(&g) || g.exe.as_ref().map(|e| e.is_absolute()).unwrap_or(false)) {
        let then = crate::boot::of_build(&st.dir, id, crate::boot::windows_build(g.proton)).and_then(|b| b.then_start());
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
    // Installed for every user (a Windows Installer package puts it in Program Files, keeping the
    // files' own dates, so "made since" can't find it): the program it names, in its folder there.
    if !usable(&g) {
        if let Some(p) = want.as_deref().and_then(in_program_files) {
            g.exe = Some(p);
        }
    }
    // A program that isn't the one the game's profile names is a tool from the download, never
    // the game: rather than start it, the player is asked to pick the game.
    if want.is_some() && !usable(&g) {
        g.exe = None;
    }
    let found = usable(&g);
    // Only a game whose profile names the program its setup makes is known for sure. Otherwise
    // the player says which program starts the game (an installer's tools, a translator or a
    // builder left beside it, aren't the game), from the launcher's pick and the programs the
    // setup just made.
    let mut candidates: Vec<String> = vec![];
    if !(found && want.is_some()) && code == Some(0) {
        if let Some(e) = g.exe.as_ref().filter(|_| found) {
            candidates.push(if e.is_absolute() { e.clone() } else { g.path.join(e) }.to_string_lossy().to_string());
        }
        let mut near: Vec<PathBuf> = said.iter().map(PathBuf::from).map(|p| if p.is_dir() { p } else { p.parent().map(Path::to_path_buf).unwrap_or(p) }).filter(|p| p.is_dir()).collect();
        near.push(g.path.clone());
        near.extend(player_places(&st));
        for p in new_programs(since, &near) {
            let s = p.to_string_lossy().to_string();
            if !candidates.iter().any(|c| c.eq_ignore_ascii_case(&s)) {
                candidates.push(s);
            }
        }
        candidates.truncate(8);
    }
    let state = if found && want.is_some() {
        "done"
    } else if !candidates.is_empty() {
        "confirm"
    } else if found {
        "done"
    } else if code == Some(0) {
        "lost"
    } else {
        "cancelled"
    };
    // It didn't finish: the game stays as it was (a version that played before still plays, and
    // the next Play opens its setup when it has to), never left without its program.
    if state == "cancelled" {
        crate::trail::note(format!("Setup {id}: its setup closed before it finished; the game is as it was"));
        return (state, candidates);
    }
    {
        let mut l = st.lib.lock().unwrap();
        if let Some(r) = l.installed.get_mut(id) {
            r.exe = g.exe.clone();
            r.args = g.args.clone();
            r.setup_done = true;
        }
    }
    st.save();
    (state, candidates)
}

/// Programs made since `since` (with two minutes' grace) in these folders, a few levels down: what
/// a setup that just ran put there. Newest first; setups, uninstallers and helpers left out.
/// Windows programs only; quick (a few seconds at most).
fn new_programs(since: SystemTime, near: &[PathBuf]) -> Vec<PathBuf> {
    if !cfg!(windows) {
        return vec![];
    }
    let since = since.checked_sub(std::time::Duration::from_secs(120)).unwrap_or(since);
    let until = std::time::Instant::now() + std::time::Duration::from_secs(6);
    let skip = ["windows", "programdata", "$recycle.bin", "system volume information", "appdata", "node_modules", ".git", "redist", "_commonredist", "directx", "vcredist"];
    let mut seen = std::collections::HashSet::new();
    let mut found: Vec<(SystemTime, PathBuf)> = vec![];
    for root in near {
        let mut stack = vec![(root.clone(), 0usize)];
        while let Some((dir, d)) = stack.pop() {
            if std::time::Instant::now() > until {
                break;
            }
            if !seen.insert(dir.to_string_lossy().to_lowercase()) {
                continue;
            }
            for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let name = e.file_name().to_string_lossy().to_lowercase();
                let Ok(ft) = e.file_type() else { continue };
                if ft.is_dir() {
                    if d < 4 && !skip.contains(&name.as_str()) && !name.starts_with('.') {
                        stack.push((e.path(), d + 1));
                    }
                } else if name.ends_with(".exe") && !name.starts_with("unins") && !install::looks_like_setup(Path::new(&name)) && !["crashpad_handler.exe", "crashreporter.exe", "dxsetup.exe", "vc_redist.x64.exe", "vc_redist.x86.exe"].contains(&name.as_str()) {
                    let made = e.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
                    if made >= since {
                        found.push((made, e.path()));
                    }
                }
            }
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().map(|f| f.1).take(8).collect()
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
                    // Not a setup's leftovers or tools (temp\…\build, deps): a half-built copy
                    // there doesn't have what the game needs beside it.
                    if d < depth && !skip.contains(&name.as_str()) && search_into(&name) {
                        stack.push((e.path(), d + 1));
                    }
                } else if crate::boot::name_fits(&want, &name) {
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

/// `file` in a folder of Program Files (either one, on any drive) or of the user's own programs
/// folder: OpenGOAL-Launcher.exe in Program Files\OpenGOAL-Launcher.
fn in_program_files(file: &str) -> Option<PathBuf> {
    let want = file.to_lowercase();
    let mut roots: Vec<PathBuf> = Vec::new();
    for d in crate::files::drives() {
        roots.push(d.path.join("Program Files"));
        roots.push(d.path.join("Program Files (x86)"));
    }
    if let Some(l) = std::env::var_os("LOCALAPPDATA") {
        roots.push(PathBuf::from(l).join("Programs"));
    }
    for root in roots {
        for e in std::fs::read_dir(&root).into_iter().flatten().flatten() {
            if !e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            for f in std::fs::read_dir(e.path()).into_iter().flatten().flatten() {
                if crate::boot::name_fits(&want, &f.file_name().to_string_lossy()) && f.path().is_file() {
                    return Some(f.path());
                }
            }
        }
    }
    None
}

/// A folder the search for a built program goes into (its name, lowercased): not a hidden one, and
/// not a setup's leftovers or tools.
fn search_into(name: &str) -> bool {
    !name.starts_with('.') && !crate::install::TOOL_DIRS.contains(&name)
}

/// Name words that say nothing about which game it is.
const GENERIC: [&str; 18] = ["recomp", "recompiled", "decomp", "decompiled", "port", "the", "launcher", "online", "edition", "native", "windows", "game", "x64", "x86", "bit", "version", "beta", "alpha"];

fn words(s: &str) -> Vec<String> {
    // A version isn't part of the name: "Hopper PC Port 1.2" is Hopper.
    let named: Vec<&str> = name_parts(s).into_iter().filter(|p| !is_version(p)).collect();
    // A lone digit is part of the word before it ("Resistance 3" is resistance3, as
    // "Resistance3Port" is), so sequels tell apart and still count as a word of the name.
    let mut joined: Vec<String> = vec![];
    for t in install::name_tokens(&named.join(" ")) {
        let digit = t.len() == 1 && t.chars().all(|c| c.is_ascii_digit());
        match joined.last_mut() {
            Some(prev) if digit && !prev.chars().all(|c| c.is_ascii_digit()) => prev.push_str(&t),
            _ => joined.push(t),
        }
    }
    joined.into_iter().filter(|t| (t.len() > 2 || (t.len() == 2 && t.chars().all(|c| c.is_ascii_digit()))) && !GENERIC.contains(&t.as_str())).collect()
}

/// Whether `word` is a shortened `full`. A lone digit after it is a sequel's: "hopper3" is a
/// different game from "hopper". Two digits aren't: "zelda64" is how Zelda's projects are named.
fn starts_the_word(word: &str, full: &str) -> bool {
    if word.len() <= 3 {
        return false;
    }
    let Some(rest) = full.strip_prefix(word) else { return false };
    let sequel_number = rest.len() == 1 && rest.chars().all(|c| c.is_ascii_digit());
    !sequel_number
}

/// A name split at anything but letters, digits and dots, so a version like "1.2" stays whole.
fn name_parts(s: &str) -> Vec<&str> {
    s.split(|c: char| !c.is_ascii_alphanumeric() && c != '.').map(|p| p.trim_matches('.')).filter(|p| !p.is_empty()).collect()
}

/// A version ("1.2", "v2"): which build of the game it is, not which game.
fn is_version(part: &str) -> bool {
    let number = part.strip_prefix(|c: char| c == 'v' || c == 'V').unwrap_or(part);
    let numeric = number.starts_with(|c: char| c.is_ascii_digit()) && number.chars().all(|c| c.is_ascii_digit() || c == '.');
    numeric && (number.len() < part.len() || number.contains('.'))
}

/// Words a number after is that disc, part or build of this game, not a sequel.
const NUMBERED: [&str; 8] = ["disc", "disk", "part", "vol", "volume", "version", "ver", "build"];

/// The number a sequel is named with that words() drops: a lone digit 2 to 9, or a Roman numeral
/// "ii" to "xx" (so "Hopper II" is "Hopper 2"). One isn't: "Hopper 1" is still Hopper. Two digits
/// are left to words(), which keeps them: shortcuts glue them on ("MarioKart64", "MaddenNFL08"),
/// and a glued number can't be told from a project's name ("Zelda64Recompiled" for Zelda Recomp).
fn sequel_number(part: &str) -> Option<u32> {
    const ROMAN: [&str; 19] = ["ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x", "xi", "xii", "xiii", "xiv", "xv", "xvi", "xvii", "xviii", "xix", "xx"];
    let lower = part.to_ascii_lowercase();
    if let Some(i) = ROMAN.iter().position(|r| *r == lower) {
        return Some(i as u32 + 2);
    }
    if part.len() != 1 || !part.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let n: u32 = part.parse().ok()?;
    (2..=9).contains(&n).then_some(n)
}

/// A lone digit glued to the end of a word ("hopper3"), which words() already reads as a sequel.
/// Two glued digits aren't one: "zelda64" is how Zelda's projects are named.
fn glued_digit(token: &str) -> Option<u32> {
    let cut = token.len().checked_sub(1)?;
    let (word, digit) = token.split_at(cut);
    let glued = word.ends_with(|c: char| c.is_ascii_alphabetic()) && digit.chars().all(|c| c.is_ascii_digit());
    if !glued || NUMBERED.contains(&word) {
        return None;
    }
    sequel_number(digit)
}

/// The sequel numbers in a name. words() drops a short word like "ii" or a "2" on its own, so
/// these come from the name itself.
fn sequel_numbers(name: &str) -> Vec<u32> {
    let parts = name_parts(name);
    let mut out = vec![];
    for (i, part) in parts.iter().enumerate() {
        let after_a_label = i > 0 && NUMBERED.contains(&parts[i - 1].to_ascii_lowercase().as_str());
        if after_a_label || is_version(part) {
            continue;
        }
        if let Some(n) = sequel_number(part) {
            out.push(n);
            continue;
        }
        out.extend(install::name_tokens(part).iter().filter_map(|t| glued_digit(t)));
    }
    out
}

/// What an app's or shortcut's name is checked against: the words of the game's names and the
/// sequel numbers in them.
struct Want {
    words: Vec<String>,
    sequels: Vec<u32>,
}

/// Whether an app's or shortcut's name is this game's: every word of it is in the game's names,
/// and enough of them ("Madden NFL 08" for Madden NFL 08 Recompiled, not Madden NFL 25). A sequel
/// number on only one side means another game of the series ("Hopper II" isn't Hopper).
fn is_ours(want: &Want, name: &str) -> bool {
    let have = words(name);
    if have.is_empty() || have.iter().any(|h| h.starts_with("unins")) {
        return false;
    }
    let sequels = sequel_numbers(name);
    let theirs_only = sequels.iter().any(|n| !want.sequels.contains(n));
    let ours_only = want.sequels.iter().any(|n| !sequels.contains(n));
    if theirs_only || ours_only {
        return false;
    }
    let want = &want.words;
    let hit = |h: &String| want.iter().any(|w| w == h || starts_the_word(h, w) || starts_the_word(w, h));
    let hits = have.iter().filter(|h| hit(h)).count();
    hits == have.len() && hits >= want.len().min(2)
}

fn want_of(names: &[&str]) -> Want {
    let mut words: Vec<String> = names.iter().flat_map(|n| self::words(n)).collect();
    words.sort();
    words.dedup();
    let mut sequels: Vec<u32> = names.iter().flat_map(|n| sequel_numbers(n)).collect();
    sequels.sort();
    sequels.dedup();
    Want { words, sequels }
}

/// Where an installer put the game: (program, its options). Its fresh shortcuts first, then its
/// entry in Settings > Apps, then older shortcuts.
#[cfg(windows)]
pub(crate) fn installed_elsewhere(names: &[&str], since: SystemTime, ours: &Path, then: Option<&Path>) -> Option<(PathBuf, String)> {
    let mut list = installed_apps();
    // Shortcuts on the desktops and in the Start menu, two folders deep.
    let env = |k: &str| std::env::var(k).ok().map(PathBuf::from);
    let places = [
        dirs::desktop_dir(),
        env("PUBLIC").map(|p| p.join("Desktop")),
        env("APPDATA").map(|p| p.join(r"Microsoft\Windows\Start Menu\Programs")),
        env("ProgramData").map(|p| p.join(r"Microsoft\Windows\Start Menu\Programs")),
    ];
    let mut files = vec![];
    fn walk(d: &Path, depth: u32, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() && depth < 2 {
                walk(&p, depth + 1, out);
            } else if p.extension().map(|x| x.eq_ignore_ascii_case("lnk")).unwrap_or(false) {
                out.push(p);
            }
        }
    }
    for d in places.into_iter().flatten() {
        walk(&d, 0, &mut files);
    }
    for (f, target, args) in crate::win::read_shortcuts(files) {
        let t = std::fs::metadata(&f).ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
        let n = f.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        list.push(json!({"k": "lnk", "n": n, "p": target.to_string_lossy(), "a": args, "t": t}));
    }
    pick(&list, names, since, ours, then)
}

/// The apps in Settings > Apps (Windows' list of installed programs).
#[cfg(windows)]
fn installed_apps() -> Vec<Value> {
    use crate::win::{reg_str, reg_subkeys, HKCU, HKLM};
    let mut o = vec![];
    for (root, key) in [(HKCU, r"Software\Microsoft\Windows\CurrentVersion\Uninstall"), (HKLM, r"Software\Microsoft\Windows\CurrentVersion\Uninstall"), (HKLM, r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall")] {
        for sub in reg_subkeys(root, key) {
            let k = format!(r"{key}\{sub}");
            let v = |name: &str| reg_str(root, &k, name).unwrap_or_default();
            o.push(json!({"k": "app", "n": v("DisplayName"), "p": v("InstallLocation"), "i": v("DisplayIcon"), "u": v("UninstallString")}));
        }
    }
    o
}

#[cfg(not(windows))]
pub(crate) fn installed_elsewhere(_names: &[&str], _since: SystemTime, _ours: &Path, _then: Option<&Path>) -> Option<(PathBuf, String)> {
    None
}

/// Where Windows says installers put this game (the InstallLocation of its entries in
/// Settings > Apps whose names are the game's), folders that are there.
#[cfg(windows)]
pub fn app_folders(names: &[&str]) -> Vec<PathBuf> {
    let want = want_of(names);
    let mut out: Vec<PathBuf> = vec![];
    for v in installed_apps() {
        let (n, p) = (v["n"].as_str().unwrap_or(""), v["p"].as_str().unwrap_or("").trim().trim_matches('"'));
        if !p.is_empty() && is_ours(&want, n) {
            let d = PathBuf::from(p);
            if d.is_dir() && !out.contains(&d) {
                out.push(d);
            }
        }
    }
    out
}

#[cfg(not(windows))]
pub fn app_folders(_names: &[&str]) -> Vec<PathBuf> {
    vec![]
}

/// The uninstaller Windows lists for a game an installer put in `dir` (its install folder holds
/// the game's program): the command to run, as Windows keeps it.
#[cfg(windows)]
pub fn uninstaller_for(dir: &Path) -> Option<String> {
    let list: Vec<Value> = installed_apps().into_iter().filter(|v| !v["u"].as_str().unwrap_or("").is_empty()).collect();
    pick_uninstaller(&list, dir)
}

#[cfg(not(windows))]
pub fn uninstaller_for(_dir: &Path) -> Option<String> {
    None
}

/// The entry whose install folder (or icon, or uninstaller) is the game's folder.
#[cfg_attr(not(windows), allow(dead_code))]
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
        let (prog, args) = crate::win::split_command(cmd);
        let dir = Path::new(&prog).parent().filter(|d| d.is_dir()).map(Path::to_path_buf).unwrap_or_else(std::env::temp_dir);
        match std::process::Command::new(&prog).raw_arg(&args).current_dir(&dir).spawn() {
            Ok(_) => {}
            Err(e) if e.raw_os_error() == Some(740) => {
                crate::win::run_as_admin(Path::new(&prog), &args, &dir)?;
            }
            Err(e) => return Err(e),
        }
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
    if let Some(x) = lnk(false) {
        return Some(x);
    }
    // One shortcut made while the installer ran, whatever it's called ("Crash TOTT" for Crash of
    // the Titans): that's where it went. With more than one, nothing is guessed.
    let fresh: Vec<(PathBuf, String)> = list.iter()
        .filter(|v| s(v, "k") == "lnk" && v.get("t").and_then(Value::as_i64).unwrap_or(0) >= since)
        .map(|v| (PathBuf::from(s(v, "p")), s(v, "a")))
        .filter(|(p, _)| good(p))
        .collect();
    let mut dirs: Vec<PathBuf> = fresh.iter().filter_map(|(p, _)| p.parent().map(Path::to_path_buf)).collect();
    dirs.dedup();
    if dirs.len() == 1 { fresh.into_iter().next() } else { None }
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_unclear_guess_not_played_long_is_asked_again() {
        assert!(!keeps_old_program(true, true, 30));
    }

    #[test]
    fn an_unclear_guess_played_a_while_is_kept() {
        assert!(keeps_old_program(true, true, 600));
    }

    #[test]
    fn a_clear_pick_is_kept() {
        assert!(keeps_old_program(true, false, 0));
    }

    /// A program file this system starts: an .exe on Windows, an executable elsewhere.
    fn put_program(dir: &Path, stem: &str, bytes: usize) -> PathBuf {
        let ext = match crate::assets::os_now() {
            "windows" => ".exe",
            "linux" => ".x86_64",
            _ => "",
        };
        let rel = PathBuf::from(format!("{stem}{ext}"));
        let p = dir.join(&rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, vec![7u8; bytes]).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        rel
    }

    /// An install from an older version, left on its automatic guess between two programs that
    /// don't name the game (the bigger one won), and what it starts after the next look.
    fn exe_after_look(tag: &str, played_secs: u64, exe_rules: u8) -> Option<PathBuf> {
        let d = std::env::temp_dir().join(format!("rl-unclear-old-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let game = d.join("game");
        put_program(&game, "runtime/EntryRunner", 4_000_000);
        let guess = put_program(&game, "host/FrameHost", 30_000_000);
        let id = format!("unclear-old-{tag}");
        let id = id.as_str();
        let g = Installed { id: id.into(), name: "Night Harbor".into(), path: game, exe: Some(guess), exe_rules, setup_checked: true, played_secs, ..Default::default() };
        let mut lib = crate::library::Library::default();
        lib.installed.insert(id.into(), g.clone());
        let st = crate::tests::app_in(&d, lib);
        let _ = pending(&st, id, &g);
        let exe = st.lib.lock().unwrap().installed.get(id).and_then(|r| r.exe.clone());
        let _ = std::fs::remove_dir_all(&d);
        exe
    }

    #[test]
    fn an_older_guess_played_briefly_is_cleared_so_the_player_is_asked() {
        assert_eq!(exe_after_look("brief", 30, install::EXE_RULES - 1), None);
    }

    #[test]
    fn an_older_guess_played_a_while_is_kept() {
        let kept = exe_after_look("long", 600, install::EXE_RULES - 1).and_then(|e| e.file_stem().map(|s| s.to_string_lossy().into_owned()));
        assert_eq!(kept.as_deref(), Some("FrameHost"));
    }

    #[test]
    fn the_players_own_pick_is_never_looked_at_again() {
        let kept = exe_after_look("mine", 0, u8::MAX).and_then(|e| e.file_stem().map(|s| s.to_string_lossy().into_owned()));
        assert_eq!(kept.as_deref(), Some("FrameHost"));
    }

    #[test]
    fn a_setups_leftovers_are_not_searched() {
        assert!(!super::search_into("temp"));
        assert!(!super::search_into("tools"));
        assert!(!super::search_into(".cache"));
        assert!(super::search_into("build"));
        assert!(super::search_into("totk"));
    }

    use super::*;

    #[test]
    fn one_fresh_shortcut_points_to_the_game_whatever_its_name() {
        let d = std::env::temp_dir().join(format!("rl-fresh-lnk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for f in ["Games/CrashTOTT/Crash.exe", "Games/Other/Other.exe", "ours/x.exe"] {
            std::fs::create_dir_all(d.join(f).parent().unwrap()).unwrap();
            std::fs::write(d.join(f), b"x").unwrap();
        }
        let since = SystemTime::now() - std::time::Duration::from_secs(10);
        let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs() as i64;
        let lnk = |n: &str, p: &Path, t: i64| json!({"k": "lnk", "n": n, "p": p.to_string_lossy(), "a": "", "t": t});
        let names = ["Crash of the Titans", "crashott-pc"];
        let one = vec![lnk("Crash TOTT", &d.join("Games/CrashTOTT/Crash.exe"), now), lnk("Old Thing", &d.join("Games/Other/Other.exe"), now - 99_999)];
        assert_eq!(pick(&one, &names, since, &d.join("ours"), None).map(|x| x.0), Some(d.join("Games/CrashTOTT/Crash.exe")));
        // Two fresh shortcuts to different places: nothing is guessed.
        let two = vec![lnk("Crash TOTT", &d.join("Games/CrashTOTT/Crash.exe"), now), lnk("Something", &d.join("Games/Other/Other.exe"), now)];
        assert_eq!(pick(&two, &names, since, &d.join("ours"), None), None);
        let _ = std::fs::remove_dir_all(&d);
    }

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
    fn a_sequels_number_counts_and_tells_the_sequels_apart() {
        let want = want_of(&["Hopper 3 Port", "Hopper 3", "Hopper3Port"]);
        assert!(is_ours(&want, "Hopper 3 PC Port"));
        assert!(is_ours(&want, "Hopper 3 PC Port (Hopper3Port)"));
        assert!(!is_ours(&want, "Hopper 2 PC Port"));
        assert!(!is_ours(&want_of(&["Hopper 2 Port", "Hopper2Port"]), "Hopper 3 PC Port"));
        // Two-digit numbers stay their own word, as before.
        assert_eq!(words("Hopper NFL 08"), vec!["hopper", "nfl", "08"]);
    }

    #[test]
    fn the_original_and_its_sequel_arent_each_other() {
        assert!(!is_ours(&want_of(&["Hopper Port", "Hopper", "HopperPort"]), "Hopper 3 PC Port"));
        assert!(!is_ours(&want_of(&["Hopper 3 Port", "Hopper3Port"]), "Hopper PC Port"));
        assert!(!is_ours(&want_of(&["Mass Effect"]), "Mass Effect 2"));
    }

    #[test]
    fn a_two_digit_number_in_a_project_name_still_matches() {
        assert!(is_ours(&want_of(&["Zelda Recomp"]), "Zelda64Recompiled"));
    }

    #[test]
    fn a_sequel_number_as_its_own_word_isnt_the_original() {
        assert!(!is_ours(&want_of(&["Hopper"]), "Hopper 2 PC Port"));
        assert!(!is_ours(&want_of(&["Hopper"]), "Hopper II PC Port"));
    }

    #[test]
    fn a_roman_numeral_sequel_matches_itself() {
        assert!(is_ours(&want_of(&["Hopper II"]), "Hopper II PC Port"));
    }

    #[test]
    fn a_version_isnt_a_sequel_number() {
        assert!(is_ours(&want_of(&["Hopper"]), "Hopper PC Port 1.2"));
        assert!(is_ours(&want_of(&["Hopper"]), "Hopper PC Port v2"));
    }

    #[test]
    fn a_disc_number_isnt_a_sequel_number() {
        assert!(sequel_numbers("Hopper Disc 2").is_empty());
        assert!(sequel_numbers("Hopper Part II").is_empty());
    }

    #[test]
    fn a_glued_sequel_digit_matches_the_same_number_written_apart() {
        assert!(is_ours(&want_of(&["Hopper 3 Port", "Hopper 3"]), "Hopper3Port"));
    }

    #[test]
    fn a_two_digit_title_number_matches_glued_on() {
        assert!(is_ours(&want_of(&["Mario Kart 64"]), "MarioKart64Recompiled"));
        assert!(is_ours(&want_of(&["Madden NFL 08", "MaddenNFL08"]), "MaddenNFL08"));
    }

    #[test]
    fn another_two_digit_title_number_isnt_ours() {
        assert!(!is_ours(&want_of(&["Madden NFL 08"]), "Madden NFL 25"));
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

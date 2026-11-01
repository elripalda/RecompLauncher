//! RecompLauncher: discover, install and play PC recomps, in one window.
//!
//! The window (ui/) asks for things through the commands at the bottom; long jobs (downloads,
//! games running) report back through events: `install` with a Progress, `game` when one starts
//! or closes.

pub mod adopt;
pub mod assets;
pub mod boot;
pub mod catalog;
pub mod cloud;
pub mod devtools;
pub mod discord;
pub mod extract;
pub mod files;
pub mod gamebanana;
pub mod info;
pub mod ingame;
pub mod install;
pub mod library;
pub mod looks;
pub mod manage;
pub mod media;
pub mod mods;
pub mod pads;
pub mod own_setup;
pub mod ghinfo;
pub mod ping;
pub mod plugins;
pub mod push;
pub mod profile;
pub mod proton;
pub mod secret;
pub mod sysenv;
pub mod roms;
pub mod saves;
pub mod shortcuts;
pub mod trail;
pub mod tray;
pub mod usage;
pub mod win;

use catalog::Release;
use install::Progress;
use library::{folder_name, now, Installed, Library, Settings};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Msg(String),
    #[error("Cancelled")]
    Cancelled,
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Net(String),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        if e.is_connect() || e.is_timeout() {
            Error::Net("Couldn't reach the internet. Check your connection and try again.".into())
        } else if e.is_decode() {
            Error::Net("recomplauncher.com sent something unexpected.".into())
        } else {
            Error::Net(format!("The download stopped: {e}"))
        }
    }
}

impl Serialize for Error {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

type Res<T> = Result<T, Error>;

/// Windows Security stopped a file ("the file contains a virus or potentially unwanted software").
fn is_blocked(e: &std::io::Error) -> bool {
    matches!(e.raw_os_error(), Some(225) | Some(226))
}

/// The message for a file Windows Security blocked. It starts with BLOCKED| and the file's name, so
/// the window can explain it properly.
pub(crate) fn defender_msg(e: std::io::Error, what: &str) -> Error {
    if is_blocked(&e) {
        Error::Msg(format!("BLOCKED|{what}|Windows Security blocked {what}: it thinks the file is harmful."))
    } else {
        Error::Io(e)
    }
}

pub struct App {
    dir: PathBuf,
    http: reqwest::Client,
    lib: Mutex<Library>,
    jobs: Mutex<HashMap<String, Arc<AtomicBool>>>,
    running: Mutex<HashMap<String, tokio::sync::oneshot::Sender<()>>>,
    pids: Mutex<HashMap<String, u32>>,
    started: Mutex<HashMap<String, u64>>,
    /// One download at a time; the rest wait their turn.
    download_slot: Arc<tokio::sync::Semaphore>,
    /// Games being moved to another drive (their folder is briefly in two places).
    moving: Mutex<HashSet<String>>,
    overlay_open: AtomicBool,
    /// Games running with a look on (their screenshots show it).
    looked: Mutex<HashSet<String>>,
    /// Started by a shortcut (`--play`): no window, and the launcher closes with the game.
    headless: AtomicBool,
}

impl App {
    fn save(&self) {
        let l = self.lib.lock().unwrap().clone();
        let _ = l.save(&self.dir);
    }
}

fn str_of<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

/// Where a game's backups go: beside it, in its library's .backups folder.
fn backups_dir(g: &Installed) -> PathBuf {
    g.path.parent().unwrap_or(Path::new(".")).join(".backups").join(g.path.file_name().unwrap_or_default())
}

// ---------- installing ----------

/// "Getting ready": asking the website for the list and the recomp's releases. Cancel works while it
/// waits, and it never waits more than a minute.
async fn getting_ready<T>(f: impl std::future::Future<Output = Result<T, Error>>, cancel: &AtomicBool) -> Result<T, Error> {
    tokio::pin!(f);
    let start = Instant::now();
    let mut told = false;
    loop {
        tokio::select! {
            r = &mut f => return r,
            _ = tokio::time::sleep(std::time::Duration::from_millis(300)) => {
                if cancel.load(Ordering::Relaxed) {
                    return Err(Error::Cancelled);
                }
                if !told && start.elapsed() > std::time::Duration::from_secs(10) {
                    told = true;
                    trail::note("Getting ready: still waiting for the website after 10 s");
                }
                if start.elapsed() > std::time::Duration::from_secs(60) {
                    trail::note("Getting ready: gave up after 60 s");
                    return Err(Error::Msg("recomplauncher.com or GitHub didn't answer in time. Try again in a moment.".into()));
                }
            }
        }
    }
}

async fn do_install(app: &AppHandle, st: &App, id: &str, dir: Option<PathBuf>, tag: Option<String>, file: Option<String>, cancel: Arc<AtomicBool>) -> Res<()> {
    let report = |p: Progress| {
        let _ = app.emit("install", p);
    };
    report(Progress::new(id, "prepare", 0, 0));
    trail::note(format!("Install {id}: getting ready{}", tag.as_deref().map(|t| format!(" ({t})")).unwrap_or_default()));
    let cat = getting_ready(catalog::catalog(&st.http, &st.dir, false), &cancel).await?.data;
    let entry = catalog::app(&cat, id).ok_or_else(|| Error::Msg("This recomp isn't listed anymore.".into()))?.clone();
    let repo = str_of(&entry, "repo").to_string();
    trail::note(format!("Install {id}: list read, asking {repo} for its releases"));
    let rels: Vec<Release> = serde_json::from_value(getting_ready(catalog::releases(&st.http, &st.dir, &repo, true), &cancel).await?.data)?;
    trail::note(format!("Install {id}: {} releases", rels.len()));
    let have = st.lib.lock().unwrap().installed.get(id).map(|g| g.tag.clone()).unwrap_or_default();
    let newest = catalog::latest_for(&rels, &have).ok_or_else(|| Error::Msg("This recomp hasn't put out a release yet.".into()))?;
    // A version the player picked (an older one, to go back to), or the newest.
    let rel = match &tag {
        Some(t) => rels.iter().find(|r| &r.tag_name == t).ok_or_else(|| Error::Msg(format!("{t} isn't one of its releases.")))?,
        None => newest,
    };
    let pinned = rel.tag_name != newest.tag_name;
    let (os, arch) = (os_for(st, &entry, id), assets::arch_now());
    let pattern = entry.get("assets").and_then(|a| a.get(os)).and_then(Value::as_str);
    let os_name = match os {
        "windows" => "Windows",
        "macos" => "Mac",
        _ => "Linux",
    };
    // The list says which systems a recomp is for: one that isn't for this one isn't installed, even
    // when its release has a file that looks like it could be.
    // A Linux build the list doesn't know about yet (the developer just added one): the release
    // itself says so.
    // Only a file that says it's for Linux counts.
    let linux_files: Vec<_> = rel.assets.iter().filter(|a| { let n = a.name.to_lowercase(); n.contains("linux") || n.ends_with(".appimage") }).cloned().collect();
    let linux_new = os == "linux" && !runs_here(&entry, os) && assets::pick(&linux_files, "linux", arch, None).is_some();
    if !runs_here(&entry, os) && !linux_new {
        return Err(Error::Msg(format!("This recomp has no {os_name} version yet.")));
    }
    let mut asset = (if linux_new { assets::pick(&linux_files, os, arch, None) } else { assets::pick(&rel.assets, os, arch, pattern) })
        .ok_or_else(|| Error::Msg(format!("The latest release ({}) has no {os_name} version.", rel.tag_name)))?
        .clone();
    // Several builds that fit this PC equally (one per region, say), or in different forms (an
    // AppImage and a plain folder, a portable zip and an installer): the one the player picked, the
    // one most like what they had, or they're asked once, with the usual pick first.
    let choices = if linux_new { assets::choices(&linux_files, os, arch, None) } else { assets::choices(&rel.assets, os, arch, pattern) };
    let all = if linux_new { assets::all_for(&linux_files, os, arch, None) } else { assets::all_for(&rel.assets, os, arch, pattern) };
    // Kinds of build, 32-bit or not alike: a 32-bit copy alone isn't worth asking about (Change Build has it).
    let forms = all.iter().map(|a| assets::describe(&a.name, os).trim_start_matches("32-Bit ").to_lowercase()).collect::<std::collections::HashSet<_>>().len();
    let had = st.lib.lock().unwrap().installed.get(id).map(|g| g.asset.clone()).filter(|a| !a.is_empty());
    match (file.as_deref(), had) {
        (Some(f), _) => {
            asset = all.iter().find(|a| a.name == f).map(|a| (*a).clone()).ok_or_else(|| Error::Msg(format!("{f} isn't in this release.")))?;
        }
        (None, Some(h)) if choices.len() > 1 || forms > 1 => {
            // Updates follow the form the player picked: of the same kind first, then the closest name.
            let same: Vec<&assets::Asset> = all.iter().copied().filter(|a| assets::describe(&a.name, os) == assets::describe(&h, os)).collect();
            if let Some(a) = assets::most_like(if same.is_empty() { &all } else { &same }, &h) {
                asset = a.clone();
            }
        }
        (None, None) if choices.len() > 1 || forms > 1 => {
            // Each as "name<tab>kind of build"; the usual pick is marked with a * and listed first.
            let tag = |n: &str| format!("{n}\t{}", assets::describe(n, os));
            let mut names: Vec<String> = vec![format!("*{}", tag(&asset.name))];
            names.extend(all.iter().filter(|a| a.name != asset.name).map(|a| tag(&a.name)));
            return Err(Error::Msg(format!("PICK_BUILD|{}", names.join("|"))));
        }
        _ => {}
    }

    let (lib_dir, keep, backup_first, prev) = {
        let l = st.lib.lock().unwrap();
        let lib_dir = dir.clone().unwrap_or_else(|| l.settings.library_dir.clone());
        (lib_dir, l.settings.keep_downloads, l.settings.backup_before_update, l.installed.get(id).cloned())
    };
    if prev.as_ref().map(|p| p.local).unwrap_or(false) {
        return Err(Error::Msg("That's your local build. Take it out of the Library first to install the release.".into()));
    }
    let target = prev.as_ref().map(|p| p.path.clone()).unwrap_or_else(|| lib_dir.join(folder_name(id)));
    let home = target.parent().map(Path::to_path_buf).unwrap_or(lib_dir.clone());
    std::fs::create_dir_all(&home).map_err(|e| {
        // The drive it's on isn't there (unplugged, or a letter that changed): said plainly.
        let drive = home.ancestors().last().filter(|r| !r.as_os_str().is_empty() && !r.exists());
        match drive {
            Some(r) => Error::Msg(format!("The drive your library is on ({}) isn't connected. Connect it, or pick another library folder in Settings → Games and folders.", r.display())),
            None => Error::Msg(format!("Couldn't use the library folder {}: {e}", home.display())),
        }
    })?;
    if let Some(free) = manage::free_space(&home) {
        if free < asset.size.saturating_mul(3) {
            return Err(Error::Msg(format!("There isn't enough space on that drive ({} free).", human(free))));
        }
    }

    // One download at a time; this one waits its turn.
    let permit = match st.download_slot.clone().try_acquire_owned() {
        Ok(p) => p,
        Err(_) => {
            report(Progress::new(id, "queued", 0, 0));
            st.download_slot.clone().acquire_owned().await.map_err(|e| Error::Msg(e.to_string()))?
        }
    };
    let started = std::time::Instant::now();
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }
    let got = install::download(&st.http, &asset, &home.join(".downloads"), id, cancel.clone(), &report).await?;
    drop(permit);
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }

    // An update: keep a copy of the player's saves and settings first.
    if let (Some(p), true) = (&prev, backup_first) {
        report(Progress::new(id, "backup", 0, 0));
        let (dir, files, bdir) = (p.path.clone(), p.files.clone(), backups_dir(p));
        let _ = tokio::task::spawn_blocking(move || manage::backup(&dir, &files, &bdir, 5)).await;
    }

    // The settings the player picked on the game's page, put back after the update's files.
    let kept = prev.as_ref().and_then(|p| profile::find(&p.path, &p.repo, &entry).map(|prof| prof.kept(&p.path))).unwrap_or_default();

    // Another build of it picked (one region for another): a single-file build it had goes, so the
    // new one is the one that starts.
    if let Some(p) = prev.as_ref().filter(|p| !p.asset.is_empty() && p.asset != asset.name) {
        let low = p.asset.to_lowercase();
        if low.ends_with(".appimage") || low.ends_with(".exe") {
            let _ = std::fs::remove_file(target.join(&p.asset));
        }
    }
    report(Progress::new(id, "unpack", 0, 0));
    let (file, t2, staging) = (got.file.clone(), target.clone(), home.join(".staging").join(folder_name(id)));
    let what = asset.name.clone();
    let files = tokio::task::spawn_blocking(move || install::unpack(&file, &t2, &staging)).await.map_err(|e| Error::Msg(e.to_string()))?.map_err(|e| match e {
        Error::Io(e) => defender_msg(e, &what),
        e => e,
    })?;
    if !keep {
        let _ = std::fs::remove_file(&got.file);
    }
    // A game whose own setup works from a clone of its repo gets one, around the release's files.
    if boot::Boot::from_entry(&entry).map(|b| b.clone).unwrap_or(false) {
        report(Progress::new(id, "clone", 0, 0));
        let (r2, t3, scratch) = (repo.clone(), target.clone(), home.join(".staging").join(format!("{}-repo", folder_name(id))));
        tokio::task::spawn_blocking(move || manage::clone_repo(&r2, &t3, &scratch)).await.map_err(|e| Error::Msg(e.to_string()))??;
    }
    if !kept.is_empty() {
        if let Some(prof) = profile::find(&target, &repo, &entry) {
            prof.restore(&target, &kept);
        }
    }

    // Keep the program the player picked, unless the update replaced it with a differently named one
    // (an AppImage carries its version in its name).
    let names = [str_of(&entry, "name"), repo.rsplit('/').next().unwrap_or(""), str_of(&entry, "game")];
    let mut exe = prev.as_ref().and_then(|p| p.exe.clone()).filter(|e| target.join(e).is_file());
    if let Some(old) = exe.clone() {
        let is_appimage = old.to_string_lossy().to_lowercase().ends_with(".appimage");
        let new_appimage = files.iter().find(|f| f.to_string_lossy().to_lowercase().ends_with(".appimage"));
        if let (true, Some(n)) = (is_appimage, new_appimage) {
            if *n != old {
                let _ = std::fs::remove_file(target.join(&old));
                exe = Some(n.clone());
            }
        }
    }
    let exe = exe.or_else(|| install::find_exe(&target, os, &names));
    // The catalog's boot profile says which program starts it, unless the player picked one.
    let boot = boot::Boot::from_entry(&entry);
    let picked = prev.as_ref().map(|p| p.exe_rules == u8::MAX).unwrap_or(false);
    let exe = match boot.as_ref().and_then(|b| b.start_in(&target)) {
        Some(s) if !picked => Some(s),
        _ => exe,
    };
    // A release that's an installer for the game it builds (totk.exe): any other program in it is
    // one of its tools (a shader translator, say), never the game.
    let exe = match (boot.as_ref().and_then(|b| b.then_name()), exe) {
        (Some(w), Some(e)) if !picked && e.file_name().map(|n| n.to_string_lossy().to_lowercase() != w).unwrap_or(true) => None,
        (_, e) => e,
    };
    // A program that has to run once first: the release's installer, or a setup beside the game.
    // An update keeps a setup beside the game as done; a new installer runs again.
    let setup = match &boot {
        Some(b) if !b.is_installer() => b.setup_in(&target),
        Some(b) => b.setup_near(&target).or_else(|| install::find_setup(&target, os, exe.as_deref().filter(|e| !e.is_absolute()))),
        None => install::find_setup(&target, os, exe.as_deref().filter(|e| !e.is_absolute())),
    };
    let setup_done = setup.is_some() && prev.as_ref().map(|p| p.setup_done && p.exe.as_ref().map(|e| !e.is_absolute()).unwrap_or(false)).unwrap_or(false);

    let rec = Installed {
        id: id.into(),
        name: str_of(&entry, "name").into(),
        game: prev.as_ref().map(|p| p.game.clone()).unwrap_or_default(),
        repo,
        tag: rel.tag_name.clone(),
        version: str_of(&entry, "version").into(),
        path: target,
        exe,
        asset: asset.name.clone(),
        size: asset.size,
        sha256: got.sha256,
        verified: got.verified,
        installed_at: prev.as_ref().map(|p| p.installed_at).unwrap_or_else(now),
        updated_at: now(),
        played_secs: prev.as_ref().map(|p| p.played_secs).unwrap_or(0),
        last_played: prev.as_ref().map(|p| p.last_played).unwrap_or(0),
        files,
        pinned,
        sessions: prev.as_ref().map(|p| p.sessions.clone()).unwrap_or_default(),
        game_copy: prev.as_ref().and_then(|p| p.game_copy.clone()),
        update_copy: prev.as_ref().and_then(|p| p.update_copy.clone()),
        proton: os == "windows" && assets::os_now() == "linux",
        picks: prev.as_ref().map(|p| p.picks.clone()).unwrap_or_default(),
        args: prev.as_ref().map(|p| p.args.clone()).unwrap_or_default(),
        auto_update: prev.as_ref().map(|p| p.auto_update).unwrap_or(false),
        local: false,
        look: prev.as_ref().map(|p| p.look.clone()).unwrap_or_default(),
        look_amount: prev.as_ref().map(|p| p.look_amount).unwrap_or(50),
        cloud: prev.as_ref().and_then(|p| p.cloud),
        found_saves: prev.as_ref().map(|p| p.found_saves.clone()).unwrap_or_default(),
        save_place: prev.as_ref().and_then(|p| p.save_place.clone()),
        cover_file: prev.as_ref().and_then(|p| p.cover_file.clone()),
        hero_file: prev.as_ref().and_then(|p| p.hero_file.clone()),
        setup,
        setup_done,
        setup_checked: true,
        exe_rules: if prev.as_ref().map(|p| p.exe_rules == u8::MAX).unwrap_or(false) { u8::MAX } else { install::EXE_RULES },
        manual: prev.as_ref().and_then(|p| p.manual.clone()),
    };
    {
        let mut l = st.lib.lock().unwrap();
        l.installed.insert(id.into(), rec);
        l.settings.asked_location = true;
    }
    st.save();
    note_download(
        &st.dir,
        json!({"id": id, "name": str_of(&entry, "name"), "tag": rel.tag_name, "from": prev.as_ref().map(|p| p.tag.clone()).unwrap_or_default(), "repo": str_of(&entry, "repo"), "size": asset.size, "secs": started.elapsed().as_secs(), "when": now(), "update": prev.is_some(), "verified": got.verified}),
    );
    let mut p = Progress::new(id, "done", 1, 1);
    p.msg = Some(if got.verified { "Checked against GitHub's checksum".into() } else { "GitHub has no checksum for this file".into() });
    // Off the list of jobs before the window hears it's done: the window asks for the state right
    // away, and a job still listed then would show as "Getting ready" for good.
    st.jobs.lock().unwrap().remove(id);
    report(p);
    Ok(())
}

/// Whether the list says this recomp runs on this system. An entry that names no systems at all is
/// left to its releases.
/// Whose build a game uses here: this system's, or on Linux with Proton on, the Windows one for a
/// recomp that has no Linux version (and for one already installed that way).
fn os_for(st: &App, entry: &Value, id: &str) -> &'static str {
    let here = assets::os_now();
    if here != "linux" {
        return here;
    }
    let l = st.lib.lock().unwrap();
    if l.installed.get(id).map(|g| g.proton).unwrap_or(false) {
        return "windows";
    }
    if l.settings.proton && !runs_here(entry, "linux") && runs_here(entry, "windows") {
        "windows"
    } else {
        here
    }
}

fn runs_here(entry: &Value, os: &str) -> bool {
    let systems: Vec<&str> = entry.get("systems").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    let has_assets = entry.get("assets").and_then(Value::as_object).map(|o| !o.is_empty()).unwrap_or(false);
    if systems.is_empty() && !has_assets {
        return true;
    }
    systems.contains(&os) || entry.get("assets").and_then(|a| a.get(os)).is_some()
}

/// How each recomp plays, by the players' word (counts only), for the lists and the versions menu.
#[tauri::command]
async fn report_counts(st: State<'_, App>, force: Option<bool>) -> Res<Value> {
    Ok(catalog::report_counts(&st.http, &st.dir, force.unwrap_or(false)).await?.data.get("counts").cloned().unwrap_or(Value::Null))
}

/// One recomp's player reports, for its page.
#[tauri::command]
async fn play_reports(st: State<'_, App>, id: String, force: Option<bool>) -> Res<Value> {
    let f = catalog::reports(&st.http, &st.dir, &id, force.unwrap_or(false)).await?;
    let mut v = f.data;
    if let Some(o) = v.as_object_mut() {
        o.insert("offline".into(), Value::Bool(f.offline));
    }
    Ok(v)
}

/// Finished downloads, newest first, for the Downloads page.
fn note_download(dir: &Path, rec: Value) {
    let p = dir.join("downloads.json");
    let mut list: Vec<Value> = std::fs::read(&p).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    list.insert(0, rec);
    list.truncate(200);
    if let Ok(b) = serde_json::to_vec(&list) {
        let _ = std::fs::write(&p, b);
    }
}

#[tauri::command]
async fn download_history(st: State<'_, App>) -> Res<Vec<Value>> {
    Ok(std::fs::read(st.dir.join("downloads.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default())
}

fn human(b: u64) -> String {
    let gb = b as f64 / 1_073_741_824.0;
    if gb >= 1.0 {
        format!("{gb:.1} GB")
    } else {
        format!("{} MB", b / 1_048_576)
    }
}

#[tauri::command]
async fn install(app: AppHandle, st: State<'_, App>, id: String, dir: Option<String>, tag: Option<String>, file: Option<String>) -> Res<()> {
    if st.moving.lock().unwrap().contains(&id) {
        return Err(Error::Msg(MOVING.into()));
    }
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut jobs = st.jobs.lock().unwrap();
        if jobs.contains_key(&id) {
            return Ok(());
        }
        jobs.insert(id.clone(), cancel.clone());
    }
    let dir = dir.filter(|d| !d.is_empty()).map(PathBuf::from);
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let st = app2.state::<App>();
        let r = do_install(&app2, &st, &id, dir, tag.filter(|t| !t.is_empty()), file.filter(|f| !f.is_empty()), cancel).await;
        st.jobs.lock().unwrap().remove(&id);
        match r {
            Ok(()) => trail::note(format!("Install {id}: done")),
            Err(Error::Cancelled) => {
                trail::note(format!("Install {id}: cancelled"));
                let _ = app2.emit("install", Progress::new(&id, "cancelled", 0, 0));
            }
            Err(e) => {
                trail::note(format!("Install {id}: {e}"));
                let mut p = Progress::new(&id, "error", 0, 0);
                p.msg = Some(e.to_string());
                let _ = app2.emit("install", p);
            }
        }
    });
    Ok(())
}

/// The builds of a recomp's newest release that fit this PC, when there's more than one (one per
/// region, say), and the one installed now.
#[tauri::command]
async fn release_builds(st: State<'_, App>, id: String) -> Res<Value> {
    let cat = catalog::catalog(&st.http, &st.dir, false).await?.data;
    let entry = catalog::app(&cat, &id).ok_or_else(|| Error::Msg("This recomp isn't listed anymore.".into()))?.clone();
    let rels: Vec<Release> = serde_json::from_value(catalog::releases(&st.http, &st.dir, str_of(&entry, "repo"), false).await?.data)?;
    let have = st.lib.lock().unwrap().installed.get(&id).map(|g| (g.tag.clone(), g.asset.clone())).unwrap_or_default();
    let rel = rels.iter().find(|r| r.tag_name == have.0).or_else(|| catalog::latest(&rels)).ok_or_else(|| Error::Msg("It has no releases yet.".into()))?;
    let os = os_for(&st, &entry, &id);
    let pattern = entry.get("assets").and_then(|a| a.get(os)).and_then(Value::as_str);
    let arch = assets::arch_now();
    let best = assets::pick(&rel.assets, os, arch, pattern).map(|a| a.name.clone()).unwrap_or_default();
    let builds: Vec<Value> = assets::all_for(&rel.assets, os, arch, pattern).iter().map(|a| json!({"name": a.name, "label": assets::describe(&a.name, os), "best": a.name == best})).collect();
    Ok(json!({"tag": rel.tag_name, "builds": builds, "current": have.1}))
}

#[tauri::command(async)]
fn cancel(st: State<'_, App>, id: String) {
    if let Some(c) = st.jobs.lock().unwrap().get(&id) {
        c.store(true, Ordering::Relaxed);
    }
}

#[tauri::command(async)]
fn uninstall(st: State<'_, App>, id: String, delete_files: bool) -> Res<Value> {
    if st.running.lock().unwrap().contains_key(&id) {
        return Err(Error::Msg("Close the game first.".into()));
    }
    if st.moving.lock().unwrap().contains(&id) || st.jobs.lock().unwrap().contains_key(&id) {
        return Err(Error::Msg(MOVING.into()));
    }
    let libs = st.lib.lock().unwrap().settings.all_libraries();
    let Some(g) = st.lib.lock().unwrap().installed.get(&id).cloned() else { return Ok(json!({})) };
    // Still open outside the launcher (or still closing): Windows won't let its files go.
    if delete_files && g.exe.as_ref().map(|e| already_open(&if e.is_absolute() { e.clone() } else { g.path.join(e) })).unwrap_or(false) {
        return Err(Error::Msg(format!("{} is still open. Close it, then uninstall it.", g.name)));
    }
    // A local build's folder is the developer's: it only leaves the Library.
    let delete_files = delete_files && !g.local;
    let mut left: Vec<String> = vec![];
    let mut kept = false;
    let mut uninstaller = false;
    if delete_files {
        // Only ever a game's own folder right inside a library. A game the player brought over
        // from somewhere else keeps its folder, and so does anything shared like Downloads.
        // Or a folder the launcher made for it (named after it) in a library folder since removed.
        let in_lib = g.path.parent().map(|p| libs.iter().any(|l| library::same_path(l, p))).unwrap_or(false);
        let made_here = !g.asset.is_empty() && g.path.file_name().map(|n| n.to_string_lossy().eq_ignore_ascii_case(&library::folder_name(&id))).unwrap_or(false);
        let ok = (in_lib || made_here) && !library::shared_folder(&g.path, &libs) && g.path.is_dir();
        if ok {
            left = remove_game_folder(&g.path);
        } else if g.path.is_dir() {
            kept = true;
        }
        // A game its own installer put somewhere else (Program Files): its own uninstaller, the
        // way Windows would, so the game itself goes too and not only its installer.
        if let Some(exe) = g.exe.as_ref().filter(|e| e.is_absolute() && !e.starts_with(&g.path)) {
            if let Some(cmd) = exe.parent().and_then(own_setup::uninstaller_for) {
                uninstaller = own_setup::run_uninstaller(&cmd).is_ok();
            }
        }
    }
    st.lib.lock().unwrap().installed.remove(&id);
    st.save();
    Ok(json!({"left": left.len(), "path": g.path, "kept": kept, "uninstaller": uninstaller}))
}

/// Deletes a game's folder, sturdier than one remove_dir_all: files marked read-only (as some zips
/// unpack them) are made writable first, and a file still being let go of (an antivirus scan, a
/// game that just closed) gets a few more tries. Links inside (to the player's own game files) are
/// removed as links, never followed. Returns what couldn't be deleted.
fn remove_game_folder(dir: &Path) -> Vec<String> {
    for attempt in 0..4 {
        if !dir.exists() || std::fs::remove_dir_all(dir).is_ok() {
            return vec![];
        }
        clear_read_only(dir);
        std::thread::sleep(std::time::Duration::from_millis(300 * (attempt + 1)));
    }
    if std::fs::remove_dir_all(dir).is_ok() || !dir.exists() {
        return vec![];
    }
    // What's left: the files that are still there.
    let mut out = vec![];
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let link = e.file_type().map(|t| t.is_symlink()).unwrap_or(false);
            if !link && e.path().is_dir() {
                stack.push(e.path());
            } else {
                out.push(e.path().to_string_lossy().to_string());
            }
            if out.len() > 50 {
                return out;
            }
        }
    }
    if out.is_empty() {
        out.push(dir.to_string_lossy().to_string());
    }
    out
}

fn clear_read_only(dir: &Path) {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let Ok(t) = e.file_type() else { continue };
            if t.is_symlink() {
                continue;
            }
            if t.is_dir() {
                stack.push(e.path());
            }
            if let Ok(m) = e.metadata() {
                let mut perm = m.permissions();
                if perm.readonly() {
                    #[allow(clippy::permissions_set_readonly_false)]
                    perm.set_readonly(false);
                    let _ = std::fs::set_permissions(e.path(), perm);
                }
            }
        }
    }
}

// ---------- libraries, moving, backups ----------

const MOVING: &str = "It's busy being moved or updated. Try again once that's done.";

#[derive(Serialize)]
struct LibInfo {
    path: PathBuf,
    free: Option<u64>,
    games: usize,
    default: bool,
}

#[tauri::command(async)]
fn libraries(st: State<'_, App>) -> Vec<LibInfo> {
    let l = st.lib.lock().unwrap();
    l.settings
        .all_libraries()
        .into_iter()
        .map(|p| LibInfo {
            free: manage::free_space(&p),
            games: l.installed.values().filter(|g| g.path.parent() == Some(p.as_path())).count(),
            default: p == l.settings.library_dir,
            path: p,
        })
        .collect()
}

/// How much room each installed game takes (its folder, when the launcher can see it), for the
/// Storage page in Big Picture mode.
#[tauri::command]
async fn game_sizes(st: State<'_, App>) -> Res<serde_json::Value> {
    let games: Vec<(String, PathBuf)> = st.lib.lock().unwrap().installed.iter().map(|(id, g)| (id.clone(), g.path.clone())).collect();
    let out = tokio::task::spawn_blocking(move || {
        let mut sizes = serde_json::Map::new();
        let mut drives: Vec<(String, u64)> = Vec::new();
        for (id, p) in games {
            if !p.is_dir() {
                continue;
            }
            sizes.insert(id, json!(manage::dir_size(&p)));
            let root = p.components().next().map(|c| c.as_os_str().to_string_lossy().to_string()).unwrap_or_default();
            if !drives.iter().any(|(r, _)| *r == root) {
                if let Some(f) = manage::free_space(&p) {
                    drives.push((root, f));
                }
            }
        }
        json!({ "sizes": sizes, "drives": drives.into_iter().map(|(r, f)| json!({ "drive": r, "free": f })).collect::<Vec<_>>() })
    })
    .await
    .map_err(|e| Error::Msg(e.to_string()))?;
    Ok(out)
}

#[tauri::command]
async fn move_game(app: AppHandle, st: State<'_, App>, id: String, to: String) -> Res<Installed> {
    if st.running.lock().unwrap().contains_key(&id) {
        return Err(Error::Msg("Close the game first.".into()));
    }
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    if g.local {
        return Err(Error::Msg("A local build stays in its own folder.".into()));
    }
    if st.moving.lock().unwrap().contains(&id) || st.jobs.lock().unwrap().contains_key(&id) {
        return Err(Error::Msg(MOVING.into()));
    }
    if library::shared_folder(&g.path, &st.lib.lock().unwrap().settings.all_libraries()) {
        return Err(Error::Msg("Its folder holds more than the game, so it stays where it is. Install it into a library to be able to move it.".into()));
    }
    let dest = PathBuf::from(&to).join(g.path.file_name().unwrap_or_default());
    if dest == g.path {
        return Ok(g);
    }
    looks::tidy(&st.dir, &id);
    st.moving.lock().unwrap().insert(id.clone());
    let (from, d2, app2, id2) = (g.path.clone(), dest.clone(), app.clone(), id.clone());
    let r = tokio::task::spawn_blocking(move || {
        let last = std::cell::Cell::new(Instant::now());
        manage::move_dir(&from, &d2, &|done, total| {
            if last.get().elapsed().as_millis() > 150 || done == total {
                last.set(Instant::now());
                let _ = app2.emit("move", json!({"id": id2, "done": done, "total": total}));
            }
        })
    })
    .await
    .map_err(|e| Error::Msg(e.to_string()))?;
    st.moving.lock().unwrap().remove(&id);
    r?;
    // Its backups go with it.
    let old_b = backups_dir(&g);
    let mut out = g.clone();
    out.path = dest;
    if old_b.is_dir() {
        let _ = manage::move_dir(&old_b, &backups_dir(&out), &|_, _| {});
    }
    st.lib.lock().unwrap().installed.insert(id.clone(), out.clone());
    st.save();
    let _ = app.emit("move", json!({"id": id, "done": 1, "total": 1, "finished": true}));
    Ok(out)
}

#[tauri::command(async)]
fn backups(st: State<'_, App>, id: String) -> Res<Vec<manage::Backup>> {
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    Ok(manage::list_backups(&backups_dir(&g)))
}

#[tauri::command]
async fn backup_now(st: State<'_, App>, id: String) -> Res<Option<manage::Backup>> {
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    if g.local {
        return Ok(None);
    }
    tokio::task::spawn_blocking(move || manage::backup(&g.path, &g.files, &backups_dir(&g), 5)).await.map_err(|e| Error::Msg(e.to_string()))?
}

#[tauri::command]
async fn restore_backup(st: State<'_, App>, id: String, name: String) -> Res<usize> {
    if st.running.lock().unwrap().contains_key(&id) {
        return Err(Error::Msg("Close the game first.".into()));
    }
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    if name.contains('/') || name.contains('\\') || !name.ends_with(".zip") {
        return Err(Error::Msg("That isn't one of its backups.".into()));
    }
    let file = backups_dir(&g).join(&name);
    tokio::task::spawn_blocking(move || manage::restore(&file, &g.path)).await.map_err(|e| Error::Msg(e.to_string()))?
}

#[tauri::command(async)]
fn pc_check() -> manage::PcCheck {
    manage::pc_check()
}

// ---------- your games (ROMs) ----------

#[tauri::command]
async fn scan_roms(st: State<'_, App>) -> Res<Vec<roms::RomFile>> {
    let dirs = st.lib.lock().unwrap().settings.rom_dirs.clone();
    let found = tokio::task::spawn_blocking(move || roms::scan(&dirs)).await.map_err(|e| Error::Msg(e.to_string()))?;
    st.lib.lock().unwrap().roms = found.clone();
    st.save();
    Ok(found)
}

#[tauri::command]
async fn rom_matches(st: State<'_, App>) -> Res<HashMap<String, roms::RomFile>> {
    let cat = catalog::catalog(&st.http, &st.dir, false).await?.data;
    let (found, dirs) = {
        let l = st.lib.lock().unwrap();
        (l.roms.clone(), l.installed.values().map(|g| (g.id.clone(), g.path.clone())).collect::<HashMap<_, _>>())
    };
    Ok(tokio::task::spawn_blocking(move || copies_for(&cat, &found, &dirs)).await.unwrap_or_default())
}

// ---------- the launcher's own file browser ----------

#[derive(Serialize)]
struct Places {
    quick: Vec<files::Place>,
    drives: Vec<files::Place>,
    /// The player's game folders (the ones the launcher looks through).
    games: Vec<files::Place>,
    /// Where recomps get installed.
    library: Vec<files::Place>,
}

#[tauri::command]
async fn fs_places(st: State<'_, App>) -> Res<Places> {
    let (roms, libs) = {
        let l = st.lib.lock().unwrap();
        (l.settings.rom_dirs.clone(), l.settings.all_libraries())
    };
    tauri::async_runtime::spawn_blocking(move || Places { quick: files::quick(), drives: files::drives(), games: files::named("games", &roms), library: files::named("library", &libs) })
        .await
        .map_err(|e| Error::Msg(e.to_string()))
}

#[derive(Serialize)]
struct GameHere {
    id: String,
    name: String,
}

#[derive(Serialize)]
struct FolderView {
    #[serde(flatten)]
    listing: files::Listing,
    /// Disc images, ROMs and game folders here that are games with recomps: path -> the recomp.
    games: HashMap<String, GameHere>,
}

/// A zip, 7z or rar from the file browser, unpacked next to itself into a folder named after it
/// ("Game (2)" when that's taken). Returns the folder.
#[tauri::command]
async fn fs_extract(path: String) -> Res<String> {
    let file = PathBuf::from(&path);
    let low = path.to_lowercase();
    if !file.is_file() || !(low.ends_with(".zip") || low.ends_with(".7z") || low.ends_with(".rar")) {
        return Err(Error::Msg("That isn't a zip, 7z or rar.".into()));
    }
    let parent = file.parent().map(Path::to_path_buf).ok_or_else(|| Error::Msg("That file has no folder.".into()))?;
    let stem = file.file_stem().map(|s| s.to_string_lossy().trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| "Extracted".into());
    let mut dest = parent.join(&stem);
    let mut n = 2;
    while dest.exists() {
        dest = parent.join(format!("{stem} ({n})"));
        n += 1;
    }
    let size = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
    if let Some(free) = manage::free_space(&parent) {
        if free < size.saturating_mul(2) {
            return Err(Error::Msg(format!("There isn't enough space on that drive to extract it ({} free).", human_size(free))));
        }
    }
    let d2 = dest.clone();
    let r = tauri::async_runtime::spawn_blocking(move || gamebanana::extract(&file, &d2)).await.map_err(|e| Error::Msg(e.to_string()))?;
    if let Err(e) = r {
        let _ = std::fs::remove_dir_all(&dest);
        return Err(e);
    }
    // A single folder inside: open that one.
    let items: Vec<_> = std::fs::read_dir(&dest).map(|r| r.flatten().collect()).unwrap_or_default();
    let open = if items.len() == 1 && items[0].path().is_dir() { items[0].path() } else { dest };
    Ok(open.to_string_lossy().to_string())
}

/// Copy (or move, with `cut`) a file or folder the player picked in the browser into another folder:
/// a disc image into their games folder, say. Nothing already there is replaced.
#[tauri::command(async)]
async fn fs_paste(from: String, to_dir: String, cut: bool) -> Res<String> {
    let from = PathBuf::from(&from);
    let dir = PathBuf::from(&to_dir);
    if !from.exists() {
        return Err(Error::Msg("It isn't there anymore.".into()));
    }
    if !dir.is_dir() {
        return Err(Error::Msg("That folder isn't there.".into()));
    }
    let name = from.file_name().ok_or_else(|| Error::Msg("That can't be copied.".into()))?.to_os_string();
    let to = dir.join(&name);
    if to == from {
        return Err(Error::Msg("It's already in this folder.".into()));
    }
    if to.exists() {
        return Err(Error::Msg(format!("There's already a {} in this folder.", name.to_string_lossy())));
    }
    if from.is_dir() && dir.starts_with(&from) {
        return Err(Error::Msg("A folder can't go inside itself.".into()));
    }
    let (a, b) = (from.clone(), to.clone());
    tauri::async_runtime::spawn_blocking(move || -> std::io::Result<()> {
        // Moving on the same drive is instant.
        if cut && std::fs::rename(&a, &b).is_ok() {
            return Ok(());
        }
        fn copy(a: &Path, b: &Path) -> std::io::Result<()> {
            if a.is_dir() {
                std::fs::create_dir_all(b)?;
                for e in std::fs::read_dir(a)?.flatten() {
                    copy(&e.path(), &b.join(e.file_name()))?;
                }
                Ok(())
            } else {
                std::fs::copy(a, b).map(|_| ())
            }
        }
        if let Err(e) = copy(&a, &b) {
            // Half a copy is no use: what was made goes again.
            let _ = if b.is_dir() { std::fs::remove_dir_all(&b) } else { std::fs::remove_file(&b) };
            return Err(e);
        }
        if cut {
            if a.is_dir() { std::fs::remove_dir_all(&a)?; } else { std::fs::remove_file(&a)?; }
        }
        Ok(())
    })
    .await
    .map_err(|e| Error::Msg(e.to_string()))?
    .map_err(|e| Error::Msg(format!("Couldn't {} it: {e}", if cut { "move" } else { "copy" })))?;
    Ok(to.to_string_lossy().into())
}

/// What's in a folder, with the games in it that have recomps marked.
#[tauri::command]
async fn fs_list(st: State<'_, App>, path: String) -> Res<FolderView> {
    let cat = cached_catalog(&st);
    let dirs: HashMap<String, PathBuf> = st.lib.lock().unwrap().installed.values().map(|g| (g.id.clone(), g.path.clone())).collect();
    tauri::async_runtime::spawn_blocking(move || {
        let listing = files::list(Path::new(&path));
        let found: Vec<roms::RomFile> = listing
            .entries
            .iter()
            .filter(|e| ["disc", "rom", "game"].contains(&e.kind.as_str()))
            .take(400)
            .filter_map(|e| if e.kind == "game" { roms::identify(&e.path.join("default.xex"), 0) } else { roms::identify(&e.path, e.size) })
            .collect();
        let mut games = HashMap::new();
        if !found.is_empty() {
            for (id, r) in copies_for(&cat, &found, &dirs) {
                let name = catalog::app(&cat, &id).map(|a| str_of(a, "name").to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| id.clone());
                games.insert(r.path.display().to_string(), GameHere { id, name });
            }
        }
        FolderView { listing, games }
    })
    .await
    .map_err(|e| Error::Msg(e.to_string()))
}

/// The player's games that have recomps: by name, and for recomps the launcher sets up, by
/// looking inside each disc image or game folder for the files that game has.
fn copies_for(cat: &Value, found: &[roms::RomFile], dirs: &HashMap<String, PathBuf>) -> HashMap<String, roms::RomFile> {
    let mut out = roms::match_catalog(cat, found);
    let none = PathBuf::new();
    for a in cat.get("apps").and_then(Value::as_array).into_iter().flatten() {
        let (Some(id), repo) = (a.get("id").and_then(Value::as_str), a.get("repo").and_then(Value::as_str).unwrap_or("")) else { continue };
        let Some(need) = profile::find(dirs.get(id).unwrap_or(&none), repo, a).and_then(|p| p.game) else { continue };
        // A check on a file every game of its console has (default.xex) can't tell games apart.
        if need.generic {
            continue;
        }
        if out.get(id).map(|r| profile::check_copy(&need, &r.path).is_ok()).unwrap_or(false) {
            continue;
        }
        out.remove(id);
        if let Some(r) = found.iter().find(|r| profile::check_copy(&need, &r.path).is_ok()) {
            out.insert(id.to_string(), r.clone());
        }
    }
    out
}

/// After a fresh catalog: tell the player about new recomps for games they own.
fn note_new_apps(app: &AppHandle, st: &App, cat: &Value) {
    // The player's own (added by link) aren't news.
    let ids: Vec<String> = cat.get("apps").and_then(Value::as_array).map(|a| a.iter().filter(|x| !x.get("custom").and_then(Value::as_bool).unwrap_or(false)).filter_map(|x| x.get("id")?.as_str().map(str::to_string)).collect()).unwrap_or_default();
    if ids.is_empty() {
        return;
    }
    let (first_time, new_ids, notify, found, news) = {
        let mut l = st.lib.lock().unwrap();
        let first = l.seen_apps.is_empty();
        let new_ids: Vec<String> = ids.iter().filter(|i| !l.seen_apps.contains(i)).cloned().collect();
        l.seen_apps = ids.clone();
        let news = followed_news(cat, &mut l, &new_ids, first);
        (first, new_ids, l.settings.notify, l.roms.clone(), news)
    };
    st.save();
    if notify {
        for (title, body, id) in &news {
            ingame::notify(app, title, body);
            let _ = app.emit("followed", id);
        }
    }
    if first_time || new_ids.is_empty() || !notify {
        return;
    }
    let owned = roms::match_catalog(cat, &found);
    for id in new_ids.iter().filter(|i| owned.contains_key(*i)) {
        if let Some(a) = catalog::app(cat, id) {
            ingame::notify(app, "New recomp for a game you own", &format!("{} is out, and you already have {}.", str_of(a, "name"), str_of(a, "game")));
            let _ = app.emit("new-recomp", id);
        }
    }
}

/// Developers the player follows (Settings: ui.follow): a new recomp of theirs, or a new version of
/// one, is worth a notification. Each version is told about once.
fn followed_news(cat: &Value, l: &mut Library, new_ids: &[String], first_time: bool) -> Vec<(String, String, String)> {
    let follow: Vec<String> = l.settings.ui.get("follow").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let mut out = Vec::new();
    if follow.is_empty() {
        return out;
    }
    let devs: HashMap<String, String> = cat.get("developers").and_then(Value::as_array).map(|a| a.iter().filter_map(|d| Some((d.get("slug")?.as_str()?.to_string(), d.get("name").and_then(Value::as_str).unwrap_or("").to_string()))).collect()).unwrap_or_default();
    for a in cat.get("apps").and_then(Value::as_array).into_iter().flatten() {
        let (Some(id), Some(dev)) = (a.get("id").and_then(Value::as_str), a.get("dev").and_then(Value::as_str)) else { continue };
        if !follow.iter().any(|f| f == dev) {
            continue;
        }
        let version = str_of(a, "version").to_string();
        let who = devs.get(dev).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| dev.to_string());
        let before = l.seen_versions.insert(id.to_string(), version.clone());
        if first_time || l.installed.contains_key(id) {
            continue;
        }
        if new_ids.iter().any(|n| n == id) {
            out.push((format!("New from {who}"), format!("{} is out.", str_of(a, "name")), id.to_string()));
        } else if let Some(b) = before.filter(|b| !b.is_empty() && *b != version && !version.is_empty()) {
            let _ = b;
            out.push((format!("New from {who}"), format!("{} {} is out.", str_of(a, "name"), version), id.to_string()));
        }
    }
    out.truncate(4);
    out
}

// ---------- shortcuts and Steam ----------

#[tauri::command]
async fn desktop_shortcut(st: State<'_, App>, id: String, menu: Option<bool>) -> Res<String> {
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let icon = g.exe.as_ref().map(|e| g.path.join(e));
    let name = game_name(&st, &id, &g.name).await;
    // Straight to the game unless the player asked for shortcuts through the launcher (Settings),
    // or the game can only be started by the launcher for now.
    let direct_on = !st.lib.lock().unwrap().settings.ui.get("shortcut_launcher").and_then(Value::as_bool).unwrap_or(false);
    let direct = if direct_on { direct_start(&st, &id, &g) } else { None };
    Ok(shortcuts::desktop_shortcut(&id, &name, icon.as_deref(), direct.as_ref().map(|(e, a)| (e.as_path(), a.as_slice())), menu.unwrap_or(false))?.to_string_lossy().into())
}

/// What a shortcut straight to the game runs: its program and arguments, as the launcher would start
/// it. None when only the launcher can start it for now: a setup still to run, its copy still to
/// pick or put in place, settings it takes from the environment, or Setup Required not done yet.
fn direct_start(st: &App, id: &str, g: &Installed) -> Option<(PathBuf, Vec<String>)> {
    let ui = st.lib.lock().unwrap().settings.ui.clone();
    if catalog::needs_setup_first(&st.dir, id, &ui) {
        return None;
    }
    let prof = profile_of(st, id, g);
    if !prof.as_ref().map(|p| p.start.is_some() && !p.from_boot).unwrap_or(false) && own_setup::pending(st, id, g).is_some() {
        return None;
    }
    let (exe, mut args) = match &prof {
        Some(p) if p.start.is_some() => {
            if p.start.as_ref().map(|s| !s.env.is_empty()).unwrap_or(false) {
                return None;
            }
            let copy = match &p.game {
                Some(_) => {
                    let c = g.game_copy.clone().filter(|c| c.exists()).or_else(|| p.already_in_place(&g.path))?;
                    if !p.missing_after_place(&g.path).is_empty() {
                        return None;
                    }
                    Some(c)
                }
                None => None,
            };
            p.command(&g.path, copy.as_deref(), &g.picks)?
        }
        Some(p) => (g.path.join(g.exe.as_ref()?), p.setting_args(&g.path, &g.picks)),
        None => (g.path.join(g.exe.as_ref()?), vec![]),
    };
    if !exe.is_file() {
        return None;
    }
    args.extend(join_paths(split_args(&g.args)));
    Some((exe, args))
}

/// The game's own name for shortcuts and Steam ("NBA Street Vol. 2", not the recomp's name).
async fn game_name(st: &App, id: &str, fallback: &str) -> String {
    let cat = catalog::catalog(&st.http, &st.dir, false).await.map(|f| f.data).unwrap_or(Value::Null);
    catalog::app(&cat, id).map(|a| str_of(a, "game").trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| fallback.to_string())
}

async fn fetch_bytes(http: &reqwest::Client, url: &str) -> Vec<u8> {
    if url.is_empty() {
        return vec![];
    }
    let full = if url.starts_with("http") { url.to_string() } else { format!("{}{}", catalog::base(), url) };
    match http.get(full).send().await {
        Ok(r) if r.status().is_success() => r.bytes().await.map(|b| b.to_vec()).unwrap_or_default(),
        _ => vec![],
    }
}

fn ext_of(url: &str) -> &'static str {
    if url.to_lowercase().contains(".png") {
        "png"
    } else {
        "jpg"
    }
}

/// Add the game to Steam. If Steam is open and `restart` is false, nothing changes and the answer
/// is "steam_running", so the window can ask first; with `restart`, Steam is closed, the game
/// added, and Steam started again.
#[tauri::command]
async fn add_to_steam(st: State<'_, App>, id: String, restart: bool) -> Res<String> {
    let steam = shortcuts::steam_dir().ok_or_else(|| Error::Msg("Steam isn't installed on this PC.".into()))?;
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let was_running = shortcuts::steam_running();
    if was_running {
        if let Some(why) = shortcuts::cant_restart_steam() {
            return Err(Error::Msg(why.into()));
        }
    }
    if was_running && !restart {
        return Ok("steam_running".into());
    }
    let cat = catalog::catalog(&st.http, &st.dir, false).await.map(|f| f.data).unwrap_or(Value::Null);
    let a = catalog::app(&cat, &id).cloned().unwrap_or(Value::Null);
    let (cover, hero, logo) = (str_of(&a, "cover").to_string(), str_of(&a, "hero").to_string(), str_of(&a, "logo").to_string());
    let art = vec![
        ("p", fetch_bytes(&st.http, &cover).await, ext_of(&cover)),
        ("_hero", fetch_bytes(&st.http, &hero).await, ext_of(&hero)),
        ("", fetch_bytes(&st.http, &hero).await, ext_of(&hero)),
        ("_logo", fetch_bytes(&st.http, &logo).await, ext_of(&logo)),
    ];
    if was_running {
        if let Some(why) = shortcuts::cant_restart_steam() {
            return Err(Error::Msg(why.into()));
        }
        trail::note(format!("Add to Steam ({id}): closing Steam"));
        shortcuts::close_steam(&steam).await.inspect_err(|e| trail::note(format!("Add to Steam: {e}")))?;
    }
    let icon = g.exe.as_ref().map(|e| g.path.join(e).to_string_lossy().to_string()).unwrap_or_default();
    let name = Some(str_of(&a, "game").trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| g.name.clone());
    let n = shortcuts::write_to_steam(&steam, &name, &id, &icon, &art)?;
    trail::note(format!("Add to Steam ({id}): written for {n} account(s)"));
    if was_running {
        shortcuts::start_steam(&steam);
    }
    Ok(format!("added:{n}"))
}

/// RecompLauncher itself in Steam, starting in Big Picture mode. Same Steam-is-open handling as a
/// game.
#[tauri::command]
async fn launcher_to_steam(st: State<'_, App>, restart: bool) -> Res<String> {
    let steam = shortcuts::steam_dir().ok_or_else(|| Error::Msg("Steam isn't installed on this PC.".into()))?;
    let was_running = shortcuts::steam_running();
    if was_running {
        if let Some(why) = shortcuts::cant_restart_steam() {
            return Err(Error::Msg(why.into()));
        }
    }
    if was_running && !restart {
        return Ok("steam_running".into());
    }
    let _ = &st;
    // Its own art in Steam's library: poster, banner, wide header and logo.
    let art = vec![
        ("p", include_bytes!("../steam-art/poster.jpg").to_vec(), "jpg"),
        ("_hero", include_bytes!("../steam-art/hero.jpg").to_vec(), "jpg"),
        ("", include_bytes!("../steam-art/header.jpg").to_vec(), "jpg"),
        ("_logo", include_bytes!("../steam-art/logo.png").to_vec(), "png"),
    ];
    if was_running {
        if let Some(why) = shortcuts::cant_restart_steam() {
            return Err(Error::Msg(why.into()));
        }
        trail::note("Add to Steam (launcher): closing Steam");
        shortcuts::close_steam(&steam).await.inspect_err(|e| trail::note(format!("Add to Steam: {e}")))?;
    }
    let n = shortcuts::launcher_to_steam(&steam, &art)?;
    trail::note(format!("Add to Steam (launcher): written for {n} account(s)"));
    if was_running {
        shortcuts::start_steam(&steam);
    }
    Ok(format!("added:{n}"))
}

// ---------- playing ----------

// Games being started right now, so a second Play while the first is still on its way does nothing.
static STARTING: Mutex<Vec<String>> = Mutex::new(Vec::new());
struct Starting(String);
impl Drop for Starting {
    fn drop(&mut self) {
        STARTING.lock().unwrap().retain(|x| x != &self.0);
    }
}
// Each start of a game has its own number. When it closes, it only tidies up after itself: Play
// pressed again while the last one is still closing must not lose the new one.
static RUNS: Mutex<Option<HashMap<String, u64>>> = Mutex::new(None);
static RUN_NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub(crate) fn start_game(app: &AppHandle, id: &str) -> Res<()> {
    let st = app.state::<App>();
    if st.moving.lock().unwrap().contains(id) {
        return Err(Error::Msg(MOVING.into()));
    }
    let _starting = {
        let mut s = STARTING.lock().unwrap();
        if s.iter().any(|x| x == id) || st.running.lock().unwrap().contains_key(id) {
            return Ok(());
        }
        s.push(id.to_string());
        Starting(id.to_string())
    };
    let g = st.lib.lock().unwrap().installed.get(id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    // Set Up By Hand: the player's own way, instead of the launcher's.
    let manual = g.manual.clone();
    // A recomp the launcher knows how to set up: the player's copy goes where the game reads it,
    // and the game starts straight away instead of its own launcher.
    let prof = if manual.is_some() { None } else { profile_of(&st, id, &g) };
    // Its own installer or first-time setup, the first time: that opens instead of the game.
    // (A developer's own profile sets the game up itself; one made from the catalog's boot profile
    // still runs the release's setup first.)
    if manual.is_none() && !prof.as_ref().map(|p| p.start.is_some() && !p.from_boot).unwrap_or(false) {
        if let Some(setup) = own_setup::pending(&st, id, &g) {
            // A setup that takes the player's copy (an ISO dropped on install.bat): asked for first.
            let copy = match prof.as_ref().filter(|p| p.game.as_ref().map(|c| c.for_setup).unwrap_or(false)) {
                Some(p) => game_copy(&st, id, &g, p)?,
                None => None,
            };
            return own_setup::run(app, id, &g, &setup, copy.as_deref());
        }
    }
    let g = st.lib.lock().unwrap().installed.get(id).cloned().unwrap_or(g);
    let mut args: Vec<String> = vec![];
    let mut game_env: Vec<(String, String)> = vec![];
    note_pad();
    let planned = match &prof {
        _ if manual.is_some() => manual_plan(&g, manual.as_ref().unwrap())?,
        Some(p) if p.start.is_some() => {
            let copy = game_copy(&st, id, &g, p)?;
            p.prepare(&g.path, copy.as_deref()).map_err(|e| Error::Msg(format!("Couldn't write its settings: {e}")))?;
            p.place(&g.path, copy.as_deref()).map_err(|e| Error::Msg(format!("Couldn't put your copy of the game where it reads it: {e}")))?;
            // Everything the game reads has to be there now; if not, the copy picked isn't the
            // right folder (the files are one level down, or it's a different disc): pick again.
            let missing = p.missing_after_place(&g.path);
            if copy.is_some() && !missing.is_empty() {
                if let Some(x) = st.lib.lock().unwrap().installed.get_mut(id) {
                    x.game_copy = None;
                }
                st.save();
                let label = p.game.as_ref().map(|c| c.label.clone()).unwrap_or_default();
                return Err(Error::Msg(format!("NEED_COPY|{label}")));
            }
            // Its update, when it needs one: put in beside the copy, or asked for.
            if let Some(up) = p.game.as_ref().and_then(|c| c.update.as_ref()) {
                match g.update_copy.clone().filter(|c| c.exists()) {
                    Some(u) => up.place(&g.path, &u).map_err(|e| Error::Msg(format!("Couldn't put the update where the game reads it: {e}")))?,
                    None if up.optional || up.in_place(&g.path).is_some() => {}
                    None => return Err(Error::Msg(format!("NEED_UPDATE|{}", up.label))),
                }
            }
            game_env = p.env_vars(&g.path, copy.as_deref());
            p.command(&g.path, copy.as_deref(), &g.picks)
        }
        // Settings only: the program starts as usual, with the settings on the command line.
        Some(p) => {
            if !p.from_boot {
                p.prepare(&g.path, None).map_err(|e| Error::Msg(format!("Couldn't write its settings: {e}")))?;
            }
            args = p.setting_args(&g.path, &g.picks);
            None
        }
        _ => None,
    };
    // The program picked for this one start (held Play): as it is, no settings on its command line.
    let once = ONCE_EXE.lock().unwrap().as_mut().and_then(|m| m.remove(id));
    let planned = match once {
        Some(rel) => Some((g.path.join(rel), vec![])),
        None => planned,
    };
    let (exe, exe_rel) = match planned {
        Some((exe, a)) => {
            args = a;
            let rel = exe.strip_prefix(&g.path).map(Path::to_path_buf).unwrap_or(exe.clone());
            (exe, rel)
        }
        None => {
            let exe_rel = g.exe.clone().ok_or_else(|| Error::Msg("The launcher couldn't tell which program starts the game. Pick it in the game's settings.".into()))?;
            (g.path.join(&exe_rel), exe_rel)
        }
    };
    if !exe.is_file() {
        return Err(Error::Msg(format!("{} is missing. Reinstall the game, or pick its program in the game's settings.", exe_rel.display())));
    }
    // Troubleshoot's Clean Start: this once, without the player's launch options and Look.
    let clean = {
        let mut c = CLEAN_START.lock().unwrap();
        let had = c.iter().any(|x| x == id);
        c.retain(|x| x != id);
        had
    };
    if !clean {
        args.extend(join_paths(split_args(&g.args)));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(m) = std::fs::metadata(&exe) {
            if m.permissions().mode() & 0o100 == 0 {
                let _ = std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(m.permissions().mode() | 0o755));
            }
        }
    }
    let cwd = exe.parent().map(Path::to_path_buf).unwrap_or(g.path.clone());
    let shown = exe_rel.display().to_string();
    // The player's look: ReShade where the game will load it, for this start only.
    let mut look_route: Option<&'static str> = None;
    let look_run = match if clean { Ok(None) } else { look_plan(&st, id, &g, prof.as_ref(), &exe) } {
        Ok(Some((look, lines, api))) => {
            // A game that can go either way: the way in that last worked for it.
            let (api, route) = looks::route_for(&st.dir, id, api);
            look_route = route.filter(|_| cfg!(windows));
            match if cfg!(windows) { looks::prepare(&st.dir, id, &exe, look, g.look_amount, lines, api) } else { looks::prepare_vkbasalt(&st.dir, id, look, g.look_amount, lines) } {
                Ok(r) => Some(r),
                Err(e) => {
                    let _ = app.emit("toast", &e);
                    ingame::notify(app, "The look is off this time", &e);
                    None
                }
            }
        }
        Ok(None) => {
            looks::tidy(&st.dir, id);
            None
        }
        Err(e) => {
            looks::tidy(&st.dir, id);
            if let Some(e) = e {
                let _ = app.emit("toast", &e);
            }
            None
        }
    };
    // Games that don't say where they save: what's in the likely places now, to see what changes.
    let watch = {
        let spots = cloud::spots_of(&st, id, &g);
        if matches!(spots.first().map(|s| s.from), Some("game") | Some("found")) {
            let exe_name = exe.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let repo_name = g.repo.rsplit('/').next().unwrap_or("").to_string();
            Some(saves::watch_start(&g, &[&g.name, &exe_name, &repo_name]))
        } else {
            None
        }
    };
    // The same program already open outside the launcher (started by hand, or still closing):
    // a second copy fights the first over its files (a first-run build fails to link, saves clash).
    if already_open(&exe) {
        let name = exe.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        return Err(Error::Msg(format!("{name} is already open. Close it (or wait for it to finish closing), then press Play.")));
    }
    // A Windows script (.cmd, .bat) on Linux: the build's own Linux start script beside it when it
    // has one (a profile written for Windows names Launcher.cmd; the Linux build has launcher.sh).
    let exe = if !cfg!(windows) && matches!(exe.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).as_deref(), Some("cmd" | "bat")) { linux_twin(&exe).unwrap_or(exe) } else { exe };
    // A Python script starts through its interpreter.
    let ext = exe.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let mut cmd = match ext.as_str() {
        "py" if cfg!(windows) => {
            let mut c = tokio::process::Command::new("py");
            c.arg("-3").arg(&exe);
            c
        }
        "py" => {
            let mut c = tokio::process::Command::new("python3");
            c.arg(&exe);
            c
        }
        // A shell script on Linux (it may have lost its run permission in the zip). Through bash:
        // most start scripts use bash's own syntax, which plain sh (dash on many systems) can't read.
        "sh" if !cfg!(windows) => {
            let bash = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).any(|d| d.join("bash").is_file())).unwrap_or(false);
            let mut c = tokio::process::Command::new(if bash { "bash" } else { "sh" });
            c.arg(&exe);
            c
        }
        // A Windows program (or script) on Linux: through Proton (or Wine).
        "exe" | "cmd" | "bat" if !cfg!(windows) => match proton::runner() {
            Some(r) => proton::command(&r, &st.dir, &exe),
            None => return Err(Error::Msg("This is a Windows version, and it needs Proton to play on Linux. Install Steam and Proton (or umu-launcher, or Wine), then press Play again.".into())),
        },
        _ => tokio::process::Command::new(&exe),
    };
    sysenv::clean_tokio(&mut cmd);
    cmd.args(&args).current_dir(&cwd).kill_on_drop(false);
    // What the game prints, kept for this start: a game that closes right away usually says why
    // there, and the crash report takes it along.
    // Not for a game that asks its questions in its text window: what it prints has to stay there.
    let console = boot::of(&st.dir, id).map(|b| b.console).unwrap_or(false);
    if let Some(f) = game_output_file(&st.dir, id).filter(|_| !console) {
        if let Ok(f2) = f.try_clone() {
            cmd.stdout(std::process::Stdio::from(f)).stderr(std::process::Stdio::from(f2));
        }
    }
    // So a game can tell it was started from here, and from where: it can open full screen with
    // its controller menus in Big Picture mode, say. The game's own `start.env` comes after.
    cmd.env("RECOMPLAUNCHER", "1")
        .env("RECOMPLAUNCHER_VERSION", env!("CARGO_PKG_VERSION"))
        .env("RECOMPLAUNCHER_MODE", if BIG_PICTURE.load(Ordering::Relaxed) { "bigpicture" } else { "desktop" })
        .env("RECOMPLAUNCHER_PAD", profile::pad_now());
    // A Python set up elsewhere on the PC (PYTHONHOME, PYTHONPATH) breaks the Python some games
    // carry inside them ("No module named 'encodings'"). Games with their own Python never want them.
    if ext != "py" {
        cmd.env_remove("PYTHONHOME").env_remove("PYTHONPATH");
    }
    for (k, v) in &game_env {
        cmd.env(k, v);
    }
    if let Some(r) = &look_run {
        for (k, v) in &r.env {
            cmd.env(k, v);
        }
    }
    note_run(&st, id, &g, &exe, &args, &cwd, prof.as_ref(), look_run.is_some());
    trail::note(format!("Play {id}: {}{}", exe.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), if look_run.is_some() { " with a Look" } else { "" }));
    let spawned = cmd.spawn();
    if let Err(e) = &spawned {
        trail::note(format!("Play {id}: didn't start: {e}"));
    }
    if spawned.is_err() {
        if let Some(p) = look_run.as_ref().and_then(|r| r.placed.as_ref()) {
            looks::finish(&st.dir, id, p);
        }
    }
    let mut child = spawned.map_err(|e| {
        if is_blocked(&e) {
            defender_msg(e, &shown)
        } else {
            Error::Msg(format!("Couldn't start {shown}: {e}"))
        }
    })?;
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let id = id.to_string();
    let run = RUN_NEXT.fetch_add(1, Ordering::Relaxed);
    RUNS.lock().unwrap().get_or_insert_with(HashMap::new).insert(id.clone(), run);
    st.running.lock().unwrap().insert(id.clone(), tx);
    pads::PLAYING.store(true, Ordering::Relaxed);
    if let Some(pid) = child.id() {
        st.pids.lock().unwrap().insert(id.clone(), pid);
    }
    let started = now();
    st.started.lock().unwrap().insert(id.clone(), started);
    let placed = look_run.as_ref().and_then(|r| r.placed.clone());
    if look_run.is_some() {
        st.looked.lock().unwrap().insert(id.clone());
    } else {
        st.looked.lock().unwrap().remove(&id);
    }
    let (quick_menu, minimize) = {
        let lib = st.lib.lock().unwrap();
        (lib.settings.quick_menu, lib.settings.minimize_playing.unwrap_or_else(|| HANDHELD.load(Ordering::Relaxed)))
    };
    pads::QUICK_MENU.store(quick_menu, Ordering::Relaxed);
    let marker = playing_file(&st.dir, &id);
    let _ = std::fs::create_dir_all(marker.parent().unwrap_or(&st.dir));
    let _ = std::fs::write(&marker, b"");
    ingame::set_hotkeys(app, true, quick_menu);
    let _ = app.emit("game", json!({"id": id, "state": "running"}));
    // From Big Picture mode the launcher fills the screen, so it steps aside for the game (its first
    // window, or a dialog asking for the player's copy, would open behind it). It comes back when
    // the game closes.
    // The player can ask for the same on the desktop (Settings, While Playing).
    let from_bp = BIG_PICTURE.load(Ordering::Relaxed) || minimize;
    if from_bp {
        let a = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(700)).await;
            if let Some(w) = a.get_webview_window("main") {
                let _ = w.minimize();
            }
        });
    }
    ping::game_start(app, &id);
    // The first few times, say how to open the quick menu.
    let tips = {
        let mut l = st.lib.lock().unwrap();
        l.settings.menu_tips += 1;
        l.settings.menu_tips
    };
    st.save();
    if tips <= 3 {
        ingame::notify(app, "Quick menu", "Hold View on a controller (Create on PlayStation), or press Ctrl+F12, while you play.");
    }
    // Discord: what's being played.
    if g_discord(&st) {
        let (app_id, game, cover) = discord_info(&st, &id);
        discord::send(discord::Msg::Playing { app_id, game, recomp: g.name.clone(), cover, start: started });
    }
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let start = Instant::now();
        let code = tokio::select! {
            r = child.wait() => r.ok().and_then(|s| s.code()),
            _ = rx => { let _ = child.kill().await; None }
        };
        let secs = start.elapsed().as_secs();
        let st = app2.state::<App>();
        if let Some(route) = look_route {
            looks::after_run(&st.dir, &id, route, secs, code);
        }
        if let Some(p) = &placed {
            let (dir, id2, p) = (st.dir.clone(), id.clone(), p.clone());
            let _ = tokio::task::spawn_blocking(move || looks::finish(&dir, &id2, &p)).await;
        }
        // Still this start's? Play may have been pressed again since Stop.
        let mine = {
            let mut r = RUNS.lock().unwrap();
            let m = r.get_or_insert_with(HashMap::new);
            let mine = m.get(&id) == Some(&run);
            if mine {
                m.remove(&id);
            }
            mine
        };
        if mine {
            st.running.lock().unwrap().remove(&id);
            st.pids.lock().unwrap().remove(&id);
            st.started.lock().unwrap().remove(&id);
        }
        pads::PLAYING.store(!st.running.lock().unwrap().is_empty(), Ordering::Relaxed);
        if !st.running.lock().unwrap().contains_key(&id) {
            let _ = std::fs::remove_file(playing_file(&st.dir, &id));
        }
        if let Some(g) = st.lib.lock().unwrap().installed.get_mut(&id) {
            g.played_secs += secs;
            g.last_played = now();
            if secs >= 10 {
                g.sessions.push([started, secs]);
                let n = g.sessions.len();
                if n > 500 {
                    g.sessions.drain(..n - 500);
                }
            }
        }
        st.save();
        if st.running.lock().unwrap().is_empty() {
            ingame::set_hotkeys(&app2, false, false);
            ingame::hide_overlay(&app2);
            discord::send(discord::Msg::Clear);
            if from_bp && mine {
                show_main(&app2);
            }
        }
        // What the game's exit code means, when its profile says.
        let said = code.and_then(|c| prof.as_ref().and_then(|p| p.exit_codes.get(&(c as u32).to_string()).cloned()));
        if mine {
            let _ = app2.emit("game", json!({"id": id, "state": "stopped", "secs": secs, "code": code, "started": started, "said": said}));
            ping::game_stop(&app2, &id, secs);
        }
        // Where it saved (when it didn't say), then a copy of its saves, and up to the cloud.
        if let Some(w) = watch {
            if let Ok(found) = tokio::task::spawn_blocking(move || saves::watch_end(w)).await {
                if !found.is_empty() {
                    let mut l = st.lib.lock().unwrap();
                    if let Some(g) = l.installed.get_mut(&id) {
                        for f in found {
                            if !g.found_saves.contains(&f) {
                                g.found_saves.push(f);
                            }
                        }
                    }
                    let _ = l.save(&st.dir);
                }
            }
        }
        // Usage: the session, and a game that closed with an error right after starting.
        {
            let version = st.lib.lock().unwrap().installed.get(&id).map(|g| g.tag.clone()).unwrap_or_default();
            let e = |kind: &str| usage::Event { kind: kind.into(), at: now(), game: id.clone(), version: version.clone(), secs, code: code.map(|c| c as i64), ..Default::default() };
            if secs < 15 && code.map(|c| c != 0).unwrap_or(false) {
                usage::event(&app2, e("crash")).await;
            } else if secs >= 30 {
                usage::event(&app2, e("session")).await;
            }
        }
        if secs >= 5 {
            match cloud::after_session(&app2, &id).await {
                Ok(Some(_)) => {
                    let _ = app2.emit("saves", json!({"id": id}));
                }
                Ok(None) => {}
                Err(e) => eprintln!("Save copy for {id}: {e}"),
            }
        }
        // Started from a shortcut with no window: the launcher's job is done.
        if st.headless.load(Ordering::Relaxed) && st.running.lock().unwrap().is_empty() {
            app2.exit(0);
        }
    });
    Ok(())
}

// ---------- setting a recomp up ----------

/// Launch options as typed: words, with "quoted words" kept together.
fn split_args(s: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let (mut quoted, mut any) = (false, false);
    for c in s.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => {
                cur.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(cur);
    }
    out
}

/// A path with spaces typed without quotes (`D:\Games\CTR Nitro-Fueled\boot.bin`) comes out of
/// split_args in pieces: put them back together when together they're a file or folder that's there
/// and the first piece alone isn't.
fn join_paths(args: Vec<String>) -> Vec<String> {
    let looks_path = |s: &str| {
        let p = s.rsplit_once('=').map(|(_, v)| v).filter(|_| s.starts_with('-')).unwrap_or(s);
        let b = p.as_bytes();
        (b.len() > 2 && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')) || p.starts_with("\\\\") || p.starts_with('/')
    };
    let exists = |s: &str| {
        let p = s.rsplit_once('=').map(|(_, v)| v).filter(|_| s.starts_with('-')).unwrap_or(s);
        Path::new(p).exists()
    };
    let mut out = vec![];
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if looks_path(a) && !exists(a) {
            let mut joined = a.clone();
            let mut found = None;
            for j in i + 1..args.len().min(i + 12) {
                joined.push(' ');
                joined.push_str(&args[j]);
                if exists(&joined) {
                    found = Some((j, joined.clone()));
                }
            }
            if let Some((j, s)) = found {
                out.push(s);
                i = j + 1;
                continue;
            }
        }
        out.push(a.clone());
        i += 1;
    }
    out
}

/// `{pad}` in profiles: the controller that's plugged in.
fn note_pad() {
    let ps = pads::connected().first().map(|p| p.kind == "ps").unwrap_or(false);
    profile::set_pad(if ps { "playstation" } else { "xbox" });
}

// Each setup run has its own number; cancelling, or starting another for the same game, ends it.
static SETUP_RUN: Mutex<Option<HashMap<String, u64>>> = Mutex::new(None);
static SETUP_NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn setup_stopped(id: &str, run: u64) -> bool {
    SETUP_RUN.lock().unwrap().as_ref().and_then(|m| m.get(id).copied()) != Some(run)
}

#[derive(Serialize, Clone, Default)]
struct SetupEv {
    id: String,
    /// "read", "verify", "extras", "setup", "done" or "error".
    step: String,
    /// The step's progress, 0 to 1.
    pct: f32,
    done: u64,
    total: u64,
    /// What's happening, or what was found.
    text: String,
    /// The known dump it matched ("USA"), when it did.
    verified: String,
    /// Whether this setup has its own step (the recomp's prepare program).
    extras: bool,
}

/// Set a game up with the player's copy: check it's the right game, read the whole thing to make
/// sure it's complete (and compare it with known good dumps), run the recomp's own setup step if it
/// has one, and write the game's settings. Progress comes back as `setup` events.
#[tauri::command(async)]
fn setup_copy(app: AppHandle, st: State<'_, App>, id: String, path: String) -> Res<()> {
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let p = profile_of(&st, &id, &g).ok_or_else(|| Error::Msg("The launcher can't set this game up yet.".into()))?;
    let copy = PathBuf::from(&path);
    let run = SETUP_NEXT.fetch_add(1, Ordering::Relaxed);
    SETUP_RUN.lock().unwrap().get_or_insert_with(HashMap::new).insert(id.clone(), run);
    std::thread::spawn(move || {
        let stopped = || setup_stopped(&id, run);
        // A disc image for a recomp that reads extracted files: the launcher unpacks it.
        let unpack = p.game.as_ref().map(|x| x.extract.clone()).filter(|x| !x.is_empty() && copy.is_file());
        let extras = p.prepare.is_some() || unpack.is_some();
        let emit = |ev: SetupEv| {
            let _ = app.emit("setup", SetupEv { id: id.clone(), extras, ..ev });
        };
        let fail = |text: String| {
            if !stopped() {
                emit(SetupEv { step: "error".into(), text, ..Default::default() })
            }
        };
        // 1. Is it the right game?
        emit(SetupEv { step: "read".into(), ..Default::default() });
        if !copy.exists() {
            return fail("That file isn't there anymore.".into());
        }
        if let Some(need) = &p.game {
            if let Err(e) = profile::check_copy(need, &copy) {
                return fail(e);
            }
        }
        let name = copy.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        emit(SetupEv { step: "read".into(), pct: 1.0, text: name, ..Default::default() });
        // 2. All of it there?
        let (verified, text) = if copy.is_file() {
            let total = std::fs::metadata(&copy).map(|m| m.len()).unwrap_or(0);
            let Ok(mut f) = std::fs::File::open(&copy) else { return fail("Couldn't open it.".into()) };
            let mut h = crc32fast::Hasher::new();
            let mut buf = vec![0u8; 4 << 20];
            let (mut done, mut last) = (0u64, Instant::now());
            loop {
                if stopped() {
                    return;
                }
                let n = match std::io::Read::read(&mut f, &mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(e) => return fail(format!("Part of it couldn't be read ({e}). The copy may be damaged.")),
                };
                h.update(&buf[..n]);
                done += n as u64;
                if last.elapsed().as_millis() >= 80 {
                    last = Instant::now();
                    emit(SetupEv { step: "verify".into(), pct: done as f32 / total.max(1) as f32, done, total, ..Default::default() });
                }
            }
            let crc = format!("{:08x}", h.finalize());
            let dumps = p.game.as_ref().map(|x| x.dumps.clone()).unwrap_or_default();
            match dumps.iter().find(|d| d.crc32.eq_ignore_ascii_case(&crc) && (d.size == 0 || d.size == total)) {
                Some(d) => (d.name.clone(), format!("Matches the verified {} disc", d.name)),
                None if dumps.is_empty() => (String::new(), format!("Read all {}, no errors", human_size(total))),
                None => (String::new(), "Read it all, no errors. It isn't a known clean dump, but it may still work.".into()),
            }
        } else {
            let mut stack = vec![copy.clone()];
            let (mut files, mut size) = (0u64, 0u64);
            while let Some(d) = stack.pop() {
                if stopped() {
                    return;
                }
                for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                    match e.metadata() {
                        Ok(m) if m.is_dir() => stack.push(e.path()),
                        Ok(m) => {
                            files += 1;
                            size += m.len();
                        }
                        _ => {}
                    }
                }
            }
            (String::new(), format!("Checked {files} files, {}", human_size(size)))
        };
        if stopped() {
            return;
        }
        emit(SetupEv { step: "verify".into(), pct: 1.0, text: text.clone(), verified: verified.clone(), ..Default::default() });
        // 3. Unpacking the disc, where the recomp reads it.
        let mut copy = copy;
        if let Some(kind) = &unpack {
            let into = p.game.as_ref().map(|x| x.into.trim().trim_matches(['/', '\\']).to_string()).unwrap_or_default();
            let dest = if into == "." { g.path.clone() } else if !into.is_empty() && !into.contains("..") { g.path.join(&into) } else { g.path.join("game-files") };
            emit(SetupEv { step: "extras".into(), text: "Unpacking your disc".into(), ..Default::default() });
            let need = match extract::size(&copy, kind) {
                Ok(n) => n,
                Err(e) => return fail(format!("Couldn't read the disc image: {e}")),
            };
            if let Some(free) = manage::free_space(&g.path) {
                if free < need + (256 << 20) {
                    return fail(format!("Unpacking it needs {} free on that drive ({} free).", human_size(need), human_size(free)));
                }
            }
            let done = extract::unpack(&copy, kind, &dest, &|d, t| emit(SetupEv { step: "extras".into(), pct: d as f32 / t.max(1) as f32, done: d, total: t, text: "Unpacking your disc".into(), ..Default::default() }), &stopped);
            if let Err(e) = done {
                if stopped() {
                    return;
                }
                return fail(format!("Couldn't unpack the disc: {e}"));
            }
            if let Some(f) = p.game.as_ref().map(|x| x.check.file.clone()).filter(|f| !f.is_empty()) {
                if !dest.join(&f).is_file() {
                    return fail(format!("The disc unpacked, but it doesn't have {f}. It may be a different game."));
                }
            }
            copy = dest;
        }
        // 4. The recomp's own setup step.
        if let Some(pr) = &p.prepare {
            emit(SetupEv { step: "extras".into(), text: pr.label.clone(), ..Default::default() });
            if let Err(e) = run_prepare(pr, &g.path, &copy, &|pct, t| emit(SetupEv { step: "extras".into(), pct, text: t, ..Default::default() }), &stopped) {
                return fail(e);
            }
        }
        if stopped() {
            return;
        }
        // 4. Its settings, and remembering the copy.
        emit(SetupEv { step: "setup".into(), ..Default::default() });
        if stopped() {
            return;
        }
        note_pad();
        if let Err(e) = p.prepare(&g.path, Some(&copy)) {
            return fail(format!("Couldn't write its settings: {e}"));
        }
        let st = app.state::<App>();
        if let Some(x) = st.lib.lock().unwrap().installed.get_mut(&id) {
            x.game_copy = Some(copy.clone());
        }
        st.save();
        if let Some(m) = SETUP_RUN.lock().unwrap().as_mut() {
            if m.get(&id) == Some(&run) {
                m.remove(&id);
            }
        }
        emit(SetupEv { step: "done".into(), pct: 1.0, text, verified, ..Default::default() });
    });
    Ok(())
}

#[tauri::command(async)]
fn setup_cancel(id: String) {
    if let Some(m) = SETUP_RUN.lock().unwrap().as_mut() {
        m.remove(&id);
    }
}

fn human_size(b: u64) -> String {
    let gb = b as f64 / 1_073_741_824.0;
    if gb >= 1.0 {
        format!("{gb:.1} GB")
    } else {
        format!("{:.0} MB", b as f64 / 1_048_576.0)
    }
}

/// Run a recomp's prepare program, reading `progress 0.4 text` lines from it.
fn run_prepare(pr: &profile::Prepare, install: &Path, copy: &Path, report: &dyn Fn(f32, String), stopped: &dyn Fn() -> bool) -> Result<(), String> {
    use std::io::BufRead;
    let exe = pr.program.iter().map(|x| install.join(x)).find(|x| x.is_file()).ok_or("Its setup program is missing. Reinstall the game.")?;
    let args: Vec<String> = pr.args.iter().map(|a| profile::fill(a, install, Some(copy))).collect();
    let mut c = std::process::Command::new(&exe);
    sysenv::clean_std(&mut c);
    c.args(&args).current_dir(exe.parent().unwrap_or(install)).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let mut child = c.spawn().map_err(|e| if is_blocked(&e) { "Windows Security stopped its setup program.".to_string() } else { format!("Couldn't run its setup program: {e}") })?;
    let out = child.stdout.take();
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        if let Some(o) = out {
            for l in std::io::BufReader::new(o).lines().map_while(Result::ok) {
                let _ = tx.send(l);
            }
        }
    });
    let start = Instant::now();
    loop {
        while let Ok(l) = rx.try_recv() {
            let mut w = l.splitn(3, ' ');
            if w.next() == Some("progress") {
                let pct = w.next().and_then(|x| x.parse::<f32>().ok()).unwrap_or(0.0).clamp(0.0, 1.0);
                report(pct, w.next().unwrap_or("").trim().to_string());
            }
        }
        if let Ok(Some(s)) = child.try_wait() {
            return if s.success() { Ok(()) } else { Err(format!("Its setup step stopped with code {}.", s.code().unwrap_or(-1))) };
        }
        if stopped() || start.elapsed().as_secs() > pr.timeout {
            let _ = child.kill();
            return Err(if stopped() { "Stopped.".into() } else { "Its setup step took too long.".into() });
        }
        std::thread::sleep(std::time::Duration::from_millis(60));
    }
}

/// Catalog games the launcher can set up itself (copy and settings): the "Easy setup" ones.
#[tauri::command(async)]
fn easy_setup(st: State<'_, App>) -> Vec<String> {
    let cat = cached_catalog(&st);
    let dirs: HashMap<String, PathBuf> = st.lib.lock().unwrap().installed.values().map(|g| (g.id.clone(), g.path.clone())).collect();
    let none = PathBuf::new();
    cat.get("apps")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    let id = x.get("id")?.as_str()?;
                    let repo = x.get("repo").and_then(Value::as_str).unwrap_or("");
                    let p = profile::find(dirs.get(id).unwrap_or(&none), repo, x)?;
                    p.game.is_some().then(|| id.to_string())
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The player's own launch options and automatic updates for a game.
#[tauri::command(async)]
fn set_game_options(st: State<'_, App>, id: String, args: Option<String>, auto_update: Option<bool>) -> Res<Installed> {
    let g = {
        let mut l = st.lib.lock().unwrap();
        let g = l.installed.get_mut(&id).ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
        if let Some(a) = args {
            g.args = a.trim().to_string();
        }
        if let Some(a) = auto_update {
            g.auto_update = a;
        }
        g.clone()
    };
    st.save();
    Ok(g)
}

// ---------- looks ----------

#[derive(Serialize)]
struct LooksInfo {
    /// Looks can be used with this game here.
    ok: bool,
    /// Why not, when they can't.
    why: String,
    look: String,
    amount: u8,
    /// Lines for the CRT looks.
    lines: u32,
    /// The ReShade the launcher has.
    reshade: Option<String>,
    /// The player's newest screenshot without a look on, for the preview.
    shot: Option<String>,
}

/// Data the program carries compressed (see build.rs), as text again.
pub fn unpack(z: &[u8]) -> String {
    use std::io::Read;
    let mut s = String::new();
    let _ = flate2::read::DeflateDecoder::new(z).read_to_string(&mut s);
    s
}

/// The window's own data files, for when it can't read them itself (a strict content policy):
/// the looks and each language's text.
const UI_LANGS: &[(&str, &[u8])] = &[
    ("ar", include_bytes!(concat!(env!("OUT_DIR"), "/i18n-ar.json.z"))),
    ("de", include_bytes!(concat!(env!("OUT_DIR"), "/i18n-de.json.z"))),
    ("en-GB", include_bytes!(concat!(env!("OUT_DIR"), "/i18n-en-GB.json.z"))),
    ("es-419", include_bytes!(concat!(env!("OUT_DIR"), "/i18n-es-419.json.z"))),
    ("es", include_bytes!(concat!(env!("OUT_DIR"), "/i18n-es.json.z"))),
    ("fr-CA", include_bytes!(concat!(env!("OUT_DIR"), "/i18n-fr-CA.json.z"))),
    ("fr", include_bytes!(concat!(env!("OUT_DIR"), "/i18n-fr.json.z"))),
    ("id", include_bytes!(concat!(env!("OUT_DIR"), "/i18n-id.json.z"))),
    ("it", include_bytes!(concat!(env!("OUT_DIR"), "/i18n-it.json.z"))),
    ("ja", include_bytes!(concat!(env!("OUT_DIR"), "/i18n-ja.json.z"))),
    ("ko", include_bytes!(concat!(env!("OUT_DIR"), "/i18n-ko.json.z"))),
    ("nl", include_bytes!(concat!(env!("OUT_DIR"), "/i18n-nl.json.z"))),
    ("pl", include_bytes!(concat!(env!("OUT_DIR"), "/i18n-pl.json.z"))),
    ("pt-BR", include_bytes!(concat!(env!("OUT_DIR"), "/i18n-pt-BR.json.z"))),
    ("pt", include_bytes!(concat!(env!("OUT_DIR"), "/i18n-pt.json.z"))),
    ("ru", include_bytes!(concat!(env!("OUT_DIR"), "/i18n-ru.json.z"))),
    ("th", include_bytes!(concat!(env!("OUT_DIR"), "/i18n-th.json.z"))),
    ("tr", include_bytes!(concat!(env!("OUT_DIR"), "/i18n-tr.json.z"))),
    ("uk", include_bytes!(concat!(env!("OUT_DIR"), "/i18n-uk.json.z"))),
    ("vi", include_bytes!(concat!(env!("OUT_DIR"), "/i18n-vi.json.z"))),
    ("zh-Hans", include_bytes!(concat!(env!("OUT_DIR"), "/i18n-zh-Hans.json.z"))),
    ("zh-Hant", include_bytes!(concat!(env!("OUT_DIR"), "/i18n-zh-Hant.json.z"))),
];

#[tauri::command(async)]
fn ui_data(name: String) -> Res<Value> {
    let text = if name == "looks" {
        looks::LOOKS_JSON.to_string()
    } else {
        UI_LANGS.iter().find(|(c, _)| *c == name).map(|(_, t)| unpack(t)).ok_or_else(|| Error::Msg("Unknown file.".into()))?
    };
    serde_json::from_str(&text).map_err(|e| Error::Msg(e.to_string()))
}

#[tauri::command(async)]
fn looks_info(st: State<'_, App>, id: String) -> Res<LooksInfo> {
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let prof = profile_of(&st, &id, &g);
    let pl = prof.as_ref().and_then(|p| p.looks.clone());
    let why = if cfg!(target_os = "macos") {
        "Looks work on Windows and Linux.".to_string()
    } else if !cfg!(windows) && !looks::vkbasalt_installed() {
        VKBASALT_MISSING.to_string()
    } else if let Some(pl) = pl.as_ref().filter(|p| !p.allowed) {
        if pl.reason.is_empty() { "This game doesn't work with looks.".into() } else { pl.reason.clone() }
    } else {
        String::new()
    };
    let cat = cached_catalog(&st);
    let lines = pl.as_ref().map(|p| p.lines).filter(|&n| n > 0).unwrap_or_else(|| looks::lines_for(catalog::app(&cat, &id).map(|e| str_of(e, "origin")).unwrap_or("")));
    let skip = looks::shots_with_look(&st.dir, &id);
    let shot = ingame::list_shots(&g.name).into_iter().find(|p| !skip.iter().any(|s| p.file_name().and_then(|n| n.to_str()) == Some(s))).map(|p| p.display().to_string());
    Ok(LooksInfo { ok: why.is_empty(), why, look: g.look.clone(), amount: g.look_amount, lines, reshade: looks::have_version(&st.dir), shot })
}

#[tauri::command(async)]
fn set_look(st: State<'_, App>, id: String, look: String, amount: Option<u8>) -> Res<Installed> {
    if !look.is_empty() && looks::find(&look).is_none() {
        return Err(Error::Msg("That look isn't one the launcher has.".into()));
    }
    let g = {
        let mut l = st.lib.lock().unwrap();
        let g = l.installed.get_mut(&id).ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
        g.look = look;
        if let Some(a) = amount {
            g.look_amount = a.min(100);
        }
        g.clone()
    };
    st.save();
    Ok(g)
}

/// A screenshot as a picture the preview can draw on (a data address, so it's never off limits to it).
#[tauri::command]
async fn look_picture(path: String) -> Res<String> {
    let p = shot_ok(&path)?;
    tauri::async_runtime::spawn_blocking(move || -> Res<String> {
        let img = image::open(&p).map_err(|e| Error::Msg(e.to_string()))?;
        let img = if img.width() > 1920 { img.resize(1920, 1920 * img.height() / img.width().max(1), image::imageops::FilterType::Triangle) } else { img };
        let mut out = std::io::Cursor::new(Vec::new());
        img.to_rgb8().write_to(&mut out, image::ImageFormat::Jpeg).map_err(|e| Error::Msg(e.to_string()))?;
        Ok(format!("data:image/jpeg;base64,{}", media::base64(out.get_ref())))
    })
    .await
    .map_err(|e| Error::Msg(e.to_string()))?
}

fn cached_catalog(st: &App) -> Value {
    let cat = std::fs::read(st.dir.join("cache/pc-catalog.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    catalog::with_mine(&st.dir, cat)
}

/// What a GitHub repo is, read before it's added: the catalog's entry when it has one; else the
/// game and console its description, topics and README point at, a catalog game it looks like, and
/// art from SteamGridDB when the player gave their key.
#[tauri::command]
async fn github_lookup(st: State<'_, App>, link: String, sgdb_key: Option<String>) -> Res<Value> {
    let repo = catalog::repo_from_link(&link).ok_or_else(|| Error::Msg("That isn't a GitHub link. It looks like github.com/owner/name.".into()))?;
    let cat = catalog::catalog(&st.http, &st.dir, false).await?.data;
    let apps = cat.get("apps").and_then(Value::as_array).cloned().unwrap_or_default();
    if let Some(a) = apps.iter().find(|a| str_of(a, "repo").eq_ignore_ascii_case(&repo)) {
        return Ok(json!({"repo": repo, "known": {"id": str_of(a, "id"), "name": str_of(a, "name")}}));
    }
    let os = github_os(&st, &repo).await?;
    let get = |url: String, raw: bool| {
        let mut r = st.http.get(url).timeout(std::time::Duration::from_secs(10));
        if raw {
            r = r.header("Accept", "application/vnd.github.raw");
        }
        r.send()
    };
    let (info, readme) = tokio::join!(get(format!("https://api.github.com/repos/{repo}"), false), get(format!("https://api.github.com/repos/{repo}/readme"), true));
    let info: Value = match info {
        Ok(r) if r.status().is_success() => r.json().await.unwrap_or(Value::Null),
        _ => Value::Null,
    };
    let readme = match readme {
        Ok(r) if r.status().is_success() => r.text().await.unwrap_or_default(),
        _ => String::new(),
    };
    let desc = str_of(&info, "description").to_string();
    let topics: Vec<String> = info.get("topics").and_then(Value::as_array).map(|l| l.iter().filter_map(|t| t.as_str().map(str::to_string)).collect()).unwrap_or_default();
    // The game: a catalog game the repo names in its own words first, then what its description,
    // README and name point at. With a SteamGridDB key, the first of those SteamGridDB knows,
    // under its name there.
    let cands = ghinfo::game_candidates(&repo, &desc, &readme);
    let hay = format!(" {} ", ghinfo::norm(&format!("{} {} {}", repo.replace(['/', '-', '_'], " "), desc, readme.chars().take(1500).collect::<String>())));
    let from_cat = apps
        .iter()
        .filter_map(|a| {
            let g = str_of(a, "game");
            let n = ghinfo::norm(g);
            (n.len() >= 6 && hay.contains(&format!(" {n} "))).then(|| g.to_string())
        })
        .max_by_key(|g| g.len());
    let mut game = from_cat.clone().or_else(|| cands.first().cloned());
    let origin = ghinfo::console_of(&topics, &desc, &readme);
    // A catalog game with the same name: likely the same game, maybe another recomp of it.
    let similar = game.as_ref().and_then(|g| {
        let want = ghinfo::norm(g);
        apps.iter().find(|a| {
            let (gn, nn) = (ghinfo::norm(str_of(a, "game")), ghinfo::norm(str_of(a, "name")));
            want.len() >= 3 && (gn == want || nn == want || (want.len() >= 6 && (nn.contains(&want) || (!gn.is_empty() && want.contains(&gn) && gn.len() >= 6))))
        })
    });
    let mut art = json!({"covers": [], "heroes": [], "logos": []});
    let mut art_error = String::new();
    if let Some(k) = sgdb_key.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
        let tries: Vec<String> = from_cat.into_iter().chain(cands.iter().cloned()).take(5).collect();
        for t in tries {
            match ghinfo::find(&st.http, k, &t).await {
                Ok(Some((id, name))) => {
                    art = ghinfo::art_of(&st.http, k, id).await;
                    game = Some(name);
                    break;
                }
                Ok(None) => {}
                Err(e) => {
                    art_error = e;
                    break;
                }
            }
        }
    }
    Ok(json!({
        "repo": repo,
        "known": Value::Null,
        "os": os,
        // The project's own name; the game it's a recomp of goes in "game" (Show Game Names uses it).
        "name": catalog::name_from_repo(&repo),
        "game": game.unwrap_or_default(),
        "origin": origin,
        "blurb": desc,
        "similar": similar.map(|a| json!({"id": str_of(a, "id"), "name": str_of(a, "name")})),
        "art": art,
        "art_error": art_error,
        "hero_default": format!("https://opengraph.githubassets.com/1/{repo}"),
    }))
}

/// Art for a game from SteamGridDB, with the player's key.
#[tauri::command]
async fn github_art(st: State<'_, App>, game: String, sgdb_key: String) -> Res<Value> {
    ghinfo::art(&st.http, sgdb_key.trim(), &game).await.map_err(Error::Msg)
}

/// The system its releases have a version for: this PC's, or Windows through Proton.
async fn github_os(st: &App, repo: &str) -> Res<&'static str> {
    let rels: Vec<Release> = serde_json::from_value(catalog::releases(&st.http, &st.dir, repo, true).await.map_err(|_| Error::Msg("Couldn't read that repo's releases. Check the link, and that it's public.".into()))?.data).unwrap_or_default();
    if rels.is_empty() {
        return Err(Error::Msg("That repo hasn't put out a release yet, so there's nothing to install.".into()));
    }
    let (here, arch) = (assets::os_now(), assets::arch_now());
    let proton = here == "linux" && st.lib.lock().unwrap().settings.proton;
    let has = |os: &str| rels.iter().any(|r| !r.draft && assets::pick(&r.assets, os, arch, None).is_some());
    if has(here) {
        Ok(here)
    } else if proton && has("windows") {
        Ok("windows")
    } else {
        let name = match here {
            "windows" => "Windows",
            "macos" => "Mac",
            _ => "Linux",
        };
        Err(Error::Msg(format!("Its releases have no {name} version.")))
    }
}

/// What the player can set on a game they added themselves.
const MINE_FIELDS: &[&str] = &["name", "game", "origin", "blurb", "cover", "hero", "logo"];

/// Add a recomp by its GitHub link. One the catalog already lists is that one; another becomes the
/// player's own entry, with what its repo says about itself and what the player set (`info`: name,
/// game, origin, cover, hero, logo), once its releases have a version for this PC. Returns
/// { id, known, name }.
#[tauri::command]
async fn add_github(st: State<'_, App>, link: String, info: Option<Value>) -> Res<Value> {
    let repo = catalog::repo_from_link(&link).ok_or_else(|| Error::Msg("That isn't a GitHub link. It looks like github.com/owner/name.".into()))?;
    let cat = catalog::catalog(&st.http, &st.dir, false).await?.data;
    let same = |a: &Value| str_of(a, "repo").eq_ignore_ascii_case(&repo);
    if let Some(a) = cat.get("apps").and_then(Value::as_array).and_then(|l| l.iter().find(|a| same(a))) {
        return Ok(json!({"id": str_of(a, "id"), "known": true, "name": str_of(a, "name")}));
    }
    let os = github_os(&st, &repo).await?;
    let rels: Vec<Release> = serde_json::from_value(catalog::releases(&st.http, &st.dir, &repo, false).await?.data).unwrap_or_default();
    // What the repo says about itself, when GitHub answers (it's fine without).
    let meta: Value = match st.http.get(format!("https://api.github.com/repos/{repo}")).timeout(std::time::Duration::from_secs(10)).send().await {
        Ok(r) if r.status().is_success() => r.json().await.unwrap_or(Value::Null),
        _ => Value::Null,
    };
    let owner = repo.split('/').next().unwrap_or("").to_string();
    let id: String = format!("gh-{}", repo.to_lowercase().replace('/', "-")).chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' }).collect();
    let newest = rels.iter().find(|r| !r.draft);
    let mut entry = json!({
        "id": id,
        "name": catalog::name_from_repo(&repo),
        "repo": repo,
        "author": owner,
        "blurb": str_of(&meta, "description"),
        "systems": [os],
        "hero": format!("https://opengraph.githubassets.com/1/{repo}"),
        "released": newest.and_then(|r| r.published_at.clone()).unwrap_or_default(),
        "stars": meta.get("stargazers_count").cloned().unwrap_or(Value::Null),
        "custom": true,
    });
    if let Some(i) = info.as_ref().and_then(Value::as_object) {
        set_mine_fields(&mut entry, i);
    }
    let mut list = catalog::mine(&st.dir);
    list.retain(|e| str_of(e, "id") != id);
    list.push(entry.clone());
    catalog::save_mine(&st.dir, &list)?;
    Ok(json!({"id": id, "known": false, "name": str_of(&entry, "name")}))
}

fn set_mine_fields(entry: &mut Value, i: &serde_json::Map<String, Value>) {
    for k in MINE_FIELDS {
        if let Some(v) = i.get(*k).and_then(Value::as_str) {
            let v: String = v.trim().chars().take(if *k == "blurb" { 600 } else { 300 }).collect();
            // Pictures from the web only (https), or none.
            if ["cover", "hero", "logo"].contains(k) && !(v.is_empty() || v.starts_with("https://")) {
                continue;
            }
            if v.is_empty() && *k == "name" {
                continue;
            }
            entry[*k] = Value::String(v);
        }
    }
}

/// Edit Game Info: the player changes a game they added themselves.
#[tauri::command(async)]
fn update_mine(st: State<'_, App>, id: String, info: Value) -> Res<()> {
    let mut list = catalog::mine(&st.dir);
    let e = list.iter_mut().find(|e| str_of(e, "id") == id).ok_or_else(|| Error::Msg("Only games you added yourself can be changed.".into()))?;
    if let Some(i) = info.as_object() {
        set_mine_fields(e, i);
    }
    catalog::save_mine(&st.dir, &list)?;
    Ok(())
}

/// Take a recomp the player added by its link back out (when it isn't installed).
#[tauri::command(async)]
fn remove_github(st: State<'_, App>, id: String) -> Res<()> {
    let mut list = catalog::mine(&st.dir);
    list.retain(|e| str_of(e, "id") != id);
    catalog::save_mine(&st.dir, &list)?;
    Ok(())
}

fn profile_of(st: &App, id: &str, g: &Installed) -> Option<profile::Profile> {
    let cat = cached_catalog(st);
    let entry = catalog::app(&cat, id).cloned().unwrap_or(Value::Null);
    profile::find(&g.path, &g.repo, &entry)
}

/// The player's copy for this game: the one they picked, or the one found in their folders. Err
/// NEED_COPY when the game needs one and there isn't one yet.
fn game_copy(st: &App, id: &str, g: &Installed, p: &profile::Profile) -> Res<Option<PathBuf>> {
    let Some(need) = &p.game else { return Ok(None) };
    if let Some(c) = g.game_copy.clone().filter(|c| c.exists()) {
        // A disc image kept for a recomp that reads the disc's files from a folder (picked before the
        // launcher unpacked those): asked for once more, and this time it's unpacked where it goes.
        if !need.extract.is_empty() && c.is_file() && p.already_in_place(&g.path).is_none() {
            if let Some(x) = st.lib.lock().unwrap().installed.get_mut(id) {
                x.game_copy = None;
            }
            st.save();
            return Err(Error::Msg(format!("NEED_COPY|{}", need.label)));
        }
        // Picked before this version of the launcher could check it (a wrong ROM, say): checked
        // again now, and asked for again when it isn't the right one.
        if let Err(why) = profile::check_copy(need, &c) {
            if let Some(x) = st.lib.lock().unwrap().installed.get_mut(id) {
                x.game_copy = None;
            }
            st.save();
            return Err(Error::Msg(format!("{why} Pick your copy again, then press Play.")));
        }
        return Ok(Some(c));
    }
    // The player put the game's files where it reads them already: nothing to pick.
    if p.already_in_place(&g.path).is_some() {
        return Ok(None);
    }
    // Not set up yet: the setup screen asks for it (and offers one found in the player's folders).
    Err(Error::Msg(format!("NEED_COPY|{}", need.label)))
}

#[derive(Serialize)]
struct SetupOption {
    id: String,
    label: String,
    help: String,
    choices: Vec<String>,
    current: usize,
    applies: bool,
}

#[derive(Serialize)]
struct SetupAction {
    id: String,
    label: String,
    help: String,
    kind: String,
}

#[derive(Serialize)]
struct Setup {
    /// The player's copy is asked for, and how.
    needs: Option<String>,
    kind: String,
    copy: Option<PathBuf>,
    settings: Vec<SetupOption>,
    actions: Vec<SetupAction>,
    /// Settings the launcher worked out for this game (not its developer's): shown as a beta.
    beta: bool,
    /// Its settings file isn't there until the game's first start.
    waiting: bool,
    /// The game's update is asked for too, and how.
    update: Option<String>,
    update_copy: Option<PathBuf>,
    update_optional: bool,
    update_folder: bool,
    update_file: bool,
}

/// What the launcher can set up for a game: its copy and its settings. None for games it can't.
#[tauri::command(async)]
fn game_setup(st: State<'_, App>, id: String) -> Option<Setup> {
    let g = st.lib.lock().unwrap().installed.get(&id).cloned()?;
    let p = profile_of(&st, &id, &g)?;
    let cur = p.current(&g.path, &g.picks);
    let up = p.game.as_ref().and_then(|c| c.update.as_ref());
    Some(Setup {
        needs: p.game.as_ref().map(|x| x.label.clone()),
        kind: p.game.as_ref().map(|x| x.kind.clone()).unwrap_or_default(),
        copy: g.game_copy.clone().filter(|c| c.exists()).or_else(|| p.already_in_place(&g.path)),
        settings: p
            .settings
            .iter()
            .zip(cur)
            .map(|(s, (i, applies))| {
                // The launcher's own settings wait for the file the game makes on its first start.
                let ready = !p.from_boot || s.target == "args" || p.config_file(&s.target, &g.path).map(|f| f.existed).unwrap_or(false);
                SetupOption { id: s.id.clone(), label: s.label.clone(), help: s.help.clone(), choices: s.choices.iter().map(|c| c.label.clone()).collect(), current: i, applies: applies && ready }
            })
            .collect(),
        actions: p.actions.iter().map(|a| SetupAction { id: a.id.clone(), label: a.label.clone(), help: a.help.clone(), kind: if a.clear.is_empty() { "open".into() } else { "clear".into() } }).collect(),
        beta: p.from_boot,
        waiting: p.from_boot && p.waiting_for_files(&g.path),
        update: up.map(|u| u.label.clone()),
        update_copy: up.and_then(|u| g.update_copy.clone().filter(|c| c.exists()).or_else(|| u.in_place(&g.path))),
        update_optional: up.map(|u| u.optional).unwrap_or(false),
        update_folder: up.map(|u| u.wants_folder()).unwrap_or(false),
        update_file: up.map(|u| u.wants_file()).unwrap_or(false),
    })
}

/// The player picked the game's update.
#[tauri::command(async)]
fn set_update_copy(st: State<'_, App>, id: String, path: String) -> Res<()> {
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let p = profile_of(&st, &id, &g).ok_or_else(|| Error::Msg("The launcher can't set this game up yet.".into()))?;
    let up = p.game.as_ref().and_then(|c| c.update.clone()).ok_or_else(|| Error::Msg("This game doesn't need an update file.".into()))?;
    let file = PathBuf::from(&path);
    up.check(&file).map_err(Error::Msg)?;
    up.place(&g.path, &file).map_err(|e| Error::Msg(format!("Couldn't put the update where the game reads it: {e}")))?;
    if let Some(x) = st.lib.lock().unwrap().installed.get_mut(&id) {
        x.update_copy = Some(file);
    }
    st.save();
    Ok(())
}

/// A copy picked as a .7z or .zip: unpacked into a folder beside it (as "Extract here" would), or
/// into the launcher's own folder when that one can't be written to. The game inside it.
fn unpack_copy(st: &App, need: &profile::GameCopy, file: &Path) -> Result<PathBuf, String> {
    let stem = file.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "game".into());
    let beside = file.parent().map(|d| d.join(&stem)).filter(|t| std::fs::create_dir_all(t).is_ok());
    let target = beside.unwrap_or_else(|| st.dir.join("game-copies").join(&stem));
    // Unpacked before: used as it is.
    if let Some(found) = copy_in(need, &target) {
        return Ok(found);
    }
    let staging = st.dir.join("cache").join("unpack-copy");
    install::unpack(file, &target, &staging).map_err(|e| format!("Couldn't unpack it: {e}"))?;
    copy_in(need, &target).ok_or_else(|| format!("It's unpacked in {}, but the game isn't in it. Pick the game's file or folder from there.", target.display()))
}

/// The game in an unpacked folder: the folder itself or one inside it, or its biggest file that
/// passes the copy's checks.
fn copy_in(need: &profile::GameCopy, dir: &Path) -> Option<PathBuf> {
    if !dir.is_dir() || std::fs::read_dir(dir).map(|mut d| d.next().is_none()).unwrap_or(true) {
        return None;
    }
    let junk = ["txt", "nfo", "md", "url", "sfv", "jpg", "jpeg", "png", "html", "pdf", "lnk", "exe", "dll"];
    let mut level = vec![dir.to_path_buf()];
    let mut best: Option<(u64, PathBuf)> = None;
    for _ in 0..4 {
        let mut next = vec![];
        for d in &level {
            if need.kind != "iso" && profile::check_copy(need, d).is_ok() {
                return Some(d.clone());
            }
            for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    next.push(p);
                } else if need.kind != "folder" {
                    let ext = p.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
                    if junk.contains(&ext.as_str()) || profile::packed(need, &p).is_some() || profile::check_copy(need, &p).is_err() {
                        continue;
                    }
                    let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                    if best.as_ref().map(|(s, _)| size > *s).unwrap_or(true) {
                        best = Some((size, p));
                    }
                }
            }
        }
        if best.is_some() {
            break;
        }
        level = next;
    }
    best.map(|(_, p)| p)
}

/// The player picked their copy of the game.
#[tauri::command(async)]
fn set_game_copy(st: State<'_, App>, id: String, path: String) -> Res<()> {
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let p = profile_of(&st, &id, &g).ok_or_else(|| Error::Msg("The launcher can't set this game up yet.".into()))?;
    let mut copy = PathBuf::from(&path);
    if let Some(need) = &p.game {
        if matches!(profile::packed(need, &copy).as_deref(), Some("7z" | "zip")) {
            copy = unpack_copy(&st, need, &copy).map_err(Error::Msg)?;
        }
        // A folder one level off (the one around the game's files, or one inside them): the right
        // one is used instead of turning the player away.
        if copy.is_dir() && copy.parent().is_some() && profile::check_copy(need, &copy).is_err() {
            if let Some(inner) = copy_in(need, &copy).filter(|c| c != &copy) {
                copy = inner;
            } else if let Some(up) = copy.parent().filter(|u| u.parent().is_some() && profile::check_copy(need, u).is_ok()) {
                copy = up.to_path_buf();
            }
        }
        profile::check_copy(need, &copy).map_err(Error::Msg)?;
    }
    if let Some(x) = st.lib.lock().unwrap().installed.get_mut(&id) {
        x.game_copy = Some(copy.clone());
    }
    st.save();
    // Written now too, so the recomp's own launcher knows it as well.
    let _ = p.prepare(&g.path, Some(&copy));
    Ok(())
}

#[tauri::command(async)]
fn set_game_setting(st: State<'_, App>, id: String, setting: String, choice: usize) -> Res<()> {
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let p = profile_of(&st, &id, &g).ok_or_else(|| Error::Msg("The launcher can't set this game up yet.".into()))?;
    let s = p.settings.iter().find(|s| s.id == setting).ok_or_else(|| Error::Msg("That isn't one of its settings.".into()))?;
    if s.target == "args" {
        if let Some(x) = st.lib.lock().unwrap().installed.get_mut(&id) {
            x.picks.insert(setting.clone(), choice.min(s.choices.len().saturating_sub(1)));
        }
        st.save();
        return Ok(());
    }
    // The settings file may not exist before the first start: write what the game needs first.
    // (Not for the launcher's own settings: the game makes that file itself.)
    if !p.from_boot {
        let _ = p.prepare(&g.path, g.game_copy.as_deref());
    }
    p.pick(&g.path, &setting, choice).map_err(Error::Msg)
}

/// A profile's button: open a folder, or clear one (the shader cache).
#[tauri::command(async)]
fn game_action(app: AppHandle, st: State<'_, App>, id: String, action: String) -> Res<String> {
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let p = profile_of(&st, &id, &g).ok_or_else(|| Error::Msg("The launcher can't set this game up yet.".into()))?;
    let a = p.actions.iter().find(|a| a.id == action).ok_or_else(|| Error::Msg("That isn't one of its buttons.".into()))?;
    let resolve = |spec: &str| -> Option<PathBuf> {
        let (name, sub) = spec.split_once('/').unwrap_or((spec, ""));
        let base = p.folder(name, &g.path)?;
        Some(if sub.is_empty() { base } else { base.join(sub) })
    };
    if !a.clear.is_empty() {
        if st.running.lock().unwrap().contains_key(&id) {
            return Err(Error::Msg("Close the game first.".into()));
        }
        let dir = resolve(&a.clear).ok_or_else(|| Error::Msg("That folder couldn't be found.".into()))?;
        // Only a subfolder of one of the game's folders, never a drive, home or a system folder.
        let named = a.clear.split_once('/').map(|(_, sub)| !sub.trim_matches('/').is_empty()).unwrap_or(false);
        let system = [dirs::home_dir(), dirs::document_dir(), dirs::data_local_dir(), dirs::config_dir(), dirs::data_dir(), dirs::desktop_dir(), dirs::download_dir()];
        if !named || dir.components().count() < 4 || dir.components().any(|c| c.as_os_str() == "..") || system.iter().flatten().any(|s| s == &dir) {
            return Err(Error::Msg("That folder can't be cleared from here.".into()));
        }
        let _ = std::fs::remove_dir_all(&dir);
        return Ok("Cleared.".into());
    }
    let dir = resolve(&a.open).ok_or_else(|| Error::Msg("That folder couldn't be found.".into()))?;
    std::fs::create_dir_all(&dir)?;
    open_thing(&app, &dir.to_string_lossy(), false)?;
    Ok(String::new())
}

fn g_discord(st: &App) -> bool {
    st.lib.lock().unwrap().settings.discord
}

/// The Discord application id, the game's own name and its cover's address, from the catalog on disk.
fn discord_info(st: &App, id: &str) -> (String, String, String) {
    let cat: Value = std::fs::read(st.dir.join("cache/pc-catalog.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    let app_id = cat.get("discord").and_then(Value::as_str).filter(|s| s.chars().all(|c| c.is_ascii_digit()) && !s.is_empty()).unwrap_or(discord::APP_ID).to_string();
    let a = catalog::app(&cat, id).cloned().unwrap_or(Value::Null);
    let game = Some(str_of(&a, "game").trim().to_string()).filter(|s| !s.is_empty()).or_else(|| st.lib.lock().unwrap().installed.get(id).map(|g| g.name.clone())).unwrap_or_default();
    let cover = str_of(&a, "cover");
    let cover = if cover.starts_with('/') { format!("{}{}", catalog::base(), cover) } else { cover.to_string() };
    (app_id, game, cover)
}

// Async so it runs inside the async runtime, which starting and watching a process needs.
// With `replace`, any other game that's running is closed first.
#[tauri::command]
async fn play(app: AppHandle, id: String, replace: Option<bool>, big_picture: Option<bool>, program: Option<String>) -> Res<()> {
    BIG_PICTURE.store(big_picture.unwrap_or(false), Ordering::Relaxed);
    // Another of the game's programs, this time only (held Play): one inside its folder.
    if let Some(rel) = program.filter(|p| !p.trim().is_empty()) {
        let st = app.state::<App>();
        let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
        let rel = PathBuf::from(rel);
        if rel.is_absolute() || rel.components().any(|c| matches!(c, std::path::Component::ParentDir)) || !g.path.join(&rel).is_file() {
            return Err(Error::Msg("That program isn't in the game's folder.".into()));
        }
        ONCE_EXE.lock().unwrap().get_or_insert_with(HashMap::new).insert(id.clone(), rel);
    }
    if replace.unwrap_or(false) {
        let st = app.state::<App>();
        let others: Vec<String> = st.running.lock().unwrap().keys().filter(|k| **k != id).cloned().collect();
        for o in &others {
            if let Some(tx) = st.running.lock().unwrap().remove(o) {
                let _ = tx.send(());
            }
        }
        // Give them a moment to close.
        for _ in 0..50 {
            if others.iter().all(|o| !st.pids.lock().unwrap().contains_key(o)) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }
    ready_look(&app, &id).await;
    start_game(&app, &id)
}

/// A program picked for one start (held Play), used instead of the game's usual one.
static ONCE_EXE: Mutex<Option<HashMap<String, PathBuf>>> = Mutex::new(None);

/// A look needs ReShade: downloaded the first time, and kept up to date.
pub(crate) async fn ready_look(app: &AppHandle, id: &str) {
    let wants_look = {
        let st = app.state::<App>();
        let l = st.lib.lock().unwrap();
        cfg!(windows) && l.installed.get(id).map(|g| looks::find(&g.look).is_some()).unwrap_or(false)
    };
    if wants_look {
        let st = app.state::<App>();
        let mirror = reshade_mirror(&st);
        let app2 = app.clone();
        // Started from a shortcut there's no window, so a notification says why it's taking a moment.
        let headless = st.headless.load(Ordering::Relaxed);
        let say = move |m: &str| {
            let _ = app2.emit("toast", m);
            if headless {
                ingame::notify(&app2, m, "Once, for the look you picked. The game starts in a moment.");
            }
        };
        if let Err(e) = looks::ensure(&st.http, &st.dir, mirror, &say).await {
            let _ = app.emit("toast", format!("{e} The game starts without its look."));
        }
    }
}

/// A copy of ReShade named by the catalog, for when reshade.me can't be reached.
fn reshade_mirror(st: &App) -> Option<looks::Source> {
    let cat = cached_catalog(st);
    let r = cat.get("tools")?.get("reshade")?;
    Some(looks::Source { version: str_of(r, "version").into(), url: str_of(r, "url").into(), sha256: str_of(r, "sha256").into() })
}

/// Why looks are off on a Linux PC without vkBasalt.
const VKBASALT_MISSING: &str = "Looks on Linux use vkBasalt. Install it from your distro's packages (it's usually called vkbasalt), then start the game again.";

/// The look a game starts with: Ok(None) for none, Err with a reason when it has one that can't be used.
fn look_plan(st: &App, id: &str, g: &Installed, prof: Option<&profile::Profile>, exe: &Path) -> Result<Option<(&'static looks::LookDef, u32, looks::Api)>, Option<String>> {
    let Some(look) = looks::find(&g.look) else { return Ok(None) };
    if cfg!(target_os = "macos") {
        return Err(None);
    }
    if !cfg!(windows) && !looks::vkbasalt_installed() {
        return Err(Some(VKBASALT_MISSING.into()));
    }
    let pl = prof.and_then(|p| p.looks.clone()).unwrap_or_else(|| looks::ProfileLooks { allowed: true, r#in: "main".into(), ..Default::default() });
    if !pl.allowed {
        return Err(Some(if pl.reason.is_empty() { "This game doesn't work with looks.".into() } else { pl.reason.clone() }));
    }
    if cfg!(windows) && looks::have_version(&st.dir).is_none() {
        return Err(None);
    }
    let lines = if pl.lines > 0 {
        pl.lines
    } else {
        let cat = cached_catalog(st);
        looks::lines_for(catalog::app(&cat, id).map(|e| str_of(e, "origin")).unwrap_or(""))
    };
    let from_setting = (!pl.graphics_key.is_empty())
        .then(|| prof.and_then(|p| p.config_file(&pl.r#in, &g.path)).and_then(|f| f.get(&pl.graphics_key)).and_then(|v| looks::Api::from_name(&v)))
        .flatten();
    let api = from_setting.or_else(|| looks::Api::from_name(&pl.graphics)).unwrap_or_else(|| looks::detect(exe));
    Ok(Some((look, lines, api)))
}

/// Back to the game: the launcher steps aside (the game is the window under it).
#[tauri::command(async)]
fn resume_game(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.minimize();
    }
}

#[tauri::command(async)]
fn stop(st: State<'_, App>, id: String) {
    if let Some(tx) = st.running.lock().unwrap().remove(&id) {
        let _ = tx.send(());
    }
}

/// What the launcher started for a game last time, for reports: the program, its options and how the
/// game was set up. Kept on disk, so a report sent later still has it. The player's home folder is
/// written as %USERPROFILE%.
fn note_run(st: &App, id: &str, g: &Installed, exe: &Path, args: &[String], cwd: &Path, prof: Option<&profile::Profile>, look: bool) {
    let home = dirs::home_dir().map(|h| h.to_string_lossy().to_string()).unwrap_or_default();
    let hide = |s: String| if home.len() > 3 { s.replace(&home, "%USERPROFILE%") } else { s };
    let quote = |a: &String| if a.contains(' ') { format!("\"{a}\"") } else { a.clone() };
    let mut l: Vec<String> = vec![];
    l.push(format!("Started: {}", hide(exe.display().to_string())));
    l.push(format!("Options: {}", if args.is_empty() { "(none)".to_string() } else { hide(args.iter().map(quote).collect::<Vec<_>>().join(" ")) }));
    l.push(format!("In folder: {}", hide(cwd.display().to_string())));
    l.push(format!("Game folder: {}", hide(g.path.display().to_string())));
    l.push(format!("Release: {} {}", g.tag, g.asset));
    l.push(format!(
        "Set up by: {}",
        match prof {
            Some(p) if p.from_boot => "the launcher's Easy Setup for it",
            Some(_) => "its developer's launcher profile",
            None => "nothing (its program as it is)",
        }
    ));
    if let Some(c) = &g.game_copy {
        let size = std::fs::metadata(c).ok().filter(|m| m.is_file()).map(|m| format!(", {} bytes", m.len())).unwrap_or_default();
        l.push(format!("Player's copy: {} ({}{size})", hide(c.display().to_string()), if c.is_dir() { "folder" } else { "file" }));
    }
    if let Some(u) = &g.update_copy {
        l.push(format!("Update file: {}", hide(u.display().to_string())));
    }
    if let Some(s) = &g.setup {
        l.push(format!("Its own setup: {} ({})", s.display(), if g.setup_done { "done" } else { "not finished" }));
    }
    if g.exe_rules == u8::MAX {
        l.push("Program: picked by the player".into());
    }
    if look {
        l.push(format!("Look: {} at {}%", g.look, g.look_amount));
    }
    if !g.args.trim().is_empty() {
        l.push(format!("Player's launch options: {}", hide(g.args.clone())));
    }
    l.push(format!("Big Picture mode: {}", if BIG_PICTURE.load(Ordering::Relaxed) { "yes" } else { "no" }));
    let dir = st.dir.join("cache/runs");
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join(format!("{}.txt", run_file(id))), l.join("\n"));
}

fn run_file(id: &str) -> String {
    id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}

/// What `note_run` wrote down for the game's last start.
fn last_run(dir: &Path, id: &str) -> Option<String> {
    std::fs::read_to_string(dir.join("cache/runs").join(format!("{}.txt", run_file(id)))).ok()
}

/// The newest log the game wrote since it started, for the crash screen: its last 60 lines.
#[tauri::command(async)]
fn recent_log(st: State<'_, App>, id: String, since: u64) -> Option<(String, String)> {
    recent_log_of(&st, &id, since)
}

fn recent_log_of(st: &App, id: &str, since: u64) -> Option<(String, String)> {
    let id = id.to_string();
    let g = st.lib.lock().unwrap().installed.get(&id).cloned()?;
    // Not told when it started: the start of its last session (an old log is never "the crash").
    let since = if since > 0 { since } else { g.sessions.last().map(|s| s[0]).unwrap_or(g.last_played) };
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    let mut stack = vec![(g.path.clone(), 0)];
    // And where its launcher profile says it keeps logs, when that's outside its folder.
    if let Some(d) = profile_of(&st, &id, &g).and_then(|p| p.folder("logs", &g.path)).filter(|d| !d.starts_with(&g.path)) {
        stack.push((d, 1));
    }
    while let Some((d, depth)) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            let Ok(m) = e.metadata() else { continue };
            if m.is_dir() {
                if depth < 3 {
                    stack.push((p, depth + 1));
                }
                continue;
            }
            let n = p.to_string_lossy().to_lowercase();
            if !(n.ends_with(".log") || n.ends_with("log.txt")) {
                continue;
            }
            let Ok(t) = m.modified() else { continue };
            let secs = t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            if secs + 5 >= since && best.as_ref().map(|b| t > b.0).unwrap_or(true) && !old_inside(&p, since) {
                best = Some((t, p));
            }
        }
    }
    // No log of the game's own: what it printed, if it printed anything.
    let best = best.or_else(|| {
        let p = game_output_path(&st.dir, &id);
        let m = std::fs::metadata(&p).ok()?;
        let t = m.modified().ok()?;
        let secs = t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        (m.len() > 0 && secs + 5 >= since).then_some((t, p))
    });
    // Nor that: ReShade's, when it ran with a look.
    let best = best.or_else(|| {
        let p = st.dir.join("looks").join(&id).join("ReShade.log");
        let t = std::fs::metadata(&p).ok()?.modified().ok()?;
        let secs = t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        (secs + 5 >= since).then_some((t, p))
    });
    let (_, p) = best?;
    let text = std::fs::read(&p).ok()?;
    let text = String::from_utf8_lossy(&text);
    let lines: Vec<&str> = text.lines().collect();
    let tail = lines[lines.len().saturating_sub(60)..].join("\n");
    Some((p.to_string_lossy().into(), tail))
}

// ---------- Troubleshoot ----------

/// One thing Troubleshoot looked at: "ok", "warn", "bad" or "info", what it found, and the fix
/// the window offers for it ("" for none).
#[derive(Serialize)]
struct Check {
    level: &'static str,
    /// English, with {name}-style blanks the window fills in from `v` after translating.
    title: &'static str,
    detail: String,
    fix: &'static str,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    v: std::collections::BTreeMap<&'static str, String>,
}

fn check(level: &'static str, title: &'static str, detail: impl Into<String>, fix: &'static str) -> Check {
    Check { level, title, detail: detail.into(), fix, v: Default::default() }
}

impl Check {
    fn with(mut self, k: &'static str, val: impl Into<String>) -> Self {
        self.v.insert(k, val.into());
        self
    }
}

/// Starts that skip the player's Look and launch options, once each (Troubleshoot's Clean Start).
static CLEAN_START: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[tauri::command(async)]
fn clean_start_next(id: String) {
    CLEAN_START.lock().unwrap().push(id);
}

/// What could be keeping a game from starting: its files, Windows, what it needs on the PC, the
/// player's copy, its setup, its folder, and what it printed last time.
#[tauri::command(async)]
fn troubleshoot(st: State<'_, App>, id: String) -> Res<Vec<Check>> {
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let mut out: Vec<Check> = vec![];
    // Its folder.
    if !g.path.is_dir() {
        out.push(check("bad", "Its folder isn't there", "{path} can't be found. Maybe it's on a drive that isn't connected, or it was moved.", "locate").with("path", g.path.display().to_string()));
        return Ok(out);
    }
    out.push(check("ok", "Its folder is there", g.path.display().to_string(), ""));
    // Letters outside English in the path: many recomps on Windows can't open files there.
    if cfg!(windows) && !g.path.to_string_lossy().is_ascii() {
        out.push(check("warn", "Its folder's path has letters some games can't read", "Many recomps can't open files in a folder whose path has accents or letters outside English. Move it to a folder with plain letters, like C:\\Recomp Library.", "move"));
    }
    // The program it starts (set up by hand: the one the player picked).
    let prof = if g.manual.is_some() { None } else { profile_of(&st, &id, &g) };
    let exe = g
        .manual
        .as_ref()
        .and_then(|m| m.exe.clone())
        .map(|e| if e.is_absolute() { e } else { g.path.join(e) })
        .or_else(|| prof.as_ref().and_then(|p| p.command(&g.path, g.game_copy.as_deref(), &g.picks).map(|(e, _)| e)))
        .or_else(|| g.exe.as_ref().map(|e| g.path.join(e)));
    let Some(exe) = exe.filter(|e| e.is_file()) else {
        if g.manual.is_some() {
            out.push(check("bad", "The program you picked isn't there", "An update may have renamed it. Pick it again in Set Up By Hand.", "manual"));
            return Ok(out);
        }
        out.push(check("bad", "The program it starts isn't there", "Antivirus may have taken it out, or the download was cut short.", "setexe"));
        out.push(check("info", "You can set it up by hand", "Point the launcher at its program and your game files yourself.", "manual"));
        return Ok(out);
    };
    if let Some(m) = &g.manual {
        let ok = m.copy.as_ref().map(|c| c.exists()).unwrap_or(true);
        out.push(check(if ok { "info" } else { "bad" }, "It's set up by hand", if ok { "It starts the way you set it up, not the launcher's way." } else { "Your game files aren't where you pointed it anymore." }, "manual"));
    }
    let name = exe.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    out.push(check("ok", "It starts {name}", "", "").with("name", &name));
    // The right build for this PC.
    let ext = exe.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if cfg!(windows) {
        if ext == "sh" || ext == "appimage" || ext.is_empty() {
            out.push(check("bad", "That's not a Windows program", "It looks like a Linux build. Reinstall the game to get its Windows build.", "reinstall"));
        }
    } else if matches!(ext.as_str(), "exe" | "cmd" | "bat") {
        let twin = if ext == "exe" { None } else { linux_twin(&exe) };
        if let Some(t) = twin {
            out.push(check("ok", "Its Linux start script is used", t.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), ""));
        } else if proton::runner().is_some() {
            out.push(check("info", "It's a Windows version, run through Proton", "If it won't start, a native Linux build (when there is one) usually runs better.", ""));
        } else {
            out.push(check("bad", "It's a Windows version and Proton isn't installed", "Install Steam with Proton, or umu-launcher, or Wine, then press Play again.", "proton"));
        }
    }
    // Files it needs that aren't on this PC: the DLLs it loads (Windows), the libraries it links
    // to (Linux). The most common reason a game closes the moment it starts.
    let lacking = missing_libraries(&exe, &g.path);
    if !lacking.is_empty() {
        let low: Vec<String> = lacking.iter().map(|d| d.to_lowercase()).collect();
        let vc = low.iter().any(|d| d.starts_with("msvcp") || d.starts_with("vcruntime") || d.starts_with("concrt") || d.starts_with("vcomp"));
        let dx = low.iter().any(|d| d.starts_with("d3dx9_") || d.starts_with("d3dx10_") || d.starts_with("d3dx11_") || d.starts_with("xinput1_3") || d.starts_with("x3daudio") || d.starts_with("xaudio2_7") || d.starts_with("d3dcompiler_4"));
        let (detail, fix) = if vc {
            ("Missing: {files}. It needs the Visual C++ runtime, and part of it is missing.", "vcredist")
        } else if dx {
            ("Missing: {files}. It needs the older DirectX runtime (June 2010), which Windows doesn't come with.", "dxruntime")
        } else if cfg!(windows) {
            ("Missing: {files}. They should come with the game. Reinstall it; if they're still missing, its developer needs to know.", "reinstall")
        } else {
            ("Missing: {files}. Install them with your system's package manager (the names are a good search), then press Play again.", "")
        };
        out.push(check("bad", "It needs files that aren't on this PC", detail, fix).with("files", lacking.iter().take(8).cloned().collect::<Vec<_>>().join(", ")));
    }
    // A folder OneDrive keeps in sync: files can be online-only, and syncing locks them.
    if cfg!(windows) && g.path.to_string_lossy().to_lowercase().contains("\\onedrive") {
        out.push(check("warn", "Its folder is in OneDrive", "Games often can't run from a folder OneDrive syncs: some files may only be online, and syncing locks them while the game runs.", "move"));
    }
    // Windows holding it back.
    #[cfg(windows)]
    {
        if std::fs::metadata(format!("{}:Zone.Identifier", exe.display())).is_ok() {
            out.push(check("warn", "Windows marked it as downloaded from the internet", "Windows can block or slow down a program with that mark. Unblock takes it off this game's files.", "unblock"));
        }
    }
    // What it needs on the PC.
    let pc = manage::pc_check();
    if cfg!(windows) {
        match pc.vcredist {
            Some(false) => out.push(check("bad", "The Visual C++ runtime isn't installed", "Almost every Windows recomp needs it.", "vcredist")),
            Some(true) => out.push(check("ok", "The Visual C++ runtime is installed", "", "")),
            None => {}
        }
    }
    if pc.vulkan == Some(false) {
        out.push(check("warn", "Your graphics driver doesn't have Vulkan", "Many recomps draw with Vulkan. Updating the graphics driver adds it.", "driver"));
    }
    // The player's copy of the game.
    if let Some(need) = prof.as_ref().and_then(|p| p.game.as_ref()) {
        match g.game_copy.as_ref() {
            Some(c) if !c.exists() => out.push(check("bad", "Your copy of the game isn't there anymore", c.display().to_string(), "copy")),
            Some(c) => match profile::check_copy(need, c) {
                Ok(()) => out.push(check("ok", "Your copy of the game checks out", c.display().to_string(), "")),
                Err(why) => out.push(check("bad", "Your copy of the game isn't the one it reads", why, "copy")),
            },
            None if prof.as_ref().map(|p| p.already_in_place(&g.path).is_some()).unwrap_or(false) => out.push(check("ok", "The game's files are in its folder", "", "")),
            None => out.push(check("bad", "Your copy of the game isn't set yet", need.label.clone(), "copy")),
        }
    }
    // Its own first-time setup.
    if g.setup.is_some() && !g.setup_done {
        out.push(check("warn", "Its first-time setup hasn't finished", "Press Play and go through the setup window to the end.", ""));
    }
    // Already open.
    if already_open(&exe) {
        out.push(check("warn", "{name} is already open", "Close it (check the taskbar and the tray), then press Play.", "").with("name", &name));
    }
    // The folder can be written to, with room to spare.
    let probe = g.path.join(".recomplauncher-write-test");
    match std::fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            out.push(check("ok", "The launcher can write to its folder", "", ""));
        }
        Err(e) => out.push(check("bad", "Its folder can't be written to", "{error}. Games often can't save or set up in protected folders like Program Files.", "move").with("error", e.to_string())),
    }
    if let Ok(free) = fs2::available_space(&g.path) {
        if free < 2 << 30 {
            out.push(check("warn", "The drive is nearly full", "{gb} GB free. Some games need room for saves and shader caches.", "move").with("gb", format!("{:.1}", free as f64 / 1_073_741_824.0)));
        }
    }
    // Extras on top of the game.
    let look_on = !g.look.is_empty() && g.look != "off";
    if look_on || !g.args.trim().is_empty() {
        let what = match (look_on, !g.args.trim().is_empty()) {
            (true, true) => "A Look (ReShade) and your own launch options are on. If it won't start, try once without them.",
            (true, false) => "A Look (ReShade) is on. If it won't start, try once without it.",
            _ => "Your own launch options are on. If it won't start, try once without them.",
        };
        out.push(check("info", "Extras are on", what, "clean"));
        // A Look kept to one way in after a start that crashed: when that's the wrong one for this
        // game, the Look never shows.
        if look_on {
            if let Some(r) = looks::kept_route(&st.dir, &id) {
                let way = if r == "layer" { "Vulkan" } else { "Direct3D" };
                out.push(check("info", "The Look only tries one way in", "Since a start that crashed, its Look goes in through {way} only. If the Look doesn't show, let it try both ways again.", "lookroute").with("way", way));
            }
        }
    }
    // What it printed last time.
    if let Ok(text) = std::fs::read_to_string(game_output_path(&st.dir, &id)) {
        // It said it's missing something (its game files, most often).
        let missing = text.lines().rev().take(80).find(|l| {
            let l = l.to_lowercase();
            l.contains("status=missing") || l.contains("no executable found") || l.contains("could not find") || l.contains("couldn't find") || l.contains("not found") || l.contains("no such file") || l.contains(" missing")
        });
        let needs_copy = prof.as_ref().map(|p| p.game.is_some()).unwrap_or(false);
        if let Some(line) = missing {
            out.push(check("bad", "It says something it needs is missing", line.trim().chars().take(300).collect::<String>(), if needs_copy { "copy" } else { "files" }));
        } else if let Some(line) = text.lines().rev().take(80).find(|l| wrong_copy_line(l)) {
            // It found game files, but not the right ones (another game, a bad dump, a folder one
            // level off): picked again, by the launcher's setup or by hand.
            out.push(check("bad", "It says your game files aren't the right ones", line.trim().chars().take(300).collect::<String>(), if needs_copy { "copy" } else { "manual" }));
        }
        let tail: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        let tail = tail[tail.len().saturating_sub(12)..].join("\n");
        if !tail.is_empty() {
            out.push(check("info", "What it printed last time", tail, ""));
        }
    }
    // Something's wrong and it isn't set up by hand yet: that's the way out when nothing else works.
    if g.manual.is_none() && out.iter().any(|c| c.level == "bad") && !out.iter().any(|c| c.fix == "manual") {
        out.push(check("info", "You can set it up by hand", "Point the launcher at its program and your game files yourself.", "manual"));
    }
    Ok(out)
}

/// A line where a game says the files it was given aren't its own (wrong game or region, a bad
/// dump, a folder one level off).
fn wrong_copy_line(l: &str) -> bool {
    let l = l.to_lowercase();
    ["wrong game", "wrong rom", "wrong disc", "wrong region", "invalid rom", "bad rom", "bad dump", "unsupported rom", "unrecognized rom", "unrecognised rom", "checksum mismatch", "hash mismatch", "one level too"]
        .iter()
        .any(|w| l.contains(w))
        || (l.contains("is not the ") && l.contains(" disc"))
}

/// The libraries a program loads that this PC doesn't have, by name. Windows: its DLLs, looked
/// for beside it, in Windows' own folders and on the PATH. Linux (a native program): what ldd says
/// isn't found and isn't anywhere in the game's own folder either.
fn missing_libraries(exe: &Path, game_dir: &Path) -> Vec<String> {
    #[cfg(windows)]
    {
        let _ = game_dir;
        let Some(pe) = std::fs::read(exe).ok().and_then(|b| looks::read_pe(&b)) else { return vec![] };
        let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let wow = root.join("SysWOW64");
        let sys = if !pe.x64 && wow.is_dir() { wow } else { root.join("System32") };
        let mut dirs: Vec<PathBuf> = vec![exe.parent().map(Path::to_path_buf).unwrap_or_default(), sys, root.clone()];
        dirs.extend(std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default());
        pe.imports
            .iter()
            .filter(|d| !d.starts_with("api-ms-") && !d.starts_with("ext-ms-"))
            .filter(|d| !dirs.iter().any(|p| p.join(d.as_str()).is_file()))
            .cloned()
            .collect()
    }
    #[cfg(not(windows))]
    {
        let mut head = [0u8; 4];
        let elf = std::fs::File::open(exe).and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head)).is_ok() && &head == b"\x7fELF";
        if !elf {
            return vec![];
        }
        let mut c = std::process::Command::new("ldd");
        sysenv::clean_std(&mut c);
        // ldd answers at once; one that doesn't is left behind rather than keeping Troubleshoot waiting.
        let Ok(mut ch) = c.arg(exe).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn() else { return vec![] };
        let started = std::time::Instant::now();
        loop {
            match ch.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if started.elapsed() < std::time::Duration::from_secs(8) => std::thread::sleep(std::time::Duration::from_millis(40)),
                _ => {
                    let _ = ch.kill();
                    let _ = ch.wait();
                    return vec![];
                }
            }
        }
        let mut text = String::new();
        if let Some(mut out) = ch.stdout.take() {
            let _ = std::io::Read::read_to_string(&mut out, &mut text);
        }
        let mut inside: Vec<String> = vec![];
        let mut walk = vec![(game_dir.to_path_buf(), 0)];
        while let Some((d, depth)) = walk.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten().take(2000) {
                let p = e.path();
                if p.is_dir() && depth < 3 {
                    walk.push((p, depth + 1));
                } else if let Some(n) = p.file_name() {
                    inside.push(n.to_string_lossy().to_string());
                }
            }
        }
        text.lines()
            .filter(|l| l.contains("=> not found"))
            .filter_map(|l| l.split("=>").next().map(|n| n.trim().to_string()))
            .filter(|n| !n.is_empty() && !inside.contains(n))
            .collect()
    }
}

/// Set Up By Hand: the program and its command line, with the player's game files put where they
/// said (in a folder inside the game's, on the command line, or nowhere: the game asks).
fn manual_plan(g: &Installed, m: &library::ManualSetup) -> Res<Option<(PathBuf, Vec<String>)>> {
    let exe = m
        .exe
        .clone()
        .or_else(|| g.exe.clone())
        .map(|e| if e.is_absolute() { e } else { g.path.join(e) })
        .ok_or_else(|| Error::Msg("Set Up By Hand doesn't have a program to start yet. Pick it there.".into()))?;
    if !exe.is_file() {
        return Err(Error::Msg(format!("{} isn't in its folder anymore (an update may have renamed it). Pick the program again in Set Up By Hand.", exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())));
    }
    let mut args = vec![];
    if let Some(c) = m.copy.as_ref() {
        if !c.exists() {
            return Err(Error::Msg(format!("Your game files aren't at {} anymore. Pick them again in Set Up By Hand.", c.display())));
        }
        match m.how.as_str() {
            "folder" => {
                let gc = profile::GameCopy { into: if m.into.trim().is_empty() { ".".into() } else { m.into.clone() }, name: m.name.clone(), ..Default::default() };
                profile::Profile { game: Some(gc), ..Default::default() }.place(&g.path, Some(c)).map_err(|e| Error::Msg(format!("Couldn't put your game files in its folder: {e}")))?;
            }
            "arg" => args = fill_copy_args(&m.arg, c),
            _ => {}
        }
    }
    trail::note(format!("Play {}: set up by hand ({})", g.id, if m.how.is_empty() { "none" } else { &m.how }));
    Ok(Some((exe, args)))
}

/// A command line written by the player ("--game {copy}", quotes for words with spaces), split
/// into its parts with {copy} as the game files' path (kept whole, spaces and all).
fn fill_copy_args(template: &str, copy: &Path) -> Vec<String> {
    let mut out = vec![];
    let (mut cur, mut quoted, mut any) = (String::new(), false, false);
    for ch in template.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    let path = copy.to_string_lossy();
    out.into_iter().map(|a| a.replace("{copy}", &path)).collect()
}

/// What Set Up By Hand starts from: the player's own, or the launcher's way of setting it up.
#[tauri::command(async)]
fn manual_defaults(st: State<'_, App>, id: String) -> Res<library::ManualSetup> {
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    if let Some(m) = g.manual.clone() {
        return Ok(m);
    }
    let prof = profile_of(&st, &id, &g);
    let mut m = library::ManualSetup { copy: g.game_copy.clone(), ..Default::default() };
    m.exe = prof
        .as_ref()
        .and_then(|p| p.start.as_ref())
        .and_then(|s| s.program.iter().find(|x| g.path.join(x).is_file()).map(PathBuf::from))
        .or_else(|| g.exe.clone());
    match prof.as_ref() {
        Some(p) if p.game.as_ref().map(|c| !c.into.trim().is_empty()).unwrap_or(false) => {
            let c = p.game.as_ref().unwrap();
            m.how = "folder".into();
            m.into = c.into.clone();
            m.name = c.name.clone();
        }
        Some(p) if p.start.as_ref().map(|s| s.args.iter().any(|a| a.contains("{game}"))).unwrap_or(false) => {
            m.how = "arg".into();
            m.arg = p.start.as_ref().unwrap().args.iter().map(|a| { let a = a.replace("{game}", "{copy}"); if a.contains(' ') { format!("\"{a}\"") } else { a } }).collect::<Vec<_>>().join(" ");
        }
        _ => m.how = "none".into(),
    }
    Ok(m)
}

/// Set Up By Hand: saved (or turned off with None).
#[tauri::command(async)]
fn set_manual(st: State<'_, App>, id: String, manual: Option<library::ManualSetup>) -> Res<Installed> {
    if let Some(m) = &manual {
        if !matches!(m.how.as_str(), "folder" | "arg" | "none") {
            return Err(Error::Msg("Pick how the game gets your files.".into()));
        }
        let into = m.into.trim().replace('\\', "/");
        if m.how == "folder" && (into.contains(':') || into.split('/').any(|c| c == "..") || into.starts_with('/')) {
            return Err(Error::Msg("That folder has to be inside the game's folder.".into()));
        }
        if m.name.contains(['/', '\\']) {
            return Err(Error::Msg("A file name can't have folders in it.".into()));
        }
    }
    let mut l = st.lib.lock().unwrap();
    let g = l.installed.get_mut(&id).ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    // A program inside the game's folder is kept by its place there, so it still works after the
    // folder moves.
    let mut manual = manual;
    if let Some(e) = manual.as_mut().and_then(|m| m.exe.as_mut()) {
        if let Ok(rel) = e.strip_prefix(&g.path) {
            *e = rel.to_path_buf();
        }
    }
    g.manual = manual;
    let out = g.clone();
    drop(l);
    st.save();
    trail::note(format!("{id}: set up by hand {}", if out.manual.is_some() { "on" } else { "off" }));
    Ok(out)
}

/// Troubleshoot: a Look tries both ways in again (Direct3D and Vulkan).
#[tauri::command(async)]
fn look_reset_route(st: State<'_, App>, id: String) {
    looks::reset_route(&st.dir, &id);
    trail::note(format!("Look for {id}: tries both ways in again"));
}

/// Unblock: takes Windows' "downloaded from the internet" mark off a game's files.
#[tauri::command(async)]
fn unblock_game(st: State<'_, App>, id: String) -> Res<u32> {
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut n = 0u32;
    #[cfg(windows)]
    {
        fn walk(d: &Path, n: &mut u32, depth: u32) {
            for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    if depth < 6 {
                        walk(&p, n, depth + 1);
                    }
                } else if std::fs::remove_file(format!("{}:Zone.Identifier", p.display())).is_ok() {
                    *n += 1;
                }
            }
        }
        walk(&g.path, &mut n, 0);
    }
    #[cfg(not(windows))]
    let _ = g;
    Ok(n)
}

/// A Linux build's start script for a Windows one: the same name as .sh, or the usual names. Only
/// in a folder that has a Linux program in it, and not a script made for Macs: a Windows build can
/// carry a Mac launcher.sh, and then its .cmd plays through Proton instead.
fn linux_twin(win: &Path) -> Option<PathBuf> {
    let dir = win.parent()?;
    let stem = win.file_stem()?.to_string_lossy().to_string();
    let wanted = [format!("{stem}.sh"), format!("{}.sh", stem.to_lowercase()), "launcher.sh".into(), "Launcher.sh".into(), "run.sh".into(), "start.sh".into(), "play.sh".into()];
    let sh = wanted.iter().map(|n| dir.join(n)).find(|p| p.is_file())?;
    let text = std::fs::read(&sh).map(|b| String::from_utf8_lossy(&b[..b.len().min(64_000)]).to_lowercase()).unwrap_or_default();
    let mac_only = (text.contains("darwin") || text.contains("macos")) && !text.contains("linux");
    (!mac_only && has_linux_program(dir, 2)).then_some(sh)
}

/// Is there a Linux program (an ELF file) in this folder or the ones just inside it?
fn has_linux_program(dir: &Path, depth: u32) -> bool {
    let mut seen = 0;
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            if depth > 0 && has_linux_program(&p, depth - 1) {
                return true;
            }
            continue;
        }
        seen += 1;
        if seen > 600 {
            break;
        }
        let mut head = [0u8; 4];
        if std::fs::File::open(&p).and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head)).is_ok() && &head == b"\x7fELF" {
            return true;
        }
    }
    false
}

/// Is a program with this file name running now? (Windows; elsewhere it's never asked.)
fn already_open(exe: &Path) -> bool {
    #[cfg(windows)]
    {
        let Some(name) = exe.file_name().map(|n| n.to_string_lossy().to_string()) else { return false };
        // Names too common to be sure it's this game.
        let low = name.to_lowercase();
        if !low.ends_with(".exe") || ["launcher.exe", "game.exe", "start.exe", "main.exe", "setup.exe", "play.exe", "run.exe"].contains(&low.as_str()) {
            return false;
        }
        win::is_running(&name)
    }
    #[cfg(not(windows))]
    {
        let _ = exe;
        false
    }
}

/// A log whose first dated line is from more than a day before `since`: one a game renamed or
/// copied (log rotation keeps the old file's times), not the one from this start.
fn old_inside(p: &Path, since: u64) -> bool {
    use std::io::Read;
    let mut head = [0u8; 400];
    let n = std::fs::File::open(p).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
    let text = String::from_utf8_lossy(&head[..n]);
    let b = text.as_bytes();
    for i in 0..b.len().saturating_sub(10) {
        let d = &b[i..i + 10];
        let digits = |r: std::ops::Range<usize>| d[r].iter().all(u8::is_ascii_digit);
        if d[0] == b'2' && d[1] == b'0' && digits(0..4) && d[4] == b'-' && digits(5..7) && d[7] == b'-' && digits(8..10) {
            let num = |r: std::ops::Range<usize>| std::str::from_utf8(&d[r]).ok().and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
            let day = days_from_civil(num(0..4), num(5..7), num(8..10));
            return day * 86400 + 86400 < since as i64;
        }
    }
    false
}

/// Days since 1970-01-01 for a calendar date.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Where what a game printed on its last start is kept.
fn game_output_path(dir: &Path, id: &str) -> PathBuf {
    let safe: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    dir.join("game-output").join(format!("{safe}.log"))
}

/// A fresh file for what a game prints on this start (the last start's is replaced).
fn game_output_file(dir: &Path, id: &str) -> Option<std::fs::File> {
    let p = game_output_path(dir, id);
    std::fs::create_dir_all(p.parent()?).ok()?;
    std::fs::File::create(&p).ok()
}

#[derive(Serialize)]
struct Shot {
    id: String,
    game: String,
    path: String,
    time: u64,
}

/// Every installed game's screenshots, newest first.
#[tauri::command(async)]
fn all_screenshots(st: State<'_, App>) -> Vec<Shot> {
    let games: Vec<(String, String)> = st.lib.lock().unwrap().installed.values().map(|g| (g.id.clone(), g.name.clone())).collect();
    let mut out = vec![];
    for (id, name) in games {
        for p in ingame::list_shots(&name) {
            let time = std::fs::metadata(&p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
            out.push(Shot { id: id.clone(), game: name.clone(), path: p.to_string_lossy().into(), time });
        }
    }
    out.sort_by(|a, b| b.time.cmp(&a.time));
    out
}

/// Only files inside Pictures\RecompLauncher.
fn shot_ok(path: &str) -> Res<PathBuf> {
    let p = PathBuf::from(path);
    let root = ingame::shots_dir("x");
    let root = root.parent().unwrap_or(&root);
    if !p.starts_with(root) || !p.is_file() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(Error::Msg("That isn't one of your screenshots.".into()));
    }
    Ok(p)
}

#[tauri::command(async)]
fn delete_screenshot(path: String) -> Res<()> {
    std::fs::remove_file(shot_ok(&path)?)?;
    Ok(())
}

#[tauri::command]
async fn copy_screenshot(path: String) -> Res<()> {
    let p = shot_ok(&path)?;
    tokio::task::spawn_blocking(move || media::copy_image(&p)).await.map_err(|e| Error::Msg(e.to_string()))?.map_err(|e| Error::Msg(format!("Couldn't copy it: {e}")))
}

#[tauri::command(async)]
fn show_screenshot(app: AppHandle, path: String) -> Res<()> {
    let p = shot_ok(&path)?;
    #[cfg(target_os = "linux")]
    return open_thing(&app, &p.parent().unwrap_or(&p).to_string_lossy(), false);
    #[cfg(not(target_os = "linux"))]
    {
        use tauri_plugin_opener::OpenerExt;
        app.opener().reveal_item_in_dir(p).map_err(|e| Error::Msg(e.to_string()))
    }
}

#[tauri::command(async)]
fn controllers() -> Vec<pads::Pad> {
    pads::connected()
}

/// Keep these pictures on disk, so pages show at once and work offline.
#[tauri::command(async)]
fn prefetch_images(st: State<'_, App>, urls: Vec<String>) {
    let (http, dir) = (st.http.clone(), st.dir.join("cache/img"));
    tauri::async_runtime::spawn(media::prefetch(http, dir, urls));
}

#[tauri::command(async)]
fn screenshots(st: State<'_, App>, id: String) -> Vec<String> {
    let name = st.lib.lock().unwrap().installed.get(&id).map(|g| g.name.clone()).unwrap_or_default();
    if name.is_empty() {
        return vec![];
    }
    ingame::shot_paths(&name)
}

#[tauri::command(async)]
fn open_shots(app: AppHandle, st: State<'_, App>, id: String) -> Res<()> {
    let name = st.lib.lock().unwrap().installed.get(&id).map(|g| g.name.clone()).unwrap_or_default();
    let d = ingame::shots_dir(&name);
    std::fs::create_dir_all(&d)?;
    open_thing(&app, &d.to_string_lossy(), false)
}

/// From the quick menu: "resume", "screenshot", "close" (the game), "launcher" (show the window).
#[tauri::command(async)]
fn overlay_action(app: AppHandle, action: String) {
    // Only from the quick menu while it's up: a hidden menu acting on a stray press must never
    // close a game or jump to the screenshots.
    if !app.state::<App>().overlay_open.load(Ordering::Relaxed) && action != "resume" {
        return;
    }
    match action.as_str() {
        "screenshot" => ingame::take_screenshot(&app),
        "close" => {
            ingame::hide_overlay(&app);
            let st = app.state::<App>();
            let ids: Vec<String> = st.running.lock().unwrap().keys().cloned().collect();
            for id in ids {
                if let Some(tx) = st.running.lock().unwrap().remove(&id) {
                    let _ = tx.send(());
                }
            }
        }
        "launcher" => {
            ingame::hide_overlay(&app);
            show_main(&app);
            // Straight to the game's screenshots; the game keeps running.
            let id = app.state::<App>().running.lock().unwrap().keys().next().cloned();
            let _ = app.emit("goto", json!({"view": "shots", "id": id}));
        }
        _ => ingame::hide_overlay(&app),
    }
}

/// The program to start, picked by the player when the launcher's guess was wrong.
#[tauri::command(async)]
fn set_exe(st: State<'_, App>, id: String, path: String) -> Res<Installed> {
    let mut l = st.lib.lock().unwrap();
    let g = l.installed.get_mut(&id).ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let p = PathBuf::from(&path);
    if !p.is_file() {
        return Err(Error::Msg("That file doesn't exist.".into()));
    }
    let outside = !p.starts_with(&g.path);
    g.exe = Some(p.strip_prefix(&g.path).map(Path::to_path_buf).unwrap_or(p));
    // The player's pick: the launcher never picks again over it.
    g.exe_rules = u8::MAX;
    // A program somewhere else: the game its own installer put there, so its setup is done.
    if outside {
        g.setup_done = true;
    }
    let out = g.clone();
    drop(l);
    st.save();
    Ok(out)
}

#[tauri::command(async)]
fn open_folder(app: AppHandle, st: State<'_, App>, id: Option<String>, path: Option<String>) -> Res<()> {
    #[cfg(not(target_os = "linux"))]
    use tauri_plugin_opener::OpenerExt;
    let p = {
        let l = st.lib.lock().unwrap();
        match (id, path) {
            (Some(id), _) => l.installed.get(&id).map(|g| g.path.clone()).ok_or_else(|| Error::Msg("It isn't installed.".into()))?,
            // Only folders the launcher knows about: libraries and ROM folders.
            (None, Some(p)) => {
                let p = PathBuf::from(p);
                let known = l.settings.all_libraries().contains(&p)
                    || l.settings.rom_dirs.contains(&p)
                    || l.roms.iter().any(|r| r.path == p || r.path.parent() == Some(p.as_path()))
                    || p == st.dir
                    || PORTABLE_MADE.lock().unwrap().as_ref() == Some(&p);
                if !known {
                    return Err(Error::Msg("That folder isn't one of the launcher's.".into()));
                }
                if p.is_file() {
                    #[cfg(target_os = "linux")]
                    return open_thing(&app, &p.parent().unwrap_or(&p).to_string_lossy(), false);
                    #[cfg(not(target_os = "linux"))]
                    return app.opener().reveal_item_in_dir(&p).map_err(|e| Error::Msg(e.to_string()));
                }
                p
            }
            (None, None) => l.settings.library_dir.clone(),
        }
    };
    std::fs::create_dir_all(&p)?;
    open_thing(&app, &p.to_string_lossy(), false)
}

// ---------- the catalog ----------

#[tauri::command]
async fn get_catalog(app: AppHandle, st: State<'_, App>, force: bool) -> Res<catalog::Fetched> {
    let f = catalog::catalog(&st.http, &st.dir, force).await?;
    if !f.offline {
        note_new_apps(&app, &st, &f.data);
    }
    Ok(f)
}

#[tauri::command]
async fn get_releases(st: State<'_, App>, repo: String, force: bool) -> Res<catalog::Fetched> {
    catalog::releases(&st.http, &st.dir, &repo, force).await
}

/// Installed games with a newer release than the one installed: id -> the new tag.
#[tauri::command]
async fn check_updates(st: State<'_, App>) -> Res<HashMap<String, String>> {
    let games: Vec<Installed> = st.lib.lock().unwrap().installed.values().cloned().collect();
    let mut out = HashMap::new();
    let (mut asked, mut failed) = (0, 0);
    for g in games.into_iter().filter(|g| !g.repo.is_empty() && !g.local) {
        asked += 1;
        let Ok(f) = catalog::releases(&st.http, &st.dir, &g.repo, false).await else {
            failed += 1;
            continue;
        };
        let Ok(rels) = serde_json::from_value::<Vec<Release>>(f.data) else { continue };
        // Added from the player's own folder without a version: the release out when its program
        // was made is the one they have, so an update isn't offered for the version they're on.
        let mut g = g;
        if g.tag.is_empty() {
            if let Some(tag) = version_from_files(&g, &rels) {
                if let Some(x) = st.lib.lock().unwrap().installed.get_mut(&g.id) {
                    x.tag = tag.clone();
                }
                st.save();
                g.tag = tag;
            }
        }
        if let Some(r) = catalog::latest_for(&rels, &g.tag) {
            if !g.pinned && r.tag_name != g.tag && assets::pick(&r.assets, if g.proton { "windows" } else { assets::os_now() }, assets::arch_now(), None).is_some() {
                out.insert(g.id.clone(), r.tag_name.clone());
            }
        }
    }
    // Not one answer: GitHub can't be reached from here (offline, or blocked on this network).
    if asked > 0 && failed == asked {
        return Err(Error::Msg("Can't reach GitHub right now, so updates can't be checked. Check your connection (some networks block GitHub), then try again.".into()));
    }
    Ok(out)
}

/// Which release a game added from the player's folder is: the newest one published by the day its
/// program was last written (a day's grace for time zones).
fn version_from_files(g: &Installed, rels: &[Release]) -> Option<String> {
    let exe = g.path.join(g.exe.as_ref()?);
    let made = std::fs::metadata(&exe).ok()?.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64 / 86400;
    let day = |s: &str| -> Option<i64> {
        let d = s.get(..10)?;
        Some(days_from_civil(d.get(..4)?.parse().ok()?, d.get(5..7)?.parse().ok()?, d.get(8..10)?.parse().ok()?))
    };
    rels.iter()
        .filter(|r| !r.draft)
        .filter_map(|r| Some((day(r.published_at.as_deref()?)?, r)))
        .filter(|(d, _)| *d <= made + 1)
        .max_by_key(|(d, _)| *d)
        .map(|(_, r)| r.tag_name.clone())
}

/// A newer RecompLauncher, from recomplauncher.com/launcher/version.json.
/// What's in launcher/version.json now: for "What's new" after an update.
#[tauri::command]
async fn launcher_notes(st: State<'_, App>) -> Res<Value> {
    let url = format!("{}/launcher/version.json", catalog::base());
    let r = st.http.get(url).timeout(std::time::Duration::from_secs(10)).send().await.map_err(|e| Error::Net(e.to_string()))?;
    Ok(if r.status().is_success() { r.json().await.unwrap_or(Value::Null) } else { Value::Null })
}

#[tauri::command]
async fn launcher_update(st: State<'_, App>) -> Res<Option<Value>> {
    let url = format!("{}/launcher/version.json", catalog::base());
    let Ok(r) = st.http.get(url).timeout(std::time::Duration::from_secs(10)).send().await else { return Ok(None) };
    if !r.status().is_success() {
        return Ok(None);
    }
    let v: Value = r.json().await.unwrap_or(Value::Null);
    // Linux has its own build (an AppImage), listed under "linux". A list without one has no
    // update for Linux yet.
    let v = if cfg!(windows) { v } else { linux_entry(&v).unwrap_or(Value::Null) };
    let newest = str_of(&v, "version").to_string();
    let mut v = v;
    // Older than the list's "min": the update isn't optional any more.
    let min = str_of(&v, "min").to_string();
    if !min.is_empty() && version_key(env!("CARGO_PKG_VERSION")) < version_key(&min) && v.is_object() {
        v["required"] = Value::Bool(true);
    }
    Ok((!newest.is_empty() && version_key(&newest) > version_key(env!("CARGO_PKG_VERSION"))).then_some(v))
}

/// version.json's Linux build as an entry of its own: its version (or the shared one), url and sha256.
fn linux_entry(v: &Value) -> Option<Value> {
    let l = v.get("linux")?;
    let url = str_of(l, "url");
    if url.is_empty() {
        return None;
    }
    let mut out = v.clone();
    for k in ["version", "url", "sha256", "notes"] {
        if let Some(x) = l.get(k) {
            out[k] = x.clone();
        }
    }
    out.as_object_mut()?.remove("linux");
    Some(out)
}

/// 0.5.0-beta.1 < 0.5.0-beta.2 < 0.5.0 < 0.5.1: a beta comes before its release.
fn version_key(s: &str) -> (u64, u64, u64, u64, u64, u64) {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let (core, pre) = s.split_once('-').unwrap_or((s, ""));
    let mut n = core.split('.').map(|p| p.trim().parse::<u64>().unwrap_or(0));
    let (a, b, c) = (n.next().unwrap_or(0), n.next().unwrap_or(0), n.next().unwrap_or(0));
    if pre.is_empty() {
        return (a, b, c, 1, 0, 0);
    }
    // beta.3.1: 3, then 1.
    let mut nums = pre.split(|ch: char| !ch.is_ascii_digit()).filter(|p| !p.is_empty()).map(|p| p.parse::<u64>().unwrap_or(0));
    (a, b, c, 0, nums.next().unwrap_or(0), nums.next().unwrap_or(0))
}

/// A game's releases that have a version for this PC, newest first: for going back to an older
/// one (or forward again).
#[derive(Serialize)]
struct Version {
    tag: String,
    name: String,
    date: String,
    pre: bool,
    newest: bool,
}

#[tauri::command]
async fn versions(st: State<'_, App>, id: String) -> Res<Vec<Version>> {
    let cat = catalog::catalog(&st.http, &st.dir, false).await?.data;
    let entry = catalog::app(&cat, &id).cloned().unwrap_or(Value::Null);
    let repo = str_of(&entry, "repo").to_string();
    let repo = if repo.is_empty() { st.lib.lock().unwrap().installed.get(&id).map(|g| g.repo.clone()).unwrap_or_default() } else { repo };
    let rels: Vec<Release> = serde_json::from_value(catalog::releases(&st.http, &st.dir, &repo, false).await?.data)?;
    let newest = catalog::latest(&rels).map(|r| r.tag_name.clone()).unwrap_or_default();
    let (os, arch) = (os_for(&st, &entry, &id), assets::arch_now());
    let pattern = entry.get("assets").and_then(|a| a.get(os)).and_then(Value::as_str);
    Ok(rels
        .iter()
        .filter(|r| !r.draft && assets::pick(&r.assets, os, arch, pattern).is_some())
        .take(15)
        .map(|r| Version {
            tag: r.tag_name.clone(),
            name: r.name.clone().unwrap_or_default(),
            date: r.published_at.clone().unwrap_or_default(),
            pre: r.prerelease,
            newest: r.tag_name == newest,
        })
        .collect())
}

/// Recomps the catalog lists without a version for this system: does their latest release have
/// one after all? id -> true for the ones that do.
#[tauri::command]
async fn build_check(st: State<'_, App>, ids: Vec<String>) -> Res<HashMap<String, bool>> {
    let cat = catalog::catalog(&st.http, &st.dir, false).await?.data;
    let (os, arch) = (assets::os_now(), assets::arch_now());
    let mut out = HashMap::new();
    for id in ids.into_iter().take(80) {
        let Some(entry) = catalog::app(&cat, &id) else { continue };
        let repo = str_of(entry, "repo");
        if repo.is_empty() {
            continue;
        }
        let Ok(f) = catalog::releases(&st.http, &st.dir, repo, false).await else { continue };
        let Ok(rels) = serde_json::from_value::<Vec<Release>>(f.data) else { continue };
        let pattern = entry.get("assets").and_then(|a| a.get(os)).and_then(Value::as_str);
        if let Some(r) = catalog::latest(&rels) {
            out.insert(id.clone(), assets::pick(&r.assets, os, arch, pattern).is_some());
        }
    }
    Ok(out)
}

/// What's new for the installed games: their releases from the last 60 days, newest first.
#[derive(Serialize)]
struct News {
    id: String,
    name: String,
    tag: String,
    title: String,
    date: String,
    notes: String,
    url: String,
    /// It's the version installed.
    have: bool,
}

#[tauri::command]
async fn news(st: State<'_, App>) -> Res<Vec<News>> {
    let games: Vec<Installed> = st.lib.lock().unwrap().installed.values().cloned().collect();
    let cutoff = chrono::Utc::now() - chrono::Duration::days(60);
    let mut out = vec![];
    for g in games.into_iter().filter(|g| !g.repo.is_empty() && !g.local) {
        let Ok(f) = catalog::releases(&st.http, &st.dir, &g.repo, false).await else { continue };
        let Ok(rels) = serde_json::from_value::<Vec<Release>>(f.data) else { continue };
        for r in rels.iter().filter(|r| !r.draft).take(3) {
            let date = r.published_at.clone().unwrap_or_default();
            let when = chrono::DateTime::parse_from_rfc3339(&date).map(|d| d.with_timezone(&chrono::Utc)).ok();
            if when.map(|w| w < cutoff).unwrap_or(true) {
                continue;
            }
            let notes: String = r.body.clone().unwrap_or_default().chars().take(1200).collect();
            out.push(News {
                id: g.id.clone(),
                name: g.name.clone(),
                tag: r.tag_name.clone(),
                title: r.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| r.tag_name.clone()),
                date,
                notes,
                url: r.html_url.clone().unwrap_or_default(),
                have: r.tag_name == g.tag,
            });
        }
    }
    out.sort_by(|a, b| b.date.cmp(&a.date));
    out.truncate(10);
    Ok(out)
}

/// About the original game, from Wikipedia.
#[tauri::command]
async fn game_info(st: State<'_, App>, id: String) -> Res<Option<info::Info>> {
    let cat = catalog::catalog(&st.http, &st.dir, false).await.map(|f| f.data).unwrap_or(Value::Null);
    let Some(a) = catalog::app(&cat, &id) else { return Ok(None) };
    let console = cat
        .get("origins")
        .and_then(Value::as_array)
        .and_then(|o| o.iter().find(|x| x.get("id").and_then(Value::as_str) == Some(str_of(a, "origin"))))
        .map(|o| str_of(o, "name").to_string())
        .unwrap_or_default();
    Ok(info::game_info(&st.http, &st.dir, str_of(a, "game"), &console).await)
}

// ---------- mods ----------

async fn mod_source(st: &App, id: &str) -> Res<(mods::Source, PathBuf)> {
    let cat = catalog::catalog(&st.http, &st.dir, false).await.map(|f| f.data).unwrap_or(Value::Null);
    let entry = catalog::app(&cat, id).cloned().unwrap_or(Value::Null);
    let src = mods::source(id, &entry).ok_or_else(|| Error::Msg("This game doesn't take mods from the launcher.".into()))?;
    let g = st.lib.lock().unwrap().installed.get(id).cloned().ok_or_else(|| Error::Msg("Install the game first.".into()))?;
    let dir = mods::mods_dir(&g.path, &src.data);
    Ok((src, dir))
}

/// A game's GameBanana page and where its mods go: (source, install folder, program's folder).
async fn gb_source(st: &App, id: &str) -> Res<(gamebanana::Source, PathBuf, PathBuf)> {
    let cat = catalog::catalog(&st.http, &st.dir, false).await.map(|f| f.data).unwrap_or(Value::Null);
    let entry = catalog::app(&cat, id).cloned().unwrap_or(Value::Null);
    let src = gamebanana::source(id, &entry).ok_or_else(|| Error::Msg("This game doesn't take mods from GameBanana.".into()))?;
    let g = st.lib.lock().unwrap().installed.get(id).cloned().ok_or_else(|| Error::Msg("Install the game first.".into()))?;
    let exe_dir = g.exe.as_ref().and_then(|e| g.path.join(e).parent().map(Path::to_path_buf)).unwrap_or_else(|| g.path.clone());
    Ok((src, g.path.clone(), exe_dir))
}

#[derive(Serialize)]
struct ModsState {
    /// The Thunderstore community, when the game has one.
    community: String,
    /// The game's id on GameBanana, when it has a page there.
    gamebanana: u64,
    dir: PathBuf,
    installed: Vec<mods::ModRec>,
    /// Mods in the folder put there by hand.
    others: Vec<String>,
}

/// A game's mods: where they come from, the folder, and what's in it. None for games without.
#[tauri::command]
async fn mods_state(st: State<'_, App>, id: String) -> Res<Option<ModsState>> {
    let cat = catalog::catalog(&st.http, &st.dir, false).await.map(|f| f.data).unwrap_or(Value::Null);
    let entry = catalog::app(&cat, &id).cloned().unwrap_or(Value::Null);
    let ts = mods::source(&id, &entry);
    let gb = gamebanana::source(&id, &entry);
    if ts.is_none() && gb.is_none() {
        return Ok(None);
    }
    let community = ts.as_ref().map(|s| s.community.clone()).unwrap_or_default();
    let gamebanana = gb.as_ref().map(|s| s.game).unwrap_or(0);
    let g = st.lib.lock().unwrap().installed.get(&id).cloned();
    let Some(g) = g else { return Ok(Some(ModsState { community, gamebanana, dir: PathBuf::new(), installed: vec![], others: vec![] })) };
    let exe_dir = g.exe.as_ref().and_then(|e| g.path.join(e).parent().map(Path::to_path_buf)).unwrap_or_else(|| g.path.clone());
    let dir = match (&ts, &gb) {
        (Some(t), _) => mods::mods_dir(&g.path, &t.data),
        (None, Some(b)) => gamebanana::mods_dir(&b.loader, &g.path, &exe_dir),
        _ => PathBuf::new(),
    };
    let gb_dir = gb.as_ref().map(|b| gamebanana::mods_dir(&b.loader, &g.path, &exe_dir));
    let mut installed = st.lib.lock().unwrap().mods.get(&id).cloned().unwrap_or_default();
    // Mods deleted by hand no longer count.
    installed.retain(|m| {
        let base = if m.full.starts_with("gb:") { gb_dir.clone().unwrap_or_else(|| dir.clone()) } else { dir.clone() };
        m.files.iter().any(|f| match f.strip_prefix("dir:") { Some(d) => base.join(d).is_dir(), None => base.join(f).is_file() })
    });
    let others = if ts.is_some() { mods::others(&dir, &installed) } else { vec![] };
    Ok(Some(ModsState { community, gamebanana, dir, installed, others }))
}

#[tauri::command]
async fn mods_online(st: State<'_, App>, id: String) -> Res<Vec<mods::Pkg>> {
    let cat = catalog::catalog(&st.http, &st.dir, false).await.map(|f| f.data).unwrap_or(Value::Null);
    let entry = catalog::app(&cat, &id).cloned().unwrap_or(Value::Null);
    let src = mods::source(&id, &entry).ok_or_else(|| Error::Msg("This game doesn't take mods from the launcher.".into()))?;
    mods::list(&st.http, &st.dir, &src.community).await
}

/// A page of a game's mods on GameBanana.
#[tauri::command]
async fn gb_mods(st: State<'_, App>, id: String, sort: String, page: u32, q: String) -> Res<gamebanana::Page> {
    let cat = catalog::catalog(&st.http, &st.dir, false).await.map(|f| f.data).unwrap_or(Value::Null);
    let entry = catalog::app(&cat, &id).cloned().unwrap_or(Value::Null);
    let src = gamebanana::source(&id, &entry).ok_or_else(|| Error::Msg("This game doesn't take mods from GameBanana.".into()))?;
    gamebanana::list(&st.http, src.game, &sort, page, &q).await
}

#[tauri::command]
async fn gb_mod(st: State<'_, App>, mod_id: u64) -> Res<gamebanana::Detail> {
    gamebanana::detail(&st.http, mod_id).await
}

/// Install one of a GameBanana mod's files.
#[tauri::command]
async fn gb_install(app: AppHandle, st: State<'_, App>, id: String, mod_id: u64, file_id: u64, variant: Option<String>) -> Res<mods::ModRec> {
    if st.running.lock().unwrap().contains_key(&id) {
        return Err(Error::Msg("Close the game first.".into()));
    }
    let _turn = mod_turn(&id).await;
    let (src, install, exe_dir) = gb_source(&st, &id).await?;
    let d = gamebanana::detail(&st.http, mod_id).await?;
    let file = d.files.iter().find(|f| f.id == file_id).cloned().ok_or_else(|| Error::Msg("That file isn't on the mod's page anymore.".into()))?;
    let key = format!("gb:{mod_id}");
    // What this mod put there before (it may be replaced), and what the game's other mods use.
    let (owned, others) = mod_files(&st, &id, &key);
    let work = st.dir.join("cache/gamebanana").join(format!("{id}-{file_id}"));
    // Asked which version: the mod unpacked last time is still here.
    let ready = variant.is_some() && work.join("unpacked.done").is_file();
    let archive = work.join("download");
    if !ready {
        let _ = std::fs::remove_dir_all(&work);
        let (app2, key2) = (app.clone(), key.clone());
        let say = move |done: u64, total: u64| {
            let _ = app2.emit("mod-progress", json!({"id": key2, "done": done, "total": total}));
        };
        let got = gamebanana::download(&st.http, &file, &archive, &say).await;
        if let Err(e) = got {
            let _ = std::fs::remove_dir_all(&work);
            return Err(e);
        }
    }
    let (loader, name, own2) = (src.loader.clone(), d.name.clone(), owned.clone());
    let (w2, i2, e2) = (work.clone(), install.clone(), exe_dir.clone());
    let files = tauri::async_runtime::spawn_blocking(move || -> Res<Vec<String>> {
        let out = w2.join("unpacked");
        if !ready {
            gamebanana::extract(&archive, &out)?;
            let _ = std::fs::remove_file(&archive);
            let _ = std::fs::write(w2.join("unpacked.done"), b"");
        }
        gamebanana::install(&loader, &out, &i2, &e2, &name, mod_id, &own2, variant.as_deref())
    })
    .await
    .map_err(|e| Error::Msg(e.to_string()))?;
    let asking = matches!(&files, Err(Error::Msg(m)) if m.starts_with("variants:"));
    if !asking {
        let _ = std::fs::remove_dir_all(&work);
    }
    let files = files?;
    // Installing again (another file, or an update): what the old one had and the new one
    // doesn't comes out.
    {
        let mut l = st.lib.lock().unwrap();
        l.mods.entry(id.clone()).or_default().retain(|m| m.full != key);
    }
    let gone: Vec<String> = owned.into_iter().filter(|f| !files.contains(f)).collect();
    if !gone.is_empty() {
        gamebanana::remove(&src.loader, &install, &exe_dir, u64::MAX, &gone, &others);
    }
    let rec = mods::ModRec { full: key, name: d.name.clone(), version: if file.version.is_empty() { file.name.clone() } else { file.version.clone() }, files, dep: false };
    st.lib.lock().unwrap().mods.entry(id.clone()).or_default().push(rec.clone());
    st.save();
    Ok(rec)
}

/// A game's mods change one at a time (installs and removals wait their turn), so two never
/// write the same files or the same ModsDB.ini at once.
async fn mod_turn(id: &str) -> tokio::sync::OwnedMutexGuard<()> {
    static TURNS: std::sync::OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = std::sync::OnceLock::new();
    let m = TURNS.get_or_init(Default::default).lock().unwrap().entry(id.to_string()).or_default().clone();
    m.lock_owned().await
}

/// The files a mod put in, and the files the game's other mods use.
fn mod_files(st: &App, id: &str, full: &str) -> (Vec<String>, Vec<String>) {
    let l = st.lib.lock().unwrap();
    let list = l.mods.get(id).cloned().unwrap_or_default();
    let mine = list.iter().find(|m| m.full == full).map(|m| m.files.clone()).unwrap_or_default();
    let others = list.iter().filter(|m| m.full != full).flat_map(|m| m.files.clone()).collect();
    (mine, others)
}

/// Install a mod (and the mods it needs) from Thunderstore.
#[tauri::command]
async fn mod_install(st: State<'_, App>, id: String, full: String) -> Res<Vec<mods::ModRec>> {
    let _turn = mod_turn(&id).await;
    let (src, dir) = mod_source(&st, &id).await?;
    let all = mods::list(&st.http, &st.dir, &src.community).await?;
    let mut todo = vec![(full.clone(), false)];
    let mut done: Vec<mods::ModRec> = vec![];
    while let Some((name, dep)) = todo.pop() {
        if done.iter().any(|d| d.full == name) {
            continue;
        }
        let have = st.lib.lock().unwrap().mods.get(&id).map(|v| v.iter().any(|m| m.full == name)).unwrap_or(false);
        let Some(p) = all.iter().find(|p| p.full == name) else {
            if dep {
                continue; // a helper Thunderstore doesn't list for this game: skip it
            }
            return Err(Error::Msg("That mod isn't on Thunderstore anymore.".into()));
        };
        if have && dep {
            continue;
        }
        if !p.url.starts_with("https://thunderstore.io/") && !p.url.starts_with("https://gcdn.thunderstore.io/") {
            return Err(Error::Msg("That mod's download isn't on Thunderstore.".into()));
        }
        let r = st.http.get(&p.url).timeout(std::time::Duration::from_secs(120)).send().await?;
        if !r.status().is_success() {
            return Err(Error::Msg(format!("Thunderstore answered {} for {}", r.status().as_u16(), p.name)));
        }
        let bytes = r.bytes().await?.to_vec();
        let d2 = dir.clone();
        let files = tokio::task::spawn_blocking(move || mods::unpack(&bytes, &d2)).await.map_err(|e| Error::Msg(e.to_string()))??;
        done.push(mods::ModRec { full: p.full.clone(), name: p.name.clone(), version: p.version.clone(), files, dep });
        for d in &p.deps {
            todo.push((d.clone(), true));
        }
    }
    {
        let mut l = st.lib.lock().unwrap();
        let list = l.mods.entry(id.clone()).or_default();
        for m in &done {
            list.retain(|x| x.full != m.full);
            list.push(m.clone());
        }
    }
    st.save();
    Ok(done)
}

#[tauri::command]
async fn mod_remove(st: State<'_, App>, id: String, full: String) -> Res<()> {
    if st.running.lock().unwrap().contains_key(&id) {
        return Err(Error::Msg("Close the game first.".into()));
    }
    let _turn = mod_turn(&id).await;
    let rec = st.lib.lock().unwrap().mods.get(&id).and_then(|l| l.iter().find(|m| m.full == full).cloned());
    if let Some(r) = &rec {
        // Files another mod also uses stay.
        let (_, others) = mod_files(&st, &id, &full);
        if let Some(mid) = full.strip_prefix("gb:").and_then(|x| x.parse::<u64>().ok()) {
            let (src, install, exe_dir) = gb_source(&st, &id).await?;
            gamebanana::remove(&src.loader, &install, &exe_dir, mid, &r.files, &others);
        } else {
            let (_, dir) = mod_source(&st, &id).await?;
            let mine: Vec<String> = r.files.iter().filter(|f| !others.contains(f)).cloned().collect();
            mods::remove(&dir, &mine);
        }
    }
    if let Some(list) = st.lib.lock().unwrap().mods.get_mut(&id) {
        list.retain(|m| m.full != full);
    }
    st.save();
    Ok(())
}

#[tauri::command]
async fn open_mods(app: AppHandle, st: State<'_, App>, id: String) -> Res<()> {
    let dir = match mod_source(&st, &id).await {
        Ok((_, dir)) => dir,
        Err(_) => {
            let (src, install, exe_dir) = gb_source(&st, &id).await?;
            gamebanana::mods_dir(&src.loader, &install, &exe_dir)
        }
    };
    std::fs::create_dir_all(&dir)?;
    open_thing(&app, &dir.to_string_lossy(), false)
}

// ---------- reports and the launcher's own updates ----------

/// Send a crash (the game's log and a note) to RecompLauncher's inbox on recomplauncher.com.
#[tauri::command]
async fn send_report(st: State<'_, App>, id: String, message: String, log: Option<String>, kind: Option<String>) -> Res<String> {
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let category = if kind.as_deref() == Some("bug") { "bug" } else { "crash" };
    let mut form: Vec<(String, String)> = vec![
        ("app".into(), id.clone()),
        ("version".into(), g.tag.clone()),
        ("category".into(), category.into()),
        ("message".into(), format!("RecompLauncher {} on {} ({})\n\n{}", env!("CARGO_PKG_VERSION"), assets::os_now(), assets::arch_now(), message.chars().take(1800).collect::<String>())),
    ];
    // The log the crash screen found: only from the places it looks (the game's folder, the logs
    // folder its profile names, ReShade's log for this game), and only a .log or .txt.
    let logs_dir = profile_of(&st, &id, &g).and_then(|p| p.folder("logs", &g.path));
    let reshade = st.dir.join("looks").join(&id).join("ReShade.log");
    let printed = game_output_path(&st.dir, &id);
    let allowed = |p: &Path| {
        let n = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        (n.ends_with(".log") || n.ends_with(".txt"))
            && (p.starts_with(&g.path) || logs_dir.as_ref().map(|d| p.starts_with(d)).unwrap_or(false) || p == reshade.as_path() || p == printed.as_path())
    };
    if let Some(p) = log.map(PathBuf::from).filter(|p| allowed(p) && p.is_file()) {
        let bytes = std::fs::read(&p)?;
        let text = String::from_utf8_lossy(&bytes[bytes.len().saturating_sub(380_000)..]).to_string();
        form.push(("name[]".into(), p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "log.txt".into())));
        form.push(("text[]".into(), text));
        // ReShade's log alone says little about the game: what the game printed goes too.
        if p == reshade && printed.is_file() {
            let fresh = std::fs::metadata(&printed).ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).map(|e| e.as_secs() < 3600).unwrap_or(false);
            if fresh {
                if let Ok(b) = std::fs::read(&printed) {
                    form.push(("name[]".into(), "game-output.txt".into()));
                    form.push(("text[]".into(), String::from_utf8_lossy(&b[b.len().saturating_sub(200_000)..]).to_string()));
                }
            }
        }
    }
    // What the launcher started, and how the game was set up.
    if let Some(run) = last_run(&st.dir, &id) {
        form.push(("name[]".into(), "launcher-details.txt".into()));
        form.push(("text[]".into(), run));
    }
    if let Some(t) = trail::tail(24_000) {
        form.push(("name[]".into(), "launcher-trail.txt".into()));
        form.push(("text[]".into(), t));
    }
    let r = st.http.post(format!("{}/api/logs.php", catalog::base())).form(&form).timeout(std::time::Duration::from_secs(30)).send().await?;
    let v: Value = r.json().await.unwrap_or(Value::Null);
    if v.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        Ok(str_of(&v, "message").to_string())
    } else {
        Err(Error::Msg(Some(str_of(&v, "error")).filter(|s| !s.is_empty()).unwrap_or("The report couldn't be sent right now.").to_string()))
    }
}

/// The programs that came with a game (its own setup or tools among them), inside its folder.
#[tauri::command]
fn game_programs(st: State<'_, App>, id: String) -> Res<Vec<String>> {
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let os = if g.proton { "windows" } else { assets::os_now() };
    let mut out = vec![];
    let mut stack = vec![(g.path.clone(), 0usize)];
    while let Some((d, depth)) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if e.file_type().map(|t| t.is_symlink()).unwrap_or(true) {
                continue;
            }
            if p.is_dir() {
                if depth < 3 {
                    stack.push((p, depth + 1));
                }
            } else if install::is_program(&p, os) {
                if let Ok(rel) = p.strip_prefix(&g.path) {
                    out.push(rel.to_string_lossy().to_string());
                }
            }
            if out.len() >= 60 {
                return Ok(out);
            }
        }
    }
    Ok(out)
}

/// The folder with the game's files, from the one the player picked: that folder, or the one
/// under it (three levels down at most) that has everything in `need` (file or folder names, any
/// case: "default.xex", or "code", "content" and "meta"). None when nothing there has them.
#[tauri::command(async)]
fn game_root(dir: String, need: Vec<String>) -> Option<String> {
    let need: Vec<String> = need.iter().map(|n| n.trim().trim_end_matches(['/', '\\']).to_lowercase()).filter(|n| !n.is_empty()).collect();
    let has_all = |d: &Path| {
        let names: Vec<String> = std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().to_lowercase()).collect();
        need.iter().all(|n| names.contains(n))
    };
    let start = PathBuf::from(&dir);
    if need.is_empty() || has_all(&start) {
        return Some(dir);
    }
    let mut level = vec![start];
    for _ in 0..3 {
        let mut next = vec![];
        for d in &level {
            for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
                if e.file_type().map(|t| t.is_dir() && !t.is_symlink()).unwrap_or(false) {
                    let p = e.path();
                    if has_all(&p) {
                        return Some(p.to_string_lossy().to_string());
                    }
                    next.push(p);
                }
            }
        }
        next.truncate(400);
        level = next;
    }
    None
}

/// Starts one of a game's own programs (its setup, say), from inside its folder only.
#[tauri::command]
fn run_program(st: State<'_, App>, id: String, rel: String) -> Res<()> {
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let rel = PathBuf::from(&rel);
    if rel.is_absolute() || rel.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(Error::Msg("That isn't in the game's folder.".into()));
    }
    let p = g.path.join(&rel);
    if !p.is_file() || !install::is_program(&p, assets::os_now()) {
        return Err(Error::Msg("That program isn't there.".into()));
    }
    own_setup::spawn(&p).map_err(|e| if is_blocked(&e) { defender_msg(e, "the setup") } else { Error::Msg(format!("Couldn't start it: {e}")) })?;
    Ok(())
}

/// Beta feedback from Settings > Beta: what it's about, what kind of note it is, how the beta is
/// going, the player's words and, if they tick it, a short file about this PC and their games.
/// It lands in the admin's Inbox with the other logs, under the app "launcher".
#[tauri::command]
async fn beta_feedback(st: State<'_, App>, kind: String, area: String, mood: Option<String>, message: String, contact: Option<String>, attach: bool, game: Option<String>) -> Res<String> {
    let message = message.trim().to_string();
    let least = if kind == "bug" { 12 } else { 3 };
    if message.chars().filter(|c| !c.is_whitespace()).count() < least {
        return Err(Error::Msg("Tell us a little more: what you did and what happened.".into()));
    }
    // A problem with one game: its last log and what the launcher started go along.
    let game = game.filter(|g| !g.is_empty()).and_then(|id| st.lib.lock().unwrap().installed.get(&id).cloned());
    let category = match kind.as_str() {
        "bug" => "bug",
        "crash" => "crash",
        "install" => "install",
        "slow" => "slow",
        _ => "other",
    };
    let label = match kind.as_str() {
        "bug" => "Bug",
        "crash" => "Crash",
        "install" => "Won't install",
        "slow" => "Slow",
        "idea" => "Idea",
        _ => "Thoughts",
    };
    let clean = |s: &str, n: usize| s.chars().filter(|c| !c.is_control()).take(n).collect::<String>();
    let contact = clean(contact.as_deref().unwrap_or("").trim(), 120);
    let mut head = format!("[Beta] {label} · {}", clean(&area, 40));
    if let Some(m) = mood.as_deref().filter(|m| !m.is_empty()) {
        head.push_str(&format!(" · feels {}", clean(m, 16)));
    }
    if !contact.is_empty() {
        head.push_str(&format!("\nReply to: {contact}"));
    }
    if let Some(g) = &game {
        head.push_str(&format!("\nGame: {} ({} {})", g.name, g.id, g.tag));
    }
    let body = format!("{head}\nRecompLauncher {} on {} ({})\n\n{}", env!("CARGO_PKG_VERSION"), assets::os_now(), assets::arch_now(), message);
    let mut form: Vec<(String, String)> = vec![
        ("app".into(), game.as_ref().map(|g| g.id.clone()).unwrap_or_else(|| "launcher".into())),
        ("version".into(), game.as_ref().map(|g| g.tag.clone()).unwrap_or_else(|| env!("CARGO_PKG_VERSION").into())),
        ("category".into(), category.into()),
        ("message".into(), body.chars().take(2000).collect()),
        ("feedback".into(), match mood.as_deref() { Some("great") | Some("good") => "works", Some("broken") => "crashes", _ => "other" }.into()),
        ("build".into(), format!("RecompLauncher {}", env!("CARGO_PKG_VERSION"))),
    ];
    if attach {
        let dir = st.dir.clone();
        let pc = tokio::task::spawn_blocking(move || usage::pc_info(&dir)).await.unwrap_or_default();
        let mut txt = format!(
            "RecompLauncher {}\nSystem: {} ({})\nWindows/OS: {}\nCPU: {} ({} threads)\nRAM: {} GB\nGPU: {} {}\nSteam Deck: {}\n\nInstalled games:\n",
            env!("CARGO_PKG_VERSION"), assets::os_now(), assets::arch_now(), pc.osver, pc.cpu, pc.cores, pc.ram_gb, pc.gpu, pc.gpu_driver, shortcuts::is_steam_deck()
        );
        {
            let lib = st.lib.lock().unwrap();
            for (id, g) in lib.installed.iter().take(80) {
                txt.push_str(&format!("- {id} {}{}\n", g.tag, if g.local { " (local build)" } else { "" }));
            }
            if lib.installed.is_empty() {
                txt.push_str("(none)\n");
            }
            let s = &lib.settings;
            txt.push_str(&format!("\nSettings: big picture at start {}, updates check {}, backups before updates {}, keep downloads {}\n", s.start_in_tv, s.check_updates, s.backup_before_update, s.keep_downloads));
        }
        if let Ok(h) = std::fs::read_to_string(st.dir.join("downloads.json")) {
            let tail: String = h.chars().rev().take(6000).collect::<Vec<_>>().into_iter().rev().collect();
            txt.push_str("\nRecent downloads (end of downloads.json):\n");
            txt.push_str(&tail);
        }
        form.push(("name[]".into(), "launcher-and-pc.txt".into()));
        form.push(("text[]".into(), txt));
        if let Some(t) = trail::tail(24_000) {
            form.push(("name[]".into(), "launcher-trail.txt".into()));
            form.push(("text[]".into(), t));
        }
    }
    if let Some(g) = &game {
        if let Some(run) = last_run(&st.dir, &g.id) {
            form.push(("name[]".into(), "launcher-details.txt".into()));
            form.push(("text[]".into(), run));
        }
        if let Some((path, tail)) = recent_log_of(&st, &g.id, 0) {
            let name = Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "log.txt".into());
            form.push(("name[]".into(), name));
            form.push(("text[]".into(), tail));
        }
    }
    let r = st.http.post(format!("{}/api/logs.php", catalog::base())).form(&form).timeout(std::time::Duration::from_secs(30)).send().await?;
    let v: Value = r.json().await.unwrap_or(Value::Null);
    if v.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        Ok("Sent. Thanks for testing.".into())
    } else {
        Err(Error::Msg(Some(str_of(&v, "error")).filter(|s| !s.is_empty()).unwrap_or("It couldn't be sent right now. Try again in a bit.").to_string()))
    }
}

/// Download the newer launcher, check it, and start it: the installer on Windows, the AppImage in
/// place of this one on Linux. Anything else opens the download page.
#[tauri::command]
async fn update_launcher(app: AppHandle, st: State<'_, App>) -> Res<()> {
    let url = format!("{}/launcher/version.json", catalog::base());
    let v: Value = st.http.get(url).timeout(std::time::Duration::from_secs(15)).send().await?.json().await?;
    let page = format!("{}/download", catalog::base());
    // On Linux an AppImage replaces itself; anything else (a .deb) opens the download page.
    let appimage = if cfg!(target_os = "linux") { std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|p| p.is_file()) } else { None };
    let v = if cfg!(windows) { v } else { linux_entry(&v).unwrap_or(Value::Null) };
    let file_url = str_of(&v, "url").to_string();
    let ok_host = file_url.starts_with("https://github.com/") || file_url.starts_with("https://recomplauncher.com/") || file_url.starts_with(&format!("{}/", catalog::base().trim_end_matches('/')));
    if !(cfg!(windows) || appimage.is_some()) || file_url.is_empty() || !ok_host {
        return open_thing(&app, &page, true);
    }
    let mut r = st.http.get(&file_url).timeout(std::time::Duration::from_secs(900)).send().await?;
    if !r.status().is_success() {
        return Err(Error::Msg(format!("The download answered {}", r.status().as_u16())));
    }
    // The download, with how far along it is for the window.
    let total = r.content_length().unwrap_or(0);
    let mut bytes: Vec<u8> = Vec::with_capacity(total as usize);
    let mut last = std::time::Instant::now();
    while let Some(chunk) = r.chunk().await? {
        bytes.extend_from_slice(&chunk);
        if last.elapsed().as_millis() > 200 {
            last = std::time::Instant::now();
            let _ = app.emit("self-update", json!({"done": bytes.len(), "total": total}));
        }
    }
    let _ = app.emit("self-update", json!({"done": bytes.len(), "total": total, "installing": true}));
    let want = str_of(&v, "sha256").to_lowercase();
    if !want.is_empty() {
        use sha2::{Digest, Sha256};
        if hex::encode(Sha256::digest(&bytes)) != want {
            return Err(Error::Msg("The new version didn't download right (its checksum doesn't match). Try again.".into()));
        }
    }
    if let Some(me) = appimage {
        // The new AppImage goes next to this one and takes its place, then starts.
        let new = me.with_extension("new");
        std::fs::write(&new, &bytes).map_err(|e| Error::Msg(format!("Couldn't save the new version next to this one: {e}")))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755))?;
        }
        std::fs::rename(&new, &me).map_err(|e| {
            let _ = std::fs::remove_file(&new);
            Error::Msg(format!("Couldn't replace RecompLauncher with the new version: {e}"))
        })?;
        let mut c = std::process::Command::new(&me);
        sysenv::clean_std(&mut c);
        c.spawn().map_err(|e| Error::Msg(format!("The new version is in place, but it didn't start: {e}. Open RecompLauncher again.")))?;
        app.exit(0);
        return Ok(());
    }
    let file = std::env::temp_dir().join(format!("RecompLauncher-{}-setup.exe", str_of(&v, "version").trim_start_matches('v')));
    std::fs::write(&file, &bytes)?;
    // Its own small window shows the progress (/P), and RecompLauncher opens again after (/R).
    // In that mode the installer closes any copy of the launcher still open before it copies.
    // This one closes right away, so none of our files are in use. /UPDATE: it replaces the files
    // in place and leaves the player's shortcuts alone (a desktop shortcut they deleted stays gone).
    let mut flags: Vec<String> = vec!["/P".into(), "/UPDATE".into(), "/R".into()];
    // A portable launcher updates itself where it is, not where an installed one would go.
    if let (Some(_), Some(d)) = (portable_dir(), exe_dir()) {
        flags.push(format!("/D={}", d.display()));
    }
    std::process::Command::new(&file)
        .args(&flags)
        .spawn()
        .map_err(|e| if is_blocked(&e) { defender_msg(e, "the installer") } else { Error::Msg(format!("Couldn't start the installer: {e}")) })?;
    app.exit(0);
    Ok(())
}

// ---------- the launcher ----------

#[tauri::command(async)]
fn get_state(st: State<'_, App>) -> Value {
    let l = st.lib.lock().unwrap();
    let moving = st.moving.lock().unwrap().clone();
    // Games whose folder isn't there right now stay in the Library, marked missing.
    let missing: Vec<String> = l.missing().into_iter().filter(|id| !moving.contains(id)).collect();
    let running: Vec<String> = st.running.lock().unwrap().keys().cloned().collect();
    let jobs: Vec<String> = st.jobs.lock().unwrap().keys().cloned().collect();
    json!({
        "os": assets::os_now(),
        "arch": assets::arch_now(),
        "version": env!("CARGO_PKG_VERSION"),
        "site": catalog::base(),
        "library": l.installed,
        "settings": l.settings,
        "roms": l.roms,
        "running": running,
        "jobs": jobs,
        "steam": shortcuts::steam_dir().is_some(),
        "pictures": ingame::shots_dir("").parent().map(|p| p.to_string_lossy().to_string()),
        "deck": shortcuts::is_steam_deck(),
        "bigArg": std::env::args().any(|a| a == "--bigpicture"),
        "missing": missing,
        // Run as an AppImage: its bundled GStreamer libraries don't match the PC's own plugins, and
        // the web view hangs the moment it opens a sound. The page then plays none.
        "appimage": cfg!(target_os = "linux") && std::env::var_os("APPIMAGE").is_some(),
    })
}

#[tauri::command(async)]
fn save_settings(st: State<'_, App>, settings: Settings) -> Res<()> {
    if settings.library_dir.as_os_str().is_empty() || !settings.library_dir.is_absolute() {
        return Err(Error::Msg("Pick a folder for the library.".into()));
    }
    if settings.library_dirs.iter().chain(settings.rom_dirs.iter()).any(|d| !d.is_absolute()) {
        return Err(Error::Msg("Pick folders, not names.".into()));
    }
    let mut l = st.lib.lock().unwrap();
    let told = l.settings.tray_told;
    l.settings = settings;
    l.settings.tray_told |= told;
    l.save(&st.dir)?;
    Ok(())
}

/// Settings back to how they start ("settings"), or first-time setup again ("setup"). Games, their
/// saves, the library folders and pinned games all stay.
#[tauri::command(async)]
fn reset_settings(st: State<'_, App>, what: String) -> Res<Settings> {
    let mut l = st.lib.lock().unwrap();
    let old = l.settings.clone();
    match what.as_str() {
        "settings" => {
            let ui = &old.ui;
            let mut keep = serde_json::Map::new();
            for k in ["pins", "lang"] {
                if let Some(v) = ui.get(k) {
                    keep.insert(k.into(), v.clone());
                }
            }
            l.settings = Settings {
                library_dir: old.library_dir.clone(),
                library_dirs: old.library_dirs.clone(),
                rom_dirs: old.rom_dirs.clone(),
                asked_location: old.asked_location,
                setup_done: old.setup_done,
                tray_told: old.tray_told,
                menu_tips: old.menu_tips,
                share_usage: old.share_usage,
                share_stats: old.share_stats,
                data_asked: old.data_asked,
                ui: Value::Object(keep),
                ..Settings::default()
            };
        }
        "setup" => l.settings.setup_done = false,
        _ => return Err(Error::Msg("Nothing to reset.".into())),
    }
    l.save(&st.dir)?;
    Ok(l.settings.clone())
}

/// Asks WebView2 to give memory back while a window is out of sight, and to take it again when
/// it's back.
pub fn rest(w: &tauri::WebviewWindow, low: bool) {
    #[cfg(windows)]
    {
        let _ = w.with_webview(move |wv| unsafe {
            use webview2_com::Microsoft::Web::WebView2::Win32::{
                ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
            };
            use windows_core::Interface;
            if let Ok(c) = wv.controller().CoreWebView2() {
                if let Ok(c) = c.cast::<ICoreWebView2_19>() {
                    let _ = c.SetMemoryUsageTargetLevel(if low { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW } else { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL });
                }
            }
        });
    }
    #[cfg(not(windows))]
    let _ = (w, low);
}

/// The main window is out of sight (minimized, or closed to the tray).
static AWAY: AtomicBool = AtomicBool::new(false);
// Whether the last Play came from Big Picture mode, for RECOMPLAUNCHER_MODE.
static BIG_PICTURE: AtomicBool = AtomicBool::new(false);
/// This PC is a handheld (worked out once at start).
pub static HANDHELD: AtomicBool = AtomicBool::new(false);

/// The player's own cover for a game: a copy of their picture in the launcher's covers folder (a
/// new name each time, so the window shows the new one), or None to go back to the usual cover.
/// A missing game found again: its folder is now `path` (moved, or on a drive with a new letter).
/// The folder has to hold the game's program, when the launcher knows which one it is.
#[tauri::command(async)]
fn locate_game(st: State<'_, App>, id: String, path: String) -> Res<Installed> {
    let to = PathBuf::from(&path);
    if !to.is_dir() {
        return Err(Error::Msg("Pick the game's folder.".into()));
    }
    let g = {
        let mut l = st.lib.lock().unwrap();
        let g = l.installed.get_mut(&id).ok_or_else(|| Error::Msg("It isn't in your Library.".into()))?;
        if let Some(exe) = g.exe.as_ref().filter(|e| !e.as_os_str().is_empty() && e.is_relative()) {
            if !to.join(exe).is_file() {
                return Err(Error::Msg(format!("That folder doesn't have {}. Pick the folder {} is in.", exe.display(), g.name)));
            }
        }
        g.path = to;
        g.clone()
    };
    st.save();
    Ok(g)
}

/// Takes a game out of the Library without touching any files (its folder is gone anyway).
#[tauri::command(async)]
fn forget_game(st: State<'_, App>, id: String) -> Res<()> {
    st.lib.lock().unwrap().installed.remove(&id);
    st.save();
    Ok(())
}

#[tauri::command(async)]
fn set_cover(st: State<'_, App>, id: String, path: Option<String>, hero: Option<bool>) -> Res<Installed> {
    let dir = st.dir.join("covers");
    let hero = hero.unwrap_or(false);
    let old = {
        let l = st.lib.lock().unwrap();
        let g = l.installed.get(&id).ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
        if hero { g.hero_file.clone() } else { g.cover_file.clone() }
    };
    let new = match path.filter(|p| !p.trim().is_empty()) {
        Some(p) => {
            let src = PathBuf::from(p);
            let ext = src.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if !["png", "jpg", "jpeg", "webp", "gif", "bmp"].contains(&ext.as_str()) {
                return Err(Error::Msg("Pick a picture: PNG, JPG, WebP, GIF or BMP.".into()));
            }
            if std::fs::metadata(&src).map(|m| m.len() > 30 * 1024 * 1024).unwrap_or(true) {
                return Err(Error::Msg("That picture is too big (30 MB at most).".into()));
            }
            std::fs::create_dir_all(&dir)?;
            let to = dir.join(format!("{}-{}{}.{ext}", library::folder_name(&id), if hero { "hero-" } else { "" }, now()));
            std::fs::copy(&src, &to)?;
            Some(to)
        }
        None => None,
    };
    if let Some(o) = old.filter(|o| o.starts_with(&dir)) {
        let _ = std::fs::remove_file(o);
    }
    let g = {
        let mut l = st.lib.lock().unwrap();
        let g = l.installed.get_mut(&id).ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
        if hero {
            g.hero_file = new;
        } else {
            g.cover_file = new;
        }
        g.clone()
    };
    st.save();
    Ok(g)
}

/// Skip the backup being made before an update (it's taking too long); the update goes on.
#[tauri::command]
fn skip_backup() {
    manage::SKIP_BACKUP.store(true, Ordering::Relaxed);
}

/// For the window: is this a handheld, so While Playing settings can show their defaults.
#[tauri::command]
fn is_handheld() -> bool {
    HANDHELD.load(Ordering::Relaxed)
}

fn away(w: &tauri::Window, now: bool) {
    if AWAY.swap(now, Ordering::Relaxed) == now {
        return;
    }
    if let Some(wv) = w.app_handle().get_webview_window(w.label()) {
        rest(&wv, now);
        let _ = wv.emit("away", now);
    }
}

/// Windows Security's App & browser control page, where Smart App Control is turned off.
#[tauri::command(async)]
fn open_app_control() -> Res<()> {
    #[cfg(windows)]
    {
        if tauri_plugin_opener::open_url("windowsdefender://appbrowser", None::<&str>).is_err() {
            let _ = tauri_plugin_opener::open_url("ms-settings:windowsdefender", None::<&str>);
        }
    }
    Ok(())
}

/// Opens a web address or a folder the system's way. On Linux the opener (xdg-open, and the browser
/// or file manager it starts) gets the PC's own environment, not the AppImage's, or it can fail
/// quietly and nothing opens.
pub fn open_thing(app: &AppHandle, target: &str, url: bool) -> Res<()> {
    #[cfg(target_os = "linux")]
    {
        let _ = app;
        // A web address goes through the desktop's own "open this link" service first: it opens
        // the player's default browser, a Flatpak one too, whatever the launcher's surroundings.
        if url {
            let mut g = std::process::Command::new("gdbus");
            g.args(["call", "--session", "--dest", "org.freedesktop.portal.Desktop", "--object-path", "/org/freedesktop/portal/desktop", "--method", "org.freedesktop.portal.OpenURI.OpenURI", "", target, "{}"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            sysenv::clean_std(&mut g);
            if g.status().map(|s| s.success()).unwrap_or(false) {
                return Ok(());
            }
        }
        let mut c = std::process::Command::new("xdg-open");
        c.arg(target).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        sysenv::clean_std(&mut c);
        let mut child = c.spawn().map_err(|e| Error::Msg(format!("Couldn't open it: {e}")))?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        use tauri_plugin_opener::OpenerExt;
        if url {
            app.opener().open_url(target, None::<&str>).map_err(|e| Error::Msg(e.to_string()))
        } else {
            app.opener().open_path(target, None::<&str>).map_err(|e| Error::Msg(e.to_string()))
        }
    }
}

#[tauri::command(async)]
fn open_url(app: AppHandle, url: String) -> Res<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(Error::Msg("Not a web address".into()));
    }
    open_thing(&app, &url, true)
}

/// A shortcut, Steam or the tray asked to play a recomp that still needs setting up: the launcher
/// opens on its Set Up window instead.
pub(crate) fn setup_first(app: &AppHandle, id: &str) -> bool {
    let st = app.state::<App>();
    let ui = st.lib.lock().unwrap().settings.ui.clone();
    if !catalog::needs_setup_first(&st.dir, id, &ui) {
        return false;
    }
    *SETUP_ASKED.lock().unwrap() = Some(id.to_string());
    show_main(app);
    let _ = app.emit("setup-required", id.to_string());
    true
}

/// The recomp a shortcut wanted to play while it still needed setting up, for a window that opened
/// after the news went out.
static SETUP_ASKED: Mutex<Option<String>> = Mutex::new(None);

#[tauri::command(async)]
fn take_setup_asked() -> Option<String> {
    SETUP_ASKED.lock().unwrap().take()
}

// ---------- the window's size and place, kept for next time ----------

static WINDOW_GEN: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize, Deserialize, Default)]
struct WindowPlace {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    maximized: bool,
}

/// Write the window's size and place a moment after it stops moving (or right away when closing).
fn keep_window_place(w: &tauri::Window, now: bool) {
    let gen = WINDOW_GEN.fetch_add(1, Ordering::Relaxed) + 1;
    let w = w.clone();
    let save = move || {
        if WINDOW_GEN.load(Ordering::Relaxed) != gen && !now {
            return;
        }
        if w.is_fullscreen().unwrap_or(false) || w.is_minimized().unwrap_or(false) || !w.is_visible().unwrap_or(false) {
            return;
        }
        let dir = w.app_handle().state::<App>().dir.clone();
        let file = dir.join("window.json");
        let maximized = w.is_maximized().unwrap_or(false);
        // Maximized: the size it goes back to stays the one kept before.
        let mut place: WindowPlace = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        place.maximized = maximized;
        if !maximized {
            if let (Ok(p), Ok(sz)) = (w.outer_position(), w.outer_size()) {
                place = WindowPlace { x: p.x, y: p.y, w: sz.width, h: sz.height, maximized: false };
            }
        }
        if place.w > 0 {
            let _ = std::fs::write(&file, serde_json::to_vec(&place).unwrap_or_default());
        }
    };
    if now {
        save();
    } else {
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(700));
            save();
        });
    }
}

/// Put the window back where it was last time, if that's still on a screen.
fn restore_window_place(w: &tauri::WebviewWindow, dir: &Path) {
    let Some(place) = std::fs::read(dir.join("window.json")).ok().and_then(|b| serde_json::from_slice::<WindowPlace>(&b).ok()) else { return };
    if place.w < 400 || place.h < 300 {
        return;
    }
    let on_screen = w.available_monitors().unwrap_or_default().iter().any(|m| {
        let (mp, ms) = (m.position(), m.size());
        // Its top middle (where the title bar is) on a screen, so it can always be grabbed.
        let (cx, cy) = (place.x + place.w as i32 / 2, place.y + 20);
        cx >= mp.x && cx < mp.x + ms.width as i32 && cy >= mp.y && cy < mp.y + ms.height as i32
    });
    if on_screen {
        let _ = w.set_size(tauri::PhysicalSize::new(place.w, place.h));
        let _ = w.set_position(tauri::PhysicalPosition::new(place.x, place.y));
    }
    if place.maximized {
        let _ = w.maximize();
    }
}

fn show_main(app: &AppHandle) {
    app.state::<App>().headless.store(false, Ordering::Relaxed);
    if let Some(w) = app.get_webview_window("main") {
        if w.is_minimized().unwrap_or(false) {
            let _ = w.unminimize();
        }
        let _ = w.show();
        let _ = w.set_focus();
        if AWAY.swap(false, Ordering::Relaxed) {
            rest(&w, false);
            let _ = w.emit("away", false);
        }
    }
}

fn play_arg(args: &[String]) -> Option<String> {
    let i = args.iter().position(|a| a == "--play")?;
    args.get(i + 1).cloned().filter(|s| !s.is_empty())
}

/// The launcher's data folder, before Tauri is up (the same place as `app_data_dir`), or the
/// portable one beside the launcher.
fn data_dir_early() -> Option<PathBuf> {
    portable_dir().or_else(|| dirs::data_dir().map(|d| d.join("com.recomplauncher.app")))
}

/// The folder RecompLauncher's program is in.
fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf))
}

/// Portable mode: a file named `portable` (or `portable.txt`) beside RecompLauncher.exe keeps
/// everything it remembers (library, settings, play time, save backups, covers) in a `data` folder
/// next to it, so it travels with it on a portable drive.
pub(crate) fn portable_dir() -> Option<PathBuf> {
    let d = exe_dir()?;
    (d.join("portable").is_file() || d.join("portable.txt").is_file()).then(|| d.join("data"))
}

/// The drive a path is on, like `E:` (empty when it has none).
fn drive_of(p: &Path) -> String {
    let s = p.to_string_lossy();
    let b = s.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        s[..2].to_ascii_uppercase()
    } else {
        String::new()
    }
}

/// A portable launcher started from a drive with another letter than last time (E: at home, F:
/// at a friend's): every path in its library on that drive moves to the new letter.
fn portable_remap(dir: &Path) {
    let Some(now) = exe_dir().map(|d| drive_of(&d)).filter(|d| !d.is_empty()) else { return };
    let mark = dir.join("portable-drive.txt");
    let was = std::fs::read_to_string(&mark).map(|s| s.trim().to_ascii_uppercase()).unwrap_or_default();
    if was.len() == 2 && was != now {
        let file = dir.join("library.json");
        if let Some(mut v) = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
            fn walk(v: &mut Value, was: &str, now: &str) {
                match v {
                    Value::String(s) => {
                        if s.len() >= 2 && s[..2].eq_ignore_ascii_case(was) && (s.len() == 2 || matches!(s.as_bytes()[2], b'\\' | b'/')) {
                            *s = format!("{now}{}", &s[2..]);
                        }
                    }
                    Value::Array(a) => a.iter_mut().for_each(|x| walk(x, was, now)),
                    Value::Object(o) => o.values_mut().for_each(|x| walk(x, was, now)),
                    _ => {}
                }
            }
            walk(&mut v, &was, &now);
            if let Ok(b) = serde_json::to_vec_pretty(&v) {
                let _ = std::fs::write(&file, b);
            }
        }
    }
    let _ = std::fs::write(&mark, &now);
}

/// Whether this launcher is portable, and where it keeps its data, for Settings.
#[tauri::command]
fn portable_info(st: State<'_, App>) -> Value {
    json!({ "portable": portable_dir().is_some(), "data": st.dir.to_string_lossy() })
}

/// The portable copy made last, so its folder can be opened.
static PORTABLE_MADE: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Make a portable copy of the launcher in `target` (a folder on a portable drive, say): the
/// program, the `portable` file, and everything it remembers except what it can download again.
#[tauri::command(async)]
fn make_portable(st: State<'_, App>, target: String) -> Res<String> {
    let to = PathBuf::from(&target);
    if !to.is_dir() {
        return Err(Error::Msg("That folder isn't there.".into()));
    }
    let to = if std::fs::read_dir(&to).map(|mut r| r.next().is_none()).unwrap_or(false) { to } else { to.join("RecompLauncher") };
    std::fs::create_dir_all(&to)?;
    let exe = std::env::current_exe()?;
    let name = exe.file_name().map(|n| n.to_os_string()).unwrap_or_else(|| "RecompLauncher.exe".into());
    std::fs::copy(&exe, to.join(&name))?;
    std::fs::write(to.join("portable"), "RecompLauncher keeps its data in the data folder next to it while this file is here.\n")?;
    st.save();
    let data = to.join("data");
    fn copy(from: &Path, to: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(to)?;
        for e in std::fs::read_dir(from)?.flatten() {
            let name = e.file_name();
            if from.parent().is_some() && (name == "cache" || name == "EBWebView") {
                continue;
            }
            let (a, b) = (e.path(), to.join(&name));
            if e.file_type()?.is_dir() {
                copy(&a, &b)?;
            } else if !b.exists() {
                std::fs::copy(&a, &b)?;
            }
        }
        Ok(())
    }
    if st.dir != data {
        copy(&st.dir, &data)?;
    }
    let _ = std::fs::write(data.join("portable-drive.txt"), drive_of(&to));
    *PORTABLE_MADE.lock().unwrap() = Some(to.clone());
    Ok(to.to_string_lossy().into())
}

/// A game's "playing" marker: there while it runs, touched every 20 seconds.
fn playing_file(dir: &Path, id: &str) -> PathBuf {
    dir.join("playing").join(library::folder_name(id))
}

/// Started with `--play` (a Steam or desktop shortcut): this process stays open until the game
/// closes, so Steam shows the player in the game for the whole session. The work is handed to a
/// second copy (`--handoff`), which either becomes the launcher or passes the game to the one
/// already running; this one only watches the game's marker.
fn wait_for_play(args: &[String]) -> bool {
    let Some(id) = play_arg(args) else { return false };
    if args.iter().any(|a| a == "--handoff") {
        return false;
    }
    let Some(dir) = data_dir_early() else { return false };
    let Ok(exe) = std::env::current_exe() else { return false };
    let mut rest: Vec<String> = args.iter().skip(1).cloned().collect();
    rest.push("--handoff".into());
    if std::process::Command::new(exe).args(&rest).spawn().is_err() {
        return false;
    }
    let file = playing_file(&dir, &id);
    let fresh = |f: &Path| std::fs::metadata(f).ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).map(|e| e.as_secs() < 90).unwrap_or(false);
    // Setting a game up, a cloud save and a Look can come first: up to five minutes.
    let until = std::time::Instant::now() + std::time::Duration::from_secs(300);
    while !fresh(&file) {
        if std::time::Instant::now() > until {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    while fresh(&file) {
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
    true
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "linux")]
    sysenv::prepare_start();
    if wait_for_play(&std::env::args().collect::<Vec<_>>()) {
        return;
    }
    tauri::Builder::default()
        // A second launch (a shortcut, a Steam entry) hands its job to the launcher already running.
        // `--quit` (from the installer and uninstaller): the one running closes, wherever it is.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| match play_arg(&argv) {
            _ if argv.iter().any(|a| a == "--quit") => app.exit(0),
            Some(id) => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    if setup_first(&app, &id) {
                        return;
                    }
                    ready_look(&app, &id).await;
                    cloud::sync_quiet(&app, &id).await;
                    if let Err(e) = start_game(&app, &id) {
                        show_main(&app);
                        let _ = app.emit("toast", e.to_string());
                    }
                });
            }
            None => {
                show_main(app);
                if argv.iter().any(|a| a == "--bigpicture") {
                    let _ = app.emit("bigpicture", true);
                }
            }
        }))
        .on_window_event(|w, ev| {
            if w.label() != "main" {
                return;
            }
            match ev {
                // With the tray on, X hides the window and the launcher keeps going by the clock.
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    let app = w.app_handle();
                    let tray = app.state::<App>().lib.lock().unwrap().settings.close_to_tray;
                    if tray && app.tray_by_id("main").is_some() {
                        api.prevent_close();
                        let _ = w.hide();
                        away(w, true);
                        tray::hid(app);
                    } else {
                        // Closing for real: the whole launcher goes. The hidden quick-menu window
                        // would otherwise keep it running with no window, and opening it again
                        // would find nothing to show.
                        api.prevent_close();
                        app.exit(0);
                    }
                }
                tauri::WindowEvent::Focused(f) => {
                    pads::FOCUSED.store(*f, Ordering::Relaxed);
                    if *f {
                        away(w, false);
                    }
                }
                tauri::WindowEvent::Resized(_) => {
                    away(w, w.is_minimized().unwrap_or(false) || !w.is_visible().unwrap_or(true));
                    keep_window_place(w, false);
                }
                tauri::WindowEvent::Moved(_) => keep_window_place(w, false),
                _ => {}
            }
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        // Pictures through the disk cache: rlimg://<address>.
        .register_asynchronous_uri_scheme_protocol("rlimg", |ctx, req, responder| {
            let app = ctx.app_handle().clone();
            let raw = req.uri().path().trim_start_matches('/').to_string();
            tauri::async_runtime::spawn(async move {
                let st = app.state::<App>();
                let url = media::decode(&raw);
                let resp = match media::get(&st.http, &st.dir.join("cache/img"), &url).await {
                    Some(b) => tauri::http::Response::builder()
                        .header("Content-Type", media::mime(&b))
                        .header("Cache-Control", "max-age=31536000")
                        .header("Access-Control-Allow-Origin", "*")
                        .body(b),
                    None => tauri::http::Response::builder().status(404).body(Vec::new()),
                };
                responder.respond(resp.unwrap_or_else(|_| tauri::http::Response::new(Vec::new())));
            });
        })
        // Plugins' pages and files: rlplug://localhost/<plugin>/<file> (see plugins.rs).
        .register_uri_scheme_protocol("rlplug", |ctx, req| {
            let st = ctx.app_handle().state::<App>();
            plugins::serve(&st.dir, req.uri().path())
        })
        .setup(|app| {
            // Asked to close, and none was running: nothing to do.
            if std::env::args().any(|a| a == "--quit") {
                std::process::exit(0);
            }
            let portable = portable_dir();
            let dir = match &portable {
                Some(d) => d.clone(),
                None => app.path().app_data_dir()?,
            };
            std::fs::create_dir_all(&dir)?;
            if portable.is_some() {
                portable_remap(&dir);
            }
            trail::start(&dir);
            let http = reqwest::Client::builder()
                .user_agent(concat!("RecompLauncher/", env!("CARGO_PKG_VERSION")))
                .connect_timeout(std::time::Duration::from_secs(15))
                .build()?;
            #[allow(unused_mut)]
            let mut lib = Library::load(&dir);
            // On Linux, Windows-only recomps play through Proton once Proton is found: turned on
            // for the player the first time (they can turn it off in Settings → General).
            #[cfg(target_os = "linux")]
            if !lib.settings.proton_auto {
                lib.settings.proton_auto = true;
                if !lib.settings.proton && proton::runner().is_some() {
                    lib.settings.proton = true;
                    trail::note("Proton found: Windows recomps play through it");
                }
                let _ = lib.save(&dir);
            }
            // Big Picture at start only once setup is done: the first-run setup is always in a window.
            let tv = (lib.settings.start_in_tv || std::env::args().any(|a| a == "--bigpicture")) && lib.settings.setup_done;
            // ReShade left in a game's folder by a start that didn't finish (the launcher closed
            // while the game ran): taken away now.
            {
                let (d, ids): (PathBuf, Vec<String>) = (dir.clone(), lib.installed.keys().cloned().collect());
                std::thread::spawn(move || {
                    ids.iter().for_each(|id| looks::tidy(&d, id));
                    // Mod downloads left from last time (a version that was never picked).
                    let _ = std::fs::remove_dir_all(d.join("cache/gamebanana"));
                });
            }
            let play = play_arg(&std::env::args().collect::<Vec<_>>());
            app.manage(App {
                dir,
                http,
                lib: Mutex::new(lib),
                jobs: Mutex::default(),
                running: Mutex::default(),
                pids: Mutex::default(),
                started: Mutex::default(),
                download_slot: Arc::new(tokio::sync::Semaphore::new(1)),
                moving: Mutex::default(),
                overlay_open: AtomicBool::new(false),
                looked: Mutex::new(HashSet::new()),
                headless: AtomicBool::new(play.is_some()),
            });
            // The games running keep their "playing" markers fresh (see wait_for_play); old ones
            // from a launcher that didn't close cleanly are cleared.
            {
                let a = app.handle().clone();
                let _ = std::fs::remove_dir_all(app.state::<App>().dir.join("playing"));
                std::thread::spawn(move || loop {
                    std::thread::sleep(std::time::Duration::from_secs(20));
                    let st = a.state::<App>();
                    let ids: Vec<String> = st.running.lock().unwrap().keys().cloned().collect();
                    for id in ids {
                        let f = playing_file(&st.dir, &id);
                        if let Ok(fh) = std::fs::OpenOptions::new().append(true).open(&f) {
                            let _ = fh.set_modified(std::time::SystemTime::now());
                        }
                    }
                });
            }
            // Is this a handheld (ROG Ally, Legion Go, Steam Deck...)? Some settings default from it.
            {
                let d = app.state::<App>().dir.clone();
                std::thread::spawn(move || {
                    let pc = usage::pc_info(&d);
                    HANDHELD.store(shortcuts::is_handheld(&pc.cpu), Ordering::Relaxed);
                });
            }
            // The quick menu: a see-through window over the game, hidden until it's asked for.
            tauri::WebviewWindowBuilder::new(app, "overlay", tauri::WebviewUrl::App("overlay.html".into()))
                .title("RecompLauncher quick menu")
                .transparent(true)
                .decorations(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(false)
                .shadow(false)
                .visible(false)
                .focused(false)
                .build()?;
            if let Some(o) = app.get_webview_window("overlay") {
                rest(&o, true);
            }
            pads::start(app.handle().clone());
            if let Err(e) = tray::start(app.handle()) {
                eprintln!("No tray icon: {e}");
            }
            // Games sent from the website, while signed in.
            cloud::watch_inbox(app.handle().clone());
            // A new RecompLauncher: looked for every five minutes while it runs (it may stay by the
            // clock), so a new build shows up soon after it's published.
            {
                let h = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let mut told = String::new();
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs(300)).await;
                        let st = h.state::<App>();
                        if let Ok(Some(v)) = launcher_update(st).await {
                            let ver = str_of(&v, "version").to_string();
                            if ver != told {
                                told = ver.clone();
                                let hidden = h.get_webview_window("main").map(|w| !w.is_visible().unwrap_or(true)).unwrap_or(true);
                                if hidden {
                                    ingame::notify(&h, &format!("RecompLauncher {ver} is out"), "Open RecompLauncher to update.");
                                }
                                let _ = h.emit("launcher-update", v);
                            }
                        }
                    }
                });
            }
            // A new build or a recomp's release, pushed by the website the moment it's out.
            push::start(app.handle().clone());
            // Save copies that didn't reach the cloud last time.
            {
                let h = app.handle().clone();
                tauri::async_runtime::spawn(async move { cloud::upload_pending(&h).await });
            }
            // Screenshots show in the window straight from their folder, wherever Pictures is.
            if let Some(root) = ingame::shots_dir("x").parent() {
                let _ = std::fs::create_dir_all(root);
                let _ = app.asset_protocol_scope().allow_directory(root, true);
            }
            let handle = app.handle().clone();
            match play {
                Some(id) => {
                    tauri::async_runtime::spawn(async move {
                        if setup_first(&handle, &id) {
                            return;
                        }
                        ready_look(&handle, &id).await;
                        cloud::sync_quiet(&handle, &id).await;
                        if let Err(e) = start_game(&handle, &id) {
                            show_main(&handle);
                            let _ = handle.emit("toast", e.to_string());
                        }
                    });
                }
                None => {
                    if let Some(w) = app.get_webview_window("main") {
                        restore_window_place(&w, &app.state::<App>().dir);
                        if tv {
                            let _ = w.set_fullscreen(true);
                        }
                        let _ = w.show();
                    }
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            add_github,
            github_lookup,
            github_art,
            update_mine,
            release_builds,
            troubleshoot,
            unblock_game,
            clean_start_next,
            look_reset_route,
            manual_defaults,
            set_manual,
            plugins::plugin_inspect,
            plugins::plugin_install,
            plugins::plugin_list,
            plugins::plugin_enable,
            plugins::plugin_all_off,
            plugins::plugin_uninstall,
            plugins::plugin_allow_site,
            plugins::plugin_forget_site,
            plugins::plugin_fetch,
            plugins::plugin_store_get,
            plugins::plugin_store_set,
            plugins::plugin_library,
            plugins::plugin_add_folder,
            plugins::plugin_install_zip,
            plugins::plugin_game_files,
            locate_game,
            forget_game,
            remove_github,
            take_setup_asked,
            get_state, get_catalog, get_releases, check_updates, launcher_update, install, cancel, uninstall, play, stop, set_exe, open_folder,
            save_settings, open_url, libraries, move_game, game_sizes, backups, backup_now, restore_backup, pc_check, scan_roms, rom_matches,
            desktop_shortcut, open_app_control, fs_paste, portable_info, make_portable, add_to_steam, launcher_to_steam, recent_log, screenshots, open_shots, overlay_action, is_handheld, skip_backup, set_cover, all_screenshots, delete_screenshot,
            copy_screenshot, show_screenshot, controllers, prefetch_images, versions, build_check, news, game_info, mods_state, mods_online,
            mod_install, mod_remove, open_mods, send_report, beta_feedback, game_programs, run_program, game_root, boot::boot_builtin, update_launcher, resume_game, game_setup, set_game_copy, set_update_copy, set_game_setting,
            game_action, setup_copy, setup_cancel, easy_setup, set_game_options, looks_info, set_look, look_picture, ui_data,
            reset_settings, tray::tray_words, cloud::account_start, cloud::account_poll, cloud::account_cancel, cloud::account_state, cloud::account_signout,
            cloud::saves_check, cloud::saves_pick, cloud::saves_cloud, download_history, report_counts, play_reports, cloud::saves_info, cloud::saves_overview, cloud::saves_backup, cloud::saves_restore, cloud::saves_forget_found, cloud::saves_set_place, cloud::saves_open,
            adopt::adopt_scan, adopt::adopt_pick, adopt::adopt_add, usage::usage_hello, usage::usage_event, usage::data_ids, usage::reset_ids, ping::usage_open, ping::usage_mode, usage::run_report, cloud::inbox, cloud::inbox_done, cloud::betas, cloud::beta_join, cloud::beta_leave, launcher_notes, devtools::dev_add, devtools::add_any, devtools::dev_check, devtools::dev_starter, fs_places, fs_list, fs_extract, gb_mods, gb_mod, gb_install
        ])
        .build(tauri::generate_context!())
        .expect("RecompLauncher couldn't start")
        .run(|app, ev| {
            if let tauri::RunEvent::Exit = ev {
                ping::close(app);
            }
        });
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_game_saying_its_files_are_wrong_is_heard() {
        assert!(wrong_copy_line("=== Super Mario Strikers: wrong game data ==="));
        assert!(wrong_copy_line("That folder is not the Super Mario Strikers disc."));
        assert!(wrong_copy_line("The path is probably pointing one level too high or too low"));
        assert!(!wrong_copy_line("Loading the disc..."));
        assert!(!wrong_copy_line("Region: USA"));
    }

    #[test]
    fn hand_written_command_lines_keep_paths_whole() {
        let c = std::path::Path::new("/home/me/My Games/disc.iso");
        assert_eq!(super::fill_copy_args("--game {copy}", c), vec!["--game", "/home/me/My Games/disc.iso"]);
        assert_eq!(super::fill_copy_args("\"--data={copy}\" -fullscreen", c), vec!["--data=/home/me/My Games/disc.iso", "-fullscreen"]);
        assert_eq!(super::fill_copy_args("{copy}", c), vec!["/home/me/My Games/disc.iso"]);
    }

    #[test]
    fn paths_typed_without_quotes() {
        let d = std::env::temp_dir().join(format!("rl args {}", std::process::id()));
        std::fs::create_dir_all(d.join("CTR Nitro Fueled")).unwrap();
        let p = d.join("CTR Nitro Fueled").to_string_lossy().to_string();
        let typed = format!("--fullscreen {p} --x");
        assert_eq!(super::join_paths(super::split_args(&typed)), vec!["--fullscreen".to_string(), p.clone(), "--x".to_string()]);
        let typed = format!("--game={p}");
        assert_eq!(super::join_paths(super::split_args(&typed)), vec![format!("--game={p}")]);
        let _ = std::fs::remove_dir_all(&d);
    }

    use super::*;

    #[test]
    fn followed_developers_news_once_per_version() {
        let mut l = Library::default();
        l.settings.ui = json!({"follow": ["shagg88"]});
        let cat = |v: &str, extra: bool| {
            let mut apps = vec![json!({"id": "cogbox", "name": "COGBox", "dev": "shagg88", "version": v}), json!({"id": "other", "name": "Other", "dev": "someone", "version": "1"})];
            if extra {
                apps.push(json!({"id": "athelas", "name": "Athelas", "dev": "shagg88", "version": "0.1"}));
            }
            json!({"apps": apps, "developers": [{"slug": "shagg88", "name": "Shagg88", "verified": true}]})
        };
        assert!(followed_news(&cat("0.2.2", false), &mut l, &[], false).is_empty());
        assert!(followed_news(&cat("0.2.2", false), &mut l, &[], false).is_empty());
        let n = followed_news(&cat("0.2.3", false), &mut l, &[], false);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].0, "New from Shagg88");
        assert!(n[0].1.contains("0.2.3"));
        let n = followed_news(&cat("0.2.3", true), &mut l, &["athelas".to_string()], false);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].1, "Athelas is out.");
    }

    #[test]
    fn betas_come_before_their_release() {
        assert!(version_key("0.5.0-beta.2") > version_key("0.5.0-beta.1"));
        assert!(version_key("0.5.0") > version_key("0.5.0-beta.9"));
        assert!(version_key("0.5.1-beta.1") > version_key("0.5.0"));
        assert!(version_key("v0.5.0-beta.10") > version_key("0.5.0-beta.9"));
        assert_eq!(version_key("0.5.0-beta.1"), version_key("0.5.0-beta.1"));
        assert!(version_key("0.75.0-beta.1") > version_key("0.9.3"));
        assert!(version_key("0.75.0-beta.1") > version_key("0.5.0-beta.1"));
        // 0.90 comes after 0.75 (and after the 0.9 that went out first); 0.9 alone would not.
        assert!(version_key("0.90.0-beta.1") > version_key("0.75.0-beta.23"));
        assert!(version_key("0.90.0-beta.1") > version_key("0.9.0-beta.1"));
        assert!(version_key("0.9.0-beta.1") < version_key("0.75.0-beta.23"));
        assert!(version_key("0.90.0-beta.3.1") > version_key("0.90.0-beta.3"));
        assert!(version_key("0.90.0-beta.4") > version_key("0.90.0-beta.3.1"));
    }

    #[test]
    fn only_what_the_list_says_runs_here() {
        let linux = json!({"systems": ["linux"], "assets": {"linux": "bbport-*.tar.gz"}});
        assert!(!runs_here(&linux, "windows"));
        assert!(runs_here(&linux, "linux"));
        assert!(runs_here(&json!({"systems": [], "assets": {}}), "windows"));
        assert!(runs_here(&json!({"systems": ["linux"], "assets": {"windows": "x.zip"}}), "windows"));
        assert!(runs_here(&json!({"name": "no info"}), "macos"));
    }

    #[test]
    fn launch_options() {
        assert_eq!(split_args(r#"--fullscreen  --data "C:\My Games\x" -v"#), vec!["--fullscreen", "--data", r"C:\My Games\x", "-v"]);
        assert_eq!(split_args("  "), Vec::<String>::new());
        assert_eq!(split_args(r#"--name """#), vec!["--name", ""]);
    }

    #[test]
    fn copies_found_by_what_is_on_the_disc() {
        // A PS2 disc image with the serial file at its top, named nothing like the game.
        let dir = std::env::temp_dir().join(format!("rl-copies-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let iso = dir.join("backup-07.iso");
        let mut img = vec![0u8; 20 * 2048];
        img[16 * 2048] = 1;
        img[16 * 2048 + 1..16 * 2048 + 6].copy_from_slice(b"CD001");
        let root = 16 * 2048 + 156;
        img[root + 2..root + 6].copy_from_slice(&18u32.to_le_bytes());
        img[root + 10..root + 14].copy_from_slice(&2048u32.to_le_bytes());
        let rec = 18 * 2048;
        let name = b"SLUS_206.51;1";
        img[rec] = (33 + name.len()) as u8;
        img[rec + 32] = name.len() as u8;
        img[rec + 33..rec + 33 + name.len()].copy_from_slice(name);
        std::fs::write(&iso, &img).unwrap();
        let cat = json!({"apps": [{"id": "streetball", "name": "Streetball Revival", "game": "NBA Street Vol. 2", "repo": "luckvj/streetball-revival"}]});
        let found = vec![roms::RomFile { path: iso.clone(), system: "ps2".into(), title: "backup-07".into(), region: String::new(), size: img.len() as u64 }];
        let got = copies_for(&cat, &found, &HashMap::new());
        assert_eq!(got.get("streetball").map(|r| r.path.clone()), Some(iso));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

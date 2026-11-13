//! RecompLauncher: discover, install and play PC recomps, in one window.
//!
//! The window (ui/) asks for things through the commands at the bottom; long jobs (downloads,
//! games running) report back through events: `install` with a Progress, `game` when one starts
//! or closes.

pub mod ach;
pub mod adopt;
pub mod xex;
mod appimage;
#[cfg(target_os = "linux")]
mod appimage_update;
pub mod art;
pub mod assets;
pub mod boot;
pub mod catalog;
pub mod cloud;
pub mod devtools;
pub mod dllcheck;
pub mod discord;
pub mod dlss;
pub mod extract;
pub mod files;
pub mod gamefiles;
pub mod games;
pub mod flatpak;
pub mod gamebanana;
pub mod info;
pub mod inputs;
pub mod ingame;
pub mod install;
pub mod library;
pub mod looks;
pub mod mac_update;
pub mod manage;
pub mod macos;
pub mod media;
pub mod mods;
pub mod net;
pub mod nup;
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
mod emus;
pub mod roms;
pub mod saves;
pub mod shortcuts;
pub mod steam_sync;
pub mod trail;
pub mod tray;
mod update_download;
mod update_launch;
pub mod usage;
pub mod win;
mod installer;

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
        // Which site didn't answer, when it's known: one site being down isn't the player's internet.
        let site = e.url().and_then(|u| u.host_str()).map(|h| h.trim_start_matches("www.").to_string());
        if e.is_timeout() {
            match site {
                Some(h) => Error::Net(format!("{h} took too long to answer. Try again in a minute.")),
                None => Error::Net("A site took too long to answer. Try again in a minute.".into()),
            }
        } else if e.is_connect() && net::proxy_shown().is_some() {
            // Through a proxy that didn't answer: that's what to check, not the site.
            Error::Net(format!("Couldn't reach the proxy {}. Check it in Settings → Network, or turn the proxy off.", net::proxy_shown().unwrap_or_default()))
        } else if e.is_connect() {
            match site {
                Some(h) => Error::Net(format!("Couldn't reach {h}. Check your connection, or try again in a minute.")),
                None => Error::Net("Couldn't reach the internet. Check your connection and try again.".into()),
            }
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

/// The release the store checked and let out for a recomp (its "tag" in the list), if it says.
fn catalog_tag(entry: &Value) -> Option<String> {
    entry.get("tag").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty()).map(str::to_string)
}

/// The same release name, give or take a leading "v" and letter case.
fn same_tag(a: &str, b: &str) -> bool {
    let n = |s: &str| s.trim().trim_start_matches(['v', 'V']).trim_start_matches('.').to_ascii_lowercase();
    !a.trim().is_empty() && n(a) == n(b)
}

async fn do_install(app: &AppHandle, st: &App, id: &str, dir: Option<PathBuf>, tag: Option<String>, file: Option<String>, cancel: Arc<AtomicBool>) -> Res<()> {
    let picked_tag = tag.is_some();
    let report = |p: Progress| {
        let _ = app.emit("install", p);
    };
    report(Progress::new(id, "prepare", 0, 0));
    trail::note(format!("Install {id}: getting ready{}", tag.as_deref().map(|t| format!(" ({t})")).unwrap_or_default()));
    let cat = getting_ready(catalog::catalog(&st.http, &st.dir, false), &cancel).await?.data;
    let entry = catalog::app(&cat, id).ok_or_else(|| Error::Msg("This recomp isn't listed anymore.".into()))?.clone();
    let repo = str_of(&entry, "repo").to_string();
    trail::note(format!("Install {id}: list read, asking {repo} for its releases"));
    let rels: Vec<Release> = serde_json::from_value(getting_ready(catalog::releases_within(&st.http, &st.dir, &repo, 600), &cancel).await?.data)?;
    trail::note(format!("Install {id}: {} releases", rels.len()));
    let have = st.lib.lock().unwrap().installed.get(id).map(|g| g.tag.clone()).unwrap_or_default();
    let newest = catalog::latest_for(&rels, &have).ok_or_else(|| Error::Msg("This recomp hasn't put out a release yet.".into()))?;
    // A version the player picked (an older one, to go back to), or the newest. A newest release
    // with nothing for this system in it (only an updater's files, say): the newest one that has.
    let os0 = os_for(st, &entry, id);
    let pattern0 = entry.get("assets").and_then(|a| a.get(os0)).and_then(Value::as_str);
    // The list is the source of truth for Windows. A Linux or Mac build the list doesn't name yet
    // (the developer just added one): the release's own file names decide.
    let named_build = |r: &Release| {
        os0 != "windows"
            && !runs_here(&entry, os0)
            && assets::pick(&named_for(&r.assets, os0), os0, assets::arch_now(), None).is_some()
    };
    let has_build = |r: &Release| assets::pick(&r.assets, os0, assets::arch_now(), pattern0).is_some() || named_build(r);
    let mut fell_back = false;
    // The release the store checked and let out (the list's "tag"): that one, unless the player
    // picked a version. When it's gone from GitHub or has nothing for this system, the usual rules.
    let listed = catalog_tag(&entry).and_then(|t| rels.iter().find(|r| !r.draft && same_tag(&r.tag_name, &t))).filter(|r| has_build(r));
    if let (None, Some(r)) = (&tag, listed) {
        if r.tag_name != newest.tag_name {
            trail::note(format!("Install {id}: {} is the version the store let out", r.tag_name));
        }
    }
    let rel = match &tag {
        Some(t) => rels.iter().find(|r| &r.tag_name == t).ok_or_else(|| Error::Msg(format!("{t} isn't one of its releases.")))?,
        None if listed.is_some() => {
            fell_back = true;
            listed.unwrap()
        }
        None if !has_build(newest) => match rels.iter().filter(|r| !r.draft && (!r.prerelease || newest.prerelease)).find(|r| has_build(r)) {
            Some(r) => {
                trail::note(format!("Install {id}: {} has no build for this PC, so {} instead", newest.tag_name, r.tag_name));
                fell_back = true;
                r
            }
            None => newest,
        },
        None => newest,
    };
    let pinned = rel.tag_name != newest.tag_name && !fell_back;
    let (mut os, arch) = (os_for(st, &entry, id), assets::arch_now());
    // On Linux with Proton, a release with a Linux build and a Windows one: the player picks the
    // first time (the Linux one is the usual pick), and updates keep the system they picked. A
    // Windows file picked is played through Proton.
    let (lin, win) = linux_and_windows(st, &entry, rel, arch);
    if let Some(f) = file.as_deref() {
        if win.iter().any(|a| a.name == f) {
            os = "windows";
        } else if lin.iter().any(|a| a.name == f) {
            os = "linux";
        }
    }
    let first_time = !st.lib.lock().unwrap().installed.contains_key(id);
    if file.is_none() && first_time && !lin.is_empty() && !win.is_empty() {
        let best = assets::pick(&lin, "linux", arch, None).map(|a| a.name.clone()).unwrap_or_else(|| lin[0].name.clone());
        let mut names: Vec<String> = vec![format!("*{best}\t{}", assets::describe(&best, "linux"))];
        names.extend(lin.iter().filter(|a| a.name != best).map(|a| format!("{}\t{}", a.name, assets::describe(&a.name, "linux"))));
        names.extend(win.iter().map(|a| format!("{}\tWindows Build", a.name)));
        return Err(Error::Msg(format!("PICK_BUILD|{}", names.join("|"))));
    }
    let pattern = entry.get("assets").and_then(|a| a.get(os)).and_then(Value::as_str);
    let os_name = match os {
        "windows" => "Windows",
        "macos" => "Mac",
        _ => "Linux",
    };
    // The list says which systems a recomp is for: one that isn't for this one isn't installed, even
    // when its release has a file that looks like it could be. A build the list doesn't know about
    // yet (the developer just added one): the release itself says so, when its file names the
    // system. The list stays the source of truth for Windows; for Linux and macOS the release's own
    // file names decide, since listings often lag behind a new build.
    let named = named_for(&rel.assets, os);
    let os_new = os != "windows" && !runs_here(&entry, os) && assets::pick(&named, os, arch, None).is_some();
    if !runs_here(&entry, os) && !os_new {
        return Err(Error::Msg(format!("This recomp has no {os_name} version yet.")));
    }
    let mut asset = (if os_new { assets::pick(&named, os, arch, None) } else { assets::pick(&rel.assets, os, arch, pattern) })
        .ok_or_else(|| Error::Msg(format!("The latest release ({}) has no {os_name} version.", rel.tag_name)))?
        .clone();
    // Several builds that fit this PC equally (one per region, say), or in different forms (an
    // AppImage and a plain folder, a portable zip and an installer): the one the player picked, the
    // one most like what they had, or they're asked once, with the usual pick first.
    let choices = if os_new { assets::choices(&named, os, arch, None) } else { assets::choices(&rel.assets, os, arch, pattern) };
    let all = if os_new { assets::all_for(&named, os, arch, None) } else { assets::all_for(&rel.assets, os, arch, pattern) };
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

    // A recomp that only comes as an installer: Windows only, and never one the store holds.
    let as_installer = installer::is_installer(&entry) && assets::os_now() == "windows";
    if as_installer && installer::held(&entry) {
        return Err(Error::Msg("This release is on hold, so its installer won't be downloaded. Try again later.".into()));
    }
    // A SourceForge file is only ever installed once the site has checked it: its SHA-256 comes
    // with the release, and the download must match it.
    let sf = catalog::is_sourceforge(&repo);
    let checked = asset.digest.as_deref().and_then(|d| d.strip_prefix("sha256:")).map(|h| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit())).unwrap_or(false);
    if sf && !checked {
        trail::note(format!("Install {id}: {} has no checked SHA-256 yet, so it wasn't downloaded", asset.name));
        return Err(Error::Msg("This file hasn't been checked yet. Try again later.".into()));
    }
    // A GitLab project's files come only from that project on gitlab.com; a SourceForge one's
    // only from its project on sourceforge.net.
    if !catalog::own_download(&repo, &asset.browser_download_url, rel.project_id) {
        trail::note(format!("Install {id}: {} isn't one of its own project's files, so it wasn't downloaded", asset.name));
        return Err(Error::Msg("That file isn't in this recomp's own releases, so it wasn't downloaded.".into()));
    }
    // One download at a time; this one waits its turn.
    let permit = match st.download_slot.clone().try_acquire_owned() {
        Ok(p) => p,
        Err(_) => {
            report(Progress::new(id, "queued", 0, 0));
            // Waiting its turn: cancelling (or pausing) answers at once, not when its turn comes.
            let slot = st.download_slot.clone();
            let wait = slot.acquire_owned();
            tokio::pin!(wait);
            loop {
                tokio::select! {
                    p = &mut wait => break p.map_err(|e| Error::Msg(e.to_string()))?,
                    _ = tokio::time::sleep(std::time::Duration::from_millis(300)) => {
                        if cancel.load(Ordering::Relaxed) {
                            return Err(Error::Cancelled);
                        }
                    }
                }
            }
        }
    };
    let started = std::time::Instant::now();
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }
    // The release it ends up from: the one picked, or a newer one when that was taken down.
    let mut tag_got = rel.tag_name.clone();
    let got = match install::download(&st.http, &asset, &home.join(".downloads"), id, cancel.clone(), &report).await {
        // The file isn't there anymore (the release was changed after its list was read): the
        // release asked for straight from GitHub, and its file for this PC tried once more.
        Err(Error::Msg(m)) if sf && m.contains("checksum doesn't match") => {
            trail::note(format!("Install {id}: {} didn't match its checked SHA-256", asset.name));
            return Err(Error::Msg("The download didn't match the checked file, so it wasn't installed. Try again.".into()));
        }
        Err(Error::Msg(m)) if !sf && (m.contains("anymore (error 404)") || m.contains("anymore (error 410)")) => {
            trail::note(format!("Install {id}: {} is gone, asking GitHub for {} again", asset.name, rel.tag_name));
            // A developer swapping a release's files: give it a moment to finish.
            tokio::time::sleep(std::time::Duration::from_secs(4)).await;
            // GitHub's own list, or the site's freshest copy when GitHub won't answer (its hourly
            // limit for this PC, say).
            let fresh_from = match catalog::from_github(&st.http, &st.dir, &repo).await {
                Ok(f) => Some(f),
                Err(_) => catalog::releases(&st.http, &st.dir, &repo, true).await.ok().filter(|f| !f.offline),
            };
            let fresh: Vec<Release> = fresh_from.and_then(|f| serde_json::from_value(f.data).ok()).unwrap_or_default();
            // The release itself gone (taken down and put up again under a new name): the newest
            // one with a build for this PC.
            // Not for a version the player picked, and never the one they already have.
            let same = fresh.iter().find(|r| r.tag_name == rel.tag_name);
            let have_tag = st.lib.lock().unwrap().installed.get(id).map(|g| g.tag.clone()).unwrap_or_default();
            let newest_now = if same.is_none() && !pinned && !fresh.is_empty() {
                fresh.iter().filter(|r| !r.draft && (!r.prerelease || rel.prerelease)).find(|r| assets::pick(&r.assets, os, arch, pattern).is_some()).filter(|r| r.tag_name != have_tag)
            } else {
                None
            };
            if let Some(r) = newest_now {
                trail::note(format!("Install {id}: {} isn't there anymore, so {} instead", rel.tag_name, r.tag_name));
            }
            let again = same
                .or(newest_now)
                .inspect(|r| tag_got = r.tag_name.clone())
                .and_then(|r| {
                    let all = assets::all_for(&r.assets, os, arch, pattern);
                    all.iter().copied().find(|a| a.name == asset.name).or_else(|| assets::most_like(&all, &asset.name)).or_else(|| assets::pick(&r.assets, os, arch, pattern)).cloned()
                });
            match again {
                Some(a) => {
                    asset = a;
                    install::download(&st.http, &asset, &home.join(".downloads"), id, cancel.clone(), &report).await.map_err(|e| match e {
                        Error::Msg(m) if m.contains("anymore (error 404)") || m.contains("anymore (error 410)") => Error::Msg(format!("{m} Its developer took that file down or renamed it. Try again in a few minutes, or pick another version on the game's page.")),
                        e => e,
                    })?
                }
                None => {
                    trail::note(format!("Install {id}: {m}"));
                    return Err(Error::Msg(format!("{m} Its developer took that file down or renamed it. Try again in a few minutes, or pick another version on the game's page.")));
                }
            }
        }
        r => r?,
    };
    drop(permit);
    // A pause that came just as the download finished: there's nothing left to pause, so the
    // install carries on.
    if install::paused(id) {
        install::unpause(id);
        cancel.store(false, Ordering::Relaxed);
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }

    // An update: keep a copy of the player's saves and settings first.
    if let (Some(p), true) = (&prev, backup_first) {
        report(Progress::new(id, "backup", 0, 0));
        let (dir, files, bdir) = (p.path.clone(), inputs::release_files(p), backups_dir(p));
        let _ = tokio::task::spawn_blocking(move || manage::backup(&dir, &files, &bdir, 5)).await;
    }

    // Only an installer: it waits in the launcher's staging folder for Set Up, which runs it when
    // the player presses Run Installer. A new game gets its place in the Library (the folder Set
    // Up suggests); an update keeps everything until its installer has run.
    if as_installer {
        report(Progress::new(id, "unpack", 0, 0));
        let (file, sdir) = (got.file.clone(), installer::staging(&st.dir, id));
        let setup = tokio::task::spawn_blocking(move || installer::stage(&file, &sdir)).await.map_err(|e| Error::Msg(e.to_string()))??;
        let _ = std::fs::remove_file(&got.file);
        {
            let mut l = st.lib.lock().unwrap();
            match l.installed.get_mut(id) {
                Some(g) => {
                    g.installer_setup = setup.to_string_lossy().to_string();
                    g.installer_tag = tag_got.clone();
                    g.asset = asset.name.clone();
                }
                None => {
                    let rec = Installed {
                        id: id.into(),
                        name: str_of(&entry, "name").into(),
                        repo: repo.clone(),
                        version: str_of(&entry, "version").into(),
                        path: target.clone(),
                        asset: asset.name.clone(),
                        size: asset.size,
                        installed_at: now(),
                        updated_at: now(),
                        setup_checked: true,
                        exe_rules: u8::MAX,
                        look_amount: 50,
                        installer: true,
                        installer_setup: setup.to_string_lossy().to_string(),
                        installer_tag: tag_got.clone(),
                        ..Default::default()
                    };
                    l.put(id, rec);
                }
            }
            l.settings.asked_location = true;
        }
        st.save();
        trail::note(format!("Install {id}: its installer {} is ready for Set Up", setup.file_name().unwrap_or_default().to_string_lossy()));
        note_download(&st.dir, json!({"id": id, "name": str_of(&entry, "name"), "tag": tag_got, "from": prev.as_ref().map(|p| p.tag.clone()).unwrap_or_default(), "repo": str_of(&entry, "repo"), "size": asset.size, "secs": started.elapsed().as_secs(), "when": now(), "update": prev.is_some(), "verified": got.verified}));
        st.jobs.lock().unwrap().remove(id);
        report(Progress::new(id, "done", 1, 1));
        return Ok(());
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
    if let Some(p) = prev.as_ref().filter(|p| p.path == target) {
        let removed = install::remove_dropped(&target, &p.files, &files, p.updated_at, p.exe.as_deref());
        if !removed.is_empty() {
            trail::note(format!("Install {id}: {} files of {} that {tag_got} no longer has, removed", removed.len(), p.tag));
        }
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
    let names = [str_of(&entry, "name"), repo.rsplit(['/', ':']).next().unwrap_or(""), str_of(&entry, "game")];
    // One the launcher picked that's one of the game's tools (an older version's guess) isn't kept.
    let kept = prev.as_ref().map(|p| p.exe_rules == u8::MAX).unwrap_or(false);
    let mut exe = prev.as_ref().and_then(|p| p.exe.clone()).filter(|e| target.join(e).is_file() && (kept || !install::looks_like_tool(e, &names)));
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
    let exe = exe.or_else(|| install::find_game_exe(&target, os, &names));
    // The catalog's boot profile says which program starts it, unless the player picked one.
    let boot = boot::Boot::from_entry(&entry).map(|b| b.for_build(os == "windows"));
    let picked = prev.as_ref().map(|p| p.exe_rules == u8::MAX).unwrap_or(false);
    let exe = match boot.as_ref().and_then(|b| b.start_in(&target)) {
        Some(s) if !picked => Some(s),
        _ => exe,
    };
    // A release that's an installer for the game it builds (totk.exe): any other program in it is
    // one of its tools (a shader translator, say), never the game.
    let exe = match (boot.as_ref().and_then(|b| b.then_name()), exe) {
        (Some(w), Some(e)) if !picked && e.file_name().map(|n| !boot::name_fits(&w, &n.to_string_lossy())).unwrap_or(true) => None,
        (_, e) => e,
    };
    // A program that has to run once first: the release's installer, or a setup beside the game.
    // An update keeps a setup beside the game as done; a new installer runs again.
    let setup = match &boot {
        Some(b) if !b.is_installer() => b.setup_in(&target),
        Some(b) => b.setup_near(&target).or_else(|| install::find_setup(&target, os, exe.as_deref().filter(|e| !e.is_absolute()))),
        None => install::find_setup(&target, os, exe.as_deref().filter(|e| !e.is_absolute())),
    };
    // A release that's the game's installer (it builds the game, like TotK-Setup.exe): a new version
    // has to be built again, so its setup runs on the next Play even though the old build is there.
    let rebuild = boot.as_ref().map(|b| b.is_installer()).unwrap_or(false) && prev.as_ref().map(|p| p.tag != tag_got).unwrap_or(false);
    let setup_done = !rebuild && setup.is_some() && prev.as_ref().map(|p| p.setup_done && p.exe.as_ref().map(|e| !e.is_absolute()).unwrap_or(false)).unwrap_or(false);
    if rebuild && setup.is_some() {
        trail::note(format!("Install {id}: a new version of its installer, it runs again on the next Play"));
    }

    let rec = Installed {
        id: id.into(),
        name: str_of(&entry, "name").into(),
        game: prev.as_ref().map(|p| p.game.clone()).unwrap_or_default(),
        repo,
        tag: tag_got.clone(),
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
        // Each of its games' own time and setup (a recomp that carries several) stays through updates.
        games: prev.as_ref().map(|p| p.games.clone()).unwrap_or_default(),
        runtime_off: prev.as_ref().map(|p| p.runtime_off.clone()).unwrap_or_default(),
        game_copy: prev.as_ref().and_then(|p| p.game_copy.clone()),
        update_copy: prev.as_ref().and_then(|p| p.update_copy.clone()),
        setup_inputs: prev.as_ref().map(|p| p.setup_inputs.clone()).unwrap_or_default(),
        inputs_ready: String::new(),
        input_staged: prev.as_ref().map(|p| p.input_staged.clone()).unwrap_or_default(),
        input_stamps: prev.as_ref().map(|p| p.input_stamps.clone()).unwrap_or_default(),
        proton: os == "windows" && assets::os_now() == "linux",
        picks: prev.as_ref().map(|p| p.picks.clone()).unwrap_or_default(),
        screen_fit: prev.as_ref().map(|p| p.screen_fit.clone()).unwrap_or_default(),
        screen_size: prev.as_ref().map(|p| p.screen_size.clone()).unwrap_or_default(),
        slider_vals: prev.as_ref().map(|p| p.slider_vals.clone()).unwrap_or_default(),
        args: prev.as_ref().map(|p| p.args.clone()).unwrap_or_default(),
        auto_update: prev.as_ref().map(|p| p.auto_update).unwrap_or(false),
        local: false,
        look: prev.as_ref().map(|p| p.look.clone()).unwrap_or_default(),
        look_amount: prev.as_ref().map(|p| p.look_amount).unwrap_or(50),
        cloud: prev.as_ref().and_then(|p| p.cloud),
        found_saves: prev.as_ref().map(|p| p.found_saves.clone()).unwrap_or_default(),
        save_place: prev.as_ref().and_then(|p| p.save_place.clone()),
        more_save_places: prev.as_ref().map(|p| p.more_save_places.clone()).unwrap_or_default(),
        cover_file: prev.as_ref().and_then(|p| p.cover_file.clone()),
        hero_file: prev.as_ref().and_then(|p| p.hero_file.clone()),
        logo_file: prev.as_ref().and_then(|p| p.logo_file.clone()),
        icon_file: prev.as_ref().and_then(|p| p.icon_file.clone()),
        art_edit: prev.as_ref().map(|p| p.art_edit.clone()).unwrap_or_default(),
        setup,
        setup_done,
        setup_checked: true,
        exe_rules: if prev.as_ref().map(|p| p.exe_rules == u8::MAX).unwrap_or(false) { u8::MAX } else { install::EXE_RULES },
        manual: prev.as_ref().and_then(|p| p.manual.clone()),
        // An update remembers the version it came from; installing the same version again keeps
        // what was remembered before.
        // The version it came from, for "go back" after a bad update. A version the player picked
        // (going back, or an older one on purpose) isn't an update, so there's nothing to go back to.
        prev_tag: if picked_tag { String::new() } else { prev.as_ref().map(|p| if !p.tag.is_empty() && p.tag != tag_got { p.tag.clone() } else { p.prev_tag.clone() }).unwrap_or_default() },
        installer: false,
        installer_setup: String::new(),
        installer_tag: String::new(),
        boot_hash: prev.as_ref().map(|p| p.boot_hash.clone()).unwrap_or_default(),
    };
    {
        let mut l = st.lib.lock().unwrap();
        l.put(id, rec);
        l.settings.asked_location = true;
    }
    st.save();
    note_download(
        &st.dir,
        json!({"id": id, "name": str_of(&entry, "name"), "tag": tag_got, "from": prev.as_ref().map(|p| p.tag.clone()).unwrap_or_default(), "repo": str_of(&entry, "repo"), "size": asset.size, "secs": started.elapsed().as_secs(), "when": now(), "update": prev.is_some(), "verified": got.verified}),
    );
    let mut p = Progress::new(id, "done", 1, 1);
    let gh = !catalog::is_gitlab(str_of(&entry, "repo")) && !catalog::is_sourceforge(str_of(&entry, "repo"));
    p.msg = Some(match (got.verified, gh) {
        (true, true) => "Checked against GitHub's checksum".into(),
        (true, false) => "Checked against its release's checksum".into(),
        (false, true) => "GitHub has no checksum for this file".into(),
        (false, false) => "Its release had no checksum for this file".into(),
    });
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
    // Installed: the system of the build the player has. A Linux build stays Linux for updates and
    // the versions list, whatever the list says the recomp is for (it may not know about the
    // Linux build yet).
    if let Some(g) = l.installed.get(id) {
        if g.proton {
            return "windows";
        }
        if looks_linux(&g.asset) {
            return "linux";
        }
    }
    if l.settings.proton && !runs_here(entry, "linux") && runs_here(entry, "windows") {
        "windows"
    } else {
        here
    }
}

/// Whether the installed build is the Windows one (on Windows, or through Proton). A native build
/// prefers a native program with the Windows one's name, even when an old `.exe` is still there.
fn is_windows_build(g: &Installed) -> bool {
    crate::boot::windows_build(g.proton)
}

/// A release file that says it's for Linux (its name, or an AppImage).
fn looks_linux(asset: &str) -> bool {
    let n = asset.to_lowercase();
    !n.is_empty() && (n.contains("linux") || n.ends_with(".appimage") || n.ends_with(".flatpak"))
}

/// A folder in `dir` (or `dir` itself), up to `depth` levels down, that has every one of `want`
/// (lowercase names) right in it.
fn folder_with(dir: &Path, want: &[String], depth: usize) -> Option<PathBuf> {
    if want.is_empty() || !dir.is_dir() {
        return None;
    }
    let mut level = vec![dir.to_path_buf()];
    for _ in 0..=depth {
        let mut next = vec![];
        for d in &level {
            let names: Vec<String> = std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().to_lowercase()).collect();
            if want.iter().all(|w| names.contains(w)) {
                return Some(d.clone());
            }
            next.extend(std::fs::read_dir(d).into_iter().flatten().flatten().filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false)).map(|e| e.path()).take(40));
        }
        level = next;
    }
    None
}

/// On Linux with Proton: the release's Linux builds and its Windows builds, when it has both (else
/// two empty lists). A Linux build the list doesn't name yet counts when its file says Linux.
fn linux_and_windows(st: &App, entry: &Value, rel: &Release, arch: &str) -> (Vec<assets::Asset>, Vec<assets::Asset>) {
    if assets::os_now() != "linux" || !st.lib.lock().unwrap().settings.proton || proton::runner().is_none() || !runs_here(entry, "windows") {
        return (vec![], vec![]);
    }
    let pat = |os: &str| entry.get("assets").and_then(|a| a.get(os)).and_then(Value::as_str);
    let lin: Vec<assets::Asset> = if runs_here(entry, "linux") {
        assets::all_for(&rel.assets, "linux", arch, pat("linux")).into_iter().cloned().collect()
    } else {
        let named = named_for(&rel.assets, "linux");
        assets::all_for(&named, "linux", arch, None).into_iter().cloned().collect()
    };
    let win: Vec<assets::Asset> = assets::all_for(&rel.assets, "windows", arch, pat("windows")).into_iter().cloned().collect();
    if lin.is_empty() || win.is_empty() {
        return (vec![], vec![]);
    }
    (lin, win)
}

/// The files of a release whose names (or kinds) say they are for `os`.
fn named_for(assets: &[assets::Asset], os: &str) -> Vec<assets::Asset> {
    assets.iter().filter(|a| assets::says_os(&a.name, os)).cloned().collect()
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

/// Steam Deck players' scores for each recomp (id -> {score, n}), for "Great on Deck".
#[tauri::command]
async fn deck_scores(st: State<'_, App>, force: Option<bool>) -> Res<Value> {
    Ok(catalog::deck_scores(&catalog::play_scores(&st.http, &st.dir, force.unwrap_or(false)).await?.data))
}

/// The RecompStore team's notes for the top of Discover (none when the site can't be reached).
#[tauri::command]
async fn team_notes(st: State<'_, App>) -> Res<Vec<Value>> {
    Ok(catalog::team_notes(&st.http).await)
}

/// How a finished download's files were checked (Downloads shows it beside each one).
#[tauri::command]
async fn scan_info(st: State<'_, App>, id: String, tag: String) -> Res<Value> {
    Ok(catalog::scan(&st.http, &st.dir, &id, &tag).await?.data)
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
        install::unpause(&id);
        install::resumed(&id);
        jobs.insert(id.clone(), cancel.clone());
    }
    let dir = dir.filter(|d| !d.is_empty()).map(PathBuf::from);
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let st = app2.state::<App>();
        let r = do_install(&app2, &st, &id, dir, tag.filter(|t| !t.is_empty()), file.filter(|f| !f.is_empty()), cancel).await;
        // Windows refusing a file (access denied) says only that: what it usually means, and what
        // to do about it.
        let r = r.map_err(|e| match e {
            Error::Io(io) => {
                let code = io.raw_os_error();
                let win = cfg!(windows);
                // The drive itself failing to read or write (a failing or unplugged drive, a bad
                // memory card), out of room, or a folder that vanished part way.
                let drive = if win { matches!(code, Some(23) | Some(483) | Some(1117) | Some(1392) | Some(1393) | Some(55) | Some(21)) } else { matches!(code, Some(5) | Some(30)) };
                let full = if win { matches!(code, Some(39) | Some(112)) } else { code == Some(28) };
                let gone = win && matches!(code, Some(3) | Some(53) | Some(67));
                if (win && code == Some(5)) || io.kind() == std::io::ErrorKind::PermissionDenied {
                    trail::note(format!("Install {id}: {io}"));
                    Error::Msg(if win {
                        "Windows didn't let the launcher write the game's files (access denied). An antivirus may be checking them, or the library folder is one Windows protects. Try again in a minute, or pick another library folder in Settings → Games and Folders.".into()
                    } else {
                        "The launcher isn't allowed to write to the library folder. Pick one in your home folder in Settings → Games and Folders.".into()
                    })
                } else if drive {
                    trail::note(format!("Install {id}: {io}"));
                    Error::Msg("The drive couldn't read or write the game's files (a drive error, not the download). Check the drive or memory card is plugged in and healthy, or pick a library folder on another drive in Settings → Games and Folders.".into())
                } else if full {
                    trail::note(format!("Install {id}: {io}"));
                    Error::Msg("The drive ran out of space while installing. Free some up, or pick a library folder on another drive in Settings → Games and Folders.".into())
                } else if gone {
                    trail::note(format!("Install {id}: {io}"));
                    Error::Msg("A folder the install was using isn't there anymore (a drive was unplugged or renamed, or a network folder dropped). Check the library folder in Settings → Games and Folders and try again.".into())
                } else {
                    Error::Io(io)
                }
            }
            e => e,
        });
        st.jobs.lock().unwrap().remove(&id);
        // Paused (while downloading, or still waiting its turn): stopped with what came kept.
        let r = match r {
            Err(_) if install::paused(&id) => Err(Error::Msg("PAUSED".into())),
            r => r,
        };
        install::unpause(&id);
        match r {
            Ok(()) => trail::note(format!("Install {id}: done")),
            Err(Error::Msg(m)) if m == "PAUSED" => {
                trail::note(format!("Install {id}: paused"));
                let _ = app2.emit("install", Progress::new(&id, "paused", 0, 0));
            }
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
    let mut builds: Vec<Value> = assets::all_for(&rel.assets, os, arch, pattern).iter().map(|a| json!({"name": a.name, "label": assets::describe(&a.name, os), "best": a.name == best})).collect();
    // On Linux with Proton, the release's builds for the other system too.
    let (lin, win) = linux_and_windows(st.inner(), &entry, rel, arch);
    let others = if os == "linux" { win.iter().map(|a| json!({"name": a.name, "label": "Windows Build", "best": false})).collect::<Vec<_>>() } else { lin.iter().map(|a| json!({"name": a.name, "label": assets::describe(&a.name, "linux"), "best": false})).collect() };
    for o in others {
        if !builds.iter().any(|b| b["name"] == o["name"]) {
            builds.push(o);
        }
    }
    Ok(json!({"tag": rel.tag_name, "builds": builds, "current": have.1}))
}

#[tauri::command(async)]
fn cancel(st: State<'_, App>, id: String) {
    if let Some(c) = st.jobs.lock().unwrap().get(&id) {
        // Cancelled, even if Pause was pressed a moment before: the part file goes too.
        install::unpause(&id);
        c.store(true, Ordering::Relaxed);
    } else {
        // Paused, so nothing's running: what it had downloaded goes.
        install::discard(&id);
    }
}

/// Stops a download where it got to, keeping what came, so Resume carries on from there.
#[tauri::command(async)]
fn pause(st: State<'_, App>, id: String) {
    if let Some(c) = st.jobs.lock().unwrap().get(&id) {
        install::pause(&id);
        c.store(true, Ordering::Relaxed);
    }
}

/// The player's download speed limit (MB a second, 0 for none) and whether downloads slow down
/// while a game is running.
#[tauri::command(async)]
fn download_limits(mb: f64, slow_playing: bool) {
    install::set_limits((mb.max(0.0) * 1048576.0) as u64, slow_playing);
}

#[tauri::command(async)]
fn uninstall(st: State<'_, App>, id: String, delete_files: bool, run_uninstaller: Option<bool>) -> Res<Value> {
    if st.running.lock().unwrap().contains_key(&id) {
        return Err(Error::Msg("Close the game first.".into()));
    }
    if st.moving.lock().unwrap().contains(&id) || st.jobs.lock().unwrap().contains_key(&id) {
        return Err(Error::Msg(MOVING.into()));
    }
    let libs = st.lib.lock().unwrap().settings.all_libraries();
    let Some(g) = st.lib.lock().unwrap().installed.get(&id).cloned() else { return Ok(json!({})) };
    // Its AppImage unpacked for the Flatpak: a copy in the launcher's cache, never the player's.
    flatpak::forget_unpacked(&id);
    // Still open outside the launcher (or still closing): Windows won't let its files go.
    if delete_files && g.exe.as_ref().map(|e| already_open(&if e.is_absolute() { e.clone() } else { g.path.join(e) })).unwrap_or(false) {
        return Err(Error::Msg(format!("{} is still open. Close it, then uninstall it.", g.name)));
    }
    // A game its own installer put in place: its uninstaller if the player said so, and the
    // folder is never deleted by the launcher (it may be Program Files or anywhere else).
    if g.installer || !g.installer_setup.is_empty() {
        let _ = std::fs::remove_dir_all(installer::staging(&st.dir, &id));
        let ran = run_uninstaller == Some(true) && g.installer && g.path.is_dir() && installer::run_uninstaller(&g.path);
        trail::note(format!("Uninstall {id}: an installed game{}", if ran { ", its uninstaller ran" } else { "" }));
        st.lib.lock().unwrap().forget(&id);
        st.save();
        return Ok(json!({"left": 0, "path": g.path, "kept": g.path.is_dir() && !ran, "uninstaller": ran, "installer": true}));
    }
    // A local build's folder is the developer's: it only leaves the Library.
    let delete_files = delete_files && !g.local;
    let mut left: Vec<String> = vec![];
    let mut kept = false;
    let mut uninstaller = false;
    let mut kept_copy: Option<PathBuf> = None;
    let mut copy_stuck = false;
    if delete_files {
        // A Flatpak the launcher installed from the game's bundle goes with it.
        if !cfg!(windows) && flatpak::available() {
            let bundle = g.exe.as_ref().map(|e| if e.is_absolute() { e.clone() } else { g.path.join(e) }).filter(|e| e.extension().map(|x| x.eq_ignore_ascii_case("flatpak")).unwrap_or(false));
            if let Some(app_id) = bundle.and_then(|b| flatpak::app_id(&b)).or_else(|| flatpak::bundle_in(&g.path).map(|(_, a)| a)) {
                flatpak::remove(&app_id);
            }
        }
        // Only ever a game's own folder right inside a library. A game the player brought over
        // from somewhere else keeps its folder, and so does anything shared like Downloads.
        // Or a folder the launcher made for it (named after it) in a library folder since removed.
        let in_lib = g.path.parent().map(|p| libs.iter().any(|l| library::same_path(l, p))).unwrap_or(false);
        let made_here = !g.asset.is_empty() && g.path.file_name().map(|n| n.to_string_lossy().eq_ignore_ascii_case(&library::folder_name(&id))).unwrap_or(false);
        let ok = (in_lib || made_here) && !library::shared_folder(&g.path, &libs) && g.path.is_dir();
        // The player's own files (their copy of the game, its update) kept inside the game's
        // folder are theirs: moved out to "Game Copies" beside it first, and if one can't be, the
        // folder stays. Only files: a folder there is the launcher's unpack of their disc.
        let mut keep_failed = false;
        if ok {
            for c in [g.game_copy.as_ref(), g.update_copy.as_ref()].into_iter().flatten() {
                if !(library::inside(c, &g.path) && c.is_file() && !c.is_symlink()) {
                    continue;
                }
                match save_copy_out(&g.path, &id, c) {
                    Some(to) => {
                        trail::note(format!("Uninstall {id}: kept the player's file in {}", to.display()));
                        kept_copy.get_or_insert(to);
                    }
                    None => keep_failed = true,
                }
            }
        }
        if ok && !keep_failed {
            left = remove_game_folder(&g.path);
        } else if g.path.is_dir() {
            copy_stuck = keep_failed;
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
    st.lib.lock().unwrap().forget(&id);
    st.save();
    Ok(json!({"left": left.len(), "path": g.path, "kept": kept, "uninstaller": uninstaller, "kept_copy": kept_copy, "copy_stuck": copy_stuck}))
}

/// Move the player's copy out of a game folder that's about to go, to "Game Copies/<id>" beside
/// it (same drive, so it's a quick rename). Where it went, or None if it couldn't move.
fn save_copy_out(game: &Path, id: &str, copy: &Path) -> Option<PathBuf> {
    let dir = game.parent()?.join("Game Copies").join(library::folder_name(id));
    std::fs::create_dir_all(&dir).ok()?;
    let name = copy.file_name()?;
    let mut to = dir.join(name);
    let mut n = 2;
    while to.exists() {
        let p = Path::new(name);
        let stem = p.file_stem().unwrap_or(name).to_string_lossy();
        to = dir.join(match p.extension().filter(|_| copy.is_file()) {
            Some(x) => format!("{stem} ({n}).{}", x.to_string_lossy()),
            None => format!("{} ({n})", name.to_string_lossy()),
        });
        n += 1;
    }
    if std::fs::rename(copy, &to).is_err() {
        // Nothing moved: leave no empty folders behind.
        let _ = std::fs::remove_dir(&dir);
        let _ = std::fs::remove_dir(dir.parent()?);
        return None;
    }
    Some(to)
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
    tokio::task::spawn_blocking(move || manage::backup(&g.path, &inputs::release_files(&g), &backups_dir(&g), 5)).await.map_err(|e| Error::Msg(e.to_string()))?
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

/// Text Size: the launcher's window zoomed (0.9 to 1.5).
#[tauri::command]
fn set_ui_scale(app: AppHandle, scale: f64) -> Res<()> {
    if !(0.5..=2.0).contains(&scale) {
        return Err(Error::Msg("That size isn't one of the choices.".into()));
    }
    let w = app.get_webview_window("main").ok_or_else(|| Error::Msg("The launcher's window isn't open.".into()))?;
    w.set_zoom(scale).map_err(|e| Error::Msg(e.to_string()))
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

#[derive(Serialize)]
struct CopyFile {
    path: String,
    name: String,
    folder: String,
    size: u64,
    region: String,
    /// "game", "update" or "dlc".
    kind: &'static str,
    disc: Option<u32>,
    /// One of the catalog's known dumps of this game.
    verified: bool,
    /// The one the launcher uses for the game.
    picked: bool,
}

#[derive(Serialize)]
struct CopyFiles {
    files: Vec<CopyFile>,
    /// Discs of a set that weren't found (2 when Disc 1 and Disc 3 are there).
    missing_discs: Vec<u32>,
}

/// Everything found in the player's folders for one game, for the "Your copy" list: its discs in
/// order, then updates, then extra content. `picked` is the copy the launcher uses (or the one the
/// player chose); `dropped` are the ones they said aren't theirs.
#[tauri::command]
async fn copy_files(st: State<'_, App>, id: String, picked: Option<String>, dropped: Vec<String>) -> Res<CopyFiles> {
    let cat = cached_catalog(&st);
    let entry = catalog::app(&cat, &id).cloned().ok_or_else(|| Error::Msg("That game isn't in the list.".into()))?;
    let roms = st.lib.lock().unwrap().roms.clone();
    tokio::task::spawn_blocking(move || {
        let mut found = roms::files_of(&entry, &roms);
        // The one the launcher picked counts even when its name didn't match (a folder found by its check).
        if let Some(p) = picked.as_deref().filter(|p| !p.is_empty()) {
            if !found.iter().any(|r| r.path.to_string_lossy() == p) {
                let path = PathBuf::from(p);
                let size = std::fs::metadata(&path).map(|m| if m.is_file() { m.len() } else { 0 }).unwrap_or(0);
                found.push(roms::RomFile { path, system: String::new(), title: String::new(), region: String::new(), size });
            }
        }
        let mut files: Vec<CopyFile> = found
            .into_iter()
            .filter(|r| !dropped.iter().any(|d| r.path.to_string_lossy() == d.as_str()))
            .map(|r| {
                let name = r.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let part = roms::part_of(&name);
                let path = r.path.to_string_lossy().to_string();
                CopyFile {
                    folder: r.path.parent().map(|f| f.to_string_lossy().to_string()).unwrap_or_default(),
                    verified: roms::known_dump(&entry, &r.path),
                    picked: picked.as_deref() == Some(path.as_str()),
                    kind: part.kind,
                    disc: part.disc,
                    size: r.size,
                    region: r.region,
                    name,
                    path,
                }
            })
            .collect();
        let rank = |k: &str| match k { "game" => 0, "update" => 1, _ => 2 };
        files.sort_by(|a, b| (rank(a.kind), a.disc.unwrap_or(0), a.name.to_lowercase()).cmp(&(rank(b.kind), b.disc.unwrap_or(0), b.name.to_lowercase())));
        let discs: Vec<u32> = files.iter().filter_map(|f| f.disc).collect();
        let of = files.iter().filter_map(|f| roms::part_of(&f.name).of).max().unwrap_or(0);
        let top = discs.iter().copied().max().unwrap_or(0).max(of);
        let missing_discs = if discs.is_empty() { vec![] } else { (1..=top.min(20)).filter(|n| !discs.contains(n)).collect() };
        CopyFiles { files, missing_discs }
    })
    .await
    .map_err(|e| Error::Msg(e.to_string()))
}

/// What an installed recomp's own recomplauncher.json (in its folder) covers, for the "Launcher
/// support" list: each feature it sets up itself. Null when it has none.
#[tauri::command]
fn support_local(st: State<'_, App>, id: String) -> Option<Value> {
    let dir = st.lib.lock().unwrap().installed.get(&id).map(|g| g.path.clone())?;
    let p = profile::release_profile(&dir)?.profile;
    let game = p.game.as_ref();
    Some(json!({
        "easy_setup": game.is_some(),
        "settings": !p.settings.is_empty(),
        "prepare": p.prepare.is_some(),
        "verified": game.map(|g| !g.dumps.is_empty() || !g.check.ids.is_empty()).unwrap_or(false),
        "saves": !p.saves.is_empty(),
    }))
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
    // A game the launcher installed isn't anyone's copy of itself (its own programs are refused by
    // the copy's checks).
    let found: Vec<roms::RomFile> = found.iter().filter(|r| !dirs.values().any(|d| !d.as_os_str().is_empty() && &r.path == d)).cloned().collect();
    let found = &found[..];
    let mut out = roms::match_catalog(cat, found);
    let none = PathBuf::new();
    for a in cat.get("apps").and_then(Value::as_array).into_iter().flatten() {
        let (Some(id), repo) = (a.get("id").and_then(Value::as_str), a.get("repo").and_then(Value::as_str).unwrap_or("")) else { continue };
        let Some(need) = profile::find(dirs.get(id).unwrap_or(&none), repo, a).and_then(|p| p.game) else { continue };
        // A check that can't tell games apart (default.xex, which every game of its console has,
        // or just "a disc image"): matched by name only, unless the catalog lists its disc IDs.
        let want: Vec<String> = need.check.ids.iter().map(|i| roms::id_key(i)).filter(|i| i.len() >= 3).chain(roms::copy_ids(a)).collect();
        let named_ok = out.get(id).map(|r| profile::check_copy(&need, &r.path).is_ok()).unwrap_or(false);
        if !need.identifies() && want.is_empty() {
            // Found by its name only: kept when its own check passes (a catalog profile's check on a
            // common file was never applied to a name, and still isn't).
            if !need.generic && !named_ok {
                out.remove(id);
            }
            continue;
        }
        // By its disc ID: one that's this game's. One that can't be read (a folder, an Xbox disc)
        // counts only when there's another check that tells it apart.
        let id_fits = |p: &Path| {
            let by_contents = need.identifies_kind(p.is_dir());
            if want.is_empty() {
                by_contents
            } else {
                roms::copy_id(p).map(|id| want.iter().any(|w| id.starts_with(w.as_str()))).unwrap_or(by_contents)
            }
        };
        if named_ok {
            continue;
        }
        out.remove(id);
        // Recognized PlayStation and Switch folders are matched by title. Shared files
        // such as eboot.bin or main.npdm can't identify another game.
        if let Some(r) = found.iter().find(|r| {
            // A build ID is the exception: it's read from the package or folder itself.
            let by_build = !need.check.nup_build_id.is_empty() && matches!(r.system.as_str(), "switch" | "switch2");
            let matched_by_title = !by_build
                && (matches!(r.system.as_str(), "ps3" | "ps4" | "ps5" | "switch2") || (r.system == "switch" && r.size == 0));
            !matched_by_title && id_fits(&r.path) && profile::check_copy(&need, &r.path).is_ok()
        }) {
            out.insert(id.to_string(), r.clone());
        }
    }
    out
}

/// A game the player added by its link that the list now has itself (the same repo, under the
/// list's own id): their copy becomes the list's, where it is, with its version, settings, save
/// backups, play time, mods, favorites and pins, so nothing has to be installed again. Not while
/// it's playing, downloading or moving (next time), and not when the list's one is installed too
/// (both stay). Gives back the ones moved over: (their id, the list's id, its name).
pub(crate) fn merge_mine(st: &App, cat: &Value) -> Vec<(String, String, String)> {
    let _turn = catalog::MINE_TURN.lock().unwrap_or_else(|e| e.into_inner());
    let list = catalog::mine(&st.dir);
    if list.is_empty() {
        return vec![];
    }
    let apps: Vec<&Value> = cat.get("apps").and_then(Value::as_array).map(|a| a.iter().filter(|x| !x.get("custom").and_then(Value::as_bool).unwrap_or(false)).collect()).unwrap_or_default();
    let (mut keep, mut out) = (vec![], vec![]);
    for e in list {
        let old = str_of(&e, "id").to_string();
        let repo = str_of(&e, "repo").to_lowercase();
        let Some(o) = apps.iter().find(|a| !repo.is_empty() && str_of(a, "repo").to_lowercase() == repo && str_of(a, "id") != old) else {
            keep.push(e);
            continue;
        };
        let new = str_of(o, "id").to_string();
        // Not while either one is installing, updating, running, moving, being set up or getting
        // mods, nor while the chosen saves folder can't be reached (its backups would stay behind):
        // it's tried again the next time the list comes in.
        let busy = [&old, &new].iter().any(|i| st.jobs.lock().unwrap().contains_key(*i) || st.running.lock().unwrap().contains_key(*i) || st.moving.lock().unwrap().contains(*i) || setup_busy(i) || mods_busy(i))
            || saves::chosen_root().is_some_and(|p| !p.is_dir());
        let had = {
            let mut l = st.lib.lock().unwrap();
            let had = l.installed.contains_key(&old);
            if busy || (had && l.installed.contains_key(&new)) {
                None
            } else {
                if let Some(a) = l.arts.remove(&old) {
                    l.arts.entry(new.clone()).or_insert(a);
                }
                if let Some(mut g) = l.installed.remove(&old) {
                    g.id = new.clone();
                    l.put(&new, g);
                }
                for target in l.project_aliases.values_mut() {
                    if target == &old { *target = new.clone(); }
                }
                if let Some(m) = l.mods.remove(&old) {
                    l.mods.entry(new.clone()).or_insert(m);
                }
                if let Some(v) = l.seen_versions.remove(&old) {
                    l.seen_versions.entry(new.clone()).or_insert(v);
                }
                rename_id(&mut l.settings.ui, &old, &new);
                Some(had)
            }
        };
        let Some(had) = had else {
            keep.push(e);
            continue;
        };
        // Its save backups and this PC's notes about them, and its Look. What the official one
        // already has stays as it is.
        let mut dirs = vec![(saves::dir_of(&st.dir, &old), saves::dir_of(&st.dir, &new)), (st.dir.join("saves").join(&old), st.dir.join("saves").join(&new)), (st.dir.join("looks").join(&old), st.dir.join("looks").join(&new))];
        dirs.dedup();
        for (from, to) in dirs {
            if let Err(err) = move_into(&from, &to) {
                trail::note(format!("{new}: couldn't move all of {} to {} ({err})", from.display(), to.display()));
            }
        }
        trail::note(format!("{old}: the list has it now as {new}; {}", if had { "the copy here moved over to it" } else { "the list's one is used from now on" }));
        out.push((old, new, str_of(o, "name").to_string()));
    }
    if !out.is_empty() {
        // The library first: if the list can't be written, the next time finds it moved already.
        st.save();
        if let Err(err) = catalog::save_mine(&st.dir, &keep) {
            trail::note(format!("Couldn't save the list of games added by link ({err})"));
        }
    }
    out
}

/// A folder's things moved into another; ones the other already has stay where they were.
fn move_into(from: &Path, to: &Path) -> std::io::Result<()> {
    if !from.is_dir() {
        return Ok(());
    }
    if !to.exists() {
        return std::fs::rename(from, to);
    }
    let mut left = 0;
    for e in std::fs::read_dir(from)?.flatten() {
        let dest = to.join(e.file_name());
        if dest.exists() || std::fs::rename(e.path(), &dest).is_err() {
            left += 1;
        }
    }
    if left == 0 {
        let _ = std::fs::remove_dir(from);
        Ok(())
    } else {
        Err(std::io::Error::other(format!("{left} left where they were")))
    }
}

/// A game's id changed (`old` to `new`) in the player's own settings: lists of ids (favorites,
/// pins, hidden games) and maps keyed by id (covers, names, choices kept per game).
fn rename_id(v: &mut Value, old: &str, new: &str) {
    match v {
        Value::String(s) if s == old => *s = new.to_string(),
        Value::Array(a) => {
            a.iter_mut().for_each(|x| rename_id(x, old, new));
            // A list that had both keeps one.
            let mut seen = false;
            a.retain(|x| x.as_str() != Some(new) || !std::mem::replace(&mut seen, true));
        }
        Value::Object(o) => {
            if let Some(x) = o.remove(old) {
                o.entry(new.to_string()).or_insert(x);
            }
            o.values_mut().for_each(|x| rename_id(x, old, new));
        }
        _ => {}
    }
}

/// After a fresh catalog: tell the player about new recomps for games they own.
pub(crate) fn note_new_apps(app: &AppHandle, st: &App, cat: &Value) {
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
    let g = installed_or_game(&st, &id)?;
    // The player's icon (Customize) or the list's, else the game's program.
    let icon = game_art(&st, &id, &g).await.icon;
    let name = shown_name(&st, &id, &g).await;
    // Straight to the game unless the player asked for shortcuts through the launcher (Settings),
    // or the game can only be started by the launcher for now.
    let direct_on = !st.lib.lock().unwrap().settings.ui.get("shortcut_launcher").and_then(Value::as_bool).unwrap_or(false);
    // One game of several always goes through the launcher (it picks the game, and counts its time).
    let direct = if direct_on && game_of(&st, &id).is_none() { direct_start(&st, &id, &g) } else { None };
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
            p.command_for(&g.path, copy.as_deref(), &g.picks, is_windows_build(g))?
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
    match net::send(http.get(&full).timeout(net::LONGER), &full, true).await {
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

/// A game's pictures for Steam and shortcuts: the player's own from Customize first, else the
/// list's. Each is (bytes, extension); empty when there's none.
struct GameArt {
    cover: (Vec<u8>, &'static str),
    hero: (Vec<u8>, &'static str),
    logo: (Vec<u8>, &'static str),
    /// A picture file for the game's icon (an .ico on Windows when there is one).
    icon: Option<PathBuf>,
}

async fn game_art(st: &App, id: &str, g: &Installed) -> GameArt {
    let cat = catalog::catalog(&st.http, &st.dir, false).await.map(|f| f.data).unwrap_or(Value::Null);
    // One game of several: its own pictures from the catalog, else the recomp's. The player's own
    // (Customize) are the recomp's, so they're left for the recomp.
    let sel = game_of(st, id);
    let a = match &sel {
        Some(pk) => {
            let mut a = catalog::app(&cat, &pk.recomp).cloned().unwrap_or_else(|| json!({}));
            for (k, v) in [("cover", &pk.game.cover), ("hero", &pk.game.hero), ("logo", &pk.game.logo), ("icon", &pk.game.icon)] {
                if !v.is_empty() {
                    a[k] = json!(if v.starts_with('/') { format!("{}{}", catalog::base(), v) } else { v.clone() });
                }
            }
            a
        }
        None => catalog::app(&cat, id).cloned().unwrap_or(Value::Null),
    };
    let plain;
    let g = match &sel {
        Some(_) => {
            plain = Installed { cover_file: None, hero_file: None, logo_file: None, icon_file: None, ..g.clone() };
            &plain
        }
        None => g,
    };
    async fn pick(st: &App, own: Option<&PathBuf>, url: &str) -> (Vec<u8>, &'static str) {
        if let Some(f) = own.filter(|f| f.is_file()) {
            let ext = if f.extension().map(|e| e.eq_ignore_ascii_case("png")).unwrap_or(false) { "png" } else { "jpg" };
            return (std::fs::read(f).unwrap_or_default(), ext);
        }
        (fetch_bytes(&st.http, url).await, ext_of(url))
    }
    let cover = pick(st, g.cover_file.as_ref(), str_of(&a, "cover")).await;
    let hero = pick(st, g.hero_file.as_ref(), str_of(&a, "hero")).await;
    let logo = pick(st, g.logo_file.as_ref(), str_of(&a, "logo")).await;
    // The icon: the player's, else the list's (kept once in the covers folder), else (Windows) the
    // game's program.
    let mut icon = g.icon_file.clone().filter(|f| f.is_file());
    if icon.is_none() {
        let url = str_of(&a, "icon");
        if !url.is_empty() {
            let file = st.dir.join("covers").join(format!("{}-icon-list.png", folder_name(id)));
            if !file.is_file() {
                let b = fetch_bytes(&st.http, url).await;
                if crate::media::mime(&b) == "image/png" {
                    let _ = std::fs::create_dir_all(st.dir.join("covers"));
                    let _ = std::fs::write(&file, &b);
                    let _ = std::fs::write(file.with_extension("ico"), art::ico_from_png(&b));
                }
            }
            icon = Some(file).filter(|f| f.is_file());
        }
    }
    let icon = match icon {
        Some(p) if cfg!(windows) => Some(Some(p.with_extension("ico")).filter(|i| i.is_file()).unwrap_or(p)),
        Some(p) => Some(p),
        None if cfg!(windows) => g.exe.as_ref().map(|e| g.path.join(e)),
        None => None,
    };
    GameArt { cover, hero, logo, icon }
}

/// Steam's wide picture (920x430): the background filled to that shape, with the logo on it when
/// there is one. The background as it is when it can't be read.
fn wide_capsule(hero: &[u8], logo: &[u8]) -> Option<Vec<u8>> {
    use image::GenericImageView;
    let base = image::load_from_memory(hero).ok()?;
    let mut base = base.resize_to_fill(920, 430, image::imageops::FilterType::Triangle).to_rgba8();
    if let Ok(l) = image::load_from_memory(logo) {
        let (lw, lh) = l.dimensions();
        if lw > 0 && lh > 0 {
            let k = (920.0 * 0.62 / lw as f32).min(430.0 * 0.55 / lh as f32);
            let l = l.resize((lw as f32 * k).max(1.0) as u32, (lh as f32 * k).max(1.0) as u32, image::imageops::FilterType::Triangle).to_rgba8();
            let (x, y) = ((920 - l.width() as i64) / 2, (430 - l.height() as i64) / 2);
            image::imageops::overlay(&mut base, &l, x, y);
        }
    }
    let rgb = image::DynamicImage::ImageRgba8(base).to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90).encode_image(&rgb).ok()?;
    Some(out)
}

/// The name for Steam and shortcuts: the player's own (Rename), else the game's, else the recomp's.
async fn shown_name(st: &App, id: &str, g: &Installed) -> String {
    let own = st.lib.lock().unwrap().settings.ui.get("names").and_then(|n| n.get(id)).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    match own {
        Some(n) => n,
        None => match game_of(st, id) {
            Some(pk) => pk.game.name,
            None => game_name(st, id, &g.name).await,
        },
    }
}

/// An installed recomp, or the recomp one of its games is part of.
fn installed_or_game(st: &App, id: &str) -> Res<Installed> {
    let g = st.lib.lock().unwrap().installed.get(id).cloned();
    g.or_else(|| game_of(st, id).map(|p| p.g)).ok_or_else(|| Error::Msg("It isn't installed.".into()))
}

/// A game's pictures for Steam's grid folder, as (suffix, bytes, extension), and its icon file.
/// The player's own pictures (Customize) when they have them, else the list's.
async fn steam_art(st: &App, id: &str, g: &Installed) -> (Vec<(&'static str, Vec<u8>, &'static str)>, String) {
    let ga = game_art(st, id, g).await;
    let wide = wide_capsule(&ga.hero.0, &ga.logo.0).map(|b| (b, "jpg")).unwrap_or_else(|| ga.hero.clone());
    let icon = ga.icon.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    let art = vec![("p", ga.cover.0, ga.cover.1), ("_hero", ga.hero.0, ga.hero.1), ("", wide.0, wide.1), ("_logo", ga.logo.0, ga.logo.1)];
    (art, icon)
}

/// Add the game to Steam. If Steam is open and `restart` is false, nothing changes and the answer
/// is "steam_running", so the window can ask first; with `restart`, Steam is closed, the game
/// added, and Steam started again.
#[tauri::command]
async fn add_to_steam(st: State<'_, App>, id: String, restart: bool) -> Res<String> {
    let steam = shortcuts::steam_dir().ok_or_else(|| Error::Msg("Steam isn't installed on this PC.".into()))?;
    let g = installed_or_game(&st, &id)?;
    let was_running = shortcuts::steam_running();
    if was_running {
        if let Some(why) = shortcuts::cant_restart_steam() {
            return Err(Error::Msg(why.into()));
        }
    }
    if was_running && !restart {
        return Ok("steam_running".into());
    }
    let (art, icon) = steam_art(&st, &id, &g).await;
    if was_running {
        if let Some(why) = shortcuts::cant_restart_steam() {
            return Err(Error::Msg(why.into()));
        }
        trail::note(format!("Add to Steam ({id}): closing Steam"));
        shortcuts::close_steam(&steam).await.inspect_err(|e| trail::note(format!("Add to Steam: {e}")))?;
    }
    let name = shown_name(&st, &id, &g).await;
    let n = shortcuts::write_to_steam(&steam, &name, &id, &icon, &art, &st.dir).inspect_err(|e| trail::note(format!("Add to Steam ({id}): {e}")))?;
    trail::note(format!("Add to Steam ({id}): written for {n} account(s) in {}{}", dirs::home_dir().and_then(|h| steam.strip_prefix(h).ok().map(|r| format!("~/{}", r.display()))).unwrap_or_else(|| steam.display().to_string()), if was_running { "" } else { " (Steam was closed)" }));
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

/// A recomp's README from its GitHub, for its page: kept on disk for a day, and the copy on disk
/// when GitHub can't be reached. None when the repo has none.
#[tauri::command]
async fn repo_readme(st: State<'_, App>, repo: String) -> Res<Option<String>> {
    let ok = repo.split('/').count() == 2 && !repo.contains("..") && repo.chars().all(|c| c.is_ascii_alphanumeric() || "-_./".contains(c));
    if !ok {
        return Ok(None);
    }
    let file = st.dir.join("cache/readme").join(format!("{}.md", repo.replace('/', "__")));
    let age = std::fs::metadata(&file).and_then(|m| m.modified()).ok().and_then(|m| m.elapsed().ok()).map(|d| d.as_secs());
    let disk = || std::fs::read_to_string(&file).ok();
    if age.map(|a| a < 24 * 3600).unwrap_or(false) {
        return Ok(disk().filter(|t| !t.is_empty()));
    }
    let got = st
        .http
        .get(format!("https://api.github.com/repos/{repo}/readme"))
        .header("Accept", "application/vnd.github.raw")
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await;
    let text = match got {
        Ok(r) if r.status() == reqwest::StatusCode::NOT_FOUND => Some(String::new()),
        Ok(r) if r.status().is_success() => r.text().await.ok(),
        _ => None,
    };
    match text {
        Some(mut t) => {
            if t.len() > 200_000 {
                let mut cut = 200_000;
                while !t.is_char_boundary(cut) {
                    cut -= 1;
                }
                t.truncate(cut);
            }
            let _ = std::fs::create_dir_all(file.parent().unwrap());
            let _ = std::fs::write(&file, &t);
            Ok(Some(t).filter(|t| !t.is_empty()))
        }
        None => Ok(disk().filter(|t| !t.is_empty())),
    }
}

/// Microsoft's Visual C++ runtime (x64), which almost every Windows recomp needs: its official
/// installer downloaded from Microsoft and run with its own small progress window. "installed",
/// "restart" (Windows wants a restart to finish) or an error.
#[tauri::command]
async fn install_vcredist(st: State<'_, App>) -> Res<String> {
    #[cfg(not(windows))]
    {
        let _ = &st;
        Err(Error::Msg("The Visual C++ runtime is for Windows.".into()))
    }
    #[cfg(windows)]
    {
        trail::note("Visual C++ runtime: getting Microsoft's installer");
        let r = st
            .http
            .get("https://aka.ms/vs/17/release/vc_redist.x64.exe")
            .timeout(std::time::Duration::from_secs(300))
            .send()
            .await
            .map_err(|e| Error::Msg(format!("Couldn't download the Visual C++ runtime: {e}")))?;
        // Only from Microsoft, wherever the short link leads.
        let host = r.url().host_str().unwrap_or("").to_ascii_lowercase();
        if !(host.ends_with(".microsoft.com") || host == "microsoft.com") || !r.status().is_success() {
            return Err(Error::Msg("Microsoft's download didn't answer as expected. Try again later, or get it from microsoft.com.".into()));
        }
        let bytes = r.bytes().await.map_err(|e| Error::Msg(format!("Couldn't download the Visual C++ runtime: {e}")))?;
        if bytes.len() < 1 << 20 || &bytes[..2] != b"MZ" {
            return Err(Error::Msg("Microsoft's download didn't answer as expected. Try again later, or get it from microsoft.com.".into()));
        }
        let dir = st.dir.join("cache");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("vc_redist.x64.exe");
        std::fs::write(&file, &bytes)?;
        let f2 = file.clone();
        let code = tokio::task::spawn_blocking(move || -> Result<Option<i32>, Error> {
            let args = "/install /passive /norestart";
            let admin = |f: &Path| -> Result<Option<i32>, Error> {
                match win::run_as_admin(f, args, f.parent().unwrap_or(Path::new("."))) {
                    Ok(h) => Ok(h.wait()),
                    // 1223: the player said No when Windows asked.
                    Err(e) if e.raw_os_error() == Some(1223) => Ok(Some(1223)),
                    Err(e) => Err(Error::Msg(format!("Windows didn't start the installer: {e}"))),
                }
            };
            let mut c = std::process::Command::new(&f2);
            c.args(args.split(' '));
            match c.spawn() {
                // 5: Windows didn't let it install without asking first. Asked once, the way
                // Windows asks (the player presses Yes).
                Ok(mut child) => match child.wait().ok().and_then(|s| s.code()) {
                    Some(5) => {
                        trail::note("Visual C++ runtime: the installer needs Windows' permission; asking");
                        admin(&f2)
                    }
                    other => Ok(other),
                },
                Err(e) if e.raw_os_error() == Some(740) => admin(&f2),
                Err(e) => Err(Error::Msg(format!("Couldn't start the installer: {e}"))),
            }
        })
        .await
        .map_err(|e| Error::Msg(e.to_string()))??;
        let _ = std::fs::remove_file(&file);
        trail::note(format!("Visual C++ runtime: installer closed with {code:?}"));
        match code {
            Some(5) | Some(1223) => Err(Error::Msg("Windows needs your permission to install it. Press the button again and choose Yes when Windows asks.".into())),
            Some(0) => Ok("installed".into()),
            // 1638: it's already there (this one or a newer one): what's missing is something else.
            Some(1638) => Ok("already".into()),
            Some(3010) | Some(1641) => Ok("restart".into()),
            Some(1602) => Err(Error::Msg("The installer was cancelled.".into())),
            _ => Err(Error::Msg("The Visual C++ installer didn't finish. Run it from microsoft.com to see why.".into())),
        }
    }
}

/// GE-Proton, for Windows recomps on Linux (and for Steam's own games too): the newest one from
/// its GitHub. Progress comes as "ge_progress".
#[tauri::command]
async fn ge_proton_install(app: AppHandle, st: State<'_, App>) -> Res<String> {
    let cancel = Arc::new(AtomicBool::new(false));
    let a = app.clone();
    let report = move |p: install::Progress| {
        let _ = a.emit("ge_progress", &p);
    };
    trail::note("GE-Proton: getting the newest");
    let tag = proton::install_ge(&st.http, &st.dir.join("cache"), cancel, &report).await.inspect_err(|e| trail::note(format!("GE-Proton: {e}")))?;
    trail::note(format!("GE-Proton: {tag} in place"));
    Ok(tag)
}

/// What Windows recomps start through on this PC now: its name, and whether it's a GE-Proton.
#[tauri::command(async)]
fn proton_now() -> Value {
    let name = proton::runner().map(|r| r.name());
    let ge = name.as_deref().map(|n| proton::rank(n).0 == 2).unwrap_or(false);
    json!({ "runner": name, "ge": ge, "umu_broken": proton::umu_broken_runtime().is_some() })
}

/// NVIDIA DLSS (nvidia.dlss): fetches the shared file now (Easy Setup's row, Try Again), with its
/// progress as "dlss" events. A failure never stops setup: the game plays without it.
#[tauri::command]
async fn dlss_fetch(app: AppHandle, st: State<'_, App>) -> Res<String> {
    if !dlss::applies(&st.dir) {
        return Err(Error::Msg(dlss::FAILED.into()));
    }
    let a = app.clone();
    let path = dlss::ensure(&st.http, &st.dir, &move |done, total| {
        let _ = a.emit("dlss", json!({ "done": done, "total": total }));
    })
    .await
    .map_err(Error::Msg)?;
    let _ = app.emit("dlss", json!({ "ready": true }));
    Ok(path.to_string_lossy().into())
}

/// Turns DLSS off (or back on) for one game. Off: the game's own managed copy goes now.
#[tauri::command(async)]
fn dlss_set(st: State<'_, App>, id: String, input: String, on: bool) -> Res<()> {
    let id = recomp_of(&st, &id);
    let mut l = st.lib.lock().unwrap();
    let g = l.installed.get_mut(&id).ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    if on {
        g.runtime_off.remove(&input);
    } else {
        g.runtime_off.insert(input.clone());
        if let Some(rel) = g.input_staged.remove(&input) {
            let target = g.path.join(&rel);
            if inputs::relative(&rel) && target.is_file() {
                let _ = std::fs::remove_file(target);
            }
            g.input_stamps.remove(&input);
        }
    }
    l.save(&st.dir)?;
    Ok(())
}

/// Settings: whether DLSS applies here and the shared file is kept, and removing it (every game
/// using it plays without it until it's downloaded again when one next needs it).
#[tauri::command(async)]
fn dlss_state(st: State<'_, App>) -> Value {
    json!({ "applies": dlss::applies(&st.dir), "have": dlss::current(&st.dir).is_some() })
}

#[tauri::command(async)]
fn dlss_remove(st: State<'_, App>) -> Res<()> {
    dlss::remove(&st.dir).map_err(Error::Msg)
}

/// Settings → Your PC: put umu-run's incomplete Steam runtime right (it downloads again).
#[tauri::command(async)]
fn umu_repair() -> Res<String> {
    proton::umu_repair().map_err(Error::Msg)
}

// ---------- playing ----------

/// A shell script run through bash (or sh where there's no bash).
fn shell_script(exe: &Path) -> tokio::process::Command {
    let bash = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).any(|d| d.join("bash").is_file())).unwrap_or(false);
    let mut c = tokio::process::Command::new(if bash { "bash" } else { "sh" });
    c.arg(exe);
    c
}

/// A text file to start that doesn't say what runs it (no "#!" line): a shell script, then. Not a
/// program (ELF), and not one that names its own interpreter.
fn headless_script(exe: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 512];
    let n = std::fs::File::open(exe).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
    let head = &head[..n];
    n > 0 && !head.starts_with(b"\x7fELF") && !head.starts_with(b"#!") && !head.contains(&0)
}

/// A Windows start script (.bat, .cmd) that only starts one program in the game's folder: that
/// program and the arguments it's given. None for anything more (a loop, a choice, several steps).
fn bat_target(script: &Path) -> Option<(PathBuf, Vec<String>)> {
    let low = script.extension()?.to_string_lossy().to_ascii_lowercase();
    if low != "bat" && low != "cmd" {
        return None;
    }
    let dir = script.parent()?;
    let text = std::fs::read(script).ok().filter(|b| b.len() < 16_000).map(|b| String::from_utf8_lossy(&b).to_string())?;
    let mut found = None;
    for line in text.lines().map(str::trim) {
        let l = line.trim_start_matches('@');
        let w = l.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
        if l.is_empty() || l.starts_with("::") || ["rem", "echo", "title", "popd", "setlocal", "endlocal", "exit", "pause", "chcp", "cls"].contains(&w.as_str()) {
            continue;
        }
        // Into the script's own folder only ("cd /d %~dp0"); into another folder, the program
        // after it is somewhere else: not a plain start script.
        if w == "cd" || w == "pushd" || w.starts_with("cd/") || w.starts_with("cd\\") {
            let to: String = l[w.len()..].trim().trim_start_matches("/d").trim().trim_matches('"').to_string();
            if to.is_empty() || to.eq_ignore_ascii_case("%~dp0") || to == "." {
                continue;
            }
            return None;
        }
        // Output sent somewhere, or a second command on the line: more than a start.
        if l.contains(['>', '<', '|', '&']) {
            return None;
        }
        // Anything that decides or repeats: not a plain start script.
        if ["if", "for", "goto", "call", "set", "choice"].contains(&w.as_str()) || l.starts_with(':') || found.is_some() {
            return None;
        }
        // "start "" Game.exe" or "start /wait Game.exe": the program after it.
        let mut parts = split_args(l);
        if parts.first().map(|p| p.eq_ignore_ascii_case("start")).unwrap_or(false) {
            parts.remove(0);
            while parts.first().map(|p| p.starts_with('/') || p.is_empty()).unwrap_or(false) {
                parts.remove(0);
            }
        }
        let prog = parts.first()?.replace("%~dp0", "").replace("%cd%", "").replace('\\', "/");
        if !prog.to_ascii_lowercase().ends_with(".exe") {
            return None;
        }
        let path = dir.join(prog.trim_start_matches("./").trim_start_matches('/'));
        if prog.contains("..") || prog.contains(':') || !path.is_file() || !path.starts_with(dir) {
            return None;
        }
        // The script's folder (%~dp0) as the real folder; %* (what the script was given) dropped;
        // any other variable can't be known here.
        let here = format!("{}{}", dir.display(), std::path::MAIN_SEPARATOR);
        let mut args = vec![];
        for a in &parts[1..] {
            if a == "%*" {
                continue;
            }
            let a = a.replace("%~dp0", &here);
            if a.contains('%') {
                return None;
            }
            args.push(a);
        }
        found = Some((path, args));
    }
    found
}

/// A line of numbers a game prints about how it's running (frame times, shader and pipeline
/// counts): "shader libraries missing (left to a worker)" there is a count, not something it needs.
/// Lowercased already.
fn stats_line(l: &str) -> bool {
    const WORDS: [&str; 12] = ["vk_pipeline", "frametime", "fast links", "lookups", "pipelines swapped", "cache hits", "shader cache", "fps", "frames in", " ms,", "telemetry", "[stats]"];
    WORDS.iter().any(|w| l.contains(w))
}

/// A line from what runs a game on Linux, not from the game: Proton and its fixes, umu, Wine,
/// Steam's runtime, and the loader skipping Steam's 32-bit overlay ("ProtonFixes WARN: Game title
/// not found in CSV", "Proton: Error: unable to use parent for game drive"). Never the reason a
/// game didn't start. Takes a lowercased line.
fn runtime_line(l: &str) -> bool {
    const WORDS: [&str; 10] = ["protonfixes", "proton:", "umu-launcher", "umu:", "pressure-vessel", "steam-runtime", "steamrt", "gameoverlayrenderer", "wine: ", "fsync:"];
    WORDS.iter().any(|w| l.contains(w)) || (l.contains("ld.so: object") && l.contains("ignored"))
}

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
    start_game_with_steam(app, id, None)
}

fn start_game_with_steam(app: &AppHandle, id: &str, _steam: Option<&sysenv::SteamLaunch>) -> Res<()> {
    let st = app.state::<App>();
    // One game of a recomp that carries several: the recomp starts, with that game's options.
    let pick = game_of(&st, id);
    let full_id = id.to_string();
    let id: &str = pick.as_ref().map(|p| p.recomp.as_str()).unwrap_or(id);
    read_screen(app);
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
    let mut manual = g.manual.clone();
    // A game whose release is its installer, with a new version to build: its setup runs even when
    // it's set up by hand (that only says how it starts). A program picked by hand in what the
    // setup leaves behind while it builds (a temp folder) is let go: the setup finds the new one.
    let installer = boot::of_build(&st.dir, id, boot::windows_build(g.proton)).map(|b| b.is_installer()).unwrap_or(false);
    let rebuild = installer && manual.is_some() && !g.setup_done && g.setup.is_some();
    if rebuild {
        let names: Vec<&str> = [g.name.as_str(), g.repo.rsplit(['/', ':']).next().unwrap_or("")].into_iter().filter(|n| !n.is_empty()).collect();
        let leftover = manual.as_ref().and_then(|m| m.exe.as_ref()).map(|e| { let rel = e.strip_prefix(&g.path).unwrap_or(e); install::looks_like_tool(rel, &names) }).unwrap_or(false);
        if leftover && manual.as_ref().map(|m| m.how == "none" || m.how.is_empty()).unwrap_or(false) {
            trail::note(format!("Play {id}: set up by hand on a program in its setup's leftovers; the setup builds the new version and the launcher finds it"));
            if let Some(x) = st.lib.lock().unwrap().installed.get_mut(id) {
                x.manual = None;
            }
            st.save();
            manual = None;
        }
    }
    // A recomp the launcher knows how to set up: the player's copy goes where the game reads it,
    // and the game starts straight away instead of its own launcher.
    let whole = if manual.is_some() { None } else { profile_of(&st, id, &g) };
    // One game of several: only its inputs, and its own saves when it has them.
    let prof = whole.clone().map(|mut p| {
        if let Some(pk) = &pick {
            games::scope(&mut p, &pk.game, &pk.all);
        }
        p
    });
    if let Some(p) = prof.as_ref().filter(|p| !p.inputs.is_empty()) {
        inputs::validate(p).map_err(Error::Msg)?;
        let ready = match (&pick, whole.as_ref()) {
            (Some(pk), Some(w)) => game_inputs_ready(p, w, &g, &pk.game, &st.dir),
            _ => inputs::ready(p, &g, &st.dir),
        };
        if !ready {
            return Err(Error::Msg("NEED_COPY|Finish choosing this game's files in Easy Setup.".into()));
        }
        let mut staged = g.clone();
        inputs::stage(p, &mut staged, &st.dir).map_err(Error::Msg)?;
        // DLSS wanted but not fetched yet (setup ran offline, or it was removed): fetched in the
        // background for next time. This start goes on without it.
        if p.inputs.iter().any(|i| i.role == "runtime" && !g.runtime_off.contains(&i.id)) && dlss::current(&st.dir).is_none() {
            let a = app.clone();
            tauri::async_runtime::spawn(async move {
                let st = a.state::<App>();
                let _ = dlss::ensure(&st.http, &st.dir, &|_, _| {}).await;
            });
        }
    }
    // Its own installer or first-time setup, the first time: that opens instead of the game.
    // (A developer's own profile sets the game up itself; one made from the catalog's boot profile
    // still runs the release's setup first.)
    if (manual.is_none() || rebuild) && !prof.as_ref().map(|p| p.start.is_some() && !p.from_boot).unwrap_or(false) {
        // Only its installer so far: Set Up runs it first.
        if !g.installer_setup.is_empty() && g.exe.is_none() {
            return Err(Error::Msg("Its installer hasn't run yet. Press Set Up on its page.".into()));
        }
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
    // Settings on Match My Screen: the monitor's size now, when the monitor changed since it was
    // last written (a size picked in the game's own menu stays otherwise).
    let monitor = screen_now().map(|(w, h)| format!("{w}x{h}")).unwrap_or_default();
    if let (Some(p), false, false) = (prof.as_ref(), g.screen_fit.is_empty(), monitor.is_empty() || monitor == g.screen_size) {
        for k in p.follow_screen(&g.path, &g.screen_fit) {
            trail::note(&format!("Settings: {k} set to this screen ({monitor})"));
        }
        if let Some(x) = st.lib.lock().unwrap().installed.get_mut(id) {
            x.screen_size = monitor.clone();
        }
        st.save();
    }
    let mut args: Vec<String> = vec![];
    let mut game_env: Vec<(String, String)> = vec![];
    note_pad();
    let planned = match &prof {
        _ if manual.is_some() => {
            let m = manual.as_ref().unwrap();
            // Set up by hand without game files of its own ("the game asks"), on a game whose
            // profile hands the copy over in a variable (files.how "env"):
            // the copy chosen in Easy Setup still goes in it. Otherwise the game would start with
            // nothing and say its data was not found.
            if m.copy.is_none() && (m.how.is_empty() || m.how == "none") {
                let p = profile_of(&st, id, &g);
                let by_env = p.as_ref().and_then(|p| p.start.as_ref()).is_some_and(|s| s.env.values().any(|v| v.contains("{game}")));
                match (g.game_copy.clone().filter(|c| c.exists()), p.filter(|_| by_env)) {
                    (Some(c), Some(p)) => {
                        game_env = p.env_vars(&g.path, Some(&c));
                        trail::note(format!("Play {id}: set up by hand, its copy still goes in {}", game_env.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(", ")));
                    }
                    // No copy chosen: the player set it up themselves (some games prepare their
                    // own data), so it starts as it is. Set up by hand never stops a start.
                    _ => {}
                }
            }
            manual_plan(&g, m)?
        }
        Some(p) if p.start.is_some() => {
            let copy = game_copy(&st, id, &g, p)?;
            p.prepare(&g.path, copy.as_deref()).map_err(|e| Error::Msg(format!("Couldn't write its settings: {e}")))?;
            p.place(&g.path, copy.as_deref()).map_err(|e| Error::Msg(format!("Couldn't put your copy of the game where it reads it: {e}")))?;
            // Everything the game reads has to be there now; if not, the copy picked isn't the
            // right folder (the files are one level down, or it's a different disc): pick again.
            let mut missing = p.missing_after_place(&g.path);
            // The files a level or two down in the folder picked (the folder around them, or one
            // named after the game's ID): that inner folder is the copy, so the game isn't asked
            // for again and again.
            if let (Some(c), false) = (copy.as_deref(), missing.is_empty()) {
                let want: Vec<String> = p.game.as_ref().map(|gc| gc.contains.iter().filter(|x| !x.contains(['*', '/', '\\'])).map(|x| x.to_lowercase()).collect()).unwrap_or_default();
                if let Some(inner) = folder_with(c, &want, 3) {
                    trail::note(format!("Play {id}: the game's files are in {}, one level down", inner.display()));
                    if let Some(x) = st.lib.lock().unwrap().installed.get_mut(id) {
                        x.game_copy = Some(inner.clone());
                    }
                    st.save();
                    p.place(&g.path, Some(inner.as_path())).map_err(|e| Error::Msg(format!("Couldn't put your copy of the game where it reads it: {e}")))?;
                    missing = p.missing_after_place(&g.path);
                }
            }
            if copy.is_some() && !missing.is_empty() {
                trail::note(format!("Play {id}: still missing after placing the copy: {}", missing.join(", ")));
                if let Some(x) = st.lib.lock().unwrap().installed.get_mut(id) {
                    x.game_copy = None;
                }
                st.save();
                let label = p.game.as_ref().map(|c| c.label.clone()).unwrap_or_default();
                // What wasn't in it goes along, so the setup can say which files to look for.
                return Err(Error::Msg(format!("NEED_COPY|{label}|{}", missing.join(", "))));
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
            // Its program under another name than the profile's (a Linux build): the one the game
            // starts with, still given the copy and the rest of the profile's command line.
            p.command_for(&g.path, copy.as_deref(), &g.picks, is_windows_build(&g)).or_else(|| {
                let e = g.exe.as_ref().map(|e| g.path.join(e)).filter(|e| e.is_file())?;
                trail::note(format!("Play {id}: the profile's program isn't here, its arguments go to {}", e.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
                Some((e, p.args_for(&g.path, copy.as_deref(), &g.picks)))
            })
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
    let picked_once = once.is_some();
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
            let exe_rel = g.exe.clone().ok_or_else(|| Error::Msg("PICK_EXE|The launcher couldn't tell which program starts the game.".into()))?;
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
    // The game picked in a recomp that carries several: its options come after the recomp's own.
    if let Some(pk) = pick.as_ref().filter(|_| !picked_once) {
        games::add_args(&mut args, &pk.game);
    }
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
            // "only": a game with one way in, looked at after the run all the same (did ReShade start?).
            look_route = if cfg!(windows) { Some(route.unwrap_or("only")) } else { None };
            match if cfg!(windows) { looks::prepare(&st.dir, id, &exe, look, g.look_amount, lines, api) } else { looks::prepare_vkbasalt(&st.dir, id, look, g.look_amount, lines) } {
                Ok(r) => Some(r),
                Err(e) => {
                    let _ = app.emit("toast", &e);
                    ingame::notify(app, "The shader is off this time", &e);
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
    // A Mac app picked as the program (Game.app): the program inside it, as its Info.plist names it.
    let exe = if assets::os_now() == "macos" && macos::is_bundle(&exe) {
        match macos::bundle_executable(&exe, assets::arch_now()) {
            Some(inner) => inner,
            None => return Err(Error::Msg(format!("{} doesn't have a program this Mac can run. It may be built for another processor.", exe.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()))),
        }
    } else {
        exe
    };
    // A game of several with its own saves: its own copies (by its full id), else the recomp's.
    let save_id = pick.as_ref().filter(|p| !p.game.saves.is_empty()).map(|_| full_id.clone()).unwrap_or_else(|| id.to_string());
    let watch = {
        let spots = cloud::spots_of(&st, &save_id, &g);
        if matches!(spots.first().map(|s| s.from), Some("game") | Some("found")) {
            let exe_name = exe.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let repo_name = g.repo.rsplit(['/', ':']).next().unwrap_or("").to_string();
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
    // A Windows script that only starts the game's program (Play.bat: "Game.exe --flag"): on Linux
    // that program goes through Proton itself, with the script's arguments. A script through
    // Proton's own launcher doesn't always start.
    let exe = match (!cfg!(windows)).then(|| bat_target(&exe)).flatten() {
        Some((target, mut first)) => {
            trail::note(format!("Play {id}: {} starts {}; that goes through Proton itself", exe.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), target.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
            first.append(&mut args);
            args = first;
            target
        }
        None => exe,
    };
    // A Python script starts through its interpreter.
    let ext = exe.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    // An AppImage in the Flatpak: unpacked once (no FUSE in the sandbox) and started from there.
    // If it won't unpack, it unpacks itself for this start below, as before.
    let unpacked = if cfg!(target_os = "linux") && ext == "appimage" && flatpak::sandboxed() {
        if !flatpak::is_unpacked(&exe, id) {
            trail::note(format!("Play {id}: unpacking its AppImage once"));
            let _ = app.emit("note", "Unpacking the game the first time it's played. Later starts are quicker.");
        }
        match flatpak::unpacked_appimage(&exe, id) {
            Ok(dir) => Some(dir),
            Err(e) => {
                trail::note(format!("Play {id}: the AppImage didn't unpack ({e}); it unpacks itself this time"));
                None
            }
        }
    } else {
        None
    };
    let mut cmd = match ext.as_str() {
        _ if unpacked.is_some() => tokio::process::Command::new(unpacked.as_ref().map(|d| d.join("AppRun")).unwrap_or_default()),
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
        // The same for a start script without a "#!" line (or an extension): Linux won't run one
        // on its own ("Exec format error").
        "sh" if !cfg!(windows) => shell_script(&exe),
        // A Windows script (.bat, .cmd) has no "#!" line either, but it's for Windows' own command
        // prompt: it goes through Proton below, never the shell.
        _ if !cfg!(windows) && !matches!(ext.as_str(), "exe" | "cmd" | "bat" | "flatpak") && headless_script(&exe) => {
            trail::note(format!("Play {id}: a start script without a #! line, through the shell"));
            shell_script(&exe)
        }
        // A Flatpak bundle: Linux can't run the file itself. Installed for this user the first
        // time (with what it needs from Flathub), then started through Flatpak.
        "flatpak" if !cfg!(windows) => {
            if flatpak::host_flatpak_blocked() {
                return Err(Error::Msg("This build comes as a Flatpak, and the RecompLauncher Flatpak is not allowed to install it. Allow it with: flatpak override --user --talk-name=org.freedesktop.Flatpak com.recomplauncher.RecompLauncher. Or switch to another build on the game page.".into()));
            }
            if !flatpak::available() {
                return Err(Error::Msg("This build comes as a Flatpak, and Flatpak isn't on this PC. Install Flatpak from your system's software center, or switch to another build on the game's page.".into()));
            }
            let app_id = flatpak::app_id(&exe).ok_or_else(|| Error::Msg("This Flatpak file doesn't say which app it holds, so it can't be installed. Switch to another build on the game's page.".into()))?;
            if !flatpak::installed(&app_id) {
                trail::note(format!("Play {id}: installing its Flatpak ({app_id})"));
                let _ = app.emit("note", "Installing its Flatpak for the first time. It can take a few minutes; the game starts when it's done.");
                if let Err(e) = flatpak::install(&exe) {
                    trail::note(format!("Play {id}: its Flatpak didn't install: {e}"));
                    return Err(Error::Msg(format!("Flatpak couldn't install it: {e}")));
                }
                trail::note(format!("Play {id}: its Flatpak is installed"));
            }
            flatpak::command(&app_id)
        }
        // A Windows program (or script) on Linux: through Proton (or Wine).
        "exe" | "cmd" | "bat" if !cfg!(windows) => match proton::runner() {
            Some(r) => proton::command(&r, &st.dir, &exe),
            None => return Err(Error::Msg("This is a Windows version, and it needs Proton to play on Linux. Settings → General → GE-Proton installs it in one press, then press Play again.".into())),
        },
        _ => tokio::process::Command::new(&exe),
    };
    sysenv::clean_tokio(&mut cmd);
    // Started from a Steam shortcut: Steam's overlay goes back for the game, so Steam Link and
    // Remote Play can stream its window.
    #[cfg(target_os = "linux")]
    if !matches!(ext.as_str(), "exe" | "cmd" | "bat") {
        sysenv::steam_overlay_for_game(&mut cmd, _steam);
    }
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
    // An AppImage on a Linux without FUSE 2 (libfuse.so.2, missing on many new systems): it unpacks
    // itself and runs from there instead of failing to start.
    #[cfg(target_os = "linux")]
    if unpacked.is_none() {
        appimage_env(&mut cmd, &exe);
    }
    #[cfg(target_os = "linux")]
    if let Some(dir) = &unpacked {
        // What an AppImage's own runtime would have set, for games that look for their files by it.
        cmd.env("APPIMAGE", &exe).env("APPDIR", dir).env("ARGV0", &exe);
        trail::note(format!("Play {id}: in the Flatpak, from its unpacked AppImage"));
    } else if ext == "appimage" && flatpak::sandboxed() {
        // Inside the Flatpak there is no fusermount, so no AppImage can mount itself. It unpacks
        // on disk (the sandbox's /tmp is memory, and a game can be several GB).
        cmd.env("APPIMAGE_EXTRACT_AND_RUN", "1");
        if let Some(dir) = flatpak::scratch_dir() {
            cmd.env("TMPDIR", dir);
        }
        trail::note(format!("Play {id}: in the Flatpak the AppImage unpacks itself to run"));
    } else if ext == "appimage" && !sysenv::has_fuse2() {
        cmd.env("APPIMAGE_EXTRACT_AND_RUN", "1");
        trail::note(format!("Play {id}: no FUSE 2 on this PC, the AppImage unpacks itself to run"));
    }
    // Recomp Achievements (test accounts): the game's channel, named in RL_SESSION.
    let ach_session = {
        let cat = cached_catalog(&st);
        let entry = catalog::app(&cat, id);
        let origin = entry.map(|e| str_of(e, "origin").to_string()).unwrap_or_default();
        let folder = entry.and_then(|e| e.get("achievements_folder").and_then(Value::as_str).or_else(|| e.pointer("/achievements/folder").and_then(Value::as_str))).map(str::to_string);
        let mut roots = vec![g.path.clone()];
        if let Some(p) = g.path.parent() {
            roots.push(p.join("Game Copies").join(library::folder_name(id)));
        }
        let title = entry.and_then(|e| e.get("title_id").and_then(Value::as_str).or_else(|| e.pointer("/achievements/title_id").and_then(Value::as_str))).map(str::to_string);
        // One game of several: its own list (by its full id), and only its own unlock file.
        match &pick {
            Some(pk) => {
                let title = Some(pk.game.title_id.clone()).filter(|t| !t.is_empty());
                ach::start(app, ach::Start { id: &full_id, name: &pk.game.name, version: &g.tag, install: &g.path, origin: &origin, prof: prof.as_ref(), catalog_folder: folder, title, roots })
            }
            None => ach::start(app, ach::Start { id, name: &g.name, version: &g.tag, install: &g.path, origin: &origin, prof: prof.as_ref(), catalog_folder: folder, title, roots }),
        }
    };
    if let Some(p) = &ach_session {
        cmd.env("RL_SESSION", p);
    }
    note_run(&st, id, &g, &exe, &args, &cwd, prof.as_ref(), look_run.is_some());
    trail::note(format!("Play {id}: {}{}", exe.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), if look_run.is_some() { " with a Look" } else { "" }));
    let spawned = cmd.spawn();
    // A program that has to run as administrator (Windows says so, error 740): Windows asks the
    // player, then it starts. A setup goes through the setup steps, which find the game after.
    #[cfg(windows)]
    if let Err(e) = &spawned {
        if e.raw_os_error() == Some(740) {
            trail::note(format!("Play {id}: needs administrator, asking Windows"));
            if install::looks_like_setup(&exe_rel) {
                return own_setup::run(app, id, &g, &exe, None);
            }
            let line = args.iter().map(|a| if a.contains(' ') || a.is_empty() { format!("\"{a}\"") } else { a.clone() }).collect::<Vec<_>>().join(" ");
            return win::run_as_admin(&exe, &line, &cwd).map(|_| ()).map_err(|e| Error::Msg(format!("{shown} has to run as administrator, and Windows didn't start it: {e}")));
        }
    }
    if let Err(e) = &spawned {
        trail::note(format!("Play {id}: didn't start: {e}"));
    }
    if spawned.is_err() {
        if ach_session.is_some() {
            ach::abort(app, &full_id);
        }
        if let Some(p) = look_run.as_ref().and_then(|r| r.placed.as_ref()) {
            looks::finish(&st.dir, id, p);
        }
    }
    let mut child = spawned.map_err(|e| {
        if is_blocked(&e) {
            defender_msg(e, &shown)
        } else if ext == "py" && e.kind() == std::io::ErrorKind::NotFound {
            // A Python game, and Python isn't on this PC (Windows' py launcher comes with it).
            Error::Msg(format!("{shown} is a Python program, and Python 3 isn't on this PC. Install it from python.org (on Windows, tick \"Add python.exe to PATH\" in its installer), then press Play again."))
        } else if matches!(e.raw_os_error(), Some(4551) | Some(4556)) && cfg!(windows) {
            // ERROR_SYSTEM_INTEGRITY_POLICY_VIOLATION: Smart App Control (or a work or school
            // policy) blocks programs that aren't signed. It has no list of allowed programs.
            Error::Msg(format!("Smart App Control blocked {shown}, because the program isn't signed. Windows can't allow one game at a time: to play it, turn Smart App Control off in Windows Security → App & browser control → Smart App Control settings (Windows only turns it back on after a reset). On a work or school PC, ask whoever manages it."))
        } else if e.raw_os_error() == Some(1455) && cfg!(windows) {
            // ERROR_COMMITMENT_LIMIT: memory and the page file together are full.
            Error::Msg(format!("Windows ran out of memory to start {shown}. Close other games and apps, and check that the page file is on (Advanced system settings → Performance → Virtual memory), then press Play again."))
        } else if e.raw_os_error() == Some(5) && cfg!(windows) {
            // Access denied on starting it: an antivirus holding the file, most often.
            Error::Msg(format!("Windows didn't let {shown} start. Your antivirus may be blocking it: allow the game's folder in it (or restore the file if it was quarantined), then press Play again."))
        } else if let Some(why) = not_a_program(&exe).filter(|_| matches!(e.raw_os_error(), Some(8) | Some(193))) {
            // A disc image, an archive or a document set as the program (by hand, most often).
            Error::Msg(why.into())
        } else if !cfg!(windows) && e.raw_os_error() == Some(8) {
            Error::Msg(format!("{shown} isn't built for this PC's processor (Exec format error). Pick the build for your system on the game's page, or choose another program in Troubleshoot."))
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
    // Which of its games is being played (a recomp that carries several), for the window.
    let game_full = pick.as_ref().map(|_| full_id.clone());
    let game_sub = pick.as_ref().map(|p| p.game.id.clone());
    PLAYING_GAME.lock().unwrap().get_or_insert_with(HashMap::new).insert(id.clone(), full_id.clone());
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
    // The launcher's own process id: a marker whose process is gone is stale (see marker_alive).
    let _ = std::fs::write(&marker, std::process::id().to_string());
    ingame::set_hotkeys(app, true, quick_menu);
    let _ = app.emit("game", json!({"id": id, "game": game_full, "state": "running"}));
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
    // The player's own icon (Customize) on the game's window and taskbar button, once its window
    // is up. Looked for a few times, since games take a while to open their window.
    #[cfg(windows)]
    if let Some(png) = g.icon_file.as_ref().filter(|f| f.extension().map(|e| e.eq_ignore_ascii_case("png")).unwrap_or(false)).and_then(|f| std::fs::read(f).ok()) {
        let dir = if exe.starts_with(&g.path) { g.path.clone() } else { exe.parent().map(Path::to_path_buf).unwrap_or_else(|| g.path.clone()) };
        let (a, idc) = (app.clone(), id.clone());
        std::thread::spawn(move || {
            for wait in [3u64, 5, 7, 15, 30] {
                std::thread::sleep(std::time::Duration::from_secs(wait));
                if !a.state::<App>().running.lock().unwrap().contains_key(&idc) {
                    break;
                }
                win::set_window_icon(&dir, &png);
            }
        });
    }
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
        // One game of several: that game, by name.
        let (game, cover) = match &pick {
            Some(pk) => (pk.game.name.clone(), if pk.game.cover.starts_with('/') { format!("{}{}", catalog::base(), pk.game.cover) } else if pk.game.cover.is_empty() { cover } else { pk.game.cover.clone() }),
            None => (game, cover),
        };
        discord::send(discord::Msg::Playing { app_id, game, recomp: g.name.clone(), cover, start: started });
    }
    // The folder its programs run from, for a game that hands off to another program and closes.
    let handoff_dir = if exe.starts_with(&g.path) { g.path.clone() } else { exe.parent().map(Path::to_path_buf).unwrap_or_else(|| g.path.clone()) };
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let start = Instant::now();
        let mut stopped = false;
        let code = tokio::select! {
            r = child.wait() => r.ok().and_then(|s| s.code()),
            _ = rx => { stopped = true; let _ = child.kill().await; None }
        };
        // Some games start through a small program that opens the game and closes (a launcher, an
        // updater): it's still being played while a program from its folder runs. Play time, and
        // the game showing as running, last until the last one closes. Only when what was started
        // closed within a few minutes: that's a hand-off, not a game that was played and quit.
        if !stopped && start.elapsed().as_secs() < 180 {
            let dir = handoff_dir.clone();
            let mut waited = false;
            while start.elapsed().as_secs() < 24 * 3600 && tokio::task::spawn_blocking({ let d = dir.clone(); move || still_running_in(&d) }).await.unwrap_or(false) {
                if !waited {
                    waited = true;
                    trail::note(format!("{id}: its program closed, but the game runs on from its folder"));
                }
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
        let secs = start.elapsed().as_secs();
        let st = app2.state::<App>();
        // A game that closes with the same code every time it's quit normally (its profile or the
        // catalog says which): that's a normal close, not a crash.
        let code = code.map(|c| if quit_code(&st, &id, prof.as_ref(), c) { 0 } else { c });
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
            ach::end(&app2, game_full.as_deref().unwrap_or(&id));
            if let Some(m) = PLAYING_GAME.lock().unwrap().as_mut() {
                m.remove(&id);
            }
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
            // And the game itself, when it's one of several.
            if let Some(sub) = &game_sub {
                g.games.entry(sub.clone()).or_default().add(started, secs, now());
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
        let said = code.and_then(|c| {
            let own = prof.as_ref().and_then(|p| p.exit_codes.get(&(c as u32).to_string()).or_else(|| p.exit_codes.get(&c.to_string())).cloned());
            // Else the catalog's profile ("exit_codes" in its boot), fixable without a launcher update.
            own.or_else(|| catalog::app(&cached_catalog(&st), &id).and_then(|e| boot::exit_says(e, c)))
        }).filter(|t| !t.trim().eq_ignore_ascii_case("ok"));
        if mine {
            let _ = app2.emit("game", json!({"id": id, "game": game_full, "state": "stopped", "secs": secs, "code": code, "started": started, "said": said}));
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
                usage::profile_event(&app2, &id, "start", false, &format!("closed with code {} after {secs} s", code.unwrap_or(0))).await;
            } else if secs >= 30 {
                usage::event(&app2, e("session")).await;
                usage::profile_event(&app2, &id, "start", true, "").await;
            }
        }
        if secs >= 5 {
            match cloud::after_session(&app2, &save_id).await {
                Ok(Some(_)) => {
                    let _ = app2.emit("saves", json!({"id": save_id}));
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
// A cancelled named-input worker may still be finishing a file copy. Keep its mutation lock
// until it actually exits, rather than allowing an immediate retry to race its staging journal.
static INPUT_WORKERS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn setup_busy(id: &str) -> bool {
    SETUP_RUN.lock().unwrap().as_ref().is_some_and(|m| m.contains_key(id)) || INPUT_WORKERS.lock().unwrap().iter().any(|x| x == id)
}

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
    if !p.inputs.is_empty() { return Err(Error::Msg("Set this game up from its Easy Setup screen.".into())); }
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
            if let Some(m) = SETUP_RUN.lock().unwrap().as_mut() {
                if m.get(&id) == Some(&run) {
                    m.remove(&id);
                }
            }
        };
        // 1. Is it the right game?
        emit(SetupEv { step: "read".into(), ..Default::default() });
        if !copy.exists() {
            return fail("That file isn't there anymore.".into());
        }
        if let Some(need) = &p.game {
            if let Err(e) = copy_ok(need, &copy, &g.path, true) {
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
                // Any case: discs name it default.xex or Default.xex, and Linux tells them apart.
                let has = dest.join(&f).is_file() || std::fs::read_dir(&dest).into_iter().flatten().flatten().any(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(&f));
                if !has {
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

/// An AppImage unpacked at install starts from its AppRun, which may look for its files through
/// APPDIR (the AppImage's runtime sets it when the AppImage starts itself): set it here.
#[cfg(target_os = "linux")]
fn appimage_env(cmd: &mut tokio::process::Command, exe: &Path) {
    if exe.file_name().map(|n| n == "AppRun").unwrap_or(false) {
        if let Some(dir) = exe.parent() {
            cmd.env("APPDIR", dir).env("ARGV0", exe);
        }
    }
}

/// Run a recomp's prepare program, reading `progress 0.4 text` lines from it.
fn run_prepare(pr: &profile::Prepare, install: &Path, copy: &Path, report: &dyn Fn(f32, String), stopped: &dyn Fn() -> bool) -> Result<(), String> {
    use std::io::BufRead;
    let exe = pr.program.iter().map(|x| install.join(x)).find(|x| x.is_file()).ok_or("Its setup program is missing. Reinstall the game.")?;
    let args: Vec<String> = pr.args.iter().map(|a| profile::fill(a, install, Some(copy))).collect();
    let mut c = std::process::Command::new(&exe);
    sysenv::clean_std(&mut c);
    // An unpacked AppImage's AppRun, run for its setup step: what its runtime would have set.
    if exe.file_name().map(|n| n == "AppRun").unwrap_or(false) {
        if let Some(dir) = exe.parent() {
            c.env("APPDIR", dir).env("ARGV0", &exe);
        }
    }
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
                    (p.game.is_some() || !p.inputs.is_empty()).then(|| id.to_string())
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
        if auto_update == Some(true) && g.local {
            return Err(Error::Msg("Untracked installations don't get automatic updates. Choose a project first.".into()));
        }
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
    /// Lines for the CRT shaders.
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
        "Shaders work on Windows and Linux.".to_string()
    } else if !cfg!(windows) && !looks::vkbasalt_installed() {
        VKBASALT_MISSING.to_string()
    } else if let Some(pl) = pl.as_ref().filter(|p| !p.allowed) {
        if pl.reason.is_empty() { "This game doesn't work with shaders.".into() } else { pl.reason.clone() }
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
        return Err(Error::Msg("That shader isn't one the launcher has.".into()));
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

pub(crate) fn cached_catalog(st: &App) -> Value {
    let cat = std::fs::read(st.dir.join("cache/pc-catalog.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    catalog::with_mine(&st.dir, cat)
}

/// What a GitHub repo is, read before it's added: the catalog's entry when it has one; else the
/// game and console its description, topics and README point at, a catalog game it looks like, and
/// art from SteamGridDB when the player gave their key.
#[tauri::command]
async fn github_lookup(st: State<'_, App>, link: String, sgdb_key: Option<String>) -> Res<Value> {
    let repo = catalog::repo_from_link(&link).ok_or_else(|| Error::Msg("That isn't a GitHub, GitLab or SourceForge link. It looks like github.com/owner/name, gitlab.com/group/name or sourceforge.net/projects/name.".into()))?;
    let cat = catalog::catalog(&st.http, &st.dir, false).await?.data;
    let apps = cat.get("apps").and_then(Value::as_array).cloned().unwrap_or_default();
    if let Some(a) = apps.iter().find(|a| str_of(a, "repo").eq_ignore_ascii_case(&repo)) {
        return Ok(json!({"repo": repo, "known": {"id": str_of(a, "id"), "name": str_of(a, "name")}}));
    }
    let os = github_os(&st, &repo).await?;
    // A GitLab or SourceForge project: only its releases are read (through the site), so its name
    // stands in for what GitHub would say about it.
    let lab = catalog::site_only(&repo);
    let api = if lab { String::new() } else { format!("https://api.github.com/repos/{repo}") };
    let get = |url: String, raw: bool| {
        let mut r = st.http.get(url).timeout(std::time::Duration::from_secs(10));
        if raw {
            r = r.header("Accept", "application/vnd.github.raw");
        }
        r.send()
    };
    let (info, readme) = if lab {
        (Value::Null, String::new())
    } else {
        let (info, readme) = tokio::join!(get(api.clone(), false), get(format!("{api}/readme"), true));
        let info: Value = match info {
            Ok(r) if r.status().is_success() => r.json().await.unwrap_or(Value::Null),
            _ => Value::Null,
        };
        let readme = match readme {
            Ok(r) if r.status().is_success() => r.text().await.unwrap_or_default(),
            _ => String::new(),
        };
        (info, readme)
    };
    let path = catalog::host_path(&repo).to_string();
    let desc = str_of(&info, "description").to_string();
    let topics: Vec<String> = info.get("topics").and_then(Value::as_array).map(|l| l.iter().filter_map(|t| t.as_str().map(str::to_string)).collect()).unwrap_or_default();
    // The game: a catalog game the repo names in its own words first, then what its description,
    // README and name point at. With a SteamGridDB key, the first of those SteamGridDB knows,
    // under its name there.
    let cands = ghinfo::game_candidates(&path, &desc, &readme);
    let hay = format!(" {} ", ghinfo::norm(&format!("{} {} {}", path.replace(['/', '-', '_'], " "), desc, readme.chars().take(1500).collect::<String>())));
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
        "name": catalog::name_from_repo(&path),
        "game": game.unwrap_or_default(),
        "origin": origin,
        "blurb": desc,
        "similar": similar.map(|a| json!({"id": str_of(a, "id"), "name": str_of(a, "name")})),
        "art": art,
        "art_error": art_error,
        "hero_default": if lab { String::new() } else { format!("https://opengraph.githubassets.com/1/{repo}") },
        "host": if lab { "gitlab" } else { "github" },
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
    let has = |os: &str| rels.iter().any(|r| !r.draft && assets::pick(&r.assets, os, arch, None).is_some());
    if has(here) {
        Ok(here)
    } else if here == "linux" && (has("windows") || rels.iter().any(|r| !r.draft && assets::pick(&r.assets, "windows", "x64", None).is_some())) {
        // A Windows-only one on Linux: added as that, and it plays through Proton (its page offers
        // to turn Proton on when it's off).
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
    let repo = catalog::repo_from_link(&link).ok_or_else(|| Error::Msg("That isn't a GitHub, GitLab or SourceForge link. It looks like github.com/owner/name, gitlab.com/group/name or sourceforge.net/projects/name.".into()))?;
    let cat = catalog::catalog(&st.http, &st.dir, false).await?.data;
    let same = |a: &Value| str_of(a, "repo").eq_ignore_ascii_case(&repo);
    if let Some(a) = cat.get("apps").and_then(Value::as_array).and_then(|l| l.iter().find(|a| same(a))) {
        return Ok(json!({"id": str_of(a, "id"), "known": true, "name": str_of(a, "name")}));
    }
    let os = github_os(&st, &repo).await?;
    let rels: Vec<Release> = serde_json::from_value(catalog::releases(&st.http, &st.dir, &repo, false).await?.data).unwrap_or_default();
    // What the repo says about itself, when GitHub answers (it's fine without). A GitLab
    // project's (or a SourceForge one's) releases are all that's read of it.
    let lab = catalog::site_only(&repo);
    let meta: Value = if lab {
        Value::Null
    } else {
        match st.http.get(format!("https://api.github.com/repos/{repo}")).timeout(std::time::Duration::from_secs(10)).send().await {
            Ok(r) if r.status().is_success() => r.json().await.unwrap_or(Value::Null),
            _ => Value::Null,
        }
    };
    let path = catalog::host_path(&repo).to_string();
    let owner = path.split('/').next().unwrap_or("").to_string();
    let id: String = format!("{}-{}", if catalog::is_sourceforge(&repo) { "sf" } else if lab { "gl" } else { "gh" }, path.to_lowercase().replace('/', "-")).chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' }).collect();
    let newest = rels.iter().find(|r| !r.draft);
    let mut entry = json!({
        "id": id,
        "name": catalog::name_from_repo(&path),
        "repo": repo,
        "author": owner,
        "blurb": str_of(&meta, "description"),
        "systems": [os],
        "hero": if lab { String::new() } else { format!("https://opengraph.githubassets.com/1/{repo}") },
        "released": newest.and_then(|r| r.published_at.clone()).unwrap_or_default(),
        "stars": meta.get("stargazers_count").cloned().unwrap_or(Value::Null),
        "custom": true,
    });
    if lab {
        entry["host"] = json!("gitlab");
    }
    if let Some(i) = info.as_ref().and_then(Value::as_object) {
        set_mine_fields(&mut entry, i);
    }
    {
        let _turn = catalog::MINE_TURN.lock().unwrap_or_else(|e| e.into_inner());
        let mut list = catalog::mine(&st.dir);
        list.retain(|e| str_of(e, "id") != id);
        list.push(entry.clone());
        catalog::save_mine(&st.dir, &list)?;
    }
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
    let _turn = catalog::MINE_TURN.lock().unwrap_or_else(|e| e.into_inner());
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
    let _turn = catalog::MINE_TURN.lock().unwrap_or_else(|e| e.into_inner());
    let mut list = catalog::mine(&st.dir);
    list.retain(|e| str_of(e, "id") != id);
    catalog::save_mine(&st.dir, &list)?;
    art::drop_set(&st, &id);
    Ok(())
}

/// A recomp's games (games.rs): its developer's list (the file in its release, or the catalog's
/// launcher profile), else the catalog entry's. Fewer than two: none.
pub(crate) fn games_of(st: &App, id: &str, g: &Installed) -> Vec<games::Game> {
    let cat = cached_catalog(st);
    let entry = catalog::app(&cat, id).cloned().unwrap_or(Value::Null);
    let own = profile::release_profile(&g.path).and_then(|r| r.profile.games).or_else(|| entry.pointer("/launcher/games").cloned());
    games::of(own.as_ref(), &entry)
}

/// One game of a recomp that carries several, by its full id (cogbox--gow2): the recomp's id, its
/// install, the game and all its games. None for anything else (an installed id always wins).
pub(crate) struct GamePick {
    pub recomp: String,
    pub g: Installed,
    pub game: games::Game,
    pub all: Vec<games::Game>,
}

pub(crate) fn game_of(st: &App, full: &str) -> Option<GamePick> {
    let (recomp, sub) = games::split(full)?;
    let g = {
        let l = st.lib.lock().unwrap();
        if l.installed.contains_key(full) {
            return None;
        }
        l.installed.get(recomp).cloned()?
    };
    let all = games_of(st, recomp, &g);
    let game = all.iter().find(|x| x.id == sub)?.clone();
    Some(GamePick { recomp: recomp.to_string(), g, game, all })
}

/// The recomp a game id belongs to: itself for anything but one game of a recomp.
pub(crate) fn recomp_of(st: &App, id: &str) -> String {
    game_of(st, id).map(|p| p.recomp).unwrap_or_else(|| id.to_string())
}

/// One game's inputs are set up: their own setup (its fingerprint kept for that game), or the
/// recomp's whole setup.
fn game_inputs_ready(scoped: &profile::Profile, whole: &profile::Profile, g: &Installed, game: &games::Game, dir: &Path) -> bool {
    let own = g.games.get(&game.id).map(|p| p.inputs_ready.as_str()).unwrap_or("");
    (!own.is_empty() && inputs::signature(scoped, g, dir).is_ok_and(|s| s == own)) || inputs::ready(whole, g, dir)
}

/// Every installed recomp that carries several games, with them: the Library's tiles.
#[derive(Serialize)]
struct GameSet {
    games: Vec<games::Game>,
    /// By game id: its inputs are set up (or it has none to set up).
    ready: std::collections::BTreeMap<String, bool>,
}

#[tauri::command(async)]
fn recomp_games(st: State<'_, App>) -> std::collections::BTreeMap<String, GameSet> {
    let list: Vec<(String, Installed)> = st.lib.lock().unwrap().installed.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    let mut out = std::collections::BTreeMap::new();
    for (id, g) in list {
        let all = games_of(&st, &id, &g);
        if all.is_empty() {
            continue;
        }
        let whole = profile_of(&st, &id, &g);
        let ready = all
            .iter()
            .map(|game| {
                let ok = match whole.as_ref().filter(|p| !p.inputs.is_empty()) {
                    None => true,
                    Some(p) => {
                        let mut scoped = p.clone();
                        games::scope(&mut scoped, game, &all);
                        scoped.inputs.is_empty() || game_inputs_ready(&scoped, p, &g, game, &st.dir)
                    }
                };
                (game.id.clone(), ok)
            })
            .collect();
        out.insert(id, GameSet { games: all, ready });
    }
    out
}

fn profile_of(st: &App, id: &str, g: &Installed) -> Option<profile::Profile> {
    let cat = cached_catalog(st);
    let entry = catalog::app(&cat, id).cloned().unwrap_or(Value::Null);
    profile::find(&g.path, &g.repo, &entry).map(|mut p| {
        p.windows_build = Some(is_windows_build(g));
        // Optional inputs this launcher can't use (newer shared types, DLSS without an RTX card).
        inputs::prepare(&mut p, dlss::applies(&st.dir));
        p.input_paths = inputs::available(&p, g, &st.dir);
        p.slider_vals = g.slider_vals.clone();
        p.add_screen_choices(screen_now());
        p
    })
}

/// The main monitor's size in pixels, for Match My Screen: read when the launcher starts, when a
/// game's settings open, and before each Play.
static SCREEN: Mutex<Option<(u32, u32)>> = Mutex::new(None);

fn screen_now() -> Option<(u32, u32)> {
    *SCREEN.lock().unwrap()
}

fn read_screen(app: &AppHandle) {
    let m = app.primary_monitor().ok().flatten().or_else(|| app.get_webview_window("main").and_then(|w| w.current_monitor().ok().flatten())).or_else(|| app.available_monitors().ok().and_then(|l| l.into_iter().next()));
    if let Some(m) = m {
        let s = m.size();
        if s.width > 0 && s.height > 0 {
            *SCREEN.lock().unwrap() = Some((s.width, s.height));
        }
    }
}

/// Whether a code is how this game closes when it's quit normally: "ok" for it in the profile's
/// exit_codes, or listed in the catalog entry's ok_codes (or its boot profile's).
fn quit_code(st: &App, id: &str, prof: Option<&profile::Profile>, c: i32) -> bool {
    let key = (c as u32).to_string();
    if prof.and_then(|p| p.exit_codes.get(&key).or_else(|| p.exit_codes.get(&c.to_string()))).map(|s| s.trim().eq_ignore_ascii_case("ok")).unwrap_or(false) {
        return true;
    }
    let cat = cached_catalog(st);
    let Some(e) = catalog::app(&cat, id) else { return false };
    if crate::boot::exit_says(e, c).map(|s| s.trim().eq_ignore_ascii_case("ok")).unwrap_or(false) {
        return true;
    }
    let listed = [e.get("ok_codes"), e.get("boot").and_then(|b| b.get("ok_codes"))]
        .into_iter()
        .flatten()
        .filter_map(Value::as_array)
        .flatten()
        .any(|v| v.as_i64() == Some(c as i64) || v.as_i64() == Some(c as u32 as i64) || v.as_str().map(|s| s.trim() == key || s.trim() == c.to_string()).unwrap_or(false));
    listed
}

/// A copy just picked (`fresh`) that's the game's own folder, or a folder around it (the folder
/// games are installed in), isn't the player's copy of the game: refused with a plain reason. Not
/// when the game reads its files from its own folder (`into: "."`, or a disc the launcher unpacks
/// there), and never for a copy set up before (an adopted game inside the player's copy). Then
/// the copy's own checks.
fn copy_ok(need: &profile::GameCopy, copy: &Path, install: &Path, fresh: bool) -> Result<(), String> {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let (c, i) = (canon(copy), canon(install));
    let reads_own = need.into.trim().trim_matches(['/', '\\']) == "." || !need.extract.is_empty();
    if fresh && !install.as_os_str().is_empty() {
        if c == i && !reads_own {
            return Err("That's the game's own folder, where the launcher installed it. Pick your copy of the game: the disc image or the folder with the original game's files.".into());
        }
        if c != i && i.starts_with(&c) {
            return Err("That's the folder the launcher installs games into. Pick your copy of the game: the disc image or the folder with the original game's files.".into());
        }
    }
    profile::check_copy(need, copy)
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
        if let Err(why) = copy_ok(need, &c, &g.path, false) {
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
    /// The heading it's under ("" for none).
    group: String,
    /// Hidden until the player shows advanced options.
    advanced: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    slider: Option<SliderNow>,
    /// A value the game wrote itself that isn't one of the choices, as written.
    #[serde(skip_serializing_if = "Option::is_none")]
    own: Option<String>,
    /// Key bindings: each action and the keys it's on now.
    #[serde(skip_serializing_if = "Option::is_none")]
    bindings: Option<BindNow>,
    /// Its last choice is the launcher's Match My Screen, writing this size or shape.
    #[serde(skip_serializing_if = "Option::is_none")]
    screen: Option<String>,
}

#[derive(Serialize)]
struct BindNow {
    /// Keys one action can have.
    max: usize,
    /// Some action isn't on the game's own keys (Reset to Default does something).
    changed: bool,
    actions: Vec<BindRow>,
}

#[derive(Serialize)]
struct BindRow {
    label: String,
    key: String,
    keys: Vec<String>,
    pad: bool,
}

/// A slider as the window shows it: its range and step, and the value exactly as written now.
#[derive(Serialize)]
struct SliderNow {
    min: f64,
    max: f64,
    step: f64,
    value: String,
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
    inputs: Vec<inputs::Row>,
    inputs_error: Option<String>,
    inputs_ready: bool,
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
fn game_setup(app: AppHandle, st: State<'_, App>, id: String) -> Option<Setup> {
    read_screen(&app);
    // One game of several: Easy Setup asks only for its own inputs and the shared ones.
    let pick = game_of(&st, &id);
    let id = pick.as_ref().map(|p| p.recomp.clone()).unwrap_or(id);
    let g = st.lib.lock().unwrap().installed.get(&id).cloned()?;
    let whole = profile_of(&st, &id, &g)?;
    let mut p = whole.clone();
    if let Some(pk) = &pick {
        games::scope(&mut p, &pk.game, &pk.all);
    }
    let p = p;
    let cur = p.current(&g.path, &g.picks);
    let up = p.game.as_ref().and_then(|c| c.update.as_ref());
    Some(Setup {
        inputs: inputs::rows(&p, &g, &st.dir).unwrap_or_default(),
        inputs_error: inputs::validate(&p).err(),
        inputs_ready: match &pick {
            Some(pk) => game_inputs_ready(&p, &whole, &g, &pk.game, &st.dir),
            None => inputs::ready(&p, &g, &st.dir),
        },
        needs: p.game.as_ref().map(|x| x.label.clone()).or_else(|| (!p.inputs.is_empty()).then(|| "Choose the files this game needs.".into())),
        kind: p.game.as_ref().map(|x| x.kind.clone()).unwrap_or_default(),
        copy: g.game_copy.clone().filter(|c| c.exists()).or_else(|| p.already_in_place(&g.path)),
        settings: p
            .settings
            .iter()
            .zip(cur)
            .map(|(s, (i, applies))| {
                // On Match My Screen, and the screen's size is also one of the listed ones: Match My Screen.
                let last = s.choices.len().saturating_sub(1);
                let i = if s.screen.is_some() && g.screen_fit.contains(&s.id) && i != last && s.choices.get(i).map(|c| c.set == s.choices[last].set).unwrap_or(false) { last } else { i };
                // The launcher's own settings wait for the file the game makes on its first start.
                let ready = !p.from_boot || s.target == "args" || p.config_file(&s.target, &g.path).map(|f| f.existed).unwrap_or(false);
                SetupOption {
                    id: s.id.clone(),
                    label: s.label.clone(),
                    help: s.help.clone(),
                    choices: s.choices.iter().map(|c| c.label.clone()).collect(),
                    current: i,
                    applies: applies && ready,
                    group: s.group.trim().to_string(),
                    advanced: p.is_advanced(s),
                    slider: s.slider.as_ref().map(|sl| SliderNow { min: sl.min, max: sl.max, step: sl.step(), value: p.slider_now(s, &g.path).unwrap_or_default() }),
                    own: p.own_value(s, &g.path),
                    screen: s.screen.clone(),
                    bindings: s.bindings.as_ref().map(|b| BindNow {
                        max: b.max(),
                        changed: p.bindings_changed(s, &g.path),
                        actions: b.actions.iter().zip(p.bindings_now(s, &g.path)).map(|(a, keys)| BindRow { label: a.label.clone(), key: a.key.clone(), keys, pad: a.pad }).collect(),
                    }),
                }
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

/// Pick/clear a named file. Shared keys are imported only through an active project declaration.
#[tauri::command(async)]
fn setup_input_set(st: State<'_, App>, id: String, input: String, path: Option<String>) -> Res<()> {
    if setup_busy(&id) || st.running.lock().unwrap().contains_key(&id) { return Err(Error::Msg("Close the game or stop setup first.".into())); }
    let mut g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let p = profile_of(&st, &id, &g).ok_or_else(|| Error::Msg("This game has no setup profile.".into()))?;
    inputs::validate(&p).map_err(Error::Msg)?;
    let declared = p.inputs.iter().find(|i| i.id == input).ok_or_else(|| Error::Msg("Unknown setup input.".into()))?;
    let row = inputs::rows(&p, &g, &st.dir).map_err(Error::Msg)?.into_iter().find(|r| r.id == input).unwrap();
    if !row.active { return Err(Error::Msg("That input isn't required by the selected game files.".into())); }
    if !declared.shared.is_empty() {
        if st.running.lock().unwrap().len() > 0 || SETUP_RUN.lock().unwrap().as_ref().is_some_and(|m| !m.is_empty()) || !INPUT_WORKERS.lock().unwrap().is_empty() {
            return Err(Error::Msg("Close running games and stop setup before replacing shared keys.".into()));
        }
        if let Some(path) = path {
            inputs::import_key(&st.dir, &declared.shared, Path::new(&path)).map_err(Error::Msg)?;
            let games: Vec<_> = st.lib.lock().unwrap().installed.values().cloned().collect();
            for other in games.into_iter().filter(|other| other.id != id) {
                if let Some(profile) = profile_of(&st, &other.id, &other) {
                    let uses = inputs::rows(&profile, &other, &st.dir).unwrap_or_default().iter().any(|r| r.active && r.shared == declared.shared);
                    if uses {
                        if let Some(x) = st.lib.lock().unwrap().installed.get_mut(&other.id) {
                            x.inputs_ready.clear();
                            if profile.game.is_none() { x.game_copy = None; }
                        }
                    }
                }
            }
        } else {
            return Err(Error::Msg("Remove shared keys from the shared-key controls.".into()));
        }
    } else if let Some(path) = path {
        let path = PathBuf::from(path);
        if !path.is_absolute() { return Err(Error::Msg("Choose an absolute file or folder path.".into())); }
        g.setup_inputs.insert(input.clone(), path);
        if !inputs::rows(&p, &g, &st.dir).map_err(Error::Msg)?.iter().any(|r| r.id == input && r.path.is_some()) {
            return Err(Error::Msg("That file or folder isn't the kind this game asks for here.".into()));
        }
        // One source cannot silently satisfy two disc/DLC slots. Identity hashing is deferred.
        let chosen = g.setup_inputs[&input].canonicalize().map_err(|_| Error::Msg("Couldn't open that input.".into()))?;
        if g.setup_inputs.iter().any(|(k, v)| k != &input && v.canonicalize().ok().as_ref() == Some(&chosen)) {
            return Err(Error::Msg("That file is already picked for another slot.".into()));
        }
    } else {
        g.setup_inputs.remove(&input);
    }
    g.inputs_ready.clear();
    if p.game.is_none() { g.game_copy = None; }
    st.lib.lock().unwrap().installed.insert(id, g);
    st.save();
    Ok(())
}

#[tauri::command(async)]
fn setup_key_remove(st: State<'_, App>, id: String, input: String) -> Res<()> {
    if st.running.lock().unwrap().len() > 0 || SETUP_RUN.lock().unwrap().as_ref().is_some_and(|m| !m.is_empty()) || !INPUT_WORKERS.lock().unwrap().is_empty() {
        return Err(Error::Msg("Close running games and stop setup before removing shared keys.".into()));
    }
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let p = profile_of(&st, &id, &g).ok_or_else(|| Error::Msg("This game has no setup profile.".into()))?;
    let resource = p.inputs.iter().find(|i| i.id == input && !i.shared.is_empty()).ok_or_else(|| Error::Msg("Unknown shared key input.".into()))?.shared.clone();
    let games: Vec<_> = st.lib.lock().unwrap().installed.values().cloned().collect();
    for other in games {
        if let Some(profile) = profile_of(&st, &other.id, &other) {
            for key in profile.inputs.iter().filter(|k| k.shared == resource) {
                inputs::remove_key(&profile, &other, &st.dir, &key.id).map_err(Error::Msg)?;
                if let Some(x) = st.lib.lock().unwrap().installed.get_mut(&other.id) {
                    x.inputs_ready.clear();
                    if profile.game.is_none() { x.game_copy = None; }
                }
            }
        }
    }
    st.save();
    Ok(())
}

/// Multi-input setup deliberately does not read/hash whole discs. Projects validate content.
#[tauri::command(async)]
fn setup_inputs_run(app: AppHandle, st: State<'_, App>, id: String) -> Res<()> {
    // One game of several: only its inputs are set up, and remembered for that game. `id` stays
    // what the window asked for (its progress comes back under it); `rid` is the install.
    let pick = game_of(&st, &id);
    let rid = pick.as_ref().map(|p| p.recomp.clone()).unwrap_or_else(|| id.clone());
    let sub = pick.as_ref().map(|p| p.game.id.clone());
    if setup_busy(&id) || setup_busy(&rid) || st.running.lock().unwrap().contains_key(&rid) { return Err(Error::Msg("Close the game or stop setup first.".into())); }
    let mut g = st.lib.lock().unwrap().installed.get(&rid).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let mut p = profile_of(&st, &rid, &g).ok_or_else(|| Error::Msg("This game has no setup profile.".into()))?;
    if let Some(pk) = &pick {
        games::scope(&mut p, &pk.game, &pk.all);
    }
    if p.inputs.is_empty() { return Err(Error::Msg("This profile doesn't declare named inputs.".into())); }
    p.input_paths = inputs::resolve(&p, &g, &st.dir).map_err(Error::Msg)?;
    if p.game.is_some() && g.game_copy.as_ref().map_or(true, |c| !c.exists()) { return Err(Error::Msg("Choose your copy of the game first.".into())); }
    let signature = inputs::signature(&p, &g, &st.dir).map_err(Error::Msg)?;
    INPUT_WORKERS.lock().unwrap().push(id.clone());
    let run = SETUP_NEXT.fetch_add(1, Ordering::Relaxed);
    SETUP_RUN.lock().unwrap().get_or_insert_with(HashMap::new).insert(id.clone(), run);
    let dir = st.dir.clone();
    std::thread::spawn(move || {
        let stopped = || setup_stopped(&id, run);
        let emit = |step: &str, pct: f32, text: String| { let _ = app.emit("setup", SetupEv { id: id.clone(), step: step.into(), pct, text, extras: p.prepare.is_some(), ..Default::default() }); };
        let result = (|| -> Result<(), String> {
            emit("read", 0.0, "Checking the selected files".into());
            if stopped() { return Err("Stopped.".into()); }
            let placed = inputs::stage_cancellable(&p, &mut g, &dir, &stopped);
            // Remember managed destinations even after a failed preparation, so retry is safe.
            let st = app.state::<App>();
            if let Some(x) = st.lib.lock().unwrap().installed.get_mut(&rid) { x.input_staged = g.input_staged.clone(); x.input_stamps = g.input_stamps.clone(); }
            st.save();
            placed?;
            emit("verify", 1.0, "Files selected.".into());
            if let Some(pr) = &p.prepare {
                let mut pr = pr.clone();
                // Omit an argument containing an absent optional/conditional token. Put flags in
                // the same argument (e.g. --dlc={input:dlc}) so a flag cannot steal the next value.
                pr.args = inputs::arguments(&pr.args, &p.input_paths, false);
                let copy = g.game_copy.as_deref().unwrap_or(&g.path);
                run_prepare(&pr, &g.path, copy, &|pct, text| emit("extras", pct, text), &stopped)?;
            }
            if stopped() { return Err("Stopped.".into()); }
            let current = st.lib.lock().unwrap().installed.get(&rid).cloned().ok_or("The game was removed during setup.")?;
            if inputs::signature(&p, &current, &dir)? != signature { return Err("The selected files changed. Run setup again.".into()); }
            p.prepare(&g.path, g.game_copy.as_deref()).map_err(|_| "Couldn't write the game's settings.")?;
            if let Some(x) = st.lib.lock().unwrap().installed.get_mut(&rid) {
                match &sub {
                    Some(sub) => x.games.entry(sub.clone()).or_default().inputs_ready = signature,
                    None => x.inputs_ready = signature,
                }
                if p.game.is_none() && (sub.is_none() || x.game_copy.as_ref().map_or(true, |c| !c.exists())) { x.game_copy = p.inputs.iter().filter(|i| i.shared.is_empty() && !i.optional).find_map(|i| p.input_paths.get(&i.id).cloned()); }
            }
            st.save();
            Ok(())
        })();
        if !stopped() {
            match result { Ok(()) => emit("done", 1.0, "Your files are set up.".into()), Err(why) => emit("error", 0.0, why) }
        }
        if let Some(m) = SETUP_RUN.lock().unwrap().as_mut() { if m.get(&id) == Some(&run) { m.remove(&id); } }
        INPUT_WORKERS.lock().unwrap().retain(|x| x != &id);
    });
    Ok(())
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
        copy_ok(need, &copy, &g.path, true).map_err(Error::Msg)?;
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
fn set_game_setting(app: AppHandle, st: State<'_, App>, id: String, setting: String, choice: usize) -> Res<()> {
    read_screen(&app);
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let p = profile_of(&st, &id, &g).ok_or_else(|| Error::Msg("The launcher can't set this game up yet.".into()))?;
    let s = p.settings.iter().find(|s| s.id == setting).ok_or_else(|| Error::Msg("That isn't one of its settings.".into()))?;
    // Match My Screen is remembered, so it follows the monitor; any other choice stops that.
    let fit = s.screen.is_some() && choice + 1 == s.choices.len();
    if fit != g.screen_fit.contains(&setting) || fit {
        if let Some(x) = st.lib.lock().unwrap().installed.get_mut(&id) {
            x.screen_fit.retain(|k| k != &setting);
            if fit {
                x.screen_fit.push(setting.clone());
                x.screen_size = screen_now().map(|(w, h)| format!("{w}x{h}")).unwrap_or_default();
            }
        }
        st.save();
    }
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

/// The player moved a slider: written into the game's settings file, or kept for the command line.
/// Gives back the value as written.
#[tauri::command(async)]
fn set_game_slider(st: State<'_, App>, id: String, setting: String, value: f64) -> Res<String> {
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let p = profile_of(&st, &id, &g).ok_or_else(|| Error::Msg("The launcher can't set this game up yet.".into()))?;
    let s = p.settings.iter().find(|s| s.id == setting).ok_or_else(|| Error::Msg("That isn't one of its settings.".into()))?;
    if s.target != "args" && !p.from_boot {
        let _ = p.prepare(&g.path, g.game_copy.as_deref());
    }
    let text = p.set_slider(&g.path, &setting, value).map_err(Error::Msg)?;
    if s.target == "args" {
        if let Some(x) = st.lib.lock().unwrap().installed.get_mut(&id) {
            x.slider_vals.insert(setting, text.clone());
        }
        st.save();
    }
    Ok(text)
}

/// The player set the keys of one action in a key bindings list. Gives back the keys as written.
#[tauri::command(async)]
fn set_game_binding(st: State<'_, App>, id: String, setting: String, action: String, keys: Vec<String>) -> Res<Vec<String>> {
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let p = profile_of(&st, &id, &g).ok_or_else(|| Error::Msg("The launcher can't set this game up yet.".into()))?;
    if !p.from_boot {
        let _ = p.prepare(&g.path, g.game_copy.as_deref());
    }
    p.set_binding(&g.path, &setting, &action, &keys).map_err(Error::Msg)
}

/// Reset to Default for a key bindings list.
#[tauri::command(async)]
fn reset_game_bindings(st: State<'_, App>, id: String, setting: String) -> Res<()> {
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let p = profile_of(&st, &id, &g).ok_or_else(|| Error::Msg("The launcher can't set this game up yet.".into()))?;
    p.reset_bindings(&g.path, &setting).map_err(Error::Msg)
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

/// Discord for a game started outside the launcher (a desktop shortcut straight to its program):
/// while the launcher is open, it looks every 20 seconds for an installed game's program running
/// from that game's folder, and shows it in Discord as if it had started it.
#[cfg(windows)]
fn watch_outside(app: AppHandle) {
    std::thread::spawn(move || {
        let mut shown: Option<String> = None;
        loop {
            std::thread::sleep(std::time::Duration::from_secs(20));
            let st = app.state::<App>();
            // Not on, or a game the launcher started itself (that one shows already).
            if !g_discord(&st) || !st.running.lock().unwrap().is_empty() {
                if shown.take().is_some() && !g_discord(&st) {
                    discord::send(discord::Msg::Clear);
                }
                continue;
            }
            let games: Vec<(String, String, PathBuf)> = {
                let l = st.lib.lock().unwrap();
                l.installed
                    .values()
                    .filter_map(|g| {
                        let exe = g.exe.as_ref().map(|e| if e.is_absolute() { e.clone() } else { g.path.join(e) })?;
                        let name = exe.file_name()?.to_string_lossy().to_lowercase();
                        let common = ["launcher.exe", "game.exe", "start.exe", "main.exe", "setup.exe", "play.exe", "run.exe"].contains(&name.as_str());
                        (name.ends_with(".exe") && !common).then(|| (g.id.clone(), name, exe.parent().map(Path::to_path_buf).unwrap_or_else(|| g.path.clone())))
                    })
                    .collect()
            };
            if games.is_empty() {
                continue;
            }
            let procs = win::processes();
            let found = games.iter().find(|(_, name, dir)| {
                procs.iter().any(|(pid, n)| n.to_lowercase() == *name && win::process_file(*pid).map(|f| f.starts_with(dir)).unwrap_or(false))
            });
            match (found, &shown) {
                (Some((id, _, _)), Some(s)) if s == id => {}
                (Some((id, _, _)), _) => {
                    let (app_id, game, cover) = discord_info(&st, id);
                    let recomp = st.lib.lock().unwrap().installed.get(id).map(|g| g.name.clone()).unwrap_or_default();
                    discord::send(discord::Msg::Playing { app_id, game, recomp, cover, start: now() });
                    trail::note(format!("Discord: {id} started outside the launcher"));
                    shown = Some(id.clone());
                }
                (None, Some(_)) => {
                    discord::send(discord::Msg::Clear);
                    shown = None;
                }
                (None, None) => {}
            }
        }
    });
}

fn g_discord(st: &App) -> bool {
    st.lib.lock().unwrap().settings.discord
}

/// The Discord application id, the game's own name and its cover's address, from the catalog on disk.
fn discord_info(st: &App, id: &str) -> (String, String, String) {
    let cat = cached_catalog(st);
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
    // One game of a recomp that carries several starts the recomp (start_game picks the game).
    let full = id.clone();
    let id = recomp_of(&app.state::<App>(), &id);
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
    start_game(&app, &full)
}

/// The game being played in each running recomp, by recomp id: its full id when it's one of
/// several, else the recomp's own.
pub(crate) static PLAYING_GAME: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

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
                ingame::notify(&app2, m, "Once, for the shader you picked. The game starts in a moment.");
            }
        };
        if let Err(e) = looks::ensure(&st.http, &st.dir, mirror, &say).await {
            let _ = app.emit("toast", format!("{e} The game starts without its shader."));
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
const VKBASALT_MISSING: &str = "Shaders on Linux use vkBasalt. Install it from your distro's packages (it's usually called vkbasalt), then start the game again.";

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
        return Err(Some(if pl.reason.is_empty() { "This game doesn't work with shaders.".into() } else { pl.reason.clone() }));
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
    let p = recent_log_path(st, id, since)?;
    let text = std::fs::read(&p).ok()?;
    let text = String::from_utf8_lossy(&text);
    let lines: Vec<&str> = text.lines().collect();
    let tail = lines[lines.len().saturating_sub(60)..].join("\n");
    Some((p.to_string_lossy().into(), tail))
}

/// The newest log the launcher knows for a game from its last session (or since `since`): the
/// game's own, what it printed, or ReShade's.
pub(crate) fn recent_log_path(st: &App, id: &str, since: u64) -> Option<PathBuf> {
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
    best.map(|(_, p)| p)
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

/// The files a game's program loads that aren't on this PC (the DLLs it needs on Windows, the
/// libraries it links to on Linux): for the crash window, when Windows said one was missing.
/// The program that starts the game: the player's own pick, the profile's, or the one installed.
fn started_exe(st: &App, id: &str, g: &Installed) -> Option<PathBuf> {
    let prof = if g.manual.is_some() { None } else { profile_of(st, id, g) };
    g.manual
        .as_ref()
        .and_then(|m| m.exe.clone())
        .map(|e| if e.is_absolute() { e } else { g.path.join(e) })
        .or_else(|| prof.as_ref().and_then(|p| p.command_for(&g.path, g.game_copy.as_deref(), &g.picks, is_windows_build(g)).map(|(e, _)| e)))
        .or_else(|| g.exe.as_ref().map(|e| g.path.join(e)))
        .filter(|e| e.is_file())
}

#[derive(Serialize)]
struct GpuChoice {
    /// Graphics cards in this PC (a laptop often has two: the built-in one and a faster one).
    cards: usize,
    /// The game is set to the faster one already.
    fast: bool,
}

/// For the crash window: whether "Use the Faster Graphics Card" can help this game.
#[tauri::command(async)]
fn gpu_choice(st: State<'_, App>, id: String) -> GpuChoice {
    #[cfg(windows)]
    {
        let exe = st.lib.lock().unwrap().installed.get(&id).cloned().and_then(|g| started_exe(&st, &id, &g));
        // A game started through a script: Windows' choice goes with a program, so it can't help.
        let script = exe.as_ref().map(|e| install::is_script(e)).unwrap_or(true);
        GpuChoice { cards: if script { 0 } else { win::gpus().len() }, fast: exe.map(|e| win::prefers_fast_gpu(&e)).unwrap_or(false) }
    }
    #[cfg(not(windows))]
    {
        let _ = (st, id);
        GpuChoice { cards: 0, fast: false }
    }
}

/// Windows starts the game on the faster graphics card from now on.
#[tauri::command(async)]
fn prefer_fast_gpu(st: State<'_, App>, id: String) -> Res<()> {
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let exe = started_exe(&st, &id, &g).ok_or_else(|| Error::Msg("The launcher couldn't tell which program starts the game.".into()))?;
    #[cfg(windows)]
    {
        win::prefer_fast_gpu(&exe).map_err(|e| Error::Msg(format!("Windows didn't save it: {e}")))?;
        trail::note(format!("{id}: set to the faster graphics card"));
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = exe;
        Err(Error::Msg("This is for Windows.".into()))
    }
}

/// The folders a game's DLLs are looked for in: its program's, and its own.
fn dll_dirs(st: &App, id: &str, g: &Installed) -> Vec<PathBuf> {
    let mut v = vec![];
    if let Some(d) = started_exe(st, id, g).and_then(|e| e.parent().map(Path::to_path_buf)) {
        v.push(d);
    }
    if !v.contains(&g.path) {
        v.push(g.path.clone());
    }
    v
}

/// What's old for this game: its folders against Windows' System32 and SysWOW64, by its program's
/// kind (64- or 32-bit).
fn dll_findings(st: &App, id: &str, g: &Installed) -> dllcheck::Findings {
    let root = PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()));
    let dirs = dll_dirs(st, id, g);
    let x64 = started_exe(st, id, g).and_then(|e| dllcheck::pe_x64(&e)).unwrap_or(true);
    dllcheck::check(&dirs.iter().map(PathBuf::as_path).collect::<Vec<_>>(), &root.join("System32"), &root.join("SysWOW64"), x64)
}

/// The game sets itself up from the player's copy (the launcher's Easy Setup knows it): files it
/// can't find that its release never had are ones it makes then.
fn makes_its_files(st: &App, id: &str, g: &Installed) -> bool {
    profile_of(st, id, g).map(|p| p.game.is_some()).unwrap_or(false)
}

/// For "a DLL it needs is too old": old copies the game came with, an old Visual C++ runtime, an
/// old Vulkan loader (an old graphics driver). Windows only; empty elsewhere.
#[tauri::command(async)]
fn dll_check(st: State<'_, App>, id: String) -> dllcheck::Findings {
    if !cfg!(windows) {
        return dllcheck::Findings::default();
    }
    let Some(g) = st.lib.lock().unwrap().installed.get(&id).cloned() else { return dllcheck::Findings::default() };
    dll_findings(&st, &id, &g)
}

/// The old copies set aside (renamed .old), so the game uses Windows' newer ones. The ones moved.
#[tauri::command(async)]
fn dll_aside(st: State<'_, App>, id: String) -> Res<Vec<String>> {
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let moved = dllcheck::set_aside(&dll_findings(&st, &id, &g).old_paths);
    trail::note(format!("{id}: old copies set aside (renamed .old): {}", moved.join(", ")));
    Ok(moved)
}

/// The files it can't find, when they're all ones the game makes itself from the player's copy
/// (none came with its release, and it sets itself up from a copy). Empty otherwise.
#[tauri::command(async)]
fn missing_made(st: State<'_, App>, id: String) -> Vec<String> {
    let Some(g) = st.lib.lock().unwrap().installed.get(&id).cloned() else { return vec![] };
    if !makes_its_files(&st, &id, &g) {
        return vec![];
    }
    let Some(e) = started_exe(&st, &id, &g) else { return vec![] };
    dllcheck::made_by_game(&missing_libraries(&e, &g.path), &g.files)
}

#[tauri::command(async)]
fn missing_files(st: State<'_, App>, id: String) -> Vec<String> {
    let Some(g) = st.lib.lock().unwrap().installed.get(&id).cloned() else { return vec![] };
    match started_exe(&st, &id, &g) {
        Some(e) => missing_libraries(&e, &g.path).into_iter().take(8).collect(),
        None => vec![],
    }
}

/// Before a picked copy is set up: the ID read off the disc or cartridge when the catalog lists the
/// ones this recomp is made for and it isn't one of them, with those. None when it fits, when the
/// catalog lists none, or when the copy's ID can't be read.
#[tauri::command(async)]
fn copy_check(st: State<'_, App>, id: String, path: String) -> Option<Value> {
    let cat = cached_catalog(&st);
    let a = catalog::app(&cat, &id)?;
    let found = roms::fits_copy_ids(a, Path::new(&path)).err()?;
    Some(serde_json::json!({ "found": found, "want": roms::copy_ids(a) }))
}

/// What could be keeping a game from starting: its files, Windows, what it needs on the PC, the
/// player's copy, its setup, its folder, and what it printed last time.
#[tauri::command(async)]
fn troubleshoot(st: State<'_, App>, id: String, code: Option<i64>) -> Res<Vec<Check>> {
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
    // The Windows user folder's name, where many keep saves and settings (AppData, Documents).
    if cfg!(windows) && g.path.to_string_lossy().is_ascii() {
        if let Some(home) = dirs::home_dir().filter(|h| !h.to_string_lossy().is_ascii()) {
            out.push(check("warn", "Your Windows user folder's name has letters some games can't read", "Some recomps keep saves and settings in your user folder ({folder}) and can't open it when its name has accents or letters outside English. If it closes right away, that may be why: tell its developer.", "").with("folder", home.display().to_string()));
        }
    }
    // The program it starts (set up by hand: the one the player picked).
    let prof = if g.manual.is_some() { None } else { profile_of(&st, &id, &g) };
    let exe = g
        .manual
        .as_ref()
        .and_then(|m| m.exe.clone())
        .map(|e| if e.is_absolute() { e } else { g.path.join(e) })
        .or_else(|| prof.as_ref().and_then(|p| p.command_for(&g.path, g.game_copy.as_deref(), &g.picks, is_windows_build(&g)).map(|(e, _)| e)))
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
    // A program in a setup's working folder (temp\…\build) or one of the game's tools: what it
    // starts is probably not the finished game.
    let rel = exe.strip_prefix(&g.path).map(Path::to_path_buf).unwrap_or_else(|_| PathBuf::from(&name));
    let names: Vec<&str> = [g.name.as_str(), g.repo.rsplit(['/', ':']).next().unwrap_or("")].into_iter().filter(|n| !n.is_empty()).collect();
    if install::looks_like_tool(&rel, &names) {
        out.push(check("warn", "It starts {name}, which looks like one of the game's tools", "It sits in a folder for tools or a setup's leftovers ({folder}). Choose the game's own program, where its setup installed it.", "setexe").with("name", &name).with("folder", rel.parent().map(|d| d.display().to_string()).unwrap_or_default()));
    } else {
        out.push(check("ok", "It starts {name}", "", "").with("name", &name));
    }
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
        let made = if makes_its_files(&st, &id, &g) { dllcheck::made_by_game(&lacking, &g.files) } else { vec![] };
        let (detail, fix) = if !made.is_empty() {
            // Files the game makes from the player's copy (it translates the disc into them):
            // reinstalling won't bring them back; setting up the copy does.
            ("Missing: {files}. The game makes them itself from your copy of the game, with its setup. Set up your game copy (or run its setup), then press Play again.", "setup")
        } else if vc {
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
    // Old copies of runtime DLLs it came with, and an old Vulkan loader: "a DLL it needs is too old".
    // Old copies are often harmless (many games bring their own runtime), so they're only raised
    // when it last closed with "a DLL it needs is too old" (0xC0000139).
    if cfg!(windows) {
        let f = dll_findings(&st, &id, &g);
        let too_old = matches!(code, Some(-1073741511) | Some(3221225785));
        if too_old && !f.old_copies.is_empty() {
            out.push(check("warn", "It came with old copies of files Windows has newer ones of", "{files}: the game uses these instead of Windows' newer ones, and may close with \"a DLL it needs is too old\". Moving them aside fixes that.", "dllaside").with("files", f.old_copies.join(", ")));
        }
        if let Some(v) = f.vulkan_old {
            out.push(check("warn", "Your graphics driver is out of date", "Its Vulkan is version {version}; recomps need 1.3 or newer. Update your graphics driver from NVIDIA, AMD or Intel.", "driver").with("version", v));
        }
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
            // Installed, but older than recomps built with today's Visual Studio need: they close
            // with "a DLL it needs is too old".
            Some(true) if pc.vcredist_old.is_some() => out.push(check("warn", "The Visual C++ runtime is out of date", "Newer recomps need a newer one than this PC has. Updating it is quick.", "vcredist")),
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
            Some(c) => match copy_ok(need, c, &g.path, false) {
                Ok(()) if profile::odd_snes_size(c) => out.push(check("warn", "Your copy of the game may be incomplete", "Its size isn't one a full dump of the cartridge has. If the game won't start, try another copy of your own cartridge.", "copy")),
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
            (true, true) => "A shader (ReShade) and your own launch options are on. If it won't start, try once without them.",
            (true, false) => "A shader (ReShade) is on. If it won't start, try once without it.",
            _ => "Your own launch options are on. If it won't start, try once without them.",
        };
        out.push(check("info", "Extras are on", what, "clean"));
        // A Look kept to one way in after a start that crashed: when that's the wrong one for this
        // game, the Look never shows.
        if look_on {
            // ReShade didn't start at all last time: the Look never showed.
            if let Some(r) = looks::never_started(&st.dir, &id) {
                if r == "only" {
                    out.push(check("warn", "The shader didn't start last time", "ReShade didn't load in this game, so its shader can't show. Play it without the shader.", "clean"));
                } else {
                    out.push(check("warn", "The shader didn't start last time", "ReShade didn't load in this game. The next time it starts, the shader tries another way in.", ""));
                }
            }
            if let Some(r) = looks::kept_route(&st.dir, &id) {
                let way = if r == "layer" { "Vulkan" } else { "Direct3D" };
                out.push(check("info", "The shader only tries one way in", "Since a start that crashed, its shader goes in through {way} only. If the shader doesn't show, let it try both ways again.", "lookroute").with("way", way));
            }
        }
    }
    // What it printed last time.
    if let Ok(text) = std::fs::read_to_string(game_output_path(&st.dir, &id)) {
        // It said it's missing something (its game files, most often).
        let missing = text.lines().rev().take(80).find(|l| {
            let l = l.to_lowercase();
            !stats_line(&l) && !runtime_line(&l) && (l.contains("status=missing") || l.contains("no executable found") || l.contains("could not find") || l.contains("couldn't find") || l.contains("not found") || l.contains("no such file") || l.contains(" missing"))
        });
        let needs_copy = prof.as_ref().map(|p| p.game.is_some()).unwrap_or(false);
        let said = |w: &str| text.lines().rev().take(120).any(|l| l.contains(w));
        if said("GLIBC_") && said("not found") {
            // A Linux build made on a newer Linux than this one (SteamOS, an older distro).
            out.push(check("bad", "Its Linux build needs a newer Linux", "It was built for a newer system library (glibc) than this Linux has. Its Windows version usually runs fine through Proton: switch to it from the game's page, or update your system.", ""));
        } else if said("--game_data_root") {
            // An Xbox 360 recomp that takes the game's files from a folder it's given.
            out.push(check("bad", "It needs to be told where your game files are", "This recomp reads your extracted game from a folder given to it when it starts, and its page doesn't set that up yet. Use Set Up By Hand and pick the folder with your game's files (the one with default.xex).", "manual"));
        } else if said("Required extensions are missing") || said("Missing required extension: VK_") || said("Unable to create instance") {
            // Vulkan's own words for a graphics driver that can't run it: not a missing file.
            out.push(check("bad", "Your graphics driver can't run it", "Vulkan couldn't start with the parts this game needs. Update your graphics driver from NVIDIA, AMD or Intel; on a laptop, make sure it uses its faster graphics card.", "driver"));
        } else if let Some(line) = missing {
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
    let exe = install::program_choice(&exe, assets::os_now()).ok_or_else(|| Error::Msg(format!("{} isn't an available program anymore (an update may have renamed it). Pick the program again in Set Up By Hand.", exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())))?;
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
        // Nothing known, but the game said last time how it takes its files (Xbox 360 recomps:
        // --game_data_root): that, ready for the player's folder.
        _ if std::fs::read_to_string(game_output_path(&st.dir, &id)).map(|t| t.contains("--game_data_root")).unwrap_or(false) => {
            m.how = "arg".into();
            m.arg = "--game_data_root {copy}".into();
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
        // The program has to be one: a disc image picked here is the game's copy, not its program.
        if let Some(why) = m.exe.as_deref().and_then(not_a_program) {
            return Err(Error::Msg(why.into()));
        }
    }
    let mut l = st.lib.lock().unwrap();
    let g = l.installed.get_mut(&id).ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    // A program inside the game's folder is kept by its place there, so it still works after the
    // folder moves.
    let mut manual = manual;
    if let Some(e) = manual.as_mut().and_then(|m| m.exe.as_mut()) {
        let path = if e.is_absolute() { e.clone() } else { g.path.join(&*e) };
        if assets::os_now() == "macos" && macos::is_bundle(&path) {
            *e = install::program_choice(&path, "macos").ok_or_else(|| Error::Msg("That app doesn't contain a program for this Mac.".into()))?;
        }
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

/// What the quick menu should show, for a page that has only just loaded: the event sent when it
/// opened can arrive before its script does.
#[tauri::command]
fn overlay_state(app: AppHandle) -> Option<Value> {
    if !app.state::<App>().overlay_open.load(Ordering::Relaxed) {
        return None;
    }
    ingame::overlay_payload(&app)
}

/// The program to start, picked by the player when the launcher's guess was wrong.
/// A file picked as the game's program that can't be one: a disc image, an archive, a save, a
/// picture. Why, for the player.
pub(crate) fn not_a_program(p: &Path) -> Option<&'static str> {
    let ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "iso" | "img" | "mdf" | "nrg" | "gdi" | "pbp" | "cue" | "chd" | "cso" | "gcm" | "rvz" | "wbfs" | "wia" | "gcz" | "xci" | "nsp" | "nup" | "nca" | "wad" | "wux" | "wud" | "rpx" | "xex" | "xbe" | "elf" | "z64" | "n64" | "v64" | "sfc" | "smc" | "gba" | "nds" | "3ds" | "cia" | "pkg" =>
            Some("That's your game's disc image or ROM, not the program that starts it. Pick the recomp's own program (usually an .exe in its folder); your copy of the game goes in its setup."),
        "zip" | "7z" | "rar" | "gz" | "xz" | "tar" =>
            Some("That's a compressed file, not a program. Unpack it first, then pick the program inside."),
        "dmg" => Some("That's a disk image. Install the application from it, then choose the application."),
        "txt" | "md" | "pdf" | "json" | "toml" | "ini" | "cfg" | "log" | "png" | "jpg" | "jpeg" | "dll" | "so" | "sav" | "dat" =>
            Some("That isn't a program. Pick the one that starts the game (usually an .exe, or on Linux the file without an extension or an .AppImage)."),
        _ => None,
    }
}

#[tauri::command(async)]
fn set_exe(st: State<'_, App>, id: String, path: String) -> Res<Installed> {
    let mut l = st.lib.lock().unwrap();
    let g = l.installed.get_mut(&id).ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let p = install::program_choice(Path::new(&path), assets::os_now()).ok_or_else(|| Error::Msg(if assets::os_now() == "macos" && macos::is_bundle(Path::new(&path)) {
        "That app has no runnable program for this Mac. Choose an Apple Silicon or universal Mac app.".into()
    } else {
        "That file doesn't exist.".into()
    }))?;
    if let Some(why) = not_a_program(&p) {
        return Err(Error::Msg(why.into()));
    }
    let outside = !p.starts_with(&g.path);
    // The release's own file when that file is its installer (one Setup.exe and nothing else): it
    // runs as the game's setup, which finds the game it installs, rather than as the game.
    let is_download = p.file_name().map(|n| n.to_string_lossy().eq_ignore_ascii_case(&g.asset)).unwrap_or(false);
    if !outside && is_download && install::looks_like_setup(&p) {
        g.setup = p.strip_prefix(&g.path).ok().map(Path::to_path_buf);
        g.setup_done = false;
        g.exe = None;
        g.exe_rules = install::EXE_RULES;
        let out = g.clone();
        drop(l);
        st.save();
        trail::note(format!("{id}: its download is its installer, so it runs as the setup"));
        return Ok(out);
    }
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

/// Whether the server is resting after trouble (the window shows "Offline" while it is).
#[tauri::command(async)]
fn net_state() -> Value {
    json!({ "offline": net::offline(), "busy": net::busy() })
}

/// The site answered again after being Offline: the list asked for again (Discover and game
/// links see what's new) and saves waiting to go up are sent.
fn back_online(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let st = app.state::<App>();
        if let Ok(f) = catalog::catalog_refresh(&st.http, &st.dir).await {
            if !f.offline {
                note_new_apps(&app, &st, &f.data);
                let _ = app.emit("catalog-fresh", ());
            }
        }
        cloud::upload_pending(&app).await;
    });
}

/// The list for the window. Without `force`, the copy on disk comes back at once (the window never
/// waits on the network for it) and, when it's more than six hours old, a new one is asked for in
/// the background and sent as "catalog-fresh". `force`: the player pressed Refresh.
#[tauri::command]
async fn get_catalog(app: AppHandle, st: State<'_, App>, force: bool) -> Res<catalog::Fetched> {
    if !force {
        if let Some((mut f, stale)) = catalog::catalog_now(&st.dir) {
            merged_mine(&app, &st, &mut f.data);
            if stale {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let st = app.state::<App>();
                    if let Ok(f) = catalog::catalog_refresh(&st.http, &st.dir).await {
                        if !f.offline {
                            note_new_apps(&app, &st, &f.data);
                            let _ = app.emit("catalog-fresh", ());
                        }
                    }
                });
            }
            return Ok(f);
        }
    }
    // Refresh, or a game link the list on disk doesn't have yet: asked straight away, resting or
    // not (one question, and the player's own doing).
    let mut f = if force { net::now(catalog::catalog(&st.http, &st.dir, true)).await? } else { catalog::catalog(&st.http, &st.dir, false).await? };
    merged_mine(&app, &st, &mut f.data);
    if !f.offline {
        note_new_apps(&app, &st, &f.data);
    }
    Ok(f)
}

/// merge_mine, with the list sent to the window without the entries moved over, and the window
/// told ("mine-merged") so it reads the Library again and says so.
fn merged_mine(app: &AppHandle, st: &App, data: &mut Value) {
    let merged = merge_mine(st, data);
    if merged.is_empty() {
        return;
    }
    if let Some(a) = data.get_mut("apps").and_then(Value::as_array_mut) {
        a.retain(|x| !merged.iter().any(|(old, _, _)| str_of(x, "id") == old));
    }
    let _ = app.emit("mine-merged", merged.iter().map(|(from, to, name)| json!({ "from": from, "to": to, "name": name })).collect::<Vec<_>>());
}

/// A recomp's releases for its page. Without `force`, the copy on disk comes back at once and,
/// when it's more than six hours old, a new one is asked for in the background and announced as
/// "releases-fresh". With nothing on disk, it's asked for now.
#[tauri::command]
async fn get_releases(app: AppHandle, st: State<'_, App>, repo: String, force: bool) -> Res<catalog::Fetched> {
    if !force {
        if let Some(f) = catalog::releases_on_disk(&st.dir, &repo) {
            if f.age >= catalog::RELEASES_FRESH && !net::resting(&format!("{}/api/gh.php", catalog::base()), false) {
                let (app, repo) = (app.clone(), repo.clone());
                tauri::async_runtime::spawn(async move {
                    let st = app.state::<App>();
                    if matches!(catalog::releases(&st.http, &st.dir, &repo, false).await, Ok(f) if !f.offline) {
                        let _ = app.emit("releases-fresh", json!({ "repo": repo }));
                    }
                });
            }
            return Ok(f);
        }
    }
    catalog::releases(&st.http, &st.dir, &repo, force).await
}

/// Installed games with a newer release than the one installed: id -> the new tag.
/// Release info comes from disk while it's younger than six hours, so this only asks the site
/// for games whose info is older (one at a time). `force`: the player pressed Check for Updates.
#[tauri::command]
async fn check_updates(st: State<'_, App>, force: Option<bool>) -> Res<HashMap<String, String>> {
    let force = force.unwrap_or(false);
    let games: Vec<Installed> = st.lib.lock().unwrap().installed.values().cloned().collect();
    let associations: HashMap<String, (String, PathBuf)> = games.iter().map(|g| (g.id.clone(), (g.repo.clone(), g.path.clone()))).collect();
    // The list says which version is newest: a game already on it needs no question to the site.
    let cat = catalog::catalog(&st.http, &st.dir, false).await.map(|f| f.data).unwrap_or(Value::Null);
    let same = |a: &str, b: &str| {
        let n = |s: &str| s.trim().trim_start_matches(['v', 'V']).to_ascii_lowercase();
        !a.trim().is_empty() && n(a) == n(b)
    };
    let mut out = HashMap::new();
    let (mut asked, mut failed) = (0, 0);
    for g in games.into_iter().filter(|g| !g.repo.is_empty() && !g.local) {
        asked += 1;
        let listed = catalog::app(&cat, &g.id).map(|a| str_of(a, "version").to_string()).unwrap_or_default();
        let on_disk = if !force && same(&listed, &g.tag) { catalog::releases_on_disk(&st.dir, &g.repo) } else { None };
        let got = match on_disk {
            Some(f) => Ok(f),
            None => catalog::releases(&st.http, &st.dir, &g.repo, force).await,
        };
        let Ok(f) = got else {
            failed += 1;
            continue;
        };
        let Ok(rels) = serde_json::from_value::<Vec<Release>>(f.data) else { continue };
        // Added from the player's own folder without a version: the release out when its program
        // was made is the one they have, so an update isn't offered for the version they're on.
        let mut g = g;
        if g.tag.is_empty() {
            if let Some(tag) = version_from_files(&g, &rels) {
                if let Some(x) = st.lib.lock().unwrap().installed.get_mut(&g.id).filter(|x| !x.local && x.repo == g.repo && x.path == g.path) {
                    x.tag = tag.clone();
                }
                st.save();
                g.tag = tag;
            }
        }
        if let Some(tag) = offered_update(&g, catalog::app(&cat, &g.id), &rels) {
            out.insert(g.id.clone(), tag);
        }
    }
    // A project may have been corrected while its network requests were in flight.
    {
        let l = st.lib.lock().unwrap();
        out.retain(|id, _| l.installed.get(id).is_some_and(|g| !g.local && associations.get(id) == Some(&(g.repo.clone(), g.path.clone()))));
    }
    // Not one answer: GitHub can't be reached from here (offline, or blocked on this network).
    if asked > 0 && failed == asked {
        return Err(Error::Msg("Can't reach GitHub right now, so updates can't be checked. Check your connection (some networks block GitHub), then try again.".into()));
    }
    Ok(out)
}

/// The one rule for whether a game has an update, used by the Library's badge, Settings > Check and
/// the Updates tab alike: the release the store has checked (the list's "tag") when it's newer than
/// the installed one and has a build for this system (Windows for a game played through Proton).
/// Without a checked release, the newest one GitHub has. Releases still being checked wait.
fn offered_update(g: &Installed, entry: Option<&Value>, rels: &[Release]) -> Option<String> {
    if g.pinned {
        return None;
    }
    let os_g = if g.proton { "windows" } else { assets::os_now() };
    let pattern = entry.and_then(|e| e.get("assets")).and_then(|a| a.get(os_g)).and_then(Value::as_str);
    let builds = |r: &Release| assets::pick(&r.assets, os_g, assets::arch_now(), pattern).is_some();
    if let Some(t) = entry.and_then(catalog_tag) {
        let at = |tag: &str| rels.iter().position(|r| same_tag(&r.tag_name, tag));
        if let Some(i) = at(&t) {
            let r = &rels[i];
            let newer = at(&g.tag).map(|j| i < j).unwrap_or_else(|| !older_version(&r.tag_name, &g.tag));
            return (!same_tag(&r.tag_name, &g.tag) && newer && builds(r)).then(|| r.tag_name.clone());
        }
    }
    let r = catalog::latest_for(rels, &g.tag)?;
    let known = rels.iter().any(|x| same_tag(&x.tag_name, &g.tag));
    (r.tag_name != g.tag && (known || !older_version(&r.tag_name, &g.tag)) && builds(r)).then(|| r.tag_name.clone())
}

/// What the Updates tab shows for each release, by the same rule as the badge: the update on offer,
/// the release the store has checked, and which releases have a build for this system.
#[tauri::command]
async fn update_info(st: State<'_, App>, id: String) -> Res<Value> {
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let cat = catalog::catalog(&st.http, &st.dir, false).await.map(|f| f.data).unwrap_or(Value::Null);
    let entry = catalog::app(&cat, &id);
    let f = match catalog::releases_on_disk(&st.dir, &g.repo) {
        Some(f) => f,
        None => catalog::releases(&st.http, &st.dir, &g.repo, false).await?,
    };
    let rels: Vec<Release> = serde_json::from_value(f.data).unwrap_or_default();
    let os_g = if g.proton { "windows" } else { assets::os_now() };
    let pattern = entry.and_then(|e| e.get("assets")).and_then(|a| a.get(os_g)).and_then(Value::as_str);
    let builds: Vec<&str> = rels.iter().filter(|r| assets::pick(&r.assets, os_g, assets::arch_now(), pattern).is_some()).map(|r| r.tag_name.as_str()).collect();
    Ok(json!({ "offered": offered_update(&g, entry, &rels), "checked": entry.and_then(catalog_tag), "builds": builds }))
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

/// How often a running launcher looks for a newer RecompLauncher.
pub(crate) const LAUNCHER_CHECK_EVERY: u64 = 6 * 3600;

/// launcher/version.json, kept on disk. A copy younger than `max_age` seconds is used as it is;
/// otherwise the site is asked, and the copy on disk is used when it can't be reached.
pub(crate) async fn version_json(st: &App, max_age: u64, player: bool) -> Option<Value> {
    let file = st.dir.join("cache/version.json");
    let disk = || std::fs::read(&file).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).filter(Value::is_object);
    let age = std::fs::metadata(&file).ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).map(|d| d.as_secs());
    if age.map(|a| a < max_age).unwrap_or(false) {
        if let Some(v) = disk() {
            return Some(v);
        }
    }
    let url = format!("{}/launcher/version.json", catalog::base());
    match net::send(st.http.get(&url).timeout(net::QUICK), &url, player).await {
        Ok(r) if r.status().is_success() => match r.json::<Value>().await {
            Ok(v) if v.is_object() => {
                let _ = std::fs::create_dir_all(st.dir.join("cache"));
                let _ = std::fs::write(&file, serde_json::to_vec(&v).unwrap_or_default());
                Some(v)
            }
            _ => {
                net::bad_answer(&url);
                disk()
            }
        },
        _ => disk(),
    }
}

/// What's in launcher/version.json: for "What's new" after an update.
#[tauri::command]
async fn launcher_notes(st: State<'_, App>) -> Res<Value> {
    Ok(version_json(&st, LAUNCHER_CHECK_EVERY, false).await.unwrap_or(Value::Null))
}

/// A newer RecompLauncher, if there is one. Read from the copy on disk when it's younger than
/// `max_age` seconds.
pub(crate) async fn newer_launcher(st: &App, max_age: u64, player: bool) -> Option<Value> {
    // A Flatpak is updated by Flatpak (from wherever it was installed), never by the launcher itself.
    if flatpak::sandboxed() {
        return None;
    }
    let v = version_json(st, max_age, player).await?;
    // Each non-Windows platform needs its own published build; never offer another OS's update.
    let v = launcher_platform_entry(&v, assets::os_now(), assets::arch_now()).unwrap_or(Value::Null);
    let newest = str_of(&v, "version").to_string();
    let mut v = v;
    // Older than the list's "min": the update isn't optional any more.
    let min = str_of(&v, "min").to_string();
    if !min.is_empty() && version_key(env!("CARGO_PKG_VERSION")) < version_key(&min) && v.is_object() {
        v["required"] = Value::Bool(true);
    }
    // An update file that turned out wrong isn't offered again; a fixed one (another checksum) is.
    if update_is_bad(&st.dir, &newest, str_of(&v, "url"), str_of(&v, "sha256")) {
        return None;
    }
    (!newest.is_empty() && version_key(&newest) > version_key(env!("CARGO_PKG_VERSION"))).then_some(v)
}

/// Programs a game leaves behind that aren't the game: crash reporters and the like.
fn leftover_helper(name: &str) -> bool {
    let n = name.to_lowercase();
    ["crash", "report", "helper", "subprocess", "webhelper", "updater", "update", "unins", "setup"].iter().any(|w| n.contains(w))
}

/// A program from this folder (or deeper) is running, other than RecompLauncher itself. Only for a
/// game's own folder: a drive or a folder near the top is too broad to tell anything.
fn still_running_in(dir: &Path) -> bool {
    if dir.components().count() < 3 {
        return false;
    }
    let me = std::process::id();
    #[cfg(windows)]
    {
        let d = dir.to_string_lossy().trim_end_matches('\\').to_lowercase() + "\\";
        return win::processes().into_iter().any(|(pid, name)| pid != me && pid != 0 && !leftover_helper(&name) && win::process_file(pid).map(|f| f.to_string_lossy().to_lowercase().starts_with(&d)).unwrap_or(false));
    }
    #[cfg(target_os = "linux")]
    {
        let Ok(rd) = std::fs::read_dir("/proc") else { return false };
        return rd
            .flatten()
            .filter_map(|e| e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()))
            .filter(|&p| p != me)
            .any(|p| std::fs::read_link(format!("/proc/{p}/exe")).map(|f| f.starts_with(dir) && !leftover_helper(&f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())).unwrap_or(false));
    }
    #[allow(unreachable_code)]
    {
        let _ = me;
        false
    }
}

/// An update file that turned out wrong: another version inside it, or two installs from it that
/// didn't go in. { version, url, sha256, fails }. Two strikes and that exact file isn't offered
/// again; it's offered again when version.json changes it (a new version, url or sha256).
fn update_bad_file(dir: &Path) -> PathBuf {
    dir.join("update-bad.json")
}

fn update_same(v: &Value, version: &str, url: &str, sha: &str) -> bool {
    str_of(v, "version").trim_start_matches('v') == version.trim_start_matches('v') && str_of(v, "url") == url && str_of(v, "sha256").eq_ignore_ascii_case(sha)
}

fn update_is_bad(dir: &Path, version: &str, url: &str, sha: &str) -> bool {
    let Some(v) = std::fs::read(update_bad_file(dir)).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else { return false };
    update_same(&v, version, url, sha) && v.get("fails").and_then(Value::as_u64).unwrap_or(0) >= 2
}

/// One more strike against this update file (`fails` more); returns how many it has now.
fn update_strike(dir: &Path, version: &str, url: &str, sha: &str, fails: u64) -> u64 {
    let f = update_bad_file(dir);
    let old = std::fs::read(&f).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).unwrap_or(Value::Null);
    let n = if update_same(&old, version, url, sha) { old.get("fails").and_then(Value::as_u64).unwrap_or(0) } else { 0 } + fails;
    let _ = std::fs::write(&f, json!({"version": version.trim_start_matches('v'), "url": url, "sha256": sha.to_lowercase(), "fails": n}).to_string());
    n
}

/// A Windows installer names its own version in its version resource (UTF-16). An installer for
/// RecompLauncher `version` that doesn't is another build, published by mistake.
fn installer_is_version(bytes: &[u8], version: &str) -> bool {
    let want: Vec<u8> = version.trim_start_matches('v').encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    !want.is_empty() && bytes.windows(want.len()).any(|w| w == want.as_slice())
}

/// The window asks as it opens (an answer from the last hour is reused, so opening it again and
/// again doesn't ask again), and with `force` when the player presses Check for Updates.
#[tauri::command]
async fn launcher_update(st: State<'_, App>, force: Option<bool>) -> Res<Option<Value>> {
    let force = force.unwrap_or(false);
    Ok(newer_launcher(&st, if force { 0 } else { 3600 }, force).await)
}

/// version.json's Linux build as an entry of its own: its version (or the shared one), url and sha256.
fn linux_entry(v: &Value) -> Option<Value> {
    let l = v.get("linux")?;
    let url = str_of(l, "url");
    if url.is_empty() {
        return None;
    }
    let mut out = v.clone();
    // A Linux download must never inherit the Windows checksum or required minimum.
    for k in ["linux", "macos", "url", "sha256", "min", "size"] {
        out.as_object_mut()?.remove(k);
    }
    for k in ["version", "url", "sha256", "notes", "min", "size"] {
        if let Some(x) = l.get(k) {
            out[k] = x.clone();
        }
    }
    Some(out)
}

/// The release on offer is an older version than the one the player has (a version from somewhere
/// else, like a game's own site, that isn't among its GitHub releases). Only when both read as
/// version numbers.
fn older_version(offer: &str, have: &str) -> bool {
    let num = |s: &str| s.trim().trim_start_matches(['v', 'V']).starts_with(|c: char| c.is_ascii_digit());
    num(offer) && num(have) && version_key(offer) < version_key(have)
}

fn launcher_platform_entry(v: &Value, os: &str, arch: &str) -> Option<Value> {
    match os {
        "windows" => Some(v.clone()),
        "linux" => linux_entry(v),
        "macos" => {
            let mac = v.get("macos")?;
            if str_of(mac, "url").is_empty() || ![arch, "universal"].contains(&str_of(mac, "arch")) {
                return None;
            }
            let mut out = v.clone();
            for key in ["linux", "macos", "url", "sha256", "min"] {
                out.as_object_mut()?.remove(key);
            }
            for key in ["version", "url", "sha256", "notes", "min"] {
                if let Some(value) = mac.get(key) {
                    out[key] = value.clone();
                }
            }
            Some(out)
        }
        _ => None,
    }
}

/// 0.5.0-beta.1 < 0.5.0-beta.2 < 0.5.0 < 0.5.1: a beta comes before its release.
fn version_key(s: &str) -> (u64, u64, u64, u64, u64, u64) {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let (core, pre) = s.split_once('-').unwrap_or((s, ""));
    // Each part's leading digits ("10a" is 10).
    let mut n = core.split('.').map(|p| p.trim().chars().take_while(char::is_ascii_digit).collect::<String>().parse::<u64>().unwrap_or(0));
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
/// one after all? id -> true for the ones that do. Answered from the release info on disk (any
/// age); only recomps never looked at before are asked for, a few per launch, one at a time.
#[tauri::command]
async fn build_check(st: State<'_, App>, ids: Vec<String>) -> Res<HashMap<String, bool>> {
    const NEW_PER_LAUNCH: usize = 5;
    let cat = catalog::catalog(&st.http, &st.dir, false).await?.data;
    let (os, arch) = (assets::os_now(), assets::arch_now());
    let mut out = HashMap::new();
    let mut asked = 0;
    for id in ids.into_iter().take(80) {
        let Some(entry) = catalog::app(&cat, &id) else { continue };
        let repo = str_of(entry, "repo");
        if repo.is_empty() {
            continue;
        }
        let f = match catalog::releases_on_disk(&st.dir, repo) {
            Some(f) => f,
            None if asked < NEW_PER_LAUNCH && !net::offline() => {
                asked += 1;
                let Ok(f) = catalog::releases(&st.http, &st.dir, repo, false).await else { continue };
                f
            }
            None => continue,
        };
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
    // From the release info on disk only (the update check keeps it current).
    for g in games.into_iter().filter(|g| !g.repo.is_empty() && !g.local) {
        let Some(f) = catalog::releases_on_disk(&st.dir, &g.repo) else { continue };
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
static MOD_TURNS: std::sync::OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = std::sync::OnceLock::new();

async fn mod_turn(id: &str) -> tokio::sync::OwnedMutexGuard<()> {
    let m = MOD_TURNS.get_or_init(Default::default).lock().unwrap().entry(id.to_string()).or_default().clone();
    m.lock_owned().await
}

/// A mod is going in or coming out for this game right now.
fn mods_busy(id: &str) -> bool {
    MOD_TURNS.get().and_then(|t| t.lock().unwrap().get(id).cloned()).is_some_and(|m| m.try_lock().is_err())
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

/// What a log sent to the website is about, so the admin can sort crash logs: "game" with the game's
/// id when one game's log goes along, "launcher" with no id otherwise.
fn log_source(game: Option<&str>) -> [(String, String); 2] {
    match game.filter(|g| !g.is_empty()) {
        Some(id) => [("source".into(), "game".into()), ("game".into(), id.into())],
        None => [("source".into(), "launcher".into()), ("game".into(), String::new())],
    }
}

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
    form.extend(log_source(Some(&id)));
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
    if let Some(t) = trail::tail_for(&id, 24_000) {
        form.push(("name[]".into(), "launcher-trail.txt".into()));
        form.push(("text[]".into(), t));
    }
    let url = format!("{}/api/logs.php", catalog::base());
    let r = net::send(st.http.post(&url).form(&form).timeout(net::TRANSFER), &url, true).await?;
    let v: Value = r.json().await.unwrap_or(Value::Null);
    if v.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        Ok(str_of(&v, "message").to_string())
    } else {
        Err(Error::Msg(Some(str_of(&v, "error")).filter(|s| !s.is_empty()).unwrap_or("The report couldn't be sent right now.").to_string()))
    }
}

/// The programs that came with a game (its own setup or tools among them), inside its folder.
#[tauri::command(async)]
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
                if os == "macos" && macos::is_bundle(&p) {
                    if let Some(exe) = macos::bundle_executable(&p, assets::arch_now()) {
                        if let Ok(rel) = exe.strip_prefix(&g.path) {
                            out.push(rel.to_string_lossy().to_string());
                        }
                    }
                    continue;
                }
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
#[tauri::command(async)]
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
        ("app".into(), game.as_ref().map(|g| g.id.clone()).unwrap_or_else(|| "recomplauncher".into())),
        ("version".into(), game.as_ref().map(|g| g.tag.clone()).unwrap_or_else(|| env!("CARGO_PKG_VERSION").into())),
        ("category".into(), category.into()),
        ("message".into(), body.chars().take(2000).collect()),
        ("feedback".into(), match mood.as_deref() { Some("great") | Some("good") => "works", Some("broken") => "crashes", _ => "other" }.into()),
        ("build".into(), format!("RecompLauncher {}", env!("CARGO_PKG_VERSION"))),
    ];
    form.extend(log_source(game.as_ref().map(|g| g.id.as_str())));
    if attach {
        let dir = st.dir.clone();
        let pc = tokio::task::spawn_blocking(move || usage::pc_info(&dir)).await.unwrap_or_default();
        // Linux: how the window is drawn (Settings → Appearance → Graphics Mode).
        let drawing = if cfg!(target_os = "linux") {
            let mode = st.lib.lock().unwrap().settings.linux_graphics.clone();
            let on = |k: &str| std::env::var_os(k).is_some_and(|v| v == "1");
            format!(
                "Graphics mode: {}{}{}\n",
                if mode.is_empty() { "auto" } else { &mode },
                if on("WEBKIT_DISABLE_DMABUF_RENDERER") { ", DMABUF off" } else { "" },
                if on("WEBKIT_DISABLE_COMPOSITING_MODE") { ", compositing off" } else { "" }
            )
        } else {
            String::new()
        };
        let mut txt = format!(
            "RecompLauncher {}\nSystem: {} ({})\nWindows/OS: {}\nCPU: {} ({} threads)\nRAM: {} GB\nGPU: {} {}\nSteam Deck: {}\n{drawing}\nInstalled games:\n",
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
        if let Some(t) = game.as_ref().map(|g| trail::tail_for(&g.id, 24_000)).unwrap_or_else(|| trail::tail(24_000)) {
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
    let url = format!("{}/api/logs.php", catalog::base());
    let r = net::send(st.http.post(&url).form(&form).timeout(net::TRANSFER), &url, true).await?;
    let v: Value = r.json().await.unwrap_or(Value::Null);
    if v.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        Ok("Sent. Thanks for testing.".into())
    } else {
        Err(Error::Msg(Some(str_of(&v, "error")).filter(|s| !s.is_empty()).unwrap_or("It couldn't be sent right now. Try again in a bit.").to_string()))
    }
}

/// A launcher download the updater may fetch: HTTPS from GitHub or recomplauncher.com, or from the
/// update server itself (`base`, which RL_API can point at a local http copy of the site for
/// testing). Matched by origin and path, so github.com.evil.example or a sibling path doesn't pass.
pub(crate) fn trusted_update_url(raw: &str, base: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(raw) else { return false };
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return false;
    }
    let under = |origin: &str| {
        let Ok(origin) = reqwest::Url::parse(origin) else { return false };
        let prefix = format!("{}/", origin.path().trim_end_matches('/'));
        url.origin() == origin.origin() && url.path().starts_with(&prefix)
    };
    ["https://github.com", net::SITE, base].into_iter().any(under)
}

/// Download the newer launcher, check it, and start it: the installer on Windows, the AppImage in
/// place of this one on Linux, the .app from the disk image in place of this one on a Mac. Anything
/// else opens the download page.
#[tauri::command]
async fn update_launcher(app: AppHandle, st: State<'_, App>, big_picture: Option<bool>) -> Res<()> {
    let result = update_launcher_inner(&app, &st, big_picture).await;
    if result.is_err() {
        let _ = app.emit("self-update", json!({"failed": true}));
    }
    result
}

async fn update_launcher_inner(app: &AppHandle, st: &App, _big_picture: Option<bool>) -> Res<()> {
    if flatpak::sandboxed() {
        return Err(Error::Msg("RecompLauncher updates through Flatpak. Update it from your software center, or run: flatpak update".into()));
    }
    let v = version_json(&st, 600, true).await.ok_or_else(|| Error::Net("Can't reach recomplauncher.com right now. Try again in a bit.".into()))?;
    let page = format!("{}/download", net::SITE);
    // On Linux an AppImage replaces itself; anything else (a .deb) opens the download page.
    let v = launcher_platform_entry(&v, assets::os_now(), assets::arch_now()).unwrap_or(Value::Null);
    #[cfg(target_os = "linux")]
    if let Some(target) = std::env::var_os("APPIMAGE") {
        // An AppImage reports missing/invalid metadata in the launcher and can be retried there.
        // A missing image on disk is an error too, not a reason to open a browser.
        return appimage_update::update(app, st, Path::new(&target), &v, _big_picture.unwrap_or_else(|| BIG_PICTURE.load(Ordering::Relaxed))).await;
    }
    let file_url = str_of(&v, "url").to_string();
    let ok_host = trusted_update_url(&file_url, &catalog::base());
    // On a Mac the app replaces itself from the release's disk image, when this account can change
    // the folder it's in. Otherwise (run from the disk image, or from a folder only an administrator
    // can change) the player gets the disk image to drag in themselves.
    let mac_app = if cfg!(target_os = "macos") { mac_update::running_bundle().filter(|b| mac_update::can_replace(b)) } else { None };
    let mac_dmg = file_url.to_ascii_lowercase().ends_with(".dmg");
    if cfg!(target_os = "macos") && (mac_app.is_none() || !mac_dmg) {
        return open_thing(&app, if ok_host { &file_url } else { &page }, true);
    }
    if !(cfg!(windows) || mac_app.is_some()) || file_url.is_empty() || !ok_host {
        return open_thing(&app, &page, true);
    }
    let mut r = update_download::response(&st.http, &file_url).await?;
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
    // The installer has to be the version on offer. Another one (a file published by mistake) would
    // install the same version again and again, so it's refused and not offered again.
    let to_ver = str_of(&v, "version").trim_start_matches('v').to_string();
    if cfg!(windows) && !to_ver.is_empty() && !installer_is_version(&bytes, &to_ver) {
        update_strike(&st.dir, &to_ver, &file_url, &want, 2);
        trail::note(format!("The update file for {to_ver} is another version; not offered again"));
        return Err(Error::Msg(format!("The update file for RecompLauncher {to_ver} looks wrong: it's a different version inside. It won't be offered again until it's fixed, and nothing on your PC needs changing.")));
    }
    if let Some(bundle) = mac_app {
        let dmg = std::env::temp_dir().join(format!("RecompLauncher-{to_ver}.dmg"));
        std::fs::write(&dmg, &bytes).map_err(|e| Error::Msg(format!("Couldn't save the new version: {e}")))?;
        let (dmg2, bundle2, ver2) = (dmg.clone(), bundle.clone(), to_ver.clone());
        let put = tokio::task::spawn_blocking(move || mac_update::install(&dmg2, &bundle2, &ver2)).await.map_err(|e| Error::Msg(e.to_string()))?;
        let _ = std::fs::remove_file(&dmg);
        match put {
            Ok(()) => {}
            Err(mac_update::Failed::OtherVersion(found)) => {
                update_strike(&st.dir, &to_ver, &file_url, &want, 2);
                trail::note(format!("The update file for {to_ver} holds {found}; not offered again"));
                return Err(Error::Msg(format!("The update file for RecompLauncher {to_ver} looks wrong: it's a different version inside. It won't be offered again until it's fixed, and nothing on your Mac needs changing.")));
            }
            Err(mac_update::Failed::Msg(m)) => return Err(Error::Msg(m)),
        }
        trail::note(format!("Updated to {to_ver} in place"));
        if !reopen_after_update(&app) {
            mac_update::forget_old(&bundle);
            app.exit(0);
            return Ok(());
        }
        // Noted, so that if the new version fails to start and this one is put back, its next
        // start tells the player (see update_check_last). No installer to run again on a Mac.
        let _ = std::fs::write(st.dir.join("update-pending.json"), json!({"to": to_ver, "from": env!("CARGO_PKG_VERSION"), "at": now(), "installer": "", "url": file_url, "sha256": want}).to_string());
        if let Err(e) = mac_update::start_and_watch(&bundle) {
            mac_update::forget_old(&bundle);
            let _ = std::fs::remove_file(st.dir.join("update-pending.json"));
            return Err(Error::Msg(format!("The new version is in place, but it didn't start: {e}. Open RecompLauncher again.")));
        }
        app.exit(0);
        return Ok(());
    }
    let file = std::env::temp_dir().join(format!("RecompLauncher-{}-setup.exe", str_of(&v, "version").trim_start_matches('v')));
    std::fs::write(&file, &bytes)?;
    // It installs with no window of its own (/S), and RecompLauncher opens again after (/R) unless
    // the player closed it meanwhile. The launcher's window says it's installing, for a moment, then
    // closes so none of our files are in use (the installer closes any copy still open before it
    // copies). /UPDATE: it replaces the files in place and leaves the player's shortcuts alone (a
    // desktop shortcut they deleted stays gone).
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    // Quit during that moment: the launcher is on its way out, so nothing starts behind the
    // player's back, and the next start doesn't take the update for one that failed.
    if update_launch::quitting() {
        return Ok(());
    }
    // Noted, so the next start can tell whether it went in (see update_check_last).
    let _ = std::fs::write(st.dir.join("update-pending.json"), json!({"to": str_of(&v, "version").trim_start_matches('v'), "from": env!("CARGO_PKG_VERSION"), "at": now(), "installer": file, "url": file_url, "sha256": want}).to_string());
    let portable = portable_dir().and_then(|_| exe_dir());
    let reopen = reopen_after_update(&app);
    update_launch::installer_command(&file, portable.as_deref(), update_launch::Install::Quiet { reopen })
        .spawn()
        .map_err(|e| if is_blocked(&e) { defender_msg(e, "the installer") } else { Error::Msg(format!("Couldn't start the installer: {e}")) })?;
    app.exit(0);
    Ok(())
}

/// Whether the launcher opens again after its own update: only while the player still has its
/// window. Checked as the update goes in, so a window opened again from the tray counts.
fn reopen_after_update(app: &AppHandle) -> bool {
    let shown = app.get_webview_window("main").is_some_and(|w| w.is_visible().unwrap_or(true));
    update_launch::reopen_after(shown, update_launch::quitting())
}

// ---------- the launcher ----------

#[tauri::command(async)]
fn get_state(st: State<'_, App>) -> Value {
    let l = st.lib.lock().unwrap();
    let moving = st.moving.lock().unwrap().clone();
    // Games whose folder isn't there right now stay in the Library, marked missing.
    let missing: Vec<String> = l.missing().into_iter().filter(|id| !moving.contains(id)).collect();
    let mut running: Vec<String> = st.running.lock().unwrap().keys().cloned().collect();
    // And the game being played in each, when it's one of several (its full id).
    let playing: Vec<String> = PLAYING_GAME.lock().unwrap().as_ref().map(|m| m.iter().filter(|(k, v)| running.contains(k) && k != v).map(|(_, v)| v.clone()).collect()).unwrap_or_default();
    running.extend(playing);
    let jobs: Vec<String> = st.jobs.lock().unwrap().keys().cloned().collect();
    json!({
        "os": assets::os_now(),
        "arch": assets::arch_now(),
        // Run as a Flatpak: files are picked with the system's picker (the sandbox sees few folders).
        "flatpak": flatpak::sandboxed(),
        "version": env!("CARGO_PKG_VERSION"),
        "site": catalog::base(),
        "thumbs": net::THUMBS,
        "library": l.installed,
        "arts": l.arts,
        "settings": l.settings,
        "roms": l.roms,
        "running": running,
        "jobs": jobs,
        "steam": shortcuts::steam_dir().is_some(),
        "pictures": ingame::shots_dir("").parent().map(|p| p.to_string_lossy().to_string()),
        "deck": shortcuts::is_steam_deck(),
        "bigArg": std::env::args().any(|a| a == "--bigpicture"),
        "desktopArg": std::env::args().any(|a| a == "--desktop"),
        "missing": missing,
        // Where the web view can't play a sound (an AppImage, run as one or unpacked, or a PC
        // without GStreamer's base plugins) it stops the moment one starts. The page then plays none.
        "noSound": no_sound(),
        // Where the web view draws on the processor, the page leaves out what keeps moving.
        "cpuDraw": cpu_draw(),
        "updateFailed": UPDATE_FAILED.lock().unwrap().clone(),
    })
}

/// The web view can't play a sound here (see sysenv::sound_works).
fn no_sound() -> bool {
    #[cfg(target_os = "linux")]
    return !sysenv::sound_works();
    #[cfg(not(target_os = "linux"))]
    false
}

/// The web view draws on the processor here (see sysenv::draws_on_cpu).
fn cpu_draw() -> bool {
    #[cfg(target_os = "linux")]
    return sysenv::draws_on_cpu();
    #[cfg(not(target_os = "linux"))]
    false
}

/// An update started from inside the launcher that didn't go in (this is still the version it
/// started from): { to, installer }. The window offers to run that installer the usual way, where
/// the player sees what stops it.
static UPDATE_FAILED: Mutex<Option<Value>> = Mutex::new(None);

/// At start: did the last update from inside the launcher go in?
fn update_check_last(dir: &Path) {
    let f = dir.join("update-pending.json");
    let Some(v) = std::fs::read(&f).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else { return };
    let (to, from, at) = (str_of(&v, "to").to_string(), str_of(&v, "from").to_string(), v.get("at").and_then(Value::as_u64).unwrap_or(0));
    let me = env!("CARGO_PKG_VERSION");
    if me == to || me != from || now().saturating_sub(at) > 3 * 86400 {
        let _ = std::fs::remove_file(&f);
        return;
    }
    let installer = str_of(&v, "installer").to_string();
    // The second time the same file doesn't go in, it's the file: it isn't offered again.
    let fails = update_strike(dir, &to, str_of(&v, "url"), str_of(&v, "sha256"), 1);
    trail::note(format!("The update to {to} didn't install (this is still {me}; {fails} time(s) with this file)"));
    *UPDATE_FAILED.lock().unwrap() = Some(json!({"to": to, "installer": installer, "there": Path::new(&installer).is_file(), "bad": fails >= 2}));
    let _ = std::fs::remove_file(&f);
}

/// Runs the update's installer the usual way (its own window, not the quiet one), so whatever stops
/// it is on screen. The launcher closes so its files are free.
#[tauri::command]
fn update_retry(app: AppHandle) -> Res<()> {
    let installer = UPDATE_FAILED.lock().unwrap().as_ref().map(|v| str_of(v, "installer").to_string()).unwrap_or_default();
    if installer.is_empty() || !Path::new(&installer).is_file() {
        return Err(Error::Msg("The installer isn't there anymore. Use Update again to download it.".into()));
    }
    let portable = portable_dir().and_then(|_| exe_dir());
    update_launch::installer_command(Path::new(&installer), portable.as_deref(), update_launch::Install::Shown).spawn().map_err(|e| if is_blocked(&e) { defender_msg(e, "the installer") } else { Error::Msg(format!("Couldn't start the installer: {e}")) })?;
    app.exit(0);
    Ok(())
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
    // The saves folder changes only through its own button (the copies move with it).
    let save_dir = l.settings.save_dir.clone();
    // The proxy changes only through its own command (its password is sealed there).
    let proxy = (l.settings.proxy.clone(), l.settings.proxy_url.clone(), l.settings.proxy_user.clone(), l.settings.proxy_pass.clone());
    l.settings = settings;
    l.settings.tray_told |= told;
    l.settings.save_dir = save_dir;
    (l.settings.proxy, l.settings.proxy_url, l.settings.proxy_user, l.settings.proxy_pass) = proxy;
    pads::STICK.store(l.settings.pad_stick, Ordering::Relaxed);
    l.save(&st.dir)?;
    Ok(())
}

/// Settings → Network: the proxy every request goes through. `pass` None keeps the saved one; the
/// password is sealed like the other secrets and never comes back to the window.
#[tauri::command(async)]
fn set_proxy(st: State<'_, App>, mode: String, url: String, user: String, pass: Option<String>) -> Res<Value> {
    let mut l = st.lib.lock().unwrap();
    let pass = match pass {
        Some(p) => p,
        None => secret::open(&l.settings.proxy_pass).unwrap_or_default(),
    };
    net::set_proxy(&mode, &url, &user, &pass).map_err(Error::Msg)?;
    l.settings.proxy = mode;
    l.settings.proxy_url = url.trim().to_string();
    l.settings.proxy_user = user;
    l.settings.proxy_pass = if pass.is_empty() { String::new() } else { secret::seal(&pass).unwrap_or_default() };
    l.save(&st.dir)?;
    Ok(json!({ "shown": net::proxy_shown(), "has_pass": !l.settings.proxy_pass.is_empty() }))
}

/// Where the copies of the player's saves go: a folder they pick (another drive, a NAS, a folder
/// Drive, OneDrive or Dropbox keeps in sync), or none for the launcher's own. The copies already
/// made move along, each checked before the old one goes.
#[tauri::command(async)]
fn saves_set_root(st: State<'_, App>, path: Option<String>) -> Res<Value> {
    let to = path.map(PathBuf::from).filter(|p| !p.as_os_str().is_empty());
    let own = st.dir.join("saves");
    if let Some(p) = &to {
        if !p.is_absolute() {
            return Err(Error::Msg("Pick folders, not names.".into()));
        }
        if p.starts_with(&own) || p.starts_with(&st.dir) {
            return Err(Error::Msg("That's inside the launcher's own folder. Pick a folder of yours, on any drive.".into()));
        }
        let probe = p.join(".recomplauncher-check");
        if std::fs::create_dir_all(p).and_then(|_| std::fs::write(&probe, b"ok")).is_err() {
            return Err(Error::Msg("The launcher can't write to that folder. Pick another one.".into()));
        }
        let _ = std::fs::remove_file(&probe);
    }
    let from = saves::root(&st.dir);
    let target = to.clone().unwrap_or_else(|| own.clone());
    let moved = if from != target && from.is_dir() {
        saves::move_copies(&from, &target).map_err(|e| Error::Msg(format!("Couldn't move the copies there: {e}")))?
    } else {
        0
    };
    saves::set_root(to.clone());
    {
        let mut l = st.lib.lock().unwrap();
        l.settings.save_dir = to.clone();
        l.save(&st.dir)?;
    }
    trail::note(format!("Saves: copies go to {} now ({moved} games moved)", if to.is_some() { "the player's folder" } else { "the launcher's folder" }));
    Ok(json!({ "path": saves::root(&st.dir), "custom": to.is_some(), "moved": moved }))
}

/// Opens the folder the copies of the player's saves are in.
#[tauri::command(async)]
fn saves_open_root(app: AppHandle, st: State<'_, App>) -> Res<()> {
    let p = saves::root(&st.dir);
    let _ = std::fs::create_dir_all(&p);
    open_thing(&app, &p.to_string_lossy(), false)
}

/// Linux: tools for the Wine prefix every Windows game here shares, for what a game's setup says
/// is missing: Winetricks ("winetricks"), a Windows program run in it ("run", an installer for a
/// runtime), or its folder ("folder"). Gives back what happened.
#[tauri::command]
async fn wine_tools(app: AppHandle, st: State<'_, App>, what: String, path: Option<String>) -> Res<String> {
    if cfg!(windows) {
        let _ = (&app, &path);
        return Err(Error::Msg("Wine tools are for Linux.".into()));
    }
    let r = proton::runner().ok_or_else(|| Error::Msg("Windows games need Proton or Wine on this PC first. Settings → General → GE-Proton installs it in one press.".into()))?;
    match what.as_str() {
        "folder" => {
            let d = proton::prefix_dir(&r, &st.dir);
            if !d.is_dir() {
                return Err(Error::Msg("There's no Wine prefix yet. Play a Windows game here once and it's made.".into()));
            }
            open_thing(&app, &d.to_string_lossy(), false)?;
            Ok("open".into())
        }
        "run" => {
            let p = PathBuf::from(path.unwrap_or_default());
            let ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            if !p.is_file() || !["exe", "msi"].contains(&ext.as_str()) {
                return Err(Error::Msg("Pick a Windows program or installer (.exe or .msi).".into()));
            }
            // An .msi goes through Windows' installer, in the prefix.
            let mut c = if ext == "msi" {
                let mut c = proton::std_command(&r, &st.dir, Path::new("msiexec"));
                c.arg("/i").arg(&p);
                c
            } else {
                proton::std_command(&r, &st.dir, &p)
            };
            sysenv::clean_std(&mut c);
            c.current_dir(p.parent().unwrap_or(Path::new("/"))).spawn().map_err(|e| Error::Msg(format!("Couldn't start it: {e}")))?;
            trail::note(&format!("Wine tools: ran {} in the prefix ({})", p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), r.name()));
            Ok("started".into())
        }
        "winetricks" => {
            // Not on this PC (a Steam Deck): the launcher keeps its own copy of the script.
            let own = st.dir.join("tools").join("winetricks");
            if !matches!(r, proton::Runner::Umu(..)) && !own.is_file() && proton::winetricks_command(&r, &st.dir, None).is_err() {
                let url = "https://raw.githubusercontent.com/Winetricks/winetricks/master/src/winetricks";
                if let Ok(resp) = net::send(st.http.get(url).timeout(std::time::Duration::from_secs(60)), url, false).await {
                    if resp.status().is_success() {
                        if let Ok(b) = resp.bytes().await {
                            if b.starts_with(b"#!") {
                                let _ = std::fs::create_dir_all(own.parent().unwrap());
                                if std::fs::write(&own, &b).is_ok() {
                                    #[cfg(unix)]
                                    {
                                        use std::os::unix::fs::PermissionsExt;
                                        let _ = std::fs::set_permissions(&own, std::fs::Permissions::from_mode(0o755));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            let mut c = proton::winetricks_command(&r, &st.dir, Some(&own)).map_err(Error::Msg)?;
            sysenv::clean_std(&mut c);
            c.spawn().map_err(|e| Error::Msg(format!("Couldn't open Winetricks: {e}")))?;
            trail::note(&format!("Wine tools: Winetricks opened ({})", r.name()));
            Ok("open".into())
        }
        _ => Err(Error::Msg("That isn't one of the Wine tools.".into())),
    }
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
                save_dir: old.save_dir.clone(),
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

/// The quick menu's window: a see-through window over the game, hidden until it's asked for.
/// It's made the first time it's opened, so a launcher that never opens it never pays for a
/// second WebKit process.
pub(crate) fn build_overlay(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    let w = tauri::WebviewWindowBuilder::new(app, "overlay", tauri::WebviewUrl::App("overlay.html".into()))
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
    rest(&w, true);
    Ok(w)
}

/// The main window is out of sight (minimized, or closed to the tray).
static AWAY: AtomicBool = AtomicBool::new(false);

/// Set by the page once its first real screen is drawn. The window stays hidden until then, so the
/// player never sees the empty first draw being replaced by the real one.
static UI_READY: AtomicBool = AtomicBool::new(false);

/// How long to wait for the page before showing the window anyway (a slow or broken start must
/// still open a window).
const UI_READY_WAIT_MS: u64 = 4000;

#[tauri::command]
fn ui_ready() {
    UI_READY.store(true, Ordering::Relaxed);
}

async fn wait_for_ui() {
    let step = std::time::Duration::from_millis(20);
    let mut waited = 0;
    while !UI_READY.load(Ordering::Relaxed) && waited < UI_READY_WAIT_MS {
        tokio::time::sleep(step).await;
        waited += 20;
    }
}
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
    st.lib.lock().unwrap().forget(&id);
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
#[tauri::command(async)]
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

/// A game link from the website: recomplauncher://game/<id>, with a plain catalog id (letters,
/// numbers, dashes). Anything else in the link is ignored, and any other link is no link at all.
/// It only ever opens the game's page: nothing is installed or started from a link.
fn link_game(link: &str) -> Option<String> {
    let link = link.trim();
    let (scheme, rest) = link.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("recomplauncher") {
        return None;
    }
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    let mut parts = rest.split('/').filter(|p| !p.is_empty());
    if !parts.next()?.eq_ignore_ascii_case("game") {
        return None;
    }
    let id = parts.next()?;
    (id.len() <= 80 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')).then(|| id.to_string())
}

/// The game a link asked for while the window was still starting: the page takes it once the
/// catalog is in.
static LINK_WAITING: Mutex<Option<String>> = Mutex::new(None);
/// The last link handled, and when: the same link arriving twice at once (two ways in) opens once.
static LINK_LAST: Mutex<Option<(String, Instant)>> = Mutex::new(None);

/// A game link for the launcher that's running: its window to the front, on that game's page.
fn open_link(app: &AppHandle, id: String) {
    {
        let mut last = LINK_LAST.lock().unwrap();
        if matches!(&*last, Some((l, at)) if *l == id && at.elapsed() < std::time::Duration::from_secs(3)) {
            return;
        }
        *last = Some((id.clone(), Instant::now()));
    }
    trail::note(format!("Link: open game {id}"));
    show_main(app);
    *LINK_WAITING.lock().unwrap() = Some(id.clone());
    let _ = app.emit("open-game", id);
}

/// Easy Install, before the download: is this the copy the game needs? The same checks its setup
/// runs (kind of file, disc and version), from the catalog's setup for it, so a wrong copy is found
/// now instead of after the download.
#[tauri::command(async)]
fn precheck_copy(st: State<'_, App>, id: String, path: String) -> Res<String> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(Error::Msg("That file isn't there anymore.".into()));
    }
    let cat = cached_catalog(&st);
    let entry = catalog::app(&cat, &id).cloned().unwrap_or(Value::Null);
    // Nothing is installed yet, so only the catalog's setup counts (a folder that isn't there).
    let none = st.dir.join("precheck").join(&id);
    let prof = profile::find(&none, str_of(&entry, "repo"), &entry).ok_or_else(|| Error::Msg("This game's copy can only be checked once it's installed.".into()))?;
    let g = prof.game.as_ref().filter(|_| prof.inputs.is_empty()).ok_or_else(|| Error::Msg("This game's copy can only be checked once it's installed.".into()))?;
    profile::check_copy(g, &p).map_err(Error::Msg)?;
    roms::fits_copy_ids(&entry, &p).map_err(|found| Error::Msg(format!("That copy reads as {found}, and this recomp is made for another version of the game.")))?;
    Ok(p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())
}

/// The game folders the player's emulators know about, that aren't among their game folders yet.
#[tauri::command(async)]
fn emulator_folders(st: State<'_, App>) -> Vec<emus::Found> {
    // The Flatpak can't read the emulators' settings (they're outside its sandbox): the player adds
    // the folders by hand.
    if flatpak::sandboxed() {
        return vec![];
    }
    let have = st.lib.lock().unwrap().settings.rom_dirs.clone();
    emus::find().into_iter().filter(|f| !have.iter().any(|h| emus::within(&f.path, h))).collect()
}

/// ~/Games/ROMs, made if it isn't there: a ROM folder the Flatpak reads directly (its sandbox
/// allows ~/Games), without the system's picker.
#[tauri::command(async)]
fn games_rom_folder() -> Res<String> {
    let dir = dirs::home_dir().ok_or_else(|| Error::Msg("Couldn't find your home folder.".into()))?.join("Games/ROMs");
    std::fs::create_dir_all(&dir).map_err(|_| Error::Msg("Couldn't create the ROMs folder in Games.".into()))?;
    Ok(dir.to_string_lossy().to_string())
}

/// For the page as it starts: a game a link asked for before it was ready, once.
#[tauri::command]
fn take_link() -> Option<String> {
    LINK_WAITING.lock().unwrap().take()
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
#[tauri::command(async)]
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

/// A marker left by a launcher that was killed (or a session that ended) names a process that's
/// gone: it's cleared rather than trusted. An empty marker (older launchers) is judged by its age.
fn marker_alive(f: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(f) else { return false };
    let Ok(pid) = text.trim().parse::<u32>() else { return true };
    let alive = process_alive(pid);
    if !alive {
        let _ = std::fs::remove_file(f);
    }
    alive
}

fn process_alive(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    return Path::new(&format!("/proc/{pid}")).exists();
    #[cfg(windows)]
    return win::processes().iter().any(|(p, _)| *p == pid);
    #[allow(unreachable_code)]
    {
        let _ = pid;
        true
    }
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
    let lib = Library::load(&dir);
    let id = lib.resolve_project_id(&id);
    // One game of several: the recomp's marker is the one written.
    let id = games::split(&id).filter(|(r, _)| !lib.installed.contains_key(&id) && lib.installed.contains_key(*r)).map(|(r, _)| r.to_string()).unwrap_or(id);
    let Ok(exe) = std::env::current_exe() else { return false };
    let rest = sysenv::play_handoff_args(args, &|k| if cfg!(target_os = "linux") { std::env::var(k).ok() } else { None });
    if std::process::Command::new(exe).args(&rest).spawn().is_err() {
        return false;
    }
    let file = playing_file(&dir, &id);
    let fresh = |f: &Path| marker_alive(f) && std::fs::metadata(f).ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).map(|e| e.as_secs() < 90).unwrap_or(false);
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
/// The single-instance plugin, on the Flatpak's own D-Bus name when the launcher runs as one.
fn single_instance<R: tauri::Runtime>() -> tauri_plugin_single_instance::Builder<R> {
    let b = tauri_plugin_single_instance::Builder::new();
    #[cfg(target_os = "linux")]
    if flatpak::sandboxed() {
        if let Ok(id) = std::env::var("FLATPAK_ID") {
            return b.dbus_id(id);
        }
    }
    b
}

pub fn run() {
    #[cfg(target_os = "linux")]
    sysenv::prepare_start(data_dir_early());
    #[cfg(target_os = "linux")]
    flatpak::name_windows_after_app();
    if wait_for_play(&std::env::args().collect::<Vec<_>>()) {
        return;
    }
    // Keep what the session's bursts leave in the allocator's arenas from staying (on Linux).
    sysenv::tune_allocator();
    tauri::Builder::default()
        // A second launch (a shortcut, a Steam entry) hands its job to the launcher already running.
        // `--quit` (from the installer and uninstaller): the one running closes, wherever it is.
        // On D-Bus as the app id, not the Tauri identifier: a Flatpak may only own names under its
        // own id, so a second copy would start instead of handing over.
        .plugin(single_instance().callback(|app, argv, _cwd| match play_arg(&argv) {
            _ if argv.iter().any(|a| a == "--quit") => app.exit(0),
            // A game link from the website, clicked while the launcher is open (or by the clock).
            _ if argv.iter().skip(1).any(|a| a.to_ascii_lowercase().starts_with("recomplauncher:")) => {
                if let Some(id) = argv.iter().skip(1).find_map(|a| link_game(a)) {
                    open_link(app, id);
                } else {
                    show_main(app);
                }
            }
            Some(id) => {
                let id = app.state::<App>().lib.lock().unwrap().resolve_project_id(&id);
                let steam = sysenv::steam_launch_from_args(&argv);
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let rid = recomp_of(&app.state::<App>(), &id);
                    if setup_first(&app, &rid) {
                        return;
                    }
                    ready_look(&app, &rid).await;
                    let sync = cloud::sync_quiet(&app, &id).await;
                    if let Err(e) = sync.and_then(|()| start_game_with_steam(&app, &id, steam.as_ref())) {
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
        }).build())
        .on_window_event(|w, ev| {
            if w.label() != "main" {
                return;
            }
            match ev {
                // Run in background hides the window only when the tray can bring it back.
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    let app = w.app_handle();
                    let background = app.state::<App>().lib.lock().unwrap().settings.close_to_tray;
                    api.prevent_close();
                    if background && app.tray_by_id("main").is_some() && w.hide().is_ok() {
                        away(w, true);
                        tray::hid(app);
                    } else {
                        // Closing for real: the whole launcher goes. The hidden quick-menu window
                        // would otherwise keep it running with no window, and opening it again
                        // would find nothing to show.
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
        // recomplauncher:// links from the website.
        .plugin(tauri_plugin_deep_link::init())
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
            update_check_last(&dir);
            #[cfg(target_os = "linux")]
            if let Some(image) = std::env::var_os("APPIMAGE") {
                appimage_update::clean_stale(Path::new(&image));
            }
            // Downloads slow down while a game is running (if the player wants that).
            {
                let h = app.handle().clone();
                install::playing_probe(move || h.try_state::<App>().map(|s| !s.running.lock().unwrap().is_empty()).unwrap_or(false));
            }
            // Started by a game link: the page opens that game once it's ready.
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                let first = app.deep_link().get_current().ok().flatten().into_iter().flatten().map(|u| u.to_string())
                    .chain(std::env::args().skip(1))
                    .find_map(|a| link_game(&a));
                if let Some(id) = first {
                    trail::note(format!("Link: started for game {id}"));
                    *LINK_WAITING.lock().unwrap() = Some(id.clone());
                    *LINK_LAST.lock().unwrap() = Some((id, Instant::now()));
                }
                // A Mac hands links to the running launcher this way.
                let h = app.handle().clone();
                app.deep_link().on_open_url(move |ev| {
                    if let Some(id) = ev.urls().iter().find_map(|u| link_game(u.as_str())) {
                        open_link(&h, id);
                    }
                });
                // Windows (a portable copy too) and Linux (an AppImage, the .deb, a build of its
                // own) tell the system about the links each time, with the link passed along.
                #[cfg(any(windows, target_os = "linux"))]
                {
                    let h = app.handle().clone();
                    std::thread::spawn(move || {
                        if let Err(e) = h.deep_link().register_all() {
                            trail::note(format!("Link: couldn't register recomplauncher:// ({e})"));
                        }
                    });
                }
            }
            net::init(app.handle());
            net::on_back(back_online);
            // While Offline, the site is asked again once its rest is over (five minutes at most),
            // so the mark clears by itself when the site is back.
            {
                let app = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
                        if net::offline() {
                            let http = app.state::<App>().http.clone();
                            net::probe(&http).await;
                        }
                    }
                });
            }
            proton::set_data(&dir);
            // The files an update moved aside (they were in use while it copied).
            #[cfg(windows)]
            if let Some(d) = exe_dir() {
                for f in ["recomplauncher.exe.old", "WebView2Loader.dll.old"] {
                    let _ = std::fs::remove_file(d.join(f));
                }
            }
            // The player's proxy (Settings → Network): off, the system's, or their own.
            {
                let l = Library::load(&dir);
                let pass = secret::open(&l.settings.proxy_pass).unwrap_or_default();
                if let Err(e) = net::set_proxy(&l.settings.proxy, &l.settings.proxy_url, &l.settings.proxy_user, &pass) {
                    trail::note(format!("Proxy setting not used: {e}"));
                }
            }
            let http = net::with_proxy(reqwest::Client::builder())
                .user_agent(concat!("RecompLauncher/", env!("CARGO_PKG_VERSION")))
                // Redirects are followed (GitHub's and SourceForge's downloads go through them), but
                // never from https down to plain http.
                .redirect(reqwest::redirect::Policy::custom(|a| {
                    if a.previous().len() >= 10 {
                        a.error("too many redirects")
                    } else if a.url().scheme() != "https" && a.previous().iter().any(|u| u.scheme() == "https") {
                        a.error("a redirect away from https")
                    } else {
                        a.follow()
                    }
                }))
                // Slow DNS or a busy mobile connection can take a while to connect.
                .connect_timeout(std::time::Duration::from_secs(15))
                .build()?;
            #[allow(unused_mut)]
            let mut lib = Library::load(&dir);
            // Save copies in the player's own folder, when they picked one; any made here while it
            // was away go to it.
            saves::set_root(lib.settings.save_dir.clone());
            {
                let d = dir.clone();
                std::thread::spawn(move || saves::gather(&d));
            }
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
            let tv = !std::env::args().any(|a| a == "--desktop") && (lib.settings.start_in_tv || std::env::args().any(|a| a == "--bigpicture")) && lib.settings.setup_done;
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
            let play = play_arg(&std::env::args().collect::<Vec<_>>()).map(|id| lib.resolve_project_id(&id));
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
            // Achievements: what the website said about this account last time.
            ach::init(&app.state::<App>().dir);
            #[cfg(windows)]
            watch_outside(app.handle().clone());
            // Text Size from last time.
            {
                let z = app.state::<App>().lib.lock().unwrap().settings.ui.get("ui_scale").and_then(Value::as_f64).filter(|z| (0.5..=2.0).contains(z));
                if let (Some(z), Some(w)) = (z, app.get_webview_window("main")) {
                    let _ = w.set_zoom(z);
                }
            }
            // Linux: scrolling eases like the rest of the desktop does, instead of jumping a notch
            // at a time.
            #[cfg(target_os = "linux")]
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.with_webview(|wv| {
                    use webkit2gtk::{SettingsExt, WebViewExt};
                    if let Some(s) = wv.inner().settings() {
                        s.set_enable_smooth_scrolling(true);
                    }
                });
            }
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
            // NVIDIA DLSS: when recomplauncher.com names a newer version than the one kept, it's
            // downloaded in the background. Games get it at their next setup or launch, never
            // in the middle of play.
            {
                let a = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    let st = a.state::<App>();
                    if dlss::current(&st.dir).is_some() && dlss::applies(&st.dir) {
                        let _ = dlss::ensure(&st.http, &st.dir, &|_, _| {}).await;
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
            pads::STICK.store(app.state::<App>().lib.lock().unwrap().settings.pad_stick, Ordering::Relaxed);
            pads::start(app.handle().clone());
            if let Err(e) = tray::start(app.handle()) {
                eprintln!("No tray icon: {e}");
            }
            // Games sent from the website, while signed in.
            cloud::watch_inbox(app.handle().clone());
            // Settings → Keep Steam in Sync: written whenever Steam is found closed.
            steam_sync::start(app.handle().clone());
            // A new RecompLauncher: the window looks as it opens; after that, every six hours or so
            // while it runs (it may stay by the clock). A push brings news of a new build sooner.
            {
                let h = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let mut told = String::new();
                    loop {
                        tokio::time::sleep(net::spread(LAUNCHER_CHECK_EVERY, 0.1)).await;
                        let st = h.state::<App>();
                        if let Some(v) = newer_launcher(&st, 0, false).await {
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
            // Screenshots show in the window straight from their folder, wherever Pictures is. The
            // folder itself is only made when the first screenshot is taken.
            if let Some(root) = ingame::shots_dir("x").parent() {
                let _ = app.asset_protocol_scope().allow_directory(root, true);
            }
            // Covers and backgrounds the player picked, wherever the launcher keeps its data (the
            // portable data folder too, which the usual app data rule doesn't reach).
            {
                let covers = app.state::<App>().dir.join("covers");
                let _ = std::fs::create_dir_all(&covers);
                let _ = app.asset_protocol_scope().allow_directory(&covers, true);
            }
            let handle = app.handle().clone();
            match play {
                Some(id) => {
                    tauri::async_runtime::spawn(async move {
                        let rid = recomp_of(&handle.state::<App>(), &id);
                        if setup_first(&handle, &rid) {
                            return;
                        }
                        ready_look(&handle, &rid).await;
                        let sync = cloud::sync_quiet(&handle, &id).await;
                        if let Err(e) = sync.and_then(|()| start_game(&handle, &id)) {
                            show_main(&handle);
                            let _ = handle.emit("toast", e.to_string());
                        }
                    });
                }
                None => {
                    if let Some(w) = app.get_webview_window("main") {
                        restore_window_place(&w, &app.state::<App>().dir);
                    }
                    tauri::async_runtime::spawn(async move {
                        wait_for_ui().await;
                        let Some(w) = handle.get_webview_window("main") else { return };
                        if tv {
                            let _ = w.set_fullscreen(true);
                        }
                        let _ = w.show();
                    });
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            take_link, ui_ready, pause, download_limits, installer::installer_run, installer::installer_cancel, installer::installer_check, installer::installer_done,
            precheck_copy,
            emulator_folders,
            games_rom_folder,
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
            get_state, get_catalog, get_releases, net_state, check_updates, launcher_update, install, cancel, uninstall, play, stop, set_exe, open_folder,
            save_settings, open_url, libraries, move_game, game_sizes, backups, backup_now, restore_backup, pc_check, set_ui_scale, update_retry, ge_proton_install, proton_now, repo_readme, install_vcredist, scan_roms, rom_matches, copy_files, support_local,
            desktop_shortcut, open_app_control, fs_paste, portable_info, make_portable, add_to_steam, launcher_to_steam, steam_sync::steam_sync_now, steam_sync::steam_sync_state, recent_log, screenshots, open_shots, overlay_action, overlay_state, is_handheld, skip_backup, set_cover, art::art_source, art::art_open, art::art_save, art::art_search, art::art_for, all_screenshots, delete_screenshot,
            copy_screenshot, show_screenshot, controllers, prefetch_images, versions, build_check, news, game_info, mods_state, mods_online,
            mod_install, mod_remove, open_mods, send_report, beta_feedback, game_programs, run_program, game_root, boot::boot_builtin, update_launcher, resume_game, game_setup, set_game_copy, set_update_copy, set_game_setting, set_game_slider, set_game_binding, reset_game_bindings, usage::report_preview, usage::profile_report, usage::my_reports, usage::report_reply,
            recomp_games, adopt::move_listing, update_info, set_proxy, umu_repair, dlss_fetch, dlss_set, dlss_state, dlss_remove, cloud::library_pull, cloud::library_push, cloud::saves_open_backups, game_action, setup_copy, setup_cancel, setup_input_set, setup_key_remove, setup_inputs_run, easy_setup, set_game_options, looks_info, set_look, look_picture, ui_data,
            reset_settings, tray::tray_words, ach::ach_access, ach::ach_status, ach::ach_apply, ach::ach_profile, ach::ach_games, ach::ach_board, ach::ach_game_web, ach::ach_report, ach::ach_tracker_get, ach::ach_game, ach::ach_set_disc, ach::ach_sound, ach::ach_log, ach::ach_peek, ach::ach_rule, ach::ach_reset, ach::ach_test_toast, ach::ach_toast_take, ach::ach_toast_done, cloud::account_start, cloud::account_poll, cloud::account_cancel, cloud::account_state, cloud::account_signout,
            cloud::saves_check, cloud::saves_pick, cloud::saves_cloud, download_history, report_counts, play_reports, cloud::saves_info, cloud::saves_upload_now, cloud::cloud_retry, cloud::saves_overview, cloud::saves_backup, cloud::saves_restore, cloud::saves_share, cloud::saves_forget_found, cloud::saves_set_place, cloud::saves_add_place, cloud::saves_remove_place, cloud::saves_open, saves_set_root, saves_open_root, missing_files, missing_made, dll_check, dll_aside, gpu_choice, prefer_fast_gpu, copy_check, wine_tools, deck_scores, team_notes, usage::notifies, usage::notify, usage::pc_specs, usage::recomp_request,
            adopt::adopt_scan, adopt::migrate_scan, adopt::migrate_pick, adopt::adopt_pick, adopt::adopt_add, adopt::adopt_check, adopt::set_project, usage::usage_hello, usage::usage_event, usage::data_ids, usage::reset_ids, ping::usage_open, ping::usage_mode, usage::run_report, cloud::inbox, cloud::inbox_done, cloud::betas, cloud::beta_join, cloud::beta_leave, launcher_notes, scan_info, devtools::dev_add, devtools::add_any, devtools::dev_check, devtools::dev_starter, fs_places, fs_list, fs_extract, gb_mods, gb_mod, gb_install
        ])
        .build(tauri::generate_context!())
        .expect("RecompLauncher couldn't start")
        .run(|app, ev| {
            match ev {
                #[cfg(target_os = "macos")]
                // Dock and Finder must restore the main window even if a helper window is visible.
                tauri::RunEvent::Reopen { .. } => show_main(app),
                // Every way out (the window, the tray's Quit, --quit) asks here first. An update still
                // on its way then leaves the launcher closed (see update_launcher).
                tauri::RunEvent::ExitRequested { .. } => update_launch::note_quitting(),
                tauri::RunEvent::Exit => {
                    pads::stop();
                    ping::close(app);
                }
                _ => {}
            }
        });
}

#[cfg(test)]
mod tests {

    #[test]
    fn the_badge_check_and_updates_tab_share_one_rule() {
        use super::{offered_update, Installed, Release};
        let rel = |tag: &str, files: &[&str]| -> Release {
            serde_json::from_value(serde_json::json!({ "tag_name": tag, "published_at": "2026-11-01T00:00:00Z",
                "assets": files.iter().map(|f| serde_json::json!({ "name": f, "browser_download_url": format!("https://github.com/a/b/releases/download/{tag}/{f}"), "size": 10 })).collect::<Vec<_>>() })).unwrap()
        };
        // Newest first, like GitHub: v3 isn't checked by the store yet, v2 is, v1 is installed.
        let all = ["game-linux-x64.tar.gz", "game-windows-x64.zip", "game-macos-arm64.zip"];
        let rels = vec![rel("v3", &all), rel("v2", &all), rel("v1", &all)];
        let entry = serde_json::json!({ "id": "g", "tag": "v2" });
        let g = Installed { id: "g".into(), tag: "v1".into(), ..Default::default() };
        // The checked release, not the newest one on GitHub.
        assert_eq!(offered_update(&g, Some(&entry), &rels).as_deref(), Some("v2"));
        // Already on it: nothing, even though GitHub has v3.
        let on2 = Installed { tag: "v2".into(), ..g.clone() };
        assert_eq!(offered_update(&on2, Some(&entry), &rels), None);
        // Played through Proton (Linux): the Windows build counts.
        let proton = Installed { proton: true, ..g.clone() };
        let win_only = vec![rel("v2", &["game-windows-x64.zip"]), rel("v1", &["game-windows-x64.zip"])];
        #[cfg(target_os = "linux")]
        assert_eq!(offered_update(&proton, Some(&entry), &win_only).as_deref(), Some("v2"));
        let _ = &proton;
        // A release with no build for this system isn't offered (on Linux the Windows-only one).
        #[cfg(target_os = "linux")]
        assert_eq!(offered_update(&g, Some(&entry), &win_only), None);
        // Picked by the player: never.
        let pinned = Installed { pinned: true, ..g.clone() };
        assert_eq!(offered_update(&pinned, Some(&entry), &rels), None);
        // No checked release in the list: the newest one.
        assert_eq!(offered_update(&g, Some(&serde_json::json!({ "id": "g" })), &rels).as_deref(), Some("v3"));
    }

    #[test]
    fn a_game_that_hands_off_keeps_counting_while_its_folder_runs() {
        assert!(leftover_helper("UnityCrashHandler64.exe"));
        assert!(leftover_helper("crashpad_handler"));
        assert!(!leftover_helper("Hookshot.exe"));
        // Too broad a folder never counts.
        assert!(!still_running_in(Path::new("/")));
        #[cfg(target_os = "linux")]
        {
            // A program running from the folder counts; one from elsewhere doesn't.
            let d = std::env::temp_dir().join(format!("rl-handoff-{}", std::process::id())).join("game");
            std::fs::create_dir_all(&d).unwrap();
            let prog = d.join("game-bin");
            std::fs::copy("/bin/sleep", &prog).unwrap();
            let mut c = std::process::Command::new(&prog).arg("5").spawn().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(300));
            assert!(still_running_in(&d));
            c.kill().unwrap();
            c.wait().unwrap();
            assert!(!still_running_in(&d));
            let _ = std::fs::remove_dir_all(d.parent().unwrap());
        }
    }

    #[test]
    fn a_wrong_update_file_is_refused_and_not_offered_again() {
        // An installer names its version in UTF-16; another version's installer doesn't match.
        let inst: Vec<u8> = b"MZ..".iter().copied().chain("ProductVersion\00.90.0-beta.32".encode_utf16().flat_map(|u| u.to_le_bytes())).collect();
        assert!(installer_is_version(&inst, "0.90.0-beta.32"));
        assert!(installer_is_version(&inst, "v0.90.0-beta.32"));
        assert!(!installer_is_version(&inst, "0.90.0-beta.33"));
        // Two strikes against the same file and it isn't offered; a fixed file (another checksum) is.
        let d = std::env::temp_dir().join(format!("rl-upd-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let u = "https://github.com/x/RecompLauncher_0.90.0-beta.33_x64-setup.exe";
        assert_eq!(update_strike(&d, "0.90.0-beta.33", u, "AAA", 1), 1);
        assert!(!update_is_bad(&d, "0.90.0-beta.33", u, "aaa"));
        assert_eq!(update_strike(&d, "0.90.0-beta.33", u, "aaa", 1), 2);
        assert!(update_is_bad(&d, "v0.90.0-beta.33", u, "AAA"));
        // version.json changed (another file, another url, another version): offered again.
        assert!(!update_is_bad(&d, "0.90.0-beta.33", u, "bbb"));
        assert!(!update_is_bad(&d, "0.90.0-beta.33", "https://github.com/x/other.exe", "aaa"));
        assert!(!update_is_bad(&d, "0.90.0-beta.34", u, "aaa"));
        // Another file starts over.
        assert_eq!(update_strike(&d, "0.90.0-beta.33", u, "bbb", 1), 1);
        std::fs::remove_dir_all(&d).unwrap();
    }
    #[test]
    fn only_plain_game_links_open_anything() {
        assert_eq!(super::link_game("recomplauncher://game/recompia").as_deref(), Some("recompia"));
        assert_eq!(super::link_game("RecompLauncher://Game/zelda64-recomp/").as_deref(), Some("zelda64-recomp"));
        assert_eq!(super::link_game("recomplauncher://game/recompia?play=1#x").as_deref(), Some("recompia"));
        assert_eq!(super::link_game("recomplauncher://game/recompia/extra").as_deref(), Some("recompia"));
        for bad in ["recomplauncher://game/", "recomplauncher://game/../x", "recomplauncher://game/a%20b", "recomplauncher://install/recompia", "https://recomplauncher.com/game/recompia", "recomplauncher:game/x", "recomplauncher://game/a.exe", "--play"] {
            assert_eq!(super::link_game(bad), None, "{bad}");
        }
        assert_eq!(super::link_game(&format!("recomplauncher://game/{}", "a".repeat(81))), None);
    }

    #[test]
    fn an_older_release_isnt_an_update() {
        assert!(super::older_version("0.42", "1.0"));
        assert!(super::older_version("v0.9.1", "1.0.0"));
        assert!(!super::older_version("1.1", "1.0"));
        assert!(!super::older_version("nightly-2026", "1.0"));
        assert!(super::older_version("v1.9", "v1.10a"));
    }

    #[test]
    fn the_players_copy_moves_out_before_uninstall() {
        let d = std::env::temp_dir().join(format!("rl-keep-{}", std::process::id()));
        let game = d.join("pokemon-platinum");
        std::fs::create_dir_all(game.join("rom")).unwrap();
        std::fs::write(game.join("rom/platinum.nds"), b"x").unwrap();
        std::fs::create_dir_all(d.join("Game Copies/pokemon-platinum")).unwrap();
        std::fs::write(d.join("Game Copies/pokemon-platinum/platinum.nds"), b"old").unwrap();
        let to = super::save_copy_out(&game, "pokemon-platinum", &game.join("rom/platinum.nds")).unwrap();
        assert_eq!(to, d.join("Game Copies/pokemon-platinum/platinum (2).nds"));
        assert!(to.is_file() && !game.join("rom/platinum.nds").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_plain_start_script_gives_its_program() {
        let d = std::env::temp_dir().join(format!("rl-bat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("bin")).unwrap();
        std::fs::write(d.join("bin/Game.exe"), b"MZ").unwrap();
        std::fs::write(d.join("Play.bat"), "@echo off\r\ncd /d %~dp0\r\nstart \"\" \"bin\\Game.exe\" --fullscreen %*\r\n").unwrap();
        let (exe, args) = super::bat_target(&d.join("Play.bat")).unwrap();
        assert_eq!(exe, d.join("bin/Game.exe"));
        assert_eq!(args, vec!["--fullscreen"]);
        // More than one step, or a choice: left to Proton as it is.
        std::fs::write(d.join("Two.bat"), "Game1.exe\r\nbin\\Game.exe\r\n").unwrap();
        assert!(super::bat_target(&d.join("Two.bat")).is_none());
        std::fs::write(d.join("If.bat"), "if exist x goto a\r\nbin\\Game.exe\r\n").unwrap();
        assert!(super::bat_target(&d.join("If.bat")).is_none());
        // Output sent to a file, a second command, or another folder first: left as it is.
        std::fs::write(d.join("Log.bat"), "bin\\Game.exe > log.txt 2>&1\r\n").unwrap();
        assert!(super::bat_target(&d.join("Log.bat")).is_none());
        std::fs::write(d.join("Cd.bat"), "cd bin\r\nGame.exe\r\n").unwrap();
        assert!(super::bat_target(&d.join("Cd.bat")).is_none());
        // The script's folder in an argument becomes the real folder.
        std::fs::write(d.join("Root.bat"), "bin\\Game.exe --root \"%~dp0data\" -w\r\n").unwrap();
        let (_, args) = super::bat_target(&d.join("Root.bat")).unwrap();
        assert_eq!(args, vec!["--root".to_string(), format!("{}{}data", d.display(), std::path::MAIN_SEPARATOR), "-w".to_string()]);
        // A program outside the game's folder: never.
        std::fs::write(d.parent().unwrap().join(format!("rl-evil-{}.exe", std::process::id())), b"MZ").unwrap();
        std::fs::write(d.join("Out.bat"), format!("..\\rl-evil-{}.exe\r\n", std::process::id())).unwrap();
        assert!(super::bat_target(&d.join("Out.bat")).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_games_own_folder_or_program_is_never_its_copy() {
        let d = std::env::temp_dir().join(format!("rl-own-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let game = d.join("Recomp Library/mm3d-dekui");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("Dekui.exe"), vec![0u8; 64]).unwrap();
        std::fs::write(game.join("data.bin"), b"x").unwrap();
        let need = crate::profile::GameCopy { kind: "iso_or_folder".into(), ..Default::default() };
        assert!(super::copy_ok(&need, &game, &game, true).unwrap_err().contains("own folder"));
        assert!(super::copy_ok(&need, &d.join("Recomp Library"), &game, true).unwrap_err().contains("installs games into"));
        assert!(super::copy_ok(&need, &game.join("Dekui.exe"), &game, true).unwrap_err().contains("program"));
        // A game that reads its files from its own folder (or unpacks the disc there): its folder is fine.
        let own = crate::profile::GameCopy { kind: "iso_or_folder".into(), into: ".".into(), ..Default::default() };
        assert!(super::copy_ok(&own, &game, &game, true).is_ok());
        // Set up before (an adopted game inside the player's copy): not turned away now.
        assert!(super::copy_ok(&need, &d.join("Recomp Library"), &game, false).is_ok());
        // A copy somewhere else is checked as before.
        let mine = d.join("My Games/mm3d.3ds");
        std::fs::create_dir_all(mine.parent().unwrap()).unwrap();
        std::fs::write(&mine, b"3ds").unwrap();
        assert!(super::copy_ok(&need, &mine, &game, true).is_ok());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn running_stats_are_not_something_missing() {
        assert!(super::stats_line("[host] vk_pipeline: gpl: 57 fast links (9 by workers; avg 0.01 ms, max 0.13 ms, 0 failed), 16 draws lookups found shader libraries missing (left to a worker)"));
        assert!(super::stats_line("[host] frametime: 60 s: 3509 frames in 60.0 s, mean 58.5 fps"));
        assert!(!super::stats_line("error: default.xex is missing"));
        assert!(!super::stats_line("could not find game data in ./assets"));
        for l in ["ProtonFixes[56194] WARN: Game title not found in CSV", "Proton: Error: unable to use parent for game drive, path /run/media/duke/GamingDrive", "ERROR: ld.so: object '/home/wes/.local/share/Steam/steamrt32/gameoverlayrenderer.so' from LD_PRELOAD cannot be preloaded (wrong ELF class: ELFCLASS32): ignored."] {
            assert!(super::runtime_line(&l.to_lowercase()), "{l}");
        }
        assert!(!super::runtime_line("error: default.xex is missing"));
    }

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
    fn folder_checks_do_not_identify_unrelated_game_files() {
        let d = std::env::temp_dir().join(format!("rl-folder-copy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("copy/58410A9F/000D0000")).unwrap();
        std::fs::write(d.join("copy/58410A9F/000D0000/package"), b"game package").unwrap();
        let wrong = d.join("Super Mario 64 DS.nds");
        std::fs::write(&wrong, vec![0u8; 8192]).unwrap();
        let found = roms::RomFile { path: wrong, system: "nds".into(), title: "Super Mario 64 DS".into(), region: String::new(), size: 8192 };
        let cat = json!({ "apps": [{ "id": "pants", "game": "The Fancy Pants Adventures", "origin": "x360", "repo": "example/pants",
            "launcher": { "format": 1, "start": { "program": ["game.exe"] }, "game": {
                "kind": "iso_or_folder", "check": { "file": "58410A9F/000D0000/package" }
            } } }] });
        let dirs = HashMap::new();
        assert!(copies_for(&cat, &[found.clone()], &dirs).is_empty());
        // Unreadable IDs must not let a folder-only check identify that file either.
        let mut with_ids = cat.clone();
        with_ids["apps"][0]["launcher"]["game"]["check"]["ids"] = json!(["58410A9F"]);
        assert!(copies_for(&with_ids, &[found.clone()], &dirs).is_empty());
        // A folder that really contains the identifying file still matches under any name.
        let folder = roms::RomFile { path: d.join("copy"), system: "x360".into(), title: "copy".into(), region: String::new(), size: 0 };
        assert_eq!(copies_for(&cat, &[found, folder.clone()], &dirs).get("pants").map(|r| &r.path), Some(&folder.path));
        // A package explicitly matched by its title is still eligible for setup.
        let named = d.join("The Fancy Pants Adventures.bin");
        std::fs::write(&named, b"game package").unwrap();
        let package = roms::RomFile { path: named, system: "x360".into(), title: "The Fancy Pants Adventures".into(), region: String::new(), size: 12 };
        assert!(copies_for(&cat, &[package], &dirs).contains_key("pants"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn disc_checks_do_not_identify_unrelated_folders() {
        let d = std::env::temp_dir().join(format!("rl-disc-copy-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("data.bin"), b"another game's data").unwrap();
        let folder = roms::RomFile { path: d.clone(), system: "x360".into(), title: "Other Game".into(), region: String::new(), size: 0 };
        let cat = json!({ "apps": [{ "id": "disc-game", "game": "My Disc Game", "repo": "example/disc-game",
            "launcher": { "format": 1, "start": { "program": ["game.exe"] }, "game": {
                "kind": "iso_or_folder", "check": { "iso9660": "SLUS_206.51" }
            } } }] });
        assert!(copies_for(&cat, &[folder], &HashMap::new()).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_switch2_package_is_found_by_its_build_id() {
        use crate::nup::tests::{nso, package};
        let d = std::env::temp_dir().join(format!("rl-nupfind-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let mut id = [7u8; 32];
        id[..4].copy_from_slice(&[0xe0, 0xf3, 0xcc, 0xfa]);
        let rom = |name: &str, id: &[u8; 32]| {
            let p = d.join(name);
            std::fs::write(&p, package(&[(0, vec![("main", nso(id), 0)])])).unwrap();
            roms::RomFile { path: p, system: "switch2".into(), title: name.into(), region: String::new(), size: 0 }
        };
        let ours = rom("backup-07.nup", &id);
        let other = rom("other.nup", &[9u8; 32]);
        let cat = json!({ "apps": [{ "id": "hs", "name": "Hookshot", "game": "Some Game", "origin": "switch2", "repo": "a/hs",
            "launcher": { "format": 1, "start": { "program": ["g.exe"] }, "game": { "kind": "iso_or_folder", "label": "Your .nup", "check": { "nup_build_id": "e0f3ccfa" } } } }] });
        let dirs = HashMap::new();
        assert!(copies_for(&cat, &[other.clone()], &dirs).is_empty());
        assert_eq!(copies_for(&cat, &[other, ours.clone()], &dirs).get("hs").map(|r| r.path.clone()), Some(ours.path));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn another_games_disc_isnt_taken_for_this_one() {
        // Dusklight reads a Twilight Princess disc: a Super Mario Sunshine one in the same folder
        // isn't it, and neither is a PS2 disc.
        let d = std::env::temp_dir().join(format!("rl-dusk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let gc = |name: &str, id: &[u8; 6]| {
            let mut h = vec![0u8; 0x400];
            h[..6].copy_from_slice(id);
            h[0x1C..0x20].copy_from_slice(&[0xC2, 0x33, 0x9F, 0x3D]);
            let p = d.join(name);
            std::fs::write(&p, h).unwrap();
            roms::RomFile { path: p, system: "gc".into(), title: name.trim_end_matches(".iso").into(), region: "US".into(), size: 0x400 }
        };
        let sms = gc("Super Mario Sunshine (USA).iso", b"GMSE01");
        let ps2 = { let p = d.join("Some PS2 Game.iso"); std::fs::write(&p, vec![0u8; 0x400]).unwrap(); roms::RomFile { path: p, system: "ps2".into(), title: "Some PS2 Game".into(), region: "US".into(), size: 0x400 } };
        let cat = json!({ "apps": [{ "id": "dusklight", "name": "Dusklight", "game": "The Legend of Zelda: Twilight Princess", "origin": "gamecube", "repo": "TwilitRealm/dusklight" }] });
        let dirs = HashMap::new();
        assert!(copies_for(&cat, &[sms.clone(), ps2.clone()], &dirs).is_empty());
        // Its own disc, under any name, is.
        let tp = gc("disc1.iso", b"GZ2E01");
        assert_eq!(copies_for(&cat, &[sms, ps2, tp.clone()], &dirs).get("dusklight").map(|r| r.path.clone()), Some(tp.path.clone()));
        // And picking Sunshine for it is refused.
        let need = profile::find(Path::new(""), "TwilitRealm/dusklight", &cat["apps"][0]).and_then(|p| p.game).unwrap();
        assert!(profile::check_copy(&need, &d.join("Super Mario Sunshine (USA).iso")).unwrap_err().contains("GMSE01"));
        assert!(profile::check_copy(&need, &tp.path).is_ok());
        // Checks that can't tell games apart: a common file, or nothing but "a disc image".
        assert!(need.identifies() && !need.identifies_by_contents());
        let common: profile::GameCopy = serde_json::from_value(json!({ "kind": "folder", "label": "x", "check": { "file": "default.xex" } })).unwrap();
        assert!(!common.identifies());
        let own: profile::GameCopy = serde_json::from_value(json!({ "kind": "folder", "label": "x", "check": { "file": "SLUS_203.12" } })).unwrap();
        assert!(own.identifies_by_contents());
        let _ = std::fs::remove_dir_all(&d);
    }

    fn app_in(dir: &Path, lib: Library) -> App {
        App {
            dir: dir.to_path_buf(),
            http: reqwest::Client::new(),
            lib: Mutex::new(lib),
            jobs: Mutex::default(),
            running: Mutex::default(),
            pids: Mutex::default(),
            started: Mutex::default(),
            download_slot: Arc::new(tokio::sync::Semaphore::new(1)),
            moving: Mutex::default(),
            overlay_open: AtomicBool::new(false),
            looked: Mutex::new(HashSet::new()),
            headless: AtomicBool::new(false),
        }
    }

    #[test]
    fn correcting_a_project_keeps_the_installation_and_history() {
        let d = std::env::temp_dir().join(format!("rl-project-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("game")).unwrap();
        std::fs::write(d.join("game/game.exe"), b"player's executable").unwrap();
        let mut lib = Library::default();
        let manual = library::ManualSetup { exe: Some(PathBuf::from("game.exe")), how: "none".into(), ..Default::default() };
        lib.installed.insert("wrong".into(), Installed {
            id: "wrong".into(), name: "My game".into(), repo: "Wrong/Project".into(), tag: "v9".into(),
            path: d.join("game"), exe: Some(PathBuf::from("game.exe")), manual: Some(manual.clone()),
            played_secs: 120, sessions: vec![[1, 120]], auto_update: true, sha256: "old checksum".into(),
            asset: "old.zip".into(), verified: true, cloud: Some(true), ..Default::default()
        });
        lib.settings.ui = json!({ "pins": ["wrong"], "names": { "wrong": "My name" } });
        let backup = saves::dir_of(&d, "wrong");
        std::fs::create_dir_all(&backup).unwrap();
        std::fs::write(backup.join("backup.zip"), b"backup").unwrap();
        let app = app_in(&d, lib);
        let cat = json!({ "apps": [{ "id": "right", "name": "Right", "repo": "Right/Project" }] });
        let tracked = adopt::change_project(&app, "wrong", "right", &cat).unwrap();
        assert_eq!((tracked.repo.as_str(), tracked.tag.as_str()), ("Right/Project", ""));
        assert!(!tracked.local && !tracked.auto_update && !tracked.verified);
        assert!(tracked.sha256.is_empty() && tracked.asset.is_empty() && tracked.cloud.is_none());
        assert_eq!(tracked.manual, Some(manual.clone()));
        assert_eq!(tracked.exe, Some(PathBuf::from("game.exe")));
        assert_eq!((tracked.played_secs, tracked.sessions.clone()), (120, vec![[1, 120]]));
        assert_eq!(std::fs::read(saves::dir_of(&d, "right").join("backup.zip")).unwrap(), b"backup");
        let own = adopt::change_project(&app, "right", "", &cat).unwrap();
        assert!(own.local && own.repo.is_empty() && own.tag == "own");
        assert_eq!(own.manual, Some(manual));
        let saved = Library::load(&d);
        assert_eq!(saved.installed.len(), 1);
        assert_eq!(saved.resolve_project_id("wrong"), own.id);
        assert_eq!(saved.resolve_project_id("right"), own.id);
        assert_eq!(saved.settings.ui["pins"][0], own.id);
        assert_eq!(saved.settings.ui["names"][&own.id], "My name");
        assert!(saves::dir_of(&d, &own.id).join("backup.zip").is_file());
        assert_eq!(std::fs::read(d.join("game/game.exe")).unwrap(), b"player's executable");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn project_changes_reject_conflicts_busy_games_and_missing_projects() {
        let d = std::env::temp_dir().join(format!("rl-project-conflict-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let mut lib = Library::default();
        for id in ["first", "second"] {
            lib.installed.insert(id.into(), Installed { id: id.into(), repo: format!("Owner/{id}"), tag: "v1".into(), ..Default::default() });
        }
        let app = app_in(&d, lib);
        let cat = json!({ "apps": [{ "id": "second", "repo": "Owner/second" }, { "id": "third", "repo": "Owner/third" }] });
        assert!(adopt::change_project(&app, "first", "second", &cat).is_err());
        assert!(adopt::change_project(&app, "first", "missing", &cat).is_err());
        app.jobs.lock().unwrap().insert("first".into(), Arc::new(AtomicBool::new(false)));
        assert!(adopt::change_project(&app, "first", "third", &cat).is_err());
        app.jobs.lock().unwrap().clear();
        std::fs::create_dir_all(d.join("saves/first")).unwrap();
        std::fs::create_dir_all(d.join("saves/third")).unwrap();
        assert!(adopt::change_project(&app, "first", "third", &cat).is_err());
        let l = app.lib.lock().unwrap();
        assert_eq!(l.installed.len(), 2);
        assert_eq!(l.installed["first"].tag, "v1");
        assert!(l.project_aliases.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn logs_for_the_website_say_whether_a_game_or_the_launcher_sent_them() {
        let f = log_source(Some("some-game"));
        assert_eq!(f[0], ("source".to_string(), "game".to_string()));
        assert_eq!(f[1], ("game".to_string(), "some-game".to_string()));
        for none in [None, Some("")] {
            let f = log_source(none);
            assert_eq!(f[0].1, "launcher");
            assert_eq!(f[1], ("game".to_string(), String::new()));
        }
    }

    #[test]
    fn a_replaced_listing_moves_only_when_asked_and_keeps_everything() {
        let d = std::env::temp_dir().join(format!("rl-replaced-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let mut lib = Library::default();
        let copy = d.join("game.iso");
        lib.installed.insert("old-recomp".into(), Installed { id: "old-recomp".into(), name: "Old Listing".into(), repo: "someone/old-recomp".into(), tag: "v0.1.1".into(), path: d.join("games/old"), played_secs: 7200, game_copy: Some(copy.clone()), ..Default::default() });
        std::fs::create_dir_all(saves::dir_of(&d, "old-recomp")).unwrap();
        std::fs::write(saves::dir_of(&d, "old-recomp").join("1.zip"), b"save").unwrap();
        let app = app_in(&d, lib);
        let cat = json!({ "replaced": { "old-recomp": "new-recomp" }, "apps": [{ "id": "new-recomp", "repo": "another/new-recomp", "name": "New Listing" }, { "id": "other", "repo": "x/y" }] });
        // Only to the listing the catalog says.
        assert!(adopt::move_listing_in(&app, &cat, "old-recomp", "other").is_err());
        let r = adopt::move_listing_in(&app, &cat, "old-recomp", "new-recomp").unwrap();
        // Another project's release: it installs next, into the same folder.
        assert_eq!(r["reinstall"], json!(true));
        let l = app.lib.lock().unwrap();
        let g = &l.installed["new-recomp"];
        assert!(!l.installed.contains_key("old-recomp"));
        assert_eq!(g.played_secs, 7200);
        assert_eq!(g.game_copy.as_ref(), Some(&copy));
        assert_eq!(g.path, d.join("games/old"));
        assert_eq!(g.name, "New Listing");
        assert_eq!(l.project_aliases.get("old-recomp").map(String::as_str), Some("new-recomp"), "old links still open it");
        assert!(saves::dir_of(&d, "new-recomp").join("1.zip").is_file());
        drop(l);
        // The same project under a new id keeps its version and files.
        let mut lib = Library::default();
        lib.installed.insert("old".into(), Installed { id: "old".into(), repo: "Me/Game".into(), tag: "v2".into(), asset: "game.zip".into(), ..Default::default() });
        let app = app_in(&d.join("same"), lib);
        let cat = json!({ "replaced": { "old": "new" }, "apps": [{ "id": "new", "repo": "me/game", "name": "Game" }] });
        let r = adopt::move_listing_in(&app, &cat, "old", "new").unwrap();
        assert_eq!(r["reinstall"], json!(false));
        assert_eq!(app.lib.lock().unwrap().installed["new"].tag, "v2");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn failed_project_save_rolls_back_the_library_and_backups() {
        let d = std::env::temp_dir().join(format!("rl-project-rollback-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let mut lib = Library::default();
        lib.installed.insert("old".into(), Installed { id: "old".into(), tag: "v1".into(), ..Default::default() });
        let backup = saves::dir_of(&d, "old");
        std::fs::create_dir_all(&backup).unwrap();
        std::fs::write(backup.join("save.zip"), b"save").unwrap();
        // A directory at the temporary-file path forces Library::save to fail.
        std::fs::create_dir_all(d.join("library.json.tmp")).unwrap();
        let app = app_in(&d, lib);
        let cat = json!({ "apps": [{ "id": "new", "repo": "Owner/New" }] });
        assert!(adopt::change_project(&app, "old", "new", &cat).is_err());
        let l = app.lib.lock().unwrap();
        assert_eq!(l.installed["old"].tag, "v1");
        assert!(!l.installed.contains_key("new") && l.project_aliases.is_empty());
        assert_eq!(std::fs::read(backup.join("save.zip")).unwrap(), b"save");
        assert!(!saves::dir_of(&d, "new").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_game_added_by_link_moves_over_when_the_list_has_it() {
        let d = std::env::temp_dir().join(format!("rl-merge-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let mine = json!([{ "id": "gh-me-game", "repo": "Me/Game", "name": "My Game", "custom": true }, { "id": "gh-me-other", "repo": "me/other", "custom": true }]);
        catalog::save_mine(&d, mine.as_array().unwrap()).unwrap();
        let mut lib = Library::default();
        lib.installed.insert("gh-me-game".into(), Installed { id: "gh-me-game".into(), name: "My Game".into(), tag: "v1.2".into(), path: d.join("games/game"), played_secs: 3600, ..Default::default() });
        lib.settings.ui = json!({ "favs": ["gh-me-game", "x", "game"], "names": { "gh-me-game": "Mine" }, "lang": "en" });
        std::fs::create_dir_all(saves::dir_of(&d, "gh-me-game")).unwrap();
        std::fs::write(saves::dir_of(&d, "gh-me-game").join("1.zip"), b"save").unwrap();
        // Backups from the official one, installed once before.
        std::fs::create_dir_all(saves::dir_of(&d, "game")).unwrap();
        std::fs::write(saves::dir_of(&d, "game").join("2.zip"), b"save").unwrap();
        let st = app_in(&d, lib);
        let cat = catalog::with_mine(&d, json!({ "apps": [{ "id": "game", "repo": "me/game", "name": "Game" }] }));
        // Until it moves over, the player's own entry stays beside the list's.
        assert!(cat["apps"].as_array().unwrap().iter().any(|a| a["id"] == "gh-me-game"));
        // Busy downloading: next time.
        st.jobs.lock().unwrap().insert("gh-me-game".into(), Arc::new(AtomicBool::new(false)));
        assert!(merge_mine(&st, &cat).is_empty());
        st.jobs.lock().unwrap().clear();
        // The official one installing: next time too.
        st.jobs.lock().unwrap().insert("game".into(), Arc::new(AtomicBool::new(false)));
        assert!(merge_mine(&st, &cat).is_empty());
        st.jobs.lock().unwrap().clear();
        assert_eq!(merge_mine(&st, &cat), vec![("gh-me-game".to_string(), "game".to_string(), "Game".to_string())]);
        let l = st.lib.lock().unwrap().clone();
        let g = &l.installed["game"];
        assert!(!l.installed.contains_key("gh-me-game"));
        assert_eq!((g.id.as_str(), g.tag.as_str(), g.played_secs, g.path.clone()), ("game", "v1.2", 3600, d.join("games/game")));
        assert_eq!(l.settings.ui["favs"], json!(["game", "x"]));
        assert_eq!(l.settings.ui["names"]["game"], "Mine");
        assert_eq!(l.settings.ui["lang"], "en");
        assert!(saves::dir_of(&d, "game").join("1.zip").is_file() && saves::dir_of(&d, "game").join("2.zip").is_file());
        assert!(!saves::dir_of(&d, "gh-me-game").exists());
        // The one the list doesn't have stays the player's.
        let left = catalog::mine(&d);
        assert_eq!(left.len(), 1);
        assert_eq!(left[0]["id"], "gh-me-other");
        // Nothing more to do the next time.
        assert!(merge_mine(&st, &catalog::with_mine(&d, json!({ "apps": [{ "id": "game", "repo": "me/game" }] }))).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn both_installed_both_stay() {
        let d = std::env::temp_dir().join(format!("rl-merge2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        catalog::save_mine(&d, json!([{ "id": "gh-me-game", "repo": "me/game", "custom": true }]).as_array().unwrap()).unwrap();
        let mut lib = Library::default();
        for id in ["gh-me-game", "game"] {
            lib.installed.insert(id.into(), Installed { id: id.into(), path: d.join(id), ..Default::default() });
        }
        let st = app_in(&d, lib);
        let cat = catalog::with_mine(&d, json!({ "apps": [{ "id": "game", "repo": "me/game" }] }));
        assert!(merge_mine(&st, &cat).is_empty());
        assert_eq!(st.lib.lock().unwrap().installed.len(), 2);
        assert_eq!(catalog::mine(&d).len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

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
    fn a_release_file_says_which_system_it_is_for() {
        let a = |n: &str| assets::Asset { name: n.into(), size: 1, digest: None, browser_download_url: String::new(), url: None };
        let rel = [a("Game-windows-x64.zip"), a("Game-linux-x86_64.tar.gz"), a("Game-macOS.zip"), a("SHA256SUMS")];
        assert_eq!(named_for(&rel, "linux").iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), vec!["Game-linux-x86_64.tar.gz"]);
        assert_eq!(named_for(&rel, "macos").iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), vec!["Game-macOS.zip"]);
        assert_eq!(named_for(&rel, "windows").iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), vec!["Game-windows-x64.zip"]);
    }

    #[test]
    fn mac_launcher_updates_do_not_use_linux_or_windows_entries() {
        let feed = json!({"version":"2.0", "url":"https://example.com/windows.exe", "min":"2.0", "linux":{"url":"https://example.com/linux.AppImage"}});
        assert!(launcher_platform_entry(&feed, "macos", "arm64").is_none());
        assert_eq!(launcher_platform_entry(&feed, "windows", "x64").unwrap()["url"], "https://example.com/windows.exe");
        assert_eq!(launcher_platform_entry(&feed, "linux", "x64").unwrap()["url"], "https://example.com/linux.AppImage");
    }

    #[test]
    fn launcher_update_downloads_only_from_trusted_places() {
        let base = "http://127.0.0.1:8787/api";
        for ok in ["https://github.com/o/r/releases/download/v1/a.exe", "https://recomplauncher.com/launcher/a.exe", "http://127.0.0.1:8787/api/launcher/a.exe"] {
            assert!(trusted_update_url(ok, base), "{ok}");
        }
        for bad in ["http://github.com/a.exe", "https://github.com.evil.example/a.exe", "https://name@github.com/a.exe", "https://github.com/a.exe#x", "http://127.0.0.1:8787/api-evil/a.exe", "not a url"] {
            assert!(!trusted_update_url(bad, base), "{bad}");
        }
    }

    #[test]
    fn linux_launcher_update_uses_its_own_checksum_and_minimum() {
        let feed = json!({"version":"999.0", "sha256":"windows-sha", "min":"999.0", "size":12,
            "linux":{"url":"https://github.com/owner/repo/Launcher.AppImage"}});
        let linux = linux_entry(&feed).unwrap();
        assert_eq!(linux["version"], "999.0");
        for key in ["sha256", "min", "size", "linux"] { assert!(linux.get(key).is_none(), "{key}"); }
        let mut own = feed;
        own["linux"]["sha256"] = json!("linux-sha");
        own["linux"]["min"] = json!("1.0");
        assert_eq!(linux_entry(&own).unwrap()["sha256"], "linux-sha");
        assert_eq!(linux_entry(&own).unwrap()["min"], "1.0");
    }

    #[test]
    fn mac_launcher_update_requires_matching_architecture_and_own_minimum() {
        let feed = json!({"version":"2.0", "min":"2.0", "notes":"shared notes", "macos":{"arch":"arm64", "url":"https://example.com/mac.dmg", "sha256":"abc", "version":"1.5"}});
        let mac = launcher_platform_entry(&feed, "macos", "arm64").unwrap();
        assert_eq!(mac["url"], "https://example.com/mac.dmg");
        assert_eq!(mac["version"], "1.5");
        assert_eq!(mac["notes"], "shared notes");
        assert!(mac.get("min").is_none(), "Windows minimum must not force a Mac update");
        assert!(launcher_platform_entry(&feed, "macos", "x64").is_none());
        let universal = json!({"version":"2.1", "macos":{"arch":"universal", "url":"https://example.com/mac.zip", "min":"1.0"}});
        assert_eq!(launcher_platform_entry(&universal, "macos", "x64").unwrap()["min"], "1.0");
        assert_eq!(launcher_platform_entry(&universal, "macos", "arm64").unwrap()["version"], "2.1");
        assert!(launcher_platform_entry(&json!({"macos":{"arch":"arm64"}}), "macos", "arm64").is_none());
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

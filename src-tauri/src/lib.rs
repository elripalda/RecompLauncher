//! RecompLauncher: discover, install and play PC recomps, in one window.
//!
//! The window (ui/) asks for things through the commands at the bottom; long jobs (downloads,
//! games running) report back through events: `install` with a Progress, `game` when one starts
//! or closes.

pub mod ach;
pub mod adopt;
pub mod xex;
mod appimage;
mod convert;
#[cfg(target_os = "linux")]
mod appimage_update;
pub mod art;
pub mod assets;
pub mod boot;
pub mod catalog;
pub mod cloud;
pub mod devtools;
pub mod display;
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
pub mod launchbox;
pub mod library;
pub mod library_convert;
pub mod logs_zip;
pub mod looks;
pub mod mac_update;
pub mod manage;
pub mod macos;
pub mod media;
pub mod mods;
pub mod net;
pub mod nup;
pub mod pads;
#[cfg(target_os = "linux")]
pub mod plain;
pub mod own_setup;
pub mod ghinfo;
pub mod ping;
pub mod plugins;
pub mod push;
pub mod profile;
pub mod proton;
pub mod secret;
pub mod setup_state;
#[cfg(test)]
mod setup_window_tests;
pub mod sysenv;
pub mod sysneeds;
mod test_control;
mod emus;
pub mod roms;
pub mod saves;
pub mod setup_temp;
pub mod shortcuts;
pub mod steam_sync;
pub mod swap;
pub mod trail;
pub mod tray;
mod update_download;
mod update_launch;
#[cfg(all(test, unix))]
mod update_rollback_tests;
#[cfg(test)]
mod update_check_tests;
pub mod usage;
pub mod webview_crash;
pub mod win;
pub mod autostart;
#[cfg(windows)]
mod game_window;
mod installer;

use catalog::Release;
use install::Progress;
use library::{folder_name, now, Installed, Library, Settings};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
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
        } else if e.is_connect() && net::tls_failed(&e) {
            // Something answered, but not with a secure connection: whatever stepped in between
            // the player and the site (a proxy too, when there is one) is what to look at.
            // Checking their connection or waiting wouldn't help.
            let h = site.unwrap_or_else(|| "the site".into());
            Error::Net(format!("Couldn't open a secure connection to {h}. Something on your network or PC may be blocking it: a router's security filter, an antivirus or a VPN."))
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

/// A downloaded release unpacked into `staging`, beside the game's folder (see install::stage).
async fn stage_release(file: &Path, staging: &Path, name: &str) -> Res<install::Staged> {
    let (file, s2, what) = (file.to_path_buf(), staging.to_path_buf(), name.to_string());
    let staged = tokio::task::spawn_blocking(move || install::stage(&file, &s2)).await.map_err(|e| Error::Msg(e.to_string()))?;
    staged.map_err(|e| match e {
        Error::Io(e) => defender_msg(e, &what),
        e => e,
    })
}

/// Said while a game's AppImage is unpacked at Play, before it starts.
#[cfg(target_os = "linux")]
const UNPACKING_NOTE: &str = "Unpacking the game into your Recomp Library. This happens once; it starts when it's done.";

/// A game's record, ready to run its programs. On Linux every game plays from a plain folder: one
/// that still starts an AppImage (the player's, brought in before this version, or one an older
/// launcher installed) is unpacked into the Recomp Library first, once, and the window says where
/// it went (see plain.rs).
fn in_plain_folder(st: &App, id: &str) -> Res<Installed> {
    let g = st.lib.lock().unwrap().installed.get(id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    #[cfg(target_os = "linux")]
    if let Some(image) = plain::image_of(&g) {
        if let Some(app) = net::app() {
            let _ = app.emit("note", UNPACKING_NOTE);
        }
        plain::tell(&block_here(|| plain::bring_in(st, id, &image))?);
        return st.lib.lock().unwrap().installed.get(id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()));
    }
    Ok(g)
}

/// Long work that blocks (a game's one-time unpack, gigabytes) where it may be on one of the async
/// runtime's workers: Play runs there, and so do the sync commands marked async (setup_copy,
/// setup_inputs_run). The runtime moves its other tasks to another thread while it waits, so
/// downloads and the window's calls keep going. Off a multi-threaded runtime (a plain thread, a
/// test) it just runs, since block_in_place would panic on a single-threaded one.
#[cfg(target_os = "linux")]
fn block_here<T>(work: impl FnOnce() -> T) -> T {
    use tokio::runtime::{Handle, RuntimeFlavor};
    let multi = Handle::try_current().is_ok_and(|h| h.runtime_flavor() == RuntimeFlavor::MultiThread);
    if !multi {
        return work();
    }
    tokio::task::block_in_place(work)
}

/// What the player sees when Play gets to an AppImage anyway (a program picked for one start).
const APPIMAGE_NOT_RUN: &str = "That's an AppImage, and on Linux the launcher plays games only from a plain folder. Bring it in with Bring Your Recomps, and it's unpacked into your Recomp Library.";

/// What the player sees when a release's AppImage won't unpack and it has no tarball either.
const NO_PLAIN_BUILD: &str = "The launcher couldn't unpack this game's AppImage, and its release has no other Linux build to use instead. On Linux the launcher plays games only from a plain folder. Pick another version on the game's page, or tell its developer.";

/// A release's tarball the install can take when its AppImage won't unpack: one of its own
/// project's files, and for SourceForge one the site has checked (as for the first pick).
fn plain_fallback(a: &assets::Asset, repo: &str, project_id: Option<u64>, sf: bool) -> bool {
    let tarball = assets::kind(&a.name).is_some_and(|k| k.starts_with("tar."));
    let checked = a.digest.as_deref().is_some_and(|d| d.starts_with("sha256:"));
    tarball && (!sf || checked) && catalog::own_download(repo, &a.browser_download_url, project_id)
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
        let _ = self.try_save();
    }

    fn try_save(&self) -> std::io::Result<()> {
        let l = self.lib.lock().unwrap().clone();
        l.save(&self.dir)
    }
}

fn str_of<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

/// Where a game's backups go: beside it, in its library's .backups folder.
pub(crate) fn backups_dir(g: &Installed) -> PathBuf {
    g.path.parent().unwrap_or(Path::new(".")).join(".backups").join(g.path.file_name().unwrap_or_default())
}

/// Every save place of an installed game: its own, and those of each of its games that keep their
/// own saves (one recomp with several games in it).
fn all_save_spots(st: &App, g: &Installed) -> Vec<saves::Spot> {
    let mut spots = cloud::spots_of(st, &g.id, g);
    for game in games_of(st, &g.id, g).iter().filter(|x| !x.saves.is_empty()) {
        spots.extend(cloud::spots_of(st, &games::full(&g.id, &game.id), g));
    }
    spots
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

/// An Xbox 360 game's title id from the list: its "title_id", the one under "achievements", or (for
/// Easy Setup) the id its disc files must have. The first that is one (8 hex digits) counts, so a
/// mistyped title_id doesn't hide a good disc id. In capitals, as the unlock file is named.
/// The listing a game the player added themselves is a copy of: the same repo name (whoever's it
/// is, in any case), else the same title id as its disc. With what matched, for the trail.
fn listing_for_copy(cat: &Value, repo: &str, disc: impl FnOnce() -> Option<String>) -> Option<(String, &'static str)> {
    let name = |r: &str| r.trim().trim_end_matches('/').trim_end_matches(".git").rsplit('/').next().unwrap_or("").to_ascii_lowercase();
    let apps = cat.get("apps").and_then(Value::as_array)?;
    let want = name(repo);
    if !want.is_empty() {
        let hits: Vec<&Value> = apps.iter().filter(|a| name(str_of(a, "repo")) == want).collect();
        if let [one] = hits.as_slice() {
            return Some((str_of(one, "id").to_string(), "repo name"));
        }
    }
    // Only worth reading the disc when some listing has a title id to match.
    if !apps.iter().any(|a| catalog_title(a).is_some()) {
        return None;
    }
    let t = disc()?.to_ascii_uppercase();
    let hits: Vec<&Value> = apps.iter().filter(|a| catalog_title(a).as_deref() == Some(t.as_str())).collect();
    if let [one] = hits.as_slice() { Some((str_of(one, "id").to_string(), "title id")) } else { None }
}

fn catalog_title(entry: &Value) -> Option<String> {
    let ok = |t: &&str| t.trim().len() == 8 && t.trim().chars().all(|c| c.is_ascii_hexdigit());
    ["/title_id", "/achievements/title_id", "/boot/files/id"].iter().filter_map(|k| entry.pointer(k).and_then(Value::as_str)).find(ok).map(|t| t.trim().to_ascii_uppercase())
}

/// The release the store checked and let out for a recomp (its "tag" in the list), if it says.
fn catalog_tag(entry: &Value) -> Option<String> {
    entry.get("tag").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty()).map(str::to_string)
}

/// Whether an install of `tag` is a version the player picked (`Installed::pinned`: no update is
/// offered until they go back). The same version again (Reinstall, Change Build) stays as it was
/// (`have`: the installed tag, and whether it was picked). The newest release, one the launcher
/// fell back to, or the one the store let out (`store`) is where updates go anyway, so never.
fn picked_by_player(tag: &str, newest: &str, fell_back: bool, store: Option<&str>, have: (&str, bool)) -> bool {
    let (have_tag, have_pinned) = have;
    // The installed tag can lack the release's "v" (imported from another launcher).
    if !have_tag.is_empty() && same_tag(tag, have_tag) {
        return have_pinned;
    }
    if fell_back || tag == newest {
        return false;
    }
    store != Some(tag)
}

/// The same release name, give or take a leading "v" and letter case.
fn same_tag(a: &str, b: &str) -> bool {
    let n = |s: &str| s.trim().trim_start_matches(['v', 'V']).trim_start_matches('.').to_ascii_lowercase();
    !a.trim().is_empty() && n(a) == n(b)
}

/// The release an installed or saved tag names, give or take a "v": a game imported from another
/// launcher can say 1.0.0 for the release v1.0.0.
fn release_on<'a>(rels: &'a [Release], tag: &str) -> Option<&'a Release> {
    rels.iter().find(|r| same_tag(&r.tag_name, tag))
}

/// A tag as its release spells it, when that's spelled differently (1.0.0 for v1.0.0). None when
/// it already matches exactly or no release has it.
fn exact_tag(rels: &[Release], tag: &str) -> Option<String> {
    release_on(rels, tag).map(|r| r.tag_name.clone()).filter(|t| t != tag)
}

/// The release to install instead of one that was taken down: the newest with a build for this PC
/// (`fits`), stable unless the one taken down was a pre-release, and never the one installed.
fn replacement<'a>(fresh: &'a [Release], was_pre: bool, have: &str, fits: impl Fn(&Release) -> bool) -> Option<&'a Release> {
    let r = fresh.iter().filter(|r| !r.draft && (!r.prerelease || was_pre)).find(|r| fits(r))?;
    (!same_tag(&r.tag_name, have)).then_some(r)
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
    // A Forgejo project's own setup file, so Easy Setup is ready when the download is in.
    if catalog::is_forgejo(&repo) {
        catalog::repo_profile(&st.http, &st.dir, &repo, false).await;
    }
    // Its project is down and the list says so: the release from the RecompLauncher backup (each
    // file checked against its SHA-256). When the backup can't help, its own releases as usual.
    let backup_ok = catalog::backup_on(&entry);
    let mut from_backup = false;
    let from_bk = if backup_ok && tag.is_none() {
        match getting_ready(catalog::backup_release(&st.http, id, os_for(st, &entry, id)), &cancel).await {
            Ok(r) => Some(r),
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(e) => {
                trail::note(format!("Install {id}: the backup couldn't help ({e}), so its own releases"));
                None
            }
        }
    } else {
        None
    };
    let rels: Vec<Release> = match from_bk {
        Some(r) => {
            trail::note(format!("Install {id}: {} from the RecompLauncher backup", r.tag_name));
            from_backup = true;
            vec![r]
        }
        None => {
            trail::note(format!("Install {id}: list read, asking {repo} for its releases"));
            serde_json::from_value(getting_ready(catalog::releases_within(&st.http, &st.dir, &repo, 600), &cancel).await?.data)?
        }
    };
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
    let listed = catalog_tag(&entry).filter(|_| !from_backup).and_then(|t| rels.iter().find(|r| !r.draft && same_tag(&r.tag_name, &t))).filter(|r| has_build(r));
    if let (None, Some(r)) = (&tag, listed) {
        if r.tag_name != newest.tag_name {
            trail::note(format!("Install {id}: {} is the version the store let out", r.tag_name));
        }
    }
    let rel = match &tag {
        // Change Build sends the installed tag, which can lack the release's "v".
        Some(t) => release_on(&rels, t).ok_or_else(|| Error::Msg(format!("{t} isn't one of its releases.")))?,
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
    let have_pinned = st.lib.lock().unwrap().installed.get(id).is_some_and(|g| g.pinned);
    let store = listed.map(|r| r.tag_name.as_str());
    let pinned = picked_by_player(&rel.tag_name, &newest.tag_name, fell_back, store, (have.as_str(), have_pinned));
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
    // A Linux game on a drive that can't keep its run permission and links would never start:
    // said before anything downloads. (A Windows build through Proton doesn't need them.)
    #[cfg(target_os = "linux")]
    if os == "linux" {
        plain::check_drive(&home)?;
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
    if !from_backup && !catalog::own_download(&repo, &asset.browser_download_url, rel.project_id) {
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
    let mut got = match install::download(&st.http, &asset, &home.join(".downloads"), id, cancel.clone(), &report).await {
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
                replacement(&fresh, rel.prerelease, &have_tag, |r| assets::pick(&r.assets, os, arch, pattern).is_some())
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
        Ok(g) => g,
        Err(Error::Cancelled) => return Err(Error::Cancelled),
        // Its own download failed, and the list says the RecompLauncher backup has it: from there.
        Err(e) if backup_ok && !from_backup && !install::paused(id) => {
            trail::note(format!("Install {id}: {} didn't download ({e}), trying the RecompLauncher backup", asset.name));
            let bk = catalog::backup_release(&st.http, id, os).await.map_err(|_| e)?;
            let all = assets::all_for(&bk.assets, os, arch, pattern);
            let a = all.iter().copied().find(|a| a.name == asset.name).or_else(|| assets::most_like(&all, &asset.name)).or_else(|| assets::pick(&bk.assets, os, arch, pattern)).or_else(|| assets::pick(&bk.assets, os, arch, None)).cloned()
                .ok_or_else(|| Error::Msg("The RecompLauncher backup has no file for this PC.".into()))?;
            asset = a;
            tag_got = bk.tag_name.clone();
            from_backup = true;
            install::download(&st.http, &asset, &home.join(".downloads"), id, cancel.clone(), &report).await?
        }
        Err(e) => return Err(e),
    };
    if from_backup {
        trail::note(format!("Install {id}: {} came from the RecompLauncher backup", asset.name));
    }
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

    // An update: keep a copy of its saves in its folder first (only the known save folders).
    if let (Some(p), true) = (&prev, backup_first) {
        report(Progress::new(id, "backup", 0, 0));
        let (g, spots, bdir) = (p.clone(), all_save_spots(st, p), backups_dir(p));
        let _ = tokio::task::spawn_blocking(move || manage::backup(&g.path, &saves::files_inside(&spots, &g), &bdir, 5)).await;
    }

    // Only an installer: it waits in the launcher's staging folder for Set Up, which runs it when
    // the player presses Run Installer. A new game gets its place in the Library (the folder Set
    // Up suggests); an update keeps everything until its installer has run.
    if as_installer {
        report(Progress::new(id, "unpack", 0, 0));
        let (file, sdir) = (got.file.clone(), installer::staging(&st.dir, id));
        let setup = tokio::task::spawn_blocking(move || installer::stage(&file, &sdir)).await.map_err(|e| Error::Msg(e.to_string()))??;
        // A zip with the installer in it stays with Keep downloaded files on, like a game's zip.
        // (A lone setup program has already moved to staging, so there's nothing left to keep.)
        if !keep {
            let _ = std::fs::remove_file(&got.file);
        }
        {
            let mut l = st.lib.lock().unwrap();
            match l.installed.get_mut(id) {
                Some(g) => {
                    g.installer_setup = setup.to_string_lossy().to_string();
                    g.installer_tag = tag_got.clone();
                    g.asset = asset.name.clone();
                    g.from_backup = from_backup;
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
                        from_backup,
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

    // Unpacked beside the game's folder first: a full drive or a bad download stops here, with the
    // game untouched.
    report(Progress::new(id, "unpack", 0, 0));
    let staging = home.join(".staging").join(folder_name(id));
    let staged = match stage_release(&got.file, &staging, &asset.name).await {
        // On Linux a game plays only from a plain folder, and its AppImage didn't unpack: the
        // release's tarball, when it has one. Never the AppImage as it is.
        Err(Error::Msg(m)) if m.starts_with(install::APPIMAGE_UNPACK) => {
            trail::note(format!("Install {id}: {}", m.trim_start_matches(install::APPIMAGE_UNPACK)));
            let tarball = all.iter().copied().find(|a| plain_fallback(a, &repo, rel.project_id, sf)).cloned().ok_or_else(|| Error::Msg(NO_PLAIN_BUILD.into()))?;
            trail::note(format!("Install {id}: its AppImage didn't unpack, so {} instead", tarball.name));
            let _ = std::fs::remove_file(&got.file);
            asset = tarball;
            got = install::download(&st.http, &asset, &home.join(".downloads"), id, cancel.clone(), &report).await?;
            report(Progress::new(id, "unpack", 0, 0));
            stage_release(&got.file, &staging, &asset.name).await?
        }
        other => other?,
    };
    let (staged, old_app_files) = staged.for_game(&target, assets::os_now())?;
    let files = staged.files.clone();

    // What of the old release goes with it: the files the new one no longer has, and a single-file
    // build of another one picked (one region for another), so the new one is the one that starts.
    let mut dropped = vec![];
    if let Some(p) = prev.as_ref() {
        dropped = install::dropped(&target, &p.files, &files, p.updated_at, p.exe.as_deref());
        let low = p.asset.to_lowercase();
        let single_file = low.ends_with(".appimage") || low.ends_with(".exe");
        let other_build = PathBuf::from(&p.asset);
        if single_file && p.asset != asset.name && !files.contains(&other_build) {
            dropped.push(other_build);
        }
    }
    // Counted before the program to keep is worked out: the old app's program may be one of them.
    dropped.extend(old_app_files);

    // Keep the program the player picked, unless the update replaced it with a differently named one
    // (an AppImage carries its version in its name). Worked out before the files move, while the old
    // one is still there to follow.
    let names = [str_of(&entry, "name"), repo.rsplit(['/', ':']).next().unwrap_or(""), str_of(&entry, "game")];
    let picked = prev.as_ref().map(|p| p.exe_rules == u8::MAX).unwrap_or(false);
    let exe = install::kept_exe(prev.as_ref().and_then(|p| p.exe.clone()), &target, os, &dropped, picked, &names);
    let (exe, replaced) = install::updated_exe(id, &target, exe, &files, picked);
    dropped.extend(replaced);

    // All of the new release in, or none of it (see swap). An update the launcher was closed in the
    // middle of is settled first.
    let gone = dropped.len();
    let (t2, files2, sha, record) = (target.clone(), files.clone(), got.sha256.clone(), prev.clone());
    let update = tokio::task::spawn_blocking(move || {
        if let Some(what) = swap::recover(&t2, record.as_ref()) {
            trail::note(format!("Install: {what} ({})", t2.display()));
        }
        let old_release = record.as_ref().map(|r| r.files.as_slice()).unwrap_or_default();
        let placed = swap::put_in_place(&t2, &staged.root, &files2, &sha, &dropped, old_release);
        let _ = std::fs::remove_dir_all(&staging);
        placed
    })
    .await
    .map_err(|e| Error::Msg(e.to_string()))??;
    if let Some(p) = prev.as_ref().filter(|_| gone > 0) {
        trail::note(format!("Install {id}: {gone} files of {} that {tag_got} no longer has, taken away", p.tag));
    }
    // A game whose own setup works from a clone of its repo gets one, around the release's files.
    if boot::Boot::from_entry(&entry).map(|b| b.clone).unwrap_or(false) {
        report(Progress::new(id, "clone", 0, 0));
        let (r2, t3, scratch) = (repo.clone(), target.clone(), home.join(".staging").join(format!("{}-repo", folder_name(id))));
        let cloned = tokio::task::spawn_blocking(move || manage::clone_repo(&r2, &t3, &scratch)).await.map_err(|e| Error::Msg(e.to_string())).and_then(|r| r);
        if let Err(e) = cloned {
            return Err(update.undo_after(e));
        }
    }
    if !kept.is_empty() {
        if let Some(prof) = profile::find(&target, &repo, &entry) {
            prof.restore(&target, &kept);
        }
    }

    let exe = exe.or_else(|| install::find_game_exe(&target, os, &names));
    // The catalog's boot profile says which program starts it, unless the player picked one.
    let boot = boot::Boot::from_entry(&entry).map(|b| b.for_build(os == "windows"));
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

    let mut rec = Installed {
        id: id.into(),
        name: str_of(&entry, "name").into(),
        game: prev.as_ref().map(|p| p.game.clone()).unwrap_or_default(),
        files_in_place: prev.as_ref().map(|p| p.files_in_place).unwrap_or(false),
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
        prepare_opts: prev.as_ref().map(|p| p.prepare_opts.clone()).unwrap_or_default(),
        from_backup,
        // What was set up, and on which release, stays through an update: a new release is told
        // apart from the one it was set up on, and builds again on the next Play.
        set_up: prev.as_ref().map(|p| p.set_up.clone()).unwrap_or_default(),
        // A build cut short before the update still has to be built again after it.
        build_cut: prev.as_ref().is_some_and(|p| p.build_cut),
        upgrade: prev.as_ref().and_then(|p| p.upgrade.clone()),
        setup_inputs: prev.as_ref().map(|p| p.setup_inputs.clone()).unwrap_or_default(),
        unpacked_from: prev.as_ref().map(|p| p.unpacked_from.clone()).unwrap_or_default(),
        input_staged: prev.as_ref().map(|p| p.input_staged.clone()).unwrap_or_default(),
        input_stamps: prev.as_ref().map(|p| p.input_stamps.clone()).unwrap_or_default(),
        proton: os == "windows" && assets::os_now() == "linux",
        picks: prev.as_ref().map(|p| p.picks.clone()).unwrap_or_default(),
        screen_fit: prev.as_ref().map(|p| p.screen_fit.clone()).unwrap_or_default(),
        screen_size: prev.as_ref().map(|p| p.screen_size.clone()).unwrap_or_default(),
        slider_vals: prev.as_ref().map(|p| p.slider_vals.clone()).unwrap_or_default(),
        args: prev.as_ref().map(|p| p.args.clone()).unwrap_or_default(),
        auto_update: prev.as_ref().map(|p| p.auto_update).unwrap_or(false),
        rebuild_after_update: prev.as_ref().map(|p| p.rebuild_after_update).unwrap_or(false),
        local: false,
        look: prev.as_ref().map(|p| p.look.clone()).unwrap_or_default(),
        look_amount: prev.as_ref().map(|p| p.look_amount).unwrap_or(50),
        cloud: prev.as_ref().and_then(|p| p.cloud),
        save_place: prev.as_ref().and_then(|p| p.save_place.clone()),
        save_override: prev.as_ref().and_then(|p| p.save_override.clone()),
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
        mode: prev.as_ref().map(|p| p.mode).unwrap_or_default(),
        // An update remembers the version it came from; installing the same version again keeps
        // what was remembered before.
        // The version it came from, for "go back" after a bad update. A version the player picked
        // (going back, or an older one on purpose) isn't an update, so there's nothing to go back to.
        prev_tag: if picked_tag { String::new() } else { prev.as_ref().map(|p| if !p.tag.is_empty() && p.tag != tag_got { p.tag.clone() } else { p.prev_tag.clone() }).unwrap_or_default() },
        installer: false,
        installer_setup: String::new(),
        installer_tag: String::new(),
        boot_hash: prev.as_ref().map(|p| p.boot_hash.clone()).unwrap_or_default(),
        unpacked: prev.as_ref().map(|p| p.unpacked).unwrap_or(false),
        from_appimage: prev.as_ref().map(|p| p.from_appimage.clone()).unwrap_or_default(),
        input_checks: prev.as_ref().map(|p| p.input_checks.clone()).unwrap_or_default(),
    };
    {
        let mut l = st.lib.lock().unwrap();
        // Skip Easy Setup ticked before it was installed, or on for every game in Settings: from
        // now on it's the game's own mode. A game installed under the Settings switch then stays
        // the player's when the switch goes off, instead of asking for their copy.
        if prev.is_none() && fresh_install_skips(&mut l.settings.ui, id) {
            rec.mode = library::SetupMode::Own;
        }
        l.put(id, rec);
        l.settings.asked_location = true;
    }
    // The update counts once the saved library has its files. Until then both releases' files stay,
    // and the next start keeps the ones the saved library has.
    match st.try_save() {
        Ok(()) => {
            let _ = tokio::task::spawn_blocking(move || update.finish()).await;
        }
        Err(e) => {
            trail::note(format!("Install {id}: the library couldn't be saved ({e}), so its old files wait for the next start"));
            update.leave_for_next_start();
        }
    }
    if !keep {
        let _ = std::fs::remove_file(&got.file);
    }
    note_download(
        &st.dir,
        json!({"id": id, "name": str_of(&entry, "name"), "tag": tag_got, "from": prev.as_ref().map(|p| p.tag.clone()).unwrap_or_default(), "repo": str_of(&entry, "repo"), "size": asset.size, "secs": started.elapsed().as_secs(), "when": now(), "update": prev.is_some(), "verified": got.verified}),
    );
    let mut p = Progress::new(id, "done", 1, 1);
    let gh = !catalog::site_only(str_of(&entry, "repo"));
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

/// The release out now for each recomp, with what its players said (the site's scores).
#[tauri::command]
async fn latest_counts(st: State<'_, App>, force: Option<bool>) -> Res<Value> {
    Ok(catalog::latest_counts(&catalog::play_scores(&st.http, &st.dir, force.unwrap_or(false)).await?.data))
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

/// This week's chart of the most played recomps (Discover's Trending row).
#[tauri::command]
async fn trending(st: State<'_, App>) -> Res<Value> {
    Ok(catalog::trending(&st.http, &st.dir).await?.data)
}

/// The partner card's data for this launch (none when the site can't be reached).
#[tauri::command]
async fn ig_info(st: State<'_, App>) -> Res<Value> {
    Ok(catalog::ig_info(&st.http).await.unwrap_or(Value::Null))
}

/// The partner card was seen on a game's page.
#[tauri::command]
async fn ig_view(st: State<'_, App>, id: String, variant: String, round: u64) -> Res<()> {
    let ok = |s: &str| !s.is_empty() && s.len() <= 80 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
    if ok(&id) && ok(&variant) {
        let http = st.http.clone();
        tauri::async_runtime::spawn(async move { catalog::ig_view(&http, &id, &variant, round).await });
    }
    Ok(())
}

/// How a finished download's files were checked (Downloads shows it beside each one).
#[tauri::command]
async fn scan_info(st: State<'_, App>, id: String, tag: String, force: Option<bool>) -> Res<Value> {
    Ok(catalog::scan(&st.http, &st.dir, &id, &tag, force.unwrap_or(false)).await?.data)
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

/// How many people want a recomp (saved it, or wait for its Notify Me email), for the buttons on its
/// page. 0 when the website can't say: it can't be reached and nothing is kept yet, it's an older site
/// without the count, or it doesn't list that recomp.
#[tauri::command]
async fn want_count(st: State<'_, App>, id: String) -> Res<u64> {
    let got = catalog::interest(&st.http, &st.dir, &id).await;
    Ok(got.map(|f| catalog::interest_n(&f.data)).unwrap_or(0))
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
                if setup_temp::io_too_long(&io) {
                    trail::note(format!("Install {id}: {io}"));
                    Error::Msg(setup_temp::TOO_LONG.into())
                } else if (win && code == Some(5)) || io.kind() == std::io::ErrorKind::PermissionDenied {
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
    let rel = release_on(&rels, &have.0).or_else(|| catalog::latest(&rels)).ok_or_else(|| Error::Msg("It has no releases yet.".into()))?;
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
fn uninstall(st: State<'_, App>, id: String, delete_files: bool, run_uninstaller: Option<bool>, keep_saves: Option<bool>) -> Res<Value> {
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
    // A local build's folder is the developer's: it only leaves the Library. Unless the launcher
    // unpacked the player's AppImage into it: that folder is the launcher's own.
    let delete_files = delete_files && (!g.local || g.unpacked);
    let mut left: Vec<String> = vec![];
    let mut kept = false;
    let mut uninstaller = false;
    let mut kept_copy: Option<PathBuf> = None;
    let mut copy_stuck = false;
    let mut saves_kept = false;
    let mut saves_stuck = false;
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
        // Its saves inside the folder ({data}/saves, say): a copy beside the library first, which
        // uninstalling leaves and the Saves tab puts back after a reinstall. Asked for unless the
        // player said not to; if it can't be made, the folder stays.
        if ok && keep_saves != Some(false) {
            match keep_saves_inside(&st, &g) {
                Ok(n) => saves_kept = n > 0,
                Err(e) => {
                    trail::note(format!("Uninstall {id}: its saves couldn't be copied first ({e}), the folder stays"));
                    keep_failed = true;
                    saves_stuck = true;
                }
            }
        }
        if ok && !keep_failed {
            for c in g.setup_inputs.values() {
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
            copy_stuck = keep_failed && !saves_stuck;
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
    Ok(json!({"left": left.len(), "path": g.path, "kept": kept, "uninstaller": uninstaller, "kept_copy": kept_copy, "copy_stuck": copy_stuck, "saves_kept": saves_kept, "saves_stuck": saves_stuck}))
}

/// A copy of the saves a game keeps inside its own folder, for it and each of its games with saves
/// of their own, made before the folder is deleted. Copies go beside the library (or to the
/// player's chosen folder), never inside the game's folder. How many games had saves to keep.
fn keep_saves_inside(st: &App, g: &Installed) -> Result<usize, Error> {
    let mut ids = vec![g.id.clone()];
    for game in games_of(st, &g.id, g).iter().filter(|x| !x.saves.is_empty()) {
        ids.push(games::full(&g.id, &game.id));
    }
    let mut kept = 0;
    for id in ids {
        let spots = cloud::spots_of(st, &id, g);
        if saves::files_inside(&spots, g).is_empty() {
            continue;
        }
        let copies = saves::dir_of(&st.dir, &id, g);
        if library::inside(&copies, &g.path) {
            return Err(Error::Msg("its copies would be inside the folder being deleted".into()));
        }
        // None: nothing changed since the newest copy, which already holds them.
        saves::backup(&st.dir, &id, &spots, g)?;
        kept += 1;
    }
    Ok(kept)
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
            sizes.insert(id, json!(manage::game_size(&p)));
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

/// How much room one installed game takes (its folder without the old files an update set aside),
/// for the On Disk box on its page. None when the launcher can't see its folder.
#[tauri::command]
async fn game_size(st: State<'_, App>, id: String) -> Res<Option<u64>> {
    let p = st.lib.lock().unwrap().installed.get(&id).map(|g| g.path.clone());
    let Some(p) = p else { return Ok(None) };
    tokio::task::spawn_blocking(move || if p.is_dir() { Some(manage::game_size(&p)) } else { None })
        .await
        .map_err(|e| Error::Msg(e.to_string()))
}

#[tauri::command]
async fn move_game(app: AppHandle, st: State<'_, App>, id: String, to: String, use_there: Option<bool>) -> Res<Installed> {
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
    // The player copied it there already: the launcher can use that copy, and the old folder stays
    // for them to delete once the game starts from its new place.
    if holds_same_game(&g, &dest) {
        if use_there != Some(true) {
            return Err(Error::Msg(format!("SAME_THERE|{}", dest.display())));
        }
        let old_b = backups_dir(&g);
        let mut out = g.clone();
        out.path = dest;
        if old_b.is_dir() && !backups_dir(&out).exists() {
            let _ = manage::move_dir(&old_b, &backups_dir(&out), &|_, _| {});
        }
        st.lib.lock().unwrap().installed.insert(id.clone(), out.clone());
        st.save();
        trail::note(format!("{id}: uses the copy already at {} (the old folder stays at {})", out.path.display(), g.path.display()));
        return Ok(out);
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
    r.map_err(|e| match e {
        Error::Io(io) if setup_temp::io_too_long(&io) => Error::Msg(setup_temp::TOO_LONG.into()),
        e => e,
    })?;
    // Its backups go with it (its {data}, caches and all, is inside its folder and went too).
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

/// A folder that already holds this game's files: its program at the same place inside, or, when
/// the launcher doesn't know its program, everything the game's folder has at its top.
fn holds_same_game(g: &Installed, dest: &Path) -> bool {
    if !dest.is_dir() || !g.path.is_dir() || dest == g.path {
        return false;
    }
    if let Some(exe) = g.exe.as_ref().filter(|e| !e.as_os_str().is_empty() && e.is_relative()) {
        return dest.join(exe).is_file() && g.path.join(exe).is_file();
    }
    // Old files an update set aside aren't the game's, and a copy elsewhere doesn't have them.
    let names: Vec<std::ffi::OsString> = std::fs::read_dir(&g.path).into_iter().flatten().flatten().map(|e| e.file_name()).filter(|n| n != swap::FOLDER).collect();
    !names.is_empty() && names.iter().all(|n| dest.join(n).exists())
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
    let spots = all_save_spots(&st, &g);
    if spots.is_empty() {
        return Err(Error::Msg(cloud::SAVES_UNKNOWN.into()));
    }
    tokio::task::spawn_blocking(move || manage::backup(&g.path, &saves::files_inside(&spots, &g), &backups_dir(&g), 5)).await.map_err(|e| Error::Msg(e.to_string()))?
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
                let part = roms::part_of_file(&r.path);
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
    support_in(&dir)
}

/// What the recomplauncher.json in a game's folder covers. Null when there is none. A file the
/// launcher can't read sets nothing up (Play and Easy Setup skip it): `{"problem": …}` says why,
/// for the box to show instead of the catalog's labels.
fn support_in(dir: &Path) -> Option<Value> {
    if let Some(owned) = profile::release_profile(dir) {
        return Some(profile_covers(&owned.profile));
    }
    let problem = profile::release_file_problem(dir)?;
    Some(json!({ "problem": problem }))
}

/// Each Launcher Support feature a recomplauncher.json sets up itself.
fn profile_covers(p: &profile::Profile) -> Value {
    let game = p.game.as_ref();
    // The player's copy asked for through named inputs (no `game` block) is Easy Setup too: a
    // required input of role game or disc that isn't a shared key. The site counts it the same way
    // (rs_sdk_easy), so the card agrees with the catalog.
    let copy_input = p.inputs.iter().any(|i| !i.optional && i.shared.is_empty() && (i.role == "game" || i.role == "disc"));
    // A `game` block alone sets nothing up: Easy Setup (Profile::easy_setup) also needs a program
    // to start, so without one the box would tick what Install doesn't offer.
    let easy_setup = (p.start.is_some() && game.is_some()) || copy_input;
    json!({
        "easy_setup": easy_setup,
        "settings": !p.settings.is_empty(),
        "prepare": p.prepare.is_some(),
        "verified": game.map(|g| !g.dumps.is_empty() || !g.check.ids.is_empty()).unwrap_or(false),
        "saves": !p.saves.is_empty(),
    })
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
        // A disc set's .m3u and the .cue files it lists sit side by side: the set is one game.
        let found = roms::fold_sets(found);
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
    // the copy's checks). An Xbox 360 update or DLC carries its game's title ID, so it would pass a
    // check by ID, but it's never the game.
    let installed = |r: &roms::RomFile| dirs.values().any(|d| !d.as_os_str().is_empty() && &r.path == d);
    let found: Vec<roms::RomFile> = found.iter().filter(|r| !installed(r) && !roms::is_extra_package(r)).cloned().collect();
    let found = &found[..];
    let mut out = roms::match_catalog(cat, found);
    let none = PathBuf::new();
    // Each file's disc ID and kind, read once: every game with a disc ID would otherwise open every
    // disc image the player has, again for each game.
    let (dirs_seen, ids_seen): (std::cell::RefCell<HashMap<PathBuf, bool>>, std::cell::RefCell<HashMap<PathBuf, Option<String>>>) = Default::default();
    let is_dir = |p: &Path| *dirs_seen.borrow_mut().entry(p.to_path_buf()).or_insert_with(|| p.is_dir());
    let disc_id = |p: &Path| -> Option<String> {
        if let Some(r) = ids_seen.borrow().get(p) {
            return r.clone();
        }
        let r = roms::copy_id(p);
        ids_seen.borrow_mut().insert(p.to_path_buf(), r.clone());
        r
    };
    for a in cat.get("apps").and_then(Value::as_array).into_iter().flatten() {
        let (Some(id), repo) = (a.get("id").and_then(Value::as_str), a.get("repo").and_then(Value::as_str).unwrap_or("")) else { continue };
        let Some(need) = profile::find(dirs.get(id).unwrap_or(&none), repo, a).and_then(|p| p.main_copy()) else { continue };
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
            let by_contents = need.identifies_kind(is_dir(p));
            if want.is_empty() {
                by_contents
            } else {
                disc_id(p).map(|id| want.iter().any(|w| id.starts_with(w.as_str()))).unwrap_or(by_contents)
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
            // So is a listed title ID for a PlayStation folder: its param.sfo says which game it is.
            let by_title_id = !want.is_empty() && matches!(r.system.as_str(), "ps3" | "ps4" | "ps5");
            let matched_by_title = !by_build
                && !by_title_id
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
                l.move_play_history(&old, &new);
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
        // Copies in the player's folder are kept by id. Beside the game they're kept by its
        // folder, which doesn't change: then both are the same folder and nothing moves.
        let renamed = st.lib.lock().unwrap().installed.get(&new).cloned();
        let copies = (saves::place(&st.dir, &old, renamed.as_ref()), saves::place(&st.dir, &new, renamed.as_ref()));
        let mut dirs = vec![copies, (st.dir.join("saves").join(&old), st.dir.join("saves").join(&new)), (st.dir.join("looks").join(&old), st.dir.join("looks").join(&new))];
        dirs.retain(|(from, to)| from != to);
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
    let (first_time, new_ids, notify, found, news, out_now) = {
        let mut l = st.lib.lock().unwrap();
        let first = l.seen_apps.is_empty();
        let new_ids: Vec<String> = ids.iter().filter(|i| !l.seen_apps.contains(i)).cloned().collect();
        l.seen_apps = ids.clone();
        let news = followed_news(cat, &mut l, &new_ids, first);
        let saved: Vec<String> = l.settings.ui.get("saved").and_then(Value::as_object).map(|o| o.keys().cloned().collect()).unwrap_or_default();
        let out_now = saved_out_now(cat, &saved, &l.seen_soon);
        l.seen_soon = soon_ids(cat);
        (first, new_ids, l.settings.notify, l.roms.clone(), news, out_now)
    };
    st.save();
    if notify {
        for (title, body, id) in &news {
            ingame::notify(app, title, body);
            let _ = app.emit("followed", id);
        }
        // Saved while it was Coming Soon (its Save button): now it can be installed.
        for id in &out_now {
            if let Some(a) = catalog::app(cat, id) {
                ingame::notify(app, &format!("{} is out", str_of(a, "name")), "A game you saved can be installed now. It's in your Library.");
            }
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

/// The catalog's Coming Soon recomps (not the player's own, added by link).
fn soon_ids(cat: &Value) -> Vec<String> {
    let apps = cat.get("apps").and_then(Value::as_array).into_iter().flatten();
    let soon = apps.filter(|a| a.get("soon").and_then(Value::as_bool).unwrap_or(false) && !a.get("custom").and_then(Value::as_bool).unwrap_or(false));
    soon.filter_map(|a| a.get("id")?.as_str().map(str::to_string)).collect()
}

/// Saved games that were Coming Soon in the last catalog and are out in this one: each is told about
/// once, as the next catalog no longer has it as Coming Soon. One that left the catalog isn't out.
fn saved_out_now(cat: &Value, saved: &[String], was_soon: &[String]) -> Vec<String> {
    let soon_now = soon_ids(cat);
    let listed = |id: &str| catalog::app(cat, id).is_some();
    saved.iter().filter(|id| was_soon.contains(id) && !soon_now.contains(id) && listed(id)).cloned().collect()
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
/// pick or put in place, or settings it takes from the environment.
fn direct_start(st: &App, id: &str, g: &Installed) -> Option<(PathBuf, Vec<String>)> {
    let prof = profile_of(st, id, g);
    if !prof.as_ref().map(|p| p.start.is_some() && !p.from_boot).unwrap_or(false) && own_setup::pending(st, id, g).is_some() {
        return None;
    }
    let (exe, mut args) = match &prof {
        Some(p) if p.start.is_some() => {
            if p.start.as_ref().map(|s| !s.env.is_empty()).unwrap_or(false) {
                return None;
            }
            // Started as it is: the copy it reads has to be there already, and placed.
            let copy = inputs::main_path(p, g);
            if p.start_uses_game() && copy.is_none() || !p.missing_after_place(&g.path).is_empty() {
                return None;
            }
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

/// The catalog entry a game's pictures come from: its own, or for one game of several the recomp's
/// with that game's own pictures over it.
async fn art_entry(st: &App, id: &str, pk: Option<&GamePick>) -> Value {
    let cat = catalog::catalog(&st.http, &st.dir, false).await.map(|f| f.data).unwrap_or(Value::Null);
    match pk {
        Some(pk) => games::with_game_art(catalog::app(&cat, &pk.recomp).unwrap_or(&Value::Null), &pk.game),
        None => catalog::app(&cat, id).cloned().unwrap_or(Value::Null),
    }
}

/// Show Game Names: recomps show their game's pictures, not their project's (catalog::art_url).
fn game_names_on(st: &App) -> bool {
    st.lib.lock().unwrap().settings.ui.get("game_names").and_then(Value::as_bool).unwrap_or(false)
}

/// The catalog's icon for a game, kept in the covers folder: the file its desktop shortcut and Steam
/// entry point at. Fetched again when the picture it should be changes (Show Game Names, a new one
/// from the website) and written over in place, so those shortcuts change with it. One that can't
/// be fetched stays as it was.
async fn list_icon(st: &App, id: &str, url: &str) -> Option<PathBuf> {
    let covers = st.dir.join("covers");
    let file = covers.join(format!("{}-icon-list.png", folder_name(id)));
    let from = file.with_extension("from");
    let kept = std::fs::read_to_string(&from).unwrap_or_default();
    if file.is_file() && kept == url {
        return Some(file);
    }
    let b = fetch_bytes(&st.http, url).await;
    if crate::media::mime(&b) == "image/png" {
        let _ = std::fs::create_dir_all(&covers);
        let _ = std::fs::write(&file, &b);
        let _ = std::fs::write(file.with_extension("ico"), art::ico_from_png(&b));
        let _ = std::fs::write(&from, url);
    } else if !b.is_empty() {
        // Not a picture shortcuts can use: the other set's old one would be wrong, the program's is not.
        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_file(file.with_extension("ico"));
        let _ = std::fs::remove_file(&from);
    }
    Some(file).filter(|f| f.is_file())
}

async fn game_art(st: &App, id: &str, g: &Installed) -> GameArt {
    // One game of several: its own pictures from the catalog, else the recomp's. The player's own
    // (Customize) are the recomp's, so they're left for the recomp.
    let sel = game_of(st, id);
    let a = art_entry(st, id, sel.as_ref()).await;
    let game_only = game_names_on(st);
    let url = |kind: &str| catalog::art_url(&a, kind, game_only).to_string();
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
    let cover = pick(st, g.cover_file.as_ref(), &url("cover")).await;
    let hero = pick(st, g.hero_file.as_ref(), &url("hero")).await;
    let logo = pick(st, g.logo_file.as_ref(), &url("logo")).await;
    // The icon: the player's, else the list's (kept in the covers folder), else (Windows) the
    // game's program.
    let mut icon = g.icon_file.clone().filter(|f| f.is_file());
    let icon_url = url("icon");
    if icon.is_none() && !icon_url.is_empty() {
        icon = list_icon(st, id, &icon_url).await;
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

/// Settings → Show Game Names changed which set of pictures recomps show: the desktop shortcuts and
/// Steam entries already made get the new set too. A desktop shortcut points at the game's icon
/// file, which list_icon writes over in place; Steam's pictures are written again in its grid
/// folders. How many Steam entries changed.
#[tauri::command]
async fn refresh_shortcut_art(st: State<'_, App>) -> Res<usize> {
    let game_only = game_names_on(&st);
    let installed: Vec<(String, Installed)> = st.lib.lock().unwrap().installed.iter().map(|(k, g)| (k.clone(), g.clone())).collect();
    let mut ids = vec![];
    for (rid, g) in &installed {
        ids.push(rid.clone());
        ids.extend(games_of(&st, rid, g).iter().map(|x| games::full(rid, &x.id)));
    }
    for id in &ids {
        // Only the games with a kept icon: the ones a shortcut or Steam entry was made for.
        let kept = st.dir.join("covers").join(format!("{}-icon-list.png", folder_name(id)));
        if !kept.is_file() {
            continue;
        }
        let a = art_entry(&st, id, game_of(&st, id).as_ref()).await;
        let url = catalog::art_url(&a, "icon", game_only);
        if !url.is_empty() {
            list_icon(&st, id, url).await;
        }
    }
    Ok(steam_sync::refresh_art(&st).await)
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
    let want = steam_sync::want_for(&st.dir, &id, &name);
    let n = steam_sync::add_game(&steam, &st.dir, &want, art, icon).await.inspect_err(|e| trail::note(format!("Add to Steam ({id}): {e}")))?;
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
    // GitHub's README, or a Forgejo project's from its own server. A GitLab or SourceForge project
    // shows the catalog's description instead.
    let fj = catalog::is_forgejo(&repo) && catalog::repo_ok(&repo);
    if catalog::site_only(&repo) && !fj {
        return Ok(None);
    }
    let ok = fj || (repo.split('/').count() == 2 && !repo.contains("..") && repo.chars().all(|c| c.is_ascii_alphanumeric() || "-_./".contains(c)));
    if !ok {
        return Ok(None);
    }
    let file = st.dir.join("cache/readme").join(format!("{}.md", repo.replace(':', "_").replace('/', "__")));
    let age = std::fs::metadata(&file).and_then(|m| m.modified()).ok().and_then(|m| m.elapsed().ok()).map(|d| d.as_secs());
    let disk = || std::fs::read_to_string(&file).ok();
    if age.map(|a| a < 24 * 3600).unwrap_or(false) {
        return Ok(disk().filter(|t| !t.is_empty()));
    }
    let text = if fj {
        catalog::forgejo_readme(&st.http, &repo).await
    } else {
        let got = st
            .http
            .get(format!("https://api.github.com/repos/{repo}/readme"))
            .header("Accept", "application/vnd.github.raw")
            .timeout(std::time::Duration::from_secs(20))
            .send()
            .await;
        match got {
            Ok(r) if r.status() == reqwest::StatusCode::NOT_FOUND => Some(String::new()),
            Ok(r) if r.status().is_success() => r.text().await.ok(),
            _ => None,
        }
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
    start_game_with_monitor_context(app, id, _steam, None)
}

fn start_game_with_monitor_context(
    app: &AppHandle,
    id: &str,
    _steam: Option<&sysenv::SteamLaunch>,
    target_monitor: Option<display::MonitorInfo>,
) -> Res<()> {
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
    let g = in_plain_folder(&st, id)?;
    // Skip Easy Setup: the program the player picked (Game Executable), or else the release's own,
    // with its launch options. The setup file's command line is for its own program, so neither
    // gets it, and nothing of Easy Setup is asked for.
    let picked = player_program(&st, id, &g);
    if let Some(mine) = picked.as_ref() {
        trail::note(format!("Play {id}: the program the player picked ({}), not its setup file's", mine.display()));
    }
    let skipped = picked.is_none() && player_sets_up(&st, id);
    if skipped {
        trail::note(format!("Play {id}: Easy Setup skipped, it starts as its own setup leaves it"));
    }
    let own_road = picked.is_some() || skipped;
    // A recomp the launcher knows how to set up: the player's copy goes where the game reads it,
    // and the game starts straight away instead of its own launcher.
    let whole = if own_road { None } else { profile_of(&st, id, &g) };
    // One game of several: only its inputs, and its own saves when it has them.
    let prof = whole.clone().map(|mut p| {
        if let Some(pk) = &pick {
            games::scope(&mut p, &pk.game, &pk.all);
        }
        p
    });
    if let Some(p) = prof.as_ref().filter(|p| !p.inputs.is_empty()) {
        inputs::validate(p).map_err(Error::Msg)?;
        // Set up, or why not: the answer the game's page shows. It comes before its settings are
        // written: the settings file often lives in the folder it built (hookshot_options.ini), and
        // writing it would make a missing build folder look present again. What the player picked
        // stays as it is: the setup that opens builds again from their files.
        let sub = pick.as_ref().map(|pk| pk.game.id.as_str());
        let status = game_status(&st, &g, p, sub);
        if !status.ready {
            trail::note(format!("Play {id}: not set up ({}) {}", status.reason, status.detail));
            return Err(not_set_up(&status));
        }
        if status.reason == "built" {
            trail::note(format!("Play {id}: its build is already on this PC, no copy needed"));
        }
        // Built: the files it no longer reads may be gone (Free Up Space), and aren't placed.
        let rows = inputs::rows(p, &g, &st.dir).map_err(Error::Msg)?;
        let now = without_freed(p, &rows);
        let mut staged = g.clone();
        inputs::stage(&now, &mut staged, &st.dir).map_err(Error::Msg)?;
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
    if picked.is_none() && !prof.as_ref().map(|p| p.start.is_some() && !p.from_boot).unwrap_or(false) {
        // Only its installer so far: Set Up runs it first.
        if !g.installer_setup.is_empty() && g.exe.is_none() {
            return Err(Error::Msg("Its installer hasn't run yet. Press Set Up on its page.".into()));
        }
        if let Some(setup) = own_setup::pending_for_play(&st, id, &g) {
            // A setup that takes the player's copy (an ISO dropped on install.bat): asked for first.
            let copy = match prof.as_ref().filter(|p| p.main_input().is_some_and(|i| i.for_setup)) {
                Some(p) => copy_for_setup(&g, p)?,
                None => None,
            };
            return own_setup::run(app, id, &g, &setup, copy.as_deref());
        }
    }
    let g = st.lib.lock().unwrap().installed.get(id).cloned().unwrap_or(g);
    if skipped && g.exe.is_none() {
        return Err(Error::Msg("Easy Setup is skipped for this game, and it doesn't have a program to start yet. Pick it with Set Game Executable in its Manage menu.".into()));
    }
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
        _ if own_road => {
            // A game whose profile hands the copy over in a variable (files.how "env"): a copy
            // chosen in Easy Setup before it was skipped still goes in it. Otherwise the game
            // would start with nothing and say its data was not found.
            let p = profile_of(&st, id, &g);
            let by_env = p.as_ref().and_then(|p| p.start.as_ref()).is_some_and(|s| s.env.values().any(|v| v.contains("{game}")));
            let copy = p.as_ref().and_then(|p| inputs::picked(&g, p.main_input()?.id.as_str()));
            if let (Some(c), Some(p)) = (copy, p.filter(|_| by_env)) {
                game_env = p.env_vars(&g.path, Some(&c));
                trail::note(format!("Play {id}: Easy Setup skipped, its copy still goes in {}", game_env.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(", ")));
            }
            own_plan(&g, picked.as_deref())?
        }
        Some(p) if p.start.is_some() => {
            // (Set up: checked above, before anything was written.)
            let copy = inputs::main_path(p, &g);
            p.prepare(&g.path, copy.as_deref()).map_err(|e| Error::Msg(format!("Couldn't write its settings: {e}")))?;
            p.place(&g.path, copy.as_deref()).map_err(|e| Error::Msg(format!("Couldn't put your copy of the game where it reads it: {e}")))?;
            // Everything the game reads has to be there now; if not, the copy picked isn't the
            // right folder (the files are one level down, or it's a different disc): pick again.
            let mut missing = p.missing_after_place(&g.path);
            // The files a level or two down in the folder picked (the folder around them, or one
            // named after the game's ID): that inner folder is the copy, so the game isn't asked
            // for again and again.
            let main = p.main_input().cloned().unwrap_or_default();
            if let (Some(c), false) = (copy.as_deref(), missing.is_empty()) {
                let want: Vec<String> = main.contains.iter().filter(|x| !x.contains(['*', '/', '\\'])).map(|x| x.to_lowercase()).collect();
                if let Some(inner) = folder_with(c, &want, 3) {
                    trail::note(format!("Play {id}: the game's files are in {}, one level down", inner.display()));
                    if let Some(x) = st.lib.lock().unwrap().installed.get_mut(id) {
                        x.setup_inputs.insert(main.id.clone(), inner.clone());
                    }
                    st.save();
                    p.place(&g.path, Some(inner.as_path())).map_err(|e| Error::Msg(format!("Couldn't put your copy of the game where it reads it: {e}")))?;
                    missing = p.missing_after_place(&g.path);
                }
            }
            if copy.is_some() && !missing.is_empty() {
                trail::note(format!("Play {id}: still missing after placing the copy: {}", missing.join(", ")));
                // What wasn't in it goes along, so the setup can say which files to look for.
                return Err(Error::Msg(format!("NEED_COPY|{}|{}", main.label, missing.join(", "))));
            }
            // Its other files that go in a folder of its own (its update, say): put there too.
            place_others(p, &g)?;
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
        let no_files = BTreeMap::new();
        let paths = prof.as_ref().map_or(&no_files, |p| &p.input_paths);
        // A Windows program under Proton is given its files as Windows paths, as start.args are.
        let wine = !cfg!(windows) && exe.extension().is_some_and(|x| x.eq_ignore_ascii_case("exe"));
        games::add_args(&mut args, &pk.game, paths, wine);
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
    // A Python game: Python has to be here before anything is put in place for this start.
    let python = if exe.extension().is_some_and(|e| e.eq_ignore_ascii_case("py")) {
        Some(python_here().ok_or_else(|| Error::Msg(format!("NEED_PYTHON|{shown}")))?)
    } else {
        None
    };
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
    // On Linux every game plays from a plain folder (its AppImage was unpacked when it came in, or
    // at the top of this start): an AppImage is never run, even one picked for a single start.
    if cfg!(target_os = "linux") && ext == "appimage" {
        trail::note(format!("Play {id}: {} is an AppImage, which the launcher doesn't run", exe.display()));
        return Err(Error::Msg(APPIMAGE_NOT_RUN.into()));
    }
    let mut cmd = match ext.as_str() {
        // Windows' py launcher, else a python found on the PATH (found above).
        "py" if python.is_some() => {
            let (py, args) = python.clone().unwrap_or_default();
            let mut c = tokio::process::Command::new(py);
            c.args(args).arg(&exe);
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
    // The one folder a game writes everything to ({data}), for a game whose developer wrote it a
    // recomplauncher.json. Easy Setup skipped too: where its saves go mustn't change with how it
    // was set up.
    if let Some(data) = profile_of(&st, id, &g).filter(|p| !p.from_boot).map(|_| profile::data_dir(&g.path)) {
        let _ = std::fs::create_dir_all(&data);
        let shown = data.to_string_lossy().to_string();
        let through_proton = !cfg!(windows) && matches!(ext.as_str(), "exe" | "cmd" | "bat");
        cmd.env("RECOMPLAUNCHER_DATA", if through_proton { profile::wine_path(&shown) } else { shown });
    }
    // The PC is older than the game says it needs: said, so a game that won't start isn't a mystery,
    // but it still starts (the developer may be wrong, or the PC may manage).
    let too_old = profile_of(&st, id, &g).and_then(|p| system_needs(&p, &g)).map(|(_, old)| old).unwrap_or_default();
    if !too_old.is_empty() {
        trail::note(format!("Play {id}: this PC is older than the game needs: {too_old:?}"));
        let _ = app.emit("system-old", json!({ "name": g.name, "old": too_old }));
    }
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
    // A program in an unpacked AppImage: what its runtime would have set (APPDIR, OWD, ...).
    #[cfg(target_os = "linux")]
    appimage_env(&mut cmd, &exe, &g.path);
    // Recomp Achievements (test accounts): the game's channel, named in RL_SESSION.
    let mut ach_id = full_id.clone();
    let ach_session = {
        let cat = cached_catalog(&st);
        let mut entry = catalog::app(&cat, id);
        // A copy the player added of a listed game (a fork, another repo of the same developer):
        // its achievements go by the listing's id.
        if entry.is_none() && pick.is_none() {
            let disc = || crate::xex::table_for(&g.path, &[g.path.clone()], &[]).map(|t| t.title_id).filter(|t| !t.is_empty());
            if let Some((lid, how)) = listing_for_copy(&cat, &g.repo, disc) {
                trail::note(format!("Achievements {id}: a copy of the listing {lid} (the same {how}), so its achievements go by {lid}"));
                ach::set_alias(&st.dir, id, &lid);
                entry = catalog::app(&cat, &lid);
                ach_id = lid;
            }
        }
        let origin = entry.map(|e| str_of(e, "origin").to_string()).unwrap_or_default();
        let folder = entry.and_then(|e| e.get("achievements_folder").and_then(Value::as_str).or_else(|| e.pointer("/achievements/folder").and_then(Value::as_str))).map(str::to_string);
        let mut roots = vec![g.path.clone()];
        if let Some(p) = g.path.parent() {
            roots.push(p.join("Game Copies").join(library::folder_name(id)));
        }
        let title = entry.and_then(catalog_title);
        // One game of several: its own list (by its full id), and only its own unlock file.
        match &pick {
            Some(pk) => {
                let title = Some(pk.game.title_id.clone()).filter(|t| !t.is_empty());
                ach::start(app, ach::Start { id: &full_id, play_id: id, name: &pk.game.name, version: &g.tag, install: &g.path, origin: &origin, prof: prof.as_ref(), catalog_folder: folder, title, roots })
            }
            None => ach::start(app, ach::Start { id: &ach_id, play_id: id, name: &g.name, version: &g.tag, install: &g.path, origin: &origin, prof: prof.as_ref(), catalog_folder: folder, title, roots }),
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
            ach::abort(app, &ach_id);
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
    #[cfg(windows)]
    {
        let tracked = child.raw_handle().is_some_and(|handle| win::track_game_process(id, handle));
        if !tracked {
            trail::note(format!("Play {id}: couldn't retain the launched process for game-window tracking"));
        }
        if let Some(monitor) = target_monitor.as_ref().filter(|_| tracked) {
            win::watch_game_window(
                id.to_string(),
                win::MonitorBounds { x: monitor.x, y: monitor.y, width: monitor.width, height: monitor.height },
            );
        }
    }
    #[cfg(not(windows))]
    let _ = target_monitor;
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
    let ach_game = ach_id.clone();
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
    // Its save may change from here: copied after the session, or at the next start if the
    // launcher closes before that.
    cloud::set_after_play(&st.dir, &save_id, true);
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
        // From the game closing to its save copied, the status above Play says so, not an "Up to
        // date" the session may have made untrue. Said before the window hears the game stopped.
        let after_step = (secs >= 5).then(|| cloud::after_play_step(&app2, &save_id));
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
            #[cfg(windows)]
            win::forget_game_process(&id);
            ach::end(&app2, &ach_game);
            if let Some(m) = PLAYING_GAME.lock().unwrap().as_mut() {
                m.remove(&id);
            }
        }
        pads::PLAYING.store(!st.running.lock().unwrap().is_empty(), Ordering::Relaxed);
        if !st.running.lock().unwrap().contains_key(&id) {
            let _ = std::fs::remove_file(playing_file(&st.dir, &id));
            // Under five seconds there's no copy after it (below): nothing left to finish.
            if secs < 5 {
                cloud::set_after_play(&st.dir, &save_id, false);
            }
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
            // Minimized for the game, by the player's setting or because its web engine died
            // during play (see remake_main): open again now.
            let remade = webview_crash::take_minimized_for_play();
            if (from_bp && mine) || remade {
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
        drop(after_step);
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
static SETUP_STATUS: Mutex<Option<HashMap<String, SetupEv>>> = Mutex::new(None);

/// A setup is going for `id`. For a recomp's id that counts a run kept under one of its games'
/// ids too: every game of a recomp is set up in the same install, so two at once would each write
/// the record back and the last one would drop the other's picks.
pub(crate) fn setup_busy(id: &str) -> bool {
    let busy = |run: &str| run == id || games::split(run).is_some_and(|(recomp, _)| recomp == id);
    let running = SETUP_RUN.lock().unwrap().as_ref().is_some_and(|m| m.keys().any(|k| busy(k)));
    running || INPUT_WORKERS.lock().unwrap().iter().any(|x| busy(x))
}

fn setup_stopped(id: &str, run: u64) -> bool {
    SETUP_RUN.lock().unwrap().as_ref().and_then(|m| m.get(id).copied()) != Some(run)
}

fn setup_track(ev: &SetupEv) {
    if ev.step == "done" || ev.step == "error" {
        if let Some(m) = SETUP_STATUS.lock().unwrap().as_mut() {
            m.remove(&ev.id);
        }
    } else {
        SETUP_STATUS.lock().unwrap().get_or_insert_with(HashMap::new).insert(ev.id.clone(), ev.clone());
    }
}

/// Kills an entire process tree, terminating any compiler or script descendants.
fn kill_process_tree(child: &mut std::process::Child) {
    let pid = child.id();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut cmd = std::process::Command::new("taskkill");
        cmd.args(["/F", "/T", "/PID", &pid.to_string()]);
        cmd.creation_flags(0x0800_0000);
        let _ = cmd.output();
    }
    // Its own process group (run_prepare starts it in one), so everything it started goes too.
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("kill").args(["-9", "--", &format!("-{pid}")]).output();
    }
    let _ = child.kill();
    let _ = child.wait();
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
    /// While several files are read whole (a game on discs): the one it's on, by its label
    /// ("Disc 2"), and that file's name. `pct` is then for all of them together.
    #[serde(skip_serializing_if = "String::is_empty")]
    part: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    file: String,
}

/// Easy Setup's one run, for every game: each file with known good dumps is read whole (the
/// verified badge; any other was checked when it was picked), a disc for a recomp that reads the
/// disc's files is unpacked, the files with a place in the game's folder go there, the recomp's own
/// setup step runs, and its settings are written. Only once all of that worked are the picks and
/// the release it finished on kept: a setup that fails or is stopped leaves the game as it was.
/// Progress comes back as `setup` events.
///
/// `path`: the main file picked on the setup screen (its box, or dropped there), checked as any
/// pick is. Empty: the one picked before, or, when that's gone, the setup step runs again from
/// what it kept (prepare.keeps_copy).
#[tauri::command(async)]
fn setup_copy(app: AppHandle, st: State<'_, App>, id: String, path: String) -> Res<()> {
    setup_run(app, &st, id, Some(path))
}

/// The same run, from the setup screen of a game with more files than its copy.
#[tauri::command(async)]
fn setup_inputs_run(app: AppHandle, st: State<'_, App>, id: String, copy: Option<String>) -> Res<()> {
    setup_run(app, &st, id, copy)
}

fn setup_run(app: AppHandle, st: &App, id: String, copy: Option<String>) -> Res<()> {
    // One game of several: only its files are set up, and the release it finished on is kept for
    // that game. `id` stays what the window asked for (its progress comes back under it); `rid` is
    // the install.
    let pick = game_of(st, &id);
    let rid = pick.as_ref().map(|p| p.recomp.clone()).unwrap_or_else(|| id.clone());
    let sub = pick.as_ref().map(|p| p.game.id.clone());
    if setup_busy(&id) || setup_busy(&rid) || st.running.lock().unwrap().contains_key(&rid) {
        return Err(Error::Msg("Close the game or stop setup first.".into()));
    }
    let mut g = with_pending(&rid, in_plain_folder(st, &rid)?);
    let mut p = profile_of(st, &rid, &g).ok_or_else(|| Error::Msg("The launcher can't set this game up yet.".into()))?;
    if let Some(pk) = &pick {
        games::scope(&mut p, &pk.game, &pk.all);
    }
    inputs::validate(&p).map_err(Error::Msg)?;
    let main = p.main_input().cloned();
    if let (Some(m), Some(path)) = (main.as_ref(), copy.filter(|c| !c.is_empty())) {
        let taken = take_pick(st, &p, m, &g, PathBuf::from(path)).map_err(Error::Msg)?;
        g.setup_inputs.insert(m.id.clone(), taken);
    }
    // No main file now: built again from what its setup step kept, without it (the arguments
    // naming it are left out, which is why it has to be one argument: --game={game}).
    let main_gone = main.as_ref().is_some_and(|m| inputs::picked(&g, &m.id).is_none() && m.in_place(&g.path).is_none());
    let from_kept = main_gone && setup_state::builds_from_kept(&p, &g);
    let mut run_p = p.clone();
    if let Some(m) = main.as_ref().filter(|_| from_kept) {
        run_p.inputs.retain(|i| i.id != m.id);
        run_p.skipped_inputs.push(m.id.clone());
    }
    inputs::resolve(&run_p, &g, &st.dir).map_err(Error::Msg)?;
    if let Some(pr) = &p.prepare {
        required_ticked(&st.dir, pr, &g.prepare_opts)?;
        settle_prepare_opts(st, &rid, pr);
    }
    INPUT_WORKERS.lock().unwrap().push(id.clone());
    let run = SETUP_NEXT.fetch_add(1, Ordering::Relaxed);
    SETUP_RUN.lock().unwrap().get_or_insert_with(HashMap::new).insert(id.clone(), run);
    let dir = st.dir.clone();
    std::thread::spawn(move || {
        let stopped = || setup_stopped(&id, run);
        let extras = run_p.prepare.is_some() || run_p.inputs.iter().any(|i| !i.extract.is_empty());
        let emit = |ev: SetupEv| {
            let ev = SetupEv { id: id.clone(), extras, ..ev };
            setup_track(&ev);
            let _ = app.emit("setup", ev);
        };
        let job = SetupJob { app: &app, id: &id, rid: &rid, sub: sub.as_deref(), p: &run_p, from_kept, dir: &dir };
        let result = setup_work(&job, &mut g, &emit, &stopped);
        if !stopped() {
            match result {
                Ok((verified, text)) => emit(SetupEv { step: "done".into(), pct: 1.0, text, verified, ..Default::default() }),
                Err(why) => emit(SetupEv { step: "error".into(), text: why, ..Default::default() }),
            }
        }
        if let Some(m) = SETUP_RUN.lock().unwrap().as_mut() {
            if m.get(&id) == Some(&run) {
                m.remove(&id);
            }
        }
        if stopped() {
            if let Some(m) = SETUP_STATUS.lock().unwrap().as_mut() {
                m.remove(&id);
            }
        }
        INPUT_WORKERS.lock().unwrap().retain(|x| x != &id);
    });
    Ok(())
}

/// What one setup run works on.
struct SetupJob<'a> {
    app: &'a AppHandle,
    /// The game as the window knows it (its setup log is kept under this).
    id: &'a str,
    /// The install whose record it changes.
    rid: &'a str,
    /// One game of several: the one being set up.
    sub: Option<&'a str>,
    p: &'a profile::Profile,
    from_kept: bool,
    dir: &'a Path,
}

/// The run itself (setup_run). Ok: the known dump it matched, and what its last step says.
fn setup_work(job: &SetupJob, g: &mut Installed, emit: &dyn Fn(SetupEv), stopped: &dyn Fn() -> bool) -> Result<(String, String), String> {
    let (p, st) = (job.p, job.app.state::<App>());
    let kept_text = if job.from_kept { "What it kept from your copy" } else { "" };
    emit(SetupEv { step: "read".into(), text: kept_text.into(), ..Default::default() });
    // 1. Each file with known good dumps, read whole and compared with them. A game on several
    // discs: one bar for all of them, each line naming the disc it's on, and a result for every disc.
    let to_read: Vec<(&inputs::Input, PathBuf)> = p.inputs.iter().filter(|i| i.picked() && !i.dumps.is_empty()).filter_map(|i| inputs::picked(g, &i.id).filter(|x| x.is_file()).map(|x| (i, x))).collect();
    let all: u64 = to_read.iter().map(|(_, x)| std::fs::metadata(x).map(|m| m.len()).unwrap_or(0)).sum();
    let several = to_read.len() > 1;
    let mut before = 0u64;
    let mut results: Vec<(String, Option<String>)> = vec![];
    for (i, path) in &to_read {
        let part = ReadPart { label: if several { i.label.clone() } else { String::new() }, before, all };
        let (crc, total) = read_whole(&st, job.rid, i, path, g, &part, emit, stopped)?;
        before += total;
        let known = i.dumps.iter().find(|d| d.crc32.eq_ignore_ascii_case(&crc) && (d.size == 0 || d.size == total));
        results.push((i.label.clone(), known.map(|d| d.name.clone())));
    }
    let (verified, said) = dumps_said(&results);
    if stopped() {
        return Err("Stopped.".into());
    }
    // Right before the build is written over (a disc unpacked, the setup step started), and not
    // before: the game is marked as cut short (saved first) until all of it has worked, so a
    // Stop, the launcher closing or the PC going off partway leaves a game that builds again
    // instead of one that plays half a build. A setup that fails before then (no room, its
    // program missing) leaves the last build playable.
    let writing = || mark_cut(&st, job, true);
    let checked = if said.is_empty() { "Files selected.".to_string() } else { said.clone() };
    emit(SetupEv { step: "verify".into(), pct: 1.0, text: checked, verified: verified.clone(), ..Default::default() });
    // 2. A disc for a recomp that reads the disc's files: unpacked where it reads them.
    for i in p.inputs.iter().filter(|i| i.picked() && !i.extract.is_empty()) {
        let Some(image) = inputs::picked(g, &i.id).filter(|x| x.is_file()) else { continue };
        let dest = unpack_disc(i, &image, &g.path, emit, stopped, &writing)?;
        g.setup_inputs.insert(i.id.clone(), dest);
        g.unpacked_from.insert(i.id.clone(), image);
    }
    // 3. The files with a place in the game's folder. What was placed is kept even when what
    // follows fails, so trying again is safe.
    let placed = inputs::stage_cancellable(p, g, job.dir, stopped);
    if let Some(x) = st.lib.lock().unwrap().installed.get_mut(job.rid) {
        x.input_staged = g.input_staged.clone();
        x.input_stamps = g.input_stamps.clone();
    }
    st.save();
    placed?;
    place_others(p, g).map_err(|e| e.to_string())?;
    // 4. The recomp's own setup step, with every file it names.
    let copy = if job.from_kept { None } else { inputs::main_path(p, g) };
    if let Some(pr) = &p.prepare {
        emit(SetupEv { step: "extras".into(), text: pr.label.clone(), ..Default::default() });
        let pr = pr.with_inputs(&inputs::available(p, g, job.dir), pr.through_proton(&g.path));
        let extra = prepare_extra(job.dir, &pr, &g.prepare_opts);
        let handed = p.handed(copy.as_deref());
        let report = |pct, t| emit(SetupEv { step: "extras".into(), pct, text: t, ..Default::default() });
        let out = setup_output_file(job.dir, job.id);
        let done = run_prepare(&pr, &g.path, handed.as_deref(), &extra, &report, stopped, &writing, out.as_ref());
        // Built from what it kept and that didn't work: the player's copy builds it.
        done.map_err(|e| if job.from_kept { kept_failed(&e) } else { e })?;
    }
    if stopped() {
        return Err("Stopped.".into());
    }
    // 5. Its settings, then what worked is kept: the picks, and the release it finished on.
    emit(SetupEv { step: "setup".into(), ..Default::default() });
    note_pad();
    p.prepare(&g.path, copy.as_deref()).map_err(|e| format!("Couldn't write its settings: {e}"))?;
    let mut l = st.lib.lock().unwrap();
    let x = l.installed.get_mut(job.rid).ok_or("The game was removed during setup.")?;
    x.setup_inputs = g.setup_inputs.clone();
    x.unpacked_from = g.unpacked_from.clone();
    let tag = x.tag.clone();
    match job.sub {
        Some(s) => {
            let own = x.games.entry(s.to_string()).or_default();
            own.set_up = tag;
            own.build_cut = false;
        }
        None => {
            x.set_up = tag;
            x.build_cut = false;
        }
    }
    x.mode = library::SetupMode::Easy;
    drop(l);
    clear_pending(job.rid);
    st.save();
    let text = match (said.is_empty(), job.from_kept) {
        (false, _) => said,
        (true, true) => "Built again from what it kept from your copy".to_string(),
        (true, false) => "Your files are set up.".to_string(),
    };
    Ok((verified, text))
}

/// Marks the game's build as cut short (or not), saved straight away.
fn mark_cut(st: &App, job: &SetupJob, cut: bool) {
    if let Some(x) = st.lib.lock().unwrap().installed.get_mut(job.rid) {
        match job.sub {
            Some(s) => x.games.entry(s.to_string()).or_default().build_cut = cut,
            None => x.build_cut = cut,
        }
    }
    st.save();
}

/// Where one file is among several read whole: its label ("Disc 2", empty when it's the only
/// one), and the bytes read before it out of all of them, so the bar fills once.
struct ReadPart {
    label: String,
    before: u64,
    all: u64,
}

/// What the check of the known dumps says: the dump all of them matched ("USA", else empty), and
/// the line the setup shows. `results`: each file's label and the dump it matched, in order.
fn dumps_said(results: &[(String, Option<String>)]) -> (String, String) {
    let Some(name) = results.iter().find_map(|(_, m)| m.clone()) else {
        let said = match results.len() {
            0 => String::new(),
            1 => "Read it all, no errors. It isn't a known clean dump, but it may still work.".into(),
            n => format!("Read all {n}, no errors. None is a known clean dump, but they may still work."),
        };
        return (String::new(), said);
    };
    if results.len() == 1 {
        return (name.clone(), format!("Matches the verified {name} disc"));
    }
    let off: Vec<&str> = results.iter().filter(|(_, m)| m.is_none()).map(|(l, _)| l.as_str()).collect();
    if off.is_empty() {
        return (name.clone(), format!("All {} discs match the verified {name} dumps", results.len()));
    }
    let on: Vec<&str> = results.iter().filter(|(_, m)| m.is_some()).map(|(l, _)| l.as_str()).collect();
    let bad = if off.len() == 1 { format!("{} isn't a known clean dump.", discs_named(&off)) } else { format!("{} aren't known clean dumps.", discs_named(&off)) };
    let good = if on.len() == 1 { format!("{} matches the {name} dump.", discs_named(&on)) } else { format!("{} match the {name} dumps.", discs_named(&on)) };
    (String::new(), format!("{bad} {good}"))
}

/// Files named for a line: "Disc 1", "Discs 2–4", "Discs 1 and 3" when they're all "Disc N";
/// else their labels ("Disc A, Disc B and the update").
fn discs_named(labels: &[&str]) -> String {
    let numbers: Vec<u32> = labels.iter().filter_map(|l| l.strip_prefix("Disc ").and_then(|n| n.trim().parse().ok())).collect();
    if numbers.len() != labels.len() {
        return and_list(&labels.iter().map(|l| l.to_string()).collect::<Vec<_>>());
    }
    if numbers.len() == 1 {
        return format!("Disc {}", numbers[0]);
    }
    // Runs of discs in a row read as a range: 2, 3, 4 is "2–4".
    let mut runs: Vec<String> = vec![];
    let mut at = 0;
    while at < numbers.len() {
        let mut end = at;
        while end + 1 < numbers.len() && numbers[end + 1] == numbers[end] + 1 {
            end += 1;
        }
        // Three or more in a row are a range; one or two are listed on their own.
        if end - at >= 2 {
            runs.push(format!("{}–{}", numbers[at], numbers[end]));
        } else {
            runs.extend(numbers[at..=end].iter().map(|n| n.to_string()));
        }
        at = end + 1;
    }
    format!("Discs {}", and_list(&runs))
}

/// "a", "a and b", "a, b and c".
fn and_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// A file read whole to check it's all there, its CRC32 for the known dumps: the one read before,
/// unchanged since (Set Up Again for a new option, say), isn't read again. What was read is kept
/// straight away: it's a fact about the file, not a choice a failed setup takes back.
fn read_whole(st: &App, rid: &str, i: &inputs::Input, path: &Path, g: &Installed, part: &ReadPart, emit: &dyn Fn(SetupEv), stopped: &dyn Fn() -> bool) -> Result<(String, u64), String> {
    let total = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let file = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    // Which disc couldn't be read, when there are several.
    let which = if part.label.is_empty() { String::new() } else { format!("{}: ", part.label) };
    if let Some(crc) = g.input_checks.get(&i.id).and_then(|c| c.still(path)) {
        trail::note(format!("Set up {rid}: {} is the one read whole before, unchanged; not read again", i.id));
        return Ok((crc.to_string(), total));
    }
    let mut f = std::fs::File::open(path).map_err(|_| format!("{which}Couldn't open it."))?;
    let mut h = crc32fast::Hasher::new();
    let mut buf = vec![0u8; 4 << 20];
    let (mut done, mut last) = (0u64, Instant::now());
    loop {
        if stopped() {
            return Err("Stopped.".into());
        }
        let n = match std::io::Read::read(&mut f, &mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => return Err(format!("{which}Part of it couldn't be read ({e}). The copy may be damaged.")),
        };
        h.update(&buf[..n]);
        done += n as u64;
        if last.elapsed().as_millis() >= 80 {
            last = Instant::now();
            // The bar is for every file read whole; done and total are this one's.
            let pct = (part.before + done) as f32 / part.all.max(total).max(1) as f32;
            emit(SetupEv { step: "verify".into(), pct, done, total, part: part.label.clone(), file: file.clone(), ..Default::default() });
        }
    }
    let crc = format!("{:08x}", h.finalize());
    remember_check(st, rid, &i.id, path, &crc);
    Ok((crc, total))
}

fn remember_check(st: &App, rid: &str, input: &str, path: &Path, crc32: &str) {
    let Some((size, modified)) = library::CopyCheck::stamp(path) else { return };
    let check = library::CopyCheck { path: path.to_path_buf(), size, modified, crc32: crc32.to_string() };
    if let Some(x) = st.lib.lock().unwrap().installed.get_mut(rid) {
        x.input_checks.insert(input.to_string(), check);
    }
    st.save();
}

/// A disc image unpacked for a recomp that reads the disc's files (`extract`): into its `into`
/// folder (or game-files), checked for the file it must have. Gives back that folder. `writing`
/// is called once its checks pass, right before it writes.
fn unpack_disc(i: &inputs::Input, image: &Path, install: &Path, emit: &dyn Fn(SetupEv), stopped: &dyn Fn() -> bool, writing: &dyn Fn()) -> Result<PathBuf, String> {
    let into = i.into.trim().trim_matches(['/', '\\']).to_string();
    let dest = if into == "." {
        install.to_path_buf()
    } else if !into.is_empty() && !into.contains("..") {
        install.join(&into)
    } else {
        install.join("game-files")
    };
    let say = "Unpacking your disc";
    emit(SetupEv { step: "extras".into(), text: say.into(), ..Default::default() });
    let need = extract::size(image, &i.extract).map_err(|e| format!("Couldn't read the disc image: {e}"))?;
    if let Some(free) = manage::free_space(install) {
        if free < need + (256 << 20) {
            return Err(format!("Unpacking it needs {} free on that drive ({} free).", human_size(need), human_size(free)));
        }
    }
    let progress = |d: u64, t: u64| emit(SetupEv { step: "extras".into(), pct: d as f32 / t.max(1) as f32, done: d, total: t, text: say.into(), ..Default::default() });
    writing();
    if let Err(e) = extract::unpack(image, &i.extract, &dest, &progress, stopped) {
        return Err(if stopped() { "Stopped.".into() } else { format!("Couldn't unpack the disc: {e}") });
    }
    let want = i.check.file.trim();
    // Any case: discs name it default.xex or Default.xex, and Linux tells them apart.
    let has = want.is_empty() || dest.join(want).is_file() || std::fs::read_dir(&dest).into_iter().flatten().flatten().any(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(want));
    if !has {
        return Err(format!("The disc unpacked, but it doesn't have {want}. It may be a different game."));
    }
    Ok(dest)
}

/// The files other than the main one that go in a folder of the game's (`into`: its update, a
/// disc it reads from there): put there, as Play does before each start. The main one is placed by
/// Profile::place, with the copy's own rules (a ROM's byte order, a sheet's tracks).
fn place_others(p: &profile::Profile, g: &Installed) -> Res<()> {
    let main = p.main_input().map(|i| i.id.as_str());
    let others = p.inputs.iter().filter(|i| i.picked() && Some(i.id.as_str()) != main && !i.into.trim().is_empty() && i.extract.is_empty());
    for i in others {
        let Some(path) = inputs::picked(g, &i.id) else { continue };
        let what = if i.role == "update" { "the update".to_string() } else { i.label.clone() };
        i.as_extra().place(&g.path, &path).map_err(|e| Error::Msg(format!("Couldn't put {what} where the game reads it: {e}")))?;
    }
    Ok(())
}

#[tauri::command(async)]
fn setup_cancel(id: String) {
    if let Some(m) = SETUP_RUN.lock().unwrap().as_mut() {
        m.remove(&id);
    }
    if let Some(m) = SETUP_STATUS.lock().unwrap().as_mut() {
        m.remove(&id);
    }
    // INPUT_WORKERS is left alone: a cancelled worker may still be finishing a file copy, and
    // holds its lock until it has actually stopped.
}

/// The launcher is closing: every setup stops, its setup step and what that started with it.
/// Left running, a builder goes on writing the game's files with no launcher to see it end, and
/// can still be at it when the next build of the same game starts. Waits a moment (a step is
/// checked on every 60 ms) for them to be gone.
fn stop_setups() {
    if let Some(m) = SETUP_RUN.lock().unwrap().as_mut() {
        m.clear();
    }
    let until = Instant::now() + std::time::Duration::from_secs(3);
    while !INPUT_WORKERS.lock().unwrap().is_empty() && Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
}

#[tauri::command]
fn setup_status() -> HashMap<String, SetupEv> {
    SETUP_STATUS.lock().unwrap().as_ref().cloned().unwrap_or_default()
}

/// Files picked in an Easy Setup window, by the game whose record they change (the recomp, for one
/// game of several): kept here, not in the library, until a setup with them works (setup_work
/// writes them). A window that closes without one, a webview that's gone, or the launcher closing
/// leaves the game set up as it was. None: the file was cleared in the window.
static PENDING: Mutex<Option<HashMap<String, BTreeMap<String, Option<PathBuf>>>>> = Mutex::new(None);

/// The game's record with the files its open Easy Setup window picked on top.
fn with_pending(rid: &str, mut g: Installed) -> Installed {
    let all = PENDING.lock().unwrap();
    let Some(picks) = all.as_ref().and_then(|m| m.get(rid)) else { return g };
    for (k, v) in picks {
        match v {
            Some(path) => g.setup_inputs.insert(k.clone(), path.clone()),
            None => g.setup_inputs.remove(k),
        };
    }
    g
}

fn pend(rid: &str, input: &str, path: Option<PathBuf>) {
    let mut all = PENDING.lock().unwrap();
    all.get_or_insert_with(HashMap::new).entry(rid.to_string()).or_default().insert(input.to_string(), path);
}

fn clear_pending(rid: &str) {
    if let Some(m) = PENDING.lock().unwrap().as_mut() {
        m.remove(rid);
    }
}

/// The game whose library record a setup changes: the recomp, for one game of several.
fn setup_record_id(st: &App, id: &str) -> String {
    game_of(st, id).map(|p| p.recomp).unwrap_or_else(|| id.to_string())
}

/// Easy Setup's window opened: it starts from how the game is set up now.
#[tauri::command(async)]
fn setup_begin(st: State<'_, App>, id: String) {
    clear_pending(&setup_record_id(&st, &id));
}

/// Easy Setup's window closed: what was picked in it and not set up goes. A run still going keeps
/// its own picks, and keeps them only if it works.
#[tauri::command(async)]
fn setup_end(st: State<'_, App>, id: String) {
    clear_pending(&setup_record_id(&st, &id));
}

fn human_size(b: u64) -> String {
    let gb = b as f64 / 1_073_741_824.0;
    if gb >= 1.0 {
        format!("{gb:.1} GB")
    } else {
        format!("{:.0} MB", b as f64 / 1_048_576.0)
    }
}

/// A program in an AppImage unpacked at install gets what the AppImage's runtime would have set
/// (see `appimage_vars`).
#[cfg(target_os = "linux")]
fn appimage_env(cmd: &mut tokio::process::Command, exe: &Path, install: &Path) {
    for (k, v) in appimage_vars(exe, install, sysenv::for_games("LD_LIBRARY_PATH")) {
        cmd.env(k, v);
    }
}

/// The unpacked AppImage a program is in: the nearest folder above it, inside the game's folder,
/// that has an AppRun.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn appimage_root(exe: &Path, install: &Path) -> Option<PathBuf> {
    let mut folders = exe.ancestors().skip(1).take_while(|d| d.starts_with(install));
    folders.find(|d| d.join("AppRun").is_file()).map(Path::to_path_buf)
}

/// What the AppImage runtime sets for the program it starts, for a program in an unpacked
/// AppImage: APPDIR (the AppImage's folder, where programs look for their own files), OWD (the
/// folder it starts in) and, for its AppRun, ARGV0. A program named by itself instead of its
/// AppRun also gets the AppImage's libraries (`appimage_libs`). Nothing for a program in a plain
/// folder. APPIMAGE is never set: there's no AppImage file anymore.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn appimage_vars(exe: &Path, install: &Path, pc_libs: Option<std::ffi::OsString>) -> Vec<(&'static str, std::ffi::OsString)> {
    let apprun = exe.file_name().is_some_and(|n| n == "AppRun");
    let root = if apprun { exe.parent().map(Path::to_path_buf) } else { appimage_root(exe, install) };
    let Some(root) = root else { return vec![] };
    let starts_in = exe.parent().unwrap_or(&root).as_os_str().to_os_string();
    let mut vars = vec![("APPDIR", root.into_os_string()), ("OWD", starts_in)];
    if apprun {
        vars.push(("ARGV0", exe.as_os_str().to_os_string()));
    }
    if let Some(libs) = appimage_libs(exe, install, pc_libs) {
        vars.push(("LD_LIBRARY_PATH", libs));
    }
    vars
}

/// Where an AppImage keeps the libraries it brings, inside its folder.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const APPIMAGE_LIB_DIRS: [&str; 6] = ["lib", "lib64", "usr/lib", "usr/lib64", "usr/lib/x86_64-linux-gnu", "usr/lib/aarch64-linux-gnu"];

/// A program inside an unpacked AppImage, started by itself instead of through its AppRun (a
/// profile that names it, like mkw-recomp's `mkw`): the library folders the AppImage brings, which
/// its AppRun would have put first. Without them it stops at once ("libvpx.so.6: cannot open
/// shared object file", code 127). The PC's own list comes after them. None for its AppRun, which
/// sets its own, and for a program in a folder that isn't an AppImage's.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn appimage_libs(exe: &Path, install: &Path, pc: Option<std::ffi::OsString>) -> Option<std::ffi::OsString> {
    if exe.file_name().is_some_and(|n| n == "AppRun") {
        return None;
    }
    let root = appimage_root(exe, install)?;
    let mut dirs: Vec<PathBuf> = APPIMAGE_LIB_DIRS.iter().map(|d| root.join(d)).filter(|d| d.is_dir()).collect();
    if dirs.is_empty() {
        return None;
    }
    let pc = pc.unwrap_or_default();
    dirs.extend(std::env::split_paths(&pc).filter(|p| !p.as_os_str().is_empty()));
    std::env::join_paths(dirs).ok()
}

/// Run a recomp's prepare program, reading `progress 0.4 text` lines from it.
/// The arguments a game's setup step gets from the options ticked for it (prepare.options), as the
/// player last set them for that game.
fn prepare_extra(dir: &Path, pr: &profile::Prepare, picked: &BTreeMap<String, bool>) -> Vec<String> {
    if pr.options.is_empty() {
        return vec![];
    }
    let (rtx, amd) = dlss::gpu_brands(dir);
    pr.option_args(assets::os_now(), rtx, amd, picked)
}

/// The options a game's setup step offers on this PC, with how each is set for it now.
fn prepare_options_here(dir: &Path, pr: &profile::Prepare, picked: &BTreeMap<String, bool>) -> Vec<Value> {
    let (rtx, amd) = dlss::gpu_brands(dir);
    pr.options_here(assets::os_now(), rtx, amd)
        .into_iter()
        .map(|o| serde_json::json!({ "id": o.id, "label": o.label, "note": o.note, "license": o.license.as_ref().map(|l| serde_json::json!({ "name": l.name, "url": l.url })), "on": o.on(picked), "new": !picked.contains_key(&o.id), "required": o.required }))
        .collect()
}

/// The setup step doesn't start while a required option this PC is offered is unticked. The
/// screen holds its button too; this is for anything that asks without it.
fn required_ticked(dir: &Path, pr: &profile::Prepare, picked: &BTreeMap<String, bool>) -> Res<()> {
    if !pr.options.iter().any(|o| o.required) {
        return Ok(());
    }
    let (rtx, amd) = dlss::gpu_brands(dir);
    let missing = pr.unticked_required(assets::os_now(), rtx, amd, picked);
    let Some(first) = missing.first() else { return Ok(()) };
    Err(Error::Msg(format!("Turn on \"{}\" on the setup screen to set this game up.", first.label)))
}

/// Starting a setup answers every option its screen showed: one the player left as it was counts
/// as answered too, so it isn't offered again as new after the next update.
fn settle_prepare_opts(st: &App, id: &str, pr: &profile::Prepare) {
    let (rtx, amd) = dlss::gpu_brands(&st.dir);
    let mut l = st.lib.lock().unwrap();
    let Some(g) = l.installed.get_mut(id) else { return };
    let answers: Vec<(String, bool)> = pr.unanswered(assets::os_now(), rtx, amd, &g.prepare_opts).into_iter().map(|o| (o.id.clone(), o.on(&g.prepare_opts))).collect();
    if answers.is_empty() {
        return;
    }
    g.prepare_opts.extend(answers);
    drop(l);
    st.save();
}

/// The setup step options this PC is offered that the player hasn't answered yet, on a game that's
/// already set up: what its project added after the player's setup (NVIDIA DLSS in an update).
fn new_prepare_options(st: &App, id: &str) -> Vec<String> {
    // Skip Easy Setup: the player sets this game up their own way, so nothing is asked.
    if player_sets_up(&st, id) {
        return vec![];
    }
    let Some(g) = st.lib.lock().unwrap().installed.get(id).cloned() else { return vec![] };
    let Some(p) = profile_of(st, id, &g) else { return vec![] };
    let Some(pr) = p.prepare.as_ref().filter(|pr| !pr.options.is_empty()) else { return vec![] };
    // Set up before (on any release): an option an update added is asked about, even with the
    // copy gone since (prepare.keeps_copy builds again from what it kept).
    let set_up = !g.set_up.is_empty() || g.games.values().any(|x| !x.set_up.is_empty()) || inputs::main_path(&p, &g).is_some();
    if !set_up {
        return vec![];
    }
    let (rtx, amd) = dlss::gpu_brands(&st.dir);
    pr.unanswered(assets::os_now(), rtx, amd, &g.prepare_opts).into_iter().map(|o| o.id.clone()).collect()
}

// ---------- plain copies of compressed images ----------

static CONVERT_STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Clone, Serialize)]
struct ConvertEv {
    done: u64,
    total: u64,
}

/// Where the plain copy of a compressed image would go, and how big it gets.
#[tauri::command(async)]
fn convert_plan(path: String) -> Res<Value> {
    let (dest, size) = convert::plan(Path::new(&path)).map_err(Error::Msg)?;
    let free = manage::free_space(dest.parent().unwrap_or(Path::new("."))).unwrap_or(u64::MAX);
    Ok(json!({ "dest": dest, "size": size, "free": free, "exists": dest.exists() }))
}

/// Make a plain copy of a compressed image beside it (convert.rs), with `convert` events as it
/// goes. Returns the file to pick.
#[tauri::command(async)]
async fn convert_copy(app: AppHandle, path: String) -> Res<String> {
    CONVERT_STOP.store(false, Ordering::Relaxed);
    trail::note(format!("Convert: {path}"));
    let a = app.clone();
    let out = tokio::task::spawn_blocking(move || {
        let last = std::sync::Mutex::new(Instant::now() - std::time::Duration::from_secs(1));
        convert::convert(
            Path::new(&path),
            &|done, total| {
                let mut l = last.lock().unwrap();
                if done >= total || l.elapsed().as_millis() >= 150 {
                    *l = Instant::now();
                    let _ = a.emit("convert", ConvertEv { done, total });
                }
            },
            &|| CONVERT_STOP.load(Ordering::Relaxed),
        )
    })
    .await
    .map_err(|e| Error::Msg(e.to_string()))?
    .map_err(Error::Msg)?;
    trail::note(format!("Convert: done, {}", out.display()));
    Ok(out.to_string_lossy().into_owned())
}

#[tauri::command]
fn convert_cancel() {
    CONVERT_STOP.store(true, Ordering::Relaxed);
}

/// The ticks the player set for a game's setup step options, kept for its next setup or rebuild.
/// Added to the ones kept: an answer to one new option (Not now) leaves the others as they were.
#[tauri::command(async)]
fn set_prepare_opts(st: State<'_, App>, id: String, opts: BTreeMap<String, bool>) -> Res<()> {
    let ok = |k: &str| !k.is_empty() && k.len() <= 40 && k.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    let mut l = st.lib.lock().unwrap();
    let g = l.installed.get_mut(&id).ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    g.prepare_opts.extend(opts.into_iter().filter(|(k, _)| ok(k)));
    drop(l);
    st.save();
    Ok(())
}

/// A build from what the setup step kept that didn't work: its first line, then that the player's
/// copy builds it instead, then what the step printed, on lines of their own as before.
fn kept_failed(said: &str) -> String {
    let (head, printed) = said.split_once('\n').unwrap_or((said, ""));
    let head = head.trim_end();
    let stop = if head.ends_with(['.', '!', '?']) { "" } else { "." };
    let line = format!("{head}{stop} Choose your copy of the game to build it from that.");
    if printed.is_empty() {
        return line;
    }
    format!("{line}\n{printed}")
}

/// What the setup step is run with: its own arguments, then those of the options the player
/// ticked. Without the copy (run again from what it kept, prepare.keeps_copy), every argument
/// that names it ({game}) is left out, which is why it has to be one argument (--game={game}).
fn prepare_args(pr: &profile::Prepare, install: &Path, copy: Option<&Path>, extra: &[String]) -> Vec<String> {
    let all = pr.args.iter().chain(extra.iter());
    let named = all.filter(|a| copy.is_some() || !a.contains("{game}"));
    named.map(|a| profile::fill(a, install, copy)).collect()
}

#[cfg(target_os = "linux")]
extern "C" {
    // Variadic in C, so declared as it is there.
    fn prctl(option: std::ffi::c_int, ...) -> std::ffi::c_int;
    fn getppid() -> std::ffi::c_int;
}

/// The setup step is sent SIGKILL when the thread that started it ends, the launcher killed
/// with it. (That thread waits for the step, so it never ends first otherwise.) prctl and
/// getppid are async-signal-safe, as pre_exec requires.
#[cfg(target_os = "linux")]
fn die_with_launcher(c: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;
    const PR_SET_PDEATHSIG: std::ffi::c_int = 1;
    const SIGKILL: std::ffi::c_ulong = 9;
    let launcher = std::process::id() as std::ffi::c_int;
    unsafe {
        c.pre_exec(move || {
            prctl(PR_SET_PDEATHSIG, SIGKILL);
            // The launcher already gone between the fork and here (the signal only comes for a
            // death after it's asked for): the step doesn't start. Its parent is then init (1),
            // or the session's reaper (systemd, Flatpak, a container): either way not the launcher.
            if getppid() != launcher {
                return Err(std::io::Error::other("the launcher is gone"));
            }
            Ok(())
        });
    }
}

/// A process put in a job of its own that Windows ends, with what it started, when the job's
/// last handle closes without being let go: when the launcher is killed or crashes and Windows
/// closes its handles. Dropped while the launcher runs (the step ended or was stopped), the job
/// is let go first, so what the step left running on purpose (a runtime installer it started
/// without waiting, a helper) goes on as it did before.
#[cfg(windows)]
mod win_job {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE};

    pub struct Job(HANDLE);

    impl Drop for Job {
        fn drop(&mut self) {
            unsafe {
                let _ = set_limits(self.0, JOB_OBJECT_LIMIT(0));
                let _ = CloseHandle(self.0);
            }
        }
    }

    unsafe fn set_limits(job: HANDLE, flags: JOB_OBJECT_LIMIT) -> windows::core::Result<()> {
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = flags;
        let size = std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32;
        SetInformationJobObject(job, JobObjectExtendedLimitInformation, &info as *const _ as *const std::ffi::c_void, size)
    }

    /// None when Windows won't make the job: the process then runs as it did before, outside one.
    pub fn kill_on_close(child: &std::process::Child) -> Option<Job> {
        use std::os::windows::io::AsRawHandle;
        unsafe {
            let job = Job(CreateJobObjectW(None, windows::core::PCWSTR::null()).ok()?);
            set_limits(job.0, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE).ok()?;
            AssignProcessToJobObject(job.0, HANDLE(child.as_raw_handle())).ok()?;
            Some(job)
        }
    }

    #[cfg(test)]
    mod tests {
        fn ping() -> std::process::Child {
            std::process::Command::new("cmd").args(["/c", "ping -n 30 127.0.0.1 > nul"]).spawn().unwrap()
        }

        fn ended(step: &mut std::process::Child) -> bool {
            let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while step.try_wait().unwrap().is_none() && std::time::Instant::now() < until {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            step.try_wait().unwrap().is_some()
        }

        /// The launcher gone (Windows closes its handle to the job, nothing let go): the step ends.
        #[test]
        fn a_step_ends_when_the_launcher_is_gone() {
            let mut step = ping();
            let job = super::kill_on_close(&step).expect("a job");
            std::thread::sleep(std::time::Duration::from_millis(300));
            assert!(step.try_wait().unwrap().is_none(), "still running while the job is held");
            let handle = job.0;
            std::mem::forget(job);
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(handle);
            }
            assert!(ended(&mut step), "ended with the launcher");
        }

        /// The job dropped while the launcher runs: what's still in it goes on.
        #[test]
        fn a_dropped_job_leaves_what_runs_in_it() {
            let mut step = ping();
            let job = super::kill_on_close(&step).expect("a job");
            drop(job);
            std::thread::sleep(std::time::Duration::from_millis(500));
            assert!(step.try_wait().unwrap().is_none(), "still running after the drop");
            // The whole tree, ping too: it's out of the job now, so nothing else would end it.
            crate::kill_process_tree(&mut step);
        }
    }
}

/// How long a setup step may print nothing before it's given up on.
const SILENT_FOR: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// How many of its last lines a setup step that failed shows the player: enough for a reason that
/// runs over a few lines (an install hint, the top of a stack trace), never the whole output.
const SAID_LINES: usize = 10;

/// What a setup step printed last, progress lines left out: its last SAID_LINES lines, each as
/// a terminal would show it.
#[derive(Default)]
struct LastSaid(std::collections::VecDeque<String>);

impl LastSaid {
    fn push(&mut self, line: &str) {
        let line = plain_line(line);
        if line.is_empty() {
            return;
        }
        if self.0.len() == SAID_LINES {
            self.0.pop_front();
        }
        self.0.push_back(line);
    }

    fn text(&self) -> String {
        Vec::from(self.0.clone()).join("\n")
    }
}

/// A line a program printed, as a terminal shows it: colour codes taken out, and only what came
/// after its last carriage return (a progress bar drawn over itself). Long lines are cut.
fn plain_line(line: &str) -> String {
    let shown = line.rsplit('\r').next().unwrap_or(line);
    without_codes(shown).trim().chars().take(300).collect()
}

/// A line without its colour and cursor codes, which a log file would show as junk.
fn without_codes(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        // ESC [ ... then one of @ to ~: a colour or cursor code.
        if chars.next() != Some('[') {
            continue;
        }
        for c in chars.by_ref() {
            if ('@'..='~').contains(&c) {
                break;
            }
        }
    }
    out
}

/// A setup step that stopped with an error code: the launcher's line, then what it printed last
/// on lines of their own. The UI translates the first line and shows the rest as it was printed.
fn step_failed(code: &str, said: &str) -> String {
    let head = format!("Its setup step stopped with code {code}.");
    if said.is_empty() {
        return head;
    }
    format!("{head}\n{said}")
}

/// Start a program that may have only just been written (unpacked with the game). On Linux,
/// another thread starting a program at that moment holds the new file open for a few
/// milliseconds, and starting it fails with "Text file busy": it's tried again for up to half a
/// second.
fn spawn_just_written(c: &mut std::process::Command) -> std::io::Result<std::process::Child> {
    for _ in 0..20 {
        match c.spawn() {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => std::thread::sleep(std::time::Duration::from_millis(25)),
            started => return started,
        }
    }
    c.spawn()
}

/// Runs the recomp's setup step. `writing` is called once the step has started, when it can
/// write over the last build; a step that can't start (its program missing, no room, no Proton)
/// never calls it. `out` gets every line it prints (the full log the player can open or send).
fn run_prepare(pr: &profile::Prepare, install: &Path, copy: Option<&Path>, extra: &[String], report: &dyn Fn(f32, String), stopped: &dyn Fn() -> bool, writing: &dyn Fn(), out: Option<&std::fs::File>) -> Result<(), String> {
    use std::io::{BufRead, Write};
    let exe = pr.program_in(install).ok_or("Its setup program is missing. Reinstall the game.")?;
    // On Linux an AppImage is never run, its setup step included: Play unpacks the game first.
    if cfg!(target_os = "linux") && exe.extension().is_some_and(|x| x.eq_ignore_ascii_case("appimage")) {
        return Err(APPIMAGE_NOT_RUN.into());
    }
    if !setup_temp::room_for_setup(manage::free_space(install)) {
        return Err(setup_temp::NO_ROOM.into());
    }
    // A temp folder of its own on the game's drive, taken away however this returns.
    let temp = setup_temp::SetupTemp::make(install)?;
    let data = profile::data_dir(install);
    let _ = std::fs::create_dir_all(&data);
    // A Windows step on Linux (a Windows-only game's builder): through the same Proton the game
    // plays with, its paths given the way Windows programs read them there (Z:\home\...).
    let wine = pr.through_proton(install);
    let _w = profile::WineFill::new(wine);
    let as_given = |p: &Path| if wine { profile::wine_path(&p.to_string_lossy()) } else { p.to_string_lossy().to_string() };
    let args = prepare_args(pr, install, copy, extra);
    let mut c = if wine {
        let r = proton::runner().ok_or("This game's setup step is a Windows program, and it needs Proton to run on Linux. Settings → General → GE-Proton installs it in one press, then set the game up again.")?;
        let shared = proton::data_dir().ok_or("Couldn't find the launcher's Proton prefix.")?;
        proton::std_command(&r, &shared, &exe)
    } else {
        std::process::Command::new(&exe)
    };
    sysenv::clean_std(&mut c);
    // As for Play: the step can tell it was started from here (so it never updates itself), and
    // where to write.
    c.env("RECOMPLAUNCHER", "1").env("RECOMPLAUNCHER_VERSION", env!("CARGO_PKG_VERSION")).env("RECOMPLAUNCHER_DATA", as_given(&data));
    for (k, v) in pr.env_vars(install, copy) {
        c.env(k, v);
    }
    // After the step's own: the temp folder is the launcher's to give.
    for (k, v) in temp.env() {
        c.env(k, as_given(v));
    }
    // A setup step in an unpacked AppImage (its AppRun, or a program named inside it): what the
    // AppImage's runtime would have set.
    #[cfg(target_os = "linux")]
    for (k, v) in appimage_vars(&exe, install, sysenv::for_games("LD_LIBRARY_PATH")) {
        c.env(k, v);
    }
    c.args(&args).current_dir(exe.parent().unwrap_or(install)).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    // A group of its own: stopping setup stops the compilers and tools it started as well.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        c.process_group(0);
    }
    // The launcher killed or crashing (no Exit to stop it): on Linux the step goes with it.
    #[cfg(target_os = "linux")]
    die_with_launcher(&mut c);
    let mut child = spawn_just_written(&mut c).map_err(|e| if is_blocked(&e) { "Windows Security stopped its setup program.".to_string() } else { format!("Couldn't run its setup program: {e}") })?;
    // Started: from here it can write over the last build. (Not before: a step Windows Security
    // blocked never ran.)
    writing();
    // On Windows the same, through a job that ends the step (and what it started) when the
    // launcher is gone. Held until the step is.
    #[cfg(windows)]
    let _job = win_job::kill_on_close(&child);
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    if let Some(o) = child.stdout.take() {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for l in std::io::BufReader::new(o).lines().map_while(Result::ok) {
                let _ = tx.send(l);
            }
        });
    }
    if let Some(e) = child.stderr.take() {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for l in std::io::BufReader::new(e).lines().map_while(Result::ok) {
                let _ = tx.send(l);
            }
        });
    }
    drop(tx);
    // What it said last (not progress lines): when it stops on an error, that's usually why.
    let mut said = LastSaid::default();
    // Whether it said Windows' path limit stopped it, on any line: the reason is usually followed
    // by a stack trace.
    let mut too_long = false;
    // When it last printed anything: a step that goes quiet for SILENT_FOR has stopped working.
    let mut heard = Instant::now();
    let read = |said: &mut LastSaid, too_long: &mut bool, heard: &mut Instant| {
        while let Ok(l) = rx.try_recv() {
            *heard = Instant::now();
            // Every line, progress ones too: the full log is for whoever reads it to fix it.
            if let Some(mut f) = out {
                let _ = writeln!(f, "{}", without_codes(&l));
            }
            let mut w = l.splitn(3, ' ');
            if w.next() == Some("progress") {
                let pct = w.next().and_then(|x| x.parse::<f32>().ok()).unwrap_or(0.0).clamp(0.0, 1.0);
                report(pct, w.next().unwrap_or("").trim().to_string());
            } else if !l.trim().is_empty() {
                said.push(&l);
                *too_long |= setup_temp::says_too_long(&l);
            }
        }
    };
    loop {
        read(&mut said, &mut too_long, &mut heard);
        if let Ok(Some(s)) = child.try_wait() {
            if s.success() {
                return Ok(());
            }
            // The last of what it printed, which can arrive just after it closed.
            std::thread::sleep(std::time::Duration::from_millis(150));
            read(&mut said, &mut too_long, &mut heard);
            let code = s.code().map(|c| c.to_string()).unwrap_or_else(|| "a signal".into());
            let said = said.text();
            trail::note(format!("Setup step stopped with code {code}; it said last: {}", said.replace('\n', " | ")));
            if cfg!(windows) && too_long {
                return Err(setup_temp::too_long_with(&said));
            }
            return Err(step_failed(&code, &said));
        }
        // No time limit: a full build on a Steam Deck can take hours. Only a step that has printed
        // nothing for SILENT_FOR is given up on, and Cancel stops it any time.
        if stopped() || heard.elapsed() > SILENT_FOR {
            kill_process_tree(&mut child);
            return Err(if stopped() { "Stopped.".into() } else { "Its setup step stopped answering: it printed nothing for an hour.".into() });
        }
        std::thread::sleep(std::time::Duration::from_millis(60));
    }
}

/// Easy Setup off for new games (the Settings switch, stored as `easy_skip_all`). A game takes it as
/// its own mode when it's installed; games already installed keep theirs, whichever way it goes.
fn easy_skipped_for_all(ui: &Value) -> bool {
    ui.get("easy_skip_all").and_then(Value::as_bool).unwrap_or(false)
}

/// Skip Easy Setup ticked in the menu of a game that isn't installed yet (the list in Settings).
fn easy_skipped(ui: &Value, id: &str) -> bool {
    easy_skipped_for_all(ui) || ui.get("easy_skip").and_then(Value::as_array).is_some_and(|a| a.iter().any(|x| x.as_str() == Some(id)))
}

/// A Skip Easy Setup tick made before the game was installed, taken off the list: true when there
/// was one. The installed game keeps it as its own mode.
fn take_skip_choice(ui: &mut Value, id: &str) -> bool {
    let Some(list) = ui.get_mut("easy_skip").and_then(Value::as_array_mut) else { return false };
    let before = list.len();
    list.retain(|x| x.as_str() != Some(id));
    list.len() != before
}

/// A game being installed for the first time skips Easy Setup: its own tick (taken off the list),
/// or Easy Setup off for new games in Settings.
fn fresh_install_skips(ui: &mut Value, id: &str) -> bool {
    let ticked = take_skip_choice(ui, id);
    ticked || easy_skipped_for_all(ui)
}

/// The player sets this game up their own way (Skip Easy Setup): its own mode once installed, or
/// its tick or the Settings switch before. A game of several in one recomp follows its recomp.
pub(crate) fn player_sets_up(st: &App, id: &str) -> bool {
    let rid = setup_record_id(st, id);
    let l = st.lib.lock().unwrap();
    match l.installed.get(&rid) {
        Some(g) => g.player_sets_up(),
        None => easy_skipped(&l.settings.ui, id),
    }
}

/// Catalog games the launcher can set up itself (copy and settings): the "Easy setup" ones.
/// (Skipping it is the page's to apply: the game still can be set up this way.)
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
                    p.easy_setup().then(|| id.to_string())
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The player's own launch options and automatic updates for a game.
#[tauri::command(async)]
fn set_game_options(st: State<'_, App>, id: String, args: Option<String>, auto_update: Option<bool>, rebuild_after_update: Option<bool>) -> Res<Installed> {
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
        if let Some(r) = rebuild_after_update {
            g.rebuild_after_update = r;
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
    catalog::with_mine(&st.dir, catalog_on_disk(&st.dir.join("cache/pc-catalog.json")))
}

/// The list on disk, read again only when the file changed (its size or time): a game page, the
/// Library and the scans as the window opens each ask for it, and it's a big file.
fn catalog_on_disk(file: &Path) -> Value {
    static READ: Mutex<Option<(PathBuf, std::time::SystemTime, u64, Value)>> = Mutex::new(None);
    let Some((time, len)) = std::fs::metadata(file).ok().and_then(|m| Some((m.modified().ok()?, m.len()))) else { return Value::Null };
    if let Some((p, t, l, v)) = READ.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        if p == file && *t == time && *l == len {
            return v.clone();
        }
    }
    let cat = std::fs::read(file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    if !cat.is_null() {
        *READ.lock().unwrap_or_else(|e| e.into_inner()) = Some((file.to_path_buf(), time, len, cat.clone()));
    }
    cat
}

/// Said for a link that isn't a project the launcher can read, or one with no release to install.
const CANT_READ: &str = "We couldn't read that project. Check the link, or make sure it has a release with your builds.";

/// The project a pasted link is: GitHub, GitLab or SourceForge by its address; any other server
/// only when it answers as a Forgejo or Gitea server does (their addresses are their own).
pub(crate) async fn repo_of_link(st: &App, link: &str) -> Res<String> {
    if let Some(r) = catalog::repo_from_link(link) {
        return Ok(r);
    }
    let Some((host, owner, name)) = catalog::forgejo_from_link(link) else { return Err(Error::Msg(CANT_READ.into())) };
    if !catalog::forgejo_server(&st.http, &host).await {
        return Err(Error::Msg(CANT_READ.into()));
    }
    let repo = format!("forgejo:{host}/{owner}/{name}");
    trail::note(format!("Add a Recomp: {host} is a Forgejo server, so {repo}"));
    Ok(repo)
}

/// What a GitHub repo is, read before it's added: the catalog's entry when it has one; else the
/// game and console its description, topics and README point at, a catalog game it looks like, and
/// art from SteamGridDB when the player gave their key.
#[tauri::command]
async fn github_lookup(st: State<'_, App>, link: String, sgdb_key: Option<String>) -> Res<Value> {
    let repo = repo_of_link(&st, &link).await?;
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
    let fj = catalog::is_forgejo(&repo);
    let (info, readme) = if fj {
        catalog::forgejo_info(&st.http, &repo).await
    } else if lab {
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
        "host": if fj { "forgejo" } else if lab { "gitlab" } else { "github" },
    }))
}

/// Art for a game from SteamGridDB, with the player's key.
#[tauri::command]
async fn github_art(st: State<'_, App>, game: String, sgdb_key: String) -> Res<Value> {
    ghinfo::art(&st.http, sgdb_key.trim(), &game).await.map_err(Error::Msg)
}

/// The system its releases have a version for: this PC's, or Windows through Proton.
async fn github_os(st: &App, repo: &str) -> Res<&'static str> {
    let rels: Vec<Release> = serde_json::from_value(catalog::releases(&st.http, &st.dir, repo, true).await.map_err(|_| Error::Msg(CANT_READ.into()))?.data).unwrap_or_default();
    if rels.is_empty() {
        return Err(Error::Msg(CANT_READ.into()));
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
async fn add_github(app: AppHandle, st: State<'_, App>, link: String, info: Option<Value>) -> Res<Value> {
    let repo = repo_of_link(&st, &link).await?;
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
    let fj = catalog::is_forgejo(&repo);
    let meta: Value = if fj {
        catalog::forgejo_info(&st.http, &repo).await.0
    } else if lab {
        Value::Null
    } else {
        match st.http.get(format!("https://api.github.com/repos/{repo}")).timeout(std::time::Duration::from_secs(10)).send().await {
            Ok(r) if r.status().is_success() => r.json().await.unwrap_or(Value::Null),
            _ => Value::Null,
        }
    };
    let path = catalog::host_path(&repo).to_string();
    let owner = path.split('/').next().unwrap_or("").to_string();
    let id: String = format!("{}-{}", if catalog::is_sourceforge(&repo) { "sf" } else if fj { "fj" } else if lab { "gl" } else { "gh" }, path.to_lowercase().replace('/', "-")).chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' }).collect();
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
        "stars": meta.get("stargazers_count").or_else(|| meta.get("stars_count")).cloned().unwrap_or(Value::Null),
        "custom": true,
    });
    if fj {
        entry["host"] = json!("forgejo");
    } else if lab {
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
    // A Forgejo project's own setup file, read now so Easy Setup shows for it straight away.
    if fj && catalog::repo_profile(&st.http, &st.dir, &repo, true).await {
        let _ = app.emit("profiles-fresh", ());
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

/// The release a game's last setup that worked finished on: one game of several's own, else the
/// recomp's whole setup.
fn mark_of<'a>(g: &'a Installed, sub: Option<&str>) -> &'a str {
    let own = sub.and_then(|s| g.games.get(s)).map(|x| x.set_up.as_str()).filter(|m| !m.is_empty());
    own.unwrap_or(&g.set_up)
}

/// Whether a game's last build was cut short, read as mark_of reads its release: one game of
/// several's own once it has its own build (or its own cut short), else the whole recomp's. So a
/// game's own build that worked counts, whatever the recomp's says.
fn cut_of(g: &Installed, sub: Option<&str>) -> bool {
    let own = sub.and_then(|s| g.games.get(s)).filter(|x| !x.set_up.is_empty() || x.build_cut);
    own.map_or(g.build_cut, |x| x.build_cut)
}

/// Set up, or why not, as Play gives it (`p` scoped to one game of several when it's that game's,
/// `sub` naming it): from what the library has picked. A setup from an older launcher is read
/// first, so the answer never comes before it.
fn game_status(st: &App, g: &Installed, p: &profile::Profile, sub: Option<&str>) -> setup_state::Status {
    let read;
    let g = if g.upgrade.is_some() {
        upgrade_setup(st, &g.id);
        read = st.lib.lock().unwrap().installed.get(&g.id).cloned().unwrap_or_else(|| g.clone());
        &read
    } else {
        g
    };
    let rows = inputs::rows(p, g, &st.dir).unwrap_or_default();
    let facts = setup_state::Facts { profile: p, game: g, rows: &rows, set_up: mark_of(g, sub), cut: cut_of(g, sub), own_program: false, skipped: false };
    setup_state::of(&facts)
}

/// Once, at startup (and before any game's answer, so none comes first): every game set up by a
/// launcher from before format 3 (see upgrade_setup).
fn upgrade_setups(st: &App) {
    let ids: Vec<String> = st.lib.lock().unwrap().installed.iter().filter(|(_, g)| g.upgrade.is_some()).map(|(k, _)| k.clone()).collect();
    for id in ids {
        upgrade_setup(st, &id);
    }
}

/// The release mark for a game set up by a launcher from before this one, when that setup is due a
/// new build: not the installed release, so it builds again on the next Play, in the Finish the
/// Update window, as after any new release.
const SET_UP_BEFORE: &str = "before";

/// A game set up by a launcher from before format 3 (library_convert keeps how in `upgrade`) gets
/// the release it's installed on as the one it was set up on, when its setup was current then:
/// named files set up, or a copy picked (or already where the game reads it, or built), with the
/// `prepare.version` the manifest still has. One that was set up and due a new build then is
/// marked as set up before (SET_UP_BEFORE), and builds again on the next Play. Never a build just
/// for this launcher.
fn upgrade_setup(st: &App, id: &str) {
    let Some(g) = st.lib.lock().unwrap().installed.get(id).cloned().filter(|g| g.upgrade.is_some()) else { return };
    let was = g.upgrade.clone().unwrap_or_default();
    let then = profile_of(st, id, &g).and_then(|p| old_setup(&p, &g, &was));
    let mut lib = st.lib.lock().unwrap();
    let Some(x) = lib.installed.get_mut(id).filter(|x| x.upgrade.is_some()) else { return };
    x.upgrade = None;
    if let Some(current) = then {
        let mark = if current { x.tag.clone() } else { SET_UP_BEFORE.to_string() };
        x.set_up = mark.clone();
        for sub in &was.games_ready {
            x.games.entry(sub.clone()).or_default().set_up = mark.clone();
        }
        let how = if current { format!("kept as set up on {mark}") } else { "due a new build, which the next Play makes".to_string() };
        trail::note(format!("Setup {id}: set up by an older launcher, {how}"));
    }
    drop(lib);
    st.save();
}

/// How a game was set up by a launcher from before format 3: None, not set up; Some(true), set up
/// and current (what that launcher's Play said); Some(false), set up and due a new build (another
/// `prepare.version` than the manifest has now).
fn old_setup(p: &profile::Profile, g: &Installed, was: &library::SetupUpgrade) -> Option<bool> {
    let lifted_only = p.inputs.iter().all(|i| i.lifted || !i.picked());
    let set_up = if lifted_only { inputs::main_path(p, g).is_some() || p.build_done(&g.path) == Some(true) } else { was.inputs_ready || !was.games_ready.is_empty() };
    if !set_up {
        return None;
    }
    let version = p.prepare.as_ref().map(|pr| pr.version.as_str()).unwrap_or("");
    Some(p.prepare.is_none() || version == was.prepare_version)
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
                        scoped.inputs.is_empty() || game_status(&st, &g, &scoped, Some(&game.id)).ready
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

/// The copy a game's own setup takes (an ISO dropped on install.bat): the one picked, none when
/// the game's files are already where it reads them, or asked for.
fn copy_for_setup(g: &Installed, p: &profile::Profile) -> Res<Option<PathBuf>> {
    let Some(main) = p.main_input() else { return Ok(None) };
    if let Some(c) = inputs::picked(g, &main.id) {
        return Ok(Some(c));
    }
    if main.in_place(&g.path).is_some() {
        return Ok(None);
    }
    Err(Error::Msg(format!("NEED_COPY|{}", main.label)))
}

/// Easy Setup's answer for a game (`p` scoped to one game of several when it's that game's, `sub`
/// naming it): set up, or why not. The page and Play read the same one; this one also says the
/// player's own program and Skip Easy Setup.
fn easy_status(st: &App, id: &str, g: &Installed, p: &profile::Profile, sub: Option<&str>) -> setup_state::Status {
    let own_program = player_program(st, id, g).is_some();
    let skipped = player_sets_up(st, id);
    if own_program || skipped {
        let rows: Vec<inputs::Row> = vec![];
        return setup_state::of(&setup_state::Facts { profile: p, game: g, rows: &rows, set_up: "", cut: false, own_program, skipped });
    }
    game_status(st, g, p, sub)
}

/// Set up and ready to start as it is, by the same answer Play and the game's page give (one game
/// of several judged with its own files): for what starts it without the launcher, like Steam.
pub(crate) fn plays_as_set_up(st: &App, recomp: &str, g: &Installed, game: Option<(&games::Game, &[games::Game])>) -> bool {
    let Some(whole) = profile_of(st, recomp, g) else { return true };
    let mut p = whole.clone();
    if let Some((x, all)) = game {
        games::scope(&mut p, x, all);
    }
    let sub = game.map(|(x, _)| x.id.as_str());
    easy_status(st, recomp, g, &p, sub).ready
}

/// The game as Play starts it once it's built: the files it no longer reads (gone with Free Up
/// Space) are left out, and named as skipped, so their placeholders in the setup step's arguments
/// aren't taken for misspelled ones.
fn without_freed(p: &profile::Profile, rows: &[inputs::Row]) -> profile::Profile {
    let mut now = p.clone();
    let has = |id: &str| rows.iter().any(|r| r.id == id && r.path.is_some());
    let freed: Vec<String> = p.inputs.iter().filter(|i| i.picked() && !i.optional && !has(&i.id)).map(|i| i.id.clone()).collect();
    now.inputs.retain(|i| !freed.contains(&i.id));
    now.skipped_inputs.extend(freed);
    now
}

/// What Play answers for a game that isn't set up: the page opens Easy Setup (or asks) for it.
fn not_set_up(s: &setup_state::Status) -> Error {
    let msg = match s.reason {
        "copy" => format!("NEED_COPY|{}", s.detail),
        "files" => "NEED_COPY|Finish choosing this game's files in Easy Setup.".to_string(),
        "rebuild" => format!("NEED_REBUILD|{}", s.detail),
        "build_missing" => format!("NEED_BUILD|{}", s.detail),
        _ => format!("NEED_COPY|{}", s.detail),
    };
    Error::Msg(msg)
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
    /// Every file it asks for but the main one (that one is `needs`, `kind` and `copy`, the big
    /// box): its update, discs, DLC, keys.
    inputs: Vec<inputs::Row>,
    inputs_error: Option<String>,
    /// Set up with what's picked now: the setup screen can skip its run.
    inputs_ready: bool,
    /// Its setup step can run again without the copy, from what it kept (prepare.keeps_copy):
    /// with no copy picked, Set Up builds it from that.
    kept: bool,
    /// That folder, for the hint ("The game's own setup builds it into ..."), only when it's
    /// outside the game's own folder. One inside it is where Easy Setup builds, so there's
    /// nothing for the player to go and find.
    build_where: Option<PathBuf>,
    /// The program the player picked with Set Game Executable, when Play starts it as it is instead of
    /// the way the setup file says (player_program). Shown, so the player can tell Easy Setup is
    /// out of the way, and undo it.
    own_program: Option<PathBuf>,
    /// The main file is asked for, and how (its label).
    needs: Option<String>,
    /// "iso", "folder" or "iso_or_folder": what the main file can be.
    kind: String,
    /// The extensions the main file's formats name, for the file picker.
    copy_extensions: Vec<String>,
    /// Its setup does something Play doesn't (a setup step, a disc to unpack): a new release builds
    /// it again.
    rebuilds: bool,
    copy: Option<PathBuf>,
    settings: Vec<SetupOption>,
    actions: Vec<SetupAction>,
    /// Settings the launcher worked out for this game (not its developer's): shown as a beta.
    beta: bool,
    /// Its settings file isn't there until the game's first start.
    waiting: bool,
    /// Ticks for its setup step that apply on this PC (prepare.options), each with how it's set.
    prepare_options: Vec<Value>,
    /// What its setup step does (prepare.label), as the developer wrote it: Easy Setup's last step
    /// says it, so the player knows what Set Up starts.
    prepare_label: Option<String>,
    /// The player's files, when the game no longer needs them (Free Up Space).
    spare: Option<SpareCopy>,
    /// Set up, or why not: the answer Play gives too (setup_state).
    status: setup_state::Status,
    /// What the installed build needs from the PC (`system`), for the game page's list.
    system: Option<sysneeds::OsNeeds>,
    /// What of that this PC is too old for: shown in red, and said when it plays.
    system_old: Vec<sysneeds::TooOld>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
struct SpareCopy {
    /// The first of them, for the card.
    path: PathBuf,
    /// The rest: every file goes or none does, since a rebuild without one of them can't work.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    more: Vec<PathBuf>,
    /// Their size in bytes (folders aren't added up just to show the card).
    size: u64,
    /// "built": the game was built from them, starts without them, and its setup step keeps what
    /// it needs to build again (prepare.keeps_copy). "unpacked": the launcher unpacked the disc
    /// image into the game's folder, and builds again from that.
    why: &'static str,
}

/// Free Up Space: the player's files, when a new release can build the game again without them.
/// Every release builds again, so only a game whose setup step keeps what it needs
/// (prepare.keeps_copy), set up on the release installed and not reading the files when it
/// starts, or a disc image the launcher unpacked into the game's folder, offers it. Never a file
/// another game, or the game's own folder, uses.
fn spare_copy(g: &Installed, p: &profile::Profile, others: &[Installed]) -> Option<SpareCopy> {
    if g.local || p.start.is_none() {
        return None;
    }
    let keeps = p.prepare.as_ref().is_some_and(|pr| pr.keeps_copy) && p.kept_from_copy(&g.path).is_some();
    let built = keeps && g.set_up == g.tag && p.build_done(&g.path) == Some(true);
    let (files, why): (Vec<PathBuf>, &'static str) = if built {
        let mut picked = vec![];
        for i in p.inputs.iter().filter(|i| i.picked()) {
            let Some(path) = inputs::picked(g, &i.id) else { continue };
            // One the game reads when it starts can't go, and the rest are kept with it.
            if setup_state::start_needs(p, &i.id) {
                return None;
            }
            picked.push(path);
        }
        (picked, "built")
    } else {
        // Unpacked, and the unpacked files still all there.
        let unpacked = |id: &str| {
            let folder = inputs::picked(g, id).filter(|f| f.is_dir() && std::fs::read_dir(f).is_ok_and(|mut d| d.next().is_some()));
            let input = p.inputs.iter().find(|i| i.id == id);
            folder.is_some() && input.is_some_and(|i| profile::missing_in(&i.as_copy(), &g.path).is_empty())
        };
        let images: Vec<PathBuf> = g.unpacked_from.iter().filter(|(id, _)| unpacked(id)).map(|(_, x)| x.clone()).filter(|x| x.is_file()).collect();
        (images, "unpacked")
    };
    if files.is_empty() {
        return None;
    }
    let overlaps = |a: &Path, b: &Path| a.starts_with(b) || b.starts_with(a);
    let ours = |x: &Path| files.iter().any(|f| overlaps(f, x));
    if ours(&g.path) {
        return None;
    }
    let used_elsewhere = others.iter().filter(|o| o.id != g.id).any(|o| o.setup_inputs.values().chain(o.unpacked_from.values()).chain(std::iter::once(&o.path)).any(|x| ours(x)));
    if used_elsewhere {
        return None;
    }
    let size = files.iter().map(|f| std::fs::metadata(f).map(|m| if m.is_file() { m.len() } else { 0 }).unwrap_or(0)).sum();
    let mut files = files.into_iter();
    let path = files.next()?;
    Some(SpareCopy { path, more: files.collect(), size, why })
}

/// What the launcher can set up for a game: its files and its settings. None for games it can't.
/// With an Easy Setup window open, what it picked and hasn't set up yet is shown.
#[tauri::command(async)]
fn game_setup(app: AppHandle, st: State<'_, App>, id: String) -> Option<Setup> {
    read_screen(&app);
    // One game of several: Easy Setup asks only for its own inputs and the shared ones.
    let pick = game_of(&st, &id);
    let id = pick.as_ref().map(|p| p.recomp.clone()).unwrap_or(id);
    let sub = pick.as_ref().map(|p| p.game.id.clone());
    upgrade_setup(&st, &id);
    let kept = st.lib.lock().unwrap().installed.get(&id).cloned()?;
    let g = with_pending(&id, kept.clone());
    let whole = profile_of(&st, &id, &g)?;
    let mut p = whole.clone();
    if let Some(pk) = &pick {
        games::scope(&mut p, &pk.game, &pk.all);
    }
    let p = p;
    let cur = p.current(&g.path, &g.picks);
    let main = p.main_input().cloned();
    let picks_waiting = g.setup_inputs != kept.setup_inputs;
    // Picked in the window and not set up yet: its Set Up keeps them.
    let status = if picks_waiting { setup_state::picked_not_set_up() } else { easy_status(&st, &id, &g, &p, sub.as_deref()) };
    let copy = inputs::main_path(&p, &g);
    let system = system_needs(&whole, &g);
    let rows = inputs::rows(&p, &g, &st.dir).unwrap_or_default();
    Some(Setup {
        inputs: rows.into_iter().filter(|r| main.as_ref().map_or(true, |m| m.id != r.id)).collect(),
        inputs_error: inputs::validate(&p).err(),
        inputs_ready: status.ready,
        kept: setup_state::builds_from_kept(&p, &g),
        build_where: if status.ready { None } else { whole.build_home(&g.path).filter(|h| !h.starts_with(&g.path)) },
        own_program: player_program(&st, &id, &g),
        needs: main.as_ref().map(|m| m.label.clone()).or_else(|| (!p.inputs.is_empty()).then(|| "Choose the files this game needs.".into())),
        kind: main.as_ref().map(|m| m.as_copy().kind).unwrap_or_default(),
        rebuilds: setup_state::needs_run(&p),
        copy_extensions: main.as_ref().map(|m| m.formats.iter().flat_map(|f| f.extensions.clone()).collect()).unwrap_or_default(),
        copy,
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
        prepare_options: p.prepare.as_ref().map(|pr| prepare_options_here(&st.dir, pr, &g.prepare_opts)).unwrap_or_default(),
        prepare_label: p.prepare.as_ref().map(|pr| pr.label.clone()).filter(|l| !l.trim().is_empty()),
        spare: if pick.is_some() || setup_busy(&id) || picks_waiting { None } else { spare_copy(&kept, &p, &st.lib.lock().unwrap().installed.values().cloned().collect::<Vec<_>>()) },
        status,
        system_old: system.as_ref().map(|(_, old)| old.clone()).unwrap_or_default(),
        system: system.map(|(n, _)| n),
    })
}

/// What the installed build needs from the PC (`system` in its recomplauncher.json), and what of it
/// this PC is too old for.
fn system_needs(p: &profile::Profile, g: &Installed) -> Option<(sysneeds::OsNeeds, Vec<sysneeds::TooOld>)> {
    let build_os = if is_windows_build(g) { "windows" } else { assets::os_now() };
    let needs = p.system.as_ref()?.for_build(build_os)?.clone();
    let old = sysneeds::too_old(&needs, build_os, assets::os_now(), sysneeds::os_version().as_deref(), sysneeds::glibc_version().as_deref());
    Some((needs, old))
}

/// Free Up Space: the player's files go to the Recycle Bin (the trash on Linux and macOS), once
/// it's checked again that a new release can build the game without them. Every one goes or none
/// does. Gives back what it freed, in bytes.
#[tauri::command]
async fn free_up_space(st: State<'_, App>, id: String) -> Res<u64> {
    if st.running.lock().unwrap().contains_key(&id) || st.jobs.lock().unwrap().contains_key(&id) || setup_busy(&id) || st.moving.lock().unwrap().contains(&id) {
        return Err(Error::Msg("Close the game or stop setup first.".into()));
    }
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let p = profile_of(&st, &id, &g).ok_or_else(|| Error::Msg("This game has no setup profile.".into()))?;
    let others: Vec<Installed> = st.lib.lock().unwrap().installed.values().cloned().collect();
    let spare = spare_copy(&g, &p, &others).ok_or_else(|| Error::Msg("This game still needs your files, so they stay.".into()))?;
    let all: Vec<PathBuf> = std::iter::once(spare.path.clone()).chain(spare.more.iter().cloned()).collect();
    let size: u64 = all.iter().map(|f| if f.is_dir() { manage::dir_size(f) } else { std::fs::metadata(f).map(|m| m.len()).unwrap_or(0) }).sum();
    let mut gone = vec![];
    for f in &all {
        let path = f.clone();
        let moved = tokio::task::spawn_blocking(move || files::to_trash(&path)).await.map_err(|e| Error::Msg(e.to_string()))?;
        if let Err(e) = moved {
            forget_files(&st, &id, &gone);
            return Err(Error::Msg(format!("Couldn't move it to the trash ({e}). You can delete it yourself: {}", f.display())));
        }
        gone.push(f.clone());
    }
    forget_files(&st, &id, &gone);
    let how = if spare.why == "built" { "it builds again from what its setup kept" } else { "it was unpacked from them" };
    trail::note(format!("Free Up Space {id}: {} files went to the trash ({size} bytes; {how})", gone.len()));
    Ok(size)
}

/// The files Free Up Space moved to the trash, no longer picked: a game set up without them.
fn forget_files(st: &App, id: &str, gone: &[PathBuf]) {
    if let Some(x) = st.lib.lock().unwrap().installed.get_mut(id) {
        x.setup_inputs.retain(|_, v| !gone.contains(v));
        x.unpacked_from.retain(|_, v| !gone.contains(v));
    }
    st.save();
}

/// Pick or clear the file for one of a game's inputs. From an Easy Setup window (`pending`) it
/// waits there until a setup with it works; from the game's page it's kept now. Shared keys are
/// imported for every game that declares them, and never un-set another game: each uses the new
/// ones at its next setup.
#[tauri::command(async)]
fn setup_input_set(st: State<'_, App>, id: String, input: String, path: Option<String>, pending: Option<bool>) -> Res<()> {
    pick_file(&st, &id, Some(&input), path, pending.unwrap_or(false))
}

/// The player picked their copy of the game: the main input's file.
#[tauri::command(async)]
fn set_game_copy(st: State<'_, App>, id: String, path: String, pending: Option<bool>) -> Res<()> {
    pick_file(&st, &id, None, Some(path), pending.unwrap_or(false))
}

/// The player picked the game's update: its input with role update.
#[tauri::command(async)]
fn set_update_copy(st: State<'_, App>, id: String, path: String, pending: Option<bool>) -> Res<()> {
    let rid = setup_record_id(&st, &id);
    let g = st.lib.lock().unwrap().installed.get(&rid).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let p = profile_of(&st, &rid, &g).ok_or_else(|| Error::Msg("The launcher can't set this game up yet.".into()))?;
    let update = p.inputs.iter().find(|i| i.role == "update" && i.picked()).map(|i| i.id.clone()).ok_or_else(|| Error::Msg("This game doesn't need an update file.".into()))?;
    pick_file(&st, &id, Some(&update), Some(path), pending.unwrap_or(false))
}

/// One pick (see setup_input_set). `input` None: the main file.
fn pick_file(st: &App, id: &str, input: Option<&str>, path: Option<String>, pending: bool) -> Res<()> {
    let rid = setup_record_id(st, id);
    if setup_busy(id) || setup_busy(&rid) || st.running.lock().unwrap().contains_key(&rid) {
        return Err(Error::Msg("Close the game or stop setup first.".into()));
    }
    let kept = st.lib.lock().unwrap().installed.get(&rid).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let mut g = if pending { with_pending(&rid, kept.clone()) } else { kept.clone() };
    let p = profile_of(st, &rid, &g).ok_or_else(|| Error::Msg("The launcher can't set this game up yet.".into()))?;
    inputs::validate(&p).map_err(Error::Msg)?;
    let declared = match input {
        Some(want) => p.inputs.iter().find(|i| i.id == want),
        None => p.main_input(),
    };
    let declared = declared.cloned().ok_or_else(|| Error::Msg("Unknown setup input.".into()))?;
    let row = inputs::rows(&p, &g, &st.dir).map_err(Error::Msg)?.into_iter().find(|r| r.id == declared.id);
    if !row.is_some_and(|r| r.active) {
        return Err(Error::Msg("That input isn't required by the selected game files.".into()));
    }
    if !declared.shared.is_empty() {
        let Some(path) = path else { return Err(Error::Msg("Remove shared keys from the shared-key controls.".into())) };
        if !st.running.lock().unwrap().is_empty() || SETUP_RUN.lock().unwrap().as_ref().is_some_and(|m| !m.is_empty()) || !INPUT_WORKERS.lock().unwrap().is_empty() {
            return Err(Error::Msg("Close running games and stop setup before replacing shared keys.".into()));
        }
        return inputs::import_key(&st.dir, &declared.shared, Path::new(&path)).map_err(Error::Msg);
    }
    let picked = match path {
        Some(path) => {
            let path = PathBuf::from(path);
            if !path.is_absolute() {
                return Err(Error::Msg("Choose an absolute file or folder path.".into()));
            }
            let taken = take_pick(st, &p, &declared, &g, path).map_err(Error::Msg)?;
            g.setup_inputs.insert(declared.id.clone(), taken.clone());
            if !inputs::rows(&p, &g, &st.dir).map_err(Error::Msg)?.iter().any(|r| r.id == declared.id && r.path.is_some()) {
                return Err(Error::Msg("That file or folder isn't the kind this game asks for here.".into()));
            }
            // One file can't fill two slots (Disc 1 and Disc 2).
            let chosen = taken.canonicalize().map_err(|_| Error::Msg("Couldn't open that input.".into()))?;
            if g.setup_inputs.iter().any(|(k, v)| k != &declared.id && v.canonicalize().ok().as_ref() == Some(&chosen)) {
                return Err(Error::Msg("That file is already picked for another slot.".into()));
            }
            Some(taken)
        }
        None => None,
    };
    if pending {
        pend(&rid, &declared.id, picked);
        return Ok(());
    }
    let main = p.main_input().is_some_and(|m| m.id == declared.id);
    if let Some(x) = st.lib.lock().unwrap().installed.get_mut(&rid) {
        match &picked {
            Some(f) => x.setup_inputs.insert(declared.id.clone(), f.clone()),
            None => x.setup_inputs.remove(&declared.id),
        };
        // A game whose setup does more than Play sets up again with other files.
        if setup_state::needs_run(&p) {
            x.set_up.clear();
            x.games.values_mut().for_each(|s| s.set_up.clear());
        }
    }
    st.save();
    // Written now too, so the recomp's own launcher knows the copy as well.
    if main {
        let _ = p.prepare(&g.path, picked.as_deref());
    }
    Ok(())
}

/// A file picked for an input, as it's kept, or why not: the update's own check for an update;
/// for the main file and any file that says how to tell it's the right one, a .7z or .zip unpacked
/// first, a folder one level off put right, then the copy's checks. Any other file only has to be
/// the kind the input takes. The one picked before, picked again, was checked then.
fn take_pick(st: &App, p: &profile::Profile, i: &inputs::Input, g: &Installed, path: PathBuf) -> Result<PathBuf, String> {
    if inputs::picked(g, &i.id).as_ref() == Some(&path) {
        return Ok(path);
    }
    if !path.exists() {
        return Err("That file isn't there anymore.".into());
    }
    if i.role == "update" {
        i.as_extra().check(&path)?;
        return Ok(path);
    }
    let main = p.main_input().is_some_and(|m| m.id == i.id);
    if !main && i.check.is_empty() && i.dumps.is_empty() {
        return Ok(path);
    }
    let need = i.as_copy();
    let mut copy = path;
    if matches!(profile::packed(&need, &copy).as_deref(), Some("7z" | "zip")) {
        copy = unpack_copy(st, &need, &copy)?;
    }
    // A folder one level off (the one around the game's files, or one inside them): the right one
    // is used instead of turning the player away.
    if copy.is_dir() && copy.parent().is_some() && profile::check_copy(&need, &copy).is_err() {
        if let Some(inner) = copy_in(&need, &copy).filter(|c| c != &copy) {
            copy = inner;
        } else if let Some(up) = copy.parent().filter(|u| u.parent().is_some() && profile::check_copy(&need, u).is_ok()) {
            copy = up.to_path_buf();
        }
    }
    copy_ok(&need, &copy, &g.path, true)?;
    Ok(copy)
}

/// Shared keys removed: from the launcher and from every game's folder it put them in. A game that
/// needs them asks for them again on its next Play; none is set up again for it.
#[tauri::command(async)]
fn setup_key_remove(st: State<'_, App>, id: String, input: String) -> Res<()> {
    if !st.running.lock().unwrap().is_empty() || SETUP_RUN.lock().unwrap().as_ref().is_some_and(|m| !m.is_empty()) || !INPUT_WORKERS.lock().unwrap().is_empty() {
        return Err(Error::Msg("Close running games and stop setup before removing shared keys.".into()));
    }
    let rid = setup_record_id(&st, &id);
    let g = st.lib.lock().unwrap().installed.get(&rid).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let p = profile_of(&st, &rid, &g).ok_or_else(|| Error::Msg("This game has no setup profile.".into()))?;
    let resource = p.inputs.iter().find(|i| i.id == input && !i.shared.is_empty()).ok_or_else(|| Error::Msg("Unknown shared key input.".into()))?.shared.clone();
    let games: Vec<_> = st.lib.lock().unwrap().installed.values().cloned().collect();
    for other in games {
        let Some(profile) = profile_of(&st, &other.id, &other) else { continue };
        for key in profile.inputs.iter().filter(|k| k.shared == resource) {
            inputs::remove_key(&profile, &other, &st.dir, &key.id).map_err(Error::Msg)?;
        }
    }
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
        let _ = p.prepare(&g.path, inputs::main_path(&p, &g).as_deref());
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
        let _ = p.prepare(&g.path, inputs::main_path(&p, &g).as_deref());
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
        let _ = p.prepare(&g.path, inputs::main_path(&p, &g).as_deref());
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
    let from_big_picture = big_picture.unwrap_or(false);
    BIG_PICTURE.store(from_big_picture, Ordering::Relaxed);
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
    #[cfg(windows)]
    let monitor = if from_big_picture {
        let preference = app.state::<App>().lib.lock().unwrap().settings.tv_monitor.clone();
        let monitors = display::enumerate_monitors(&app);
        let target = display::resolve_target_monitor(&monitors, Some(&preference));
        if target.is_none() {
            trail::note("Big Picture game monitor targeting skipped: no monitor was available");
        }
        target
    } else {
        None
    };
    #[cfg(not(windows))]
    let monitor = None;
    start_game_with_monitor_context(&app, &full, None, monitor)
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
    for (input, c) in &g.setup_inputs {
        let size = std::fs::metadata(c).ok().filter(|m| m.is_file()).map(|m| format!(", {} bytes", m.len())).unwrap_or_default();
        let what = match input.as_str() {
            "game" => "Player's copy".to_string(),
            "update" => "Update file".to_string(),
            other => format!("Picked for {other}"),
        };
        l.push(format!("{what}: {} ({}{size})", hide(c.display().to_string()), if c.is_dir() { "folder" } else { "file" }));
    }
    if !g.set_up.is_empty() {
        l.push(format!("Set up on: {}", g.set_up));
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
pub(crate) fn last_run(dir: &Path, id: &str) -> Option<String> {
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
    let prof = profile_of(st, id, g);
    prof.as_ref().and_then(|p| p.command_for(&g.path, inputs::main_path(p, g).as_deref(), &g.picks, is_windows_build(g)).map(|(e, _)| e))
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
    profile_of(st, id, g).is_some_and(|p| p.main_input().is_some())
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
    // The program it starts.
    let prof = profile_of(&st, &id, &g);
    let exe = prof.as_ref().and_then(|p| p.command_for(&g.path, inputs::main_path(p, &g).as_deref(), &g.picks, is_windows_build(&g)).map(|(e, _)| e))
        .or_else(|| g.exe.as_ref().map(|e| g.path.join(e)));
    let Some(exe) = exe.filter(|e| e.is_file()) else {
        out.push(check("bad", "The program it starts isn't there", "Antivirus may have taken it out, or the download was cut short.", "setexe"));
        return Ok(out);
    };
    let name = exe.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    // A program in a setup's working folder (temp\…\build) or one of the game's tools: what it
    // starts is probably not the finished game.
    let rel = exe.strip_prefix(&g.path).map(Path::to_path_buf).unwrap_or_else(|_| PathBuf::from(&name));
    let names: Vec<&str> = [g.name.as_str(), g.repo.rsplit(['/', ':']).next().unwrap_or("")].into_iter().filter(|n| !n.is_empty()).collect();
    if install::system_program(&exe) {
        out.push(check("bad", "It starts {name}, which isn't the game", "That's a program from Windows or another app ({folder}), not this game's. Choose the game's own program.", "setexe").with("name", &name).with("folder", exe.parent().map(|d| d.display().to_string()).unwrap_or_default()));
    } else if install::looks_like_tool(&rel, &names) {
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
    if let Some((need, main)) = prof.as_ref().and_then(|p| Some((p.main_copy()?, p.main_input()?.id.clone()))) {
        match g.setup_inputs.get(&main) {
            Some(c) if !c.exists() => out.push(check("bad", "Your copy of the game isn't there anymore", c.display().to_string(), "copy")),
            Some(c) => match copy_ok(&need, c, &g.path, false) {
                Ok(()) if profile::odd_snes_size(c) => out.push(check("warn", "Your copy of the game may be incomplete", "Its size isn't one a full dump of the cartridge has. If the game won't start, try another copy of your own cartridge.", "copy")),
                Ok(()) if need.checks_anything() => out.push(check("ok", "Your copy of the game checks out", c.display().to_string(), "")),
                // Nothing in it is checked: it's there, and that's all the launcher can say.
                Ok(()) => out.push(check("info", "Your copy of the game is there", "The launcher can't check what's in it for this game. If the game says files are missing, pick the folder or file that holds the whole game.", "copy")),
                Err(why) => out.push(check("bad", "Your copy of the game isn't the one it reads", why, "copy")),
            },
            None if prof.as_ref().map(|p| p.already_in_place(&g.path).is_some()).unwrap_or(false) => out.push(check("ok", "The game's files are in its folder", "", "")),
            // A game its setup builds from the copy: nothing to start until that's done.
            None if prof.as_ref().map(|p| p.prepare.is_some()).unwrap_or(false) => out.push(check("bad", "It hasn't been built from your copy yet", "Press Play and pick your copy. The launcher builds the game from it before it starts.", "copy")),
            None => out.push(check("bad", "Your copy of the game isn't set yet", need.label.clone(), "copy")),
        }
    }
    // Built, but not where it starts from anymore.
    if let (Some(gone), Some(_)) = (prof.as_ref().and_then(|p| p.built_folder_missing(&g.path)), prof.as_ref().and_then(|p| p.main_input()).and_then(|m| g.setup_inputs.get(&m.id))) {
        out.push(check("bad", "The game built from your copy isn't there anymore", gone.display().to_string(), "copy"));
    }
    // Its own first-time setup (not when the developer's setup file sets the game up instead).
    let dev_start = prof.as_ref().map(|p| p.start.is_some() && !p.from_boot).unwrap_or(false);
    if g.setup.is_some() && !g.setup_done && !dev_start {
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
        let needs_copy = prof.as_ref().is_some_and(|p| p.main_input().is_some());
        let said = |w: &str| text.lines().rev().take(120).any(|l| l.contains(w));
        if said("GLIBC_") && said("not found") {
            // A Linux build made on a newer Linux than this one (SteamOS, an older distro).
            out.push(check("bad", "Its Linux build needs a newer Linux", "It was built for a newer system library (glibc) than this Linux has. Its Windows version usually runs fine through Proton: switch to it from the game's page, or update your system.", ""));
        } else if said("Required extensions are missing") || said("Missing required extension: VK_") || said("Unable to create instance") {
            // Vulkan's own words for a graphics driver that can't run it: not a missing file.
            out.push(check("bad", "Your graphics driver can't run it", "Vulkan couldn't start with the parts this game needs. Update your graphics driver from NVIDIA, AMD or Intel; on a laptop, make sure it uses its faster graphics card.", "driver"));
        } else if let Some(line) = missing {
            out.push(check("bad", "It says something it needs is missing", line.trim().chars().take(300).collect::<String>(), if needs_copy { "copy" } else { "files" }));
        } else if let Some(line) = text.lines().rev().take(80).find(|l| wrong_copy_line(l)) {
            // It found game files, but not the right ones (another game, a bad dump, a folder one
            // level off): picked again in Easy Setup.
            out.push(check("bad", "It says your game files aren't the right ones", line.trim().chars().take(300).collect::<String>(), if needs_copy { "copy" } else { "" }));
        }
        let tail: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        let tail = tail[tail.len().saturating_sub(12)..].join("\n");
        if !tail.is_empty() {
            out.push(check("info", "What it printed last time", tail, ""));
        }
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

/// How a Python game starts here: Windows' py launcher (asked for Python 3), or python3 / python
/// from the PATH. On Windows the copies in WindowsApps are left out: without a Python installed
/// from the Store they only open the Store.
fn python_here() -> Option<(PathBuf, Vec<&'static str>)> {
    let path = std::env::var_os("PATH")?;
    let find = |name: &str| std::env::split_paths(&path).map(|d| d.join(name)).find(|p| p.is_file() && !p.to_string_lossy().to_lowercase().contains("windowsapps"));
    if cfg!(windows) {
        if let Some(py) = find("py.exe") {
            return Some((py, vec!["-3"]));
        }
        return find("python3.exe").or_else(|| find("python.exe")).map(|p| (p, vec![]));
    }
    find("python3").or_else(|| find("python")).map(|p| (p, vec![]))
}

/// The program the player picked in the game's settings, when the game's setup file starts another
/// one: a moved game, or a program of their own. None when they didn't pick, or picked the same.
fn picked_program(st: &App, id: &str, g: &Installed) -> Option<(PathBuf, profile::Profile)> {
    if g.exe_rules != u8::MAX {
        return None;
    }
    let exe = g.exe.as_ref().map(|e| if e.is_absolute() { e.clone() } else { g.path.join(e) }).filter(|e| e.is_file())?;
    let p = profile_of(st, id, g)?;
    (p.start.is_some() && !p.starts_program(&exe)).then_some((exe, p))
}

/// The player's picked program, when Play starts it as it is. On a game Easy Setup can do, only
/// while Easy Setup is off for it: Skip Easy Setup is the one switch for who sets a game up, so
/// the game's page and Play can't disagree (picking the program turns it off, in the UI).
fn player_program(st: &App, id: &str, g: &Installed) -> Option<PathBuf> {
    let (exe, p) = picked_program(st, id, g)?;
    let easy_on = p.easy_setup() && !player_sets_up(&st, id);
    (!easy_on).then_some(exe)
}

/// The Easy Setup switch of an installed game (its Manage tab, Skip Easy Setup in its menu, Use
/// Easy Setup Instead): who sets it up, "easy" or "own". Only the mode changes; the program or
/// build the player picked stays recorded, and Play uses it again when the switch goes back.
#[tauri::command(async)]
fn set_setup_mode(st: State<'_, App>, id: String, mode: library::SetupMode) -> Res<Installed> {
    let rid = setup_record_id(&st, &id);
    let out = {
        let mut l = st.lib.lock().unwrap();
        let g = l.installed.get_mut(&rid).ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
        g.mode = mode;
        g.clone()
    };
    st.save();
    trail::note(format!("{rid}: set up by {}", match mode { library::SetupMode::Easy => "Easy Setup", library::SetupMode::Own => "the player" }));
    Ok(out)
}

/// Skip Easy Setup: the program the player picked, or the release's own, as it is.
fn own_plan(g: &Installed, picked: Option<&Path>) -> Res<Option<(PathBuf, Vec<String>)>> {
    let exe = picked
        .map(Path::to_path_buf)
        .or_else(|| g.exe.clone())
        .map(|e| if e.is_absolute() { e } else { g.path.join(e) })
        .ok_or_else(|| Error::Msg("It doesn't have a program to start yet. Pick it with Set Game Executable in its Manage menu.".into()))?;
    let name = exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let exe = install::program_choice(&exe, assets::os_now()).ok_or_else(|| Error::Msg(format!("{name} isn't an available program anymore (an update may have renamed it). Pick it again with Set Game Executable in its Manage menu.")))?;
    Ok(Some((exe, vec![])))
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
            // The old release an update set aside isn't what plays.
            if depth > 0 && e.file_name() != swap::FOLDER && has_linux_program(&p, depth - 1) {
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

/// The launcher stopping before its window opened (Windows without a working WebView2, say) said
/// nothing at all: a launcher without a console has nowhere to print. The reason shows in a box.
#[cfg(windows)]
fn say_why_it_stopped() {
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        before(info);
        // Only the main thread: a helper thread that stops doesn't take the launcher with it.
        if std::thread::current().name() != Some("main") {
            return;
        }
        let what = info.payload().downcast_ref::<String>().cloned().or_else(|| info.payload().downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
        trail::note(format!("The launcher stopped: {what}"));
        // Every window that fails to open reads "failed to create webview: WebView2 error", so the
        // text can't tell a missing WebView2 from anything else: ask WebView2 for its version.
        let text = if webview2_missing() {
            format!("RecompLauncher couldn't open its window, because Microsoft Edge WebView2 isn't installed on this PC.\n\nInstall the WebView2 Runtime from Microsoft (search for \"WebView2 Runtime\"), then open RecompLauncher again.\n\n{what}")
        } else {
            format!("RecompLauncher couldn't start.\n\n{what}\n\nPlease report this at https://github.com/elripalda/RecompLauncher/issues and attach launcher-trail.log from %APPDATA%\\com.recomplauncher.app.")
        };
        #[link(name = "user32")]
        extern "system" {
            fn MessageBoxW(hwnd: *mut std::ffi::c_void, text: *const u16, caption: *const u16, kind: u32) -> i32;
        }
        let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
        let (t, c) = (wide(&text), wide("RecompLauncher"));
        // MB_OK | MB_ICONERROR | MB_SETFOREGROUND
        // SAFETY: both strings end with a zero and live until the call returns.
        unsafe { MessageBoxW(std::ptr::null_mut(), t.as_ptr(), c.as_ptr(), 0x0000_0010 | 0x0001_0000) };
    }));
}

/// True only when WebView2 says for sure it isn't there. It asks from a thread of its own: a panic
/// inside the panic hook would end the launcher without the box, and a helper's panic only fails
/// the join. Anything it can't tell counts as not missing.
#[cfg(windows)]
fn webview2_missing() -> bool {
    let Ok(asking) = std::thread::Builder::new().spawn(tauri::webview_version) else {
        return false;
    };
    matches!(asking.join(), Ok(Err(_)))
}

/// Where what a game printed on its last start is kept.
pub(crate) fn game_output_path(dir: &Path, id: &str) -> PathBuf {
    dir.join("game-output").join(log_name(id))
}

/// A game id as a log's file name.
fn log_name(id: &str) -> String {
    let safe: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    format!("{safe}.log")
}

/// A fresh file for what a game prints on this start (the last start's is replaced).
fn game_output_file(dir: &Path, id: &str) -> Option<std::fs::File> {
    fresh_file(&game_output_path(dir, id))
}

/// Where everything a game's setup step printed on its last run is kept: the full log behind
/// the few lines a failed setup shows.
pub(crate) fn setup_output_path(dir: &Path, id: &str) -> PathBuf {
    dir.join("setup-output").join(log_name(id))
}

/// A fresh file for what the setup step prints on this run (the last run's is replaced).
fn setup_output_file(dir: &Path, id: &str) -> Option<std::fs::File> {
    fresh_file(&setup_output_path(dir, id))
}

fn fresh_file(p: &Path) -> Option<std::fs::File> {
    std::fs::create_dir_all(p.parent()?).ok()?;
    std::fs::File::create(p).ok()
}

/// A failed setup's Open Full Log: everything its setup step printed, in the PC's text viewer.
#[tauri::command(async)]
fn open_setup_log(app: AppHandle, st: State<'_, App>, id: String) -> Res<()> {
    let p = setup_output_path(&st.dir, &id);
    if !p.is_file() {
        return Err(Error::Msg("There's no setup log for it yet.".into()));
    }
    open_thing(&app, &p.to_string_lossy(), false)
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

/// The window must leave controllers to the launcher (see pads::window_stays_off).
#[tauri::command(async)]
fn pads_window_off() -> bool {
    pads::window_stays_off()
}

/// The right stick, for Big Picture's box tilt.
#[tauri::command(async)]
fn pad_right_stick() -> [f32; 2] {
    pads::right_stick()
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
    if !p.starts_with(&g.path) && install::system_program(&p) {
        return Err(Error::Msg("That's a program from Windows or another app, not this game's. Choose the game's own program.".into()));
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

/// Set Game Executable undone (Use Default): the player's pick goes, and the launcher
/// picks the program again by its own rules, so Play starts the game its setup file's way.
#[tauri::command]
fn forget_program(st: State<'_, App>, id: String) -> Res<Installed> {
    let g = {
        let mut l = st.lib.lock().unwrap();
        let g = l.installed.get_mut(&id).ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
        if g.exe_rules == u8::MAX {
            g.exe = None;
            g.exe_rules = 0;
        }
        g.clone()
    };
    st.save();
    // The same pick Play makes when the launcher's rules change, made now so the game's page
    // shows the program again straight away.
    let _ = own_setup::pending(&st, &id, &g);
    trail::note(format!("{id}: the program the player picked is let go; the launcher starts it its own way again"));
    st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))
}

#[tauri::command(async)]
fn open_folder(app: AppHandle, st: State<'_, App>, id: Option<String>, path: Option<String>) -> Res<()> {
    #[cfg(not(target_os = "linux"))]
    use tauri_plugin_opener::OpenerExt;
    let p = {
        let l = st.lib.lock().unwrap();
        match (id, path) {
            (Some(id), _) => l.installed.get(&id).map(|g| g.path.clone()).ok_or_else(|| Error::Msg("It isn't installed.".into()))?,
            // Only folders the launcher knows about: libraries, their kept downloads and ROM folders.
            (None, Some(p)) => {
                let p = PathBuf::from(p);
                let named_downloads = p.file_name() == Some(std::ffi::OsStr::new(".downloads"));
                let in_library = p.parent().is_some_and(|lib| l.settings.all_libraries().iter().any(|x| x == lib));
                let kept_downloads = named_downloads && in_library;
                let known = l.settings.all_libraries().contains(&p)
                    || kept_downloads
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
            refresh_repo_profiles(&app, &st, &f.data, false);
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
    refresh_repo_profiles(&app, &st, &f.data, force);
    if !f.offline {
        note_new_apps(&app, &st, &f.data);
    }
    Ok(f)
}

/// Forgejo projects' own recomplauncher.json, read in the background when it isn't on disk yet or
/// is old (`force`: the player pressed Refresh). The window hears when one came or changed
/// ("profiles-fresh"), so Easy Setup shows for it.
fn refresh_repo_profiles(app: &AppHandle, st: &App, cat: &Value, force: bool) {
    let due = catalog::repo_profiles_due(&st.dir, cat, force);
    if due.is_empty() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let st = app.state::<App>();
        let mut changed = false;
        for repo in due {
            if catalog::repo_profile(&st.http, &st.dir, &repo, true).await {
                trail::note(format!("Setup file for {repo} read from its project"));
                changed = true;
            }
        }
        if changed {
            let _ = app.emit("profiles-fresh", ());
        }
    });
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
            // A Forgejo project's list comes from its own server, which rests on its own.
            let from = if catalog::is_forgejo(&repo) { catalog::releases_url(&repo) } else { format!("{}/api/gh.php", catalog::base()) };
            if f.age >= catalog::RELEASES_FRESH && !net::resting(&from, false) {
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
    check_updates_for(&st, force.unwrap_or(false)).await
}

async fn check_updates_for(st: &App, force: bool) -> Res<HashMap<String, String>> {
    let games: Vec<Installed> = st.lib.lock().unwrap().installed.values().cloned().collect();
    let associations: HashMap<String, (String, PathBuf)> = games.iter().map(|g| (g.id.clone(), (g.repo.clone(), g.path.clone()))).collect();
    // Manual checks refresh the approved release too; a cached tag can otherwise hide a newly
    // approved update even when its release info was just fetched.
    let cat = catalog::catalog(&st.http, &st.dir, force).await.map(|f| f.data).unwrap_or(Value::Null);
    let same = |a: &str, b: &str| {
        let n = |s: &str| s.trim().trim_start_matches(['v', 'V']).to_ascii_lowercase();
        !a.trim().is_empty() && n(a) == n(b)
    };
    let mut out = HashMap::new();
    let (mut asked, mut failed) = (0, 0);
    for g in games.into_iter().filter(|g| !g.repo.is_empty() && !g.local) {
        asked += 1;
        let entry = catalog::app(&cat, &g.id);
        // The project the catalog names, as installing and the Updates tab read it: a project
        // that moved (Hookshot, to its own Forgejo server) puts new releases there, not in the old.
        let repo = listed_repo(&g, entry);
        let listed = entry.map(|a| str_of(a, "version").to_string()).unwrap_or_default();
        let on_disk = if !force && same(&listed, &g.tag) { catalog::releases_on_disk(&st.dir, &repo) } else { None };
        let got = match on_disk {
            Some(f) => Ok(f),
            None => catalog::releases(&st.http, &st.dir, &repo, force).await,
        };
        let Ok(f) = got else {
            failed += 1;
            continue;
        };
        let Ok(rels) = serde_json::from_value::<Vec<Release>>(f.data) else { continue };
        // Added from the player's own folder without a version: the release out when its program
        // was made is the one they have, so an update isn't offered for the version they're on.
        // One with a tag spelled differently from its release (1.0.0 for v1.0.0, from another
        // launcher): saved as the release spells it, so every place that compares tags after
        // this, in the launcher and its window, sees the release it's on.
        let mut g = g;
        let found = if g.tag.is_empty() { version_from_files(&g, &rels) } else { exact_tag(&rels, &g.tag) };
        if let Some(tag) = found {
            keep_tag(st, &mut g, tag);
        }
        // The waiting installer ran outside the launcher (the player built the new version
        // themselves): its version is the one they have now.
        if !g.installer_tag.is_empty() && installer::built_since_staged(&g) {
            if let Some(done) = installer::finished_outside(&st, &g.id) {
                g = done;
            }
        }
        if let Some(tag) = offered_update(&g, entry, &rels) {
            out.insert(g.id.clone(), tag);
        }
    }
    // A project may have been corrected while its network requests were in flight. The game's own
    // repo is left as it is when its project moved: it says where the files on disk came from, and
    // the next install writes the catalog's.
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
    // An update whose installer is downloaded and waiting for Set Up is the version on offer
    // already: the button is Set Up, not the same update again.
    let waiting;
    let g = if g.installer_tag.is_empty() {
        g
    } else {
        waiting = Installed { tag: g.installer_tag.clone(), ..g.clone() };
        &waiting
    };
    let os_g = if g.proton { "windows" } else { assets::os_now() };
    let pattern = entry.and_then(|e| e.get("assets")).and_then(|a| a.get(os_g)).and_then(Value::as_str);
    let builds = |r: &Release| assets::pick(&r.assets, os_g, assets::arch_now(), pattern).is_some();
    if let Some(t) = entry.and_then(catalog_tag) {
        let at = |tag: &str| rels.iter().position(|r| same_tag(&r.tag_name, tag));
        if let Some(i) = at(&t) {
            let r = &rels[i];
            let newer = at(&g.tag).map(|j| i < j).unwrap_or_else(|| newer_than_gone(r, g));
            return (!same_tag(&r.tag_name, &g.tag) && newer && builds(r)).then(|| r.tag_name.clone());
        }
    }
    let r = catalog::latest_for(rels, &g.tag)?;
    let known = rels.iter().any(|x| same_tag(&x.tag_name, &g.tag));
    (!same_tag(&r.tag_name, &g.tag) && (known || newer_than_gone(r, g)) && builds(r)).then(|| r.tag_name.clone())
}

/// Whether `r` is newer than the installed release when that one isn't in the list anymore (a
/// nightly its project deleted): by version when both tags are versions, else by date, published
/// after the game was installed. "nightly-680" against "v3.0.0" can't be told by its name, and
/// taking it for older sent Longshot players back to v3.0.0 (#393).
fn newer_than_gone(r: &Release, g: &Installed) -> bool {
    let version = |s: &str| s.trim().trim_start_matches(['v', 'V']).starts_with(|c: char| c.is_ascii_digit());
    if version(&r.tag_name) && version(&g.tag) {
        return !older_version(&r.tag_name, &g.tag);
    }
    let Some(out) = r.published_at.as_deref().and_then(released_secs) else { return false };
    out > g.updated_at as i64
}

/// A release's date ("2026-11-19T02:18:27Z", or with an offset like "+02:00", which a Forgejo
/// server may send) in seconds since 1970.
fn released_secs(s: &str) -> Option<i64> {
    let num = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    let days = days_from_civil(num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, m, sec) = (num(11, 13).unwrap_or(0), num(14, 16).unwrap_or(0), num(17, 19).unwrap_or(0));
    // After the seconds (and any fraction of one): the offset from UTC, ahead of it with "+".
    let zone = s.get(19..).unwrap_or("").trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    let sign = if zone.starts_with('+') { 1 } else if zone.starts_with('-') { -1 } else { 0 };
    let hours = zone.get(1..3).and_then(|x| x.parse::<i64>().ok()).unwrap_or(0);
    let minutes = zone.get(4..6).and_then(|x| x.parse::<i64>().ok()).unwrap_or(0);
    let offset = sign * (hours * 3600 + minutes * 60);
    Some(days * 86400 + h * 3600 + m * 60 + sec - offset)
}

/// The project whose releases a game's Updates tab lists and its update badge and news read, which
/// is also where installing takes them from: the one the catalog names, or the game's own when the
/// catalog doesn't list it. The two differ once a project moves (Hookshot, from GitHub to its own
/// Forgejo server): the installed game still names the old one, which never gets the new releases.
fn listed_repo(g: &Installed, entry: Option<&Value>) -> String {
    let listed = entry.map(|e| str_of(e, "repo")).unwrap_or_default();
    if listed.is_empty() {
        return g.repo.clone();
    }
    listed.to_string()
}

/// What the Updates tab shows for each release, by the same rule as the badge: the update on offer,
/// the release the store has checked, and which releases have a build for this system.
#[tauri::command]
async fn update_info(st: State<'_, App>, id: String) -> Res<Value> {
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let cat = catalog::catalog(&st.http, &st.dir, false).await.map(|f| f.data).unwrap_or(Value::Null);
    let entry = catalog::app(&cat, &id);
    let repo = listed_repo(&g, entry);
    let f = match catalog::releases_on_disk(&st.dir, &repo) {
        Some(f) => f,
        None => catalog::releases(&st.http, &st.dir, &repo, false).await?,
    };
    let rels: Vec<Release> = serde_json::from_value(f.data).unwrap_or_default();
    let os_g = if g.proton { "windows" } else { assets::os_now() };
    let pattern = entry.and_then(|e| e.get("assets")).and_then(|a| a.get(os_g)).and_then(Value::as_str);
    let builds: Vec<&str> = rels.iter().filter(|r| assets::pick(&r.assets, os_g, assets::arch_now(), pattern).is_some()).map(|r| r.tag_name.as_str()).collect();
    Ok(json!({ "offered": offered_update(&g, entry, &rels), "checked": entry.and_then(catalog_tag), "builds": builds }))
}

/// Saves the release a game is on as `tag`, unless it was moved or changed since `g` was read.
fn keep_tag(st: &App, g: &mut Installed, tag: String) {
    let old = g.tag.clone();
    if let Some(x) = st.lib.lock().unwrap().installed.get_mut(&g.id).filter(|x| !x.local && x.repo == g.repo && x.path == g.path) {
        retag(x, &old, &tag);
    }
    st.save();
    retag(g, &old, &tag);
}

/// A game's tag spelled as `new`, with every other field that names the same release (the release
/// its setup finished on, each game's own, the one before, a waiting installer's): left as `old`,
/// an Easy Setup game would read as a new release and build again for nothing.
fn retag(g: &mut Installed, old: &str, new: &str) {
    let fix = |t: &mut String| {
        if same_tag(t, old) {
            *t = new.to_string();
        }
    };
    g.tag = new.to_string();
    fix(&mut g.set_up);
    fix(&mut g.prev_tag);
    fix(&mut g.installer_tag);
    g.games.values_mut().for_each(|p| fix(&mut p.set_up));
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
    // A Flatpak is updated by Flatpak (from wherever it was installed), and a Nix install by Nix,
    // never by the launcher itself.
    if flatpak::sandboxed() || nix_managed() {
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
pub(crate) fn still_running_in(dir: &Path) -> bool {
    program_running_in(dir).is_some()
}

/// The file name of a program running from this folder (or deeper), as `still_running_in` finds
/// it: so a message can say what to close, when it isn't the game itself (a mod tool, say).
pub(crate) fn program_running_in(dir: &Path) -> Option<String> {
    if dir.components().count() < 3 {
        return None;
    }
    let me = std::process::id();
    #[cfg(windows)]
    {
        let d = dir.to_string_lossy().trim_end_matches('\\').to_lowercase() + "\\";
        let inside = |pid: u32| win::process_file(pid).map(|f| f.to_string_lossy().to_lowercase().starts_with(&d)).unwrap_or(false);
        return win::processes().into_iter().find(|(pid, name)| *pid != me && *pid != 0 && !leftover_helper(name) && inside(*pid)).map(|(_, name)| name);
    }
    #[cfg(target_os = "linux")]
    {
        let rd = std::fs::read_dir("/proc").ok()?;
        let pids = rd.flatten().filter_map(|e| e.file_name().to_str().and_then(|n| n.parse::<u32>().ok())).filter(|&p| p != me);
        for p in pids {
            let Ok(exe) = std::fs::read_link(format!("/proc/{p}/exe")) else { continue };
            let name = exe.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if exe.starts_with(dir) && !leftover_helper(&name) {
                return Some(name);
            }
        }
        return None;
    }
    #[allow(unreachable_code)]
    {
        let _ = me;
        None
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
    let version = version.trim_start_matches('v');
    if version.is_empty() {
        return false;
    }
    // NSIS surrounds each version value with UTF-16 nulls (the key's terminator/padding, then
    // the value's terminator). Match the whole value so beta.410 cannot pass for beta.41.
    let want: Vec<u8> = format!("\0{version}\0").encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    bytes.windows(want.len()).any(|w| w == want.as_slice())
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

/// How far along a pre-release's label is: an rc.1 is past a beta.5 whatever their numbers.
/// A label we don't know sits with beta, so it's never taken for older than an alpha or newer
/// than an rc.
fn prerelease_stage(label: &str) -> u64 {
    let label = label.to_ascii_lowercase();
    let starts = |prefixes: &[&str]| prefixes.iter().any(|p| label.starts_with(p));
    if starts(&["dev", "nightly", "snapshot"]) {
        return 0;
    }
    if starts(&["alpha"]) {
        return 1;
    }
    // Before "pre": "preview" starts with it.
    if starts(&["beta", "preview"]) {
        return 2;
    }
    if starts(&["rc", "pre"]) {
        return 3;
    }
    2
}

/// 0.5.0-beta.1 < 0.5.0-beta.2 < 0.5.0-rc.1 < 0.5.0 < 0.5.1: a pre-release comes before its release.
fn version_key(s: &str) -> (u64, u64, u64, u64, u64, u64, u64, u64) {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let (core, pre) = s.split_once('-').unwrap_or((s, ""));
    // Each part's leading digits ("10a" is 10).
    let mut n = core.split('.').map(|p| p.trim().chars().take_while(char::is_ascii_digit).collect::<String>().parse::<u64>().unwrap_or(0));
    let (a, b, c, d) = (n.next().unwrap_or(0), n.next().unwrap_or(0), n.next().unwrap_or(0), n.next().unwrap_or(0));
    if pre.is_empty() {
        return (a, b, c, d, 1, 0, 0, 0);
    }
    // beta.3.1: 3, then 1.
    let mut nums = pre.split(|ch: char| !ch.is_ascii_digit()).filter(|p| !p.is_empty()).map(|p| p.parse::<u64>().unwrap_or(0));
    (a, b, c, d, 0, prerelease_stage(pre), nums.next().unwrap_or(0), nums.next().unwrap_or(0))
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
    news_for(&st).await
}

async fn news_for(st: &App) -> Res<Vec<News>> {
    let games: Vec<Installed> = st.lib.lock().unwrap().installed.values().cloned().collect();
    let cat = catalog::catalog(&st.http, &st.dir, false).await.map(|f| f.data).unwrap_or(Value::Null);
    let cutoff = chrono::Utc::now() - chrono::Duration::days(60);
    let mut out = vec![];
    // From the release info on disk only (the update check keeps it current, for the project the
    // catalog names).
    for g in games.into_iter().filter(|g| !g.repo.is_empty() && !g.local) {
        let repo = listed_repo(&g, catalog::app(&cat, &g.id));
        let Some(f) = catalog::releases_on_disk(&st.dir, &repo) else { continue };
        let Ok(rels) = serde_json::from_value::<Vec<Release>>(f.data) else { continue };
        for r in rels.iter().filter(|r| !r.draft).take(3) {
            let date = r.published_at.clone().unwrap_or_default();
            let when = chrono::DateTime::parse_from_rfc3339(&date).map(|d| d.with_timezone(&chrono::Utc)).ok();
            if when.map(|w| w < cutoff).unwrap_or(true) {
                continue;
            }
            out.push(news_item(&g, r));
        }
    }
    out.sort_by(|a, b| b.date.cmp(&a.date));
    out.truncate(10);
    Ok(out)
}

/// A release of an installed game, as news.
fn news_item(g: &Installed, r: &Release) -> News {
    let notes: String = r.body.clone().unwrap_or_default().chars().take(1200).collect();
    News {
        id: g.id.clone(),
        name: g.name.clone(),
        tag: r.tag_name.clone(),
        title: r.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| r.tag_name.clone()),
        date: r.published_at.clone().unwrap_or_default(),
        notes,
        url: r.html_url.clone().unwrap_or_default(),
        // The installed tag can lack the release's "v" (imported from another launcher).
        have: same_tag(&r.tag_name, &g.tag),
    }
}

/// About the original game, from Wikipedia.
#[tauri::command]
async fn game_info(st: State<'_, App>, id: String) -> Res<Option<info::Info>> {
    let cat = catalog::catalog(&st.http, &st.dir, false).await.map(|f| f.data).unwrap_or(Value::Null);
    // One game of several: about that game, not the one its recomp's entry names.
    let pack = games::split(&id).filter(|_| catalog::app(&cat, &id).is_none());
    let Some(a) = catalog::app(&cat, pack.map_or(id.as_str(), |(r, _)| r)) else { return Ok(None) };
    let game = match pack {
        Some((_, sub)) => game_of(&st, &id).map(|p| p.game.name).or_else(|| games::of(None, a).into_iter().find(|g| g.id == sub).map(|g| g.name)),
        None => None,
    };
    let game = game.unwrap_or_else(|| str_of(a, "game").to_string());
    let console = cat
        .get("origins")
        .and_then(Value::as_array)
        .and_then(|o| o.iter().find(|x| x.get("id").and_then(Value::as_str) == Some(str_of(a, "origin"))))
        .map(|o| str_of(o, "name").to_string())
        .unwrap_or_default();
    Ok(info::game_info(&st.http, &st.dir, &game, &console).await)
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
    let code = r.status().as_u16();
    let v: Value = r.json().await.unwrap_or(Value::Null);
    if let Some(e) = usage::sign_in_needed(code, &v) {
        return Err(e);
    }
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
    Ok(programs_in(&g))
}

/// Start With, in the game's "…" menu and Big Picture's Options sheet: the program Play starts by
/// the game's recomplauncher.json, and the one the player picked; a game without one offers every
/// program in its folder, as before (#464).
#[tauri::command(async)]
fn start_with_programs(st: State<'_, App>, id: String) -> Res<Vec<String>> {
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    // Only a program the player picked in the game's settings (as picked_program reads it), not the
    // one the launcher found for itself: that can be the bare game a start script wraps.
    let picked = g.exe.as_ref().filter(|_| g.exe_rules == u8::MAX).map(|e| if e.is_absolute() { e.clone() } else { g.path.join(e) });
    let named = profile_of(&st, &id, &g).and_then(|p| p.start_with(&g.path, picked.as_deref()));
    Ok(named.unwrap_or_else(|| programs_in(&g)))
}

/// Every program in a game's folder, three folders down at most, relative to it.
fn programs_in(g: &Installed) -> Vec<String> {
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
                return out;
            }
        }
    }
    out
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

/// The PC, the installed games and a few settings, as launcher-and-pc.txt: in a report with
/// "Include my PC and launcher info" on, and in the logs zip.
pub(crate) fn launcher_and_pc(st: &App, pc: &usage::PcInfo) -> String {
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
    txt
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
        form.push(("name[]".into(), "launcher-and-pc.txt".into()));
        form.push(("text[]".into(), launcher_and_pc(&st, &pc)));
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
    let code = r.status().as_u16();
    let v: Value = r.json().await.unwrap_or(Value::Null);
    if let Some(e) = usage::sign_in_needed(code, &v) {
        return Err(e);
    }
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
    if nix_managed() {
        return Err(Error::Msg("RecompLauncher updates through Nix. Update the flake input (nix flake update) or nixpkgs, then rebuild.".into()));
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
        if let Err(e) = mac_update::start_and_watch(&bundle, &st.dir) {
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
    // Looked up before the library is locked: none of it needs it, and a slow drive or the
    // registry shouldn't hold up everything else that waits on the library.
    let steam = shortcuts::steam_dir().is_some();
    let deck = shortcuts::is_steam_deck();
    let pictures = ingame::shots_dir("").parent().map(|p| p.to_string_lossy().to_string());
    let moving = st.moving.lock().unwrap().clone();
    // Games whose folder isn't there right now stay in the Library, marked missing. Their folders
    // are looked at without the lock too: one on a drive that's gone can take seconds to answer.
    let folders: Vec<(String, PathBuf)> = st.lib.lock().unwrap().installed.values().filter(|g| g.installer_setup.is_empty() || g.exe.is_some()).map(|g| (g.id.clone(), g.path.clone())).collect();
    let missing: Vec<String> = folders.into_iter().filter(|(id, p)| !p.is_dir() && !moving.contains(id)).map(|(id, _)| id).collect();
    let l = st.lib.lock().unwrap();
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
        "removedPlayHistory": l.removed_play_history,
        "arts": l.arts,
        "settings": l.settings,
        "roms": l.roms,
        "running": running,
        "jobs": jobs,
        "setupJobs": SETUP_STATUS.lock().unwrap().clone().unwrap_or_default(),
        "steam": steam,
        "pictures": pictures,
        "deck": deck,
        "bigArg": std::env::args().any(|a| a == "--bigpicture"),
        // Started at sign-in with Big Picture mode at start: the page waits in the desktop view
        // while the window is hidden, and Big Picture opens with the window (see show_main).
        "desktopArg": std::env::args().any(|a| a == "--desktop") || TV_LATER.load(Ordering::Relaxed),
        "canStartAtLogin": autostart::supported(),
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
    let discord_was = l.settings.discord;
    // The saves folder changes only through its own button (the copies move with it).
    let save_dir = l.settings.save_dir.clone();
    // The proxy changes only through its own command (its password is sealed there).
    let proxy = (l.settings.proxy.clone(), l.settings.proxy_url.clone(), l.settings.proxy_user.clone(), l.settings.proxy_pass.clone());
    // Start with your PC changes only through its own command (the system's list changes with it).
    let start_at_login = l.settings.start_at_login;
    l.settings = settings;
    l.settings.tray_told |= told;
    l.settings.save_dir = save_dir;
    l.settings.start_at_login = start_at_login;
    (l.settings.proxy, l.settings.proxy_url, l.settings.proxy_user, l.settings.proxy_pass) = proxy;
    pads::STICK.store(l.settings.pad_stick, Ordering::Relaxed);
    saves::use_chosen_device(&l.settings.ui);
    let discord_now = l.settings.discord;
    l.save(&st.dir)?;
    drop(l);
    // The Discord status turned on or off: "RecompLauncher" shows or goes with it.
    if discord_now != discord_was {
        discord_idle(&st, discord_now);
    }
    Ok(())
}

/// Settings → Start with your PC. The system's own startup list changes first, and the switch
/// keeps what the system took: a Flatpak's desktop or a Mac can ask the player, who can say no.
#[tauri::command(async)]
fn set_start_at_login(st: State<'_, App>, on: bool) -> Res<bool> {
    let now = autostart::set(on, &st.dir).map_err(Error::Msg)?;
    let mut l = st.lib.lock().unwrap();
    l.settings.start_at_login = now;
    l.save(&st.dir)?;
    Ok(now)
}

/// Since when the launcher has been open, for the Discord status.
static OPEN_SINCE: std::sync::OnceLock<u64> = std::sync::OnceLock::new();

/// "RecompLauncher" in the Discord status while no game runs (or nothing, when it's off).
fn discord_idle(st: &App, on: bool) {
    let app_id = discord_info(st, "").0;
    discord::send(discord::Msg::Idle(on.then(|| (app_id, *OPEN_SINCE.get_or_init(now)))));
}

#[tauri::command]
fn list_monitors(app: AppHandle) -> Res<Vec<display::MonitorInfo>> {
    Ok(display::enumerate_monitors(&app))
}

#[tauri::command]
fn prepare_tv_mode(app: AppHandle, monitor_id: Option<String>) -> Res<display::MonitorInfo> {
    display::prepare_tv_mode(&app, monitor_id).map_err(Error::Msg)
}

#[tauri::command]
fn leave_tv_mode(app: AppHandle) -> Res<()> {
    display::leave_tv_mode(&app).map_err(Error::Msg)
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
/// Drive, OneDrive or Dropbox keeps in sync), or none for beside each game in its library. The
/// copies already made move along, each checked before the old one goes.
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
    let installed: Vec<Installed> = st.lib.lock().unwrap().installed.values().cloned().collect();
    let moved = saves::move_all(&st.dir, &installed, to.as_deref()).map_err(|e| Error::Msg(format!("Couldn't move the copies there: {e}")))?;
    saves::set_root(to.clone());
    {
        let mut l = st.lib.lock().unwrap();
        l.settings.save_dir = to.clone();
        l.save(&st.dir)?;
    }
    trail::note(format!("Saves: copies go to {} now ({moved} folders moved)", if to.is_some() { "the player's folder" } else { "each game's library" }));
    Ok(json!({ "path": cloud::shown_root(&st), "custom": to.is_some(), "moved": moved }))
}

/// Opens the folder the copies of the player's saves are in.
#[tauri::command(async)]
fn saves_open_root(app: AppHandle, st: State<'_, App>) -> Res<()> {
    let p = cloud::shown_root(&st);
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
    webview_crash::watch(&w);
    Ok(w)
}

/// How the main window was showing, so a window made again after its engine died shows the same.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shown {
    /// Closed to the tray, or hidden while a game starts: it stays out of sight.
    Hidden,
    Minimized,
    Open,
}

/// The main window made again, after its web engine died (see webview_crash). Only the page starts
/// over: downloads, a game being played and the tray go on as they were.
pub(crate) async fn remake_main(app: &AppHandle, shown: Shown) {
    webview_crash::set_main_gone(true);
    if let Some(old) = app.get_webview_window("main") {
        let _ = old.destroy();
    }
    // The name "main" is free only once the old window is really gone.
    for _ in 0..50 {
        if app.get_webview_window("main").is_none() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let Some(mut config) = app.config().app.windows.iter().find(|c| c.label == "main").cloned() else { return };
    // Never in front of a game being played: it comes back minimized then, and opens when the game
    // ends.
    let playing = !app.state::<App>().running.lock().unwrap().is_empty();
    let shown = if playing && shown == Shown::Open {
        webview_crash::set_minimized_for_play();
        Shown::Minimized
    } else {
        shown
    };
    config.focus = shown == Shown::Open;
    UI_READY.store(false, Ordering::Relaxed);
    // Windows are made on the main thread.
    let (tx, rx) = tokio::sync::oneshot::channel();
    let a = app.clone();
    let _ = app.run_on_main_thread(move || {
        let made = tauri::WebviewWindowBuilder::from_config(&a, &config).and_then(|b| b.build());
        let _ = tx.send(made);
    });
    let w = match rx.await {
        Ok(Ok(w)) => w,
        Ok(Err(e)) => {
            trail::note(format!("Window: couldn't be made again ({e})"));
            return;
        }
        Err(_) => return,
    };
    webview_crash::set_main_gone(false);
    webview_crash::watch(&w);
    restore_window_place(&w, &app.state::<App>().dir);
    apply_saved_zoom(app, &w);
    if shown == Shown::Hidden {
        rest(&w, true);
        return;
    }
    wait_for_ui().await;
    let _ = w.show();
    if shown == Shown::Minimized {
        let _ = w.minimize();
        return;
    }
    // It was in Big Picture: the new page goes back to it.
    if display::in_big_picture() {
        let _ = w.emit("bigpicture", true);
    }
}

/// Text Size from last time.
fn apply_saved_zoom(app: &AppHandle, w: &tauri::WebviewWindow) {
    let z = app.state::<App>().lib.lock().unwrap().settings.ui.get("ui_scale").and_then(Value::as_f64).filter(|z| (0.5..=2.0).contains(z));
    if let Some(z) = z {
        let _ = w.set_zoom(z);
    }
}

/// The main window is out of sight (minimized, or closed to the tray).
static AWAY: AtomicBool = AtomicBool::new(false);

/// Started at sign-in by the clock, set to start in Big Picture mode: it opens with the window.
static TV_LATER: AtomicBool = AtomicBool::new(false);

/// Set by the page once its first real screen is drawn. The window stays hidden until then, so the
/// player never sees the empty first draw being replaced by the real one.
static UI_READY: AtomicBool = AtomicBool::new(false);

/// The start window shows at least this long once it's up, so its logo has time to build.
const SPLASH_MIN: std::time::Duration = std::time::Duration::from_millis(900);

/// The small start window: the logo in the middle of the screen while the launcher gets ready.
fn open_splash(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    tauri::WebviewWindowBuilder::new(app, "splash", tauri::WebviewUrl::App("splash.html".into()))
        .title("RecompLauncher")
        .inner_size(420.0, 280.0)
        .resizable(false)
        .decorations(false)
        .center()
        .skip_taskbar(true)
        .focused(false)
        .background_color(tauri::window::Color(11, 18, 32, 255))
        .build()
        .inspect_err(|e| trail::note(format!("Start window: {e}")))
        .ok()
}

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

/// A shortcut asking to play a game whose project added setup options since its setup: the window
/// opens on the game with the question (turn it on, or Not now and play as it is) instead.
pub(crate) fn options_first(app: &AppHandle, id: &str) -> bool {
    if new_prepare_options(&app.state::<App>(), id).is_empty() {
        return false;
    }
    *OPTIONS_ASKED.lock().unwrap() = Some(id.to_string());
    show_main(app);
    let _ = app.emit("options-new", id.to_string());
    true
}

/// The game a shortcut wanted to play while it had new options, for a window that opened after the
/// news went out.
static OPTIONS_ASKED: Mutex<Option<String>> = Mutex::new(None);

#[tauri::command(async)]
fn take_options_asked() -> Option<String> {
    OPTIONS_ASKED.lock().unwrap().take()
}

// ---------- the window's size and place, kept for next time ----------

static WINDOW_GEN: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
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
        // But it moves to the screen the window is maximized on: moved there while maximized
        // (Win+Shift+arrow, or dragged), it opens on that screen next time, like any window.
        if maximized {
            if let Ok(Some(screen)) = w.current_monitor() {
                place = normal_place_on(place, &monitor_rect(&screen), screen.work_area());
            }
        }
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

/// Put the window back where it was last time, if that's still on a screen. Otherwise (the first
/// start, or that screen is gone) it stays where it opened, made smaller if it doesn't fit there.
fn restore_window_place(w: &tauri::WebviewWindow, dir: &Path) {
    let kept = std::fs::read(dir.join("window.json")).ok().and_then(|b| serde_json::from_slice::<WindowPlace>(&b).ok());
    let kept = kept.filter(|p| p.w >= 400 && p.h >= 300);
    let place = kept.as_ref().and_then(|p| kept_place_on_a_screen(w, p)).or_else(|| first_place(w));
    if let Some(place) = place {
        // Place first, size second. The window starts on the main screen, and moving it onto a
        // screen with another scale (100% and 150%, say) makes Windows rescale it: a size set
        // before the move grew with it, past the screen, and was kept that way for next time.
        let _ = w.set_position(tauri::PhysicalPosition::new(place.x, place.y));
        let _ = w.set_size(tauri::PhysicalSize::new(place.w, place.h));
    }
    if kept.is_some_and(|p| p.maximized) {
        let _ = w.maximize();
    }
}

/// The kept place fitted to the screen its title bar is on, so it can always be grabbed. None
/// when no screen has it any more.
fn kept_place_on_a_screen(w: &tauri::WebviewWindow, place: &WindowPlace) -> Option<WindowPlace> {
    let monitors = w.available_monitors().ok()?;
    let screen = monitors.iter().find(|m| title_bar_within(place, &monitor_rect(m)))?;
    Some(fit_on_screen(place, screen.work_area()))
}

/// The window as it first opens (the size in tauri.conf.json, centred on the main screen), made
/// to fit when that screen is smaller: 1360x860 at 150% is more than a 1080p laptop shows. None
/// when it fits, so it stays exactly as it opened.
fn first_place(w: &tauri::WebviewWindow) -> Option<WindowPlace> {
    let screen = w.current_monitor().ok().flatten()?;
    let size = w.outer_size().ok()?;
    let area = screen.work_area();
    let fits = size.width <= area.size.width && size.height <= area.size.height;
    if fits {
        return None;
    }
    Some(centred_on_screen(size.width, size.height, area))
}

fn monitor_rect(m: &tauri::Monitor) -> tauri::PhysicalRect<i32, u32> {
    tauri::PhysicalRect { position: *m.position(), size: *m.size() }
}

/// Whether the window's top middle, where the title bar is, is inside the rect.
fn title_bar_within(place: &WindowPlace, rect: &tauri::PhysicalRect<i32, u32>) -> bool {
    let (x, y) = (place.x + place.w as i32 / 2, place.y + 20);
    let (left, top) = (rect.position.x, rect.position.y);
    let right = left + rect.size.width as i32;
    let bottom = top + rect.size.height as i32;
    x >= left && x < right && y >= top && y < bottom
}

/// A window centred on a screen's free area, at most nine tenths of it each way, so it still
/// looks like a window and not like a maximized one.
fn centred_on_screen(w: u32, h: u32, area: &tauri::PhysicalRect<i32, u32>) -> WindowPlace {
    let w = w.min(area.size.width / 10 * 9);
    let h = h.min(area.size.height / 10 * 9);
    let x = area.position.x + ((area.size.width - w) / 2) as i32;
    let y = area.position.y + ((area.size.height - h) / 2) as i32;
    WindowPlace { x, y, w, h, maximized: false }
}

/// The size a maximized window goes back to, kept on the screen it's maximized on: where it was
/// if that's this screen, otherwise centred on it.
fn normal_place_on(place: WindowPlace, screen: &tauri::PhysicalRect<i32, u32>, area: &tauri::PhysicalRect<i32, u32>) -> WindowPlace {
    if title_bar_within(&place, screen) {
        return place;
    }
    WindowPlace { maximized: place.maximized, ..centred_on_screen(place.w, place.h, area) }
}

/// The kept place made to fit a screen's free area (the screen without its taskbar): never
/// bigger than it, and moved in only as far as needed for all of it to show. A size kept by an
/// older launcher can be wider than both screens together, and would come back that way forever.
fn fit_on_screen(place: &WindowPlace, area: &tauri::PhysicalRect<i32, u32>) -> WindowPlace {
    let w = place.w.min(area.size.width);
    let h = place.h.min(area.size.height);
    let right_most = area.position.x + (area.size.width - w) as i32;
    let bottom_most = area.position.y + (area.size.height - h) as i32;
    let x = place.x.clamp(area.position.x, right_most);
    let y = place.y.clamp(area.position.y, bottom_most);
    WindowPlace { x, y, w, h, maximized: place.maximized }
}

fn show_main(app: &AppHandle) {
    app.state::<App>().headless.store(false, Ordering::Relaxed);
    // Its web engine kept dying and it wasn't made again (see webview_crash): the player asks for
    // it now, so one more try.
    if app.get_webview_window("main").is_none() && webview_crash::main_gone() {
        let a = app.clone();
        tauri::async_runtime::spawn(async move { remake_main(&a, Shown::Open).await });
        return;
    }
    if let Some(w) = app.get_webview_window("main") {
        if w.is_minimized().unwrap_or(false) {
            let _ = w.unminimize();
        }
        let _ = w.show();
        let _ = w.set_focus();
        discord::send(discord::Msg::Shown(true));
        if AWAY.swap(false, Ordering::Relaxed) {
            rest(&w, false);
            let _ = w.emit("away", false);
        }
        if TV_LATER.swap(false, Ordering::Relaxed) {
            let _ = app.emit("bigpicture", true);
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

/// Installed from the Nix store (the repo's flake, or nixpkgs): the store is read-only and Nix
/// updates it, so the launcher never offers or runs an update itself, like a Flatpak.
pub(crate) fn nix_managed() -> bool {
    cfg!(target_os = "linux") && exe_dir().is_some_and(|d| d.starts_with("/nix/store"))
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
    // First, before any thread starts: the test control token leaves the environment here, so
    // games and tools the launcher starts never see it.
    test_control::take_from_env();
    #[cfg(windows)]
    say_why_it_stopped();
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
                    if options_first(&app, &rid) {
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
                // Close to tray hides the window only when the tray can bring it back.
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    let app = w.app_handle();
                    let background = app.state::<App>().lib.lock().unwrap().settings.close_to_tray;
                    api.prevent_close();
                    if background && app.tray_by_id("main").is_some() && w.hide().is_ok() {
                        away(w, true);
                        discord::send(discord::Msg::Shown(false));
                        tray::hid(app);
                        let jobs = SETUP_STATUS.lock().unwrap().clone();
                        if let Some(m) = jobs.as_ref() {
                            if !m.is_empty() {
                                let count = m.len();
                                let msg = if count == 1 {
                                    let (id, ev) = m.iter().next().unwrap();
                                    let pct = (ev.pct * 100.0).round() as u32;
                                    let name = app.state::<App>().lib.lock().unwrap().installed.get(id.as_str()).map(|g| g.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| id.clone());
                                    format!("{name} is still setting up ({pct}%). RecompLauncher keeps going in the tray.")
                                } else {
                                    format!("{count} recomps are still setting up. RecompLauncher keeps going in the tray.")
                                };
                                let _ = tauri_plugin_notification::NotificationExt::notification(app)
                                    .builder()
                                    .title("RecompLauncher")
                                    .body(msg)
                                    .show();
                            }
                        }
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
                        // However the window came back, a focused window is one the player sees.
                        discord::send(discord::Msg::Shown(true));
                    }
                }
                tauri::WindowEvent::Resized(_) => {
                    let hidden = w.is_minimized().unwrap_or(false) || !w.is_visible().unwrap_or(true);
                    away(w, hidden);
                    // Minimized isn't being used: Discord shows the launcher only while it's open.
                    discord::send(discord::Msg::Shown(!hidden));
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
            // An update the launcher was closed in the middle of: finished, or undone, before
            // anything can start its game.
            for g in lib.installed.values() {
                if let Some(what) = swap::recover(&g.path, Some(g)) {
                    trail::note(format!("Update {}: {what}", g.id));
                }
            }
            flatpak::forget_old_unpacked();
            if art::trim_saved_logos(&dir, &mut lib) {
                let _ = lib.save(&dir);
            }
            // Save copies in the player's own folder, when they picked one, else beside each game in
            // its library. Copies kept in the launcher's folder before go there (once: they're
            // moved), and any made beside a game while the player's folder was away go to it.
            saves::set_root(lib.settings.save_dir.clone());
            {
                let d = dir.clone();
                let installed: Vec<Installed> = lib.installed.values().cloned().collect();
                std::thread::spawn(move || saves::gather(&d, &installed));
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
            // Start with your PC follows the system's own startup list: the player may have turned
            // it off (or on) there since.
            let at_login = autostart::refresh(lib.settings.start_at_login, &dir);
            if at_login != lib.settings.start_at_login {
                lib.settings.start_at_login = at_login;
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
            saves::use_chosen_device(&lib.settings.ui);
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
            if let Some(w) = app.get_webview_window("main") {
                apply_saved_zoom(app.handle(), &w);
                webview_crash::watch(&w);
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
            // Test control, for testing a build with an agent (off unless asked for: see
            // test_control). Not for a game's shortcut start, which has no window to drive.
            if play.is_none() {
                test_control::start(app.handle());
            }
            // Discord: "RecompLauncher" while the launcher's window is open and no game runs.
            if g_discord(&app.state::<App>()) {
                discord_idle(&app.state::<App>(), true);
            }
            // Games set up by an older launcher keep counting as set up: their fingerprints are
            // rewritten as v2 before anyone presses Play.
            {
                let h = app.handle().clone();
                std::thread::spawn(move || {
                    let st = h.state::<App>();
                    upgrade_setups(&st);
                });
            }
            // Sessions the launcher didn't see to the end (it closed or crashed before their saves
            // were copied) are finished first, then save copies that didn't reach the cloud.
            {
                let h = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    cloud::finish_open_sessions(&h).await;
                    cloud::upload_pending(&h).await;
                });
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
                        if options_first(&handle, &rid) {
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
                    // Started at sign-in (Settings → Start with your PC): the window waits by the
                    // clock until it's opened from there. With no tray icon to open it from, it
                    // shows as usual.
                    let at_login = std::env::args().any(|a| a == autostart::ARG) && app.tray_by_id("main").is_some();
                    if at_login {
                        TV_LATER.store(tv, Ordering::Relaxed);
                        discord::send(discord::Msg::Shown(false));
                        if let Some(w) = app.get_webview_window("main") {
                            away(&w.as_ref().window(), true);
                        }
                        return Ok(());
                    }
                    let monitor_id = app.state::<App>().lib.lock().unwrap().settings.tv_monitor.clone();
                    // While the window gets ready: a small one in the middle of the screen with the
                    // logo. Big Picture mode has its own start animation, so not there.
                    let splash = if tv { None } else { open_splash(app.handle()) };
                    let opened = Instant::now();
                    tauri::async_runtime::spawn(async move {
                        wait_for_ui().await;
                        let Some(w) = handle.get_webview_window("main") else {
                            if let Some(s) = &splash {
                                let _ = s.close();
                            }
                            return;
                        };
                        if tv {
                            match display::prepare_tv_mode(&handle, Some(monitor_id)) {
                                Ok(_) => {}
                                Err(e) => trail::note(format!("Big Picture startup aborted: {e}")),
                            }
                        }
                        // Long enough for the logo to read, never longer than the window takes.
                        if let Some(rest) = splash.as_ref().and_then(|_| SPLASH_MIN.checked_sub(opened.elapsed())) {
                            tokio::time::sleep(rest).await;
                        }
                        let _ = w.show();
                        discord::send(discord::Msg::Shown(true));
                        // Sized while it was still hidden, it counted as out of sight: shown now.
                        if AWAY.swap(false, Ordering::Relaxed) {
                            rest(&w, false);
                            let _ = w.emit("away", false);
                        }
                        if let Some(s) = splash {
                            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                            let _ = s.close();
                        }
                    });
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            take_link, ui_ready, pause, download_limits, installer::installer_run, installer::installer_cancel, installer::installer_check, installer::installer_done,
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
            take_options_asked,
            get_state, get_catalog, get_releases, net_state, check_updates, launcher_update, install, cancel, uninstall, play, stop, set_exe, forget_program, set_setup_mode, open_folder, open_setup_log,
            save_settings, set_start_at_login, list_monitors, prepare_tv_mode, leave_tv_mode, open_url, libraries, move_game, game_sizes, game_size, backups, backup_now, restore_backup, pc_check, set_ui_scale, update_retry, ge_proton_install, proton_now, repo_readme, install_vcredist, scan_roms, rom_matches, copy_files, support_local,
            desktop_shortcut, refresh_shortcut_art, open_app_control, fs_paste, portable_info, make_portable, add_to_steam, launcher_to_steam, steam_sync::steam_sync_now, steam_sync::steam_sync_state, steam_sync::steam_remove_all, recent_log, screenshots, open_shots, overlay_action, overlay_state, is_handheld, skip_backup, set_cover, art::art_source, art::art_open, art::art_save, art::art_search, art::art_for, launchbox::launchbox_find, launchbox::launchbox_search, launchbox::launchbox_pictures, all_screenshots, delete_screenshot,
            copy_screenshot, show_screenshot, controllers, pads_window_off, pad_right_stick, prefetch_images, versions, build_check, news, game_info, mods_state, mods_online,
            mod_install, mod_remove, open_mods, send_report, beta_feedback, logs_zip::export_logs, game_programs, start_with_programs, run_program, game_root, boot::boot_builtin, update_launcher, resume_game, game_setup, set_game_copy, free_up_space, set_update_copy, set_game_setting, set_game_slider, set_game_binding, reset_game_bindings, usage::report_preview, usage::profile_report, usage::my_reports, usage::report_reply, usage::review_delete,
            recomp_games, adopt::move_listing, update_info, set_proxy, umu_repair, dlss_fetch, dlss_set, dlss_state, dlss_remove, cloud::library_pull, cloud::library_push, cloud::playtime_push, cloud::playtime_pull, cloud::playtime_reset, cloud::spons_mine, cloud::spons_seen, cloud::home_layout, cloud::saves_open_backups, game_action, setup_copy, setup_cancel, setup_status, setup_begin, setup_end, setup_input_set, setup_key_remove, setup_inputs_run, easy_setup, set_game_options,
            set_prepare_opts,
            convert_plan,
            convert_copy,
            convert_cancel, looks_info, set_look, look_picture, ui_data,
            reset_settings, tray::tray_words, tray::quit_launcher, ach::ach_access, ach::ach_status, ach::ach_apply, ach::ach_profile, ach::ach_games, ach::ach_board, ach::ach_game_web, ach::ach_report, ach::ach_tracker_get, ach::ach_game, ach::ach_set_disc, ach::ach_set_folder, ach::ach_sound, ach::ach_log, ach::ach_peek, ach::ach_rule, ach::ach_reset, ach::ach_test_toast, ach::ach_toast_take, ach::ach_toast_done, cloud::account_start, cloud::account_poll, cloud::account_cancel, cloud::account_state, cloud::account_signout,
            cloud::saves_check, cloud::saves_pick, cloud::saves_play_without_cloud, cloud::saves_cloud, cloud::saves_cloud_follow_all, download_history, report_counts, play_reports, want_count, cloud::saves_info, cloud::saves_status, cloud::saves_peek, cloud::saves_upload_now, cloud::cloud_retry, cloud::saves_overview, cloud::saves_backup, cloud::saves_restore, cloud::saves_cloud_delete, cloud::saves_share, cloud::saves_set_place, cloud::saves_set_override, cloud::saves_add_place, cloud::saves_remove_place, cloud::saves_open, saves_set_root, saves_open_root, missing_files, missing_made, dll_check, dll_aside, gpu_choice, prefer_fast_gpu, copy_check, wine_tools, deck_scores, latest_counts, team_notes, trending, ig_info, ig_view, usage::pc_specs, usage::recomp_request,
            adopt::adopt_scan, adopt::migrate_scan, adopt::migrate_pick, adopt::adopt_pick, adopt::adopt_add, adopt::adopt_check, adopt::set_project, usage::usage_hello, usage::usage_event, usage::data_ids, usage::reset_ids, ping::usage_open, ping::usage_mode, usage::run_report, cloud::inbox, cloud::inbox_done, cloud::betas, cloud::beta_join, cloud::beta_leave, launcher_notes, scan_info, devtools::dev_add, devtools::add_any, devtools::dev_check, devtools::dev_starter, fs_places, fs_list, fs_extract, gb_mods, gb_mod, gb_install
        ])
        .build(tauri::generate_context!())
        .expect("RecompLauncher couldn't start")
        .run(|app, ev| {
            match ev {
                #[cfg(target_os = "macos")]
                // Dock and Finder must restore the main window even if a helper window is visible.
                tauri::RunEvent::Reopen { .. } => show_main(app),
                // No window left because the main one's web engine died: the launcher goes on (the
                // window is being made again, or the tray brings it back). A real way out has a code.
                tauri::RunEvent::ExitRequested { code: None, api, .. } if webview_crash::main_gone() => api.prevent_exit(),
                // Every way out (the window, the tray's Quit, --quit) asks here first. An update still
                // on its way then leaves the launcher closed (see update_launcher).
                tauri::RunEvent::ExitRequested { .. } => update_launch::note_quitting(),
                tauri::RunEvent::Exit => {
                    stop_setups();
                    pads::stop();
                    ping::close(app);
                }
                _ => {}
            }
        });
}

#[cfg(test)]
mod tests {

    // Longshot's case: built, its dump freed (Free Up Space), and its setup step names the dump.
    fn freed_game() -> (crate::profile::Profile, crate::library::Installed) {
        let p: crate::profile::Profile = serde_json::from_value(serde_json::json!({
            "format": 1,
            "inputs": [{ "id": "game", "label": "Your copy", "formats": [{ "type": "file" }] }],
            "prepare": { "program": ["build.sh"], "args": ["--game={input:game}"], "keeps_copy": true },
            "start": { "program": ["game.sh"] }
        }))
        .unwrap();
        (p, crate::library::Installed::default())
    }

    fn checked(list: &[(&str, Option<&str>)]) -> (String, String) {
        let results: Vec<(String, Option<String>)> = list.iter().map(|(l, m)| (l.to_string(), m.map(str::to_string))).collect();
        super::dumps_said(&results)
    }

    #[test]
    fn one_disc_says_what_it_matched_as_before() {
        assert_eq!(checked(&[("Disc 1", Some("USA"))]), ("USA".into(), "Matches the verified USA disc".into()));
        assert_eq!(checked(&[("Disc 1", None)]).0, "");
    }

    #[test]
    fn several_discs_are_verified_only_when_every_one_matched() {
        let all = [("Disc 1", Some("USA")), ("Disc 2", Some("USA")), ("Disc 3", Some("USA")), ("Disc 4", Some("USA"))];
        assert_eq!(checked(&all), ("USA".into(), "All 4 discs match the verified USA dumps".into()));
    }

    #[test]
    fn a_bad_first_disc_is_named_even_when_the_last_one_matched() {
        let first_bad = [("Disc 1", None), ("Disc 2", Some("USA")), ("Disc 3", Some("USA")), ("Disc 4", Some("USA"))];
        assert_eq!(checked(&first_bad), ("".into(), "Disc 1 isn't a known clean dump. Discs 2–4 match the USA dumps.".into()));
    }

    #[test]
    fn a_bad_last_disc_is_named() {
        let last_bad = [("Disc 1", Some("USA")), ("Disc 2", Some("USA")), ("Disc 3", Some("USA")), ("Disc 4", None)];
        assert_eq!(checked(&last_bad).1, "Disc 4 isn't a known clean dump. Discs 1–3 match the USA dumps.");
    }

    #[test]
    fn discs_out_of_a_row_and_other_labels_are_listed() {
        assert_eq!(super::discs_named(&["Disc 1", "Disc 3"]), "Discs 1 and 3");
        assert_eq!(super::discs_named(&["Disc 1", "Disc 2", "Disc 4"]), "Discs 1, 2 and 4");
        assert_eq!(super::discs_named(&["Disc A", "The update"]), "Disc A and The update");
    }

    #[test]
    fn play_leaves_out_a_freed_file_and_its_placeholder_still_reads() {
        let (p, g) = freed_game();
        let rows = crate::inputs::rows(&p, &g, &std::env::temp_dir()).unwrap();
        let now = super::without_freed(&p, &rows);
        assert!(now.inputs.is_empty());
        assert_eq!(now.skipped_inputs, vec!["game".to_string()]);
        assert_eq!(crate::inputs::validate(&now), Ok(()));
    }

    #[test]
    fn play_keeps_a_file_that_is_still_picked() {
        let (p, mut g) = freed_game();
        let dump = std::env::temp_dir().join(format!("rl-freed-{}.bin", std::process::id()));
        std::fs::write(&dump, b"dump").unwrap();
        g.setup_inputs.insert("game".into(), dump.clone());
        let rows = crate::inputs::rows(&p, &g, &std::env::temp_dir()).unwrap();
        let now = super::without_freed(&p, &rows);
        assert_eq!(now.inputs.len(), 1);
        assert!(now.skipped_inputs.is_empty());
        let _ = std::fs::remove_file(dump);
    }

    #[test]
    fn launcher_support_counts_named_inputs_without_a_game_block_as_easy_setup() {
        let p: crate::profile::Profile = serde_json::from_value(serde_json::json!({
            "format": 1,
            "inputs": [{ "id": "jar", "label": "Your copy", "role": "game", "formats": [{ "type": "file", "extensions": ["jar"] }] }],
            "start": { "program": ["game.exe"], "args": ["--game", "{input:jar}"] }
        }))
        .unwrap();
        assert_eq!(super::profile_covers(&p)["easy_setup"], true);
    }

    #[test]
    fn launcher_support_does_not_count_optional_named_inputs_alone_as_easy_setup() {
        let p: crate::profile::Profile = serde_json::from_value(serde_json::json!({
            "format": 1,
            "inputs": [{ "id": "dlc", "label": "Extra levels", "role": "dlc", "optional": true }],
            "start": { "program": ["game.exe"] }
        }))
        .unwrap();
        assert_eq!(super::profile_covers(&p)["easy_setup"], false);
    }

    #[test]
    fn launcher_support_does_not_count_a_game_block_without_a_program_as_easy_setup() {
        let p: crate::profile::Profile = serde_json::from_value(serde_json::json!({ "format": 1, "game": { "kind": "folder" } })).unwrap();
        assert_eq!(super::profile_covers(&p)["easy_setup"], false);
    }

    // A game folder holding this recomplauncher.json, for support_in.
    fn folder_with_file(name: &str, file: serde_json::Value) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rl-support-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("recomplauncher.json"), file.to_string()).unwrap();
        dir
    }

    // Mario Tennis Fever's file as released (v0.1.2): `config.file` is one path, not a list, so
    // the launcher can't read the file at all, though every section is there.
    fn unreadable_like_mtf() -> serde_json::Value {
        serde_json::json!({
            "format": 1,
            "game": { "kind": "folder", "check": { "file": "exefs/main" } },
            "start": { "program": ["mtf-recomp.exe"] },
            "saves": ["{install}/profile"],
            "config": [{ "id": "main", "format": "ini", "file": "{install}/mtf.ini" }],
            "settings": [{ "id": "fps", "label": "FPS", "choices": [{ "label": "60", "set": { "FPS_LIMIT": "60" } }] }],
            "prepare": { "program": ["Setup.exe"], "args": ["--prepare", "--game", "{game}"], "done": ["{install}/mtf-recomp.exe"] }
        })
    }

    #[test]
    fn launcher_support_gives_only_the_problem_for_a_file_the_launcher_cant_read() {
        let dir = folder_with_file("unreadable", unreadable_like_mtf());
        let found = super::support_in(&dir);
        let _ = std::fs::remove_dir_all(dir);
        assert_eq!(found, Some(serde_json::json!({ "problem": "\"config[0].file\" should be a list." })));
    }

    #[test]
    fn launcher_support_gives_nothing_for_a_folder_without_a_file() {
        let dir = std::env::temp_dir().join(format!("rl-support-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let found = super::support_in(&dir);
        let _ = std::fs::remove_dir_all(dir);
        assert_eq!(found, None);
    }

    #[test]
    fn launcher_support_counts_every_feature_of_a_complete_file() {
        let mut file = unreadable_like_mtf();
        file["config"][0]["file"] = serde_json::json!([{ "path": "{install}/mtf.ini" }]);
        file["game"]["dumps"] = serde_json::json!([{ "name": "USA", "crc32": "1234abcd" }]);
        let dir = folder_with_file("complete", file);
        let all = serde_json::json!({ "easy_setup": true, "settings": true, "prepare": true, "verified": true, "saves": true });
        assert_eq!(super::support_in(&dir), Some(all));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_catalogs_xbox_360_title_id_is_used_first_and_in_capitals() {
        use super::catalog_title;
        let e = |v: serde_json::Value| catalog_title(&v);
        assert_eq!(e(serde_json::json!({"title_id": "4d5307e6", "boot": {"files": {"id": "454108D2"}}})).as_deref(), Some("4D5307E6"));
        assert_eq!(e(serde_json::json!({"achievements": {"title_id": "4D5307E6"}})).as_deref(), Some("4D5307E6"));
        // A mistyped one doesn't hide a good disc id; none at all is none.
        assert_eq!(e(serde_json::json!({"title_id": "4D53-07E6", "boot": {"files": {"id": "454108D2"}}})).as_deref(), Some("454108D2"));
        assert_eq!(e(serde_json::json!({"id": "a"})), None);
    }

    #[test]
    fn the_updates_tab_reads_the_releases_of_the_project_the_catalog_lists() {
        use super::{listed_repo, Installed};
        // Hookshot moved from GitHub to its own Forgejo server after it was installed. The tab lists
        // the Forgejo releases (v2.3.2 and on), so their builds are looked for there too.
        let g = Installed { id: "hookshot".into(), repo: "NotRexed/Hookshot".into(), tag: "v2.1.0".into(), ..Default::default() };
        let entry = serde_json::json!({ "id": "hookshot", "repo": "forgejo:git.notrexed.dev/Hookshot/Hookshot" });
        assert_eq!(listed_repo(&g, Some(&entry)), "forgejo:git.notrexed.dev/Hookshot/Hookshot");
    }

    #[test]
    fn the_updates_tab_reads_a_games_own_project_when_the_catalog_has_none() {
        use super::{listed_repo, Installed};
        let g = Installed { id: "mine".into(), repo: "someone/own-recomp".into(), ..Default::default() };
        assert_eq!(listed_repo(&g, None), "someone/own-recomp");
    }

    fn tagged_release(tag: &str, prerelease: bool) -> super::Release {
        let file = serde_json::json!({ "name": "game-linux-x64-windows-macos-arm64.zip", "browser_download_url": "https://github.com/a/b/releases/download/t/game.zip", "size": 10 });
        serde_json::from_value(serde_json::json!({ "tag_name": tag, "prerelease": prerelease, "published_at": "2026-11-01T00:00:00Z", "assets": [file] })).unwrap()
    }

    #[test]
    fn the_same_version_with_a_v_isnt_an_update() {
        let g = super::Installed { id: "g".into(), tag: "1.0.0".into(), ..Default::default() };
        let rels = vec![tagged_release("v1.0.0", false)];
        assert_eq!(super::offered_update(&g, None, &rels), None);
    }

    #[test]
    fn a_prerelease_tag_without_its_v_isnt_sent_back_to_stable() {
        let g = super::Installed { id: "g".into(), tag: "2.0.0-beta.1".into(), ..Default::default() };
        let rels = vec![tagged_release("v2.0.0-beta.1", true), tagged_release("v1.9.0", false)];
        assert_eq!(super::offered_update(&g, None, &rels), None);
    }

    #[test]
    fn news_marks_a_release_installed_without_its_v_as_installed() {
        let g = super::Installed { id: "g".into(), tag: "1.0.0".into(), ..Default::default() };
        assert!(super::news_item(&g, &tagged_release("v1.0.0", false)).have);
    }

    #[test]
    fn the_installed_release_is_found_when_its_tag_lacks_the_v() {
        let rels = vec![tagged_release("v1.1.0", false), tagged_release("v1.0.0", false)];
        assert_eq!(super::release_on(&rels, "1.0.0").map(|r| r.tag_name.as_str()), Some("v1.0.0"));
    }

    #[test]
    fn a_taken_down_release_isnt_replaced_by_the_one_installed_without_its_v() {
        let fresh = vec![tagged_release("v1.0.0", false)];
        assert!(super::replacement(&fresh, false, "1.0.0", |_| true).is_none());
    }

    #[test]
    fn reinstalling_the_version_installed_without_its_v_keeps_it_unpinned() {
        assert!(!super::picked_by_player("v1.0.0", "v2.0.0", false, None, ("1.0.0", false)));
    }

    #[test]
    fn a_tag_without_its_v_is_saved_as_the_release_spells_it() {
        let d = std::env::temp_dir().join(format!("rl-exact-tag-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let mut g = Installed { id: "g".into(), repo: "a/b".into(), tag: "1.0.0".into(), ..Default::default() };
        let mut lib = Library::default();
        lib.put("g", g.clone());
        let st = app_in(&d, lib);
        let tag = super::exact_tag(&[tagged_release("v1.0.0", false)], &g.tag).unwrap();
        super::keep_tag(&st, &mut g, tag);
        assert_eq!(st.lib.lock().unwrap().installed["g"].tag, "v1.0.0");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_tag_spelled_like_its_release_isnt_rewritten() {
        assert_eq!(super::exact_tag(&[tagged_release("v1.0.0", false)], "v1.0.0"), None);
    }

    #[test]
    fn a_game_set_up_on_a_tag_without_its_v_isnt_set_up_again_after_the_rewrite() {
        let (d, game) = take_pick_dir("retag");
        let copy = d.join("My Games/Combo");
        std::fs::create_dir_all(&copy).unwrap();
        let mut p: crate::profile::Profile = serde_json::from_value(serde_json::json!({
            "format": 1,
            "game": { "kind": "iso_or_folder", "label": "Your Combo dump" },
            "prepare": { "program": ["builder.sh"], "args": ["--game={game}"] },
            "start": { "program": ["play.sh"] }
        }))
        .unwrap();
        p.lift_game();
        let mut g = Installed { id: "combo".into(), repo: "a/b".into(), path: game, tag: "1.0.0".into(), set_up: "1.0.0".into(), ..Default::default() };
        g.setup_inputs.insert("game".into(), copy);
        let mut lib = Library::default();
        lib.put("combo", g.clone());
        let st = app_in(&d, lib);
        super::keep_tag(&st, &mut g, "v1.0.0".into());
        let saved = st.lib.lock().unwrap().installed["combo"].clone();
        assert_eq!((saved.tag.as_str(), saved.set_up.as_str()), ("v1.0.0", "v1.0.0"));
        let rows = crate::inputs::rows(&p, &saved, &d).unwrap();
        let facts = crate::setup_state::Facts { profile: &p, game: &saved, rows: &rows, set_up: &saved.set_up, cut: false, own_program: false, skipped: false };
        let status = crate::setup_state::of(&facts);
        assert!(status.ready, "{status:?}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_rewrite_reaches_every_field_naming_the_same_release() {
        let mut g = Installed { tag: "1.0.0".into(), prev_tag: "1.0.0".into(), installer_tag: "v1.1.0".into(), ..Default::default() };
        g.games.insert("one".into(), crate::games::Played { set_up: "1.0.0".into(), ..Default::default() });
        super::retag(&mut g, "1.0.0", "v1.0.0");
        assert_eq!((g.prev_tag.as_str(), g.installer_tag.as_str(), g.games["one"].set_up.as_str()), ("v1.0.0", "v1.1.0", "v1.0.0"));
    }

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
        let _win_only = vec![rel("v2", &["game-windows-x64.zip"]), rel("v1", &["game-windows-x64.zip"])];
        #[cfg(target_os = "linux")]
        assert_eq!(offered_update(&proton, Some(&entry), &_win_only).as_deref(), Some("v2"));
        let _ = &proton;
        // A release with no build for this system isn't offered (on Linux the Windows-only one).
        #[cfg(target_os = "linux")]
        assert_eq!(offered_update(&g, Some(&entry), &_win_only), None);
        // Picked by the player: never.
        let pinned = Installed { pinned: true, ..g.clone() };
        assert_eq!(offered_update(&pinned, Some(&entry), &rels), None);
        // No checked release in the list: the newest one.
        assert_eq!(offered_update(&g, Some(&serde_json::json!({ "id": "g" })), &rels).as_deref(), Some("v3"));
    }

    // Longshot (#393): the store let out v3.0.0, the newest release is a nightly.
    #[test]
    fn reinstalling_the_version_the_player_picked_keeps_it_picked() {
        assert!(picked_by_player("nightly-681", "v3.0.0", false, Some("v3.0.0"), ("nightly-681", true)));
    }

    #[test]
    fn reinstalling_a_version_nobody_picked_doesnt_pick_it() {
        assert!(!picked_by_player("nightly-681", "nightly-681", false, Some("v3.0.0"), ("nightly-681", false)));
        assert!(!picked_by_player("nightly-680", "nightly-681", false, Some("v3.0.0"), ("nightly-680", false)));
    }

    #[test]
    fn going_to_another_version_than_the_newest_picks_it() {
        assert!(picked_by_player("nightly-681", "v3.0.0", false, Some("v3.0.0"), ("v3.0.0", false)));
    }

    /// Longshot's list on 2026-11-21: the store let out v3.0.0; nightly-680 was installed on the
    /// 21st and deleted from the server since, nightly-681 is the newest.
    fn longshot_after_a_nightly_went() -> (Vec<Release>, serde_json::Value, Installed) {
        // A build for every test machine (Windows and Linux x64, Mac arm64), so each finds one.
        let rel = |tag: &str, pre: bool, at: &str| -> Release {
            let names = [format!("Longshot-{tag}-windows-x64.zip"), format!("Longshot-{tag}-linux-x64.tar.gz"), format!("Longshot-{tag}-macos-arm64.zip")];
            let assets: Vec<serde_json::Value> = names.iter().map(|n| serde_json::json!({ "name": n, "browser_download_url": format!("https://git.magicbean.dev/x/{n}"), "size": 10 })).collect();
            serde_json::from_value(serde_json::json!({ "tag_name": tag, "prerelease": pre, "published_at": at, "assets": assets })).unwrap()
        };
        let rels = vec![rel("nightly-681", true, "2026-11-21T16:26:36Z"), rel("v3.0.0", false, "2026-11-19T02:18:27Z")];
        let entry = serde_json::json!({ "id": "longshot", "tag": "v3.0.0" });
        let installed_on_the_21st = released_secs("2026-11-21T09:30:00Z").unwrap() as u64;
        let g = Installed { id: "longshot".into(), tag: "nightly-680".into(), updated_at: installed_on_the_21st, ..Default::default() };
        (rels, entry, g)
    }

    #[test]
    fn a_deleted_nightly_is_not_updated_back_to_an_older_release() {
        let (rels, entry, g) = longshot_after_a_nightly_went();
        assert_eq!(offered_update(&g, Some(&entry), &rels), None);
    }

    #[test]
    fn a_release_out_after_a_deleted_nightly_was_installed_is_offered() {
        let (rels, entry, mut g) = longshot_after_a_nightly_went();
        g.updated_at = released_secs("2026-11-18T00:00:00Z").unwrap() as u64;
        assert_eq!(offered_update(&g, Some(&entry), &rels).as_deref(), Some("v3.0.0"));
    }

    #[test]
    fn a_release_date_reads_as_seconds() {
        assert_eq!(released_secs("1970-01-02T00:00:01Z"), Some(86401));
        // Two hours ahead of UTC, with a fraction of a second: the same moment.
        assert_eq!(released_secs("1970-01-02T02:00:01.250+02:00"), Some(86401));
        assert_eq!(released_secs("not a date"), None);
    }

    #[test]
    fn going_to_the_store_version_or_the_newest_picks_nothing() {
        // "Go Back to v3.0.0" from a picked nightly, and an update to the version on offer.
        assert!(!picked_by_player("v3.0.0", "nightly-681", false, Some("v3.0.0"), ("nightly-681", true)));
        assert!(!picked_by_player("v3.1.0", "v3.1.0", false, None, ("v3.0.0", false)));
        assert!(!picked_by_player("v2.9.0", "v3.0.0", true, None, ("", false)));
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
    fn installer_versions_must_match_the_whole_resource_value() {
        for version in ["0.90.0-beta.410", "0.90.0-beta.41.1", "0.90.0-beta.41-extra", "10.90.0-beta.41"] {
            let resource: Vec<u8> = format!("ProductVersion\0{version}\0").encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
            assert!(!installer_is_version(&resource, "0.90.0-beta.41"), "accepted {version} as beta.41");
        }
    }

    #[test]
    fn a_complete_installer_version_accepts_resource_padding() {
        for padding in ["", "\0"] {
            let resource: Vec<u8> = format!("ProductVersion\0{padding}0.90.0-beta.41\0").encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
            assert!(installer_is_version(&resource, "0.90.0-beta.41"));
            assert!(installer_is_version(&resource, "v0.90.0-beta.41"));
            assert!(!installer_is_version(&resource, ""));
        }
    }

    #[test]
    fn a_wrong_update_file_is_refused_and_not_offered_again() {
        // An installer names its version in UTF-16; another version's installer doesn't match.
        let inst: Vec<u8> = b"MZ..".iter().copied().chain("ProductVersion\00.90.0-beta.32\0".encode_utf16().flat_map(|u| u.to_le_bytes())).collect();
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

    /// A 1920x1080 screen whose bottom 40 pixels are the taskbar.
    fn screen_area() -> tauri::PhysicalRect<i32, u32> {
        tauri::PhysicalRect { position: tauri::PhysicalPosition::new(0, 0), size: tauri::PhysicalSize::new(1920, 1040) }
    }

    #[test]
    fn a_kept_window_that_fits_comes_back_unchanged() {
        let place = super::WindowPlace { x: 100, y: 50, w: 1360, h: 860, maximized: false };
        let fit = super::fit_on_screen(&place, &screen_area());
        assert_eq!(fit, place);
    }

    #[test]
    fn a_kept_window_wider_than_both_screens_shrinks_to_one() {
        let place = super::WindowPlace { x: -200, y: 120, w: 4600, h: 1500, maximized: false };
        let fit = super::fit_on_screen(&place, &screen_area());
        assert_eq!(fit, super::WindowPlace { x: 0, y: 0, w: 1920, h: 1040, maximized: false });
    }

    #[test]
    fn a_kept_window_hanging_off_the_edge_moves_back_on() {
        let place = super::WindowPlace { x: 1000, y: 600, w: 1360, h: 860, maximized: true };
        let fit = super::fit_on_screen(&place, &screen_area());
        assert_eq!(fit, super::WindowPlace { x: 560, y: 180, w: 1360, h: 860, maximized: true });
    }

    #[test]
    fn a_kept_window_on_a_second_screen_stays_there() {
        let right_screen = tauri::PhysicalRect { position: tauri::PhysicalPosition::new(1920, 0), size: tauri::PhysicalSize::new(2560, 1400) };
        let place = super::WindowPlace { x: 2200, y: 100, w: 1360, h: 860, maximized: false };
        let fit = super::fit_on_screen(&place, &right_screen);
        assert_eq!(fit, place);
    }

    #[test]
    fn a_first_window_too_big_for_the_screen_shrinks_and_centres() {
        // 1360x860 at 150% on a 1080p laptop.
        let place = super::centred_on_screen(2040, 1290, &screen_area());
        assert_eq!(place, super::WindowPlace { x: 96, y: 52, w: 1728, h: 936, maximized: false });
    }

    /// A 2560x1440 screen right of the 1920x1080 one, its bottom 40 pixels the taskbar.
    fn right_screen() -> (tauri::PhysicalRect<i32, u32>, tauri::PhysicalRect<i32, u32>) {
        let position = tauri::PhysicalPosition::new(1920, 0);
        let whole = tauri::PhysicalRect { position, size: tauri::PhysicalSize::new(2560, 1440) };
        let free = tauri::PhysicalRect { position, size: tauri::PhysicalSize::new(2560, 1400) };
        (whole, free)
    }

    #[test]
    fn a_window_maximized_on_its_own_screen_keeps_its_normal_place() {
        let (whole, free) = right_screen();
        let place = super::WindowPlace { x: 2200, y: 100, w: 1360, h: 860, maximized: true };
        let kept = super::normal_place_on(place, &whole, &free);
        assert_eq!(kept, super::WindowPlace { x: 2200, y: 100, w: 1360, h: 860, maximized: true });
    }

    #[test]
    fn a_window_maximized_on_another_screen_moves_its_normal_place_there() {
        let (whole, free) = right_screen();
        let place = super::WindowPlace { x: 200, y: 100, w: 1360, h: 860, maximized: true };
        let kept = super::normal_place_on(place, &whole, &free);
        assert_eq!(kept, super::WindowPlace { x: 2520, y: 270, w: 1360, h: 860, maximized: true });
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
    fn a_release_candidate_is_newer_than_a_later_numbered_beta() {
        assert!(!super::older_version("v2.0.0-rc.1", "v2.0.0-beta.5"));
        assert!(super::older_version("v2.0.0-beta.5", "v2.0.0-rc.1"));
    }

    #[test]
    fn a_preview_is_newer_than_an_alpha() {
        assert!(super::older_version("1.0.0-alpha.1", "1.0.0-preview.2"));
    }

    #[test]
    fn a_pre_is_newer_than_a_beta() {
        assert!(super::older_version("1.0.0-beta.3", "1.0.0-pre.1"));
    }

    #[test]
    fn a_nightly_is_older_than_an_alpha() {
        assert!(super::older_version("1.0.0-nightly.5", "1.0.0-alpha.1"));
    }

    #[test]
    fn an_unknown_label_sits_between_alpha_and_rc() {
        assert!(super::older_version("1.0.0-alpha.1", "1.0.0-foo.1"));
        assert!(super::older_version("1.0.0-foo.1", "1.0.0-rc.1"));
    }

    #[test]
    fn a_fourth_version_part_counts() {
        assert!(super::older_version("1.2.3.4", "1.2.3.5"));
        assert!(!super::older_version("1.2.3.5", "1.2.3.4"));
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

    // #374: the copy picked in a setup screen's box goes through take_pick, as any pick does.
    fn take_pick_dir(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let d = std::env::temp_dir().join(format!("rl-take-pick-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let game = d.join("Recomp Library/aoe");
        std::fs::create_dir_all(&game).unwrap();
        (d, game)
    }

    fn copy_game() -> crate::profile::Profile {
        let mut p: crate::profile::Profile = serde_json::from_value(serde_json::json!({ "format": 1, "game": { "kind": "iso_or_folder", "label": "Your copy" }, "start": { "program": ["g"] } })).unwrap();
        p.lift_game();
        p
    }

    #[test]
    fn take_pick_takes_the_copy_picked_on_the_screen() {
        let (d, game) = take_pick_dir("picked");
        let st = app_in(&d, Library::default());
        let mine = d.join("My Games/aoe.nsp");
        std::fs::create_dir_all(mine.parent().unwrap()).unwrap();
        std::fs::write(&mine, b"nsp").unwrap();
        let p = copy_game();
        let g = Installed { path: game, ..Default::default() };
        assert_eq!(super::take_pick(&st, &p, p.main_input().unwrap(), &g, mine.clone()).unwrap(), mine);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn take_pick_turns_away_a_pick_that_is_gone() {
        let (d, game) = take_pick_dir("gone");
        let st = app_in(&d, Library::default());
        let p = copy_game();
        let g = Installed { path: game, ..Default::default() };
        let gone = d.join("My Games/gone.nsp");
        assert_eq!(super::take_pick(&st, &p, p.main_input().unwrap(), &g, gone).unwrap_err(), "That file isn't there anymore.");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn take_pick_checks_a_new_pick_and_not_the_one_from_before() {
        let (d, game) = take_pick_dir("own");
        let st = app_in(&d, Library::default());
        let p = copy_game();
        let mut g = Installed { path: game.clone(), ..Default::default() };
        assert!(super::take_pick(&st, &p, p.main_input().unwrap(), &g, game.clone()).unwrap_err().contains("own folder"));
        // The same one, set up before (an adopted game inside the player's copy): not checked again.
        g.setup_inputs.insert("game".into(), game.clone());
        assert_eq!(super::take_pick(&st, &p, p.main_input().unwrap(), &g, game.clone()).unwrap(), game);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_file_with_no_rules_only_has_to_be_there() {
        let (d, game) = take_pick_dir("dlc");
        let st = app_in(&d, Library::default());
        let mut p = copy_game();
        p.inputs.push(serde_json::from_value(serde_json::json!({ "id": "dlc", "label": "DLC", "role": "dlc", "optional": true, "formats": [{ "type": "file" }] })).unwrap());
        let dlc = d.join("dlc.zip");
        std::fs::write(&dlc, b"a zip the game reads itself").unwrap();
        let g = Installed { path: game, ..Default::default() };
        let i = p.inputs.iter().find(|i| i.id == "dlc").unwrap();
        assert_eq!(super::take_pick(&st, &p, i, &g, dlc.clone()).unwrap(), dlc);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_game_with_a_copy_and_named_inputs_is_ready_after_its_setup() {
        let (d, game) = take_pick_dir("kept");
        let copy = d.join("My Games/Combo");
        std::fs::create_dir_all(&copy).unwrap();
        let mut p: crate::profile::Profile = serde_json::from_value(serde_json::json!({
            "format": 1,
            "game": { "kind": "iso_or_folder", "label": "Your Combo dump" },
            "inputs": [{ "id": "dlc", "label": "DLC", "role": "dlc", "optional": true, "formats": [{ "type": "file" }] }],
            "prepare": { "program": ["builder.sh"], "args": ["--game={game}"] },
            "start": { "program": ["play.sh"] }
        }))
        .unwrap();
        p.lift_game();
        let mut g = Installed { id: "combo".into(), path: game, tag: "v1".into(), set_up: "v1".into(), ..Default::default() };
        g.setup_inputs.insert("game".into(), copy);
        let rows = crate::inputs::rows(&p, &g, &d).unwrap();
        let facts = crate::setup_state::Facts { profile: &p, game: &g, rows: &rows, set_up: &g.set_up, cut: false, own_program: false, skipped: false };
        let status = crate::setup_state::of(&facts);
        assert!(status.ready, "{status:?}");
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
    fn a_ps4_folder_is_found_by_the_title_ids_its_game_check_lists() {
        let d = std::env::temp_dir().join(format!("rl-ps4-ids-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let ours = d.join("Diablo 3/CUSA00242");
        let other = d.join("The Order 1886/CUSA00035");
        roms::tests::ps4_folder(&ours, "Diablo III: Reaper of Souls – Ultimate Evil Edition", "CUSA00242");
        roms::tests::ps4_folder(&other, "The Order: 1886", "CUSA00035");
        let found = roms::scan(&[d.clone()]);
        assert_eq!(found.len(), 2);
        let cat = json!({ "apps": [{ "id": "sanctuary", "game": "Diablo III", "origin": "ps4", "repo": "example/sanctuary",
            "launcher": { "format": 1, "start": { "program": ["game.exe"] }, "game": {
                "kind": "folder", "check": { "file": "eboot.bin", "ids": ["CUSA00243", "CUSA00242"] }
            } } }] });
        let matched = copies_for(&cat, &found, &HashMap::new());
        assert_eq!(matched.get("sanctuary").map(|r| &r.path), Some(&ours));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_ps4_folder_is_not_found_by_eboot_bin_alone() {
        let d = std::env::temp_dir().join(format!("rl-ps4-noids-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        roms::tests::ps4_folder(&d.join("CUSA00242"), "Diablo III: Reaper of Souls – Ultimate Evil Edition", "CUSA00242");
        let found = roms::scan(&[d.clone()]);
        let cat = json!({ "apps": [{ "id": "sanctuary", "game": "Diablo III", "origin": "ps4", "repo": "example/sanctuary",
            "launcher": { "format": 1, "start": { "program": ["game.exe"] }, "game": {
                "kind": "folder", "check": { "file": "eboot.bin" }
            } } }] });
        assert!(copies_for(&cat, &found, &HashMap::new()).is_empty());
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
        let need = profile::find(Path::new(""), "TwilitRealm/dusklight", &cat["apps"][0]).and_then(|p| p.main_copy()).unwrap();
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

    pub(crate) fn app_in(dir: &Path, lib: Library) -> App {
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

    // A player who deleted their copy after the build is still set up when the step kept what it
    // needs (prepare.keeps_copy): an option an update added is asked about, as with the copy there.
    #[test]
    fn an_option_an_update_added_is_new_with_the_copy_gone_when_the_step_kept_it() {
        let d = std::env::temp_dir().join(format!("rl-newopt-kept-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let game = d.join("game");
        std::fs::create_dir_all(game.join("Hookshot")).unwrap();
        std::fs::write(game.join("Hookshot").join(".hookshot_exefs"), b"kept").unwrap();
        let manifest = json!({
            "format": 1,
            "game": { "kind": "iso_or_folder", "label": "Your copy" },
            "prepare": { "program": ["Hookshot.AppImage"], "args": ["--game={game}", "--out", "{install}/Hookshot"],
                         "done": ["{install}/Hookshot/hookshot"], "keeps_copy": true,
                         "options": [{ "id": "fsr", "label": "AMD FSR 4" }] },
            "start": { "program": ["Hookshot.AppImage"], "args": ["--play", "{install}/Hookshot"] },
            "folders": { "game": [{ "path": "{install}/Hookshot" }] }
        });
        std::fs::write(game.join("recomplauncher.json"), manifest.to_string()).unwrap();
        let mut lib = Library::default();
        lib.installed.insert("hookshot".into(), Installed { id: "hookshot".into(), path: game.clone(), tag: "v1".into(), set_up: "v1".into(), ..Default::default() });
        let app = app_in(&d, lib);
        assert_eq!(new_prepare_options(&app, "hookshot"), vec!["fsr".to_string()]);
        // Never set up, and no copy: there's nothing new to ask yet.
        app.lib.lock().unwrap().installed.get_mut("hookshot").unwrap().set_up.clear();
        assert!(new_prepare_options(&app, "hookshot").is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    // With one worker, the other task can only run while `block_here` waits if the worker is handed
    // over: without block_in_place it would wait for the whole timeout and get nothing.
    #[cfg(target_os = "linux")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn long_work_on_the_only_worker_lets_the_other_tasks_run() {
        let ran = tokio::spawn(async {
            let (tx, rx) = std::sync::mpsc::channel();
            let other = tokio::spawn(async move { tx.send(()).unwrap() });
            let got = block_here(|| rx.recv_timeout(std::time::Duration::from_secs(5)).is_ok());
            other.await.unwrap();
            got
        });
        assert!(ran.await.unwrap());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn long_work_off_the_runtime_just_runs() {
        assert_eq!(block_here(|| 7), 7);
    }

    #[test]
    fn correcting_a_project_keeps_the_installation_and_history() {
        let d = std::env::temp_dir().join(format!("rl-project-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("game")).unwrap();
        std::fs::write(d.join("game/game.exe"), b"player's executable").unwrap();
        let mut lib = Library::default();
        lib.installed.insert("wrong".into(), Installed {
            id: "wrong".into(), name: "My game".into(), repo: "Wrong/Project".into(), tag: "v9".into(),
            path: d.join("game"), exe: Some(PathBuf::from("game.exe")), exe_rules: u8::MAX, mode: library::SetupMode::Own,
            played_secs: 120, sessions: vec![[1, 120]], auto_update: true, sha256: "old checksum".into(),
            asset: "old.zip".into(), verified: true, cloud: Some(true), ..Default::default()
        });
        lib.settings.ui = json!({ "pins": ["wrong"], "names": { "wrong": "My name" } });
        let backup = d.join("saves").join("wrong");
        std::fs::create_dir_all(&backup).unwrap();
        std::fs::write(backup.join("backup.zip"), b"backup").unwrap();
        let app = app_in(&d, lib);
        let cat = json!({ "apps": [{ "id": "right", "name": "Right", "repo": "Right/Project" }] });
        let tracked = adopt::change_project(&app, "wrong", "right", &cat).unwrap();
        assert_eq!((tracked.repo.as_str(), tracked.tag.as_str()), ("Right/Project", ""));
        assert!(!tracked.local && !tracked.auto_update && !tracked.verified);
        assert!(tracked.sha256.is_empty() && tracked.asset.is_empty() && tracked.cloud.is_none());
        assert_eq!((tracked.mode, tracked.exe_rules), (library::SetupMode::Own, u8::MAX));
        assert_eq!(tracked.exe, Some(PathBuf::from("game.exe")));
        assert_eq!((tracked.played_secs, tracked.sessions.clone()), (120, vec![[1, 120]]));
        assert_eq!(std::fs::read(d.join("saves").join("right").join("backup.zip")).unwrap(), b"backup");
        let own = adopt::change_project(&app, "right", "", &cat).unwrap();
        assert!(own.local && own.repo.is_empty() && own.tag == "own");
        assert_eq!(own.mode, library::SetupMode::Own);
        let saved = Library::load(&d);
        assert_eq!(saved.installed.len(), 1);
        assert_eq!(saved.resolve_project_id("wrong"), own.id);
        assert_eq!(saved.resolve_project_id("right"), own.id);
        assert_eq!(saved.settings.ui["pins"][0], own.id);
        assert_eq!(saved.settings.ui["names"][&own.id], "My name");
        assert!(d.join("saves").join(&own.id).join("backup.zip").is_file());
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
        let mut old = Installed { id: "old-recomp".into(), name: "Old Listing".into(), repo: "someone/old-recomp".into(), tag: "v0.1.1".into(), path: d.join("games/old"), played_secs: 7200, ..Default::default() };
        old.setup_inputs.insert("game".into(), copy.clone());
        lib.installed.insert("old-recomp".into(), old);
        std::fs::create_dir_all(d.join("saves").join("old-recomp")).unwrap();
        std::fs::write(d.join("saves").join("old-recomp").join("1.zip"), b"save").unwrap();
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
        assert_eq!(g.setup_inputs.get("game"), Some(&copy));
        assert_eq!(g.path, d.join("games/old"));
        assert_eq!(g.name, "New Listing");
        assert_eq!(l.project_aliases.get("old-recomp").map(String::as_str), Some("new-recomp"), "old links still open it");
        assert!(d.join("saves").join("new-recomp").join("1.zip").is_file());
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
        let backup = d.join("saves").join("old");
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
        assert!(!d.join("saves").join("new").exists());
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
        std::fs::create_dir_all(d.join("saves").join("gh-me-game")).unwrap();
        std::fs::write(d.join("saves").join("gh-me-game").join("1.zip"), b"save").unwrap();
        // Backups from the official one, installed once before.
        std::fs::create_dir_all(d.join("saves").join("game")).unwrap();
        std::fs::write(d.join("saves").join("game").join("2.zip"), b"save").unwrap();
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
        assert!(d.join("saves").join("game").join("1.zip").is_file() && d.join("saves").join("game").join("2.zip").is_file());
        assert!(!d.join("saves").join("gh-me-game").exists());
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

    #[test]
    fn a_setup_step_run_without_the_copy_leaves_out_the_arguments_that_name_it() {
        let pr: profile::Prepare = serde_json::from_value(serde_json::json!({
            "program": ["Builder.exe"], "args": ["--cli", "--prepare", "--game={game}", "--out", "{install}/Game"], "keeps_copy": true
        })).unwrap();
        let install = Path::new("/games/hookshot");
        let extra = vec!["--fetch-ngx-sdk".to_string()];
        let without = prepare_args(&pr, install, None, &extra);
        assert_eq!(without, ["--cli", "--prepare", "--out", "/games/hookshot/Game", "--fetch-ngx-sdk"]);
        let with = prepare_args(&pr, install, Some(Path::new("/roms/game.nup")), &extra);
        assert_eq!(with[2], "--game=/roms/game.nup");
        assert_eq!(with.len(), 6);
    }

    fn fresh_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rl-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// An unpacked AppImage like mkw-recomp's: AppRun, the program beside it, and lib/.
    fn unpacked_appimage(name: &str) -> PathBuf {
        let d = fresh_dir(name);
        std::fs::write(d.join("AppRun"), "#!/bin/sh\n").unwrap();
        std::fs::write(d.join("mkw"), "").unwrap();
        std::fs::create_dir_all(d.join("lib")).unwrap();
        d
    }

    #[test]
    fn a_program_named_inside_an_unpacked_appimage_gets_its_libraries() {
        let d = unpacked_appimage("appimage-libs-program");
        let libs = appimage_libs(&d.join("mkw"), &d, None).unwrap();
        assert_eq!(std::env::split_paths(&libs).collect::<Vec<_>>(), [d.join("lib")]);
    }

    #[test]
    fn the_pcs_own_library_folders_come_after_the_appimages() {
        let d = unpacked_appimage("appimage-libs-pc");
        let libs = appimage_libs(&d.join("mkw"), &d, Some("/opt/mine/lib".into())).unwrap();
        assert_eq!(std::env::split_paths(&libs).collect::<Vec<_>>(), [d.join("lib"), PathBuf::from("/opt/mine/lib")]);
    }

    #[test]
    fn an_appimages_apprun_sets_its_own_libraries() {
        let d = unpacked_appimage("appimage-libs-apprun");
        assert!(appimage_libs(&d.join("AppRun"), &d, None).is_none());
    }

    #[test]
    fn a_program_in_a_plain_folder_gets_no_libraries_added() {
        let d = fresh_dir("appimage-libs-plain");
        std::fs::write(d.join("game"), "").unwrap();
        std::fs::create_dir_all(d.join("lib")).unwrap();
        assert!(appimage_libs(&d.join("game"), &d, None).is_none());
    }

    fn var(vars: &[(&str, std::ffi::OsString)], k: &str) -> Option<PathBuf> {
        vars.iter().find(|(name, _)| *name == k).map(|(_, v)| PathBuf::from(v))
    }

    #[test]
    fn an_unpacked_appimages_apprun_gets_appdir_owd_and_argv0() {
        let d = unpacked_appimage("appimage-vars-apprun");
        let vars = appimage_vars(&d.join("AppRun"), &d, None);
        assert_eq!(var(&vars, "APPDIR"), Some(d.clone()));
        assert_eq!(var(&vars, "OWD"), Some(d.clone()));
        assert_eq!(var(&vars, "ARGV0"), Some(d.join("AppRun")));
    }

    #[test]
    fn a_program_named_inside_an_unpacked_appimage_gets_its_appdir() {
        let d = unpacked_appimage("appimage-vars-program");
        std::fs::create_dir_all(d.join("usr/bin")).unwrap();
        std::fs::write(d.join("usr/bin/game"), "").unwrap();
        let vars = appimage_vars(&d.join("usr/bin/game"), &d, None);
        assert_eq!(var(&vars, "APPDIR"), Some(d.clone()));
        assert_eq!(var(&vars, "OWD"), Some(d.join("usr/bin")));
        assert!(var(&vars, "ARGV0").is_none());
    }

    #[test]
    fn a_program_in_a_plain_folder_gets_nothing_of_an_appimage() {
        let d = fresh_dir("appimage-vars-plain");
        std::fs::write(d.join("game"), "").unwrap();
        assert!(appimage_vars(&d.join("game"), &d, None).is_empty());
    }

    #[test]
    fn a_build_from_what_was_kept_that_fails_asks_for_the_copy_in_a_sentence_of_its_own() {
        let said = "Its setup step stopped with code 3.\nnothing kept";
        assert_eq!(kept_failed(said), "Its setup step stopped with code 3. Choose your copy of the game to build it from that.\nnothing kept");
        assert_eq!(kept_failed("It stopped. "), "It stopped. Choose your copy of the game to build it from that.");
    }

    #[test]
    fn a_failed_setup_step_shows_the_launchers_line_then_what_it_printed_last() {
        let said = step_failed("1", "Missing: vulkan\nsudo apt install libvulkan-dev");
        assert_eq!(said, "Its setup step stopped with code 1.\nMissing: vulkan\nsudo apt install libvulkan-dev");
    }

    #[test]
    fn a_setup_step_that_printed_nothing_shows_only_its_code() {
        assert_eq!(step_failed("2", ""), "Its setup step stopped with code 2.");
    }

    #[test]
    fn a_setup_step_keeps_only_its_last_lines() {
        let mut said = LastSaid::default();
        for i in 0..SAID_LINES + 5 {
            said.push(&format!("line {i}"));
        }
        let text = said.text();
        let last = format!("line {}", SAID_LINES + 4);
        assert_eq!(text.lines().count(), SAID_LINES);
        assert_eq!(text.lines().next(), Some("line 5"));
        assert_eq!(text.lines().last(), Some(last.as_str()));
    }

    #[test]
    fn a_setup_steps_blank_lines_are_not_kept() {
        let mut said = LastSaid::default();
        said.push("reason");
        said.push("   ");
        assert_eq!(said.text(), "reason");
    }

    #[test]
    fn a_setup_steps_colour_codes_are_taken_out() {
        assert_eq!(plain_line("\u{1b}[1;31merror:\u{1b}[0m vulkan missing"), "error: vulkan missing");
    }

    #[test]
    fn a_progress_bar_drawn_over_itself_keeps_only_its_last_drawing() {
        assert_eq!(plain_line("10%\r50%\rdone"), "done");
    }

    #[test]
    fn run_prepare_cancellation_kills_process_and_preserves_external_files() {
        let dir = std::env::temp_dir().join(format!("rl-prep-cancel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        #[cfg(windows)]
        let (prog_name, script_content) = ("builder.bat", "@echo off\r\necho generated > generated.txt\r\nping 127.0.0.1 -n 30 >nul\r\n");
        #[cfg(not(windows))]
        let (prog_name, script_content) = ("builder.sh", "#!/bin/sh\necho generated > generated.txt\nsleep 30\n");

        let prog_path = dir.join(prog_name);
        std::fs::write(&prog_path, script_content).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&prog_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let pr = profile::Prepare {
            program: vec![prog_name.into()],
            args: vec![],
            label: "Test Builder".into(),
            options: vec![],
            version: String::new(),
            done: vec![],
            env: BTreeMap::new(),
            keeps_copy: false,
        };

        let generated_file = dir.join("generated.txt");
        let stopped = std::sync::atomic::AtomicBool::new(false);

        let res = run_prepare(
            &pr,
            &dir,
            Some(&dir),
            &[],
            &|_, _| {},
            &|| {
                if generated_file.exists() {
                    stopped.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                stopped.load(std::sync::atomic::Ordering::Relaxed)
            },
            &|| {},
            None,
        );

        assert_eq!(res.unwrap_err(), "Stopped.");
        assert!(!dir.join(setup_temp::FOLDER).exists(), "a stopped step's temp folder goes too");
        // The process was stopped immediately, but the external file it wrote remains:
        assert!(generated_file.exists(), "File generated by external tool before cancellation was preserved");
        let content = std::fs::read_to_string(&generated_file).unwrap();
        assert!(content.contains("generated"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod prepare_env_tests {
    use super::*;

    /// A setup step that writes down what it was given: its temp folder (and whether it was
    /// there), the launcher's names and one of its own from prepare.env.
    fn tattler(dir: &Path) -> profile::Prepare {
        #[cfg(windows)]
        // The redirection first: `echo 1>rl.txt` would be read as sending handle 1 to the file.
        let (name, script) = ("tattle.bat", "@echo off\r\n>temp.txt echo %TEMP%\r\n>tmp.txt echo %TMP%\r\nif exist \"%TEMP%\" >exists.txt echo yes\r\n>rl.txt echo %RECOMPLAUNCHER%\r\n>data.txt echo %RECOMPLAUNCHER_DATA%\r\n>cache.txt echo %BUILD_CACHE%\r\n");
        #[cfg(not(windows))]
        let (name, script) = ("tattle.sh", "#!/bin/sh\necho \"$TMPDIR\" > temp.txt\necho \"$TMP\" > tmp.txt\ntest -d \"$TMPDIR\" && echo yes > exists.txt\necho \"$RECOMPLAUNCHER\" > rl.txt\necho \"$RECOMPLAUNCHER_DATA\" > data.txt\necho \"$BUILD_CACHE\" > cache.txt\n");
        std::fs::write(dir.join(name), script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.join(name), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        profile::Prepare {
            program: vec![name.into()],
            env: BTreeMap::from([("BUILD_CACHE".to_string(), "{data}/cache".to_string()), ("TEMP".to_string(), "not-this".to_string())]),
            ..Default::default()
        }
    }

    fn said(dir: &Path, file: &str) -> PathBuf {
        PathBuf::from(std::fs::read_to_string(dir.join(file)).unwrap().trim())
    }

    /// A step that prints a progress line and 15 numbered lines, then fails with code 1.
    #[cfg(unix)]
    fn talker(dir: &Path) -> profile::Prepare {
        use std::os::unix::fs::PermissionsExt;
        let script = "#!/bin/sh\necho 'progress 0.5 Checking'\nfor i in $(seq 1 15); do echo \"line $i\"; done\nexit 1\n";
        std::fs::write(dir.join("talk.sh"), script).unwrap();
        std::fs::set_permissions(dir.join("talk.sh"), std::fs::Permissions::from_mode(0o755)).unwrap();
        profile::Prepare { program: vec!["talk.sh".into()], ..Default::default() }
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_step_shows_its_last_lines_and_its_full_log_keeps_every_line() {
        let dir = std::env::temp_dir().join(format!("rl-prep-talk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pr = talker(&dir);
        let log = setup_output_file(&dir, "talker").unwrap();
        let e = run_prepare(&pr, &dir, Some(&dir), &[], &|_, _| {}, &|| false, &|| {}, Some(&log)).unwrap_err();
        let full = std::fs::read_to_string(setup_output_path(&dir, "talker")).unwrap();
        assert_eq!(e.lines().next(), Some("Its setup step stopped with code 1."));
        assert_eq!(e.lines().nth(1), Some("line 6"));
        assert_eq!(e.lines().last(), Some("line 15"));
        assert_eq!(full.lines().count(), 16, "the progress line and all 15 lines");
        assert_eq!(full.lines().next(), Some("progress 0.5 Checking"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Set Up Again whose step can't start (its program missing): the last build isn't marked
    /// as cut short, so the game still plays.
    #[test]
    fn a_step_that_cant_start_leaves_the_build_unmarked() {
        let dir = std::env::temp_dir().join(format!("rl-prep-nostart-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pr = profile::Prepare { program: vec!["missing-builder".into()], ..Default::default() };
        let marked = std::cell::Cell::new(0);
        let res = run_prepare(&pr, &dir, Some(&dir), &[], &|_, _| {}, &|| false, &|| marked.set(marked.get() + 1), None);
        assert!(res.is_err());
        assert_eq!(marked.get(), 0, "nothing was written");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A step that starts is marked once, as it starts.
    #[test]
    fn a_step_that_starts_marks_the_build() {
        let dir = std::env::temp_dir().join(format!("rl-prep-marks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pr = tattler(&dir);
        let marked = std::cell::Cell::new(0);
        run_prepare(&pr, &dir, Some(&dir), &[], &|_, _| {}, &|| false, &|| marked.set(marked.get() + 1), None).unwrap();
        assert_eq!(marked.get(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_setup_step_gets_its_own_temp_folder_and_the_data_folder() {
        let dir = std::env::temp_dir().join(format!("rl-prep-env-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pr = tattler(&dir);
        run_prepare(&pr, &dir, Some(&dir), &[], &|_, _| {}, &|| false, &|| {}, None).unwrap();
        let temp = dir.join(setup_temp::FOLDER);
        assert_eq!(said(&dir, "temp.txt"), temp);
        assert_eq!(said(&dir, "tmp.txt"), temp);
        assert_eq!(said(&dir, "exists.txt"), PathBuf::from("yes"));
        assert!(!temp.exists(), "the temp folder is gone after the run");
        assert_eq!(said(&dir, "rl.txt"), PathBuf::from("1"));
        assert_eq!(said(&dir, "data.txt"), dir.join("recompdata"));
        assert!(dir.join("recompdata").is_dir());
        assert_eq!(said(&dir, "cache.txt"), dir.join("recompdata").join("cache"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_temp_folder_the_launcher_didnt_make_stops_setup_untouched() {
        let dir = std::env::temp_dir().join(format!("rl-prep-theirs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(setup_temp::FOLDER)).unwrap();
        std::fs::write(dir.join(setup_temp::FOLDER).join("release.txt"), b"theirs").unwrap();
        let pr = tattler(&dir);
        let e = run_prepare(&pr, &dir, Some(&dir), &[], &|_, _| {}, &|| false, &|| {}, None).unwrap_err();
        assert_eq!(e, setup_temp::NOT_OURS);
        assert!(!dir.join("temp.txt").exists(), "the step didn't run");
        assert!(dir.join(setup_temp::FOLDER).join("release.txt").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod set_up_marks_tests {
    use super::*;

    /// A recomp with two games, each built from its own file.
    fn two_games() -> (profile::Profile, Vec<games::Game>) {
        let whole: profile::Profile = serde_json::from_value(serde_json::json!({
            "prepare": { "program": ["Builder.exe"] },
            "start": { "program": ["Game.exe"] },
            "inputs": [
                { "id": "a", "label": "Game A", "role": "game", "formats": [{ "type": "file" }] },
                { "id": "b", "label": "Game B", "role": "game", "formats": [{ "type": "file" }] }
            ]
        })).unwrap();
        let all: Vec<games::Game> = serde_json::from_value(serde_json::json!([
            { "id": "ga", "name": "A", "inputs": ["a"] }, { "id": "gb", "name": "B", "inputs": ["b"] }
        ])).unwrap();
        (whole, all)
    }

    fn status(p: &profile::Profile, g: &Installed, sub: &str, dir: &Path) -> setup_state::Status {
        let rows = inputs::rows(p, g, dir).unwrap();
        setup_state::of(&setup_state::Facts { profile: p, game: g, rows: &rows, set_up: mark_of(g, Some(sub)), cut: cut_of(g, Some(sub)), own_program: false, skipped: false })
    }

    #[test]
    fn one_game_of_several_keeps_its_own_setup_through_the_others() {
        let dir = std::env::temp_dir().join(format!("recomp-marks-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (whole, all) = two_games();
        let file = dir.join("a.bin");
        std::fs::write(&file, b"a").unwrap();
        let mut g = Installed { path: dir.clone(), tag: "v1".into(), ..Default::default() };
        g.setup_inputs.insert("a".into(), file);
        g.games.entry("ga".into()).or_default().set_up = "v1".into();
        let mut a = whole.clone();
        games::scope(&mut a, &all[0], &all);
        assert!(status(&a, &g, "ga", &dir).ready);
        // Game B isn't set up: its own file is asked for.
        let mut b = whole.clone();
        games::scope(&mut b, &all[1], &all);
        assert!(!status(&b, &g, "gb", &dir).ready);
        // A new release builds game A again.
        let newer = Installed { tag: "v2".into(), ..g.clone() };
        let s = status(&a, &newer, "ga", &dir);
        assert_eq!((s.reason, s.changed), ("rebuild", true));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_game_of_several_without_its_own_mark_goes_by_the_recomps() {
        let g = Installed { set_up: "v1".into(), ..Default::default() };
        assert_eq!(mark_of(&g, Some("ga")), "v1");
        let mut own = g.clone();
        own.games.entry("ga".into()).or_default().set_up = "v0".into();
        assert_eq!(mark_of(&own, Some("ga")), "v0");
        assert_eq!(mark_of(&own, None), "v1");
    }

    /// The whole recomp's build was cut short, then game A was built on its own and that worked:
    /// A plays, and B (no build of its own) still builds again.
    #[test]
    fn a_game_of_several_built_after_the_recomps_cut_build_plays() {
        let mut g = Installed { set_up: "v1".into(), build_cut: true, ..Default::default() };
        let a = g.games.entry("ga".into()).or_default();
        a.set_up = "v1".into();
        a.build_cut = false;
        assert!(!cut_of(&g, Some("ga")));
        assert!(cut_of(&g, Some("gb")));
        assert!(cut_of(&g, None));
    }

    /// Game A's own build cut short before it had one that worked: A builds again, the recomp's
    /// other games don't.
    #[test]
    fn a_game_of_several_cut_short_on_its_first_own_build_builds_again() {
        let mut g = Installed { set_up: "v1".into(), ..Default::default() };
        g.games.entry("ga".into()).or_default().build_cut = true;
        assert!(cut_of(&g, Some("ga")));
        assert!(!cut_of(&g, Some("gb")));
    }

    #[test]
    fn a_setup_from_an_older_launcher_counts_when_it_was_current() {
        let (whole, _) = two_games();
        let dir = std::env::temp_dir();
        let g = Installed { path: dir.clone(), ..Default::default() };
        let set = library::SetupUpgrade { inputs_ready: true, ..Default::default() };
        assert_eq!(old_setup(&whole, &g, &set), Some(true));
        // Not set up then: nothing now.
        assert_eq!(old_setup(&whole, &g, &library::SetupUpgrade::default()), None);
        // Built with another prepare.version than the manifest has: set up, and due a build then, so
        // now too (in the Finish the Update window).
        let mut bumped = whole.clone();
        bumped.prepare.as_mut().unwrap().version = "2".into();
        assert_eq!(old_setup(&bumped, &g, &set), Some(false));
        let marked = Installed { tag: "v1".into(), set_up: SET_UP_BEFORE.into(), ..g.clone() };
        assert_ne!(marked.set_up, marked.tag);
    }
}

#[cfg(test)]
mod copy_listing_tests {
    use super::*;

    #[test]
    fn a_copy_the_player_added_goes_by_the_listings_id() {
        let cat = serde_json::json!({ "apps": [
            { "id": "hopper", "repo": "dev/Hopper-Recomp", "origin": "x360" },
            { "id": "racer", "repo": "dev/SkyRacer", "title_id": "4D5307E6" }
        ]});
        // A fork of the listing's repo: the same name, another owner, another case.
        assert_eq!(listing_for_copy(&cat, "someone/hopper-recomp", || None), Some(("hopper".to_string(), "repo name")));
        assert_eq!(listing_for_copy(&cat, "https://github.com/someone/Hopper-Recomp.git", || None).map(|x| x.0).as_deref(), Some("hopper"));
        // Another repo of the developer's: its disc's title id.
        assert_eq!(listing_for_copy(&cat, "dev/racer-builds", || Some("4d5307e6".into())), Some(("racer".to_string(), "title id")));
        assert_eq!(listing_for_copy(&cat, "dev/unrelated", || Some("11111111".into())), None);
        assert_eq!(listing_for_copy(&cat, "dev/unrelated", || None), None);
    }
}

#[cfg(test)]
mod easy_skip_tests {
    use super::*;

    #[test]
    fn easy_setup_is_skipped_for_one_game_or_for_all_of_them() {
        assert!(!easy_skipped(&json!({}), "hopper"));
        assert!(!easy_skipped(&Value::Null, "hopper"));
        let one = json!({ "easy_skip": ["hopper"] });
        assert!(easy_skipped(&one, "hopper"));
        assert!(!easy_skipped(&one, "skyracer"));
        assert!(easy_skipped(&json!({ "easy_skip_all": true }), "skyracer"));
        assert!(!easy_skipped(&json!({ "easy_skip_all": false, "easy_skip": [] }), "skyracer"));
        // A game renamed in the catalog keeps its choice: the list follows the new id.
        let mut ui = json!({ "easy_skip": ["hopper"] });
        rename_id(&mut ui, "hopper", "hopper-2");
        assert!(easy_skipped(&ui, "hopper-2"));
    }

    #[test]
    fn a_game_installed_under_the_settings_switch_keeps_skipping() {
        let mut ui = json!({ "easy_skip_all": true });
        assert!(fresh_install_skips(&mut ui, "hopper"));
    }

    #[test]
    fn a_game_installed_without_a_skip_gets_easy_setup() {
        let mut ui = json!({ "easy_skip_all": false, "easy_skip": ["skyracer"] });
        assert!(!fresh_install_skips(&mut ui, "hopper"));
    }

    #[test]
    fn a_ticked_game_skips_and_leaves_the_list() {
        let mut ui = json!({ "easy_skip": ["hopper"] });
        assert!(fresh_install_skips(&mut ui, "hopper"));
        assert!(!easy_skipped(&ui, "hopper"));
    }
}

#[cfg(test)]
mod beta54_tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rl-b54-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_disc_image_unpacked_into_the_game_is_spare_unless_something_else_uses_it() {
        let d = tmp("spare");
        let install = d.join("games/hopper");
        std::fs::create_dir_all(install.join("data")).unwrap();
        std::fs::write(install.join("data/default.xex"), b"x").unwrap();
        let iso = d.join("dumps/Hopper.iso");
        std::fs::create_dir_all(iso.parent().unwrap()).unwrap();
        std::fs::write(&iso, vec![0u8; 2048]).unwrap();
        let b: Value = serde_json::from_str(r#"{"start":"hopper.exe","files":{"how":"folder","folder":"data","accepts":"folder or iso","extract":"xdvdfs","contains":["default.xex"]}}"#).unwrap();
        let mut p = profile::from_boot(&b).unwrap();
        p.lift_game();
        let mut g = Installed { id: "hopper".into(), path: install.clone(), ..Default::default() };
        g.setup_inputs.insert("game".into(), install.join("data"));
        g.unpacked_from.insert("game".into(), iso.clone());
        let spare = spare_copy(&g, &p, std::slice::from_ref(&g)).expect("unpacked, so the image isn't read anymore");
        assert_eq!((spare.why, spare.size, spare.path.clone()), ("unpacked", 2048, iso.clone()));
        // Another game set up with the same image keeps it.
        let mut other = Installed { id: "skyracer".into(), path: d.join("games/skyracer"), ..Default::default() };
        other.setup_inputs.insert("game".into(), iso.clone());
        assert!(spare_copy(&g, &p, &[g.clone(), other]).is_none());
        // Not unpacked (its files aren't in the game's folder): the game still reads the image.
        std::fs::remove_file(install.join("data/default.xex")).unwrap();
        assert!(spare_copy(&g, &p, std::slice::from_ref(&g)).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_same_game_already_in_the_new_place_is_recognized() {
        let d = tmp("same");
        let from = d.join("a/hopper");
        let to = d.join("b/hopper");
        for dir in [&from, &to] {
            std::fs::create_dir_all(dir.join("bin")).unwrap();
            std::fs::write(dir.join("bin/hopper.exe"), b"x").unwrap();
        }
        let mut g = Installed { id: "hopper".into(), path: from.clone(), exe: Some(PathBuf::from("bin/hopper.exe")), ..Default::default() };
        assert!(holds_same_game(&g, &to));
        // Another game's files there aren't this one.
        std::fs::remove_file(to.join("bin/hopper.exe")).unwrap();
        std::fs::write(to.join("bin/skyracer.exe"), b"x").unwrap();
        assert!(!holds_same_game(&g, &to));
        // No program known: everything at the top of its folder is there too.
        g.exe = None;
        assert!(holds_same_game(&g, &to), "bin is in both");
        std::fs::write(from.join("readme.txt"), b"x").unwrap();
        assert!(!holds_same_game(&g, &to));
        assert!(!holds_same_game(&g, &from), "its own folder isn't another place");
        std::fs::remove_file(from.join("readme.txt")).unwrap();
        std::fs::create_dir_all(from.join(swap::FOLDER).join("old")).unwrap();
        assert!(holds_same_game(&g, &to), "old files an update set aside aren't the game's");
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[cfg(test)]
mod saved_soon_tests {
    use super::*;

    fn cat(apps: Value) -> Value {
        json!({ "apps": apps })
    }

    #[test]
    fn coming_soon_ids_leave_out_the_players_own() {
        let c = cat(json!([{ "id": "a", "soon": true }, { "id": "b" }, { "id": "c", "soon": true, "custom": true }]));
        assert_eq!(soon_ids(&c), vec!["a".to_string()]);
    }

    #[test]
    fn a_saved_game_that_came_out_is_told_about() {
        let c = cat(json!([{ "id": "a" }, { "id": "b", "soon": true }]));
        let saved = vec!["a".to_string(), "b".to_string()];
        let was = vec!["a".to_string(), "b".to_string()];
        assert_eq!(saved_out_now(&c, &saved, &was), vec!["a".to_string()]);
    }

    #[test]
    fn only_saved_games_that_were_coming_soon_are_told_about() {
        let c = cat(json!([{ "id": "a" }, { "id": "b" }]));
        // b was never Coming Soon, and c wasn't saved.
        assert!(saved_out_now(&c, &["b".to_string()], &["a".to_string()]).is_empty());
        assert!(saved_out_now(&c, &[], &["a".to_string()]).is_empty());
    }

    #[test]
    fn a_saved_game_that_left_the_catalog_is_not_out() {
        let c = cat(json!([{ "id": "b", "soon": true }]));
        assert!(saved_out_now(&c, &["a".to_string()], &["a".to_string()]).is_empty());
    }

    #[test]
    fn nothing_is_told_before_a_catalog_was_seen() {
        let c = cat(json!([{ "id": "a" }]));
        assert!(saved_out_now(&c, &["a".to_string()], &[]).is_empty());
    }
}

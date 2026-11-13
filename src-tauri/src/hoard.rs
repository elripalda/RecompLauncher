//! Settings → Data Hoarder Mode: keep a copy of every release of every game in the Library.
//!
//! Projects get taken down. With the mode on, the launcher downloads each release of each game the
//! player has (installed, or just added to the Library), newest first and then back through the
//! old ones, into `<root>/<game>/<release>/`, and saves the release's notes there as a Markdown
//! file next to its files. Nothing is installed: installed games keep updating only through the
//! usual update flow, and this never touches their folders.
//!
//! A sweep runs when the mode is turned on, a game is added, updates are checked, the launcher
//! starts, and every few hours after that. It is safe to run again at any time: a file is only
//! counted as saved once it has been downloaded whole and checked (install::download keeps a
//! `.part` file and carries on from it), so a stopped sweep picks up where it left off.

use crate::catalog::{self, Release};
use crate::install::{self, Progress};
use crate::library::{folder_name, Library, Settings};
use crate::{assets, net, trail, App, Error};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager};

/// The file a release's description is saved in.
pub const NOTES_FILE: &str = "RELEASE-NOTES.md";
/// GitHub's own answer for the release, kept as it came (tag, dates, every asset's address).
pub const INFO_FILE: &str = "release.json";
/// How often the launcher sweeps by the clock while it runs.
const SWEEP_EVERY: u64 = 6 * 3600;

/// A game to keep: where its releases come from and what its folder is called.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub id: String,
    pub name: String,
    pub repo: String,
    /// Its folder under the hoard's root.
    pub folder: String,
}

/// What the Settings page shows while a sweep runs, and after it.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Status {
    pub running: bool,
    pub game: String,
    pub release: String,
    pub file: String,
    pub done: u64,
    pub total: u64,
    /// Files saved this sweep.
    pub saved: u32,
    /// Files that couldn't be saved this sweep, each as "game release file: why".
    pub failed: Vec<String>,
    /// Why the sweep stopped early, if it did (rate limit, no space, offline).
    pub stopped: String,
    /// When the last sweep finished (seconds since 1970), 0 if none has.
    pub last_done: u64,
}

static STATUS: Mutex<Status> = Mutex::new(Status {
    running: false,
    game: String::new(),
    release: String::new(),
    file: String::new(),
    done: 0,
    total: 0,
    saved: 0,
    failed: Vec::new(),
    stopped: String::new(),
    last_done: 0,
});
static RUNNING: AtomicBool = AtomicBool::new(false);
/// Something asked for a sweep while one was running: go round once more when it ends.
static AGAIN: AtomicBool = AtomicBool::new(false);

fn stop_flag() -> Arc<AtomicBool> {
    static STOP: OnceLock<Arc<AtomicBool>> = OnceLock::new();
    STOP.get_or_init(|| Arc::new(AtomicBool::new(false))).clone()
}

pub fn status() -> Status {
    STATUS.lock().unwrap().clone()
}

fn update(app: &AppHandle, f: impl FnOnce(&mut Status)) {
    let snapshot = {
        let mut s = STATUS.lock().unwrap();
        f(&mut s);
        s.clone()
    };
    let _ = app.emit("hoard", snapshot);
}

/// A name that is safe as one folder or file name on every system: no separators or characters
/// Windows refuses, no trailing dots or spaces, never empty and never "." or "..".
pub fn safe_name(s: &str) -> String {
    let cleaned: String = s.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
    let trimmed = cleaned.trim().trim_end_matches(['.', ' ']);
    if trimmed.is_empty() || trimmed.chars().all(|c| c == '_') {
        return "unnamed".into();
    }
    // Windows won't make a file or folder named for a device (CON, NUL, COM1...), with or without an extension.
    let stem = trimmed.split('.').next().unwrap_or("").to_ascii_uppercase();
    let device = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str()) || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.ends_with(|c: char| c.is_ascii_digit() && c != '0'));
    if device {
        return format!("_{trimmed}");
    }
    trimmed.to_string()
}

/// Where one release is kept: <root>/<game>/<release>.
pub fn release_dir(root: &Path, game_folder: &str, tag: &str) -> PathBuf {
    root.join(game_folder).join(safe_name(tag))
}

/// The games to keep: everything installed from a project (not a developer's own local build)
/// plus everything added to the Library without being installed (the window keeps that list in
/// the settings' `ui.saved`, by catalog id). One entry per project.
pub fn targets(lib: &Library, catalog: &Value) -> Vec<Target> {
    let mut found: Vec<(String, String, String)> = vec![];
    for g in lib.installed.values().filter(|g| !g.repo.is_empty() && !g.local) {
        found.push((g.id.clone(), g.name.clone(), g.repo.clone()));
    }
    let saved = lib.settings.ui.get("saved").and_then(Value::as_object).cloned().unwrap_or_default();
    for id in saved.keys() {
        let Some(entry) = catalog::app(catalog, id) else { continue };
        let repo = entry.get("repo").and_then(Value::as_str).unwrap_or("");
        let name = entry.get("name").and_then(Value::as_str).unwrap_or(id);
        if !repo.is_empty() {
            found.push((id.clone(), name.to_string(), repo.to_string()));
        }
    }
    let mut repos = HashSet::new();
    let mut folders = HashSet::new();
    let mut out = vec![];
    for (id, name, repo) in found {
        if !repos.insert(repo.to_lowercase()) {
            continue;
        }
        let mut folder = safe_name(&name);
        // Two projects with one name (case aside) must not share a folder.
        if !folders.insert(folder.to_lowercase()) {
            folder = format!("{folder} ({})", folder_name(&id));
            folders.insert(folder.to_lowercase());
        }
        out.push(Target { id, name, repo, folder });
    }
    out
}

/// The release's description as a Markdown file: a short header, then the notes as the developer
/// wrote them.
pub fn notes_markdown(game: &str, rel: &Release) -> String {
    let title = rel.name.as_deref().filter(|n| !n.trim().is_empty()).unwrap_or(&rel.tag_name);
    let mut out = format!("# {game}: {title}\n\n- Release: `{}`\n", rel.tag_name);
    if let Some(when) = rel.published_at.as_deref() {
        out.push_str(&format!("- Published: {when}\n"));
    }
    if rel.prerelease {
        out.push_str("- Pre-release: yes\n");
    }
    if let Some(url) = rel.html_url.as_deref() {
        out.push_str(&format!("- Page: {url}\n"));
    }
    out.push_str("\n---\n\n");
    let body = rel.body.as_deref().map(str::trim).unwrap_or("");
    if body.is_empty() {
        out.push_str("_The developer didn't write any notes for this release._\n");
    } else {
        out.push_str(body);
        out.push('\n');
    }
    out
}

/// The files of a release to keep: all of them, or (`this_pc_only`) the builds that run here.
pub fn wanted_assets(rel: &Release, this_pc_only: bool) -> Vec<assets::Asset> {
    if !this_pc_only {
        return rel.assets.clone();
    }
    assets::all_for(&rel.assets, assets::os_now(), assets::arch_now(), None).into_iter().cloned().collect()
}

/// Already kept whole: the file is there under its saved name and, when the size is known, as big
/// as it should be. (A download in progress is a `.part` file until it has been checked.)
pub fn asset_saved(dir: &Path, asset: &assets::Asset) -> bool {
    let Ok(meta) = std::fs::metadata(dir.join(install::file_name_for(&asset.name))) else { return false };
    asset.size == 0 || meta.len() == asset.size
}

/// A file written whole or not at all: a stopped sweep never leaves half a notes file.
fn write_if_changed(path: &Path, text: &[u8]) -> std::io::Result<()> {
    if std::fs::read(path).map(|old| old == text).unwrap_or(false) {
        return Ok(());
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// Ask for a sweep. Does nothing when the mode is off. `app` is the running launcher.
pub fn kick(app: &AppHandle) {
    let on = app.state::<App>().lib.lock().unwrap().settings.hoarder;
    if !on {
        return;
    }
    if RUNNING.swap(true, Ordering::SeqCst) {
        AGAIN.store(true, Ordering::SeqCst);
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            AGAIN.store(false, Ordering::SeqCst);
            stop_flag().store(false, Ordering::SeqCst);
            sweep(&app).await;
            if !AGAIN.load(Ordering::SeqCst) || !app.state::<App>().lib.lock().unwrap().settings.hoarder {
                break;
            }
        }
        RUNNING.store(false, Ordering::SeqCst);
    });
}

/// The mode was turned off: the sweep stops after the file it's on (a half-done one carries on from
/// its `.part` file if the mode is turned on again).
pub fn stop() {
    // A plain cancel throws the half-downloaded file away. Pausing it first keeps the `.part` file,
    // so a sweep after the mode comes back on carries on from where this one got to.
    let id = CURRENT.lock().unwrap().clone();
    if !id.is_empty() {
        install::pause(&id);
    }
    stop_flag().store(true, Ordering::SeqCst);
}

/// The download id of the file being saved right now, empty between files (see `stop`).
static CURRENT: Mutex<String> = Mutex::new(String::new());

/// The launcher's clock for it: a first look shortly after start, then every few hours.
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(45)).await;
        loop {
            kick(&app);
            tokio::time::sleep(net::spread(SWEEP_EVERY, 0.1)).await;
        }
    });
}

fn enabled(app: &AppHandle) -> bool {
    app.state::<App>().lib.lock().unwrap().settings.hoarder && !stop_flag().load(Ordering::SeqCst)
}

async fn sweep(app: &AppHandle) {
    update(app, |s| *s = Status { running: true, last_done: s.last_done, ..Status::default() });
    let st = app.state::<App>();
    let finish = |app: &AppHandle, why: &str| {
        update(app, |s| {
            s.running = false;
            s.game.clear();
            s.release.clear();
            s.file.clear();
            s.stopped = why.to_string();
            s.last_done = crate::library::now();
        });
    };
    if net::offline() {
        return finish(app, "Offline");
    }
    let cat = catalog::catalog(&st.http, &st.dir, false).await.map(|f| f.data).unwrap_or(Value::Null);
    let (list, settings) = {
        let l = st.lib.lock().unwrap();
        (targets(&l, &cat), l.settings.clone())
    };
    trail::note(format!("Data Hoarder: sweeping {} games into {}", list.len(), settings.hoard_root().display()));
    for t in list {
        if !enabled(app) {
            return finish(app, "");
        }
        if let Err(why) = hoard_game(app, &settings, &t).await {
            trail::note(format!("Data Hoarder: stopped at {}: {why}", t.name));
            return finish(app, &why);
        }
    }
    trail::note("Data Hoarder: sweep finished");
    finish(app, "");
}

/// A trouble that will be the same for the next game too, so the sweep stops there rather than
/// asking again for each one (GitHub's hourly limit, a full drive).
fn stops_sweep(msg: &str) -> bool {
    let low = msg.to_lowercase();
    ["429", "403", "rate limit", "is busy", "enough space", "isn't connected"].iter().any(|k| low.contains(k))
}

/// All of one game's releases. Returns Err(why) only when the whole sweep should stop.
async fn hoard_game(app: &AppHandle, settings: &Settings, t: &Target) -> Result<(), String> {
    let st = app.state::<App>();
    let raw = match catalog::releases_all(&st.http, &st.dir, &t.repo).await {
        Ok(r) => r,
        Err(e) => {
            // This game's list couldn't be read: the next game may well be fine.
            let why = format!("{}: couldn't read its releases ({e})", t.name);
            update(app, |s| s.failed.push(why));
            return if stops_sweep(&e.to_string()) { Err(e.to_string()) } else { Ok(()) };
        }
    };
    for (rel, value) in newest_kept(raw, settings.hoard_limit()) {
        if !enabled(app) {
            return Ok(());
        }
        hoard_release(app, settings, t, &rel, &value).await?;
    }
    Ok(())
}

/// The releases to keep, newest first: drafts never count, and `limit` (None for all) cuts the list
/// to the newest few. The list comes newest first, so no sorting is needed.
pub fn newest_kept(raw: Vec<Value>, limit: Option<usize>) -> Vec<(Release, Value)> {
    let parsed = raw.into_iter().filter_map(|v| serde_json::from_value::<Release>(v.clone()).ok().map(|r| (r, v)));
    parsed.filter(|(r, _)| !r.draft).take(limit.unwrap_or(usize::MAX)).collect()
}

async fn hoard_release(app: &AppHandle, settings: &Settings, t: &Target, rel: &Release, raw: &Value) -> Result<(), String> {
    let st = app.state::<App>();
    let dir = release_dir(&settings.hoard_root(), &t.folder, &rel.tag_name);
    let missing: Vec<assets::Asset> = wanted_assets(rel, settings.hoard_this_pc_only).into_iter().filter(|a| !asset_saved(&dir, a)).collect();
    let notes = notes_markdown(&t.name, rel);
    let info = serde_json::to_vec_pretty(raw).unwrap_or_default();
    // Notes and info are small, so they're kept current every sweep (a developer can edit a release's
    // description after it's out).
    let wrote = std::fs::create_dir_all(&dir).and_then(|_| write_if_changed(&dir.join(NOTES_FILE), notes.as_bytes())).and_then(|_| write_if_changed(&dir.join(INFO_FILE), &info));
    if let Err(e) = wrote {
        let why = format!("Couldn't write in {}: {e}", dir.display());
        update(app, |s| s.failed.push(why.clone()));
        return Err(why);
    }
    for asset in missing {
        if !enabled(app) {
            return Ok(());
        }
        // A project's files come only from that project (the same rule as installing).
        if !catalog::own_download(&t.repo, &asset.browser_download_url, rel.project_id) {
            continue;
        }
        if let Some(free) = crate::manage::free_space(&dir) {
            if free < asset.size.saturating_add(asset.size / 10).saturating_add(64 << 20) {
                return Err(format!("There isn't enough space on the drive for {}. Free some up or pick another folder.", dir.display()));
            }
        }
        update(app, |s| {
            s.game = t.name.clone();
            s.release = rel.tag_name.clone();
            s.file = asset.name.clone();
            s.done = 0;
            s.total = asset.size;
        });
        // One download at a time, shared with installs: a player's own install waits for at most
        // the file being saved now.
        let Ok(permit) = st.download_slot.clone().acquire_owned().await else { return Ok(()) };
        // The mode may have been turned off while this waited for its turn.
        if !enabled(app) {
            return Ok(());
        }
        let id = format!("hoard:{}", t.id);
        *CURRENT.lock().unwrap() = id.clone();
        let report = |p: Progress| {
            let (done, total) = (p.done, p.total);
            update(app, |s| {
                s.done = done;
                s.total = total.max(s.total);
            });
        };
        let got = install::download(&st.http, &asset, &dir, &id, stop_flag(), &report).await;
        drop(permit);
        CURRENT.lock().unwrap().clear();
        // Paused by `stop`: its part file stays, and nothing is left registered under this id.
        install::unpause(&id);
        install::resumed(&id);
        match got {
            Ok(_) => update(app, |s| s.saved += 1),
            Err(Error::Cancelled) => return Ok(()),
            Err(Error::Msg(m)) if m == "PAUSED" => return Ok(()),
            Err(e) => {
                let msg = e.to_string();
                trail::note(format!("Data Hoarder: {} {} {}: {msg}", t.name, rel.tag_name, asset.name));
                update(app, |s| s.failed.push(format!("{} {} {}: {msg}", t.name, rel.tag_name, asset.name)));
                if stops_sweep(&msg) {
                    return Err(msg);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Installed;
    use serde_json::json;

    fn release(tag: &str, body: Option<&str>) -> Release {
        serde_json::from_value(json!({ "tag_name": tag, "name": "Big one", "published_at": "2024-05-01T00:00:00Z", "body": body, "html_url": "https://github.com/a/b/releases/tag/v1", "assets": [] })).unwrap()
    }

    fn asset(name: &str, size: u64) -> assets::Asset {
        serde_json::from_value(json!({ "name": name, "size": size, "browser_download_url": format!("https://github.com/a/b/releases/download/v1/{name}") })).unwrap()
    }

    #[test]
    fn a_name_never_escapes_its_folder() {
        assert_eq!(safe_name("../../etc"), ".._.._etc");
        assert!(!safe_name("a/b\\c").contains(['/', '\\']));
        assert_eq!(safe_name(".."), "unnamed");
        assert_eq!(safe_name("   "), "unnamed");
    }

    #[test]
    fn windows_device_names_are_not_used_as_they_are() {
        assert_eq!(safe_name("con"), "_con");
        assert_eq!(safe_name("NUL.txt"), "_NUL.txt");
        assert_eq!(safe_name("COM3"), "_COM3");
        assert_eq!(safe_name("Console"), "Console");
        assert_eq!(safe_name("COM0"), "COM0");
    }

    #[test]
    fn windows_forbidden_characters_are_replaced() {
        assert_eq!(safe_name("Zelda 64: Recompiled?"), "Zelda 64_ Recompiled_");
        assert_eq!(safe_name("name. "), "name");
    }

    #[test]
    fn a_release_lives_in_game_then_release() {
        let dir = release_dir(Path::new("/hoard"), "Game", "v1.2/rc");
        assert_eq!(dir, Path::new("/hoard/Game/v1.2_rc"));
    }

    #[test]
    fn notes_keep_the_developers_words() {
        let md = notes_markdown("Game", &release("v1", Some("## Fixed\n- a thing")));
        assert!(md.starts_with("# Game: Big one"));
        assert!(md.contains("- Release: `v1`"));
        assert!(md.ends_with("## Fixed\n- a thing\n"));
    }

    #[test]
    fn empty_notes_say_so() {
        let md = notes_markdown("Game", &release("v1", None));
        assert!(md.contains("didn't write any notes"));
    }

    #[test]
    fn a_file_is_saved_only_when_whole() {
        let dir = std::env::temp_dir().join(format!("hoard-saved-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = asset("game 1.zip", 5);
        assert!(!asset_saved(&dir, &a));
        std::fs::write(dir.join("game_1.zip"), b"123").unwrap();
        assert!(!asset_saved(&dir, &a));
        std::fs::write(dir.join("game_1.zip"), b"12345").unwrap();
        assert!(asset_saved(&dir, &a));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_of_unknown_size_counts_when_it_is_there() {
        let dir = std::env::temp_dir().join(format!("hoard-unknown-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("x.zip"), b"1").unwrap();
        assert!(asset_saved(&dir, &asset("x.zip", 0)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn notes_are_not_rewritten_when_unchanged() {
        let dir = std::env::temp_dir().join(format!("hoard-notes-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join(NOTES_FILE);
        write_if_changed(&f, b"one").unwrap();
        write_if_changed(&f, b"two").unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"two");
        assert!(!dir.join("RELEASE-NOTES.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn library_with(installed: &[(&str, &str, &str, bool)], saved: &[&str]) -> Library {
        let mut lib = Library::default();
        for (id, name, repo, local) in installed {
            lib.installed.insert(id.to_string(), Installed { id: id.to_string(), name: name.to_string(), repo: repo.to_string(), local: *local, ..Installed::default() });
        }
        let map: serde_json::Map<String, Value> = saved.iter().map(|id| (id.to_string(), json!(1))).collect();
        lib.settings.ui = json!({ "saved": map });
        lib
    }

    fn catalog_of(apps: &[(&str, &str, &str)]) -> Value {
        json!({ "apps": apps.iter().map(|(id, name, repo)| json!({ "id": id, "name": name, "repo": repo })).collect::<Vec<_>>() })
    }

    #[test]
    fn installed_and_added_games_are_both_kept() {
        let lib = library_with(&[("a", "Alpha", "x/alpha", false)], &["b"]);
        let cat = catalog_of(&[("b", "Beta", "x/beta")]);
        let list = targets(&lib, &cat);
        let repos: Vec<&str> = list.iter().map(|t| t.repo.as_str()).collect();
        assert_eq!(repos, ["x/alpha", "x/beta"]);
    }

    #[test]
    fn a_local_build_has_no_releases_to_keep() {
        let lib = library_with(&[("a", "Alpha", "x/alpha", true)], &[]);
        assert!(targets(&lib, &Value::Null).is_empty());
    }

    #[test]
    fn a_project_installed_and_added_is_kept_once() {
        let lib = library_with(&[("a", "Alpha", "x/alpha", false)], &["a2"]);
        let cat = catalog_of(&[("a2", "Alpha again", "X/Alpha")]);
        assert_eq!(targets(&lib, &cat).len(), 1);
    }

    #[test]
    fn two_projects_with_one_name_get_two_folders() {
        let lib = library_with(&[("a", "Same", "x/one", false), ("b", "same", "y/two", false)], &[]);
        let list = targets(&lib, &Value::Null);
        assert_eq!(list.len(), 2);
        assert_ne!(list[0].folder.to_lowercase(), list[1].folder.to_lowercase());
    }

    #[test]
    fn an_added_game_the_catalog_no_longer_lists_is_skipped() {
        let lib = library_with(&[], &["gone"]);
        assert!(targets(&lib, &catalog_of(&[])).is_empty());
    }

    #[test]
    fn this_pc_only_leaves_other_systems_out() {
        let mut rel = release("v1", None);
        rel.assets = vec![asset("Game-Windows.zip", 1), asset("Game-Linux.tar.gz", 1), asset("Game-macOS.zip", 1)];
        assert_eq!(wanted_assets(&rel, false).len(), 3);
        assert_eq!(wanted_assets(&rel, true).len(), 1);
    }

    #[test]
    fn the_root_follows_the_library_unless_picked() {
        let mut s = Settings { library_dir: PathBuf::from("/games"), ..Settings::default() };
        assert_eq!(s.hoard_root(), Path::new("/games/Releases"));
        s.hoard_dir = Some(PathBuf::from("/vault"));
        assert_eq!(s.hoard_root(), Path::new("/vault"));
    }

    fn list(tags: &[&str]) -> Vec<Value> {
        tags.iter().map(|t| json!({ "tag_name": t, "draft": *t == "draft", "assets": [] })).collect()
    }

    fn tags(kept: &[(Release, Value)]) -> Vec<&str> {
        kept.iter().map(|(r, _)| r.tag_name.as_str()).collect()
    }

    #[test]
    fn all_keeps_every_release_but_drafts() {
        let kept = newest_kept(list(&["v3", "draft", "v2", "v1"]), None);
        assert_eq!(tags(&kept), ["v3", "v2", "v1"]);
    }

    #[test]
    fn latest_keeps_only_the_newest() {
        let kept = newest_kept(list(&["draft", "v3", "v2"]), Some(1));
        assert_eq!(tags(&kept), ["v3"]);
    }

    #[test]
    fn a_number_keeps_that_many_newest() {
        let kept = newest_kept(list(&["v4", "v3", "v2", "v1"]), Some(2));
        assert_eq!(tags(&kept), ["v4", "v3"]);
    }

    #[test]
    fn a_number_larger_than_the_list_keeps_all() {
        assert_eq!(newest_kept(list(&["v2", "v1"]), Some(50)).len(), 2);
    }

    #[test]
    fn the_keep_setting_becomes_a_limit() {
        let mut s = Settings::default();
        assert_eq!(s.hoard_limit(), None);
        s.hoard_keep = "latest".into();
        assert_eq!(s.hoard_limit(), Some(1));
        s.hoard_keep = "count".into();
        s.hoard_count = 7;
        assert_eq!(s.hoard_limit(), Some(7));
        s.hoard_count = 0;
        assert_eq!(s.hoard_limit(), Some(1));
        s.hoard_keep = "nonsense".into();
        assert_eq!(s.hoard_limit(), None);
    }

    #[test]
    fn a_full_drive_or_a_rate_limit_stops_the_sweep() {
        assert!(stops_sweep("GitHub answered 403"));
        assert!(stops_sweep("There isn't enough space on that drive"));
        assert!(!stops_sweep("The download of x.zip didn't start"));
    }
}

//! Resolve a game folder once and retain ownership of unfinished installations across restarts.

use crate::library::{folder_name, Library};
use crate::{manage, Error, Res};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

const MARKER: &str = ".recomplauncher-install";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub target: PathBuf,
    pub tag: Option<String>,
    #[serde(default)]
    pub selected_tag: Option<String>,
    #[serde(default)]
    pub backup_release: Option<crate::catalog::Release>,
    pub file: Option<String>,
    pub token: String,
    #[serde(default)]
    pub source: Option<PathBuf>,
    pub anchor: PathBuf,
    pub volume: Option<u64>,
}

impl Request {
    pub fn installed_path(&self, previous: Option<&crate::library::Installed>) -> PathBuf {
        // Stored source paths use the saved folder's spelling, including aliases. Keep them aligned.
        previous.map(|game| game.path.clone()).unwrap_or_else(|| self.target.clone())
    }

    pub fn release_tag(&self) -> Option<String> {
        self.backup_release.as_ref().map(|r| r.tag_name.clone()).or(self.tag.clone()).or(self.selected_tag.clone())
    }

    pub fn remember_release(&mut self, tag: &str, file: Option<&str>, backup: Option<&crate::catalog::Release>) {
        if self.tag.is_some() {
            self.tag = Some(tag.into());
        }
        self.selected_tag = Some(tag.into());
        if let Some(file) = file {
            self.file = Some(file.into());
        }
        if let Some(backup) = backup {
            self.backup_release = Some(backup.clone());
        } else if self.backup_release.as_ref().is_some_and(|r| r.tag_name != tag) {
            self.backup_release = None;
        }
    }

    pub async fn releases<B, U>(&self, backup_enabled: bool, backup: impl FnOnce() -> B, upstream: impl FnOnce() -> U) -> Res<(Vec<crate::catalog::Release>, bool)>
    where
        B: std::future::Future<Output = Res<crate::catalog::Release>>,
        U: std::future::Future<Output = Res<Vec<crate::catalog::Release>>>,
    {
        // A checked backup can be the only surviving source. Retrying must not depend on upstream.
        if let Some(release) = &self.backup_release {
            return Ok((vec![release.clone()], true));
        }
        if backup_enabled && self.tag.is_none() && self.selected_tag.is_none() {
            match backup().await {
                Ok(release) => return Ok((vec![release], true)),
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                Err(e) => crate::trail::note(format!("The RecompLauncher backup couldn't help ({e}), so trying the project's releases")),
            }
        }
        Ok((upstream().await?, false))
    }
}

#[derive(Serialize)]
pub struct Preview {
    pub target: PathBuf,
    pub free: Option<u64>,
}

// Canonicalize the existing part too, so an alias to another game's folder is still that folder.
pub fn normalized(path: &Path) -> Res<PathBuf> {
    if !path.is_absolute() || path.components().any(|c| c == Component::ParentDir) {
        return Err(Error::Msg("Choose an absolute game folder without '..'.".into()));
    }
    let mut existing = path;
    let mut missing = Vec::new();
    while !existing.try_exists()? {
        // A dangling link must not become a new directory if its drive comes back later.
        if existing.symlink_metadata().is_ok() {
            return Err(Error::Msg("That folder points to a location that isn't available.".into()));
        }
        missing.push(existing.file_name().ok_or_else(|| Error::Msg("Choose a game folder on a connected drive.".into()))?);
        existing = existing.parent().ok_or_else(|| Error::Msg("Choose a game folder on a connected drive.".into()))?;
    }
    let mut full = existing.canonicalize()?;
    // Programs receiving {install} often cannot read Windows' verbatim \\?\ spelling.
    #[cfg(windows)]
    {
        let text = full.to_string_lossy();
        full = if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            PathBuf::from(format!(r"\\{rest}"))
        } else if let Some(rest) = text.strip_prefix(r"\\?\") {
            PathBuf::from(rest)
        } else {
            full
        };
    }
    for part in missing.into_iter().rev() {
        full.push(part);
    }
    Ok(full)
}

fn names(path: &Path) -> Vec<String> {
    path.components().filter(|c| *c != Component::CurDir).map(|c| {
        let name = c.as_os_str().to_string_lossy().into_owned();
        if cfg!(windows) { name.to_lowercase() } else { name }
    }).collect()
}

pub fn same(a: &Path, b: &Path) -> bool {
    names(a) == names(b)
}

pub fn same_location(a: &Path, b: &Path) -> bool {
    match (normalized(a), normalized(b)) {
        (Ok(a), Ok(b)) => same(&a, &b),
        _ => false,
    }
}

pub fn overlaps(a: &Path, b: &Path) -> bool {
    let (a, b) = (names(a), names(b));
    a.starts_with(&b) || b.starts_with(&a)
}

pub fn conflicts(target: &Path, other: &Path) -> bool {
    normalized(other).map(|p| overlaps(target, &p)).unwrap_or_else(|_| overlaps(target, other))
}

pub fn resolve(lib: &Library, id: &str, dir: Option<&Path>, target: Option<&Path>) -> Res<PathBuf> {
    if dir.is_some() && target.is_some() {
        return Err(Error::Msg("Choose either a library or an exact game folder.".into()));
    }
    if let Some(game) = lib.installed.get(id) {
        let existing = normalized(&game.path)?;
        if target.is_some_and(|p| normalized(p).map(|p| !same(&p, &existing)).unwrap_or(true)) {
            return Err(Error::Msg("Use Move to change an installed game's folder.".into()));
        }
        return Ok(existing);
    }
    let chosen = match (dir, target) {
        (_, Some(path)) => path.to_path_buf(),
        (Some(parent), _) => parent.join(folder_name(id)),
        _ => lib.pending_installs.get(id).map(|r| r.target.clone()).unwrap_or_else(|| lib.settings.library_dir.join(folder_name(id))),
    };
    normalized(&chosen)
}

pub fn validate(lib: &Library, app_dir: &Path, id: &str, target: &Path) -> Res<()> {
    if target.parent().is_none() || target.file_name().is_none() {
        return Err(Error::Msg("Choose a folder for this game, rather than a drive root.".into()));
    }
    let reserved = target.components().any(|c| {
        let name = c.as_os_str().to_string_lossy().to_lowercase();
        [".downloads", ".staging", ".backups"].contains(&name.as_str()) || name.starts_with(".recomplauncher-work-")
    });
    if reserved || conflicts(target, app_dir) {
        return Err(Error::Msg("Choose a game folder outside the launcher's data and work folders.".into()));
    }
    let libraries = lib.settings.all_libraries();
    if crate::library::shared_folder(target, &libraries) || libraries.iter().any(|p| normalized(p).is_ok_and(|p| same(target, &p))) {
        return Err(Error::Msg("Choose a separate game folder inside that location.".into()));
    }
    if let Some((_, game)) = lib.installed.iter().find(|(other, g)| other.as_str() != id && conflicts(target, &g.path)) {
        return Err(Error::Msg(format!("That folder overlaps {}. Choose a separate game folder.", game.name)));
    }
    if lib.installed.contains_key(id) {
        if !target.is_dir() {
            return Err(Error::Msg("The game's drive or folder isn't available. Connect it before updating.".into()));
        }
        return Ok(());
    }
    for game in lib.installed.values() {
        let sources = game.game_copy.iter().chain(game.update_copy.iter()).chain(game.setup_inputs.values());
        for source in sources {
            if !crate::library::inside(source, &game.path) && conflicts(target, source) {
                return Err(Error::Msg("Choose a game folder separate from your original game files.".into()));
            }
        }
    }
    if lib.roms.iter().any(|r| conflicts(target, &r.path)) {
        return Err(Error::Msg("Choose a game folder separate from your original game files.".into()));
    }
    if lib.pending_installs.iter().any(|(other, r)| other != id && conflicts(target, &r.target)) {
        return Err(Error::Msg("Another unfinished installation is using that folder. Choose a separate game folder.".into()));
    }
    if let Some(source) = lib.pending_installs.get(id).and_then(|r| r.source.as_deref()) {
        validate_source(target, Some(source))?;
    }
    if !target.try_exists()? {
        return Ok(());
    }
    let pending = lib.pending_installs.get(id).filter(|r| same(&r.target, target));
    if pending.is_some_and(|r| owns(target, &r.token)) {
        return Ok(());
    }
    if fs::read_dir(target)?.next().is_some() {
        return Err(Error::Msg("That folder already has files in it. Choose an empty folder, or use I Already Have It to add an existing build.".into()));
    }
    Ok(())
}

pub fn validate_source(target: &Path, source: Option<&Path>) -> Res<()> {
    if source.is_some_and(|p| conflicts(target, p)) {
        return Err(Error::Msg("Choose a game folder separate from your original game files.".into()));
    }
    Ok(())
}

pub fn request(lib: &Library, id: &str, target: PathBuf, tag: Option<String>, file: Option<String>, reset_release: bool) -> Request {
    let previous = lib.pending_installs.get(id);
    let location = previous.filter(|r| same(&r.target, &target));
    let same_release = !reset_release && tag.as_ref().map(|t| previous.is_some_and(|r| r.tag.as_ref() == Some(t) || r.selected_tag.as_ref() == Some(t))).unwrap_or(true);
    let token = location.map(|r| r.token.clone()).unwrap_or_else(|| {
        let time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        format!("{}-{time}-{}", folder_name(id), std::process::id())
    });
    let anchor = location.map(|r| r.anchor.clone()).unwrap_or_else(|| {
        target.ancestors().find(|p| p.is_dir()).unwrap_or(&target).to_path_buf()
    });
    let volume = location.and_then(|r| r.volume).or_else(|| volume(&anchor));
    Request {
        target,
        tag: tag.or_else(|| previous.filter(|_| same_release).and_then(|r| r.tag.clone())),
        selected_tag: previous.filter(|_| same_release).and_then(|r| r.selected_tag.clone()),
        backup_release: previous.filter(|_| same_release).and_then(|r| r.backup_release.clone()),
        file: file.or_else(|| previous.filter(|_| same_release).and_then(|r| r.file.clone())),
        token,
        source: previous.and_then(|r| r.source.clone()),
        anchor,
        volume,
    }
}

fn volume(path: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        fs::metadata(path).ok().map(|m| m.dev())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

fn owns(dir: &Path, token: &str) -> bool {
    if !dir.symlink_metadata().is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink()) {
        return false;
    }
    let marker = dir.join(MARKER);
    marker.symlink_metadata().map(|m| m.is_file() && !m.file_type().is_symlink()).unwrap_or(false)
        && fs::read_to_string(marker).is_ok_and(|s| s == token)
}

fn mark(dir: &Path, token: &str) -> Res<()> {
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(dir.join(MARKER))?;
    file.write_all(token.as_bytes())?;
    Ok(())
}

pub fn writable(target: &Path) -> Res<()> {
    let parent = target.parent().ok_or_else(|| Error::Msg("Choose a game folder.".into()))?;
    if !target.is_dir() {
        fs::create_dir_all(parent)?;
    }
    let time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    let place = if target.is_dir() { target } else { parent };
    let probe = place.join(format!(".recomplauncher-probe-{}-{time}", std::process::id()));
    let file = fs::OpenOptions::new().write(true).create_new(true).open(&probe)?;
    drop(file);
    fs::remove_file(probe)?;
    Ok(())
}

fn work_dir(request: &Request, app_dir: &Path) -> PathBuf {
    app_dir.join("cache").join("install-work").join(&request.token)
}

pub fn claim(request: &Request, installed: bool, app_dir: &Path) -> Res<PathBuf> {
    if !request.anchor.is_dir() || request.volume.is_some_and(|v| volume(&request.anchor) != Some(v)) {
        return Err(Error::Msg("The selected drive isn't available. Reconnect it before retrying this installation.".into()));
    }
    if normalized(&request.target)? != request.target {
        return Err(Error::Msg("The selected folder points somewhere else now. Choose the game folder again.".into()));
    }
    writable(&request.target)?;
    if !installed && !owns(&request.target, &request.token) {
        fs::create_dir_all(&request.target)?;
        if fs::read_dir(&request.target)?.next().is_some() {
            return Err(Error::Msg("The selected folder changed. Choose an empty game folder and try again.".into()));
        }
        mark(&request.target, &request.token)?;
    }
    // Folder pickers can grant only the target. Temporary files must not require its parent.
    let work = work_dir(request, app_dir);
    fs::create_dir_all(work.parent().unwrap())?;
    match fs::create_dir(&work) {
        Ok(()) => mark(&work, &request.token)?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && owns(&work, &request.token) => {}
        Err(e) => return Err(e.into()),
    }
    Ok(work)
}

pub fn preview(target: PathBuf) -> Preview {
    Preview { free: manage::free_space(&target), target }
}

pub fn finish(request: &Request, keep_downloads: bool, app_dir: &Path) {
    if owns(&request.target, &request.token) {
        let _ = fs::remove_file(request.target.join(MARKER));
    }
    let work = work_dir(request, app_dir);
    if !keep_downloads && owns(&work, &request.token) {
        let _ = fs::remove_dir_all(work);
    }
}

pub fn complete(lib: &mut Library, id: &str, game: crate::library::Installed, request: &Request, keep_downloads: bool, app_dir: &Path) -> Res<()> {
    let before = lib.clone();
    lib.put(id, game);
    lib.settings.asked_location = true;
    lib.pending_installs.remove(id);
    if let Err(error) = lib.save(app_dir) {
        // A populated first-install folder stays recoverable until the installed record is durable.
        *lib = before;
        return Err(error.into());
    }
    finish(request, keep_downloads, app_dir);
    Ok(())
}

pub fn discard_empty(request: &Request, app_dir: &Path) -> bool {
    if request.target.exists() {
        let Ok(mut entries) = fs::read_dir(&request.target) else { return false };
        let only_marker = entries.all(|e| e.is_ok_and(|e| e.file_name() == MARKER));
        if !only_marker || (request.target.join(MARKER).exists() && !owns(&request.target, &request.token)) {
            return false;
        }
    }
    // Cancelling a download releases its reservation, but a partially unpacked game stays recoverable.
    finish(request, false, app_dir);
    true
}

pub fn forget(lib: &mut Library, id: &str, app_dir: &Path) -> Option<crate::library::Installed> {
    clear_pending(lib, id, app_dir);
    lib.forget(id)
}

pub fn clear_pending(lib: &mut Library, id: &str, app_dir: &Path) {
    if let Some(request) = lib.pending_installs.remove(id) {
        finish(&request, false, app_dir);
    }
}

pub fn ensure_movable(lib: &Library, id: &str) -> Res<()> {
    if lib.pending_installs.contains_key(id) {
        return Err(Error::Msg("Finish or cancel this game's pending update before moving it.".into()));
    }
    Ok(())
}

pub fn reserve(lib: &mut Library, id: &str, request: Request, app_dir: &Path) -> Res<()> {
    if let Some(previous) = lib.pending_installs.get(id).filter(|r| !same(&r.target, &request.target)) {
        let available = previous.target.ancestors().find(|path| path.is_dir());
        if available.is_none() || previous.volume.is_some_and(|v| available.and_then(volume) != Some(v)) {
            return Err(Error::Msg("Reconnect the previous installation's drive before choosing another folder.".into()));
        }
        previous.target.try_exists()?;
        // A partial game cannot lose its only ownership record. Download-only work can be abandoned.
        if !discard_empty(previous, app_dir) {
            return Err(Error::Msg(format!("An unfinished extraction is still in {}. Retry there, or remove that partial folder before choosing another location.", previous.target.display())));
        }
        if work_dir(previous, app_dir).try_exists()? {
            return Err(Error::Msg("The previous download's work folder couldn't be cleared. Try again before choosing another destination.".into()));
        }
    }
    lib.pending_installs.insert(id.into(), request);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Installed;

    fn temp() -> PathBuf {
        let time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("recomp-location-{}-{time}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        normalized(&path).unwrap()
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn updating_through_a_folder_alias_keeps_original_files_protected_on_uninstall() {
        let root = temp();
        let actual = root.join("actual");
        let alias = root.join("alias");
        fs::create_dir_all(actual.join("game")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&actual, &alias).unwrap();
        #[cfg(windows)]
        crate::win::junction(&actual, &alias).unwrap();
        let path = alias.join("game");
        let copy = path.join("original.iso");
        let update = path.join("update.iso");
        fs::write(&copy, b"original").unwrap();
        fs::write(&update, b"update").unwrap();
        let mut lib = Library::default();
        let mut game = Installed { id: "game".into(), path: path.clone(), game_copy: Some(copy.clone()), update_copy: Some(update.clone()), managed_install: true, ..Default::default() };
        lib.installed.insert("game".into(), game.clone());
        let target = resolve(&lib, "game", None, None).unwrap();
        assert_eq!(target, actual.join("game"));
        let pending = request(&lib, "game", target, Some("v2".into()), None, false);
        game.path = pending.installed_path(Some(&game));
        game.tag = "v2".into();
        lib.installed.insert("game".into(), game);
        let app_dir = root.join("data");
        lib.save(&app_dir).unwrap();
        let loaded = Library::load(&app_dir);
        let saved = &loaded.installed["game"];
        assert_eq!(saved.path, path);
        assert!(saved.owns_folder(&[]));
        assert!(crate::library::inside(&copy, &saved.path));
        assert!(crate::library::inside(&update, &saved.path));
        let kept_copy = crate::save_copy_out(&saved.path, "game", &copy).unwrap();
        let kept_update = crate::save_copy_out(&saved.path, "game", &update).unwrap();
        assert!(crate::remove_game_folder(&saved.path).is_empty());
        assert!(!actual.join("game").exists());
        assert_eq!(fs::read(kept_copy).unwrap(), b"original");
        assert_eq!(fs::read(kept_update).unwrap(), b"update");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_final_save_preserves_owned_files_and_retry_state_across_restart() {
        let root = temp();
        let app_dir = root.join("data");
        let target = root.join("game");
        let mut lib = Library::default();
        let pending = request(&lib, "game", target.clone(), None, None, false);
        let work = claim(&pending, false, &app_dir).unwrap();
        fs::write(target.join("game.exe"), b"game").unwrap();
        fs::write(work.join("download"), b"archive").unwrap();
        lib.pending_installs.insert("game".into(), pending.clone());
        lib.save(&app_dir).unwrap();
        fs::create_dir(app_dir.join("library.json.tmp")).unwrap();
        let game = Installed { id: "game".into(), path: target.clone(), managed_install: true, ..Default::default() };
        assert!(complete(&mut lib, "game", game.clone(), &pending, false, &app_dir).is_err());
        assert!(!lib.installed.contains_key("game"));
        assert_eq!(lib.pending_installs["game"].token, pending.token);
        assert!(owns(&target, &pending.token));
        assert_eq!(fs::read(work.join("download")).unwrap(), b"archive");
        let mut restarted = Library::load(&app_dir);
        validate(&restarted, &app_dir, "game", &target).unwrap();
        claim(&restarted.pending_installs["game"], false, &app_dir).unwrap();
        fs::remove_dir(app_dir.join("library.json.tmp")).unwrap();
        complete(&mut restarted, "game", game, &pending, false, &app_dir).unwrap();
        assert!(!target.join(MARKER).exists());
        assert!(!work.exists());
        let saved = Library::load(&app_dir);
        assert_eq!(saved.installed["game"].path, target);
        assert!(saved.pending_installs.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refreshed_asset_is_saved_before_an_interrupted_replacement_download_resumes() {
        let root = temp();
        let app_dir = root.join("data");
        let mut lib = Library::default();
        let mut pending = request(&lib, "game", root.join("game"), None, None, false);
        pending.remember_release("v1", Some("removed.zip"), None);
        pending.remember_release("v1", Some("renamed.zip"), None);
        lib.pending_installs.insert("game".into(), pending);
        lib.save(&app_dir).unwrap();
        let restarted = Library::load(&app_dir);
        let resumed = request(&restarted, "game", root.join("game"), None, None, false);
        assert_eq!(resumed.release_tag().as_deref(), Some("v1"));
        assert_eq!(resumed.file.as_deref(), Some("renamed.zip"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_replacement_release_updates_an_explicit_tag_before_resuming() {
        let root = temp();
        let app_dir = root.join("data");
        let mut lib = Library::default();
        let mut pending = request(&lib, "game", root.join("game"), Some("v1".into()), Some("v1.zip".into()), false);
        pending.remember_release("v2", Some("v2.zip"), None);
        lib.pending_installs.insert("game".into(), pending);
        lib.save(&app_dir).unwrap();
        let restarted = Library::load(&app_dir);
        let resumed = request(&restarted, "game", root.join("game"), None, None, false);
        assert_eq!(resumed.release_tag().as_deref(), Some("v2"));
        assert_eq!(resumed.file.as_deref(), Some("v2.zip"));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn relinking_ignores_an_unavailable_unrelated_installation() {
        let root = temp();
        let target = root.join("found");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("game.exe"), b"program").unwrap();
        let unavailable = root.join("unavailable");
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("disconnected-drive"), &unavailable).unwrap();
        #[cfg(windows)]
        crate::win::junction(&root.join("disconnected-drive"), &unavailable).unwrap();
        assert!(normalized(&unavailable).is_err());
        let mut lib = Library::default();
        lib.installed.insert("game".into(), Installed { path: root.join("missing"), exe: Some("game.exe".into()), ..Default::default() });
        lib.installed.insert("other".into(), Installed { path: unavailable, ..Default::default() });
        assert_eq!(lib.relink("game", &target).unwrap().path, target);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn unavailable_paths_still_block_lexically_overlapping_relinks() {
        let root = temp();
        let target = root.join("found");
        fs::create_dir(&target).unwrap();
        let unavailable = target.join("other");
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("disconnected-drive"), &unavailable).unwrap();
        #[cfg(windows)]
        crate::win::junction(&root.join("disconnected-drive"), &unavailable).unwrap();
        let mut lib = Library::default();
        lib.installed.insert("game".into(), Installed { path: root.join("missing"), ..Default::default() });
        lib.installed.insert("other".into(), Installed { path: unavailable, ..Default::default() });
        assert!(lib.relink("game", &target).is_err());
        assert_eq!(lib.installed["game"].path, root.join("missing"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn paused_updates_block_move_without_changing_resume_state() {
        let root = temp();
        let target = root.join("game");
        fs::create_dir(&target).unwrap();
        let mut lib = Library::default();
        lib.installed.insert("game".into(), Installed { path: target.clone(), ..Default::default() });
        let pending = request(&lib, "game", target.clone(), Some("v2".into()), Some("game.zip".into()), false);
        reserve(&mut lib, "game", pending.clone(), &root.join("data")).unwrap();
        assert!(ensure_movable(&lib, "game").is_err());
        assert_eq!(lib.installed["game"].path, target);
        assert_eq!(resolve(&lib, "game", None, Some(&pending.target)).unwrap(), target);
        assert_eq!(lib.pending_installs["game"].tag.as_deref(), Some("v2"));
        assert_eq!(lib.pending_installs["game"].file.as_deref(), Some("game.zip"));
        clear_pending(&mut lib, "game", &root.join("data"));
        ensure_movable(&lib, "game").unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn changing_a_download_only_destination_removes_its_old_owned_work() {
        let root = temp();
        let app_dir = root.join("data");
        let mut lib = Library::default();
        let old = request(&lib, "game", root.join("first"), None, None, false);
        let work = claim(&old, false, &app_dir).unwrap();
        fs::write(work.join("download.part"), b"partial download").unwrap();
        reserve(&mut lib, "game", old.clone(), &app_dir).unwrap();
        let new = request(&lib, "game", root.join("second"), None, None, false);
        reserve(&mut lib, "game", new.clone(), &app_dir).unwrap();
        assert!(!work.exists());
        assert!(fs::read_dir(&old.target).unwrap().next().is_none());
        assert_eq!(lib.pending_installs["game"].target, new.target);
        claim(&new, false, &app_dir).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn changing_a_partially_extracted_destination_keeps_its_recovery_metadata() {
        let root = temp();
        let app_dir = root.join("data");
        let mut lib = Library::default();
        let old = request(&lib, "game", root.join("first"), Some("v1".into()), None, false);
        let work = claim(&old, false, &app_dir).unwrap();
        fs::write(old.target.join("extracted.dat"), b"partial game").unwrap();
        reserve(&mut lib, "game", old.clone(), &app_dir).unwrap();
        let new = request(&lib, "game", root.join("second"), None, None, false);
        assert!(reserve(&mut lib, "game", new, &app_dir).is_err());
        assert_eq!(lib.pending_installs["game"].token, old.token);
        assert_eq!(lib.pending_installs["game"].target, old.target);
        assert!(work.is_dir());
        lib.save(&app_dir).unwrap();
        let loaded = Library::load(&app_dir);
        validate(&loaded, &app_dir, "game", &old.target).unwrap();
        assert_eq!(claim(&loaded.pending_installs["game"], false, &app_dir).unwrap(), work);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn destination_replacement_does_not_drop_work_it_cannot_safely_clean() {
        let root = temp();
        let app_dir = root.join("data");
        let mut lib = Library::default();
        let old = request(&lib, "game", root.join("first"), None, None, false);
        let work = claim(&old, false, &app_dir).unwrap();
        fs::write(work.join("keep"), b"unverified work").unwrap();
        fs::remove_file(work.join(MARKER)).unwrap();
        reserve(&mut lib, "game", old.clone(), &app_dir).unwrap();
        let new = request(&lib, "game", root.join("second"), None, None, false);
        assert!(reserve(&mut lib, "game", new, &app_dir).is_err());
        assert_eq!(lib.pending_installs["game"].token, old.token);
        assert_eq!(fs::read(work.join("keep")).unwrap(), b"unverified work");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn removing_the_partial_folder_allows_choosing_a_new_destination() {
        let root = temp();
        let app_dir = root.join("data");
        let mut lib = Library::default();
        let target = root.join("first");
        fs::create_dir(&target).unwrap();
        let old = request(&lib, "game", target.clone(), None, None, false);
        let work = claim(&old, false, &app_dir).unwrap();
        fs::write(target.join("partial.dat"), b"partial game").unwrap();
        reserve(&mut lib, "game", old, &app_dir).unwrap();
        fs::remove_dir_all(target).unwrap();
        let new = request(&lib, "game", root.join("second"), None, None, false);
        reserve(&mut lib, "game", new.clone(), &app_dir).unwrap();
        assert!(!work.exists());
        claim(&new, false, &app_dir).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn update_removes_obsolete_files_when_the_saved_install_path_uses_a_folder_link() {
        let root = temp();
        let target = root.join("real-game");
        let alias = root.join("saved-game");
        fs::create_dir(&target).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &alias).unwrap();
        #[cfg(windows)]
        crate::win::junction(&target, &alias).unwrap();
        fs::write(target.join("obsolete.c"), b"old source").unwrap();
        fs::write(target.join("game.exe"), b"new release").unwrap();
        fs::write(target.join("save.sav"), b"save").unwrap();
        let previous = Installed {
            path: alias, files: vec!["obsolete.c".into(), "game.exe".into()],
            updated_at: crate::library::now() + 5, ..Default::default()
        };
        let removed = crate::install::remove_dropped_for(&previous, &target, &["game.exe".into()]);
        assert_eq!(removed, vec![PathBuf::from("obsolete.c")]);
        assert!(!target.join("obsolete.c").exists());
        assert_eq!(fs::read(target.join("game.exe")).unwrap(), b"new release");
        assert_eq!(fs::read(target.join("save.sav")).unwrap(), b"save");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn relinking_a_managed_game_preserves_unrelated_files_even_with_legacy_ownership_hints() {
        let root = temp();
        let target = root.join("library").join("game");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("game.exe"), b"program").unwrap();
        fs::write(target.join("unrelated.txt"), b"keep").unwrap();
        let mut lib = Library::default();
        lib.settings.library_dir = root.join("library");
        lib.installed.insert("game".into(), Installed {
            id: "game".into(), path: root.join("missing"), exe: Some("game.exe".into()),
            asset: "game.zip".into(), managed_install: true, ..Default::default()
        });
        let relinked = lib.relink("game", &target).unwrap();
        assert!(!relinked.managed_install);
        assert!(relinked.preserve_folder);
        assert!(!relinked.owns_folder(&lib.settings.all_libraries()));
        assert_eq!(fs::read(target.join("unrelated.txt")).unwrap(), b"keep");
        lib.save(&root.join("data")).unwrap();
        let loaded = Library::load(&root.join("data"));
        assert!(!loaded.installed["game"].owns_folder(&loaded.settings.all_libraries()));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn relinking_cannot_use_another_adopted_games_folder() {
        let root = temp();
        let target = root.join("adopted");
        fs::create_dir(&target).unwrap();
        fs::create_dir(target.join("nested")).unwrap();
        fs::write(target.join("game.exe"), b"program").unwrap();
        let mut lib = Library::default();
        lib.installed.insert("game".into(), Installed { id: "game".into(), path: root.join("missing"), managed_install: true, ..Default::default() });
        lib.installed.insert("other".into(), Installed { id: "other".into(), path: target.clone(), ..Default::default() });
        assert!(lib.relink("game", &target).is_err());
        assert!(lib.relink("game", &root).is_err());
        assert!(lib.relink("game", &target.join("nested")).is_err());
        assert_eq!(lib.installed["game"].path, root.join("missing"));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn relinking_cannot_use_an_alias_to_another_games_folder() {
        let root = temp();
        let target = root.join("adopted");
        fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&target, root.join("alias")).unwrap();
        let mut lib = Library::default();
        lib.installed.insert("game".into(), Installed { path: root.join("missing"), ..Default::default() });
        lib.installed.insert("other".into(), Installed { path: target, ..Default::default() });
        assert!(lib.relink("game", &root.join("alias")).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn forgetting_a_failed_update_cleans_work_and_allows_reinstalling_the_deleted_folder() {
        let root = temp();
        let app_dir = root.join("data");
        let target = root.join("game");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("game.exe"), b"program").unwrap();
        let mut lib = Library::default();
        lib.installed.insert("game".into(), Installed { id: "game".into(), path: target.clone(), managed_install: true, ..Default::default() });
        let pending = request(&lib, "game", target.clone(), Some("old".into()), None, false);
        assert_eq!(pending.anchor, target);
        let work = claim(&pending, true, &app_dir).unwrap();
        lib.pending_installs.insert("game".into(), pending);
        fs::remove_dir_all(&target).unwrap();
        forget(&mut lib, "game", &app_dir);
        assert!(!lib.pending_installs.contains_key("game"));
        assert!(!work.exists());
        validate(&lib, &app_dir, "game", &target).unwrap();
        let fresh = request(&lib, "game", target, None, None, false);
        assert_eq!(fresh.tag, None);
        claim(&fresh, false, &app_dir).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn forgetting_a_game_keeps_its_folder_and_removes_pending_work() {
        let root = temp();
        let target = root.join("game");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("game.exe"), b"keep").unwrap();
        let mut lib = Library::default();
        lib.installed.insert("game".into(), Installed { path: target.clone(), ..Default::default() });
        let pending = request(&lib, "game", target.clone(), None, None, false);
        let work = claim(&pending, true, &root.join("data")).unwrap();
        lib.pending_installs.insert("game".into(), pending);
        forget(&mut lib, "game", &root.join("data"));
        assert!(lib.pending_installs.is_empty());
        assert!(!work.exists());
        assert_eq!(fs::read(target.join("game.exe")).unwrap(), b"keep");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn library_forget_clears_a_reservation_even_without_an_installed_record() {
        let root = temp();
        let mut lib = Library::default();
        let pending = request(&lib, "game", root.join("game"), Some("old".into()), None, false);
        lib.pending_installs.insert("game".into(), pending);
        assert!(lib.forget("game").is_none());
        assert!(lib.pending_installs.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn explicit_newest_release_clears_old_tag_asset_and_backup_but_keeps_target_ownership() {
        let root = temp();
        let mut lib = Library::default();
        let target = root.join("game");
        let mut pending = request(&lib, "game", target.clone(), Some("old".into()), Some("old.zip".into()), false);
        pending.selected_tag = Some("old".into());
        pending.backup_release = Some(backup_release());
        let token = pending.token.clone();
        lib.pending_installs.insert("game".into(), pending);
        let fresh = request(&lib, "game", target, None, None, true);
        assert_eq!(fresh.tag, None);
        assert_eq!(fresh.selected_tag, None);
        assert_eq!(fresh.file, None);
        assert!(fresh.backup_release.is_none());
        assert_eq!(fresh.token, token);
        fs::remove_dir_all(root).unwrap();
    }

    fn backup_release() -> crate::catalog::Release {
        let answer = serde_json::json!({
            "version": "v1",
            "files": [
                {"name": "game-linux.zip", "url": "https://backup.example/game.zip", "os": "linux", "size": 100, "sha256": "a".repeat(64)},
                {"name": "game-linux-eu.zip", "url": "https://backup.example/game-eu.zip", "os": "linux", "size": 100, "sha256": "b".repeat(64)}
            ]
        });
        crate::catalog::backup_release_from(&answer, "linux").unwrap()
    }

    #[test]
    fn retargeting_after_restart_keeps_the_automatic_release_and_asset_with_new_ownership() {
        let root = temp();
        let app_dir = root.join("data");
        let first = root.join("first");
        let second = root.join("other/second");
        fs::create_dir(&first).unwrap();
        fs::create_dir(second.parent().unwrap()).unwrap();
        let mut lib = Library::default();
        let mut old = request(&lib, "game", first, None, None, false);
        old.remember_release("v1", Some("v1-linux.zip"), None);
        let work = claim(&old, false, &app_dir).unwrap();
        fs::write(work.join("partial"), b"download").unwrap();
        lib.pending_installs.insert("game".into(), old.clone());
        lib.save(&app_dir).unwrap();
        let mut loaded = Library::load(&app_dir);
        let changed = request(&loaded, "game", second.clone(), None, Some("v1-linux.zip".into()), false);
        assert_eq!(changed.release_tag().as_deref(), Some("v1"));
        assert_eq!(changed.tag, None);
        assert_eq!(changed.file.as_deref(), Some("v1-linux.zip"));
        assert_ne!(changed.token, old.token);
        assert_eq!(changed.anchor, second.parent().unwrap());
        reserve(&mut loaded, "game", changed.clone(), &app_dir).unwrap();
        assert!(!work.exists());
        claim(&changed, false, &app_dir).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn retargeting_keeps_the_checked_backup_source_without_contacting_upstream() {
        let root = temp();
        let mut lib = Library::default();
        let mut old = request(&lib, "game", root.join("first"), None, None, false);
        old.remember_release("v1", Some("game-linux.zip"), Some(&backup_release()));
        lib.pending_installs.insert("game".into(), old.clone());
        let changed = request(&lib, "game", root.join("second"), None, None, false);
        let (releases, from_backup) = changed.releases(
            false,
            || async { panic!("The checked backup must stay with its selected asset") },
            || async { panic!("A different destination must not require the removed project") },
        ).await.unwrap();
        assert!(from_backup);
        assert_eq!(changed.file.as_deref(), Some("game-linux.zip"));
        assert_eq!(releases[0].assets[0].digest, backup_release().assets[0].digest);
        assert_ne!(changed.token, old.token);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn retargeting_and_selecting_newest_clears_the_old_release_and_backup() {
        let root = temp();
        let mut lib = Library::default();
        let mut old = request(&lib, "game", root.join("first"), Some("v1".into()), None, false);
        old.remember_release("v1", Some("game-linux.zip"), Some(&backup_release()));
        lib.pending_installs.insert("game".into(), old.clone());
        let changed = request(&lib, "game", root.join("second"), None, None, true);
        assert_eq!(changed.tag, None);
        assert_eq!(changed.selected_tag, None);
        assert_eq!(changed.file, None);
        assert!(changed.backup_release.is_none());
        assert_ne!(changed.token, old.token);
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn automatic_install_uses_the_backup_without_querying_upstream() {
        let root = temp();
        let pending = request(&Library::default(), "game", root.join("game"), None, None, false);
        let (releases, from_backup) = pending.releases(
            true,
            || async { Ok(backup_release()) },
            || async { panic!("The removed upstream project must not be queried") },
        ).await.unwrap();
        assert!(from_backup);
        assert_eq!(releases[0].tag_name, "v1");
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn paused_backup_install_resumes_after_restart_without_querying_upstream() {
        let root = temp();
        let mut lib = Library::default();
        let mut pending = request(&lib, "game", root.join("game"), None, None, false);
        let release = backup_release();
        pending.remember_release("v1", Some("game-linux.zip"), Some(&release));
        lib.pending_installs.insert("game".into(), pending);
        lib.save(&root.join("data")).unwrap();
        let loaded = Library::load(&root.join("data"));
        let resumed = request(&loaded, "game", root.join("game"), None, None, false);
        let (releases, from_backup) = resumed.releases(
            true,
            || async { panic!("The saved checked release must not need refetching") },
            || async { panic!("The removed upstream project must not be queried") },
        ).await.unwrap();
        assert!(from_backup);
        assert_eq!(resumed.release_tag().as_deref(), Some("v1"));
        assert_eq!(resumed.tag, None);
        assert_eq!(resumed.file.as_deref(), Some("game-linux.zip"));
        assert_eq!(releases[0].assets[0].digest, release.assets[0].digest);
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn failed_backup_download_retries_the_recorded_source_even_if_the_listing_changes() {
        let root = temp();
        let mut lib = Library::default();
        let mut pending = request(&lib, "game", root.join("game"), None, None, false);
        pending.remember_release("v1", Some("game-linux.zip"), Some(&backup_release()));
        pending.remember_release("v1", Some("game-linux.zip"), None);
        lib.pending_installs.insert("game".into(), pending);
        let retried = request(&lib, "game", root.join("game"), None, None, false);
        let (releases, from_backup) = retried.releases(
            false,
            || async { panic!("Retry must retain its checked metadata") },
            || async { panic!("Retry must retain its backup source") },
        ).await.unwrap();
        assert!(from_backup);
        assert_eq!(releases[0].assets[0].browser_download_url, "https://backup.example/game.zip");
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn picking_a_build_preserves_the_backup_release_source() {
        let root = temp();
        let mut lib = Library::default();
        let mut pending = request(&lib, "game", root.join("game"), None, None, false);
        pending.remember_release("v1", None, Some(&backup_release()));
        lib.pending_installs.insert("game".into(), pending);
        let chosen = request(&lib, "game", root.join("game"), None, Some("game-linux-eu.zip".into()), false);
        let (releases, from_backup) = chosen.releases(
            true,
            || async { panic!("Build selection must use the saved release") },
            || async { panic!("Build selection must not query a removed project") },
        ).await.unwrap();
        assert!(from_backup);
        assert_eq!(chosen.file.as_deref(), Some(releases[0].assets[1].name.as_str()));
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn explicitly_selecting_a_release_still_queries_upstream() {
        let root = temp();
        let pending = request(&Library::default(), "game", root.join("game"), Some("v1".into()), None, false);
        let (_, from_backup) = pending.releases(
            true,
            || async { panic!("An explicit upstream version must not select a different backup") },
            || async { Ok(vec![backup_release()]) },
        ).await.unwrap();
        assert!(!from_backup);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn folder_only_access_does_not_require_listing_or_writing_the_parent() {
        use std::os::unix::fs::PermissionsExt;
        let root = temp();
        let parent = root.join("restricted");
        let target = parent.join("granted");
        fs::create_dir_all(&target).unwrap();
        let pending = request(&Library::default(), "game", target, None, None, false);
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o111)).unwrap();
        let claimed = claim(&pending, false, &root.join("data"));
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(claimed.unwrap().starts_with(root.join("data")));
        assert_eq!(fs::read_dir(parent).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn writable_game_folder_under_a_read_only_parent_can_install() {
        use std::os::unix::fs::PermissionsExt;
        let root = temp();
        let parent = root.join("read-only");
        let target = parent.join("game");
        fs::create_dir_all(&target).unwrap();
        let pending = request(&Library::default(), "game", target.clone(), None, None, false);
        let archive = root.join("game.zip");
        let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
        zip.start_file("game.exe", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(b"game").unwrap();
        zip.finish().unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o555)).unwrap();
        let claimed = claim(&pending, false, &root.join("data"));
        let installed = claimed.and_then(|work| crate::install::unpack(&archive, &target, &work.join("staging")));
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();
        installed.unwrap();
        assert_eq!(fs::read(target.join("game.exe")).unwrap(), b"game");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn exact_destination_does_not_append_a_game_folder() {
        let root = temp();
        let target = root.join("My récomp 日本");
        assert_eq!(resolve(&Library::default(), "game", None, Some(&target)).unwrap(), target);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_library_argument_still_appends_the_catalog_folder() {
        let root = temp();
        assert_eq!(resolve(&Library::default(), "game-id", Some(&root), None).unwrap(), root.join("game-id"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn library_and_exact_destination_cannot_be_combined() {
        let root = temp();
        assert!(resolve(&Library::default(), "game", Some(&root), Some(&root.join("game"))).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn existing_game_ignores_a_changed_default_library() {
        let root = temp();
        let target = root.join("old");
        let mut lib = Library::default();
        lib.installed.insert("game".into(), Installed { path: target.clone(), ..Default::default() });
        lib.settings.library_dir = root.join("new");
        assert_eq!(resolve(&lib, "game", Some(&lib.settings.library_dir), None).unwrap(), target);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn existing_game_requires_move_for_a_different_exact_target() {
        let root = temp();
        let mut lib = Library::default();
        lib.installed.insert("game".into(), Installed { path: root.join("old"), ..Default::default() });
        assert!(resolve(&lib, "game", None, Some(&root.join("new"))).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn updating_keeps_support_for_source_files_already_inside_the_game() {
        let root = temp();
        let target = root.join("game");
        fs::create_dir(&target).unwrap();
        let copy = target.join("original.iso");
        fs::write(&copy, b"game copy").unwrap();
        let mut lib = Library::default();
        lib.installed.insert("game".into(), Installed { path: target.clone(), game_copy: Some(copy), ..Default::default() });
        validate(&lib, &root.join("data"), "game", &target).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn occupied_folder_is_preserved_and_rejected() {
        let root = temp();
        let target = root.join("occupied");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("mine.txt"), b"keep").unwrap();
        let error = validate(&Library::default(), &root.join("data"), "game", &target).unwrap_err();
        assert!(error.to_string().contains("already has files"));
        assert_eq!(fs::read(target.join("mine.txt")).unwrap(), b"keep");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn empty_exact_folder_is_allowed_without_registering_its_parent() {
        let root = temp();
        let target = root.join("empty");
        fs::create_dir(&target).unwrap();
        let lib = Library::default();
        validate(&lib, &root.join("data"), "game", &target).unwrap();
        assert!(lib.settings.library_dirs.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn preview_does_not_create_directories() {
        let root = temp();
        let target = root.join("new").join("game");
        let lib = Library::default();
        validate(&lib, &root.join("data"), "game", &target).unwrap();
        assert_eq!(preview(target.clone()).target, target);
        assert!(!root.join("new").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn another_game_cannot_reserve_an_overlapping_pending_destination() {
        let root = temp();
        let mut lib = Library::default();
        let target = root.join("game");
        let pending = request(&lib, "first", target.clone(), None, None, false);
        lib.pending_installs.insert("first".into(), pending);
        assert!(validate(&lib, &root.join("data"), "second", &target.join("nested")).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn original_source_folder_cannot_contain_the_destination() {
        let root = temp();
        assert!(validate_source(&root.join("game"), Some(&root)).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn launcher_data_cannot_be_used_as_an_installation() {
        let root = temp();
        assert!(validate(&Library::default(), &root, "game", &root.join("game")).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn relative_paths_and_parent_traversal_are_rejected() {
        assert!(normalized(Path::new("relative/game")).is_err());
        let root = temp();
        assert!(normalized(&root.join("..").join("game")).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pending_request_survives_save_load_and_keeps_version_and_asset() {
        let root = temp();
        let mut lib = Library::default();
        let target = root.join("games").join("Chosen");
        let pending = request(&lib, "game", target.clone(), Some("v1".into()), Some("eu.zip".into()), false);
        lib.pending_installs.insert("game".into(), pending.clone());
        lib.save(&root.join("data")).unwrap();
        let loaded = Library::load(&root.join("data"));
        assert_eq!(resolve(&loaded, "game", None, None).unwrap(), target);
        let resumed = request(&loaded, "game", target, None, None, false);
        assert_eq!(resumed.tag.as_deref(), Some("v1"));
        assert_eq!(resumed.file.as_deref(), Some("eu.zip"));
        assert_eq!(resumed.token, pending.token);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_automatically_selected_release_survives_retry_without_becoming_a_pinned_version() {
        let root = temp();
        let target = root.join("game");
        let mut lib = Library::default();
        let mut pending = request(&lib, "game", target.clone(), None, None, false);
        pending.selected_tag = Some("v1".into());
        pending.file = Some("native.zip".into());
        lib.pending_installs.insert("game".into(), pending);
        let resumed = request(&lib, "game", target, None, None, false);
        assert_eq!(resumed.tag, None);
        assert_eq!(resumed.selected_tag.as_deref(), Some("v1"));
        assert_eq!(resumed.file.as_deref(), Some("native.zip"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn choosing_another_release_drops_the_old_asset_choice() {
        let root = temp();
        let target = root.join("game");
        let mut lib = Library::default();
        let mut pending = request(&lib, "game", target.clone(), Some("v1".into()), Some("old.zip".into()), false);
        pending.selected_tag = Some("v1".into());
        pending.backup_release = Some(backup_release());
        lib.pending_installs.insert("game".into(), pending);
        let changed = request(&lib, "game", target, Some("v2".into()), None, false);
        assert_eq!(changed.tag.as_deref(), Some("v2"));
        assert_eq!(changed.file, None);
        assert_eq!(changed.selected_tag, None);
        assert!(changed.backup_release.is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn owned_partial_install_can_be_retried_after_restart() {
        let root = temp();
        let mut lib = Library::default();
        let pending = request(&lib, "game", root.join("chosen"), None, None, false);
        lib.pending_installs.insert("game".into(), pending.clone());
        let work = claim(&pending, false, &root.join("data")).unwrap();
        fs::write(pending.target.join("partial.dat"), b"partial").unwrap();
        lib.save(&root.join("data")).unwrap();
        let loaded = Library::load(&root.join("data"));
        validate(&loaded, &root.join("data"), "game", &pending.target).unwrap();
        assert_eq!(claim(&loaded.pending_installs["game"], false, &root.join("data")).unwrap(), work);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn losing_the_ownership_marker_prevents_overwriting_a_partial_folder() {
        let root = temp();
        let mut lib = Library::default();
        let pending = request(&lib, "game", root.join("chosen"), None, None, false);
        lib.pending_installs.insert("game".into(), pending.clone());
        claim(&pending, false, &root.join("data")).unwrap();
        fs::write(pending.target.join("mine"), b"keep").unwrap();
        fs::remove_file(pending.target.join(MARKER)).unwrap();
        assert!(validate(&lib, &root.join("data"), "game", &pending.target).is_err());
        assert!(claim(&pending, false, &root.join("data")).is_err());
        assert_eq!(fs::read(pending.target.join("mine")).unwrap(), b"keep");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn work_directories_are_isolated_for_downloads_with_the_same_name() {
        let root = temp();
        let lib = Library::default();
        let first = request(&lib, "first", root.join("a"), None, None, false);
        let second = request(&lib, "second", root.join("b"), None, None, false);
        let first_work = claim(&first, false, &root.join("data")).unwrap();
        let second_work = claim(&second, false, &root.join("data")).unwrap();
        assert_ne!(first_work, second_work);
        assert!(!first_work.starts_with(&first.target));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn finishing_removes_only_owned_work_and_keeps_game_content() {
        let root = temp();
        let pending = request(&Library::default(), "game", root.join("game"), None, None, false);
        let work = claim(&pending, false, &root.join("data")).unwrap();
        fs::write(pending.target.join("game.exe"), b"game").unwrap();
        finish(&pending, false, &root.join("data"));
        assert!(!work.exists());
        assert!(!pending.target.join(MARKER).exists());
        assert_eq!(fs::read(pending.target.join("game.exe")).unwrap(), b"game");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cancelling_a_download_releases_work_but_preserves_the_selected_empty_folder() {
        let root = temp();
        let target = root.join("selected");
        fs::create_dir(&target).unwrap();
        let pending = request(&Library::default(), "game", target.clone(), None, None, false);
        let work = claim(&pending, false, &root.join("data")).unwrap();
        assert!(discard_empty(&pending, &root.join("data")));
        assert!(!work.exists());
        assert!(target.is_dir());
        assert!(fs::read_dir(target).unwrap().next().is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cancelling_after_unpacking_keeps_the_partial_game_recoverable() {
        let root = temp();
        let pending = request(&Library::default(), "game", root.join("game"), None, None, false);
        let work = claim(&pending, false, &root.join("data")).unwrap();
        fs::write(pending.target.join("partial"), b"partial").unwrap();
        assert!(!discard_empty(&pending, &root.join("data")));
        assert!(work.is_dir());
        assert!(owns(&pending.target, &pending.token));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn disconnected_anchor_is_not_recreated_on_retry() {
        let root = temp();
        let anchor = root.join("drive");
        fs::create_dir(&anchor).unwrap();
        let pending = request(&Library::default(), "game", anchor.join("game"), None, None, false);
        fs::remove_dir(&anchor).unwrap();
        assert!(claim(&pending, false, &root.join("data")).is_err());
        assert!(!anchor.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlink_aliases_cannot_overlap_another_installed_game() {
        let root = temp();
        let target = root.join("installed");
        fs::create_dir(&target).unwrap();
        let alias = root.join("alias");
        std::os::unix::fs::symlink(&target, &alias).unwrap();
        let mut lib = Library::default();
        lib.installed.insert("other".into(), Installed { path: target, ..Default::default() });
        let resolved = resolve(&lib, "game", None, Some(&alias.join("child"))).unwrap();
        assert!(validate(&lib, &root.join("data"), "game", &resolved).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}

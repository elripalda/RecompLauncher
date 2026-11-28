//! A game update in one step: all of the new release goes in, or the game stays as it was.
//!
//! The release is unpacked beside the game's folder first (`install::stage`), so a full disk or a
//! bad archive stops the update before the game is touched. Then what of the game's the new release
//! replaces, and the old release's files it no longer has, are set aside in the launcher's folder
//! inside the game's folder, and the new files are moved in. On one drive both are renames, so no
//! file is ever half-written. Anything going wrong moves the new files out and the old ones back.
//! A marker in that folder lists every move before the first one, so a launcher closed partway
//! finishes or undoes the update the next time it starts. Nothing else in the game's folder
//! (builds, saves, settings, the player's own files) moves at all.

use crate::library::Installed;
use crate::Error;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// The launcher's own folder in a game's folder while an update is under way. Looking through the
/// game's folder (for its program, its setup, the player's files) leaves it out.
pub const FOLDER: &str = ".recomplauncher-game-update";
const MARKER: &str = "update.json";
const OLD: &str = "old";

/// What an update moves, written before it moves anything.
#[derive(Debug, Serialize, Deserialize)]
struct Marker {
    /// The new release's files and download. Once the game's record has these, the update counts.
    files: Vec<PathBuf>,
    sha256: String,
    /// What of the game's was in the new release's way, or is an old release file it no longer
    /// has, in the order it's set aside.
    set_aside: Vec<PathBuf>,
    /// What comes in from the unpacked release, in order: a file, a link, a Mac app, or a whole
    /// folder the game didn't have.
    moved_in: Vec<PathBuf>,
}

/// An update whose new files are in place but that doesn't count yet: `finish` once the game's
/// record has its files. Dropped before that (an error, the launcher closing), it's undone.
pub struct Update {
    game: PathBuf,
    open: bool,
}

/// Moves one file or folder; tests swap in one that stops partway.
type Mover<'a> = &'a mut dyn FnMut(&Path, &Path) -> io::Result<()>;

/// Put the release unpacked at `staged` into `game`: what it replaces and `dropped` (the old
/// release's files it no longer has) set aside, then its own moved in. `files` and `sha256` are
/// what the game's record says once the update counts; `old_release` is what the record says now
/// (the old release's files). On a failure nothing is changed, and the error says what stopped it.
pub fn put_in_place(game: &Path, staged: &Path, files: &[PathBuf], sha256: &str, dropped: &[PathBuf], old_release: &[PathBuf]) -> Result<Update, Error> {
    put_in_place_with(game, staged, files, sha256, dropped, old_release, &mut move_path)
}

fn put_in_place_with(game: &Path, staged: &Path, files: &[PathBuf], sha256: &str, dropped: &[PathBuf], old_release: &[PathBuf], mv: Mover) -> Result<Update, Error> {
    let folder = game.join(FOLDER);
    if folder.join(MARKER).exists() {
        return Err(Error::Msg(format!("An earlier update of this game didn't finish, and its files couldn't be put back yet ({}). Restart the launcher, then try again.", folder.display())));
    }
    // Without a marker, nothing in the folder is the game's (see `close`).
    let _ = fs::remove_dir_all(&folder);
    let own: HashSet<PathBuf> = old_release.iter().chain(dropped).cloned().collect();
    let (set_aside, moved_in) = plan(game, staged, dropped, &own)?;
    let marker = Marker { files: files.to_vec(), sha256: sha256.into(), set_aside, moved_in };
    let Err(stopped) = apply(game, staged, &marker, mv) else {
        return Ok(Update { game: game.to_path_buf(), open: true });
    };
    let what = stopped.say();
    crate::trail::note(format!("Update in {}: {what}", game.display()));
    match undo(game) {
        Ok(()) => Err(Error::Msg(format!("{what} Nothing was changed: the game is as it was before the update."))),
        Err(stuck) => Err(Error::Msg(format!("{what} {stuck}"))),
    }
}

impl Update {
    /// The update counts (the game's record has its files): the old files go.
    pub fn finish(mut self) {
        self.open = false;
        if let Err(e) = finish(&self.game) {
            crate::trail::note(format!("Update in {}: {e}", self.game.display()));
        }
    }

    /// A step after the files went in failed with `e`: the update undone, and `e` saying so.
    pub fn undo_after(mut self, e: Error) -> Error {
        self.open = false;
        match undo(&self.game) {
            Ok(()) => Error::Msg(format!("{e} The update was undone: the game is as it was before it.")),
            Err(stuck) => Error::Msg(format!("{e} {stuck}")),
        }
    }

    /// The game's record couldn't be saved: both releases' files stay, and the next start keeps
    /// the one the saved record has.
    pub fn leave_for_next_start(mut self) {
        self.open = false;
    }
}

impl Drop for Update {
    fn drop(&mut self) {
        if !self.open {
            return;
        }
        let said = match undo(&self.game) {
            Ok(()) => "undone".to_string(),
            Err(stuck) => stuck,
        };
        crate::trail::note(format!("Update in {}: stopped before it counted, {said}", self.game.display()));
    }
}

/// An update in `game` the launcher was closed in the middle of: finished when `record` (the
/// game's entry in the library) already has its files, undone otherwise. What it did, for the
/// trail; None when there was nothing to do.
pub fn recover(game: &Path, record: Option<&Installed>) -> Option<String> {
    let folder = game.join(FOLDER);
    if !folder.join(MARKER).exists() {
        // Left from a start that stopped before writing its marker: it holds none of the game's.
        let _ = fs::remove_dir_all(&folder);
        return None;
    }
    let Some(m) = read_marker(&folder) else {
        return Some(format!("the notes of an unfinished update in {} couldn't be read, so its files were left as they are", folder.display()));
    };
    let counts = record.is_some_and(|r| r.sha256 == m.sha256 && r.files == m.files);
    if counts {
        return Some(match finish(game) {
            Ok(()) => "an update the launcher was closed in the middle of is finished".into(),
            Err(e) => e,
        });
    }
    Some(match undo(game) {
        Ok(()) => "an update the launcher was closed in the middle of is undone: the game is as it was before it".into(),
        Err(stuck) => stuck,
    })
}

/// Where an update stopped, and why.
struct Stopped {
    at: PathBuf,
    error: io::Error,
}

impl Stopped {
    /// For the player.
    fn say(&self) -> String {
        let name = self.at.display();
        if crate::install::in_use(&self.error) {
            return format!("The update couldn't move {name}: it's in use, or the launcher isn't allowed to change it. Close the game if it's running and check that your antivirus isn't holding its folder, then try again.");
        }
        if self.error.kind() == io::ErrorKind::StorageFull {
            return format!("The update couldn't move {name}: the drive is full.");
        }
        format!("The update couldn't move {name} ({}).", self.error)
    }
}

/// What of `game` to set aside and what of `staged` to move in, both relative. `own` is the old
/// release's files: only those are ever set aside in a way that loses them.
fn plan(game: &Path, staged: &Path, dropped: &[PathBuf], own: &HashSet<PathBuf>) -> Result<(Vec<PathBuf>, Vec<PathBuf>), Error> {
    let mut set_aside = vec![];
    let mut moved_in = vec![];
    walk(game, staged, Path::new(""), own, &mut set_aside, &mut moved_in)?;
    for rel in dropped {
        let inside = rel.components().all(|c| matches!(c, Component::Normal(_)));
        let covered = set_aside.iter().any(|a| rel.starts_with(a));
        if inside && !covered && present(&game.join(rel)) {
            set_aside.push(rel.clone());
        }
    }
    Ok((set_aside, moved_in))
}

/// A folder the game has too is gone into, so only the release's own files in it move; anything
/// else of the release moves in whole, and what of the game's is in its place is set aside.
fn walk(game: &Path, staged: &Path, rel: &Path, own: &HashSet<PathBuf>, set_aside: &mut Vec<PathBuf>, moved_in: &mut Vec<PathBuf>) -> Result<(), Error> {
    let mut entries = fs::read_dir(staged.join(rel))?.collect::<io::Result<Vec<_>>>()?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let path = rel.join(e.file_name());
        if path == Path::new(FOLDER) {
            continue;
        }
        // Not following links: a link where the release has a folder is replaced, never written through.
        let there = fs::symlink_metadata(game.join(&path)).ok();
        let folder_there = there.as_ref().is_some_and(|m| m.is_dir());
        let new_is_folder = e.file_type()?.is_dir();
        // A Mac app is one signed unit: replaced whole, never merged.
        let both_folders = new_is_folder && folder_there && !crate::macos::is_bundle(&e.path());
        if both_folders {
            walk(game, staged, &path, own, set_aside, moved_in)?;
            continue;
        }
        // A folder in the way of a release file is set aside whole and deleted when the update
        // finishes, so it may only hold the old release's files, never the player's.
        if folder_there && !new_is_folder {
            if let Some(theirs) = not_own_within(game, &path, own) {
                return Err(Error::Msg(format!("The update can't go in: the new release has a file named {}, where the game has a folder that holds your own files ({}). Move them out of that folder, then try again. Nothing was changed.", path.display(), theirs.display())));
            }
        }
        // The same the other way: a file in the way of a release folder goes only if it's the old
        // release's. A link (a release folder the player moved to another drive) isn't refused:
        // only the link is set aside, and what it points at is never touched.
        let link_there = there.as_ref().is_some_and(|m| m.file_type().is_symlink());
        let file_there = there.is_some() && !folder_there && !link_there;
        if file_there && new_is_folder && !own.contains(&path) {
            return Err(Error::Msg(format!("The update can't go in: the new release has a folder named {0}, where the game has a file of yours named {0}. Move or rename that file, then try again. Nothing was changed.", path.display())));
        }
        if there.is_some() {
            set_aside.push(path.clone());
        }
        moved_in.push(path);
    }
    Ok(())
}

/// The first thing in the game's folder `rel` (looked through, links not followed) that isn't one
/// of the old release's files. Folders themselves don't count: only what's in them.
fn not_own_within(game: &Path, rel: &Path, own: &HashSet<PathBuf>) -> Option<PathBuf> {
    let mut stack = vec![rel.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = fs::read_dir(game.join(&d)) else {
            // Can't be looked through, so it can't be shown to be the release's.
            return Some(d);
        };
        for e in entries.flatten() {
            let p = d.join(e.file_name());
            let is_folder = e.file_type().is_ok_and(|t| t.is_dir());
            if is_folder {
                stack.push(p);
                continue;
            }
            if !own.contains(&p) {
                return Some(p);
            }
        }
    }
    None
}

/// The marker written, then everything set aside, then everything moved in. Stops at the first
/// failure and leaves things as they are: `undo` puts them back.
fn apply(game: &Path, staged: &Path, m: &Marker, mv: Mover) -> Result<(), Stopped> {
    let folder = game.join(FOLDER);
    let starting = |error| Stopped { at: PathBuf::from(FOLDER), error };
    fs::create_dir_all(folder.join(OLD)).map_err(starting)?;
    write_marker(&folder, m).map_err(starting)?;
    for rel in &m.set_aside {
        move_one(&game.join(rel), &folder.join(OLD).join(rel), mv).map_err(|error| Stopped { at: rel.clone(), error })?;
    }
    for rel in &m.moved_in {
        move_one(&staged.join(rel), &game.join(rel), mv).map_err(|error| Stopped { at: rel.clone(), error })?;
    }
    Ok(())
}

/// The new files out and the old ones back, as the marker in `game` says. Safe to run again after
/// it stopped partway. Err says, for the player, what it couldn't put back.
fn undo(game: &Path) -> Result<(), String> {
    let folder = game.join(FOLDER);
    let Some(m) = read_marker(&folder) else { return Ok(()) };
    let old = folder.join(OLD);
    let mut stuck = vec![];
    for rel in m.moved_in.iter().rev() {
        // What was in this place before and isn't set aside yet is still the game's own.
        let still_the_old_one = m.set_aside.contains(rel) && !present(&old.join(rel));
        if still_the_old_one {
            continue;
        }
        let path = game.join(rel);
        if crate::install::patient(|| remove_any(&path)).is_err() {
            stuck.push(rel);
            continue;
        }
        crate::install::remove_empty_parents(game, &path);
    }
    for rel in m.set_aside.iter().rev() {
        let from = old.join(rel);
        if !present(&from) {
            continue;
        }
        if move_one(&from, &game.join(rel), &mut rename_patiently).is_err() {
            stuck.push(rel);
        }
    }
    if let Some(first) = stuck.first() {
        let more = if stuck.len() > 1 { format!(" and {} more", stuck.len() - 1) } else { String::new() };
        return Err(format!("Some of the old files couldn't be put back yet ({}{more}). The launcher puts them back the next time it starts; don't play the game until then.", first.display()));
    }
    close(&folder).map_err(|e| format!("The old files are back, but the launcher's folder {} couldn't be removed ({e}).", folder.display()))
}

/// The update counts: the old files go, and the folders the dropped ones leave empty.
fn finish(game: &Path) -> Result<(), String> {
    let folder = game.join(FOLDER);
    let set_aside = read_marker(&folder).map(|m| m.set_aside).unwrap_or_default();
    close(&folder).map_err(|e| format!("the update counts, but its old files in {} couldn't all be removed yet ({e}); the next start tries again", folder.display()))?;
    for rel in &set_aside {
        crate::install::remove_empty_parents(game, &game.join(rel));
    }
    Ok(())
}

/// The launcher's folder removed. The marker goes only once the old files have, so a folder without
/// one never holds any of the game's.
fn close(folder: &Path) -> io::Result<()> {
    let old = folder.join(OLD);
    crate::install::patient(|| remove_any(&old))?;
    crate::install::patient(|| remove_any(&folder.join(MARKER)))?;
    fs::remove_dir_all(folder)
}

fn move_one(from: &Path, to: &Path, mv: Mover) -> io::Result<()> {
    if let Some(parent) = to.parent() {
        crate::install::patient(|| fs::create_dir_all(parent))?;
    }
    mv(from, to)
}

/// A rename; copied and removed when it's to another drive (a game folder that's a link to one).
fn move_path(from: &Path, to: &Path) -> io::Result<()> {
    match rename_patiently(from, to) {
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {
            copy_all(from, to)?;
            remove_any(from)
        }
        r => r,
    }
}

fn rename_patiently(from: &Path, to: &Path) -> io::Result<()> {
    crate::install::patient(|| fs::rename(from, to))
}

fn copy_all(from: &Path, to: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(from)?;
    #[cfg(unix)]
    if meta.is_symlink() {
        return std::os::unix::fs::symlink(fs::read_link(from)?, to);
    }
    if !meta.is_dir() {
        return fs::copy(from, to).map(|_| ());
    }
    fs::create_dir(to)?;
    for e in fs::read_dir(from)? {
        let e = e?;
        copy_all(&e.path(), &to.join(e.file_name()))?;
    }
    Ok(())
}

/// A file, link or folder removed; one that isn't there is fine.
fn remove_any(p: &Path) -> io::Result<()> {
    let Ok(meta) = fs::symlink_metadata(p) else { return Ok(()) };
    if meta.is_dir() {
        return fs::remove_dir_all(p);
    }
    // On Windows a link to a folder (a symlink or a junction) isn't a folder to symlink_metadata,
    // but it is one to Windows: remove_file is refused (Access is denied), remove_dir takes the
    // link and leaves the folder it points at.
    #[cfg(windows)]
    if folder_link(&meta) {
        return fs::remove_dir(p);
    }
    fs::remove_file(p)
}

#[cfg(windows)]
fn folder_link(meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    meta.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0
}

fn present(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok()
}

/// Written whole and to disk before anything moves: a launcher closed just after still finds it.
fn write_marker(folder: &Path, m: &Marker) -> io::Result<()> {
    let tmp = folder.join(format!("{MARKER}.tmp"));
    let mut f = fs::File::create(&tmp)?;
    io::Write::write_all(&mut f, &serde_json::to_vec_pretty(m)?)?;
    f.sync_all()?;
    drop(f);
    fs::rename(tmp, folder.join(MARKER))
}

/// The marker, when it's whole and every path in it stays inside the game's folder.
fn read_marker(folder: &Path) -> Option<Marker> {
    let m: Marker = serde_json::from_slice(&fs::read(folder.join(MARKER)).ok()?).ok()?;
    let inside = |p: &PathBuf| !p.as_os_str().is_empty() && p.components().all(|c| matches!(c, Component::Normal(_)));
    let all_inside = m.set_aside.iter().all(inside) && m.moved_in.iter().all(inside);
    all_inside.then_some(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rl-swap-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(p: &Path, text: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    /// Every file and folder under `dir`, with what the files say.
    fn snapshot(dir: &Path) -> BTreeMap<PathBuf, String> {
        let mut out = BTreeMap::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in fs::read_dir(&d).unwrap().flatten() {
                let rel = e.path().strip_prefix(dir).unwrap().to_path_buf();
                if e.file_type().unwrap().is_dir() {
                    out.insert(rel, "<folder>".into());
                    stack.push(e.path());
                } else {
                    out.insert(rel, fs::read_to_string(e.path()).unwrap());
                }
            }
        }
        out
    }

    /// A game with an old release, the player's files, and a new release unpacked beside it.
    /// Returns the game's folder and the unpacked release.
    fn game_and_update(name: &str) -> (PathBuf, PathBuf) {
        let d = tmp(name);
        let game = d.join("game");
        put(&game.join("Game.exe"), "old exe");
        put(&game.join("data/a.bin"), "old a");
        put(&game.join("data/save.dat"), "player save");
        put(&game.join("gone.txt"), "old only");
        put(&game.join("cfg"), "old cfg file");
        put(&game.join("Mods/theirs.pak"), "player mod");
        put(&game.join("Game.app/Contents/Info.plist"), "old plist");
        put(&game.join("Game.app/Contents/old-only"), "old bundle file");
        let staged = d.join("staging");
        put(&staged.join("Game.exe"), "new exe");
        put(&staged.join("data/a.bin"), "new a");
        put(&staged.join("data/b.bin"), "new b");
        put(&staged.join("cfg/settings.ini"), "new cfg");
        put(&staged.join("newdir/x.txt"), "new x");
        put(&staged.join("Game.app/Contents/Info.plist"), "new plist");
        (game, staged)
    }

    fn release_files() -> Vec<PathBuf> {
        ["Game.exe", "data/a.bin", "data/b.bin", "cfg/settings.ini", "newdir/x.txt", "Game.app/Contents/Info.plist"].iter().map(PathBuf::from).collect()
    }

    fn dropped() -> Vec<PathBuf> {
        vec![PathBuf::from("gone.txt")]
    }

    /// The old release's files, as the game's record has them before the update.
    fn old_release() -> Vec<PathBuf> {
        ["Game.exe", "data/a.bin", "gone.txt", "cfg", "Game.app/Contents/Info.plist", "Game.app/Contents/old-only"].iter().map(PathBuf::from).collect()
    }

    fn own() -> HashSet<PathBuf> {
        old_release().into_iter().chain(dropped()).collect()
    }

    /// The game's record once the update counts.
    fn new_record() -> Installed {
        Installed { files: release_files(), sha256: "new".into(), ..Default::default() }
    }

    /// The number of moves the update makes.
    fn moves(game: &Path, staged: &Path) -> usize {
        let (set_aside, moved_in) = plan(game, staged, &dropped(), &own()).unwrap();
        set_aside.len() + moved_in.len()
    }

    /// Moves `n` things, then fails as a locked file or a full disk would.
    fn failing_after(n: usize) -> impl FnMut(&Path, &Path) -> io::Result<()> {
        let mut left = n;
        move |from, to| {
            if left == 0 {
                return Err(io::Error::other("stopped for the test"));
            }
            left -= 1;
            move_path(from, to)
        }
    }

    fn marker_for(game: &Path, staged: &Path) -> Marker {
        let (set_aside, moved_in) = plan(game, staged, &dropped(), &own()).unwrap();
        Marker { files: release_files(), sha256: "new".into(), set_aside, moved_in }
    }

    #[test]
    fn an_update_puts_the_new_release_in_and_leaves_the_players_files() {
        let (game, staged) = game_and_update("whole");
        put_in_place(&game, &staged, &release_files(), "new", &dropped(), &old_release()).unwrap().finish();
        let got = snapshot(&game);
        assert_eq!(got[Path::new("Game.exe")], "new exe");
        assert_eq!(got[Path::new("data/a.bin")], "new a");
        assert_eq!(got[Path::new("data/b.bin")], "new b");
        assert_eq!(got[Path::new("data/save.dat")], "player save");
        assert_eq!(got[Path::new("cfg/settings.ini")], "new cfg");
        assert_eq!(got[Path::new("newdir/x.txt")], "new x");
        assert_eq!(got[Path::new("Mods/theirs.pak")], "player mod");
        assert_eq!(got[Path::new("Game.app/Contents/Info.plist")], "new plist");
        assert!(!got.contains_key(Path::new("Game.app/Contents/old-only")), "a Mac app is replaced whole");
        assert!(!got.contains_key(Path::new("gone.txt")));
        assert!(!game.join(FOLDER).exists());
    }

    #[test]
    fn a_failure_at_any_point_leaves_the_game_as_it_was() {
        let (game, staged) = game_and_update("fail-count");
        let total = moves(&game, &staged);
        for n in 0..total {
            let (game, staged) = game_and_update("fail-at");
            let before = snapshot(&game);
            let got = put_in_place_with(&game, &staged, &release_files(), "new", &dropped(), &old_release(), &mut failing_after(n));
            let Err(Error::Msg(said)) = got else { panic!("the update failing after {n} moves went ahead") };
            assert!(said.contains("Nothing was changed"), "{said}");
            assert_eq!(snapshot(&game), before, "after failing at move {n}");
        }
    }

    #[test]
    fn a_launcher_closed_at_any_point_undoes_the_update_at_next_start() {
        let (game, staged) = game_and_update("closed-count");
        let total = moves(&game, &staged);
        for n in 0..=total {
            let (game, staged) = game_and_update("closed-at");
            let before = snapshot(&game);
            let marker = marker_for(&game, &staged);
            // The launcher closing: whatever was moved stays where it is.
            let _ = apply(&game, &staged, &marker, &mut failing_after(n));
            let old_record = Installed { sha256: "old".into(), ..Default::default() };
            assert!(recover(&game, Some(&old_record)).is_some());
            assert_eq!(snapshot(&game), before, "after closing at move {n}");
        }
    }

    #[test]
    fn a_launcher_closed_right_after_writing_its_marker_undoes_it() {
        let (game, staged) = game_and_update("closed-at-start");
        let before = snapshot(&game);
        let _ = apply(&game, &staged, &marker_for(&game, &staged), &mut failing_after(0));
        assert!(game.join(FOLDER).join(MARKER).exists());
        recover(&game, None);
        assert_eq!(snapshot(&game), before);
    }

    #[test]
    fn a_launcher_closed_after_the_record_was_saved_finishes_the_update() {
        let (game, staged) = game_and_update("closed-after-record");
        let update = put_in_place(&game, &staged, &release_files(), "new", &dropped(), &old_release()).unwrap();
        update.leave_for_next_start();
        recover(&game, Some(&new_record()));
        let got = snapshot(&game);
        assert_eq!(got[Path::new("Game.exe")], "new exe");
        assert!(!got.contains_key(Path::new("gone.txt")));
        assert!(!game.join(FOLDER).exists());
    }

    #[test]
    fn a_launcher_closed_while_undoing_finishes_undoing() {
        let (game, staged) = game_and_update("closed-while-undoing");
        let before = snapshot(&game);
        let marker = marker_for(&game, &staged);
        assert!(apply(&game, &staged, &marker, &mut move_path).is_ok());
        // The first thing undoing does, then the launcher closing.
        remove_any(&game.join(marker.moved_in.last().unwrap())).unwrap();
        recover(&game, None);
        assert_eq!(snapshot(&game), before);
    }

    #[test]
    fn an_update_dropped_before_it_counts_is_undone() {
        let (game, staged) = game_and_update("dropped-guard");
        let before = snapshot(&game);
        drop(put_in_place(&game, &staged, &release_files(), "new", &dropped(), &old_release()).unwrap());
        assert_eq!(snapshot(&game), before);
    }

    #[test]
    fn a_step_after_the_files_went_in_failing_undoes_the_update() {
        let (game, staged) = game_and_update("undo-after");
        let before = snapshot(&game);
        let update = put_in_place(&game, &staged, &release_files(), "new", &dropped(), &old_release()).unwrap();
        let said = update.undo_after(Error::Msg("Couldn't get its project files.".into())).to_string();
        assert!(said.contains("undone"), "{said}");
        assert_eq!(snapshot(&game), before);
    }

    #[test]
    fn a_folder_the_dropped_files_leave_empty_goes() {
        let d = tmp("dropped-folder");
        let (game, staged) = (d.join("game"), d.join("staging"));
        put(&game.join("old/deep/a.bin"), "old");
        put(&staged.join("new.bin"), "new");
        let update = put_in_place(&game, &staged, &[PathBuf::from("new.bin")], "new", &[PathBuf::from("old/deep/a.bin")], &[]).unwrap();
        update.finish();
        assert!(!game.join("old").exists());
        assert!(game.join("new.bin").exists());
    }

    #[test]
    fn a_dropped_file_outside_the_game_folder_stays() {
        let d = tmp("dropped-outside");
        let (game, staged) = (d.join("game"), d.join("staging"));
        put(&d.join("other.txt"), "not the game's");
        put(&staged.join("new.bin"), "new");
        put_in_place(&game, &staged, &[PathBuf::from("new.bin")], "new", &[PathBuf::from("../other.txt")], &[]).unwrap().finish();
        assert!(d.join("other.txt").exists());
    }

    #[test]
    fn an_update_waits_for_one_that_couldnt_be_put_back() {
        let (game, staged) = game_and_update("waits");
        let _ = apply(&game, &staged, &marker_for(&game, &staged), &mut failing_after(2));
        let got = put_in_place(&game, &staged, &release_files(), "new", &dropped(), &old_release());
        assert!(got.is_err());
    }

    #[test]
    fn a_marker_pointing_outside_the_game_is_never_followed() {
        let d = tmp("tampered");
        let game = d.join("game");
        put(&d.join("outside.txt"), "not the game's");
        let marker = Marker { files: vec![], sha256: String::new(), set_aside: vec![], moved_in: vec![PathBuf::from("../outside.txt")] };
        fs::create_dir_all(game.join(FOLDER)).unwrap();
        write_marker(&game.join(FOLDER), &marker).unwrap();
        recover(&game, None);
        assert!(d.join("outside.txt").exists());
    }

    /// A game whose folder `cfg` the new release has as a file. `player_file` adds one of the
    /// player's to that folder.
    fn folder_becomes_file(name: &str, player_file: bool) -> (PathBuf, PathBuf) {
        let d = tmp(name);
        let (game, staged) = (d.join("game"), d.join("staging"));
        put(&game.join("cfg/old.ini"), "old release");
        if player_file {
            put(&game.join("cfg/mine.ini"), "player's");
        }
        put(&staged.join("cfg"), "new cfg file");
        (game, staged)
    }

    #[test]
    fn a_release_file_replaces_a_folder_of_only_old_release_files() {
        let (game, staged) = folder_becomes_file("folder-to-file", false);
        let old = [PathBuf::from("cfg/old.ini")];
        put_in_place(&game, &staged, &[PathBuf::from("cfg")], "new", &[], &old).unwrap().finish();
        assert_eq!(fs::read_to_string(game.join("cfg")).unwrap(), "new cfg file");
        assert!(!game.join(FOLDER).exists());
    }

    #[test]
    fn a_release_file_never_replaces_a_folder_holding_player_files() {
        let (game, staged) = folder_becomes_file("folder-to-file-player", true);
        let before = snapshot(&game);
        let old = [PathBuf::from("cfg/old.ini")];
        let got = put_in_place(&game, &staged, &[PathBuf::from("cfg")], "new", &[], &old);
        let Err(Error::Msg(said)) = got else { panic!("the update set aside a folder with the player's files") };
        assert!(said.contains("cfg") && said.contains("your own files"), "{said}");
        assert!(said.contains("Nothing was changed"), "{said}");
        assert_eq!(snapshot(&game), before);
    }

    /// A game with a file `cfg` where the new release has a folder.
    fn file_becomes_folder(name: &str) -> (PathBuf, PathBuf) {
        let d = tmp(name);
        let (game, staged) = (d.join("game"), d.join("staging"));
        put(&game.join("cfg"), "a cfg file");
        put(&staged.join("cfg/settings.ini"), "new settings");
        (game, staged)
    }

    #[test]
    fn a_release_folder_replaces_an_old_release_file() {
        let (game, staged) = file_becomes_folder("file-to-folder");
        let files = [Path::new("cfg").join("settings.ini")];
        put_in_place(&game, &staged, &files, "new", &[], &[PathBuf::from("cfg")]).unwrap().finish();
        assert_eq!(fs::read_to_string(game.join("cfg/settings.ini")).unwrap(), "new settings");
    }

    #[test]
    fn a_release_folder_never_replaces_a_player_file() {
        let (game, staged) = file_becomes_folder("file-to-folder-player");
        let before = snapshot(&game);
        let files = [Path::new("cfg").join("settings.ini")];
        let got = put_in_place(&game, &staged, &files, "new", &[], &[]);
        let Err(Error::Msg(said)) = got else { panic!("the update set aside a file of the player's") };
        assert!(said.contains("cfg") && said.contains("Nothing was changed"), "{said}");
        assert_eq!(snapshot(&game), before);
    }

    #[cfg(unix)]
    #[test]
    fn a_release_folder_replaces_a_link_and_leaves_what_it_points_at() {
        let d = tmp("linked-folder");
        let (game, staged, elsewhere) = (d.join("game"), d.join("staging"), d.join("other-drive/data"));
        put(&elsewhere.join("big.pak"), "the player's moved data");
        fs::create_dir_all(&game).unwrap();
        std::os::unix::fs::symlink(&elsewhere, game.join("data")).unwrap();
        put(&staged.join("data/x.bin"), "new x");
        let files = [PathBuf::from("data/x.bin")];
        put_in_place(&game, &staged, &files, "new", &[], &[]).unwrap().finish();
        assert_eq!(fs::read_to_string(game.join("data/x.bin")).unwrap(), "new x");
        assert!(!fs::symlink_metadata(game.join("data")).unwrap().file_type().is_symlink());
        assert_eq!(snapshot(&elsewhere), BTreeMap::from([(PathBuf::from("big.pak"), "the player's moved data".to_string())]));
    }

    /// A Nix AppImage's link out of the game's folder, in the old release and the new: the link is
    /// replaced, and what the old one pointed at is never touched.
    #[cfg(unix)]
    #[test]
    fn a_link_out_of_the_folder_is_replaced_as_a_link() {
        let d = tmp("outside-link");
        let (game, staged, store) = (d.join("game"), d.join("staging"), d.join("nix/store"));
        put(&store.join("old-game/bin/game"), "not the game's to remove");
        fs::create_dir_all(&game).unwrap();
        fs::create_dir_all(&staged).unwrap();
        std::os::unix::fs::symlink(store.join("old-game/bin/game"), game.join("entrypoint")).unwrap();
        std::os::unix::fs::symlink("/nix/store/new-game/bin/game", staged.join("entrypoint")).unwrap();
        let files = [PathBuf::from("entrypoint")];
        put_in_place(&game, &staged, &files, "new", &[], &files).unwrap().finish();
        assert_eq!(fs::read_link(game.join("entrypoint")).unwrap(), Path::new("/nix/store/new-game/bin/game"));
        assert_eq!(fs::read_to_string(store.join("old-game/bin/game")).unwrap(), "not the game's to remove");
    }

    #[test]
    fn an_update_still_counts_after_the_library_is_saved_and_loaded_again() {
        let d = tmp("record-round-trip");
        let (game, staged, lib) = (d.join("game"), d.join("staging"), d.join("lib"));
        put(&game.join("data/deep/a.bin"), "old a");
        put(&staged.join("data/deep/a.bin"), "new a");
        put(&staged.join("data/deep/b.bin"), "new b");
        // Joined, so on Windows these are written the Windows way, as an unpacked release's are.
        let files = vec![Path::new("data").join("deep").join("a.bin"), Path::new("data").join("deep").join("b.bin")];
        let old = [Path::new("data").join("deep").join("a.bin")];
        put_in_place(&game, &staged, &files, "new", &[], &old).unwrap().leave_for_next_start();
        let mut library = crate::library::Library::default();
        let record = Installed { id: "g".into(), path: game.clone(), files, sha256: "new".into(), ..Default::default() };
        library.installed.insert("g".into(), record);
        library.save(&lib).unwrap();
        let loaded = crate::library::Library::load(&lib);
        let said = recover(&game, loaded.installed.get("g")).unwrap();
        assert!(said.contains("finished"), "{said}");
        assert_eq!(fs::read_to_string(game.join("data/deep/b.bin")).unwrap(), "new b");
        assert!(!game.join(FOLDER).exists());
    }

    /// Holds `file` the way a running game or an antivirus does, until dropped.
    #[cfg(windows)]
    fn lock(file: &Path) -> Option<fs::File> {
        use std::os::windows::fs::OpenOptionsExt;
        Some(fs::OpenOptions::new().read(true).share_mode(0).open(file).unwrap())
    }

    /// Unix doesn't lock open files: a folder that can't be changed stops a move the same way.
    #[cfg(unix)]
    struct Unlock(PathBuf);

    #[cfg(unix)]
    impl Drop for Unlock {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
        }
    }

    /// None when the folder can still be changed (run as root).
    #[cfg(unix)]
    fn lock(file: &Path) -> Option<Unlock> {
        use std::os::unix::fs::PermissionsExt;
        let dir = file.parent().unwrap().to_path_buf();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
        let unlock = Unlock(dir.clone());
        fs::write(dir.join("probe"), b"").is_err().then_some(unlock)
    }

    #[test]
    fn a_locked_file_stops_the_update_and_changes_nothing() {
        let (game, staged) = game_and_update("locked");
        let before = snapshot(&game);
        let Some(held) = lock(&game.join("data/a.bin")) else { return };
        let got = put_in_place(&game, &staged, &release_files(), "new", &dropped(), &old_release());
        let Err(Error::Msg(said)) = got else { panic!("the update went ahead past a locked file") };
        assert!(said.contains("in use"), "{said}");
        assert!(said.contains("Nothing was changed"), "{said}");
        drop(held);
        assert_eq!(snapshot(&game), before);
    }

    /// A junction (`mklink /J`, no administrator needed) to a folder of the player's: removing it
    /// takes the link and leaves the folder it points at, and its files.
    #[cfg(windows)]
    #[test]
    fn a_link_to_a_folder_goes_and_the_folder_stays() {
        let d = tmp("junction");
        put(&d.join("theirs/save.dat"), "player save");
        let link = d.join("link");
        let made = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(&link).arg(d.join("theirs")).output().unwrap();
        assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));
        remove_any(&link).unwrap();
        assert!(fs::symlink_metadata(&link).is_err(), "the link is gone");
        assert_eq!(fs::read_to_string(d.join("theirs/save.dat")).unwrap(), "player save");
    }
}

//! On Linux every game plays from a plain folder, never from an AppImage. A release's AppImage is
//! unpacked when it installs (install::unpack_appimages). One the player brings in, or one an older
//! launcher left in a game's folder, is unpacked into the Recomp Library here, the first time:
//! read once, never run, and its path not kept. Plain folders are what stays: the player's
//! AppImage can be deleted after, and a folder needs no FUSE, no mount and no sandbox exception.
//! Also the check that a library folder's drive can hold a Linux game at all.

use crate::library::{folder_name, now, Installed};
use crate::{install, swap, trail, App, Error};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// What to tell the player when a library folder's drive can't hold a Linux game.
pub const BAD_DRIVE: &str = "Your library folder is on a drive Linux games can't play from: it's formatted for Windows (exFAT, NTFS or FAT), or set up so programs can't run from it. Games there lose their run permission and their links. Pick a library folder on a Linux drive, like your home folder, in Settings → Games and folders.";

/// Kinds of drive that can't keep a program's run permission or a link (fuseblk is how NTFS-3G
/// and exFAT-FUSE show up).
const WINDOWS_DRIVES: [&str; 7] = ["exfat", "vfat", "msdos", "fat", "ntfs", "ntfs3", "fuseblk"];

/// Why the drive `dir` is on can't hold a Linux game, for the trail; None when it can, or when it
/// can't be told.
pub fn drive_problem(dir: &Path) -> Option<String> {
    let info = std::fs::read_to_string("/proc/self/mountinfo").ok()?;
    let at = nearest_existing(dir)?;
    let (kind, noexec) = mount_of(&info, &at)?;
    if WINDOWS_DRIVES.contains(&kind.as_str()) {
        return Some(format!("it's on a {kind} drive"));
    }
    noexec.then(|| "its drive is mounted noexec".to_string())
}

/// Err with `BAD_DRIVE` when a game can't play from `dir`'s drive.
pub fn check_drive(dir: &Path) -> Result<(), Error> {
    let Some(why) = drive_problem(dir) else { return Ok(()) };
    trail::note(format!("Library folder {}: {why}", dir.display()));
    Err(Error::Msg(BAD_DRIVE.into()))
}

/// `dir`, or the closest folder above it that's there yet, with links followed.
fn nearest_existing(dir: &Path) -> Option<PathBuf> {
    let mut at = dir;
    while !at.exists() {
        at = at.parent()?;
    }
    std::fs::canonicalize(at).ok()
}

/// The drive `path` is on, from /proc/self/mountinfo: its kind, and whether it's mounted noexec.
/// The mount with the longest mount point holding `path` wins, and of two at the same point the
/// later one (it's mounted over the first).
fn mount_of(info: &str, path: &Path) -> Option<(String, bool)> {
    let mut best: Option<(usize, String, bool)> = None;
    for line in info.lines() {
        let Some((mount, fs)) = line.split_once(" - ") else { continue };
        let fields: Vec<&str> = mount.split(' ').collect();
        if fields.len() < 6 {
            continue;
        }
        let point = unescape(fields[4]);
        if !path.starts_with(&point) {
            continue;
        }
        let depth = point.components().count();
        let kind = fs.split(' ').next().unwrap_or("").to_string();
        let noexec = fields[5].split(',').any(|o| o == "noexec");
        if best.as_ref().map_or(true, |b| depth >= b.0) {
            best = Some((depth, kind, noexec));
        }
    }
    best.map(|(_, kind, noexec)| (kind, noexec))
}

/// A mount point as mountinfo writes it: a space is `\040`, a tab `\011`, a backslash `\134`.
fn unescape(s: &str) -> PathBuf {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let octal = bytes[i] == b'\\' && i + 3 < bytes.len() && bytes[i + 1..i + 4].iter().all(|b| (b'0'..=b'7').contains(b));
        if !octal {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        let digit = |k: usize| u32::from(bytes[i + k] - b'0');
        let value = digit(1) * 64 + digit(2) * 8 + digit(3);
        out.push(u8::try_from(value).unwrap_or(b'?'));
        i += 4;
    }
    PathBuf::from(String::from_utf8_lossy(&out).to_string())
}

/// What `bring_in` did, for the window's message: the unpacked game took the AppImage's place
/// (`replaced`: it was in the Recomp Library) or the AppImage stayed where it is. `own`: a game
/// the launcher doesn't update, so a newer AppImage brought in the same way updates it.
#[derive(Debug, Clone, Serialize)]
pub struct Brought {
    pub id: String,
    pub name: String,
    pub replaced: bool,
    pub own: bool,
}

/// The AppImage a game in the Library starts, when it starts one: the program the player picked
/// (Game Executable, or added as it), the one the launcher picked, or with none picked the only AppImage at
/// the top of its folder (one an older launcher installed, which its recomplauncher.json names).
pub fn image_of(g: &Installed) -> Option<PathBuf> {
    let full = |e: &PathBuf| if e.is_absolute() { e.clone() } else { g.path.join(e) };
    if let Some(found) = g.exe.as_ref().map(full).filter(|p| is_image(p)) {
        return Some(found);
    }
    if g.exe.is_some() || g.path.as_os_str().is_empty() {
        return None;
    }
    crate::appimage::installed_image(&g.path).filter(|p| is_image(p))
}

fn is_image(p: &Path) -> bool {
    p.is_file() && crate::appimage::is_real_appimage(p)
}

/// A game's name from its AppImage's file name, for one that doesn't say its own: the words before
/// its version, system or processor (Dusklight-v2.0.3-linux-x86_64.AppImage is Dusklight).
pub fn image_title(image: &Path) -> String {
    let stem = image.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let tail = |w: &str| {
        let low = w.to_ascii_lowercase();
        let number = low.strip_prefix('v').unwrap_or(&low);
        let version = !number.is_empty() && number.chars().all(|c| c.is_ascii_digit());
        version || matches!(low.as_str(), "linux" | "x86" | "x64" | "x86_64" | "amd64" | "arm64" | "aarch64" | "appimage")
    };
    let words: Vec<&str> = stem.split(['-', '_', ' ', '.']).filter(|w| !w.is_empty()).take_while(|w| !tail(w)).collect();
    if words.is_empty() {
        return stem;
    }
    words.join(" ")
}

/// The name a player's AppImage goes by for updates: its file name without the version, so
/// Game-1.2-x86_64.AppImage and Game-1.3-x86_64.AppImage are the same game.
pub fn image_key(image: &Path) -> String {
    image.file_name().map(|n| crate::assets::unversioned(&n.to_string_lossy())).unwrap_or_default()
}

/// The game `id` unpacked from `image` into a plain folder in the Recomp Library, and its record
/// moved to it. A game folder of its own in a library (an older launcher's install, or one the
/// player put there) gets the AppImage's files in its place, all at once (see swap). A game the
/// launcher unpacked before is updated in its folder. Anything else gets a new folder, and the
/// AppImage stays where it is, untouched.
pub fn bring_in(st: &App, id: &str, image: &Path) -> Result<Brought, Error> {
    let (g, libs, default_lib) = {
        let l = st.lib.lock().unwrap();
        let g = l.installed.get(id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
        (g, l.settings.all_libraries(), l.settings.library_dir.clone())
    };
    let in_library = libs.iter().any(|l| image.starts_with(l));
    let folder_of_its_own = libs.iter().any(|l| g.path.starts_with(l) && g.path != *l);
    let in_place = folder_of_its_own || g.unpacked;
    // The library it goes into, unpacked there first (the same drive, so it moves in whole).
    let home = if in_place { g.path.parent().map(Path::to_path_buf).unwrap_or_else(|| default_lib.clone()) } else { default_lib.clone() };
    check_drive(&home)?;
    std::fs::create_dir_all(&home)?;
    let staging = home.join(".staging").join(folder_name(id));
    trail::note(format!("Bring in {id}: unpacking {} into {}", image.display(), home.display()));
    let staged = install::stage_appimage(image, &staging).map_err(|e| unpack_failed(id, e))?;
    let start = PathBuf::from("AppRun");
    let mut next = g.clone();
    next.exe = Some(start.clone());
    next.files = staged.files.clone();
    // The folder is the launcher's own (uninstall may delete it) unless it's one the player put in
    // the library themselves: that keeps whatever else of theirs is in there, and uninstall leaves it.
    let players_folder = in_place && g.local && !g.unpacked;
    next.unpacked = !players_folder;
    next.from_appimage = image_key(image);
    next.updated_at = now();
    // A game the player added before this version is named after its folder (Downloads, say):
    // the name its desktop file gives, now that it's unpacked.
    if next.local && next.name == g.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default() {
        if let Some(name) = desktop_name(&staging) {
            next.name = name;
        }
    }
    let target = if in_place { g.path.clone() } else { new_folder(&default_lib, &next) };
    next.path = target.clone();
    // A setup that was the AppImage is nothing to run anymore.
    if next.setup.as_deref().is_some_and(|s| g.path.join(s) == image) {
        next.setup = None;
    }
    if in_place {
        put_in_place(st, &g, next, image, &staged, &staging)?;
    } else {
        let moved = std::fs::rename(&staged.root, &target);
        let _ = std::fs::remove_dir_all(&staging);
        moved?;
        save(st, id, next)?;
    }
    // The player's AppImage in the top of a library (not in a game's folder): the game takes its place.
    let replaced = in_library;
    if in_library && image.exists() {
        let _ = std::fs::remove_file(image);
    }
    let name = st.lib.lock().unwrap().installed.get(id).map(|x| x.name.clone()).unwrap_or_default();
    trail::note(format!("Bring in {id}: it plays from {} now", target.display()));
    Ok(Brought { id: id.to_string(), name, replaced, own: g.local })
}

/// The unpacked files into the game's own folder all at once, the AppImage (when it's in there)
/// and the files an earlier unpacked one had that this one hasn't taken away with the rest.
fn put_in_place(st: &App, g: &Installed, next: Installed, image: &Path, staged: &install::Staged, staging: &Path) -> Result<(), Error> {
    let target = next.path.clone();
    let mut dropped = vec![];
    if let Ok(rel) = image.strip_prefix(&target) {
        dropped.push(rel.to_path_buf());
    }
    // An earlier unpack's files (in the player's own folder too) are the ones a newer one may take away.
    if g.unpacked || !g.from_appimage.is_empty() {
        dropped.extend(install::dropped(&target, &g.files, &staged.files, g.updated_at, None));
    }
    if let Some(what) = swap::recover(&target, Some(g)) {
        trail::note(format!("Bring in {}: {what}", g.id));
    }
    // What the game's record says it has (an older launcher's release, or what an earlier unpack put
    // there) may be set aside where the unpacked files need its place; nothing else of the folder.
    let placed = swap::put_in_place(&target, &staged.root, &staged.files, &g.sha256, &dropped, &g.files);
    let _ = std::fs::remove_dir_all(staging);
    let update = placed?;
    // The new files count once the saved library has them (see swap::recover).
    match save(st, &g.id, next) {
        Ok(()) => update.finish(),
        Err(e) => {
            update.leave_for_next_start();
            return Err(e);
        }
    }
    Ok(())
}

fn save(st: &App, id: &str, next: Installed) -> Result<(), Error> {
    st.lib.lock().unwrap().installed.insert(id.to_string(), next);
    st.try_save().map_err(Error::Io)
}

/// A new folder in the Recomp Library for a game brought in: named after the game, and never one
/// that's there already.
fn new_folder(lib: &Path, g: &Installed) -> PathBuf {
    let base = folder_name(if g.local { &g.name } else { &g.id });
    let mut target = lib.join(&base);
    let mut n = 2;
    while target.exists() {
        target = lib.join(format!("{base}-{n}"));
        n += 1;
    }
    target
}

/// The name in an unpacked AppImage's desktop file (Name=), for a game added without one.
fn desktop_name(dir: &Path) -> Option<String> {
    let entries = std::fs::read_dir(dir).ok()?;
    let file = entries.flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "desktop"))?;
    let text = std::fs::read_to_string(file).ok()?;
    text.lines().find_map(|l| l.strip_prefix("Name=")).map(str::trim).filter(|n| !n.is_empty()).map(str::to_string)
}

/// What the player sees when the AppImage won't unpack. Why goes to the trail.
pub const UNPACK_FAILED: &str = "The launcher couldn't unpack this game's AppImage, and on Linux it plays games only from a plain folder. Install it again from its page, or bring in another build of it.";

fn unpack_failed(id: &str, e: Error) -> Error {
    let why = e.to_string();
    if !why.starts_with(install::APPIMAGE_UNPACK) {
        return e;
    }
    trail::note(format!("Bring in {id}: {}", why.trim_start_matches(install::APPIMAGE_UNPACK)));
    Error::Msg(UNPACK_FAILED.into())
}

/// The window says where the game went, once (desktop and Big Picture alike).
pub fn tell(b: &Brought) {
    use tauri::Emitter;
    if let Some(app) = crate::net::app() {
        let _ = app.emit("unpacked", b);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INFO: &str = "22 1 0:21 / / rw,relatime shared:1 - btrfs /dev/nvme0n1p3 rw,ssd\n\
        30 22 0:21 /home /var/home rw,relatime shared:2 - btrfs /dev/nvme0n1p3 rw,ssd\n\
        41 22 8:17 / /run/media/me/SD\\040Card rw,nosuid,nodev,relatime shared:9 - exfat /dev/sdb1 rw,uid=1000\n\
        42 22 8:33 / /mnt/stuff rw,nosuid,nodev,noexec,relatime shared:10 - ext4 /dev/sdc1 rw\n\
        43 22 8:49 / /mnt/win rw,relatime shared:11 - fuseblk /dev/sdd1 rw,user_id=0\n";

    #[test]
    fn the_deepest_mount_holding_a_folder_is_its_drive() {
        assert_eq!(mount_of(INFO, Path::new("/var/home/me/Games")), Some(("btrfs".into(), false)));
        assert_eq!(mount_of(INFO, Path::new("/run/media/me/SD Card/Games")), Some(("exfat".into(), false)));
        assert_eq!(mount_of(INFO, Path::new("/mnt/stuff/Games")), Some(("ext4".into(), true)));
        assert_eq!(mount_of(INFO, Path::new("/mnt/win")), Some(("fuseblk".into(), false)));
    }

    #[test]
    fn a_mount_point_with_a_space_in_it_reads_back() {
        assert_eq!(unescape("/run/media/me/SD\\040Card"), PathBuf::from("/run/media/me/SD Card"));
        assert_eq!(unescape("/a\\134b"), PathBuf::from("/a\\b"));
        assert_eq!(unescape("/plain\\0"), PathBuf::from("/plain\\0"));
    }

    #[test]
    fn a_folder_named_like_another_mount_isnt_on_it() {
        // /mnt/stuffing isn't inside /mnt/stuff.
        assert_eq!(mount_of(INFO, Path::new("/mnt/stuffing")), Some(("btrfs".into(), false)));
    }

    #[test]
    fn the_image_a_game_starts_is_found_by_its_program() {
        let d = std::env::temp_dir().join(format!("rl-plain-image-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("Game-1.0-x86_64.AppImage"), b"x").unwrap();
        let mut g = Installed { path: d.clone(), exe: Some("Game-1.0-x86_64.AppImage".into()), ..Default::default() };
        assert_eq!(image_of(&g), Some(d.join("Game-1.0-x86_64.AppImage")));
        // With no program picked, the only AppImage at the top.
        g.exe = None;
        assert_eq!(image_of(&g), Some(d.join("Game-1.0-x86_64.AppImage")));
        // A plain folder's game starts its own program, whatever else is in there.
        g.exe = Some("AppRun".into());
        assert_eq!(image_of(&g), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A folder for one test, with a Recomp Library in it, and the launcher's data beside it.
    fn place(name: &str) -> (PathBuf, crate::Library) {
        let d = std::env::temp_dir().join(format!("rl-plain-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("lib")).unwrap();
        let mut lib = crate::Library::default();
        lib.settings.library_dir = d.join("lib");
        (d, lib)
    }

    fn put_image(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, crate::appimage::tests::game_image()).unwrap();
    }

    #[test]
    fn the_players_appimage_is_unpacked_into_the_library_and_stays() {
        let (d, mut lib) = place("outside");
        let image = d.join("Downloads/Game-1.0-x86_64.AppImage");
        put_image(&image);
        let g = Installed { id: "own-game".into(), name: "Game".into(), path: d.join("Downloads"), exe: Some("Game-1.0-x86_64.AppImage".into()), local: true, ..Default::default() };
        lib.installed.insert(g.id.clone(), g);
        let st = crate::tests::app_in(&d, lib);

        let b = bring_in(&st, "own-game", &image).unwrap();

        assert!(!b.replaced && b.own);
        let g = st.lib.lock().unwrap().installed["own-game"].clone();
        assert_eq!(g.path, d.join("lib/Game"));
        assert_eq!(g.exe, Some(PathBuf::from("AppRun")));
        assert!(g.unpacked);
        assert_eq!(g.from_appimage, image_key(&image));
        assert!(g.path.join("usr/bin/game").is_file());
        assert!(image.is_file(), "the player's AppImage stays where it is");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_appimage_an_older_launcher_installed_is_replaced_by_its_files() {
        let (d, mut lib) = place("in-library");
        let image = d.join("lib/game/Game-x86_64.AppImage");
        put_image(&image);
        std::fs::write(d.join("lib/game/settings.ini"), b"player's").unwrap();
        let g = Installed { id: "game".into(), name: "Game".into(), path: d.join("lib/game"), exe: Some("Game-x86_64.AppImage".into()), files: vec!["Game-x86_64.AppImage".into()], sha256: "abc".into(), ..Default::default() };
        lib.installed.insert(g.id.clone(), g);
        let st = crate::tests::app_in(&d, lib);

        let b = bring_in(&st, "game", &image).unwrap();

        assert!(b.replaced && !b.own);
        let g = st.lib.lock().unwrap().installed["game"].clone();
        assert_eq!(g.path, d.join("lib/game"));
        assert_eq!(g.sha256, "abc", "it's still the release it was");
        assert!(g.files.contains(&PathBuf::from("AppRun")));
        assert!(!image.exists());
        assert_eq!(std::fs::read(d.join("lib/game/settings.ini")).unwrap(), b"player's");
        assert!(!d.join("lib/game").join(swap::FOLDER).exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    const STAND_IN: &[u8] = b"#!/bin/sh\nexec \"$(dirname \"$0\")/AppRun\" \"$@\"\n";

    #[test]
    fn a_script_named_like_an_appimage_isnt_the_games_image() {
        let (d, _) = place("stand-in-image");
        let game = d.join("lib/game");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("AppRun"), b"#!/bin/sh\n").unwrap();
        std::fs::write(game.join("OldName.AppImage"), STAND_IN).unwrap();
        assert!(!is_image(&game.join("OldName.AppImage")));
        let mut g = Installed { path: game.clone(), exe: None, ..Default::default() };
        assert_eq!(image_of(&g), None);
        g.exe = Some("OldName.AppImage".into());
        assert_eq!(image_of(&g), None);
        // A real one beside it is still the game's image.
        put_image(&game.join("Game.AppImage"));
        assert!(is_image(&game.join("Game.AppImage")));
        g.exe = None;
        assert_eq!(image_of(&g), Some(game.join("Game.AppImage")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn play_with_no_program_leaves_a_script_named_like_an_appimage_alone() {
        let (d, mut lib) = place("stand-in-play");
        let game = d.join("lib/game");
        std::fs::create_dir_all(game.join("usr/bin")).unwrap();
        std::fs::write(game.join("AppRun"), b"#!/bin/sh\n").unwrap();
        std::fs::write(game.join("OldName.AppImage"), STAND_IN).unwrap();
        let files = vec![PathBuf::from("AppRun"), PathBuf::from("OldName.AppImage")];
        let g = Installed { id: "game".into(), name: "Game".into(), path: game.clone(), exe: None, files: files.clone(), sha256: "abc".into(), ..Default::default() };
        lib.installed.insert(g.id.clone(), g);
        let st = crate::tests::app_in(&d, lib);

        let got = crate::in_plain_folder(&st, "game").unwrap();

        assert_eq!(got.path, game);
        assert_eq!(got.exe, None);
        assert_eq!(got.files, files);
        assert!(!got.unpacked);
        assert_eq!(std::fs::read(game.join("OldName.AppImage")).unwrap(), STAND_IN);
        assert!(!d.join("lib/.staging").exists(), "nothing was unpacked");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn play_with_no_program_still_unpacks_a_real_appimage_an_older_launcher_left() {
        let (d, mut lib) = place("old-image-play");
        let image = d.join("lib/game/Game-x86_64.AppImage");
        put_image(&image);
        let g = Installed { id: "game".into(), name: "Game".into(), path: d.join("lib/game"), exe: None, files: vec!["Game-x86_64.AppImage".into()], sha256: "abc".into(), ..Default::default() };
        lib.installed.insert(g.id.clone(), g);
        let st = crate::tests::app_in(&d, lib);

        let got = crate::in_plain_folder(&st, "game").unwrap();

        assert_eq!(got.path, d.join("lib/game"));
        assert_eq!(got.exe, Some(PathBuf::from("AppRun")));
        assert!(d.join("lib/game/usr/bin/game").is_file());
        assert!(!image.exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_game_unpacked_before_plays_as_it_was() {
        let (d, mut lib) = place("unpacked-play");
        let game = d.join("lib/game");
        std::fs::create_dir_all(game.join("usr/bin")).unwrap();
        std::fs::write(game.join("AppRun"), b"#!/bin/sh\n").unwrap();
        std::fs::write(game.join("usr/bin/game"), b"\x7fELF game").unwrap();
        let g = Installed { id: "game".into(), name: "Game".into(), path: game.clone(), exe: None, files: vec!["AppRun".into(), "usr/bin/game".into()], unpacked: true, ..Default::default() };
        lib.installed.insert(g.id.clone(), g);
        let st = crate::tests::app_in(&d, lib);
        for exe in [Some(PathBuf::from("AppRun")), None] {
            st.lib.lock().unwrap().installed.get_mut("game").unwrap().exe = exe.clone();
            let got = crate::in_plain_folder(&st, "game").unwrap();
            assert_eq!(got.path, game);
            assert_eq!(got.exe, exe);
            assert!(got.unpacked);
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_folder_the_player_put_in_the_library_stays_theirs_when_unpacked_in_place() {
        let (d, mut lib) = place("players-folder");
        let image = d.join("lib/Mine/Game-1.0-x86_64.AppImage");
        put_image(&image);
        std::fs::write(d.join("lib/Mine/notes.txt"), b"player's").unwrap();
        let g = Installed { id: "own-mine".into(), name: "Mine".into(), path: d.join("lib/Mine"), exe: Some("Game-1.0-x86_64.AppImage".into()), local: true, ..Default::default() };
        lib.installed.insert(g.id.clone(), g);
        let st = crate::tests::app_in(&d, lib);

        bring_in(&st, "own-mine", &image).unwrap();

        let g = st.lib.lock().unwrap().installed["own-mine"].clone();
        assert_eq!(g.path, d.join("lib/Mine"));
        assert!(!g.unpacked, "the folder is the player's: uninstall must not delete it");
        assert!(g.files.contains(&PathBuf::from("AppRun")), "what the unpack put there is still known");
        assert!(!image.exists());
        assert_eq!(std::fs::read(d.join("lib/Mine/notes.txt")).unwrap(), b"player's");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_newer_appimage_updates_the_players_own_folder_and_only_its_files_go() {
        let (d, mut lib) = place("players-update");
        std::fs::create_dir_all(d.join("lib/Mine/usr")).unwrap();
        std::fs::write(d.join("lib/Mine/usr/old.bin"), b"old release").unwrap();
        std::fs::write(d.join("lib/Mine/notes.txt"), b"player's").unwrap();
        let earlier = image_key(Path::new("Game-1.0-x86_64.AppImage"));
        let g = Installed { id: "own-mine".into(), name: "Mine".into(), path: d.join("lib/Mine"), exe: Some("AppRun".into()), files: vec!["usr/old.bin".into()], local: true, from_appimage: earlier, updated_at: now() + 60, ..Default::default() };
        lib.installed.insert(g.id.clone(), g);
        let image = d.join("Downloads/Game-1.1-x86_64.AppImage");
        put_image(&image);
        let st = crate::tests::app_in(&d, lib);

        bring_in(&st, "own-mine", &image).unwrap();

        let g = st.lib.lock().unwrap().installed["own-mine"].clone();
        assert_eq!(g.path, d.join("lib/Mine"));
        assert!(!g.unpacked);
        assert!(!d.join("lib/Mine/usr/old.bin").exists(), "the earlier unpack's file it no longer has goes");
        assert_eq!(std::fs::read(d.join("lib/Mine/notes.txt")).unwrap(), b"player's");
        assert!(d.join("lib/Mine/usr/bin/game").is_file());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_newer_appimage_updates_the_game_unpacked_before() {
        let (d, mut lib) = place("update");
        std::fs::create_dir_all(d.join("lib/Game/usr")).unwrap();
        std::fs::write(d.join("lib/Game/usr/old.bin"), b"old release").unwrap();
        let g = Installed { id: "own-game".into(), name: "Game".into(), path: d.join("lib/Game"), exe: Some("AppRun".into()), files: vec!["usr/old.bin".into()], local: true, unpacked: true, updated_at: now() + 60, ..Default::default() };
        lib.installed.insert(g.id.clone(), g);
        let image = d.join("Downloads/Game-1.1-x86_64.AppImage");
        put_image(&image);
        let st = crate::tests::app_in(&d, lib);

        bring_in(&st, "own-game", &image).unwrap();

        let g = st.lib.lock().unwrap().installed["own-game"].clone();
        assert_eq!(g.path, d.join("lib/Game"));
        assert!(!d.join("lib/Game/usr/old.bin").exists(), "the old version's file it no longer has goes");
        assert!(d.join("lib/Game/usr/bin/game").is_file());
        assert!(image.is_file());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_appimage_that_wont_unpack_leaves_the_game_as_it_was() {
        let (d, mut lib) = place("broken");
        let image = d.join("Downloads/Game.AppImage");
        std::fs::create_dir_all(image.parent().unwrap()).unwrap();
        std::fs::write(&image, b"#!/bin/sh\n").unwrap();
        let g = Installed { id: "own-game".into(), name: "Game".into(), path: d.join("Downloads"), exe: Some("Game.AppImage".into()), local: true, ..Default::default() };
        lib.installed.insert(g.id.clone(), g);
        let st = crate::tests::app_in(&d, lib);

        let Err(Error::Msg(m)) = bring_in(&st, "own-game", &image) else { panic!("it must not come in") };

        assert_eq!(m, UNPACK_FAILED);
        assert_eq!(st.lib.lock().unwrap().installed["own-game"].path, d.join("Downloads"));
        assert_eq!(std::fs::read_dir(d.join("lib")).unwrap().filter(|e| e.as_ref().unwrap().file_name() != ".staging").count(), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_appimage_without_a_name_of_its_own_is_named_by_its_file() {
        assert_eq!(image_title(Path::new("/x/Dusklight-v2.0.3-linux-x86_64.AppImage")), "Dusklight");
        assert_eq!(image_title(Path::new("/x/SoH-Dewey-Bravo.appimage")), "SoH Dewey Bravo");
        assert_eq!(image_title(Path::new("/x/2Ship-x86_64.AppImage")), "2Ship");
        assert_eq!(image_title(Path::new("/x/1.0.AppImage")), "1.0");
    }

    #[test]
    fn a_newer_appimage_goes_by_the_same_name() {
        assert_eq!(image_key(Path::new("/x/Game-1.2-x86_64.AppImage")), image_key(Path::new("/y/Game-1.3-x86_64.AppImage")));
        assert_ne!(image_key(Path::new("/x/Game-x86_64.AppImage")), image_key(Path::new("/x/Other-x86_64.AppImage")));
    }
}

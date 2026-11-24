//! Looking after installed games: moving them between drives, backing up their saves before an
//! update and putting them back, and checking the PC has what recomps need.

use crate::Error;
use serde::Serialize;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

/// Free space on the drive a path is on (the nearest folder that exists, if the path doesn't yet).
pub fn free_space(p: &Path) -> Option<u64> {
    let mut q = p.to_path_buf();
    while !q.exists() {
        q = q.parent()?.to_path_buf();
    }
    fs2::available_space(&q).ok()
}

/// Size of everything under a folder.
pub fn dir_size(p: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![p.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).into_iter().flatten().flatten() {
            match e.metadata() {
                Ok(m) if m.is_dir() => stack.push(e.path()),
                Ok(m) => total += m.len(),
                Err(_) => {}
            }
        }
    }
    total
}

/// How much room a game takes, as the player sees it: its folder without the old files an update
/// set aside and couldn't remove yet (they go at the next start). Moving or removing the game
/// still takes the whole folder (`dir_size`).
pub fn game_size(p: &Path) -> u64 {
    dir_size(p).saturating_sub(dir_size(&p.join(crate::swap::FOLDER)))
}

/// Move a game's folder to `to`. A rename when it's the same drive; otherwise a copy, reporting
/// bytes as it goes, then the old folder is deleted once everything has been copied.
pub fn move_dir(from: &Path, to: &Path, report: &dyn Fn(u64, u64)) -> Result<(), Error> {
    if to.exists() && fs::read_dir(to).map(|mut r| r.next().is_some()).unwrap_or(false) {
        return Err(Error::Msg(format!("{} already has files in it.", to.display())));
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    let total = dir_size(from);
    if let Some(free) = free_space(to) {
        if free < total + 64 * 1024 * 1024 {
            return Err(Error::Msg("There isn't enough space on that drive.".into()));
        }
    }
    let mut done = 0u64;
    let r = copy_tree(from, to, &mut done, total, report);
    if let Err(e) = r {
        // Half a copy is no use to anyone: take it away and leave the game where it was.
        let _ = fs::remove_dir_all(to);
        return Err(e.into());
    }
    // The copy is whole, so the game lives there now. Anything left behind (a file Windows keeps
    // open) is only clutter, not a reason to say the move failed.
    let _ = fs::remove_dir_all(from);
    Ok(())
}

fn copy_tree(from: &Path, to: &Path, done: &mut u64, total: u64, report: &dyn Fn(u64, u64)) -> io::Result<()> {
    let real_from = fs::canonicalize(from).unwrap_or_else(|_| from.to_path_buf());
    let roots = Roots { from: from.to_path_buf(), real_from, to: to.to_path_buf() };
    copy_folder(from, to, &roots, done, total, report)
}

/// The game's folder and where it goes, so a link that points into the folder by full path can be
/// pointed at the new place. Its real path counts too: on Bazzite /home is a link to /var/home.
#[cfg_attr(not(unix), allow(dead_code))]
struct Roots {
    from: PathBuf,
    real_from: PathBuf,
    to: PathBuf,
}

fn copy_folder(from: &Path, to: &Path, roots: &Roots, done: &mut u64, total: u64, report: &dyn Fn(u64, u64)) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for e in fs::read_dir(from)? {
        let e = e?;
        let (src, dst) = (e.path(), to.join(e.file_name()));
        let kind = e.file_type()?;
        #[cfg(unix)]
        if kind.is_symlink() && copy_link(&src, &dst, roots)? {
            continue;
        }
        if kind.is_dir() {
            copy_folder(&src, &dst, roots, done, total, report)?;
            continue;
        }
        copy_file(&src, &dst, done, total, report)?;
    }
    Ok(())
}

/// A link copied as a link, the way a move on the same drive keeps it: a Mac app's frameworks are
/// links its signature checks, and a Linux game's libraries often are too. Copied as files, a link
/// to a folder stopped the move. false when the drive can't hold links (FAT32, exFAT): a link to a
/// file is then copied as that file, as before.
#[cfg(unix)]
fn copy_link(src: &Path, dst: &Path, roots: &Roots) -> io::Result<bool> {
    let target = moved_target(src, fs::read_link(src)?, roots);
    match std::os::unix::fs::symlink(&target, dst) {
        Ok(()) => Ok(true),
        Err(_) if src.is_file() => Ok(false),
        Err(e) => Err(e),
    }
}

/// Where the link at `src` points once the game moves, so it reaches what it reached before. One
/// into the game follows it to the new place (relative ones already do); one out of it keeps
/// pointing at the same place by full path, since `..` from the new place leads somewhere else.
#[cfg(unix)]
fn moved_target(src: &Path, target: PathBuf, roots: &Roots) -> PathBuf {
    if target.is_absolute() {
        let inside = target.strip_prefix(&roots.from).or_else(|_| target.strip_prefix(&roots.real_from));
        return match inside {
            Ok(rest) => roots.to.join(rest),
            Err(_) => target,
        };
    }
    // Followed from the game's real folder, where no part of the path is a link that would change
    // what `..` means.
    let Some(folder) = src.parent().and_then(|p| p.strip_prefix(&roots.from).ok()) else { return target };
    let reached = without_dots(&roots.real_from.join(folder).join(&target));
    if reached.starts_with(&roots.real_from) {
        return target;
    }
    reached
}

/// The path with its `.` and `..` worked out, without asking the drive (the link may point nowhere).
#[cfg(unix)]
fn without_dots(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

fn copy_file(src: &Path, dst: &Path, done: &mut u64, total: u64, report: &dyn Fn(u64, u64)) -> io::Result<()> {
    let mut i = fs::File::open(src)?;
    let mut o = fs::File::create(dst)?;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = io::Read::read(&mut i, &mut buf)?;
        if n == 0 {
            break;
        }
        o.write_all(&buf[..n])?;
        *done += n as u64;
        report(*done, total);
    }
    // A drive that can't keep them (FAT32, exFAT) is no reason to stop the move.
    if let Ok(m) = fs::metadata(src) {
        let _ = fs::set_permissions(dst, m.permissions());
    }
    Ok(())
}

/// The player asked to skip the backup that's being made (the update goes on without it).
pub static SKIP_BACKUP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Debug, Clone, Serialize)]
pub struct Backup {
    pub file: PathBuf,
    pub name: String,
    pub when: u64,
    pub size: u64,
    pub files: usize,
}

/// Zip `files` (paths inside the game's folder `dir`: its saves there, see `saves::files_inside`)
/// into `<backups>/<when>.zip`, keeping the newest `keep`. None when there are none.
pub fn backup(dir: &Path, files: &[PathBuf], backups: &Path, keep: usize) -> Result<Option<Backup>, Error> {
    SKIP_BACKUP.store(false, std::sync::atomic::Ordering::Relaxed);
    if files.is_empty() {
        return Ok(None);
    }
    fs::create_dir_all(backups)?;
    let when = crate::library::now();
    let file = backups.join(format!("{when}.zip"));
    {
        let mut z = zip::ZipWriter::new(fs::File::create(&file)?);
        let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for rel in files {
            if SKIP_BACKUP.load(std::sync::atomic::Ordering::Relaxed) {
                drop(z);
                let _ = fs::remove_file(&file);
                return Ok(None);
            }
            let name = rel.to_string_lossy().replace('\\', "/");
            z.start_file(name, o).map_err(|e| Error::Msg(e.to_string()))?;
            let mut f = fs::File::open(dir.join(rel))?;
            io::copy(&mut f, &mut z)?;
        }
        z.finish().map_err(|e| Error::Msg(e.to_string()))?;
    }
    let mut all = list_backups(backups);
    while all.len() > keep {
        if let Some(old) = all.pop() {
            let _ = fs::remove_file(old.file);
        }
    }
    let size = fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
    Ok(Some(Backup { name: format!("{when}.zip"), file, when, size, files: files.len() }))
}

/// A game's backups, newest first.
pub fn list_backups(backups: &Path) -> Vec<Backup> {
    let mut out: Vec<Backup> = fs::read_dir(backups)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let name = p.file_name()?.to_str()?.to_string();
            let when: u64 = name.strip_suffix(".zip")?.parse().ok()?;
            let size = e.metadata().ok()?.len();
            let files = zip::ZipArchive::new(fs::File::open(&p).ok()?).map(|z| z.len()).unwrap_or(0);
            Some(Backup { file: p, name, when, size, files })
        })
        .collect();
    out.sort_by(|a, b| b.when.cmp(&a.when));
    out
}

/// Put a backup's files back into the game's folder, over what's there.
pub fn restore(zip_file: &Path, dir: &Path) -> Result<usize, Error> {
    let mut z = zip::ZipArchive::new(fs::File::open(zip_file)?).map_err(|e| Error::Msg(e.to_string()))?;
    let mut n = 0;
    for i in 0..z.len() {
        let mut e = z.by_index(i).map_err(|e| Error::Msg(e.to_string()))?;
        let Some(rel) = e.enclosed_name() else { continue };
        if e.is_dir() {
            continue;
        }
        let dest = dir.join(rel);
        if let Some(d) = dest.parent() {
            fs::create_dir_all(d)?;
        }
        io::copy(&mut e, &mut fs::File::create(&dest)?)?;
        n += 1;
    }
    Ok(n)
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct PcCheck {
    /// Microsoft Visual C++ runtime (2015 to 2022, x64): almost every Windows recomp needs it.
    pub vcredist: Option<bool>,
    /// Its version, when it's older than games built with today's Visual Studio need (14.44).
    pub vcredist_old: Option<String>,
    pub vulkan: Option<bool>,
    /// vkBasalt, for looks on Linux.
    pub vkbasalt: Option<bool>,
    /// On Linux, what plays Windows recomps (a Proton's name, umu-run or Wine); empty when nothing does.
    pub proton: Option<String>,
    pub avx: bool,
    pub avx2: bool,
    pub cores: usize,
    /// Visual Studio's C++ build tools (with CMake), for recomps that build themselves on the PC.
    pub build_tools: Option<bool>,
    /// Clang (Visual Studio's "C++ Clang tools", or LLVM), for the ones whose setup builds with it.
    pub clang: Option<bool>,
    /// Python 3's version ("3.12.4"); empty when there's none. For setups that run Python.
    pub python: Option<String>,
    /// Windows' ("10.0.22631") or macOS's ("14.5") version, for what a game says it needs
    /// (sysneeds.rs). None on Linux.
    pub os_version: Option<String>,
    /// glibc's version ("2.39") on Linux, for a game's Linux build.
    pub glibc: Option<String>,
}

pub fn pc_check() -> PcCheck {
    let mut c = PcCheck { cores: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1), ..Default::default() };
    c.os_version = crate::sysneeds::os_version();
    c.glibc = crate::sysneeds::glibc_version();
    #[cfg(target_arch = "x86_64")]
    {
        c.avx = std::arch::is_x86_feature_detected!("avx");
        c.avx2 = std::arch::is_x86_feature_detected!("avx2");
    }
    #[cfg(windows)]
    {
        c.vcredist = Some(crate::win::has_vcredist());
        c.vcredist_old = crate::win::vcredist_version().filter(|&(a, b, _)| a == 14 && b < 44).map(|(a, b, d)| format!("{a}.{b}.{d}"));
        let sys = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        c.vulkan = Some(Path::new(&sys).join("System32").join("vulkan-1.dll").is_file());
        c.build_tools = Some(crate::win::has_build_tools());
        c.clang = Some(crate::win::has_clang());
        c.python = Some(crate::win::python_version().unwrap_or_default());
    }
    #[cfg(target_os = "linux")]
    {
        c.vulkan = Some(["/usr/lib/x86_64-linux-gnu/libvulkan.so.1", "/usr/lib/libvulkan.so.1", "/usr/lib64/libvulkan.so.1"].iter().any(|p| Path::new(p).exists()));
        c.vkbasalt = Some(crate::looks::vkbasalt_installed());
        c.proton = Some(crate::proton::runner().map(|r| r.name()).unwrap_or_default());
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rl-manage-{n}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_games_size_leaves_out_what_an_update_set_aside() {
        let game = tmp("size").join("game");
        fs::create_dir_all(game.join(crate::swap::FOLDER).join("old")).unwrap();
        fs::write(game.join("Game.exe"), [0u8; 100]).unwrap();
        fs::write(game.join(crate::swap::FOLDER).join("old/Game.exe"), [0u8; 40]).unwrap();
        assert_eq!(game_size(&game), 100);
        assert_eq!(dir_size(&game), 140, "moving the game still counts all of it");
    }

    #[test]
    fn a_backup_holds_the_files_it_is_given_and_puts_them_back() {
        let d = tmp("bk");
        let game = d.join("game");
        fs::create_dir_all(game.join("saves")).unwrap();
        fs::write(game.join("Game.exe"), b"exe").unwrap();
        fs::write(game.join("saves/slot1.sav"), b"save").unwrap();
        fs::write(game.join("settings.json"), b"{}").unwrap();
        let b = backup(&game, &[PathBuf::from("saves/slot1.sav")], &d.join("backups"), 2).unwrap().unwrap();
        assert_eq!(b.files, 1);
        fs::write(game.join("saves/slot1.sav"), b"broken").unwrap();
        assert_eq!(restore(&b.file, &game).unwrap(), 1);
        assert_eq!(fs::read(game.join("saves/slot1.sav")).unwrap(), b"save");
    }

    /// A game with its {data} caches and saves, as a builder leaves it.
    fn game_with_caches(d: &Path) -> PathBuf {
        let game = d.join("Game");
        fs::create_dir_all(game.join("recompdata/cache/shaders")).unwrap();
        fs::create_dir_all(game.join("recompdata/saves")).unwrap();
        fs::write(game.join("Game.exe"), b"exe").unwrap();
        fs::write(game.join("recompdata/cache/shaders/pipeline.bin"), b"cache").unwrap();
        fs::write(game.join("recompdata/saves/slot1.sav"), b"save").unwrap();
        game
    }

    fn has_caches(game: &Path) -> bool {
        fs::read(game.join("recompdata/cache/shaders/pipeline.bin")).ok().as_deref() == Some(b"cache".as_slice())
            && fs::read(game.join("recompdata/saves/slot1.sav")).ok().as_deref() == Some(b"save".as_slice())
    }

    #[test]
    fn moving_a_game_carries_its_caches() {
        let d = tmp("move-caches");
        let game = game_with_caches(&d);
        let to = d.join("other-library").join("Game");
        move_dir(&game, &to, &|_, _| {}).unwrap();
        assert!(has_caches(&to));
        assert!(!game.exists());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn copying_a_game_to_another_drive_carries_its_caches() {
        // What a move to another drive does: every file, the caches in {data} too.
        let d = tmp("copy-caches");
        let game = game_with_caches(&d);
        let to = d.join("other-drive").join("Game");
        let total = dir_size(&game);
        let mut done = 0;
        copy_tree(&game, &to, &mut done, total, &|_, _| {}).unwrap();
        assert!(has_caches(&to));
        assert_eq!(done, total);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_game_moves_into_a_deep_library_folder() {
        // Past Windows' old 260-character limit, the launcher's own copying still goes through.
        let d = tmp("deep");
        let game = game_with_caches(&d);
        let mut lib = d.join("libraries");
        while lib.as_os_str().len() < 300 {
            lib = lib.join("a-library-folder-with-a-long-name");
        }
        let to = lib.join("Game");
        let mut done = 0;
        copy_tree(&game, &to, &mut done, dir_size(&game), &|_, _| {}).unwrap();
        assert!(has_caches(&to));
        move_dir(&game, &lib.join("Game2"), &|_, _| {}).unwrap();
        assert!(has_caches(&lib.join("Game2")));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn nothing_to_back_up_makes_no_backup() {
        let d = tmp("bk-none");
        assert!(backup(&d.join("game"), &[], &d.join("backups"), 2).unwrap().is_none());
        assert!(!d.join("backups").exists());
    }

    #[test]
    fn keeps_the_newest_few() {
        let d = tmp("keep");
        let b = d.join("backups");
        fs::create_dir_all(&b).unwrap();
        for t in [100u64, 200, 300] {
            fs::write(b.join(format!("{t}.zip")), b"").unwrap();
        }
        let game = d.join("g");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("s.sav"), b"x").unwrap();
        backup(&game, &[PathBuf::from("s.sav")], &b, 2).unwrap();
        let left: Vec<u64> = list_backups(&b).iter().map(|x| x.when).collect();
        assert_eq!(left.len(), 2);
        assert!(!left.contains(&100) && !left.contains(&200));
    }

    #[test]
    fn moves_folders() {
        let d = tmp("mv");
        let a = d.join("a/game");
        fs::create_dir_all(a.join("sub")).unwrap();
        fs::write(a.join("sub/f.txt"), b"hello").unwrap();
        move_dir(&a, &d.join("b/game"), &|_, _| {}).unwrap();
        assert!(!a.exists());
        assert_eq!(fs::read(d.join("b/game/sub/f.txt")).unwrap(), b"hello");
        let mut done = 0;
        copy_tree(&d.join("b/game"), &d.join("c/game"), &mut done, 5, &|_, _| {}).unwrap();
        assert_eq!(done, 5);
    }

    /// A Mac app's framework, the way it ships: a version folder and links to it.
    #[cfg(unix)]
    fn app_with_framework(d: &Path) -> PathBuf {
        use std::os::unix::fs::symlink;
        let app = d.join("source/Game.app");
        let framework = app.join("Contents/Frameworks/Engine.framework");
        fs::create_dir_all(framework.join("Versions/A")).unwrap();
        fs::write(framework.join("Versions/A/Engine"), b"library").unwrap();
        symlink("A", framework.join("Versions/Current")).unwrap();
        symlink("Versions/Current/Engine", framework.join("Engine")).unwrap();
        app
    }

    #[test]
    #[cfg(unix)]
    fn copying_to_another_drive_keeps_links_as_links() {
        let d = tmp("copy-links");
        let app = app_with_framework(&d);
        let to = d.join("dest/Game.app");
        let mut done = 0;
        copy_tree(&app, &to, &mut done, dir_size(&app), &|_, _| {}).unwrap();
        let framework = to.join("Contents/Frameworks/Engine.framework");
        assert_eq!(fs::read_link(framework.join("Versions/Current")).unwrap(), PathBuf::from("A"));
        assert_eq!(fs::read_link(framework.join("Engine")).unwrap(), PathBuf::from("Versions/Current/Engine"));
        assert_eq!(fs::read(framework.join("Engine")).unwrap(), b"library");
        assert_eq!(done, 7, "what a link points to is copied once, not again through the link");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    #[cfg(unix)]
    fn a_link_that_points_nowhere_is_copied_too() {
        let d = tmp("copy-dangling");
        let game = d.join("game");
        fs::create_dir_all(&game).unwrap();
        std::os::unix::fs::symlink("missing", game.join("optional")).unwrap();
        let mut done = 0;
        copy_tree(&game, &d.join("dest"), &mut done, 0, &|_, _| {}).unwrap();
        assert_eq!(fs::read_link(d.join("dest/optional")).unwrap(), PathBuf::from("missing"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    #[cfg(unix)]
    fn a_link_into_the_game_by_full_path_points_at_the_new_place() {
        let d = tmp("copy-full-link");
        let game = d.join("game");
        fs::create_dir_all(game.join("lib")).unwrap();
        fs::write(game.join("lib/libgame.so.1"), b"library").unwrap();
        std::os::unix::fs::symlink(game.join("lib/libgame.so.1"), game.join("libgame.so")).unwrap();
        let to = d.join("dest");
        let mut done = 0;
        copy_tree(&game, &to, &mut done, 0, &|_, _| {}).unwrap();
        assert_eq!(fs::read_link(to.join("libgame.so")).unwrap(), to.join("lib/libgame.so.1"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    #[cfg(unix)]
    fn a_link_into_the_games_real_path_points_at_the_new_place() {
        // The game is reached through a link (Bazzite's /home), its link names the real path.
        let d = fs::canonicalize(tmp("copy-real-link")).unwrap();
        let game = d.join("real/game");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("data.pak"), b"data").unwrap();
        std::os::unix::fs::symlink(game.join("data.pak"), game.join("current.pak")).unwrap();
        std::os::unix::fs::symlink(d.join("real"), d.join("home")).unwrap();
        let to = d.join("dest");
        let mut done = 0;
        copy_tree(&d.join("home/game"), &to, &mut done, 0, &|_, _| {}).unwrap();
        assert_eq!(fs::read_link(to.join("current.pak")).unwrap(), to.join("data.pak"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    #[cfg(unix)]
    fn a_link_outside_the_game_stays_as_it_is() {
        let d = tmp("copy-outside-link");
        let game = d.join("game");
        fs::create_dir_all(&game).unwrap();
        std::os::unix::fs::symlink("/", game.join("z:")).unwrap();
        let mut done = 0;
        copy_tree(&game, &d.join("dest"), &mut done, 0, &|_, _| {}).unwrap();
        assert_eq!(fs::read_link(d.join("dest/z:")).unwrap(), PathBuf::from("/"));
        let _ = fs::remove_dir_all(&d);
    }

    /// A game beside a Shared folder, its asset.dat a relative link into it (../Shared/asset.dat).
    #[cfg(unix)]
    fn game_linking_beside_it(d: &Path) -> PathBuf {
        fs::create_dir_all(d.join("source/Shared")).unwrap();
        fs::write(d.join("source/Shared/asset.dat"), b"shared").unwrap();
        let game = d.join("source/Game");
        fs::create_dir_all(&game).unwrap();
        std::os::unix::fs::symlink("../Shared/asset.dat", game.join("asset.dat")).unwrap();
        game
    }

    #[test]
    #[cfg(unix)]
    fn a_relative_link_out_of_the_game_still_reaches_its_file() {
        let d = tmp("copy-relative-out");
        let game = game_linking_beside_it(&d);
        let mut done = 0;
        copy_tree(&game, &d.join("dest/Game"), &mut done, 0, &|_, _| {}).unwrap();
        assert_eq!(fs::read(d.join("dest/Game/asset.dat")).unwrap(), b"shared");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    #[cfg(unix)]
    fn a_relative_link_out_of_the_game_points_there_by_full_path() {
        // The real path: on a Mac the temp folder is reached through /var, a link to /private/var.
        let d = fs::canonicalize(tmp("copy-relative-full")).unwrap();
        let game = game_linking_beside_it(&d);
        let mut done = 0;
        copy_tree(&game, &d.join("dest/Game"), &mut done, 0, &|_, _| {}).unwrap();
        assert_eq!(fs::read_link(d.join("dest/Game/asset.dat")).unwrap(), d.join("source/Shared/asset.dat"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    #[cfg(unix)]
    fn a_relative_link_up_and_back_into_the_game_stays_relative() {
        let d = tmp("copy-relative-in");
        let game = d.join("game");
        fs::create_dir_all(game.join("lib")).unwrap();
        fs::create_dir_all(game.join("data")).unwrap();
        fs::write(game.join("data/level.pak"), b"level").unwrap();
        std::os::unix::fs::symlink("../data/level.pak", game.join("lib/level.pak")).unwrap();
        let mut done = 0;
        copy_tree(&game, &d.join("dest"), &mut done, 0, &|_, _| {}).unwrap();
        assert_eq!(fs::read_link(d.join("dest/lib/level.pak")).unwrap(), PathBuf::from("../data/level.pak"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    #[cfg(unix)]
    fn a_copied_program_can_still_run() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmp("copy-mode");
        let game = d.join("game");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("Game"), b"program").unwrap();
        fs::set_permissions(game.join("Game"), fs::Permissions::from_mode(0o755)).unwrap();
        let mut done = 0;
        copy_tree(&game, &d.join("dest"), &mut done, 0, &|_, _| {}).unwrap();
        assert_eq!(fs::metadata(d.join("dest/Game")).unwrap().permissions().mode() & 0o777, 0o755);
        let _ = fs::remove_dir_all(&d);
    }
}

/// For a game whose own setup works from a clone of its repo: clone it (shallow) and put it in
/// the game's folder around the release's files, which stay as they are. A folder that's already
/// a clone is brought up to date instead.
pub fn clone_repo(repo: &str, into: &Path, scratch: &Path) -> Result<(), Error> {
    let git = |args: &[&std::ffi::OsStr]| -> io::Result<std::process::Output> {
        let mut c = std::process::Command::new("git");
        crate::sysenv::clean_std(&mut c);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000);
        }
        c.args(args).env("GIT_TERMINAL_PROMPT", "0").output()
    };
    let no_git = || Error::Msg("This game builds itself from its project files, which need Git. Install Git for Windows from git-scm.com, then press Install again.".into());
    if into.join(".git").exists() {
        let _ = git(&["-C".as_ref(), into.as_os_str(), "pull".as_ref(), "--ff-only".as_ref()]);
        return Ok(());
    }
    let _ = fs::remove_dir_all(scratch);
    if let Some(p) = scratch.parent() {
        fs::create_dir_all(p)?;
    }
    // Only a GitHub project is cloned (a GitLab or SourceForge one has no such path here).
    if crate::catalog::site_only(repo) {
        return Err(Error::Msg("This game builds itself from its project files, which only works for projects on GitHub for now.".into()));
    }
    let url = format!("https://github.com/{repo}.git");
    let out = git(&["clone".as_ref(), "--depth".as_ref(), "1".as_ref(), url.as_ref(), scratch.as_os_str()]).map_err(|e| if e.kind() == io::ErrorKind::NotFound { no_git() } else { Error::Io(e) })?;
    if !out.status.success() {
        let _ = fs::remove_dir_all(scratch);
        let why = String::from_utf8_lossy(&out.stderr).lines().filter(|l| !l.trim().is_empty()).last().unwrap_or("").to_string();
        return Err(Error::Msg(format!("Couldn't get its project files from GitHub. {why}")));
    }
    fs::create_dir_all(into)?;
    for e in fs::read_dir(scratch)?.flatten() {
        let to = into.join(e.file_name());
        if !to.exists() {
            fs::rename(e.path(), &to)?;
        }
    }
    let _ = fs::remove_dir_all(scratch);
    Ok(())
}

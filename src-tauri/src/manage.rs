//! Looking after installed games: moving them between drives, backing up what's the player's
//! (saves, settings, mods) and putting it back, and checking the PC has what recomps need.

use std::collections::HashSet;
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
    fs::create_dir_all(to)?;
    for e in fs::read_dir(from)? {
        let e = e?;
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if e.file_type()?.is_dir() {
            copy_tree(&src, &dst, done, total, report)?;
        } else {
            let mut i = fs::File::open(&src)?;
            let mut o = fs::File::create(&dst)?;
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
            if let Ok(m) = e.metadata() {
                let _ = fs::set_permissions(&dst, m.permissions());
            }
        }
    }
    Ok(())
}

const PROGRAM_ENDS: [&str; 9] = [".exe", ".dll", ".so", ".appimage", ".pdb", ".msi", ".zip", ".7z", ".tar.gz"];

/// The files in a game's folder that are the player's rather than the release's: saves, settings,
/// mods. `release` is what the last install put there (empty for games installed before the launcher
/// kept that list: then anything that isn't a program). Never a file over 50 MB.
pub fn player_files(dir: &Path, release: &[PathBuf]) -> Vec<PathBuf> {
    player_files_within(dir, release).unwrap_or_default()
}

/// The player asked to skip the backup that's being made (the update goes on without it).
pub static SKIP_BACKUP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Folders that hold a game's data (a disc or console dump copied in), never saves or settings.
const DATA_DIRS: [&str; 14] = [
    "ps3_game", "usrdir", "sce_sys", "sce_module", "ps4_game", "app0", "romfs", "exefs", "shadercache", "shader_cache", "cache", "logs", "crashdumps", ".backups",
];
/// Bigger than this, a single file isn't a save or a setting.
const PLAYER_FILE_MAX: u64 = 16 * 1024 * 1024;
/// More than this altogether is game data, not the player's files: no backup then.
const PLAYER_TOTAL_MAX: u64 = 512 * 1024 * 1024;
const PLAYER_COUNT_MAX: usize = 8_000;

/// None when what's there is too much to be the player's files (a dump unpacked into the game's
/// folder), took too long to look through, or the player skipped it.
fn player_files_within(dir: &Path, release: &[PathBuf]) -> Option<Vec<PathBuf>> {
    use std::sync::atomic::Ordering;
    let release: HashSet<&PathBuf> = release.iter().collect();
    let start = std::time::Instant::now();
    let mut out = vec![];
    let mut total = 0u64;
    let mut seen = 0usize;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).into_iter().flatten().flatten() {
            seen += 1;
            if seen % 256 == 0 && (start.elapsed().as_secs() > 20 || SKIP_BACKUP.load(Ordering::Relaxed)) {
                return None;
            }
            if e.file_type().map(|t| t.is_symlink()).unwrap_or(true) {
                continue;
            }
            let p = e.path();
            let Ok(m) = e.metadata() else { continue };
            if m.is_dir() {
                let name = e.file_name().to_string_lossy().to_lowercase();
                if !DATA_DIRS.contains(&name.as_str()) {
                    stack.push(p);
                }
                continue;
            }
            let Ok(rel) = p.strip_prefix(dir) else { continue };
            let rel = rel.to_path_buf();
            let n = rel.to_string_lossy().to_lowercase();
            // Saves, settings and mods, not the player's own game files (a disc's worth of data
            // copied in): those don't change with an update and would make every backup gigabytes.
            let mine = if release.is_empty() { !PROGRAM_ENDS.iter().any(|e| n.ends_with(e)) } else { !release.contains(&rel) };
            if mine && m.len() <= PLAYER_FILE_MAX && !DATA_ENDS.iter().any(|e| n.ends_with(e)) {
                total += m.len();
                out.push(rel);
                if total > PLAYER_TOTAL_MAX || out.len() > PLAYER_COUNT_MAX {
                    return None;
                }
            }
        }
    }
    out.sort();
    Some(out)
}

/// Game data and disc images: never the player's files.
const DATA_ENDS: [&str; 16] = [".iso", ".bin", ".cue", ".chd", ".rvz", ".wbfs", ".xex", ".xbe", ".sdat", ".edat", ".sprx", ".self", ".pkg", ".psarc", ".z64", ".log"];

#[derive(Debug, Clone, Serialize)]
pub struct Backup {
    pub file: PathBuf,
    pub name: String,
    pub when: u64,
    pub size: u64,
    pub files: usize,
}

/// Zip the player's files into `<backups>/<when>.zip`, keeping the newest `keep`. None when there's
/// nothing of theirs to keep.
pub fn backup(dir: &Path, release: &[PathBuf], backups: &Path, keep: usize) -> Result<Option<Backup>, Error> {
    SKIP_BACKUP.store(false, std::sync::atomic::Ordering::Relaxed);
    let Some(files) = player_files_within(dir, release) else { return Ok(None) };
    if files.is_empty() {
        return Ok(None);
    }
    fs::create_dir_all(backups)?;
    let when = crate::library::now();
    let file = backups.join(format!("{when}.zip"));
    {
        let mut z = zip::ZipWriter::new(fs::File::create(&file)?);
        let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for rel in &files {
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
    pub vulkan: Option<bool>,
    /// vkBasalt, for looks on Linux.
    pub vkbasalt: Option<bool>,
    /// On Linux, what plays Windows recomps (a Proton's name, umu-run or Wine); empty when nothing does.
    pub proton: Option<String>,
    pub avx: bool,
    pub avx2: bool,
    pub cores: usize,
}

#[cfg(windows)]
fn quiet(cmd: &str) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    let mut c = std::process::Command::new(cmd);
    c.creation_flags(0x0800_0000); // no console window
    c
}

pub fn pc_check() -> PcCheck {
    let mut c = PcCheck { cores: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1), ..Default::default() };
    #[cfg(target_arch = "x86_64")]
    {
        c.avx = std::arch::is_x86_feature_detected!("avx");
        c.avx2 = std::arch::is_x86_feature_detected!("avx2");
    }
    #[cfg(windows)]
    {
        let out = quiet("reg").args(["query", r"HKLM\SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64", "/v", "Installed"]).output();
        c.vcredist = Some(out.map(|o| String::from_utf8_lossy(&o.stdout).contains("0x1")).unwrap_or(false));
        let sys = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        c.vulkan = Some(Path::new(&sys).join("System32").join("vulkan-1.dll").is_file());
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
    fn backups_keep_only_the_players_files() {
        let d = tmp("bk");
        let game = d.join("game");
        fs::create_dir_all(game.join("saves")).unwrap();
        fs::write(game.join("Game.exe"), b"exe").unwrap();
        fs::write(game.join("data.bin"), b"release data").unwrap();
        fs::write(game.join("saves/slot1.sav"), b"save").unwrap();
        fs::write(game.join("settings.json"), b"{}").unwrap();
        let release = vec![PathBuf::from("Game.exe"), PathBuf::from("data.bin")];
        let b = backup(&game, &release, &d.join("backups"), 2).unwrap().unwrap();
        assert_eq!(b.files, 2);
        fs::write(game.join("saves/slot1.sav"), b"broken").unwrap();
        assert_eq!(restore(&b.file, &game).unwrap(), 2);
        assert_eq!(fs::read(game.join("saves/slot1.sav")).unwrap(), b"save");
        // Old installs without a list: programs left out.
        let guess = player_files(&game, &[]);
        assert!(guess.contains(&PathBuf::from("saves/slot1.sav")) && !guess.contains(&PathBuf::from("Game.exe")));
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
        backup(&game, &[], &b, 2).unwrap();
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

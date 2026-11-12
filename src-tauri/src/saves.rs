//! Save backups. Where each game keeps its saves (the developer's recomplauncher.json says, or the
//! catalog does, or the launcher noticed while the game ran, or else the player's files in the
//! game's folder), a copy after every session (the newest ten on this PC), and putting one back.
//!
//! A copy is a zip: the files under `<n>/<path>` for the n-th save place, and `rlsave.json` saying
//! what the places were (as `{local}/Game/saves`, not this PC's own path, so a copy made on one PC
//! goes to the right place on another), each file's checksum, and a checksum of the whole set.

use crate::library::Installed;
use crate::Error;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// The newest copies kept on this PC, per game.
pub const KEEP: usize = 10;
/// Nothing bigger than this is a save (it's a disc image, a video, a mod pack).
const MAX_FILE: u64 = 64 * 1024 * 1024;
/// The most the game-folder guess may come to.
const GAME_FOLDER_MAX: u64 = 50 * 1024 * 1024;
/// No save place is bigger than this.
const SPOT_MAX: u64 = 1024 * 1024 * 1024;
/// Save places the last look left out for being too big (the backup says so).
static OVERSIZE: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
/// More files than this in a save place means it isn't one.
const MAX_FILES: usize = 20_000;
/// Entries and seconds a save place may take to look through before it counts as too big.
const WALK_MAX: usize = 250_000;
const WALK_SECS: u64 = 20;
/// Never in a save copy: game files, programs, archives, logs.
const NOT_SAVES: [&str; 37] = [
    ".iso", ".cue", ".chd", ".cso", ".gcm", ".rvz", ".wbfs", ".wad", ".nsp", ".xci", ".nup", ".xex", ".xbe", ".z64", ".n64", ".v64",
    ".sfc", ".smc", ".gba", ".nds", ".3ds", ".elf", ".rpx", ".exe", ".dll", ".so", ".pdb", ".msi", ".zip", ".7z", ".rar", ".appimage",
    ".log", ".tmp", ".dmp", ".mp4", ".mkv",
];
/// Folders that are never saves.
const NOT_SAVE_DIRS: [&str; 16] = [
    "mods", "shadercache", "shader_cache", "shaders", "cache", "caches", "logs", "log", "crashdumps", "dumps", "screenshots", ".backups",
    "texture_packs", "textures", "packs", "temp",
];

/// A place a game keeps its saves.
#[derive(Debug, Clone, Serialize)]
pub struct Spot {
    /// As written: `{local}/Zelda64Recompiled/saves`, `{install}/save/*.sav`.
    pub template: String,
    /// "developer", "catalog", "found" or "game".
    pub from: &'static str,
}

/// Where a game's saves are, best first: the developer's file, the catalog, what was noticed while
/// it ran, else its own folder.
pub fn spots(dev: &[String], catalog: &[String], g: &Installed) -> Vec<Spot> {
    // The player's own folders: one, or several (a launcher with more than one game in it).
    let mut mine: Vec<String> = vec![];
    for t in g.save_place.iter().chain(g.more_save_places.iter()).map(|t| t.trim()).filter(|t| !t.is_empty()) {
        if !mine.iter().any(|m| m == t) {
            mine.push(t.to_string());
        }
    }
    if !mine.is_empty() {
        return mine.into_iter().map(|template| Spot { template, from: "player" }).collect();
    }
    let list = |v: &[String], from: &'static str| v.iter().filter(|t| !t.trim().is_empty()).map(|t| Spot { template: t.trim().to_string(), from }).collect::<Vec<_>>();
    // Places the launcher once saw the game write to, without the ones that turned out to hold
    // programs (a runtime it downloaded on its first start): those are never saves.
    let found: Vec<String> = g.found_saves.iter().filter(|t| !(t.starts_with("{install}/") && holds_programs(&resolve_for(t, g)))).cloned().collect();
    for (v, from) in [(dev, "developer"), (catalog, "catalog"), (&found[..], "found")] {
        let s = list(v, from);
        if !s.is_empty() {
            return s;
        }
    }
    vec![Spot { template: "{install}".into(), from: "game" }]
}

/// The player's save folders with `dir` added to `now` (the ones it has): refused when it's one of
/// them already, or inside one or around one (its files would be in two places).
pub fn add_place(now: &[String], dir: &Path, g: &Installed) -> Result<Vec<String>, String> {
    let t = templatize_for(dir, g);
    let new = resolve_for(&t, g);
    for old in now {
        let o = resolve_for(old, g);
        if *old == t || o == new {
            return Err("That folder is already one of its save folders.".into());
        }
        if new.starts_with(&o) || o.starts_with(&new) {
            return Err("That folder is inside one of its save folders, or holds one. Pick folders side by side.".into());
        }
    }
    let mut out = now.to_vec();
    out.push(t);
    Ok(out)
}

/// A template on this PC.
pub fn resolve(template: &str, install: &Path) -> PathBuf {
    let t = crate::profile::fill(template, install, None);
    PathBuf::from(if cfg!(windows) { t.replace('/', "\\") } else { t })
}

/// A Windows game on Linux, started through Proton (or Wine): its AppData and Documents are the
/// prefix's, not Linux's.
fn windows_user(g: &Installed) -> Option<PathBuf> {
    if cfg!(windows) {
        return None;
    }
    let exe = g.exe.as_ref().and_then(|e| e.extension()).map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if !(g.proton || matches!(exe.as_str(), "exe" | "bat" | "cmd")) {
        return None;
    }
    crate::proton::windows_user()
}

/// Windows' own folders, as they are under a Windows user folder.
const WIN_PLACES: [(&str, &str); 5] = [("{appdata}", "AppData/Roaming"), ("{local}", "AppData/Local"), ("{config}", "AppData/Roaming"), ("{documents}", "Documents"), ("{home}", "")];

fn in_user(t: &str, user: &Path) -> Option<PathBuf> {
    for (k, sub) in WIN_PLACES {
        let Some(rest) = t.strip_prefix(k) else { continue };
        if !(rest.is_empty() || rest.starts_with('/') || rest.starts_with('\\')) {
            continue;
        }
        let base = if sub.is_empty() { user.to_path_buf() } else { user.join(sub) };
        let rest = rest.trim_start_matches(['/', '\\']);
        return Some(if rest.is_empty() { base } else { base.join(rest) });
    }
    None
}

/// A template on this PC, for this game: the same place on Windows and in the Proton prefix on
/// Linux for a Windows game, so cloud saves go between a Windows PC and a Linux handheld.
pub fn resolve_for(template: &str, g: &Installed) -> PathBuf {
    if let Some(p) = windows_user(g).and_then(|u| in_user(template, &u)) {
        return p;
    }
    resolve(template, &g.path)
}

/// This PC's path as a template, for this game (see `resolve_for`).
pub fn templatize_for(p: &Path, g: &Installed) -> String {
    if let Some(user) = windows_user(g) {
        if !p.starts_with(&g.path) {
            for (k, sub) in WIN_PLACES {
                let base = if sub.is_empty() { user.clone() } else { user.join(sub) };
                if let Ok(rest) = p.strip_prefix(&base) {
                    let r = rest.to_string_lossy().replace('\\', "/");
                    return if r.is_empty() { k.to_string() } else { format!("{k}/{r}") };
                }
            }
        }
    }
    templatize(p, &g.path)
}

/// This PC's path as a template that works on any PC: `C:\Users\Ruben\AppData\Local\X` -> `{local}/X`.
pub fn templatize(p: &Path, install: &Path) -> String {
    let bases: Vec<(&str, Option<PathBuf>)> = vec![
        ("{install}", Some(install.to_path_buf())),
        ("{local}", dirs::data_local_dir()),
        ("{config}", dirs::config_dir()),
        ("{documents}", dirs::document_dir().or_else(|| dirs::home_dir().map(|h| h.join("Documents")))),
        ("{home}", dirs::home_dir()),
    ];
    for (name, base) in bases {
        let Some(b) = base else { continue };
        if let Ok(rest) = p.strip_prefix(&b) {
            let r = rest.to_string_lossy().replace('\\', "/");
            return if r.is_empty() { name.to_string() } else { format!("{name}/{r}") };
        }
    }
    p.to_string_lossy().replace('\\', "/")
}

fn lower(p: &Path) -> String {
    p.to_string_lossy().to_lowercase()
}

/// A .bin bigger than this is a disc image's track, not a save (Xbox and many PC ports keep saves
/// in small .bin files).
const BIN_SAVE_MAX: u64 = 16 * 1024 * 1024;

/// A folder with programs in it (a runtime a game downloads on its first start, like Java or
/// Python, or its own tools): never a save place, even when the game writes small files there.
pub fn holds_programs(dir: &Path) -> bool {
    const PROGRAMS: [&str; 7] = [".exe", ".dll", ".so", ".dylib", ".jar", ".jmod", ".pyd"];
    let mut stack = vec![(dir.to_path_buf(), 0usize)];
    let mut seen = 0usize;
    while let Some((d, depth)) = stack.pop() {
        for e in fs::read_dir(&d).into_iter().flatten().flatten() {
            seen += 1;
            if seen > 4000 {
                return false;
            }
            let Ok(t) = e.file_type() else { continue };
            let n = e.file_name().to_string_lossy().to_lowercase();
            if t.is_dir() {
                if depth < 3 {
                    stack.push((e.path(), depth + 1));
                }
            } else if PROGRAMS.iter().any(|x| n.ends_with(x)) || n.contains(".so.") || crate::proton::elf_arch(&e.path()).is_some() {
                return true;
            }
        }
    }
    false
}

fn is_save_file(name: &str, size: u64) -> bool {
    let n = name.to_lowercase();
    if n.starts_with(".recomplauncher-input-") { return false; }
    if n.ends_with(".bin") && size > BIN_SAVE_MAX {
        return false;
    }
    size <= MAX_FILE && !NOT_SAVES.iter().any(|e| n.ends_with(e))
}

fn skip_dir(name: &str) -> bool {
    NOT_SAVE_DIRS.contains(&name.to_lowercase().as_str())
}

/// `*.sav` against a file name.
fn wild(pat: &str, name: &str) -> bool {
    let (p, n) = (pat.to_lowercase(), name.to_lowercase());
    let parts: Vec<&str> = p.split('*').collect();
    if parts.len() == 1 {
        return p == n;
    }
    let mut rest = n.as_str();
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            if !rest.starts_with(part) {
                return false;
            }
            rest = &rest[part.len()..];
        } else if i == parts.len() - 1 {
            return rest.ends_with(part);
        } else if let Some(k) = rest.find(part) {
            rest = &rest[k + part.len()..];
        } else {
            return false;
        }
    }
    true
}

/// One file of a save: which place it's in, and its path inside it.
#[derive(Debug, Clone)]
pub struct Entry {
    pub spot: usize,
    pub rel: String,
    pub abs: PathBuf,
    pub size: u64,
}

/// Every save file under a folder. Gives up (false) on a folder far too big to be a save place:
/// more than WALK_MAX entries, or longer than WALK_SECS to look through (a slow or network drive).
/// Links aren't followed, so a folder that links back into itself can't loop forever.
fn walk(root: &Path, out: &mut Vec<(String, PathBuf, u64)>, release: &HashSet<String>, budget: u64, secs: u64) -> bool {
    let start = std::time::Instant::now();
    let mut seen = 0usize;
    let mut total = 0u64;
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((d, depth)) = stack.pop() {
        for e in fs::read_dir(&d).into_iter().flatten().flatten() {
            if out.len() >= MAX_FILES {
                return true;
            }
            seen += 1;
            if seen > WALK_MAX || total > budget || (seen % 256 == 0 && start.elapsed().as_secs() > secs) {
                return false;
            }
            if e.file_type().map(|t| t.is_symlink()).unwrap_or(true) {
                continue;
            }
            let p = e.path();
            let Ok(m) = e.metadata() else { continue };
            let name = e.file_name().to_string_lossy().to_string();
            if m.is_dir() {
                if !skip_dir(&name) && depth < 16 {
                    stack.push((p, depth + 1));
                }
                continue;
            }
            let Ok(rel) = p.strip_prefix(root) else { continue };
            let rel = rel.to_string_lossy().replace('\\', "/");
            if is_save_file(&name, m.len()) && !release.contains(&rel.to_lowercase()) {
                total += m.len();
                out.push((rel, p, m.len()));
            }
        }
    }
    true
}

/// Every file in the save places, as they are now.
pub fn collect(spots: &[Spot], g: &Installed) -> Vec<Entry> {
    let release: HashSet<String> = g.files.iter().map(|f| f.to_string_lossy().replace('\\', "/").to_lowercase()).collect();
    let none = HashSet::new();
    let mut out = vec![];
    for (i, s) in spots.iter().enumerate() {
        let p = resolve_for(&s.template, g);
        let last = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let mut found: Vec<(String, PathBuf, u64)> = vec![];
        if last.contains('*') {
            let dir = p.parent().map(Path::to_path_buf).unwrap_or_default();
            for e in fs::read_dir(&dir).into_iter().flatten().flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if let Ok(m) = e.metadata() {
                    if m.is_file() && wild(&last, &name) && is_save_file(&name, m.len()) {
                        found.push((name, e.path(), m.len()));
                    }
                }
            }
        } else if p.is_file() {
            let size = fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            if is_save_file(&last, size) {
                found.push((last.clone(), p.clone(), size));
            }
        } else if p.is_dir() {
            // The game's own folder: only what the release didn't put there.
            let rel_of_install = s.template.starts_with("{install}");
            // A guess (the game's folder, or a place it was seen writing to) gives up as soon as
            // it's too big to be saves, rather than looking through gigabytes of game data.
            let guess = s.from == "game" || s.from == "found";
            let (budget, secs) = if guess { (GAME_FOLDER_MAX, 6) } else { (SPOT_MAX, WALK_SECS) };
            if !walk(&p, &mut found, if rel_of_install && lower(&p) == lower(&g.path) { &release } else { &none }, budget, secs) {
                if s.from != "game" {
                    if let Ok(mut o) = OVERSIZE.lock() {
                        o.push(s.template.clone());
                    }
                }
                found.clear();
            }
            // A guess at the whole game folder that comes out big is game data (unpacked from the
            // player's disc, say), not saves: nothing is copied rather than the game.
            if s.from == "game" && found.iter().map(|f| f.2).sum::<u64>() > GAME_FOLDER_MAX {
                found.clear();
            }
        }
        // A place the launcher noticed the game writing to (a guess too) that comes out big is a
        // cache or game data, not saves; and no save place is ever this big.
        let total: u64 = found.iter().map(|f| f.2).sum();
        if (s.from == "found" && total > GAME_FOLDER_MAX) || total > SPOT_MAX {
            if let Ok(mut o) = OVERSIZE.lock() {
                o.push(s.template.clone());
            }
            found.clear();
        }
        found.retain(|(_, abs, _)| !crate::inputs::excluded(g, abs));
        found.sort();
        out.extend(found.into_iter().map(|(rel, abs, size)| Entry { spot: i, rel, abs, size }));
    }
    out
}

fn sha_file(p: &Path) -> io::Result<String> {
    let mut f = fs::File::open(p)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// The checksum of a whole set of files: the same files with the same contents, the same checksum.
pub fn set_hash(files: &[FileInfo]) -> String {
    let mut lines: Vec<String> = files.iter().map(|f| format!("{}\t{}\t{}", f.spot, f.rel, f.sha256)).collect();
    lines.sort();
    hex::encode(Sha256::digest(lines.join("\n").as_bytes()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub spot: usize,
    pub rel: String,
    pub size: u64,
    pub sha256: String,
}

/// What `rlsave.json` in a copy says.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Manifest {
    pub format: u32,
    pub game: String,
    pub made: u64,
    #[serde(default)]
    pub device: String,
    #[serde(default)]
    pub played: u64,
    pub hash: String,
    pub spots: Vec<String>,
    pub files: Vec<FileInfo>,
}

/// The saves as they are now: each file's checksum, and the set's.
pub fn current(spots: &[Spot], g: &Installed) -> (Vec<Entry>, Vec<FileInfo>, String) {
    let entries = collect(spots, g);
    let infos: Vec<FileInfo> = entries
        .iter()
        .filter_map(|e| Some(FileInfo { spot: e.spot, rel: e.rel.clone(), size: e.size, sha256: sha_file(&e.abs).ok()? }))
        .collect();
    let hash = if infos.is_empty() { String::new() } else { set_hash(&infos) };
    (entries, infos, hash)
}

pub fn device_name() -> String {
    std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "This PC".into())
}

/// A copy of a game's saves, on this PC.
#[derive(Debug, Clone, Serialize)]
pub struct Copy {
    pub name: String,
    pub when: u64,
    pub size: u64,
    pub files: usize,
    pub hash: String,
    pub device: String,
    pub played: u64,
}

/// A folder the player picked for the copies (another drive, a NAS, a folder Drive or OneDrive
/// keeps in sync): each game's copies in a folder of their own inside it.
static ROOT: std::sync::RwLock<Option<PathBuf>> = std::sync::RwLock::new(None);

pub fn set_root(p: Option<PathBuf>) {
    *ROOT.write().unwrap() = p.filter(|p| !p.as_os_str().is_empty());
}

/// The player's folder, when they picked one.
pub fn chosen_root() -> Option<PathBuf> {
    ROOT.read().unwrap().clone()
}

/// Where the copies go: the player's folder, or the launcher's own when none is picked or it can't
/// be reached right now (a drive that's unplugged, a network folder that's offline). Those copies
/// are kept here meanwhile, and nothing waits on the missing folder.
pub fn root(app_dir: &Path) -> PathBuf {
    match chosen_root() {
        Some(p) if p.is_dir() => p,
        _ => app_dir.join("saves"),
    }
}

pub fn dir_of(app_dir: &Path, id: &str) -> PathBuf {
    root(app_dir).join(id)
}

/// Move completed backup archives only after every destination has been verified by checksum.
/// Conflicting names never overwrite history, and a failed copy leaves all originals in place.
pub fn move_copies(from: &Path, to: &Path) -> io::Result<usize> {
    fs::create_dir_all(to)?;
    let (from, to) = (fs::canonicalize(from)?, fs::canonicalize(to)?);
    if from == to {
        return Ok(0);
    }
    if to.starts_with(&from) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Pick a backup folder outside the current backup folder."));
    }
    let mut verified = Vec::new();
    let mut games = Vec::new();
    for game in fs::read_dir(&from)? {
        let game = game?;
        if !game.file_type()?.is_dir() {
            continue;
        }
        let src = game.path();
        let dst = to.join(game.file_name());
        let before = verified.len();
        for f in fs::read_dir(&src)? {
            let f = f?;
            let name = f.file_name();
            // state.json belongs to this PC; .part files may still be written by a backup.
            // Only the timestamp-named ZIPs returned by list() are completed save copies.
            if !f.file_type()?.is_file() || name.to_str().and_then(|n| n.strip_suffix(".zip")).and_then(|n| n.parse::<u64>().ok()).is_none() {
                continue;
            }
            fs::create_dir_all(&dst)?;
            if fs::symlink_metadata(&dst)?.file_type().is_symlink() {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "A game's destination folder is a link. Pick another backup folder."));
            }
            let (a, b) = (f.path(), dst.join(name));
            let hash = sha_file(&a)?;
            match fs::symlink_metadata(&b) {
                Ok(m) => {
                    if !m.is_file() || sha_file(&b)? != hash {
                        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} already contains a different backup. Both copies were kept; pick another backup folder.", b.display())));
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    // create_new also prevents a copy from replacing a file created since the check.
                    let mut out = fs::OpenOptions::new().write(true).create_new(true).open(&b)?;
                    let copied = (|| -> io::Result<()> {
                        out.set_permissions(fs::metadata(&a)?.permissions())?;
                        io::copy(&mut fs::File::open(&a)?, &mut out)?;
                        out.sync_all()?;
                        if sha_file(&b)? != hash {
                            return Err(io::Error::new(io::ErrorKind::InvalidData, "The copied backup didn't match its original."));
                        }
                        Ok(())
                    })();
                    drop(out);
                    if let Err(e) = copied {
                        let _ = fs::remove_file(&b);
                        return Err(e);
                    }
                }
                Err(e) => return Err(e),
            }
            verified.push((a, b, hash));
        }
        if verified.len() > before {
            games.push(src);
        }
    }
    // A later conflict or write failure must not leave an earlier original missing. Check the
    // entire batch again before deleting anything, including sources a sync tool may have changed.
    for (a, b, hash) in &verified {
        if sha_file(a)? != *hash || sha_file(b)? != *hash {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "A backup changed during the move. The originals were kept; try again."));
        }
    }
    for (a, _, _) in verified {
        fs::remove_file(a)?;
    }
    for src in &games {
        let _ = fs::remove_dir(src);
    }
    Ok(games.len())
}

/// What the launcher remembers about a game's saves and its cloud copies.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct State {
    /// The set's checksum when the last copy was made.
    #[serde(default)]
    pub last_hash: String,
    /// The set's checksum this PC and the cloud last agreed on.
    #[serde(default)]
    pub synced_hash: String,
    /// A copy that still has to go up (the upload failed or the PC was offline).
    #[serde(default)]
    pub pending: Option<String>,
}

/// This PC's notes about a game's saves stay in the launcher's own folder, even with the copies
/// somewhere else: a folder other PCs keep in sync too would mix up theirs.
fn state_file(app_dir: &Path, id: &str) -> PathBuf {
    app_dir.join("saves").join(id).join("state.json")
}

pub fn state(app_dir: &Path, id: &str) -> State {
    let read = |p: PathBuf| fs::read(p).ok().and_then(|b| serde_json::from_slice(&b).ok());
    // One from before the copies moved, beside them.
    read(state_file(app_dir, id)).or_else(|| read(dir_of(app_dir, id).join("state.json"))).unwrap_or_default()
}

pub fn set_state(app_dir: &Path, id: &str, s: &State) {
    let f = state_file(app_dir, id);
    if let Some(d) = f.parent() {
        let _ = fs::create_dir_all(d);
    }
    let _ = fs::write(f, serde_json::to_vec_pretty(s).unwrap_or_default());
}

/// Copies made in the launcher's own folder while the player's folder couldn't be reached go to
/// it once it's back. This PC's notes stay.
pub fn gather(app_dir: &Path) {
    let Some(to) = chosen_root().filter(|p| p.is_dir()) else { return };
    let from = app_dir.join("saves");
    let has_copies = fs::read_dir(&from).into_iter().flatten().flatten().any(|g| fs::read_dir(g.path()).into_iter().flatten().flatten().any(|f| f.file_name() != "state.json"));
    if has_copies {
        if let Ok(n) = move_copies(&from, &to) {
            if n > 0 {
                crate::trail::note(&format!("Saves: copies made while the backup folder was away went to it ({n} games)"));
            }
        }
    }
}

fn read_manifest(zip_file: &Path) -> Option<Manifest> {
    let mut z = zip::ZipArchive::new(fs::File::open(zip_file).ok()?).ok()?;
    let mut e = z.by_name("rlsave.json").ok()?;
    let mut s = String::new();
    e.read_to_string(&mut s).ok()?;
    serde_json::from_str(&s).ok()
}

/// A game's copies on this PC, newest first.
pub fn list(app_dir: &Path, id: &str) -> Vec<Copy> {
    let mut out: Vec<Copy> = fs::read_dir(dir_of(app_dir, id))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let name = p.file_name()?.to_str()?.to_string();
            let when: u64 = name.strip_suffix(".zip")?.parse().ok()?;
            let m = read_manifest(&p)?;
            Some(Copy { name, when, size: e.metadata().ok()?.len(), files: m.files.len(), hash: m.hash, device: m.device, played: m.played })
        })
        .collect();
    out.sort_by(|a, b| b.when.cmp(&a.when));
    out
}

/// Writes a copy of the saves as they are now. None when there are none, or nothing changed since
/// the last copy.
pub fn backup(app_dir: &Path, id: &str, spots: &[Spot], g: &Installed) -> Result<Option<Copy>, Error> {
    // How big it is first: a place too big to be saves is left out before anything is read.
    if let Ok(mut o) = OVERSIZE.lock() {
        o.clear();
    }
    let pre = collect(spots, g);
    let big: Vec<String> = OVERSIZE.lock().map(|o| o.clone()).unwrap_or_default();
    if pre.is_empty() && !big.is_empty() {
        return Err(Error::Msg(format!("{}'s save folder is too big to be just saves ({}), so it wasn't backed up.", g.name, big.join(", "))));
    }
    if pre.is_empty() {
        return Ok(None);
    }
    let (entries, infos, hash) = current(spots, g);
    if entries.is_empty() {
        return Ok(None);
    }
    let mut st = state(app_dir, id);
    let all = list(app_dir, id);
    if st.last_hash == hash && all.first().map(|c| c.hash == hash).unwrap_or(false) {
        return Ok(None);
    }
    let d = dir_of(app_dir, id);
    fs::create_dir_all(&d)?;
    // Always after the newest, so the names keep their order.
    let when = crate::library::now().max(all.first().map(|c| c.when + 1).unwrap_or(0));
    let man = Manifest {
        format: 1,
        game: id.into(),
        made: when,
        device: device_name(),
        played: g.played_secs,
        hash: hash.clone(),
        spots: spots.iter().map(|s| s.template.clone()).collect(),
        files: infos.clone(),
    };
    let file = d.join(format!("{when}.zip"));
    let part = d.join(format!("{when}.part"));
    {
        let mut z = zip::ZipWriter::new(fs::File::create(&part)?);
        let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        z.start_file("rlsave.json", o).map_err(|e| Error::Msg(e.to_string()))?;
        z.write_all(&serde_json::to_vec_pretty(&man).unwrap_or_default())?;
        let shas: HashMap<(usize, &str), &str> = infos.iter().map(|f| ((f.spot, f.rel.as_str()), f.sha256.as_str())).collect();
        for e in &entries {
            if !shas.contains_key(&(e.spot, e.rel.as_str())) {
                continue;
            }
            z.start_file(format!("{}/{}", e.spot, e.rel), o).map_err(|er| Error::Msg(er.to_string()))?;
            io::copy(&mut fs::File::open(&e.abs)?, &mut z)?;
        }
        z.finish().map_err(|e| Error::Msg(e.to_string()))?;
    }
    fs::rename(&part, &file)?;
    let mut all = list(app_dir, id);
    while all.len() > KEEP {
        if let Some(old) = all.pop() {
            let _ = fs::remove_file(d.join(old.name));
        }
    }
    st.last_hash = hash.clone();
    set_state(app_dir, id, &st);
    let size = fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
    Ok(Some(Copy { name: format!("{when}.zip"), when, size, files: infos.len(), hash, device: man.device, played: man.played }))
}

/// Puts a copy's files back where the game reads them, on this PC. Saves that are there now and
/// aren't in the copy are taken away, so the game sees exactly what was copied; what was there is
/// kept as a copy first, so this can be undone.
pub fn restore(app_dir: &Path, id: &str, zip_file: &Path, spots_now: &[Spot], g: &Installed) -> Result<usize, Error> {
    let man = read_manifest(zip_file).ok_or_else(|| Error::Msg("That isn't a save copy.".into()))?;
    // A picked folder belongs to this PC: another PC may have a different user/profile ID in
    // its path. Copies already hold each folder's contents, so keep their indices and use this
    // PC's folders for both writing and clearing. Old copies need no format change.
    let same: Vec<Spot> = if !spots_now.is_empty() && spots_now.iter().all(|s| s.from == "player") {
        if man.spots.len() != spots_now.len() {
            return Err(Error::Msg(format!("This copy has {} save folders, but you picked {} on this PC. Pick the same number of folders in the same order as on the PC that made the copy, then try again.", man.spots.len(), spots_now.len())));
        }
        // A known folder at another index means these were reordered. Mapping by index would
        // swap saves; reordering the copy's indices would change its checksum for cloud sync.
        if man.spots.iter().enumerate().any(|(i, t)| spots_now.iter().enumerate().any(|(j, s)| i != j && s.template == *t)) {
            return Err(Error::Msg("These save folders are in a different order from the copy. Pick them in the same order as on the PC that made the copy, then try again.".into()));
        }
        spots_now.to_vec()
    } else {
        man.spots.iter().map(|t| Spot { template: t.clone(), from: "copy" }).collect()
    };
    // What's there now, kept first.
    // Kept means a copy of it exists: a new one now, or the newest one already matches (backup
    // makes no new copy then). Nothing found (or too much to copy) is not kept.
    let found_now = !current(spots_now, g).0.is_empty();
    let kept = match backup(app_dir, id, spots_now, g) {
        Ok(Some(_)) => true,
        Ok(None) => found_now,
        Err(_) => false,
    };
    let mut z = zip::ZipArchive::new(fs::File::open(zip_file)?).map_err(|e| Error::Msg(e.to_string()))?;
    let roots: Vec<PathBuf> = same.iter().map(|s| resolve_for(&s.template, g)).collect();
    let wanted: HashSet<(usize, String)> = man.files.iter().map(|f| (f.spot, f.rel.clone())).collect();
    // Saves in the same places that the copy doesn't have: only once what's there now is safely
    // kept, and never in the game's own folder as a whole, where other files live too.
    if kept {
        // Where extra saves can go: places the game's developer, the catalog or the player named
        // (never the game's whole folder), and places outside the game's folder. Not a place that
        // was only a guess inside the game's folder, or one that holds programs: those can be part
        // of the game (a runtime it downloaded).
        let named = |t: &str| spots_now.iter().any(|s| s.template == t && matches!(s.from, "developer" | "catalog" | "player"));
        let may_clear: Vec<bool> = same
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let t = &s.template;
                let whole = t.trim_end_matches(['/', '\\']) == "{install}";
                let in_game = t.starts_with("{install}");
                !whole && (!in_game || named(t)) && !roots.get(i).map(|r| holds_programs(r)).unwrap_or(true)
            })
            .collect();
        for e in collect(&same, g) {
            let ok = may_clear.get(e.spot).copied().unwrap_or(false);
            if ok && !wanted.contains(&(e.spot, e.rel.clone())) {
                let _ = fs::remove_file(&e.abs);
            }
        }
    }
    let today: HashSet<&str> = spots_now.iter().map(|s| s.template.as_str()).collect();
    let skip: HashSet<usize> = same.iter().enumerate().filter(|(i, s)| s.template.starts_with("{install}/") && !today.contains(s.template.as_str()) && roots.get(*i).map(|r| holds_programs(r)).unwrap_or(false)).map(|(i, _)| i).collect();
    let mut n = 0;
    for i in 0..z.len() {
        let mut e = z.by_index(i).map_err(|e| Error::Msg(e.to_string()))?;
        if e.is_dir() || e.name() == "rlsave.json" {
            continue;
        }
        let name = e.name().to_string();
        let Some((spot, rel)) = name.split_once('/') else { continue };
        let Ok(spot) = spot.parse::<usize>() else { continue };
        let Some(root) = roots.get(spot) else { continue };
        // A place in the game's folder that holds programs and isn't one of its save places
        // anymore (a runtime an older version mistook for saves): left as it is.
        if skip.contains(&spot) {
            continue;
        }
        // Only plain names: nothing that climbs out, or that Windows reads as a drive or a root.
        if rel.split('/').any(|c| c == ".." || c.is_empty() || c.contains('\\') || c.contains(':')) {
            continue;
        }
        // A file place (`{install}/game.sav`) is the file itself; a pattern's files sit beside it.
        let t = &same[spot].template;
        let last = Path::new(t).file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
        let dest = if same[spot].from == "player" {
            // The picker always selects a folder, even if it doesn't exist on this PC yet.
            root.join(rel)
        } else if last.contains('*') {
            root.parent().unwrap_or(root).join(rel)
        } else if rel == last && !root.is_dir() {
            root.clone()
        } else {
            root.join(rel)
        };
        if let Some(d) = dest.parent() {
            fs::create_dir_all(d)?;
        }
        if crate::inputs::excluded(g, &dest) { continue; }
        let tmp = dest.with_extension("rl-part");
        io::copy(&mut e, &mut fs::File::create(&tmp)?)?;
        fs::rename(&tmp, &dest)?;
        n += 1;
    }
    let mut st = state(app_dir, id);
    st.last_hash = man.hash.clone();
    // A successful restore replaces the save that was queued for upload. Keep its archive in
    // local history, but do not let a retry send it over the player's restored selection.
    st.pending = None;
    set_state(app_dir, id, &st);
    Ok(n)
}

pub fn manifest(zip_file: &Path) -> Option<Manifest> {
    read_manifest(zip_file)
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct ShareFile {
    path: PathBuf,
    hash: String,
    pub modified: Option<std::time::SystemTime>,
}

impl ShareFile {
    pub fn when(&self) -> Option<u64> {
        self.modified.map(|t| t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs())
    }
}

/// shortcut: copy one file from a known save location, add format-specific support for multi-file saves.
pub fn share_file(spot: &Spot, g: &Installed, peer: Option<&ShareFile>) -> Option<ShareFile> {
    if !matches!(spot.from, "developer" | "catalog" | "player") {
        return None;
    }
    let place = resolve_for(&spot.template, g);
    let folder = spot.from == "player" || place.is_dir();
    if place.file_name()?.to_str()?.contains('*') || place == g.path || (folder && holds_programs(&place)) {
        return None;
    }
    let entries = collect(std::slice::from_ref(spot), g);
    let (path, modified) = match entries.as_slice() {
        [entry] => (entry.abs.clone(), Some(fs::metadata(&entry.abs).ok()?.modified().ok()?)),
        [] => {
            let peer = peer?;
            if !folder && place.file_name() != peer.path.file_name() {
                return None;
            }
            let path = if folder { place.join(peer.path.file_name()?) } else { place };
            if !matches!(fs::symlink_metadata(&path), Err(e) if e.kind() == io::ErrorKind::NotFound) {
                return None;
            }
            (path, None)
        }
        _ => return None,
    };
    let name = path.file_name()?.to_str()?;
    let parent = path.parent()?.canonicalize().ok()?;
    if folder && (parent != resolve_for(&spot.template, g).canonicalize().ok()?
        || peer.is_some_and(|p| p.path.file_name() != path.file_name())) {
        return None;
    }
    if crate::inputs::excluded(g, &path) || !is_save_file(name, 0)
        || g.files.iter().any(|f| lower(&g.path.join(f)) == lower(&path)) {
        return None;
    }
    let hash = if modified.is_some() {
        if !fs::symlink_metadata(&path).ok()?.is_file() {
            return None;
        }
        sha_file(&path).ok()?
    } else {
        String::new()
    };
    Some(ShareFile { path: parent.join(name), hash, modified })
}

pub fn newer_save(source: &ShareFile, target: &ShareFile) -> bool {
    lower(&source.path) != lower(&target.path) && source.hash != target.hash && source.modified > target.modified
}

pub fn share_save(app_dir: &Path, id: &str, spot: &Spot, g: &Installed, source: &ShareFile, target: &ShareFile) -> Result<(), Error> {
    let changed = || Error::Msg("The saves changed. Check them again before copying.".into());
    if !newer_save(source, target) || share_file(spot, g, Some(source)).as_ref() != Some(target) {
        return Err(changed());
    }
    // Check the contents of the recovery copy before replacing the live save.
    if target.modified.is_some() {
        backup(app_dir, id, std::slice::from_ref(spot), g)?;
        let saved = list(app_dir, id).into_iter().next().ok_or_else(changed)?;
        let mut zip = zip::ZipArchive::new(fs::File::open(dir_of(app_dir, id).join(saved.name))?)
            .map_err(|e| Error::Msg(e.to_string()))?;
        let entry = format!("0/{}", target.path.file_name().unwrap().to_string_lossy());
        let mut copy = Vec::new();
        zip.by_name(&entry).map_err(|e| Error::Msg(e.to_string()))?.take(MAX_FILE + 1).read_to_end(&mut copy)?;
        if copy.len() as u64 > MAX_FILE || hex::encode(Sha256::digest(&copy)) != target.hash {
            return Err(Error::Msg("The existing save could not be backed up. It was kept in place.".into()));
        }
    }
    let mut bytes = Vec::new();
    fs::File::open(&source.path)?.take(MAX_FILE + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE || hex::encode(Sha256::digest(&bytes)) != source.hash {
        return Err(changed());
    }
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temp = target.path.with_file_name(format!(".recomplauncher-save-{}-{n}.tmp", std::process::id()));
    let mut file = fs::File::create_new(&temp)?;
    let staged = (|| -> Result<(), Error> {
        file.write_all(&bytes)?;
        file.set_modified(source.modified.ok_or_else(changed)?)?;
        file.sync_all()?;
        drop(file);
        if share_file(spot, g, Some(source)).as_ref() != Some(target) {
            return Err(changed());
        }
        fs::rename(&temp, &target.path)?;
        Ok(())
    })();
    if staged.is_err() {
        let _ = fs::remove_file(temp);
    }
    staged?;
    let mut s = state(app_dir, id);
    s.pending = None;
    set_state(app_dir, id, &s);
    Ok(())
}

// ---------- noticing where a game saves ----------

/// The files in the likely save places when a game started.
pub struct Watch {
    roots: Vec<PathBuf>,
    install: PathBuf,
    before: HashMap<PathBuf, (u64, u64)>,
    release: HashSet<String>,
    game: Installed,
}

const NOT_NAMES: [&str; 14] = ["recomp", "recompiled", "edition", "game", "games", "the", "port", "version", "windows", "release", "remastered", "launcher", "native", "decomp"];

fn tokens(names: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for n in names {
        let joined: String = n.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
        if joined.len() >= 4 {
            out.push(joined);
        }
        for w in n.split(|c: char| !c.is_alphanumeric()) {
            let w = w.to_lowercase();
            if w.len() >= 4 && !NOT_NAMES.contains(&w.as_str()) {
                out.push(w);
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn mtime(m: &fs::Metadata) -> u64 {
    m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn snap(root: &Path, into: &mut HashMap<PathBuf, (u64, u64)>, budget: &mut usize) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).into_iter().flatten().flatten() {
            if *budget == 0 {
                return;
            }
            let Ok(m) = e.metadata() else { continue };
            if m.is_dir() {
                if !skip_dir(&e.file_name().to_string_lossy()) {
                    stack.push(e.path());
                }
            } else {
                *budget -= 1;
                into.insert(e.path(), (mtime(&m), m.len()));
            }
        }
    }
}

/// Before the game starts: the folders it might save in (its own, and ones named like it in
/// Documents, Saved Games and AppData), and what's in them.
pub fn watch_start(g: &Installed, names: &[&str]) -> Watch {
    let toks = tokens(names);
    // A Windows game on Linux saves in the prefix's Windows folders.
    let win = windows_user(g);
    let home = win.clone().or_else(dirs::home_dir);
    let docs = if win.is_some() { home.as_ref().map(|h| h.join("Documents")) } else { dirs::document_dir().or_else(|| home.as_ref().map(|h| h.join("Documents"))) };
    let (config, local) = match &win {
        Some(u) => (Some(u.join("AppData/Roaming")), Some(u.join("AppData/Local"))),
        None => (dirs::config_dir(), dirs::data_local_dir()),
    };
    let mut bases: Vec<PathBuf> = vec![];
    for b in [
        docs.clone(),
        docs.as_ref().map(|d| d.join("My Games")),
        home.as_ref().map(|h| h.join("Saved Games")),
        config,
        local,
        home.as_ref().map(|h| h.join("AppData").join("LocalLow")),
        home.as_ref().map(|h| h.join(".local/share")),
        home.as_ref().map(|h| h.join(".config")),
    ]
    .into_iter()
    .flatten()
    {
        if !bases.contains(&b) {
            bases.push(b);
        }
    }
    let mut roots = vec![g.path.clone()];
    for b in &bases {
        for e in fs::read_dir(b).into_iter().flatten().flatten() {
            if !e.metadata().map(|m| m.is_dir()).unwrap_or(false) {
                continue;
            }
            let n: String = e.file_name().to_string_lossy().chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
            if n.len() >= 4 && n != "recomplauncher" && toks.iter().any(|t| n.contains(t.as_str()) || (n.len() >= 6 && t.contains(n.as_str()))) {
                roots.push(e.path());
            }
        }
    }
    let mut before = HashMap::new();
    let mut budget = MAX_FILES;
    for r in &roots {
        snap(r, &mut before, &mut budget);
    }
    let release = g.files.iter().map(|f| f.to_string_lossy().replace('\\', "/").to_lowercase()).collect();
    Watch { roots, install: g.path.clone(), before, release, game: g.clone() }
}

/// After it closed: the places where it wrote something that looks like a save, as templates.
pub fn watch_end(w: Watch) -> Vec<String> {
    let mut after = HashMap::new();
    let mut budget = MAX_FILES;
    for r in &w.roots {
        snap(r, &mut after, &mut budget);
    }
    let mut out: Vec<String> = vec![];
    for (p, (t, size)) in &after {
        if w.before.get(p) == Some(&(*t, *size)) {
            continue;
        }
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if !is_save_file(&name, *size) || name.to_lowercase().contains("crash") {
            continue;
        }
        let Some(root) = w.roots.iter().filter(|r| p.starts_with(r)).max_by_key(|r| r.as_os_str().len()) else { continue };
        let t = if *root == w.install {
            let Ok(rel) = p.strip_prefix(&w.install) else { continue };
            let rel_s = rel.to_string_lossy().replace('\\', "/");
            if w.release.contains(&rel_s.to_lowercase()) {
                continue;
            }
            match rel_s.split_once('/') {
                // Not a folder of programs: a runtime the game fetched (Java, Python) writes small
                // files too.
                Some((first, _)) if holds_programs(&w.install.join(first)) => continue,
                Some((first, _)) => format!("{{install}}/{first}"),
                None => format!("{{install}}/{rel_s}"),
            }
        } else if holds_programs(root) {
            continue;
        } else {
            templatize_for(root, &w.game)
        };
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out.sort();
    out
}

/// The size of each game's copies on this PC.
pub fn sizes(app_dir: &Path) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    for e in fs::read_dir(root(app_dir)).into_iter().flatten().flatten() {
        let id = e.file_name().to_string_lossy().to_string();
        out.insert(id.clone(), list(app_dir, &id).iter().map(|c| c.size).sum());
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn several_save_folders_the_player_picked() {
        let d = std::env::temp_dir().join(format!("rl-saves-many-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        for sub in ["TT-Xbox/saves", "ROTK-Xbox/saves"] {
            fs::create_dir_all(d.join(sub)).unwrap();
        }
        fs::write(d.join("TT-Xbox/saves/slot1.sav"), b"tt").unwrap();
        fs::write(d.join("ROTK-Xbox/saves/slot1.sav"), b"rotk").unwrap();
        let mut g = game(&d);
        let one = add_place(&[], &d.join("TT-Xbox/saves"), &g).unwrap();
        let two = add_place(&one, &d.join("ROTK-Xbox/saves"), &g).unwrap();
        assert_eq!(two, vec!["{install}/TT-Xbox/saves".to_string(), "{install}/ROTK-Xbox/saves".to_string()]);
        assert!(add_place(&two, &d.join("TT-Xbox/saves"), &g).is_err());
        assert!(add_place(&two, &d.join("TT-Xbox"), &g).is_err());
        g.save_place = Some(two[0].clone());
        g.more_save_places = vec![two[1].clone()];
        let s = spots(&["{install}/dev".into()], &[], &g);
        assert_eq!(s.iter().map(|x| (x.template.as_str(), x.from)).collect::<Vec<_>>(), vec![("{install}/TT-Xbox/saves", "player"), ("{install}/ROTK-Xbox/saves", "player")]);
        // Both folders' files go in the backup, kept apart.
        let mut from: Vec<usize> = collect(&s, &g).iter().map(|e| e.spot).collect();
        from.sort();
        assert_eq!(from, vec![0, 1]);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_downloaded_runtime_is_never_a_save_place() {
        use crate::library::Installed;
        use std::path::PathBuf;
        let d = std::env::temp_dir().join("rl-watch-jdk");
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("launch.bat"), "x").unwrap();
        let g = Installed { id: "sc".into(), name: "Severed Chains".into(), path: d.clone(), files: vec![PathBuf::from("launch.bat")], ..Default::default() };
        let w = super::watch_start(&g, &["Severed Chains"]);
        // The first start: Java arrives, and the game saves.
        std::fs::create_dir_all(d.join("jdk25/lib")).unwrap();
        std::fs::create_dir_all(d.join("jdk25/bin")).unwrap();
        std::fs::write(d.join("jdk25/lib/jvm.cfg"), "-server KNOWN").unwrap();
        std::fs::write(d.join("jdk25/bin/java.dll"), "MZ").unwrap();
        std::fs::create_dir_all(d.join("saves")).unwrap();
        std::fs::write(d.join("saves/slot1.dat"), "save").unwrap();
        let found = super::watch_end(w);
        assert_eq!(found, vec!["{install}/saves".to_string()]);
        // One an older version recorded is dropped.
        let g2 = Installed { found_saves: vec!["{install}/jdk25".into(), "{install}/saves".into()], ..g.clone() };
        let spots = super::spots(&[], &[], &g2);
        assert_eq!(spots.iter().map(|s| s.template.as_str()).collect::<Vec<_>>(), vec!["{install}/saves"]);
        let _ = std::fs::remove_dir_all(&d);
    }


    #[test]
    fn small_bin_files_are_saves() {
        assert!(super::is_save_file("SaveData.bin", 300_000));
        assert!(super::is_save_file("profile.BIN", 64));
        assert!(!super::is_save_file("Track 01.bin", 400 * 1024 * 1024));
        assert!(!super::is_save_file("game.iso", 10));
    }

    use super::*;

    fn tmp(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rl-saves-{n}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn game(dir: &Path) -> Installed {
        Installed { id: "g".into(), name: "Test Game".into(), path: dir.to_path_buf(), files: vec![PathBuf::from("game.exe"), PathBuf::from("data/a.bin")], played_secs: 60, ..Default::default() }
    }

    #[test]
    fn sharing_keeps_the_previous_save_and_the_source() {
        let d = tmp("sharing-copy");
        let source = game(&d.join("source"));
        let target = game(&d.join("target"));
        let spot = Spot { template: "{install}/saves".into(), from: "player" };
        for (g, data, seconds) in [(&source, b"new", 2000), (&target, b"old", 1000)] {
            fs::create_dir_all(g.path.join("saves")).unwrap();
            let mut file = fs::File::create(g.path.join("saves/slot.sav")).unwrap();
            file.write_all(data).unwrap();
            file.set_modified(UNIX_EPOCH + std::time::Duration::from_secs(seconds)).unwrap();
        }
        let from = share_file(&spot, &source, None).unwrap();
        let to = share_file(&spot, &target, Some(&from)).unwrap();
        share_save(&d, "target", &spot, &target, &from, &to).unwrap();
        assert_eq!(fs::read(&from.path).unwrap(), b"new");
        assert_eq!(fs::read(&to.path).unwrap(), b"new");
        assert_eq!(share_file(&spot, &target, Some(&from)).unwrap().modified, from.modified);
        let copy = dir_of(&d, "target").join(&list(&d, "target")[0].name);
        restore(&d, "target", &copy, &[spot], &target).unwrap();
        assert_eq!(fs::read(&to.path).unwrap(), b"old");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn sharing_rejects_a_save_changed_since_the_prompt() {
        let d = tmp("sharing-stale");
        let source = game(&d);
        let target = game(&d);
        let from_spot = Spot { template: "{install}/from.sav".into(), from: "developer" };
        let to_spot = Spot { template: "{install}/to.sav".into(), from: "developer" };
        fs::write(d.join("from.sav"), b"new").unwrap();
        fs::write(d.join("to.sav"), b"old").unwrap();
        fs::File::options().write(true).open(d.join("to.sav")).unwrap().set_modified(UNIX_EPOCH).unwrap();
        let from = share_file(&from_spot, &source, None).unwrap();
        let to = share_file(&to_spot, &target, Some(&from)).unwrap();
        fs::write(&from.path, b"changed").unwrap();
        assert!(share_save(&d, "target", &to_spot, &target, &from, &to).is_err());
        assert_eq!(fs::read(&to.path).unwrap(), b"old");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn wildcards() {
        assert!(wild("*.sav", "slot1.SAV"));
        assert!(wild("slot*.sav", "slot1.sav"));
        assert!(!wild("slot*.sav", "x1.sav"));
        assert!(wild("a*b*c", "aXXbYYc"));
    }

    #[test]
    fn game_folder_keeps_only_the_players_files() {
        let d = tmp("folder");
        fs::create_dir_all(d.join("data")).unwrap();
        fs::create_dir_all(d.join("save")).unwrap();
        fs::create_dir_all(d.join("mods")).unwrap();
        fs::write(d.join("game.exe"), "x").unwrap();
        fs::write(d.join("data/a.bin"), "x").unwrap();
        fs::write(d.join("save/slot1.sav"), "one").unwrap();
        fs::write(d.join("settings.toml"), "a=1").unwrap();
        fs::write(d.join("mods/big.pak"), "m").unwrap();
        fs::write(d.join("game.iso"), "disc").unwrap();
        fs::write(d.join("run.log"), "log").unwrap();
        let g = game(&d);
        let s = spots(&[], &[], &g);
        assert_eq!(s[0].from, "game");
        let rels: Vec<String> = collect(&s, &g).into_iter().map(|e| e.rel).collect();
        assert_eq!(rels, vec!["save/slot1.sav", "settings.toml"]);
    }

    #[test]
    fn copies_skip_unchanged_and_restore_exactly() {
        let d = tmp("copy");
        let app = d.join("app");
        let inst = d.join("inst");
        fs::create_dir_all(inst.join("save")).unwrap();
        fs::write(inst.join("save/slot1.sav"), "one").unwrap();
        let g = game(&inst);
        let s = vec![Spot { template: "{install}/save".into(), from: "developer" }];
        let a = backup(&app, "g", &s, &g).unwrap().unwrap();
        assert_eq!(a.files, 1);
        assert!(backup(&app, "g", &s, &g).unwrap().is_none(), "nothing changed: no new copy");
        fs::write(inst.join("save/slot1.sav"), "two").unwrap();
        fs::write(inst.join("save/slot2.sav"), "new").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let b = backup(&app, "g", &s, &g).unwrap().unwrap();
        assert_ne!(a.hash, b.hash);
        assert_eq!(list(&app, "g").len(), 2);
        // Back to the first: slot2 goes, slot1 is "one" again, and what was there is kept.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        restore(&app, "g", &dir_of(&app, "g").join(&a.name), &s, &g).unwrap();
        assert_eq!(fs::read_to_string(inst.join("save/slot1.sav")).unwrap(), "one");
        assert!(!inst.join("save/slot2.sav").exists());
        let (_, _, now) = current(&s, &g);
        assert_eq!(now, a.hash);
    }

    #[test]
    fn restoring_a_save_cancels_its_superseded_upload_but_keeps_history() {
        let d = tmp("restore-pending");
        let app = d.join("app");
        let inst = d.join("inst");
        fs::create_dir_all(inst.join("save")).unwrap();
        let path = inst.join("save/slot1.sav");
        let g = game(&inst);
        let spots = vec![Spot { template: "{install}/save".into(), from: "developer" }];
        fs::write(&path, "chosen save").unwrap();
        let chosen = backup(&app, "g", &spots, &g).unwrap().unwrap();
        fs::write(&path, "newer local save waiting to upload").unwrap();
        let queued = backup(&app, "g", &spots, &g).unwrap().unwrap();
        assert!(queued.when > chosen.when);
        let queued_path = dir_of(&app, "g").join(&queued.name);
        let queued_bytes = fs::read(&queued_path).unwrap();
        set_state(&app, "g", &State { last_hash: queued.hash.clone(), synced_hash: "previous cloud".into(), pending: Some(queued.name.clone()) });
        set_state(&app, "other", &State { pending: Some("123.zip".into()), ..Default::default() });

        restore(&app, "g", &dir_of(&app, "g").join(&chosen.name), &spots, &g).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "chosen save");
        let state = state(&app, "g");
        assert!(state.pending.is_none(), "the superseded local archive must not upload after a restore");
        assert_eq!(state.last_hash, chosen.hash);
        assert_eq!(state.synced_hash, "previous cloud", "a local restore does not claim a cloud upload");
        assert_eq!(super::state(&app, "other").pending.as_deref(), Some("123.zip"));
        assert_eq!(fs::read(&queued_path).unwrap(), queued_bytes, "the superseded save remains in local history");
        // A later session can still back up and upload the restored selection normally.
        let next = backup(&app, "g", &spots, &g).unwrap().unwrap();
        assert_eq!(next.hash, chosen.hash);
        assert!(next.when > queued.when);
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_failed_restore_keeps_the_pending_upload() {
        let d = tmp("restore-pending-error");
        let app = d.join("app");
        let inst = d.join("inst");
        fs::create_dir_all(&inst).unwrap();
        fs::write(inst.join("slot.sav"), "local save").unwrap();
        let g = game(&inst);
        let spots = vec![Spot { template: "{install}/slot.sav".into(), from: "developer" }];
        let queued = backup(&app, "g", &spots, &g).unwrap().unwrap();
        set_state(&app, "g", &State { last_hash: queued.hash, pending: Some(queued.name.clone()), ..Default::default() });
        let invalid = d.join("broken.zip");
        fs::write(&invalid, b"not a save archive").unwrap();
        assert!(restore(&app, "g", &invalid, &spots, &g).is_err());
        assert_eq!(state(&app, "g").pending.as_deref(), Some(queued.name.as_str()));
        // Even a valid archive can fail after restore has begun (the destination is unwritable).
        fs::create_dir(inst.join("slot.rl-part")).unwrap();
        assert!(restore(&app, "g", &dir_of(&app, "g").join(&queued.name), &spots, &g).is_err());
        assert_eq!(state(&app, "g").pending.as_deref(), Some(queued.name.as_str()));
        assert_eq!(fs::read_to_string(inst.join("slot.sav")).unwrap(), "local save");
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn moving_copies_preserves_conflicting_backups() {
        for (suffix, previous) in [("same-size", b"other save".as_slice()), ("different-size", b"older save with more bytes".as_slice())] {
            let d = tmp(&format!("move-conflict-{suffix}"));
            let (from, to) = (d.join("from"), d.join("to"));
            fs::create_dir_all(from.join("g")).unwrap();
            fs::create_dir_all(to.join("g")).unwrap();
            fs::write(from.join("g/123.zip"), b"first save").unwrap();
            fs::write(from.join("g/124.zip"), b"another backup").unwrap();
            fs::write(to.join("g/123.zip"), previous).unwrap();
            assert!(move_copies(&from, &to).is_err(), "a different backup must not be replaced or mistaken for a duplicate");
            assert_eq!(fs::read(from.join("g/123.zip")).unwrap(), b"first save");
            assert_eq!(fs::read(from.join("g/124.zip")).unwrap(), b"another backup");
            assert_eq!(fs::read(to.join("g/123.zip")).unwrap(), previous);
            fs::remove_dir_all(d).unwrap();
        }
    }

    #[test]
    fn moving_copies_reuses_identical_backups_and_keeps_local_state() {
        let d = tmp("move-identical");
        let (from, to) = (d.join("from"), d.join("to"));
        fs::create_dir_all(from.join("g")).unwrap();
        fs::create_dir_all(to.join("g")).unwrap();
        fs::write(from.join("g/123.zip"), b"same backup").unwrap();
        fs::write(to.join("g/123.zip"), b"same backup").unwrap();
        fs::write(from.join("g/124.zip"), b"new backup").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(from.join("g/124.zip"), fs::Permissions::from_mode(0o600)).unwrap();
        }
        fs::write(from.join("g/state.json"), b"local notes").unwrap();
        fs::write(from.join("g/125.part"), b"still being written").unwrap();
        assert_eq!(move_copies(&from, &to).unwrap(), 1);
        assert!(!from.join("g/123.zip").exists());
        assert!(!from.join("g/124.zip").exists());
        assert_eq!(fs::read(to.join("g/123.zip")).unwrap(), b"same backup");
        assert_eq!(fs::read(to.join("g/124.zip")).unwrap(), b"new backup");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(to.join("g/124.zip")).unwrap().permissions().mode() & 0o777, 0o600);
        }
        assert_eq!(fs::read(from.join("g/state.json")).unwrap(), b"local notes");
        assert_eq!(fs::read(from.join("g/125.part")).unwrap(), b"still being written");
        assert!(!to.join("g/state.json").exists());
        assert!(!to.join("g/125.part").exists());
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn moving_copies_keeps_sources_when_the_destination_fails() {
        let d = tmp("move-failed");
        let (from, to) = (d.join("from"), d.join("to"));
        fs::create_dir_all(from.join("g")).unwrap();
        fs::create_dir_all(to.join("g/124.zip")).unwrap();
        fs::write(from.join("g/123.zip"), b"first backup").unwrap();
        fs::write(from.join("g/124.zip"), b"second backup").unwrap();
        assert!(move_copies(&from, &to).is_err());
        assert_eq!(fs::read(from.join("g/123.zip")).unwrap(), b"first backup");
        assert_eq!(fs::read(from.join("g/124.zip")).unwrap(), b"second backup");
        assert!(to.join("g/124.zip").is_dir());
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn moving_copies_into_the_same_folder_keeps_every_backup() {
        let d = tmp("move-same");
        fs::create_dir_all(d.join("g")).unwrap();
        fs::write(d.join("g/123.zip"), b"our only backup").unwrap();
        assert_eq!(move_copies(&d, &d.join(".")).unwrap(), 0);
        assert_eq!(fs::read(d.join("g/123.zip")).unwrap(), b"our only backup");
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn moving_copies_rejects_a_nested_destination() {
        let d = tmp("move-nested");
        fs::create_dir_all(d.join("g")).unwrap();
        fs::write(d.join("g/123.zip"), b"original backup").unwrap();
        assert!(move_copies(&d, &d.join("nested")).is_err());
        assert_eq!(fs::read(d.join("g/123.zip")).unwrap(), b"original backup");
        assert!(!d.join("nested/g/123.zip").exists());
        fs::remove_dir_all(d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn moving_copies_rejects_a_destination_game_folder_link() {
        let d = tmp("move-linked-game");
        let (from, to) = (d.join("from"), d.join("to"));
        fs::create_dir_all(from.join("g")).unwrap();
        fs::create_dir_all(&to).unwrap();
        fs::write(from.join("g/123.zip"), b"original backup").unwrap();
        std::os::unix::fs::symlink(from.join("g"), to.join("g")).unwrap();
        assert!(move_copies(&from, &to).is_err());
        assert_eq!(fs::read(from.join("g/123.zip")).unwrap(), b"original backup");
        assert!(fs::symlink_metadata(to.join("g")).unwrap().file_type().is_symlink());
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn restore_uses_this_devices_selected_folder_and_keeps_previous_saves() {
        let d = tmp("restore-selected");
        let source = d.join("source");
        let target = d.join("target");
        let source_save = source.join("profile/save/source-id");
        let target_save = target.join("profile/save/target-id");
        fs::create_dir_all(source_save.join("slot1")).unwrap();
        fs::create_dir_all(&target_save).unwrap();
        fs::write(source_save.join("slot1/progress.sav"), "cloud progress").unwrap();
        fs::write(target_save.join("previous.sav"), "local progress").unwrap();
        let mut from = game(&source);
        from.save_place = Some("{install}/profile/save/source-id".into());
        let mut to = game(&target);
        to.save_place = Some("{install}/profile/save/target-id".into());
        let source_spots = spots(&[], &[], &from);
        let target_spots = spots(&[], &[], &to);
        let copy = backup(&d.join("source-app"), "g", &source_spots, &from).unwrap().unwrap();
        let archive = dir_of(&d.join("source-app"), "g").join(copy.name);
        let target_app = d.join("target-app");

        assert_eq!(restore(&target_app, "g", &archive, &target_spots, &to).unwrap(), 1);
        assert_eq!(fs::read_to_string(target_save.join("slot1/progress.sav")).unwrap(), "cloud progress");
        assert!(!target.join("profile/save/source-id").exists());
        assert!(!target_save.join("previous.sav").exists());
        assert_eq!(current(&target_spots, &to).2, copy.hash);
        let kept = list(&target_app, "g");
        assert_eq!(kept.len(), 1);
        let mut kept_zip = zip::ZipArchive::new(fs::File::open(dir_of(&target_app, "g").join(&kept[0].name)).unwrap()).unwrap();
        let mut previous = String::new();
        kept_zip.by_name("0/previous.sav").unwrap().read_to_string(&mut previous).unwrap();
        assert_eq!(previous, "local progress");
    }

    #[test]
    fn restore_selected_folder_is_a_directory_even_when_missing() {
        let d = tmp("restore-missing-selected");
        let source = d.join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("new-folder"), "progress").unwrap();
        // Older copies may contain an absolute path from a different PC.
        let source_spots = vec![Spot { template: source.to_string_lossy().into(), from: "player" }];
        let from = game(&d.join("source-install"));
        let copy = backup(&d.join("source-app"), "g", &source_spots, &from).unwrap().unwrap();
        fs::write(source.join("new-folder"), "source is left alone").unwrap();
        let to = game(&d.join("target"));
        let target_spots = vec![Spot { template: "{install}/new-folder".into(), from: "player" }];
        let archive = dir_of(&d.join("source-app"), "g").join(copy.name);

        assert_eq!(restore(&d.join("target-app"), "g", &archive, &target_spots, &to).unwrap(), 1);
        assert_eq!(fs::read_to_string(to.path.join("new-folder/new-folder")).unwrap(), "progress");
        assert_eq!(fs::read_to_string(source.join("new-folder")).unwrap(), "source is left alone");
    }

    #[test]
    fn restore_does_not_guess_when_selected_folder_counts_differ() {
        let d = tmp("restore-count");
        let g = game(&d.join("game"));
        fs::create_dir_all(g.path.join("one")).unwrap();
        fs::create_dir_all(g.path.join("two")).unwrap();
        fs::write(g.path.join("one/progress.sav"), "one").unwrap();
        fs::write(g.path.join("two/progress.sav"), "two").unwrap();
        let source_spots = vec![Spot { template: "{install}/one".into(), from: "player" }, Spot { template: "{install}/two".into(), from: "player" }];
        let copy = backup(&d.join("app"), "g", &source_spots, &g).unwrap().unwrap();
        fs::write(g.path.join("one/progress.sav"), "new one").unwrap();
        fs::write(g.path.join("two/progress.sav"), "new two").unwrap();
        let archive = dir_of(&d.join("app"), "g").join(copy.name);

        assert!(restore(&d.join("app"), "g", &archive, &source_spots[..1], &g).is_err());
        assert_eq!(fs::read_to_string(g.path.join("one/progress.sav")).unwrap(), "new one");
        assert_eq!(fs::read_to_string(g.path.join("two/progress.sav")).unwrap(), "new two");
    }

    #[test]
    fn restore_rejects_reordered_selected_folders_before_changing_saves() {
        let d = tmp("restore-order");
        let g = game(&d.join("game"));
        fs::create_dir_all(g.path.join("one")).unwrap();
        fs::create_dir_all(g.path.join("two")).unwrap();
        fs::write(g.path.join("one/progress.sav"), "one").unwrap();
        fs::write(g.path.join("two/progress.sav"), "two").unwrap();
        let source_spots = vec![Spot { template: "{install}/one".into(), from: "player" }, Spot { template: "{install}/two".into(), from: "player" }];
        let copy = backup(&d.join("app"), "g", &source_spots, &g).unwrap().unwrap();
        fs::write(g.path.join("one/progress.sav"), "new one").unwrap();
        fs::write(g.path.join("two/progress.sav"), "new two").unwrap();
        let archive = dir_of(&d.join("app"), "g").join(copy.name);
        let target_spots = vec![source_spots[1].clone(), source_spots[0].clone()];

        assert!(restore(&d.join("app"), "g", &archive, &target_spots, &g).is_err());
        assert_eq!(fs::read_to_string(g.path.join("one/progress.sav")).unwrap(), "new one");
        assert_eq!(fs::read_to_string(g.path.join("two/progress.sav")).unwrap(), "new two");
        assert_eq!(list(&d.join("app"), "g").len(), 1);
    }

    #[test]
    fn restore_maps_several_selected_folders_in_order() {
        let d = tmp("restore-several");
        let from = game(&d.join("source"));
        let to = game(&d.join("target"));
        fs::create_dir_all(from.path.join("one")).unwrap();
        fs::create_dir_all(from.path.join("two")).unwrap();
        fs::write(from.path.join("one/progress.sav"), "first game").unwrap();
        fs::write(from.path.join("two/progress.sav"), "second game").unwrap();
        let source_spots = vec![Spot { template: "{install}/one".into(), from: "player" }, Spot { template: "{install}/two".into(), from: "player" }];
        let target_spots = vec![Spot { template: "{install}/first".into(), from: "player" }, Spot { template: "{install}/second".into(), from: "player" }];
        let copy = backup(&d.join("source-app"), "g", &source_spots, &from).unwrap().unwrap();
        let archive = dir_of(&d.join("source-app"), "g").join(copy.name);

        assert_eq!(restore(&d.join("target-app"), "g", &archive, &target_spots, &to).unwrap(), 2);
        assert_eq!(fs::read_to_string(to.path.join("first/progress.sav")).unwrap(), "first game");
        assert_eq!(fs::read_to_string(to.path.join("second/progress.sav")).unwrap(), "second game");
        assert_eq!(current(&target_spots, &to).2, copy.hash);
        assert!(!to.path.join("one").exists());
        assert!(!to.path.join("two").exists());
    }

    #[test]
    fn keeps_the_newest_ten() {
        let d = tmp("ten");
        let app = d.join("app");
        let inst = d.join("inst");
        fs::create_dir_all(&inst).unwrap();
        let g = game(&inst);
        let s = vec![Spot { template: "{install}/s.sav".into(), from: "developer" }];
        for i in 0..13 {
            fs::write(inst.join("s.sav"), format!("v{i}")).unwrap();
            backup(&app, "g", &s, &g).unwrap();
        }
        assert_eq!(list(&app, "g").len(), KEEP);
        restore(&app, "g", &dir_of(&app, "g").join(&list(&app, "g").last().unwrap().name), &s, &g).unwrap();
        assert_eq!(fs::read_to_string(inst.join("s.sav")).unwrap(), "v3");
    }

    #[test]
    fn notices_where_it_saves() {
        let d = tmp("watch");
        let inst = d.join("inst");
        fs::create_dir_all(inst.join("saves")).unwrap();
        fs::write(inst.join("game.exe"), "x").unwrap();
        let g = game(&inst);
        let w = watch_start(&g, &["Test Game"]);
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(inst.join("saves/file1.dat"), "s").unwrap();
        fs::write(inst.join("game.exe"), "patched").unwrap();
        fs::write(inst.join("crash.log"), "x").unwrap();
        assert_eq!(watch_end(w), vec!["{install}/saves"]);
    }

    #[test]
    fn windows_places_inside_a_prefix() {
        let u = PathBuf::from("/data/proton/pfx/drive_c/users/steamuser");
        assert_eq!(in_user("{local}/Game/saves", &u), Some(u.join("AppData/Local").join("Game/saves")));
        assert_eq!(in_user("{config}/Game", &u), Some(u.join("AppData/Roaming").join("Game")));
        assert_eq!(in_user("{documents}/My Games/X", &u), Some(u.join("Documents").join("My Games/X")));
        assert_eq!(in_user("{home}", &u), Some(u.clone()));
        assert_eq!(in_user("{install}/save", &u), None);
        assert_eq!(in_user("{localx}/a", &u), None);
    }

    #[test]
    fn templates_round_trip() {
        let inst = PathBuf::from("/games/x");
        assert_eq!(templatize(&inst.join("save"), &inst), "{install}/save");
        if let Some(h) = dirs::home_dir() {
            let t = templatize(&h.join("Some Game"), &inst);
            assert!(t.starts_with('{'), "{t}");
            assert_eq!(resolve(&t, &inst), h.join("Some Game"));
        }
    }
}

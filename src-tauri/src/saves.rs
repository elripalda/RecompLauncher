//! Save backups. Where each game keeps its saves (the player picked it, or the developer's
//! recomplauncher.json says, or the catalog does; nothing is guessed), a copy after every session
//! (the newest ten on this PC), and putting one back.
//!
//! A copy is a zip: the files under `<n>/<path>` for the n-th save place, and `rlsave.json` saying
//! what the places were (as `{local}/Game/saves`, not this PC's own path, so a copy made on one PC
//! goes to the right place on another), each file's checksum, and a checksum of the whole set.

use crate::library::Installed;
use crate::Error;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// The newest copies kept on this PC, per game.
pub const KEEP: usize = 10;
/// Nothing bigger than this is a save (it's a disc image, a video, a mod pack).
const MAX_FILE: u64 = 64 * 1024 * 1024;
/// No save place is bigger than this.
const SPOT_MAX: u64 = 1024 * 1024 * 1024;
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
/// Folders that are never saves: a game's caches (`{data}/cache`) and temp folders, and the
/// launcher's own work folders in a game's folder (its setup temp folder, the old release an update
/// sets aside), which a save place around them would otherwise take in.
const NOT_SAVE_DIRS: [&str; 19] = [
    "mods", "shadercache", "shader_cache", "shaders", "cache", "caches", "logs", "log", "crashdumps", "dumps", "screenshots", ".backups",
    "texture_packs", "textures", "packs", "temp", "tmp", crate::setup_temp::FOLDER, ".recomplauncher-game-update",
];

/// A place a game keeps its saves.
#[derive(Debug, Clone, Serialize)]
pub struct Spot {
    /// As written: `{local}/Zelda64Recompiled/saves`, `{install}/save/*.sav`.
    pub template: String,
    /// "player", "developer" or "catalog" (and "copy" for the places a copy itself names).
    pub from: &'static str,
}

/// Where a game's saves are: the developer's file, else the catalog, else the player's folders.
/// A folder the developer or RecompLauncher named is the right one (the launcher never moves where
/// a game saves, it only reads that folder), so a folder the player picked before one was named
/// isn't used anymore. Empty when nobody says: the launcher doesn't know where this game saves.
pub fn spots(dev: &[String], catalog: &[String], g: &Installed) -> Vec<Spot> {
    let list = |v: &[String], from: &'static str| v.iter().filter(|t| !t.trim().is_empty()).map(|t| Spot { template: t.trim().to_string(), from }).collect::<Vec<_>>();
    let named = list(dev, "developer");
    if !named.is_empty() {
        return named;
    }
    let ours = list(catalog, "catalog");
    if !ours.is_empty() {
        return ours;
    }
    // The player's own folders: one, or several (a launcher with more than one game in it). Not a
    // guess (the whole game folder, or files it was seen writing): that backed up caches and build
    // folders as saves, and could put the wrong files back.
    let mut mine: Vec<String> = vec![];
    for t in g.save_place.iter().chain(g.more_save_places.iter()).map(|t| t.trim()).filter(|t| !t.is_empty()) {
        if !mine.iter().any(|m| m == t) {
            mine.push(t.to_string());
        }
    }
    mine.into_iter().map(|template| Spot { template, from: "player" }).collect()
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
    // `{data}` before `{install}`: it's inside it, and the narrower name is the one the game uses.
    let bases: Vec<(&str, Option<PathBuf>)> = vec![
        ("{data}", Some(crate::profile::data_dir(install))),
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
    // An update's old files, set aside while it goes in, are never the player's saves.
    name == crate::swap::FOLDER || NOT_SAVE_DIRS.contains(&name.to_lowercase().as_str())
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
fn walk(root: &Path, out: &mut Vec<(String, PathBuf, u64)>, release: &HashSet<String>, budget: u64, secs: u64, strict: bool) -> bool {
    let start = std::time::Instant::now();
    let mut seen = 0usize;
    let mut total = 0u64;
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((d, depth)) = stack.pop() {
        let entries = fs::read_dir(&d);
        if strict && entries.is_err() { return false; }
        for e in entries.into_iter().flatten() {
            let Ok(e) = e else {
                if strict { return false; }
                continue;
            };
            if out.len() >= MAX_FILES {
                return !strict;
            }
            seen += 1;
            if seen > WALK_MAX || total > budget || (seen % 256 == 0 && start.elapsed().as_secs() > secs) {
                return false;
            }
            if e.file_type().map(|t| t.is_symlink()).unwrap_or(true) {
                if strict { return false; }
                continue;
            }
            let p = e.path();
            let Ok(m) = fs::symlink_metadata(&p) else {
                if strict { return false; }
                continue;
            };
            if strict && is_save_link(&m) { return false; }
            let name = e.file_name().to_string_lossy().to_string();
            if m.is_dir() {
                if !skip_dir(&name) && depth < 16 {
                    stack.push((p, depth + 1));
                } else if strict {
                    return false;
                }
                continue;
            }
            let Ok(rel) = p.strip_prefix(root) else { continue };
            let rel = rel.to_string_lossy().replace('\\', "/");
            if is_save_file(&name, m.len()) && !release.contains(&rel.to_lowercase()) {
                if strict && (!m.is_file() || !plain_save_path(&rel) || root.join(&rel) != p || total + m.len() > budget) {
                    return false;
                }
                total += m.len();
                out.push((rel, p, m.len()));
            } else if strict {
                return false;
            }
        }
    }
    true
}

fn is_save_link(m: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // Junctions and other reparse points can lead outside the selected folder.
        if m.file_attributes() & 0x400 != 0 { return true; }
    }
    m.file_type().is_symlink()
}

fn plain_save_path(rel: &str) -> bool {
    !rel.split('/').any(|c| matches!(c, "" | "." | "..") || c.contains(['\\', ':']))
}

/// Every file in the save places, as they are now.
pub fn collect(spots: &[Spot], g: &Installed) -> Vec<Entry> {
    look(spots, g).0
}

/// The save files inside the game's own folder, as paths in it: what the backup before an update
/// keeps. An update only replaces the release's files, so saves elsewhere stay as they are, and
/// nothing outside the known save places is copied (build caches, work folders, game data).
pub fn files_inside(spots: &[Spot], g: &Installed) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = collect(spots, g).into_iter().filter_map(|e| e.abs.strip_prefix(&g.path).ok().map(Path::to_path_buf)).collect();
    out.sort();
    out.dedup();
    out
}

/// Every file in the save places, and the places left out for being too big to be just saves
/// (their indices). Nothing found in a place that was left out doesn't mean it has no saves: they
/// weren't looked at.
pub fn look(spots: &[Spot], g: &Installed) -> (Vec<Entry>, Vec<usize>) {
    let release: HashSet<String> = g.files.iter().map(|f| f.to_string_lossy().replace('\\', "/").to_lowercase()).collect();
    let none = HashSet::new();
    let mut out = vec![];
    let mut too_big = vec![];
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
            let skip = if rel_of_install && lower(&p) == lower(&g.path) { &release } else { &none };
            if !walk(&p, &mut found, skip, SPOT_MAX, WALK_SECS, false) {
                too_big.push(i);
                found.clear();
            }
        }
        // No save place is ever this big.
        let total: u64 = found.iter().map(|f| f.2).sum();
        if total > SPOT_MAX {
            too_big.push(i);
            found.clear();
        }
        found.retain(|(_, abs, _)| !crate::inputs::excluded(g, abs));
        found.sort();
        out.extend(found.into_iter().map(|(rel, abs, size)| Entry { spot: i, rel, abs, size }));
    }
    (out, too_big)
}

/// Why nothing could be backed up when every place was too big to look through.
pub fn too_big_error(g: &Installed, spots: &[Spot], too_big: &[usize]) -> Error {
    let places: Vec<&str> = too_big.iter().filter_map(|&i| spots.get(i)).map(|s| s.template.as_str()).collect();
    Error::Msg(format!("{}'s save folder is too big to be just saves ({}), so it wasn't backed up.", g.name, places.join(", ")))
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

/// The saves as they are now: each file's checksum, the set's, and the places that were too big to
/// look through (see `look`).
pub fn current(spots: &[Spot], g: &Installed) -> (Vec<Entry>, Vec<FileInfo>, String, Vec<usize>) {
    let (entries, too_big) = look(spots, g);
    let infos: Vec<FileInfo> = entries
        .iter()
        .filter_map(|e| Some(FileInfo { spot: e.spot, rel: e.rel.clone(), size: e.size, sha256: sha_file(&e.abs).ok()? }))
        .collect();
    let hash = if infos.is_empty() { String::new() } else { set_hash(&infos) };
    (entries, infos, hash, too_big)
}

/// The name the player gave this PC (Settings → Account), kept here so the many places that stamp a
/// copy or link the account don't each need the settings. Empty: the system's name is used.
static CHOSEN_DEVICE: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

const DEVICE_NAME_MAX: usize = 40;

/// Reads the chosen name out of the settings' `ui` (called at start and whenever settings are saved).
pub fn use_chosen_device(ui: &serde_json::Value) {
    let said = ui.get("device_name").and_then(|v| v.as_str()).unwrap_or("");
    let name: String = said.trim().chars().take(DEVICE_NAME_MAX).collect();
    *CHOSEN_DEVICE.lock().unwrap_or_else(|e| e.into_inner()) = name;
}

pub fn system_device_name() -> String {
    std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "This PC".into())
}

/// Shown next to each save copy ("from DESKTOP-ABC"), so a player can tell their PCs apart. Copies
/// already made keep the name they were made under: it's written into each copy.
pub fn device_name() -> String {
    let chosen = CHOSEN_DEVICE.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if chosen.is_empty() {
        return system_device_name();
    }
    chosen
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

/// Where a game's copies go when the player hasn't picked a folder, or it can't be reached right
/// now (a drive that's unplugged, a network folder that's offline): in its library, beside the
/// backups made before its updates (`<library>/.backups/<game>/saves`). Everything of a game stays
/// in the player's own folders, and the cloud is the copy off the drive. A developer's own build
/// (a folder of theirs, not the library) and a game that isn't installed anymore keep theirs in
/// the launcher's folder.
fn home(app_dir: &Path, id: &str, g: Option<&Installed>) -> PathBuf {
    let own = app_dir.join("saves").join(id);
    let Some(g) = g.filter(|g| !g.local) else { return own };
    let has_parent = g.path.parent().is_some_and(|p| !p.as_os_str().is_empty());
    if !has_parent || g.path.file_name().is_none() {
        return own;
    }
    let saves = crate::backups_dir(g).join("saves");
    // One game of several with saves of its own: a folder of its own inside the recomp's.
    match crate::games::split(id) {
        Some((recomp, sub)) if recomp == g.id => saves.join(sub),
        _ => saves,
    }
}

/// The player's folder (each game's copies in a folder of their own inside it), else `home`.
pub(crate) fn place(app_dir: &Path, id: &str, g: Option<&Installed>) -> PathBuf {
    match chosen_root() {
        Some(p) if p.is_dir() => p.join(id),
        _ => home(app_dir, id, g),
    }
}

/// Where a game's copies are.
pub fn dir_of(app_dir: &Path, id: &str, g: &Installed) -> PathBuf {
    place(app_dir, id, Some(g))
}

/// The completed copies in a folder: the timestamp-named ZIPs `list` reads. Not state.json (this
/// PC's notes) or a .part a backup may still be writing.
fn copy_files(dir: &Path) -> io::Result<Vec<fs::DirEntry>> {
    let mut out = vec![];
    for f in fs::read_dir(dir)? {
        let f = f?;
        let stamp = f.file_name().to_str().and_then(|n| n.strip_suffix(".zip")).is_some_and(|n| n.parse::<u64>().is_ok());
        if stamp && f.file_type()?.is_file() {
            out.push(f);
        }
    }
    Ok(out)
}

/// Moves games' copies (`pairs`: a game's folder of copies, and where they go), each removed only
/// after every one has been checked by checksum. Conflicting names never overwrite history, and a
/// failed copy leaves all originals in place. How many folders it moved copies out of.
pub fn move_copies(pairs: &[(PathBuf, PathBuf)]) -> io::Result<usize> {
    let mut verified = Vec::new();
    let mut games = Vec::new();
    for (src, dst) in pairs {
        // Nothing there to move: no folder made for it either.
        let files = match copy_files(src) {
            Ok(f) if !f.is_empty() => f,
            Ok(_) => continue,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        fs::create_dir_all(dst)?;
        if fs::symlink_metadata(dst)?.file_type().is_symlink() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "A game's destination folder is a link. Pick another backup folder."));
        }
        let (from, to) = (fs::canonicalize(src)?, fs::canonicalize(dst)?);
        if from == to {
            continue;
        }
        if to.starts_with(&from) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Pick a backup folder outside the current backup folder."));
        }
        for f in files {
            let (a, b) = (f.path(), to.join(f.file_name()));
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
        games.push(from);
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

/// Every game whose copies may have to move, with the installed game they belong to (none for one
/// that isn't installed anymore: its copies stay put unless the player's folder changes). The
/// installed games, the games with saves of their own beside them, and the folders of copies in
/// the launcher's folder and the player's.
fn copy_sets(app_dir: &Path, installed: &[Installed]) -> Vec<(String, Option<Installed>)> {
    let folder_names = |d: &Path| -> Vec<String> {
        fs::read_dir(d).into_iter().flatten().flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.file_name().to_string_lossy().to_string()).collect()
    };
    let mut ids: Vec<String> = installed.iter().map(|g| g.id.clone()).collect();
    for g in installed {
        let subs = folder_names(&home(app_dir, &g.id, Some(g)));
        ids.extend(subs.iter().map(|sub| crate::games::full(&g.id, sub)).filter(|id| crate::games::split(id).is_some()));
    }
    ids.extend(folder_names(&app_dir.join("saves")));
    if let Some(root) = chosen_root().filter(|p| p.is_dir()) {
        ids.extend(folder_names(&root));
    }
    // An empty id would name the folder that holds every game's copies.
    ids.retain(|id| !id.is_empty());
    ids.sort();
    ids.dedup();
    let find =|id: &str| installed.iter().find(|g| g.id == id).cloned();
    let owner = |id: &str| find(id).or_else(|| crate::games::split(id).and_then(|(recomp, _)| find(recomp)));
    ids.into_iter().map(|id| {
        let g = owner(&id);
        (id, g)
    }).collect()
}

/// Moves every game's copies to `to` (the player's folder, a folder per game inside it), or to
/// each game's own place in its library when None. Copies from before, in the launcher's folder,
/// go too. How many folders it moved copies out of.
pub fn move_all(app_dir: &Path, installed: &[Installed], to: Option<&Path>) -> io::Result<usize> {
    let mut pairs = vec![];
    for (id, g) in copy_sets(app_dir, installed) {
        let target = match to {
            Some(root) => root.join(&id),
            None => home(app_dir, &id, g.as_ref()),
        };
        let mut from = vec![place(app_dir, &id, g.as_ref()), app_dir.join("saves").join(&id), home(app_dir, &id, g.as_ref())];
        from.sort();
        from.dedup();
        pairs.extend(from.into_iter().filter(|f| *f != target).map(|f| (f, target.clone())));
    }
    move_copies(&pairs)
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
    /// When this PC and the cloud last agreed, and when and where that save was made. Kept here
    /// so the game page can say "in the cloud since 14:02" without asking the website.
    #[serde(default)]
    pub synced_at: u64,
    #[serde(default)]
    pub synced_made: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub synced_device: String,
    /// The cloud copy that changed while this PC's save changed too. Kept until the player
    /// picks one, because the toast that says so is gone after a few seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflict: Option<String>,
    /// Why the last sync didn't go through. Kept until one does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<Problem>,
    /// A session started and its save hasn't been copied since: set at Play, cleared once the
    /// copy after it is made. Still set when the launcher closed (or crashed) before that, so the
    /// next start makes the copy and sends it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub after_play: bool,
}

/// Why a game's saves didn't reach the cloud. `kind` is one of: offline, full, paused,
/// too-big-copy, too-big-folder, error.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Problem {
    pub kind: String,
    pub text: String,
    pub at: u64,
}

impl State {
    /// This PC and the cloud hold the same save now: whatever was in the way is over.
    pub fn mark_synced(&mut self, hash: &str, made: u64, device: &str) {
        self.synced_hash = hash.to_string();
        self.synced_at = crate::library::now();
        self.synced_made = made;
        self.synced_device = device.to_string();
        self.conflict = None;
        self.problem = None;
    }

    /// The newest copy on this PC isn't the one in the cloud: it's queued, or it goes up after
    /// the next session. A copy put back from the cloud sets both hashes, so it isn't behind.
    pub fn behind(&self) -> bool {
        self.pending.is_some() || (!self.last_hash.is_empty() && self.last_hash != self.synced_hash)
    }

    /// One word for where this game's saves stand, from what this PC knows (no website asked):
    /// signed-out, off, conflict, problem, waiting, none or up-to-date. Live work (checking,
    /// uploading, downloading) comes from the save-sync event instead.
    pub fn sync_state(&self, cloud_on: bool, signed_in: bool) -> &'static str {
        if !signed_in {
            return "signed-out";
        }
        if !cloud_on {
            return "off";
        }
        if self.conflict.is_some() {
            return "conflict";
        }
        let kind = self.problem.as_ref().map(|p| p.kind.as_str()).unwrap_or("");
        // A folder too big to look through stops every sync, whether or not a copy waits.
        if kind == "too-big-folder" {
            return "problem";
        }
        // A session whose save hasn't been copied yet may have changed it: not up to date until the
        // copy after it is made.
        if !self.behind() && !self.after_play {
            return if self.synced_hash.is_empty() && self.last_hash.is_empty() { "none" } else { "up-to-date" };
        }
        // No connection is only waiting: it goes up by itself once the website answers.
        if kind.is_empty() || kind == "offline" {
            return "waiting";
        }
        "problem"
    }
}

/// This PC's notes about a game's saves stay in the launcher's own folder, even with the copies
/// somewhere else: a folder other PCs keep in sync too would mix up theirs.
fn state_file(app_dir: &Path, id: &str) -> PathBuf {
    app_dir.join("saves").join(id).join("state.json")
}

pub fn state(app_dir: &Path, id: &str) -> State {
    let read = |p: PathBuf| fs::read(p).ok().and_then(|b| serde_json::from_slice(&b).ok());
    // One from before the notes moved, beside the copies in the player's folder.
    let beside = || chosen_root().and_then(|r| read(r.join(id).join("state.json")));
    read(state_file(app_dir, id)).or_else(beside).unwrap_or_default()
}

pub fn set_state(app_dir: &Path, id: &str, s: &State) {
    let f = state_file(app_dir, id);
    if let Some(d) = f.parent() {
        let _ = fs::create_dir_all(d);
    }
    let _ = fs::write(f, serde_json::to_vec_pretty(s).unwrap_or_default());
}

/// Copies go where they belong now: from the launcher's own folder (where they were kept before
/// they moved into each game's library), and the ones made beside each game while the player's
/// folder couldn't be reached go to it once it's back. This PC's notes stay.
pub fn gather(app_dir: &Path, installed: &[Installed]) {
    let to = chosen_root().filter(|p| p.is_dir());
    match move_all(app_dir, installed, to.as_deref()) {
        Ok(0) => {}
        Ok(n) => crate::trail::note(format!("Saves: copies moved to where they're kept now ({n} folders)")),
        Err(e) => crate::trail::note(format!("Saves: couldn't move copies to where they're kept now ({e})")),
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
pub fn list(app_dir: &Path, id: &str, g: &Installed) -> Vec<Copy> {
    let mut out: Vec<Copy> = fs::read_dir(dir_of(app_dir, id, g))
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

/// The save as it is now has a copy (or there is none to copy): no session waits on it anymore.
/// Every copy clears it, whatever made it (after a session, Sync Now, Back Up Now, before a restore
/// or a cloud save), so "Waiting to upload" can't outlive the copy. All of those run with the game
/// closed, so none ends a session that's still going.
fn copied(app_dir: &Path, id: &str) {
    let mut st = state(app_dir, id);
    if !st.after_play {
        return;
    }
    st.after_play = false;
    set_state(app_dir, id, &st);
}

/// Writes a copy of the saves as they are now. None when there are none, or nothing changed since
/// the last copy.
pub fn backup(app_dir: &Path, id: &str, spots: &[Spot], g: &Installed) -> Result<Option<Copy>, Error> {
    // How big it is first: a place too big to be saves is left out before anything is read.
    // Nothing read is said as such, not as "nothing changed": the saves may well have.
    let (pre, too_big) = look(spots, g);
    if pre.is_empty() && !too_big.is_empty() {
        return Err(too_big_error(g, spots, &too_big));
    }
    if pre.is_empty() {
        copied(app_dir, id);
        return Ok(None);
    }
    let (entries, infos, hash, _) = current(spots, g);
    if entries.is_empty() {
        copied(app_dir, id);
        return Ok(None);
    }
    let mut st = state(app_dir, id);
    let all = list(app_dir, id, g);
    if st.last_hash == hash && all.first().map(|c| c.hash == hash).unwrap_or(false) {
        copied(app_dir, id);
        return Ok(None);
    }
    let d = dir_of(app_dir, id, g);
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
    let mut all = list(app_dir, id, g);
    while all.len() > KEEP {
        if let Some(old) = all.pop() {
            let _ = fs::remove_file(d.join(old.name));
        }
    }
    st.last_hash = hash.clone();
    st.after_play = false;
    set_state(app_dir, id, &st);
    let size = fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
    Ok(Some(Copy { name: format!("{when}.zip"), when, size, files: infos.len(), hash, device: man.device, played: man.played }))
}

/// The game's save folders now, when its developer or the catalog moved them since the copy was
/// made (Hookshot's saves going from {home}/Games into its library folder): the copy's folder N
/// goes into folder N now. Only for folders the developer or the catalog named (the player's own
/// folders are matched before this). Not when the count differs, when a
/// folder only changed places in the list, or when a folder is the game's whole folder.
fn moved_places(copy: &[String], now: &[Spot]) -> Option<Vec<Spot>> {
    if now.is_empty() || now.len() != copy.len() {
        return None;
    }
    if !now.iter().all(|s| matches!(s.from, "developer" | "catalog")) {
        return None;
    }
    if now.iter().any(|s| s.template.trim_end_matches(['/', '\\']) == "{install}") {
        return None;
    }
    let moved = copy.iter().zip(now).any(|(t, s)| *t != s.template);
    if !moved {
        return None;
    }
    let reordered = copy.iter().enumerate().any(|(i, t)| now.iter().enumerate().any(|(j, s)| i != j && s.template == *t));
    if reordered {
        return None;
    }
    // A pattern's files sit beside it and a folder's files inside it: the two don't map onto
    // each other.
    let same_kind = copy.iter().zip(now).all(|(t, s)| t.contains('*') == s.template.contains('*'));
    same_kind.then(|| now.to_vec())
}

/// Said when a save file can't be written where the game reads it. The folder and the system's
/// reason go on the line under it: "Read-only file system (os error 30)" alone named neither.
pub const SAVE_NOT_WRITTEN: &str = "The save couldn't be written to its folder.";

fn not_written(path: &Path, e: io::Error) -> Error {
    Error::Msg(format!("{SAVE_NOT_WRITTEN}\n{}: {e}", path.display()))
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
    } else if let Some(now) = moved_places(&man.spots, spots_now) {
        crate::trail::note(format!("Saves {id}: the copy's save folders moved ({} -> {}), restored into the new ones", man.spots.join(", "), now.iter().map(|s| s.template.as_str()).collect::<Vec<_>>().join(", ")));
        now
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
        // (never the game's whole folder), and places outside the game's folder. Not a place inside
        // the game's folder that only the copy names (older launchers guessed those), or one that
        // holds programs: those can be part of the game (a runtime it downloaded).
        let named = |t: &str| spots_now.iter().any(|s| s.template == t && matches!(s.from, "developer" | "catalog" | "player"));
        let may_clear: Vec<bool> = same
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let t = &s.template;
                let whole = t.trim_end_matches(['/', '\\']) == "{install}";
                let in_game = t.starts_with("{install}") || t.starts_with("{data}");
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
    let skip: HashSet<usize> = same.iter().enumerate().filter(|(i, s)| (s.template.starts_with("{install}/") || s.template.starts_with("{data}/")) && !today.contains(s.template.as_str()) && roots.get(*i).map(|r| holds_programs(r)).unwrap_or(false)).map(|(i, _)| i).collect();
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
            fs::create_dir_all(d).map_err(|why| not_written(d, why))?;
        }
        if crate::inputs::excluded(g, &dest) { continue; }
        let tmp = dest.with_extension("rl-part");
        let mut out = fs::File::create(&tmp).map_err(|why| not_written(&dest, why))?;
        io::copy(&mut e, &mut out).map_err(|why| not_written(&dest, why))?;
        // Closed before the rename: Windows won't rename a file that's still open.
        drop(out);
        fs::rename(&tmp, &dest).map_err(|why| not_written(&dest, why))?;
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
    #[serde(default)]
    pub is_dir: bool,
}

impl ShareFile {
    pub fn when(&self) -> Option<u64> {
        self.modified.map(|t| t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs())
    }
}

fn share_dir_files(path: &Path) -> Option<Vec<(String, PathBuf, u64)>> {
    let mut files = Vec::new();
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() && !is_save_link(&m) => {
            if !walk(path, &mut files, &HashSet::new(), SPOT_MAX, WALK_SECS, true) { return None; }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        _ => return None,
    }
    Some(files)
}

fn share_path_allowed(g: &Installed, path: &Path) -> bool {
    !crate::inputs::excluded(g, path) && !g.files.iter().any(|f| lower(&g.path.join(f)) == lower(path))
}

/// Compare a save at one known location. A selected folder is a save set even when it has one slot.
pub fn share_file(spot: &Spot, g: &Installed, peer: Option<&ShareFile>) -> Option<ShareFile> {
    if !matches!(spot.from, "developer" | "catalog" | "player") { return None; }
    let place = resolve_for(&spot.template, g);
    let name = place.file_name()?.to_str()?;
    if name.contains('*') || lower(&place) == lower(&g.path) { return None; }
    if let Ok(install) = g.path.canonicalize() {
        if place.canonicalize().is_ok_and(|p| p == install) { return None; }
    }
    let folder = spot.from == "player" || place.is_dir();
    if !folder || peer.is_some_and(|p| !p.is_dir) {
        if peer.is_some_and(|p| p.is_dir) { return None; }
        let path = if folder {
            let name = peer?.path.file_name()?;
            let files = share_dir_files(&place)?;
            if !files.is_empty() && (files.len() != 1 || files[0].1 != place.join(name)) { return None; }
            place.join(name)
        } else {
            place
        };
        let name = path.file_name()?.to_str()?;
        if !share_path_allowed(g, &path) { return None; }
        return match fs::symlink_metadata(&path) {
            Ok(m) if m.is_file() && !is_save_link(&m) && is_save_file(name, m.len()) => {
                Some(ShareFile { hash: sha_file(&path).ok()?, modified: Some(m.modified().ok()?), path, is_dir: false })
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound && peer.is_some() && is_save_file(name, 0) => {
                if peer?.path.file_name()? != name { return None; }
                Some(ShareFile { path, hash: String::new(), modified: None, is_dir: false })
            }
            _ => None,
        };
    }
    if holds_programs(&place) { return None; }
    let files = share_dir_files(&place)?;
    if files.is_empty() && peer.is_none() { return None; }
    let mut modified = None;
    let mut infos = Vec::new();
    for (rel, abs, size) in files {
        if !share_path_allowed(g, &place.join(&rel)) { return None; }
        modified = modified.max(Some(fs::metadata(&abs).ok()?.modified().ok()?));
        infos.push(FileInfo { spot: 0, rel, size, sha256: sha_file(&abs).ok()? });
    }
    let hash = if infos.is_empty() { String::new() } else { set_hash(&infos) };
    Some(ShareFile { path: place, hash, modified, is_dir: true })
}

pub fn share_pair(spot: &Spot, g: &Installed, mut source: ShareFile) -> Option<(ShareFile, ShareFile)> {
    if let Some(target) = share_file(spot, g, Some(&source)) {
        return Some((source, target));
    }
    let place = resolve_for(&spot.template, g);
    if !source.is_dir || spot.from == "player" || place.is_dir() { return None; }
    let files = share_dir_files(&source.path)?;
    let [(rel, path, _)] = files.as_slice() else { return None };
    if Path::new(rel).file_name()? != rel.as_str() { return None; }
    source = ShareFile { path: path.clone(), hash: sha_file(path).ok()?, modified: source.modified, is_dir: false };
    let target = share_file(spot, g, Some(&source))?;
    Some((source, target))
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
        let saved = list(app_dir, id, g).into_iter().next().ok_or_else(changed)?;
        let zip_path = dir_of(app_dir, id, g).join(saved.name);
        if !target.is_dir {
            let mut zip = zip::ZipArchive::new(fs::File::open(&zip_path)?)
                .map_err(|e| Error::Msg(e.to_string()))?;
            let entry = format!("0/{}", target.path.file_name().unwrap().to_string_lossy());
            let mut copy = Vec::new();
            zip.by_name(&entry).map_err(|e| Error::Msg(e.to_string()))?.take(MAX_FILE + 1).read_to_end(&mut copy)?;
            if copy.len() as u64 > MAX_FILE || hex::encode(Sha256::digest(&copy)) != target.hash {
                return Err(Error::Msg("The existing save could not be backed up. It was kept in place.".into()));
            }
        } else {
            let man = read_manifest(&zip_path).ok_or_else(changed)?;
            let mut zip = zip::ZipArchive::new(fs::File::open(&zip_path)?)
                .map_err(|e| Error::Msg(e.to_string()))?;
            if man.hash != target.hash || set_hash(&man.files) != target.hash {
                return Err(Error::Msg("The existing save could not be backed up. It was kept in place.".into()));
            }
            for file in &man.files {
                let mut bytes = Vec::new();
                let name = format!("0/{}", file.rel);
                zip.by_name(&name).map_err(|e| Error::Msg(e.to_string()))?.take(MAX_FILE + 1).read_to_end(&mut bytes)?;
                if bytes.len() as u64 != file.size || hex::encode(Sha256::digest(&bytes)) != file.sha256 {
                    return Err(Error::Msg("The existing save could not be backed up. It was kept in place.".into()));
                }
            }
        }
    }

    if !source.is_dir && !target.is_dir {
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
    } else if source.is_dir && target.is_dir {
        let src_found = share_dir_files(&source.path).ok_or_else(changed)?;
        let parent = target.path.parent().ok_or_else(changed)?;
        fs::create_dir_all(parent)?;
        let source_root = source.path.canonicalize()?;
        let target_root = match target.path.canonicalize() {
            Ok(path) => path,
            Err(e) if e.kind() == io::ErrorKind::NotFound => parent.canonicalize()?.join(target.path.file_name().ok_or_else(changed)?),
            Err(e) => return Err(e.into()),
        };
        if source_root.starts_with(&target_root) || target_root.starts_with(&source_root) { return Err(changed()); }
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temp_dir = target.path.with_file_name(format!(".recomplauncher-savedir-{}-{n}.tmp", std::process::id()));
        fs::create_dir(&temp_dir)?;
        let mut keep_previous = false;
        let staged = (|| -> Result<(), Error> {
            let incoming = temp_dir.join("incoming");
            let previous = temp_dir.join("previous");
            fs::create_dir(&incoming)?;
            let mut staged_infos = Vec::new();
            for (rel, abs_src, size) in &src_found {
                if !share_path_allowed(g, &target.path.join(rel)) { return Err(changed()); }
                let temp_dest = incoming.join(rel);
                if let Some(parent) = temp_dest.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut bytes = Vec::new();
                fs::File::open(abs_src)?.take(MAX_FILE + 1).read_to_end(&mut bytes)?;
                if bytes.len() as u64 != *size {
                    return Err(changed());
                }
                let sha256 = hex::encode(Sha256::digest(&bytes));
                staged_infos.push(FileInfo {
                    spot: 0,
                    rel: rel.clone(),
                    size: *size,
                    sha256,
                });
                let mut f = fs::File::create_new(&temp_dest)?;
                f.write_all(&bytes)?;
                f.set_modified(fs::metadata(abs_src)?.modified()?)?;
                f.sync_all()?;
            }
            if set_hash(&staged_infos) != source.hash {
                return Err(changed());
            }

            if share_file(spot, g, Some(source)).as_ref() != Some(target) {
                return Err(changed());
            }
            let moved = target.path.try_exists()?;
            if moved { fs::rename(&target.path, &previous)?; }
            if let Err(e) = fs::rename(&incoming, &target.path) {
                if moved && fs::rename(&previous, &target.path).is_err() {
                    keep_previous = true;
                    return Err(Error::Msg(format!("The save could not be copied. The original is at {}.", previous.display())));
                }
                return Err(e.into());
            }
            Ok(())
        })();
        if !keep_previous {
            let _ = fs::remove_dir_all(&temp_dir);
        }
        staged?;
    } else {
        return Err(changed());
    }

    let mut s = state(app_dir, id);
    s.pending = None;
    set_state(app_dir, id, &s);
    Ok(())
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
        let s = spots(&[], &[], &g);
        assert_eq!(s.iter().map(|x| (x.template.as_str(), x.from)).collect::<Vec<_>>(), vec![("{install}/TT-Xbox/saves", "player"), ("{install}/ROTK-Xbox/saves", "player")]);
        // Both folders' files go in the backup, kept apart.
        let mut from: Vec<usize> = collect(&s, &g).iter().map(|e| e.spot).collect();
        from.sort();
        assert_eq!(from, vec![0, 1]);
        let _ = fs::remove_dir_all(&d);
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

    // A game whose saves are in {install}/save, with one save file in it.
    fn game_with_a_save(d: &Path) -> (Installed, Vec<Spot>, PathBuf) {
        let g = game(&d.join("lib/Game"));
        fs::create_dir_all(d.join("lib/Game/save")).unwrap();
        fs::write(d.join("lib/Game/save/slot.sav"), b"one").unwrap();
        let s = spots(&["{install}/save".into()], &[], &g);
        (g, s, d.join("app"))
    }

    #[test]
    fn a_copy_of_the_save_ends_the_open_session() {
        let d = tmp("copy-ends-session");
        let (g, s, app) = game_with_a_save(&d);
        set_state(&app, "g", &State { after_play: true, ..Default::default() });
        backup(&app, "g", &s, &g).unwrap();
        assert!(!state(&app, "g").after_play);
    }

    #[test]
    fn a_save_already_copied_ends_the_open_session_too() {
        let d = tmp("copied-ends-session");
        let (g, s, app) = game_with_a_save(&d);
        backup(&app, "g", &s, &g).unwrap();
        let mut st = state(&app, "g");
        st.after_play = true;
        set_state(&app, "g", &st);
        assert!(backup(&app, "g", &s, &g).unwrap().is_none());
        assert!(!state(&app, "g").after_play);
    }

    #[test]
    fn a_folder_in_data_is_named_by_data() {
        let d = tmp("data-name");
        assert_eq!(templatize(&d.join("recompdata").join("saves"), &d), "{data}/saves");
        assert_eq!(templatize(&d.join("recompdata"), &d), "{data}");
        assert_eq!(templatize(&d.join("save"), &d), "{install}/save");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn data_as_a_save_place_leaves_out_caches_and_the_launchers_work_folders() {
        let d = tmp("data-skip");
        let data = d.join("recompdata");
        for sub in ["saves", "cache/shaders", ".recomplauncher-tmp", ".recomplauncher-game-update/old", "tmp"] {
            fs::create_dir_all(data.join(sub)).unwrap();
        }
        fs::write(data.join("saves/slot1.sav"), b"save").unwrap();
        fs::write(data.join("settings.toml"), b"x = 1").unwrap();
        fs::write(data.join("cache/shaders/a.bin"), b"cache").unwrap();
        fs::write(data.join(".recomplauncher-tmp/part.bin"), b"temp").unwrap();
        fs::write(data.join(".recomplauncher-game-update/old/old.sav"), b"old").unwrap();
        fs::write(data.join("tmp/x.sav"), b"temp").unwrap();
        let g = game(&d);
        let s = vec![Spot { template: "{data}".into(), from: "developer" }];
        let mut got: Vec<String> = collect(&s, &g).into_iter().map(|e| e.rel).collect();
        got.sort();
        assert_eq!(got, vec!["saves/slot1.sav", "settings.toml"]);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn the_whole_game_folder_as_a_save_place_leaves_out_the_setup_temp_folder() {
        let d = tmp("install-skip");
        fs::create_dir_all(d.join(".recomplauncher-tmp")).unwrap();
        fs::write(d.join(".recomplauncher-tmp/half.sav"), b"temp").unwrap();
        fs::write(d.join("slot1.sav"), b"save").unwrap();
        let g = game(&d);
        let s = vec![Spot { template: "{install}".into(), from: "player" }];
        let got: Vec<String> = collect(&s, &g).into_iter().map(|e| e.rel).collect();
        assert_eq!(got, vec!["slot1.sav"]);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn saves_in_data_are_kept_beside_the_library() {
        // What uninstalling with "keep a copy of its saves" relies on: the copy is outside the game.
        let d = tmp("data-keep");
        let app = d.join("app");
        let inst = d.join("lib").join("Game");
        fs::create_dir_all(inst.join("recompdata/saves")).unwrap();
        fs::write(inst.join("recompdata/saves/slot1.sav"), b"save").unwrap();
        let g = game(&inst);
        let s = vec![Spot { template: "{data}/saves".into(), from: "developer" }];
        assert_eq!(files_inside(&s, &g), vec![PathBuf::from("recompdata").join("saves").join("slot1.sav")]);
        let copy = backup(&app, "g", &s, &g).unwrap().unwrap();
        let kept = dir_of(&app, "g", &g).join(&copy.name);
        assert!(kept.is_file() && !kept.starts_with(&inst), "{}", kept.display());
        fs::remove_dir_all(&inst).unwrap();
        assert!(kept.is_file(), "the copy outlives the game's folder");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn nothing_known_means_no_save_places() {
        let g = game(Path::new("/games/test"));
        assert!(spots(&[], &[], &g).is_empty());
    }

    #[test]
    fn the_catalog_says_when_the_developer_does_not() {
        let g = game(Path::new("/games/test"));
        let s = spots(&[], &["{local}/Test/saves".into()], &g);
        assert_eq!(s.iter().map(|x| (x.template.as_str(), x.from)).collect::<Vec<_>>(), vec![("{local}/Test/saves", "catalog")]);
    }

    #[test]
    fn the_backup_before_an_update_keeps_only_saves_in_the_game_folder() {
        let d = tmp("files-inside");
        let inst = d.join("inst");
        fs::create_dir_all(inst.join("save")).unwrap();
        fs::create_dir_all(inst.join("build/cache")).unwrap();
        fs::create_dir_all(d.join("elsewhere")).unwrap();
        fs::write(inst.join("save/slot1.sav"), "one").unwrap();
        fs::write(inst.join("build/cache/obj.o"), "not a save").unwrap();
        fs::write(inst.join("settings.toml"), "a=1").unwrap();
        fs::write(d.join("elsewhere/slot2.sav"), "two").unwrap();
        let g = game(&inst);
        let elsewhere = d.join("elsewhere").to_string_lossy().replace('\\', "/");
        let s = spots(&["{install}/save".into(), elsewhere], &[], &g);
        assert_eq!(files_inside(&s, &g), vec![PathBuf::from("save").join("slot1.sav")]);
        assert!(files_inside(&[], &g).is_empty());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn copies_go_beside_the_game_in_its_library() {
        let d = tmp("copies-home");
        let app = d.join("app");
        let g = game(&d.join("lib/Game"));
        assert_eq!(dir_of(&app, "g", &g), d.join("lib/.backups/Game/saves"));
        let full = crate::games::full("g", "two");
        assert_eq!(dir_of(&app, &full, &g), d.join("lib/.backups/Game/saves/two"));
        let own_build = Installed { local: true, ..g.clone() };
        assert_eq!(dir_of(&app, "g", &own_build), app.join("saves/g"));
    }

    #[test]
    fn copies_from_the_launchers_folder_move_beside_the_game_once() {
        let d = tmp("copies-gather");
        let app = d.join("app");
        let g = game(&d.join("lib/Game"));
        fs::create_dir_all(app.join("saves/g")).unwrap();
        fs::write(app.join("saves/g/100.zip"), b"old copy").unwrap();
        fs::write(app.join("saves/g/state.json"), b"{}").unwrap();
        assert_eq!(move_all(&app, std::slice::from_ref(&g), None).unwrap(), 1);
        assert_eq!(fs::read(d.join("lib/.backups/Game/saves/100.zip")).unwrap(), b"old copy");
        assert!(!app.join("saves/g/100.zip").exists());
        // This PC's notes stay in the launcher's folder.
        assert!(app.join("saves/g/state.json").is_file());
        assert_eq!(move_all(&app, std::slice::from_ref(&g), None).unwrap(), 0);
    }

    #[test]
    fn a_game_with_saves_of_its_own_moves_into_its_own_folder() {
        let d = tmp("copies-gather-sub");
        let app = d.join("app");
        let g = game(&d.join("lib/Game"));
        let full = crate::games::full("g", "two");
        fs::create_dir_all(app.join("saves").join(&full)).unwrap();
        fs::write(app.join("saves").join(&full).join("100.zip"), b"its copy").unwrap();
        move_all(&app, std::slice::from_ref(&g), None).unwrap();
        assert_eq!(fs::read(d.join("lib/.backups/Game/saves/two/100.zip")).unwrap(), b"its copy");
    }

    #[test]
    fn copies_go_to_the_players_folder() {
        let d = tmp("copies-root");
        let app = d.join("app");
        let g = game(&d.join("lib/Game"));
        let home = d.join("lib/.backups/Game/saves");
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join("100.zip"), b"copy").unwrap();
        let mine = d.join("mine");
        assert_eq!(move_all(&app, std::slice::from_ref(&g), Some(&mine)).unwrap(), 1);
        assert_eq!(fs::read(mine.join("g/100.zip")).unwrap(), b"copy");
        assert!(!home.join("100.zip").exists());
    }

    #[test]
    fn the_developer_comes_before_the_catalog() {
        let g = game(Path::new("/games/test"));
        let s = spots(&["{install}/save".into()], &["{local}/Test/saves".into()], &g);
        assert_eq!(s.iter().map(|x| (x.template.as_str(), x.from)).collect::<Vec<_>>(), vec![("{install}/save", "developer")]);
    }

    #[test]
    fn the_developers_folder_comes_before_one_the_player_picked() {
        let mut g = game(Path::new("/games/test"));
        g.save_place = Some("{install}/mine".into());
        let s = spots(&["{install}/save".into()], &[], &g);
        assert_eq!(s.iter().map(|x| (x.template.as_str(), x.from)).collect::<Vec<_>>(), vec![("{install}/save", "developer")]);
    }

    #[test]
    fn recomplaunchers_folder_comes_before_one_the_player_picked() {
        let mut g = game(Path::new("/games/test"));
        g.save_place = Some("{install}/mine".into());
        let s = spots(&[], &["{local}/Test/saves".into()], &g);
        assert_eq!(s.iter().map(|x| (x.template.as_str(), x.from)).collect::<Vec<_>>(), vec![("{local}/Test/saves", "catalog")]);
    }

    #[test]
    fn the_players_folder_counts_when_nobody_named_one() {
        let mut g = game(Path::new("/games/test"));
        g.save_place = Some("{install}/mine".into());
        let s = spots(&[], &[], &g);
        assert_eq!(s.iter().map(|x| (x.template.as_str(), x.from)).collect::<Vec<_>>(), vec![("{install}/mine", "player")]);
    }

    #[test]
    fn sharing_keeps_the_previous_save_and_the_source() {
        let d = tmp("sharing-copy");
        let source = game(&d.join("source"));
        let target = game(&d.join("target"));
        let spot = Spot { template: "{install}/saves/slot.sav".into(), from: "developer" };
        for (g, data, seconds) in [(&source, b"new", 2000), (&target, b"old", 1000)] {
            fs::create_dir_all(g.path.join("saves")).unwrap();
            let mut file = fs::File::create(g.path.join("saves/slot.sav")).unwrap();
            file.write_all(data).unwrap();
            file.set_modified(UNIX_EPOCH + std::time::Duration::from_secs(seconds)).unwrap();
        }
        let from = share_file(&spot, &source, None).unwrap();
        let to = share_file(&spot, &target, Some(&from)).unwrap();
        let folder = Spot { template: "{install}/saves".into(), from: "player" };
        assert_eq!(share_file(&folder, &target, Some(&from)).as_ref(), Some(&to));
        let folder_source = share_file(&folder, &source, None).unwrap();
        let (paired_from, paired_to) = share_pair(&spot, &target, folder_source).unwrap();
        assert_eq!((&paired_from, &paired_to), (&from, &to));
        share_save(&d, "target", &spot, &target, &from, &to).unwrap();
        assert_eq!(fs::read(&from.path).unwrap(), b"new");
        assert_eq!(fs::read(&to.path).unwrap(), b"new");
        assert_eq!(share_file(&spot, &target, Some(&from)).unwrap().modified, from.modified);
        let copy = dir_of(&d, "target", &target).join(&list(&d, "target", &target)[0].name);
        restore(&d, "target", &copy, &[spot], &target).unwrap();
        assert_eq!(fs::read(&to.path).unwrap(), b"old");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn sharing_multi_file_directory_save() {
        let d = tmp("sharing-multi");
        let source = game(&d.join("source"));
        let target = game(&d.join("target"));
        let spot = Spot { template: "{install}/save".into(), from: "player" };
        let src_dir = source.path.join("save");
        let tgt_dir = target.path.join("save");
        fs::create_dir_all(src_dir.join("slot_00")).unwrap();
        fs::create_dir_all(tgt_dir.join("slot_00")).unwrap();

        fs::write(src_dir.join("account.sav"), b"account-new").unwrap();
        fs::File::options().write(true).open(src_dir.join("account.sav")).unwrap()
            .set_modified(UNIX_EPOCH + std::time::Duration::from_secs(3000)).unwrap();
        fs::write(src_dir.join("slot_00/progress.sav"), b"progress-new").unwrap();
        fs::File::options().write(true).open(src_dir.join("slot_00/progress.sav")).unwrap()
            .set_modified(UNIX_EPOCH + std::time::Duration::from_secs(3000)).unwrap();

        fs::write(tgt_dir.join("account.sav"), b"account-old").unwrap();
        fs::File::options().write(true).open(tgt_dir.join("account.sav")).unwrap()
            .set_modified(UNIX_EPOCH + std::time::Duration::from_secs(1000)).unwrap();
        fs::write(tgt_dir.join("slot_00/progress.sav"), b"progress-old").unwrap();
        fs::File::options().write(true).open(tgt_dir.join("slot_00/progress.sav")).unwrap()
            .set_modified(UNIX_EPOCH + std::time::Duration::from_secs(1000)).unwrap();
        fs::write(tgt_dir.join("deleted.sav"), b"deleted-slot").unwrap();
        fs::File::options().write(true).open(tgt_dir.join("deleted.sav")).unwrap()
            .set_modified(UNIX_EPOCH + std::time::Duration::from_secs(1000)).unwrap();

        let from = share_file(&spot, &source, None).unwrap();
        assert!(from.is_dir);
        let to = share_file(&spot, &target, Some(&from)).unwrap();
        assert!(to.is_dir);
        assert!(newer_save(&from, &to));

        share_save(&d, "target", &spot, &target, &from, &to).unwrap();
        assert_eq!(fs::read(tgt_dir.join("account.sav")).unwrap(), b"account-new");
        assert_eq!(fs::read(tgt_dir.join("slot_00/progress.sav")).unwrap(), b"progress-new");
        assert!(!tgt_dir.join("deleted.sav").exists());

        // previous target save was backed up
        let copy = dir_of(&d, "target", &target).join(&list(&d, "target", &target)[0].name);
        restore(&d, "target", &copy, &[spot], &target).unwrap();
        assert_eq!(fs::read(tgt_dir.join("account.sav")).unwrap(), b"account-old");
        assert_eq!(fs::read(tgt_dir.join("slot_00/progress.sav")).unwrap(), b"progress-old");
        assert_eq!(fs::read(tgt_dir.join("deleted.sav")).unwrap(), b"deleted-slot");

        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn sharing_multi_file_directory_to_empty_destination() {
        let d = tmp("sharing-multi-empty");
        let source = game(&d.join("source"));
        let target = game(&d.join("target"));
        let spot = Spot { template: "{install}/save".into(), from: "player" };
        let src_dir = source.path.join("save");
        let tgt_dir = target.path.join("save");
        fs::create_dir_all(src_dir.join("slot_00")).unwrap();
        fs::write(src_dir.join("account.sav"), b"account-new").unwrap();
        fs::write(src_dir.join("slot_00/progress.sav"), b"progress-new").unwrap();

        let from = share_file(&spot, &source, None).unwrap();
        let to = share_file(&spot, &target, Some(&from)).unwrap();
        assert_eq!(to.modified, None);
        assert!(newer_save(&from, &to));

        share_save(&d, "target", &spot, &target, &from, &to).unwrap();
        assert_eq!(fs::read(tgt_dir.join("account.sav")).unwrap(), b"account-new");
        assert_eq!(fs::read(tgt_dir.join("slot_00/progress.sav")).unwrap(), b"progress-new");

        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn sharing_one_slot_can_replace_a_directory_with_more_slots() {
        let d = tmp("sharing-one-slot");
        let source = game(&d.join("source"));
        let target = game(&d.join("target"));
        let spot = Spot { template: "{install}/save".into(), from: "player" };
        for (g, slots, time) in [(&source, 1, 2000), (&target, 2, 1000)] {
            let dir = g.path.join("save");
            fs::create_dir_all(&dir).unwrap();
            for slot in 0..slots {
                let mut file = fs::File::create(dir.join(format!("{slot}.sav"))).unwrap();
                file.write_all(&[slots]).unwrap();
                file.set_modified(UNIX_EPOCH + std::time::Duration::from_secs(time)).unwrap();
            }
        }
        let from = share_file(&spot, &source, None).unwrap();
        let to = share_file(&spot, &target, Some(&from)).unwrap();
        fs::write(target.path.join("settings.ini"), b"local settings").unwrap();
        share_save(&d, "target", &spot, &target, &from, &to).unwrap();
        assert_eq!(fs::read(to.path.join("0.sav")).unwrap(), [1]);
        assert!(!to.path.join("1.sav").exists());
        assert_eq!(fs::read(target.path.join("settings.ini")).unwrap(), b"local settings");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn sharing_rejects_a_directory_backup_with_wrong_file_bytes() {
        let d = tmp("sharing-bad-backup");
        let source = game(&d.join("source"));
        let target = game(&d.join("target"));
        let spot = Spot { template: "{install}/save".into(), from: "player" };
        for (g, bytes, time) in [(&source, b"new", 2000), (&target, b"old", 1000)] {
            let dir = g.path.join("save");
            fs::create_dir_all(&dir).unwrap();
            let mut file = fs::File::create(dir.join("slot.sav")).unwrap();
            file.write_all(bytes).unwrap();
            file.set_modified(UNIX_EPOCH + std::time::Duration::from_secs(time)).unwrap();
        }
        let copy = backup(&d, "target", std::slice::from_ref(&spot), &target).unwrap().unwrap();
        let path = dir_of(&d, "target", &target).join(copy.name);
        let manifest = read_manifest(&path).unwrap();
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("rlsave.json", options).unwrap();
        serde_json::to_writer(&mut zip, &manifest).unwrap();
        zip.start_file("0/slot.sav", options).unwrap();
        zip.write_all(b"bad").unwrap();
        zip.finish().unwrap();

        let from = share_file(&spot, &source, None).unwrap();
        let to = share_file(&spot, &target, Some(&from)).unwrap();
        assert!(share_save(&d, "target", &spot, &target, &from, &to).is_err());
        assert_eq!(fs::read(to.path.join("slot.sav")).unwrap(), b"old");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn sharing_rejects_a_save_changed_since_the_prompt() {
        let d = tmp("sharing-stale");
        let inst = d.join("game");
        fs::create_dir_all(&inst).unwrap();
        let source = game(&inst);
        let target = game(&inst);
        let from_spot = Spot { template: "{install}/from.sav".into(), from: "developer" };
        let to_spot = Spot { template: "{install}/to.sav".into(), from: "developer" };
        fs::write(inst.join("from.sav"), b"new").unwrap();
        fs::write(inst.join("to.sav"), b"old").unwrap();
        fs::File::options().write(true).open(inst.join("to.sav")).unwrap().set_modified(UNIX_EPOCH).unwrap();
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
    fn a_named_save_folder_too_big_is_named_in_the_error() {
        let g = game(Path::new("/games/test"));
        let s = vec![Spot { template: "{install}/save".into(), from: "developer" }];
        let e = too_big_error(&g, &s, &[0]);
        assert_eq!(e.to_string(), "Test Game's save folder is too big to be just saves ({install}/save), so it wasn't backed up.");
    }

    /// Cloud sync stops on any place left out, so it has to be reported even when another place
    /// had saves in it.
    #[test]
    fn a_place_too_big_next_to_one_with_saves_is_still_reported() {
        let d = tmp("big-next-to-saves");
        fs::create_dir_all(d.join("saves")).unwrap();
        fs::write(d.join("saves/slot1.sav"), "one").unwrap();
        // More than any save place holds, in files each small enough to be a save. Sparse, so
        // they take no room on the disk.
        fs::create_dir_all(d.join("extra")).unwrap();
        for n in 0..20 {
            fs::File::create(d.join("extra").join(format!("part-{n}.dat"))).unwrap().set_len(60 * 1024 * 1024).unwrap();
        }
        let g = game(&d);
        let s = spots(&["{install}/extra".into(), "{install}/saves".into()], &[], &g);
        let (entries, too_big) = look(&s, &g);
        assert_eq!(entries.len(), 1);
        assert_eq!(too_big, vec![0]);
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
        assert_eq!(list(&app, "g", &g).len(), 2);
        // Back to the first: slot2 goes, slot1 is "one" again, and what was there is kept.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        restore(&app, "g", &dir_of(&app, "g", &g).join(&a.name), &s, &g).unwrap();
        assert_eq!(fs::read_to_string(inst.join("save/slot1.sav")).unwrap(), "one");
        assert!(!inst.join("save/slot2.sav").exists());
        let (_, _, now, _) = current(&s, &g);
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
        let queued_path = dir_of(&app, "g", &g).join(&queued.name);
        let queued_bytes = fs::read(&queued_path).unwrap();
        set_state(&app, "g", &State { last_hash: queued.hash.clone(), synced_hash: "previous cloud".into(), pending: Some(queued.name.clone()), ..Default::default() });
        set_state(&app, "other", &State { pending: Some("123.zip".into()), ..Default::default() });

        restore(&app, "g", &dir_of(&app, "g", &g).join(&chosen.name), &spots, &g).unwrap();

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
        assert!(restore(&app, "g", &dir_of(&app, "g", &g).join(&queued.name), &spots, &g).is_err());
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
            assert!(move_copies(&[(from.join("g"), to.join("g"))]).is_err(), "a different backup must not be replaced or mistaken for a duplicate");
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
        assert_eq!(move_copies(&[(from.join("g"), to.join("g"))]).unwrap(), 1);
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
        assert!(move_copies(&[(from.join("g"), to.join("g"))]).is_err());
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
        assert_eq!(move_copies(&[(d.join("g"), d.join(".").join("g"))]).unwrap(), 0);
        assert_eq!(fs::read(d.join("g/123.zip")).unwrap(), b"our only backup");
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn moving_copies_rejects_a_nested_destination() {
        let d = tmp("move-nested");
        fs::create_dir_all(d.join("g")).unwrap();
        fs::write(d.join("g/123.zip"), b"original backup").unwrap();
        assert!(move_copies(&[(d.join("g"), d.join("g").join("nested"))]).is_err());
        assert_eq!(fs::read(d.join("g/123.zip")).unwrap(), b"original backup");
        assert!(!d.join("g/nested/123.zip").exists());
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
        assert!(move_copies(&[(from.join("g"), to.join("g"))]).is_err());
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
        let archive = dir_of(&d.join("source-app"), "g", &from).join(copy.name);
        let target_app = d.join("target-app");

        assert_eq!(restore(&target_app, "g", &archive, &target_spots, &to).unwrap(), 1);
        assert_eq!(fs::read_to_string(target_save.join("slot1/progress.sav")).unwrap(), "cloud progress");
        assert!(!target.join("profile/save/source-id").exists());
        assert!(!target_save.join("previous.sav").exists());
        assert_eq!(current(&target_spots, &to).2, copy.hash);
        let kept = list(&target_app, "g", &to);
        assert_eq!(kept.len(), 1);
        let mut kept_zip = zip::ZipArchive::new(fs::File::open(dir_of(&target_app, "g", &to).join(&kept[0].name)).unwrap()).unwrap();
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
        let archive = dir_of(&d.join("source-app"), "g", &from).join(copy.name);

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
        let archive = dir_of(&d.join("app"), "g", &g).join(copy.name);

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
        let archive = dir_of(&d.join("app"), "g", &g).join(copy.name);
        let target_spots = vec![source_spots[1].clone(), source_spots[0].clone()];

        assert!(restore(&d.join("app"), "g", &archive, &target_spots, &g).is_err());
        assert_eq!(fs::read_to_string(g.path.join("one/progress.sav")).unwrap(), "new one");
        assert_eq!(fs::read_to_string(g.path.join("two/progress.sav")).unwrap(), "new two");
        assert_eq!(list(&d.join("app"), "g", &g).len(), 1);
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
        let archive = dir_of(&d.join("source-app"), "g", &from).join(copy.name);

        assert_eq!(restore(&d.join("target-app"), "g", &archive, &target_spots, &to).unwrap(), 2);
        assert_eq!(fs::read_to_string(to.path.join("first/progress.sav")).unwrap(), "first game");
        assert_eq!(fs::read_to_string(to.path.join("second/progress.sav")).unwrap(), "second game");
        assert_eq!(current(&target_spots, &to).2, copy.hash);
        assert!(!to.path.join("one").exists());
        assert!(!to.path.join("two").exists());
    }

    /// A copy of `{install}/old/progress.sav`, made while the developer's file named that folder.
    fn copy_from_old_folder(d: &Path) -> (Installed, PathBuf) {
        let g = game(&d.join("game"));
        fs::create_dir_all(g.path.join("old")).unwrap();
        fs::write(g.path.join("old/progress.sav"), "progress").unwrap();
        let old = spots(&["{install}/old".into()], &[], &g);
        let copy = backup(&d.join("app"), "g", &old, &g).unwrap().unwrap();
        fs::remove_dir_all(g.path.join("old")).unwrap();
        let archive = dir_of(&d.join("app"), "g", &g).join(copy.name);
        (g, archive)
    }

    #[test]
    fn restore_follows_a_save_folder_the_developer_moved() {
        let d = tmp("restore-moved");
        let (g, archive) = copy_from_old_folder(&d);
        let now = spots(&["{install}/new".into()], &[], &g);

        assert_eq!(restore(&d.join("app"), "g", &archive, &now, &g).unwrap(), 1);
        assert_eq!(fs::read_to_string(g.path.join("new/progress.sav")).unwrap(), "progress");
        assert!(!g.path.join("old").exists());
    }

    /// A copy of `{install}/sub/saves`, whose folder can't be made anymore: `sub` is a file now,
    /// which stops it the way a read-only drive does.
    fn copy_into_unwritable_folder(d: &Path) -> (Installed, PathBuf, Vec<Spot>) {
        let g = game(&d.join("game"));
        fs::create_dir_all(g.path.join("sub/saves")).unwrap();
        fs::write(g.path.join("sub/saves/progress.sav"), "progress").unwrap();
        let place = spots(&["{install}/sub/saves".into()], &[], &g);
        let copy = backup(&d.join("app"), "g", &place, &g).unwrap().unwrap();
        fs::remove_dir_all(g.path.join("sub")).unwrap();
        fs::write(g.path.join("sub"), "not a folder").unwrap();
        let archive = dir_of(&d.join("app"), "g", &g).join(copy.name);
        (g, archive, place)
    }

    #[test]
    fn restore_that_cant_write_says_so_first() {
        let d = tmp("restore-unwritable-first");
        let (g, archive, place) = copy_into_unwritable_folder(&d);
        let why = restore(&d.join("app"), "g", &archive, &place, &g).unwrap_err().to_string();
        assert_eq!(why.lines().next(), Some(SAVE_NOT_WRITTEN));
    }

    #[test]
    fn restore_that_cant_write_names_the_folder() {
        let d = tmp("restore-unwritable-folder");
        let (g, archive, place) = copy_into_unwritable_folder(&d);
        let why = restore(&d.join("app"), "g", &archive, &place, &g).unwrap_err().to_string();
        let second = why.lines().nth(1).unwrap_or_default();
        assert!(second.starts_with(&g.path.display().to_string()), "{why}");
        assert!(second.contains("saves"), "{why}");
    }

    #[test]
    fn restore_keeps_the_copys_folder_when_the_folder_count_changed() {
        let d = tmp("restore-moved-count");
        let (g, archive) = copy_from_old_folder(&d);
        let now = spots(&["{install}/new".into(), "{install}/more".into()], &[], &g);

        assert_eq!(restore(&d.join("app"), "g", &archive, &now, &g).unwrap(), 1);
        assert_eq!(fs::read_to_string(g.path.join("old/progress.sav")).unwrap(), "progress");
        assert!(!g.path.join("new").exists());
    }

    #[test]
    fn restore_keeps_the_copys_folders_when_they_were_only_reordered() {
        let d = tmp("restore-moved-order");
        let g = game(&d.join("game"));
        fs::create_dir_all(g.path.join("one")).unwrap();
        fs::create_dir_all(g.path.join("two")).unwrap();
        fs::write(g.path.join("one/progress.sav"), "one").unwrap();
        fs::write(g.path.join("two/progress.sav"), "two").unwrap();
        let before = spots(&["{install}/one".into(), "{install}/two".into()], &[], &g);
        let copy = backup(&d.join("app"), "g", &before, &g).unwrap().unwrap();
        let archive = dir_of(&d.join("app"), "g", &g).join(copy.name);
        fs::write(g.path.join("one/progress.sav"), "new one").unwrap();
        let now = spots(&["{install}/two".into(), "{install}/one".into()], &[], &g);

        assert_eq!(restore(&d.join("app"), "g", &archive, &now, &g).unwrap(), 2);
        assert_eq!(fs::read_to_string(g.path.join("one/progress.sav")).unwrap(), "one");
        assert_eq!(fs::read_to_string(g.path.join("two/progress.sav")).unwrap(), "two");
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
        assert_eq!(list(&app, "g", &g).len(), KEEP);
        restore(&app, "g", &dir_of(&app, "g", &g).join(&list(&app, "g", &g).last().unwrap().name), &s, &g).unwrap();
        assert_eq!(fs::read_to_string(inst.join("s.sav")).unwrap(), "v3");
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

#[cfg(test)]
mod sync_state_tests {
    use super::{Problem, State};

    fn problem(kind: &str) -> Option<Problem> {
        Some(Problem { kind: kind.into(), text: String::new(), at: 1 })
    }

    #[test]
    fn a_session_not_copied_yet_is_waiting_not_up_to_date() {
        let s = State { last_hash: "a".into(), synced_hash: "a".into(), after_play: true, ..Default::default() };
        assert_eq!(s.sync_state(true, true), "waiting");
    }

    #[test]
    fn a_first_session_not_copied_yet_is_waiting_not_none() {
        let s = State { after_play: true, ..Default::default() };
        assert_eq!(s.sync_state(true, true), "waiting");
    }

    #[test]
    fn a_session_not_copied_yet_is_left_out_of_the_file_when_there_is_none() {
        let s = State { last_hash: "a".into(), ..Default::default() };
        assert!(!serde_json::to_string(&s).unwrap().contains("after_play"));
    }

    #[test]
    fn signed_out_comes_first() {
        let s = State { last_hash: "a".into(), synced_hash: "a".into(), ..Default::default() };
        assert_eq!(s.sync_state(true, false), "signed-out");
    }

    #[test]
    fn cloud_saves_off_is_off() {
        let s = State { last_hash: "a".into(), ..Default::default() };
        assert_eq!(s.sync_state(false, true), "off");
    }

    #[test]
    fn nothing_backed_up_or_synced_is_none() {
        assert_eq!(State::default().sync_state(true, true), "none");
    }

    #[test]
    fn the_same_save_here_and_in_the_cloud_is_up_to_date() {
        let s = State { last_hash: "a".into(), synced_hash: "a".into(), ..Default::default() };
        assert_eq!(s.sync_state(true, true), "up-to-date");
    }

    #[test]
    fn a_newer_copy_here_is_waiting() {
        let s = State { last_hash: "b".into(), synced_hash: "a".into(), ..Default::default() };
        assert_eq!(s.sync_state(true, true), "waiting");
    }

    #[test]
    fn a_queued_copy_is_waiting() {
        let s = State { last_hash: "a".into(), synced_hash: "a".into(), pending: Some("1.zip".into()), ..Default::default() };
        assert_eq!(s.sync_state(true, true), "waiting");
    }

    #[test]
    fn no_connection_is_still_only_waiting() {
        let s = State { last_hash: "b".into(), synced_hash: "a".into(), problem: problem("offline"), ..Default::default() };
        assert_eq!(s.sync_state(true, true), "waiting");
    }

    #[test]
    fn a_copy_too_big_to_send_is_a_problem_not_waiting() {
        // Its upload clears pending without moving synced_hash; without the stored problem the
        // page would say it's waiting to go up for good.
        let s = State { last_hash: "big".into(), synced_hash: "a".into(), problem: problem("too-big-copy"), ..Default::default() };
        assert_eq!(s.sync_state(true, true), "problem");
    }

    #[test]
    fn a_folder_too_big_to_read_is_a_problem_even_when_nothing_waits() {
        let s = State { last_hash: "a".into(), synced_hash: "a".into(), problem: problem("too-big-folder"), ..Default::default() };
        assert_eq!(s.sync_state(true, true), "problem");
    }

    #[test]
    fn a_problem_left_from_before_an_upload_does_not_show_once_synced() {
        let s = State { last_hash: "a".into(), synced_hash: "a".into(), problem: problem("full"), ..Default::default() };
        assert_eq!(s.sync_state(true, true), "up-to-date");
    }

    #[test]
    fn a_conflict_shows_over_everything_but_off_and_signed_out() {
        let s = State { last_hash: "b".into(), synced_hash: "a".into(), conflict: Some("v9".into()), problem: problem("full"), ..Default::default() };
        assert_eq!(s.sync_state(true, true), "conflict");
    }

    #[test]
    fn syncing_clears_a_conflict_and_a_problem() {
        let mut s = State { conflict: Some("v9".into()), problem: problem("full"), ..Default::default() };
        s.mark_synced("a", 100, "Steam Deck");
        assert!(s.conflict.is_none());
        assert!(s.problem.is_none());
    }

    #[test]
    fn syncing_remembers_when_and_where_the_save_was_made() {
        let mut s = State::default();
        s.mark_synced("a", 100, "Steam Deck");
        assert_eq!((s.synced_hash.as_str(), s.synced_made, s.synced_device.as_str()), ("a", 100, "Steam Deck"));
        assert!(s.synced_at > 0);
    }

    #[test]
    fn a_state_file_from_before_reads_with_the_new_fields_empty() {
        let s: State = serde_json::from_str(r#"{"last_hash":"a","synced_hash":"a","pending":null}"#).unwrap();
        assert_eq!((s.synced_at, s.conflict.is_none(), s.problem.is_none()), (0, true, true));
    }
}

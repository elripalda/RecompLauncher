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
    ".iso", ".bin", ".cue", ".chd", ".cso", ".gcm", ".rvz", ".wbfs", ".wad", ".nsp", ".xci", ".xex", ".xbe", ".z64", ".n64", ".v64",
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
    if let Some(t) = g.save_place.as_ref().filter(|t| !t.trim().is_empty()) {
        return vec![Spot { template: t.trim().to_string(), from: "player" }];
    }
    let list = |v: &[String], from: &'static str| v.iter().filter(|t| !t.trim().is_empty()).map(|t| Spot { template: t.trim().to_string(), from }).collect::<Vec<_>>();
    for (v, from) in [(dev, "developer"), (catalog, "catalog"), (&g.found_saves[..], "found")] {
        let s = list(v, from);
        if !s.is_empty() {
            return s;
        }
    }
    vec![Spot { template: "{install}".into(), from: "game" }]
}

/// A template on this PC.
pub fn resolve(template: &str, install: &Path) -> PathBuf {
    let t = crate::profile::fill(template, install, None);
    PathBuf::from(if cfg!(windows) { t.replace('/', "\\") } else { t })
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

fn is_save_file(name: &str, size: u64) -> bool {
    let n = name.to_lowercase();
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
        let p = resolve(&s.template, &g.path);
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

pub fn dir_of(app_dir: &Path, id: &str) -> PathBuf {
    app_dir.join("saves").join(id)
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

pub fn state(app_dir: &Path, id: &str) -> State {
    fs::read(dir_of(app_dir, id).join("state.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn set_state(app_dir: &Path, id: &str, s: &State) {
    let d = dir_of(app_dir, id);
    let _ = fs::create_dir_all(&d);
    let _ = fs::write(d.join("state.json"), serde_json::to_vec_pretty(s).unwrap_or_default());
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
    let roots: Vec<PathBuf> = man.spots.iter().map(|t| resolve(t, &g.path)).collect();
    let wanted: HashSet<(usize, String)> = man.files.iter().map(|f| (f.spot, f.rel.clone())).collect();
    // Saves in the same places that the copy doesn't have: only once what's there now is safely
    // kept, and never in the game's own folder as a whole, where other files live too.
    if kept {
        let same: Vec<Spot> = man.spots.iter().map(|t| Spot { template: t.clone(), from: "copy" }).collect();
        for e in collect(&same, g) {
            let whole_folder = man.spots.get(e.spot).map(|t| t.trim_end_matches(['/', '\\']) == "{install}").unwrap_or(false);
            if !whole_folder && !wanted.contains(&(e.spot, e.rel.clone())) {
                let _ = fs::remove_file(&e.abs);
            }
        }
    }
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
        // Only plain names: nothing that climbs out, or that Windows reads as a drive or a root.
        if rel.split('/').any(|c| c == ".." || c.is_empty() || c.contains('\\') || c.contains(':')) {
            continue;
        }
        // A file place (`{install}/game.sav`) is the file itself; a pattern's files sit beside it.
        let t = &man.spots[spot];
        let last = Path::new(t).file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
        let dest = if last.contains('*') {
            root.parent().unwrap_or(root).join(rel)
        } else if rel == last && !root.is_dir() {
            root.clone()
        } else {
            root.join(rel)
        };
        if let Some(d) = dest.parent() {
            fs::create_dir_all(d)?;
        }
        let tmp = dest.with_extension("rl-part");
        io::copy(&mut e, &mut fs::File::create(&tmp)?)?;
        fs::rename(&tmp, &dest)?;
        n += 1;
    }
    let mut st = state(app_dir, id);
    st.last_hash = man.hash.clone();
    set_state(app_dir, id, &st);
    Ok(n)
}

pub fn manifest(zip_file: &Path) -> Option<Manifest> {
    read_manifest(zip_file)
}

// ---------- noticing where a game saves ----------

/// The files in the likely save places when a game started.
pub struct Watch {
    roots: Vec<PathBuf>,
    install: PathBuf,
    before: HashMap<PathBuf, (u64, u64)>,
    release: HashSet<String>,
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
    let home = dirs::home_dir();
    let docs = dirs::document_dir().or_else(|| home.as_ref().map(|h| h.join("Documents")));
    let mut bases: Vec<PathBuf> = vec![];
    for b in [
        docs.clone(),
        docs.as_ref().map(|d| d.join("My Games")),
        home.as_ref().map(|h| h.join("Saved Games")),
        dirs::config_dir(),
        dirs::data_local_dir(),
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
    Watch { roots, install: g.path.clone(), before, release }
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
                Some((first, _)) => format!("{{install}}/{first}"),
                None => format!("{{install}}/{rel_s}"),
            }
        } else {
            templatize(root, &w.install)
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
    for e in fs::read_dir(app_dir.join("saves")).into_iter().flatten().flatten() {
        let id = e.file_name().to_string_lossy().to_string();
        out.insert(id.clone(), list(app_dir, &id).iter().map(|c| c.size).sum());
    }
    out
}

#[cfg(test)]
mod tests {
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

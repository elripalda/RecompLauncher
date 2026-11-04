//! Finding the player's own games in their ROM and backup folders, and which ones have a recomp.
//!
//! N64 ROMs carry the game's name in their header, so those are matched on it (whatever the file
//! is called). Extracted PlayStation games are identified by their local metadata. Other disc images
//! and game folders are matched on their file or folder name. Nothing leaves the PC.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RomFile {
    pub path: PathBuf,
    /// n64, x360, gc, wii, ps4 or "" when the kind of file doesn't say.
    pub system: String,
    /// The title from the game's metadata, or the file's name.
    pub title: String,
    /// US, Europe, Japan or "".
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub size: u64,
}

const N64: [&str; 3] = ["z64", "n64", "v64"];
const DISC: [&str; 6] = ["iso", "gcm", "rvz", "wbfs", "ciso", "chd"];
/// Cartridge ROMs and game files of the other consoles recomps come from (NES, Game Boy, DS, 3DS,
/// Switch, Virtual Boy, Genesis, 32X, Game Gear, Master System, SNES, Amiga). Matched to a recomp by
/// name; which console is left open, as their ids in the catalog vary.
const CART: [&str; 27] = ["nes", "fds", "unf", "gb", "gbc", "gba", "nds", "3ds", "cci", "cia", "cxi", "vb", "vboy", "md", "gen", "smd", "32x", "gg", "sms", "sfc", "smc", "adf", "hdf", "ipf", "lha", "xci", "nsp"];

fn n64_header(p: &Path) -> Option<(String, String)> {
    let mut f = fs::File::open(p).ok()?;
    let mut h = [0u8; 64];
    f.read_exact(&mut h).ok()?;
    // The three byte orders N64 dumps come in, told apart by their first word.
    match [h[0], h[1], h[2], h[3]] {
        [0x80, 0x37, 0x12, 0x40] => {}
        [0x37, 0x80, 0x40, 0x12] => {
            for c in h.chunks_mut(2) {
                c.swap(0, 1);
            }
        }
        [0x40, 0x12, 0x37, 0x80] => {
            for c in h.chunks_mut(4) {
                c.reverse();
            }
        }
        _ => return None,
    }
    let title: String = h[0x20..0x34].iter().map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { ' ' }).collect();
    let region = match h[0x3E] {
        b'E' | b'N' => "US",
        b'P' | b'D' | b'F' | b'I' | b'S' | b'U' | b'X' | b'Y' => "Europe",
        b'J' => "Japan",
        _ => "",
    };
    Some((title.trim().to_string(), region.to_string()))
}

fn system_for(ext: &str) -> &'static str {
    match ext {
        "z64" | "n64" | "v64" => "n64",
        "gcm" | "rvz" => "gc",
        "wbfs" => "wii",
        "xex" => "x360",
        // Cartridges say by their file type which console they're for.
        "nes" | "fds" | "unf" => "nes",
        "gb" => "gb",
        "gbc" => "gbc",
        "gba" => "gba",
        "nds" => "ds",
        "3ds" | "cci" | "cia" | "cxi" => "3ds",
        "vb" | "vboy" => "vb",
        "md" | "gen" | "smd" => "genesis",
        "32x" => "s32x",
        "gg" => "gg",
        "sms" => "sms",
        "sfc" | "smc" => "snes",
        "adf" | "hdf" | "ipf" | "lha" => "amiga",
        "xci" | "nsp" => "switch",
        _ => "",
    }
}

/// Consoles whose games come on cartridges (or as cartridge files): a disc image is never one of theirs.
const CART_ORIGINS: [&str; 13] = ["n64", "snes", "nes", "gb", "gbc", "gba", "ds", "3ds", "vb", "genesis", "s32x", "switch", "amiga"];

/// Can this file be a game for that console? The same console, or one that plays its games
/// (Game Boy and Game Boy Color), or a disc image of unknown kind for a console that uses discs.
fn fits(rom: &RomFile, origin: &str) -> bool {
    if matches!(rom.system.as_str(), "ps3" | "ps5" | "switch2") {
        return origin == rom.system.as_str();
    }
    if origin.is_empty() || origin == "other" || origin == "arcade" {
        return true;
    }
    // Files found before the launcher told cartridges apart: their type says it now.
    let ext = rom.path.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
    let sys = if rom.system.is_empty() { system_for(&ext) } else { rom.system.as_str() };
    if sys.is_empty() {
        return !CART_ORIGINS.contains(&origin);
    }
    sys == origin || matches!((sys, origin), ("gb", "gbc") | ("gbc", "gb"))
}

/// One file: a ROM, a disc image, or an Xbox 360 game's default.xex (which stands for its folder).
pub fn identify(p: &Path, size: u64) -> Option<RomFile> {
    identify_with_size(p, || Some(size))
}

fn identify_with_size(p: &Path, size: impl FnOnce() -> Option<u64>) -> Option<RomFile> {
    let ext = p.extension()?.to_str()?.to_ascii_lowercase();
    let is_n64 = N64.contains(&ext.as_str());
    let is_disc = DISC.contains(&ext.as_str());
    let is_cart = CART.contains(&ext.as_str());
    if !is_n64 && !is_disc && !is_cart && ext != "xex" {
        return None;
    }
    let size = size()?;
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
    if is_n64 {
        let (title, region) = n64_header(p).unwrap_or((stem.clone(), String::new()));
        Some(RomFile { path: p.to_path_buf(), system: "n64".into(), title, region, size })
    } else if is_disc && size > 50 * 1024 * 1024 {
        Some(RomFile { path: p.to_path_buf(), system: system_for(&ext).into(), title: stem, region: String::new(), size })
    } else if is_cart && size >= 8 * 1024 {
        Some(RomFile { path: p.to_path_buf(), system: system_for(&ext).into(), title: stem, region: String::new(), size })
    } else if ext == "xex" && stem.eq_ignore_ascii_case("default") {
        // An extracted Xbox 360 game: the folder's name is the game's.
        let folder = p.parent()?;
        let title = folder.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
        Some(RomFile { path: folder.to_path_buf(), system: "x360".into(), title, region: String::new(), size: 0 })
    } else {
        None
    }
}

/// Read the title and title ID from the little-endian PSF index and string tables.
fn sfo_title(sfo: &[u8]) -> Option<(&str, &str)> {
    let read_u32 = |offset: usize| Some(u32::from_le_bytes(sfo.get(offset..offset + 4)?.try_into().ok()?) as usize);
    if !sfo.starts_with(b"\0PSF") {
        return None;
    }
    let keys = read_u32(8)?;
    let values = read_u32(12)?;
    let index_end = read_u32(16)?.checked_mul(16)?.checked_add(20)?;
    if index_end > keys || keys > values || values > sfo.len() {
        return None;
    }
    let mut title = None;
    let mut title_id = None;
    for (i, entry) in sfo[20..index_end].chunks_exact(16).enumerate() {
        if u16::from_le_bytes([entry[2], entry[3]]) != 0x0204 {
            continue;
        }
        let key_offset = u16::from_le_bytes([entry[0], entry[1]]) as usize;
        let key = sfo.get(keys + key_offset..values)?.split(|&b| b == 0).next()?;
        if key != b"TITLE" && key != b"TITLE_ID" {
            continue;
        }
        let index = 20 + i * 16;
        let len = read_u32(index + 4)?;
        let start = values.checked_add(read_u32(index + 12)?)?;
        let value = sfo.get(start..start.checked_add(len)?)?.split(|&b| b == 0).next()?;
        let value = std::str::from_utf8(value).ok()?.trim();
        if key == b"TITLE" {
            title = Some(value);
        } else {
            title_id = Some(value);
        }
    }
    Some((title.filter(|t| !t.is_empty())?, title_id?))
}

fn identify_sfo(folder: &Path, path: &Path, system: &str, valid_id: impl FnOnce(&str) -> bool) -> Option<RomFile> {
    const MAX_SFO: u64 = 64 * 1024;
    let mut sfo = Vec::new();
    fs::File::open(path).ok()?.take(MAX_SFO + 1).read_to_end(&mut sfo).ok()?;
    if sfo.len() as u64 > MAX_SFO {
        return None;
    }
    let (title, id) = sfo_title(&sfo)?;
    if !valid_id(id) {
        return None;
    }
    Some(RomFile { path: folder.to_path_buf(), system: system.into(), title: title.into(), region: String::new(), size: 0 })
}

fn valid_playstation_id(id: &str) -> bool {
    id.len() == 9 && id.bytes().take(4).all(|b| b.is_ascii_uppercase())
        && id.bytes().skip(4).all(|b| b.is_ascii_digit())
}

/// Resolve each component exactly when possible. A folded name must be unique.
fn game_path(folder: &Path, relative: &str) -> Option<PathBuf> {
    let mut path = folder.to_path_buf();
    for name in relative.split('/') {
        let exact = path.join(name);
        match fs::symlink_metadata(&exact) {
            Ok(_) => path = exact,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let mut matches = fs::read_dir(&path).ok()?.flatten().filter(|entry| {
                    entry.file_name().to_str().is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))
                });
                path = matches.next()?.path();
                if matches.next().is_some() {
                    return None;
                }
            }
            Err(_) => return None,
        }
    }
    Some(path)
}

fn identify_ps5(folder: &Path) -> Option<RomFile> {
    fn title_of(entry: &Value) -> Option<&str> {
        entry.get("titleName")?.as_str().map(str::trim).filter(|title| !title.is_empty())
    }

    const MAX_PARAM: u64 = 64 * 1024;
    let mut data = Vec::new();
    fs::File::open(game_path(folder, "sce_sys/param.json")?).ok()?
        .take(MAX_PARAM + 1).read_to_end(&mut data).ok()?;
    if data.len() as u64 > MAX_PARAM {
        return None;
    }
    let param: Value = serde_json::from_slice(&data).ok()?;
    let digits = param.get("titleId")?.as_str()?.strip_prefix("PPSA")?;
    let digits = digits.strip_prefix('-').unwrap_or(digits);
    if digits.len() != 5 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let localized = param.get("localizedParameters")?.as_object()?;
    let preferred = localized.get("defaultLanguage").and_then(Value::as_str);
    let title = preferred.and_then(|lang| localized.get(lang)).and_then(title_of)
        .or_else(|| localized.get("en-US").and_then(title_of))
        .or_else(|| localized.values().find_map(title_of))?;
    Some(RomFile { path: folder.to_path_buf(), system: "ps5".into(), title: title.into(), region: String::new(), size: 0 })
}

fn identify_playstation(folder: &Path) -> Option<RomFile> {
    let has_file = |relative| game_path(folder, relative).is_some_and(|path| path.is_file());
    if has_file("eboot.bin") {
        let ps4 = game_path(folder, "sce_sys/param.sfo").and_then(|path| {
            identify_sfo(folder, &path, "ps4", |id| id.starts_with("CUSA") && valid_playstation_id(id))
        });
        if let Some(game) = ps4.or_else(|| identify_ps5(folder)) {
            return Some(game);
        }
    }
    if has_file("PS3_DISC.SFB") && has_file("PS3_GAME/USRDIR/EBOOT.BIN") {
        return identify_sfo(folder, &game_path(folder, "PS3_GAME/PARAM.SFO")?, "ps3", |id| {
            (id.starts_with("BC") || id.starts_with("BL")) && valid_playstation_id(id)
        });
    }
    None
}

fn identify_switch(folder: &Path) -> Option<RomFile> {
    // Switch and Switch 2 share these file signatures. Use the library's platform
    // folder as the platform hint, and require the extracted base game layout.
    let (depth, system) = folder.ancestors().skip(1).enumerate().find_map(|(depth, parent)| {
        let name = parent.file_name()?.to_str()?;
        if name.eq_ignore_ascii_case("switch2") || name.eq_ignore_ascii_case("switch 2") {
            Some((depth, "switch2"))
        } else if name.eq_ignore_ascii_case("switch") {
            Some((depth, "switch"))
        } else {
            None
        }
    })?;
    let valid_exefs = |relative| {
        let Some(exefs) = game_path(folder, relative) else { return false };
        [("main.npdm", b"META"), ("main", b"NSO0")].iter().all(|(name, magic)| {
            let Some(path) = game_path(&exefs, name) else { return false };
            let mut header = [0; 4];
            fs::File::open(path).and_then(|mut file| file.read_exact(&mut header)).is_ok()
                && &header == *magic
        })
    };
    // A flat exefs is accepted directly under the console folder to avoid
    // treating an update inside a collection as a standalone game.
    if !valid_exefs("base/exefs") && !(depth == 0 && valid_exefs("exefs")) {
        return None;
    }
    let title = folder.file_name()?.to_str()?.to_string();
    Some(RomFile { path: folder.to_path_buf(), system: system.into(), title, region: String::new(), size: 0 })
}

/// Look through the folders for ROMs, disc images and extracted games.
/// Linked folders are looked through once (a link back into a folder already seen is skipped), so
/// the scan can't go round in circles, and a whole drive of millions of files still ends.
pub fn scan(dirs: &[PathBuf]) -> Vec<RomFile> {
    const MOST: usize = 1_000_000;
    let mut out = vec![];
    let mut seen = 0usize;
    let mut been: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    let mut pending: Vec<_> = dirs.iter().rev().cloned().collect();
    while let Some(dir) = pending.pop() {
        if !been.insert(fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone())) {
            continue;
        }
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        let entries: Vec<fs::DirEntry> = rd.flatten().collect();
        // An extracted game is one entry, and what's inside it isn't looked through. Only a folder
        // with the files that mark one is checked, so the rest are read once.
        let marked = entries.iter().any(|e| {
            let n = e.file_name();
            let n = n.to_string_lossy();
            ["eboot.bin", "ps3_disc.sfb", "base", "exefs"].iter().any(|m| n.eq_ignore_ascii_case(m))
        });
        if marked {
            if let Some(game) = identify_playstation(&dir).or_else(|| identify_switch(&dir)) {
                out.push(game);
                continue;
            }
        }
        for e in entries {
            seen += 1;
            if seen > MOST {
                return out;
            }
            let Ok(mut kind) = e.file_type() else { continue };
            // A link counts as what it points to.
            let linked = if kind.is_symlink() { fs::metadata(e.path()).ok() } else { None };
            if let Some(m) = &linked {
                kind = m.file_type();
            }
            if kind.is_dir() {
                let name = e.file_name().to_string_lossy().to_lowercase();
                if name.starts_with('.') || name == "node_modules" || name == "windows" {
                    continue;
                }
                pending.push(e.path());
                continue;
            }
            if !kind.is_file() {
                continue;
            }
            if let Some(r) = identify_with_size(&e.path(), || linked.as_ref().map(|m| m.len()).or_else(|| e.metadata().ok().map(|meta| meta.len()))) {
                out.push(r);
            }
        }
    }
    out
}

/// The words in a name, lower case, with the noise taken out: "Zelda - Majora's Mask (USA).z64"
/// becomes zelda, majoras, mask.
pub fn words(s: &str) -> Vec<String> {
    let s = s.to_lowercase().replace('\'', "").replace('’', "");
    // Region and dump tags in brackets say nothing about which game it is.
    let mut clean = String::new();
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = (depth as i32 - 1).max(0) as usize,
            _ if depth == 0 => clean.push(c),
            _ => {}
        }
    }
    const NOISE: [&str; 9] = ["the", "of", "a", "and", "usa", "us", "europe", "rev", "v1"];
    clean
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty() && !NOISE.contains(w))
        .map(str::to_string)
        .collect()
}

/// Does this file look like `game`? Every word of the game's name has to be in the file's.
fn matches(game: &str, rom: &RomFile) -> bool {
    let want = words(game);
    if want.is_empty() {
        return false;
    }
    let have = words(&rom.title);
    let file = rom.path.file_stem().and_then(|s| s.to_str()).map(words).unwrap_or_default();
    let hit = |w: &String, h: &String| h == w || (w.len() > 4 && h.starts_with(w.as_str()) && h.len() - w.len() <= 2);
    // Every word of the game's name is in the file's, and at least half of the file's words are
    // the game's: "Tony Hawk's Pro Skater 3" isn't "Skate 3", though it has both words in it.
    let all = |pool: &[String]| !pool.is_empty() && want.iter().all(|w| pool.iter().any(|h| hit(w, h))) && pool.iter().filter(|h| want.iter().any(|w| hit(w, h))).count() * 2 >= pool.len();
    all(&have) || all(&file)
}

/// For each catalog app, the player's file for its game, if they have one.
pub fn match_catalog(cat: &Value, roms: &[RomFile]) -> HashMap<String, RomFile> {
    let mut out = HashMap::new();
    let Some(apps) = cat.get("apps").and_then(Value::as_array) else { return out };
    for a in apps {
        let id = a.get("id").and_then(Value::as_str).unwrap_or("");
        let game = a.get("game").and_then(Value::as_str).unwrap_or("");
        let origin = a.get("origin").and_then(Value::as_str).unwrap_or("");
        if id.is_empty() || game.is_empty() {
            continue;
        }
        let pick = roms
            .iter()
            .filter(|r| fits(r, origin))
            .filter(|r| matches(game, r))
            // A US dump first: most recomps are built against it.
            .max_by_key(|r| (r.region == "US") as u8);
        if let Some(r) = pick {
            out.insert(id.to_string(), r.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rl-roms-{n}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn rom(order: &str, title: &str, region: u8) -> Vec<u8> {
        let mut h = vec![0u8; 4096];
        h[0..4].copy_from_slice(&[0x80, 0x37, 0x12, 0x40]);
        h[0x20..0x20 + title.len()].copy_from_slice(title.as_bytes());
        for b in &mut h[0x20 + title.len()..0x34] {
            *b = b' ';
        }
        h[0x3E] = region;
        match order {
            "v64" => h.chunks_mut(2).for_each(|c| c.swap(0, 1)),
            "n64" => h.chunks_mut(4).for_each(|c| c.reverse()),
            _ => {}
        }
        h
    }

    #[test]
    fn prefers_exact_game_paths_over_ambiguous_case_matches() {
        let root = tmp("case-match");
        let upper = root.join("PARAM.SFO");
        let lower = root.join("param.sfo");
        fs::write(&upper, []).unwrap();
        // Default Windows filesystems cannot represent both spellings.
        if fs::OpenOptions::new().write(true).create_new(true).open(&lower).is_ok() {
            assert_eq!(game_path(&root, "param.sfo"), Some(lower));
            assert_eq!(game_path(&root, "PARAM.SFO"), Some(upper));
            assert!(game_path(&root, "Param.sfo").is_none());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn treats_valid_extracted_switch_games_as_single_entries() {
        let root = tmp("switch2");
        let game = root.join("SWITCH2/Mario Kart World");
        let exefs = game.join("base/exefs");
        fs::create_dir_all(&exefs).unwrap();
        fs::write(exefs.join("main"), b"NSO0").unwrap();
        fs::write(exefs.join("main.npdm"), b"META").unwrap();
        fs::write(exefs.join("nested.gba"), [0; 8192]).unwrap();
        let switch = root.join("SWITCH/Example Game");
        let flat_exefs = switch.join("ExeFS");
        fs::create_dir_all(&flat_exefs).unwrap();
        fs::write(flat_exefs.join("main"), b"NSO0").unwrap();
        fs::write(flat_exefs.join("MAIN.NPDM"), b"META").unwrap();
        let sibling = root.join("Pokemon Emerald.gba");
        fs::write(&sibling, [0; 8192]).unwrap();

        let found = scan(&[root.clone()]);
        assert_eq!(found.len(), 3);
        let detected = found.iter().find(|r| r.path == game).unwrap();
        assert_eq!(detected.title, "Mario Kart World");
        assert!(fits(detected, "switch2"));
        assert!(!fits(detected, "switch"));
        assert!(found.iter().any(|r| r.path == switch && r.system == "switch"));
        assert!(found.iter().any(|r| r.path == sibling));

        fs::write(exefs.join("main.npdm"), b"bad!").unwrap();
        assert!(scan(&[root.clone()]).iter().any(|r| r.path == exefs.join("nested.gba")));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn linked_folders_and_files_are_looked_through_once() {
        let root = tmp("links");
        let real = root.join("gamecube");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("Game.gba"), [0; 8192]).unwrap();
        // EmuDeck-style second name for the same folder, a link back up (a loop), and a linked file.
        std::os::unix::fs::symlink(&real, root.join("gc")).unwrap();
        std::os::unix::fs::symlink(&root, real.join("up")).unwrap();
        std::os::unix::fs::symlink(real.join("Game.gba"), root.join("Linked.gba")).unwrap();
        let found = scan(&[root.clone()]);
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found.iter().any(|r| r.path.ends_with("Linked.gba") && r.size == 8192));
    }

    #[test]
    fn scans_deep_rom_folders() {
        let root = tmp("deep-scan");
        let folder = root.join("GBA/Collection/Backups/Regions/USA/Roleplaying/Pokemon");
        fs::create_dir_all(&folder).unwrap();
        let emerald = folder.join("Pokemon Emerald.gba");
        fs::write(&emerald, [0; 8192]).unwrap();

        let found = scan(&[root.clone()]);
        fs::remove_dir_all(root).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, emerald);
    }

    #[test]
    fn reads_all_three_byte_orders() {
        let d = tmp("orders");
        fs::write(d.join("a.z64"), rom("z64", "ZELDA MAJORA'S MASK", b'E')).unwrap();
        fs::write(d.join("b.v64"), rom("v64", "Banjo-Kazooie", b'P')).unwrap();
        fs::write(d.join("weird name.n64"), rom("n64", "SUPER MARIO 64", b'J')).unwrap();
        let mut r = scan(&[d]);
        r.sort_by(|x, y| x.title.cmp(&y.title));
        let t: Vec<(&str, &str)> = r.iter().map(|x| (x.title.as_str(), x.region.as_str())).collect();
        assert_eq!(t, vec![("Banjo-Kazooie", "Europe"), ("SUPER MARIO 64", "Japan"), ("ZELDA MAJORA'S MASK", "US")]);
    }

    #[test]
    fn keeps_consoles_and_whole_names_apart() {
        let f = |name: &str, ext: &str| RomFile { path: PathBuf::from(format!("{name}.{ext}")), system: system_for(ext).into(), title: name.into(), region: String::new(), size: 1 << 20 };
        // A Game Boy Advance game isn't the PlayStation one with the same start to its name.
        assert!(!fits(&f("Driver 2 Advanced", "gba"), "ps1"));
        assert!(fits(&f("Driver 2 Advanced", "gba"), "gba"));
        // A disc image isn't a cartridge game, and Game Boy and Game Boy Color go together.
        assert!(!fits(&f("Mario Kart 64", "iso"), "n64"));
        assert!(fits(&f("Link's Awakening DX", "gb"), "gbc"));
        // Every word of the game, and the file mostly the game's name.
        assert!(!matches("Skate 3", &f("Tony Hawk's Pro Skater 3", "gba")));
        assert!(matches("Skate 3", &f("Skate 3 (USA)", "iso")));
        assert!(matches("The Legend of Zelda: Majora's Mask", &f("Legend of Zelda, The - Majora's Mask (USA)", "z64")));
    }

    #[test]
    fn matches_games_to_recomps() {
        let d = tmp("match");
        fs::write(d.join("mm.z64"), rom("z64", "ZELDA MAJORA'S MASK", b'E')).unwrap();
        fs::write(d.join("Banjo-Kazooie (USA) (Rev 1).z64"), rom("z64", "Banjo-Kazooie", b'E')).unwrap();
        let x = d.join("Skate 3");
        fs::create_dir_all(&x).unwrap();
        fs::write(x.join("default.xex"), b"x").unwrap();
        let roms = scan(&[d]);
        let cat = serde_json::json!({"apps": [
            {"id": "zelda64recomp", "game": "Majora's Mask", "origin": "n64"},
            {"id": "banjo", "game": "Banjo-Kazooie", "origin": "n64"},
            {"id": "skate3", "game": "Skate 3", "origin": "x360"},
            {"id": "sm64", "game": "Super Mario 64", "origin": "n64"},
            {"id": "skate-wrong-system", "game": "Skate 3", "origin": "n64"}
        ]});
        let m = match_catalog(&cat, &roms);
        assert!(m.contains_key("zelda64recomp"));
        assert!(m.contains_key("banjo"));
        assert!(m.contains_key("skate3"));
        assert!(!m.contains_key("sm64"));
        assert!(!m.contains_key("skate-wrong-system"));
    }

    #[test]
    fn scans_extracted_ps5_games_by_param_json() {
        let root = tmp("ps5");
        let game = root.join("PS5/PPSA21559");
        fs::create_dir_all(game.join("sce_sys")).unwrap();
        fs::write(game.join("eboot.bin"), b"").unwrap();
        fs::write(game.join("assets.gba"), [0; 8192]).unwrap();
        let param = game.join("sce_sys/param.json");
        let mut metadata = serde_json::json!({
            "titleId": "PPSA21559",
            "localizedParameters": {
                "defaultLanguage": "en-US",
                "en-US": {"titleName": "ASTRO BOT"}
            }
        });
        fs::write(&param, metadata.to_string()).unwrap();
        let found = scan(&[root.clone()]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, game);
        assert_eq!(found[0].title, "ASTRO BOT");
        assert!(fits(&found[0], "ps5"));
        assert!(!fits(&found[0], "ps4"));

        metadata["titleId"] = serde_json::json!("CUSA21559");
        fs::write(&param, metadata.to_string()).unwrap();
        assert!(scan(&[root.clone()]).iter().all(|r| r.system != "ps5"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn scans_extracted_playstation_games_by_sfo_title() {
        let root = tmp("ps4");
        let game = root.join("PS4/The Order 1886/CUSA00035");
        fs::create_dir_all(game.join("sce_sys")).unwrap();
        fs::write(game.join("eboot.bin"), b"").unwrap();

        let keys = b"TITLE\0TITLE_ID\0";
        let title = b"The Order: 1886\0";
        let id = b"CUSA00035\0";
        let mut sfo = b"\0PSF".to_vec();
        for field in [0x101u32, 52, 52 + keys.len() as u32, 2] {
            sfo.extend(field.to_le_bytes());
        }
        for (key, length, offset) in [(0u16, title.len(), 0), (6, id.len(), title.len())] {
            sfo.extend(key.to_le_bytes());
            sfo.extend(0x0204u16.to_le_bytes());
            for field in [length, length, offset] {
                sfo.extend((field as u32).to_le_bytes());
            }
        }
        sfo.extend(keys);
        sfo.extend(title);
        sfo.extend(id);
        let metadata = game.join("sce_sys/param.sfo");
        fs::write(&metadata, &sfo).unwrap();

        let found = scan(&[root.clone()]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].system, "ps4");
        assert_eq!(found[0].title, "The Order: 1886");
        assert_eq!(found[0].path, game);
        assert_eq!(scan(&[game.clone()]), found);
        let catalog = serde_json::json!({"apps": [
            {"id": "order", "game": "The Order: 1886", "origin": "ps4"},
            {"id": "wrong-platform", "game": "The Order: 1886", "origin": "ps3"},
            {"id": "wrong-title", "game": "Bloodborne", "origin": "ps4"}
        ]});
        let matched = match_catalog(&catalog, &found);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched["order"].path, game);

        sfo.pop();
        fs::write(metadata, &sfo).unwrap();
        assert!(scan(&[root.clone()]).is_empty());

        let ps3 = root.join("PS3/Demons Souls");
        fs::create_dir_all(ps3.join("ps3_game/USRDIR")).unwrap();
        fs::write(ps3.join("PS3_DISC.SFB"), b"").unwrap();
        fs::write(ps3.join("ps3_game/USRDIR/eboot.bin"), b"").unwrap();
        sfo.extend(b"\0");
        let id_start = sfo.len() - id.len();
        sfo[id_start..id_start + 9].copy_from_slice(b"BLES00932");
        fs::write(ps3.join("ps3_game/param.sfo"), sfo).unwrap();
        let found = scan(&[root.clone()]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, ps3);
        assert_eq!(found[0].system, "ps3");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn noise_words_go() {
        assert_eq!(words("The Legend of Zelda - Majora's Mask (USA) [!]"), vec!["legend", "zelda", "majoras", "mask"]);
    }
}

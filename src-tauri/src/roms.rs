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
const CART: [&str; 28] = ["nes", "fds", "unf", "gb", "gbc", "gba", "nds", "3ds", "cci", "cia", "cxi", "vb", "vboy", "md", "gen", "smd", "32x", "gg", "sms", "sfc", "smc", "adf", "hdf", "ipf", "lha", "xci", "nsp", "nup"];

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
        // A .nup package is a Switch or Switch 2 dump; `fits` lets it serve either console.
        "nup" => "switch2",
        _ => "",
    }
}

fn read_at(f: &mut fs::File, at: u64, buf: &mut [u8]) -> std::io::Result<()> {
    use std::io::{Seek, SeekFrom};
    f.seek(SeekFrom::Start(at))?;
    f.read_exact(buf)
}

fn has_bytes(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Which console a disc image is for, from what's inside it: an Xbox or Xbox 360 disc (its XDVDFS
/// volume, and default.xbe or default.xex at its root), a GameCube or Wii disc (their magic
/// numbers), a PlayStation disc (ISO 9660 saying PLAYSTATION; its SYSTEM.CNF's BOOT2 line makes it a
/// PS2 one), a PSP disc, a Saturn or Dreamcast one. "xdvdfs" and "playstation" when it's one of
/// two, "" when it can't tell (a compressed image, a PC disc).
pub fn disc_kind(p: &Path) -> &'static str {
    let Ok(mut f) = fs::File::open(p) else { return "" };
    let mut head = [0u8; 0x40];
    if read_at(&mut f, 0, &mut head).is_err() {
        return "";
    }
    if head[0x18..0x1C] == [0x5D, 0x1C, 0x9E, 0xA3] {
        return "wii";
    }
    if head[0x1C..0x20] == [0xC2, 0x33, 0x9F, 0x3D] {
        return "gc";
    }
    for (magic, kind) in [(&b"SEGA SEGASATURN"[..], "saturn"), (&b"SEGA SEGAKATANA"[..], "dc")] {
        if head.starts_with(magic) || head[0x10..].starts_with(magic) {
            return kind;
        }
    }
    // Xbox and Xbox 360: the XDVDFS volume, right at the start of a trimmed image (xiso), or after
    // the video partition of a full one (where that partition ends says which console).
    for (at, kind) in [(0x1_0000u64, ""), (0x1831_0000, "xbox"), (0xFDA_0000, "x360"), (0x209_0000, "x360")] {
        let mut m = [0u8; 28];
        if read_at(&mut f, at, &mut m).is_err() || !m.starts_with(b"MICROSOFT*XBOX*MEDIA") {
            continue;
        }
        if !kind.is_empty() {
            return kind;
        }
        let sector = u32::from_le_bytes([m[20], m[21], m[22], m[23]]) as u64;
        let size = (u32::from_le_bytes([m[24], m[25], m[26], m[27]]) as usize).min(256 * 1024);
        let mut dir = vec![0u8; size];
        if size > 0 && read_at(&mut f, sector * 2048, &mut dir).is_ok() {
            let low = dir.to_ascii_lowercase();
            if has_bytes(&low, b"default.xex") {
                return "x360";
            }
            if has_bytes(&low, b"default.xbe") {
                return "xbox";
            }
        }
        return "xdvdfs";
    }
    match iso_kind(&mut f) {
        Iso::Psp => "psp",
        Iso::PlayStation(Some(cnf)) => {
            let up = cnf.to_ascii_uppercase();
            if has_bytes(&up, b"BOOT2") {
                "ps2"
            } else if has_bytes(&up, b"BOOT") {
                "ps1"
            } else {
                "playstation"
            }
        }
        Iso::PlayStation(None) => "playstation",
        Iso::Other => "",
    }
}

enum Iso {
    Psp,
    /// A PlayStation or PS2 disc, with the start of its SYSTEM.CNF when it has one.
    PlayStation(Option<[u8; 256]>),
    Other,
}

/// An ISO 9660 disc, in 2048-byte sectors or raw CD ones (2352 bytes, data after a 16 or 24-byte
/// header): a PSP one, or a PlayStation one with its SYSTEM.CNF ("BOOT2 = cdrom0:\..." on a PS2
/// disc, "BOOT = cdrom:\..." on a PlayStation one).
fn iso_kind(f: &mut fs::File) -> Iso {
    for (ss, off) in [(2048u64, 0u64), (2352, 24), (2352, 16)] {
        let mut pvd = [0u8; 190];
        if read_at(f, 16 * ss + off, &mut pvd).is_err() || &pvd[1..6] != b"CD001" {
            continue;
        }
        let sys = String::from_utf8_lossy(&pvd[8..40]).trim().to_ascii_uppercase();
        if sys.starts_with("PSP GAME") {
            return Iso::Psp;
        }
        if !sys.starts_with("PLAYSTATION") {
            return Iso::Other;
        }
        let root = &pvd[156..190];
        let lba = u32::from_le_bytes([root[2], root[3], root[4], root[5]]) as u64;
        let len = (u32::from_le_bytes([root[10], root[11], root[12], root[13]]) as u64).min(64 * 1024);
        for s in 0..len.div_ceil(2048) {
            let mut dir = [0u8; 2048];
            if read_at(f, (lba + s) * ss + off, &mut dir).is_err() {
                break;
            }
            let mut i = 0usize;
            while i + 33 < dir.len() && dir[i] > 0 {
                let rec = dir[i] as usize;
                let n = dir[i + 32] as usize;
                let name = &dir[(i + 33).min(2048)..(i + 33 + n).min(2048)];
                if name.to_ascii_uppercase().starts_with(b"SYSTEM.CNF") {
                    let at = u32::from_le_bytes([dir[i + 2], dir[i + 3], dir[i + 4], dir[i + 5]]) as u64;
                    let mut cnf = [0u8; 256];
                    return Iso::PlayStation(read_at(f, at * ss + off, &mut cnf).is_ok().then_some(cnf));
                }
                i += rec.max(1);
            }
        }
        return Iso::PlayStation(None);
    }
    Iso::Other
}

/// A disc or cartridge ID the way the catalog's "copy_ids" lists them: upper case, letters and
/// digits only ("SLUS-203.12" is "SLUS20312").
pub fn id_key(s: &str) -> String {
    s.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_uppercase()).collect()
}

/// The game's own ID, read off the player's copy: an N64 cartridge's game code ("NZSE"), a
/// GameCube or Wii disc's ("GZLE01", also inside a WBFS, CISO, WIA or RVZ image), a PlayStation or
/// PS2 disc's serial from its SYSTEM.CNF ("SLUS20312"). None when the file doesn't carry one this
/// reads (a CHD, a folder, another console).
pub fn copy_id(p: &Path) -> Option<String> {
    // An extracted Xbox 360 disc: its default.xex says the title.
    if p.is_dir() {
        return xex_title(&mut fs::File::open(p.join("default.xex")).ok()?, 0);
    }
    if !p.is_file() {
        return None;
    }
    let mut f = fs::File::open(p).ok()?;
    let mut head = [0u8; 0x60];
    read_at(&mut f, 0, &mut head).ok()?;
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if N64.contains(&ext.as_str()) || matches!([head[0], head[1], head[2], head[3]], [0x80, 0x37, 0x12, 0x40] | [0x37, 0x80, 0x40, 0x12] | [0x40, 0x12, 0x37, 0x80]) {
        let mut h = [0u8; 0x40];
        h.copy_from_slice(&head[..0x40]);
        match [h[0], h[1], h[2], h[3]] {
            [0x80, 0x37, 0x12, 0x40] => {}
            [0x37, 0x80, 0x40, 0x12] => h.chunks_mut(2).for_each(|c| c.swap(0, 1)),
            [0x40, 0x12, 0x37, 0x80] => h.chunks_mut(4).for_each(|c| c.reverse()),
            _ => return None,
        }
        return game_code(&h[0x3B..0x3F]);
    }
    // GameCube and Wii: the disc's header, at the start of a plain image, or where WBFS, CISO and
    // WIA/RVZ images keep a copy of it.
    let at = match &head[..4] {
        b"WBFS" => 0x200,
        b"CISO" => 0x8000,
        b"WIA\x01" | b"RVZ\x01" => 0x58,
        _ => 0,
    };
    let mut disc = [0u8; 0x20];
    if read_at(&mut f, at, &mut disc).is_ok() && (disc[0x18..0x1C] == [0x5D, 0x1C, 0x9E, 0xA3] || disc[0x1C..0x20] == [0xC2, 0x33, 0x9F, 0x3D]) {
        return game_code(&disc[..6]);
    }
    if let Iso::PlayStation(Some(cnf)) = iso_kind(&mut f) {
        return ps_serial(&cnf);
    }
    // Xbox 360: the title ID in default.xex, a picked one or the one on the disc image.
    if head.starts_with(b"XEX2") {
        return xex_title(&mut f, 0);
    }
    if let Some(at) = x360_default_xex(&mut f) {
        return xex_title(&mut f, at);
    }
    None
}

/// The title ID ("454107EC") in an Xbox 360 program's header (XEX2) starting at `at`: in its
/// execution info (optional header 0x00040006), after the media ID and two versions.
fn xex_title(f: &mut fs::File, at: u64) -> Option<String> {
    let be = |b: &[u8], i: usize| u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let mut head = [0u8; 0x18];
    read_at(f, at, &mut head).ok()?;
    if &head[..4] != b"XEX2" {
        return None;
    }
    let count = be(&head, 0x14).min(64) as usize;
    let mut opt = vec![0u8; count * 8];
    read_at(f, at + 0x18, &mut opt).ok()?;
    let info = opt.chunks(8).find(|c| be(c, 0) == 0x0004_0006).map(|c| be(c, 4) as u64)?;
    let mut e = [0u8; 16];
    read_at(f, at + info, &mut e).ok()?;
    let id = be(&e, 12);
    (id != 0).then(|| format!("{id:08X}"))
}

/// Where default.xex starts on an Xbox 360 disc image (its XDVDFS root folder), trimmed or full.
fn x360_default_xex(f: &mut fs::File) -> Option<u64> {
    for part in [0u64, 0x2080000, 0xFD90000] {
        let mut m = [0u8; 28];
        if read_at(f, part + 32 * 2048, &mut m).is_err() || !m.starts_with(b"MICROSOFT*XBOX*MEDIA") {
            continue;
        }
        let le = |i: usize| u32::from_le_bytes([m[i], m[i + 1], m[i + 2], m[i + 3]]) as u64;
        let (sector, size) = (le(20), le(24).min(256 * 1024) as usize);
        let mut t = vec![0u8; size];
        read_at(f, part + sector * 2048, &mut t).ok()?;
        let mut pos = 0usize;
        while pos + 14 <= t.len() {
            if t[pos] == 0xFF && t[pos + 1] == 0xFF {
                pos = (pos / 2048 + 1) * 2048;
                continue;
            }
            let nl = t[pos + 13] as usize;
            if nl == 0 || pos + 14 + nl > t.len() {
                break;
            }
            let start = u32::from_le_bytes([t[pos + 4], t[pos + 5], t[pos + 6], t[pos + 7]]) as u64;
            if t[pos + 14..pos + 14 + nl].eq_ignore_ascii_case(b"default.xex") && t[pos + 12] & 0x10 == 0 {
                return Some(part + start * 2048);
            }
            pos = (pos + 14 + nl + 3) & !3;
        }
        return None;
    }
    None
}

/// Letters and digits only, or None.
fn game_code(b: &[u8]) -> Option<String> {
    b.iter().all(u8::is_ascii_alphanumeric).then(|| String::from_utf8_lossy(b).to_ascii_uppercase())
}

/// The serial in SYSTEM.CNF's BOOT or BOOT2 line: "BOOT2 = cdrom0:\SLUS_203.12;1" is SLUS20312.
fn ps_serial(cnf: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(cnf);
    let line = text.lines().find(|l| l.trim_start().to_ascii_uppercase().starts_with("BOOT"))?;
    let file = line.split(['\\', ':', '/']).next_back()?.split(';').next()?.trim();
    let key = id_key(file);
    (key.len() >= 6 && key.len() <= 12 && key.chars().take(4).all(|c| c.is_ascii_alphabetic())).then_some(key)
}

/// Whether the player's copy is one this recomp is made for, by the catalog's "copy_ids" (a list,
/// or a single ID; a shorter one, like "GZLE", fits every disc that starts with it). Ok when the
/// catalog lists none or the copy's ID can't be read; otherwise the ID that was read.
pub fn fits_copy_ids(app: &Value, p: &Path) -> Result<(), String> {
    let want = copy_ids(app);
    if want.is_empty() {
        return Ok(());
    }
    match copy_id(p) {
        Some(id) if !want.iter().any(|w| id.starts_with(w.as_str())) => Err(id),
        _ => Ok(()),
    }
}

pub fn copy_ids(app: &Value) -> Vec<String> {
    let v = app.get("copy_ids");
    let list: Vec<&str> = match v {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect(),
        Some(Value::String(s)) => vec![s.as_str()],
        _ => vec![],
    };
    list.into_iter().map(id_key).filter(|k| k.len() >= 3).collect()
}

/// The console a folder the file sits in is named for ("PS2", "Sony - PlayStation 2", "xbox"): the
/// nearest such folder. "" when none is.
pub fn folder_kind(p: &Path) -> &'static str {
    const WORDS: [(&str, &str); 25] = [
        ("xbox 360", "x360"), ("xbox360", "x360"), ("x360", "x360"), ("original xbox", "xbox"), ("ogxbox", "xbox"), ("xbox", "xbox"),
        ("playstation 2", "ps2"), ("playstation2", "ps2"), ("ps2", "ps2"), ("playstation portable", "psp"), ("psp", "psp"),
        ("playstation 3", "ps3"), ("ps3", "ps3"), ("playstation 1", "ps1"), ("psx", "ps1"), ("ps1", "ps1"), ("psone", "ps1"), ("playstation", "playstation"),
        ("gamecube", "gc"), ("ngc", "gc"), ("gc", "gc"), ("wii u", "wiiu"), ("wiiu", "wiiu"), ("wii", "wii"), ("dreamcast", "dc"),
    ];
    let word = |text: &str, w: &str| {
        let b = text.as_bytes();
        text.match_indices(w).any(|(s, _)| {
            let e = s + w.len();
            (s == 0 || !b[s - 1].is_ascii_alphanumeric()) && (e >= b.len() || !b[e].is_ascii_alphanumeric())
        })
    };
    for dir in p.ancestors().skip(1) {
        let Some(n) = dir.file_name().and_then(|n| n.to_str()) else { continue };
        let n = n.to_lowercase().replace(['_', '-'], " ");
        if let Some((_, k)) = WORDS.iter().find(|(w, _)| word(&n, w)) {
            return *k;
        }
    }
    ""
}

/// A disc image's console: what's inside it says, and where it says one of two, the folder it's in
/// picks (an Xbox disc in a folder named "Xbox 360").
fn disc_system(p: &Path, ext: &str) -> &'static str {
    let inside = if ext == "iso" { disc_kind(p) } else { "" };
    let folder = folder_kind(p);
    match inside {
        "" => folder,
        "xdvdfs" if matches!(folder, "xbox" | "x360") => folder,
        "playstation" if matches!(folder, "ps1" | "ps2") => folder,
        k => k,
    }
}

/// Consoles whose games come on cartridges (or as cartridge files): a disc image is never one of theirs.
const CART_ORIGINS: [&str; 14] = ["n64", "snes", "nes", "gb", "gbc", "gba", "ds", "3ds", "vb", "genesis", "s32x", "switch", "switch2", "amiga"];

/// Can this file be a game for that console? The same console, or one that plays its games
/// (Game Boy and Game Boy Color), or a disc image of unknown kind for a console that uses discs.
fn fits(rom: &RomFile, origin: &str) -> bool {
    // A .nup package (NX Package Format) holds Switch or Switch 2 dumps, and its name doesn't say which.
    let is_nup = rom.path.extension().and_then(|s| s.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("nup"));
    if is_nup && rom.system == "switch2" {
        return matches!(origin, "switch" | "switch2");
    }
    if matches!(rom.system.as_str(), "ps3" | "ps5" | "switch2") {
        return origin == rom.system.as_str();
    }
    if origin.is_empty() || origin == "other" || origin == "arcade" {
        return true;
    }
    // Files found before the launcher told cartridges apart: their type says it now, or, for a
    // disc image, the folder it's kept in ("PS2", "Xbox").
    let ext = rom.path.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
    let sys = if !rom.system.is_empty() {
        rom.system.as_str()
    } else {
        match system_for(&ext) {
            "" if DISC.contains(&ext.as_str()) => folder_kind(&rom.path),
            s => s,
        }
    };
    match sys {
        "" => !CART_ORIGINS.contains(&origin),
        // One of two: an Xbox or Xbox 360 disc, a PlayStation or PS2 one.
        "xdvdfs" => matches!(origin, "xbox" | "x360"),
        "playstation" => matches!(origin, "ps1" | "ps2"),
        // A Switch 2 game card dumps to the same .xci/.nsp files as a Switch one.
        _ => sys == origin || matches!((sys, origin), ("gb", "gbc") | ("gbc", "gb") | ("switch", "switch2")),
    }
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
        // A disc image's file type rarely says its console (an .iso is any of them): what's inside
        // it, or the folder it's kept in, does.
        let system = match system_for(&ext) {
            "" => disc_system(p, &ext),
            s => s,
        };
        Some(RomFile { path: p.to_path_buf(), system: system.into(), title: stem, region: String::new(), size })
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
/// What a found file is to its game: the game itself (with its disc number in a set), an update,
/// or extra content. Read from its name: "(Disc 2)", "[UPD]", "Title Update", a Switch title ID
/// ending in 800 (an update) or another non-zero ending (extra content).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Part {
    pub kind: &'static str,
    pub disc: Option<u32>,
    /// "Disc 2 of 3": how many discs the set has, when the name says.
    pub of: Option<u32>,
}

pub fn part_of(name: &str) -> Part {
    let low = name.to_lowercase();
    let word = |w: &str| low.split(|c: char| !c.is_ascii_alphanumeric()).any(|x| x == w);
    // A Switch title ID: 16 hex digits starting 01. Base games end in 000, updates in 800.
    let tid = low
        .split(|c: char| !c.is_ascii_hexdigit())
        .find(|x| x.len() == 16 && x.starts_with("01"))
        .map(|x| x[13..].to_string());
    let kind = match tid.as_deref() {
        Some("800") => "update",
        Some(e) if e != "000" => "dlc",
        _ if word("upd") || word("update") || low.contains("title update") || low.starts_with("tu_") || word("patch") => "update",
        _ if word("dlc") || low.contains("add-on") || word("addon") => "dlc",
        _ => "game",
    };
    let mut disc = None;
    let mut of = None;
    if kind == "game" {
        for key in ["disc", "disk", "cd"] {
            let mut rest = low.as_str();
            while let Some(i) = rest.find(key) {
                let before = rest[..i].chars().last();
                let after = &rest[i + key.len()..];
                let num: String = after.trim_start_matches([' ', '_', '-']).chars().take_while(char::is_ascii_digit).collect();
                if before.map(|c| !c.is_ascii_alphanumeric()).unwrap_or(true) && !num.is_empty() {
                    disc = num.parse().ok();
                    let tail = after.trim_start_matches([' ', '_', '-']).trim_start_matches(|c: char| c.is_ascii_digit());
                    let tail = tail.trim_start();
                    if let Some(t) = tail.strip_prefix("of") {
                        of = t.trim_start().chars().take_while(char::is_ascii_digit).collect::<String>().parse().ok();
                    }
                    break;
                }
                rest = &rest[i + key.len()..];
            }
            if disc.is_some() {
                break;
            }
        }
    }
    Part { kind, disc, of }
}

/// Every found file that belongs to this catalog entry's game: its discs, updates and extra
/// content, by the same name match as `match_catalog`.
pub fn files_of(app: &Value, roms: &[RomFile]) -> Vec<RomFile> {
    let game = app.get("game").and_then(Value::as_str).unwrap_or("");
    let origin = app.get("origin").and_then(Value::as_str).unwrap_or("");
    if game.is_empty() {
        return vec![];
    }
    roms.iter().filter(|r| fits(r, origin) && matches(game, r) && fits_copy_ids(app, &r.path).is_ok()).cloned().collect()
}

/// A file whose disc ID is one of the catalog's known dumps for this game.
pub fn known_dump(app: &Value, p: &Path) -> bool {
    let want = copy_ids(app);
    !want.is_empty() && copy_id(p).map(|id| want.iter().any(|w| id.starts_with(w.as_str()))).unwrap_or(false)
}

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
            // Its catalog entry lists the discs or cartridges it's made for: one with another ID isn't it.
            .filter(|r| fits_copy_ids(a, &r.path).is_ok())
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

    /// A tiny XEX2 header with an execution info carrying this title ID.
    fn xex(title: u32) -> Vec<u8> {
        let mut b = vec![0u8; 0x100];
        b[..4].copy_from_slice(b"XEX2");
        b[0x14..0x18].copy_from_slice(&1u32.to_be_bytes());
        b[0x18..0x1C].copy_from_slice(&0x0004_0006u32.to_be_bytes());
        b[0x1C..0x20].copy_from_slice(&0x28u32.to_be_bytes());
        b[0x28 + 12..0x28 + 16].copy_from_slice(&title.to_be_bytes());
        b
    }

    #[test]
    fn parts_are_read_from_names() {
        let p = |n: &str| part_of(n);
        assert_eq!(p("Game (USA) (Disc 2).iso"), Part { kind: "game", disc: Some(2), of: None });
        assert_eq!(p("Game Disc 1 of 3.chd"), Part { kind: "game", disc: Some(1), of: Some(3) });
        assert_eq!(p("Game [0100509005AF2000][v0].nsp").kind, "game");
        assert_eq!(p("Game [0100509005AF2800][v786432].nsp").kind, "update");
        assert_eq!(p("Game [0100509005AF3001][v0].nsp").kind, "dlc");
        assert_eq!(p("Game [UPD].nsp").kind, "update");
        assert_eq!(p("TU_16L61VP_0000004000000.00000000000G1").kind, "update");
        assert_eq!(p("Game - Expansion DLC.nsp").kind, "dlc");
        assert_eq!(p("Discovery.iso"), Part { kind: "game", disc: None, of: None });
        assert_eq!(p("Uncharted.iso").disc, None);
    }

    #[test]
    fn xbox_360_title_ids_are_read() {
        let d = std::env::temp_dir().join(format!("rl-xex-{}", std::process::id()));
        std::fs::create_dir_all(d.join("disc")).unwrap();
        std::fs::write(d.join("disc/default.xex"), xex(0x4541_07EC)).unwrap();
        assert_eq!(copy_id(&d.join("disc")).as_deref(), Some("454107EC"));
        assert_eq!(copy_id(&d.join("disc/default.xex")).as_deref(), Some("454107EC"));
        // A trimmed disc image: the volume at sector 32, its root folder at sector 40 naming
        // default.xex at sector 48.
        let mut img = vec![0u8; 50 * 2048];
        img[32 * 2048..32 * 2048 + 20].copy_from_slice(b"MICROSOFT*XBOX*MEDIA");
        img[32 * 2048 + 20..32 * 2048 + 24].copy_from_slice(&40u32.to_le_bytes());
        img[32 * 2048 + 24..32 * 2048 + 28].copy_from_slice(&2048u32.to_le_bytes());
        let r = 40 * 2048;
        img[r + 4..r + 8].copy_from_slice(&48u32.to_le_bytes());
        img[r + 8..r + 12].copy_from_slice(&0x40u32.to_le_bytes());
        img[r + 13] = 11;
        img[r + 14..r + 25].copy_from_slice(b"default.xex");
        img[r + 28..r + 2048].fill(0xFF);
        img[48 * 2048..48 * 2048 + 0x100].copy_from_slice(&xex(0x4541_07EC));
        std::fs::write(d.join("nfs.iso"), &img).unwrap();
        assert_eq!(copy_id(&d.join("nfs.iso")).as_deref(), Some("454107EC"));
        let _ = std::fs::remove_dir_all(&d);
    }

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
    fn a_disc_from_another_region_is_told() {
        // Eternal Darkness reads the European disc (GEDP01) only.
        let d = tmp("gedp");
        let disc = |id: &[u8; 6]| { let mut h = vec![0u8; 0x400]; h[..6].copy_from_slice(id); h[0x1C..0x20].copy_from_slice(&[0xC2, 0x33, 0x9F, 0x3D]); h };
        fs::write(d.join("usa.iso"), disc(b"GEDE01")).unwrap();
        fs::write(d.join("eur.iso"), disc(b"GEDP01")).unwrap();
        let app = serde_json::json!({ "id": "eternaldarknessrecomp", "copy_ids": ["GEDP01"] });
        assert_eq!(fits_copy_ids(&app, &d.join("usa.iso")), Err("GEDE01".to_string()));
        assert!(fits_copy_ids(&app, &d.join("eur.iso")).is_ok());
        fs::remove_dir_all(d).unwrap();
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
    fn scans_switch2_nup_packages() {
        let root = tmp("nup-scan");
        fs::create_dir_all(&root).unwrap();
        let nup = root.join("Zelda [0400486024DEC000][v131072].nup");
        fs::write(&nup, [0; 8192]).unwrap();

        let found = scan(&[root.clone()]);
        fs::remove_dir_all(root).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, nup);
        assert_eq!(found[0].system, "switch2");
        assert!(fits(&found[0], "switch2"));
        assert!(fits(&found[0], "switch"));
        assert!(!fits(&found[0], "ps3"));
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
    fn copies_say_their_game_id() {
        let d = tmp("ids");
        // N64: the game code in the header, in any byte order.
        let mut n = rom("z64", "ZELDA MAJORA'S MASK", b'E');
        n[0x3B..0x3E].copy_from_slice(b"NZS");
        fs::write(d.join("mm.z64"), &n).unwrap();
        assert_eq!(copy_id(&d.join("mm.z64")).as_deref(), Some("NZSE"));
        let mut v = n.clone();
        v.chunks_mut(2).for_each(|c| c.swap(0, 1));
        fs::write(d.join("mm.v64"), &v).unwrap();
        assert_eq!(copy_id(&d.join("mm.v64")).as_deref(), Some("NZSE"));
        // GameCube: the ID at the start, plain and inside a CISO image.
        let mut gc = vec![0u8; 0x400];
        gc[..6].copy_from_slice(b"GZLE01");
        gc[0x1C..0x20].copy_from_slice(&[0xC2, 0x33, 0x9F, 0x3D]);
        fs::write(d.join("tww.iso"), &gc).unwrap();
        assert_eq!(copy_id(&d.join("tww.iso")).as_deref(), Some("GZLE01"));
        let mut ciso = vec![0u8; 0x8000];
        ciso[..4].copy_from_slice(b"CISO");
        ciso.extend_from_slice(&gc);
        fs::write(d.join("tww.ciso"), &ciso).unwrap();
        assert_eq!(copy_id(&d.join("tww.ciso")).as_deref(), Some("GZLE01"));
        // PS2: the serial in SYSTEM.CNF.
        let mut ps2 = vec![0u8; 2048 * 22];
        let pvd = 16 * 2048;
        ps2[pvd] = 1;
        ps2[pvd + 1..pvd + 6].copy_from_slice(b"CD001");
        ps2[pvd + 8..pvd + 19].copy_from_slice(b"PLAYSTATION");
        ps2[pvd + 156 + 2..pvd + 156 + 6].copy_from_slice(&18u32.to_le_bytes());
        ps2[pvd + 156 + 10..pvd + 156 + 14].copy_from_slice(&2048u32.to_le_bytes());
        let rec = 18 * 2048;
        ps2[rec] = 46;
        ps2[rec + 2..rec + 6].copy_from_slice(&20u32.to_le_bytes());
        ps2[rec + 32] = 12;
        ps2[rec + 33..rec + 45].copy_from_slice(b"SYSTEM.CNF;1");
        let cnf = b"BOOT2 = cdrom0:\\SLUS_203.12;1\r\nVER = 1.00";
        ps2[20 * 2048..20 * 2048 + cnf.len()].copy_from_slice(cnf);
        fs::write(d.join("ps2.iso"), &ps2).unwrap();
        assert_eq!(copy_id(&d.join("ps2.iso")).as_deref(), Some("SLUS20312"));
        // Against the catalog: listed, a shorter ID that starts it, another one, and none listed.
        let app = |ids: Value| serde_json::json!({ "id": "x", "copy_ids": ids });
        assert!(fits_copy_ids(&app(serde_json::json!(["SLUS-203.12"])), &d.join("ps2.iso")).is_ok());
        assert!(fits_copy_ids(&app(serde_json::json!("GZLE")), &d.join("tww.iso")).is_ok());
        assert_eq!(fits_copy_ids(&app(serde_json::json!(["GZLP01"])), &d.join("tww.iso")), Err("GZLE01".to_string()));
        assert!(fits_copy_ids(&serde_json::json!({ "id": "x" }), &d.join("tww.iso")).is_ok());
        // A file with no ID it reads can't be told apart, so it's fine.
        fs::write(d.join("other.chd"), b"MComprHD").unwrap();
        assert!(fits_copy_ids(&app(serde_json::json!(["GZLE01"])), &d.join("other.chd")).is_ok());
        // The scan leaves out a copy with another ID.
        let cat = serde_json::json!({ "apps": [{ "id": "mm", "game": "Zelda Majora's Mask", "origin": "n64", "copy_ids": ["NZSP"] }] });
        let found = vec![RomFile { path: d.join("mm.z64"), system: "n64".into(), title: "ZELDA MAJORA'S MASK".into(), region: "US".into(), size: 0 }];
        assert!(match_catalog(&cat, &found).is_empty());
        let cat = serde_json::json!({ "apps": [{ "id": "mm", "game": "Zelda Majora's Mask", "origin": "n64", "copy_ids": ["NZSE"] }] });
        assert!(match_catalog(&cat, &found).contains_key("mm"));
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn disc_images_say_their_console() {
        let d = tmp("discs");
        // A PS2 DVD: ISO 9660 that says PLAYSTATION, its SYSTEM.CNF with a BOOT2 line.
        let mut ps2 = vec![0u8; 2048 * 22];
        let pvd = 16 * 2048;
        ps2[pvd] = 1;
        ps2[pvd + 1..pvd + 6].copy_from_slice(b"CD001");
        ps2[pvd + 8..pvd + 19].copy_from_slice(b"PLAYSTATION");
        ps2[pvd + 156 + 2..pvd + 156 + 6].copy_from_slice(&18u32.to_le_bytes());
        ps2[pvd + 156 + 10..pvd + 156 + 14].copy_from_slice(&2048u32.to_le_bytes());
        let rec = 18 * 2048;
        ps2[rec] = 46;
        ps2[rec + 2..rec + 6].copy_from_slice(&20u32.to_le_bytes());
        ps2[rec + 32] = 12;
        ps2[rec + 33..rec + 45].copy_from_slice(b"SYSTEM.CNF;1");
        let cnf = b"BOOT2 = cdrom0:\\SLUS_203.12;1\r\nVER";
        ps2[20 * 2048..20 * 2048 + cnf.len()].copy_from_slice(cnf);
        fs::write(d.join("game.iso"), &ps2).unwrap();
        assert_eq!(disc_kind(&d.join("game.iso")), "ps2");
        // The same with a PlayStation BOOT line.
        let mut ps1 = ps2.clone();
        ps1[20 * 2048..20 * 2048 + 4].copy_from_slice(b"BOOT");
        ps1[20 * 2048 + 4] = b' ';
        fs::write(d.join("ps1.iso"), &ps1).unwrap();
        assert_eq!(disc_kind(&d.join("ps1.iso")), "ps1");
        // An Xbox disc, trimmed: XDVDFS at 0x10000, default.xbe in its root folder.
        let mut xb = vec![0u8; 0x1_0000 + 2048 * 40];
        xb[0x1_0000..0x1_0014].copy_from_slice(b"MICROSOFT*XBOX*MEDIA");
        xb[0x1_0014..0x1_0018].copy_from_slice(&36u32.to_le_bytes());
        xb[0x1_0018..0x1_001C].copy_from_slice(&2048u32.to_le_bytes());
        xb[36 * 2048 + 14..36 * 2048 + 25].copy_from_slice(b"default.xbe");
        fs::write(d.join("xbox.iso"), &xb).unwrap();
        assert_eq!(disc_kind(&d.join("xbox.iso")), "xbox");
        // A GameCube disc.
        let mut gc = vec![0u8; 4096];
        gc[0x1C..0x20].copy_from_slice(&[0xC2, 0x33, 0x9F, 0x3D]);
        fs::write(d.join("gc.iso"), &gc).unwrap();
        assert_eq!(disc_kind(&d.join("gc.iso")), "gc");
        fs::write(d.join("pc.iso"), vec![0u8; 4096]).unwrap();
        assert_eq!(disc_kind(&d.join("pc.iso")), "");
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn folders_named_for_a_console() {
        assert_eq!(folder_kind(Path::new("D:/Roms/PS2/Black (USA).iso")), "ps2");
        assert_eq!(folder_kind(Path::new("/home/me/roms/Sony - PlayStation 2/Black/Black.iso")), "ps2");
        assert_eq!(folder_kind(Path::new("E:/Games/Xbox/Black (USA)/Black.iso")), "xbox");
        assert_eq!(folder_kind(Path::new("E:/Games/Xbox 360/Black.iso")), "x360");
        assert_eq!(folder_kind(Path::new("E:/Games/psx/Driver 2.iso")), "ps1");
        // The nearest folder that names one.
        assert_eq!(folder_kind(Path::new("E:/xbox/backups/ps2/Black.iso")), "ps2");
        assert_eq!(folder_kind(Path::new("E:/Games/Black.iso")), "");
        assert_eq!(folder_kind(Path::new("E:/Gamecube Stuff/gcc/Black.iso")), "gc");
        // A PS2 disc never matches an original Xbox recomp; one of two only matches those two.
        let r = |p: &str, sys: &str| RomFile { path: PathBuf::from(p), system: sys.into(), title: "Black".into(), region: String::new(), size: 1 << 30 };
        assert!(!fits(&r("D:/Roms/PS2/Black (USA).iso", "ps2"), "xbox"));
        assert!(fits(&r("D:/Roms/PS2/Black (USA).iso", "ps2"), "ps2"));
        assert!(!fits(&r("D:/Roms/PS2/Black (USA).iso", ""), "xbox"));
        assert!(fits(&r("E:/Black.iso", "xdvdfs"), "xbox"));
        assert!(!fits(&r("E:/Black.iso", "xdvdfs"), "ps2"));
        assert!(fits(&r("E:/Black.iso", ""), "xbox"));
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

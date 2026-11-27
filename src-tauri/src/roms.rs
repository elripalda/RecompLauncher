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
const DISC: [&str; 7] = ["iso", "gcm", "rvz", "wia", "wbfs", "ciso", "chd"];
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

/// A Mega Drive or Genesis cartridge names its console at 0x100 ("SEGA MEGA DRIVE", " SEGA
/// GENESIS"): the console itself checks for SEGA there before it boots one.
fn mega_drive_header(p: &Path) -> bool {
    let mut console = [0u8; 16];
    let read = fs::File::open(p).and_then(|mut f| read_at(&mut f, 0x100, &mut console));
    read.is_ok() && has_bytes(&console, b"SEGA")
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
/// PS2 disc's serial from its SYSTEM.CNF ("SLUS20312"), a Switch game's title ID
/// ("0100F2C0115B6000"), an extracted PS3, PS4 or PS5 game's title ID ("CUSA00242"). None when the file doesn't carry one this reads (a CHD, another console).
pub fn copy_id(p: &Path) -> Option<String> {
    // An extracted game: an Xbox 360 disc's default.xex says the title, a Switch game's main.npdm,
    // a PlayStation game's param.sfo (param.json on PS5).
    if p.is_dir() {
        let xex = fs::File::open(p.join("default.xex")).ok().and_then(|mut f| xex_title(&mut f, 0));
        return xex.or_else(|| switch_folder_id(p)).or_else(|| playstation_folder_id(p));
    }
    if !p.is_file() {
        return None;
    }
    // A .cue, .gdi or .m3u: the disc's ID is on its first data track that has one.
    if let Some(set) = folded(p) {
        return set.data.iter().find_map(|t| copy_id(t));
    }
    let mut f = fs::File::open(p).ok()?;
    let mut head = [0u8; 0x60];
    read_at(&mut f, 0, &mut head).ok()?;
    // An Xbox 360 update or DLC package: the title ID of the game it's for.
    if let Some(h) = stfs_header(&mut f) {
        return stfs_title_id(&h);
    }
    if let Some(id) = ps3_pkg_content_id(&head) {
        return Some(ps3_pkg_title_id(id));
    }
    let is_wua = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("wua"));
    if is_wua {
        return wua_title_id(p);
    }
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    // A Switch dump: its ticket's name, or else the title ID its own name carries. An XCI's header
    // has no plain title ID, and a Switch 2 .nup isn't read, so only their names can say.
    if ext == "nsp" {
        return nsp_title_id(&mut f).or_else(|| name_title_id(p));
    }
    if ext == "xci" || ext == "nup" {
        return name_title_id(p);
    }
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
    if let Some((disc, _)) = nintendo_disc(&mut f) {
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

/// A GameCube or Wii disc's header and which of the two it is, read from its magic numbers (Wii
/// at 0x18, GameCube at 0x1C). The header is at the start of a plain image; WBFS, CISO and WIA/RVZ
/// images keep a plain copy of it, so no keys or unpacking are needed.
fn nintendo_disc(f: &mut fs::File) -> Option<([u8; 0x20], &'static str)> {
    let mut magic = [0u8; 4];
    read_at(f, 0, &mut magic).ok()?;
    let at = match &magic {
        b"WBFS" => 0x200,
        b"CISO" => 0x8000,
        b"WIA\x01" | b"RVZ\x01" => 0x58,
        _ => 0,
    };
    let mut disc = [0u8; 0x20];
    read_at(f, at, &mut disc).ok()?;
    if disc[0x18..0x1C] == [0x5D, 0x1C, 0x9E, 0xA3] {
        return Some((disc, "wii"));
    }
    if disc[0x1C..0x20] == [0xC2, 0x33, 0x9F, 0x3D] {
        return Some((disc, "gc"));
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

/// How much of an Xbox 360 content package (STFS: CON, LIVE or PIRS) is read: its plain header up
/// to the end of the title's name. The header is signed but never encrypted, so no keys are needed.
const STFS_HEADER: usize = 0x1711;

/// An Xbox 360 package's header, when the file starts with one of the three package magics.
fn stfs_header(f: &mut fs::File) -> Option<Vec<u8>> {
    let mut magic = [0u8; 4];
    read_at(f, 0, &mut magic).ok()?;
    if !matches!(&magic, b"CON " | b"LIVE" | b"PIRS") {
        return None;
    }
    let mut h = vec![0u8; STFS_HEADER];
    read_at(f, 0, &mut h).ok()?;
    Some(h)
}

/// The title ID ("4D5307E6") of the game an Xbox 360 package belongs to.
fn stfs_title_id(h: &[u8]) -> Option<String> {
    let id = u32::from_be_bytes([h[0x360], h[0x361], h[0x362], h[0x363]]);
    (id != 0).then(|| format!("{id:08X}"))
}

/// A name in a package header: UTF-16 big-endian, ended by a zero or the field's end.
fn stfs_name(field: &[u8]) -> String {
    let units: Vec<u16> = field.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).take_while(|&u| u != 0).collect();
    String::from_utf16_lossy(&units).trim().to_string()
}

/// An Xbox 360 title update or downloadable content package, as its header says.
struct Package {
    /// "update" or "dlc".
    kind: &'static str,
    /// The game's name, which is what it's matched to its game by.
    title: String,
    /// The package's own name (the DLC's, often empty in a title update).
    display: String,
}

/// Title updates and DLC are kept as files named only by a content ID, with no file type, so their
/// header says what they are. Its content type tells an update from DLC; other packages (Arcade
/// games, saves, profiles, demos) aren't extra content for a game and are left out.
fn x360_package(p: &Path) -> Option<Package> {
    let h = stfs_header(&mut fs::File::open(p).ok()?)?;
    let content_type = u32::from_be_bytes([h[0x344], h[0x345], h[0x346], h[0x347]]);
    let kind = match content_type {
        0x000B_0000 => "update",
        0x0000_0002 => "dlc",
        _ => return None,
    };
    // The first of each name table is the English one.
    let display = stfs_name(&h[0x411..0x411 + 0x80]);
    let title = stfs_name(&h[0x1691..0x1691 + 0x80]);
    Some(Package { kind, title, display })
}

/// An Xbox 360 update or DLC package: named for its game, but never a copy of the game itself.
pub fn is_extra_package(r: &RomFile) -> bool {
    r.system == "x360" && r.path.is_file() && x360_package(&r.path).is_some()
}

/// The start of a PS3 package, up to the end of its content ID. It's plain, so no keys are needed.
fn ps3_pkg_header(p: &Path) -> Option<[u8; 0x54]> {
    let mut h = [0u8; 0x54];
    read_at(&mut fs::File::open(p).ok()?, 0, &mut h).ok()?;
    Some(h)
}

/// A PS3 package's content ID ("UP0082-BLUS31197_00-DOD3PATCH0000101"), when the header is one: a
/// PS4 package also ends in .pkg but starts with 0x7F CNT instead, and isn't read.
fn ps3_pkg_content_id(h: &[u8]) -> Option<&str> {
    if h.len() < 0x54 || !h.starts_with(b"\x7FPKG") {
        return None;
    }
    let id = std::str::from_utf8(&h[0x30..0x54]).ok()?.trim_end_matches('\0');
    let title_id = id.get(7..16)?;
    valid_playstation_id(title_id).then_some(id)
}

/// The game's title ID inside a content ID that `ps3_pkg_content_id` accepted.
fn ps3_pkg_title_id(content_id: &str) -> String {
    content_id[7..16].to_string()
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
    const WORDS: [(&str, &str); 30] = [
        ("arcade", "arcade"), ("mame", "arcade"), ("fbneo", "arcade"), ("fba", "arcade"), ("finalburn", "arcade"),
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

/// A disc image's console: what's inside it says (`inside`, from `disc_kind`), and where it says
/// one of two, or nothing, the folder it's in picks (an Xbox disc in a folder named "Xbox 360").
fn disc_system(p: &Path, inside: &'static str) -> &'static str {
    let folder = folder_kind(p);
    match inside {
        "" => folder,
        "xdvdfs" if matches!(folder, "xbox" | "x360") => folder,
        "playstation" if matches!(folder, "ps1" | "ps2") => folder,
        k => k,
    }
}

/// A disc kept as several files has a small text sheet that lists them: a .cue and its .bin
/// tracks (PlayStation, Saturn), a .gdi and its tracks (Dreamcast), or an .m3u playlist naming
/// each disc of a set. The sheet stands for the whole disc or set, so it's found once.
const SHEET: [&str; 3] = ["cue", "gdi", "m3u"];

/// Real sheets are a few KB; a big file with one of these names isn't a sheet and isn't read.
const MAX_SHEET: u64 = 64 * 1024;

/// What a sheet stands for: every file it folds in (so none is listed again on its own), and the
/// data tracks, in disc order, that say which console it's for.
#[derive(Default)]
struct Folded {
    files: Vec<PathBuf>,
    data: Vec<PathBuf>,
}

impl Folded {
    /// The disc's or set's size: its tracks and disc images, not the sheets in it. A file named
    /// twice counts once.
    fn size(&self) -> u64 {
        let mut counted = std::collections::HashSet::new();
        let mut total = 0;
        for f in &self.files {
            if is_sheet(f) || !counted.insert(f) {
                continue;
            }
            total += fs::metadata(f).map(|m| m.len()).unwrap_or(0);
        }
        total
    }
}

fn ext_of(p: &Path) -> String {
    p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

fn is_sheet(p: &Path) -> bool {
    SHEET.contains(&ext_of(p).as_str())
}

fn read_sheet(p: &Path) -> Option<String> {
    let mut b = Vec::new();
    fs::File::open(p).ok()?.take(MAX_SHEET + 1).read_to_end(&mut b).ok()?;
    if b.len() as u64 > MAX_SHEET {
        return None;
    }
    Some(String::from_utf8_lossy(&b).into_owned())
}

/// A file a sheet names, found next to the sheet. Sheets made on Windows use backslashes and
/// often another case than the files have on disk, so both are allowed for. None when it's missing.
fn sheet_file(sheet: &Path, name: &str) -> Option<PathBuf> {
    let name = name.trim().replace('\\', "/");
    if name.is_empty() {
        return None;
    }
    let given = Path::new(&name);
    if given.is_absolute() {
        return given.is_file().then(|| given.to_path_buf());
    }
    let path = game_path(sheet.parent()?, &name)?;
    path.is_file().then_some(path)
}

/// The name in a line's quotes (`FILE "Game (Track 1).bin" BINARY`), or else its first word.
fn quoted_or_first(rest: &str) -> Option<&str> {
    let rest = rest.trim();
    if let Some(q) = rest.strip_prefix('"') {
        return q.split('"').next();
    }
    rest.split_whitespace().next()
}

/// A .cue's tracks: each FILE line names a file, and the TRACK lines under it say whether it holds
/// data ("MODE2/2352") or music ("AUDIO").
fn cue_folded(sheet: &Path, text: &str) -> Folded {
    let mut out = Folded::default();
    // The file the TRACK lines that follow belong to; None while it's missing.
    let mut current: Option<PathBuf> = None;
    for line in text.lines() {
        let line = line.trim();
        let upper = line.to_ascii_uppercase();
        if upper.starts_with("FILE ") {
            current = quoted_or_first(&line[5..]).and_then(|n| sheet_file(sheet, n));
            out.files.extend(current.clone());
            continue;
        }
        if !upper.starts_with("TRACK ") || upper.ends_with("AUDIO") {
            continue;
        }
        let Some(file) = &current else { continue };
        if !out.data.contains(file) {
            out.data.push(file.clone());
        }
    }
    out
}

/// A .gdi's tracks: after the count, one line each, "number start type sector-size file offset",
/// where type 4 is data and 0 is music. The file's name may be quoted when it has spaces.
fn gdi_folded(sheet: &Path, text: &str) -> Folded {
    let mut out = Folded::default();
    for line in text.lines().skip(1) {
        let Some((kind, name)) = gdi_line(line) else { continue };
        let Some(file) = sheet_file(sheet, name) else { continue };
        if kind == "4" {
            out.data.push(file.clone());
        }
        out.files.push(file);
    }
    out
}

/// A .gdi track line's type and file name.
fn gdi_line(line: &str) -> Option<(&str, &str)> {
    let mut fields = line.split_whitespace();
    let kind = fields.nth(2)?;
    // The name starts after the fourth field; it may have spaces in it, so the rest is kept whole.
    let mut rest = line.trim_start();
    for _ in 0..4 {
        rest = rest.split_once(char::is_whitespace)?.1.trim_start();
    }
    Some((kind, quoted_or_first(rest)?))
}

/// An .m3u's discs, in the order it lists them (lines starting with # are comments). A disc that
/// is a .cue or .gdi brings its tracks along. A playlist inside a playlist isn't followed, so one
/// that names itself can't go round in circles.
fn m3u_folded(sheet: &Path, text: &str) -> Folded {
    let mut out = Folded::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(disc) = sheet_file(sheet, line) else { continue };
        let ext = ext_of(&disc);
        if ext == "m3u" {
            continue;
        }
        out.files.push(disc.clone());
        let tracks = if ext == "cue" || ext == "gdi" { folded(&disc) } else { None };
        match tracks {
            Some(t) => {
                out.files.extend(t.files);
                out.data.extend(t.data);
            }
            None => out.data.push(disc),
        }
    }
    out
}

/// What a .cue, .gdi or .m3u folds in. None for any other file, or a sheet too big to be one.
fn folded(p: &Path) -> Option<Folded> {
    let ext = ext_of(p);
    if !SHEET.contains(&ext.as_str()) {
        return None;
    }
    let text = read_sheet(p)?;
    match ext.as_str() {
        "cue" => Some(cue_folded(p, &text)),
        "gdi" => Some(gdi_folded(p, &text)),
        _ => Some(m3u_folded(p, &text)),
    }
}

/// The data tracks a .cue, .gdi or .m3u names that are there, in disc order. None for any other
/// file, so a copy check knows to look at the file itself.
pub fn sheet_data_tracks(p: &Path) -> Option<Vec<PathBuf>> {
    folded(p).map(|set| set.data)
}

/// Every file a .cue, .gdi or .m3u names that's there: its tracks (and an .m3u's discs with theirs).
/// None for any other file.
pub fn sheet_files(p: &Path) -> Option<Vec<PathBuf>> {
    folded(p).map(|set| set.files)
}

/// The first disc an .m3u lists that's there (its .cue, .gdi or disc image).
pub fn first_disc(p: &Path) -> Option<PathBuf> {
    if ext_of(p) != "m3u" {
        return None;
    }
    folded(p)?.files.into_iter().next()
}

/// A sheet found as one copy. Its console is read from its data tracks like an .iso's (a raw
/// 2352-byte track works too), its size is its tracks' (only the sheet's own when they're missing,
/// so a stray sheet doesn't look like a whole disc). It's kept even with tracks missing, so the
/// player sees the copy and can fix it.
fn identify_sheet(p: &Path, size: impl FnOnce() -> Option<u64>) -> Option<RomFile> {
    let own = size()?;
    let set = folded(p)?;
    let tracks = set.size();
    let size = if tracks > 0 { tracks } else { own };
    let inside = set.data.iter().map(|t| disc_kind(t)).find(|k| !k.is_empty()).unwrap_or("");
    let mut system = disc_system(p, inside);
    // GD-ROM dumps are the Dreamcast's (and NAOMI's, an arcade board any arcade recomp takes).
    if system.is_empty() && ext_of(p) == "gdi" {
        system = "dc";
    }
    let title = p.file_stem()?.to_str()?.to_string();
    Some(RomFile { path: p.to_path_buf(), system: system.into(), title, region: String::new(), size })
}

/// Takes out the files a found sheet already stands for: a .cue listed in an .m3u, or a disc image
/// a .cue names. Paths are compared as the file system resolves them, since a sheet may name a
/// file through another case or a "..".
pub fn fold_sets(found: Vec<RomFile>) -> Vec<RomFile> {
    let key = |p: &Path| fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let inside: std::collections::HashSet<PathBuf> = found.iter().filter_map(|r| folded(&r.path)).flat_map(|s| s.files).map(|f| key(&f)).collect();
    if inside.is_empty() {
        return found;
    }
    // Only disc images and sheets can be in a set: other files aren't looked up.
    let could_be_in_one = |p: &Path| {
        let ext = ext_of(p);
        DISC.contains(&ext.as_str()) || SHEET.contains(&ext.as_str())
    };
    found.into_iter().filter(|r| !could_be_in_one(&r.path) || !inside.contains(&key(&r.path))).collect()
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
            "" if DISC.contains(&ext.as_str()) || SHEET.contains(&ext.as_str()) => folder_kind(&rom.path),
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

/// One file: a ROM, a disc image (or the .cue, .gdi or .m3u that stands for one or a set), an Xbox
/// 360 game's default.xex (which stands for its folder), or an Xbox 360 title update or DLC package.
pub fn identify(p: &Path, size: u64) -> Option<RomFile> {
    identify_with_size(p, || Some(size))
}

fn identify_by_type(p: &Path, size: impl FnOnce() -> Option<u64>) -> Option<RomFile> {
    let Some(ext) = p.extension() else { return identify_package(p, size) };
    let ext = ext.to_str()?.to_ascii_lowercase();
    if ext == "zip" || ext == "7z" {
        return identify_archive(p, &ext, size);
    }
    if WIIU.contains(&ext.as_str()) {
        return identify_wiiu(p, &ext, size);
    }
    // Title updates are also kept under names like TU_16L61VP_0000004000000.00000000000G1, where
    // what looks like a file type is part of the content ID. No file type this scans for is that long.
    if ext.len() > 4 {
        return identify_package(p, size);
    }
    if ext == "pkg" {
        return identify_ps3_pkg(p, size);
    }
    if SHEET.contains(&ext.as_str()) {
        return identify_sheet(p, size);
    }
    let is_n64 = N64.contains(&ext.as_str());
    let is_disc = DISC.contains(&ext.as_str());
    // .md is also Markdown, and READMEs and licenses sit in every recomp's folder: one only counts
    // when it has a Mega Drive header.
    let is_cart = CART.contains(&ext.as_str()) && (ext != "md" || mega_drive_header(p));
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
        let inside = if ext == "iso" { disc_kind(p) } else { "" };
        let system = match system_for(&ext) {
            "" => disc_system(p, inside),
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

/// An Xbox 360 title update or DLC package, matched to its game by the game's name in its header.
/// Only a file big enough to hold the header is opened, and only its first four bytes are read
/// unless they're a package's.
fn identify_package(p: &Path, size: impl FnOnce() -> Option<u64>) -> Option<RomFile> {
    let size = size()?;
    if size < STFS_HEADER as u64 {
        return None;
    }
    let pkg = x360_package(p)?;
    let file_name = p.file_name()?.to_str()?.to_string();
    let title = [pkg.title, pkg.display].into_iter().find(|t| !t.is_empty()).unwrap_or(file_name);
    Some(RomFile { path: p.to_path_buf(), system: "x360".into(), title, region: String::new(), size })
}

/// A PS3 package: a PSN game or an update for one. Its title is only in the encrypted part, so
/// it's matched to its game by its file name, like a disc image.
fn identify_ps3_pkg(p: &Path, size: impl FnOnce() -> Option<u64>) -> Option<RomFile> {
    let size = size()?;
    ps3_pkg_content_id(&ps3_pkg_header(p)?)?;
    let title = p.file_stem()?.to_str()?.to_string();
    Some(RomFile { path: p.to_path_buf(), system: "ps3".into(), title, region: String::new(), size })
}

/// Disc image types Dolphin writes for both GameCube and Wii games.
const GC_OR_WII: [&str; 6] = ["iso", "gcm", "rvz", "wia", "wbfs", "ciso"];

/// One file, by its type, then a GameCube or Wii disc image told apart by its header: an .rvz
/// (or .gcm, .wia...) can hold either console's game, so the file type alone files Wii games as
/// GameCube ones. Where the header can't be read, what the file type or folder said stays.
fn identify_with_size(p: &Path, size: impl FnOnce() -> Option<u64>) -> Option<RomFile> {
    let mut rom = identify_by_type(p, size)?;
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if !p.is_file() || !GC_OR_WII.contains(&ext.as_str()) {
        return Some(rom);
    }
    let read = fs::File::open(p).ok().and_then(|mut f| nintendo_disc(&mut f));
    if let Some((_, console)) = read {
        rom.system = console.into();
    }
    Some(rom)
}

/// Wii U copies: Cemu's .wua archive, and .wux (compressed) and .wud (raw) disc images.
const WIIU: [&str; 3] = ["wua", "wux", "wud"];

/// The last bytes of a .wua (Cemu's ZArchive): six section offsets and sizes, a hash, the total
/// size, the version and this magic, all big-endian.
const ZARCHIVE_FOOTER: usize = 144;
const ZARCHIVE_MAGIC: u32 = 0x169F_52D6;

/// The names table of a .wua. Unlike its files, it isn't compressed, and its first names are the
/// title folders ("000500001012bd00_v16"), so the title IDs are read without unpacking anything.
fn wua_names(p: &Path) -> Option<Vec<String>> {
    use std::io::{Seek, SeekFrom};
    let mut f = fs::File::open(p).ok()?;
    let len = f.seek(SeekFrom::End(0)).ok()?;
    let mut foot = [0u8; ZARCHIVE_FOOTER];
    read_at(&mut f, len.checked_sub(ZARCHIVE_FOOTER as u64)?, &mut foot).ok()?;
    let be64 = |i: usize| u64::from_be_bytes(foot[i..i + 8].try_into().unwrap());
    if u32::from_be_bytes(foot[140..144].try_into().unwrap()) != ZARCHIVE_MAGIC {
        return None;
    }
    // The title folders come first, so the start of the table is enough.
    let (at, size) = (be64(32), be64(40).min(64 * 1024) as usize);
    let mut table = vec![0u8; size];
    read_at(&mut f, at, &mut table).ok()?;
    let mut names = vec![];
    let mut i = 0usize;
    while i < table.len() {
        // A length byte, or two when its top bit is set.
        let mut n = table[i] as usize;
        i += 1;
        if n & 0x80 != 0 {
            let Some(&low) = table.get(i) else { break };
            n = ((n & 0x7F) << 8) | low as usize;
            i += 1;
        }
        let Some(name) = table.get(i..i + n) else { break };
        names.push(String::from_utf8_lossy(name).into_owned());
        i += n;
    }
    Some(names)
}

/// The Wii U title IDs a .wua holds, upper case: its game's, its update's (0005000E...) and DLC's
/// (0005000C...), from its title folders' names.
fn wua_titles(p: &Path) -> Vec<String> {
    let is_title = |id: &str| id.len() == 16 && id.starts_with("0005") && id.bytes().all(|b| b.is_ascii_hexdigit());
    let names = wua_names(p).unwrap_or_default();
    names.iter().filter_map(|n| n.split('_').next()).filter(|id| is_title(id)).map(str::to_ascii_uppercase).collect()
}

/// The game's title ID for a .wua: its base game's, or, in one holding only an update or DLC, the
/// game's ID they share (the same low half under 00050000).
fn wua_title_id(p: &Path) -> Option<String> {
    let titles = wua_titles(p);
    if let Some(base) = titles.iter().find(|t| t.starts_with("00050000")) {
        return Some(base.clone());
    }
    titles.first().map(|t| format!("00050000{}", &t[8..]))
}

/// A Wii U copy, matched by its file name. Each kind is checked by what it starts or ends with, so
/// a file that merely has the extension isn't taken: a .wua's footer magic, a .wux's "WUX0", a
/// .wud's product code ("WUP-P-ARDE").
fn identify_wiiu(p: &Path, ext: &str, size: impl FnOnce() -> Option<u64>) -> Option<RomFile> {
    let ok = match ext {
        "wua" => wua_names(p).is_some(),
        _ => {
            let mut head = [0u8; 4];
            read_at(&mut fs::File::open(p).ok()?, 0, &mut head).ok()?;
            &head == if ext == "wux" { b"WUX0" } else { b"WUP-" }
        }
    };
    if !ok {
        return None;
    }
    let title = p.file_stem()?.to_str()?.to_string();
    Some(RomFile { path: p.to_path_buf(), system: "wiiu".into(), title, region: String::new(), size: size()? })
}

/// The names of the files in a zip or 7z, read from its index only: nothing is unpacked. None when
/// it can't be opened as one (damaged, or a 7z whose index is behind a password).
fn archive_names(p: &Path, ext: &str) -> Option<Vec<String>> {
    if ext == "7z" {
        let archive = sevenz_rust::Archive::open(p).ok()?;
        return Some(archive.files.iter().filter(|f| !f.is_directory()).map(|f| f.name().to_string()).collect());
    }
    let zip = zip::ZipArchive::new(fs::File::open(p).ok()?).ok()?;
    Some(zip.file_names().filter(|n| !n.ends_with('/')).map(str::to_string).collect())
}

/// The file type of a name inside an archive, when it's a cartridge ROM's.
fn rom_ext(name: &str) -> Option<String> {
    let ext = Path::new(name).extension()?.to_str()?.to_ascii_lowercase();
    let is_rom = N64.contains(&ext.as_str()) || CART.contains(&ext.as_str());
    is_rom.then_some(ext)
}

/// A MAME-style set name ("hopr", "racr2ce", "hopr_x"): short, with no spaces or brackets. A zip
/// named like a game ("Some Game (USA).zip") with no ROM inside is something else.
fn is_set_name(stem: &str) -> bool {
    let ok_chars = stem.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    !stem.is_empty() && stem.len() <= 20 && ok_chars
}

/// A zipped game. Most zips aren't games, so one counts only when it holds exactly one cartridge
/// ROM (its console from that ROM's type), or when it's an arcade set: MAME sets are zips of chip
/// dumps with no ROM file type, so they're told only by being kept in an arcade folder under the
/// set's short name.
fn identify_archive(p: &Path, ext: &str, size: impl FnOnce() -> Option<u64>) -> Option<RomFile> {
    let names = archive_names(p, ext)?;
    let roms: Vec<String> = names.iter().filter_map(|n| rom_ext(n)).collect();
    let stem = p.file_stem()?.to_str()?.to_string();
    if let [inner] = roms.as_slice() {
        let system = system_for(inner);
        return Some(RomFile { path: p.to_path_buf(), system: system.into(), title: stem, region: String::new(), size: size()? });
    }
    let arcade_set = roms.is_empty() && !names.is_empty() && folder_kind(p) == "arcade" && is_set_name(&stem);
    if !arcade_set {
        return None;
    }
    Some(RomFile { path: p.to_path_buf(), system: "arcade".into(), title: stem, region: String::new(), size: size()? })
}

/// An arcade set is named for its MAME short name ("hopr"), not the game, so it's matched when the
/// catalog lists that name in the game's copy_ids. The whole name has to be listed: a listed
/// "hopr" isn't the "hopr2" set.
fn listed_set(app: &Value, rom: &RomFile) -> bool {
    if rom.system != "arcade" {
        return false;
    }
    let Some(stem) = rom.path.file_stem().and_then(|s| s.to_str()) else { return false };
    copy_ids(app).contains(&id_key(stem))
}

/// An extracted PlayStation game whose title ID the catalog lists. Its own title often carries an
/// edition or subtitle the catalog's name doesn't ("Diablo III: Reaper of Souls – Ultimate Evil
/// Edition" for "Diablo III"), so its ID is what tells it is this game.
fn listed_playstation(app: &Value, rom: &RomFile) -> bool {
    matches!(rom.system.as_str(), "ps3" | "ps4" | "ps5") && known_dump(app, &rom.path)
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

/// The game in a PARAM.SFO, with its title ID.
fn identify_sfo(folder: &Path, path: &Path, system: &str, valid_id: impl FnOnce(&str) -> bool) -> Option<(RomFile, String)> {
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
    let game = RomFile { path: folder.to_path_buf(), system: system.into(), title: title.into(), region: String::new(), size: 0 };
    Some((game, id.to_string()))
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

/// The game in a PS5 folder's sce_sys/param.json, with its title ID ("PPSA21559").
fn identify_ps5(folder: &Path) -> Option<(RomFile, String)> {
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
    let game = RomFile { path: folder.to_path_buf(), system: "ps5".into(), title: title.into(), region: String::new(), size: 0 };
    Some((game, format!("PPSA{digits}")))
}

fn identify_playstation(folder: &Path) -> Option<RomFile> {
    playstation_game(folder).map(|(game, _)| game)
}

/// An extracted PlayStation game's title ID ("CUSA00242"), from the same file its title is read from.
fn playstation_folder_id(folder: &Path) -> Option<String> {
    playstation_game(folder).map(|(_, id)| id)
}

/// An extracted PS3, PS4 or PS5 game, with its title ID.
fn playstation_game(folder: &Path) -> Option<(RomFile, String)> {
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
    // A PSN game, installed or extracted (RPCS3 keeps them in dev_hdd0/game/NPUB30467). Only NP
    // titles count: a disc game's update installed there has the same layout under its BL/BC ID.
    if has_file("PARAM.SFO") && has_file("USRDIR/EBOOT.BIN") {
        return identify_sfo(folder, &game_path(folder, "PARAM.SFO")?, "ps3", |id| id.starts_with("NP") && valid_playstation_id(id));
    }
    None
}

/// An extracted Switch game's title ID: the program ID in its exefs/main.npdm (under base/ when the
/// game is kept with its update and DLC). The part of the file that holds it, ACI0, is plain, so no
/// keys are needed.
fn switch_folder_id(folder: &Path) -> Option<String> {
    let npdm = ["base/exefs/main.npdm", "exefs/main.npdm"].iter().find_map(|r| game_path(folder, r))?;
    npdm_program_id(&mut fs::File::open(npdm).ok()?)
}

fn npdm_program_id(f: &mut fs::File) -> Option<String> {
    let mut meta = [0u8; 0x80];
    read_at(f, 0, &mut meta).ok()?;
    if &meta[..4] != b"META" {
        return None;
    }
    let aci0_at = u32::from_le_bytes(meta[0x70..0x74].try_into().ok()?);
    let mut aci0 = [0u8; 0x18];
    read_at(f, aci0_at.into(), &mut aci0).ok()?;
    if &aci0[..4] != b"ACI0" {
        return None;
    }
    let id = u64::from_le_bytes(aci0[0x10..0x18].try_into().ok()?);
    (id != 0).then(|| format!("{id:016X}"))
}

/// An NSP's title ID, from its ticket's name. An NSP is a PFS0, whose list of files is plain, and a
/// ticket is named for its rights ID, which starts with the title ID
/// ("01007300020FA0000000000000000008.tik"). None when it has no ticket: then only its encrypted
/// files say.
fn nsp_title_id(f: &mut fs::File) -> Option<String> {
    let mut head = [0u8; 0x10];
    read_at(f, 0, &mut head).ok()?;
    if &head[..4] != b"PFS0" {
        return None;
    }
    let count = u32::from_le_bytes(head[4..8].try_into().ok()?) as u64;
    let names_len = u32::from_le_bytes(head[8..12].try_into().ok()?) as usize;
    // A real NSP lists a handful of files; anything far bigger isn't one.
    if count == 0 || count > 1000 || names_len > 0x10000 {
        return None;
    }
    let mut names = vec![0u8; names_len];
    read_at(f, 0x10 + count * 0x18, &mut names).ok()?;
    names.split(|&b| b == 0).filter_map(|n| std::str::from_utf8(n).ok()).find_map(|name| {
        let lower = name.to_ascii_lowercase();
        let rights = lower.strip_suffix(".tik")?;
        switch_title_id(rights.get(..16)?)
    })
}

/// The title ID a Switch dump's own name carries, the way dumps are usually named
/// ("ASTRAL CHAIN [01007300020FA800][v65536].nsp").
fn name_title_id(p: &Path) -> Option<String> {
    let name = p.file_name()?.to_str()?;
    name.split(|c: char| !c.is_ascii_hexdigit()).find_map(switch_title_id)
}

/// A Switch title ID: 16 hex digits starting 01, or 04 for a Switch 2 game, upper case.
fn switch_title_id(s: &str) -> Option<String> {
    let is_id = s.len() == 16 && (s.starts_with("01") || s.starts_with("04")) && s.chars().all(|c| c.is_ascii_hexdigit());
    is_id.then(|| s.to_ascii_uppercase())
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
            ["eboot.bin", "ps3_disc.sfb", "param.sfo", "base", "exefs"].iter().any(|m| n.eq_ignore_ascii_case(m))
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
    fold_sets(out)
}

/// The words in a name, lower case, with the noise taken out: "Zelda - Majora's Mask (USA).z64"
/// becomes zelda, majoras, mask.
pub fn words(s: &str) -> Vec<String> {
    let clean = outside_brackets(s);
    const NOISE: [&str; 9] = ["the", "of", "a", "and", "usa", "us", "europe", "rev", "v1"];
    clean
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty() && !NOISE.contains(w))
        .map(str::to_string)
        .collect()
}

/// A name in lower case without its bracketed tags: region and dump tags say nothing about which
/// game it is.
fn outside_brackets(s: &str) -> String {
    let s = s.to_lowercase().replace('\'', "").replace('’', "");
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
    clean
}

/// A number or Roman numeral that can be a sequel's. One is left out: "Game 1" is still "Game".
fn is_sequel_number(word: &str) -> bool {
    // Up to XX so long series are covered too: Final Fantasy XIII isn't Final Fantasy XII.
    const ROMAN: [&str; 19] = ["ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x", "xi", "xii", "xiii", "xiv", "xv", "xvi", "xvii", "xviii", "xix", "xx"];
    if ROMAN.contains(&word) {
        return true;
    }
    let digits = !word.is_empty() && word.len() <= 4 && word.chars().all(|c| c.is_ascii_digit());
    digits && !word.trim_start_matches('0').is_empty() && word != "1"
}

/// Whether the name carries a sequel number the game's name doesn't: "Gears of War 2" isn't
/// "Gears of War". Dots stay inside a word so a version like "1.2" isn't read as a sequel, and a
/// number after "Disc" or "Part" is that disc of this game.
fn names_a_sequel(name: &str, want: &[String]) -> bool {
    const NUMBERED: [&str; 12] = ["disc", "disk", "cd", "part", "track", "side", "vol", "volume", "rev", "ver", "version", "of"];
    let clean = outside_brackets(name);
    let parts: Vec<&str> = clean.split(|c: char| !c.is_ascii_alphanumeric() && c != '.').map(|p| p.trim_matches('.')).filter(|p| !p.is_empty()).collect();
    for (i, part) in parts.iter().enumerate() {
        if !is_sequel_number(part) || want.iter().any(|w| w == part) {
            continue;
        }
        let after_a_label = i > 0 && NUMBERED.contains(&parts[i - 1]);
        if !after_a_label {
            return true;
        }
    }
    false
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
    let stem = rom.path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    (all(&have) && !names_a_sequel(&rom.title, &want)) || (all(&file) && !names_a_sequel(stem, &want))
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
    // A Switch title ID: 16 hex digits starting 01 (04 for Switch 2). Base games end in 000, updates in 800.
    let tid = low.split(|c: char| !c.is_ascii_hexdigit()).find_map(switch_title_id).map(|x| x[13..].to_string());
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

/// What a found file is to its game. An Xbox 360 package's header says (its name is only a content
/// ID); any other file's name does.
pub fn part_of_file(p: &Path) -> Part {
    if let Some(pkg) = x360_package(p) {
        return Part { kind: pkg.kind, disc: None, of: None };
    }
    // A PS3 update package's content ID usually says so ("UP0082-BLUS31197_00-DOD3PATCH0000101"),
    // even when its file is only named by that ID.
    let is_pkg = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pkg"));
    let pkg = if is_pkg { ps3_pkg_header(p) } else { None };
    let pkg_patch = pkg.as_ref().and_then(|h| ps3_pkg_content_id(h)).is_some_and(|id| id.to_ascii_uppercase().contains("PATCH"));
    if pkg_patch {
        return Part { kind: "update", disc: None, of: None };
    }
    // A .wua named "(DLC)" often packs the game with its DLC and update: it's the game when its
    // title folders include the game's own.
    let is_wua = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("wua"));
    if is_wua && wua_titles(p).iter().any(|t| t.starts_with("00050000")) {
        return Part { kind: "game", disc: None, of: None };
    }
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    part_of(&name)
}

/// Every found file that belongs to this catalog entry's game: its discs, updates and extra
/// content, by the same name match as `match_catalog`.
pub fn files_of(app: &Value, roms: &[RomFile]) -> Vec<RomFile> {
    let game = app.get("game").and_then(Value::as_str).unwrap_or("");
    let origin = app.get("origin").and_then(Value::as_str).unwrap_or("");
    if game.is_empty() {
        return vec![];
    }
    roms.iter().filter(|r| fits(r, origin) && (matches(game, r) || listed_set(app, r) || listed_playstation(app, r)) && fits_copy_ids(app, &r.path).is_ok()).cloned().collect()
}

/// A file whose disc ID is one of the catalog's known dumps for this game.
pub fn known_dump(app: &Value, p: &Path) -> bool {
    let want = copy_ids(app);
    !want.is_empty() && copy_id(p).map(|id| want.iter().any(|w| id.starts_with(w.as_str()))).unwrap_or(false)
}

pub fn match_catalog(cat: &Value, roms: &[RomFile]) -> HashMap<String, RomFile> {
    let mut out = HashMap::new();
    let Some(apps) = cat.get("apps").and_then(Value::as_array) else { return out };
    // Looked at once, not once per game: an update or DLC is named for its game but can't be played.
    let copies: Vec<&RomFile> = roms.iter().filter(|r| !is_extra_package(r)).collect();
    for a in apps {
        let id = a.get("id").and_then(Value::as_str).unwrap_or("");
        let game = a.get("game").and_then(Value::as_str).unwrap_or("");
        let origin = a.get("origin").and_then(Value::as_str).unwrap_or("");
        if id.is_empty() || game.is_empty() {
            continue;
        }
        let pick = copies
            .iter()
            .copied()
            .filter(|r| fits(r, origin))
            .filter(|r| matches(game, r) || listed_set(a, r) || listed_playstation(a, r))
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
pub(crate) mod tests {
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

    /// A tiny Xbox 360 package header: its magic, content type, game's title ID and names.
    fn stfs(content_type: u32, title: &str, display: &str) -> Vec<u8> {
        let mut b = vec![0u8; STFS_HEADER + 0x100];
        b[..4].copy_from_slice(b"LIVE");
        b[0x344..0x348].copy_from_slice(&content_type.to_be_bytes());
        b[0x360..0x364].copy_from_slice(&0x4D53_07E6u32.to_be_bytes());
        let utf16 = |s: &str| s.encode_utf16().flat_map(u16::to_be_bytes).collect::<Vec<u8>>();
        let (d, t) = (utf16(display), utf16(title));
        b[0x411..0x411 + d.len()].copy_from_slice(&d);
        b[0x1691..0x1691 + t.len()].copy_from_slice(&t);
        b
    }

    #[test]
    fn scans_xbox_360_title_updates() {
        let d = tmp("x360-tu");
        let p = d.join("4D5307E6/000B0000/7C1C4E5D5C6E2B6F3A8D6E14F2D9A1B3C4E5F60718");
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, stfs(0x000B_0000, "SkyRacer", "")).unwrap();
        let found = scan(&[d.clone()]);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].system.as_str(), found[0].title.as_str()), ("x360", "SkyRacer"));
        assert_eq!(part_of_file(&p).kind, "update");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn scans_xbox_360_dlc() {
        let d = tmp("x360-dlc");
        let p = d.join("4D5307E6/00000002/58410A9F0C1D2E3F");
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, stfs(0x0000_0002, "SkyRacer", "Bonus Track Pack")).unwrap();
        let found = scan(&[d.clone()]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "SkyRacer");
        assert_eq!(part_of_file(&p).kind, "dlc");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn reads_title_update_names_with_a_long_content_id_suffix() {
        let d = tmp("x360-tu-name");
        let p = d.join("TU_16L61VP_0000004000000.00000000000G1");
        fs::write(&p, stfs(0x000B_0000, "SkyRacer", "")).unwrap();
        assert_eq!(scan(&[d.clone()]).len(), 1);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn ignores_files_without_a_package_magic() {
        let d = tmp("x360-none");
        let mut other = stfs(0x000B_0000, "SkyRacer", "");
        other[..4].copy_from_slice(b"ABCD");
        fs::write(d.join("0123456789ABCDEF"), other).unwrap();
        assert!(scan(&[d.clone()]).is_empty());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn ignores_other_xbox_360_packages() {
        let d = tmp("x360-save");
        fs::write(d.join("0123456789ABCDEF"), stfs(0x0000_0001, "SkyRacer", "")).unwrap();
        assert!(scan(&[d.clone()]).is_empty());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn packages_say_their_games_title_id() {
        let d = tmp("x360-id");
        let p = d.join("0123456789ABCDEF");
        fs::write(&p, stfs(0x0000_0002, "SkyRacer", "")).unwrap();
        assert_eq!(copy_id(&p).as_deref(), Some("4D5307E6"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn an_update_package_is_not_picked_as_the_game() {
        let d = tmp("x360-pick");
        fs::write(d.join("0123456789ABCDEF"), stfs(0x000B_0000, "SkyRacer", "")).unwrap();
        let found = scan(&[d.clone()]);
        let cat = serde_json::json!({ "apps": [{ "id": "skyracer", "game": "SkyRacer", "origin": "x360" }] });
        assert!(match_catalog(&cat, &found).is_empty());
        assert_eq!(files_of(&cat["apps"][0], &found).len(), 1);
        let _ = fs::remove_dir_all(&d);
    }

    fn tmp(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rl-roms-{n}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn write_zip(p: &Path, names: &[&str]) {
        use std::io::Write;
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        let mut z = zip::ZipWriter::new(fs::File::create(p).unwrap());
        for name in names {
            z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(&[0u8; 64]).unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn a_zip_with_one_snes_rom_is_a_snes_copy() {
        let d = tmp("zip-snes");
        let p = d.join("SNES/ActRaiser (USA).zip");
        write_zip(&p, &["ActRaiser (USA).sfc", "readme.txt"]);
        let found = scan(&[d.clone()]);
        let _ = fs::remove_dir_all(&d);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].system.as_str(), found[0].title.as_str()), ("snes", "ActRaiser (USA)"));
    }

    #[test]
    fn a_zip_with_no_rom_inside_is_ignored() {
        let d = tmp("zip-none");
        write_zip(&d.join("Some Game (USA).zip"), &["manual.pdf", "notes.txt"]);
        let found = scan(&[d.clone()]);
        let _ = fs::remove_dir_all(&d);
        assert!(found.is_empty());
    }

    #[test]
    fn a_zip_with_two_roms_is_ignored() {
        let d = tmp("zip-two");
        write_zip(&d.join("Pack.zip"), &["One.sfc", "Two.sfc"]);
        let found = scan(&[d.clone()]);
        let _ = fs::remove_dir_all(&d);
        assert!(found.is_empty());
    }

    #[test]
    fn a_7z_with_one_rom_is_a_copy() {
        let d = tmp("7z-gba");
        let src = d.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("Game.gba"), [0u8; 64]).unwrap();
        let p = d.join("Game.7z");
        sevenz_rust::compress_to_path(&src, &p).unwrap();
        let found = identify(&p, 1);
        let _ = fs::remove_dir_all(&d);
        assert_eq!(found.map(|r| r.system), Some("gba".to_string()));
    }

    #[test]
    fn an_arcade_set_in_an_arcade_folder_is_found() {
        let d = tmp("zip-arcade");
        let p = d.join("ARCADE/hopr.zip");
        write_zip(&p, &["963-a23.k8", "963-a24.k10", "963-x22.h22"]);
        let found = scan(&[d.clone()]);
        let _ = fs::remove_dir_all(&d);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].system.as_str(), found[0].title.as_str()), ("arcade", "hopr"));
    }

    #[test]
    fn an_arcade_set_outside_an_arcade_folder_is_ignored() {
        // Not named for arcade itself: the temporary folder is one of the folders looked at.
        let d = tmp("zip-set-elsewhere");
        write_zip(&d.join("Downloads/hopr.zip"), &["963-a23.k8", "963-a24.k10"]);
        let found = scan(&[d.clone()]);
        let _ = fs::remove_dir_all(&d);
        assert!(found.is_empty());
    }

    #[test]
    fn an_arcade_set_is_matched_by_its_listed_set_name() {
        let set = |name: &str| RomFile { path: PathBuf::from(format!("D:/ROMS/ARCADE/{name}.zip")), system: "arcade".into(), title: name.into(), region: String::new(), size: 1 };
        let found = vec![set("hopr2"), set("hopr")];
        let cat = serde_json::json!({ "apps": [{ "id": "hopper-port", "game": "Hopper Arcade", "origin": "arcade", "copy_ids": ["hopr"] }] });
        let picked = match_catalog(&cat, &found);
        assert_eq!(picked["hopper-port"].title, "hopr");
        assert_eq!(files_of(&cat["apps"][0], &found).len(), 1);
    }

    /// A tiny .wua: a names table holding these names, then the ZArchive footer pointing at it.
    fn wua(names: &[&str]) -> Vec<u8> {
        let mut table = vec![];
        for n in names {
            table.push(n.len() as u8);
            table.extend_from_slice(n.as_bytes());
        }
        let mut b = vec![0u8; 16];
        let at = b.len() as u64;
        b.extend_from_slice(&table);
        let mut foot = [0u8; ZARCHIVE_FOOTER];
        foot[32..40].copy_from_slice(&at.to_be_bytes());
        foot[40..48].copy_from_slice(&(table.len() as u64).to_be_bytes());
        foot[140..144].copy_from_slice(&ZARCHIVE_MAGIC.to_be_bytes());
        b.extend_from_slice(&foot);
        b
    }

    #[test]
    fn a_wua_is_a_wii_u_copy() {
        let d = tmp("wua");
        let p = d.join("SkyRacer HD (USA).wua");
        fs::write(&p, wua(&["0005000010143500_v0", "code", "app.xml"])).unwrap();
        let found = scan(&[d.clone()]);
        let id = copy_id(&p);
        let _ = fs::remove_dir_all(&d);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].system, "wiiu");
        assert_eq!(id.as_deref(), Some("0005000010143500"));
    }

    #[test]
    fn a_wua_is_matched_to_its_game_by_name() {
        let d = tmp("wua-match");
        fs::write(d.join("SkyRacer HD (USA).wua"), wua(&["0005000010143500_v0"])).unwrap();
        let found = scan(&[d.clone()]);
        let _ = fs::remove_dir_all(&d);
        let cat = serde_json::json!({ "apps": [{ "id": "skyracerhd", "game": "SkyRacer HD", "origin": "wiiu" }] });
        assert!(match_catalog(&cat, &found).contains_key("skyracerhd"));
    }

    #[test]
    fn a_wua_with_only_an_update_says_its_games_id() {
        let d = tmp("wua-update");
        let p = d.join("Game Update.wua");
        fs::write(&p, wua(&["0005000e10143500_v16"])).unwrap();
        let id = copy_id(&p);
        let _ = fs::remove_dir_all(&d);
        assert_eq!(id.as_deref(), Some("0005000010143500"));
    }

    #[test]
    fn a_wua_named_dlc_that_holds_the_game_is_the_game() {
        let d = tmp("wua-dlc");
        let p = d.join("HOPPER 3 (USA) (DLC) (v96).wua");
        fs::write(&p, wua(&["000500001012bd00_v16", "0005000c1012bd00_v16"])).unwrap();
        let kind = part_of_file(&p).kind;
        let _ = fs::remove_dir_all(&d);
        assert_eq!(kind, "game");
    }

    #[test]
    fn a_wua_without_its_footer_is_ignored() {
        let d = tmp("wua-bad");
        fs::write(d.join("Game.wua"), [0u8; 512]).unwrap();
        let found = scan(&[d.clone()]);
        let _ = fs::remove_dir_all(&d);
        assert!(found.is_empty());
    }

    #[test]
    fn wux_and_wud_images_are_wii_u_copies() {
        let d = tmp("wux-wud");
        let mut wux = vec![0u8; 64];
        wux[..4].copy_from_slice(b"WUX0");
        fs::write(d.join("Game A.wux"), wux).unwrap();
        let mut wud = vec![0u8; 64];
        wud[..10].copy_from_slice(b"WUP-P-ARDE");
        fs::write(d.join("Game B.wud"), wud).unwrap();
        let found = scan(&[d.clone()]);
        let _ = fs::remove_dir_all(&d);
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|r| r.system == "wiiu"));
    }

    /// A disc image big enough to be scanned (sparse), with a GameCube or Wii header where its
    /// container keeps it: at 0 for a plain image (no container magic), at 0x58 after an RVZ's or
    /// WIA's own magic.
    fn nintendo_image(p: &Path, container: &[u8], magic_at: usize, magic: [u8; 4]) {
        let mut head = vec![0u8; 0x100];
        let at = if container.is_empty() { 0 } else { 0x58 };
        head[..container.len()].copy_from_slice(container);
        head[at..at + 6].copy_from_slice(b"RHPE01");
        head[at + magic_at..at + magic_at + 4].copy_from_slice(&magic);
        fs::write(p, head).unwrap();
        fs::OpenOptions::new().write(true).open(p).unwrap().set_len(60 * 1024 * 1024).unwrap();
    }

    const WII_MAGIC: [u8; 4] = [0x5D, 0x1C, 0x9E, 0xA3];
    const GC_MAGIC: [u8; 4] = [0xC2, 0x33, 0x9F, 0x3D];

    #[test]
    fn an_rvz_with_a_wii_header_is_wii() {
        let d = tmp("rvz-wii");
        let p = d.join("SkyRacer Wii (USA).rvz");
        nintendo_image(&p, b"RVZ\x01", 0x18, WII_MAGIC);
        let found = identify(&p, 60 * 1024 * 1024);
        let _ = fs::remove_dir_all(&d);
        assert_eq!(found.map(|r| r.system), Some("wii".to_string()));
    }

    #[test]
    fn an_rvz_with_a_gamecube_header_is_gamecube() {
        let d = tmp("rvz-gc");
        let p = d.join("Hopper (USA).rvz");
        nintendo_image(&p, b"RVZ\x01", 0x1C, GC_MAGIC);
        let found = identify(&p, 60 * 1024 * 1024);
        let _ = fs::remove_dir_all(&d);
        assert_eq!(found.map(|r| r.system), Some("gc".to_string()));
    }

    #[test]
    fn a_gcm_with_a_wii_header_is_wii() {
        let d = tmp("gcm-wii");
        let p = d.join("Party Hop 8 (USA).gcm");
        nintendo_image(&p, b"", 0x18, WII_MAGIC);
        let found = identify(&p, 60 * 1024 * 1024);
        let _ = fs::remove_dir_all(&d);
        assert_eq!(found.map(|r| r.system), Some("wii".to_string()));
    }

    #[test]
    fn an_iso_with_a_wii_header_is_wii() {
        let d = tmp("iso-wii");
        let p = d.join("Star Hopper M (USA).iso");
        nintendo_image(&p, b"", 0x18, WII_MAGIC);
        let found = identify(&p, 60 * 1024 * 1024);
        let _ = fs::remove_dir_all(&d);
        assert_eq!(found.map(|r| r.system), Some("wii".to_string()));
    }

    #[test]
    fn a_wia_with_a_wii_header_is_found_as_wii() {
        let d = tmp("wia-wii");
        let p = d.join("No More Heroes (USA).wia");
        nintendo_image(&p, b"WIA\x01", 0x18, WII_MAGIC);
        let found = identify(&p, 60 * 1024 * 1024);
        let _ = fs::remove_dir_all(&d);
        assert_eq!(found.map(|r| r.system), Some("wii".to_string()));
    }

    #[test]
    fn an_rvz_whose_header_cant_be_read_keeps_its_file_type_console() {
        let d = tmp("rvz-unknown");
        let p = d.join("Some Game.rvz");
        fs::write(&p, [0u8; 0x100]).unwrap();
        fs::OpenOptions::new().write(true).open(&p).unwrap().set_len(60 * 1024 * 1024).unwrap();
        let found = identify(&p, 60 * 1024 * 1024);
        let _ = fs::remove_dir_all(&d);
        assert_eq!(found.map(|r| r.system), Some("gc".to_string()));
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

    /// A tiny main.npdm: its META header pointing at an ACI0 that holds this program ID.
    fn npdm(id: u64) -> Vec<u8> {
        let mut b = vec![0u8; 0x100];
        b[..4].copy_from_slice(b"META");
        b[0x70..0x74].copy_from_slice(&0x80u32.to_le_bytes());
        b[0x80..0x84].copy_from_slice(b"ACI0");
        b[0x90..0x98].copy_from_slice(&id.to_le_bytes());
        b
    }

    /// A tiny NSP: a PFS0 header listing these files, padded like a real one's start.
    fn pfs0(files: &[&str]) -> Vec<u8> {
        let names: Vec<u8> = files.iter().flat_map(|n| n.bytes().chain([0])).collect();
        let mut b = b"PFS0".to_vec();
        b.extend((files.len() as u32).to_le_bytes());
        b.extend((names.len() as u32).to_le_bytes());
        b.extend([0u8; 4]);
        b.extend(vec![0u8; files.len() * 0x18]);
        b.extend(names);
        b.resize(0x400, 0);
        b
    }

    #[test]
    fn an_extracted_switch_game_says_its_title_id() {
        let d = tmp("switch-npdm");
        let game = d.join("Astral Chain");
        fs::create_dir_all(game.join("base/exefs")).unwrap();
        fs::write(game.join("base/exefs/main.npdm"), npdm(0x0100_7300_020F_A000)).unwrap();
        assert_eq!(copy_id(&game).as_deref(), Some("01007300020FA000"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_flat_extracted_switch_game_says_its_title_id() {
        let d = tmp("switch-flat");
        let game = d.join("Game");
        fs::create_dir_all(game.join("ExeFS")).unwrap();
        fs::write(game.join("ExeFS/MAIN.NPDM"), npdm(0x0100_1520_0002_2000)).unwrap();
        assert_eq!(copy_id(&game).as_deref(), Some("0100152000022000"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn an_nsp_ticket_says_its_title_id() {
        let d = tmp("switch-tik");
        let p = d.join("ASTRAL CHAIN.nsp");
        fs::write(&p, pfs0(&["f37d68f25842223eaf7bcca5ffea058b.cnmt.nca", "01007300020fa0000000000000000008.tik"])).unwrap();
        assert_eq!(copy_id(&p).as_deref(), Some("01007300020FA000"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn an_nsp_without_a_ticket_goes_by_its_name() {
        let d = tmp("switch-name");
        let p = d.join("ASTRAL CHAIN [01007300020FA800][v65536].nsp");
        fs::write(&p, pfs0(&["edf3faff382e616118adb122a78d9a9f.cnmt.nca"])).unwrap();
        assert_eq!(copy_id(&p).as_deref(), Some("01007300020FA800"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_switch_2_dump_goes_by_its_name() {
        let d = tmp("switch2-name");
        let p = d.join("Mario Kart World [0400C3F00006E000].nup");
        fs::write(&p, [0u8; 0x400]).unwrap();
        assert_eq!(copy_id(&p).as_deref(), Some("0400C3F00006E000"));
        assert_eq!(part_of("Pokemon Pokopia [0400A3A01EE4C800][v655360].nup").kind, "update");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn an_nsp_that_says_nothing_has_no_id() {
        let d = tmp("switch-none");
        let p = d.join("Metroid Dread.nsp");
        fs::write(&p, pfs0(&["1163c288acb2ca356cf083a314e469c5.nca"])).unwrap();
        assert_eq!(copy_id(&p), None);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_listed_switch_id_turns_away_another_games_copy() {
        let d = tmp("switch-fits");
        let p = d.join("Mario Kart 8 Deluxe.nsp");
        fs::write(&p, pfs0(&["01001520000220000000000000000000.tik"])).unwrap();
        let mk8 = serde_json::json!({ "copy_ids": ["0100152000022000"] });
        let other = serde_json::json!({ "copy_ids": ["01007300020FA000"] });
        assert!(fits_copy_ids(&mk8, &p).is_ok());
        assert_eq!(fits_copy_ids(&other, &p), Err("0100152000022000".to_string()));
        let _ = fs::remove_dir_all(&d);
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
    fn skips_markdown_files() {
        let root = tmp("markdown-scan");
        fs::write(root.join("README.md"), "# Some Recomp\n".repeat(1024)).unwrap();

        let found = scan(&[root.clone()]);
        fs::remove_dir_all(root).unwrap();
        assert!(found.is_empty());
    }

    #[test]
    fn scans_mega_drive_md_roms() {
        let root = tmp("mega-drive-scan");
        let mut cart = vec![0u8; 8192];
        cart[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
        let sonic = root.join("Sonic the Hedgehog (USA, Europe).md");
        fs::write(&sonic, cart).unwrap();

        let found = scan(&[root.clone()]);
        fs::remove_dir_all(root).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, sonic);
        assert_eq!(found[0].system, "genesis");
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
    fn a_sequels_disc_isnt_the_earlier_game() {
        let f = |name: &str| RomFile { path: PathBuf::from(format!("{name}.iso")), system: system_for("iso").into(), title: name.into(), region: String::new(), size: 1 << 20 };
        assert!(!matches("Gears of War", &f("Gears of War 2 (USA)")));
        assert!(!matches("God of War", &f("God of War II (USA)")));
        assert!(matches("Gears of War 2", &f("Gears of War 2 (USA)")));
    }

    #[test]
    fn a_disc_number_or_revision_is_still_the_same_game() {
        let f = |name: &str| RomFile { path: PathBuf::from(format!("{name}.iso")), system: system_for("iso").into(), title: name.into(), region: String::new(), size: 1 << 20 };
        assert!(matches("Gears of War", &f("Gears of War Disc 2")));
        assert!(matches("Gears of War", &f("Gears of War Track 02")));
        assert!(matches("Gears of War", &f("Gears of War 1.2")));
        assert!(matches("Gears of War", &f("Gears of War (USA) (Disc 2)")));
        assert!(matches("Banjo-Kazooie", &f("Banjo-Kazooie (USA) (Rev 1)")));
    }

    #[test]
    fn a_later_roman_numeral_isnt_the_earlier_game() {
        let f = |name: &str| RomFile { path: PathBuf::from(format!("{name}.iso")), system: system_for("iso").into(), title: name.into(), region: String::new(), size: 1 << 20 };
        assert!(!matches("Final Fantasy XII", &f("Final Fantasy XIII (USA)")));
        assert!(!matches("Final Fantasy VII", &f("Final Fantasy VIII (USA)")));
    }

    #[test]
    fn the_same_roman_numeral_is_still_the_same_game() {
        let f = |name: &str| RomFile { path: PathBuf::from(format!("{name}.iso")), system: system_for("iso").into(), title: name.into(), region: String::new(), size: 1 << 20 };
        assert!(matches("Final Fantasy XII", &f("Final Fantasy XII (USA)")));
        assert!(matches("Final Fantasy VII", &f("Final Fantasy VII (USA) (Disc 2)")));
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

    /// A PARAM.SFO with just a title and a title ID.
    pub(crate) fn sfo(title: &str, id: &str) -> Vec<u8> {
        let keys = b"TITLE\0TITLE_ID\0";
        let title = format!("{title}\0").into_bytes();
        let id = format!("{id}\0").into_bytes();
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
        sfo
    }

    /// An extracted PS4 game folder: eboot.bin and sce_sys/param.sfo.
    pub(crate) fn ps4_folder(at: &Path, title: &str, id: &str) {
        fs::create_dir_all(at.join("sce_sys")).unwrap();
        fs::write(at.join("eboot.bin"), b"").unwrap();
        fs::write(at.join("sce_sys/param.sfo"), sfo(title, id)).unwrap();
    }

    const DIABLO: &str = "Diablo III: Reaper of Souls – Ultimate Evil Edition";

    #[test]
    fn a_ps4_folder_id_is_read_from_its_param_sfo() {
        let root = tmp("ps4-id");
        let game = root.join("PS4/Diablo 3/CUSA00242");
        ps4_folder(&game, DIABLO, "CUSA00242");
        assert_eq!(copy_id(&game).as_deref(), Some("CUSA00242"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_ps4_folder_is_matched_by_a_title_id_the_catalog_lists() {
        let root = tmp("ps4-listed");
        let game = root.join("PS4/Diablo 3/CUSA00242");
        ps4_folder(&game, DIABLO, "CUSA00242");
        let found = scan(&[root.clone()]);
        // "Diablo III" is 2 of the title's 7 words: too few to match by name.
        let by_name = serde_json::json!({"apps": [{"id": "sanctuary", "game": "Diablo III", "origin": "ps4"}]});
        assert!(match_catalog(&by_name, &found).is_empty());
        let by_id = serde_json::json!({"apps": [
            {"id": "sanctuary", "game": "Diablo III", "origin": "ps4", "copy_ids": ["CUSA00243", "CUSA00242"]}
        ]});
        assert_eq!(match_catalog(&by_id, &found)["sanctuary"].path, game);
        assert_eq!(files_of(&by_id["apps"][0], &found).len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_ps4_folder_with_an_unlisted_title_id_is_not_matched_by_name() {
        let root = tmp("ps4-unlisted");
        ps4_folder(&root.join("PS4/CUSA00242"), DIABLO, "CUSA00242");
        let found = scan(&[root.clone()]);
        let cat = serde_json::json!({"apps": [{"id": "sanctuary", "game": DIABLO, "origin": "ps4", "copy_ids": ["CUSA00433"]}]});
        assert!(match_catalog(&cat, &found).is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    /// A PS3 game folder laid out the way PSN games are installed: PARAM.SFO and USRDIR/EBOOT.BIN.
    fn psn_folder(at: &Path, title: &str, id: &str) {
        fs::create_dir_all(at.join("USRDIR")).unwrap();
        fs::write(at.join("USRDIR/EBOOT.BIN"), b"").unwrap();
        fs::write(at.join("PARAM.SFO"), sfo(title, id)).unwrap();
    }

    #[test]
    fn scans_psn_game_folders_as_one_ps3_game() {
        let root = tmp("psn");
        let game = root.join("dev_hdd0/game/NPUB30467");
        psn_folder(&game, "Resident Evil Code: Veronica X", "NPUB30467");
        // Something inside that would be found on its own if the folder were looked through.
        fs::write(game.join("USRDIR/0123456789ABCDEF"), stfs(0x000B_0000, "SkyRacer", "")).unwrap();
        let found = scan(&[root.clone()]);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].system.as_str(), found[0].title.as_str()), ("ps3", "Resident Evil Code: Veronica X"));
        assert_eq!(found[0].path, game);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_disc_games_update_installed_like_a_psn_game_is_not_a_game() {
        let root = tmp("psn-update");
        psn_folder(&root.join("BLUS31197"), "Drakengard 3", "BLUS31197");
        assert!(scan(&[root.clone()]).is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ps3_disc_folders_are_still_found() {
        let root = tmp("ps3-disc");
        let disc = root.join("Demons Souls");
        fs::create_dir_all(disc.join("PS3_GAME/USRDIR")).unwrap();
        fs::write(disc.join("PS3_DISC.SFB"), b"").unwrap();
        fs::write(disc.join("PS3_GAME/USRDIR/EBOOT.BIN"), b"").unwrap();
        fs::write(disc.join("PS3_GAME/PARAM.SFO"), sfo("Demon's Souls", "BLES00932")).unwrap();
        let found = scan(&[root.clone()]);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].system.as_str(), found[0].path.as_path()), ("ps3", disc.as_path()));
        let _ = fs::remove_dir_all(root);
    }

    /// A tiny PS3 package header with this content ID.
    fn ps3_pkg(content_id: &str) -> Vec<u8> {
        let mut b = vec![0u8; 0x100];
        b[..4].copy_from_slice(b"\x7FPKG");
        b[0x30..0x30 + content_id.len()].copy_from_slice(content_id.as_bytes());
        b
    }

    #[test]
    fn scans_ps3_packages_with_their_title_id() {
        let d = tmp("ps3-pkg");
        let p = d.join("Drakengard 3.pkg");
        fs::write(&p, ps3_pkg("EP0082-BLES01953_00-DOD3000000000001")).unwrap();
        let found = scan(&[d.clone()]);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].system.as_str(), found[0].title.as_str()), ("ps3", "Drakengard 3"));
        assert_eq!(copy_id(&p).as_deref(), Some("BLES01953"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_ps3_patch_package_is_an_update() {
        let d = tmp("ps3-patch");
        let p = d.join("UP0082-BLUS31197_00-DOD3PATCH0000101.pkg");
        fs::write(&p, ps3_pkg("UP0082-BLUS31197_00-DOD3PATCH0000101")).unwrap();
        assert_eq!(part_of_file(&p).kind, "update");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn ignores_pkg_files_without_the_ps3_magic() {
        let d = tmp("ps4-pkg");
        let mut ps4 = ps3_pkg("UP0082-CUSA01234_00-SOMEGAME00000001");
        ps4[..4].copy_from_slice(b"\x7FCNT");
        fs::write(d.join("Some Game.pkg"), ps4).unwrap();
        assert!(scan(&[d.clone()]).is_empty());
        let _ = fs::remove_dir_all(&d);
    }

    /// A raw (2352-byte sector) PlayStation data track: ISO 9660 saying PLAYSTATION, with a
    /// SYSTEM.CNF whose BOOT line names SLUS_000.67.
    fn raw_ps1_track() -> Vec<u8> {
        let sector = |n: usize| n * 2352 + 24;
        let mut b = vec![0u8; 24 * 2352];
        let pvd = sector(16);
        b[pvd + 1..pvd + 6].copy_from_slice(b"CD001");
        b[pvd + 8..pvd + 19].copy_from_slice(b"PLAYSTATION");
        b[pvd + 158..pvd + 162].copy_from_slice(&20u32.to_le_bytes());
        b[pvd + 166..pvd + 170].copy_from_slice(&2048u32.to_le_bytes());
        let dir = sector(20);
        b[dir] = 46;
        b[dir + 2..dir + 6].copy_from_slice(&22u32.to_le_bytes());
        b[dir + 32] = 12;
        b[dir + 33..dir + 45].copy_from_slice(b"SYSTEM.CNF;1");
        let cnf = sector(22);
        let text = b"BOOT = cdrom:\\SLUS_000.67;1\r\n";
        b[cnf..cnf + text.len()].copy_from_slice(text);
        b
    }

    /// A raw track whose first sector carries a Sega header after its 16-byte sync and address.
    fn raw_sega_track(magic: &[u8]) -> Vec<u8> {
        let mut b = vec![0u8; 4 * 2352];
        b[0x10..0x10 + magic.len()].copy_from_slice(magic);
        b
    }

    /// A .cue for one data track and, when given, one music track.
    fn cue(data: &str, audio: Option<&str>) -> String {
        let mut s = format!("FILE \"{data}\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\n");
        if let Some(a) = audio {
            s += &format!("FILE \"{a}\" BINARY\n  TRACK 02 AUDIO\n    INDEX 01 00:00:00\n");
        }
        s
    }

    #[test]
    fn a_cue_with_two_bin_tracks_is_one_copy_of_their_size() {
        let d = tmp("cue-two");
        fs::write(d.join("Game (Track 1).bin"), raw_ps1_track()).unwrap();
        fs::write(d.join("Game (Track 2).bin"), vec![0u8; 1000]).unwrap();
        fs::write(d.join("Game.cue"), cue("Game (Track 1).bin", Some("Game (Track 2).bin"))).unwrap();
        let found = scan(&[d.clone()]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, d.join("Game.cue"));
        assert_eq!(found[0].size, 24 * 2352 + 1000);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_cues_bin_tracks_are_not_listed() {
        let d = tmp("cue-bins");
        fs::write(d.join("Game (Track 1).bin"), raw_ps1_track()).unwrap();
        fs::write(d.join("Game (Track 2).bin"), vec![0u8; 1000]).unwrap();
        fs::write(d.join("Game.cue"), cue("Game (Track 1).bin", Some("Game (Track 2).bin"))).unwrap();
        let found = scan(&[d.clone()]);
        assert!(found.iter().all(|r| ext_of(&r.path) != "bin"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_disc_image_a_cue_names_is_not_listed_again() {
        let d = tmp("cue-iso");
        // Big enough to be found on its own as a disc image; only its length matters here.
        fs::File::create(d.join("Game.iso")).unwrap().set_len(60 * 1024 * 1024).unwrap();
        fs::write(d.join("Game.cue"), "FILE \"Game.iso\" BINARY\n  TRACK 01 MODE1/2048\n").unwrap();
        let found = scan(&[d.clone()]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, d.join("Game.cue"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_saturn_and_a_ps1_raw_track_are_told_apart() {
        let d = tmp("cue-consoles");
        fs::write(d.join("ps1.bin"), raw_ps1_track()).unwrap();
        fs::write(d.join("saturn.bin"), raw_sega_track(b"SEGA SEGASATURN")).unwrap();
        fs::write(d.join("PS1 Game.cue"), cue("ps1.bin", None)).unwrap();
        fs::write(d.join("Saturn Game.cue"), cue("saturn.bin", None)).unwrap();
        let mut found: Vec<(String, String)> = scan(&[d.clone()]).into_iter().map(|r| (r.title, r.system)).collect();
        found.sort();
        assert_eq!(found, vec![("PS1 Game".to_string(), "ps1".to_string()), ("Saturn Game".to_string(), "saturn".to_string())]);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_cue_gives_its_data_tracks_disc_id() {
        let d = tmp("cue-id");
        fs::write(d.join("track.bin"), raw_ps1_track()).unwrap();
        fs::write(d.join("Game.cue"), cue("track.bin", None)).unwrap();
        assert_eq!(copy_id(&d.join("Game.cue")).as_deref(), Some("SLUS00067"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_cue_finds_its_tracks_in_another_case_and_with_backslashes() {
        let d = tmp("cue-case");
        fs::create_dir_all(d.join("tracks")).unwrap();
        fs::write(d.join("tracks/game.bin"), raw_ps1_track()).unwrap();
        fs::write(d.join("Game.cue"), cue("Tracks\\GAME.BIN", None)).unwrap();
        let found = scan(&[d.clone()]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].size, 24 * 2352);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_cue_with_a_missing_track_is_still_found() {
        let d = tmp("cue-missing");
        let sheet = cue("Game (Track 1).bin", None);
        fs::write(d.join("Game.cue"), &sheet).unwrap();
        let found = scan(&[d.clone()]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].size, sheet.len() as u64);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_gdi_set_is_one_dreamcast_copy() {
        let d = tmp("gdi");
        fs::write(d.join("track01.bin"), vec![0u8; 2352]).unwrap();
        fs::write(d.join("track02.raw"), vec![0u8; 3000]).unwrap();
        fs::write(d.join("track 03.bin"), raw_sega_track(b"SEGA SEGAKATANA")).unwrap();
        let sheet = "3\n1 0 4 2352 track01.bin 0\n2 756 0 2352 track02.raw 0\n3 45000 4 2352 \"track 03.bin\" 0\n";
        fs::write(d.join("Game.gdi"), sheet).unwrap();
        let found = scan(&[d.clone()]);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].path.clone(), found[0].system.as_str()), (d.join("Game.gdi"), "dc"));
        assert_eq!(found[0].size, 2352 + 3000 + 4 * 2352);
        let _ = fs::remove_dir_all(&d);
    }

    /// Three discs, each a .cue with one raw PlayStation track, and an .m3u listing them.
    fn three_disc_set(d: &Path) {
        for n in 1..=3 {
            fs::write(d.join(format!("Game (Disc {n}).bin")), raw_ps1_track()).unwrap();
            fs::write(d.join(format!("Game (Disc {n}).cue")), cue(&format!("Game (Disc {n}).bin"), None)).unwrap();
        }
        fs::write(d.join("Game.m3u"), "#EXTM3U\nGame (Disc 1).cue\nGame (Disc 2).cue\nGame (Disc 3).cue\n").unwrap();
    }

    #[test]
    fn an_m3u_of_three_discs_is_one_copy() {
        let d = tmp("m3u");
        three_disc_set(&d);
        let found = scan(&[d.clone()]);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].path.clone(), found[0].system.as_str()), (d.join("Game.m3u"), "ps1"));
        assert_eq!(found[0].size, 3 * 24 * 2352);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn an_m3u_keeps_its_disc_order() {
        let d = tmp("m3u-order");
        for name in ["a.bin", "b.bin"] {
            fs::write(d.join(name), raw_ps1_track()).unwrap();
        }
        fs::write(d.join("a.cue"), cue("a.bin", None)).unwrap();
        fs::write(d.join("b.cue"), cue("b.bin", None)).unwrap();
        fs::write(d.join("Game.m3u"), "b.cue\na.cue\n").unwrap();
        let set = folded(&d.join("Game.m3u")).unwrap();
        assert_eq!(set.data, vec![d.join("b.bin"), d.join("a.bin")]);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_cue_is_matched_to_its_game() {
        let d = tmp("cue-match");
        fs::write(d.join("track.bin"), raw_ps1_track()).unwrap();
        fs::write(d.join("Some Game (USA).cue"), cue("track.bin", None)).unwrap();
        let cat = serde_json::json!({ "apps": [{ "id": "some", "game": "Some Game", "origin": "ps1" }] });
        let found = scan(&[d.clone()]);
        assert_eq!(match_catalog(&cat, &found).get("some").map(|r| r.path.clone()), Some(d.join("Some Game (USA).cue")));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn noise_words_go() {
        assert_eq!(words("The Legend of Zelda - Majora's Mask (USA) [!]"), vec!["legend", "zelda", "majoras", "mask"]);
    }
}

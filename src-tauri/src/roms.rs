//! Finding the player's own games in their ROM and backup folders, and which ones have a recomp.
//!
//! N64 ROMs carry the game's name in their header, so those are matched on it (whatever the file
//! is called). Disc images and game folders are matched on their file or folder name. Nothing is
//! read beyond a ROM's 64-byte header, and nothing leaves the PC.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RomFile {
    pub path: PathBuf,
    /// n64, x360, gc, wii, ps2 or "" when the kind of file doesn't say.
    pub system: String,
    /// The name inside the ROM's header, or the file's name.
    pub title: String,
    /// US, Europe, Japan or "".
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub size: u64,
}

const N64: [&str; 3] = ["z64", "n64", "v64"];
const DISC: [&str; 6] = ["iso", "gcm", "rvz", "wbfs", "ciso", "chd"];

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
        _ => "",
    }
}

/// One file: a ROM, a disc image, or an Xbox 360 game's default.xex (which stands for its folder).
pub fn identify(p: &Path, size: u64) -> Option<RomFile> {
    let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
    if N64.contains(&ext.as_str()) {
        let (title, region) = n64_header(p).unwrap_or((stem.clone(), String::new()));
        Some(RomFile { path: p.to_path_buf(), system: "n64".into(), title, region, size })
    } else if DISC.contains(&ext.as_str()) && size > 50 * 1024 * 1024 {
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

/// Look through the folders for ROMs, disc images and Xbox 360 game folders (a folder holding
/// default.xex). Stops at a few thousand files so a whole drive can't hang it.
pub fn scan(dirs: &[PathBuf]) -> Vec<RomFile> {
    let mut out = vec![];
    let mut seen = 0usize;
    for d in dirs {
        let mut stack = vec![(d.clone(), 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            let Ok(rd) = fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                seen += 1;
                if seen > 50_000 {
                    return out;
                }
                let p = e.path();
                let Ok(meta) = e.metadata() else { continue };
                if meta.is_dir() {
                    let name = e.file_name().to_string_lossy().to_lowercase();
                    if name.starts_with('.') || name == "node_modules" || name == "windows" {
                        continue;
                    }
                    if depth < 6 {
                        stack.push((p, depth + 1));
                    }
                    continue;
                }
                if let Some(r) = identify(&p, meta.len()) {
                    out.push(r);
                }
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
    let all = |pool: &[String]| want.iter().all(|w| pool.iter().any(|h| h == w || (w.len() > 4 && h.starts_with(w.as_str()))));
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
            .filter(|r| r.system.is_empty() || origin.is_empty() || r.system == origin)
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
    fn noise_words_go() {
        assert_eq!(words("The Legend of Zelda - Majora's Mask (USA) [!]"), vec!["legend", "zelda", "majoras", "mask"]);
    }
}

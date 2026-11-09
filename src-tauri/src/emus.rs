//! The game folders the player's emulators already know about (Dolphin, RPCS3, PCSX2, RetroArch,
//! Cemu, Ryujinx): offered as places to look for their copies, so Easy Setup finds them without
//! picking folders by hand. Only read, never changed.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct Found {
    /// The emulator's name, as players know it.
    pub emu: String,
    pub path: PathBuf,
}

/// Every folder the emulators on this PC list for their games, that's there, once each.
pub fn find() -> Vec<Found> {
    let home = dirs::home_dir().unwrap_or_default();
    let config = dirs::config_dir().unwrap_or_else(|| home.join(".config"));
    let docs = dirs::document_dir().unwrap_or_else(|| home.join("Documents"));
    let data = dirs::data_dir().unwrap_or_else(|| home.join(".local/share"));
    let flat = |app: &str, rest: &str| home.join(".var/app").join(app).join(rest);
    let mut out: Vec<Found> = vec![];
    let mut add = |emu: &str, dirs: Vec<PathBuf>| {
        for d in dirs {
            if d.is_dir() && !out.iter().any(|f| same(&f.path, &d)) {
                out.push(Found { emu: emu.into(), path: d });
            }
        }
    };
    // Dolphin: [General] ISOPath0, ISOPath1… in Dolphin.ini.
    for ini in [
        config.join("Dolphin Emulator/Config/Dolphin.ini"),
        docs.join("Dolphin Emulator/Config/Dolphin.ini"),
        config.join("dolphin-emu/Dolphin.ini"),
        flat("org.DolphinEmu.dolphin-emu", "config/dolphin-emu/Dolphin.ini"),
        home.join("Library/Application Support/Dolphin/Config/Dolphin.ini"),
    ] {
        if let Ok(t) = std::fs::read_to_string(&ini) {
            add("Dolphin", dolphin(&t));
        }
    }
    // PCSX2: [GameList] Paths and RecursivePaths in PCSX2.ini.
    for ini in [
        docs.join("PCSX2/inis/PCSX2.ini"),
        config.join("PCSX2/inis/PCSX2.ini"),
        flat("net.pcsx2.PCSX2", "config/PCSX2/inis/PCSX2.ini"),
        home.join("Library/Application Support/PCSX2/inis/PCSX2.ini"),
    ] {
        if let Ok(t) = std::fs::read_to_string(&ini) {
            add("PCSX2", pcsx2(&t));
        }
    }
    // RPCS3: games.yml lists each game's folder; the folders they're in.
    for yml in [
        config.join("rpcs3/games.yml"),
        flat("net.rpcs3.RPCS3", "config/rpcs3/games.yml"),
        home.join("Library/Application Support/rpcs3/games.yml"),
    ] {
        if let Ok(t) = std::fs::read_to_string(&yml) {
            add("RPCS3", rpcs3(&t));
        }
    }
    // RetroArch: its content folder in retroarch.cfg, and the folders its playlists point into.
    for (cfg, lists) in [
        (config.join("RetroArch/retroarch.cfg"), config.join("RetroArch/playlists")),
        (config.join("retroarch/retroarch.cfg"), config.join("retroarch/playlists")),
        (flat("org.libretro.RetroArch", "config/retroarch/retroarch.cfg"), flat("org.libretro.RetroArch", "config/retroarch/playlists")),
        (home.join("Library/Application Support/RetroArch/config/retroarch.cfg"), home.join("Library/Application Support/RetroArch/playlists")),
    ] {
        if let Ok(t) = std::fs::read_to_string(&cfg) {
            add("RetroArch", retroarch_cfg(&t));
        }
        for e in std::fs::read_dir(&lists).into_iter().flatten().flatten() {
            if e.path().extension().map(|x| x == "lpl").unwrap_or(false) {
                if let Ok(t) = std::fs::read_to_string(e.path()) {
                    add("RetroArch", playlist(&t));
                }
            }
        }
    }
    // Cemu: <GamePaths><Entry>…</Entry> in settings.xml.
    for xml in [
        config.join("Cemu/settings.xml"),
        data.join("Cemu/settings.xml"),
        flat("info.cemu.Cemu", "config/Cemu/settings.xml"),
        home.join("Library/Application Support/Cemu/settings.xml"),
    ] {
        if let Ok(t) = std::fs::read_to_string(&xml) {
            add("Cemu", cemu(&t));
        }
    }
    // Ryujinx: "game_dirs" in Config.json.
    for json in [
        config.join("Ryujinx/Config.json"),
        flat("org.ryujinx.Ryujinx", "config/Ryujinx/Config.json"),
        home.join("Library/Application Support/Ryujinx/Config.json"),
    ] {
        if let Ok(t) = std::fs::read_to_string(&json) {
            add("Ryujinx", ryujinx(&t));
        }
    }
    out
}

/// The same folder (on Windows, whatever the letters' case).
pub fn same(a: &Path, b: &Path) -> bool {
    if cfg!(windows) { a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase() } else { a == b }
}
/// Inside the folder `b` (on Windows, whatever the case).
pub fn within(a: &Path, b: &Path) -> bool {
    if cfg!(windows) { Path::new(&a.to_string_lossy().to_lowercase()).starts_with(b.to_string_lossy().to_lowercase()) } else { a.starts_with(b) }
}

fn ini_values(text: &str, section: &str, key: impl Fn(&str) -> bool) -> Vec<String> {
    let mut on = false;
    let mut out = vec![];
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') && l.ends_with(']') {
            on = l[1..l.len() - 1].eq_ignore_ascii_case(section);
            continue;
        }
        if !on {
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            if key(k.trim()) {
                let v = v.trim().trim_matches('"').trim();
                if !v.is_empty() {
                    out.push(v.to_string());
                }
            }
        }
    }
    out
}

fn dolphin(text: &str) -> Vec<PathBuf> {
    ini_values(text, "General", |k| k.starts_with("ISOPath") && k != "ISOPaths").into_iter().map(PathBuf::from).collect()
}

fn pcsx2(text: &str) -> Vec<PathBuf> {
    ini_values(text, "GameList", |k| k == "Paths" || k == "RecursivePaths").into_iter().map(PathBuf::from).collect()
}

/// `SERIAL: /path/to/game/`: the folder each game folder sits in.
fn rpcs3(text: &str) -> Vec<PathBuf> {
    let mut set = BTreeSet::new();
    for line in text.lines() {
        let Some((_, v)) = line.split_once(':') else { continue };
        let v = v.trim().trim_matches(['"', '\'']).trim_end_matches(['/', '\\']);
        if v.is_empty() || !(v.starts_with('/') || v.chars().nth(1) == Some(':')) {
            continue;
        }
        if let Some(parent) = Path::new(v).parent() {
            set.insert(parent.to_path_buf());
        }
    }
    set.into_iter().collect()
}

fn retroarch_cfg(text: &str) -> Vec<PathBuf> {
    text.lines()
        .filter_map(|l| l.split_once('='))
        .filter(|(k, _)| k.trim() == "rgui_browser_directory")
        .map(|(_, v)| v.trim().trim_matches('"').to_string())
        .filter(|v| !v.is_empty() && v != "default")
        .map(PathBuf::from)
        .collect()
}

/// A RetroArch playlist (JSON): the folders its games are in. An entry inside an archive
/// (game.zip#game.sfc) counts as the archive.
fn playlist(text: &str) -> Vec<PathBuf> {
    let mut set = BTreeSet::new();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
        for it in v.get("items").and_then(|i| i.as_array()).into_iter().flatten() {
            if let Some(p) = it.get("path").and_then(|p| p.as_str()) {
                let p = p.split('#').next().unwrap_or(p);
                if let Some(parent) = Path::new(p).parent().filter(|x| !x.as_os_str().is_empty()) {
                    set.insert(parent.to_path_buf());
                }
            }
        }
    }
    set.into_iter().collect()
}

fn cemu(text: &str) -> Vec<PathBuf> {
    let Some(start) = text.find("<GamePaths>") else { return vec![] };
    let end = text[start..].find("</GamePaths>").map(|e| start + e).unwrap_or(text.len());
    text[start..end]
        .split("<Entry>")
        .skip(1)
        .filter_map(|e| e.split("</Entry>").next())
        .map(|e| e.trim().replace("&amp;", "&").replace("&apos;", "'").replace("&quot;", "\""))
        .filter(|e| !e.is_empty())
        .map(PathBuf::from)
        .collect()
}

fn ryujinx(text: &str) -> Vec<PathBuf> {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| v.get("game_dirs").and_then(|d| d.as_array()).cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|d| d.as_str())
        .filter(|d| !d.trim().is_empty())
        .map(PathBuf::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_each_emulators_folders() {
        let d = dolphin("[Interface]\nISOPath0 = nope\n[General]\nISOPaths = 2\nISOPath0 = D:\\Games\\GC\nISOPath1 = /home/a/wii\n[Core]\nX=1");
        assert_eq!(d, vec![PathBuf::from("D:\\Games\\GC"), PathBuf::from("/home/a/wii")]);
        assert_eq!(pcsx2("[GameList]\nRecursivePaths = E:\\PS2\nPaths = \n[UI]\nPaths = x"), vec![PathBuf::from("E:\\PS2")]);
        let r = rpcs3("BLUS30443: /games/ps3/Demons Souls/\nNPUB31419: \"/games/ps3/Journey/\"\nconfig: true\n");
        assert_eq!(r, vec![PathBuf::from("/games/ps3")]);
        assert_eq!(retroarch_cfg("video_driver = \"vulkan\"\nrgui_browser_directory = \"/roms\"\n"), vec![PathBuf::from("/roms")]);
        assert!(retroarch_cfg("rgui_browser_directory = \"default\"").is_empty());
        let pl = playlist(r#"{"version":"1.5","items":[{"path":"/roms/snes/smw.zip#smw.sfc","label":"SMW"},{"path":"/roms/n64/mm.z64"}]}"#);
        assert_eq!(pl, vec![PathBuf::from("/roms/n64"), PathBuf::from("/roms/snes")]);
        assert_eq!(cemu("<content><GamePaths><Entry>C:\\WiiU\\games</Entry><Entry></Entry></GamePaths></content>"), vec![PathBuf::from("C:\\WiiU\\games")]);
        assert_eq!(cemu("<GamePaths><Entry>D:\\A &amp; B</Entry></GamePaths>"), vec![PathBuf::from("D:\\A & B")]);
        assert_eq!(ryujinx(r#"{"game_dirs":["/switch/games",""],"x":1}"#), vec![PathBuf::from("/switch/games")]);
    }
}

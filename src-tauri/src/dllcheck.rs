//! Why Windows says "a DLL it needs is too old" (the game closes with 0xC0000139): an old copy of a
//! runtime DLL the game came with, an old Visual C++ runtime, or an old Vulkan loader from an old
//! graphics driver. And which missing DLLs a game makes itself, from the player's copy.

use serde::Serialize;
use std::path::Path;

/// A Windows program's own version (its VS_FIXEDFILEINFO): major, minor, build, revision. Read off
/// the file, so it works anywhere.
pub fn file_version(path: &Path) -> Option<(u16, u16, u16, u16)> {
    let b = std::fs::read(path).ok().filter(|b| b.len() < 64 << 20)?;
    // The fixed file info starts with its signature, 0xFEEF04BD (little-endian), then its
    // structure version, then the file version (most significant, least significant).
    let sig = [0xBD, 0x04, 0xEF, 0xFE];
    let at = b.windows(4).position(|w| w == sig)?;
    let word = |o: usize| b.get(at + o..at + o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]));
    let (ms, ls) = (word(8)?, word(12)?);
    Some(((ms >> 16) as u16, (ms & 0xffff) as u16, (ls >> 16) as u16, (ls & 0xffff) as u16))
}

/// Runtime DLLs a game can bring an old copy of, which then shadows Windows' newer one.
pub const RUNTIMES: [&str; 9] = [
    "msvcp140.dll",
    "msvcp140_1.dll",
    "msvcp140_2.dll",
    "msvcp140_atomic_wait.dll",
    "vcruntime140.dll",
    "vcruntime140_1.dll",
    "concrt140.dll",
    "vccorlib140.dll",
    "vulkan-1.dll",
];

/// The Visual C++ runtime new recomps are built against (Visual Studio 2022 17.14 and later).
pub const VC_WANTED: (u16, u16) = (14, 44);
/// The Vulkan loader version recomps need (Vulkan 1.3).
pub const VULKAN_WANTED: (u16, u16) = (1, 3);

#[derive(Serialize, Default, Debug, PartialEq)]
pub struct Findings {
    /// Runtime DLLs in the game's folders that are older than Windows' own copy for the same kind
    /// of program (64-bit: System32; 32-bit: SysWOW64).
    pub old_copies: Vec<String>,
    /// Exactly which files those are (only these are ever set aside).
    #[serde(skip)]
    pub old_paths: Vec<std::path::PathBuf>,
    /// Windows' Visual C++ runtime version (for the game's kind), when it's older than VC_WANTED.
    pub vc_old: Option<String>,
    /// Windows' Vulkan loader version, when it's older than VULKAN_WANTED.
    pub vulkan_old: Option<String>,
}

fn shown(v: (u16, u16, u16, u16)) -> String {
    format!("{}.{}.{}", v.0, v.1, v.2)
}

/// Is this Windows program 64-bit (x64 or ARM64)? None when it isn't one.
pub fn pe_x64(path: &Path) -> Option<bool> {
    let mut b = vec![0u8; 4096];
    let n = std::io::Read::read(&mut std::fs::File::open(path).ok()?, &mut b).ok()?;
    let b = &b[..n];
    if !b.starts_with(b"MZ") {
        return None;
    }
    let pe = u32::from_le_bytes(b.get(0x3C..0x40)?.try_into().ok()?) as usize;
    if b.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    match u16::from_le_bytes(b.get(pe + 4..pe + 6)?.try_into().ok()?) {
        0x8664 | 0xAA64 => Some(true),
        0x014C => Some(false),
        _ => None,
    }
}

/// What's old, looking at the game's folders (its own, and its program's). `sys64` is System32,
/// `sys32` SysWOW64 (where a 32-bit program's runtime is); `game_x64` the game program's kind.
/// A copy is only compared with Windows' copy of the same kind, and only when Windows has one.
pub fn check(dirs: &[&Path], sys64: &Path, sys32: &Path, game_x64: bool) -> Findings {
    let mut f = Findings::default();
    for name in RUNTIMES {
        for d in dirs {
            let p = d.join(name);
            let Some(mine) = file_version(&p) else { continue };
            let Some(x64) = pe_x64(&p) else { continue };
            let Some(sys) = file_version(&(if x64 { sys64 } else { sys32 }).join(name)) else { continue };
            if mine < sys && !f.old_paths.contains(&p) {
                f.old_paths.push(p);
                if !f.old_copies.iter().any(|n| n.eq_ignore_ascii_case(name)) {
                    f.old_copies.push(name.to_string());
                }
            }
        }
    }
    if let Some(v) = file_version(&(if game_x64 { sys64 } else { sys32 }).join("msvcp140.dll")) {
        if (v.0, v.1) < VC_WANTED {
            f.vc_old = Some(shown(v));
        }
    }
    if let Some(v) = file_version(&sys64.join("vulkan-1.dll")) {
        if (v.0, v.1) < VULKAN_WANTED {
            f.vulkan_old = Some(shown(v));
        }
    }
    f
}

/// The checked old copies set aside (renamed to name.old, or name.old2 and on if that's taken), so
/// the game loads Windows' own. The ones moved.
pub fn set_aside(paths: &[std::path::PathBuf]) -> Vec<String> {
    let mut moved = vec![];
    for p in paths {
        let (Some(dir), Some(name)) = (p.parent(), p.file_name().map(|n| n.to_string_lossy().to_string())) else { continue };
        let mut to = dir.join(format!("{name}.old"));
        let mut n = 2;
        while to.exists() {
            to = dir.join(format!("{name}.old{n}"));
            n += 1;
        }
        if p.is_file() && std::fs::rename(p, &to).is_ok() && !moved.contains(&name) {
            moved.push(name);
        }
    }
    moved
}

/// The DLLs a game's program can't find, when none of them came with its release (the release's files are
/// known): the game makes them itself from the player's copy (rdr.dll, translated from the disc),
/// so reinstalling won't bring them. Runtime families aren't counted (they're Windows' or the
/// release's).
pub fn made_by_game(missing: &[String], release_files: &[std::path::PathBuf]) -> Vec<String> {
    if release_files.is_empty() {
        return vec![];
    }
    let came: std::collections::HashSet<String> = release_files.iter().filter_map(|p| p.file_name()).map(|n| n.to_string_lossy().to_lowercase()).collect();
    let runtime = |d: &str| {
        ["msvcp", "vcruntime", "concrt", "vcomp", "mfc1", "vccorlib", "d3dx", "xinput", "x3daudio", "xaudio", "d3dcompiler", "lib", "sdl", "vulkan", "api-ms-", "ucrtbase", "openal", "zlib"].iter().any(|p| d.starts_with(p))
    };
    let made: Vec<String> = missing.iter().filter(|d| { let l = d.to_lowercase(); !came.contains(&l) && !runtime(&l) }).cloned().collect();
    // Only when that's all of them: otherwise something the release should have had is missing too.
    if made.len() == missing.len() { made } else { vec![] }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny file with a version block in it, as a DLL has.
    fn fake(path: &Path, v: (u16, u16, u16, u16)) {
        fake_kind(path, v, true)
    }

    fn fake_kind(path: &Path, v: (u16, u16, u16, u16), x64: bool) {
        // MZ, the PE header at 0x40 with its machine, then the version block.
        let mut b = vec![0u8; 64];
        b[0] = b'M';
        b[1] = b'Z';
        b[0x3C] = 0x40;
        b.extend_from_slice(b"PE\0\0");
        b.extend_from_slice(&(if x64 { 0x8664u16 } else { 0x014Cu16 }).to_le_bytes());
        b.extend_from_slice(&[0u8; 18]);
        b.extend_from_slice(&[0xBD, 0x04, 0xEF, 0xFE]);
        b.extend_from_slice(&0x0001_0000u32.to_le_bytes());
        b.extend_from_slice(&(((v.0 as u32) << 16) | v.1 as u32).to_le_bytes());
        b.extend_from_slice(&(((v.2 as u32) << 16) | v.3 as u32).to_le_bytes());
        b.extend_from_slice(&[0u8; 32]);
        std::fs::write(path, b).unwrap();
    }

    #[test]
    fn old_copies_and_old_runtimes_are_found() {
        let d = std::env::temp_dir().join(format!("rl-dll-{}", std::process::id()));
        let (sys, wow, game, bin) = (d.join("System32"), d.join("SysWOW64"), d.join("game"), d.join("game/bin"));
        for x in [&sys, &wow, &bin] {
            std::fs::create_dir_all(x).unwrap();
        }
        fake(&sys.join("msvcp140.dll"), (14, 38, 33130, 0));
        fake(&sys.join("vcruntime140.dll"), (14, 44, 0, 0));
        fake(&sys.join("vulkan-1.dll"), (1, 2, 189, 0));
        fake(&bin.join("msvcp140.dll"), (14, 29, 30133, 0));
        fake(&game.join("msvcp140.dll"), (14, 40, 0, 0)); // newer than Windows': stays
        // A 32-bit copy, and Windows has no 32-bit runtime: nothing to compare it with.
        fake_kind(&bin.join("vcruntime140.dll"), (14, 29, 0, 0), false);
        assert_eq!(file_version(&bin.join("msvcp140.dll")), Some((14, 29, 30133, 0)));
        assert_eq!(pe_x64(&bin.join("vcruntime140.dll")), Some(false));
        let f = check(&[&bin, &game], &sys, &wow, true);
        assert_eq!(f.old_copies, vec!["msvcp140.dll".to_string()]);
        assert_eq!(f.old_paths, vec![bin.join("msvcp140.dll")]);
        assert_eq!(f.vc_old.as_deref(), Some("14.38.33130"));
        assert_eq!(f.vulkan_old.as_deref(), Some("1.2.189"));
        // A 32-bit game: Visual C++ is SysWOW64's, and there's none.
        assert_eq!(check(&[&bin], &sys, &wow, false).vc_old, None);
        assert_eq!(set_aside(&f.old_paths), vec!["msvcp140.dll".to_string()]);
        assert!(bin.join("msvcp140.dll.old").is_file() && !bin.join("msvcp140.dll").exists());
        assert!(game.join("msvcp140.dll").is_file() && bin.join("vcruntime140.dll").is_file());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn dlls_a_game_makes_are_told_apart() {
        let files = vec![std::path::PathBuf::from("rdr.exe"), std::path::PathBuf::from("SDL2.dll")];
        let missing = vec!["rdr.dll".to_string(), "SDL2_mixer.dll".to_string(), "msvcp140.dll".to_string()];
        // A runtime is missing too: not all of them are the game's own.
        assert!(made_by_game(&missing, &files).is_empty());
        let missing = vec!["rdr.dll".to_string(), "rdr_audio.dll".to_string()];
        assert_eq!(made_by_game(&missing, &files), missing);
        // The release's files aren't known: nothing is assumed.
        assert!(made_by_game(&missing, &[]).is_empty());
    }
}

//! Which file of a GitHub release is the build for this PC.
//!
//! The catalog names one file per system as a pattern (`Zelda64Recompiled-*-Windows.zip`). That's the
//! first choice, as long as it's built for this processor. When the pattern matches nothing (a project
//! renamed its files) or only a build for another processor, every file of the release is scored by
//! what its name says: the system, the processor and the kind of file.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Asset {
    pub name: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub digest: Option<String>,
    pub browser_download_url: String,
}

pub fn os_now() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

pub fn arch_now() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x64"
    }
}

/// `*` any run of characters, `?` one; letter case doesn't matter.
pub fn wild(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let n: Vec<char> = name.to_lowercase().chars().collect();
    let (mut pi, mut ni, mut star, mut mark) = (0usize, 0usize, None::<usize>, 0usize);
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ni;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// The words in a file name: `Zelda64Recompiled-v1.2.2-Linux-X64.zip` -> zelda64recompiled, v1, 2, 2, linux, x64, zip.
fn words(name: &str) -> Vec<String> {
    name.to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

const OS_WORDS: [(&str, &[&str]); 3] = [
    ("windows", &["windows", "win", "win64", "win32", "win10", "msvc", "pc"]),
    ("linux", &["linux", "appimage", "ubuntu", "debian", "steamdeck", "deck", "x86_64"]),
    ("macos", &["macos", "mac", "osx", "darwin", "apple", "dmg", "universal"]),
];
const ARM: [&str; 3] = ["arm64", "aarch64", "arm"];
const X64: [&str; 5] = ["x64", "x86_64", "amd64", "win64", "64bit"];

/// What kind of file it is, by its ending, and whether this system can use it as a build.
pub fn kind(name: &str) -> Option<&'static str> {
    let n = name.to_lowercase();
    for (end, k) in [(".tar.gz", "tar.gz"), (".tgz", "tar.gz"), (".tar.xz", "tar.xz"), (".zip", "zip"), (".7z", "7z"), (".appimage", "appimage"), (".exe", "exe"), (".dmg", "dmg")] {
        if n.ends_with(end) {
            return Some(k);
        }
    }
    None
}

fn usable(os: &str, k: &str) -> bool {
    match os {
        "windows" => matches!(k, "zip" | "7z" | "exe"),
        "linux" => matches!(k, "appimage" | "tar.gz" | "zip" | "7z"),
        _ => matches!(k, "zip" | "dmg"),
    }
}

/// Files that are never the game: debug symbols, sources, checksums, signatures.
fn not_a_build(name: &str) -> bool {
    let w = words(name);
    let n = name.to_lowercase();
    n.ends_with(".sha256") || n.ends_with(".sig") || n.ends_with(".asc") || n.ends_with(".pdb")
        || w.iter().any(|x| matches!(x.as_str(), "symbols" | "pdb" | "debug" | "dbg" | "source" | "sources" | "src"))
}

/// Built for another processor than this one?
fn other_arch(name: &str, arch: &str) -> bool {
    let w = words(name);
    let has = |list: &[&str]| w.iter().any(|x| list.contains(&x.as_str()));
    match arch {
        "arm64" => has(&X64) && !has(&ARM),
        _ => has(&ARM) && !has(&X64),
    }
}

fn other_os(name: &str, os: &str) -> bool {
    let w = words(name);
    OS_WORDS.iter().any(|(o, list)| *o != os && w.iter().any(|x| list.contains(&x.as_str()) && !(*o == "linux" && x == "x86_64")))
        && !OS_WORDS.iter().any(|(o, list)| *o == os && w.iter().any(|x| list.contains(&x.as_str())))
}

fn score(name: &str, os: &str, arch: &str) -> i32 {
    let k = match kind(name) {
        Some(k) if usable(os, k) => k,
        _ => return i32::MIN,
    };
    if not_a_build(name) || other_arch(name, arch) || other_os(name, os) {
        return i32::MIN;
    }
    let w = words(name);
    let mut s = 0;
    if let Some((_, list)) = OS_WORDS.iter().find(|(o, _)| *o == os) {
        if w.iter().any(|x| list.contains(&x.as_str())) {
            s += 50;
        }
    }
    let mine: &[&str] = if arch == "arm64" { &ARM } else { &X64 };
    if w.iter().any(|x| mine.contains(&x.as_str())) {
        s += 20;
    }
    // Flatpak bundles need Flatpak itself; installers want to be run as setup.
    if w.iter().any(|x| x == "flatpak") {
        s -= 40;
    }
    if w.iter().any(|x| matches!(x.as_str(), "setup" | "installer" | "install")) {
        s -= 15;
    }
    s += match (os, k) {
        ("windows", "zip") => 12,
        ("windows", "7z") => 10,
        ("windows", "exe") => 6,
        ("linux", "appimage") => 14,
        ("linux", "tar.gz") => 12,
        ("linux", "zip") => 8,
        _ => 4,
    };
    // A game's server (for playing online) comes with some releases: the game itself comes first,
    // and the server only when it's all there is.
    if w.iter().any(|x| matches!(x.as_str(), "server" | "dedicated" | "srv")) {
        s = (s - 60).max(1);
    }
    s
}

/// The build for this PC, or None when the release has none.
pub fn pick<'a>(assets: &'a [Asset], os: &str, arch: &str, pattern: Option<&str>) -> Option<&'a Asset> {
    if let Some(p) = pattern.filter(|p| !p.trim().is_empty()) {
        let best = assets
            .iter()
            .filter(|a| wild(p, &a.name) && !other_arch(&a.name, arch) && !not_a_build(&a.name))
            .filter(|a| kind(&a.name).map(|k| usable(os, k)).unwrap_or(false))
            .max_by_key(|a| score(&a.name, os, arch).max(0));
        if best.is_some() {
            return best;
        }
    }
    assets
        .iter()
        .map(|a| (score(&a.name, os, arch), a))
        .filter(|(s, _)| *s > 0)
        .max_by_key(|(s, _)| *s)
        .map(|(_, a)| a)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(n: &str) -> Asset {
        Asset { name: n.into(), size: 1, digest: None, browser_download_url: format!("https://x/{n}") }
    }

    #[test]
    fn the_game_before_its_server() {
        let r = [a("ttt2-server-0.1.7-windows-amd64.zip"), a("ttt2-0.1.7-windows-amd64.zip")];
        assert_eq!(pick(&r, "windows", "x64", None).unwrap().name, "ttt2-0.1.7-windows-amd64.zip");
        let r2 = [a("ttt2-0.1.7-windows-amd64.zip"), a("ttt2-server-0.1.7-windows-amd64.zip")];
        assert_eq!(pick(&r2, "windows", "x64", None).unwrap().name, "ttt2-0.1.7-windows-amd64.zip");
        let only = [a("ttt2-server-0.1.7-windows-amd64.zip")];
        assert!(pick(&only, "windows", "x64", None).is_some());
    }

    #[test]
    fn wildcards() {
        assert!(wild("Zelda64Recompiled-*-Windows.zip", "Zelda64Recompiled-v1.2.2-Windows.zip"));
        assert!(wild("ssbu-*053.zip", "SSBU-runtime-daa0053.zip"));
        assert!(!wild("Foo-*.zip", "Foo-1.7z"));
        assert!(wild("*", "anything"));
        assert!(wild("a?c", "abc"));
    }

    #[test]
    fn zelda_linux_pattern_points_at_arm_but_x64_wins() {
        let r = [a("Zelda64Recompiled-v1.2.2-Linux-ARM64.zip"), a("Zelda64Recompiled-v1.2.2-Linux-Flatpak-X64.zip"), a("Zelda64Recompiled-v1.2.2-Linux-X64.zip"), a("Zelda64Recompiled-v1.2.2-Windows.zip"), a("Zelda64Recompiled-v1.2.2-macOS.zip")];
        assert_eq!(pick(&r, "linux", "x64", Some("Zelda64Recompiled-*-Linux-ARM64.zip")).unwrap().name, "Zelda64Recompiled-v1.2.2-Linux-X64.zip");
        assert_eq!(pick(&r, "windows", "x64", Some("Zelda64Recompiled-*-Windows.zip")).unwrap().name, "Zelda64Recompiled-v1.2.2-Windows.zip");
        assert_eq!(pick(&r, "linux", "arm64", Some("Zelda64Recompiled-*-Linux-ARM64.zip")).unwrap().name, "Zelda64Recompiled-v1.2.2-Linux-ARM64.zip");
    }

    #[test]
    fn renamed_files_still_found() {
        let r = [a("Game-2.0-win64.zip"), a("Game-2.0-linux.AppImage"), a("Game-2.0-symbols.zip"), a("Source code.zip")];
        assert_eq!(pick(&r, "windows", "x64", Some("Game-1.*-Windows.zip")).unwrap().name, "Game-2.0-win64.zip");
        assert_eq!(pick(&r, "linux", "x64", None).unwrap().name, "Game-2.0-linux.AppImage");
    }

    #[test]
    fn nothing_for_this_system() {
        let r = [a("Game-macOS.dmg"), a("Game-Linux.tar.gz")];
        assert!(pick(&r, "windows", "x64", None).is_none());
    }

    #[test]
    fn plain_zip_when_it_is_the_only_one() {
        let r = [a("nectar-windows.zip")];
        assert_eq!(pick(&r, "windows", "x64", Some("nectar-windows.zip")).unwrap().name, "nectar-windows.zip");
        let r = [a("UnleashedRecomp-Windows.zip"), a("UnleashedRecomp-Flatpak.zip")];
        assert_eq!(pick(&r, "linux", "x64", Some("UnleashedRecomp-Flatpak.zip")).unwrap().name, "UnleashedRecomp-Flatpak.zip");
    }
}

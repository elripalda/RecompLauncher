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
    // "x86_64" and "x86-64" are one word (64-bit), not "x86" (32-bit) and "64".
    name.to_lowercase()
        .replace("x86_64", "x64")
        .replace("x86-64", "x64")
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
    for (end, k) in [(".tar.gz", "tar.gz"), (".tgz", "tar.gz"), (".tar.xz", "tar.xz"), (".zip", "zip"), (".7z", "7z"), (".rar", "rar"), (".appimage", "appimage"), (".exe", "exe"), (".msi", "msi"), (".dmg", "dmg")] {
        if n.ends_with(end) {
            return Some(k);
        }
    }
    None
}

fn usable(os: &str, k: &str) -> bool {
    match os {
        "windows" => matches!(k, "zip" | "7z" | "rar" | "exe" | "msi"),
        "linux" => matches!(k, "appimage" | "tar.gz" | "zip" | "7z" | "rar"),
        "macos" => k == "zip",
        _ => false,
    }
}

/// A file this system can use as a build: by its kind, and a .tar.gz on Windows only when its name
/// says it's for Windows (tmc-multi-windows-x86_64-v1.tar.gz).
fn usable_file(os: &str, name: &str) -> bool {
    match kind(name) {
        Some(k) if usable(os, k) => true,
        Some("tar.gz" | "tar.xz") if os == "windows" => words(name).iter().any(|w| matches!(w.as_str(), "windows" | "win" | "win64" | "win32")),
        _ => false,
    }
}

/// Files that are never the game: debug symbols, sources, checksums, signatures.
fn not_a_build(name: &str) -> bool {
    let w = words(name);
    let n = name.to_lowercase();
    n.ends_with(".sha256") || n.ends_with(".sig") || n.ends_with(".asc") || n.ends_with(".pdb")
        || w.iter().any(|x| matches!(x.as_str(), "symbols" | "pdb" | "debug" | "dbg" | "source" | "sources" | "src"))
}

/// Words that tag a file as a build for a console or a phone: never a build for a PC.
const PLATFORM_TAGS: [&str; 19] = ["switch", "nx", "nsp", "xci", "nro", "android", "apk", "aab", "ios", "ipa", "ps4", "ps5", "vita", "psp", "3ds", "wiiu", "wii", "xbox", "vpk"];
/// Files for a console or a phone by their ending.
const PLATFORM_ENDS: [&str; 7] = [".apk", ".ipa", ".nsp", ".xci", ".nro", ".vpk", ".pkg"];

/// A file built for a console or a phone (Game-0.5.3.1-switch.zip, Game-1.0-android-arm64.apk),
/// never a build for this PC. Only the words after the version count: a game's own name can have
/// console words in it ("Xbox 360", "Wii"), and with no version to go by the file is left in.
fn for_other_platform(name: &str) -> bool {
    let n = name.to_lowercase();
    if PLATFORM_ENDS.iter().any(|e| n.ends_with(e)) {
        return true;
    }
    static VERSION: std::sync::OnceLock<regex_lite::Regex> = std::sync::OnceLock::new();
    let re = VERSION.get_or_init(|| regex_lite::Regex::new(r"(?:^|[^a-z0-9])v?\d+(?:\.\d+)+|(?:^|[^a-z0-9])v\d+").unwrap());
    let Some(m) = re.find(&n) else { return false };
    words(&n[m.end()..]).iter().any(|w| PLATFORM_TAGS.contains(&w.as_str()))
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
        Some(k) if usable_file(os, name) => k,
        _ => return i32::MIN,
    };
    if not_a_build(name) || other_arch(name, arch) || other_os(name, os) || for_other_platform(name) {
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
        ("windows", "msi") => 2,
        ("linux", "appimage") => 14,
        ("linux", "tar.gz") => 12,
        ("linux", "zip") => 8,
        _ => 4,
    };
    // A game's server (for playing online) comes with some releases: the game itself comes first,
    // and the server only when it's all there is. The same for an editor or tools beside the game.
    if w.iter().any(|x| matches!(x.as_str(), "server" | "dedicated" | "srv" | "editor" | "tools" | "toolkit" | "sdk" | "modkit" | "devkit")) {
        s = (s - 60).max(1);
    }
    s
}

/// The build for this PC, or None when the release has none.
/// Builds of one release that are equally good for this PC and differ only in what they're for (one
/// per region, say: Game-JP and Game-US). The player picks; one alone needs no choice.
pub fn choices<'a>(assets: &'a [Asset], os: &str, arch: &str, pattern: Option<&str>) -> Vec<&'a Asset> {
    let pool: Vec<&Asset> = match pattern.filter(|p| !p.trim().is_empty()) {
        Some(p) => assets.iter().filter(|a| wild(p, &a.name)).collect(),
        None => assets.iter().collect(),
    };
    let best = pool.iter().map(|a| score(&a.name, os, arch)).max().unwrap_or(i32::MIN);
    if best <= 0 {
        return vec![];
    }
    pool.into_iter().filter(|a| score(&a.name, os, arch) == best).collect()
}

/// Every build of a release that can run here, best first: every format (an AppImage and a plain
/// folder, a portable zip and an installer) and 32-bit builds too. A game server is left out unless
/// it's all there is. With a pattern from the list, only what it matches.
pub fn all_for<'a>(assets: &'a [Asset], os: &str, arch: &str, pattern: Option<&str>) -> Vec<&'a Asset> {
    let pool: Vec<&Asset> = match pattern.filter(|p| !p.trim().is_empty()) {
        Some(p) => assets.iter().filter(|a| wild(p, &a.name)).collect(),
        None => assets.iter().collect(),
    };
    let mut v: Vec<(i32, &Asset)> = pool.into_iter().map(|a| (score(&a.name, os, arch), a)).filter(|(s, _)| *s != i32::MIN).collect();
    let server = |a: &Asset| words(&a.name).iter().any(|x| matches!(x.as_str(), "server" | "dedicated" | "srv"));
    if v.iter().any(|(_, a)| !server(a)) {
        v.retain(|(_, a)| !server(a));
    }
    // A Windows Installer package only when there's nothing else for this PC: not one more form
    // of build to ask about.
    let msi = |a: &Asset| a.name.to_lowercase().ends_with(".msi");
    if v.iter().any(|(_, a)| !msi(a)) {
        v.retain(|(_, a)| !msi(a));
    }
    // When a release says which files are for this system, a file that says nothing is for another.
    if v.iter().any(|(_, a)| says_os(&a.name, os)) {
        v.retain(|(_, a)| says_os(&a.name, os));
    }
    v.sort_by(|x, y| y.0.cmp(&x.0).then_with(|| x.1.name.cmp(&y.1.name)));
    v.into_iter().map(|(_, a)| a).collect()
}

/// Does the file's name (or its kind: a .exe, an AppImage, a .dmg) say it's for `os`?
fn says_os(name: &str, os: &str) -> bool {
    let w = words(name);
    let by_kind = matches!((os, kind(name).unwrap_or("")), ("windows", "exe") | ("linux", "appimage") | ("macos", "dmg"));
    by_kind || OS_WORDS.iter().any(|(o, list)| *o == os && w.iter().any(|x| list.contains(&x.as_str())))
}

/// What kind of build it is, for the player choosing one: its format, and 32-bit when it is.
pub fn describe(name: &str, os: &str) -> &'static str {
    let w = words(name);
    let has = |l: &[&str]| w.iter().any(|x| l.contains(&x.as_str()));
    let bit32 = has(&["x86", "i386", "i686", "win32", "32bit"]) && !has(&X64);
    match (os, kind(name).unwrap_or("")) {
        (_, _) if has(&["flatpak"]) => "Flatpak",
        ("linux", "appimage") => "AppImage",
        ("linux", _) => "Linux Folder",
        ("windows", "msi") => "Installer",
        ("windows", "exe") if has(&["setup", "installer", "install"]) => if bit32 { "32-Bit Installer" } else { "Installer" },
        ("windows", "exe") => if bit32 { "32-Bit Program" } else { "Program" },
        ("windows", _) => if bit32 { "32-Bit Portable" } else { "Portable" },
        _ => "Build",
    }
}

/// Of `list`, the one most like `before` (the build the player had): same words but the version.
pub fn most_like<'a>(list: &[&'a Asset], before: &str) -> Option<&'a Asset> {
    let had = words(before);
    // Of equally close names, the first (lists come best first).
    list.iter().copied().rev().max_by_key(|a| words(&a.name).iter().filter(|w| had.contains(w) && !w.chars().all(|c| c.is_ascii_digit() || c == '.')).count())
}

pub fn pick<'a>(assets: &'a [Asset], os: &str, arch: &str, pattern: Option<&str>) -> Option<&'a Asset> {
    if let Some(p) = pattern.filter(|p| !p.trim().is_empty()) {
        let best = assets
            .iter()
            .filter(|a| wild(p, &a.name) && !other_arch(&a.name, arch) && !not_a_build(&a.name) && !for_other_platform(&a.name))
            .filter(|a| usable_file(os, &a.name))
            .max_by_key(|a| score(&a.name, os, arch).max(0));
        if best.is_some() {
            return best;
        }
        // The list says what this system's file is called, and nothing in this release is: it has
        // no build for this system (an older release that does is used instead). Only when the
        // pattern names a file that's there but for another processor (a list pointing at the ARM
        // build) is the best guess used.
        if !assets.iter().any(|a| wild(p, &a.name)) {
            return None;
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

    #[test]
    fn mac_zip_is_installable_but_dmg_is_not() {
        let files = [a("Game-macOS.dmg"), a("Game-macOS.zip")];
        assert_eq!(all_for(&files, "macos", "arm64", None).iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), vec!["Game-macOS.zip"]);
        assert!(pick(&files[..1], "macos", "arm64", None).is_none());
    }

    fn a(n: &str) -> Asset {
        Asset { name: n.into(), size: 1, digest: None, browser_download_url: String::new() }
    }

    #[test]
    fn sixty_four_bit_names_and_editor_builds() {
        // x86_64 is 64-bit, not 32.
        assert_eq!(describe("orchid-v0.7.5-windows-x86_64.zip", "windows"), "Portable");
        assert_eq!(describe("game-win32.zip", "windows"), "32-Bit Portable");
        assert!(other_arch("game-linux-x86_64.AppImage", "arm64"));
        assert!(!other_arch("game-linux-x86_64.AppImage", "x64"));
        // The game before its editor.
        let r = [a("fm-editor-v0.3.1-preview.1-windows.zip"), a("memories-pc-v0.3.1-preview.1-windows.zip")];
        assert_eq!(pick(&r, "windows", "x64", None).unwrap().name, "memories-pc-v0.3.1-preview.1-windows.zip");
        assert_eq!(all_for(&r, "windows", "x64", None).len(), 2);
        // An editor alone is still picked.
        assert_eq!(pick(&r[..1], "windows", "x64", None).unwrap().name, "fm-editor-v0.3.1-preview.1-windows.zip");
    }

    #[test]
    fn rar_and_windows_tarballs_are_builds() {
        assert_eq!(pick(&[a("release.rar")], "windows", "x64", Some("release.rar")).map(|x| x.name.as_str()), Some("release.rar"));
        assert_eq!(pick(&[a("bloodyextreme.rar")], "windows", "x64", None).map(|x| x.name.as_str()), Some("bloodyextreme.rar"));
        let tmc = [a("tmc-multi-linux-x86_64-v0.9.6.tar.gz"), a("tmc-multi-windows-arm64-v0.9.6.tar.gz"), a("tmc-multi-windows-x86_64-v0.9.6.tar.gz"), a("tmc-multi-android-v0.9.6.apk")];
        assert_eq!(pick(&tmc, "windows", "x64", Some("tmc-multi-windows-x86_64-*.tar.gz")).map(|x| x.name.as_str()), Some("tmc-multi-windows-x86_64-v0.9.6.tar.gz"));
        assert_eq!(pick(&tmc, "windows", "x64", None).map(|x| x.name.as_str()), Some("tmc-multi-windows-x86_64-v0.9.6.tar.gz"));
        // A tarball that doesn't say Windows is still Linux's.
        assert!(pick(&[a("Game-1.0.tar.gz")], "windows", "x64", None).is_none());
        // An updater's files alone are no build.
        assert!(pick(&[a("update.json"), a("update.json.sig")], "windows", "x64", None).is_none());
    }

    #[test]
    fn every_format_for_this_system_is_offered() {
        let rel = [a("Game-1.0-linux-x86_64.AppImage"), a("Game-1.0-linux-x86_64.tar.gz"), a("Game-1.0-windows-x64.zip"), a("Game-1.0-Setup.exe"), a("Game-1.0-win32.zip"), a("Game-server-linux.tar.gz"), a("Game-1.0.pdb")];
        let l: Vec<&str> = all_for(&rel, "linux", "x64", None).iter().map(|x| x.name.as_str()).collect();
        assert_eq!(l, vec!["Game-1.0-linux-x86_64.AppImage", "Game-1.0-linux-x86_64.tar.gz"]);
        let w: Vec<&str> = all_for(&rel, "windows", "x64", None).iter().map(|x| describe(&x.name, "windows")).collect();
        assert_eq!(w, vec!["Portable", "32-Bit Portable", "Installer"]);
        assert_eq!(describe("Game-1.0-linux-x86_64.tar.gz", "linux"), "Linux Folder");
        // A release that names its Linux files: the unnamed zip is for Windows.
        let named = [a("Game.AppImage"), a("Game.zip")];
        assert_eq!(all_for(&named, "linux", "x64", None).len(), 1);
        let unnamed = [a("Game.tar.gz"), a("Game.zip")];
        assert_eq!(all_for(&unnamed, "linux", "x64", None).len(), 2);
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
    fn a_pattern_that_matches_nothing_means_no_build() {
        let r = [a("Game-2.0-win64.zip"), a("Game-2.0-linux.AppImage"), a("Game-2.0-symbols.zip"), a("Source code.zip")];
        // The list says what the Windows file is called and this release has none: no build here
        // (an older release that has one is installed instead). Without a pattern, the best guess.
        assert!(pick(&r, "windows", "x64", Some("Game-1.*-Windows.zip")).is_none());
        assert_eq!(pick(&r, "windows", "x64", None).unwrap().name, "Game-2.0-win64.zip");
        assert_eq!(pick(&r, "linux", "x64", None).unwrap().name, "Game-2.0-linux.AppImage");
    }

    #[test]
    fn console_and_phone_builds_are_never_for_a_pc() {
        // Only a Switch build in the newest release, and the list's pattern matches nothing.
        assert!(pick(&[a("Marchwind64-0.5.3.1-switch.zip")], "windows", "x64", Some("Marchwind64-*-windows-x64.zip")).is_none());
        assert!(pick(&[a("Marchwind64-0.5.3.1-switch.zip")], "windows", "x64", None).is_none());
        assert!(pick(&[a("Marchwind64-0.5.3.1-switch.zip")], "linux", "x64", None).is_none());
        let r = [a("Game-1.0-android-arm64.apk"), a("Game-1.0-windows-x64.zip")];
        assert_eq!(pick(&r, "windows", "x64", None).unwrap().name, "Game-1.0-windows-x64.zip");
        assert_eq!(all_for(&[a("Game-1.0-ps5.zip"), a("Game-1.0-win64.zip")], "windows", "x64", None).len(), 1);
        assert!(choices(&[a("Game-v2-nx.zip")], "windows", "x64", None).is_empty());
        // Console words in a game's own name are its name, not a platform.
        assert_eq!(pick(&[a("Xbox 360 Something-windows.zip")], "windows", "x64", None).unwrap().name, "Xbox 360 Something-windows.zip");
        assert_eq!(pick(&[a("Wii Sports Recomp-1.2-windows-x64.zip")], "windows", "x64", None).unwrap().name, "Wii Sports Recomp-1.2-windows-x64.zip");
        assert_eq!(pick(&[a("PS2 Racer 2.0-win64.zip")], "windows", "x64", None).unwrap().name, "PS2 Racer 2.0-win64.zip");
        // A later version word isn't a platform: xbox360 as one word stays in.
        assert_eq!(pick(&[a("Game-1.0-xbox360-windows.zip")], "windows", "x64", None).unwrap().name, "Game-1.0-xbox360-windows.zip");
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

//! Boot profiles: how each recomp in the catalog starts, worked out from its release and its
//! README and kept in the catalog as `"boot": {...}`, so the launcher doesn't have to guess.
//!
//! - `start`: the program to start, as a path in the release (its own launcher when it has one).
//! - `setup`: a program in the release that runs once before the first play.
//! - `kind: "installer"` with `installer.then_start`: the release is an installer; after it, the
//!   game starts from that path in the folder it installed to.
//! - `console`: the program asks questions in its text window, so its output isn't captured.
//! - `files`: where the player's copy goes. `flag` and `folder` become an easy setup
//!   (profile::from_boot); the rest the game asks for itself.
//! - `player_note`: something the player has to do by hand, shown on the game's page.

use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Boot {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub start: Option<String>,
    /// The program a Linux or Mac build starts with, when it isn't `start` (build/witch-time beside
    /// build/witch-time.exe, play.sh beside play.bat).
    #[serde(default)]
    pub start_linux: Option<String>,
    #[serde(default)]
    pub start_macos: Option<String>,
    #[serde(default)]
    pub setup: Option<String>,
    #[serde(default)]
    pub console: bool,
    #[serde(default)]
    pub installer: Option<Installer>,
    /// The game's own setup works from a clone of its repo: the launcher clones it into the game's
    /// folder and puts the release's files in it.
    #[serde(default)]
    pub clone: bool,
    /// What the game's own setup makes inside its folder (files or folders). Until they're all
    /// there, the setup hasn't finished, whatever happened before.
    #[serde(default)]
    pub makes: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Installer {
    #[serde(default)]
    pub then_start: Option<String>,
}

/// A program or script on Linux or a Mac: allowed to run.
pub fn runnable(p: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(m) = std::fs::metadata(p) {
            let mode = m.permissions().mode();
            if mode & 0o111 != 0o111 {
                let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode | 0o755));
            }
        }
    }
    #[cfg(not(unix))]
    let _ = p;
}

/// The start a boot profile's JSON names for this system, then its plain one: the programs to try,
/// first one there wins.
pub fn starts(b: &Value) -> Vec<String> {
    let s = |k: &str| b.get(k).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty()).map(String::from);
    let own = if cfg!(target_os = "linux") { s("start_linux") } else if cfg!(target_os = "macos") { s("start_macos") } else { None };
    own.into_iter().chain(s("start")).collect()
}

/// A path from the profile, safe inside the game's folder: relative, no `..`.
fn rel(p: &str) -> Option<PathBuf> {
    let p = p.trim().replace('\\', "/");
    if p.is_empty() || p.starts_with('/') || p.contains(':') || p.split('/').any(|c| c == "..") {
        return None;
    }
    Some(PathBuf::from(p))
}

/// The profiles this launcher shipped with, for catalog entries that don't carry one yet.
static BUILT_IN: std::sync::OnceLock<Value> = std::sync::OnceLock::new();

fn built_in() -> &'static Value {
    BUILT_IN.get_or_init(|| serde_json::from_str(&crate::unpack(include_bytes!(concat!(env!("OUT_DIR"), "/boot-profiles.json.z")))).unwrap_or(Value::Null))
}

/// The shipped profiles, for the window (the game page's "Before you play").
#[tauri::command(async)]
pub fn boot_builtin() -> Value {
    built_in().clone()
}

/// Additions this launcher knows for catalog profiles (a game's update, say), merged in where the
/// catalog's own profile doesn't say them yet.
static EXTRAS: std::sync::OnceLock<Value> = std::sync::OnceLock::new();

fn extras() -> &'static Value {
    EXTRAS.get_or_init(|| serde_json::from_str(include_str!("../boot/boot-extras.json")).unwrap_or(Value::Null))
}

/// `add` into `to`, keeping what `to` already says.
/// Adds what the catalog's profile doesn't say. A key written as "=name" sets those fields even
/// when the catalog has its own (a fix that has to win over what's published).
fn merge(to: &mut Value, add: &Value) {
    if let (Some(t), Some(a)) = (to.as_object_mut(), add.as_object()) {
        for (k, v) in a {
            if let Some(name) = k.strip_prefix('=') {
                match (t.get_mut(name), v.as_object()) {
                    (Some(Value::Object(have)), Some(fields)) => {
                        for (fk, fv) in fields {
                            have.insert(fk.clone(), fv.clone());
                        }
                    }
                    _ => {
                        t.insert(name.to_string(), v.clone());
                    }
                }
                continue;
            }
            match t.get_mut(k) {
                Some(have) if have.is_object() && v.is_object() => merge(have, v),
                Some(_) => {}
                None => {
                    t.insert(k.clone(), v.clone());
                }
            }
        }
    }
}

/// A catalog entry's boot profile as JSON: its own, or the one this launcher shipped with.
pub fn raw(entry: &Value) -> Option<Value> {
    let id = entry.get("id").and_then(Value::as_str);
    let mut b = match entry.get("boot").filter(|b| b.is_object()) {
        Some(b) => {
            let mut b = b.clone();
            // The catalog's own profile without game settings: the ones this launcher shipped with
            // stay (they'd be lost otherwise: a profile published for a game's setup says nothing
            // about its settings).
            if b.get("settings").is_none() {
                if let Some(s) = id.and_then(|i| built_in().get(i)).and_then(|p| p.get("settings")).filter(|s| s.is_object()) {
                    b["settings"] = s.clone();
                }
            }
            b
        }
        None => built_in().get(id?).cloned()?,
    };
    if let Some(x) = id.and_then(|i| extras().get(i)) {
        merge(&mut b, x);
    }
    Some(b)
}

impl Boot {
    pub fn from_entry(entry: &Value) -> Option<Boot> {
        raw(entry).and_then(|b| serde_json::from_value(b).ok())
    }

    /// The first of the things its setup makes that isn't in `dir` yet.
    pub fn not_made(&self, dir: &Path) -> Option<String> {
        self.makes.iter().find(|m| rel(m).map(|p| !dir.join(p).exists()).unwrap_or(false)).cloned()
    }

    /// This system's own start, when the profile names one.
    fn start_here(&self) -> Option<&str> {
        if cfg!(target_os = "linux") {
            self.start_linux.as_deref()
        } else if cfg!(target_os = "macos") {
            self.start_macos.as_deref()
        } else {
            None
        }
    }

    /// The program to start, when it's there in this folder: this system's own first (made
    /// runnable, since a zip doesn't always keep that), then `start`.
    pub fn start_in(&self, dir: &Path) -> Option<PathBuf> {
        if let Some(p) = self.start_here().and_then(rel).filter(|p| dir.join(p).is_file()) {
            runnable(&dir.join(&p));
            return Some(p);
        }
        self.start.as_deref().and_then(rel).filter(|p| dir.join(p).is_file())
    }

    /// The setup to run once, when it's there in this folder.
    pub fn setup_in(&self, dir: &Path) -> Option<PathBuf> {
        self.setup.as_deref().and_then(rel).filter(|p| dir.join(p).is_file())
    }

    /// The setup by its name, also a folder or two down (a release wrapped in a folder of its own).
    pub fn setup_near(&self, dir: &Path) -> Option<PathBuf> {
        if let Some(p) = self.setup_in(dir) {
            return Some(p);
        }
        let want = self.setup.as_deref().and_then(rel)?.file_name()?.to_string_lossy().to_lowercase();
        let mut stack = vec![(dir.to_path_buf(), 0usize)];
        while let Some((d, depth)) = stack.pop() {
            let mut entries: Vec<_> = std::fs::read_dir(&d).into_iter().flatten().flatten().collect();
            entries.sort_by_key(|e| e.file_name());
            for e in entries {
                let Ok(t) = e.file_type() else { continue };
                if t.is_dir() && depth < 2 {
                    stack.push((e.path(), depth + 1));
                } else if t.is_file() && e.file_name().to_string_lossy().to_lowercase() == want {
                    return e.path().strip_prefix(dir).ok().map(Path::to_path_buf);
                }
            }
        }
        None
    }

    /// The file name of the program an installer builds (`totk.exe`), lowercase.
    pub fn then_name(&self) -> Option<String> {
        self.then_start().and_then(|t| t.file_name().map(|n| n.to_string_lossy().to_lowercase()))
    }

    /// Where the game starts in the folder an installer put it in.
    pub fn then_start(&self) -> Option<PathBuf> {
        self.installer.as_ref().and_then(|i| i.then_start.as_deref()).and_then(rel)
    }

    pub fn is_installer(&self) -> bool {
        self.kind == "installer"
    }

    /// The profile for the build that's installed. A Linux or Mac build (not one run through
    /// Proton) has no use for a Windows setup or installer it names (TotK-Setup.exe beside its
    /// AppImage): those are left out, and it starts like any game.
    pub fn for_build(mut self, windows_build: bool) -> Boot {
        if windows_build {
            return self;
        }
        let win = |s: &str| { let l = s.to_lowercase(); [".exe", ".bat", ".cmd", ".msi"].iter().any(|x| l.ends_with(x)) };
        if self.setup.as_deref().map(win).unwrap_or(false) {
            self.setup = None;
        }
        if self.installer.as_ref().and_then(|i| i.then_start.as_deref()).map(win).unwrap_or(false) {
            self.installer = None;
            if self.kind == "installer" {
                self.kind = "portable".into();
            }
        }
        if self.setup.is_none() {
            self.makes.clear();
        }
        self
    }
}

/// A game's boot profile for the build that's installed (see `for_build`).
pub fn of_build(dir: &Path, id: &str, windows_build: bool) -> Option<Boot> {
    of(dir, id).map(|b| b.for_build(windows_build))
}

/// The installed build is a Windows one (on Windows, or through Proton).
pub fn windows_build(proton: bool) -> bool {
    cfg!(windows) || proton
}

/// A file name against the name a profile gives (any case). `*` stands for any text, for names
/// that carry a version ("Play Pokopia *.cmd").
pub fn name_fits(pattern: &str, name: &str) -> bool {
    let (p, n) = (pattern.to_lowercase(), name.to_lowercase());
    if !p.contains('*') {
        return p == n;
    }
    let parts: Vec<&str> = p.split('*').collect();
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !n.starts_with(first) || n.len() < first.len() + last.len() || !n.ends_with(last) {
        return false;
    }
    let mut rest = &n[first.len()..n.len() - last.len()];
    for mid in &parts[1..parts.len() - 1] {
        match rest.find(mid) {
            Some(i) => rest = &rest[i + mid.len()..],
            None => return false,
        }
    }
    true
}

/// A game's boot profile, from the catalog the launcher last loaded.
pub fn of(dir: &Path, id: &str) -> Option<Boot> {
    let cat: Value = std::fs::read(dir.join("cache/pc-catalog.json")).ok().and_then(|b| serde_json::from_slice(&b).ok())?;
    Boot::from_entry(crate::catalog::app(&cat, id)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_linux_build_skips_a_windows_installer() {
        let e: Value = serde_json::from_str(r#"{"id":"totk-pc-recomp","boot":{"kind":"installer","setup":"TotK-Setup.exe","installer":{"then_start":"totk.exe"}}}"#).unwrap();
        let b = Boot::from_entry(&e).unwrap().for_build(false);
        assert!(!b.is_installer() && b.setup.is_none() && b.then_name().is_none());
        let b = Boot::from_entry(&e).unwrap().for_build(true);
        assert!(b.is_installer() && b.then_name().as_deref() == Some("totk.exe"));
    }

    #[test]
    fn names_with_a_version_fit() {
        assert!(name_fits("Play Pokopia *.cmd", "play pokopia 1.0.5.cmd"));
        assert!(name_fits("totk.exe", "TotK.exe"));
        assert!(!name_fits("Play Pokopia *.cmd", "Play Pokopia.cmd.bak"));
        assert!(!name_fits("a*b*c", "ac"));
        assert!(name_fits("a*b*c", "axbyc"));
    }

    #[test]
    fn reads_boot_profiles() {
        let e: Value = serde_json::from_str(r#"{"id":"x","boot":{"kind":"installer","start":null,"setup":"../evil.exe","console":true,"installer":{"then_start":"dist/Launch Madden NFL 08.bat"},"files":{"how":"self"}}}"#).unwrap();
        let b = Boot::from_entry(&e).unwrap();
        assert!(b.is_installer() && b.console);
        assert_eq!(b.then_start().unwrap(), PathBuf::from("dist/Launch Madden NFL 08.bat"));
        assert!(b.setup_in(Path::new("/tmp")).is_none());
        // The ones shipped with the launcher load, and fill in for entries without one.
        let b = Boot::from_entry(&serde_json::json!({"id": "tenkaichi3decomp"})).unwrap();
        assert_eq!(b.setup.as_deref(), Some("Tenkaichi3Decomp-setup.exe"));
        assert!(built_in().as_object().map(|o| o.len() > 200).unwrap_or(false));
        assert!(rel("C:/x.exe").is_none() && rel("a/../b").is_none() && rel("bin\\Game.exe") == Some(PathBuf::from("bin/Game.exe")));
    }

    #[test]
    fn installer_release_starts_its_setup_not_its_tools() {
        // TotK's release: TotK-Setup.exe builds totk.exe somewhere else; bin/totk_xlate.exe is a tool.
        let d = std::env::temp_dir().join(format!("rl-boot-totk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for f in ["TotK-Setup.exe", "bin/totk_xlate.exe", "bin/hactool.exe", "python/pythonw.exe"] {
            let p = d.join("TotK-Setup").join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b"MZ").unwrap();
        }
        let e: Value = serde_json::from_str(r#"{"id":"totk-pc-recomp","boot":{"kind":"installer","setup":"TotK-Setup.exe","installer":{"then_start":"totk.exe"}}}"#).unwrap();
        let b = Boot::from_entry(&e).unwrap();
        assert_eq!(b.then_name().as_deref(), Some("totk.exe"));
        assert_eq!(b.setup_near(&d.join("TotK-Setup")), Some(PathBuf::from("TotK-Setup.exe")));
        assert_eq!(b.setup_near(&d), Some(PathBuf::from("TotK-Setup/TotK-Setup.exe")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_setup_that_made_nothing_isnt_done() {
        let e: Value = serde_json::from_str(r#"{"id":"tenkaichi3decomp","boot":{"kind":"portable","start":"Tenkaichi3Decomp.exe","setup":"Tenkaichi3Decomp-setup.exe"}}"#).unwrap();
        let b = Boot::from_entry(&e).unwrap();
        let d = std::env::temp_dir().join(format!("rl-made-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("gamedata/disc")).unwrap();
        assert_eq!(b.not_made(&d).as_deref(), Some("gamedata/disc/SLUS_216.78"));
        std::fs::write(d.join("gamedata/disc/SLUS_216.78"), b"x").unwrap();
        assert!(b.not_made(&d).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn metroid_prime_gets_its_disc_by_env() {
        let e: Value = serde_json::from_str(r#"{"id":"metroidprimeport","boot":{"kind":"portable","start":"metroid_prime_port.exe","start_is":"game","files":{"how":"self","accepts":"iso","what":"Your disc"}}}"#).unwrap();
        let b = raw(&e).unwrap();
        assert_eq!(b["files"]["how"], "env");
        let p = crate::profile::from_boot(&b).unwrap();
        assert!(p.start.unwrap().env.contains_key("MP_DISC"));
    }

    #[test]
    fn a_published_profile_keeps_the_shipped_settings() {
        // AC6: the catalog's profile says how it's set up and nothing about settings.
        let e: Value = serde_json::from_str(r#"{"id":"ac6-recomp","boot":{"kind":"portable","start":"ac6recomp.exe","start_is":"game","files":{"how":"folder","folder":"assets","accepts":"folder or iso","extract":"xdvdfs","contains":["default.xex"]}}}"#).unwrap();
        let b = raw(&e).unwrap();
        assert_eq!(b["files"]["folder"], "assets");
        assert!(b["settings"]["settings"].as_array().is_some_and(|s| s.iter().any(|x| x["id"] == "resolution")));
        // Its own settings win.
        let e: Value = serde_json::from_str(r#"{"id":"ac6-recomp","boot":{"kind":"portable","start":"ac6recomp.exe","settings":{"config":[],"settings":[]}}}"#).unwrap();
        assert_eq!(raw(&e).unwrap()["settings"]["settings"], serde_json::json!([]));
    }

    #[test]
    fn a_linux_build_starts_its_own_program() {
        let e: Value = serde_json::from_str(r#"{"id":"witch-time","boot":{"kind":"portable","start":"build/witch-time.exe","start_linux":"build/witch-time","start_is":"launcher"}}"#).unwrap();
        let b = Boot::from_entry(&e).unwrap();
        let d = std::env::temp_dir().join(format!("rl-startlinux-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("build")).unwrap();
        // Only the Windows one there (played through Proton): that one.
        std::fs::write(d.join("build/witch-time.exe"), b"MZ").unwrap();
        assert_eq!(b.start_in(&d).unwrap(), PathBuf::from("build/witch-time.exe"));
        std::fs::write(d.join("build/witch-time"), b"#!/bin/sh\n").unwrap();
        if cfg!(target_os = "linux") {
            assert_eq!(b.start_in(&d).unwrap(), PathBuf::from("build/witch-time"));
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(std::fs::metadata(d.join("build/witch-time")).unwrap().permissions().mode() & 0o111, 0o111);
            }
            assert_eq!(starts(&e["boot"]), vec!["build/witch-time".to_string(), "build/witch-time.exe".to_string()]);
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn extras_add_an_update_without_overwriting() {
        let e: Value = serde_json::from_str(r#"{"id":"pgr4-native","boot":{"kind":"portable","start":"pgr4_recompiled.exe","files":{"how":"folder","folder":"game","accepts":"folder or iso","contains":["default.xex"]}}}"#).unwrap();
        let b = raw(&e).unwrap();
        assert_eq!(b["files"]["folder"], "game");
        assert_eq!(b["files"]["update"]["folder"], "game/Content");
        let g = crate::profile::from_boot(&b).unwrap().game.unwrap();
        let u = g.update.unwrap();
        assert!(!u.optional && u.wants_file() && !u.wants_folder());
        let d = std::env::temp_dir().join(format!("rl-upd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("dl")).unwrap();
        let tu = d.join("dl/tu.bin");
        std::fs::write(&tu, b"not it").unwrap();
        assert!(u.check(&tu).is_err());
        let plain = crate::profile::Extra { into: "game/Content".into(), name: "TU_X".into(), label: "x.".into(), ..Default::default() };
        assert!(plain.check(&tu).is_err());
        std::fs::write(d.join("dl/TU_X"), b"x").unwrap();
        assert!(plain.check(&d.join("dl/TU_X")).is_ok());
        assert!(plain.in_place(&d.join("inst")).is_none());
        plain.place(&d.join("inst"), &d.join("dl/TU_X")).unwrap();
        assert!(d.join("inst/game/Content/TU_X").is_file() && plain.in_place(&d.join("inst")).is_some());
        let _ = std::fs::remove_dir_all(&d);
    }
}

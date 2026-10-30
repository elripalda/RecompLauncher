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
    #[serde(default)]
    pub setup: Option<String>,
    #[serde(default)]
    pub console: bool,
    #[serde(default)]
    pub installer: Option<Installer>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Installer {
    #[serde(default)]
    pub then_start: Option<String>,
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
    BUILT_IN.get_or_init(|| serde_json::from_str(include_str!("../boot/boot-profiles.json")).unwrap_or(Value::Null))
}

/// The shipped profiles, for the window (the game page's "Before you play").
#[tauri::command]
pub fn boot_builtin() -> Value {
    built_in().clone()
}

/// A catalog entry's boot profile as JSON: its own, or the one this launcher shipped with.
pub fn raw(entry: &Value) -> Option<Value> {
    if let Some(b) = entry.get("boot").filter(|b| b.is_object()) {
        return Some(b.clone());
    }
    let id = entry.get("id").and_then(Value::as_str)?;
    built_in().get(id).cloned()
}

impl Boot {
    pub fn from_entry(entry: &Value) -> Option<Boot> {
        raw(entry).and_then(|b| serde_json::from_value(b).ok())
    }

    /// The program to start, when it's there in this folder.
    pub fn start_in(&self, dir: &Path) -> Option<PathBuf> {
        self.start.as_deref().and_then(rel).filter(|p| dir.join(p).is_file())
    }

    /// The setup to run once, when it's there in this folder.
    pub fn setup_in(&self, dir: &Path) -> Option<PathBuf> {
        self.setup.as_deref().and_then(rel).filter(|p| dir.join(p).is_file())
    }

    /// Where the game starts in the folder an installer put it in.
    pub fn then_start(&self) -> Option<PathBuf> {
        self.installer.as_ref().and_then(|i| i.then_start.as_deref()).and_then(rel)
    }

    pub fn is_installer(&self) -> bool {
        self.kind == "installer"
    }
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
}

//! What's installed and the launcher's settings, kept in library.json in the app's data folder.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Installed {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub repo: String,
    /// The release's tag, which is what updates compare.
    pub tag: String,
    #[serde(default)]
    pub version: String,
    /// The game's folder.
    pub path: PathBuf,
    /// The program to start, relative to `path`.
    #[serde(default)]
    pub exe: Option<PathBuf>,
    #[serde(default)]
    pub asset: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub installed_at: u64,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub played_secs: u64,
    #[serde(default)]
    pub last_played: u64,
    /// The files the release put in the folder, relative. Everything else in it is the player's
    /// (saves, settings, mods, game files) and is what backups keep.
    #[serde(default)]
    pub files: Vec<PathBuf>,
    /// The player picked this version on purpose (an older one): no update nagging.
    #[serde(default)]
    pub pinned: bool,
    /// Each time it was played: [started, seconds]. The newest 500.
    #[serde(default)]
    pub sessions: Vec<[u64; 2]>,
    /// The player's copy of the game (disc image or folder), for recomps the launcher sets up.
    #[serde(default)]
    pub game_copy: Option<PathBuf>,
    /// The game's update the player picked, for recomps that need one beside their copy.
    #[serde(default)]
    pub update_copy: Option<PathBuf>,
    /// A Windows build, started through Proton on Linux.
    #[serde(default)]
    pub proton: bool,
    /// Settings passed on the command line: setting id -> choice.
    #[serde(default)]
    pub picks: BTreeMap<String, usize>,
    /// The player's own launch options, added after everything else.
    #[serde(default)]
    pub args: String,
    /// Install updates as soon as they're found.
    #[serde(default)]
    pub auto_update: bool,
    /// A developer's own build, added from a folder on this PC: never updated, moved or deleted.
    #[serde(default)]
    pub local: bool,
    /// The look the game starts with (looks.rs), empty for the game's own picture.
    #[serde(default)]
    pub look: String,
    /// How strong it is, 0 to 100.
    #[serde(default = "half")]
    pub look_amount: u8,
    /// Cloud saves for this game: None until the player is asked (before its first Play while
    /// signed in), then their answer.
    #[serde(default)]
    pub cloud: Option<bool>,
    /// Where the launcher noticed the game saving, as templates (`{local}/Game/saves`), for games
    /// whose developer or catalog doesn't say.
    #[serde(default)]
    pub found_saves: Vec<String>,
    /// Where the player says the game saves (Saves, Change): ahead of everything else.
    #[serde(default)]
    pub save_place: Option<String>,
    /// A picture the player picked for its cover (a copy in the launcher's covers folder).
    #[serde(default)]
    pub cover_file: Option<PathBuf>,
    /// The player's own picture behind the game's page (a wide one), in the launcher's covers folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hero_file: Option<PathBuf>,
    /// A program that has to run once before the game: the release's own installer, or a
    /// first-time setup beside the game. Relative to `path`.
    #[serde(default)]
    pub setup: Option<PathBuf>,
    /// It ran and finished.
    #[serde(default)]
    pub setup_done: bool,
    /// The launcher has looked for one (installs from before 0.75 Beta 8 get looked at on Play).
    #[serde(default)]
    pub setup_checked: bool,
    /// Which version of the launcher's rules picked `exe` (install::EXE_RULES); older picks are
    /// made again on the next Play. The player's own pick is never redone.
    #[serde(default)]
    pub exe_rules: u8,
}

fn half() -> u8 {
    50
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Where new games go.
    pub library_dir: PathBuf,
    #[serde(default)]
    pub hide_ai: bool,
    /// Look for updates when the launcher opens.
    #[serde(default = "yes")]
    pub check_updates: bool,
    /// Keep the downloaded zip after installing (off: deleted once it's unpacked).
    #[serde(default)]
    pub keep_downloads: bool,
    /// Anything else the window wants remembered (last tab, filters).
    #[serde(default)]
    pub ui: Value,
    /// Other library folders, on other drives. `library_dir` is the one new games go to by default.
    #[serde(default)]
    pub library_dirs: Vec<PathBuf>,
    /// Folders with the player's ROMs and game backups, looked through for games that have recomps.
    #[serde(default)]
    pub rom_dirs: Vec<PathBuf>,
    /// Asked where the first game should go yet?
    #[serde(default)]
    pub asked_location: bool,
    #[serde(default)]
    pub start_in_tv: bool,
    /// Windows notifications (new recomps for games you own, updates).
    #[serde(default = "yes")]
    pub notify: bool,
    /// Back up saves and settings before every update.
    #[serde(default = "yes")]
    pub backup_before_update: bool,
    /// TV mode's size: "small", "medium" or "large" (empty: picked from the screen).
    #[serde(default)]
    pub tv_size: String,
    /// TV mode's sounds.
    #[serde(default = "yes")]
    pub sounds: bool,
    /// How many times the "how to open the quick menu" tip has shown.
    #[serde(default)]
    pub menu_tips: u32,
    /// Show the game being played on Discord.
    #[serde(default = "yes")]
    pub discord: bool,
    /// The first-run setup has been gone through.
    #[serde(default)]
    pub setup_done: bool,
    /// Closing the window leaves the launcher running by the clock.
    #[serde(default = "yes")]
    pub close_to_tray: bool,
    /// On Linux: recomps with only a Windows version install that and play through Proton.
    #[serde(default)]
    pub proton: bool,
    /// The "still running by the clock" notification has shown once.
    #[serde(default)]
    pub tray_told: bool,
    /// Send usage and diagnostics to recomplauncher.com (usage.rs). Off until the player says yes.
    #[serde(default)]
    pub share_usage: bool,
    /// Anonymous usage stats for recomplauncher.com's dashboard (ping.rs). Off until the player says yes.
    #[serde(default)]
    pub share_stats: bool,
    /// The player has been asked about both (first-run setup, or once after updating). Nothing is
    /// sent before that, whatever the two switches say.
    #[serde(default)]
    pub data_asked: bool,
    /// The quick menu over games (holding View, Ctrl+F12). Off: the launcher leaves controllers
    /// and the keyboard alone while a game runs.
    #[serde(default = "yes")]
    pub quick_menu: bool,
    /// Minimize the launcher while a game runs. Not set: on for handhelds, off otherwise.
    #[serde(default)]
    pub minimize_playing: Option<bool>,
    /// Still pictures instead of moving ones, and no catalog refresh during games. Not set: on for
    /// handhelds, off otherwise.
    #[serde(default)]
    pub lighter: Option<bool>,
}

fn yes() -> bool {
    true
}

pub fn default_library_dir() -> PathBuf {
    if cfg!(target_os = "windows") {
        // A short path near the top of the drive: some games break in long or OneDrive-synced paths.
        let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        PathBuf::from(format!("{drive}\\Recomp Library"))
    } else {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join("Games/Recomp Library")
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            library_dir: default_library_dir(),
            hide_ai: false,
            check_updates: true,
            keep_downloads: false,
            ui: Value::Null,
            library_dirs: vec![],
            rom_dirs: vec![],
            asked_location: false,
            start_in_tv: false,
            setup_done: false,
            close_to_tray: true,
            proton: false,
            tray_told: false,
            share_usage: false,
            share_stats: false,
            data_asked: false,
            notify: true,
            backup_before_update: true,
            tv_size: String::new(),
            sounds: true,
            menu_tips: 0,
            discord: true,
            quick_menu: true,
            minimize_playing: None,
            lighter: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Library {
    #[serde(default)]
    pub installed: BTreeMap<String, Installed>,
    #[serde(default)]
    pub settings: Settings,
    /// What the last ROM scan found.
    #[serde(default)]
    pub roms: Vec<crate::roms::RomFile>,
    /// Catalog ids already seen, so a new recomp can be told apart.
    #[serde(default)]
    pub seen_apps: Vec<String>,
    /// For recomps by developers the player follows: the version last seen, to tell a new one apart.
    #[serde(default)]
    pub seen_versions: BTreeMap<String, String>,
    /// Mods the launcher put in, by game id.
    #[serde(default)]
    pub mods: BTreeMap<String, Vec<crate::mods::ModRec>>,
}

impl Settings {
    /// Every library folder, the default one first.
    pub fn all_libraries(&self) -> Vec<PathBuf> {
        let mut out = vec![self.library_dir.clone()];
        for d in &self.library_dirs {
            if !out.contains(d) {
                out.push(d.clone());
            }
        }
        out
    }
}

/// The same folder, however it's written: letter case and a trailing slash don't matter.
pub fn same_path(a: &Path, b: &Path) -> bool {
    let n = |x: &Path| x.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_lowercase();
    n(a) == n(b)
}

/// A folder that holds more than one thing of the player's: a drive, their home folder, Downloads,
/// Desktop, Documents and the like, or a library itself. Never taken as one game's folder, so
/// never moved or deleted as one.
pub fn shared_folder(p: &Path, libs: &[PathBuf]) -> bool {
    if p.parent().is_none() || p.components().count() <= 2 {
        return true;
    }
    let known = [dirs::home_dir(), dirs::download_dir(), dirs::desktop_dir(), dirs::document_dir(), dirs::data_local_dir(), dirs::data_dir(), dirs::config_dir(),
        dirs::picture_dir(), dirs::video_dir(), dirs::audio_dir(), dirs::public_dir()];
    known.iter().flatten().chain(libs.iter()).any(|k| same_path(k, p))
}

impl Library {
    pub fn load(dir: &Path) -> Library {
        let p = dir.join("library.json");
        let mut l = match std::fs::read(&p) {
            Ok(b) => match serde_json::from_slice(&b) {
                Ok(l) => l,
                Err(_) => {
                    // A file this launcher can't read whole: everything in it that it can read is
                    // kept (each game on its own), and the file itself is kept for a look later.
                    let got = serde_json::from_slice::<Value>(&b).map(|v| Library::lenient(&v)).unwrap_or_default();
                    let _ = std::fs::rename(&p, dir.join("library.broken.json"));
                    got
                }
            },
            Err(_) => Library::default(),
        };
        // A library an older launcher set aside as broken (and then started empty): its games come
        // back, those that aren't here already.
        let broken = dir.join("library.broken.json");
        if let Some(v) = std::fs::read(&broken).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
            let old = Library::lenient(&v);
            let mut back = 0;
            for (id, g) in old.installed {
                if !l.installed.contains_key(&id) {
                    l.installed.insert(id, g);
                    back += 1;
                }
            }
            if back > 0 && !l.settings.setup_done && old.settings.setup_done {
                l.settings = old.settings;
            }
            let _ = std::fs::rename(&broken, dir.join("library.recovered.json"));
            let _ = l.save(dir);
        }
        l
    }

    /// As much of a library file as can be read: every game that reads on its own, and each
    /// setting that does.
    pub fn lenient(v: &Value) -> Library {
        let mut l = Library::default();
        if let Some(m) = v.get("installed").and_then(Value::as_object) {
            for (id, g) in m {
                if let Ok(g) = serde_json::from_value::<Installed>(g.clone()) {
                    l.installed.insert(id.clone(), g);
                }
            }
        }
        if let Some(s) = v.get("settings").and_then(Value::as_object) {
            let mut have = serde_json::to_value(&l.settings).unwrap_or(Value::Null);
            for (k, val) in s {
                let mut next = have.clone();
                next[k] = val.clone();
                if serde_json::from_value::<Settings>(next.clone()).is_ok() {
                    have = next;
                }
            }
            if let Ok(s) = serde_json::from_value::<Settings>(have) {
                l.settings = s;
            }
        }
        let field = |k: &str| v.get(k).cloned().unwrap_or(Value::Null);
        l.roms = serde_json::from_value(field("roms")).unwrap_or_default();
        l.seen_apps = serde_json::from_value(field("seen_apps")).unwrap_or_default();
        l.seen_versions = serde_json::from_value(field("seen_versions")).unwrap_or_default();
        l.mods = serde_json::from_value(field("mods")).unwrap_or_default();
        l
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join("library.json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, dir.join("library.json"))
    }

    /// Games whose folder can't be found right now: deleted, or on a drive that isn't connected.
    /// They stay in the Library, shown as missing, until the player finds them or takes them out.
    pub fn missing(&self) -> Vec<String> {
        self.installed.values().filter(|g| !g.path.is_dir()).map(|g| g.id.clone()).collect()
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The folder name for a game: its catalog id, kept to safe characters.
pub fn folder_name(id: &str) -> String {
    let s: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
    if s.is_empty() || s.chars().all(|c| c == '-') {
        "game".into()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_old_files_still_load() {
        let d = std::env::temp_dir().join(format!("rl-lib-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let mut l = Library::default();
        l.installed.insert("z".into(), Installed { id: "z".into(), name: "Z".into(), tag: "v1".into(), path: d.clone(), ..Default::default() });
        l.save(&d).unwrap();
        let back = Library::load(&d);
        assert_eq!(back.installed["z"].tag, "v1");
        std::fs::write(d.join("library.json"), br#"{"installed":{},"settings":{"library_dir":"/x"}}"#).unwrap();
        let old = Library::load(&d);
        assert!(old.settings.check_updates);
        std::fs::write(d.join("library.json"), b"{broken").unwrap();
        assert!(Library::load(&d).installed.is_empty());
        assert!(d.join("library.broken.json").is_file());
    }

    #[test]
    fn a_bad_entry_never_takes_the_whole_library() {
        let d = std::env::temp_dir().join(format!("rl-lib2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        // One game with a field of the wrong kind, one fine, and a setting of the wrong kind.
        let f = br#"{"installed":{"a":{"id":"a","name":"A","tag":"v1","path":"/x/a","played_secs":"lots"},"b":{"id":"b","name":"B","tag":"v2","path":"/x/b"}},"settings":{"library_dir":"/lib","notify":"yes","setup_done":true}}"#;
        std::fs::write(d.join("library.json"), f).unwrap();
        let l = Library::load(&d);
        assert_eq!(l.installed.len(), 1);
        assert_eq!(l.installed["b"].tag, "v2");
        assert_eq!(l.settings.library_dir, PathBuf::from("/lib"));
        assert!(l.settings.setup_done);
        // Set aside by an older launcher, which then started empty: the games come back.
        std::fs::write(d.join("library.json"), br#"{"installed":{}}"#).unwrap();
        std::fs::write(d.join("library.broken.json"), br#"{"installed":{"c":{"id":"c","name":"C","tag":"v3","path":"/x/c"}},"settings":{"library_dir":"/lib","setup_done":true}}"#).unwrap();
        let l = Library::load(&d);
        assert_eq!(l.installed["c"].tag, "v3");
        assert!(l.settings.setup_done);
        assert!(!d.join("library.broken.json").exists() && d.join("library.recovered.json").is_file());
        assert_eq!(Library::load(&d).installed.len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn folder_names() {
        assert_eq!(folder_name("zelda64recomp"), "zelda64recomp");
        assert_eq!(folder_name("../x"), "---x");
        assert_eq!(folder_name("."), "game");
    }
}

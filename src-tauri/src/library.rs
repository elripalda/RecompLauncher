//! What's installed and the launcher's settings, kept in library.json in the app's data folder.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Set Up By Hand: how a game starts and gets the player's game files, as the player said.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ManualSetup {
    /// The program that starts it (in its folder, or anywhere).
    #[serde(default)]
    pub exe: Option<PathBuf>,
    /// The player's game files: a disc image, a ROM, or a folder.
    #[serde(default)]
    pub copy: Option<PathBuf>,
    /// "folder": put in a folder inside the game's folder (`into`, "." for the folder itself);
    /// "arg": handed to the program on its command line (`arg`, with {copy} for where they are);
    /// "none": the game asks for them itself.
    #[serde(default)]
    pub how: String,
    #[serde(default)]
    pub into: String,
    /// A single file's name there, when the game wants it named a certain way.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub arg: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Installed {
    pub id: String,
    pub name: String,
    /// The game a folder the player added is a recomp of, when the launcher could tell (its
    /// recomplauncher.json, or its README). Show Game Names uses it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub game: String,
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
    /// Set Up By Hand: the player says which program starts it and how it gets their game files.
    /// While it's on, it's used instead of the launcher's own setup for this game.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manual: Option<ManualSetup>,
    /// The game's update the player picked, for recomps that need one beside their copy.
    #[serde(default)]
    pub update_copy: Option<PathBuf>,
    /// Named Easy Setup selections; shared keys are kept separately.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub setup_inputs: BTreeMap<String, PathBuf>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub inputs_ready: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub input_staged: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub input_stamps: BTreeMap<String, String>,
    /// A Windows build, started through Proton on Linux.
    #[serde(default)]
    pub proton: bool,
    /// Settings passed on the command line: setting id -> choice.
    #[serde(default)]
    pub picks: BTreeMap<String, usize>,
    /// Settings the player put on Match My Screen: they follow the monitor, written again before
    /// Play when its size changed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub screen_fit: Vec<String>,
    /// The monitor's size ("3440x1440") when Match My Screen last wrote it: written again only when
    /// the monitor changes, so a size the player set in the game's own menu stays.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub screen_size: String,
    /// Sliders passed on the command line: setting id -> the value as written.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub slider_vals: BTreeMap<String, String>,
    /// Optional files the launcher fetches itself (NVIDIA DLSS) that the player turned off for this
    /// game in Easy Setup, by input id.
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub runtime_off: std::collections::BTreeSet<String>,
    /// A recomp that carries several games: each one's play time and setup (games.rs), by game id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub games: BTreeMap<String, crate::games::Played>,
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
    /// More folders the player added (a launcher with several games, each saving in its own).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub more_save_places: Vec<String>,
    /// A picture the player picked for its cover (a copy in the launcher's covers folder).
    #[serde(default)]
    pub cover_file: Option<PathBuf>,
    /// The player's own picture behind the game's page (a wide one), in the launcher's covers folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hero_file: Option<PathBuf>,
    /// The player's own logo (a see-through picture over the background), in the covers folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo_file: Option<PathBuf>,
    /// The player's own icon (square), in the covers folder: for shortcuts, Steam and the game's
    /// window while it runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_file: Option<PathBuf>,
    /// How the player framed each of their pictures, so Customize can open it again as it was:
    /// "cover" / "hero" / "logo" / "icon" -> { src: the picture as it came, x, y, zoom }.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub art_edit: serde_json::Map<String, serde_json::Value>,
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
    /// The version it had before its last update: what "go back" installs again when the update
    /// breaks it. Empty until it has been updated.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub prev_tag: String,
    /// Put in place by its own installer (the catalog's "installer"): Remove offers its
    /// uninstaller and never deletes the folder itself.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub installer: bool,
    /// Its downloaded installer, waiting for Set Up (in the launcher's staging folder), and the
    /// release it installs.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub installer_setup: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub installer_tag: String,
    /// The fingerprint of how its profile started it at the last Play (boot::start_hash_of): a new
    /// one from the catalog is applied on the next Play.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub boot_hash: String,
}

fn half() -> u8 {
    50
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Where new games go.
    pub library_dir: PathBuf,
    /// Settings → Network: "off", "system" (or empty, the default) or "custom", with the custom
    /// address, its user, and its password sealed (secret::seal), never stored in the clear.
    #[serde(default)]
    pub proxy: String,
    #[serde(default)]
    pub proxy_url: String,
    #[serde(default)]
    pub proxy_user: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub proxy_pass: String,
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
    /// A folder the player picked for the copies of their saves (none: the launcher's own).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_dir: Option<PathBuf>,
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
    /// Settings → Run in background. Keep the existing field name so saved choices still apply.
    #[serde(default)]
    pub close_to_tray: bool,
    /// Settings → Controllers: the left stick moves around the launcher as the d-pad does. Off,
    /// only the d-pad does (for players whose stick drifts, or who move a mouse with it).
    #[serde(default = "yes")]
    pub pad_stick: bool,
    /// On Linux: recomps with only a Windows version install that and play through Proton.
    #[serde(default)]
    pub proton: bool,
    /// Proton was turned on for the player once, when the launcher found it (Beta 6). After that
    /// it stays however they set it.
    #[serde(default)]
    pub proton_auto: bool,
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
    /// Linux: how the launcher's window is drawn. "fast" (the graphics card's quickest way),
    /// "safe" (the way that works everywhere) or empty (picked for this PC). Read at start.
    #[serde(default)]
    pub linux_graphics: String,
    /// Settings → Keep Steam in Sync: every installed game that's ready to play is in Steam, in a
    /// RecompLauncher collection, and leaves it when uninstalled (steam_sync.rs).
    #[serde(default)]
    pub steam_sync: bool,
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
            proxy: String::new(),
            proxy_url: String::new(),
            proxy_user: String::new(),
            proxy_pass: String::new(),
            hide_ai: false,
            check_updates: true,
            keep_downloads: false,
            ui: Value::Null,
            library_dirs: vec![],
            rom_dirs: vec![],
            asked_location: false,
            start_in_tv: false,
            setup_done: false,
            close_to_tray: false,
            pad_stick: true,
            proton: false,
            proton_auto: false,
            tray_told: false,
            share_usage: false,
            share_stats: false,
            data_asked: false,
            notify: true,
            backup_before_update: true,
            save_dir: None,
            tv_size: String::new(),
            sounds: true,
            menu_tips: 0,
            discord: true,
            quick_menu: true,
            minimize_playing: None,
            lighter: None,
            linux_graphics: String::new(),
            steam_sync: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Library {
    /// Old ids retained for Steam and desktop shortcuts after correcting a project match.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub project_aliases: BTreeMap<String, String>,
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
    /// The player's own pictures for games that aren't installed (Customize from Discover or the
    /// Library first, or kept when a game is uninstalled): put on the game when it's installed.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub arts: BTreeMap<String, ArtSet>,
}

/// A game's own pictures without the game: what Customize keeps for it.
#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
pub struct ArtSet {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_file: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hero_file: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo_file: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_file: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub art_edit: serde_json::Map<String, serde_json::Value>,
}

impl ArtSet {
    pub fn is_empty(&self) -> bool {
        self.cover_file.is_none() && self.hero_file.is_none() && self.logo_file.is_none() && self.icon_file.is_none() && self.art_edit.is_empty()
    }
    /// A game's pictures, to keep when it leaves the Library.
    pub fn of(g: &Installed) -> ArtSet {
        ArtSet { cover_file: g.cover_file.clone(), hero_file: g.hero_file.clone(), logo_file: g.logo_file.clone(), icon_file: g.icon_file.clone(), art_edit: g.art_edit.clone() }
    }
    /// Put on a game being installed, where it has none of its own.
    pub fn onto(self, g: &mut Installed) {
        g.cover_file = g.cover_file.take().or(self.cover_file);
        g.hero_file = g.hero_file.take().or(self.hero_file);
        g.logo_file = g.logo_file.take().or(self.logo_file);
        g.icon_file = g.icon_file.take().or(self.icon_file);
        for (k, v) in self.art_edit {
            g.art_edit.entry(k).or_insert(v);
        }
    }
}

impl Library {
    pub fn resolve_project_id(&self, id: &str) -> String {
        if self.installed.contains_key(id) { return id.to_string(); }
        self.project_aliases.get(id).filter(|new| self.installed.contains_key(*new)).cloned().unwrap_or_else(|| id.to_string())
    }

    /// A game into the Library: pictures the player picked for it before go on it.
    pub fn put(&mut self, id: &str, mut g: Installed) {
        if let Some(a) = self.arts.remove(id) {
            a.onto(&mut g);
        }
        g.exe = g.exe.map(|e| unix_path(&g.path, e));
        self.installed.insert(id.to_string(), g);
    }

    /// A game out of the Library: its own pictures stay for it.
    pub fn forget(&mut self, id: &str) -> Option<Installed> {
        let g = self.installed.remove(id)?;
        let a = ArtSet::of(&g);
        if !a.is_empty() {
            self.arts.insert(id.to_string(), a);
        }
        Some(g)
    }
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
    /// Programs written the Windows way (issn-windows\\Game.exe) on Linux or a Mac: their path
    /// the way this system reads it.
    fn tidy_paths(&mut self) {
        for g in self.installed.values_mut() {
            let base = g.path.clone();
            g.exe = g.exe.take().map(|e| unix_path(&base, e));
            if let Some(m) = g.manual.as_mut() {
                m.exe = m.exe.take().map(|e| unix_path(&base, e));
            }
        }
    }

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
        l.tidy_paths();
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
        l.project_aliases = serde_json::from_value(field("project_aliases")).unwrap_or_default();
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
/// `child` is inside `parent` (not the folder itself), comparing names the way the system does
/// (any case on Windows); a path that steps back up (..) never counts.
pub fn inside(child: &Path, parent: &Path) -> bool {
    use std::path::Component;
    if child.components().any(|c| c == Component::ParentDir) {
        return false;
    }
    let names = |p: &Path| -> Vec<String> {
        p.components()
            .filter(|c| !matches!(c, Component::CurDir))
            .map(|c| { let s = c.as_os_str().to_string_lossy().to_string(); if cfg!(windows) { s.to_lowercase() } else { s } })
            .collect()
    };
    let (c, p) = (names(child), names(parent));
    c.len() > p.len() && c[..p.len()] == p[..]
}

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
    #[test]
    fn run_in_background_is_off_by_default() {
        assert!(!super::Settings::default().close_to_tray);
        let settings: super::Settings = serde_json::from_str(r#"{"library_dir":"/games"}"#).unwrap();
        assert!(!settings.close_to_tray);
    }

    #[test]
    fn an_existing_background_choice_stays_on() {
        let settings: super::Settings = serde_json::from_str(r#"{"library_dir":"/games","close_to_tray":true}"#).unwrap();
        assert!(settings.close_to_tray);
        let saved = serde_json::to_string(&settings).unwrap();
        let restored: super::Settings = serde_json::from_str(&saved).unwrap();
        assert!(restored.close_to_tray);
    }

    #[test]
    fn an_existing_background_choice_stays_off() {
        let settings: super::Settings = serde_json::from_str(r#"{"library_dir":"/games","close_to_tray":false}"#).unwrap();
        assert!(!settings.close_to_tray);
        let saved = serde_json::to_string(&settings).unwrap();
        let restored: super::Settings = serde_json::from_str(&saved).unwrap();
        assert!(!restored.close_to_tray);
    }

    #[test]
    #[cfg(not(windows))]
    fn a_windows_path_reads_on_linux() {
        let d = std::env::temp_dir().join(format!("rl-unix-{}", std::process::id()));
        let base = d.join("issn-windows");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("inFAMOUS Second Son.exe"), b"MZ").unwrap();
        // The folder's own name repeated, the Windows way: found where it is.
        assert_eq!(super::unix_path(&base, std::path::PathBuf::from("issn-windows\\inFAMOUS Second Son.exe")), std::path::PathBuf::from("inFAMOUS Second Son.exe"));
        // Plain backslashes become slashes.
        assert_eq!(super::unix_path(&base, std::path::PathBuf::from("bin\\game.exe")), std::path::PathBuf::from("bin/game.exe"));
        // A Linux path is left alone.
        assert_eq!(super::unix_path(&base, std::path::PathBuf::from("bin/game")), std::path::PathBuf::from("bin/game"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_games_own_pictures_stay_when_it_leaves_and_come_back_with_it() {
        let mut l = Library::default();
        let g = Installed { id: "x".into(), cover_file: Some(PathBuf::from("/c/x-cover.jpg")), ..Default::default() };
        l.installed.insert("x".into(), g);
        assert!(l.forget("x").is_some());
        assert_eq!(l.arts["x"].cover_file, Some(PathBuf::from("/c/x-cover.jpg")));
        // Picked a logo for it while it wasn't installed, then installed it again.
        l.arts.get_mut("x").unwrap().logo_file = Some(PathBuf::from("/c/x-logo.png"));
        l.put("x", Installed { id: "x".into(), ..Default::default() });
        let g = &l.installed["x"];
        assert_eq!((g.cover_file.clone(), g.logo_file.clone()), (Some(PathBuf::from("/c/x-cover.jpg")), Some(PathBuf::from("/c/x-logo.png"))));
        assert!(l.arts.is_empty());
        // A game without pictures leaves nothing behind.
        l.installed.insert("y".into(), Installed { id: "y".into(), ..Default::default() });
        l.forget("y");
        assert!(!l.arts.contains_key("y"));
    }

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
    fn inside_a_folder() {
        assert!(inside(Path::new("/g/tst/rom/a.nds"), Path::new("/g/tst")));
        assert!(!inside(Path::new("/g/tst"), Path::new("/g/tst")));
        assert!(!inside(Path::new("/g/tst/../x.iso"), Path::new("/g/tst")));
        assert!(!inside(Path::new("/g/tst2/a.iso"), Path::new("/g/tst")));
    }

    #[test]
    fn folder_names() {
        assert_eq!(folder_name("zelda64recomp"), "zelda64recomp");
        assert_eq!(folder_name("../x"), "---x");
        assert_eq!(folder_name("."), "game");
    }
}

/// A game's program as this system reads its path: on Linux or a Mac, backslashes become slashes,
/// and a path that repeats the game's own folder name (issn-windows/issn-windows/Game.exe, from a
/// layout that's since changed) is found where it is, when it's there.
pub fn unix_path(base: &Path, e: PathBuf) -> PathBuf {
    if cfg!(windows) || e.is_absolute() {
        return e;
    }
    let s = e.to_string_lossy();
    if !s.contains('\\') {
        return e;
    }
    let fixed = PathBuf::from(s.replace('\\', "/"));
    if base.join(&fixed).exists() {
        return fixed;
    }
    let own = base.file_name().map(|n| n.to_owned());
    let mut parts = fixed.components();
    if let (Some(first), Some(own)) = (parts.next(), own) {
        let rest: PathBuf = parts.collect();
        if first.as_os_str() == own.as_os_str() && base.join(&rest).exists() {
            return rest;
        }
    }
    fixed
}

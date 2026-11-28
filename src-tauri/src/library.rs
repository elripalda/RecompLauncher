//! What's installed and the launcher's settings, kept in library.json in the app's data folder.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A copy read whole, with no errors: which file, as it was then, and its checksum.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct CopyCheck {
    pub path: PathBuf,
    pub size: u64,
    /// Its modified time, in seconds.
    pub modified: i64,
    pub crc32: String,
}

impl CopyCheck {
    /// The file as it is now: None when it can't be read.
    pub fn stamp(path: &Path) -> Option<(u64, i64)> {
        let m = std::fs::metadata(path).ok()?;
        let t = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64;
        Some((m.len(), t))
    }

    /// Its checksum, when `path` is the file it read and it hasn't changed since.
    pub fn still(&self, path: &Path) -> Option<&str> {
        let (size, modified) = Self::stamp(path)?;
        (self.path == path && self.size == size && self.modified == modified).then_some(self.crc32.as_str())
    }
}

/// Who sets a game up. Switching only changes this: what the player picked for each way (their
/// program, their copy) stays recorded, so switching back
/// finds it as it was.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SetupMode {
    /// The launcher, with Easy Setup (on games it can do; other games start as they come).
    #[default]
    Easy,
    /// The player (Skip Easy Setup): Play runs the release's program, or the one they picked.
    Own,
}

impl SetupMode {
    fn is_easy(&self) -> bool {
        *self == SetupMode::Easy
    }
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
    /// The release (its tag) the last Easy Setup that worked finished on. Empty: never set up.
    /// A game whose setup does something Play doesn't (a setup step, a disc to unpack) is set up
    /// while this is the installed `tag`; a new release builds it again. Picking other files
    /// clears it. One game of several keeps its own in `games`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub set_up: String,
    /// A build (a setup step, a disc unpacked) started and didn't finish: stopped, the launcher
    /// closed, or the PC went off partway. What it wrote over the last build may be half there,
    /// so the game builds again before it plays, even on the release `set_up` names. One game of
    /// several keeps its own in `games`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub build_cut: bool,
    /// Read once from a library saved before named inputs were the one way (format 3), to tell
    /// which games were set up then: see `SetupUpgrade`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upgrade: Option<SetupUpgrade>,
    /// Brought into the Library from a folder where it already plays: its game files are there, so
    /// Easy Setup doesn't ask for the player's copy.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub files_in_place: bool,
    /// Who sets this game up.
    #[serde(default, skip_serializing_if = "SetupMode::is_easy")]
    pub mode: SetupMode,
    /// The files the player picked for Easy Setup, by input id: `game` for their copy of the game,
    /// `update` for its update, and the rest by the manifest's ids. Shared keys are kept apart.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub setup_inputs: BTreeMap<String, PathBuf>,
    /// The disc image a picked file was unpacked from (`extract`), by input id: the game reads the
    /// unpacked folder (in `setup_inputs`), so Free Up Space can offer the image.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub unpacked_from: BTreeMap<String, PathBuf>,
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
    /// The ticks the player set for its setup step's options (prepare.options), by option id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub prepare_opts: BTreeMap<String, bool>,
    /// Its last download came from the RecompLauncher backup (its project was down).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub from_backup: bool,
    /// A recomp that carries several games: each one's play time and setup (games.rs), by game id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub games: BTreeMap<String, crate::games::Played>,
    /// The player's own launch options, added after everything else.
    #[serde(default)]
    pub args: String,
    /// Install updates as soon as they're found.
    #[serde(default)]
    pub auto_update: bool,
    /// Build it again right after an automatic update to a new release, in the background, when
    /// nothing has to be asked (the launcher queues them, one at a time). Off: Play builds it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rebuild_after_update: bool,
    /// A developer's own build, added from a folder on this PC: never updated, moved or deleted.
    #[serde(default)]
    pub local: bool,
    /// Its folder is one the launcher unpacked the player's AppImage into (plain.rs), so it's the
    /// launcher's own: uninstalling can delete it, even for a game the player added.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unpacked: bool,
    /// The player's AppImage it came from, by its name without the version
    /// (game-x86-64.appimage): a newer one they bring in updates it. Its path isn't kept.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub from_appimage: String,
    /// The look the game starts with (looks.rs), empty for the game's own picture.
    #[serde(default)]
    pub look: String,
    /// How strong it is, 0 to 100.
    #[serde(default = "half")]
    pub look_amount: u8,
    /// Cloud saves for this game, when the player switched them for it (its Manage tab). None:
    /// what they picked for every game (see `cloud_on`).
    #[serde(default)]
    pub cloud: Option<bool>,
    /// Where the player says the game saves (Saves, Change). Only used while neither its developer
    /// nor the catalog names a folder (saves::spots): a pick made before one was named is stale.
    #[serde(default)]
    pub save_place: Option<String>,
    /// The player's folder in place of the one its developer or the catalog names (Saves, Change
    /// on that folder), for a game they set up themselves. Its own field, not `save_place`: old
    /// stale picks there must not come back. Ignored while Easy Setup is on (saves::with_override).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_override: Option<String>,
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
    /// The move to one road (one_road.rs), done once: "moved" (it's the player's own now, with what
    /// our profile did kept in its launch options), or "again: <why>" (left as it was; set up again
    /// once the catalog stops carrying our profile).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub one_road: String,
    /// The files Easy Setup read whole, by input id, and what it read: Set Up Again with the same
    /// file, unchanged since, doesn't read it all again (a Switch 2 package is over 20 GB).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub input_checks: BTreeMap<String, CopyCheck>,
}

/// How a game was set up in a library from before format 3, kept until the launcher has read it
/// once with the game's profile (lib.rs, `upgrade_setups`): a setup current then is marked as set
/// up on the installed release, so nobody builds again just for this launcher. One that was due a
/// new build then gets no mark and builds on the next Play.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct SetupUpgrade {
    /// It had named inputs set up (an old fingerprint was kept).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub inputs_ready: bool,
    /// The `prepare.version` it was last built with.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub prepare_version: String,
    /// One game of several whose own inputs were set up, by id.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub games_ready: Vec<String>,
}

impl Installed {
    /// Cloud saves on for this game: its own switch when it was set, otherwise what the player
    /// picked for every game (`ui.cloud_all`, asked once after signing in; not asked yet is off).
    /// Paired with `cloudOn` in ui/app.js, which shows it on the switches: change both together.
    pub fn cloud_on(&self, ui: &Value) -> bool {
        let every_game = ui.get("cloud_all").and_then(Value::as_bool).unwrap_or(false);
        self.cloud.unwrap_or(every_game)
    }

    /// Its installer is downloaded and waits for Set Up: its folder is made by that installer, so
    /// a folder that isn't there yet isn't a missing game.
    pub fn installer_waits(&self) -> bool {
        !self.installer_setup.is_empty() && self.exe.is_none() && !self.path.is_dir()
    }

    /// The player sets this game up their own way (Skip Easy Setup). Only its own mode counts: the
    /// Settings switch is for new games, and a game installed under it took it as its mode then.
    pub fn player_sets_up(&self) -> bool {
        self.mode == SetupMode::Own
    }

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
    /// The monitor Big Picture mode opens on (empty or "auto": the main display).
    #[serde(default)]
    pub tv_monitor: String,
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
    /// Settings → Close to tray (called Run in background before; the field name never changed).
    #[serde(default)]
    pub close_to_tray: bool,
    /// Settings → Start with your PC. The system's own startup list is what counts: this follows
    /// it (see autostart.rs), and changes only through set_start_at_login.
    #[serde(default)]
    pub start_at_login: bool,
    /// Settings → Controllers: the left stick moves around the launcher as the d-pad does. Off,
    /// only the d-pad does (for players whose stick drifts, or who move a mouse with it).
    #[serde(default = "yes")]
    pub pad_stick: bool,
    /// Settings → Controllers: the controller moves around the desktop view too. Off, the desktop
    /// view leaves the controller alone (for players who use it as a mouse, where its buttons would
    /// also press things in the launcher); holding Menu still opens Big Picture mode. Its switch,
    /// Controller as a Mouse, shows the opposite: on when this is false.
    #[serde(default = "yes")]
    pub pad_desktop: bool,
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
    /// Settings → Auto-Install: once an Easy Setup game or a game that only comes as an installer
    /// finishes downloading (an update too), its setup opens straight away. Off: it waits for Set Up.
    #[serde(default = "yes")]
    pub auto_install: bool,
    /// Settings → Update All Games Automatically: every installed game takes its new release as
    /// soon as the launcher finds it, like each game's own Update automatically. Off by default,
    /// so nobody's games start updating by themselves after the launcher updates.
    #[serde(default)]
    pub auto_update_all: bool,
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
            start_at_login: false,
            pad_stick: true,
            pad_desktop: true,
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
            tv_monitor: String::new(),
            sounds: true,
            menu_tips: 0,
            discord: true,
            quick_menu: true,
            minimize_playing: None,
            lighter: None,
            linux_graphics: String::new(),
            steam_sync: false,
            auto_install: true,
            auto_update_all: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Library {
    /// The file's format (`library_convert::FORMAT`): an older one is converted when it's read.
    #[serde(default)]
    pub format: u32,
    /// Old ids retained for Steam and desktop shortcuts after correcting a project match.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub project_aliases: BTreeMap<String, String>,
    #[serde(default)]
    pub installed: BTreeMap<String, Installed>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) removed_play_history: BTreeMap<String, PlayHistory>,
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
    /// Coming Soon recomps in the last catalog, so a saved one that came out can be told apart.
    #[serde(default)]
    pub seen_soon: Vec<String>,
    /// Mods the launcher put in, by game id.
    #[serde(default)]
    pub mods: BTreeMap<String, Vec<crate::mods::ModRec>>,
    /// The player's own pictures for games that aren't installed (Customize from Discover or the
    /// Library first, or kept when a game is uninstalled): put on the game when it's installed.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub arts: BTreeMap<String, ArtSet>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub(crate) struct PlayHistory {
    #[serde(flatten)]
    recomp: crate::games::Played,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    games: BTreeMap<String, crate::games::Played>,
}

/// One game's time on this PC as the website keeps it: seconds played here, ever, and when last;
/// for its play time card, the sessions this PC kept and the longest of them, and the recent ones
/// ([start, secs]) for this week, this month and the bars. Only recent ones go: older sessions
/// would only make each send bigger, as the card shows nothing of them but count and longest.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct PlayEntry {
    pub game: String,
    pub secs: u64,
    pub last: u64,
    pub count: u64,
    pub longest: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub recent: Vec<[u64; 2]>,
}

/// How far back a play time card looks: a month, plus a day for time zones.
pub const PLAY_RECENT_SECS: u64 = 31 * 86400;

/// A developer's own builds ("local-…", "own-…") aren't catalog games: their time stays on this PC.
fn own_build(id: &str) -> bool {
    id.starts_with("local-") || id.starts_with("own-")
}

/// `since`: the oldest session start that still counts as recent.
fn entry(game: String, p: &crate::games::Played, since: u64) -> Option<PlayEntry> {
    if p.played_secs == 0 {
        return None;
    }
    let longest = p.sessions.iter().map(|s| s[1]).max().unwrap_or(0);
    let recent = p.sessions.iter().filter(|s| s[0] >= since).copied().collect();
    let count = p.sessions.len() as u64;
    Some(PlayEntry { game, secs: p.played_secs, last: p.last_played, count, longest, recent })
}

impl Library {
    /// Every game's time on this PC, for the account: installed games and what's kept from
    /// uninstalled ones. A recomp with several games also sends each game as "recomp--game".
    /// `only`: just those ids (a recomp's id brings its games along). Sessions that started
    /// before `since` count toward count and longest but aren't sent themselves.
    pub fn play_entries(&self, only: Option<&[String]>, since: u64) -> Vec<PlayEntry> {
        let wanted = |id: &str| only.is_none_or(|ids| ids.iter().any(|w| w == id));
        let mut out = Vec::new();
        for (id, g) in &self.installed {
            if g.local || own_build(id) || !wanted(id) {
                continue;
            }
            let recomp = crate::games::Played { played_secs: g.played_secs, last_played: g.last_played, sessions: g.sessions.clone(), ..Default::default() };
            out.extend(entry(id.clone(), &recomp, since));
            for (sub, p) in &g.games {
                out.extend(entry(crate::games::full(id, sub), p, since));
            }
        }
        for (id, h) in &self.removed_play_history {
            if own_build(id) || !wanted(id) || self.installed.contains_key(id) {
                continue;
            }
            out.extend(entry(id.clone(), &h.recomp, since));
            for (sub, p) in &h.games {
                out.extend(entry(crate::games::full(id, sub), p, since));
            }
        }
        out
    }

    /// A project's new id takes over the play history kept for its old one, so a renamed
    /// project's uninstalled time isn't left behind under an id nothing uses any more.
    pub(crate) fn move_play_history(&mut self, old: &str, new: &str) {
        let Some(history) = self.removed_play_history.remove(old) else { return };
        let kept = self.removed_play_history.entry(new.to_string()).or_default();
        kept.recomp.absorb(history.recomp);
        for (sub, played) in history.games {
            kept.games.entry(sub).or_default().absorb(played);
        }
    }

    /// Forgets this PC's play time for an installed game (issue #487: some setups start the game
    /// and count as play). A recomp's id clears it and all its games; one game of several clears
    /// that game and takes its time off the recomp's, so the two still add up.
    /// -> the ids the website keeps this PC's time under, to clear there too. None: not installed.
    pub fn reset_play(&mut self, id: &str) -> Option<Vec<String>> {
        if let Some(g) = self.installed.get_mut(id) {
            let mut ids = vec![id.to_string()];
            ids.extend(g.games.keys().map(|sub| crate::games::full(id, sub)));
            g.played_secs = 0;
            g.last_played = 0;
            g.sessions.clear();
            for p in g.games.values_mut() {
                clear_played(p);
            }
            return Some(ids);
        }
        let (rid, sub) = crate::games::split(id)?;
        let g = self.installed.get_mut(rid)?;
        let p = g.games.get_mut(sub)?;
        g.played_secs = g.played_secs.saturating_sub(p.played_secs);
        // Each of its sessions was counted on the recomp too, as the same [start, secs].
        for s in &p.sessions {
            if let Some(i) = g.sessions.iter().position(|x| x == s) {
                g.sessions.remove(i);
            }
        }
        if let Some(end) = g.sessions.iter().map(|s| s[0] + s[1]).max() {
            g.last_played = end;
        }
        if g.played_secs == 0 {
            g.last_played = 0;
        }
        clear_played(p);
        Some(vec![id.to_string(), rid.to_string()])
    }
}

/// No time and no sessions; its setup stays as it is.
fn clear_played(p: &mut crate::games::Played) {
    p.played_secs = 0;
    p.last_played = 0;
    p.sessions.clear();
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

    /// A game into the Library: restore its pictures and play history.
    pub fn put(&mut self, id: &str, mut g: Installed) {
        if let Some(a) = self.arts.remove(id) {
            a.onto(&mut g);
        }
        // Added to, not replaced: a record that already has time (a rename, a backup) keeps it.
        if let Some(history) = self.removed_play_history.remove(id) {
            let mut recomp = crate::games::Played { played_secs: g.played_secs, last_played: g.last_played, sessions: std::mem::take(&mut g.sessions), ..Default::default() };
            recomp.absorb(history.recomp);
            g.played_secs = recomp.played_secs;
            g.last_played = recomp.last_played;
            g.sessions = recomp.sessions;
            for (sub, played) in history.games {
                g.games.entry(sub).or_default().absorb(played);
            }
        }
        g.exe = g.exe.map(|e| unix_path(&g.path, e));
        self.installed.insert(id.to_string(), g);
    }

    /// A game out of the Library: keep its pictures and play history.
    pub fn forget(&mut self, id: &str) -> Option<Installed> {
        let g = self.installed.remove(id)?;
        let a = ArtSet::of(&g);
        if !a.is_empty() {
            self.arts.insert(id.to_string(), a);
        }
        let mut history = PlayHistory::default();
        history.recomp.played_secs = g.played_secs;
        history.recomp.last_played = g.last_played;
        history.recomp.sessions = g.sessions.clone();
        for (sub, played) in &g.games {
            let mut played = played.clone();
            played.set_up.clear();
            if played != crate::games::Played::default() {
                history.games.insert(sub.clone(), played);
            }
        }
        if history != PlayHistory::default() {
            self.removed_play_history.insert(id.to_string(), history);
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
        }
    }

    pub fn load(dir: &Path) -> Library {
        let p = dir.join("library.json");
        let mut converted = false;
        let mut l = match std::fs::read(&p) {
            Ok(b) => match serde_json::from_slice::<Value>(&b) {
                Ok(mut v) => {
                    converted = crate::library_convert::to_current(&mut v);
                    match serde_json::from_value::<Library>(v.clone()) {
                        Ok(l) => l,
                        Err(_) => {
                            // A file this launcher can't read whole: everything in it that it can
                            // read is kept (each game on its own), and the file itself is kept for
                            // a look later.
                            let _ = std::fs::rename(&p, dir.join("library.broken.json"));
                            Library::lenient(&v)
                        }
                    }
                }
                Err(_) => {
                    let _ = std::fs::rename(&p, dir.join("library.broken.json"));
                    Library::default()
                }
            },
            Err(_) => Library::default(),
        };
        l.format = crate::library_convert::FORMAT;
        // A phone has no home folder to put the library in: it goes in the launcher's own folder.
        if cfg!(target_os = "android") && !l.settings.library_dir.is_absolute() {
            l.settings.library_dir = dir.join("Library");
        }
        if converted {
            let _ = l.save(dir);
        }
        // A library an older launcher set aside as broken (and then started empty): its games come
        // back, those that aren't here already.
        let broken = dir.join("library.broken.json");
        if let Some(mut v) = std::fs::read(&broken).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
            crate::library_convert::to_current(&mut v);
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
        // Every other field, each one kept when it reads: one that doesn't is left out, not the
        // rest with it. Taken by what the file has rather than a list here, so a field added later
        // (the player's pictures, say) isn't lost when the library is read this way.
        let Some(fields) = v.as_object() else { return l };
        let mut have = serde_json::to_value(&l).unwrap_or(Value::Null);
        for (k, val) in fields {
            if k == "installed" || k == "settings" {
                continue;
            }
            let mut next = have.clone();
            next[k] = val.clone();
            if serde_json::from_value::<Library>(next.clone()).is_ok() {
                have = next;
            }
        }
        serde_json::from_value(have).unwrap_or(l)
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
        self.installed.values().filter(|g| !g.path.is_dir() && !g.installer_waits()).map(|g| g.id.clone()).collect()
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
    fn auto_install_is_on_by_default_and_for_settings_saved_before_it() {
        // On keeps what every launcher before the switch did.
        assert!(super::Settings::default().auto_install);
        let settings: super::Settings = serde_json::from_str(r#"{"library_dir":"/games"}"#).unwrap();
        assert!(settings.auto_install);
    }

    #[test]
    fn auto_install_turned_off_stays_off() {
        let settings: super::Settings = serde_json::from_str(r#"{"library_dir":"/games","auto_install":false}"#).unwrap();
        let saved = serde_json::to_string(&settings).unwrap();
        let restored: super::Settings = serde_json::from_str(&saved).unwrap();
        assert!(!restored.auto_install);
    }

    #[test]
    fn the_controller_moves_around_the_desktop_view_by_default_and_for_settings_saved_before_it() {
        // On keeps what every launcher before the switch did.
        assert!(super::Settings::default().pad_desktop);
        let settings: super::Settings = serde_json::from_str(r#"{"library_dir":"/games"}"#).unwrap();
        assert!(settings.pad_desktop);
    }

    #[test]
    fn the_controller_turned_off_on_the_desktop_view_stays_off() {
        let settings: super::Settings = serde_json::from_str(r#"{"library_dir":"/games","pad_desktop":false}"#).unwrap();
        let saved = serde_json::to_string(&settings).unwrap();
        let restored: super::Settings = serde_json::from_str(&saved).unwrap();
        assert!(!restored.pad_desktop);
    }

    #[test]
    fn update_all_games_automatically_is_off_by_default_and_for_settings_saved_before_it() {
        assert!(!super::Settings::default().auto_update_all);
        let settings: super::Settings = serde_json::from_str(r#"{"library_dir":"/games"}"#).unwrap();
        assert!(!settings.auto_update_all);
    }

    #[test]
    fn update_all_games_automatically_turned_on_stays_on() {
        let settings: super::Settings = serde_json::from_str(r#"{"library_dir":"/games","auto_update_all":true}"#).unwrap();
        let saved = serde_json::to_string(&settings).unwrap();
        let restored: super::Settings = serde_json::from_str(&saved).unwrap();
        assert!(restored.auto_update_all);
    }

    #[test]
    fn run_in_background_is_off_by_default() {
        assert!(!super::Settings::default().close_to_tray);
        let settings: super::Settings = serde_json::from_str(r#"{"library_dir":"/games"}"#).unwrap();
        assert!(!settings.close_to_tray);
    }

    #[test]
    fn start_with_your_pc_is_off_by_default() {
        let settings: super::Settings = serde_json::from_str(r#"{"library_dir":"/games"}"#).unwrap();
        assert!(!settings.start_at_login);
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

    /// A library file with one game this launcher can't read (a number saved as text), beside a
    /// cover the player chose for a game that isn't installed.
    fn library_with_a_broken_game() -> Value {
        serde_json::json!({
            "installed": { "a": { "id": "a", "name": "A", "tag": "v1", "path": "/x/a", "played_secs": "lots" } },
            "settings": { "library_dir": "/lib" },
            "arts": { "c": { "cover_file": "/covers/c.png" } }
        })
    }

    #[test]
    fn reading_a_damaged_library_keeps_the_players_pictures() {
        let l = Library::lenient(&library_with_a_broken_game());
        assert_eq!(l.arts.get("c").and_then(|a| a.cover_file.clone()), Some(PathBuf::from("/covers/c.png")));
    }

    #[test]
    fn a_damaged_library_is_saved_again_with_the_players_pictures() {
        let d = std::env::temp_dir().join(format!("rl-library-damaged-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("library.json"), library_with_a_broken_game().to_string()).unwrap();
        Library::load(&d);
        let again = Library::load(&d);
        let _ = std::fs::remove_dir_all(&d);
        assert!(again.arts.contains_key("c"));
    }

    #[test]
    fn reading_a_damaged_library_keeps_every_field_that_reads() {
        let mut v = library_with_a_broken_game();
        v["seen_apps"] = serde_json::json!(["x"]);
        v["mods"] = serde_json::json!("not a list of mods");
        let l = Library::lenient(&v);
        assert_eq!(l.seen_apps, vec!["x".to_string()]);
        assert!(l.mods.is_empty());
    }

    #[test]
    fn reinstall_restores_saved_play_history() {
        let mut l = Library::default();
        let mut g = Installed { id: "series".into(), played_secs: 90, last_played: 200, sessions: vec![[110, 90]], ..Default::default() };
        g.games.insert("first".into(), crate::games::Played {
            played_secs: 90, last_played: 200, sessions: vec![[110, 90]], set_up: "v1".into(), ..Default::default()
        });
        l.put("series", g);
        l.forget("series").unwrap();

        let saved = serde_json::to_value(&l).unwrap();
        let mut l: Library = serde_json::from_value(saved.clone()).unwrap();
        assert_eq!(Library::lenient(&saved).removed_play_history, l.removed_play_history);
        l.put("series", Installed { id: "series".into(), ..Default::default() });

        let g = &l.installed["series"];
        assert_eq!((g.played_secs, g.last_played, g.sessions.as_slice()), (90, 200, &[[110, 90]][..]));
        let game = &g.games["first"];
        assert_eq!((game.played_secs, game.last_played, game.sessions.as_slice()), (90, 200, &[[110, 90]][..]));
        assert!(game.set_up.is_empty());
        assert!(l.removed_play_history.is_empty());
    }

    #[test]
    fn restored_play_history_adds_to_time_already_there() {
        let mut l = Library::default();
        l.put("x", Installed { id: "x".into(), played_secs: 60, last_played: 100, sessions: vec![[40, 60]], ..Default::default() });
        l.forget("x").unwrap();
        l.put("x", Installed { id: "x".into(), played_secs: 30, last_played: 300, sessions: vec![[270, 30]], ..Default::default() });

        let g = &l.installed["x"];
        assert_eq!((g.played_secs, g.last_played, g.sessions.as_slice()), (90, 300, &[[40, 60], [270, 30]][..]));
    }

    #[test]
    fn renamed_project_takes_its_old_play_history() {
        let mut l = Library::default();
        l.put("old", Installed { id: "old".into(), played_secs: 60, last_played: 100, ..Default::default() });
        l.forget("old").unwrap();
        l.move_play_history("old", "new");
        l.put("new", Installed { id: "new".into(), ..Default::default() });

        assert_eq!(l.installed["new"].played_secs, 60);
        assert!(l.removed_play_history.is_empty());
    }

    fn series_with_two_games() -> Library {
        let mut l = Library::default();
        let mut g = Installed { id: "series".into(), played_secs: 150, last_played: 400, sessions: vec![[100, 50], [300, 100]], set_up: "v1".into(), ..Default::default() };
        g.games.insert("first".into(), crate::games::Played { played_secs: 50, last_played: 150, sessions: vec![[100, 50]], set_up: "v1".into(), ..Default::default() });
        g.games.insert("second".into(), crate::games::Played { played_secs: 100, last_played: 400, sessions: vec![[300, 100]], ..Default::default() });
        l.installed.insert("series".into(), g);
        l
    }

    #[test]
    fn reset_play_clears_a_recomp_and_its_games() {
        let mut l = series_with_two_games();
        let ids = l.reset_play("series").unwrap();
        assert_eq!(ids, vec!["series", "series--first", "series--second"]);
        let g = &l.installed["series"];
        assert_eq!((g.played_secs, g.last_played, g.sessions.len()), (0, 0, 0));
        assert_eq!(g.games["second"].played_secs, 0);
        assert!(l.play_entries(None, 0).is_empty());
    }

    #[test]
    fn reset_play_keeps_setup_state() {
        let mut l = series_with_two_games();
        l.reset_play("series").unwrap();
        assert_eq!(l.installed["series"].set_up, "v1");
        assert_eq!(l.installed["series"].games["first"].set_up, "v1");
    }

    #[test]
    fn reset_play_of_one_game_takes_its_time_off_the_recomp() {
        let mut l = series_with_two_games();
        let ids = l.reset_play("series--second").unwrap();
        assert_eq!(ids, vec!["series--second", "series"]);
        let g = &l.installed["series"];
        assert_eq!((g.played_secs, g.last_played, g.sessions.as_slice()), (50, 150, &[[100, 50]][..]));
        assert_eq!(g.games["second"].played_secs, 0);
        assert_eq!(g.games["first"].played_secs, 50);
    }

    #[test]
    fn reset_play_of_a_game_not_installed_is_none() {
        let mut l = series_with_two_games();
        assert!(l.reset_play("other").is_none());
        assert!(l.reset_play("series--third").is_none());
    }

    #[test]
    fn play_entries_cover_installed_games_and_their_games() {
        let mut l = Library::default();
        let mut g = Installed { id: "bundle".into(), played_secs: 90, last_played: 200, ..Default::default() };
        g.games.insert("first".into(), crate::games::Played { played_secs: 60, last_played: 150, ..Default::default() });
        l.put("bundle", g);

        let ids: Vec<String> = l.play_entries(None, 0).into_iter().map(|e| e.game).collect();
        assert_eq!(ids, ["bundle", "bundle--first"]);
    }

    #[test]
    fn play_entries_include_uninstalled_games() {
        let mut l = Library::default();
        l.put("gone", Installed { id: "gone".into(), played_secs: 30, last_played: 100, ..Default::default() });
        l.forget("gone").unwrap();

        let gone = PlayEntry { game: "gone".into(), secs: 30, last: 100, count: 0, longest: 0, recent: vec![] };
        assert_eq!(l.play_entries(None, 0), [gone]);
    }

    #[test]
    fn play_entries_carry_each_games_recent_sessions() {
        let mut l = Library::default();
        let mut g = Installed { id: "bundle".into(), played_secs: 90, last_played: 200, sessions: vec![[110, 90]], ..Default::default() };
        g.games.insert("first".into(), crate::games::Played { played_secs: 60, last_played: 150, sessions: vec![[90, 60]], ..Default::default() });
        l.put("bundle", g);

        let recent: Vec<Vec<[u64; 2]>> = l.play_entries(None, 0).into_iter().map(|e| e.recent).collect();
        assert_eq!(recent, [vec![[110, 90]], vec![[90, 60]]]);
    }

    #[test]
    fn older_sessions_count_but_are_not_sent() {
        let mut l = Library::default();
        let sessions = vec![[10, 500], [100, 60], [200, 30]];
        l.put("g", Installed { id: "g".into(), played_secs: 590, last_played: 230, sessions, ..Default::default() });

        let e = &l.play_entries(None, 100)[0];
        assert_eq!((e.count, e.longest, e.recent.as_slice()), (3, 500, &[[100, 60], [200, 30]][..]));
    }

    #[test]
    fn play_entries_leave_out_own_builds_and_unplayed_games() {
        let mut l = Library::default();
        l.put("local-test", Installed { id: "local-test".into(), played_secs: 30, local: true, ..Default::default() });
        l.put("fresh", Installed { id: "fresh".into(), ..Default::default() });

        assert!(l.play_entries(None, 0).is_empty());
    }

    #[test]
    fn play_entries_for_one_game_only() {
        let mut l = Library::default();
        l.put("a", Installed { id: "a".into(), played_secs: 30, ..Default::default() });
        l.put("b", Installed { id: "b".into(), played_secs: 40, ..Default::default() });

        let ids: Vec<String> = l.play_entries(Some(&["b".to_string()]), 0).into_iter().map(|e| e.game).collect();
        assert_eq!(ids, ["b"]);
    }

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

#[cfg(test)]
mod installer_waits_tests {
    use super::*;

    #[test]
    fn a_game_whose_installer_waits_for_set_up_is_not_missing() {
        let mut lib = Library::default();
        let gone = std::env::temp_dir().join(format!("rl-not-there-{}", std::process::id()));
        // Downloaded installer, Set Up not run yet: the folder doesn't exist and that's expected.
        lib.installed.insert("hopper".into(), Installed { id: "hopper".into(), path: gone.clone(), installer: true, installer_setup: "C:/x/HopperSetup.exe".into(), ..Default::default() });
        // A plain game whose folder is gone is missing.
        lib.installed.insert("skyracer".into(), Installed { id: "skyracer".into(), path: gone.join("sky"), ..Default::default() });
        assert_eq!(lib.missing(), vec!["skyracer".to_string()]);
        // Once its installer ran (no setup waiting) a gone folder is missing again.
        lib.installed.get_mut("hopper").unwrap().installer_setup.clear();
        let mut m = lib.missing();
        m.sort();
        assert_eq!(m, vec!["hopper".to_string(), "skyracer".to_string()]);
    }
}

#[cfg(test)]
mod mode_tests {
    use super::*;

    fn picked() -> Installed {
        Installed { exe: Some(PathBuf::from("bin/hopper.exe")), exe_rules: u8::MAX, ..Default::default() }
    }

    #[test]
    fn switching_who_sets_it_up_keeps_the_players_program() {
        let mut g = picked();
        g.mode = SetupMode::Own;
        assert!(g.player_sets_up());
        // Easy Setup on: only the mode changes; the pick is still there for coming back.
        g.mode = SetupMode::Easy;
        assert!(!g.player_sets_up());
        assert_eq!(g.exe, Some(PathBuf::from("bin/hopper.exe")));
        assert_eq!(g.exe_rules, u8::MAX);
    }

    #[test]
    fn an_installed_game_is_easy_setup_by_its_own_mode() {
        assert!(!Installed::default().player_sets_up());
    }

    #[test]
    fn the_mode_is_written_only_when_it_is_not_easy_setup() {
        let easy = serde_json::to_value(Installed::default()).unwrap();
        assert!(easy.get("mode").is_none());
        let own = serde_json::to_value(Installed { mode: SetupMode::Own, ..Default::default() }).unwrap();
        assert_eq!(own["mode"], "own");
    }
}

#[cfg(test)]
mod copy_check_tests {
    use super::*;

    #[test]
    fn a_copy_read_before_counts_only_while_it_is_the_same_file_unchanged() {
        let d = std::env::temp_dir().join(format!("rl-copy-check-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let nup = d.join("game.nup");
        std::fs::write(&nup, b"package").unwrap();
        let (size, modified) = CopyCheck::stamp(&nup).unwrap();
        let check = CopyCheck { path: nup.clone(), size, modified, crc32: "e0f3ccfa".into() };
        assert_eq!(check.still(&nup), Some("e0f3ccfa"));
        // Another file, even with the same size and time.
        let other = d.join("other.nup");
        std::fs::write(&other, b"package").unwrap();
        assert_eq!(check.still(&other), None);
        // The same file, changed: read again.
        std::fs::write(&nup, b"a longer package").unwrap();
        assert_eq!(check.still(&nup), None);
        // Gone: nothing to go by.
        std::fs::remove_file(&nup).unwrap();
        assert_eq!(check.still(&nup), None);
        let _ = std::fs::remove_dir_all(&d);
    }
}

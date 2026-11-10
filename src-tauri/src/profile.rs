//! Launcher profiles: what RecompLauncher needs to know to set a recomp up and change its settings
//! without the recomp's own launcher. Where the player's copy of the game goes, which program to
//! start and how, where its settings file is, and which settings to offer.
//!
//! A developer ships one as `recomplauncher.json` in their release (it lands in the game's folder),
//! or the catalog carries it (`"launcher": {...}`), or one is built in here (none right now; docs/examples has two).
//! docs/DEVELOPERS.md describes the format.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::nup;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Profile {
    /// Named disc/DLC inputs and explicit key dependencies. See docs/SETUP-INPUTS.md.
    #[serde(default)]
    pub inputs: Vec<crate::inputs::Input>,
    #[serde(skip)]
    pub input_paths: BTreeMap<String, PathBuf>,
    #[serde(default)]
    pub format: u32,
    /// GitHub repo this built-in profile is for.
    #[serde(default)]
    pub repo: String,
    #[serde(default)]
    pub game: Option<GameCopy>,
    #[serde(default)]
    pub start: Option<Start>,
    #[serde(default)]
    pub config: Vec<Config>,
    #[serde(default)]
    pub folders: BTreeMap<String, Vec<Place>>,
    /// Exit codes and what they mean, said to the player.
    #[serde(default)]
    pub exit_codes: BTreeMap<String, String>,
    #[serde(default)]
    pub settings: Vec<Setting>,
    /// Groups of settings, next to "settings": a whole group can be advanced (hidden until the
    /// player shows advanced options).
    #[serde(default)]
    pub groups: Vec<Group>,
    #[serde(default)]
    pub actions: Vec<Action>,
    /// The recomp's own setup step, run once after the player's copy is picked (and again when
    /// it changes): pulls what the game needs from the disc, builds caches.
    #[serde(default)]
    pub prepare: Option<Prepare>,
    /// Whether looks (ReShade filters) can be used with it, and how.
    #[serde(default)]
    pub looks: Option<crate::looks::ProfileLooks>,
    /// Where the game keeps its saves: folders, files, or `*` patterns, with the same {local},
    /// {documents}, {install}... as everywhere else. Backups and cloud saves copy these.
    #[serde(default)]
    pub saves: Vec<String>,
    /// Made by the launcher from the catalog's boot profile, not by the game's developer: the
    /// release's own setup still runs first, and its settings only change files that are there.
    #[serde(skip)]
    pub from_boot: bool,
    /// The player's slider values for settings on the command line (setting id -> value), from
    /// the Library: filled in by the launcher, never read from the file.
    #[serde(skip)]
    pub slider_vals: BTreeMap<String, String>,
}

/// A program in the release that sets the game up without showing a window. It may print lines
/// like `progress 0.4 Unpacking menu art` while it works, and exits 0 when it's done.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Prepare {
    pub program: Vec<String>,
    #[serde(default)]
    pub args: Vec<String>,
    /// What the setup screen calls this step.
    #[serde(default)]
    pub label: String,
    /// Seconds before it's given up on.
    #[serde(default = "five_min")]
    pub timeout: u64,
}

fn five_min() -> u64 {
    300
}

/// A known good dump of the disc (from Redump), to check the player's copy against.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Dump {
    /// Said when it matches ("USA", "Europe").
    pub name: String,
    #[serde(default)]
    pub size: u64,
    /// CRC32 of the whole image, hex.
    pub crc32: String,
}

/// The player's own copy of the game.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GameCopy {
    /// "iso", "folder" or "iso_or_folder".
    #[serde(default = "iso")]
    pub kind: String,
    /// Said when asking for it ("your disc image (.iso) of ...").
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub check: Check,
    /// Known good dumps of the disc image.
    #[serde(default)]
    pub dumps: Vec<Dump>,
    /// A folder inside the game's folder the recomp reads the copy from (`assets`, `PSP_DATA`,
    /// `.` for the game's folder itself). The launcher puts the copy there before each start: a
    /// folder's contents as links, a disc image as a link (or a copy on another drive).
    #[serde(default)]
    pub into: String,
    /// The name a single-file copy (a ROM) must have there, e.g. `sote.us.v1.2.z64`.
    #[serde(default)]
    pub name: String,
    /// A disc image the launcher unpacks for a recomp that reads extracted files: "xdvdfs" (Xbox,
    /// Xbox 360) or "iso9660" (PlayStation 2, PSP). The unpacked folder is then the copy.
    #[serde(default)]
    pub extract: String,
    /// Made from the catalog's boot profile: its check is a common file (default.xex), so it isn't
    /// used to recognise this game among the player's copies, only to check the one they pick.
    #[serde(skip)]
    pub generic: bool,
    /// The game's own setup takes the copy (drag the ISO onto install.bat): the launcher asks for
    /// it first and hands it to the setup, instead of placing it anywhere.
    #[serde(skip)]
    pub for_setup: bool,
    /// What the folder the game reads must hold once the copy is there (`default.xex`, `Media`).
    #[serde(skip)]
    pub contains: Vec<String>,
    /// A second file it needs beside the copy: the game's update (a title update package, say).
    #[serde(default)]
    pub update: Option<Extra>,
}

fn sha256_file(p: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut f = std::fs::File::open(p)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// A file the game needs along with the player's copy, like its update, and where it goes.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Extra {
    /// A folder inside the game's folder it goes in (`game/Content`).
    #[serde(default)]
    pub into: String,
    /// Said when asking for it.
    #[serde(default)]
    pub label: String,
    /// "file", "folder" or "file or folder".
    #[serde(default)]
    pub accepts: String,
    /// The game plays without it.
    #[serde(default)]
    pub optional: bool,
    /// The name it must have there (it's put in under this name).
    #[serde(default)]
    pub name: String,
    /// The one known file: checked when it's picked.
    #[serde(default)]
    pub sha256: String,
}

impl Extra {
    fn dir(&self, install: &Path) -> Option<PathBuf> {
        let into = self.into.trim().replace('\\', "/");
        let into = into.trim_matches('/');
        if into.is_empty() || into.contains(':') || into.split('/').any(|c| c == "..") {
            return None;
        }
        Some(if into == "." { install.to_path_buf() } else { install.join(into) })
    }

    pub fn wants_folder(&self) -> bool {
        self.accepts.to_ascii_lowercase().contains("folder")
    }

    pub fn wants_file(&self) -> bool {
        let a = self.accepts.to_ascii_lowercase();
        a.is_empty() || a.contains("file")
    }

    /// Whether this is the file asked for. Err says why not.
    pub fn check(&self, p: &Path) -> Result<(), String> {
        if p.is_dir() {
            if !self.wants_folder() {
                return Err("It needs the update's file, not a folder.".into());
            }
            return Ok(());
        }
        if !p.is_file() {
            return Err("That file isn't there anymore.".into());
        }
        if !self.wants_file() {
            return Err("It needs the update's folder, not a file.".into());
        }
        let sha = self.sha256.trim().to_ascii_lowercase();
        if !sha.is_empty() {
            let got = sha256_file(p).map_err(|e| format!("Couldn't read it: {e}"))?;
            if got != sha {
                return Err("That isn't the update this recomp needs. Use the one its page names, unchanged.".into());
            }
            return Ok(());
        }
        let name = self.name.trim();
        if !name.is_empty() && !p.file_name().map(|n| n.to_string_lossy().eq_ignore_ascii_case(name)).unwrap_or(false) {
            return Err(format!("That isn't the update this recomp needs. Its file is named {name}."));
        }
        Ok(())
    }

    /// Already where the game reads it (put there by the player).
    pub fn in_place(&self, install: &Path) -> Option<PathBuf> {
        let dir = self.dir(install)?;
        let name = self.name.trim();
        if !name.is_empty() {
            let p = dir.join(name);
            return p.exists().then_some(p);
        }
        std::fs::read_dir(&dir).ok()?.flatten().next().map(|_| dir)
    }

    /// Puts it where the game reads it: a file as a hard link (or a copy on another drive), a
    /// folder's contents the same way. What's already there stays.
    pub fn place(&self, install: &Path, from: &Path) -> std::io::Result<()> {
        let Some(dir) = self.dir(install) else { return Ok(()) };
        if dir == from {
            return Ok(());
        }
        std::fs::create_dir_all(&dir)?;
        let items: Vec<PathBuf> = if from.is_dir() { std::fs::read_dir(from)?.flatten().map(|e| e.path()).collect() } else { vec![from.to_path_buf()] };
        let single = !from.is_dir();
        for f in items {
            let Some(n) = f.file_name() else { continue };
            let name = self.name.trim();
            let to = if single && !name.is_empty() && !name.contains(['/', '\\']) { dir.join(name) } else { dir.join(n) };
            // An empty folder the release came with (targets/exefs, say) is a place for it, not
            // something already there.
            if empty_dir(&to) {
                let _ = std::fs::remove_dir(&to);
            }
            if to.symlink_metadata().is_ok() {
                continue;
            }
            if f.is_dir() {
                link_dir(&f, &to)?;
            } else if std::fs::hard_link(&f, &to).is_err() {
                copy_checked(&f, &to)?;
            }
        }
        Ok(())
    }
}

fn iso() -> String {
    "iso".into()
}

/// How to tell it's the right game: a file in the folder, or on the disc image.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Check {
    /// In a folder.
    #[serde(default)]
    pub file: String,
    /// On an Xbox 360 disc image.
    #[serde(default)]
    pub xdvdfs: String,
    /// On a PlayStation 2 (or any ISO 9660) disc image, in its top folder.
    #[serde(default)]
    pub iso9660: String,
    /// A Nintendo Switch 2 `.nup` package's build ID (hex, at least 8 digits; a prefix is fine),
    /// read from its newest title's `exefs/main`. A folder with `exefs/main` is read the same way.
    #[serde(default)]
    pub nup_build_id: String,
    /// An N64 ROM's region: the country letters it may have (`E` for USA).
    #[serde(default)]
    pub n64_region: String,
    /// An N64 ROM's game: the two letters of its cartridge ID (`GE` for GoldenEye 007).
    #[serde(default)]
    pub n64_id: String,
    /// An N64 ROM's revision (1 for a v1.1 release), when the recomp reads one revision only.
    #[serde(default)]
    pub n64_rev: Option<u8>,
    /// The disc or cartridge IDs it reads (`GZ2E01`, or `GZ2` for every region): a GameCube or
    /// Wii disc's, an N64 cartridge's game code, a PlayStation disc's serial (crate::roms::copy_id).
    #[serde(default)]
    pub ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Start {
    /// The program to start, first one that exists (relative to the game's folder).
    pub program: Vec<String>,
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables for the game, such as SDL hints. Paths fill in like everywhere else.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// A path, used when `if` (another path) exists, or always without one.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Place {
    pub path: String,
    #[serde(default, rename = "if")]
    pub when: String,
}

/// A settings file the game reads.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// A name settings refer to ("main" when there's one).
    #[serde(default = "main")]
    pub id: String,
    /// "toml", "ini" or "json".
    pub format: String,
    /// Where it is: the first place that applies.
    pub file: Vec<Place>,
    /// The key that holds where the player's copy of the game is.
    #[serde(default)]
    pub game_path: String,
    /// Set when missing (what the game needs to run right).
    #[serde(default)]
    pub ensure: BTreeMap<String, String>,
    /// Set every time.
    #[serde(default)]
    pub always: BTreeMap<String, String>,
    /// Set when the file is new.
    #[serde(default)]
    pub new: BTreeMap<String, String>,
}

fn main() -> String {
    "main".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Setting {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub help: String,
    /// "main" (a settings file's id) or "args" (passed when the game starts).
    #[serde(default = "main", rename = "in")]
    pub target: String,
    /// What it can be set to. A slider has none.
    #[serde(default)]
    pub choices: Vec<Choice>,
    /// The game's own values for keys the file doesn't have yet.
    #[serde(default)]
    pub defaults: BTreeMap<String, String>,
    /// Only when these keys have these values (otherwise shown greyed out).
    #[serde(default)]
    pub when: BTreeMap<String, String>,
    /// The heading it's shown under ("Controls"). Settings keep their order: a heading shows where
    /// the group changes from one setting to the next.
    #[serde(default)]
    pub group: String,
    /// Hidden until the player presses Show Advanced Options (its value stays as it is).
    #[serde(default)]
    pub advanced: bool,
    /// A number on a slider instead of choices.
    #[serde(default)]
    pub slider: Option<Slider>,
    /// For a slider on the command line ("in": "args"): its arguments, with {value} for the number.
    #[serde(default)]
    pub args: Vec<String>,
    /// Key bindings instead of choices: the game's actions and the keys each one is on.
    #[serde(default)]
    pub bindings: Option<Bindings>,
    /// The launcher's own last choice, Match My Screen, was added (with the size or shape it writes).
    #[serde(skip)]
    pub screen: Option<String>,
}

/// Key bindings in a settings file, one key per action (`key.cross=Space`, same key rules as
/// "set"), several keys on one action joined with `separator` (`Escape|Return`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Bindings {
    #[serde(default)]
    pub separator: String,
    /// Keys one action can have (2 when not said).
    #[serde(default)]
    pub max: usize,
    pub actions: Vec<BindAction>,
    /// The launcher's name for a key or button, to the name the game's file uses for it
    /// (`{"Return": "Enter"}`).
    #[serde(default)]
    pub names: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BindAction {
    pub label: String,
    pub key: String,
    /// It takes a controller button too.
    #[serde(default)]
    pub pad: bool,
}

/// Names a key or button goes by, the launcher's own first: a file that already uses one of the
/// others ("Enter", "Esc") gets that one.
const KEY_NAMES: &[&[&str]] = &[
    &["Return", "Enter"],
    &["Escape", "Esc"],
    &["Ctrl", "Control", "Ctl"],
    &["Space", "Spacebar", "Space Bar"],
    &["Backspace", "Back Space"],
    &["Delete", "Del"],
    &["Insert", "Ins"],
    &["Page Up", "PageUp", "PgUp"],
    &["Page Down", "PageDown", "PgDn"],
    &["Up", "Arrow Up", "ArrowUp", "Up Arrow", "UpArrow"],
    &["Down", "Arrow Down", "ArrowDown", "Down Arrow", "DownArrow"],
    &["Left", "Arrow Left", "ArrowLeft", "Left Arrow", "LeftArrow"],
    &["Right", "Arrow Right", "ArrowRight", "Right Arrow", "RightArrow"],
    &["Mouse Left", "Mouse1", "Mouse 1", "LMB", "Left Mouse", "MouseLeft", "Left Click"],
    &["Mouse Right", "Mouse2", "Mouse 2", "RMB", "Right Mouse", "MouseRight", "Right Click"],
    &["Mouse Middle", "Mouse3", "Mouse 3", "MMB", "Middle Mouse", "MouseMiddle", "Middle Click"],
    &["Mouse 4", "Mouse4", "Mouse Back", "XButton1", "Mouse X1"],
    &["Mouse 5", "Mouse5", "Mouse Forward", "XButton2", "Mouse X2"],
    &["Wheel Up", "Mouse Wheel Up", "MWheelUp", "WheelUp", "Scroll Up"],
    &["Wheel Down", "Mouse Wheel Down", "MWheelDown", "WheelDown", "Scroll Down"],
    &["A", "Cross"],
    &["B", "Circle"],
    &["X", "Square"],
    &["Y", "Triangle"],
    &["LB", "L1"],
    &["RB", "R1"],
    &["LT", "L2"],
    &["RT", "R2"],
    &["LS", "L3"],
    &["RS", "R3"],
    &["Back", "Select", "View", "Share", "Create"],
    &["Start", "Options", "Menu"],
    &["D-Pad Up", "DPad Up", "DPadUp"],
    &["D-Pad Down", "DPad Down", "DPadDown"],
    &["D-Pad Left", "DPad Left", "DPadLeft"],
    &["D-Pad Right", "DPad Right", "DPadRight"],
];

impl Bindings {
    pub fn max(&self) -> usize {
        if self.max == 0 {
            2
        } else {
            self.max.min(8)
        }
    }

    pub fn sep(&self) -> &str {
        if self.separator.is_empty() {
            "|"
        } else {
            &self.separator
        }
    }

    /// The keys in a value as written (`"Escape|Return"`, quotes or not), in their order.
    pub fn split(&self, v: &str) -> Vec<String> {
        let v = v.trim();
        let v = if v.len() >= 2 && (v.starts_with('"') && v.ends_with('"') || v.starts_with('\'') && v.ends_with('\'')) { &v[1..v.len() - 1] } else { v };
        v.split(self.sep()).map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect()
    }

    /// The name a key is written under: the profile's own name for it, else the one this file
    /// already uses for that key, else the launcher's.
    pub fn file_name(&self, name: &str, present: &[String]) -> String {
        let group: Vec<&str> = KEY_NAMES.iter().find(|g| g.iter().any(|n| n.eq_ignore_ascii_case(name))).map(|g| g.to_vec()).unwrap_or_else(|| vec![name]);
        let mut tries = vec![name];
        tries.extend(group.iter().copied().filter(|n| !n.eq_ignore_ascii_case(name)));
        for n in &tries {
            if let Some(v) = self.names.iter().find(|(k, _)| k.eq_ignore_ascii_case(n)).map(|(_, v)| v.trim()).filter(|v| !v.is_empty()) {
                return v.to_string();
            }
        }
        for n in &tries {
            if let Some(p) = present.iter().find(|p| p.eq_ignore_ascii_case(n)) {
                return p.clone();
            }
        }
        name.to_string()
    }
}

/// A slider: the key it sets (Section/Key in a settings file, like "set"), its range and step, and
/// how the number is written (printf style: "%.2f", "%d"; "%g" when not said).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Slider {
    #[serde(default)]
    pub key: String,
    pub min: f64,
    pub max: f64,
    #[serde(default)]
    pub step: f64,
    #[serde(default)]
    pub format: String,
    /// The same as the setting's "args", for a slider on the command line.
    #[serde(default)]
    pub args: Vec<String>,
}

/// A group of settings, by the name they use in "group". `advanced`: the whole group is hidden
/// until the player shows advanced options.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Group {
    pub id: String,
    #[serde(default)]
    pub advanced: bool,
}

impl Slider {
    /// The step it moves by (a hundredth of the range when it doesn't say).
    pub fn step(&self) -> f64 {
        if self.step > 0.0 && self.step.is_finite() {
            self.step
        } else if self.max > self.min {
            (self.max - self.min) / 100.0
        } else {
            1.0
        }
    }

    /// Digits after the point in its step (and its ends), so 0.1 + 0.2 stays 0.3.
    fn places(&self) -> usize {
        let p = |x: f64| {
            let s = format!("{x}");
            s.split_once('.').map(|(_, d)| d.len()).unwrap_or(0).min(10)
        };
        p(self.step()).max(p(self.min)).max(p(self.max))
    }

    /// A number the player moved it to: on a step, inside its ends.
    pub fn snap(&self, v: f64) -> f64 {
        let (lo, hi) = if self.min <= self.max { (self.min, self.max) } else { (self.max, self.min) };
        let st = self.step();
        let n = ((v.clamp(lo, hi) - lo) / st).round();
        let x = (lo + n * st).clamp(lo, hi);
        let m = 10f64.powi(self.places() as i32);
        (x * m).round() / m
    }

    /// The number written its way (printf style).
    pub fn text(&self, v: f64) -> String {
        printf(if self.format.trim().is_empty() { "%g" } else { self.format.trim() }, v)
    }
}

/// A number in a printf-style format: %d, %i, %f, %.2f, %g, %.3g, %e, with text around it ("%.1fx").
pub fn printf(fmt: &str, v: f64) -> String {
    let re = regex_lite::Regex::new(r"%([+]?)(?:\.(\d+))?([dfgieFGE])").unwrap();
    let Some(c) = re.captures(fmt) else { return gfmt(v, 6) };
    let plus = !c[1].is_empty();
    let prec = c.get(2).and_then(|m| m.as_str().parse::<usize>().ok());
    let body = match &c[3] {
        "d" | "i" => format!("{}", v.round() as i64),
        "f" | "F" => format!("{:.*}", prec.unwrap_or(6).min(12), v),
        "e" | "E" => format!("{:.*e}", prec.unwrap_or(6).min(12), v),
        _ => gfmt(v, prec.unwrap_or(6).clamp(1, 17)),
    };
    let body = if plus && v >= 0.0 { format!("+{body}") } else { body };
    let m = c.get(0).unwrap();
    format!("{}{}{}", &fmt[..m.start()], body, &fmt[m.end()..]).replace("%%", "%")
}

/// C's %g: `sig` significant digits, trailing zeros left off.
fn gfmt(v: f64, sig: usize) -> String {
    if v == 0.0 || !v.is_finite() {
        return if v.is_finite() { "0".into() } else { format!("{v}") };
    }
    let exp = v.abs().log10().floor() as i32;
    if exp < -4 || exp >= sig as i32 {
        let s = format!("{:.*e}", sig.saturating_sub(1), v);
        // 1.500000e3 -> 1.5e+03, as C writes it.
        let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
        let m = if m.contains('.') { m.trim_end_matches('0').trim_end_matches('.') } else { m };
        let e: i32 = e.parse().unwrap_or(0);
        return format!("{m}e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs());
    }
    let decimals = (sig as i32 - 1 - exp).max(0) as usize;
    let s = format!("{:.*}", decimals, v);
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Choice {
    pub label: String,
    #[serde(default)]
    pub set: BTreeMap<String, String>,
    #[serde(default)]
    pub args: Vec<String>,
}

/// A button: open a folder, or clear one.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Action {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub help: String,
    #[serde(default)]
    pub open: String,
    #[serde(default)]
    pub clear: String,
}

// ---------- finding a profile ----------

/// Launcher-maintained profiles for official releases that ship without one.
const BUILT_IN: &[&str] = &[
    include_str!("../boot/dusklight.json"),
    #[cfg(test)]
    include_str!("../../docs/examples/fifawc14.json"),
    #[cfg(test)]
    include_str!("../../docs/examples/streetball.json"),
];

/// The profile for a game: the AppImage's embedded file, the folder's own, the catalog's, a built-in
/// one for its repo, or one made from the catalog's boot profile when that
/// says where the player's copy goes.
pub fn find(game_dir: &Path, repo: &str, entry: &Value) -> Option<Profile> {
    find_own(game_dir, repo, entry).or_else(|| crate::boot::raw(entry).and_then(|b| from_boot(&b)))
}

/// From the catalog's boot profile (crate::boot): the player's copy and how it reaches the game,
/// when it goes on the command line (`files.how: "flag"`) or into a folder (`"folder"`), and the
/// game's settings the launcher can show (`settings: {config, settings}`).
/// A native build's program for the Windows one a game's setup names: the same name without its
/// Windows and processor parts. pd.i686.exe is pd.x86_64, not pd.pal.x86_64 (a build for another
/// region, with a word the Windows name doesn't have).
pub fn native_twin(dir: &Path, win: &str) -> Option<PathBuf> {
    const SKIP: [&str; 15] = ["exe", "i686", "i386", "x86", "x86_64", "x64", "amd64", "win32", "win64", "windows", "linux", "bin", "appimage", "sh", "64"];
    let words = |n: &str| -> Vec<String> { n.to_lowercase().split(['.', '_', '-', ' ']).filter(|w| !w.is_empty() && !SKIP.contains(w)).map(String::from).collect() };
    let want = words(Path::new(win).file_name()?.to_str()?);
    if want.is_empty() {
        return None;
    }
    let runnable = |p: &Path| -> bool {
        let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
        if [".dll", ".so", ".exe", ".txt", ".md", ".ini", ".cfg", ".json", ".pdb", ".zip", ".7z", ".dat", ".bin"].iter().any(|e| n.ends_with(e)) || n.contains(".so.") {
            return false;
        }
        if n.ends_with(".x86_64") || n.ends_with(".appimage") || n.ends_with(".sh") {
            return true;
        }
        // A program, by its run permission or its first bytes (an ELF): zips made on Windows drop
        // the permission.
        let elf = || {
            let mut head = [0u8; 4];
            std::fs::File::open(p).and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head)).is_ok() && &head == b"\x7fELF"
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(p).map(|m| m.permissions().mode() & 0o111 != 0).unwrap_or(false) || elf()
        }
        #[cfg(not(unix))]
        elf()
    };
    let mut best: Option<(usize, usize, PathBuf)> = None;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if !p.is_file() || !runnable(&p) {
            continue;
        }
        let name = p.file_name()?.to_string_lossy().to_string();
        let have = words(&name);
        if !want.iter().all(|w| have.contains(w)) {
            continue;
        }
        let extra = have.iter().filter(|w| !want.contains(w)).count();
        if best.as_ref().map(|(x, l, _)| (extra, name.len()) < (*x, *l)).unwrap_or(true) {
            best = Some((extra, name.len(), p));
        }
    }
    best.map(|(_, _, p)| p)
}

pub fn from_boot(b: &Value) -> Option<Profile> {
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
    let (config, settings) = boot_settings(b);
    let copy = boot_copy(b);
    if copy.is_none() && settings.is_empty() {
        return None;
    }
    // `files.how: "env"`: the copy reaches the game through an environment variable it reads
    // (`files.env`, e.g. STRIKERS_DATA), a disc image or a folder alike.
    let mut env = BTreeMap::new();
    if let Some(f) = b.get("files").filter(|f| s(f, "how") == "env") {
        let name = s(f, "env");
        if env_name_ok(&name) {
            env.insert(name, "{game}".to_string());
        }
    }
    Some(Profile {
        format: 1,
        start: copy.as_ref().map(|(_, args)| Start { program: crate::boot::starts(b), args: args.clone(), env: env.clone() }),
        game: copy.map(|(g, _)| g),
        config,
        settings,
        groups: b.get("settings").and_then(|v| v.get("groups")).and_then(|g| serde_json::from_value::<Vec<Group>>(g.clone()).ok()).unwrap_or_default(),
        from_boot: true,
        ..Default::default()
    })
}

/// The settings a boot profile describes, when they're sound: each one in a file it lists (or on
/// the command line), with at least two choices.
fn boot_settings(b: &Value) -> (Vec<Config>, Vec<Setting>) {
    let Some(v) = b.get("settings").filter(|v| v.is_object()) else { return (vec![], vec![]) };
    let config: Vec<Config> = v.get("config").and_then(|c| serde_json::from_value::<Vec<Config>>(c.clone()).ok()).unwrap_or_default();
    let config: Vec<Config> = config
        .into_iter()
        .filter(|c| matches!(c.format.as_str(), "toml" | "ini" | "json") && !c.file.is_empty() && c.file.iter().all(|p| safe_place(&p.path) && (p.when.is_empty() || safe_place(&p.when))))
        // Only what the player picks is written: nothing on every start.
        .map(|c| Config { game_path: String::new(), ensure: BTreeMap::new(), always: BTreeMap::new(), new: BTreeMap::new(), ..c })
        .collect();
    let settings: Vec<Setting> = v.get("settings").and_then(|c| serde_json::from_value::<Vec<Setting>>(c.clone()).ok()).unwrap_or_default();
    let settings: Vec<Setting> = settings
        .into_iter()
        .filter(|st| {
            (st.choices.len() >= 2
                || st.slider.as_ref().map(|sl| sl.min < sl.max && (st.target != "args" || st.args.iter().chain(sl.args.iter()).any(|a| a.contains("{value}")))).unwrap_or(false)
                || st.bindings.as_ref().map(|b| st.target != "args" && !b.actions.is_empty() && b.actions.iter().all(|a| !a.key.trim().is_empty())).unwrap_or(false))
                && (st.target == "args" || config.iter().any(|c| c.id == st.target))
        })
        .collect();
    let used: Vec<Config> = config.into_iter().filter(|c| settings.iter().any(|st| st.target == c.id)).collect();
    (used, settings)
}

/// A settings file's place from the catalog: in the game's folder or the player's own folders, never
/// out of them.
fn safe_place(p: &str) -> bool {
    let p = p.trim().replace('\\', "/");
    ["{install}/", "{documents}/", "{local}/", "{config}/", "{home}/"].iter().any(|r| p.starts_with(r)) && !p.split('/').any(|c| c == "..")
}

/// The player's copy, when the boot profile says it goes on the command line or into a folder:
/// what it is, and the arguments the game starts with.
fn boot_copy(b: &Value) -> Option<(GameCopy, Vec<String>)> {
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
    let f = b.get("files")?;
    let how = s(f, "how");
    let start = s(b, "start");
    if start.is_empty() || !matches!(how.as_str(), "flag" | "folder" | "setup" | "env") {
        return None;
    }
    let flag = s(f, "flag");
    let into = s(f, "folder");
    // A flag, or none: the copy as the first thing on the command line.
    if (how == "flag" && !flag.is_empty() && !flag.starts_with('-')) || (how == "folder" && (into.is_empty() || into.contains(".."))) {
        return None;
    }
    let accepts = s(f, "accepts").to_ascii_lowercase();
    let contains_early: Vec<String> = f.get("contains").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_ascii_lowercase).collect()).unwrap_or_default();
    let extract = match s(f, "extract").as_str() {
        x @ ("xdvdfs" | "iso9660") => x.to_string(),
        // A recomp that reads the disc's files from a folder and takes the disc image too, without
        // saying how to unpack it: an Xbox or Xbox 360 disc (it has default.xbe or default.xex) is
        // unpacked like one, so the folder ends up with the files the game reads, not the image.
        _ if how == "folder" && accepts.contains("iso") && accepts.contains("folder") && contains_early.iter().any(|c| c == "default.xbe" || c == "default.xex") => "xdvdfs".to_string(),
        _ if how == "folder" && accepts.contains("iso") && accepts.contains("folder") && contains_early.iter().any(|c| c == "system.cnf") => "iso9660".to_string(),
        _ => String::new(),
    };
    let kind = if !extract.is_empty() || (accepts.contains("iso") && accepts.contains("folder")) {
        "iso_or_folder"
    } else if accepts.contains("iso") || accepts == "file" {
        "iso"
    } else {
        "folder"
    };
    let contains: Vec<String> = f.get("contains").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
    // A file it must have, to check the folder picked (a folder name like `code` can't be checked
    // as a file, so only names with a dot).
    let check_file = contains.iter().find(|c| c.contains('.')).cloned().unwrap_or_default();
    // A ROM the game reads under one name only.
    let name = if how == "folder" && kind == "iso" && contains.len() == 1 && contains[0].contains('.') && !contains[0].contains(['/', '\\', '*']) { contains[0].clone() } else { String::new() };
    let what = s(f, "what");
    // A disc image the game reads as it is: checked to be one it can read when it's picked.
    let image_file = if s(f, "check_image") == "xdvdfs" && kind == "iso" { "default.xex".to_string() } else { String::new() };
    // The game's update, when it needs one beside the copy.
    let update = f.get("update").filter(|u| u.is_object()).map(|u| Extra {
        into: s(u, "folder"),
        label: { let w = s(u, "what"); if w.is_empty() || w.ends_with('.') { w } else { format!("{w}.") } },
        accepts: s(u, "accepts"),
        optional: u.get("optional").and_then(Value::as_bool).unwrap_or(false),
        name: s(u, "name"),
        sha256: s(u, "sha256"),
    }).filter(|x| x.dir(Path::new("/x")).is_some() && !x.label.is_empty() && !x.name.contains(['/', '\\']));
    let mut args: Vec<String> = crate::split_args(&s(b, "args"));
    if how == "flag" {
        if !flag.is_empty() {
            args.push(flag);
        }
        args.push("{game}".into());
    }
    Some((
        GameCopy {
            kind: kind.into(),
            label: if what.is_empty() || what.ends_with('.') { what.clone() } else { format!("{what}.") },
            check: Check {
                // On the disc image too, when the launcher unpacks it: the same file at its top.
                xdvdfs: if extract == "xdvdfs" { check_file.clone() } else { image_file },
                iso9660: if extract == "iso9660" { contains.first().cloned().unwrap_or_default() } else { String::new() },
                file: if kind == "iso" { String::new() } else { check_file },
                n64_region: s(f, "region").to_ascii_uppercase(),
                n64_id: s(f, "n64_id").to_ascii_uppercase(),
                n64_rev: f.get("n64_rev").and_then(Value::as_u64).map(|v| v as u8),
                ids: f.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect()).unwrap_or_default(),
                nup_build_id: String::new(),
            },
            into: if how == "folder" { into } else { String::new() },
            name,
            extract,
            generic: true,
            for_setup: how == "setup",
            contains: if how == "folder" { contains.clone() } else { vec![] },
            update,
            ..Default::default()
        },
        args,
    ))
}

/// A release-owned profile and its source, shared with Developer tools.
pub(crate) struct ReleaseProfile {
    pub profile: Profile,
    pub raw: Value,
    pub location: String,
    pub embedded: bool,
}

/// Rewrite AppImage program entries to the actual container's name. All paths
/// still resolve from the install directory, never from a temporary mount.
fn bind_image(raw: &mut Value, image: &Path) -> Option<()> {
    let name = image.file_name()?.to_str()?;
    for field in ["start", "prepare"] {
        if let Some(programs) = raw.get_mut(field).and_then(|v| v.get_mut("program")).and_then(Value::as_array_mut) {
            for program in programs {
                if program.as_str().and_then(|s| Path::new(s).extension()).and_then(|s| s.to_str())
                    .map(|s| s.eq_ignore_ascii_case("appimage")) == Some(true)
                {
                    *program = Value::String(name.to_string());
                }
            }
        }
    }
    Some(())
}

/// Embedded metadata belongs to the installed image and wins over a stale
/// sidecar. Unsupported, missing or unreadable metadata keeps the old fallback.
pub(crate) fn release_profile(dir: &Path) -> Option<ReleaseProfile> {
    if dir.as_os_str().is_empty() {
        return None;
    }
    let parse = |bytes: &[u8], image: Option<&Path>| -> Option<ReleaseProfile> {
        let mut raw: Value = serde_json::from_slice(bytes).ok()?;
        if let Some(image) = image {
            bind_image(&mut raw, image)?;
        }
        let profile: Profile = serde_json::from_value(raw.clone()).ok()?;
        if profile.format != 1 {
            return None;
        }
        Some(ReleaseProfile {
            profile,
            raw,
            embedded: image.is_some(),
            location: image.map(|p| format!("{}!/recomplauncher.json", p.display()))
                .unwrap_or_else(|| dir.join(crate::appimage::MANIFEST).display().to_string()),
        })
    };
    if let Some(image) = crate::appimage::installed_image(dir) {
        if let Some(owned) = crate::appimage::manifest(&image).ok().and_then(|b| parse(&b, Some(&image))) {
            return Some(owned);
        }
    }
    let bytes = std::fs::read(dir.join(crate::appimage::MANIFEST)).ok()?;
    parse(&bytes, None)
}

fn find_own(game_dir: &Path, repo: &str, entry: &Value) -> Option<Profile> {
    let ok = |p: Profile| (p.format == 1).then_some(p);
    if let Some(owned) = release_profile(game_dir) {
        return Some(owned.profile);
    }
    if let Some(p) = entry.get("launcher").and_then(|v| serde_json::from_value::<Profile>(v.clone()).ok()).and_then(ok) {
        return Some(p);
    }
    BUILT_IN
        .iter()
        .filter_map(|t| serde_json::from_str::<Profile>(t).ok())
        .find(|p| !repo.is_empty() && p.repo.eq_ignore_ascii_case(repo))
}

// ---------- paths ----------

static PAD: std::sync::RwLock<&'static str> = std::sync::RwLock::new("xbox");

/// The kind of controller in use, for `{pad}`: "xbox" or "playstation".
pub fn set_pad(kind: &'static str) {
    *PAD.write().unwrap() = kind;
}

/// The controller kind `{pad}` stands for right now.
pub fn pad_now() -> &'static str {
    *PAD.read().unwrap()
}

/// A name a game's environment may take: letters, digits and _, not starting with a digit, and
/// never one of the launcher's own or the ones Windows and Linux need to find programs.
pub fn env_name_ok(k: &str) -> bool {
    let mut c = k.chars();
    matches!(c.next(), Some(x) if x.is_ascii_alphabetic() || x == '_')
        && c.all(|x| x.is_ascii_alphanumeric() || x == '_')
        && k.len() <= 64
        && !k.to_ascii_uppercase().starts_with("RECOMPLAUNCHER")
        && !matches!(k.to_ascii_uppercase().as_str(), "PATH" | "PATHEXT" | "SYSTEMROOT" | "WINDIR" | "COMSPEC" | "LD_PRELOAD" | "LD_LIBRARY_PATH" | "HOME" | "USERPROFILE" | "APPDATA" | "LOCALAPPDATA" | "TEMP" | "TMP")
}

thread_local! {
    /// Filling in for a Windows program that runs through Proton or Wine on Linux: the player's copy
    /// is given the way Windows programs name it there (Z:\run\media\...).
    static WINE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A Linux path as a Windows program running under Proton or Wine reads it.
pub fn wine_path(p: &str) -> String {
    if p.starts_with('/') {
        format!("Z:{}", p.replace('/', "\\"))
    } else {
        p.to_string()
    }
}

/// While it's alive, filling in is for a Windows program under Proton (when `on`).
struct WineFill(bool);
impl WineFill {
    fn new(on: bool) -> Self {
        WineFill(WINE.with(|w| w.replace(on)))
    }
}
impl Drop for WineFill {
    fn drop(&mut self) {
        WINE.with(|w| w.set(self.0));
    }
}

/// Fills in {install}, {documents}, {local}, {config}, {home}, {game} and {pad}.
/// `{appdata}`: where this system keeps programs' own data, the folder SDL-based games save in
/// (AppData\\Roaming on Windows, Application Support on a Mac, ~/.local/share on Linux). One word
/// for all three, so a save copied on one system lands in the right place on another.
pub fn app_data() -> Option<PathBuf> {
    if cfg!(windows) {
        dirs::config_dir()
    } else if cfg!(target_os = "macos") {
        dirs::data_dir()
    } else {
        dirs::data_local_dir()
    }
}

pub fn fill(t: &str, install: &Path, game: Option<&Path>) -> String {
    if !t.contains('{') {
        return t.to_string();
    }
    let s = |p: Option<PathBuf>| p.map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    t.replace("{pad}", &PAD.read().unwrap())
        .replace("{install}", &install.to_string_lossy())
        .replace("{documents}", &s(dirs::document_dir().or_else(|| dirs::home_dir().map(|h| h.join("Documents")))))
        .replace("{appdata}", &s(app_data()))
        .replace("{local}", &s(dirs::data_local_dir()))
        .replace("{config}", &s(dirs::config_dir()))
        .replace("{home}", &s(dirs::home_dir()))
        .replace("{game}", &game.map(|g| { let s = g.to_string_lossy().to_string(); if WINE.with(|w| w.get()) { wine_path(&s) } else { s } }).unwrap_or_default())
}

/// The first place that applies.
/// A settings file that isn't where the profile says, found beside it: the same name in other
/// letter case (Linux tells them apart), or the one `*.cfg.json` a new version of the game writes
/// under another name.
fn near_config(path: &Path) -> Option<PathBuf> {
    let dir = path.parent()?;
    let want = path.file_name()?.to_string_lossy().to_lowercase();
    let entries: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
    if let Some(p) = entries.iter().find(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase() == want).unwrap_or(false)) {
        return Some(p.clone());
    }
    if want.ends_with(".cfg.json") {
        let cfgs: Vec<&PathBuf> = entries.iter().filter(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase().ends_with(".cfg.json")).unwrap_or(false)).collect();
        if cfgs.len() == 1 {
            return Some(cfgs[0].clone());
        }
    }
    None
}

/// A real folder (not a link) with nothing in it.
fn empty_dir(p: &Path) -> bool {
    p.symlink_metadata().map(|m| m.is_dir() && !m.file_type().is_symlink()).unwrap_or(false) && std::fs::read_dir(p).map(|mut r| r.next().is_none()).unwrap_or(false)
}

/// A copy of the player's file into the game's folder (another drive, so no link), after checking
/// it fits: a big copy (a whole disc) that fills the drive halfway helps nobody.
fn copy_checked(from: &Path, to: &Path) -> std::io::Result<()> {
    let size = std::fs::metadata(from).map(|m| m.len()).unwrap_or(0);
    if let Some(free) = crate::manage::free_space(to.parent().unwrap_or(to)) {
        if free < size.saturating_add(64 << 20) {
            let gb = |b: u64| format!("{:.1} GB", b as f64 / 1_073_741_824.0);
            return Err(std::io::Error::other(format!(
                "Not enough space to copy {} ({}) into the game's folder: {} is free on that drive. Free some space, or keep your game files on the same drive as your library so they don't need copying.",
                from.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                gb(size),
                gb(free)
            )));
        }
    }
    std::fs::copy(from, to).map(|_| ())
}

pub fn place(list: &[Place], install: &Path) -> Option<PathBuf> {
    list.iter().find(|p| p.when.is_empty() || Path::new(&fill(&p.when, install, None)).exists()).map(|p| PathBuf::from(fill(&p.path, install, None)))
}

// ---------- settings files ----------

/// A settings file, edited line by line so its comments and order stay as they are.
pub struct File {
    path: PathBuf,
    format: String,
    lines: Vec<String>,
    json: serde_json::Map<String, Value>,
    pub existed: bool,
}

fn key_line(format: &str, key: &str) -> regex_lite::Regex {
    let k = regex_lite::escape(key);
    if format == "ini" {
        regex_lite::Regex::new(&format!(r"^(\s*{k}\s*=\s*)([^;#]*?)(\s*(?:[;#].*)?)$")).unwrap()
    } else {
        regex_lite::Regex::new(&format!(r#"^(\s*{k}\s*=\s*)((?:"[^"]*"|[^#\s])*)(.*)$"#)).unwrap()
    }
}

impl File {
    pub fn open(path: PathBuf, format: &str) -> File {
        let text = std::fs::read_to_string(&path).ok();
        let existed = text.is_some();
        let text = text.unwrap_or_default();
        let json = if format == "json" { serde_json::from_str::<Value>(&text).ok().and_then(|v| v.as_object().cloned()).unwrap_or_default() } else { Default::default() };
        File { path, format: format.into(), lines: text.lines().map(|l| l.trim_end_matches('\r').to_string()).collect(), json, existed }
    }

    /// A key and the section it's in: `Graphics/Fullscreen` is `Fullscreen` under `[Graphics]` (INI
    /// sections, TOML tables). A plain key is looked for anywhere, as before.
    fn split<'a>(&self, key: &'a str) -> (Option<&'a str>, &'a str) {
        match key.split_once('/') {
            Some((sec, k)) if self.format != "json" && !sec.trim().is_empty() && !k.trim().is_empty() => (Some(sec.trim()), k.trim()),
            _ => (None, key),
        }
    }

    fn is_header(l: &str) -> Option<&str> {
        let t = l.trim();
        (t.starts_with('[') && !t.starts_with("[[") && t.contains(']')).then(|| t[1..t.find(']').unwrap()].trim())
    }

    /// The lines a key may be on: a section's (without its header), or the whole file.
    fn range(&self, sec: Option<&str>) -> Option<(usize, usize)> {
        let Some(sec) = sec else { return Some((0, self.lines.len())) };
        let ini = self.format == "ini";
        let start = self.lines.iter().position(|l| Self::is_header(l).map(|h| if ini { h.eq_ignore_ascii_case(sec) } else { h == sec }).unwrap_or(false))? + 1;
        let end = self.lines[start..].iter().position(|l| l.trim().starts_with('[')).map(|i| start + i).unwrap_or(self.lines.len());
        Some((start, end))
    }

    /// A setting's value as written (strings with their quotes).
    pub fn get(&self, key: &str) -> Option<String> {
        if self.format == "json" {
            // `CVars/gInternalResolution`: inside nested objects.
            let mut parts = key.split('/');
            let mut v = self.json.get(parts.next()?)?;
            for p in parts {
                v = v.get(p)?;
            }
            return Some(v.to_string());
        }
        let (sec, k) = self.split(key);
        let (a, b) = self.range(sec)?;
        let re = key_line(&self.format, k);
        self.lines[a..b].iter().find_map(|l| re.captures(l).map(|c| c[2].to_string()))
    }

    pub fn set(&mut self, key: &str, value: &str) {
        if self.format == "json" {
            let v = serde_json::from_str::<Value>(value).unwrap_or(Value::String(value.into()));
            let parts: Vec<&str> = key.split('/').collect();
            let (last, path) = parts.split_last().unwrap();
            let mut obj = &mut self.json;
            for p in path {
                let e = obj.entry(p.to_string()).or_insert_with(|| Value::Object(Default::default()));
                if !e.is_object() {
                    *e = Value::Object(Default::default());
                }
                obj = e.as_object_mut().unwrap();
            }
            obj.insert(last.to_string(), v);
            return;
        }
        let (sec, k) = self.split(key);
        let re = key_line(&self.format, k);
        if let Some((a, b)) = self.range(sec) {
            for l in self.lines[a..b].iter_mut() {
                if let Some(c) = re.captures(l) {
                    *l = format!("{}{}{}", &c[1], value, &c[3]);
                    return;
                }
            }
            // Written out but commented (`;VH_SCALE=2`, `# vsync = true`): that line, switched on, in its own style.
            let off = regex_lite::Regex::new(&format!(r"^\s*[;#]+\s*({}\s*=\s*)", regex_lite::escape(k))).unwrap();
            if let Some(i) = (a..b).find(|i| off.is_match(&self.lines[*i])) {
                let lead = off.captures(&self.lines[i]).map(|c| c[1].to_string()).unwrap_or_else(|| format!("{k} = "));
                self.lines[i] = format!("{lead}{value}");
                return;
            }
            // Not there yet: at the end of its section, or for a TOML top-level key, before the
            // first table (after it, the key would belong to that table).
            let at = if sec.is_none() && self.format == "toml" {
                self.lines.iter().position(|l| l.trim().starts_with('[')).unwrap_or(self.lines.len())
            } else {
                b
            };
            let at = (a..at).rev().find(|i| !self.lines[*i].trim().is_empty()).map(|i| i + 1).unwrap_or(a);
            let eq = self.eq_style();
            self.lines.insert(at, format!("{k}{eq}{value}"));
            return;
        }
        if self.lines.last().map(|l| !l.trim().is_empty()).unwrap_or(false) {
            self.lines.push(String::new());
        }
        let eq = self.eq_style();
        self.lines.push(format!("[{}]", sec.unwrap_or_default()));
        self.lines.push(format!("{k}{eq}{value}"));
    }

    /// How the file writes `=`: `key=value` or `key = value`, so a new line reads like the rest
    /// (some games don't trim the spaces).
    fn eq_style(&self) -> &'static str {
        let re = regex_lite::Regex::new(r"^\s*[A-Za-z0-9_.\-]+(\s*)=(\s*)\S").unwrap();
        match self.lines.iter().find_map(|l| re.captures(l).map(|c| (c[1].is_empty(), c[2].is_empty()))) {
            Some((true, true)) => "=",
            _ => " = ",
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        if let Some(d) = self.path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let text = if self.format == "json" {
            serde_json::to_string_pretty(&self.json).unwrap_or_default() + "\n"
        } else {
            self.lines.iter().map(|l| format!("{l}\n")).collect()
        };
        let tmp = self.path.with_extension("rltmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &self.path)
    }
}

/// Two values as the same setting: quotes aside, numbers by value, words in any case.
fn same(a: &str, b: &str) -> bool {
    let un = |x: &str| x.trim().trim_matches(|c| c == '"' || c == '\'').trim().to_string();
    let (a, b) = (un(a), un(b));
    match (a.parse::<f64>(), b.parse::<f64>()) {
        (Ok(x), Ok(y)) => (x - y).abs() < 1e-6,
        _ => a.eq_ignore_ascii_case(&b),
    }
}

/// A path as a value in the file's own syntax.
pub fn path_value(format: &str, p: &Path) -> String {
    match format {
        "json" => Value::String(p.to_string_lossy().to_string()).to_string(),
        "ini" => p.to_string_lossy().to_string(),
        // TOML basic strings treat \ as an escape: forward slashes, as the games' own launchers write.
        _ => format!("\"{}\"", p.to_string_lossy().replace('\\', "/").replace('"', "")),
    }
}

impl Profile {
    pub fn config_file(&self, id: &str, install: &Path) -> Option<File> {
        let c = self.config.iter().find(|c| c.id == id)?;
        let path = place(&c.file, install)?;
        let path = if path.exists() { path } else { near_config(&path).unwrap_or(path) };
        Some(File::open(path, &c.format))
    }

    /// Before the game starts: its settings files get what it needs and where the player's copy is.
    /// Its program is a Windows one that plays through Proton here (on Linux).
    pub fn through_proton(&self, install: &Path) -> bool {
        !cfg!(windows)
            && self.start.as_ref().and_then(|s| s.program.iter().map(|p| install.join(p)).find(|p| p.is_file())).map(|e| e.extension().map(|x| x.eq_ignore_ascii_case("exe")).unwrap_or(false)).unwrap_or(false)
    }

    pub fn prepare(&self, install: &Path, game: Option<&Path>) -> std::io::Result<()> {
        let _w = WineFill::new(self.through_proton(install));
        for c in &self.config {
            // A path put into a TOML or JSON string: forward slashes, so the backslashes aren't escapes.
            let fill = |v: &str, install: &Path, game: Option<&Path>| {
                let f = fill(v, install, game);
                if c.format != "ini" && ["{install}", "{game}", "{documents}", "{local}", "{config}", "{home}"].iter().any(|k| v.contains(k)) { f.replace('\\', "/") } else { f }
            };
            let Some(path) = place(&c.file, install) else { continue };
            let mut f = File::open(path, &c.format);
            for (k, v) in &c.ensure {
                if f.get(k).map(|x| x.is_empty()).unwrap_or(true) {
                    f.set(k, &fill(v, install, game));
                }
            }
            for (k, v) in &c.always {
                f.set(k, &fill(v, install, game));
            }
            if !f.existed {
                for (k, v) in &c.new {
                    f.set(k, &fill(v, install, game));
                }
            }
            if let (false, Some(g)) = (c.game_path.is_empty(), game) {
                f.set(&c.game_path, &path_value(&c.format, g));
            }
            f.save()?;
        }
        Ok(())
    }

    /// The program and its arguments, with the settings that go on the command line.
    /// `start.env`, filled in; names that aren't allowed are left out.
    /// Puts the player's copy where the recomp reads it (`game.into`), before a start. A folder's
    /// files and folders go in as links (junctions on Windows), a disc image as a hard link, or a
    /// copy when it's on another drive. What's already there stays.
    pub fn place(&self, install: &Path, game: Option<&Path>) -> std::io::Result<()> {
        let (Some(g), Some(copy)) = (&self.game, game) else { return Ok(()) };
        let into = g.into.trim().trim_matches(['/', '\\']);
        if into.is_empty() || into.contains("..") {
            return Ok(());
        }
        let dir = if into == "." { install.to_path_buf() } else { install.join(into) };
        // The copy picked is that folder already.
        if dir == copy {
            return Ok(());
        }
        std::fs::create_dir_all(&dir)?;
        let mut items: Vec<PathBuf> = if copy.is_dir() { std::fs::read_dir(copy)?.flatten().map(|e| e.path()).collect() } else { vec![copy.to_path_buf()] };
        // A .cue sheet is only the table of contents: the .bin tracks it names come along too.
        if !copy.is_dir() {
            let tracks = cue_tracks(copy);
            // The game wants the disc's one .bin under its own name (ctr-u.bin), and the player
            // picked the sheet: the track it names goes there, not the sheet renamed.
            if tracks.len() == 1 && g.name.trim().to_ascii_lowercase().ends_with(".bin") {
                items = tracks;
            } else {
                items.extend(tracks);
            }
        }
        let single = !copy.is_dir();
        let first = items.first().cloned();
        for from in items {
            let Some(name) = from.file_name() else { continue };
            let to = if single && Some(&from) == first.as_ref() && !g.name.trim().is_empty() && !g.name.contains(['/', '\\']) { dir.join(g.name.trim()) } else { dir.join(name) };
            // A ROM put there in the wrong byte order by an older version: done again.
            if single && to.is_file() {
                let head = std::fs::File::open(&to).ok().and_then(|mut f| read_at(&mut f, 0, 4));
                if head.as_deref().and_then(n64_order).map(|o| o != 0).unwrap_or(false) && to.extension().map(|e| e.eq_ignore_ascii_case("z64")).unwrap_or(false) {
                    let _ = std::fs::remove_file(&to);
                }
            }
            // An empty folder the release came with (targets/exefs, say) is a place for it, not
            // something already there.
            if empty_dir(&to) {
                let _ = std::fs::remove_dir(&to);
            }
            if to.symlink_metadata().is_ok() {
                continue;
            }
            if from.is_dir() {
                link_dir(&from, &to)?;
            } else if place_z64(&from, &to)? {
                continue;
            } else if std::fs::hard_link(&from, &to).is_err() {
                copy_checked(&from, &to)?;
            }
        }
        Ok(())
    }

    /// After `place`: what the game's folder is still missing of what it must hold (empty: all there).
    pub fn missing_after_place(&self, install: &Path) -> Vec<String> {
        let Some(g) = &self.game else { return vec![] };
        let into = g.into.trim().trim_matches(['/', '\\']);
        if into.is_empty() || into.contains("..") || g.contains.is_empty() {
            return vec![];
        }
        let dir = if into == "." { install.to_path_buf() } else { install.join(into) };
        // An empty folder doesn't count: a release can come with the folders the player's files go in.
        let names: Vec<String> = std::fs::read_dir(&dir).into_iter().flatten().flatten().filter(|e| !empty_dir(&e.path())).map(|e| e.file_name().to_string_lossy().to_lowercase()).collect();
        g.contains
            .iter()
            .filter(|c| !c.contains(['*', '/', '\\']))
            .filter(|c| !names.contains(&c.to_lowercase()))
            .cloned()
            .collect()
    }

    /// The game's files already where it reads them (put there by the player): the folder they're
    /// in. Then no copy has to be picked.
    pub fn already_in_place(&self, install: &Path) -> Option<PathBuf> {
        let g = self.game.as_ref()?;
        let into = g.into.trim().trim_matches(['/', '\\']);
        if into.is_empty() || into.contains("..") || !g.contains.iter().any(|c| !c.contains(['*', '/', '\\'])) {
            return None;
        }
        if !self.missing_after_place(install).is_empty() {
            return None;
        }
        Some(if into == "." { install.to_path_buf() } else { install.join(into) })
    }

    pub fn env_vars(&self, install: &Path, game: Option<&Path>) -> Vec<(String, String)> {
        let _w = WineFill::new(self.through_proton(install));
        self.start.as_ref().map(|s| s.env.iter().filter(|(k, _)| env_name_ok(k)).filter_map(|(k, v)| {
            let value = crate::inputs::expand(&fill(v, install, game), &self.input_paths, self.through_proton(install));
            (!value.contains("{input:")).then(|| (k.clone(), value))
        }).collect()).unwrap_or_default()
    }

    pub fn command(&self, install: &Path, game: Option<&Path>, picks: &BTreeMap<String, usize>) -> Option<(PathBuf, Vec<String>)> {
        self.command_for(install, game, picks, true)
    }

    /// The command it runs. `windows_build`: the installed build is the Windows one (on Windows, or
    /// through Proton), so the profile's Windows program comes first. With a native build, a program
    /// with the same name comes first: an old `.exe` left beside it (a switch from the Windows build)
    /// must not hide the native one.
    pub fn command_for(&self, install: &Path, game: Option<&Path>, picks: &BTreeMap<String, usize>, windows_build: bool) -> Option<(PathBuf, Vec<String>)> {
        let _w = WineFill::new(self.through_proton(install));
        let s = self.start.as_ref()?;
        let twin = || if cfg!(windows) { None } else { s.program.iter().find_map(|p| native_twin(install, p)) };
        let exe = if windows_build { None } else { twin() }
            .or_else(|| s.program.iter().map(|p| install.join(p)).find(|p| p.is_file()))
            .or_else(twin)?;
        crate::boot::runnable(&exe);
        Some((exe, self.args_for(install, game, picks)))
    }

    /// The command line it gives its program: its own arguments (the player's copy among them) and
    /// the settings that go there.
    pub fn args_for(&self, install: &Path, game: Option<&Path>, picks: &BTreeMap<String, usize>) -> Vec<String> {
        let _w = WineFill::new(self.through_proton(install));
        let Some(s) = self.start.as_ref() else { return vec![] };
        let mut args: Vec<String> = s.args.iter().map(|a| fill(a, install, game)).collect();
        for st in self.settings.iter().filter(|s| s.target == "args") {
            if let Some(c) = picks.get(&st.id).and_then(|i| st.choices.get(*i)) {
                args.extend(c.args.iter().map(|a| fill(a, install, game)));
            }
            args.extend(self.slider_args(st).into_iter().map(|a| fill(&a, install, game)));
        }
        crate::inputs::arguments(&args, &self.input_paths, self.through_proton(install))
    }

    /// A command-line slider the player moved: its arguments with the number in place of {value}.
    /// Not moved: nothing, so the game keeps its own value.
    fn slider_args(&self, st: &Setting) -> Vec<String> {
        let (Some(sl), Some(v)) = (st.slider.as_ref(), self.slider_vals.get(&st.id)) else { return vec![] };
        let list = if st.args.is_empty() { &sl.args } else { &st.args };
        list.iter().map(|a| a.replace("{value}", v)).collect()
    }

    /// Whether a setting is hidden behind Show Advanced Options: itself, or its whole group.
    pub fn is_advanced(&self, st: &Setting) -> bool {
        st.advanced || (!st.group.is_empty() && self.groups.iter().any(|g| g.advanced && g.id == st.group))
    }

    /// A slider's value now, exactly as written: in its file (or the player's, on the command line),
    /// else the game's default, else its low end. Never rounded or rewritten here.
    pub fn slider_now(&self, st: &Setting, install: &Path) -> Option<String> {
        let sl = st.slider.as_ref()?;
        let un = |x: &str| x.trim().trim_matches(|c| c == '"' || c == '\'').trim().to_string();
        let key = if sl.key.is_empty() { st.id.as_str() } else { sl.key.as_str() };
        let now = if st.target == "args" { self.slider_vals.get(&st.id).cloned() } else { self.config_file(&st.target, install).and_then(|f| f.get(key)) };
        Some(now.or_else(|| st.defaults.get(key).cloned()).map(|v| un(&v)).filter(|v| !v.is_empty()).unwrap_or_else(|| sl.text(sl.min)))
    }

    /// The player moved a slider: the number, on a step and written its way, goes into its file in
    /// the file's own style (quoted when the value there is). On the command line the caller keeps
    /// it. Gives back what was written.
    pub fn set_slider(&self, install: &Path, id: &str, value: f64) -> Result<String, String> {
        let st = self.settings.iter().find(|s| s.id == id).ok_or("That isn't one of its settings.")?;
        let sl = st.slider.as_ref().ok_or("That setting isn't a slider.")?;
        if !value.is_finite() {
            return Err("That isn't a number.".into());
        }
        let text = sl.text(sl.snap(value));
        if st.target == "args" {
            return Ok(text);
        }
        let key = if sl.key.is_empty() { st.id.as_str() } else { sl.key.as_str() };
        let mut f = self.config_file(&st.target, install).ok_or("Its settings file couldn't be found.")?;
        if self.from_boot && !f.existed {
            return Err("Play the game once first: it makes its settings file then.".into());
        }
        let now = f.get(key).map(|v| v.trim().to_string()).unwrap_or_default();
        let q = now.chars().next().filter(|c| (*c == '"' || *c == '\'') && now.len() >= 2 && now.ends_with(*c));
        let out = match q {
            Some(q) => format!("{q}{text}{q}"),
            None => text.clone(),
        };
        f.set(key, &out);
        f.save().map_err(|e| format!("Couldn't save its settings: {e}"))?;
        Ok(text)
    }

    /// A bindings setting's actions now: each one's keys as its file has them (or the game's
    /// defaults), in order.
    pub fn bindings_now(&self, st: &Setting, install: &Path) -> Vec<Vec<String>> {
        let Some(b) = st.bindings.as_ref() else { return vec![] };
        let f = self.config_file(&st.target, install);
        b.actions.iter().map(|a| f.as_ref().and_then(|f| f.get(&a.key)).or_else(|| st.defaults.get(&a.key).cloned()).map(|v| b.split(&v)).unwrap_or_default()).collect()
    }

    /// Whether any action of a bindings setting is on other keys than the game's defaults.
    pub fn bindings_changed(&self, st: &Setting, install: &Path) -> bool {
        let Some(b) = st.bindings.as_ref() else { return false };
        b.actions.iter().zip(self.bindings_now(st, install)).any(|(a, now)| st.defaults.get(&a.key).map(|d| b.split(d) != now).unwrap_or(false))
    }

    /// A value written the way its file writes them: quoted in JSON and TOML, and in an INI file
    /// when the value there was.
    fn bind_text(f: &File, key: &str, text: &str) -> String {
        match f.format.as_str() {
            "json" => Value::String(text.to_string()).to_string(),
            _ => {
                let now = f.get(key).map(|v| v.trim().to_string()).unwrap_or_default();
                let q = now.chars().next().filter(|c| (*c == '"' || *c == '\'') && now.len() >= 2 && now.ends_with(*c)).or(if f.format == "toml" { Some('"') } else { None });
                match q {
                    Some(q) => format!("{q}{}{q}", text.replace(q, "")),
                    None => text.to_string(),
                }
            }
        }
    }

    /// The player set an action's keys. Keys it already had stay as written; new ones get the
    /// name the file uses for them. Nothing is written when nothing changed. Gives back the keys
    /// as written.
    pub fn set_binding(&self, install: &Path, id: &str, action: &str, keys: &[String]) -> Result<Vec<String>, String> {
        let st = self.settings.iter().find(|s| s.id == id).ok_or("That isn't one of its settings.")?;
        let b = st.bindings.as_ref().ok_or("That setting isn't a list of keys.")?;
        let act = b.actions.iter().find(|a| a.key == action).ok_or("That isn't one of its actions.")?;
        let mut f = self.config_file(&st.target, install).ok_or("Its settings file couldn't be found.")?;
        if self.from_boot && !f.existed {
            return Err("Play the game once first: it makes its settings file then.".into());
        }
        let now = f.get(&act.key).or_else(|| st.defaults.get(&act.key).cloned()).map(|v| b.split(&v)).unwrap_or_default();
        // Every name this setting uses already, in the file and in the game's defaults.
        let present: Vec<String> = b.actions.iter().flat_map(|a| f.get(&a.key).into_iter().chain(st.defaults.get(&a.key).cloned())).flat_map(|v| b.split(&v)).collect();
        let mut out: Vec<String> = vec![];
        for k in keys.iter().map(|k| k.trim()).filter(|k| !k.is_empty() && !k.contains(b.sep())) {
            let name = if present.iter().any(|p| p == k) { k.to_string() } else { b.file_name(k, &present) };
            if !out.iter().any(|o| o.eq_ignore_ascii_case(&name)) {
                out.push(name);
            }
            if out.len() >= b.max() {
                break;
            }
        }
        if out == now {
            return Ok(out);
        }
        let text = Self::bind_text(&f, &act.key, &out.join(b.sep()));
        f.set(&act.key, &text);
        f.save().map_err(|e| format!("Couldn't save its settings: {e}"))?;
        Ok(out)
    }

    /// Reset to Default: every action back on the game's own keys, where the file has others.
    pub fn reset_bindings(&self, install: &Path, id: &str) -> Result<(), String> {
        let st = self.settings.iter().find(|s| s.id == id).ok_or("That isn't one of its settings.")?;
        let b = st.bindings.as_ref().ok_or("That setting isn't a list of keys.")?;
        let mut f = self.config_file(&st.target, install).ok_or("Its settings file couldn't be found.")?;
        let mut changed = false;
        for a in &b.actions {
            let (Some(d), Some(now)) = (st.defaults.get(&a.key), f.get(&a.key)) else { continue };
            if b.split(d) != b.split(&now) {
                let text = Self::bind_text(&f, &a.key, &b.split(d).join(b.sep()));
                f.set(&a.key, &text);
                changed = true;
            }
        }
        if changed {
            f.save().map_err(|e| format!("Couldn't save its settings: {e}"))?;
        }
        Ok(())
    }

    /// A value the game wrote itself (in its own menu) that isn't one of a setting's choices: as
    /// written, to show as it is. Only for a setting whose choices all set the same one key.
    pub fn own_value(&self, st: &Setting, install: &Path) -> Option<String> {
        if st.target == "args" || st.slider.is_some() || st.bindings.is_some() || st.choices.is_empty() {
            return None;
        }
        let keys: std::collections::BTreeSet<&String> = st.choices.iter().flat_map(|c| c.set.keys()).collect();
        if keys.len() != 1 || st.choices.iter().any(|c| c.set.len() != 1) {
            return None;
        }
        let k = keys.into_iter().next()?;
        let v = self.config_file(&st.target, install)?.get(k)?;
        if st.choices.iter().any(|c| c.set.get(k).map(|x| same(x, &v)).unwrap_or(false)) {
            return None;
        }
        let v = v.trim().trim_matches(|c| c == '"' || c == '\'').trim().to_string();
        (!v.is_empty()).then_some(v)
    }

    /// Which choice each setting is on (the first whose values all match), and whether it applies.
    pub fn current(&self, install: &Path, picks: &BTreeMap<String, usize>) -> Vec<(usize, bool)> {
        let mut files: BTreeMap<String, File> = BTreeMap::new();
        for c in &self.config {
            if let Some(f) = self.config_file(&c.id, install) {
                files.insert(c.id.clone(), f);
            }
        }
        self.settings
            .iter()
            .map(|st| {
                if st.target == "args" {
                    return (picks.get(&st.id).copied().filter(|i| *i < st.choices.len()).unwrap_or(0), true);
                }
                let Some(f) = files.get(&st.target) else { return (0, true) };
                let val = |k: &str| f.get(k).or_else(|| st.defaults.get(k).cloned()).unwrap_or_default();
                // Written by the game in its own way (`1.0` for `1`): matched loosely, and when no
                // choice matches, the one with the game's defaults.
                let i = st
                    .choices
                    .iter()
                    .position(|c| c.set.iter().all(|(k, v)| same(&val(k), v)))
                    .or_else(|| st.choices.iter().position(|c| c.set.iter().all(|(k, v)| st.defaults.get(k).map(|d| same(d, v)).unwrap_or(false))))
                    .unwrap_or(0);
                let applies = st.when.iter().all(|(k, v)| same(&val(k), v) || (f.get(k).is_none() && self.default_of(k).map(|d| same(&d, v)).unwrap_or(false)));
                (i, applies)
            })
            .collect()
    }

    /// The arguments the settings on the command line add (for a profile that doesn't say how the
    /// game starts: the launcher starts the program as usual, with these).
    pub fn setting_args(&self, install: &Path, picks: &BTreeMap<String, usize>) -> Vec<String> {
        let mut args = vec![];
        for st in self.settings.iter().filter(|s| s.target == "args") {
            if let Some(c) = picks.get(&st.id).and_then(|i| st.choices.get(*i)) {
                args.extend(c.args.iter().map(|a| fill(a, install, None)));
            }
            args.extend(self.slider_args(st).into_iter().map(|a| fill(&a, install, None)));
        }
        args
    }

    /// Whether a settings file its settings change isn't there yet (the game makes it on its first start).
    pub fn waiting_for_files(&self, install: &Path) -> bool {
        self.settings.iter().filter(|s| s.target != "args").any(|s| self.config_file(&s.target, install).map(|f| !f.existed).unwrap_or(true))
    }

    /// The values its settings have in their files, to put back after an update replaces the files.
    pub fn kept(&self, install: &Path) -> Vec<(String, String, String)> {
        let mut out = vec![];
        for c in &self.config {
            let Some(f) = self.config_file(&c.id, install).filter(|f| f.existed) else { continue };
            let mut keys: Vec<&String> = self
                .settings
                .iter()
                .filter(|s| s.target == c.id)
                .flat_map(|s| s.choices.iter().flat_map(|ch| ch.set.keys()).chain(s.slider.as_ref().map(|sl| if sl.key.is_empty() { &s.id } else { &sl.key })).chain(s.bindings.iter().flat_map(|b| b.actions.iter().map(|a| &a.key))))
                .collect();
            keys.sort();
            keys.dedup();
            for k in keys {
                if let Some(v) = f.get(k) {
                    out.push((c.id.clone(), k.clone(), v));
                }
            }
        }
        out
    }

    /// Puts back what `kept` read, where an update changed it.
    pub fn restore(&self, install: &Path, kept: &[(String, String, String)]) {
        for c in &self.config {
            let mine: Vec<_> = kept.iter().filter(|(id, _, _)| *id == c.id).collect();
            if mine.is_empty() {
                continue;
            }
            let Some(mut f) = self.config_file(&c.id, install).filter(|f| f.existed) else { continue };
            let mut changed = false;
            for (_, k, v) in mine {
                // Only keys the new file still has.
                if f.get(k).map(|now| now != *v).unwrap_or(false) {
                    f.set(k, v);
                    changed = true;
                }
            }
            if changed {
                let _ = f.save();
            }
        }
    }

    fn default_of(&self, key: &str) -> Option<String> {
        self.settings.iter().find_map(|s| s.defaults.get(key).cloned())
    }

    /// Put a setting on one of its choices (a settings file's settings; "args" ones are kept by the caller).
    pub fn pick(&self, install: &Path, id: &str, choice: usize) -> Result<(), String> {
        let st = self.settings.iter().find(|s| s.id == id).ok_or("That isn't one of its settings.")?;
        let c = st.choices.get(choice).ok_or("That isn't one of its choices.")?;
        if st.target == "args" {
            return Ok(());
        }
        let mut f = self.config_file(&st.target, install).ok_or("Its settings file couldn't be found.")?;
        // The launcher's own settings for a game only change a file the game made.
        if self.from_boot && !f.existed {
            return Err("Play the game once first: it makes its settings file then.".into());
        }
        for (k, v) in &c.set {
            f.set(k, v);
        }
        f.save().map_err(|e| format!("Couldn't save its settings: {e}"))
    }

    pub fn folder(&self, name: &str, install: &Path) -> Option<PathBuf> {
        place(self.folders.get(name)?, install)
    }
}

// ---------- Match My Screen ----------

impl Profile {
    /// A last choice, Match My Screen, on each setting that writes a resolution (width and height
    /// keys, or one "WxH" value) or a screen shape (a number like 1.777, or "16:9"), when all its
    /// choices are plain values in one format: the main monitor's size or shape, written the way
    /// the other choices are. Settings with named choices ("Stretch") are left as they are.
    pub fn add_screen_choices(&mut self, screen: Option<(u32, u32)>) {
        let Some((w, h)) = screen.filter(|(w, h)| *w >= 320 && *h >= 200) else { return };
        for st in self.settings.iter_mut() {
            if st.target == "args" || st.slider.is_some() || st.bindings.is_some() || st.choices.len() < 2 || st.screen.is_some() {
                continue;
            }
            if let Some((set, shown)) = screen_values(st, w, h) {
                st.choices.push(Choice { label: format!("Match my screen \u{b7} {shown}"), set, args: vec![] });
                st.screen = Some(shown);
            }
        }
    }

    /// Before Play, after the monitor changed: the settings the player put on Match My Screen get
    /// its size or shape now, where the file doesn't have it already. Gives back the ones it wrote.
    pub fn follow_screen(&self, install: &Path, ids: &[String]) -> Vec<String> {
        let mut out = vec![];
        for st in self.settings.iter().filter(|s| s.screen.is_some() && ids.contains(&s.id)) {
            let last = st.choices.len() - 1;
            let Some(f) = self.config_file(&st.target, install).filter(|f| f.existed) else { continue };
            let on = st.choices[last].set.iter().all(|(k, v)| f.get(k).map(|x| same(&x, v)).unwrap_or(false));
            if !on && self.pick(install, &st.id, last).is_ok() {
                out.push(st.id.clone());
            }
        }
        out
    }
}

/// The common name of a screen's shape ("21:9" for 3440x1440), or its own when none is close.
pub fn shape_name(w: u32, h: u32) -> String {
    const SHAPES: [(u32, u32); 9] = [(4, 3), (5, 4), (3, 2), (16, 10), (5, 3), (16, 9), (21, 9), (32, 10), (32, 9)];
    let r = w as f64 / h as f64;
    let (best, err) = SHAPES.iter().map(|&(a, b)| ((a, b), ((a as f64 / b as f64) - r).abs() / r)).fold(((0, 0), f64::MAX), |m, x| if x.1 < m.1 { x } else { m });
    if err <= 0.035 {
        return format!("{}:{}", best.0, best.1);
    }
    let g = gcd(w, h);
    format!("{}:{}", w / g, h / g)
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a.max(1) } else { gcd(b, a % b) }
}

/// What Match My Screen writes on this setting, and what it shows ("3440\u{d7}1440", "21:9"); None
/// when its choices aren't all plain sizes or shapes in one format.
fn screen_values(st: &Setting, w: u32, h: u32) -> Option<(BTreeMap<String, String>, String)> {
    let keys: Vec<&String> = st.choices[0].set.keys().collect();
    if keys.is_empty() || st.choices.iter().any(|c| !c.args.is_empty() || c.set.keys().collect::<Vec<_>>() != keys) {
        return None;
    }
    let plain = |v: &str| -> Option<u32> { let v = v.trim(); (!v.is_empty() && v.len() <= 5 && v.bytes().all(|b| b.is_ascii_digit())).then(|| v.parse().ok()).flatten() };
    let size = format!("{w}\u{d7}{h}");
    let hint = |k: &str, words: &[&str]| { let k = k.to_ascii_lowercase(); words.iter().any(|w| k.contains(w)) };
    if keys.len() == 2 {
        // Width and height in their own keys: whole numbers, the width the bigger one in each.
        let (a, b) = (keys[0], keys[1]);
        let mut pairs = vec![];
        for c in &st.choices {
            pairs.push((plain(&c.set[a])?, plain(&c.set[b])?));
        }
        let fits = |p: &[(u32, u32)]| p.iter().all(|&(x, y)| y >= 200 && x <= 16384 && (x as f64 / y as f64) >= 1.2 && (x as f64 / y as f64) <= 4.0);
        let flipped: Vec<(u32, u32)> = pairs.iter().map(|&(x, y)| (y, x)).collect();
        let named = |k: &str, end: char| hint(k, &["width", "height", "res", "size"]) || k.to_ascii_lowercase().ends_with(end) || k.to_ascii_lowercase().ends_with(if end == 'x' { 'w' } else { 'h' });
        let (wk, hk) = if fits(&pairs) { (a, b) } else if fits(&flipped) { (b, a) } else { return None };
        if !named(wk, 'x') || !named(hk, 'y') {
            return None;
        }
        return Some((BTreeMap::from([(wk.clone(), w.to_string()), (hk.clone(), h.to_string())]), size));
    }
    if keys.len() != 1 {
        return None;
    }
    let k = keys[0];
    // A text value in the file's own syntax keeps its quotes ("\"1280x720\"" in TOML or JSON): read
    // inside them, and written with the same ones.
    let unq = |v: &str| -> (String, String) {
        let v = v.trim();
        for q in ["\"", "'"] {
            if v.len() >= 2 && v.starts_with(q) && v.ends_with(q) {
                return (v[1..v.len() - 1].trim().to_string(), q.to_string());
            }
        }
        (v.to_string(), String::new())
    };
    let parsed: Vec<(String, String)> = st.choices.iter().map(|c| unq(&c.set[k])).collect();
    let q = parsed[0].1.clone();
    if parsed.iter().any(|(_, x)| *x != q) {
        return None;
    }
    let vals: Vec<&str> = parsed.iter().map(|(v, _)| v.as_str()).collect();
    // "1280x720": the same separator in each (x, X, the times sign or *, with or without spaces).
    let split = |v: &str| -> Option<(String, String, String)> {
        let i = v.find(|c: char| !c.is_ascii_digit())?;
        let j = v.rfind(|c: char| !c.is_ascii_digit())?;
        let j = j + v[j..].chars().next()?.len_utf8();
        let (x, sep, y) = (&v[..i], &v[i..j], &v[j..]);
        let mid = sep.trim();
        (plain(x).is_some() && plain(y).is_some() && ["x", "X", "\u{d7}", "*"].contains(&mid)).then(|| (x.to_string(), sep.to_string(), y.to_string()))
    };
    if let Some(first) = split(vals[0]) {
        if vals.iter().all(|v| split(v).map(|p| p.1 == first.1 && plain(&p.0).unwrap_or(0) > plain(&p.2).unwrap_or(u32::MAX)).unwrap_or(false)) {
            return Some((BTreeMap::from([(k.clone(), format!("{q}{w}{}{h}{q}", first.1))]), size));
        }
        return None;
    }
    let shape = shape_name(w, h);
    // "16:9": a shape, written the same way.
    let ratio = |v: &str| -> Option<String> { let (x, y) = v.split_once(':')?; (plain(x).is_some() && plain(y).is_some()).then(|| v[x.len()..v.len() - y.len()].to_string()) };
    if let Some(sep) = ratio(vals[0]) {
        if vals.iter().all(|v| ratio(v).as_deref() == Some(sep.as_str())) {
            let (x, y) = shape.split_once(':')?;
            return Some((BTreeMap::from([(k.clone(), format!("{q}{x}{sep}{y}{q}"))]), shape));
        }
        return None;
    }
    // 1.777: a shape as a number, on a setting that says it's one, with as many decimals as the others.
    if !hint(k, &["aspect", "ratio", "shape"]) && !hint(&st.label, &["aspect", "ratio", "shape"]) && !hint(&st.id, &["aspect", "ratio", "shape"]) {
        return None;
    }
    let num = |v: &str| -> Option<(f64, usize)> {
        let d = v.split_once('.').map(|(_, f)| f.len()).unwrap_or(0);
        (v.bytes().all(|b| b.is_ascii_digit() || b == b'.') && v.matches('.').count() <= 1).then(|| v.parse::<f64>().ok().filter(|x| (1.0..=4.0).contains(x)).map(|x| (x, d))).flatten()
    };
    let mut decimals = 0;
    for v in &vals {
        decimals = decimals.max(num(v)?.1);
    }
    if decimals == 0 {
        return None;
    }
    Some((BTreeMap::from([(k.clone(), format!("{q}{:.*}{q}", decimals, w as f64 / h as f64))]), shape))
}

// ---------- the player's copy ----------

fn read_at(f: &mut std::fs::File, at: u64, n: usize) -> Option<Vec<u8>> {
    f.seek(SeekFrom::Start(at)).ok()?;
    let mut b = vec![0u8; n];
    f.read_exact(&mut b).ok()?;
    Some(b)
}

#[derive(Debug, PartialEq)]
enum Holds {
    Nothing,
    OnlyMedia,
    Files,
}

/// What a folder picked as the player's copy holds, a few levels down: nothing, only pictures,
/// videos and documents, or other files. Windows' own notes about a folder don't count.
fn folder_holds(p: &Path) -> Holds {
    let mut stack = vec![(p.to_path_buf(), 0usize)];
    let (mut seen, mut any) = (0usize, false);
    while let Some((d, depth)) = stack.pop() {
        // A folder it can't read, or one deeper than it looks, might have the game in it.
        let Ok(list) = std::fs::read_dir(&d) else { return Holds::Files };
        for e in list.flatten() {
            seen += 1;
            if seen > 5000 {
                return Holds::Files;
            }
            let path = e.path();
            if path.is_dir() {
                if depth >= 4 {
                    return Holds::Files;
                }
                stack.push((path, depth + 1));
                continue;
            }
            let name = e.file_name().to_string_lossy().to_ascii_lowercase();
            if matches!(name.as_str(), "desktop.ini" | "thumbs.db" | ".ds_store") {
                continue;
            }
            any = true;
            let ext = path.extension().map(|x| x.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            if !NOT_A_GAME.contains(&ext.as_str()) {
                return Holds::Files;
            }
        }
    }
    if any { Holds::OnlyMedia } else { Holds::Nothing }
}

/// A file in the top folder of an ISO 9660 disc image (PlayStation 2 and most others).
fn iso9660_has(path: &Path, name: &str) -> bool {
    let Ok(mut f) = std::fs::File::open(path) else { return false };
    let Some(pvd) = read_at(&mut f, 16 * 2048, 2048) else { return false };
    if &pvd[1..6] != b"CD001" {
        return false;
    }
    let root = &pvd[156..190];
    let lba = u32::from_le_bytes([root[2], root[3], root[4], root[5]]) as u64;
    let size = (u32::from_le_bytes([root[10], root[11], root[12], root[13]]) as usize).min(1 << 20);
    let Some(dir) = read_at(&mut f, lba * 2048, size) else { return false };
    let want = name.to_ascii_uppercase();
    let mut i = 0;
    while i < dir.len() {
        let len = dir[i] as usize;
        if len == 0 {
            i = (i / 2048 + 1) * 2048;
            continue;
        }
        if i + 33 > dir.len() {
            break;
        }
        let nl = dir[i + 32] as usize;
        if i + 33 + nl <= dir.len() {
            let n = String::from_utf8_lossy(&dir[i + 33..i + 33 + nl]).to_ascii_uppercase();
            if n.split(';').next() == Some(want.as_str()) {
                return true;
            }
        }
        i += len;
    }
    false
}

/// A file in the top folder of an Xbox 360 disc image (XDVDFS).
/// Where an Xbox disc image's file system is, if it has one this reader (and the games) know.
fn xdvdfs_part(path: &Path) -> Option<u64> {
    let mut f = std::fs::File::open(path).ok()?;
    [0u64, 0x2080000, 0xFD90000, 0x18300000].into_iter().find(|part| read_at(&mut f, part + 32 * 2048, 20).map(|d| &d[..] == b"MICROSOFT*XBOX*MEDIA").unwrap_or(false))
}

fn xdvdfs_has(path: &Path, name: &str) -> bool {
    let Ok(mut f) = std::fs::File::open(path) else { return false };
    let want = name.to_ascii_lowercase();
    for part in [0u64, 0x2080000, 0xFD90000, 0x18300000] {
        let Some(d) = read_at(&mut f, part + 32 * 2048, 28) else { continue };
        if &d[..20] != b"MICROSOFT*XBOX*MEDIA" {
            continue;
        }
        let sector = u32::from_le_bytes([d[20], d[21], d[22], d[23]]) as u64;
        let size = (u32::from_le_bytes([d[24], d[25], d[26], d[27]]) as usize).min(1 << 20);
        let Some(t) = read_at(&mut f, part + sector * 2048, size) else { return false };
        let mut pos = 0;
        while pos + 14 <= t.len() {
            if t[pos] == 0xFF && t[pos + 1] == 0xFF {
                pos = (pos / 2048 + 1) * 2048;
                continue;
            }
            let nl = t[pos + 13] as usize;
            if pos + 14 + nl > t.len() {
                break;
            }
            if String::from_utf8_lossy(&t[pos + 14..pos + 14 + nl]).to_ascii_lowercase() == want {
                return true;
            }
            pos = (pos + 14 + nl + 3) & !3;
        }
        return false;
    }
    false
}

/// A link to a folder: a junction on Windows (no administrator rights needed), a symlink elsewhere.
fn link_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        crate::win::junction(from, to).map_err(|e| std::io::Error::other(format!("Couldn't link {} into the game's folder: {e}", from.display())))
    }
    #[cfg(not(windows))]
    {
        std::os::unix::fs::symlink(from, to)
    }
}

/// Whether this is the player's copy the game wants. Err says why not.
/// A copy packed in an archive (.7z, .zip, .rar) for a game that doesn't read archives itself:
/// its kind.
pub fn packed(g: &GameCopy, p: &Path) -> Option<String> {
    let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    (p.is_file() && ["7z", "zip", "rar"].contains(&ext.as_str()) && !g.name.to_lowercase().ends_with(&format!(".{ext}")) && !g.label.to_lowercase().contains(&format!(".{ext}"))).then_some(ext)
}

impl GameCopy {
    /// Whether its check tells this game's copy from another game's of the same console: a file
    /// only this game has, a disc ID. "Any disc image" or "any N64 ROM" doesn't, so
    /// it isn't used to find the game among the player's copies.
    /// A catalog boot profile's file checks are common files (`generic`): only an ID counts there.
    pub fn identifies(&self) -> bool {
        self.identifies_by_contents() || self.check.ids.iter().any(|i| crate::roms::id_key(i).len() >= 3)
    }

    /// Told apart by what's in it (a file only this game has, its N64 cartridge code), not an ID.
    pub fn identifies_by_contents(&self) -> bool {
        self.identifies_kind(true) || self.identifies_kind(false)
    }

    /// Only checks that `check_copy` applies to this kind of input can identify it. A file
    /// inside an extracted folder says nothing about an arbitrary package or ROM file.
    pub fn identifies_kind(&self, folder: bool) -> bool {
        let c = &self.check;
        let own = |f: &str| !f.is_empty() && !COMMON_FILES.iter().any(|x| f.eq_ignore_ascii_case(x));
        !c.nup_build_id.is_empty() || if folder {
            !self.generic && own(&c.file)
        } else {
            !c.n64_id.is_empty() || (!self.generic && (own(&c.xdvdfs) || own(&c.iso9660)))
        }
    }
}

/// Files every game of a console has, so they can't tell one game from another.
const COMMON_FILES: [&str; 9] = ["default.xex", "default.xbe", "eboot.bin", "param.sfo", "system.cnf", "main.npdm", "main", "main.dol", "boot.bin"];

pub fn check_copy(g: &GameCopy, p: &Path) -> Result<(), String> {
    let folder = p.is_dir();
    // A game packed in an archive: unpacked first (set_game_copy does that for .7z and .zip).
    if let Some(ext) = packed(g, p) {
        return Err(format!("That's a compressed file (.{ext}). Unpack it first with 7-Zip or Windows, then pick the game file inside it."));
    }
    let kind_ok = match g.kind.as_str() {
        "folder" => folder,
        "iso_or_folder" => folder || p.is_file(),
        _ => p.is_file(),
    };
    if !kind_ok {
        let disc = g.label.to_lowercase().contains("iso") || g.label.to_lowercase().contains("disc");
        return Err(if !folder { "It needs the game's folder, not a file.".into() } else if disc { "It needs the disc image (.iso), not a folder.".into() } else { "It needs the file itself, not a folder.".into() });
    }
    if folder {
        if !g.check.file.is_empty() && !p.join(&g.check.file).is_file() && !std::fs::read_dir(p).into_iter().flatten().flatten().any(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(&g.check.file)) {
            return Err(format!("That folder doesn't have {}.", g.check.file));
        }
        if !g.check.nup_build_id.is_empty() {
            return check_nup_build(&g.check.nup_build_id, nup::build_id_of_folder(p));
        }
        // With nothing more to check by: a folder with nothing in it, or only pictures and videos
        // (the launcher's own screenshots folder, say), is never anyone's game.
        if g.check.file.is_empty() {
            match folder_holds(p) {
                Holds::Nothing => return Err("That folder is empty. Pick the folder with your copy of the game.".into()),
                Holds::OnlyMedia => return Err("That folder only has pictures, videos or documents in it. Pick the folder with your copy of the game.".into()),
                Holds::Files => {}
            }
        }
        return Ok(());
    }
    // A compressed disc image (.rvz, .chd...) for a recomp that reads plain ones: said now, rather
    // than the game failing to read it. One whose label names that kind takes it. An NKit image
    // keeps .iso at the end (Game.nkit.iso), so its name is looked at too.
    let mut ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let lower_name = p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if lower_name.ends_with(".nkit.iso") || lower_name.ends_with(".nkit.gcz") || (ext == "iso" && nkit_image(p)) {
        ext = "nkit".into();
    }
    // A picture, a document or a video picked by mistake: never anyone's game.
    if NOT_A_GAME.contains(&ext.as_str()) && !g.label.to_lowercase().contains(&format!(".{ext}")) && !g.name.to_lowercase().ends_with(&format!(".{ext}")) {
        return Err("That's a picture, document or video, not your copy of the game. Pick the game file itself.".into());
    }
    // A program (the recomp's own .exe, a .dll beside it): never the player's copy, unless the copy
    // it asks for is a program of that kind.
    if PROGRAMS.contains(&ext.as_str()) && !g.label.to_lowercase().contains(&format!(".{ext}")) && !g.name.to_lowercase().ends_with(&format!(".{ext}")) {
        return Err("That's a program, not your copy of the game. Pick the game file itself.".into());
    }
    if ["rvz", "wia", "gcz", "wbfs", "chd", "cso", "zso", "nkit"].contains(&ext.as_str()) && !g.label.to_lowercase().contains(&format!(".{ext}")) && !g.label.to_lowercase().contains(&format!("{ext} ")) {
        let dolphin = ["rvz", "wia", "gcz", "wbfs", "nkit"].contains(&ext.as_str());
        return Err(if dolphin {
            format!("That's a compressed .{ext} image, and this recomp reads plain .iso files. Convert it first: in Dolphin, right-click the game, Convert File, and pick ISO.")
        } else {
            format!("That's a compressed .{ext} image, and this recomp reads plain .iso files. Convert it to .iso first.")
        });
    }
    if !g.check.iso9660.is_empty() && !iso9660_has(p, &g.check.iso9660) {
        return Err("That disc image isn't the release this recomp reads.".into());
    }
    // A disc ID that isn't one of this game's (a copy whose ID can't be read isn't refused).
    let ids: Vec<String> = g.check.ids.iter().map(|i| crate::roms::id_key(i)).filter(|i| i.len() >= 3).collect();
    if !ids.is_empty() {
        if let Some(id) = crate::roms::copy_id(p) {
            if !ids.iter().any(|w| id.starts_with(w.as_str())) {
                return Err(format!("That's a different game's disc image (its ID is {id})."));
            }
        }
    }
    if !g.check.xdvdfs.is_empty() && !xdvdfs_has(p, &g.check.xdvdfs) {
        return Err(if xdvdfs_part(p).is_none() {
            "That disc image can't be read: it may be incomplete, compressed or not an Xbox disc. Use a full, uncompressed .iso of your disc.".into()
        } else {
            "That disc image isn't the game this recomp reads.".into()
        });
    }
    if !g.check.nup_build_id.is_empty() {
        // By the package's magic, not its name; only its headers and `main`'s first bytes are read.
        let id = if nup::is_package(p) { nup::build_id_of_package(p) } else { Err("That isn't a Nintendo Switch 2 .nup package.".into()) };
        return check_nup_build(&g.check.nup_build_id, id);
    }
    let wants_n64 = !g.check.n64_region.is_empty() || !g.check.n64_id.is_empty() || g.check.n64_rev.is_some() || [".z64", ".n64", ".v64"].iter().any(|e| g.name.to_lowercase().ends_with(e));
    if wants_n64 {
        // An N64 ROM starts with one of three byte patterns; anything else isn't one.
        let Some(h) = n64_header(p) else {
            return Err("That isn't an N64 ROM. Pick your game's .z64, .n64 or .v64 file.".into());
        };
        {
            if !g.check.n64_id.is_empty() && !h[0x3C..0x3E].eq_ignore_ascii_case(g.check.n64_id.as_bytes()) {
                return Err("That ROM is a different game.".into());
            }
            let c = h[0x3E] as char;
            if !g.check.n64_region.is_empty() && c.is_ascii_uppercase() && !g.check.n64_region.contains(c) {
                return Err(if g.check.n64_region == "E" {
                    "That ROM isn't the USA version, which this recomp needs.".into()
                } else {
                    "That ROM is from a region this recomp doesn't read.".into()
                });
            }
            if let Some(rev) = g.check.n64_rev {
                if h[0x3F] != rev {
                    return Err("That ROM is a different version (revision) of the game than this recomp reads.".into());
                }
            }
        }
    }
    Ok(())
}

/// The build a package or folder is, against the one the recomp needs.
fn check_nup_build(want: &str, have: Result<String, String>) -> Result<(), String> {
    let have = have?;
    if nup::matches(&have, want)? {
        Ok(())
    } else {
        Err(format!("That copy is build {}. This recomp needs build {}.", &have[..8], want.trim().to_ascii_lowercase()))
    }
}

/// Kinds of file that are never a game: pictures, documents, videos, shortcuts.
const PROGRAMS: [&str; 9] = ["exe", "dll", "so", "appimage", "msi", "bat", "cmd", "sh", "pyd"];
const NOT_A_GAME: [&str; 24] = ["jpg", "jpeg", "png", "gif", "bmp", "webp", "heic", "avif", "tif", "tiff", "ico", "txt", "pdf", "doc", "docx", "rtf", "html", "htm", "url", "lnk", "mp4", "mkv", "avi", "mov"];

/// An NKit image under a plain .iso name: NKit writes "NKIT" at 0x200 of the GameCube and Wii
/// images it makes.
fn nkit_image(p: &Path) -> bool {
    let Ok(mut f) = std::fs::File::open(p) else { return false };
    read_at(&mut f, 0x200, 4).map(|b| b == b"NKIT").unwrap_or(false)
}

/// A SNES ROM whose size isn't one a full dump has (a multiple of 32 KB, plus 512 bytes when it
/// keeps a copier's header): most likely an incomplete or damaged file. Said, never refused.
pub fn odd_snes_size(p: &Path) -> bool {
    let ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if ext != "sfc" && ext != "smc" {
        return false;
    }
    let Ok(m) = std::fs::metadata(p) else { return false };
    let n = m.len();
    n >= 32 * 1024 && n % 0x8000 != 0 && n % 0x8000 != 512
}

/// The byte order an N64 ROM is in: 0 for .z64 (as the console reads it), 2 for .v64 (every two
/// bytes swapped), 4 for .n64 (every four bytes reversed). None: not an N64 ROM.
fn n64_order(first: &[u8]) -> Option<u8> {
    match first.get(..4)? {
        [0x80, 0x37, 0x12, 0x40] => Some(0),
        [0x37, 0x80, 0x40, 0x12] => Some(2),
        [0x40, 0x12, 0x37, 0x80] => Some(4),
        _ => None,
    }
}

/// Bytes from any N64 ROM in .z64 order.
pub fn n64_to_z64(b: &mut [u8], order: u8) {
    match order {
        2 => b.chunks_exact_mut(2).for_each(|c| c.swap(0, 1)),
        4 => b.chunks_exact_mut(4).for_each(|c| c.reverse()),
        _ => {}
    }
}

/// The track files a .cue sheet names (`FILE "Game (Track 1).bin" BINARY`), when they're next to it.
pub fn cue_tracks(cue: &Path) -> Vec<PathBuf> {
    if !cue.extension().map(|e| e.eq_ignore_ascii_case("cue")).unwrap_or(false) {
        return vec![];
    }
    let Some(dir) = cue.parent() else { return vec![] };
    let text = std::fs::read(cue).map(|b| String::from_utf8_lossy(&b[..b.len().min(64 * 1024)]).to_string()).unwrap_or_default();
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            if !l.to_ascii_uppercase().starts_with("FILE ") {
                return None;
            }
            let rest = l[5..].trim();
            let name = if let Some(r) = rest.strip_prefix('"') { r.split('"').next()? } else { rest.split_whitespace().next()? };
            let p = Path::new(name);
            // Only a plain file name next to the sheet, never a path out of its folder.
            if p.components().count() != 1 || name.contains("..") {
                return None;
            }
            Some(dir.join(p)).filter(|f| f.is_file())
        })
        .collect()
}

/// An N64 ROM's first 64 bytes (its header), in .z64 order whatever order the file is in.
fn n64_header(path: &Path) -> Option<Vec<u8>> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut h = read_at(&mut f, 0, 64)?;
    let order = n64_order(&h)?;
    n64_to_z64(&mut h, order);
    Some(h)
}

/// An N64 ROM's country letter (byte 0x3E), in any of the three byte orders ROMs come in.
#[allow(dead_code)]
fn n64_country(path: &Path) -> Option<char> {
    let c = n64_header(path)?[0x3E] as char;
    c.is_ascii_uppercase().then_some(c)
}

/// A .v64 or .n64 ROM placed under a .z64 name: written out in .z64 order (a link would hand the
/// game bytes in the wrong order: "bad N64 magic"). True when it did that.
fn place_z64(from: &Path, to: &Path) -> std::io::Result<bool> {
    let wants_z64 = to.extension().map(|e| e.eq_ignore_ascii_case("z64")).unwrap_or(false);
    if !wants_z64 {
        return Ok(false);
    }
    let mut f = std::fs::File::open(from)?;
    let Some(head) = read_at(&mut f, 0, 4) else { return Ok(false) };
    let Some(order) = n64_order(&head).filter(|o| *o != 0) else { return Ok(false) };
    let mut b = std::fs::read(from)?;
    n64_to_z64(&mut b, order);
    std::fs::write(to, b)?;
    Ok(true)
}

#[cfg(test)]
mod tests {

    #[test]
    fn appimage_embedded_profile_wins_and_binds_renamed_container() {
        use crate::appimage::tests::{image, TempDir};
        let dir = TempDir::new();
        let path = dir.0.join("Renamed game \u{03b1}.AppImage");
        let raw = br#"{"format":1,"name":"Embedded game","game":{"label":"embedded"},
            "start":{"program":["Original.AppImage"],"args":["--disc","{game}"],"env":{"MP_DISC":"{game}"}},
            "prepare":{"program":["Original.AppImage"],"args":["--prepare"]}}"#;
        std::fs::write(&path, image(Some(raw), backhand::compression::Compressor::Gzip)).unwrap();
        std::fs::write(dir.0.join("recomplauncher.json"), r#"{"format":1,"game":{"label":"adjacent"}}"#).unwrap();
        let entry = serde_json::json!({"launcher":{"format":1,"game":{"label":"catalog"}}});
        let owned = release_profile(&dir.0).unwrap();
        assert!(owned.embedded);
        assert_eq!(owned.raw["name"], "Embedded game");
        assert!(owned.location.ends_with(".AppImage!/recomplauncher.json"));
        let profile = find_own(&dir.0, "", &entry).unwrap();
        assert_eq!(profile.game.unwrap().label, "embedded");
        let start = profile.start.unwrap();
        assert_eq!(start.program, ["Renamed game \u{03b1}.AppImage"]);
        assert_eq!(start.args, ["--disc", "{game}"]);
        assert_eq!(start.env["MP_DISC"], "{game}");
        assert_eq!(profile.prepare.unwrap().program, ["Renamed game \u{03b1}.AppImage"]);
    }

    #[test]
    fn appimage_invalid_or_missing_metadata_falls_back_to_adjacent_then_catalog() {
        use crate::appimage::tests::{image, TempDir};
        let dir = TempDir::new();
        let entry = serde_json::json!({"launcher":{"format":1,"game":{"label":"catalog"}}});
        for content in [None, Some(&b"not JSON"[..]), Some(&br#"{"format":2}"#[..]),
                        Some(&br#"{"format":1,"start":{"program":42}}"#[..])] {
            std::fs::write(dir.0.join("game.AppImage"), image(content, backhand::compression::Compressor::Xz)).unwrap();
            let adjacent = dir.0.join("recomplauncher.json");
            std::fs::write(&adjacent, r#"{"format":1,"game":{"label":"adjacent"}}"#).unwrap();
            assert_eq!(find_own(&dir.0, "", &entry).unwrap().game.unwrap().label, "adjacent");
            assert!(!release_profile(&dir.0).unwrap().embedded);
            std::fs::remove_file(&adjacent).unwrap();
            assert_eq!(find_own(&dir.0, "", &entry).unwrap().game.unwrap().label, "catalog");
        }
    }

    #[test]
    fn appimage_replacement_is_read_without_a_stale_extracted_copy() {
        use crate::appimage::tests::{image, TempDir};
        let dir = TempDir::new();
        let path = dir.0.join("game.AppImage");
        for label in ["first", "other"] {
            let raw = serde_json::json!({"format":1,"game":{"label":label}}).to_string();
            std::fs::write(&path, image(Some(raw.as_bytes()), backhand::compression::Compressor::Zstd)).unwrap();
            assert_eq!(find_own(&dir.0, "", &Value::Null).unwrap().game.unwrap().label, label);
        }
        assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 1);
    }

    #[test]
    fn appimage_multiple_containers_do_not_override_the_folder_profile() {
        use crate::appimage::tests::{image, TempDir};
        let dir = TempDir::new();
        for name in ["first.AppImage", "second.AppImage"] {
            std::fs::write(dir.0.join(name), image(Some(br#"{"format":1,"game":{"label":"embedded"}}"#),
                                                   backhand::compression::Compressor::Gzip)).unwrap();
        }
        std::fs::write(dir.0.join("recomplauncher.json"), r#"{"format":1,"game":{"label":"adjacent"}}"#).unwrap();
        assert_eq!(find_own(&dir.0, "", &Value::Null).unwrap().game.unwrap().label, "adjacent");
    }

    #[test]
    fn appimage_plain_folders_keep_existing_fallback_order() {
        use crate::appimage::tests::TempDir;
        let dir = TempDir::new();
        let entry = serde_json::json!({"launcher":{"format":1,"game":{"label":"catalog"}}});
        assert_eq!(find_own(&dir.0, "", &entry).unwrap().game.unwrap().label, "catalog");
        std::fs::write(dir.0.join("recomplauncher.json"), r#"{"format":1,"game":{"label":"adjacent"}}"#).unwrap();
        assert_eq!(find_own(&dir.0, "", &entry).unwrap().game.unwrap().label, "adjacent");
        assert!(release_profile(Path::new("")).is_none());
    }


    #[test]
    fn game_data_profiles_from_the_store() {
        use super::*;
        let mk = |b: &str| from_boot(&serde_json::from_str::<Value>(b).unwrap()).unwrap();
        // A flag that takes the disc's files: an image is unpacked, the folder goes after the flag.
        let p = mk(r#"{"kind":"portable","start":"pgr4_recompiled.exe","start_is":"game","args":"--execute_unclipped_draw_vs_on_cpu --gpu_plugin=xenos","files":{"how":"flag","flag":"--game_data_root","accepts":"folder or iso","extract":"xdvdfs","contains":["default.xex"]}}"#);
        let g = p.game.as_ref().unwrap();
        assert_eq!((g.kind.as_str(), g.extract.as_str(), g.check.file.as_str()), ("iso_or_folder", "xdvdfs", "default.xex"));
        assert_eq!(p.start.as_ref().unwrap().args, ["--execute_unclipped_draw_vs_on_cpu", "--gpu_plugin=xenos", "--game_data_root", "{game}"]);
        // A folder holding only the package file is fine.
        let p = mk(r#"{"kind":"portable","start":"NHL3on3 Launcher.bat","files":{"how":"flag","flag":"--game_data_root","accepts":"folder"}}"#);
        let g = p.game.as_ref().unwrap();
        assert_eq!((g.kind.as_str(), g.check.file.as_str()), ("folder", ""));
        let d = std::env::temp_dir().join("rl-3on3-test");
        let _ = std::fs::create_dir_all(&d);
        std::fs::write(d.join("58410975"), b"x").unwrap();
        assert!(check_copy(g, &d).is_ok());
        // A file beside the program, under its own name.
        let p = mk(r#"{"kind":"portable","start":"NBA JAM Mod Manager.exe","files":{"how":"folder","folder":".","accepts":"file"}}"#);
        let g = p.game.as_ref().unwrap();
        assert_eq!((g.kind.as_str(), g.name.as_str(), g.into.as_str()), ("iso", "", "."));
        let inst = std::env::temp_dir().join("rl-openjam-test");
        let _ = std::fs::remove_dir_all(&inst);
        std::fs::create_dir_all(&inst).unwrap();
        let file = d.join("0123456789ABCDEF0123456789ABCDEF0123456789");
        std::fs::write(&file, b"pkg").unwrap();
        p.place(&inst, Some(&file)).unwrap();
        assert!(inst.join("0123456789ABCDEF0123456789ABCDEF0123456789").is_file());
        assert!(check_copy(g, &d).is_err());
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(&inst);
    }
    #[cfg(unix)]
    #[test]
    fn a_native_build_starts_its_own_region() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join("rl-test-twin");
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for n in ["pd.pal.x86_64", "pd.jpn.x86_64", "pd.x86_64", "libSDL2.so", "pd.ini"] {
            let p = d.join(n);
            std::fs::write(&p, b"x").unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert_eq!(super::native_twin(&d, "pd.i686.exe").unwrap().file_name().unwrap(), "pd.x86_64");
        assert_eq!(super::native_twin(&d, "pd.pal.i686.exe").unwrap().file_name().unwrap(), "pd.pal.x86_64");
        assert!(super::native_twin(&d, "other.exe").is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn a_native_build_ignores_a_leftover_windows_exe() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("rl-native-start-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for n in ["parasite-eve-pc.exe", "parasite-eve-pc"] {
            let p = d.join(n);
            std::fs::write(&p, b"x").unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let p: Profile = serde_json::from_str(r#"{"format":1,"start":{"program":["parasite-eve-pc.exe"]}}"#).unwrap();
        let picks = BTreeMap::new();
        // A Windows build (through Proton) starts the .exe; a native build starts the native program
        // even when the old .exe from a previous Windows install is still in the folder.
        assert_eq!(p.command_for(&d, None, &picks, true).unwrap().0.file_name().unwrap(), "parasite-eve-pc.exe");
        assert_eq!(p.command_for(&d, None, &picks, false).unwrap().0.file_name().unwrap(), "parasite-eve-pc");
        // The plain call keeps the Windows-first behavior for callers that don't know the build.
        assert_eq!(p.command(&d, None, &picks).unwrap().0.file_name().unwrap(), "parasite-eve-pc.exe");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn windows_games_through_proton_get_windows_paths() {
        assert_eq!(wine_path("/home/deck/Games/AC6/disc.iso"), r"Z:\home\deck\Games\AC6\disc.iso");
    }

    #[test]
    fn settings_file_under_another_name() {
        let d = std::env::temp_dir().join(format!("rl-near-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        assert!(super::near_config(&d.join("starship.cfg.json")).is_none());
        std::fs::write(d.join("Starship2.cfg.json"), "{}").unwrap();
        assert_eq!(super::near_config(&d.join("starship.cfg.json")), Some(d.join("Starship2.cfg.json")));
        std::fs::write(d.join("PORT_SETTINGS.INI"), "").unwrap();
        assert_eq!(super::near_config(&d.join("port_settings.ini")), Some(d.join("PORT_SETTINGS.INI")));
        let _ = std::fs::remove_dir_all(&d);
    }

    use super::*;

    fn tmp(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rl-prof-{n}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn groups_advanced_settings_and_sliders_parse() {
        let p: Profile = serde_json::from_str(r#"{"format":1,
            "config":[{"id":"main","format":"toml","file":[{"path":"{install}/config.toml"}]}],
            "groups":[{"id":"Advanced","advanced":true}],
            "settings":[
              {"id":"fs","label":"Full screen","group":"Display","choices":[{"label":"Off","set":{"Video/fullscreen":"false"}},{"label":"On","set":{"Video/fullscreen":"true"}}]},
              {"id":"sens","label":"Mouse Sensitivity","group":"Controls","slider":{"key":"Input/mouse_sensitivity","min":0.1,"max":5,"step":0.05,"format":"%.2f"},"defaults":{"Input/mouse_sensitivity":"1.0"}},
              {"id":"vs","label":"V-Sync","advanced":true,"choices":[{"label":"Off","set":{"Video/vsync":"false"}},{"label":"On","set":{"Video/vsync":"true"}}]},
              {"id":"tx","label":"Texture Cache","group":"Advanced","choices":[{"label":"Off","set":{"Video/tc":"0"}},{"label":"On","set":{"Video/tc":"1"}}]},
              {"id":"fov","label":"Field of View","in":"args","args":["--fov","{value}"],"slider":{"min":60,"max":120,"step":5,"format":"%d"}}
            ]}"#).unwrap();
        assert_eq!(p.settings.len(), 5);
        let adv: Vec<bool> = p.settings.iter().map(|s| p.is_advanced(s)).collect();
        assert_eq!(adv, vec![false, false, true, true, false]);
        assert_eq!(p.settings[1].group, "Controls");
        assert!(p.settings[1].choices.is_empty() && p.settings[1].slider.is_some());
        // A command-line slider: nothing until the player moves it, then its number in {value}.
        let mut p = p;
        assert!(p.setting_args(Path::new("/x"), &BTreeMap::new()).is_empty());
        assert_eq!(p.set_slider(Path::new("/x"), "fov", 93.0).unwrap(), "95");
        p.slider_vals.insert("fov".into(), "95".into());
        assert_eq!(p.setting_args(Path::new("/x"), &BTreeMap::new()), vec!["--fov", "95"]);
    }

    #[test]
    fn a_slider_leaves_the_games_own_value_alone_until_moved() {
        let d = tmp("slider");
        let f = d.join("config.toml");
        let text = "[Input]\nmouse_sensitivity = 1.37\ninvert = false\n";
        std::fs::write(&f, text).unwrap();
        let p: Profile = serde_json::from_str(r#"{"format":1,"config":[{"id":"main","format":"toml","file":[{"path":"{install}/config.toml"}]}],
            "settings":[{"id":"sens","label":"Mouse Sensitivity","slider":{"key":"Input/mouse_sensitivity","min":0.1,"max":5,"step":0.05,"format":"%.2f"},"defaults":{"Input/mouse_sensitivity":"1.0"}}]}"#).unwrap();
        // Off its step, set from the game's own menu: shown as it is, and reading it writes nothing.
        assert_eq!(p.slider_now(&p.settings[0], &d).as_deref(), Some("1.37"));
        let _ = p.current(&d, &BTreeMap::new());
        let _ = p.kept(&d);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), text);
        // Moved: on a step, written with its format, the rest of the file as it was.
        assert_eq!(p.set_slider(&d, "sens", 2.4987).unwrap(), "2.50");
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "[Input]\nmouse_sensitivity = 2.50\ninvert = false\n");
        assert_eq!(p.slider_now(&p.settings[0], &d).as_deref(), Some("2.50"));
        // A quoted value stays quoted; past the end goes to the end.
        std::fs::write(&f, "[Input]\nmouse_sensitivity = \"1.0\"\n").unwrap();
        assert_eq!(p.set_slider(&d, "sens", 9.0).unwrap(), "5.00");
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "[Input]\nmouse_sensitivity = \"5.00\"\n");
        // Not in the file yet: the game's default shows.
        std::fs::write(&f, "[Input]\ninvert = false\n").unwrap();
        assert_eq!(p.slider_now(&p.settings[0], &d).as_deref(), Some("1.0"));
        let _ = std::fs::remove_dir_all(&d);
    }

    const KEYS: &str = r#"{"format":1,"config":[{"id":"main","format":"ini","file":[{"path":"{install}/input.ini"}]}],
        "settings":[{"id":"keys","label":"Keyboard & Mouse","group":"Controls","bindings":{"separator":"|","max":2,"actions":[
            {"label":"Cross","key":"key.cross"},{"label":"L2","key":"key.l2"},{"label":"Options","key":"key.options"},{"label":"Left Stick Up","key":"key.ls_up"}]},
            "defaults":{"key.cross":"Space","key.l2":"Mouse Right","key.options":"Escape|Return","key.ls_up":"W"}}]}"#;

    #[test]
    fn key_bindings_read_and_write_only_what_changed() {
        let d = tmp("bindings");
        let f = d.join("input.ini");
        let text = "[Keys]\nkey.cross=Space\nkey.l2=Mouse Right\nkey.options=Escape|Return\nkey.ls_up=W\n";
        std::fs::write(&f, text).unwrap();
        let p: Profile = serde_json::from_str(KEYS).unwrap();
        let st = &p.settings[0];
        // Two keys on one action: two chips, in their order.
        assert_eq!(p.bindings_now(st, &d)[2], vec!["Escape".to_string(), "Return".to_string()]);
        assert!(!p.bindings_changed(st, &d));
        // Reading writes nothing, and the same keys set again write nothing either.
        let _ = p.current(&d, &BTreeMap::new());
        assert_eq!(p.set_binding(&d, "keys", "key.options", &["Escape".into(), "Return".into()]).unwrap(), vec!["Escape", "Return"]);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), text);
        // Mouse 4 in place of Return: only that line changes.
        p.set_binding(&d, "keys", "key.options", &["Escape".into(), "Mouse 4".into()]).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "[Keys]\nkey.cross=Space\nkey.l2=Mouse Right\nkey.options=Escape|Mouse 4\nkey.ls_up=W\n");
        assert!(p.bindings_changed(&p.settings[0], &d));
        // One cleared: the rest is written.
        p.set_binding(&d, "keys", "key.options", &["Mouse 4".into()]).unwrap();
        assert!(std::fs::read_to_string(&f).unwrap().contains("key.options=Mouse 4\n"));
        // No more than "max" keys.
        assert_eq!(p.set_binding(&d, "keys", "key.cross", &["Space".into(), "E".into(), "F".into()]).unwrap(), vec!["Space", "E"]);
        // Reset to Default: back to the game's own keys.
        p.reset_bindings(&d, "keys").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), text);
        // An update keeps them.
        assert!(p.kept(&d).iter().any(|(_, k, v)| k == "key.options" && v == "Escape|Return"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn key_names_follow_the_file() {
        let b = Bindings { separator: "|".into(), max: 2, actions: vec![], names: BTreeMap::new() };
        // The file says Enter: Enter it is.
        assert_eq!(b.file_name("Return", &["Enter".into(), "W".into()]), "Enter");
        assert_eq!(b.file_name("Mouse 4", &["Space".into()]), "Mouse 4");
        assert_eq!(b.file_name("space", &["SPACE".into()]), "SPACE");
        // The profile's own name wins.
        let mut names = BTreeMap::new();
        names.insert("Enter".to_string(), "Return".to_string());
        names.insert("Mouse 4".to_string(), "MB4".to_string());
        let b = Bindings { names, ..b };
        assert_eq!(b.file_name("Return", &["Enter".into()]), "Return");
        assert_eq!(b.file_name("Mouse 4", &[]), "MB4");
        assert_eq!(b.split("\"Escape | Return\""), vec!["Escape", "Return"]);
        assert!(b.split("").is_empty());
    }

    #[test]
    fn key_bindings_keep_quotes_in_toml_and_json() {
        let d = tmp("bindings-q");
        let f = d.join("input.toml");
        std::fs::write(&f, "[keys]\ncross = \"Space\"\n").unwrap();
        let p: Profile = serde_json::from_str(&KEYS.replace("\"ini\"", "\"toml\"").replace("input.ini", "input.toml").replace("\"key.cross\"}", "\"keys/cross\"}")).unwrap();
        p.set_binding(&d, "keys", "keys/cross", &["Space".into(), "Return".into()]).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "[keys]\ncross = \"Space|Return\"\n");
        let j = d.join("input.json");
        std::fs::write(&j, "{\"keys\": {\"cross\": \"1\"}}").unwrap();
        let p: Profile = serde_json::from_str(&KEYS.replace("\"ini\"", "\"json\"").replace("input.ini", "input.json").replace("\"key.cross\"}", "\"keys/cross\"}")).unwrap();
        assert_eq!(p.bindings_now(&p.settings[0], &d)[0], vec!["1"]);
        p.set_binding(&d, "keys", "keys/cross", &["1".into(), "2".into()]).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&j).unwrap()).unwrap();
        assert_eq!(v["keys"]["cross"], "1|2");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_profile_without_bindings_is_read_as_before() {
        let d = tmp("nobind");
        std::fs::write(d.join("config.toml"), "[Display]\nfullscreen = true\n").unwrap();
        let p: Profile = serde_json::from_str(r#"{"format":1,"config":[{"id":"main","format":"toml","file":[{"path":"{install}/config.toml"}]}],
            "settings":[{"id":"fs","label":"Display","choices":[{"label":"Window","set":{"Display/fullscreen":"false"}},{"label":"Full Screen","set":{"Display/fullscreen":"true"}}]}]}"#).unwrap();
        assert!(p.settings[0].bindings.is_none());
        assert_eq!(p.current(&d, &BTreeMap::new())[0].0, 1);
        assert!(p.bindings_now(&p.settings[0], &d).is_empty());
        assert_eq!(p.own_value(&p.settings[0], &d), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn settings_changed_in_the_game_show_when_read_again() {
        let d = tmp("reread");
        let f = d.join("config.toml");
        std::fs::write(&f, "[Display]\nscale = 1\n").unwrap();
        let p: Profile = serde_json::from_str(r#"{"format":1,"config":[{"id":"main","format":"toml","file":[{"path":"{install}/config.toml"}]}],
            "settings":[{"id":"scale","label":"Scale","choices":[{"label":"1x","set":{"Display/scale":"1"}},{"label":"2x","set":{"Display/scale":"2"}}]}]}"#).unwrap();
        assert_eq!(p.current(&d, &BTreeMap::new())[0].0, 0);
        // The game changed it in its own menu while it ran.
        std::fs::write(&f, "[Display]\nscale = 2\n").unwrap();
        assert_eq!(p.current(&d, &BTreeMap::new())[0].0, 1);
        // A value that isn't one of the choices: shown as it is, never rewritten.
        std::fs::write(&f, "[Display]\nscale = 1.5\n").unwrap();
        assert_eq!(p.own_value(&p.settings[0], &d).as_deref(), Some("1.5"));
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "[Display]\nscale = 1.5\n");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn match_my_screen_is_offered_in_the_settings_format() {
        let d = tmp("screen");
        let f = d.join("config.toml");
        std::fs::write(&f, "[Video]\nwidth = 1280\nheight = 720\nres = \"1280x720\"\naspect = 1.777\nmode = \"stretch\"\nshape = \"16:9\"\n").unwrap();
        let mut p: Profile = serde_json::from_str(r#"{"format":1,"config":[{"id":"main","format":"toml","file":[{"path":"{install}/config.toml"}]}],
            "settings":[
              {"id":"size","label":"Resolution","choices":[{"label":"720p","set":{"Video/width":"1280","Video/height":"720"}},{"label":"1080p","set":{"Video/width":"1920","Video/height":"1080"}}]},
              {"id":"res","label":"Resolution","choices":[{"label":"720p","set":{"Video/res":"\"1280x720\""}},{"label":"1080p","set":{"Video/res":"\"1920x1080\""}}]},
              {"id":"aspect","label":"Aspect ratio","choices":[{"label":"16:9","set":{"Video/aspect":"1.777"}},{"label":"4:3","set":{"Video/aspect":"1.333"}}]},
              {"id":"mode","label":"Widescreen","choices":[{"label":"Stretch","set":{"Video/mode":"stretch"}},{"label":"Hor+","set":{"Video/mode":"hor+"}}]},
              {"id":"shape","label":"Screen","choices":[{"label":"16:9","set":{"Video/shape":"\"16:9\""}},{"label":"4:3","set":{"Video/shape":"\"4:3\""}}]},
              {"id":"scale","label":"UI scale","choices":[{"label":"Small","set":{"Video/scale":"1.25"}},{"label":"Big","set":{"Video/scale":"1.5"}}]}
            ]}"#).unwrap();
        p.add_screen_choices(Some((3440, 1440)));
        let last = |i: usize| p.settings[i].choices.last().unwrap().clone();
        assert_eq!(last(0).label, "Match my screen \u{b7} 3440\u{d7}1440");
        assert_eq!(last(0).set, BTreeMap::from([("Video/width".to_string(), "3440".to_string()), ("Video/height".to_string(), "1440".to_string())]));
        assert_eq!(last(1).set["Video/res"], "\"3440x1440\"");
        assert_eq!(last(2).set["Video/aspect"], "2.389");
        assert_eq!(p.settings[2].screen.as_deref(), Some("21:9"));
        assert_eq!(last(4).set["Video/shape"], "\"21:9\"");
        // Named choices, and numbers that aren't a shape: nothing added.
        assert_eq!(p.settings[3].choices.len(), 2);
        assert_eq!(p.settings[5].choices.len(), 2);
        // Picked: written in the same format; on another monitor, written again before Play.
        p.pick(&d, "res", 2).unwrap();
        p.pick(&d, "aspect", 2).unwrap();
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(text.contains("res = \"3440x1440\""), "{text}");
        assert_eq!(p.current(&d, &BTreeMap::new())[1].0, 2);
        let mut q: Profile = p.clone();
        q.settings.iter_mut().for_each(|s| { if s.screen.is_some() { s.choices.pop(); s.screen = None; } });
        q.add_screen_choices(Some((5120, 1440)));
        assert_eq!(q.follow_screen(&d, &["res".to_string(), "aspect".to_string()]), vec!["res".to_string(), "aspect".to_string()]);
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(text.contains("res = \"5120x1440\"") && text.contains("aspect = 3.556"), "{text}");
        // Nothing changed: nothing written.
        assert!(q.follow_screen(&d, &["res".to_string()]).is_empty());
        assert_eq!(shape_name(1920, 1200), "16:10");
        assert_eq!(shape_name(2560, 1080), "21:9");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_folder_with_no_game_in_it_is_refused() {
        let d = tmp("nogamefolder");
        let need = GameCopy { kind: "folder".into(), ..Default::default() };
        let empty = d.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(check_copy(&need, &empty).unwrap_err().contains("empty"));
        // The launcher's screenshots: pictures only.
        let shots = d.join("Pictures/RecompLauncher/Some Game");
        std::fs::create_dir_all(&shots).unwrap();
        std::fs::write(shots.join("shot 1.png"), b"png").unwrap();
        std::fs::write(d.join("Pictures/RecompLauncher/desktop.ini"), b"[x]").unwrap();
        assert!(check_copy(&need, &d.join("Pictures/RecompLauncher")).unwrap_err().contains("pictures"));
        let game = d.join("game");
        std::fs::create_dir_all(game.join("us")).unwrap();
        std::fs::write(game.join("default.xex"), b"XEX2").unwrap();
        assert!(check_copy(&need, &game).is_ok());
        // Deeper than it looks: let through.
        std::fs::create_dir_all(d.join("deep/a/b/c/d/e")).unwrap();
        assert!(check_copy(&need, &d.join("deep")).is_ok());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn copies_that_cant_be_the_game_are_refused() {
        let d = tmp("notgame");
        let jpg = d.join("cover.jpg");
        std::fs::write(&jpg, b"\xff\xd8\xff\xe0 not a rom").unwrap();
        let n64 = GameCopy { kind: "iso".into(), name: "game.z64".into(), ..Default::default() };
        assert!(check_copy(&n64, &jpg).is_err());
        let bin = d.join("game.bin");
        std::fs::write(&bin, vec![0u8; 4096]).unwrap();
        assert!(check_copy(&n64, &bin).unwrap_err().contains("N64"));
        let disc = GameCopy { kind: "iso".into(), label: "Your disc image (.iso)".into(), ..Default::default() };
        let nkit = d.join("Game.nkit.iso");
        std::fs::write(&nkit, vec![0u8; 4096]).unwrap();
        assert!(check_copy(&disc, &nkit).unwrap_err().contains("nkit"));
        let mut b = vec![0u8; 0x400];
        b[0x200..0x204].copy_from_slice(b"NKIT");
        let hidden = d.join("Game.iso");
        std::fs::write(&hidden, &b).unwrap();
        assert!(check_copy(&disc, &hidden).is_err());
        std::fs::write(&hidden, vec![0u8; 0x400]).unwrap();
        assert!(check_copy(&disc, &hidden).is_ok());
        // A SNES ROM of an odd size is said, not refused.
        let sfc = d.join("game.smc");
        std::fs::write(&sfc, vec![0u8; 541_184]).unwrap();
        assert!(odd_snes_size(&sfc));
        std::fs::write(&sfc, vec![0u8; 524_288 + 512]).unwrap();
        assert!(!odd_snes_size(&sfc));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn numbers_are_written_printf_style() {
        assert_eq!(printf("%.2f", 2.5), "2.50");
        assert_eq!(printf("%d", 94.6), "95");
        assert_eq!(printf("%g", 0.30000000000000004), "0.3");
        assert_eq!(printf("%g", 1.0), "1");
        assert_eq!(printf("%g", 1500000.0), "1.5e+06");
        assert_eq!(printf("%.1fx", 1.25), "1.2x");
        let sl = Slider { key: "k".into(), min: 0.1, max: 5.0, step: 0.05, ..Default::default() };
        assert_eq!(sl.text(sl.snap(0.1 + 0.2)), "0.3");
        assert_eq!(sl.text(sl.snap(-3.0)), "0.1");
    }

    #[test]
    fn a_cue_picked_for_a_named_bin_places_its_track() {
        let d = tmp("cuebin");
        let disc = d.join("disc");
        std::fs::create_dir_all(&disc).unwrap();
        std::fs::write(disc.join("CTR (USA).bin"), vec![7u8; 4096]).unwrap();
        std::fs::write(disc.join("CTR (USA).cue"), "FILE \"CTR (USA).bin\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\n").unwrap();
        let install = d.join("game");
        std::fs::create_dir_all(&install).unwrap();
        let b: Value = serde_json::from_str(r#"{"start":"ctr.exe","files":{"how":"folder","folder":"assets","accepts":"iso","contains":["ctr-u.bin"],"what":"Your USA Crash Team Racing disc (.bin)"}}"#).unwrap();
        let p = from_boot(&b).unwrap();
        p.place(&install, Some(&disc.join("CTR (USA).cue"))).unwrap();
        assert_eq!(std::fs::metadata(install.join("assets/ctr-u.bin")).unwrap().len(), 4096);
        assert!(!install.join("assets/CTR (USA).cue").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn empty_folders_from_the_release_get_filled() {
        let d = tmp("emptyfold");
        let copy = d.join("dump");
        std::fs::create_dir_all(copy.join("exefs")).unwrap();
        std::fs::create_dir_all(copy.join("romfs")).unwrap();
        std::fs::write(copy.join("exefs/main"), b"x").unwrap();
        std::fs::write(copy.join("romfs/data.arc"), b"x").unwrap();
        let install = d.join("game");
        std::fs::create_dir_all(install.join("targets/exefs")).unwrap();
        std::fs::create_dir_all(install.join("targets/romfs")).unwrap();
        let b: Value = serde_json::from_str(r#"{"start":"build/xcde_launcher.exe","files":{"how":"folder","folder":"targets","accepts":"folder","contains":["exefs","romfs"]}}"#).unwrap();
        let p = from_boot(&b).unwrap();
        assert!(p.already_in_place(&install).is_none());
        assert_eq!(p.missing_after_place(&install).len(), 2);
        p.place(&install, Some(&copy)).unwrap();
        assert!(install.join("targets/exefs/main").is_file() && install.join("targets/romfs/data.arc").is_file());
        assert!(p.missing_after_place(&install).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn boot_profiles_become_easy_setup() {
        let b: Value = serde_json::from_str(r#"{"start":"fable2.exe","files":{"how":"flag","flag":"--game_data_root","accepts":"folder","contains":["default.xex"],"what":"Xbox 360 disc, extracted"}}"#).unwrap();
        let p = from_boot(&b).unwrap();
        let g = p.game.as_ref().unwrap();
        assert_eq!((g.kind.as_str(), g.check.file.as_str(), g.generic), ("folder", "default.xex", true));
        assert_eq!(p.start.as_ref().unwrap().args, vec!["--game_data_root", "{game}"]);
        // Into a folder: the copy's files go there as links.
        let d = tmp("place");
        let copy = d.join("my copy");
        std::fs::create_dir_all(copy.join("Media")).unwrap();
        std::fs::write(copy.join("default.xbe"), b"x").unwrap();
        let install = d.join("game");
        std::fs::create_dir_all(install.join("assets")).unwrap();
        std::fs::write(install.join("assets/README.txt"), b"put files here").unwrap();
        let b: Value = serde_json::from_str(r#"{"start":"SCDA.exe","files":{"how":"folder","folder":"assets","accepts":"folder or iso","contains":["default.xbe","Media"]}}"#).unwrap();
        let p = from_boot(&b).unwrap();
        assert_eq!(p.game.as_ref().unwrap().kind, "iso_or_folder");
        // An Xbox disc image picked for it is unpacked into the folder, and checked to be this game.
        assert_eq!(p.game.as_ref().unwrap().extract, "xdvdfs");
        assert_eq!(p.game.as_ref().unwrap().check.xdvdfs, "default.xbe");
        p.place(&install, Some(&copy)).unwrap();
        assert!(install.join("assets/default.xbe").is_file());
        assert!(install.join("assets/Media").is_dir());
        p.place(&install, Some(&copy)).unwrap();
        // A ROM goes in under the name the game reads.
        let rom = d.join("Shadows (USA).z64");
        std::fs::write(&rom, b"rom").unwrap();
        let b: Value = serde_json::from_str(r#"{"start":"sote.exe","files":{"how":"folder","folder":".","accepts":"file","contains":["sote.us.v1.2.z64"]}}"#).unwrap();
        from_boot(&b).unwrap().place(&install, Some(&rom)).unwrap();
        assert_eq!(std::fs::read(install.join("sote.us.v1.2.z64")).unwrap(), b"rom");
        // Nothing to do for a game that asks for its copy itself.
        assert!(from_boot(&serde_json::from_str(r#"{"start":"x.exe","files":{"how":"self"}}"#).unwrap()).is_none());
    }

    #[test]
    fn built_in_profiles_load() {
        let f = find(Path::new("/nope"), "HossEz/fifa-world-cup-2014-recompiled", &Value::Null).unwrap();
        assert!(f.settings.iter().any(|s| s.id == "resolution"));
        let s = find(Path::new("/nope"), "luckvj/streetball-revival", &Value::Null).unwrap();
        assert_eq!(s.game.unwrap().check.iso9660, "SLUS_206.51");
    }

    #[test]
    fn dusklight_official_package_uses_the_launcher_profile() {
        let install = tmp("dusklight");
        let program = if cfg!(windows) {
            "dusklight.exe"
        } else if cfg!(target_os = "macos") {
            "Dusklight.app/Contents/MacOS/Dusklight"
        } else {
            "Dusklight-v2.0.3-linux-x86_64.AppImage"
        };
        let executable = install.join(program);
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::write(&executable, b"").unwrap();
        let disc = install.join("Twilight Princess.rvz");
        std::fs::write(&disc, b"").unwrap();
        let profile = find(&install, "twilitrealm/dusklight", &Value::Null).unwrap();
        check_copy(profile.game.as_ref().unwrap(), &disc).unwrap();
        let picks = BTreeMap::new();
        let (exe, args) = profile.command(&install, Some(&disc), &picks).unwrap();
        assert_eq!(exe, executable);
        assert_eq!(args, vec!["--dvd".to_string(), disc.to_string_lossy().into_owned()]);

        let data = install.join("data");
        std::fs::create_dir(&data).unwrap();
        std::fs::write(data.join("config.json"), "{}").unwrap();
        let cap = profile.settings.iter().position(|s| s.id == "fps_cap").unwrap();
        assert!(!profile.current(&install, &picks)[cap].1);
        profile.pick(&install, "fps", 1).unwrap();
        assert!(profile.current(&install, &picks)[cap].1);
        profile.pick(&install, "fps_cap", 2).unwrap();
        assert_eq!(profile.config_file("main", &install).unwrap().get("video.maxFrameRate").as_deref(), Some("90"));

        let user_root = "{appdata}";
        assert!(profile.saves.contains(&format!("{user_root}/TwilitRealm/Dusk/states.json")));
        let game = crate::library::Installed { path: install.clone(), ..Default::default() };
        let portable: Vec<_> = profile.saves.into_iter().filter(|path| path.starts_with("{install}/")).collect();
        let spots = crate::saves::spots(&portable, &[], &game);
        let card = data.join("MemoryCardA.USA.raw");
        let states = data.join("states.json");
        std::fs::write(&card, b"card").unwrap();
        std::fs::write(&states, b"[]").unwrap();
        let app = install.join("launcher-data");
        let backup = crate::saves::backup(&app, "dusklight", &spots, &game).unwrap().unwrap();
        assert_eq!(backup.files, 2);
        std::fs::remove_file(&card).unwrap();
        std::fs::remove_file(&states).unwrap();
        let archive = crate::saves::dir_of(&app, "dusklight").join(backup.name);
        assert_eq!(crate::saves::restore(&app, "dusklight", &archive, &spots, &game).unwrap(), 2);
        assert_eq!(std::fs::read(card).unwrap(), b"card");
        assert_eq!(std::fs::read(states).unwrap(), b"[]");
        let _ = std::fs::remove_dir_all(&install);
    }

    #[test]
    fn sections_and_top_level_keys() {
        let d = tmp("sections");
        let p = d.join("config.ini");
        std::fs::write(&p, "[General]\nAutosave = 0\n\n[Graphics]\n# Fullscreen mode\nFullscreen = 0\n\n[KeyMap]\nFullscreen = Alt+Return\n").unwrap();
        let mut f = File::open(p.clone(), "ini");
        assert_eq!(f.get("Graphics/Fullscreen").as_deref(), Some("0"));
        assert_eq!(f.get("KeyMap/Fullscreen").as_deref(), Some("Alt+Return"));
        assert_eq!(f.get("graphics/Fullscreen").as_deref(), Some("0"));
        f.set("Graphics/Fullscreen", "1");
        f.set("Graphics/WindowScale", "2");
        f.set("Sound/EnableAudio", "1");
        f.save().unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "[General]\nAutosave = 0\n\n[Graphics]\n# Fullscreen mode\nFullscreen = 1\nWindowScale = 2\n\n[KeyMap]\nFullscreen = Alt+Return\n\n[Sound]\nEnableAudio = 1\n");
        // A key that's written out but commented is switched on where it is.
        let v = d.join("vh.ini");
        std::fs::write(&v, "[video]\n; Window scale\n;VH_SCALE=2\n\n[audio]\n").unwrap();
        let mut f = File::open(v.clone(), "ini");
        assert_eq!(f.get("video/VH_SCALE"), None);
        f.set("video/VH_SCALE", "4");
        f.save().unwrap();
        assert_eq!(std::fs::read_to_string(&v).unwrap(), "[video]\n; Window scale\nVH_SCALE=4\n\n[audio]\n");
        // A file written `key=value` gets new keys the same way.
        let n = d.join("tight.ini");
        std::fs::write(&n, "resScale=1\n").unwrap();
        let mut f = File::open(n.clone(), "ini");
        f.set("aniso", "4");
        f.save().unwrap();
        assert_eq!(std::fs::read_to_string(&n).unwrap(), "resScale=1\naniso=4\n");
        // JSON: keys inside objects too.
        let j = d.join("s.json");
        std::fs::write(&j, r#"{"CVars":{"gRes":1,"x":true},"top":2}"#).unwrap();
        let mut f = File::open(j.clone(), "json");
        assert_eq!(f.get("CVars/gRes").as_deref(), Some("1"));
        f.set("CVars/gRes", "2");
        f.set("Window/Fullscreen", "true");
        f.save().unwrap();
        let back: Value = serde_json::from_str(&std::fs::read_to_string(&j).unwrap()).unwrap();
        assert_eq!((back["CVars"]["gRes"].as_i64(), back["CVars"]["x"].as_bool(), back["Window"]["Fullscreen"].as_bool(), back["top"].as_i64()), (Some(2), Some(true), Some(true), Some(2)));
        // A new top-level TOML key goes before the first table, not into it.
        let t = d.join("game.toml");
        std::fs::write(&t, "fullscreen = true\n\n[video]\nvsync = true\n").unwrap();
        let mut f = File::open(t.clone(), "toml");
        f.set("frame_rate", "\"60\"");
        f.set("video/vsync", "false");
        f.save().unwrap();
        assert_eq!(std::fs::read_to_string(&t).unwrap(), "fullscreen = true\nframe_rate = \"60\"\n\n[video]\nvsync = false\n");
    }

    #[test]
    fn boot_settings_only_change_what_is_there() {
        let b: Value = serde_json::from_str(r#"{"start":"game.exe","files":{"how":"self"},"settings":{
            "config":[{"id":"main","format":"toml","file":[{"path":"{install}/game.toml"}],"always":{"evil":"1"}},{"id":"bad","format":"toml","file":[{"path":"C:/Windows/win.ini"}]}],
            "settings":[
              {"id":"fs","label":"Full screen","choices":[{"label":"Off","set":{"fullscreen":"false"}},{"label":"On","set":{"fullscreen":"true"}}],"defaults":{"fullscreen":"false"}},
              {"id":"x","label":"Nowhere","in":"bad","choices":[{"label":"A","set":{"a":"1"}},{"label":"B","set":{"a":"2"}}]},
              {"id":"w","label":"Window","in":"args","choices":[{"label":"Game's choice","args":[]},{"label":"Windowed","args":["--windowed"]}]}
            ]}}"#).unwrap();
        let p = from_boot(&b).unwrap();
        assert!(p.from_boot && p.start.is_none() && p.game.is_none());
        assert_eq!(p.settings.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), vec!["fs", "w"]);
        assert_eq!(p.config.len(), 1);
        assert!(p.config[0].always.is_empty());
        let d = tmp("bootset");
        // Before the game made its file: nothing is written.
        assert!(p.waiting_for_files(&d));
        assert!(p.pick(&d, "fs", 1).is_err());
        assert!(!d.join("game.toml").exists());
        std::fs::write(d.join("game.toml"), "fullscreen = false\nlog = 1\n").unwrap();
        assert!(!p.waiting_for_files(&d));
        p.pick(&d, "fs", 1).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("game.toml")).unwrap(), "fullscreen = true\nlog = 1\n");
        assert!(same("1.0", "1") && same("\"4x MSAA\"", "\"4x msaa\"") && !same("2", "1"));
        let mut picks = BTreeMap::new();
        picks.insert("w".to_string(), 1usize);
        assert_eq!(p.setting_args(&d, &picks), vec!["--windowed"]);
        // An update's file comes with its defaults; the pick goes back in.
        let kept = p.kept(&d);
        std::fs::write(d.join("game.toml"), "fullscreen = false\nlog = 1\nnew = 2\n").unwrap();
        p.restore(&d, &kept);
        assert_eq!(std::fs::read_to_string(d.join("game.toml")).unwrap(), "fullscreen = true\nlog = 1\nnew = 2\n");
        // A copy and settings together.
        let b: Value = serde_json::from_str(r#"{"start":"g.exe","files":{"how":"flag","flag":"--game","accepts":"folder"},"settings":{"config":[],"settings":[{"id":"w","label":"W","in":"args","choices":[{"label":"A","args":[]},{"label":"B","args":["-w"]}]}]}}"#).unwrap();
        let p = from_boot(&b).unwrap();
        assert!(p.game.is_some() && p.start.is_some() && p.settings.len() == 1);
    }

    #[test]
    fn rom_region_and_disc_checks() {
        let d = tmp("region");
        let rom = d.join("ge.z64");
        let mut h = vec![0u8; 64];
        h[..4].copy_from_slice(&[0x80, 0x37, 0x12, 0x40]);
        h[0x3E] = b'P';
        std::fs::write(&rom, &h).unwrap();
        let b: Value = serde_json::from_str(r#"{"start":"ge.exe","files":{"how":"folder","folder":"data","accepts":"file","contains":["ge007.ntsc-final.z64"],"region":"E"}}"#).unwrap();
        let g = from_boot(&b).unwrap().game.unwrap();
        assert_eq!(check_copy(&g, &rom).unwrap_err(), "That ROM isn't the USA version, which this recomp needs.");
        // The same ROM byte-swapped (.v64), from the USA.
        let mut v = h.clone();
        for c in v.chunks_mut(2) {
            c.swap(0, 1);
        }
        v[0x3F] = b'E';
        std::fs::write(&rom, &v).unwrap();
        assert!(check_copy(&g, &rom).is_ok());
        // An Xbox 360 disc image read as it is: one with no Xbox file system is turned down.
        let b: Value = serde_json::from_str(r#"{"start":"svr.exe","files":{"how":"folder","folder":".","accepts":"iso","check_image":"xdvdfs"}}"#).unwrap();
        let g = from_boot(&b).unwrap().game.unwrap();
        let iso = d.join("bad.iso");
        std::fs::write(&iso, vec![0u8; 100_000]).unwrap();
        assert!(check_copy(&g, &iso).unwrap_err().starts_with("That disc image can't be read"));
    }

    #[test]
    fn cue_brings_its_tracks() {
        let d = tmp("cue");
        std::fs::write(d.join("TS (USA).cue"), "FILE \"TS (USA).bin\" BINARY\n  TRACK 01 MODE2/2352\nFILE \"../evil.bin\" BINARY\n").unwrap();
        std::fs::write(d.join("TS (USA).bin"), b"disc").unwrap();
        let b: Value = serde_json::from_str(r#"{"start":"ts.exe","files":{"how":"folder","folder":".","accepts":"iso"}}"#).unwrap();
        let p = from_boot(&b).unwrap();
        let install = d.join("game");
        std::fs::create_dir_all(&install).unwrap();
        p.place(&install, Some(&d.join("TS (USA).cue"))).unwrap();
        assert!(install.join("TS (USA).cue").is_file() && install.join("TS (USA).bin").is_file());
    }

    #[test]
    fn rom_game_version_and_byte_order() {
        let d = tmp("n64id");
        let mut h = vec![0u8; 128];
        h[..4].copy_from_slice(&[0x80, 0x37, 0x12, 0x40]);
        h[0x3B] = b'N';
        h[0x3C] = b'P';
        h[0x3D] = b'D';
        h[0x3E] = b'E';
        h[0x3F] = 0;
        h[100] = 0xAB;
        let rom = d.join("pd.z64");
        std::fs::write(&rom, &h).unwrap();
        let b: Value = serde_json::from_str(r#"{"start":"pd.exe","files":{"how":"folder","folder":"data","accepts":"file","contains":["pd.ntsc-final.z64"],"region":"E","n64_id":"PD","n64_rev":1}}"#).unwrap();
        let p = from_boot(&b).unwrap();
        let g = p.game.clone().unwrap();
        assert!(check_copy(&g, &rom).unwrap_err().contains("different version"));
        h[0x3F] = 1;
        std::fs::write(&rom, &h).unwrap();
        assert!(check_copy(&g, &rom).is_ok());
        h[0x3C] = b'G';
        h[0x3D] = b'E';
        std::fs::write(&rom, &h).unwrap();
        assert_eq!(check_copy(&g, &rom).unwrap_err(), "That ROM is a different game.");
        // A .v64 placed as .z64 comes out in .z64 order.
        h[0x3C] = b'P';
        h[0x3D] = b'D';
        let mut v = h.clone();
        v.chunks_mut(2).for_each(|c| c.swap(0, 1));
        let vrom = d.join("pd.v64");
        std::fs::write(&vrom, &v).unwrap();
        assert!(check_copy(&g, &vrom).is_ok());
        let install = d.join("game");
        std::fs::create_dir_all(&install).unwrap();
        p.place(&install, Some(&vrom)).unwrap();
        assert_eq!(std::fs::read(install.join("data/pd.ntsc-final.z64")).unwrap(), h);
    }

    #[test]
    fn shipped_settings_all_load() {
        let all: Value = serde_json::from_str(include_str!("../boot/boot-profiles.json")).unwrap();
        let mut n = 0;
        for (id, b) in all.as_object().unwrap() {
            let Some(want) = b.get("settings").and_then(|s| s.get("settings")).and_then(Value::as_array) else { continue };
            let p = from_boot(b).unwrap_or_else(|| panic!("{id}"));
            assert_eq!(p.settings.len(), want.len(), "{id}");
            for st in &p.settings {
                assert!(st.target == "args" || p.config.iter().any(|c| c.id == st.target), "{id} {}", st.id);
            }
            n += 1;
        }
        assert!(n >= 30, "{n}");
    }

    #[test]
    fn toml_edits_keep_the_rest() {
        let d = tmp("toml");
        let p = d.join("a.toml");
        std::fs::write(&p, "# comment\nfullscreen = true # mine\nname = \"x y\"\n").unwrap();
        let mut f = File::open(p.clone(), "toml");
        assert_eq!(f.get("fullscreen").as_deref(), Some("true"));
        assert_eq!(f.get("name").as_deref(), Some("\"x y\""));
        f.set("fullscreen", "false");
        f.set("new_key", "3");
        f.save().unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "# comment\nfullscreen = false # mine\nname = \"x y\"\nnew_key = 3\n");
    }

    #[test]
    fn fifa_setup_and_settings() {
        let d = tmp("fifa");
        std::fs::write(d.join("portable.txt"), "").unwrap();
        std::fs::write(d.join("fifawc14.exe"), "").unwrap();
        let p = find(&d, "HossEz/fifa-world-cup-2014-recompiled", &Value::Null).unwrap();
        p.prepare(&d, Some(Path::new("C:\\Games\\FIFA 14.iso"))).unwrap();
        let t = std::fs::read_to_string(d.join("fifawc14.toml")).unwrap();
        assert!(t.contains("game_data_root = \"C:/Games/FIFA 14.iso\""), "{t}");
        assert!(t.contains("gpu_plugin = \"xenos\""));
        assert!(t.contains("frame_rate_floor = 60"));
        let picks = BTreeMap::new();
        let cur = p.current(&d, &picks);
        let res = p.settings.iter().position(|s| s.id == "resolution").unwrap();
        assert_eq!(cur[res].0, 0);
        p.pick(&d, "resolution", 1).unwrap();
        assert_eq!(p.current(&d, &picks)[res].0, 1);
        // Window size only applies in a window.
        let size = p.settings.iter().position(|s| s.id == "window_size").unwrap();
        assert!(!p.current(&d, &picks)[size].1);
        p.pick(&d, "window", 2).unwrap();
        assert!(p.current(&d, &picks)[size].1);
        let (exe, args) = p.command(&d, None, &picks).unwrap();
        assert!(exe.ends_with("fifawc14.exe"));
        assert_eq!(args, vec!["--from_launcher"]);
    }

    #[test]
    fn streetball_command() {
        let d = tmp("sb");
        std::fs::create_dir_all(d.join("bin")).unwrap();
        std::fs::write(d.join("bin").join(if cfg!(windows) { "street-game.exe" } else { "street-game" }), "").unwrap();
        let p = find(&d, "luckvj/streetball-revival", &Value::Null).unwrap();
        let mut picks = BTreeMap::new();
        picks.insert("intro".to_string(), 1usize);
        let (exe, args) = p.command(&d, Some(Path::new("/isos/nbas2.iso")), &picks).unwrap();
        assert!(exe.parent().unwrap().ends_with("bin"));
        assert_eq!(args, vec!["--iso", "/isos/nbas2.iso", "--no-intro"]);
    }

    #[test]
    fn disc_checks() {
        let d = tmp("iso");
        // A tiny ISO 9660 image with SLUS_206.51 in its top folder.
        let mut img = vec![0u8; 20 * 2048];
        img[16 * 2048 + 1..16 * 2048 + 6].copy_from_slice(b"CD001");
        let root = 16 * 2048 + 156;
        img[root + 2..root + 6].copy_from_slice(&18u32.to_le_bytes());
        img[root + 10..root + 14].copy_from_slice(&2048u32.to_le_bytes());
        let name = b"SLUS_206.51;1";
        let e = 18 * 2048;
        img[e] = (33 + name.len()) as u8;
        img[e + 32] = name.len() as u8;
        img[e + 33..e + 33 + name.len()].copy_from_slice(name);
        let iso = d.join("game.iso");
        std::fs::write(&iso, &img).unwrap();
        let g = GameCopy { kind: "iso".into(), label: String::new(), check: Check { iso9660: "SLUS_206.51".into(), ..Default::default() }, ..Default::default() };
        assert!(check_copy(&g, &iso).is_ok());
        let g2 = GameCopy { check: Check { iso9660: "SLUS_999.99".into(), ..Default::default() }, ..g.clone() };
        assert!(check_copy(&g2, &iso).is_err());
        assert!(check_copy(&g, &d).is_err());
    }

    #[test]
    fn nup_build_id_checks_a_package_or_folder_by_content() {
        let d = tmp("nupcheck");
        let mut main = vec![0u8; 0x100];
        main[..4].copy_from_slice(b"NSO0");
        main[0x40..0x44].copy_from_slice(&[0xe0, 0xf3, 0xcc, 0xfa]);
        std::fs::create_dir_all(d.join("dump").join("exefs")).unwrap();
        std::fs::write(d.join("dump").join("exefs").join("main"), &main).unwrap();
        let g = |id: &str| GameCopy {
            kind: "iso_or_folder".into(),
            label: String::new(),
            check: Check { nup_build_id: id.into(), ..Default::default() },
            ..Default::default()
        };
        assert!(check_copy(&g("e0f3ccfa"), &d.join("dump")).is_ok());
        let e = check_copy(&g("11223344"), &d.join("dump")).unwrap_err();
        assert!(e.contains("e0f3ccfa") && e.contains("11223344"), "{e}");
        assert!(check_copy(&g("e0f3ccfa"), &d).unwrap_err().contains("exefs/main"));
        // A file is judged by its content, whatever it is called.
        std::fs::write(d.join("a.nup"), b"just text").unwrap();
        assert!(check_copy(&g("e0f3ccfa"), &d.join("a.nup")).unwrap_err().contains("isn't a Nintendo Switch 2"));
        assert!(g("e0f3ccfa").identifies_by_contents() && g("e0f3ccfa").identifies());
        // No check, no change: any file is still accepted.
        assert!(check_copy(&g(""), &d.join("a.nup")).is_ok());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn start_env_fills_in_and_keeps_out_what_it_must() {
        let p: Profile = serde_json::from_str(r#"{"format":1,"start":{"program":["g.exe"],"env":{"SDL_GAMECONTROLLER_USE_BUTTON_LABELS":"0","GAME_DATA":"{install}/data","PATH":"x","RECOMPLAUNCHER_MODE":"y","1BAD":"z"}}}"#).unwrap();
        let e = p.env_vars(Path::new("/games/x"), None);
        assert!(e.contains(&("SDL_GAMECONTROLLER_USE_BUTTON_LABELS".to_string(), "0".to_string())));
        assert!(e.iter().any(|(k, v)| k == "GAME_DATA" && v.ends_with("data") && v.contains("games")));
        assert_eq!(e.len(), 2, "PATH, the launcher's own and bad names stay out: {e:?}");
        assert!(env_name_ok("_X1") && !env_name_ok("A-B") && !env_name_ok("path"));
    }
}

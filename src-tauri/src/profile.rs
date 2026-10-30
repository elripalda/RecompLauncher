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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Profile {
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
    /// An N64 ROM's region: the country letters it may have (`E` for USA).
    #[serde(default)]
    pub n64_region: String,
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
    pub choices: Vec<Choice>,
    /// The game's own values for keys the file doesn't have yet.
    #[serde(default)]
    pub defaults: BTreeMap<String, String>,
    /// Only when these keys have these values (otherwise shown greyed out).
    #[serde(default)]
    pub when: BTreeMap<String, String>,
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

/// None ship built in right now: each recomp's own developer adds easy setup to their release.
/// The two examples in docs/examples are still loaded by the tests.
#[cfg(not(test))]
const BUILT_IN: &[&str] = &[];
#[cfg(test)]
const BUILT_IN: &[&str] = &[include_str!("../../docs/examples/fifawc14.json"), include_str!("../../docs/examples/streetball.json")];

/// The profile for a game: the developer's own (in the game's folder), the catalog's, a built-in
/// one for its repo (none at the moment), or one made from the catalog's boot profile when that
/// says where the player's copy goes.
pub fn find(game_dir: &Path, repo: &str, entry: &Value) -> Option<Profile> {
    find_own(game_dir, repo, entry).or_else(|| crate::boot::raw(entry).and_then(|b| from_boot(&b)))
}

/// From the catalog's boot profile (crate::boot): the player's copy and how it reaches the game,
/// when it goes on the command line (`files.how: "flag"`) or into a folder (`"folder"`), and the
/// game's settings the launcher can show (`settings: {config, settings}`).
pub fn from_boot(b: &Value) -> Option<Profile> {
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
    let (config, settings) = boot_settings(b);
    let copy = boot_copy(b);
    if copy.is_none() && settings.is_empty() {
        return None;
    }
    let start = s(b, "start");
    Some(Profile {
        format: 1,
        start: copy.as_ref().map(|(_, args)| Start { program: vec![start], args: args.clone(), env: BTreeMap::new() }),
        game: copy.map(|(g, _)| g),
        config,
        settings,
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
        .filter(|st| st.choices.len() >= 2 && (st.target == "args" || config.iter().any(|c| c.id == st.target)))
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
    if start.is_empty() || !matches!(how.as_str(), "flag" | "folder") {
        return None;
    }
    let flag = s(f, "flag");
    let into = s(f, "folder");
    // A flag, or none: the copy as the first thing on the command line.
    if (how == "flag" && !flag.is_empty() && !flag.starts_with('-')) || (how == "folder" && (into.is_empty() || into.contains(".."))) {
        return None;
    }
    let accepts = s(f, "accepts").to_ascii_lowercase();
    let extract = match s(f, "extract").as_str() {
        x @ ("xdvdfs" | "iso9660") => x.to_string(),
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
            },
            into: if how == "folder" { into } else { String::new() },
            name,
            extract,
            generic: true,
            ..Default::default()
        },
        args,
    ))
}

fn find_own(game_dir: &Path, repo: &str, entry: &Value) -> Option<Profile> {
    let ok = |p: Profile| (p.format == 1).then_some(p);
    if game_dir.as_os_str().is_empty() {
        // Not installed: no folder to look in.
    } else if let Some(p) = std::fs::read(game_dir.join("recomplauncher.json")).ok().and_then(|b| serde_json::from_slice::<Profile>(&b).ok()).and_then(ok) {
        return Some(p);
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

/// Fills in {install}, {documents}, {local}, {config}, {home}, {game} and {pad}.
pub fn fill(t: &str, install: &Path, game: Option<&Path>) -> String {
    if !t.contains('{') {
        return t.to_string();
    }
    let s = |p: Option<PathBuf>| p.map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    t.replace("{pad}", &PAD.read().unwrap())
        .replace("{install}", &install.to_string_lossy())
        .replace("{documents}", &s(dirs::document_dir().or_else(|| dirs::home_dir().map(|h| h.join("Documents")))))
        .replace("{local}", &s(dirs::data_local_dir()))
        .replace("{config}", &s(dirs::config_dir()))
        .replace("{home}", &s(dirs::home_dir()))
        .replace("{game}", &game.map(|g| g.to_string_lossy().to_string()).unwrap_or_default())
}

/// The first place that applies.
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
        Some(File::open(place(&c.file, install)?, &c.format))
    }

    /// Before the game starts: its settings files get what it needs and where the player's copy is.
    pub fn prepare(&self, install: &Path, game: Option<&Path>) -> std::io::Result<()> {
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
        let items: Vec<PathBuf> = if copy.is_dir() { std::fs::read_dir(copy)?.flatten().map(|e| e.path()).collect() } else { vec![copy.to_path_buf()] };
        let single = !copy.is_dir();
        for from in items {
            let Some(name) = from.file_name() else { continue };
            let to = if single && !g.name.trim().is_empty() && !g.name.contains(['/', '\\']) { dir.join(g.name.trim()) } else { dir.join(name) };
            if to.symlink_metadata().is_ok() {
                continue;
            }
            if from.is_dir() {
                link_dir(&from, &to)?;
            } else if std::fs::hard_link(&from, &to).is_err() {
                std::fs::copy(&from, &to)?;
            }
        }
        Ok(())
    }

    pub fn env_vars(&self, install: &Path, game: Option<&Path>) -> Vec<(String, String)> {
        self.start.as_ref().map(|s| s.env.iter().filter(|(k, _)| env_name_ok(k)).map(|(k, v)| (k.clone(), fill(v, install, game))).collect()).unwrap_or_default()
    }

    pub fn command(&self, install: &Path, game: Option<&Path>, picks: &BTreeMap<String, usize>) -> Option<(PathBuf, Vec<String>)> {
        let s = self.start.as_ref()?;
        let exe = s.program.iter().map(|p| install.join(p)).find(|p| p.is_file())?;
        let mut args: Vec<String> = s.args.iter().map(|a| fill(a, install, game)).collect();
        for st in self.settings.iter().filter(|s| s.target == "args") {
            if let Some(c) = picks.get(&st.id).and_then(|i| st.choices.get(*i)) {
                args.extend(c.args.iter().map(|a| fill(a, install, game)));
            }
        }
        Some((exe, args))
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
            let mut keys: Vec<&String> = self.settings.iter().filter(|s| s.target == c.id).flat_map(|s| s.choices.iter().flat_map(|ch| ch.set.keys())).collect();
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

// ---------- the player's copy ----------

fn read_at(f: &mut std::fs::File, at: u64, n: usize) -> Option<Vec<u8>> {
    f.seek(SeekFrom::Start(at)).ok()?;
    let mut b = vec![0u8; n];
    f.read_exact(&mut b).ok()?;
    Some(b)
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
        use std::os::windows::process::CommandExt;
        let out = std::process::Command::new("cmd").arg("/c").arg("mklink").arg("/J").arg(to).arg(from).creation_flags(0x0800_0000).output()?;
        if !out.status.success() || !to.exists() {
            return Err(std::io::Error::other(format!("Couldn't link {} into the game's folder.", from.display())));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        std::os::unix::fs::symlink(from, to)
    }
}

/// Whether this is the player's copy the game wants. Err says why not.
pub fn check_copy(g: &GameCopy, p: &Path) -> Result<(), String> {
    let folder = p.is_dir();
    let kind_ok = match g.kind.as_str() {
        "folder" => folder,
        "iso_or_folder" => folder || p.is_file(),
        _ => p.is_file(),
    };
    if !kind_ok {
        return Err(if folder { "It needs the disc image (.iso), not a folder.".into() } else { "It needs the game's folder, not a file.".into() });
    }
    if folder {
        if !g.check.file.is_empty() && !p.join(&g.check.file).is_file() && !std::fs::read_dir(p).into_iter().flatten().flatten().any(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(&g.check.file)) {
            return Err(format!("That folder doesn't have {}.", g.check.file));
        }
        return Ok(());
    }
    if !g.check.iso9660.is_empty() && !iso9660_has(p, &g.check.iso9660) {
        return Err("That disc image isn't the release this recomp reads.".into());
    }
    if !g.check.xdvdfs.is_empty() && !xdvdfs_has(p, &g.check.xdvdfs) {
        return Err(if xdvdfs_part(p).is_none() {
            "That disc image can't be read: it may be incomplete, compressed or not an Xbox disc. Use a full, uncompressed .iso of your disc.".into()
        } else {
            "That disc image isn't the game this recomp reads.".into()
        });
    }
    if !g.check.n64_region.is_empty() {
        if let Some(c) = n64_country(p) {
            if !g.check.n64_region.contains(c) {
                return Err(if g.check.n64_region == "E" {
                    "That ROM isn't the USA version, which this recomp needs.".into()
                } else {
                    "That ROM is from a region this recomp doesn't read.".into()
                });
            }
        }
    }
    Ok(())
}

/// An N64 ROM's country letter (byte 0x3E), in any of the three byte orders ROMs come in.
fn n64_country(path: &Path) -> Option<char> {
    let mut f = std::fs::File::open(path).ok()?;
    let h = read_at(&mut f, 0, 64)?;
    let at = match &h[..4] {
        [0x80, 0x37, 0x12, 0x40] => 0x3E, // .z64, as the console reads it
        [0x37, 0x80, 0x40, 0x12] => 0x3F, // .v64, every two bytes swapped
        [0x40, 0x12, 0x37, 0x80] => 0x3D, // .n64, every four bytes reversed
        _ => return None,
    };
    let c = h[at] as char;
    c.is_ascii_uppercase().then_some(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rl-prof-{n}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
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
    fn start_env_fills_in_and_keeps_out_what_it_must() {
        let p: Profile = serde_json::from_str(r#"{"format":1,"start":{"program":["g.exe"],"env":{"SDL_GAMECONTROLLER_USE_BUTTON_LABELS":"0","GAME_DATA":"{install}/data","PATH":"x","RECOMPLAUNCHER_MODE":"y","1BAD":"z"}}}"#).unwrap();
        let e = p.env_vars(Path::new("/games/x"), None);
        assert!(e.contains(&("SDL_GAMECONTROLLER_USE_BUTTON_LABELS".to_string(), "0".to_string())));
        assert!(e.iter().any(|(k, v)| k == "GAME_DATA" && v.ends_with("data") && v.contains("games")));
        assert_eq!(e.len(), 2, "PATH, the launcher's own and bad names stay out: {e:?}");
        assert!(env_name_ok("_X1") && !env_name_ok("A-B") && !env_name_ok("path"));
    }
}

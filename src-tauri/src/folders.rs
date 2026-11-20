//! Folder Structure: the one folder layout the player designs for where games go on disk
//! (Settings → Games and folders → Folder Structure).
//!
//! This file is the pure part: the layout and its presets, what each field reads from the catalog
//! and the Library (metadata resolution), the fallback when a field has nothing, and turning all
//! of that into a relative folder path. Nothing here touches the disk except `load`/`save` of the
//! layout itself. Moving games that are already installed is folders_move.rs.
//!
//! What the launcher knows about a game, field by field (checked against the catalog and the
//! Library, not assumed):
//! - console: the catalog's `origin` (n64, ps2…) and its name in `origins`. Games the player added
//!   from GitHub have none.
//! - manufacturer: not in the catalog; worked out from the console by a fixed table below.
//! - game title: the catalog's `game`, else its title group's name, else what the Library noted.
//! - recompilation name: always there (the Library's name for it).
//! - recompilation year: the year of the catalog's `first` (its first release).
//! - recompilation team: the developer page's name, else the catalog's `author`, else the owner
//!   in its repo address.
//! - genre: the catalog's `genre`. Technology/type: a tag such as "decomp" or "port", when the
//!   catalog gives one.
//! - region: only for an N64 copy the player linked (read from the cartridge header).
//! - installation source, version, alphabetical letter: always there.
//! - franchise, original release year, original developer, publisher: the catalog has none of
//!   these today. They come only from the player's own per-game values (read from the catalog too,
//!   if it ever carries `franchise`, `year`, `developer` or `publisher`). Each has a fallback.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Every field a folder name can be made from, in the order the designer lists them.
pub const FIELDS: [&str; 17] = [
    "maker", "console", "game", "recomp", "franchise", "year", "recomp_year", "developer", "team", "publisher", "genre", "region", "tech", "source", "letter", "version", "text",
];

/// What happens when a field has nothing for a game.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Fallback {
    /// "text": use `text` instead. "skip": leave the field out (a folder level with nothing left
    /// in it is left out too). "field": use another field's value (`field`), and if that has
    /// nothing either, leave it out.
    pub mode: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub field: String,
}

impl Fallback {
    fn text(s: &str) -> Self {
        Fallback { mode: "text".into(), text: s.into(), field: String::new() }
    }
    fn skip() -> Self {
        Fallback { mode: "skip".into(), text: String::new(), field: String::new() }
    }
    fn field(f: &str) -> Self {
        Fallback { mode: "field".into(), text: String::new(), field: f.into() }
    }
}

/// The fallback a field starts with. Fields the catalog doesn't carry yet get a plain "Unknown …"
/// folder, so games without the value stay together and are easy to spot; franchise and region
/// are extras most games won't have, so they're left out instead of filling folders with
/// "Unknown".
pub fn default_fallback(field: &str) -> Fallback {
    match field {
        "maker" => Fallback::text("Other"),
        "console" => Fallback::text("Other"),
        "game" => Fallback::field("recomp"),
        "franchise" => Fallback::skip(),
        "year" | "recomp_year" => Fallback::text("Unknown Year"),
        "developer" => Fallback::text("Unknown Developer"),
        "team" => Fallback::text("Unknown Team"),
        "publisher" => Fallback::text("Unknown Publisher"),
        "genre" => Fallback::text("Other"),
        "region" => Fallback::skip(),
        "tech" => Fallback::text("Other"),
        _ => Fallback::skip(),
    }
}

/// One piece of a folder name: a field, or (field "text") words the player typed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Part {
    pub field: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
}

/// One folder level: its pieces joined by `sep` ("Ocarina of Time - Ship of Harkinian").
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Level {
    pub parts: Vec<Part>,
    #[serde(default = "dash")]
    pub sep: String,
}

fn dash() -> String {
    " - ".into()
}

/// The one folder structure. Kept in folders.json beside library.json.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Layout {
    /// The preset it started from (shown in the designer; editing doesn't change it).
    #[serde(default)]
    pub preset: String,
    /// The folder levels, outermost first. The last one is the game's own folder.
    pub levels: Vec<Level>,
    /// Per field, what to do when a game has nothing for it. Fields not listed use
    /// `default_fallback`.
    #[serde(default)]
    pub fallbacks: BTreeMap<String, Fallback>,
    /// Consoles by short name (N64) rather than full name (Nintendo 64).
    #[serde(default = "yes")]
    pub short_consoles: bool,
    /// "keep", "upper", "lower" or "title".
    #[serde(default = "keep")]
    pub case: String,
    /// The player's own names for consoles, by catalog console id: { "genesis": "Mega Drive" }.
    #[serde(default)]
    pub consoles: BTreeMap<String, String>,
    /// The player's own values per game (by recomp id), any field: { "soh": { "franchise": "The
    /// Legend of Zelda", "year": "1998" } }. Kept here, so a catalog refresh never resets them.
    #[serde(default)]
    pub games: BTreeMap<String, BTreeMap<String, String>>,
    /// Automatically use my folder structure for new installations. Off: new games get the
    /// launcher's usual folder (the library, then the recomp's id).
    #[serde(default)]
    pub apply_new: bool,
    /// Games the launcher put in a folder of this structure, by id, with that folder. Uninstall
    /// deletes such a folder like one it made the usual way.
    #[serde(default)]
    pub placed: BTreeMap<String, PathBuf>,
}

fn yes() -> bool {
    true
}

fn keep() -> String {
    "keep".into()
}

impl Default for Layout {
    fn default() -> Self {
        preset("console-game-recomp").unwrap()
    }
}

fn level(fields: &[&str]) -> Level {
    Level { parts: fields.iter().map(|f| Part { field: (*f).into(), text: String::new() }).collect(), sep: dash() }
}

/// The ready-made structures, each fully editable afterwards. (id, name, levels)
pub const PRESETS: [(&str, &str, &[&[&str]]); 9] = [
    ("console-game-recomp", "Console → Game → Recompilation", &[&["console"], &["game"], &["recomp"]]),
    ("console-gamerecomp", "Console → Game - Recompilation", &[&["console"], &["game", "recomp"]]),
    ("maker-console-game-recomp", "Manufacturer → Console → Game → Recompilation", &[&["maker"], &["console"], &["game"], &["recomp"]]),
    ("maker-console", "Manufacturer → Console", &[&["maker"], &["console"], &["recomp"]]),
    ("game-recomp", "Game → Recompilation", &[&["game"], &["recomp"]]),
    ("recomp", "Recompilation Only", &[&["recomp"]]),
    ("franchise-game-recomp", "Franchise → Game → Recompilation", &[&["franchise"], &["game"], &["recomp"]]),
    ("year-console-game", "Year → Console → Game", &[&["year"], &["console"], &["game"]]),
    ("letter-console-game", "Alphabetical → Console → Game", &[&["letter"], &["console"], &["game"]]),
];

pub fn preset(id: &str) -> Option<Layout> {
    let (pid, _, levels) = PRESETS.iter().find(|p| p.0 == id)?;
    Some(Layout {
        preset: (*pid).into(),
        levels: levels.iter().map(|l| level(l)).collect(),
        fallbacks: BTreeMap::new(),
        short_consoles: true,
        case: keep(),
        consoles: BTreeMap::new(),
        games: BTreeMap::new(),
        apply_new: false,
        placed: BTreeMap::new(),
    })
}

fn file(dir: &Path) -> PathBuf {
    dir.join("folders.json")
}

/// The saved structure, or the default one (Console → Game → Recompilation, not applied to new
/// installs) when there's none or it can't be read.
pub fn load(dir: &Path) -> Layout {
    std::fs::read(file(dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save(dir: &Path, layout: &Layout) -> std::io::Result<()> {
    let body = serde_json::to_vec_pretty(layout).map_err(std::io::Error::other)?;
    let tmp = file(dir).with_extension("json.tmp");
    std::fs::write(&tmp, body)?;
    std::fs::rename(tmp, file(dir))
}

// ---------- consoles ----------

/// Each catalog console id: its maker, full name and short name. The catalog's own name for a
/// console is used as its full name when it has one.
const CONSOLES: [(&str, &str, &str, &str); 31] = [
    ("n64", "Nintendo", "Nintendo 64", "N64"),
    ("snes", "Nintendo", "Super Nintendo", "SNES"),
    ("nes", "Nintendo", "Nintendo Entertainment System", "NES"),
    ("gc", "Nintendo", "GameCube", "GameCube"),
    ("wii", "Nintendo", "Wii", "Wii"),
    ("wiiu", "Nintendo", "Wii U", "Wii U"),
    ("gb", "Nintendo", "Game Boy", "GB"),
    ("gbc", "Nintendo", "Game Boy Color", "GBC"),
    ("gba", "Nintendo", "Game Boy Advance", "GBA"),
    ("ds", "Nintendo", "Nintendo DS", "DS"),
    ("3ds", "Nintendo", "Nintendo 3DS", "3DS"),
    ("vb", "Nintendo", "Virtual Boy", "VB"),
    ("switch", "Nintendo", "Nintendo Switch", "Switch"),
    ("switch2", "Nintendo", "Nintendo Switch 2", "Switch 2"),
    ("ps1", "Sony", "PlayStation", "PS1"),
    ("ps2", "Sony", "PlayStation 2", "PS2"),
    ("ps3", "Sony", "PlayStation 3", "PS3"),
    ("ps4", "Sony", "PlayStation 4", "PS4"),
    ("ps5", "Sony", "PlayStation 5", "PS5"),
    ("psp", "Sony", "PlayStation Portable", "PSP"),
    ("xbox", "Microsoft", "Xbox", "Xbox"),
    ("x360", "Microsoft", "Xbox 360", "X360"),
    ("dc", "Sega", "Dreamcast", "Dreamcast"),
    ("saturn", "Sega", "Sega Saturn", "Saturn"),
    ("genesis", "Sega", "Sega Genesis", "Genesis"),
    ("s32x", "Sega", "Sega 32X", "32X"),
    ("gg", "Sega", "Game Gear", "Game Gear"),
    ("cdi", "Philips", "CD-i", "CD-i"),
    ("amiga", "Commodore", "Amiga", "Amiga"),
    // Arcade boards and "other" have no one maker: the maker field falls back for them.
    ("arcade", "", "Arcade", "Arcade"),
    ("other", "", "Other", "Other"),
];

/// Other names a console goes by, so a value the player types (or a catalog that names a console
/// differently) lands on the same console: "Super Famicom" is snes, "Mega Drive" is genesis.
const ALIASES: [(&str, &str); 22] = [
    ("nintendo 64", "n64"),
    ("super nintendo", "snes"),
    ("super nintendo entertainment system", "snes"),
    ("super famicom", "snes"),
    ("famicom", "nes"),
    ("nintendo gamecube", "gc"),
    ("playstation", "ps1"),
    ("playstation 1", "ps1"),
    ("psx", "ps1"),
    ("psone", "ps1"),
    ("playstation 2", "ps2"),
    ("playstation 3", "ps3"),
    ("playstation 4", "ps4"),
    ("playstation 5", "ps5"),
    ("sega mega drive", "genesis"),
    ("mega drive", "genesis"),
    ("sega genesis", "genesis"),
    ("xbox360", "x360"),
    ("xbox 360", "x360"),
    ("sega dreamcast", "dc"),
    ("sega saturn", "saturn"),
    ("32x", "s32x"),
];

/// A console id for a name or id in any of its usual spellings, or None when it isn't one the
/// launcher knows.
pub fn console_id(name: &str) -> Option<&'static str> {
    let low = name.trim().to_lowercase();
    if let Some(c) = CONSOLES.iter().find(|c| c.0 == low || c.2.to_lowercase() == low || c.3.to_lowercase() == low) {
        return Some(c.0);
    }
    ALIASES.iter().find(|a| a.0 == low).map(|a| a.1)
}

/// Every console the launcher knows: (id, maker, full name, short name).
pub fn consoles() -> Vec<(&'static str, &'static str, &'static str, &'static str)> {
    CONSOLES.to_vec()
}

// ---------- metadata ----------

/// What the launcher knows about one game, field by field. A field that's missing has no entry.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Meta {
    pub id: String,
    pub values: BTreeMap<String, String>,
}

/// The facts metadata is worked out from: an installed (or example) game and its catalog entry.
pub struct Source<'a> {
    pub id: &'a str,
    /// The Library's name for it.
    pub name: &'a str,
    /// The game it's a recomp of, as the Library noted it (games the player added).
    pub game: &'a str,
    pub repo: &'a str,
    pub version: &'a str,
    pub local: bool,
    /// The player's linked copy of the game, for its region.
    pub copy: Option<&'a Path>,
    /// Its catalog entry, when the catalog lists it.
    pub entry: Option<&'a Value>,
    /// The whole catalog (consoles, title groups, developers).
    pub cat: &'a Value,
}

fn s<'a>(v: Option<&'a Value>, k: &str) -> &'a str {
    v.and_then(|v| v.get(k)).and_then(Value::as_str).map(str::trim).unwrap_or("")
}

/// The title group (the catalog's `titles`) a recomp belongs to.
fn title_of<'a>(cat: &'a Value, id: &str) -> Option<&'a Value> {
    let titles = cat.get("titles")?.as_array()?;
    titles.iter().find(|t| t.get("pc").and_then(Value::as_array).is_some_and(|pc| pc.iter().any(|x| x.as_str() == Some(id))))
}

fn year_of(date: &str) -> String {
    let y: String = date.chars().take(4).collect();
    if y.len() == 4 && y.chars().all(|c| c.is_ascii_digit()) {
        y
    } else {
        String::new()
    }
}

/// The repo owner in a project address: owner/repo, gitlab:owner/…, forgejo:host/owner/repo.
fn repo_owner(repo: &str) -> String {
    let forgejo = repo.starts_with("forgejo:");
    let bare = repo.split_once(':').filter(|(k, _)| k.chars().all(|c| c.is_ascii_lowercase())).map(|(_, r)| r).unwrap_or(repo);
    let parts: Vec<&str> = bare.split('/').collect();
    let owner = if forgejo { parts.get(1) } else { parts.first() };
    owner.map(|o| o.to_string()).unwrap_or_default()
}

fn source_name(repo: &str, local: bool) -> String {
    if local {
        return "Local Build".into();
    }
    let kind = repo.split_once(':').map(|(k, _)| k).filter(|k| k.chars().all(|c| c.is_ascii_lowercase())).unwrap_or("");
    match kind {
        "" if repo.contains('/') => "GitHub".into(),
        "gitlab" => "GitLab".into(),
        "forgejo" | "codeberg" => "Forgejo".into(),
        "sourceforge" | "sf" => "SourceForge".into(),
        "" => "Added by You".into(),
        other => other.to_string(),
    }
}

/// A catalog tag that says what kind of project it is, as a folder word.
fn tech_of(entry: Option<&Value>) -> String {
    let tags: Vec<String> = entry.and_then(|e| e.get("tags")).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_lowercase).collect()).unwrap_or_default();
    let kinds = [("static recomp", "Static Recompilation"), ("recomp", "Recompilation"), ("decomp", "Decompilation"), ("port", "Port"), ("reimplementation", "Reimplementation"), ("remake", "Remake"), ("emulat", "Emulation")];
    for (needle, word) in kinds {
        if tags.iter().any(|t| t.contains(needle)) {
            return word.into();
        }
    }
    String::new()
}

const GENRES: [(&str, &str); 9] = [
    ("action", "Action"),
    ("adventure", "Adventure"),
    ("platformer", "Platformer"),
    ("racing", "Racing"),
    ("fighting", "Fighting"),
    ("shooter", "Shooter"),
    ("sports", "Sports"),
    ("rpg", "RPG"),
    ("puzzle", "Puzzle"),
];

fn put(m: &mut BTreeMap<String, String>, k: &str, v: impl Into<String>) {
    let v = v.into();
    let v = v.trim();
    if !v.is_empty() {
        m.insert(k.into(), v.to_string());
    }
}

/// The region of an N64 copy, from its cartridge header (other consoles' copies don't say).
fn region_of(copy: Option<&Path>) -> String {
    let Some(p) = copy.filter(|p| p.is_file()) else { return String::new() };
    let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if !["z64", "n64", "v64"].contains(&ext.as_str()) {
        return String::new();
    }
    crate::roms::n64_header(p).map(|(_, r)| r).unwrap_or_default()
}

/// Everything known about one game, before the player's own values and the fallbacks.
pub fn resolve(src: &Source, layout: &Layout) -> Meta {
    let mut v = BTreeMap::new();
    let entry = src.entry;
    let title = title_of(src.cat, src.id);
    let origin = s(entry, "origin");
    let known = console_id(origin);
    if let Some(c) = known.and_then(|id| CONSOLES.iter().find(|c| c.0 == id)) {
        put(&mut v, "maker", c.1);
        let catalog_name = src.cat.get("origins").and_then(Value::as_array).and_then(|a| a.iter().find(|o| s(Some(o), "id") == origin)).map(|o| s(Some(o), "name")).unwrap_or("");
        let full = if catalog_name.is_empty() { c.2 } else { catalog_name };
        let name = layout.consoles.get(c.0).map(String::as_str).filter(|n| !n.trim().is_empty()).unwrap_or(if layout.short_consoles { c.3 } else { full });
        put(&mut v, "console", name);
    } else if !origin.is_empty() {
        // A console the table doesn't know yet: the catalog's own name for it.
        let name = src.cat.get("origins").and_then(Value::as_array).and_then(|a| a.iter().find(|o| s(Some(o), "id") == origin)).map(|o| s(Some(o), "name")).unwrap_or(origin);
        put(&mut v, "console", layout.consoles.get(origin).cloned().unwrap_or_else(|| name.to_string()));
    }
    let game = [s(entry, "game"), s(title, "name"), src.game].into_iter().find(|g| !g.is_empty()).unwrap_or("");
    put(&mut v, "game", game);
    put(&mut v, "recomp", if src.name.trim().is_empty() { s(entry, "name") } else { src.name });
    // Read when the catalog carries them (it doesn't today): on the recomp, or on its game.
    for (field, keys) in [("franchise", ["franchise", "series"]), ("developer", ["developer", "original_developer"]), ("publisher", ["publisher", "publisher"])] {
        let found = keys.iter().flat_map(|k| [s(entry, k), s(title, k)]).find(|x| !x.is_empty()).unwrap_or("");
        put(&mut v, field, found);
    }
    let year = [s(entry, "year"), s(title, "year")].into_iter().map(year_of).find(|y| !y.is_empty()).unwrap_or_default();
    put(&mut v, "year", year);
    put(&mut v, "recomp_year", year_of(s(entry, "first")));
    let dev_slug = s(entry, "dev");
    let dev_page = src.cat.get("developers").and_then(Value::as_array).and_then(|a| a.iter().find(|d| !dev_slug.is_empty() && s(Some(d), "slug") == dev_slug));
    let repo = if src.repo.is_empty() { s(entry, "repo") } else { src.repo };
    let team = [s(dev_page, "name"), s(entry, "author")].into_iter().find(|x| !x.is_empty()).map(str::to_string).unwrap_or_else(|| repo_owner(repo));
    put(&mut v, "team", team);
    let genre = s(entry, "genre");
    put(&mut v, "genre", GENRES.iter().find(|g| g.0 == genre).map(|g| g.1).unwrap_or(genre));
    put(&mut v, "region", region_of(src.copy));
    put(&mut v, "tech", tech_of(entry));
    put(&mut v, "source", if src.local || !repo.is_empty() { source_name(repo, src.local) } else { String::new() });
    put(&mut v, "version", src.version);
    // The player's own values win over anything worked out.
    if let Some(own) = layout.games.get(src.id) {
        for (k, val) in own {
            let val = if k == "console" { console_override(val, layout) } else { val.clone() };
            put(&mut v, k, val);
        }
    }
    // The letter follows the game title as it ends up (a player's own title included).
    let lead = v.get("game").or_else(|| v.get("recomp")).cloned().unwrap_or_default();
    put(&mut v, "letter", letter_of(&lead));
    Meta { id: src.id.into(), values: v }
}

/// A console the player typed for one game, named the way the layout names consoles.
fn console_override(typed: &str, layout: &Layout) -> String {
    let Some(c) = console_id(typed).and_then(|id| CONSOLES.iter().find(|c| c.0 == id)) else { return typed.to_string() };
    layout.consoles.get(c.0).cloned().unwrap_or_else(|| (if layout.short_consoles { c.3 } else { c.2 }).to_string())
}

/// The alphabetical group: the first letter of the title, leading "The " and "A " aside; "0-9" for
/// a digit and "#" for anything else.
pub fn letter_of(title: &str) -> String {
    let t = title.trim();
    let low = t.to_lowercase();
    let t = ["the ", "a "].iter().find(|a| low.starts_with(*a) && t.len() > a.len()).map(|a| &t[a.len()..]).unwrap_or(t);
    match t.chars().find(|c| !c.is_whitespace()) {
        Some(c) if c.is_ascii_alphabetic() => c.to_ascii_uppercase().to_string(),
        Some(c) if c.is_ascii_digit() => "0-9".into(),
        Some(_) => "#".into(),
        None => String::new(),
    }
}

// ---------- folder names ----------

/// A field's value for a game, with its fallback applied. None: leave it out.
fn value(meta: &Meta, part: &Part, layout: &Layout) -> Option<String> {
    if part.field == "text" {
        return Some(part.text.clone()).filter(|t| !t.trim().is_empty());
    }
    if let Some(v) = meta.values.get(&part.field) {
        return Some(v.clone());
    }
    let fb = layout.fallbacks.get(&part.field).cloned().unwrap_or_else(|| default_fallback(&part.field));
    match fb.mode.as_str() {
        "text" => Some(fb.text).filter(|t| !t.trim().is_empty()),
        // One step only, so two fields pointing at each other can't loop.
        "field" => meta.values.get(&fb.field).cloned(),
        _ => None,
    }
}

fn cased(s: &str, case: &str) -> String {
    match case {
        "upper" => s.to_uppercase(),
        "lower" => s.to_lowercase(),
        "title" => s.split(' ').map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        }).collect::<Vec<_>>().join(" "),
        _ => s.to_string(),
    }
}

const RESERVED: [&str; 22] = ["con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9"];
/// Longest folder name, in characters: long enough for "Game - Recompilation (Version)", short
/// enough that nested levels stay well under Windows' path limit.
const MAX_NAME: usize = 64;

/// A folder name every system accepts: no characters Windows refuses (a colon becomes " -"),
/// no leading or trailing dots and spaces, not a name Windows reserves (CON, NUL…), and not too
/// long. Empty when nothing usable is left.
pub fn safe_name(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.chars() {
        match c {
            ':' => out.push_str(" -"),
            '/' | '\\' | '|' => out.push('-'),
            '<' | '>' | '"' | '?' | '*' => {}
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim_matches(|c: char| c == '.' || c == ' ');
    let mut name: String = trimmed.chars().take(MAX_NAME).collect();
    name = name.trim_end_matches(|c: char| c == '.' || c == ' ' || c == '-').to_string();
    if name.is_empty() {
        return name;
    }
    let stem = name.split('.').next().unwrap_or("").to_lowercase();
    if RESERVED.contains(&stem.as_str()) {
        name.push('_');
    }
    name
}

/// One level's folder name for a game. Empty when every piece of it was left out.
fn level_name(meta: &Meta, level: &Level, layout: &Layout) -> String {
    let pieces: Vec<String> = level.parts.iter().filter_map(|p| value(meta, p, layout)).map(|v| cased(v.trim(), &layout.case)).filter(|v| !v.is_empty()).collect();
    safe_name(&pieces.join(&level.sep))
}

/// The folders a game goes in, inside its library: one per level, levels with nothing left out.
/// Never empty: with every level left out, the game's own id.
pub fn relative(meta: &Meta, layout: &Layout) -> Vec<String> {
    let mut out: Vec<String> = layout.levels.iter().map(|l| level_name(meta, l, layout)).filter(|n| !n.is_empty()).collect();
    if out.is_empty() {
        out.push(crate::library::folder_name(&meta.id));
    }
    out
}

/// The folder for a game, made unique among `taken` (folders other games use or are planned for):
/// first the recompilation's name is added to its last folder ("Ocarina of Time - Ship of
/// Harkinian") when it isn't in it already, then a number ("… (2)").
pub fn unique(base: &Path, rel: &[String], meta: &Meta, taken: &dyn Fn(&Path) -> bool) -> PathBuf {
    let parent = rel[..rel.len() - 1].iter().fold(base.to_path_buf(), |p, s| p.join(s));
    let last = rel.last().cloned().unwrap_or_default();
    let first = parent.join(&last);
    if !taken(&first) {
        return first;
    }
    let recomp = meta.values.get("recomp").cloned().unwrap_or_default();
    let named = if !recomp.is_empty() && !last.to_lowercase().contains(&recomp.to_lowercase()) { safe_name(&format!("{last} - {recomp}")) } else { last.clone() };
    let second = parent.join(&named);
    if !taken(&second) {
        return second;
    }
    (2..1000).map(|n| parent.join(format!("{named} ({n})"))).find(|p| !taken(p)).unwrap_or_else(|| parent.join(format!("{named} ({})", meta.id)))
}

/// A few made-up games for the preview, when the Library has fewer than three: a catalog that
/// knows most fields, one that knows little, and two recomps of the same game.
pub fn examples() -> Vec<Meta> {
    let mk = |id: &str, pairs: &[(&str, &str)]| Meta { id: id.into(), values: pairs.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect() };
    vec![
        mk("example-soh", &[("maker", "Nintendo"), ("console", "N64"), ("game", "Ocarina of Time"), ("recomp", "Ship of Harkinian"), ("recomp_year", "2022"), ("team", "HarbourMasters"), ("genre", "Adventure"), ("tech", "Decompilation"), ("source", "GitHub"), ("letter", "O"), ("version", "9.0.2")]),
        mk("example-mm", &[("maker", "Nintendo"), ("console", "N64"), ("game", "Majora's Mask"), ("recomp", "Zelda64Recomp"), ("recomp_year", "2024"), ("team", "Zelda64Recomp"), ("genre", "Adventure"), ("source", "GitHub"), ("letter", "M"), ("version", "1.2.0")]),
        mk("example-2ship", &[("maker", "Nintendo"), ("console", "N64"), ("game", "Majora's Mask"), ("recomp", "2 Ship 2 Harkinian"), ("recomp_year", "2024"), ("team", "HarbourMasters"), ("source", "GitHub"), ("letter", "M"), ("version", "2.0.0")]),
        mk("example-added", &[("recomp", "Some Recomp You Added"), ("team", "someone"), ("source", "GitHub"), ("letter", "S"), ("version", "v0.3")]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cat() -> Value {
        json!({
            "origins": [{ "id": "n64", "name": "Nintendo 64" }, { "id": "genesis", "name": "Sega Genesis" }],
            "titles": [{ "id": "oot", "name": "The Legend of Zelda: Ocarina of Time", "pc": ["soh"] }],
            "developers": [{ "slug": "hm", "name": "HarbourMasters" }],
            "apps": [
                { "id": "soh", "name": "Ship of Harkinian", "game": "Ocarina of Time", "origin": "n64", "first": "2022-03-01", "dev": "hm", "genre": "adventure", "tags": ["Decomp"], "repo": "HarbourMasters/Shipwright" },
                { "id": "bare", "name": "Bare", "repo": "someone/bare" }
            ]
        })
    }

    fn meta(id: &str, layout: &Layout) -> Meta {
        let c = cat();
        let entry = crate::catalog::app(&c, id).cloned();
        let name = s(entry.as_ref(), "name").to_string();
        let src = Source { id, name: &name, game: "", repo: "", version: "v1", local: false, copy: None, entry: entry.as_ref(), cat: &c };
        resolve(&src, layout)
    }

    fn path(id: &str, layout: &Layout) -> String {
        relative(&meta(id, layout), layout).join("/")
    }

    #[test]
    fn every_preset_makes_the_folders_its_name_says() {
        let want = [
            ("console-game-recomp", "N64/Ocarina of Time/Ship of Harkinian"),
            ("console-gamerecomp", "N64/Ocarina of Time - Ship of Harkinian"),
            ("maker-console-game-recomp", "Nintendo/N64/Ocarina of Time/Ship of Harkinian"),
            ("maker-console", "Nintendo/N64/Ship of Harkinian"),
            ("game-recomp", "Ocarina of Time/Ship of Harkinian"),
            ("recomp", "Ship of Harkinian"),
            ("year-console-game", "Unknown Year/N64/Ocarina of Time"),
            ("letter-console-game", "O/N64/Ocarina of Time"),
        ];
        for (id, p) in want {
            assert_eq!(path("soh", &preset(id).unwrap()), p, "{id}");
        }
    }

    #[test]
    fn a_franchise_nobody_gave_is_left_out_and_one_the_player_gave_is_used() {
        let mut l = preset("franchise-game-recomp").unwrap();
        assert_eq!(path("soh", &l), "Ocarina of Time/Ship of Harkinian");
        l.games.insert("soh".into(), [("franchise".to_string(), "The Legend of Zelda".to_string())].into());
        assert_eq!(path("soh", &l), "The Legend of Zelda/Ocarina of Time/Ship of Harkinian");
    }

    #[test]
    fn a_missing_year_uses_the_fallback_the_player_set() {
        let mut l = preset("year-console-game").unwrap();
        l.fallbacks.insert("year".into(), Fallback::skip());
        assert_eq!(path("soh", &l), "N64/Ocarina of Time");
        l.fallbacks.insert("year".into(), Fallback::text("Undated"));
        assert_eq!(path("soh", &l), "Undated/N64/Ocarina of Time");
        l.fallbacks.insert("year".into(), Fallback::field("recomp_year"));
        assert_eq!(path("soh", &l), "2022/N64/Ocarina of Time");
        l.games.insert("soh".into(), [("year".to_string(), "1998".to_string())].into());
        assert_eq!(path("soh", &l), "1998/N64/Ocarina of Time");
    }

    #[test]
    fn a_game_the_catalog_knows_little_about_still_gets_a_folder() {
        let l = preset("maker-console-game-recomp").unwrap();
        // No console: maker and console fall back to Other; no game: the recomp's own name.
        assert_eq!(path("bare", &l), "Other/Other/Bare/Bare");
        let mut l = preset("console-gamerecomp").unwrap();
        l.fallbacks.insert("console".into(), Fallback::skip());
        assert_eq!(path("bare", &l), "Bare - Bare");
    }

    #[test]
    fn consoles_can_go_by_full_names_or_the_players_own() {
        let mut l = preset("console-game-recomp").unwrap();
        l.short_consoles = false;
        assert_eq!(path("soh", &l), "Nintendo 64/Ocarina of Time/Ship of Harkinian");
        l.consoles.insert("n64".into(), "Ultra 64".into());
        assert_eq!(path("soh", &l), "Ultra 64/Ocarina of Time/Ship of Harkinian");
    }

    #[test]
    fn console_names_in_other_spellings_are_the_same_console() {
        assert_eq!(console_id("Nintendo 64"), Some("n64"));
        assert_eq!(console_id("Super Nintendo"), Some("snes"));
        assert_eq!(console_id("PlayStation 2"), Some("ps2"));
        assert_eq!(console_id("Sega Mega Drive"), Some("genesis"));
        assert_eq!(console_id("Pippin"), None);
    }

    #[test]
    fn the_other_fields_read_what_the_catalog_has() {
        let m = meta("soh", &Layout::default());
        let get = |k: &str| m.values.get(k).cloned().unwrap_or_default();
        assert_eq!(get("team"), "HarbourMasters");
        assert_eq!(get("recomp_year"), "2022");
        assert_eq!(get("genre"), "Adventure");
        assert_eq!(get("tech"), "Decompilation");
        assert_eq!(get("source"), "GitHub");
        assert_eq!(get("letter"), "O");
        assert!(!m.values.contains_key("publisher"));
        assert!(!m.values.contains_key("developer"));
    }

    #[test]
    fn names_windows_refuses_are_made_safe() {
        assert_eq!(safe_name("Zelda: Ocarina of Time"), "Zelda - Ocarina of Time");
        assert_eq!(safe_name("What?*<>\"|"), "What");
        assert_eq!(safe_name("CON"), "CON_");
        assert_eq!(safe_name("..."), "");
        assert_eq!(safe_name("../../etc"), "-..-etc");
        assert_eq!(safe_name(&"x".repeat(200)).len(), MAX_NAME);
    }

    #[test]
    fn combined_names_capitalise_and_join_as_asked() {
        let mut l = preset("console-gamerecomp").unwrap();
        l.levels[1].sep = " | ".into();
        l.case = "upper".into();
        assert_eq!(path("soh", &l), "N64/OCARINA OF TIME - SHIP OF HARKINIAN");
        l.levels[1].parts.push(Part { field: "text".into(), text: "mine".into() });
        l.case = "title".into();
        assert_eq!(path("soh", &l), "N64/Ocarina Of Time - Ship Of Harkinian - Mine");
    }

    #[test]
    fn two_recomps_of_one_game_get_folders_of_their_own() {
        let l = preset("year-console-game").unwrap();
        let m = meta("soh", &l);
        let rel = relative(&m, &l);
        let base = Path::new("/lib");
        let first = base.join("Unknown Year/N64/Ocarina of Time");
        let taken = |p: &Path| p == first;
        assert_eq!(unique(base, &rel, &m, &taken), base.join("Unknown Year/N64/Ocarina of Time - Ship of Harkinian"));
        let second = base.join("Unknown Year/N64/Ocarina of Time - Ship of Harkinian");
        let both = |p: &Path| p == first || p == second;
        assert_eq!(unique(base, &rel, &m, &both), base.join("Unknown Year/N64/Ocarina of Time - Ship of Harkinian (2)"));
    }

    #[test]
    fn a_structure_with_nothing_left_still_gives_the_game_a_folder() {
        let mut l = preset("recomp").unwrap();
        l.levels = vec![level(&["franchise"])];
        assert_eq!(path("soh", &l), "soh");
    }

    #[test]
    fn letters_skip_the_and_group_digits() {
        assert_eq!(letter_of("The Legend of Zelda"), "L");
        assert_eq!(letter_of("2 Ship 2 Harkinian"), "0-9");
        assert_eq!(letter_of("¡Hola!"), "#");
    }

    #[test]
    fn a_saved_structure_reads_back_the_same_and_old_files_get_defaults() {
        let d = std::env::temp_dir().join(format!("rl-folders-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let mut l = preset("game-recomp").unwrap();
        l.apply_new = true;
        save(&d, &l).unwrap();
        assert_eq!(load(&d), l);
        std::fs::write(d.join("folders.json"), br#"{"levels":[{"parts":[{"field":"game"}]}]}"#).unwrap();
        let old = load(&d);
        assert!(old.short_consoles && !old.apply_new && old.levels[0].sep == " - ");
        let _ = std::fs::remove_dir_all(&d);
    }
}

//! Tools for recomp developers: add a build from a folder on this PC (nothing is downloaded, the
//! folder stays theirs), check its recomplauncher.json and see what the launcher makes of it, and
//! write a starter file to begin from.

use crate::library::Installed;
use crate::{catalog, install, profile, App, Error, Res};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use tauri::State;

const FILE: &str = "recomplauncher.json";
const TOP_KEYS: &[&str] = &["$schema", "format", "name", "repo", "game", "start", "config", "folders", "exit_codes", "settings", "actions", "prepare", "looks", "mods", "saves"];

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() { "build".into() } else { out.chars().take(40).collect() }
}

/// The name a build goes by: `name` in its file, or its folder's name.
fn name_of(dir: &Path, raw: Option<&Value>) -> String {
    raw.and_then(|v| v.get("name")).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(String::from).unwrap_or_else(|| {
        dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Local build".into())
    })
}

fn read_raw(dir: &Path) -> Option<Value> {
    std::fs::read(dir.join(FILE)).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok())
}

/// The program Play starts: the file's own `start.program`, or the likeliest one in the folder.
fn program_in(dir: &Path, p: Option<&profile::Profile>, name: &str) -> Option<PathBuf> {
    if let Some(start) = p.and_then(|p| p.start.as_ref()) {
        if let Some(found) = start.program.iter().map(|x| PathBuf::from(profile::fill(x, dir, None))).find(|x| dir.join(x).is_file()) {
            return Some(found);
        }
    }
    install::find_exe(dir, crate::assets::os_now(), &[name])
}

/// Add a build from a folder on this PC. It shows in the Library like any game, marked as a local
/// build. The folder is never moved, updated or deleted by the launcher.
#[tauri::command(async)]
pub fn dev_add(st: State<'_, App>, path: String) -> Res<Installed> {
    let dir = PathBuf::from(&path);
    if !dir.is_dir() {
        return Err(Error::Msg("That folder isn't there.".into()));
    }
    let raw = read_raw(&dir);
    let prof = raw.as_ref().and_then(|v| serde_json::from_value::<profile::Profile>(v.clone()).ok());
    let name = name_of(&dir, raw.as_ref());
    let exe = program_in(&dir, prof.as_ref(), &name);
    let mut l = st.lib.lock().unwrap();
    // The same folder added again: the entry it already has.
    if let Some(g) = l.installed.values().find(|g| g.local && g.path == dir) {
        return Ok(g.clone());
    }
    let base = format!("local-{}", slug(&name));
    let mut id = base.clone();
    let mut n = 2;
    while l.installed.contains_key(&id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    let g = Installed {
        id: id.clone(),
        name,
        repo: prof.as_ref().map(|p| p.repo.clone()).unwrap_or_default(),
        tag: "local".into(),
        version: String::new(),
        path: dir,
        exe,
        asset: String::new(),
        installed_at: now(),
        updated_at: now(),
        local: true,
        ..Default::default()
    };
    l.installed.insert(id, g.clone());
    drop(l);
    st.save();
    Ok(g)
}

#[derive(Serialize)]
pub struct Item {
    /// "ok", "warn" or "err".
    level: &'static str,
    text: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    detail: String,
}

#[derive(Serialize)]
pub struct Report {
    /// Where the launcher's profile for it comes from: "folder", "catalog", "built-in" or "none".
    source: &'static str,
    file: String,
    has_file: bool,
    /// The file can't be read as JSON, or doesn't fit the format: what's wrong and where.
    error: String,
    /// What the file gives players: 0 nothing yet, 1 plays from the launcher, 2 settings, 3 full setup.
    level: u8,
    items: Vec<Item>,
}

fn ok(text: impl Into<String>) -> Item {
    Item { level: "ok", text: text.into(), detail: String::new() }
}
fn warn(text: impl Into<String>, detail: impl Into<String>) -> Item {
    Item { level: "warn", text: text.into(), detail: detail.into() }
}
fn err(text: impl Into<String>, detail: impl Into<String>) -> Item {
    Item { level: "err", text: text.into(), detail: detail.into() }
}

/// What the launcher makes of a game's recomplauncher.json: read fresh from its folder each time.
#[tauri::command(async)]
pub fn dev_check(st: State<'_, App>, id: String) -> Res<Report> {
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't in your Library.".into()))?;
    let dir = g.path.clone();
    let file = dir.join(FILE);
    let mut items = vec![];
    let mut error = String::new();
    let mut prof: Option<profile::Profile> = None;
    let has_file = file.is_file();
    if has_file {
        match std::fs::read_to_string(&file) {
            Err(e) => error = format!("It can't be read: {e}"),
            Ok(text) => match serde_json::from_str::<Value>(&text) {
                Err(e) => error = format!("It isn't valid JSON: {e}."),
                Ok(v) => {
                    if let Some(o) = v.as_object() {
                        let unknown: Vec<&String> = o.keys().filter(|k| !TOP_KEYS.contains(&k.as_str())).collect();
                        if !unknown.is_empty() {
                            items.push(warn(
                                format!("Not part of the format: {}", unknown.iter().map(|k| format!("\"{k}\"")).collect::<Vec<_>>().join(", ")),
                                "These are skipped. Check the spelling against the developer guide.",
                            ));
                        }
                    } else {
                        error = "The file has to be one JSON object, in { }.".into();
                    }
                    if error.is_empty() {
                        match serde_json::from_value::<profile::Profile>(v.clone()) {
                            Err(e) => error = format!("It doesn't fit the format: {e}."),
                            Ok(p) => prof = Some(p),
                        }
                    }
                }
            },
        }
    }
    // What's actually used: the folder's own file when it reads, else the catalog's or a built-in one.
    let cat = crate::cached_catalog(&st);
    let entry = catalog::app(&cat, &id).cloned().unwrap_or(Value::Null);
    let used = profile::find(&dir, &g.repo, &entry);
    let source = match (&prof, &used) {
        (Some(p), Some(_)) if p.format == 1 => "folder",
        (_, Some(_)) if entry.get("launcher").is_some() => "catalog",
        (_, Some(_)) => "built-in",
        _ => "none",
    };
    let mut level = 0u8;
    if let Some(p) = prof.as_ref() {
        if p.format != 1 {
            items.push(err("\"format\" has to be 1", "Without it the launcher skips the whole file."));
        } else {
            items.push(ok("Format 1"));
        }
        level = check_profile(p, &dir, &g, &mut items);
    } else if !has_file && source == "none" {
        items.push(warn(
            "No recomplauncher.json in this folder",
            "Play still starts the program below, but players set the game up by hand. Make a starter file to begin.",
        ));
    } else if let Some(p) = used.as_ref() {
        items.push(warn(
            if source == "catalog" { "Using the file from recomplauncher.com" } else { "Using the file built into the launcher" },
            "A recomplauncher.json in your release takes its place.",
        ));
        level = check_profile(p, &dir, &g, &mut items);
    }
    Ok(Report { source, file: file.display().to_string(), has_file, error, level, items })
}

/// The checks, top to bottom in the order of the developer guide. Gives back the level reached.
fn check_profile(p: &profile::Profile, dir: &Path, g: &Installed, items: &mut Vec<Item>) -> u8 {
    let mut level = 0u8;
    // Level 1: start and game
    match p.start.as_ref() {
        None => items.push(warn("No \"start\"", "Play starts the program the launcher found, with no arguments of yours.")),
        Some(s) if s.program.is_empty() => items.push(err("\"start.program\" is empty", "List the path of your program, from the top of your release.")),
        Some(s) => match s.program.iter().find(|x| dir.join(profile::fill(x, dir, None)).is_file()) {
            Some(x) => {
                items.push(ok(format!("Play starts {x}{}", if s.args.is_empty() { String::new() } else { format!(" {}", s.args.join(" ")) })));
                level = 1;
            }
            None => items.push(err(format!("None of \"start.program\" is in this folder: {}", s.program.join(", ")), "The paths start from the top of your release, next to recomplauncher.json.")),
        },
    }
    if let Some(s) = p.start.as_ref().filter(|s| !s.env.is_empty()) {
        let bad: Vec<&String> = s.env.keys().filter(|k| !profile::env_name_ok(k)).collect();
        if bad.is_empty() {
            items.push(ok(format!("The game also gets {}", s.env.keys().cloned().collect::<Vec<_>>().join(", "))));
        } else {
            items.push(warn(format!("Left out of \"start.env\": {}", bad.iter().map(|x| x.as_str()).collect::<Vec<_>>().join(", ")),
                "Names are letters, digits and _. The launcher's own RECOMPLAUNCHER_ ones and system ones like PATH can't be set."));
        }
    }
    if let Some(gc) = p.game.as_ref() {
        if !["iso", "folder", "iso_or_folder"].contains(&gc.kind.as_str()) {
            items.push(err(format!("\"game.kind\" is \"{}\"", gc.kind), "Use \"iso\", \"folder\" or \"iso_or_folder\"."));
        } else if gc.label.trim().is_empty() {
            items.push(warn("\"game.label\" is empty", "It's what the setup screen asks the player for. Name the region or serial if only one works."));
        } else {
            items.push(ok(format!("Asks for: {}", gc.label)));
        }
        let c = &gc.check;
        if c.file.is_empty() && c.xdvdfs.is_empty() && c.iso9660.is_empty() {
            items.push(warn("No \"game.check\"", "Any disc or folder is accepted, and the launcher can't find the player's copy in their folders."));
        } else {
            items.push(ok("Checks the player's copy and finds it in their folders"));
        }
        for d in &gc.dumps {
            if d.size == 0 || d.crc32.trim().is_empty() {
                items.push(warn(format!("The \"{}\" dump needs both \"size\" and \"crc32\"", d.name), "Take them from Redump."));
            }
        }
        match g.game_copy.as_ref() {
            Some(c) => items.push(ok(format!("Your copy: {}", c.display()))),
            None => items.push(warn("No copy set up yet", "Use Set up from the game's page to try the setup screen with your own disc.")),
        }
    } else {
        items.push(ok("No \"game\": your recomp finds the game itself"));
    }
    // Where it saves: what backups and cloud saves copy.
    if p.saves.is_empty() {
        items.push(warn(
            "No \"saves\"",
            "Backups and cloud saves copy what the launcher notices the game writing, or the player's files in its folder. Say where it saves to be sure.",
        ));
    }
    for t in &p.saves {
        let f = crate::saves::resolve(t, &g.path);
        let shown = if t.contains('*') { f.parent().map(Path::to_path_buf).unwrap_or(f.clone()) } else { f.clone() };
        if shown.exists() {
            items.push(ok(format!("Saves: {}", f.display())));
        } else {
            items.push(warn(format!("Saves: {} isn't there yet", f.display()), "Play once so the game makes it. Check the path if it still isn't there after."));
        }
    }
    // Level 2: config and settings
    let mut ids = BTreeSet::new();
    for c in &p.config {
        if !["toml", "ini", "json"].contains(&c.format.as_str()) {
            items.push(err(format!("Settings file \"{}\": format \"{}\"", c.id, c.format), "Use \"toml\", \"ini\" or \"json\"."));
            continue;
        }
        ids.insert(c.id.clone());
        match profile::place(&c.file, dir) {
            Some(f) if f.is_file() => items.push(ok(format!("Settings file \"{}\": {}", c.id, f.display()))),
            Some(f) if !c.new.is_empty() => items.push(ok(format!("Settings file \"{}\": made on the first start, at {}", c.id, f.display()))),
            Some(f) => items.push(warn(format!("Settings file \"{}\" isn't there yet: {}", c.id, f.display()), "Start the game once so it writes it, or give \"new\" defaults.")),
            None => items.push(err(format!("Settings file \"{}\": no path applies", c.id), "Give an entry without \"if\" as the fallback.")),
        }
    }
    let mut seen = BTreeSet::new();
    let current = p.current(dir, &g.picks);
    for (i, s) in p.settings.iter().enumerate() {
        if !seen.insert(s.id.clone()) {
            items.push(err(format!("Two settings are called \"{}\"", s.id), "Each needs its own id."));
        }
        if s.target != "args" && !ids.contains(&s.target) {
            items.push(err(format!("\"{}\" is in \"{}\", which isn't one of your settings files", s.label, s.target), "Use a \"config\" id, or \"args\" for the command line."));
            continue;
        }
        if s.choices.len() < 2 {
            items.push(warn(format!("\"{}\" has {} choice", s.label, s.choices.len()), "Give it at least two."));
            continue;
        }
        let shown = current.get(i).map(|c| c.0).unwrap_or(0);
        let label = s.choices.get(shown).map(|c| c.label.as_str()).unwrap_or("");
        if s.target == "args" {
            items.push(ok(format!("{}: {} (on the command line)", s.label, label)));
        } else if let Some(f) = p.config_file(&s.target, dir) {
            let val = |k: &str| f.get(k).or_else(|| s.defaults.get(k).cloned());
            let matched = s.choices.iter().any(|c| c.set.iter().all(|(k, v)| val(k).as_deref() == Some(v.as_str())));
            let missing: Vec<&String> = s.choices.iter().flat_map(|c| c.set.keys()).filter(|k| val(k).is_none()).collect::<BTreeSet<_>>().into_iter().collect();
            if matched {
                items.push(ok(format!("{}: {}", s.label, label)));
            } else if !missing.is_empty() {
                items.push(warn(format!("{}: the file has no {}", s.label, missing.iter().map(|k| format!("\"{k}\"")).collect::<Vec<_>>().join(", ")), "Put the game's own values in \"defaults\" so the right choice shows."));
            } else {
                items.push(warn(format!("{}: none of its choices match the file, so it shows \"{}\"", s.label, s.choices[0].label), "Check the values are written exactly as in the file, quotes included."));
            }
        }
    }
    if level >= 1 && !p.settings.is_empty() {
        level = 2;
    }
    // Level 3: prepare and dumps
    if let Some(pr) = p.prepare.as_ref() {
        match pr.program.iter().find(|x| dir.join(profile::fill(x, dir, None)).is_file()) {
            Some(x) => {
                items.push(ok(format!("Setup step: {x}")));
                if level >= 2 {
                    level = 3;
                }
            }
            None => items.push(err("None of \"prepare.program\" is in this folder", pr.program.join(", "))),
        }
    }
    level
}

/// Write a starter recomplauncher.json into a build's folder (only when it has none).
#[tauri::command(async)]
pub fn dev_starter(st: State<'_, App>, id: String) -> Res<String> {
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't in your Library.".into()))?;
    let file = g.path.join(FILE);
    if file.exists() {
        return Err(Error::Msg("It already has a recomplauncher.json.".into()));
    }
    let exe = g.exe.as_ref().map(|e| e.to_string_lossy().replace('\\', "/")).unwrap_or_else(|| "game.exe".into());
    let v = serde_json::json!({
        "$schema": "https://recomplauncher.com/schema/recomplauncher.json",
        "format": 1,
        "name": g.name,
        "repo": if g.repo.is_empty() { "you/your-recomp".to_string() } else { g.repo.clone() },
        "game": {
            "kind": "iso",
            "label": "Your disc image (.iso) of the game.",
            "check": { "file": "default.xex" }
        },
        "start": { "program": [exe], "args": [] }
    });
    let text = serde_json::to_string_pretty(&v)? + "\n";
    std::fs::write(&file, text)?;
    Ok(file.display().to_string())
}

/// Whether a game is a local build (never updated, moved or deleted by the launcher).
pub fn is_local(st: &App, id: &str) -> bool {
    st.lib.lock().unwrap().installed.get(id).map(|g| g.local).unwrap_or(false)
}

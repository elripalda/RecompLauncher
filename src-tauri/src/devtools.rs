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
const TOP_KEYS: &[&str] = &["$schema", "format", "name", "repo", "game", "start", "config", "folders", "exit_codes", "settings", "groups", "actions", "prepare", "looks", "mods", "saves", "inputs", "games", "system"];
/// Written by manifests from before, and ignored: what a release's own manifest describes is the
/// release, so it never has to tell an older launcher what to leave out.
const IGNORED_KEYS: &[&str] = &["requires", "min"];

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

/// The game a folder is a recomp of: its recomplauncher.json says, or its README does
/// ("a recompilation of …"), or its own name does ("Some Game Recomp"). Empty when it can't tell.
fn game_in(dir: &Path, raw: Option<&Value>, name: &str) -> String {
    if let Some(g) = raw.and_then(|v| v.get("game")).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
        return g.to_string();
    }
    let readme = ["README.md", "readme.md", "README.txt", "readme.txt", "README"]
        .iter()
        .find_map(|f| std::fs::read(dir.join(f)).ok())
        .map(|b| String::from_utf8_lossy(&b[..b.len().min(20_000)]).to_string())
        .unwrap_or_default();
    let folder = name.replace([' ', '_'], "-");
    crate::ghinfo::game_candidates(&folder, "", &readme)
        .into_iter()
        .find(|g| crate::ghinfo::norm(g) != crate::ghinfo::norm(name))
        .unwrap_or_default()
}

fn read_raw(dir: &Path) -> Option<Value> {
    profile::release_profile(dir).map(|p| p.raw)
        .or_else(|| std::fs::read(dir.join(FILE)).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()))
}

/// The program Play starts: the file's own `start.program`, or the likeliest one in the folder.
fn program_in(dir: &Path, p: Option<&profile::Profile>, name: &str) -> Option<PathBuf> {
    if let Some(start) = p.and_then(|p| p.start.as_ref()) {
        if let Some(found) = start.program.iter().map(|x| PathBuf::from(profile::fill(x, dir, None))).find(|x| profile::named_program(dir.join(x)).is_some()) {
            return Some(found);
        }
    }
    install::find_exe(dir, crate::assets::os_now(), &[name])
}

/// Add a build from a folder on this PC. It shows in the Library like any game, marked as a local
/// build. The folder is never moved, updated or deleted by the launcher.
#[tauri::command(async)]
pub fn dev_add(st: State<'_, App>, path: String) -> Res<Installed> {
    add_folder(&st, path, None, "local")
}

/// Any game the player has that the launcher doesn't know (Add Games You Have, Add It Anyway):
/// a folder, or a program in one. Added the same way as a local build: it plays, keeps time and
/// can go to Steam, but nothing updates it.
#[tauri::command(async)]
pub fn add_any(st: State<'_, App>, path: String) -> Res<Installed> {
    let p = PathBuf::from(&path);
    #[cfg(target_os = "linux")]
    if p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("appimage")) {
        return add_appimage(&st, &p);
    }
    if p.is_file() {
        let dir = p.parent().map(Path::to_path_buf).ok_or_else(|| Error::Msg("That file has no folder.".into()))?;
        let rel = p.file_name().map(PathBuf::from);
        return add_folder(&st, dir.to_string_lossy().to_string(), rel, "own");
    }
    add_folder(&st, path, None, "own")
}

/// The player's own AppImage, on Linux: unpacked into the Recomp Library (see plain::bring_in) as
/// a game of its own, or as the newer version of one brought in the same way before (the launcher
/// doesn't update games it doesn't know, so this is how they update). Not added when it won't unpack.
#[cfg(target_os = "linux")]
fn add_appimage(st: &App, image: &Path) -> Res<Installed> {
    let key = crate::plain::image_key(image);
    let earlier = st.lib.lock().unwrap().installed.values().find(|g| g.local && !key.is_empty() && g.from_appimage == key).map(|g| g.id.clone());
    let id = match &earlier {
        Some(id) => id.clone(),
        None => add_image_record(st, image)?,
    };
    match crate::plain::bring_in(st, &id, image) {
        Ok(b) => crate::plain::tell(&b),
        Err(e) => {
            if earlier.is_none() {
                st.lib.lock().unwrap().forget(&id);
                st.save();
            }
            return Err(e);
        }
    }
    st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))
}

/// A new game of the player's own for their AppImage, before it's unpacked: named by its embedded
/// recomplauncher.json, else by the AppImage's own name. Never another game the player added from
/// the same folder (Downloads holds many).
#[cfg(target_os = "linux")]
fn add_image_record(st: &App, image: &Path) -> Res<String> {
    let dir = image.parent().map(Path::to_path_buf).ok_or_else(|| Error::Msg("That file has no folder.".into()))?;
    let raw: Option<Value> = crate::appimage::manifest(image).ok().and_then(|b| serde_json::from_slice(&b).ok());
    let said = |k: &str| raw.as_ref().and_then(|v| v.get(k)).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    let name = said("name").unwrap_or_else(|| crate::plain::image_title(image));
    let mut l = st.lib.lock().unwrap();
    let base = format!("own-{}", slug(&name));
    let mut id = base.clone();
    let mut n = 2;
    while l.installed.contains_key(&id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    let g = Installed {
        id: id.clone(),
        name,
        game: said("game").unwrap_or_default(),
        repo: said("repo").unwrap_or_default(),
        tag: "own".into(),
        path: dir,
        exe: image.file_name().map(PathBuf::from),
        installed_at: now(),
        updated_at: now(),
        local: true,
        ..Default::default()
    };
    l.put(&id, g);
    Ok(id)
}

/// `tag`: "local" for a developer's build, "own" for a game the player added themselves.
pub(crate) fn add_folder(st: &App, path: String, exe_pick: Option<PathBuf>, tag: &str) -> Res<Installed> {
    let dir = PathBuf::from(&path);
    if !dir.is_dir() {
        return Err(Error::Msg("That folder isn't there.".into()));
    }
    let raw = read_raw(&dir);
    let prof = raw.as_ref().and_then(|v| serde_json::from_value::<profile::Profile>(v.clone()).ok());
    let name = name_of(&dir, raw.as_ref());
    let game = game_in(&dir, raw.as_ref(), &name);
    let exe = exe_pick.filter(|e| dir.join(e).is_file()).or_else(|| program_in(&dir, prof.as_ref(), &name));
    let mut l = st.lib.lock().unwrap();
    // The same folder added again: the entry it already has.
    if let Some(g) = l.installed.values().find(|g| g.local && g.path == dir) {
        return Ok(g.clone());
    }
    let base = format!("{}-{}", if tag == "own" { "own" } else { "local" }, slug(&name));
    let mut id = base.clone();
    let mut n = 2;
    while l.installed.contains_key(&id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    let g = Installed {
        id: id.clone(),
        name,
        game,
        repo: prof.as_ref().map(|p| p.repo.clone()).unwrap_or_default(),
        tag: tag.into(),
        version: String::new(),
        path: dir,
        exe,
        asset: String::new(),
        installed_at: now(),
        updated_at: now(),
        local: true,
        ..Default::default()
    };
    l.put(&id, g);
    let g = l.installed[&id].clone();
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
    /// Kept for older pages; the page shows `flow` and `caps` instead.
    level: u8,
    /// How the game gets the player's copy: "launch" (passed to it when it starts), "prepare" (a
    /// step that runs once before Play), or "" when it asks for nothing.
    flow: &'static str,
    /// What the setup can do besides: "settings", "verify" (known dumps), "inputs" (more than one
    /// file), "saves" (says where it saves).
    caps: Vec<&'static str>,
    items: Vec<Item>,
}

/// The setup's flow and what it can do, read from the profile as it is (no new fields needed).
fn summary(p: &profile::Profile) -> (&'static str, Vec<&'static str>) {
    let flow = if p.prepare.is_some() { "prepare" } else if p.game.is_some() || !p.inputs.is_empty() { "launch" } else { "" };
    let mut caps = vec![];
    if !p.settings.is_empty() {
        caps.push("settings");
    }
    if p.game.as_ref().map(|g| !g.dumps.is_empty()).unwrap_or(false) {
        caps.push("verify");
    }
    if p.inputs.len() > 1 {
        caps.push("inputs");
    }
    if !p.saves.is_empty() {
        caps.push("saves");
    }
    (flow, caps)
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
    let embedded = profile::release_profile(&dir).filter(|p| p.embedded);
    let mut items = vec![];
    let mut error = String::new();
    let mut prof: Option<profile::Profile> = None;
    let has_file = file.is_file() || embedded.is_some();
    if has_file {
        let text = embedded.as_ref().map(|p| Ok(p.raw.to_string())).unwrap_or_else(|| std::fs::read_to_string(&file));
        match text {
            Err(e) => error = format!("It can't be read: {e}"),
            Ok(text) => match serde_json::from_str::<Value>(&text) {
                Err(e) => error = format!("It isn't valid JSON: {e}."),
                Ok(v) => {
                    if let Some(o) = v.as_object() {
                        let unknown: Vec<&String> = o.keys().filter(|k| !TOP_KEYS.contains(&k.as_str()) && !IGNORED_KEYS.contains(&k.as_str())).collect();
                        let old: Vec<&String> = o.keys().filter(|k| IGNORED_KEYS.contains(&k.as_str())).collect();
                        if !old.is_empty() {
                            items.push(warn(
                                format!("Ignored: {}", old.iter().map(|k| format!("\"{k}\"")).collect::<Vec<_>>().join(", ")),
                                "The launcher reads the recomplauncher.json in your release, so it never has to say which launcher it needs. You can delete them.",
                            ));
                        }
                        if !unknown.is_empty() {
                            items.push(warn(
                                format!("Not part of the format: {}", unknown.iter().map(|k| format!("\"{k}\"")).collect::<Vec<_>>().join(", ")),
                                "These are skipped. Check the spelling against the developer guide.",
                            ));
                        }
                        // Several games in one recomp: each with its own tile, time and achievements.
                        if let Some(list) = o.get("games") {
                            match crate::games::check(list) {
                                Err(why) => items.push(err("\"games\" can't be used as it is", why)),
                                Ok(()) => match list.as_array().map(Vec::len).unwrap_or(0) {
                                    0 | 1 => items.push(warn("\"games\" lists fewer than two games", "With one game or none, the recomp shows as one game, as before.")),
                                    n => items.push(ok(format!("Carries {n} games, each with its own tile"))),
                                },
                            }
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
    // What's actually used: embedded metadata, the folder file, then catalog/built-in.
    let cat = crate::cached_catalog(&st);
    let entry = catalog::app(&cat, &id).cloned().unwrap_or(Value::Null);
    let used = profile::find(&dir, &g.repo, &entry);
    let source = match (&prof, &used) {
        (Some(p), Some(_)) if p.format == 1 => if embedded.is_some() { "appimage" } else { "folder" },
        (_, Some(_)) if entry.get("launcher_from").and_then(Value::as_str) == Some("repo") => "repo",
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
    let location = embedded.map(|p| p.location).unwrap_or_else(|| file.display().to_string());
    let (flow, caps) = prof.as_ref().filter(|p| p.format == 1).or(used.as_ref()).map(summary).unwrap_or(("", vec![]));
    Ok(Report { source, file: location, has_file, error, level, flow, caps, items })
}

/// What "game.check" lets the launcher do with the player's copy.
fn check_item(c: &profile::Check) -> Item {
    if c.file.is_empty() && c.xdvdfs.is_empty() && c.iso9660.is_empty() && c.nup_build_id.is_empty() {
        return warn("No \"game.check\"", "Any disc or folder is accepted, and the launcher can't find the player's copy in their folders.");
    }
    // Every PlayStation game folder has these, so without title IDs the folder is found only by
    // its title, and that often has an edition or subtitle the game's name doesn't.
    let file_name = c.file.rsplit(['/', '\\']).next().unwrap_or("");
    let shared = file_name.eq_ignore_ascii_case("eboot.bin") || file_name.eq_ignore_ascii_case("param.sfo");
    if shared && c.ids.is_empty() {
        return warn(
            "\"game.check\" lists no title IDs",
            format!("Every PlayStation game has {file_name}, so the launcher finds the player's folder only by its title, which can have an edition or subtitle your game's name doesn't. Add \"ids\" with each region's title ID, like [\"CUSA00242\"]."),
        );
    }
    ok("Checks the player's copy and finds it in their folders")
}

/// The checks, top to bottom in the order of the developer guide. Gives back the level reached.
fn check_profile(p: &profile::Profile, dir: &Path, g: &Installed, items: &mut Vec<Item>) -> u8 {
    let mut level = 0u8;
    // Level 1: start and game
    if !p.inputs.is_empty() {
        match crate::inputs::validate(p) {
            Ok(()) => items.push(ok(format!("{} named setup inputs; multidisc hashing deferred", p.inputs.len()))),
            Err(why) => items.push(err("Invalid named setup inputs", why)),
        }
    }
    match p.start.as_ref() {
        None => items.push(warn("No \"start\"", "Play starts the program the launcher found, with no arguments of yours.")),
        Some(s) if s.program.is_empty() => items.push(err("\"start.program\" is empty", "List the path of your program, from the top of your release.")),
        Some(s) => match s.program.iter().find(|x| profile::named_program(dir.join(profile::fill(x, dir, None))).is_some()) {
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
        items.push(check_item(&gc.check));
        for d in &gc.dumps {
            if d.size == 0 || d.crc32.trim().is_empty() {
                items.push(warn(format!("The \"{}\" dump needs both \"size\" and \"crc32\"", d.name), "Take them from Redump."));
            }
        }
        match g.setup_inputs.get("game") {
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
        let f = crate::saves::resolve_for(t, &g);
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
        if let Some(sl) = s.slider.as_ref() {
            if !(sl.min < sl.max) {
                items.push(err(format!("\"{}\": the slider's \"min\" has to be below its \"max\"", s.label), "Give the lowest and highest values the game takes."));
                continue;
            }
            if s.target == "args" {
                let list = if s.args.is_empty() { &sl.args } else { &s.args };
                if !list.iter().any(|a| a.contains("{value}")) {
                    items.push(err(format!("\"{}\": no {{value}} in its \"args\"", s.label), "Write where the number goes, like [\"--sens\", \"{value}\"]."));
                    continue;
                }
                items.push(ok(format!("{}: a slider on the command line, {} to {}", s.label, sl.min, sl.max)));
                continue;
            }
            let key = if sl.key.is_empty() { s.id.as_str() } else { sl.key.as_str() };
            let has = p.config_file(&s.target, dir).and_then(|f| f.get(key)).is_some() || s.defaults.contains_key(key);
            if has {
                items.push(ok(format!("{}: {} (a slider, {} to {})", s.label, p.slider_now(s, dir).unwrap_or_default(), sl.min, sl.max)));
            } else {
                items.push(warn(format!("{}: the file has no \"{key}\"", s.label), "Put the game's own value in \"defaults\" so the slider starts there."));
            }
            continue;
        }
        if let Some(b) = s.bindings.as_ref() {
            if b.actions.is_empty() || b.actions.iter().any(|a| a.key.trim().is_empty()) {
                items.push(err(format!("\"{}\": every action in its \"bindings\" needs a \"key\"", s.label), "List the actions, each with the key it has in the file."));
                continue;
            }
            if s.target == "args" {
                items.push(err(format!("\"{}\": key bindings go in a settings file", s.label), "Put it \"in\" one of your \"config\" files."));
                continue;
            }
            let f = p.config_file(&s.target, dir);
            let missing: Vec<String> = b.actions.iter().filter(|a| f.as_ref().and_then(|f| f.get(&a.key)).is_none() && !s.defaults.contains_key(&a.key)).map(|a| format!("\"{}\"", a.key)).collect();
            if missing.is_empty() {
                items.push(ok(format!("{}: {} actions (key bindings)", s.label, b.actions.len())));
            } else {
                items.push(warn(format!("{}: the file has no {}", s.label, missing.join(", ")), "Put the game's own keys in \"defaults\" so each action shows them."));
            }
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
        match pr.program_in(dir) {
            Some(x) => {
                items.push(ok(format!("Setup step: {}", x.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())));
                if level >= 2 {
                    level = 3;
                }
            }
            None => items.push(err("None of \"prepare.program\" is in this folder", pr.program.join(", "))),
        }
        items.extend(option_items(&pr.options));
        items.extend(version_item(&pr.version));
        items.extend(done_item(p));
        let bad: Vec<&str> = pr.env.keys().filter(|k| !profile::env_name_ok(k)).map(String::as_str).collect();
        if !bad.is_empty() {
            items.push(warn(format!("Left out of \"prepare.env\": {}", bad.join(", ")),
                "Names are letters, digits and _. The launcher's own RECOMPLAUNCHER_ ones and system ones like PATH and TEMP can't be set: the step gets a temp folder of its own."));
        }
        items.extend(keeps_copy_items(p, dir));
    }
    items.extend(system_item(p));
    items.extend(outside_item(p));
    level
}

/// `system`: values the launcher can't read as a version, or a graphics API it doesn't know. Those
/// are ignored, so the PC is never checked against them.
fn system_item(p: &profile::Profile) -> Option<Item> {
    let bad = crate::sysneeds::bad_values(p.system.as_ref()?);
    if bad.is_empty() {
        return None;
    }
    Some(warn(
        format!("Not read in \"system\": {}", bad.join(", ")),
        format!("Versions are numbers with dots (\"10\", \"2.35\", \"1.3\"), and graphics APIs are {}. Linux builds give glibc, not a version.", crate::sysneeds::GRAPHICS.join(", ")),
    ))
}

/// A step that builds the game from the player's copy, without `prepare.done`: the launcher can't
/// tell the build is there, so it can't keep the game set up after an update, or use a build the
/// player already has. Only then: a step that only fetches extras needs no list. A keeps_copy
/// step gets its own, more exact warning from keeps_copy_items.
fn done_item(p: &profile::Profile) -> Option<Item> {
    let pr = p.prepare.as_ref()?;
    let from_copy = p.game.is_some() || !p.inputs.is_empty();
    if !from_copy || !pr.done.is_empty() || pr.keeps_copy {
        return None;
    }
    Some(warn(
        "No \"prepare.done\"",
        "List a file your step always leaves, like \"{data}/build/game.pak\": the launcher then knows the game is built, keeps it set up after an update, and can use a build the player already has.",
    ))
}

/// The places outside the game's folder a profile has the game write to (saves, settings, builds,
/// the folders it passes). Not wrong, but everything in `{install}` or `{data}` moves, backs up and
/// uninstalls with the game, and nothing is left behind.
fn outside_item(p: &profile::Profile) -> Option<Item> {
    const OUTSIDE: [&str; 5] = ["{home}", "{local}", "{config}", "{documents}", "{appdata}"];
    let mut written: Vec<&str> = p.saves.iter().map(String::as_str).collect();
    written.extend(p.config.iter().flat_map(|c| c.file.iter().map(|f| f.path.as_str())));
    written.extend(p.folders.values().flatten().map(|f| f.path.as_str()));
    if let Some(s) = p.start.as_ref() {
        written.extend(s.args.iter().chain(s.env.values()).map(String::as_str));
    }
    if let Some(pr) = p.prepare.as_ref() {
        written.extend(pr.args.iter().chain(pr.env.values()).chain(pr.done.iter()).map(String::as_str));
    }
    let mut outside: Vec<&str> = written.into_iter().filter(|w| OUTSIDE.iter().any(|o| w.contains(o))).collect();
    outside.sort();
    outside.dedup();
    if outside.is_empty() {
        return None;
    }
    Some(warn(
        format!("Writes outside {{install}} and {{data}}: {}", outside.join(", ")),
        "Keep saves, settings, caches and builds in {data} (or {install}): they then move, back up and uninstall with the game. Players' saves already elsewhere are fine to keep reading.",
    ))
}

/// prepare.keeps_copy: run again without the copy, leaving out the arguments with {game} in them.
/// Only what would make that run go wrong, or never happen, is said; nothing stops the game.
fn keeps_copy_items(p: &profile::Profile, dir: &Path) -> Vec<Item> {
    let Some(pr) = p.prepare.as_ref().filter(|pr| pr.keeps_copy) else { return vec![] };
    let mut out = vec![];
    // "--game", "{game}": leaving out "{game}" alone would hand --game the next argument.
    if pr.args.iter().any(|a| a == "{game}") {
        out.push(warn("\"{game}\" is an argument of its own in \"prepare.args\"", "With keeps_copy, the launcher leaves out every argument with {game} in it when it builds again without the copy. Write the copy as one argument, like \"--game={game}\"."));
    }
    if p.build_home(dir).is_none() {
        out.push(warn("\"prepare.keeps_copy\" has no build folder to look in", "List the folder your step keeps the copy's files in as the first \"folders.game\" (not the game's own folder). Without it, players are asked for their copy again."));
    }
    if pr.done.is_empty() {
        out.push(warn("\"prepare.keeps_copy\" without \"prepare.done\"", "List the files a finished build has, so the launcher can tell when it has to build again."));
    }
    out
}

/// prepare.version: changing it is how a json asks players who set up already to run it again.
fn version_item(v: &str) -> Option<Item> {
    if v.is_empty() {
        return None;
    }
    Some(if v.trim() != v {
        warn(format!("\"prepare.version\" is \"{v}\" with spaces around it"), "Spaces count too: take them out so it doesn't change by accident.")
    } else {
        ok(format!("Setup step version: {v}. Changing it asks everyone who set up already to run setup again"))
    })
}

/// What's wrong with a setup step's options (prepare.options), in plain words, or what's fine.
fn option_items(opts: &[profile::PrepareOption]) -> Vec<Item> {
    let mut items = vec![];
    let mut seen = BTreeSet::new();
    for (i, o) in opts.iter().enumerate() {
        let name = if o.label.trim().is_empty() { format!("Option {}", i + 1) } else { o.label.clone() };
        let id_ok = !o.id.is_empty() && o.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        let mut bad = vec![];
        if o.id.is_empty() {
            bad.push("it has no \"id\"".to_string());
        } else if !id_ok {
            bad.push(format!("its id \"{}\" can only have a-z, 0-9 and _", o.id));
        } else if !seen.insert(o.id.clone()) {
            bad.push(format!("another option already has the id \"{}\"", o.id));
        }
        if o.label.trim().is_empty() {
            bad.push("it has no \"label\" to show the player".into());
        }
        if o.args.is_empty() {
            bad.push("it has no \"args\", so ticking it changes nothing".into());
        }
        for os in o.os.iter().filter(|x| !["windows", "linux", "macos"].contains(&x.to_ascii_lowercase().as_str())) {
            bad.push(format!("\"os\" has \"{os}\": it takes windows, linux and macos"));
        }
        if !o.gpu.is_empty() && !["nvidia", "amd"].contains(&o.gpu.to_ascii_lowercase().as_str()) {
            bad.push(format!("\"gpu\" is \"{}\": it takes nvidia or amd", o.gpu));
        }
        if let Some(l) = &o.license {
            if l.url.trim().is_empty() || !l.url.starts_with("https://") {
                bad.push("its license has no https \"url\" for the player to read it at".into());
            }
            if l.name.trim().is_empty() {
                bad.push("its license has no \"name\"".into());
            }
        }
        if bad.is_empty() {
            let when = [(!o.os.is_empty()).then(|| o.os.join(", ")), (!o.gpu.is_empty()).then(|| format!("{} cards", o.gpu))].into_iter().flatten().collect::<Vec<_>>().join(", ");
            let lic = if o.license.is_some() { ", starts off: the player accepts its license" } else if o.start_on == Some(false) { ", starts off" } else { ", starts on" };
            let req = if o.required { ", required: setup waits until it's ticked" } else { "" };
            items.push(ok(format!("Setup option: {name}{}{lic}{req}", if when.is_empty() { String::new() } else { format!(" (only on {when})") })));
        } else {
            items.push(err(format!("Setup option \"{name}\": {}", bad.join("; ")), "See prepare.options in the developer guide."));
        }
    }
    items
}

/// Write a starter recomplauncher.json into a build's folder (only when it has none).
#[tauri::command(async)]
pub fn dev_starter(st: State<'_, App>, id: String) -> Res<String> {
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't in your Library.".into()))?;
    let file = g.path.join(FILE);
    if file.exists() || profile::release_profile(&g.path).map(|p| p.embedded).unwrap_or(false) {
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

#[cfg(test)]
mod tests {
    #[test]
    fn keeps_copy_says_what_would_stop_a_build_without_the_copy() {
        let p: crate::profile::Profile = serde_json::from_value(serde_json::json!({
            "prepare": { "program": ["Builder.exe"], "args": ["--game", "{game}"], "keeps_copy": true },
            "start": { "program": ["Builder.exe"] }
        })).unwrap();
        let items = super::keeps_copy_items(&p, &std::env::temp_dir().join("x"));
        let said: Vec<&str> = items.iter().map(|i| i.text.as_str()).collect();
        assert_eq!(items.len(), 3, "{said:?}");
        assert!(items.iter().all(|i| i.level == "warn"));
        let fine: crate::profile::Profile = serde_json::from_value(serde_json::json!({
            "prepare": { "program": ["Builder.exe"], "args": ["--game={game}"], "done": ["{install}/Game/game"], "keeps_copy": true },
            "start": { "program": ["Builder.exe"] },
            "folders": { "game": [{ "path": "{install}/Game" }] }
        })).unwrap();
        assert!(super::keeps_copy_items(&fine, &std::env::temp_dir().join("x")).is_empty());
    }

    #[test]
    fn setup_options_are_checked_in_plain_words() {
        let p: crate::profile::Prepare = serde_json::from_str(r#"{"program":["Builder.exe"],"options":[
            {"id":"dlss","label":"Upscaler A","args":["--a"],"gpu":"nvidia","license":{"name":"A license","url":"https://example.com/a"}},
            {"id":"Bad Id","label":"","args":[],"os":["dos"],"gpu":"intel","license":{"name":"","url":"ftp://x"}},
            {"id":"dlss","label":"Twice","args":["--b"],"extra":"ignored"}]}"#).unwrap();
        let items = super::option_items(&p.options);
        assert_eq!(items.len(), 3);
        assert!(items[0].text.contains("Upscaler A") && items[0].text.contains("starts off"), "{}", items[0].text);
        let bad = &items[1].text;
        for w in ["a-z, 0-9", "no \"label\"", "no \"args\"", "\"dos\"", "\"intel\"", "https \"url\"", "no \"name\""] {
            assert!(bad.contains(w), "{w}: {bad}");
        }
        assert!(items[2].text.contains("already has the id"));
    }

    #[test]
    fn a_required_setup_option_is_named_as_required() {
        let p: crate::profile::Prepare = serde_json::from_str(r#"{"program":["Builder.exe"],"options":[
            {"id":"vs","label":"Build tools","args":["--vs"],"required":true,"license":{"name":"VS","url":"https://example.com/vs"}}]}"#).unwrap();
        let items = super::option_items(&p.options);
        assert!(items[0].text.contains("starts off") && items[0].text.contains("required"), "{}", items[0].text);
    }

    fn profile(json: &str) -> crate::profile::Profile {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn a_step_building_from_the_copy_without_done_is_warned() {
        let p = profile(r#"{"format":1,"game":{"kind":"iso"},"prepare":{"program":["Builder.exe"],"args":["{game}"]}}"#);
        let item = super::done_item(&p).unwrap();
        assert_eq!(item.level, "warn");
        assert!(item.text.contains("prepare.done"), "{}", item.text);
    }

    #[test]
    fn a_step_with_done_or_without_a_copy_is_left_alone() {
        let with_done = profile(r#"{"format":1,"game":{"kind":"iso"},"prepare":{"program":["Builder.exe"],"done":["{data}/build/game.pak"]}}"#);
        assert!(super::done_item(&with_done).is_none());
        let extras_only = profile(r#"{"format":1,"prepare":{"program":["Fetcher.exe"]}}"#);
        assert!(super::done_item(&extras_only).is_none());
        let no_step = profile(r#"{"format":1,"game":{"kind":"iso"}}"#);
        assert!(super::done_item(&no_step).is_none());
    }

    #[test]
    fn writing_outside_the_game_folder_is_named() {
        let p = profile(r#"{"format":1,"saves":["{local}/Game/saves","{data}/saves"],
            "prepare":{"program":["Builder.exe"],"args":["--out","{home}/Games/Game"],"env":{"CACHE":"{data}/cache"}}}"#);
        let item = super::outside_item(&p).unwrap();
        assert_eq!(item.level, "warn");
        assert!(item.text.contains("{local}/Game/saves") && item.text.contains("{home}/Games/Game"), "{}", item.text);
        assert!(!item.text.contains("{data}/"), "{}", item.text);
    }

    #[test]
    fn writing_only_in_install_and_data_is_fine() {
        let p = profile(r#"{"format":1,"saves":["{data}/saves","{install}/settings.toml"],"start":{"program":["g.exe"],"args":["--data","{data}"]}}"#);
        assert!(super::outside_item(&p).is_none());
    }

    #[test]
    fn a_system_value_the_launcher_cant_read_is_named() {
        let p: crate::profile::Profile = serde_json::from_str(r#"{"format":1,"system":{"linux":{"glibc":"latest","graphics":{"vulkan":"1.3"}}}}"#).unwrap();
        let item = super::system_item(&p).unwrap();
        assert_eq!(item.level, "warn");
        assert!(item.text.contains("system.linux.glibc"), "{}", item.text);
    }

    #[test]
    fn a_system_part_that_reads_well_is_left_alone() {
        let p: crate::profile::Profile = serde_json::from_str(r#"{"format":1,"system":{"windows":{"version":"10","graphics":{"d3d12":"12_0"}},"linux":{"glibc":"2.35"}}}"#).unwrap();
        assert!(super::system_item(&p).is_none());
        let none: crate::profile::Profile = serde_json::from_str(r#"{"format":1}"#).unwrap();
        assert!(super::system_item(&none).is_none());
    }

    #[test]
    fn a_playstation_check_without_title_ids_is_warned() {
        let c = crate::profile::Check { file: "eboot.bin".into(), ..Default::default() };
        let item = super::check_item(&c);
        assert_eq!(item.level, "warn");
        assert!(item.text.contains("no title IDs"), "{}", item.text);
    }

    #[test]
    fn a_ps3_check_by_its_usrdir_eboot_without_title_ids_is_warned() {
        let c = crate::profile::Check { file: "USRDIR/EBOOT.BIN".into(), ..Default::default() };
        assert!(super::check_item(&c).detail.contains("EBOOT.BIN"));
    }

    #[test]
    fn a_playstation_check_with_title_ids_is_fine() {
        let c = crate::profile::Check { file: "eboot.bin".into(), ids: vec!["CUSA00242".into()], ..Default::default() };
        assert_eq!(super::check_item(&c).level, "ok");
    }

    #[test]
    fn a_check_by_a_games_own_file_needs_no_ids() {
        let c = crate::profile::Check { file: "data/sanctuary.pak".into(), ..Default::default() };
        assert_eq!(super::check_item(&c).level, "ok");
    }

    #[test]
    fn the_setup_steps_version_is_named_in_the_check() {
        assert!(super::version_item("").is_none());
        assert!(super::version_item("2").unwrap().text.contains("run setup again"));
        assert!(super::version_item(" 2").unwrap().text.contains("spaces"));
    }

    use super::*;

    fn schema() -> Value {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs");
        serde_json::from_str(&std::fs::read_to_string(root.join("recomplauncher.schema.json")).unwrap()).unwrap()
    }

    // The docs and the schema stay in step with the launcher (AGENTS.md): recomplauncher.com copies
    // docs/recomplauncher.schema.json from main, so a feature that isn't in it doesn't exist there.

    #[test]
    fn the_schema_is_the_one_the_website_copies() {
        let s = schema();
        assert_eq!(s["$id"], "https://recomplauncher.com/schema/recomplauncher.json");
        assert!(s["properties"].get("format").is_some());
        assert!(s["properties"].get("start").is_some());
    }

    #[test]
    fn the_schema_the_guide_and_the_checker_list_the_same_keys() {
        // Every top-level key the schema allows is known to the checker and described in the guide.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs");
        let s = schema();
        let guide = std::fs::read_to_string(root.join("DEVELOPERS.md")).unwrap();
        let props = s["properties"].as_object().unwrap();
        for k in props.keys() {
            assert!(TOP_KEYS.contains(&k.as_str()), "{k} is in the schema but not the checker");
            assert!(guide.contains(&format!("`{k}`")) || guide.contains(&format!("\"{k}\"")), "{k} is in the schema but not the guide");
        }
        for k in TOP_KEYS {
            assert!(props.contains_key(*k), "{k} is in the checker but not the schema");
        }
        let games = &props["games"]["items"];
        assert_eq!(games["required"], serde_json::json!(["id", "name"]));
        for f in ["id", "name", "args", "title_id", "inputs", "saves"] {
            assert!(games["properties"].get(f).is_some(), "games[].{f}");
        }
    }

    #[test]
    fn requires_and_min_are_the_stores_not_the_sdks() {
        // boot::FEATURES stays for the store's catalog profiles (boot::fit); a developer's manifest
        // ships in its own release, so it never says which launcher it needs.
        let s = schema();
        for k in IGNORED_KEYS {
            assert!(s["properties"].get(*k).is_none(), "{k} is still in the schema");
            assert!(!TOP_KEYS.contains(k));
        }
        assert!(s["definitions"].get("launcherFeature").is_none());
    }

    #[test]
    fn the_shared_types_and_roles_are_the_schemas() {
        let s = schema();
        let input = &s["properties"]["inputs"]["items"]["properties"];
        let list = |v: &Value| v["enum"].as_array().unwrap().iter().filter_map(Value::as_str).map(str::to_string).collect::<std::collections::BTreeSet<_>>();
        let shared: std::collections::BTreeSet<String> = ["switch.prod-keys", "switch.title-keys", "wiiu.common-key"]
            .iter()
            .filter(|k| crate::inputs::key_name(k).is_some())
            .map(|k| k.to_string())
            .chain([crate::dlss::SHARED.to_string()])
            .collect();
        assert_eq!(list(&input["shared"]), shared);
        assert_eq!(list(&input["role"]), crate::inputs::roles().iter().map(|r| r.to_string()).collect());
    }

    #[test]
    fn every_field_a_developer_writes_is_in_the_schema() {
        // Read from profile.rs itself, so a new field can't be added without one or the other.
        const INTERNAL: [&str; 5] = ["input_paths", "skipped_inputs", "windows_build", "from_boot", "slider_vals"];
        // A Windows checkout has CRLF line endings.
        let src = include_str!("profile.rs").replace("\r\n", "\n");
        let body = &src[src.find("pub struct Profile {").unwrap() + "pub struct Profile {".len()..];
        let body = &body[..body.find("\n}\n").unwrap()];
        let mut attrs = String::new();
        let props = schema()["properties"].as_object().unwrap().clone();
        for line in body.lines().map(str::trim) {
            if line.starts_with("#[") {
                attrs.push_str(line);
                continue;
            }
            let Some(rest) = line.strip_prefix("pub ") else { continue };
            let name = rest.split(':').next().unwrap().trim();
            let skipped = attrs.contains("serde(skip)");
            let shown = attrs.split("rename = \"").nth(1).and_then(|r| r.split('"').next()).unwrap_or(name).to_string();
            attrs.clear();
            if skipped {
                assert!(INTERNAL.contains(&name), "{name} is skipped by serde but not on the internal list");
                continue;
            }
            assert!(!INTERNAL.contains(&name), "{name} is on the internal list but a developer can write it");
            assert!(props.contains_key(&shown), "Profile.{name} isn't in docs/recomplauncher.schema.json");
        }
    }
}

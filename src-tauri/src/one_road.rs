//! Moving to one road (a game's setup comes from the project's own recomplauncher.json, or from a
//! profile of ours the store turned on by hand for that game): a game the player set up with one of
//! our older profiles, which the catalog no longer keeps, becomes the player's own ("Set Up by You")
//! once. What that profile did at every start goes where the player keeps their own: its program
//! (the game's executable), its arguments and variables (the launch options) and its save folder
//! (the Saves tab's folder). A game that can't be moved exactly is left as it is, with the reason
//! kept, and is set up again by the player once the catalog stops carrying our profile.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::library::Installed;
use crate::profile::{Origin, Profile};

/// What a game becomes: its program (relative to its folder; None keeps what it has), its launch
/// options, and a save folder when ours named one and the player hasn't picked their own.
#[derive(Debug, PartialEq, Eq)]
pub struct Moved {
    pub exe: Option<PathBuf>,
    pub args: String,
    pub save_place: Option<String>,
}

/// Whether the catalog says this game's profile is ours and no longer the way it should be set up:
/// `launcher_from: "store"`, or only an old-style `boot`. A catalog that doesn't say whose (from
/// before `launcher_from`) is left alone, so a project's own file is never taken for ours.
pub fn ours_to_move(entry: &Value) -> bool {
    match entry.get("launcher") {
        Some(_) => entry.get("launcher_from").and_then(Value::as_str) == Some("store"),
        None => entry.get("boot").is_some_and(|b| b.is_object()),
    }
}

/// How `p` (ours) started the game, as the player's own settings. Err: why it can't be moved exactly.
pub fn plan(g: &Installed, p: &Profile, copy: Option<&Path>, windows_build: bool) -> Result<Moved, String> {
    if !matches!(p.origin, Origin::Store | Origin::Boot) {
        return Err("its profile isn't ours".into());
    }
    if !g.games.is_empty() {
        return Err("a recomp with several games keeps no launch options of its own for each".into());
    }
    let existing = g.args.trim();
    if existing.contains(crate::launch_options::COMMAND) {
        return Err("its launch options already set variables".into());
    }
    let save_place = save_folder(g, p);
    let Some(_) = p.start.as_ref() else {
        // Ours never chose what to start: Play already runs the game's own program.
        return Ok(Moved { exe: None, args: existing.to_string(), save_place });
    };
    if copy.is_none() && p.start_uses_game() {
        return Err("it isn't set up yet: the player sets it up".into());
    }
    let (exe, args) = p.command_for(&g.path, copy, &g.picks, windows_build).ok_or("its program isn't in the game's folder")?;
    let env = p.env_vars(&g.path, copy);
    if proton_passes_copy(copy, windows_build, &args, &env) {
        return Err("through Proton the copy's path is written the Windows way, which a moved library or copy would break".into());
    }
    if args.iter().chain(env.iter().map(|(_, v)| v)).any(|a| a.contains('"')) {
        return Err("an argument has a quote in it, which launch options can't carry".into());
    }
    let rel = exe.strip_prefix(&g.path).map(Path::to_path_buf).map_err(|_| "its program is outside the game's folder".to_string())?;
    let ours = args.iter().map(|a| quoted(a)).collect::<Vec<_>>().join(" ");
    let vars = env.iter().map(|(k, v)| format!("{k}={}", quoted(v))).collect::<Vec<_>>().join(" ");
    let text = if vars.is_empty() { format!("{ours} {existing}") } else { format!("{vars} {} {ours} {existing}", crate::launch_options::COMMAND) };
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    // It has to read back as exactly what ours did, or it isn't moved.
    let back = crate::launch_options::parse(&text).map_err(|e| e.to_string())?;
    let mut want = args.clone();
    want.extend(crate::split_args(existing));
    if back.env != env || back.args != want {
        return Err("its start can't be written as launch options exactly".into());
    }
    Ok(Moved { exe: Some(rel), args: text, save_place })
}

/// Under Proton a Windows build is given its copy as a Windows path (Z:\...), in arguments or
/// variables: kept as text, that breaks the day the library or the copy moves.
fn proton_passes_copy(copy: Option<&Path>, windows_build: bool, args: &[String], env: &[(String, String)]) -> bool {
    let Some(copy) = copy else { return false };
    if cfg!(windows) || !windows_build {
        return false;
    }
    let wine = crate::profile::wine_path(&copy.to_string_lossy());
    args.iter().chain(env.iter().map(|(_, v)| v)).any(|a| a.contains(&wine) || a.contains(&*copy.to_string_lossy()))
}

/// Ours named one save folder (no patterns), and the player hasn't picked their own.
fn save_folder(g: &Installed, p: &Profile) -> Option<String> {
    if g.save_place.is_some() || p.saves.len() != 1 || p.saves[0].contains('*') {
        return None;
    }
    Some(crate::saves::resolve_for(&p.saves[0], g).to_string_lossy().into_owned())
}

fn quoted(s: &str) -> String {
    if s.is_empty() || s.chars().any(char::is_whitespace) {
        format!("\"{s}\"")
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    // Off Windows a .exe program goes through Proton, which writes paths the Windows way.
    const NATIVE: &str = if cfg!(windows) { "game.exe" } else { "game" };

    fn game(dir: &Path) -> Installed {
        Installed { id: "x".into(), path: dir.to_path_buf(), tag: "v1".into(), ..Default::default() }
    }
    fn ours(v: Value, origin: Origin) -> Profile {
        let mut p: Profile = serde_json::from_value(v).unwrap();
        p.origin = origin;
        p
    }
    fn dir_with(name: &str, program: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rl-oneroad-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(program), b"MZ").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(d.join(program), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        d
    }

    #[test]
    fn only_profiles_the_catalog_says_are_ours_move() {
        assert!(ours_to_move(&serde_json::json!({ "launcher": {}, "launcher_from": "store" })));
        assert!(ours_to_move(&serde_json::json!({ "boot": { "files": { "how": "flag" } } })));
        assert!(!ours_to_move(&serde_json::json!({ "launcher": {}, "launcher_from": "release" })));
        assert!(!ours_to_move(&serde_json::json!({ "launcher": {} })), "a catalog that doesn't say whose is left alone");
        assert!(!ours_to_move(&serde_json::json!({})));
    }

    #[test]
    fn a_copy_passed_as_an_argument_becomes_launch_options() {
        let d = dir_with("flag", NATIVE);
        let copy = d.join("My Disc.iso");
        let p = ours(serde_json::json!({ "format": 1, "start": { "program": [NATIVE], "args": ["--iso", "{game}"] } }), Origin::Store);
        let m = plan(&game(&d), &p, Some(&copy), false).unwrap();
        assert_eq!(m.exe, Some(PathBuf::from(NATIVE)));
        assert_eq!(m.args, format!("--iso \"{}\"", copy.display()));
        assert_eq!(crate::launch_options::parse(&m.args).unwrap().args, vec!["--iso".to_string(), copy.to_string_lossy().into_owned()]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_copy_passed_in_a_variable_becomes_a_variable_before_command() {
        let d = dir_with("env", NATIVE);
        let copy = d.join("ico.iso");
        let p = ours(serde_json::json!({ "format": 1, "start": { "program": [NATIVE], "env": { "ICO_ISO": "{game}" } } }), Origin::Boot);
        let mut g = game(&d);
        g.args = "--fullscreen".into();
        let m = plan(&g, &p, Some(&copy), false).unwrap();
        let back = crate::launch_options::parse(&m.args).unwrap();
        assert_eq!(back.env, vec![("ICO_ISO".to_string(), copy.to_string_lossy().into_owned())]);
        assert_eq!(back.args, vec!["--fullscreen".to_string()], "the player's own options stay, after ours");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn what_cant_be_moved_exactly_is_left_with_the_reason() {
        let d = dir_with("again", "game.exe");
        let p = ours(serde_json::json!({ "format": 1, "start": { "program": ["game.exe"], "args": ["--name=\"x\""] } }), Origin::Store);
        assert!(plan(&game(&d), &p, None, false).unwrap_err().contains("quote"));
        let mut several = game(&d);
        several.games.insert("a".into(), Default::default());
        assert!(plan(&several, &p, None, false).unwrap_err().contains("several"));
        let mut vars = game(&d);
        vars.args = "A=1 %command%".into();
        assert!(plan(&vars, &p, None, false).unwrap_err().contains("variables"));
        let project = ours(serde_json::json!({ "format": 1, "start": { "program": ["game.exe"] } }), Origin::Release);
        assert!(plan(&game(&d), &project, None, false).is_err(), "the project's own file is never moved");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn proton_games_that_pass_the_copy_are_set_up_again() {
        let d = dir_with("proton", "game.exe");
        let copy = d.join("disc.iso");
        let p = ours(serde_json::json!({ "format": 1, "start": { "program": ["game.exe"], "args": ["--iso", "{game}"] } }), Origin::Store);
        assert!(plan(&game(&d), &p, Some(&copy), true).unwrap_err().contains("Proton"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn ours_without_a_start_keeps_the_games_program_and_moves_its_save_folder() {
        let d = dir_with("saves", "game.exe");
        let p = ours(serde_json::json!({ "format": 1, "saves": ["{install}/saves"] }), Origin::Store);
        let m = plan(&game(&d), &p, None, false).unwrap();
        assert_eq!(m.exe, None);
        assert_eq!(m.save_place, Some(d.join("saves").to_string_lossy().into_owned()));
        let _ = BTreeMap::<String, usize>::new();
        let _ = std::fs::remove_dir_all(&d);
    }
}

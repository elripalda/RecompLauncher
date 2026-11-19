//! Libraries saved by older launchers, moved to the current format once, as they're read.
//!
//! Beta only: deleted at 1.0. A library older than that then starts its setups again.
//!
//! Format 2 (Beta 65): who sets each game up is the game's own `mode`. Before, it was worked out
//! from three places: Set Up By Hand being on (`manual`), the Skip Easy Setup list in the
//! settings (`ui.easy_skip`), and a program the player picked (`exe_rules` 255, which turned
//! Easy Setup off for that game at every start).

use serde_json::Value;

/// The format this launcher writes.
pub const FORMAT: u32 = 2;

/// A program the player picked (`Installed::exe_rules`): the launcher never picks again over it.
const PLAYER_PICK: u64 = u8::MAX as u64;

/// Brings a library file's JSON up to `FORMAT`, before it's read. True when it changed anything.
pub fn to_current(v: &mut Value) -> bool {
    let from = v.get("format").and_then(Value::as_u64).unwrap_or(1);
    if from >= FORMAT as u64 {
        return false;
    }
    modes_from_old_fields(v);
    v["format"] = FORMAT.into();
    true
}

/// Format 1 -> 2: each installed game's mode, from the fields that said it before. The Skip Easy
/// Setup list keeps only games that aren't installed (a choice made before installing).
fn modes_from_old_fields(v: &mut Value) {
    let skip: Vec<String> = v
        .pointer("/settings/ui/easy_skip")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let Some(games) = v.get_mut("installed").and_then(Value::as_object_mut) else { return };
    let mut moved = Vec::new();
    for (id, g) in games.iter_mut() {
        let Some(g) = g.as_object_mut() else { continue };
        let by_hand = g.get("manual").is_some_and(|m| !m.is_null());
        let skipped = skip.iter().any(|s| s == id);
        let picked = g.get("exe_rules").and_then(Value::as_u64) == Some(PLAYER_PICK);
        let mode = if by_hand {
            "by_hand"
        } else if skipped || picked {
            "own"
        } else {
            "easy"
        };
        if mode != "easy" {
            g.insert("mode".into(), mode.into());
        }
        if skipped {
            moved.push(id.clone());
        }
    }
    if moved.is_empty() {
        return;
    }
    if let Some(list) = v.pointer_mut("/settings/ui/easy_skip").and_then(Value::as_array_mut) {
        list.retain(|x| !x.as_str().is_some_and(|s| moved.iter().any(|m| m == s)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mode(v: &Value, id: &str) -> Option<String> {
        v["installed"][id].get("mode").and_then(Value::as_str).map(str::to_string)
    }

    #[test]
    fn set_up_by_hand_becomes_its_mode() {
        let mut v = json!({ "installed": { "hopper": { "manual": { "how": "none" } } } });
        assert!(to_current(&mut v));
        assert_eq!(mode(&v, "hopper").as_deref(), Some("by_hand"));
    }

    #[test]
    fn a_game_on_the_skip_list_is_the_players_own_and_leaves_the_list() {
        let mut v = json!({
            "installed": { "hopper": {} },
            "settings": { "ui": { "easy_skip": ["hopper", "not-installed-yet"] } }
        });
        to_current(&mut v);
        assert_eq!(mode(&v, "hopper").as_deref(), Some("own"));
        // A choice made before installing stays for when it's installed.
        assert_eq!(v["settings"]["ui"]["easy_skip"], json!(["not-installed-yet"]));
    }

    #[test]
    fn a_program_the_player_picked_is_the_players_own() {
        // Before Beta 58 the pick took over by itself; after, it turned Easy Setup off at every
        // start. Now that's said once, here.
        let mut v = json!({ "installed": { "hopper": { "exe": "bin/hopper.exe", "exe_rules": 255 } } });
        to_current(&mut v);
        assert_eq!(mode(&v, "hopper").as_deref(), Some("own"));
    }

    #[test]
    fn everything_else_is_easy_setup_and_writes_nothing() {
        let mut v = json!({ "installed": { "hopper": { "exe": "hopper.exe", "exe_rules": 3 } } });
        to_current(&mut v);
        assert_eq!(mode(&v, "hopper"), None);
        assert_eq!(v["format"], json!(FORMAT));
    }

    #[test]
    fn a_library_already_converted_is_left_as_it_is() {
        let mut v = json!({ "format": FORMAT, "installed": { "hopper": { "manual": {} } } });
        assert!(!to_current(&mut v));
        assert_eq!(mode(&v, "hopper"), None);
    }

    #[test]
    fn an_empty_or_odd_file_still_converts() {
        let mut v = json!({});
        assert!(to_current(&mut v));
        let mut odd = json!({ "installed": { "hopper": "not a game" }, "settings": { "ui": { "easy_skip": "x" } } });
        assert!(to_current(&mut odd));
    }

    #[test]
    fn a_converted_library_reads_with_the_modes() {
        let mut v = json!({
            "installed": {
                "hopper": { "id": "hopper", "name": "Hopper", "tag": "v1", "path": "/games/hopper", "manual": { "how": "none" } },
                "skyracer": { "id": "skyracer", "name": "Sky Racer", "tag": "v1", "path": "/games/skyracer" }
            },
            "settings": { "ui": { "easy_skip": ["skyracer"] } }
        });
        to_current(&mut v);
        let game = |id: &str| serde_json::from_value::<crate::library::Installed>(v["installed"][id].clone()).unwrap();
        assert!(game("hopper").hand().is_some());
        assert!(game("skyracer").player_sets_up(false));
    }
}

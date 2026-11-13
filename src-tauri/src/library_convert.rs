//! Libraries saved by older launchers, moved to the current format once, as they're read.
//!
//! Beta only: deleted at 1.0. A library older than that then starts its setups again.
//!
//! Format 2 (Beta 65): who sets each game up is the game's own `mode`. Before, it was worked out
//! from three places: Set Up By Hand being on (`manual`), the Skip Easy Setup list in the
//! settings (`ui.easy_skip`), and a program the player picked (`exe_rules` 255, which turned
//! Easy Setup off for that game at every start).
//!
//! Format 3: a game has two roads, Easy Setup or Skip Easy Setup. Set Up By Hand (`mode`
//! "by_hand", `manual`) and a build the player pointed the launcher at (`build_dir`, `prebuilt`)
//! are gone: those games are Skip Easy Setup, since the program the player runs knows where its
//! build is.
//!
//! Also in format 3: named inputs are the one way to ask for files. The player's copy
//! (`game_copy`) and update (`update_copy`) are the inputs `game` and `update` in `setup_inputs`,
//! the copy read whole (`copy_check`) is `input_checks.game`, and the setup fingerprints
//! (`inputs_ready`, each game's own) and `prepare_version` go: what they said is kept in `upgrade`
//! for the launcher to read once with the game's profile. An Easy Setup left open when the
//! launcher last closed (`setup_before`) is put back first, as that launcher would have.
//!
//! Format 4: Skip Easy Setup in Settings (`ui.easy_skip_all`) is for new games only. Before, it
//! covered every installed game while it was on, without changing their mode. With it on, each
//! installed game takes it as its own mode, so nothing changes for the player now that installed
//! games go by their mode alone.

use serde_json::Value;

/// The format this launcher writes.
pub const FORMAT: u32 = 4;

/// A program the player picked (`Installed::exe_rules`): the launcher never picks again over it.
const PLAYER_PICK: u64 = u8::MAX as u64;

/// Brings a library file's JSON up to `FORMAT`, before it's read. True when it changed anything.
pub fn to_current(v: &mut Value) -> bool {
    let from = v.get("format").and_then(Value::as_u64).unwrap_or(1);
    if from >= FORMAT as u64 {
        return false;
    }
    if from < 2 {
        modes_from_old_fields(v);
    }
    if from < 3 {
        two_roads(v);
        one_way_inputs(v);
    }
    skip_all_is_each_games(v);
    v["format"] = FORMAT.into();
    true
}

/// Format 3 -> 4: with Skip Easy Setup on in Settings, every installed game takes it as its own
/// mode. See the top of this file.
fn skip_all_is_each_games(v: &mut Value) {
    let on = v.pointer("/settings/ui/easy_skip_all").and_then(Value::as_bool).unwrap_or(false);
    if !on {
        return;
    }
    let Some(games) = v.get_mut("installed").and_then(Value::as_object_mut) else { return };
    for g in games.values_mut() {
        let Some(g) = g.as_object_mut() else { continue };
        g.insert("mode".into(), "own".into());
    }
}

/// Format 2 -> 3: hand setups and the player's own builds become Skip Easy Setup. A hand setup's
/// program becomes the picked Game Executable; with none recorded the release's program runs.
fn two_roads(v: &mut Value) {
    let Some(games) = v.get_mut("installed").and_then(Value::as_object_mut) else { return };
    for g in games.values_mut() {
        let Some(g) = g.as_object_mut() else { continue };
        let hand = g.remove("manual").filter(|m| !m.is_null());
        let by_hand = g.get("mode").and_then(Value::as_str) == Some("by_hand");
        let built_elsewhere = g.remove("build_dir").is_some_and(|d| !d.is_null());
        let here = g.remove("prebuilt").and_then(|p| p.as_bool()).unwrap_or(false);
        if !(by_hand || built_elsewhere || here) {
            continue;
        }
        g.insert("mode".into(), "own".into());
        if !by_hand {
            continue;
        }
        let Some(exe) = hand.as_ref().and_then(|m| m.get("exe")).filter(|e| e.is_string()) else { continue };
        g.insert("exe".into(), exe.clone());
        g.insert("exe_rules".into(), PLAYER_PICK.into());
    }
}


/// Format 2 -> 3: the copy and the update are named inputs, and how the game was set up is kept
/// for one read with its profile (`upgrade`). See the top of this file.
fn one_way_inputs(v: &mut Value) {
    let Some(games) = v.get_mut("installed").and_then(Value::as_object_mut) else { return };
    for g in games.values_mut() {
        let Some(g) = g.as_object_mut() else { continue };
        // An engine game's fingerprint identifies its completed build, and its picks are what that
        // build is recovered from: neither is an old setup to read once or put back.
        if g.get("engine_build").is_some_and(|e| !e.is_null()) {
            g.remove("setup_before");
            continue;
        }
        put_back_open_setup(g);
        let mut inputs = g.remove("setup_inputs").and_then(|x| x.as_object().cloned()).unwrap_or_default();
        for (old, id) in [("game_copy", "game"), ("update_copy", "update")] {
            let Some(path) = g.remove(old).filter(Value::is_string) else { continue };
            inputs.entry(id.to_string()).or_insert(path);
        }
        if !inputs.is_empty() {
            g.insert("setup_inputs".into(), Value::Object(inputs));
        }
        if let Some(check) = g.remove("copy_check").filter(Value::is_object) {
            g.insert("input_checks".into(), serde_json::json!({ "game": check }));
        }
        let ready = g.remove("inputs_ready").and_then(|x| x.as_str().map(str::to_string)).unwrap_or_default();
        let version = g.remove("prepare_version").and_then(|x| x.as_str().map(str::to_string)).unwrap_or_default();
        let mut games_ready = vec![];
        if let Some(each) = g.get_mut("games").and_then(Value::as_object_mut) {
            for (id, x) in each.iter_mut() {
                let Some(x) = x.as_object_mut() else { continue };
                let had = x.remove("inputs_ready").and_then(|r| r.as_str().map(str::to_string)).unwrap_or_default();
                if !had.is_empty() {
                    games_ready.push(id.clone());
                }
            }
        }
        if ready.is_empty() && version.is_empty() && games_ready.is_empty() {
            continue;
        }
        g.insert("upgrade".into(), serde_json::json!({ "inputs_ready": !ready.is_empty(), "prepare_version": version, "games_ready": games_ready }));
    }
}

/// An Easy Setup window open when the launcher closed: what it kept when it opened goes back on
/// the game, as the launcher that saved it would have done at its next start.
fn put_back_open_setup(g: &mut serde_json::Map<String, Value>) {
    let Some(before) = g.remove("setup_before").and_then(|b| b.as_object().cloned()) else { return };
    for k in ["game_copy", "update_copy", "setup_inputs", "inputs_ready", "prepare_version", "prepare_opts", "runtime_off", "mode"] {
        match before.get(k) {
            Some(x) if !x.is_null() => g.insert(k.into(), x.clone()),
            _ => g.remove(k),
        };
    }
    let Some(each) = g.get_mut("games").and_then(Value::as_object_mut) else { return };
    let was = before.get("games_ready").and_then(Value::as_object).cloned().unwrap_or_default();
    for (id, x) in each.iter_mut() {
        let Some(x) = x.as_object_mut() else { continue };
        match was.get(id) {
            Some(r) => x.insert("inputs_ready".into(), r.clone()),
            None => x.remove("inputs_ready"),
        };
    }
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
    fn skip_for_every_game_becomes_each_installed_games_mode() {
        let mut v = json!({ "format": 3, "installed": { "hopper": {}, "skyracer": { "mode": "own" } }, "settings": { "ui": { "easy_skip_all": true } } });
        assert!(to_current(&mut v));
        assert_eq!(mode(&v, "hopper").as_deref(), Some("own"));
        assert_eq!(mode(&v, "skyracer").as_deref(), Some("own"));
    }

    #[test]
    fn without_skip_for_every_game_installed_games_keep_easy_setup() {
        let mut v = json!({ "format": 3, "installed": { "hopper": {} }, "settings": { "ui": { "easy_skip_all": false } } });
        assert!(to_current(&mut v));
        assert_eq!(mode(&v, "hopper"), None);
    }

    #[test]
    fn a_format_4_library_is_left_as_it_is() {
        let mut v = json!({ "format": 4, "installed": { "hopper": {} }, "settings": { "ui": { "easy_skip_all": true } } });
        assert!(!to_current(&mut v));
        assert_eq!(mode(&v, "hopper"), None);
    }

    #[test]
    fn an_engine_game_keeps_its_fingerprint_and_picks() {
        let mut v = json!({ "installed": { "hopper": {
            "engine_build": { "state": "prepared" },
            "inputs_ready": "fingerprint",
            "setup_inputs": { "base": "/games/hopper.nsp" },
            "setup_before": { "setup_inputs": { "base": "/games/older.nsp" } }
        } } });
        assert!(to_current(&mut v));
        let g = &v["installed"]["hopper"];
        assert_eq!(g["inputs_ready"], "fingerprint");
        assert_eq!(g["setup_inputs"]["base"], "/games/hopper.nsp");
        assert!(g.get("setup_before").is_none());
        assert!(g.get("upgrade").is_none());
    }

    #[test]
    fn set_up_by_hand_becomes_skip_easy_setup() {
        let mut v = json!({ "installed": { "hopper": { "manual": { "how": "none" } } } });
        assert!(to_current(&mut v));
        assert_eq!(mode(&v, "hopper").as_deref(), Some("own"));
        assert!(v["installed"]["hopper"].get("manual").is_none());
    }

    #[test]
    fn a_hand_setups_program_becomes_the_picked_game_executable() {
        let mut v = json!({ "format": 2, "installed": { "hopper": { "mode": "by_hand", "exe": "hopper.exe", "manual": { "exe": "bin/real.exe", "how": "arg" } } } });
        to_current(&mut v);
        assert_eq!(v["installed"]["hopper"]["exe"], json!("bin/real.exe"));
        assert_eq!(v["installed"]["hopper"]["exe_rules"], json!(PLAYER_PICK));
    }

    #[test]
    fn a_hand_setup_kept_for_later_is_dropped_and_the_game_stays_easy() {
        let mut v = json!({ "format": 2, "installed": { "hopper": { "manual": { "exe": "x.exe" } } } });
        to_current(&mut v);
        assert_eq!(mode(&v, "hopper"), None);
        assert!(v["installed"]["hopper"].get("manual").is_none());
    }

    #[test]
    fn a_build_the_player_pointed_at_becomes_skip_easy_setup() {
        let mut v = json!({ "format": 2, "installed": { "hopper": { "build_dir": "/builds/hopper" }, "sky": { "prebuilt": true } } });
        to_current(&mut v);
        assert_eq!(mode(&v, "hopper").as_deref(), Some("own"));
        assert_eq!(mode(&v, "sky").as_deref(), Some("own"));
        assert!(v["installed"]["hopper"].get("build_dir").is_none());
        assert!(v["installed"]["sky"].get("prebuilt").is_none());
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
        assert!(game("hopper").player_sets_up());
        assert!(game("skyracer").player_sets_up());
    }

    #[test]
    fn the_copy_and_the_update_become_named_inputs() {
        let mut v = json!({ "format": 2, "installed": { "hopper": {
            "game_copy": "/games/hopper.iso", "update_copy": "/games/tu.bin", "setup_inputs": { "dlc": "/games/dlc.bin" },
            "copy_check": { "path": "/games/hopper.iso", "size": 1, "modified": 2, "crc32": "0011aabb" }
        } } });
        to_current(&mut v);
        let g = &v["installed"]["hopper"];
        assert_eq!(g["setup_inputs"], json!({ "game": "/games/hopper.iso", "update": "/games/tu.bin", "dlc": "/games/dlc.bin" }));
        assert_eq!(g["input_checks"]["game"]["crc32"], json!("0011aabb"));
        for old in ["game_copy", "update_copy", "copy_check"] {
            assert!(g.get(old).is_none(), "{old}");
        }
    }

    #[test]
    fn how_a_game_was_set_up_is_kept_for_one_read() {
        let mut v = json!({ "format": 2, "installed": {
            "hopper": { "id": "hopper", "name": "Hopper", "repo": "", "tag": "", "version": "", "path": "/g", "inputs_ready": "v2:abc", "prepare_version": "3", "games": { "a": { "played_secs": 5, "inputs_ready": "v2:x" }, "b": {} } },
            "plain": { "game_copy": "/games/plain.iso" }
        } });
        to_current(&mut v);
        let g = &v["installed"]["hopper"];
        assert_eq!(g["upgrade"], json!({ "inputs_ready": true, "prepare_version": "3", "games_ready": ["a"] }));
        assert!(g.get("inputs_ready").is_none() && g.get("prepare_version").is_none());
        assert!(g["games"]["a"].get("inputs_ready").is_none());
        assert_eq!(g["games"]["a"]["played_secs"], json!(5));
        // A copy with no setup step to remember: nothing to read later.
        assert!(v["installed"]["plain"].get("upgrade").is_none());
        let read: crate::library::Installed = serde_json::from_value(v["installed"]["hopper"].clone()).unwrap();
        assert_eq!(read.upgrade.unwrap().prepare_version, "3");
    }

    #[test]
    fn an_easy_setup_left_open_is_put_back_before_it_moves() {
        let mut v = json!({ "format": 2, "installed": { "hopper": {
            "game_copy": "/games/picked-in-the-window.iso", "inputs_ready": "",
            "setup_before": { "game_copy": "/games/hopper.iso", "inputs_ready": "v2:abc", "prepare_version": "2", "mode": "easy" }
        } } });
        to_current(&mut v);
        let g = &v["installed"]["hopper"];
        assert_eq!(g["setup_inputs"]["game"], json!("/games/hopper.iso"));
        assert_eq!(g["upgrade"]["inputs_ready"], json!(true));
        assert!(g.get("setup_before").is_none());
    }
}

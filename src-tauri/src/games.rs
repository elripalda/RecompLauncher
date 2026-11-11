//! Recomps that carry several games in one release (a whole series in one install). The recomp
//! lists them in `"games"` in its recomplauncher.json, or the catalog entry carries the same list
//! for developers who don't write one; the developer's file wins. Each game gets its own tile,
//! play time, achievements and shortcuts, while the install, updates and removal stay one.
//!
//! A game's full id is `<recomp id>--<game id>` (cogbox--gow2). One game, or none, is exactly the
//! recomp as before.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// What joins a recomp's id and one of its games' ids.
pub const SEP: &str = "--";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Game {
    pub id: String,
    pub name: String,
    /// Added after `start.args` when this game is played. None: Play opens the recomp's own menu.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// Xbox 360: this game's title id, which names its unlock file (<title_id>.toml).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title_id: String,
    /// The setup inputs this game needs. Inputs no game lists are shared by all.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<String>,
    /// Where this game keeps its own saves (same shape as the top-level "saves"). None: shared.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub saves: Vec<String>,
    /// Art from the catalog (never the developer's file): falls back to the recomp's.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub icon: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cover: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub hero: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub logo: String,
}

/// A game's id: 1 to 16 of a-z and 0-9.
pub fn id_ok(id: &str) -> bool {
    (1..=16).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

pub fn full(recomp: &str, game: &str) -> String {
    format!("{recomp}{SEP}{game}")
}

/// A full id taken apart: (recomp id, game id). None when it isn't one.
pub fn split(full: &str) -> Option<(&str, &str)> {
    let (r, g) = full.rsplit_once(SEP)?;
    (!r.is_empty() && id_ok(g)).then_some((r, g))
}

/// What's wrong with a list, for the developer tools. Ok for a list the launcher uses as it is.
pub fn check(list: &Value) -> Result<(), String> {
    let Some(arr) = list.as_array() else { return Err("\"games\" has to be a list.".into()) };
    let mut seen = BTreeSet::new();
    for (n, v) in arr.iter().enumerate() {
        let g: Game = serde_json::from_value(v.clone()).map_err(|e| format!("Game {} doesn't fit the format: {e}.", n + 1))?;
        if !id_ok(&g.id) {
            return Err(format!("\"{}\" isn't a game id: use 1 to 16 of a-z and 0-9.", g.id));
        }
        if !seen.insert(g.id.clone()) {
            return Err(format!("\"{}\" is used by more than one game.", g.id));
        }
        if g.name.trim().is_empty() {
            return Err(format!("Game \"{}\" has no name.", g.id));
        }
    }
    Ok(())
}

/// The games in a list, as the launcher uses them: one that doesn't fit (a bad or repeated id, no
/// name) is left out rather than failing the rest. Fewer than two: none (the recomp as before).
pub fn parse(list: &Value) -> Vec<Game> {
    let mut seen = BTreeSet::new();
    let out: Vec<Game> = list
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| serde_json::from_value::<Game>(v.clone()).ok())
        .filter(|g| id_ok(&g.id) && !g.name.trim().is_empty() && seen.insert(g.id.clone()))
        .collect();
    if out.len() < 2 {
        vec![]
    } else {
        out
    }
}

/// A recomp's games: its developer's list wins over the catalog's. Art always comes from the
/// catalog (matched by id), since that's where it lives.
pub fn of(own: Option<&Value>, entry: &Value) -> Vec<Game> {
    let cat = entry.get("games").map(parse).unwrap_or_default();
    let mine = own.map(parse).unwrap_or_default();
    if mine.is_empty() {
        return cat;
    }
    let art: BTreeMap<&str, &Game> = cat.iter().map(|g| (g.id.as_str(), g)).collect();
    mine.into_iter()
        .map(|mut g| {
            if let Some(c) = art.get(g.id.as_str()) {
                for (mine, theirs) in [(&mut g.icon, &c.icon), (&mut g.cover, &c.cover), (&mut g.hero, &c.hero), (&mut g.logo, &c.logo)] {
                    if mine.is_empty() {
                        *mine = theirs.clone();
                    }
                }
            }
            g
        })
        .collect()
}

/// The inputs one game asks for: its own, and the ones no game lists (shared by all).
pub fn inputs_for(game: &Game, all: &[Game], inputs: &[crate::inputs::Input]) -> Vec<crate::inputs::Input> {
    let listed: BTreeSet<&str> = all.iter().flat_map(|g| g.inputs.iter().map(String::as_str)).collect();
    inputs.iter().filter(|i| game.inputs.contains(&i.id) || !listed.contains(i.id.as_str())).cloned().collect()
}

/// A profile for playing one game: only its inputs, and its saves in place of the shared ones
/// when it has its own.
pub fn scope(p: &mut crate::profile::Profile, game: &Game, all: &[Game]) {
    p.inputs = inputs_for(game, all, &p.inputs);
    p.input_paths.retain(|k, _| p.inputs.iter().any(|i| &i.id == k));
    if !game.saves.is_empty() {
        p.saves = game.saves.clone();
    }
}

/// What a game adds to the command line: its own options after the recomp's (its `start.args`
/// and settings). The player's own launch options still come last.
pub fn add_args(args: &mut Vec<String>, game: &Game) {
    args.extend(game.args.iter().cloned());
}

/// Play time and the last session of one game in a recomp, kept on its install.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Played {
    #[serde(default)]
    pub played_secs: u64,
    #[serde(default)]
    pub last_played: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sessions: Vec<[u64; 2]>,
    /// Setup for this game's own inputs is done (their fingerprint, as `inputs_ready` is).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub inputs_ready: String,
}

impl Played {
    pub fn add(&mut self, started: u64, secs: u64, now: u64) {
        self.played_secs += secs;
        self.last_played = now;
        if secs >= 10 {
            self.sessions.push([started, secs]);
            let n = self.sessions.len();
            if n > 500 {
                self.sessions.drain(..n - 500);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn two() -> Value {
        json!([
            { "id": "gow2", "name": "Gears of War 2", "args": ["--game=gow2"], "title_id": "4D53082D", "inputs": ["disc2"] },
            { "id": "gow3", "name": "Gears of War 3", "args": ["--game=gow3"], "title_id": "4D530906", "inputs": ["disc3"] }
        ])
    }

    #[test]
    fn ids_are_short_lowercase_and_unique() {
        assert!(id_ok("gow2") && id_ok("a") && id_ok("abcdefghij012345"));
        assert!(!id_ok("") && !id_ok("GOW2") && !id_ok("gow-2") && !id_ok("abcdefghij0123456") && !id_ok("gow 2"));
        assert!(check(&two()).is_ok());
        let dup = json!([{ "id": "a", "name": "A" }, { "id": "a", "name": "B" }]);
        assert!(check(&dup).unwrap_err().contains("more than one"));
        assert!(check(&json!([{ "id": "Bad", "name": "A" }])).is_err());
        assert!(check(&json!([{ "id": "a" }])).is_err());
        assert!(check(&json!({})).is_err());
        // The launcher itself leaves the bad and repeated ones out.
        let list = parse(&json!([{ "id": "a", "name": "A" }, { "id": "a", "name": "Again" }, { "id": "B!", "name": "Bad" }, { "id": "c", "name": "C" }]));
        assert_eq!(list.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(), ["a", "c"]);
    }

    #[test]
    fn one_game_or_none_is_the_recomp_as_before() {
        assert!(parse(&json!([])).is_empty());
        assert!(parse(&json!([{ "id": "a", "name": "A" }])).is_empty());
        assert!(parse(&Value::Null).is_empty());
        assert!(of(None, &json!({})).is_empty());
        assert_eq!(parse(&two()).len(), 2);
    }

    #[test]
    fn full_ids_go_both_ways() {
        assert_eq!(full("cogbox", "gow2"), "cogbox--gow2");
        assert_eq!(split("cogbox--gow2"), Some(("cogbox", "gow2")));
        assert_eq!(split("my--recomp--gow2"), Some(("my--recomp", "gow2")));
        assert_eq!(split("cogbox"), None);
        assert_eq!(split("--gow2"), None);
        assert_eq!(split("cogbox--GOW2"), None);
    }

    #[test]
    fn the_developers_list_wins_and_keeps_the_catalogs_art() {
        let entry = json!({ "games": [
            { "id": "gow2", "name": "Catalog 2", "cover": "/c2.jpg" },
            { "id": "gow3", "name": "Catalog 3" },
            { "id": "gowj", "name": "Judgment" }
        ]});
        let catalog_only = of(None, &entry);
        assert_eq!(catalog_only.len(), 3);
        let mine = of(Some(&two()), &entry);
        assert_eq!(mine.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), ["Gears of War 2", "Gears of War 3"]);
        assert_eq!(mine[0].cover, "/c2.jpg");
        assert_eq!(mine[0].args, ["--game=gow2"]);
        // A developer list that doesn't hold up: the catalog's is used.
        assert_eq!(of(Some(&json!([{ "id": "x", "name": "Only one" }])), &entry).len(), 3);
    }

    #[test]
    fn each_game_asks_for_its_own_inputs_and_the_shared_ones() {
        let input = |id: &str| serde_json::from_value::<crate::inputs::Input>(json!({ "id": id, "label": id })).unwrap();
        let all = parse(&two());
        let inputs = vec![input("disc2"), input("disc3"), input("dlc")];
        let ids = |l: Vec<crate::inputs::Input>| l.into_iter().map(|i| i.id).collect::<Vec<_>>();
        assert_eq!(ids(inputs_for(&all[0], &all, &inputs)), ["disc2", "dlc"]);
        assert_eq!(ids(inputs_for(&all[1], &all, &inputs)), ["disc3", "dlc"]);
        let mut p = crate::profile::Profile { inputs: inputs.clone(), saves: vec!["{install}/save".into()], ..Default::default() };
        let mut own_saves = all[1].clone();
        own_saves.saves = vec!["{install}/gow3/save".into()];
        scope(&mut p, &own_saves, &all);
        assert_eq!(ids(p.inputs.clone()), ["disc3", "dlc"]);
        assert_eq!(p.saves, ["{install}/gow3/save"]);
    }

    #[test]
    fn a_games_options_come_after_the_recomps_start_args() {
        let p: crate::profile::Profile = serde_json::from_value(json!({
            "format": 1,
            "start": { "program": ["cogbox.exe"], "args": ["--fullscreen"] },
            "games": two()
        })).unwrap();
        let all = of(p.games.as_ref(), &Value::Null);
        let mut args = p.start.as_ref().unwrap().args.clone();
        add_args(&mut args, &all[1]);
        assert_eq!(args, ["--fullscreen", "--game=gow3"]);
        // No options of its own: Play opens the recomp's menu, as it starts.
        let mut plain = p.start.as_ref().unwrap().args.clone();
        add_args(&mut plain, &Game { id: "gowj".into(), name: "Judgment".into(), ..Default::default() });
        assert_eq!(plain, ["--fullscreen"]);
    }

    #[test]
    fn older_launchers_ignore_the_list() {
        // An unknown key at the top of the file never fails the profile (what an older launcher
        // does with "games"), and the list sits beside everything else.
        let p: crate::profile::Profile = serde_json::from_value(json!({ "format": 1, "games": two(), "something_new": { "x": 1 } })).unwrap();
        assert_eq!(p.format, 1);
        assert_eq!(of(p.games.as_ref(), &Value::Null).len(), 2);
    }

    #[test]
    fn play_time_is_kept_per_game() {
        let mut p = Played::default();
        p.add(100, 5, 105);
        p.add(200, 60, 260);
        assert_eq!(p.played_secs, 65);
        assert_eq!(p.last_played, 260);
        assert_eq!(p.sessions, vec![[200, 60]]);
    }
}

use crate::engines::catalog::EngineGame;
use crate::profile::Profile;
use serde_json::{json, Value};
pub fn parse(tsv: &str, notes: &str, evidence: &str) -> Option<EngineGame> {
    let header = tsv.lines().next()?;
    let console = if header.contains("(Switch 2") { "switch2" } else if header.contains("(Switch") { "switch" } else { return None };
    let rows: Vec<Vec<&str>> = tsv.lines().filter(|l| !l.starts_with('#')).map(|l| l.split('\t').collect()).collect();
    let key = |k| rows.iter().find(|r| r.first() == Some(&k)).and_then(|r| r.get(1)).copied();
    let title_id = key("title_id")?.to_uppercase();
    if title_id.len() != 16 || !title_id.bytes().all(|b| b.is_ascii_hexdigit()) { return None; }
    let name = key("name")?.to_string();
    let package = key("package")?.to_string();
    let build = rows.iter().find(|r| r.len() >= 4 && r[0] == "build" && r[1] == package)?;
    let build_id = build.iter().find_map(|v| v.strip_prefix("main="))?.to_string();
    if name.is_empty() || build[2].is_empty() || build[2] == "?" || build_id.len() < 16 || !build_id.bytes().all(|b| b.is_ascii_hexdigit()) { return None; }
    Some(EngineGame { engine:"nxnative".into(), title_id, name, console: console.into(), package, version: build[2].into(), build_id, notes: notes.into(), evidence: evidence.into() })
}


pub fn setup_profile(game: &EngineGame) -> Profile {
        let mut formats = vec![json!({"type":"file", "extensions":["nup"]}), json!({"type":"folder"})];
        let mut inputs = vec![];
        if game.console == "switch" {
            formats.push(json!({"type":"file", "extensions":["nsp","xci","nsz","xcz"], "requires":["keys"]}));
        }
        inputs.push(json!({"id":"base", "label":format!("Your game ({})", game.version), "formats":formats}));
        if game.console == "switch" {
            inputs.push(json!({"id":"update", "label":format!("Game update ({}, if separate)", game.version), "optional":true, "role":"update", "formats":[{"type":"file","extensions":["nsp","xci","nsz","xcz"],"requires":["keys"]}]}));
            inputs.push(json!({"id":"keys", "label":"Nintendo Switch prod.keys", "role":"key", "shared":"switch.prod-keys"}));
        }
        serde_json::from_value(json!({"format":1,"inputs":inputs})).expect("internal setup profile")
}

pub fn games() -> Vec<EngineGame> {
    let records: Vec<Value> = serde_json::from_str(include_str!("catalog-nxnative.json")).expect("checked engine catalog");
    records.iter().filter_map(|v| parse(v["profile"].as_str()?, v["notes"].as_str()?, v["evidence"].as_str()?)).collect()
}

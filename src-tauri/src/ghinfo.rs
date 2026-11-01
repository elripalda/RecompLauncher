//! Working out what a GitHub repo is a recomp of: the game and its console, from the repo's
//! description, topics and README. Art from SteamGridDB, with the player's own key.

use serde_json::{json, Value};

/// Consoles by the words people use for them: (id, name, words).
pub const CONSOLES: &[(&str, &str, &[&str])] = &[
    ("n64", "Nintendo 64", &["nintendo 64", "n64"]),
    ("x360", "Xbox 360", &["xbox 360", "xbox360", "x360"]),
    ("xbox", "Xbox", &["original xbox", "xbox"]),
    ("ps3", "PlayStation 3", &["playstation 3", "ps3"]),
    ("ps2", "PlayStation 2", &["playstation 2", "ps2"]),
    ("ps1", "PlayStation", &["playstation 1", "playstation one", "psx", "ps1", "playstation"]),
    ("psp", "PSP", &["playstation portable", "psp"]),
    ("wiiu", "Wii U", &["wii u", "wiiu"]),
    ("wii", "Wii", &["wii"]),
    ("gc", "GameCube", &["gamecube", "game cube", "ngc"]),
    ("switch", "Nintendo Switch", &["nintendo switch", "switch"]),
    ("3ds", "Nintendo 3DS", &["nintendo 3ds", "3ds"]),
    ("nds", "Nintendo DS", &["nintendo ds", "nds"]),
    ("gba", "Game Boy Advance", &["game boy advance", "gameboy advance", "gba"]),
    ("gb", "Game Boy", &["game boy color", "gameboy color", "game boy", "gameboy", "gbc"]),
    ("snes", "Super Nintendo", &["super nintendo", "super nes", "snes", "super famicom"]),
    ("nes", "NES", &["nintendo entertainment system", "famicom", "nes"]),
    ("genesis", "Sega Genesis", &["mega drive", "megadrive", "genesis"]),
    ("saturn", "Sega Saturn", &["sega saturn", "saturn"]),
    ("dc", "Dreamcast", &["dreamcast"]),
    ("pc", "PC", &["ms-dos", "dos game", "windows game"]),
];

/// `word` in `text` as a word of its own (not part of another).
fn has_word(text: &str, word: &str) -> bool {
    let b = text.as_bytes();
    let mut from = 0;
    while let Some(i) = text[from..].find(word) {
        let s = from + i;
        let e = s + word.len();
        let ok_l = s == 0 || !b[s - 1].is_ascii_alphanumeric();
        let ok_r = e >= b.len() || !b[e].is_ascii_alphanumeric();
        if ok_l && ok_r {
            return true;
        }
        from = s + 1;
    }
    false
}

/// The console a repo's words point at: topics first, then its description, then its README.
pub fn console_of(topics: &[String], description: &str, readme: &str) -> Option<&'static str> {
    let topics = topics.iter().map(|t| t.replace('-', " ")).collect::<Vec<_>>().join(" , ").to_lowercase();
    let readme_top: String = readme.chars().take(3000).collect();
    // Words that mean other things in a sentence count only as topics.
    let unsure = ["switch", "saturn", "genesis"];
    for (n, text) in [topics, description.to_lowercase(), readme_top.to_lowercase()].into_iter().enumerate() {
        // The earliest mention wins ("a PS2 game, also on GameCube" is a PS2 recomp).
        let mut best: Option<(usize, &str)> = None;
        for (id, _, words) in CONSOLES {
            for w in *words {
                if n > 0 && unsure.contains(w) {
                    continue;
                }
                if has_word(&text, w) {
                    let at = text.find(w).unwrap_or(usize::MAX);
                    if best.map(|(b, _)| at < b).unwrap_or(true) {
                        best = Some((at, id));
                    }
                }
            }
        }
        if let Some((_, id)) = best {
            return Some(id);
        }
    }
    None
}

/// The game a repo says it's a recomp of: "A static recompilation of Super Mario 64 for PC" →
/// Super Mario 64. Else its README's title, without the "Recomp" words.
pub fn game_of(description: &str, readme: &str) -> Option<String> {
    let d = description.trim();
    let low = d.to_lowercase();
    for key in ["recompilation of ", "recomp of ", "decompilation of ", "decomp of ", "port of ", "reimplementation of ", "rewrite of ", "recompiled version of ", "native version of "] {
        if let Some(i) = low.find(key) {
            let rest = &d[i + key.len()..];
            let mut end = rest.len();
            for stop in [" for ", " to ", " on ", " that ", " which ", " using ", " with ", " into ", ". ", ", ", " (", " - ", " – ", " | "] {
                if let Some(j) = rest.to_lowercase().find(stop) {
                    end = end.min(j);
                }
            }
            let g = clean(&rest[..end]);
            if g.len() >= 2 {
                return Some(g);
            }
        }
    }
    let title = readme.lines().map(str::trim).find(|l| l.starts_with("# ")).map(|l| l[2..].to_string())?;
    let g = clean(&strip_recomp_words(&title));
    (g.len() >= 2).then_some(g)
}

fn clean(s: &str) -> String {
    let s = s.trim().trim_matches(|c: char| c == '"' || c == '*' || c == '_' || c == '`' || c == '.' || c == ':').trim();
    let s = s.strip_prefix("the game ").unwrap_or(s);
    // Markdown links and pictures: their text.
    let mut out = String::new();
    let mut in_link = false;
    for c in s.chars() {
        match c {
            '!' | '[' => in_link = true,
            ']' => in_link = false,
            _ if in_link => out.push(c),
            _ => out.push(c),
        }
    }
    let out = out.split("](").next().unwrap_or(&out).to_string();
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_recomp_words(s: &str) -> String {
    let drop = ["recompiled", "recompilation", "recomp", "decompiled", "decomp", "pc port", "port", "native"];
    s.split([' ', ':', '-'])
        .filter(|w| !w.is_empty() && !drop.contains(&w.to_lowercase().as_str()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Words of a name for comparing ("Super Mario 64" ~ "super-mario-64").
pub fn norm(s: &str) -> String {
    s.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect::<String>().split_whitespace().filter(|w| !["the", "a", "of"].contains(w)).collect::<Vec<_>>().join(" ")
}

// ---------- SteamGridDB ----------

async fn sgdb(http: &reqwest::Client, key: &str, path: &str) -> Option<Value> {
    let r = http
        .get(format!("https://www.steamgriddb.com/api/v2/{path}"))
        .bearer_auth(key)
        .timeout(std::time::Duration::from_secs(12))
        .send()
        .await
        .ok()?;
    if !r.status().is_success() {
        return None;
    }
    let v: Value = r.json().await.ok()?;
    v.get("success").and_then(Value::as_bool).filter(|s| *s)?;
    v.get("data").cloned()
}

fn urls(v: Option<Value>, n: usize) -> Vec<String> {
    v.and_then(|d| d.as_array().cloned()).unwrap_or_default().iter().filter_map(|x| x.get("url").and_then(Value::as_str).map(str::to_string)).take(n).collect()
}

/// Covers, backgrounds and a logo for a game, from SteamGridDB with the player's key. Err when the
/// key isn't taken.
pub async fn art(http: &reqwest::Client, key: &str, game: &str) -> Result<Value, String> {
    let term: String = game.chars().map(|c| if c.is_alphanumeric() || c == ' ' { c } else { ' ' }).collect();
    let term = term.split_whitespace().collect::<Vec<_>>().join(" ");
    if term.is_empty() {
        return Ok(json!({"covers": [], "heroes": [], "logos": []}));
    }
    let probe = http
        .get(format!("https://www.steamgriddb.com/api/v2/search/autocomplete/{}", term.replace(' ', "%20")))
        .bearer_auth(key)
        .timeout(std::time::Duration::from_secs(12))
        .send()
        .await
        .map_err(|e| format!("Couldn't reach SteamGridDB: {e}"))?;
    if probe.status().as_u16() == 401 {
        return Err("SteamGridDB didn't take that API key. Check it in Settings → Games and folders.".into());
    }
    let found: Value = probe.json().await.unwrap_or(Value::Null);
    let Some(g) = found.get("data").and_then(Value::as_array).and_then(|l| l.first()).cloned() else {
        return Ok(json!({"covers": [], "heroes": [], "logos": []}));
    };
    let id = g.get("id").and_then(Value::as_u64).unwrap_or(0);
    let (pc, ph, pl) = (format!("grids/game/{id}?dimensions=600x900&types=static"), format!("heroes/game/{id}?types=static"), format!("logos/game/{id}?types=static"));
    let (covers, heroes, logos) = tokio::join!(sgdb(http, key, &pc), sgdb(http, key, &ph), sgdb(http, key, &pl));
    Ok(json!({"game": g.get("name"), "covers": urls(covers, 8), "heroes": urls(heroes, 6), "logos": urls(logos, 3)}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_game_comes_from_the_description_or_readme() {
        assert_eq!(game_of("A static recompilation of Super Mario 64 for PC.", "").as_deref(), Some("Super Mario 64"));
        assert_eq!(game_of("PC port of The Legend of Zelda: Majora's Mask, with mods", "").as_deref(), Some("The Legend of Zelda: Majora's Mask"));
        assert_eq!(game_of("", "Some intro\n# Banjo Recompiled\nmore").as_deref(), Some("Banjo"));
        assert_eq!(game_of("Fast and fun", "").as_deref(), None);
    }

    #[test]
    fn the_console_comes_from_topics_first() {
        assert_eq!(console_of(&["n64".into(), "recompilation".into()], "", ""), Some("n64"));
        assert_eq!(console_of(&[], "A recomp of a PS2 game, also on GameCube", ""), Some("ps2"));
        assert_eq!(console_of(&["xbox-360".into()], "", ""), Some("x360"));
        assert_eq!(console_of(&[], "Switch to the new renderer", ""), None);
        assert_eq!(console_of(&["switch".into()], "", ""), Some("switch"));
        assert_eq!(console_of(&[], "A Nintendo Switch recomp", ""), Some("switch"));
        assert_eq!(console_of(&[], "", "Runs games natively"), None);
        assert_eq!(console_of(&[], "Wii U and Wii", ""), Some("wiiu"));
        assert_eq!(console_of(&[], "Dreamcast games", ""), Some("dc"));
        assert_eq!(console_of(&[], "a snes game", ""), Some("snes"));
    }

    #[test]
    fn names_compare_by_their_words() {
        assert_eq!(norm("The Legend of Zelda: Majora's Mask"), norm("legend-zelda majora s mask"));
    }
}

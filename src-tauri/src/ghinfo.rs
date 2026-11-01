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

/// Recomp words, dropped from names: "Mario Kart 64: Recompiled" → Mario Kart 64.
const RECOMP_WORDS: &[&str] = &["recompiled", "recompilation", "recomp", "decompiled", "decompilation", "decomp", "port", "pc", "native", "unofficial", "remastered", "enhanced", "windows", "linux", "x64", "static", "project", "the", "a", "an", "for", "of"];

/// The names a repo could be a recomp of, likeliest first: from "a recompilation of X" phrases,
/// a description or README title that is the name with recomp words around it, then the repo's
/// own name.
pub fn game_candidates(repo: &str, description: &str, readme: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |g: String| {
        let g = clean(&g);
        if g.chars().filter(|c| c.is_alphanumeric()).count() >= 2 && g.split_whitespace().count() <= 10 && !out.iter().any(|x| norm(x) == norm(&g)) {
            out.push(g);
        }
    };
    let title = readme.lines().map(str::trim).find(|l| l.starts_with("# ")).map(|l| l[2..].to_string()).unwrap_or_default();
    let first_para: String = readme.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('!') && !l.starts_with('[') && !l.starts_with('<')).take(3).collect::<Vec<_>>().join(" ");
    for text in [description, first_para.as_str()] {
        if let Some(g) = after_key(text) {
            push(g);
        }
    }
    for text in [description, title.as_str()] {
        // The whole line, when it's short: "Super Mario 64 PC Port", "Banjo-Kazooie: Recompiled".
        let name = before_recomp_words(text);
        if !name.is_empty() {
            push(name);
        }
    }
    push(strip_recomp_words(&split_words(repo.rsplit('/').next().unwrap_or(repo))));
    out
}

pub fn game_of(description: &str, readme: &str) -> Option<String> {
    game_candidates("", description, readme).into_iter().next()
}

/// "…a static recompilation of Majora's Mask (and soon…) for PC" → Majora's Mask.
fn after_key(d: &str) -> Option<String> {
    let low = d.to_lowercase();
    let keys = [
        "recompilation of ", "recompile ", "recompiles ", "recompiled version of ", "recomp of ", "decompilation of ", "decomp of ", "native port of ", "pc port of ", "port of ",
        "port for ", "reimplementation of ", "reimplements ", "rewrite of ", "remake of ", "native version of ", "pc version of ", "version of ",
    ];
    let (i, key) = keys.iter().filter_map(|k| low.find(k).map(|i| (i, *k))).min_by_key(|(i, _)| *i)?;
    let rest = &d[i + key.len()..];
    let rl = rest.to_lowercase();
    let mut end = rest.len();
    for stop in [" for ", " to ", " into ", " on ", " that ", " which ", " using ", " with ", " made ", " created ", " built ", " by ", " via ", " (", ", ", ". ", " - ", " – ", " | ", " and ", ";"] {
        if let Some(j) = rl.find(stop) {
            end = end.min(j);
        }
    }
    let mut g = rest[..end].trim();
    // "the Xbox 360 version of Sonic Unleashed" → Sonic Unleashed.
    if let Some(k) = g.to_lowercase().find("version of ") {
        g = g[k + "version of ".len()..].trim();
    }
    let g = g.strip_prefix("the game ").or_else(|| g.strip_prefix("The game ")).unwrap_or(g);
    // "statically recompile the N64 game Mario Kart 64" → after the console words.
    Some(g.to_string()).filter(|g| !g.is_empty())
}

/// "Super Mario 64 PC Port" → Super Mario 64; "Banjo-Kazooie: Recompiled" → Banjo-Kazooie.
/// Empty when the line is a sentence, not a name.
fn before_recomp_words(t: &str) -> String {
    let t = t.trim();
    if t.is_empty() || t.split_whitespace().count() > 9 {
        return String::new();
    }
    let low = t.to_lowercase();
    let has = ["recomp", "decomp", "port", "native", "for pc", "pc version"].iter().any(|w| low.contains(w));
    if !has {
        return String::new();
    }
    // Up to the first recomp word (keeping a colon inside a name like "Zelda: Majora's Mask").
    let words: Vec<&str> = t.split_whitespace().collect();
    let mut keep = vec![];
    for w in &words {
        let lw = w.to_lowercase();
        let bare = lw.trim_matches(|c: char| !c.is_alphanumeric());
        if ["recompiled", "recompilation", "recomp", "decompiled", "decomp", "port", "native", "unofficial", "pc", "for", "static"].contains(&bare) || bare.starts_with("recomp") {
            if keep.is_empty() && ["unofficial", "native", "static", "pc"].contains(&bare) {
                continue;
            }
            break;
        }
        keep.push(*w);
    }
    keep.join(" ").trim_end_matches([':', '-', '–', '|', ',']).trim().to_string()
}

/// "MarioKart64Recomp" → Mario Kart 64 Recomp; "sm64-pc-port" → sm64 pc port.
fn split_words(s: &str) -> String {
    let mut out = String::new();
    let cs: Vec<char> = s.chars().collect();
    for (i, c) in cs.iter().enumerate() {
        if *c == '-' || *c == '_' || *c == '.' {
            out.push(' ');
            continue;
        }
        if i > 0 {
            let p = cs[i - 1];
            if (p.is_lowercase() && c.is_uppercase()) || (p.is_lowercase() && c.is_ascii_digit()) || (p.is_ascii_digit() && c.is_uppercase()) {
                out.push(' ');
            }
        }
        out.push(*c);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clean(s: &str) -> String {
    let s = s.trim().trim_matches(|c: char| c == '"' || c == '*' || c == '_' || c == '`' || c == '.' || c == ':' || c == '\'' || c == '“' || c == '”').trim();
    // Markdown links and pictures: their text.
    let mut out = String::new();
    for c in s.chars() {
        if !matches!(c, '!' | '[' | ']') {
            out.push(c);
        }
    }
    let out = out.split("](").next().unwrap_or(&out).to_string();
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_recomp_words(s: &str) -> String {
    s.split([' ', ':', '-'])
        .filter(|w| !w.is_empty() && !RECOMP_WORDS.contains(&w.to_lowercase().as_str()))
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

/// Pictures as { url, thumb } (the thumb for choosing, the url to keep).
fn pics(v: Option<Value>, n: usize) -> Vec<Value> {
    v.and_then(|d| d.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|x| {
            let url = x.get("url").and_then(Value::as_str)?;
            Some(json!({"url": url, "thumb": x.get("thumb").and_then(Value::as_str).unwrap_or(url)}))
        })
        .take(n)
        .collect()
}

/// A game on SteamGridDB by name: (its id, its name there). Err when the key isn't taken.
pub async fn find(http: &reqwest::Client, key: &str, name: &str) -> Result<Option<(u64, String)>, String> {
    let term: String = name.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '\'' { c } else { ' ' }).collect();
    let term = term.split_whitespace().collect::<Vec<_>>().join(" ");
    if term.is_empty() {
        return Ok(None);
    }
    let probe = http
        .get(format!("https://www.steamgriddb.com/api/v2/search/autocomplete/{}", term.replace(' ', "%20").replace('\'', "%27")))
        .bearer_auth(key)
        .timeout(std::time::Duration::from_secs(12))
        .send()
        .await
        .map_err(|e| format!("Couldn't reach SteamGridDB: {e}"))?;
    if probe.status().as_u16() == 401 {
        return Err("SteamGridDB didn't take that API key. Check it in Settings → Games and folders.".into());
    }
    let found: Value = probe.json().await.unwrap_or(Value::Null);
    Ok(found.get("data").and_then(Value::as_array).and_then(|l| l.first()).and_then(|g| Some((g.get("id")?.as_u64()?, g.get("name")?.as_str()?.to_string()))))
}

/// Every cover, background and logo SteamGridDB has for a game (by its SteamGridDB id).
pub async fn art_of(http: &reqwest::Client, key: &str, id: u64) -> Value {
    let (pc, ph, pl) = (format!("grids/game/{id}?dimensions=600x900&types=static"), format!("heroes/game/{id}?types=static"), format!("logos/game/{id}?types=static"));
    let (covers, heroes, logos) = tokio::join!(sgdb(http, key, &pc), sgdb(http, key, &ph), sgdb(http, key, &pl));
    json!({"covers": pics(covers, 40), "heroes": pics(heroes, 30), "logos": pics(logos, 12)})
}

/// Art for a game by name.
pub async fn art(http: &reqwest::Client, key: &str, game: &str) -> Result<Value, String> {
    match find(http, key, game).await? {
        Some((id, name)) => {
            let mut a = art_of(http, key, id).await;
            a["game"] = Value::String(name);
            Ok(a)
        }
        None => Ok(json!({"covers": [], "heroes": [], "logos": []})),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_game_comes_from_how_recomps_describe_themselves() {
        let g = |repo: &str, d: &str, r: &str| game_candidates(repo, d, r).into_iter().next().unwrap_or_default();
        assert_eq!(g("Zelda64Recomp/Zelda64Recomp", "Static recompilation of Majora's Mask (and soon Ocarina of Time) for PC (Windows/Linux)", ""), "Majora's Mask");
        assert_eq!(g("sonicdcer/MarioKart64Recomp", "Mario Kart 64: Recompiled is a project that uses N64: Recompiled to statically recompile Mario Kart 64 into a native port", ""), "Mario Kart 64");
        assert_eq!(g("hedge-dev/UnleashedRecomp", "An unofficial PC port of the Xbox 360 version of Sonic Unleashed created through the process of static recompilation", ""), "Sonic Unleashed");
        assert_eq!(g("someone/sm64", "Super Mario 64 PC Port", ""), "Super Mario 64");
        assert_eq!(g("x/BanjoRecomp", "", "# Banjo-Kazooie: Recompiled\nmore"), "Banjo-Kazooie");
        assert_eq!(g("x/perfect_dark", "Perfect Dark PC port with mods", ""), "Perfect Dark");
        assert_eq!(g("x/StarFox64Recomp", "", ""), "Star Fox 64");
        assert_eq!(g("x/dusk", "Dusk is a reimplementation of Twilight Princess", ""), "Twilight Princess");
        assert_eq!(g("x/y", "A native PC port of Star Fox 64 with enhancements", ""), "Star Fox 64");
    }

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

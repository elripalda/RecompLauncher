//! About the original game, from Wikipedia: the opening of its article (release, story,
//! reception), shown on the game's page with a link back. Kept on disk for a month.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Info {
    pub title: String,
    /// Wikipedia's one-liner ("2003 video game").
    pub kind: String,
    /// The article's opening paragraphs, plain text.
    pub text: String,
    pub url: String,
}

const API: &str = "https://en.wikipedia.org/w/api.php";

fn words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| (w.len() > 1 || w.chars().all(|c| c.is_ascii_digit())) && !["the", "of", "and", "vol", "edition"].contains(w))
        .map(str::to_string)
        .collect()
}

/// How well an article title fits the game's name: the share of the name's words it has.
fn fit(title: &str, game: &str) -> f32 {
    let (t, g) = (words(title), words(game));
    if g.is_empty() {
        return 0.0;
    }
    g.iter().filter(|w| t.contains(w)).count() as f32 / g.len() as f32
}

async fn get(http: &reqwest::Client, q: &[(&str, &str)]) -> Option<Value> {
    let r = http.get(API).query(q).timeout(Duration::from_secs(15)).send().await.ok()?;
    if !r.status().is_success() {
        return None;
    }
    r.json().await.ok()
}

async fn lookup(http: &reqwest::Client, game: &str, console: &str) -> Option<Info> {
    let q = format!("{game} {console} video game");
    let found = get(http, &[("action", "query"), ("list", "search"), ("srsearch", &q), ("srlimit", "5"), ("format", "json")]).await?;
    let titles: Vec<String> = found.pointer("/query/search")?.as_array()?.iter().filter_map(|x| x.get("title")?.as_str().map(str::to_string)).collect();
    // The best-fitting title, as long as it has most of the name's words.
    let title = titles.iter().map(|t| (fit(t, game), t)).filter(|(f, _)| *f >= 0.6).max_by(|a, b| a.0.partial_cmp(&b.0).unwrap())?.1.clone();
    let page = get(
        http,
        &[("action", "query"), ("prop", "extracts|description|info"), ("exintro", "1"), ("explaintext", "1"), ("inprop", "url"), ("redirects", "1"), ("titles", &title), ("format", "json")],
    )
    .await?;
    let p = page.pointer("/query/pages")?.as_object()?.values().next()?.clone();
    let text = p.get("extract")?.as_str()?.trim().to_string();
    let kind = p.get("description").and_then(Value::as_str).unwrap_or("").to_string();
    // Only a game's article: not a film, a series or a disambiguation page.
    let k = kind.to_lowercase();
    if text.len() < 80 || !(k.contains("game") || text.to_lowercase().contains("video game")) || k.contains("series") || k.contains("disambiguation") {
        return None;
    }
    Some(Info {
        title: p.get("title").and_then(Value::as_str).unwrap_or(&title).to_string(),
        kind,
        text: text.split("\n").map(str::trim).filter(|l| !l.is_empty()).take(4).collect::<Vec<_>>().join("\n\n"),
        url: p.get("fullurl").and_then(Value::as_str).unwrap_or("").to_string(),
    })
}

/// The game's article, from the copy on disk when there's a recent one. None when Wikipedia has
/// nothing that fits (kept too, so it isn't asked again for a week).
pub async fn game_info(http: &reqwest::Client, dir: &Path, game: &str, console: &str) -> Option<Info> {
    if game.trim().is_empty() {
        return None;
    }
    let file = dir.join("cache/info").join(format!("{}.json", &hex::encode(Sha256::digest(format!("{game}|{console}").as_bytes()))[..24]));
    let age = std::fs::metadata(&file).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).map(|d| d.as_secs());
    let saved: Option<Option<Info>> = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok());
    if let (Some(age), Some(v)) = (age, &saved) {
        let keep = if v.is_some() { 30 * 86400 } else { 7 * 86400 };
        if age < keep {
            return v.clone();
        }
    }
    match tokio::time::timeout(Duration::from_secs(25), lookup(http, game, console)).await {
        Ok(found) => {
            let _ = std::fs::create_dir_all(file.parent().unwrap());
            let _ = std::fs::write(&file, serde_json::to_vec(&found).unwrap_or_default());
            found
        }
        // Offline or slow: the old copy, if any.
        Err(_) => saved.flatten(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_that_fit() {
        assert!(fit("NBA Street Vol. 2", "NBA Street Vol. 2") > 0.99);
        assert!(fit("The Legend of Zelda: Majora's Mask", "Majora's Mask") > 0.99);
        assert!(fit("NBA Street (video game)", "NBA Street Vol. 2") < 0.99);
        assert!(fit("Super Mario 64", "Banjo-Kazooie") < 0.1);
    }
}

//! The list of recomps and each one's GitHub releases, read from recomplauncher.com.
//!
//! Both are kept on disk, so the launcher opens with the last copy when it's offline and the
//! Library always works. Releases come through recomplauncher.com/api/gh.php, which asks GitHub
//! once for everyone and keeps working past GitHub's rate limit.

use crate::assets::Asset;
use crate::Error;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// recomplauncher.com, or RL_API for testing against a local copy of the site.
pub fn base() -> String {
    std::env::var("RL_API").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "https://recomplauncher.com".into()).trim_end_matches('/').to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub html_url: Option<String>,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

#[derive(Debug, Serialize)]
pub struct Fetched {
    pub data: Value,
    /// True when this is the copy on disk because the site couldn't be reached.
    pub offline: bool,
    /// Seconds since the copy on disk was saved (0 when fresh).
    pub age: u64,
}

fn age_of(p: &Path) -> Option<u64> {
    let m = std::fs::metadata(p).ok()?.modified().ok()?;
    Some(SystemTime::now().duration_since(m).unwrap_or_default().as_secs())
}

fn save(p: &Path, v: &Value) {
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let tmp = p.with_extension("tmp");
    if std::fs::write(&tmp, serde_json::to_vec(v).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(&tmp, p);
    }
}

fn load(p: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(p).ok()?).ok()
}

async fn get_json(http: &reqwest::Client, url: &str) -> Result<Value, Error> {
    let r = http.get(url).timeout(Duration::from_secs(20)).send().await?;
    if !r.status().is_success() {
        return Err(Error::Msg(format!("recomplauncher.com answered {}", r.status().as_u16())));
    }
    Ok(r.json::<Value>().await?)
}

/// Ask the site, or fall back to the copy on disk. A copy younger than `fresh` seconds is used
/// without asking.
async fn cached(http: &reqwest::Client, url: &str, file: PathBuf, fresh: u64, check: fn(&Value) -> bool) -> Result<Fetched, Error> {
    if let (Some(age), Some(v)) = (age_of(&file), load(&file)) {
        if age < fresh && check(&v) {
            return Ok(Fetched { data: v, offline: false, age });
        }
    }
    match get_json(http, url).await {
        Ok(v) if check(&v) => {
            save(&file, &v);
            Ok(Fetched { data: v, offline: false, age: 0 })
        }
        Ok(_) => Err(Error::Msg("recomplauncher.com sent something unexpected".into())),
        Err(e) => match load(&file) {
            Some(v) => Ok(Fetched { data: v, offline: true, age: age_of(&file).unwrap_or(0) }),
            None => Err(e),
        },
    }
}

pub async fn catalog(http: &reqwest::Client, dir: &Path, force: bool) -> Result<Fetched, Error> {
    let fresh = if force { 0 } else { 300 };
    cached(http, &format!("{}/pc-catalog.json", base()), dir.join("cache/pc-catalog.json"), fresh, |v| v.get("apps").map(Value::is_array).unwrap_or(false)).await
}

/// What players say about each recomp, in counts: { id: { runs, issues, broken } }.
pub async fn report_counts(http: &reqwest::Client, dir: &Path, force: bool) -> Result<Fetched, Error> {
    let fresh = if force { 0 } else { 900 };
    cached(http, &format!("{}/api/pc.php?a=counts", base()), dir.join("cache/pc-counts.json"), fresh, |v| v.get("counts").map(Value::is_object).unwrap_or(false)).await
}

/// One recomp's player reports: the counts and the newest reports, with the PC each was played on.
pub async fn reports(http: &reqwest::Client, dir: &Path, id: &str, force: bool) -> Result<Fetched, Error> {
    if id.is_empty() || id.len() > 120 || !id.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)) {
        return Err(Error::Msg("That isn't a recomp.".into()));
    }
    let fresh = if force { 0 } else { 600 };
    cached(http, &format!("{}/api/pc.php?a=feedback&id={id}", base()), dir.join("cache/reports").join(format!("{id}.json")), fresh, |v| v.get("counts").map(Value::is_object).unwrap_or(false)).await
}

fn repo_ok(repo: &str) -> bool {
    let mut it = repo.split('/');
    let ok = |s: Option<&str>| s.map(|s| !s.is_empty() && s.len() <= 100 && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)) && s != "." && s != "..").unwrap_or(false);
    ok(it.next()) && ok(it.next()) && it.next().is_none()
}

pub async fn releases(http: &reqwest::Client, dir: &Path, repo: &str, force: bool) -> Result<Fetched, Error> {
    if !repo_ok(repo) {
        return Err(Error::Msg("That isn't a GitHub repo".into()));
    }
    let file = dir.join("cache/releases").join(format!("{}.json", repo.replace('/', "__")));
    let url = format!("{}/api/gh.php?repo={}", base(), repo);
    cached(http, &url, file, if force { 0 } else { 600 }, Value::is_array).await
}

/// The newest release to install: not a draft, and not a pre-release unless that's all there is.
pub fn latest(rels: &[Release]) -> Option<&Release> {
    rels.iter().find(|r| !r.draft && !r.prerelease).or_else(|| rels.iter().find(|r| !r.draft))
}

/// One app's entry in the catalog, by id.
pub fn app<'a>(cat: &'a Value, id: &str) -> Option<&'a Value> {
    cat.get("apps")?.as_array()?.iter().find(|a| a.get("id").and_then(Value::as_str) == Some(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repos() {
        assert!(repo_ok("Zelda64Recomp/Zelda64Recomp"));
        assert!(repo_ok("a-b/c.d_e"));
        assert!(!repo_ok("a/b/c"));
        assert!(!repo_ok("../etc"));
        assert!(!repo_ok("a/.."));
        assert!(!repo_ok("a/b?x=1"));
    }

    #[test]
    fn newest_stable_first() {
        let r = |t: &str, pre: bool, draft: bool| Release { tag_name: t.into(), name: None, published_at: None, prerelease: pre, draft, body: None, html_url: None, assets: vec![] };
        assert_eq!(latest(&[r("v3", false, true), r("v2", true, false), r("v1", false, false)]).unwrap().tag_name, "v1");
        assert_eq!(latest(&[r("v2", true, false)]).unwrap().tag_name, "v2");
        assert!(latest(&[r("v2", false, true)]).is_none());
    }
}

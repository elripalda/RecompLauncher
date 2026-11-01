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
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

/// The server everything is read from (see net.rs, where its address lives).
pub fn base() -> String {
    crate::net::base()
}

/// How long the list and each recomp's release info are used before asking again.
pub const LIST_FRESH: u64 = 6 * 3600;
/// How long what players say about each recomp is used before asking again.
pub const COUNTS_FRESH: u64 = 3600;

/// The list has been asked for once since the launcher opened: after that it comes from disk,
/// whatever its age, until the player presses Refresh.
static LIST_ASKED: AtomicBool = AtomicBool::new(false);

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

async fn get_json(http: &reqwest::Client, url: &str, wait: Duration, player: bool) -> Result<Value, Error> {
    let r = crate::net::send(http.get(url).timeout(wait), url, player).await?;
    if !r.status().is_success() {
        return Err(Error::Net(format!("recomplauncher.com answered {}", r.status().as_u16())));
    }
    match r.json::<Value>().await {
        Ok(v) => Ok(v),
        Err(e) => {
            crate::net::bad_answer(url);
            Err(e.into())
        }
    }
}

/// The copy on disk, if there's one that looks right, with how old it is.
fn on_disk(file: &Path, check: fn(&Value) -> bool) -> Option<Fetched> {
    let v = load(file).filter(check)?;
    Some(Fetched { data: v, offline: crate::net::offline(), age: age_of(file).unwrap_or(0) })
}

/// The copy on disk when it's younger than `fresh` seconds; otherwise the site is asked, and the
/// copy on disk (any age) is used when it can't be reached. A site that's resting isn't asked.
async fn cached(http: &reqwest::Client, url: &str, file: PathBuf, fresh: u64, wait: Duration, player: bool, check: fn(&Value) -> bool) -> Result<Fetched, Error> {
    let have = on_disk(&file, check);
    if let Some(h) = &have {
        if h.age < fresh || crate::net::resting(url, player) {
            return Ok(Fetched { data: h.data.clone(), offline: h.age >= fresh, age: h.age });
        }
    }
    match get_json(http, url, wait, player).await {
        Ok(v) if check(&v) => {
            save(&file, &v);
            Ok(Fetched { data: v, offline: false, age: 0 })
        }
        Ok(_) => {
            crate::net::bad_answer(url);
            have.map(|h| Fetched { offline: true, ..h }).ok_or_else(|| Error::Net("recomplauncher.com sent something unexpected".into()))
        }
        Err(e) => match have {
            Some(h) => Ok(Fetched { offline: true, ..h }),
            None => Err(e),
        },
    }
}

fn list_file(dir: &Path) -> PathBuf {
    dir.join("cache/pc-catalog.json")
}

fn list_ok(v: &Value) -> bool {
    v.get("apps").map(Value::is_array).unwrap_or(false)
}

/// The list. `force`: the player pressed Refresh. Otherwise it's asked for once per launch, and
/// only when the copy on disk is more than six hours old; everything after that reads the disk.
pub async fn catalog(http: &reqwest::Client, dir: &Path, force: bool) -> Result<Fetched, Error> {
    if !force && LIST_ASKED.load(Ordering::Relaxed) {
        if let Some(f) = on_disk(&list_file(dir), list_ok) {
            return Ok(Fetched { data: with_mine(dir, f.data), ..f });
        }
    }
    LIST_ASKED.store(true, Ordering::Relaxed);
    let fresh = if force { 0 } else { LIST_FRESH };
    let mut f = cached(http, &format!("{}/pc-catalog.json", base()), list_file(dir), fresh, crate::net::LONGER, force, list_ok).await?;
    f.data = with_mine(dir, f.data);
    Ok(f)
}

/// For the window as it opens: the copy on disk straight away (None when there isn't one), and
/// whether it's old enough to be asked for again in the background.
pub fn catalog_now(dir: &Path) -> Option<(Fetched, bool)> {
    let f = on_disk(&list_file(dir), list_ok)?;
    // Decided once per launch: after this, the list is only asked for again by Refresh or a push.
    let stale = f.age >= LIST_FRESH && !LIST_ASKED.swap(true, Ordering::Relaxed);
    Some((Fetched { data: with_mine(dir, f.data), ..f }, stale))
}

/// The list asked for again in the background (not the player's doing).
pub async fn catalog_refresh(http: &reqwest::Client, dir: &Path) -> Result<Fetched, Error> {
    LIST_ASKED.store(true, Ordering::Relaxed);
    let mut f = cached(http, &format!("{}/pc-catalog.json", base()), list_file(dir), 0, crate::net::LONGER, false, list_ok).await?;
    f.data = with_mine(dir, f.data);
    Ok(f)
}

// ---------- recomps the player added by their GitHub link ----------

fn mine_file(dir: &Path) -> PathBuf {
    dir.join("my-recomps.json")
}

/// The recomps the player added themselves, as catalog entries.
pub fn mine(dir: &Path) -> Vec<Value> {
    std::fs::read(mine_file(dir)).ok().and_then(|b| serde_json::from_slice::<Vec<Value>>(&b).ok()).unwrap_or_default()
}

pub fn save_mine(dir: &Path, list: &[Value]) -> std::io::Result<()> {
    std::fs::write(mine_file(dir), serde_json::to_vec_pretty(list).unwrap_or_default())
}

/// The catalog with the player's own recomps added after it (one the catalog has since listed
/// itself, by the same repo, stays the catalog's).
pub fn with_mine(dir: &Path, mut cat: Value) -> Value {
    let list = mine(dir);
    if list.is_empty() {
        return cat;
    }
    if !cat.is_object() {
        cat = serde_json::json!({ "apps": [] });
    }
    if !cat.get("apps").map(Value::is_array).unwrap_or(false) {
        cat["apps"] = Value::Array(vec![]);
    }
    let apps = cat["apps"].as_array_mut().unwrap();
    for e in list {
        let repo = e.get("repo").and_then(Value::as_str).unwrap_or("").to_lowercase();
        let id = e.get("id").and_then(Value::as_str).unwrap_or("");
        let listed = apps.iter().any(|a| a.get("id").and_then(Value::as_str) == Some(id) || (!repo.is_empty() && a.get("repo").and_then(Value::as_str).map(|r| r.to_lowercase()) == Some(repo.clone())));
        if !listed {
            apps.push(e);
        }
    }
    cat
}

/// owner/repo from a GitHub link (or "owner/repo" itself).
pub fn repo_from_link(s: &str) -> Option<String> {
    let s = s.trim().trim_end_matches('/');
    let s = s.strip_prefix("https://").or_else(|| s.strip_prefix("http://")).unwrap_or(s);
    let s = s.strip_prefix("www.").unwrap_or(s);
    // Only GitHub: another site's link (its host has a dot) isn't one.
    let rest = match s.strip_prefix("github.com/") {
        Some(r) => r,
        None if s.split('/').next().map(|h| h.contains('.')).unwrap_or(false) => return None,
        None => s,
    };
    let mut parts = rest.split(['/', '?', '#']).filter(|p| !p.is_empty());
    let (o, r) = (parts.next()?, parts.next()?);
    let r = r.strip_suffix(".git").unwrap_or(r);
    let repo = format!("{o}/{r}");
    repo_ok(&repo).then_some(repo)
}

/// A repo's name as a game's: "burnout3-pc" -> "Burnout3 PC".
pub fn name_from_repo(repo: &str) -> String {
    let r = repo.rsplit('/').next().unwrap_or(repo);
    r.split(['-', '_', '.'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let lw = w.to_lowercase();
            if matches!(lw.as_str(), "pc" | "hd" | "ps2" | "ps3" | "n64" | "gc" | "wii" | "xbox" | "x360") {
                lw.to_uppercase()
            } else {
                let mut c = w.chars();
                c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// What players say about each recomp, in counts: { id: { runs, issues, broken } }.
pub async fn report_counts(http: &reqwest::Client, dir: &Path, force: bool) -> Result<Fetched, Error> {
    let fresh = if force { 0 } else { COUNTS_FRESH };
    cached(http, &format!("{}/api/pc.php?a=counts", base()), dir.join("cache/pc-counts.json"), fresh, crate::net::QUICK, force, |v| v.get("counts").map(Value::is_object).unwrap_or(false)).await
}

/// One recomp's player reports: the counts and the newest reports, with the PC each was played on.
pub async fn reports(http: &reqwest::Client, dir: &Path, id: &str, force: bool) -> Result<Fetched, Error> {
    if id.is_empty() || id.len() > 120 || !id.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)) {
        return Err(Error::Msg("That isn't a recomp.".into()));
    }
    let fresh = if force { 0 } else { COUNTS_FRESH };
    cached(http, &format!("{}/api/pc.php?a=feedback&id={id}", base()), dir.join("cache/reports").join(format!("{id}.json")), fresh, crate::net::QUICK, true, |v| v.get("counts").map(Value::is_object).unwrap_or(false)).await
}

fn repo_ok(repo: &str) -> bool {
    let mut it = repo.split('/');
    let ok = |s: Option<&str>| s.map(|s| !s.is_empty() && s.len() <= 100 && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)) && s != "." && s != "..").unwrap_or(false);
    ok(it.next()) && ok(it.next()) && it.next().is_none()
}

fn releases_file(dir: &Path, repo: &str) -> PathBuf {
    dir.join("cache/releases").join(format!("{}.json", repo.replace('/', "__")))
}

fn releases_url(repo: &str) -> String {
    format!("{}/api/gh.php?repo={}", base(), repo)
}

/// A recomp's releases. Used from disk while younger than six hours. `force`: the player asked
/// for it now (installing, updating, adding a repo).
pub async fn releases(http: &reqwest::Client, dir: &Path, repo: &str, force: bool) -> Result<Fetched, Error> {
    if !repo_ok(repo) {
        return Err(Error::Msg("That isn't a GitHub repo".into()));
    }
    cached(http, &releases_url(repo), releases_file(dir, repo), if force { 0 } else { LIST_FRESH }, crate::net::QUICK, force, Value::is_array).await
}

/// For installing or updating: the player's doing, but a copy from the last `secs` seconds (an
/// update that was just found) is used rather than asking again.
pub async fn releases_within(http: &reqwest::Client, dir: &Path, repo: &str, secs: u64) -> Result<Fetched, Error> {
    if !repo_ok(repo) {
        return Err(Error::Msg("That isn't a GitHub repo".into()));
    }
    cached(http, &releases_url(repo), releases_file(dir, repo), secs, crate::net::QUICK, true, Value::is_array).await
}

/// A recomp's releases as kept on disk (any age), without asking anyone.
pub fn releases_on_disk(dir: &Path, repo: &str) -> Option<Fetched> {
    if !repo_ok(repo) {
        return None;
    }
    on_disk(&releases_file(dir, repo), Value::is_array)
}

/// Asked again in the background (not the player's doing): a push about a new release.
pub async fn releases_refresh(http: &reqwest::Client, dir: &Path, repo: &str) -> Result<Fetched, Error> {
    if !repo_ok(repo) {
        return Err(Error::Msg("That isn't a GitHub repo".into()));
    }
    cached(http, &releases_url(repo), releases_file(dir, repo), 0, crate::net::QUICK, false, Value::is_array).await
}

/// The newest release to install: not a draft, and not a pre-release unless that's all there is.
pub fn latest(rels: &[Release]) -> Option<&Release> {
    rels.iter().find(|r| !r.draft && !r.prerelease).or_else(|| rels.iter().find(|r| !r.draft))
}

/// The release to update an installed one to: the newest stable one, or for a player on a
/// pre-release, the newest of all. Never one older than what they have (a beta of the next
/// version is newer than the last stable one).
pub fn latest_for<'a>(rels: &'a [Release], installed: &str) -> Option<&'a Release> {
    let cur = rels.iter().find(|r| r.tag_name == installed);
    let pick = if cur.map(|r| r.prerelease).unwrap_or(false) { rels.iter().find(|r| !r.draft) } else { latest(rels) }?;
    if let (Some(c), Some(new), Some(old)) = (cur, pick.published_at.as_deref(), cur.and_then(|r| r.published_at.as_deref())) {
        if new < old {
            return Some(c);
        }
    }
    Some(pick)
}

/// One app's entry in the catalog, by id.
pub fn app<'a>(cat: &'a Value, id: &str) -> Option<&'a Value> {
    cat.get("apps")?.as_array()?.iter().find(|a| a.get("id").and_then(Value::as_str) == Some(id))
}

#[cfg(test)]
mod tests {
    #[test]
    fn github_links() {
        assert_eq!(super::repo_from_link("https://github.com/danielgomesvieira2000/burnout3-pc").as_deref(), Some("danielgomesvieira2000/burnout3-pc"));
        assert_eq!(super::repo_from_link("github.com/a/b.git").as_deref(), Some("a/b"));
        assert_eq!(super::repo_from_link("https://github.com/a/b/releases/tag/v1").as_deref(), Some("a/b"));
        assert_eq!(super::repo_from_link("a/b").as_deref(), Some("a/b"));
        assert_eq!(super::repo_from_link("https://gitlab.com/a/b"), None);
        assert_eq!(super::name_from_repo("x/burnout3-pc"), "Burnout3 PC");
        assert_eq!(super::name_from_repo("x/gt3_revisited"), "Gt3 Revisited");
    }

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

/// A recomp that needs things on the PC before it plays (Python, build tools, a long first build),
/// going by the copy of the list on disk: true until the player has said it's set up.
pub fn needs_setup_first(dir: &Path, id: &str, ui: &Value) -> bool {
    let done = ui.get("setup_ok").and_then(|o| o.get(id)).and_then(Value::as_bool).unwrap_or(false);
    if done {
        return false;
    }
    load(&list_file(dir))
        .and_then(|c| c.get("apps").and_then(Value::as_array).and_then(|a| a.iter().find(|x| x.get("id").and_then(Value::as_str) == Some(id)).cloned()))
        .and_then(|a| a.get("setup_required").and_then(Value::as_bool))
        .unwrap_or(false)
}

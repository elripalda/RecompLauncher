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

/// How long the list is used before asking again: an hour, so a recomp the store just added shows
/// soon after.
pub const LIST_FRESH: u64 = 3600;
/// How long each recomp's release info is used before asking again.
pub const RELEASES_FRESH: u64 = 6 * 3600;
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
    /// A GitLab project's number (gl.php gives it): its uploaded files live under
    /// gitlab.com/-/project/<number>/uploads/.
    #[serde(default)]
    pub project_id: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct Fetched {
    pub data: Value,
    /// True when this is the copy on disk because the site couldn't be reached.
    pub offline: bool,
    /// Seconds since the copy on disk was saved (0 when fresh).
    pub age: u64,
    /// Best-effort GitHub repository status, separate from the release array.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
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

/// What the site said last time about a file kept on disk (its ETag and Last-Modified), so the
/// next ask can be "only if it changed" and an unchanged file comes back as a tiny 304.
fn tag_file(file: &Path) -> PathBuf {
    let mut n = file.as_os_str().to_owned();
    n.push(".tag");
    PathBuf::from(n)
}

fn load_tag(file: &Path) -> (Option<String>, Option<String>) {
    let v: Value = std::fs::read(tag_file(file)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    let s = |k: &str| v.get(k).and_then(Value::as_str).filter(|s| !s.is_empty() && s.len() < 300).map(String::from);
    (s("etag"), s("modified"))
}

/// The copy on disk is current again (the site said it hasn't changed): its age starts over.
fn touch(file: &Path) {
    if let Ok(f) = std::fs::OpenOptions::new().append(true).open(file) {
        let _ = f.set_modified(SystemTime::now());
    }
}

/// Asks the site for a JSON file. With `file` (the copy on disk), only if it changed: None means
/// it didn't (304), and the copy on disk is still right.
async fn get_json(http: &reqwest::Client, url: &str, wait: Duration, player: bool, file: Option<&Path>) -> Result<Option<(Value, reqwest::header::HeaderMap)>, Error> {
    let mut rb = http.get(url).timeout(wait);
    if let Some(f) = file.filter(|f| f.is_file()) {
        let (etag, modified) = load_tag(f);
        if let Some(e) = etag {
            rb = rb.header(reqwest::header::IF_NONE_MATCH, e);
        }
        if let Some(m) = modified {
            rb = rb.header(reqwest::header::IF_MODIFIED_SINCE, m);
        }
    }
    let r = crate::net::send(rb, url, player).await?;
    if r.status() == reqwest::StatusCode::NOT_MODIFIED && file.map(|f| f.is_file()).unwrap_or(false) {
        return Ok(None);
    }
    if !r.status().is_success() {
        return Err(Error::Net(format!("{} answered {}", host_of(url), r.status().as_u16())));
    }
    let headers = r.headers().clone();
    match r.json::<Value>().await {
        Ok(v) => Ok(Some((v, headers))),
        Err(e) => {
            crate::net::bad_answer(url);
            Err(e.into())
        }
    }
}

/// Who a request went to, for what the player reads: the site, or a Forgejo project's own server.
fn host_of(url: &str) -> String {
    reqwest::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_else(|| "recomplauncher.com".into())
}

fn save_tag_from(file: &Path, h: &reqwest::header::HeaderMap) {
    let g = |k: reqwest::header::HeaderName| h.get(k).and_then(|v| v.to_str().ok()).map(String::from);
    let (etag, modified) = (g(reqwest::header::ETAG), g(reqwest::header::LAST_MODIFIED));
    if etag.is_none() && modified.is_none() {
        let _ = std::fs::remove_file(tag_file(file));
    } else {
        let _ = std::fs::write(tag_file(file), serde_json::to_vec(&serde_json::json!({ "etag": etag, "modified": modified })).unwrap_or_default());
    }
}

/// The copy on disk, if there's one that looks right, with how old it is.
fn on_disk(file: &Path, check: fn(&Value) -> bool) -> Option<Fetched> {
    let v = load(file).filter(check)?;
    Some(Fetched { archived: None, data: v, offline: crate::net::offline(), age: age_of(file).unwrap_or(0) })
}

/// The copy on disk when it's younger than `fresh` seconds; otherwise the site is asked, and the
/// copy on disk (any age) is used when it can't be reached. A site that's resting isn't asked.
async fn cached(http: &reqwest::Client, url: &str, file: PathBuf, fresh: u64, wait: Duration, player: bool, check: fn(&Value) -> bool) -> Result<Fetched, Error> {
    let have = on_disk(&file, check);
    if let Some(h) = &have {
        if h.age < fresh || crate::net::resting(url, player) {
            return Ok(Fetched { archived: h.archived, data: h.data.clone(), offline: h.age >= fresh, age: h.age });
        }
    }
    match get_json(http, url, wait, player, have.as_ref().map(|_| file.as_path())).await {
        // Unchanged since the copy on disk: a 304 of a few bytes instead of the whole file.
        Ok(None) => {
            touch(&file);
            match have {
                Some(h) => Ok(Fetched { offline: false, age: 0, ..h }),
                None => Err(Error::Net(format!("{} sent something unexpected", host_of(url)))),
            }
        }
        Ok(Some((v, headers))) if check(&v) => {
            save(&file, &v);
            save_tag_from(&file, &headers);
            Ok(Fetched { archived: None, data: v, offline: false, age: 0 })
        }
        Ok(Some(_)) => {
            crate::net::bad_answer(url);
            have.map(|h| Fetched { offline: true, ..h }).ok_or_else(|| Error::Net(format!("{} sent something unexpected", host_of(url))))
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
/// only when the copy on disk is more than an hour old; everything after that reads the disk.
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

/// Held while the list of games added by link is read, changed and written back, so two changes
/// at once never undo each other.
pub static MINE_TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The recomps the player added themselves, as catalog entries.
pub fn mine(dir: &Path) -> Vec<Value> {
    std::fs::read(mine_file(dir)).ok().and_then(|b| serde_json::from_slice::<Vec<Value>>(&b).ok()).unwrap_or_default()
}

/// Written to a new file that then takes the old one's place, so a launcher closed (or updated)
/// halfway through never leaves the list cut short.
pub fn save_mine(dir: &Path, list: &[Value]) -> std::io::Result<()> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let f = mine_file(dir);
    let tmp = f.with_extension(format!("json.{}-{}.tmp", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::write(&tmp, serde_json::to_vec_pretty(list).unwrap_or_default())?;
    std::fs::rename(&tmp, &f)
}

/// The catalog with the player's own recomps added after it. One the catalog lists under the same
/// id is the catalog's; one whose repo it lists under another id stays too, so a copy the player
/// has keeps its page and updates until it moves over to the catalog's (merge_mine in lib.rs).
pub fn with_mine(dir: &Path, mut cat: Value) -> Value {
    take_newer(&mut cat, env!("CARGO_PKG_VERSION"));
    mark_hosts(&mut cat);
    crate::boot::from_catalog(&mut cat);
    let list = mine(dir);
    if list.is_empty() {
        crate::engines::catalog::merge(&mut cat, crate::engines::Target::current());
        return with_repo_profiles(dir, with_known_project_status(dir, cat));
    }
    if !cat.is_object() {
        cat = serde_json::json!({ "apps": [] });
    }
    if !cat.get("apps").map(Value::is_array).unwrap_or(false) {
        cat["apps"] = Value::Array(vec![]);
    }
    let apps = cat["apps"].as_array_mut().unwrap();
    for e in list {
        let id = e.get("id").and_then(Value::as_str).unwrap_or("");
        if !apps.iter().any(|a| a.get("id").and_then(Value::as_str) == Some(id)) {
            apps.push(e);
        }
    }
    crate::engines::catalog::merge(&mut cat, crate::engines::Target::current());
    with_repo_profiles(dir, with_known_project_status(dir, cat))
}

/// owner/repo from a GitHub link (or "owner/repo" itself), or gitlab:group/project from a GitLab
/// one (with or without /-/releases, a trailing .git, subgroups too).
pub fn repo_from_link(s: &str) -> Option<String> {
    let s = s.trim().trim_end_matches('/');
    let s = s.strip_prefix("https://").or_else(|| s.strip_prefix("http://")).unwrap_or(s);
    let s = s.strip_prefix("www.").unwrap_or(s);
    // sourceforge.net/projects/<name>/... or sourceforge.net/p/<name>/...
    if let Some(rest) = s.strip_prefix("sourceforge.net/projects/").or_else(|| s.strip_prefix("sourceforge.net/p/")) {
        let name = rest.split(['/', '?', '#']).next().unwrap_or("").to_ascii_lowercase();
        let repo = format!("sourceforge:{name}");
        return repo_ok(&repo).then_some(repo);
    }
    if let Some(n) = s.strip_prefix("sourceforge:") {
        let repo = format!("sourceforge:{n}");
        return repo_ok(&repo).then_some(repo);
    }
    if let Some(rest) = s.strip_prefix("gitlab.com/") {
        let rest = rest.split(['?', '#']).next().unwrap_or("");
        let rest = rest.split("/-/").next().unwrap_or("").trim_end_matches('/');
        let rest = rest.strip_suffix(".git").unwrap_or(rest);
        let repo = format!("gitlab:{rest}");
        return repo_ok(&repo).then_some(repo);
    }
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

/// A link to a project on some other server, as host, owner and name: https://<host>/<owner>/<name>
/// and anything after it (/releases, /src/branch/main). Whether that server is a Forgejo or Gitea
/// one only the server can say (forgejo_server).
pub fn forgejo_from_link(s: &str) -> Option<(String, String, String)> {
    let s = s.trim().trim_end_matches('/');
    let s = s.strip_prefix("https://").or_else(|| s.strip_prefix("http://"))?;
    let mut parts = s.split(['/', '?', '#']);
    let host = parts.next()?.to_ascii_lowercase();
    let owner = parts.next()?.to_string();
    let name = parts.next()?;
    let name = name.strip_suffix(".git").unwrap_or(name).to_string();
    let known = ["github.com", "www.github.com", "gitlab.com", "www.gitlab.com", "sourceforge.net", "www.sourceforge.net"];
    if known.contains(&host.as_str()) || !server_ok(&host) || !part_ok(&owner) || !part_ok(&name) {
        return None;
    }
    Some((host, owner, name))
}

/// Servers that answered as Forgejo or Gitea since the launcher opened: a second link to one is
/// taken at once.
static FORGEJO_SERVERS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Whether `host` is a Forgejo or Gitea server: both answer /api/v1/version with {"version": ...}.
/// Only a public host name is asked, briefly, with nothing of the player's.
pub async fn forgejo_server(http: &reqwest::Client, host: &str) -> bool {
    if !server_ok(host) {
        return false;
    }
    if FORGEJO_SERVERS.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|h| h == host) {
        return true;
    }
    if !forgejo_version(http, &format!("https://{host}/api/v1/version")).await {
        return false;
    }
    FORGEJO_SERVERS.lock().unwrap_or_else(|e| e.into_inner()).push(host.to_string());
    true
}

async fn forgejo_version(http: &reqwest::Client, url: &str) -> bool {
    let Ok(r) = http.get(url).timeout(Duration::from_secs(6)).send().await else { return false };
    if !r.status().is_success() {
        return false;
    }
    let v: Value = r.json().await.unwrap_or(Value::Null);
    v.get("version").and_then(Value::as_str).is_some_and(|x| !x.trim().is_empty())
}

/// A project's page: GitHub's, GitLab's, SourceForge's, or its Forgejo server's.
pub fn repo_home(repo: &str) -> String {
    if let Some((server, owner, name)) = forgejo_parts(repo) {
        return format!("https://{server}/{owner}/{name}");
    }
    if let Some(p) = gitlab_path(repo) {
        return format!("https://gitlab.com/{p}");
    }
    if let Some(n) = sourceforge_name(repo) {
        return format!("https://sourceforge.net/projects/{n}");
    }
    format!("https://github.com/{repo}")
}

/// A Forgejo project's README, straight from its server (the default branch's README.md): Some("")
/// when it has none, None when the server couldn't be asked. No token is sent.
pub async fn forgejo_readme(http: &reqwest::Client, repo: &str) -> Option<String> {
    let (server, owner, name) = forgejo_parts(repo).filter(|_| repo_ok(repo))?;
    let url = format!("https://{server}/api/v1/repos/{owner}/{name}/raw/README.md");
    let r = http.get(&url).timeout(Duration::from_secs(20)).send().await.ok()?;
    if r.status() == reqwest::StatusCode::NOT_FOUND {
        return Some(String::new());
    }
    if !r.status().is_success() {
        return None;
    }
    r.text().await.ok()
}

/// What a Forgejo project says about itself (its description, stars and topics) and its README,
/// for recognizing the game when the player adds it. Either can be missing.
pub async fn forgejo_info(http: &reqwest::Client, repo: &str) -> (Value, String) {
    let Some((server, owner, name)) = forgejo_parts(repo).filter(|_| repo_ok(repo)) else { return (Value::Null, String::new()) };
    let api = format!("https://{server}/api/v1/repos/{owner}/{name}");
    let wait = std::time::Duration::from_secs(10);
    let (info, readme) = tokio::join!(http.get(&api).timeout(wait).send(), http.get(format!("{api}/raw/README.md")).timeout(wait).send());
    let info: Value = match info {
        Ok(r) if r.status().is_success() => r.json().await.unwrap_or(Value::Null),
        _ => Value::Null,
    };
    let readme = match readme {
        Ok(r) if r.status().is_success() => r.text().await.unwrap_or_default(),
        _ => String::new(),
    };
    (info, readme)
}

/// A repo's name as a game's: "burnout3-pc" -> "Burnout3 PC".
pub fn name_from_repo(repo: &str) -> String {
    let r = repo.rsplit(['/', ':']).next().unwrap_or(repo);
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

/// How each recomp plays, by where it was played (the site's scores): Steam Deck players' score
/// and how many of them said, for "Great on Deck". Kept an hour, like the counts.
pub async fn play_scores(http: &reqwest::Client, dir: &Path, force: bool) -> Result<Fetched, Error> {
    let fresh = if force { 0 } else { COUNTS_FRESH };
    cached(http, &format!("{}/api/play.php?a=scores", base()), dir.join("cache/play-scores.json"), fresh, crate::net::QUICK, force, |v| v.get("pc").is_some_and(|p| p.is_object() || p.as_array().is_some_and(Vec::is_empty))).await
}

/// Of the scores, the Steam Deck ones: id -> {score, n}.
pub fn deck_scores(scores: &Value) -> Value {
    let mut out = serde_json::Map::new();
    for (id, v) in scores.get("pc").and_then(Value::as_object).into_iter().flatten() {
        if let Some(d) = v.get("sys").and_then(|s| s.get("deck")).filter(|d| d.is_object()) {
            let score = d.get("score").and_then(Value::as_f64).unwrap_or(0.0);
            let n = d.get("n").and_then(Value::as_u64).unwrap_or(0);
            out.insert(id.clone(), serde_json::json!({ "score": score, "n": n }));
        }
    }
    Value::Object(out)
}

/// The RecompStore team's notes for the top of Discover: each with its id, text, a link (https
/// only, else none) and its color (cyan, gold or violet). Nothing when the site can't be reached.
pub async fn team_notes(http: &reqwest::Client) -> Vec<Value> {
    let url = format!("{}/api/notes.php", base());
    let Ok(r) = crate::net::send(http.get(&url).timeout(crate::net::QUICK), &url, false).await else { return vec![] };
    if !r.status().is_success() {
        return vec![];
    }
    let v: Value = r.json().await.unwrap_or(Value::Null);
    notes_from(&v)
}

pub fn notes_from(v: &Value) -> Vec<Value> {
    if !v.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        return vec![];
    }
    let text = |n: &Value, k: &str, max: usize| n.get(k).and_then(Value::as_str).map(|s| s.trim().chars().take(max).collect::<String>()).unwrap_or_default();
    v.get("notes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|n| {
            let id = match n.get("id") {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Number(x)) => x.to_string(),
                _ => return None,
            };
            let body = text(n, "body", 400);
            if id.is_empty() || body.is_empty() {
                return None;
            }
            let link = text(n, "link", 500);
            let link = if link.starts_with("https://") && !link.contains(char::is_whitespace) { link } else { String::new() };
            let tone = text(n, "tone", 12);
            let tone = if ["cyan", "gold", "violet"].contains(&tone.as_str()) { tone } else { "cyan".into() };
            Some(serde_json::json!({ "id": id, "body": body, "link": link, "linkText": if link.is_empty() { String::new() } else { text(n, "linkText", 60) }, "tone": tone }))
        })
        .take(5)
        .collect()
}

/// One recomp's player reports: the counts and the newest reports, with the PC each was played on.
pub async fn reports(http: &reqwest::Client, dir: &Path, id: &str, force: bool) -> Result<Fetched, Error> {
    if id.is_empty() || id.len() > 120 || !id.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)) {
        return Err(Error::Msg("That isn't a recomp.".into()));
    }
    let fresh = if force { 0 } else { COUNTS_FRESH };
    cached(http, &format!("{}/api/pc.php?a=feedback&id={id}", base()), dir.join("cache/reports").join(format!("{id}.json")), fresh, crate::net::QUICK, true, |v| v.get("counts").map(Value::is_object).unwrap_or(false)).await
}

/// How a release's files were checked before the store offered them (ClamAV on the website's server,
/// or VirusTotal): for Downloads, beside each finished download. Kept an hour.
pub async fn scan(http: &reqwest::Client, dir: &Path, id: &str, tag: &str) -> Result<Fetched, Error> {
    let ok_id = !id.is_empty() && id.len() <= 120 && id.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    let ok_tag = !tag.is_empty() && tag.len() <= 120 && tag.chars().all(|c| c.is_ascii_alphanumeric() || "-_.+".contains(c));
    if !ok_id || !ok_tag {
        return Err(Error::Msg("Nothing to look up.".into()));
    }
    let url = format!("{}/api/pc.php?a=scan&id={id}&tag={}", base(), tag.replace('+', "%2B"));
    let file = dir.join("cache/scans").join(format!("{id}@{}.json", tag.replace('+', "_")));
    cached(http, &url, file, 3600, crate::net::QUICK, true, |v| v.get("files").map(Value::is_array).unwrap_or(false)).await
}

/// A project on GitLab, as the launcher keeps it: `gitlab:group/project` (subgroups too:
/// `gitlab:group/subgroup/project`). Its path on gitlab.com.
pub fn gitlab_path(repo: &str) -> Option<&str> {
    repo.strip_prefix("gitlab:")
}

pub fn is_gitlab(repo: &str) -> bool {
    gitlab_path(repo).is_some()
}

/// A project on SourceForge, as the launcher keeps it: `sourceforge:<name>`, the project's short
/// name (sourceforge.net/projects/<name>/). Its name.
pub fn sourceforge_name(repo: &str) -> Option<&str> {
    repo.strip_prefix("sourceforge:")
}

pub fn is_sourceforge(repo: &str) -> bool {
    sourceforge_name(repo).is_some()
}

/// A project on a Forgejo server, as the launcher keeps it: `forgejo:<server>/<owner>/<name>`
/// (the catalog sends "host": "forgejo" with "repo": "<server>/<owner>/<name>"). Its server,
/// owner and name.
pub fn forgejo_parts(repo: &str) -> Option<(&str, &str, &str)> {
    let rest = repo.strip_prefix("forgejo:")?;
    let mut it = rest.split('/');
    let server = it.next()?;
    let owner = it.next()?;
    let name = it.next()?;
    if it.next().is_some() {
        return None;
    }
    Some((server, owner, name))
}

pub fn is_forgejo(repo: &str) -> bool {
    forgejo_parts(repo).is_some()
}

/// Not on GitHub (GitLab, SourceForge, a Forgejo server): GitHub's API is never asked about it.
pub fn site_only(repo: &str) -> bool {
    is_gitlab(repo) || is_sourceforge(repo) || is_forgejo(repo)
}

/// The project's own path: GitLab's group/project, SourceForge's name, a Forgejo project's
/// owner/name, GitHub's owner/repo.
pub fn host_path(repo: &str) -> &str {
    if let Some((server, _, _)) = forgejo_parts(repo) {
        return &repo["forgejo:".len() + server.len() + 1..];
    }
    gitlab_path(repo).or_else(|| sourceforge_name(repo)).unwrap_or(repo)
}

/// A Forgejo server's name: a public host name with a dot in it, in lowercase, with no port, user
/// or path. Never an IP address, so a catalog entry can't point the launcher at a machine on the
/// player's own network.
fn server_ok(s: &str) -> bool {
    if !(4..=253).contains(&s.len()) || !s.contains('.') {
        return false;
    }
    let labels: Vec<&str> = s.split('.').collect();
    let label_ok = |l: &&str| (1..=63).contains(&l.len()) && !l.starts_with('-') && !l.ends_with('-') && l.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !labels.iter().all(label_ok) {
        return false;
    }
    // A top-level domain is never all digits: that's an IP address.
    let last = labels.last().copied().unwrap_or("");
    !last.chars().all(|c| c.is_ascii_digit())
}

/// A SourceForge project name: lowercase letters, digits and dashes, 2 to 63 of them.
fn sf_name_ok(n: &str) -> bool {
    (2..=63).contains(&n.len()) && n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn part_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= 100 && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)) && s != "." && s != ".."
}

pub fn repo_ok(repo: &str) -> bool {
    if let Some(n) = sourceforge_name(repo) {
        return sf_name_ok(n);
    }
    if let Some((server, owner, name)) = forgejo_parts(repo) {
        return server_ok(server) && part_ok(owner) && part_ok(name);
    }
    if repo.starts_with("forgejo:") {
        return false;
    }
    if let Some(p) = gitlab_path(repo) {
        let parts: Vec<&str> = p.split('/').collect();
        return (2..=20).contains(&parts.len()) && parts.iter().all(|s| part_ok(s));
    }
    let mut it = repo.split('/');
    let ok = |s: Option<&str>| s.map(part_ok).unwrap_or(false);
    ok(it.next()) && ok(it.next()) && it.next().is_none()
}

fn releases_file(dir: &Path, repo: &str) -> PathBuf {
    dir.join("cache/releases").join(format!("{}.json", repo.replace(':', "_").replace('/', "__")))
}

/// Where a project's releases are read: GitHub's through gh.php, GitLab's through gl.php, which
/// gives them in the same shape. A Forgejo project's straight from its own server, whose release
/// list is already in GitHub's shape.
pub(crate) fn releases_url(repo: &str) -> String {
    if let Some(n) = sourceforge_name(repo) {
        return format!("{}/api/sf.php?repo={}", base(), n);
    }
    if let Some((server, owner, name)) = forgejo_parts(repo) {
        return forgejo_releases_url(&format!("https://{server}"), owner, name);
    }
    match gitlab_path(repo) {
        Some(p) => format!("{}/api/gl.php?repo={}", base(), p.replace('/', "%2F")),
        None => format!("{}/api/gh.php?repo={}", base(), repo),
    }
}

/// A Forgejo server's release list for one project (its newest 30, drafts and pre-releases too).
fn forgejo_releases_url(origin: &str, owner: &str, name: &str) -> String {
    format!("{origin}/api/v1/repos/{owner}/{name}/releases?limit=30")
}

/// Many GitLab projects put their downloads in the release notes (files uploaded to the project,
/// linked as `/uploads/<secret>/<file>`) rather than as release assets. For a release with no
/// assets, those links become its assets, each with the SHA-256 the notes give for it, if any.
/// GitLab serves those files only by the project's number (`/-/project/<number>/uploads/...`), so
/// this needs the `project_id` gl.php gives with each release.
pub fn gitlab_assets(repo: &str, data: &mut Value) {
    let Some(path) = gitlab_path(repo) else { return };
    let Some(list) = data.as_array_mut() else { return };
    let link = regex_lite::Regex::new(r#"(https://gitlab\.com/([A-Za-z0-9_./-]+?))?/(?:-/project/\d+/)?uploads/([0-9a-fA-F]{16,64})/([^\s)\]"'<>]+)"#).unwrap();
    let hash = regex_lite::Regex::new(r"\b([0-9a-fA-F]{64})\b").unwrap();
    let norm = |n: &str| n.trim().to_lowercase().replace(' ', "_");
    for r in list.iter_mut() {
        if r.get("assets").and_then(Value::as_array).map(|a| !a.is_empty()).unwrap_or(false) {
            continue;
        }
        let Some(project) = r.get("project_id").and_then(Value::as_u64) else { continue };
        let body = r.get("body").and_then(Value::as_str).unwrap_or("").to_string();
        // "<sha256> <file>" (or "<file> <sha256>") lines.
        let mut sums: Vec<(String, String)> = vec![];
        for line in body.lines() {
            if let Some(m) = hash.find(line) {
                let rest = format!("{} {}", &line[..m.start()], &line[m.end()..]);
                let name = rest.trim_matches(|c: char| c.is_whitespace() || "-*`:|".contains(c)).to_string();
                if !name.is_empty() {
                    sums.push((norm(&name), m.as_str().to_lowercase()));
                }
            }
        }
        let mut assets: Vec<Value> = vec![];
        for c in link.captures_iter(&body) {
            // Another project's file isn't this one's.
            if c.get(2).map(|p| !p.as_str().eq_ignore_ascii_case(path) && p.as_str() != format!("-/project/{project}")).unwrap_or(false) {
                continue;
            }
            let file = c[4].trim_end_matches(['.', ',']).to_string();
            let name = percent_decode(&file);
            if name.contains('/') || name.contains('\\') || crate::assets::kind(&name).is_none() {
                continue;
            }
            let url = format!("https://gitlab.com/-/project/{project}/uploads/{}/{file}", &c[3]);
            if assets.iter().any(|a| a["browser_download_url"] == url.as_str()) {
                continue;
            }
            let digest = sums.iter().find(|(n, _)| *n == norm(&name)).map(|(_, h)| format!("sha256:{h}"));
            assets.push(serde_json::json!({ "name": name, "size": 0, "browser_download_url": url, "digest": digest }));
        }
        if !assets.is_empty() {
            r["assets"] = Value::Array(assets);
        }
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// A release list with files only in its notes and no project number from the site: the number
/// is needed to download them.
fn needs_project_id(data: &Value) -> bool {
    data.as_array()
        .map(|l| l.iter().any(|r| r.get("assets").and_then(Value::as_array).map(|a| a.is_empty()).unwrap_or(true) && r.get("project_id").and_then(Value::as_u64).is_none()))
        .unwrap_or(false)
}

fn put_project_id(data: &mut Value, id: u64) {
    for r in data.as_array_mut().into_iter().flatten() {
        if r.get("project_id").and_then(Value::as_u64).is_none() {
            r["project_id"] = Value::from(id);
        }
    }
}

fn project_ids_file(dir: &Path) -> PathBuf {
    dir.join("cache/gitlab-projects.json")
}

/// A GitLab project's number as kept on disk (it never changes).
fn project_id_on_disk(dir: &Path, path: &str) -> Option<u64> {
    load(&project_ids_file(dir))?.get(path.to_lowercase())?.as_u64()
}

/// A GitLab project's number when the site doesn't give it: asked of GitLab once, then kept.
async fn project_id(http: &reqwest::Client, dir: &Path, path: &str) -> Option<u64> {
    if let Some(id) = project_id_on_disk(dir, path) {
        return Some(id);
    }
    let url = format!("https://gitlab.com/api/v4/projects/{}", path.replace('/', "%2F"));
    let r = http.get(&url).timeout(crate::net::QUICK).send().await.ok()?;
    if !r.status().is_success() {
        return None;
    }
    let v: Value = r.json().await.ok()?;
    // Only the project asked about.
    if !v.get("path_with_namespace").and_then(Value::as_str).map(|p| p.eq_ignore_ascii_case(path)).unwrap_or(false) {
        return None;
    }
    let id = v.get("id").and_then(Value::as_u64)?;
    let file = project_ids_file(dir);
    let mut all = load(&file).filter(Value::is_object).unwrap_or_else(|| serde_json::json!({}));
    all[path.to_lowercase()] = Value::from(id);
    save(&file, &all);
    crate::trail::note(&format!("GitLab project {path} is number {id}"));
    Some(id)
}

/// A Forgejo release list as the launcher uses it: each file's download link only. GitHub's lists
/// carry a second, API address for a file, which the downloader falls back to; a file on a Forgejo
/// server is only ever downloaded from that server.
pub fn forgejo_assets(repo: &str, data: &mut Value) {
    if !is_forgejo(repo) {
        return;
    }
    for r in data.as_array_mut().into_iter().flatten() {
        for a in r.get_mut("assets").and_then(Value::as_array_mut).into_iter().flatten() {
            if let Some(o) = a.as_object_mut() {
                o.remove("url");
            }
        }
    }
}

/// A GitLab release list made ready to use: its project's number where the site left it out
/// (asked of GitLab once when `http` is given, else only from disk), and the files its notes link to.
async fn with_gitlab(http: Option<&reqwest::Client>, dir: &Path, repo: &str, mut f: Fetched) -> Fetched {
    f.archived = f.archived.or_else(|| github_archived_on_disk(dir, repo));
    forgejo_assets(repo, &mut f.data);
    let Some(path) = gitlab_path(repo) else { return f };
    if needs_project_id(&f.data) {
        let id = match http {
            Some(h) => project_id(h, dir, path).await,
            None => project_id_on_disk(dir, path),
        };
        if let Some(id) = id {
            put_project_id(&mut f.data, id);
        }
    }
    gitlab_assets(repo, &mut f.data);
    f
}

fn with_gitlab_assets(dir: &Path, repo: &str, mut f: Fetched) -> Fetched {
    f.archived = f.archived.or_else(|| github_archived_on_disk(dir, repo));
    forgejo_assets(repo, &mut f.data);
    if let Some(id) = gitlab_path(repo).filter(|_| needs_project_id(&f.data)).and_then(|p| project_id_on_disk(dir, p)) {
        put_project_id(&mut f.data, id);
    }
    gitlab_assets(repo, &mut f.data);
    f
}

/// A GitLab project's files come only from that project on gitlab.com: under its path (release
/// links, package files, job artifacts), or under its number (files uploaded to it, which GitLab
/// serves at /-/project/<number>/uploads/). GitHub's are as before.
pub fn own_download(repo: &str, url: &str, project_id: Option<u64>) -> bool {
    // A Forgejo project's files: over https from the server the project is on, and nowhere else
    // (where that server then sends the download is its own choice, as with SourceForge's mirrors).
    if let Some((server, owner, name)) = forgejo_parts(repo) {
        let Ok(u) = reqwest::Url::parse(url) else { return false };
        let raw_ok = !url.contains("/../") && !url.contains("/./") && !url.to_ascii_lowercase().contains("%2f");
        let path = u.path().to_ascii_lowercase();
        let project = format!("/{}/{}/", owner.to_ascii_lowercase(), name.to_ascii_lowercase());
        let mine = path.starts_with(&project) || path.starts_with("/attachments/");
        return raw_ok && u.scheme() == "https" && u.host_str() == Some(server) && u.port().is_none() && u.username().is_empty() && u.password().is_none() && mine;
    }
    // A SourceForge project's files: https://sourceforge.net/projects/<name>/files/... only (the
    // mirror it redirects to after that is SourceForge's choice).
    if let Some(n) = sourceforge_name(repo) {
        let Ok(u) = reqwest::Url::parse(url) else { return false };
        let raw_ok = !url.contains("/../") && !url.contains("/./") && !url.to_ascii_lowercase().contains("%2f");
        return raw_ok && u.scheme() == "https" && u.host_str() == Some("sourceforge.net") && u.port().is_none() && u.username().is_empty() && u.password().is_none() && u.path().starts_with(&format!("/projects/{n}/files/"));
    }
    let Some(p) = gitlab_path(repo) else { return true };
    let Ok(u) = reqwest::Url::parse(url) else { return false };
    let path = u.path().to_ascii_lowercase();
    let mine = path.starts_with(&format!("/{}/", p.to_ascii_lowercase())) || project_id.map(|n| path.starts_with(&format!("/-/project/{n}/uploads/"))).unwrap_or(false);
    u.scheme() == "https" && u.host_str() == Some("gitlab.com") && u.port().is_none() && u.username().is_empty() && u.password().is_none() && mine
}

/// Catalog entries for GitLab projects ("host": "gitlab") get their repo the launcher's way
/// (gitlab:group/project), so everything after tells the two apart by the repo alone.
/// Entries only newer launchers can use come in a list of their own, which older launchers never
/// read: `"apps_min": [{"min": "0.90.0-beta.27", "apps": [...]}]`. A group this launcher is new
/// enough for joins the catalog; a newer one is left out. An id the catalog already lists stays
/// the catalog's.
pub fn take_newer(cat: &mut Value, version: &str) {
    let Some(o) = cat.as_object_mut() else { return };
    let Some(groups) = o.remove("apps_min") else { return };
    let mine = crate::boot::ver_key(version);
    let mut add: Vec<Value> = vec![];
    for g in groups.as_array().into_iter().flatten() {
        let Some(min) = g.get("min").and_then(Value::as_str) else { continue };
        if crate::boot::ver_key(min) > mine {
            continue;
        }
        add.extend(g.get("apps").and_then(Value::as_array).into_iter().flatten().filter(|a| a.get("id").and_then(Value::as_str).is_some()).cloned());
    }
    if add.is_empty() {
        return;
    }
    if !o.get("apps").map(Value::is_array).unwrap_or(false) {
        o.insert("apps".into(), Value::Array(vec![]));
    }
    let Some(apps) = o.get_mut("apps").and_then(Value::as_array_mut) else { return };
    for a in add {
        let id = a.get("id").and_then(Value::as_str).unwrap_or("");
        if !apps.iter().any(|x| x.get("id").and_then(Value::as_str) == Some(id)) {
            apps.push(a);
        }
    }
}

pub fn mark_hosts(cat: &mut Value) {
    let Some(apps) = cat.get_mut("apps").and_then(Value::as_array_mut) else { return };
    for a in apps {
        let host = a.get("host").and_then(Value::as_str).map(|h| h.trim().to_ascii_lowercase()).unwrap_or_default();
        let lab = host == "gitlab";
        let Some(r) = a.get("repo").and_then(Value::as_str).map(str::to_string) else { continue };
        if host == "sourceforge" && !is_sourceforge(&r) {
            // The project's short name, or a link to it (any page of the project).
            let repo = repo_from_link(&r).filter(|x| is_sourceforge(x)).unwrap_or_else(|| format!("sourceforge:{}", r.trim().trim_end_matches('/').to_ascii_lowercase()));
            a["repo"] = Value::String(repo);
            continue;
        }
        if host == "forgejo" && !is_forgejo(&r) {
            // "<server>/<owner>/<name>", or a link to the project. The server's name in lowercase,
            // as it's compared with every download's host.
            let r = r.trim().trim_start_matches("https://").trim_start_matches("http://").trim_end_matches('/');
            let r = r.strip_suffix(".git").unwrap_or(r);
            let (server, rest) = r.split_once('/').unwrap_or((r, ""));
            a["repo"] = Value::String(format!("forgejo:{}/{rest}", server.to_ascii_lowercase()));
            continue;
        }
        if lab && !is_gitlab(&r) {
            let r = r.trim().trim_start_matches("https://").trim_start_matches("gitlab.com/").trim_end_matches('/');
            let r = r.strip_suffix(".git").unwrap_or(r);
            a["repo"] = Value::String(format!("gitlab:{r}"));
        }
    }
}

fn github_status_file(dir: &Path, repo: &str) -> PathBuf {
    dir.join("cache/github-repositories").join(format!("{}.json", repo.to_ascii_lowercase().replace('/', "__")))
}

fn github_archived_on_disk(dir: &Path, repo: &str) -> Option<bool> {
    if !repo_ok(repo) || site_only(repo) {
        return None;
    }
    load(&github_status_file(dir, repo))?.get("archived")?.as_bool()
}

fn with_known_project_status(dir: &Path, mut cat: Value) -> Value {
    for a in cat.get_mut("apps").and_then(Value::as_array_mut).into_iter().flatten() {
        // Keep fallback provenance: a later release response can supersede this cached flag,
        // while an explicit catalog boolean remains authoritative.
        if a.get("archived").and_then(Value::as_bool).is_none() {
            if let Some(archived) = a.get("repo").and_then(Value::as_str).and_then(|repo| github_archived_on_disk(dir, repo)) {
                a["github_archived"] = Value::Bool(archived);
            }
        }
    }
    cat
}

async fn github_archived(http: &reqwest::Client, dir: &Path, repo: &str, api: &str) -> Option<bool> {
    let read = async {
        let r = http.get(format!("{api}/repos/{repo}")).header("Accept", "application/vnd.github+json").timeout(crate::net::QUICK).send().await.ok()?;
        if !r.status().is_success() {
            return None;
        }
        let v: Value = r.json().await.ok()?;
        if !v.get("full_name").and_then(Value::as_str).map(|name| name.eq_ignore_ascii_case(repo)).unwrap_or(false) {
            return None;
        }
        let archived = v.get("archived")?.as_bool()?;
        save(&github_status_file(dir, repo), &serde_json::json!({"archived": archived}));
        Some(archived)
    };
    read.await.or_else(|| github_archived_on_disk(dir, repo))
}

/// Release info straight from GitHub, for when the site can't be reached: GitHub's own list (what
/// gh.php passes on), so installing and updating still work while the site is down. Downloads
/// always come from GitHub anyway.
pub(crate) async fn from_github(http: &reqwest::Client, dir: &Path, repo: &str) -> Result<Fetched, Error> {
    from_github_at(http, dir, repo, "https://api.github.com").await
}

async fn from_github_at(http: &reqwest::Client, dir: &Path, repo: &str, api: &str) -> Result<Fetched, Error> {
    // A GitLab or SourceForge project's releases only come through the site.
    if site_only(repo) {
        return Err(Error::Net("The site couldn't be reached".into()));
    }
    let url = format!("{api}/repos/{repo}/releases?per_page=30");
    // Ask alongside releases so status never adds a second network timeout. A missing/rate-
    // limited metadata answer must not stop the player installing the available release.
    let (release, archived) = tokio::join!(
        crate::net::send(http.get(&url).header("Accept", "application/vnd.github+json").timeout(crate::net::QUICK), &url, true),
        github_archived(http, dir, repo, api)
    );
    let r = release?;
    if !r.status().is_success() {
        return Err(Error::Net(format!("GitHub answered {}", r.status().as_u16())));
    }
    let v: Value = r.json().await?;
    if !v.is_array() {
        return Err(Error::Net("GitHub sent something unexpected".into()));
    }
    let file = releases_file(dir, repo);
    save(&file, &v);
    // Not the site's copy: its tag no longer goes with it.
    let _ = std::fs::remove_file(tag_file(&file));
    // Said once each time the launcher runs, so it doesn't crowd a report's newest lines (and the
    // site doesn't take it for the reason a game closed).
    static SAID: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !SAID.swap(true, std::sync::atomic::Ordering::Relaxed) {
        crate::trail::note(&format!("note: release info comes from GitHub this time (the site was slow or busy), first for {repo}"));
    }
    Ok(Fetched { archived, data: v, offline: false, age: 0 })
}

/// The site's answer, or GitHub's when the site couldn't give one (`fresh`: an old copy on disk
/// isn't good enough, as for an install).
async fn or_github(http: &reqwest::Client, dir: &Path, repo: &str, got: Result<Fetched, Error>, fresh: bool) -> Result<Fetched, Error> {
    match got {
        Ok(f) if !(f.offline && fresh) => Ok(f),
        Ok(f) => Ok(from_github(http, dir, repo).await.unwrap_or(f)),
        // Neither could answer: the copy on disk, however old, rather than nothing (its files are
        // usually still there).
        Err(e) => match from_github(http, dir, repo).await {
            Ok(f) => Ok(f),
            Err(_) => on_disk(&releases_file(dir, repo), Value::is_array).map(|h| Fetched { offline: true, ..h }).ok_or(e),
        },
    }
}

/// A recomp's releases. Used from disk while younger than six hours. `force`: the player asked
/// for it now (installing, updating, adding a repo).
pub async fn releases(http: &reqwest::Client, dir: &Path, repo: &str, force: bool) -> Result<Fetched, Error> {
    if !repo_ok(repo) {
        return Err(Error::Msg("That isn't a GitHub repo".into()));
    }
    let got = cached(http, &releases_url(repo), releases_file(dir, repo), if force { 0 } else { RELEASES_FRESH }, crate::net::QUICK, force, Value::is_array).await;
    let f = or_github(http, dir, repo, got, force).await?;
    Ok(with_gitlab(Some(http), dir, repo, f).await)
}

/// For installing or updating: the player's doing, but a copy from the last `secs` seconds (an
/// update that was just found) is used rather than asking again.
pub async fn releases_within(http: &reqwest::Client, dir: &Path, repo: &str, secs: u64) -> Result<Fetched, Error> {
    if !repo_ok(repo) {
        return Err(Error::Msg("That isn't a GitHub repo".into()));
    }
    let got = cached(http, &releases_url(repo), releases_file(dir, repo), secs, crate::net::QUICK, true, Value::is_array).await;
    let f = or_github(http, dir, repo, got, true).await?;
    Ok(with_gitlab(Some(http), dir, repo, f).await)
}

// ---------- a Forgejo project's own recomplauncher.json ----------

/// A Forgejo project's recomplauncher.json, read from its repo (the catalog can't carry it for
/// these): the same file a developer would put in their release, used the same way until one is.
fn repo_profile_url(origin: &str, owner: &str, name: &str) -> String {
    format!("{origin}/api/v1/repos/{owner}/{name}/raw/recomplauncher.json")
}

fn repo_profile_file(dir: &Path, repo: &str) -> PathBuf {
    dir.join("cache/repo-profiles").join(format!("{}.json", repo.replace(':', "_").replace('/', "__")))
}

/// A file worth keeping: a format 1 profile. A project without one is kept as `{"none": true}`,
/// so it isn't asked for again until the copy is old.
fn profile_ok(v: &Value) -> bool {
    v.get("format").and_then(Value::as_u64) == Some(1)
}

/// The repo's file as kept on disk, if it is one.
pub fn repo_profile_on_disk(dir: &Path, repo: &str) -> Option<Value> {
    load(&repo_profile_file(dir, repo)).filter(profile_ok)
}

/// Asked again only when the copy on disk is older than the release info's six hours, or missing
/// (`force`: asked now, as for an install). True when what's kept changed.
pub async fn repo_profile(http: &reqwest::Client, dir: &Path, repo: &str, force: bool) -> bool {
    let Some((server, owner, name)) = forgejo_parts(repo).filter(|_| repo_ok(repo)) else { return false };
    let file = repo_profile_file(dir, repo);
    if !force && age_of(&file).is_some_and(|a| a < RELEASES_FRESH) {
        return false;
    }
    let url = repo_profile_url(&format!("https://{server}"), owner, name);
    let Some(got) = fetch_profile(http, &url).await else { return false };
    if load(&file).as_ref() == Some(&got) {
        touch(&file);
        return false;
    }
    save(&file, &got);
    true
}

/// The file at `url`: the profile, `{"none": true}` when the project has none (404), or None when
/// the server couldn't be asked (the copy on disk stays). No token is sent.
async fn fetch_profile(http: &reqwest::Client, url: &str) -> Option<Value> {
    let r = crate::net::send(http.get(url).timeout(crate::net::QUICK), url, false).await.ok()?;
    if r.status() == reqwest::StatusCode::NOT_FOUND {
        return Some(serde_json::json!({ "none": true }));
    }
    if !r.status().is_success() {
        return None;
    }
    let v: Value = r.json().await.ok()?;
    Some(if profile_ok(&v) { v } else { serde_json::json!({ "none": true }) })
}

/// Forgejo entries without the catalog's own "launcher" get the one from their repo, kept on disk,
/// marked as the project's ("launcher_from": "repo").
fn with_repo_profiles(dir: &Path, mut cat: Value) -> Value {
    for a in cat.get_mut("apps").and_then(Value::as_array_mut).into_iter().flatten() {
        if a.get("launcher").is_some() {
            continue;
        }
        let Some(repo) = a.get("repo").and_then(Value::as_str).filter(|r| is_forgejo(r)).map(str::to_string) else { continue };
        if let Some(p) = repo_profile_on_disk(dir, &repo) {
            a["launcher"] = p;
            a["launcher_from"] = Value::String("repo".into());
        }
    }
    cat
}

/// The Forgejo projects in the list whose file isn't on disk yet or is old (`force`: all of them).
pub fn repo_profiles_due(dir: &Path, cat: &Value, force: bool) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for a in cat.get("apps").and_then(Value::as_array).into_iter().flatten() {
        let from_repo = a.get("launcher_from").and_then(Value::as_str) == Some("repo");
        if a.get("launcher").is_some() && !from_repo {
            continue;
        }
        let Some(repo) = a.get("repo").and_then(Value::as_str).filter(|r| is_forgejo(r) && repo_ok(r)) else { continue };
        let fresh = age_of(&repo_profile_file(dir, repo)).is_some_and(|x| x < RELEASES_FRESH);
        if (force || !fresh) && !out.iter().any(|r| r == repo) {
            out.push(repo.to_string());
        }
    }
    out
}

/// A recomp's releases as kept on disk (any age), without asking anyone.
pub fn releases_on_disk(dir: &Path, repo: &str) -> Option<Fetched> {
    if !repo_ok(repo) {
        return None;
    }
    on_disk(&releases_file(dir, repo), Value::is_array).map(|f| with_gitlab_assets(dir, repo, f))
}

/// Asked again in the background (not the player's doing): a push about a new release.
pub async fn releases_refresh(http: &reqwest::Client, dir: &Path, repo: &str) -> Result<Fetched, Error> {
    if !repo_ok(repo) {
        return Err(Error::Msg("That isn't a GitHub repo".into()));
    }
    let f = cached(http, &releases_url(repo), releases_file(dir, repo), 0, crate::net::QUICK, false, Value::is_array).await?;
    Ok(with_gitlab(Some(http), dir, repo, f).await)
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
    fn gitlab_projects() {
        assert_eq!(super::repo_from_link("https://gitlab.com/a/b").as_deref(), Some("gitlab:a/b"));
        assert_eq!(super::repo_from_link("https://gitlab.com/a/b/c.git").as_deref(), Some("gitlab:a/b/c"));
        assert_eq!(super::repo_from_link("gitlab.com/a/b/-/releases").as_deref(), Some("gitlab:a/b"));
        assert_eq!(super::repo_from_link("https://gitlab.com/a/b/-/releases/v1?x=1").as_deref(), Some("gitlab:a/b"));
        assert_eq!(super::repo_from_link("https://gitlab.com/a"), None);
        assert_eq!(super::repo_from_link("https://gitlab.com/a/../b"), None);
        // Releases through the site's gl.php, cached apart from GitHub's.
        assert!(super::releases_url("gitlab:a/b/c").ends_with("/api/gl.php?repo=a%2Fb%2Fc"));
        assert!(super::releases_url("a/b").ends_with("/api/gh.php?repo=a/b"));
        assert!(super::releases_file(std::path::Path::new("d"), "gitlab:a/b").ends_with("gitlab_a__b.json"));
        // Downloads only from the project itself.
        assert!(super::own_download("gitlab:a/b", "https://gitlab.com/a/b/-/releases/v1/downloads/game.zip", None));
        assert!(super::own_download("gitlab:a/b", "https://gitlab.com/a/b/-/package_files/12/download", None));
        assert!(super::own_download("gitlab:a/b", "https://gitlab.com/a/b/uploads/abc/game.zip", None));
        assert!(!super::own_download("gitlab:a/b", "https://gitlab.com/a/other/-/releases/v1/downloads/game.zip", None));
        assert!(!super::own_download("gitlab:a/b", "https://gitlab.com/a/bb/-/releases/v1/downloads/game.zip", None));
        assert!(!super::own_download("gitlab:a/b", "https://gitlab.com/a/b/../c/game.zip", None));
        assert!(!super::own_download("gitlab:a/b", "https://evil.example/a/b/game.zip", None));
        assert!(!super::own_download("gitlab:a/b", "http://gitlab.com/a/b/game.zip", None));
        // GitHub as before.
        assert!(super::own_download("a/b", "https://github.com/a/b/releases/download/v1/x.zip", None));
        // SourceForge: only the project's own files on sourceforge.net, over https.
        assert!(super::own_download("sourceforge:cogbox", "https://sourceforge.net/projects/cogbox/files/0.4.8/COGBox.7z/download", None));
        assert!(!super::own_download("sourceforge:cogbox", "https://sourceforge.net/projects/athelas/files/1.0/A.7z/download", None));
        assert!(!super::own_download("sourceforge:cogbox", "https://sourceforge.net/projects/cogboxx/files/1.0/A.7z/download", None));
        assert!(!super::own_download("sourceforge:cogbox", "http://sourceforge.net/projects/cogbox/files/0.4.8/COGBox.7z/download", None));
        assert!(!super::own_download("sourceforge:cogbox", "https://sourceforge.net:8443/projects/cogbox/files/x.7z/download", None));
        assert!(!super::own_download("sourceforge:cogbox", "https://user@sourceforge.net/projects/cogbox/files/x.7z/download", None));
        assert!(!super::own_download("sourceforge:cogbox", "https://evil.example/projects/cogbox/files/x.7z", None));
        assert!(!super::own_download("sourceforge:cogbox", "https://sourceforge.net/projects/cogbox/files/../../athelas/files/x.7z", None));
        assert!(!super::own_download("sourceforge:cogbox", "https://cogbox.dl.sourceforge.net/project/cogbox/x.7z", None));
        assert_eq!(super::repo_from_link("https://sourceforge.net/projects/cogbox/").as_deref(), Some("sourceforge:cogbox"));
        assert_eq!(super::repo_from_link("sourceforge.net/projects/cogbox/files/0.4.8/").as_deref(), Some("sourceforge:cogbox"));
        assert_eq!(super::repo_from_link("https://sourceforge.net/p/athelas/wiki/Home/").as_deref(), Some("sourceforge:athelas"));
        assert_eq!(super::repo_from_link("https://sourceforge.net/projects/x/"), None);
        assert!(super::releases_url("sourceforge:cogbox").ends_with("/api/sf.php?repo=cogbox"));
        assert!(super::releases_file(std::path::Path::new("d"), "sourceforge:cogbox").ends_with("sourceforge_cogbox.json"));
        assert!(super::repo_ok("sourceforge:co-gbox2") && !super::repo_ok("sourceforge:CogBox") && !super::repo_ok("sourceforge:a/b") && !super::repo_ok("sourceforge:a"));
        let mut cat = serde_json::json!({"apps": [{"id": "cogbox", "host": "sourceforge", "repo": "cogbox"}, {"id": "y", "host": "SourceForge", "repo": "https://sourceforge.net/projects/athelas/"}, {"id": "z", "host": "sourceforge", "repo": "http://www.sourceforge.net/projects/athelas/files/"}, {"id": "w", "host": "sourceforge", "repo": "https://sourceforge.net/p/cogbox/wiki/"}]});
        super::mark_hosts(&mut cat);
        assert_eq!(cat["apps"][0]["repo"], "sourceforge:cogbox");
        assert_eq!(cat["apps"][1]["repo"], "sourceforge:athelas");
        assert_eq!(cat["apps"][2]["repo"], "sourceforge:athelas");
        assert_eq!(cat["apps"][3]["repo"], "sourceforge:cogbox");
        // Entries for newer launchers join only when this one is new enough.
        let mut cat = serde_json::json!({"apps": [{"id": "a"}], "apps_min": [{"min": "0.90.0-beta.27", "apps": [{"id": "cogbox", "host": "sourceforge", "repo": "cogbox"}, {"id": "a", "name": "dup"}]}, {"min": "0.90.0-beta.30", "apps": [{"id": "later"}]}, {"min": 5, "apps": [{"id": "odd"}]}, "junk"]});
        super::take_newer(&mut cat, "0.90.0-beta.27");
        let ids: Vec<&str> = cat["apps"].as_array().unwrap().iter().map(|a| a["id"].as_str().unwrap()).collect();
        assert_eq!(ids, vec!["a", "cogbox"]);
        assert!(cat.get("apps_min").is_none() && cat["apps"][0].get("name").is_none());
        let mut old = serde_json::json!({"apps": [], "apps_min": [{"min": "0.90.0-beta.27", "apps": [{"id": "cogbox"}]}]});
        super::take_newer(&mut old, "0.90.0-beta.26");
        assert!(old["apps"].as_array().unwrap().is_empty());
        let mut rel = serde_json::json!({"apps_min": [{"min": "0.90.0-beta.27", "apps": [{"id": "cogbox"}]}]});
        super::take_newer(&mut rel, "0.90.0");
        assert_eq!(rel["apps"][0]["id"], "cogbox");
        // A catalog entry with "host": "gitlab" gets the launcher's kind of repo; GitHub's don't change.
        let mut cat = serde_json::json!({"apps": [{"id": "x", "host": "gitlab", "repo": "g/s/p"}, {"id": "y", "repo": "o/r"}]});
        super::mark_hosts(&mut cat);
        assert_eq!(cat["apps"][0]["repo"], "gitlab:g/s/p");
        assert_eq!(cat["apps"][1]["repo"], "o/r");
        super::mark_hosts(&mut cat);
        assert_eq!(cat["apps"][0]["repo"], "gitlab:g/s/p");
    }

    #[test]
    fn gitlab_downloads_in_the_release_notes() {
        let body = "**Downloads**\n\n- Windows:\n\n [Power Moon Recomp-0.2.1-windows.zip](/uploads/c2943d662cc803b10ca028c02907c8c5/Power_Moon_Recomp-0.2.1-windows.zip)\n- Linux:\n\n [Power Moon Recomp-0.2.1-linux-x86_64.AppImage](/uploads/de58d1e7804bf4126901bf9dcbd42b78/Power_Moon_Recomp-0.2.1-linux-x86_64.AppImage)\n![shot](/uploads/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/shot.png)\n[other](https://gitlab.com/someone/else/uploads/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/x-windows.zip)\n\nSHA-256\n\n```\nb14f6b7acdfa06915f383a9edaabd041e336e8a89f5e1a367615a3fae4837f44 Power Moon Recomp-0.2.1-windows.zip\n```";
        let mut v = serde_json::json!([{"tag_name": "v0.2.1", "body": body, "assets": [], "project_id": 88379810}, {"tag_name": "v0.2.0", "body": body, "project_id": 88379810, "assets": [{"name": "kept.zip", "size": 1, "browser_download_url": "https://gitlab.com/g/p/-/releases/v0.2.0/downloads/kept.zip"}]}]);
        super::gitlab_assets("gitlab:g/p", &mut v);
        let a = v[0]["assets"].as_array().unwrap();
        assert_eq!(a.len(), 2);
        assert_eq!(a[0]["name"], "Power_Moon_Recomp-0.2.1-windows.zip");
        assert_eq!(a[0]["browser_download_url"], "https://gitlab.com/-/project/88379810/uploads/c2943d662cc803b10ca028c02907c8c5/Power_Moon_Recomp-0.2.1-windows.zip");
        assert_eq!(a[0]["digest"], "sha256:b14f6b7acdfa06915f383a9edaabd041e336e8a89f5e1a367615a3fae4837f44");
        assert_eq!(a[1]["name"], "Power_Moon_Recomp-0.2.1-linux-x86_64.AppImage");
        assert!(a[1]["digest"].is_null());
        assert!(a.iter().all(|x| super::own_download("gitlab:g/p", x["browser_download_url"].as_str().unwrap(), Some(88379810))));
        // Only this project's number.
        assert!(!super::own_download("gitlab:g/p", "https://gitlab.com/-/project/1/uploads/c2943d662cc803b10ca028c02907c8c5/x.zip", Some(88379810)));
        assert!(!super::own_download("gitlab:g/p", "https://gitlab.com/-/project/88379810/uploads/c2943d662cc803b10ca028c02907c8c5/x.zip", None));
        // A release with its own assets keeps them.
        assert_eq!(v[1]["assets"].as_array().unwrap().len(), 1);
        // The picker finds the Windows build.
        let rels: Vec<super::Release> = serde_json::from_value(v).unwrap();
        assert_eq!(crate::assets::pick(&rels[0].assets, "windows", "x64", None).map(|a| a.name.as_str()), Some("Power_Moon_Recomp-0.2.1-windows.zip"));
        assert_eq!(super::percent_decode("Game%20One%2B.zip"), "Game One+.zip");
        // The number kept on disk fills a list the site sent without one.
        let d = std::env::temp_dir().join(format!("rl-glid-{}", std::process::id()));
        let _ = std::fs::create_dir_all(d.join("cache"));
        super::save(&super::project_ids_file(&d), &serde_json::json!({"g/p": 88379810}));
        let f = super::Fetched { archived: None, data: serde_json::json!([{"tag_name": "v1", "body": body, "assets": []}]), offline: false, age: 0 };
        let f = super::with_gitlab_assets(&d, "gitlab:g/p", f);
        assert_eq!(f.data[0]["project_id"], 88379810);
        assert_eq!(f.data[0]["assets"].as_array().unwrap().len(), 2);
        let _ = std::fs::remove_dir_all(&d);
        // Without the project's number there's no address that works: left as it is.
        let mut n = serde_json::json!([{"tag_name": "v1", "body": body, "assets": []}]);
        super::gitlab_assets("gitlab:g/p", &mut n);
        assert!(n[0]["assets"].as_array().unwrap().is_empty());
        assert_eq!(super::percent_decode("50%"), "50%");
        // GitHub's lists aren't touched.
        let mut g = serde_json::json!([{"tag_name": "v1", "body": body, "assets": []}]);
        super::gitlab_assets("o/r", &mut g);
        assert!(g[0]["assets"].as_array().unwrap().is_empty());
    }

    #[test]
    fn github_links() {
        assert_eq!(super::repo_from_link("https://github.com/danielgomesvieira2000/burnout3-pc").as_deref(), Some("danielgomesvieira2000/burnout3-pc"));
        assert_eq!(super::repo_from_link("github.com/a/b.git").as_deref(), Some("a/b"));
        assert_eq!(super::repo_from_link("https://github.com/a/b/releases/tag/v1").as_deref(), Some("a/b"));
        assert_eq!(super::repo_from_link("a/b").as_deref(), Some("a/b"));
        assert_eq!(super::repo_from_link("https://gitlab.com/a/b").as_deref(), Some("gitlab:a/b"));
        assert_eq!(super::repo_from_link("https://codeberg.org/a/b"), None);
        assert_eq!(super::name_from_repo("x/burnout3-pc"), "Burnout3 PC");
        assert_eq!(super::name_from_repo("x/gt3_revisited"), "Gt3 Revisited");
    }

    use super::*;

    #[test]
    fn a_forgejo_entry_gets_its_repo_the_launchers_way() {
        let mut cat = serde_json::json!({"apps": [
            {"id": "a", "host": "forgejo", "repo": "Git.Example.dev/Studio/Hopper"},
            {"id": "b", "host": "Forgejo", "repo": "https://git.example.dev/Studio/Hopper.git/"},
            {"id": "c", "host": "forgejo", "repo": "forgejo:git.example.dev/Studio/Hopper"},
        ]});
        mark_hosts(&mut cat);
        for i in 0..3 {
            assert_eq!(cat["apps"][i]["repo"], "forgejo:git.example.dev/Studio/Hopper");
        }
    }

    #[test]
    fn a_forgejo_repo_names_one_project_on_a_public_server() {
        assert!(repo_ok("forgejo:git.example.dev/Studio/Hopper"));
        assert!(repo_ok("forgejo:code.my-site.org/a-b/c.d_e"));
        assert!(!repo_ok("forgejo:192.168.1.20/Studio/Hopper"));
        assert!(!repo_ok("forgejo:localhost/Studio/Hopper"));
        assert!(!repo_ok("forgejo:git.example.dev:3000/Studio/Hopper"));
        assert!(!repo_ok("forgejo:Git.Example.dev/Studio/Hopper"));
        assert!(!repo_ok("forgejo:git.example.dev/Studio"));
        assert!(!repo_ok("forgejo:git.example.dev/Studio/Hopper/extra"));
        assert!(!repo_ok("forgejo:git.example.dev/../Hopper"));
        assert!(!repo_ok("forgejo:user@git.example.dev/Studio/Hopper"));
        assert!(!repo_ok("forgejo:-git.example.dev/Studio/Hopper"));
    }

    #[test]
    fn a_forgejo_projects_releases_come_from_its_own_server() {
        let repo = "forgejo:git.example.dev/Studio/Hopper";
        assert_eq!(releases_url(repo), "https://git.example.dev/api/v1/repos/Studio/Hopper/releases?limit=30");
        assert_eq!(forgejo_parts(repo), Some(("git.example.dev", "Studio", "Hopper")));
        assert_eq!(host_path(repo), "Studio/Hopper");
        assert!(site_only(repo) && is_forgejo(repo) && !is_gitlab(repo) && !is_sourceforge(repo));
        assert!(releases_file(Path::new("d"), repo).ends_with("forgejo_git.example.dev__Studio__Hopper.json"));
        // GitHub's, GitLab's and SourceForge's stay as they were.
        assert!(releases_url("owner/game").ends_with("/api/gh.php?repo=owner/game"));
        assert!(releases_url("gitlab:group/game").ends_with("/api/gl.php?repo=group%2Fgame"));
        assert!(!site_only("owner/game") && !is_forgejo("owner/game") && !is_forgejo("gitlab:group/game"));
    }

    #[test]
    fn a_forgejo_projects_files_come_only_from_its_server() {
        let repo = "forgejo:git.example.dev/Studio/Hopper";
        let ok = |u: &str| own_download(repo, u, None);
        assert!(ok("https://git.example.dev/Studio/Hopper/releases/download/nightly-2/Hopper-x86_64.AppImage"));
        assert!(ok("https://git.example.dev/studio/hopper/releases/download/nightly-2/Hopper-x86_64.AppImage"));
        assert!(ok("https://git.example.dev/attachments/0b5c2c3e-1d2f-4a51-9d7e-2d6b1c1f0a11"));
        assert!(!ok("https://github.com/Studio/Hopper/releases/download/nightly-2/Hopper-x86_64.AppImage"));
        assert!(!ok("https://other.example.dev/Studio/Hopper/releases/download/nightly-2/x.zip"));
        assert!(!ok("https://git.example.dev.evil.example/Studio/Hopper/releases/download/nightly-2/x.zip"));
        assert!(!ok("http://git.example.dev/Studio/Hopper/releases/download/nightly-2/x.zip"));
        assert!(!ok("https://git.example.dev:8443/Studio/Hopper/releases/download/nightly-2/x.zip"));
        assert!(!ok("https://user@git.example.dev/Studio/Hopper/releases/download/nightly-2/x.zip"));
        assert!(!ok("https://git.example.dev/Other/Project/releases/download/v1/x.zip"));
        assert!(!ok("https://git.example.dev/Studio/Hopper/../../Other/Project/x.zip"));
    }

    /// Two nightlies and a release candidate, as a Forgejo server lists them.
    const FORGEJO_RELEASES: &str = r#"[
        {"id": 3, "tag_name": "nightly-2-bbb", "name": "Hopper nightly-2-bbb", "body": "Nightly build", "draft": false, "prerelease": true,
         "published_at": "2026-11-13T17:13:35Z", "html_url": "https://git.example.dev/Studio/Hopper/releases/tag/nightly-2-bbb",
         "url": "https://git.example.dev/api/v1/repos/Studio/Hopper/releases/3",
         "assets": [
            {"id": 31, "name": "Hopper-nightly-2-bbb-x86_64.AppImage", "size": 98662904, "download_count": 426, "uuid": "u31", "type": "attachment",
             "browser_download_url": "https://git.example.dev/Studio/Hopper/releases/download/nightly-2-bbb/Hopper-nightly-2-bbb-x86_64.AppImage"},
            {"id": 32, "name": "HopperBuilder-nightly-2-bbb-windows-x64.zip", "size": 42018777, "download_count": 1217, "uuid": "u32", "type": "attachment",
             "browser_download_url": "https://git.example.dev/Studio/Hopper/releases/download/nightly-2-bbb/HopperBuilder-nightly-2-bbb-windows-x64.zip",
             "url": "https://api.github.com/repos/Studio/Hopper/releases/assets/32"},
            {"id": 33, "name": "SHA256SUMS", "size": 300, "download_count": 4, "uuid": "u33", "type": "attachment",
             "browser_download_url": "https://git.example.dev/Studio/Hopper/releases/download/nightly-2-bbb/SHA256SUMS"}
         ]},
        {"id": 2, "tag_name": "nightly-1-aaa", "name": "Hopper nightly-1-aaa", "body": "", "draft": false, "prerelease": true,
         "published_at": "2026-11-12T10:00:00Z",
         "assets": [
            {"id": 21, "name": "HopperBuilder-nightly-1-aaa-windows-x64.zip", "size": 42000000, "download_count": 9, "uuid": "u21", "type": "attachment",
             "browser_download_url": "https://git.example.dev/Studio/Hopper/releases/download/nightly-1-aaa/HopperBuilder-nightly-1-aaa-windows-x64.zip"}
         ]},
        {"id": 1, "tag_name": "v3.0.0-rc.1", "name": "Hopper v3.0.0-rc.1", "body": "", "draft": false, "prerelease": true,
         "published_at": "2026-11-01T10:00:00Z", "assets": []}
    ]"#;

    #[test]
    fn a_forgejo_release_list_reads_like_githubs() {
        let repo = "forgejo:git.example.dev/Studio/Hopper";
        let mut v: Value = serde_json::from_str(FORGEJO_RELEASES).unwrap();
        forgejo_assets(repo, &mut v);
        let rels: Vec<Release> = serde_json::from_value(v).unwrap();
        assert_eq!(rels.len(), 3);
        assert_eq!(rels[0].assets.len(), 3);
        assert_eq!(rels[0].assets[1].size, 42018777);
        // Only pre-releases: the newest of them, as for a GitHub project with no full release.
        assert_eq!(latest(&rels).unwrap().tag_name, "nightly-2-bbb");
        assert_eq!(latest_for(&rels, "nightly-1-aaa").unwrap().tag_name, "nightly-2-bbb");
        let win = crate::assets::pick(&rels[0].assets, "windows", "x64", Some("HopperBuilder-*-windows-x64.zip")).unwrap();
        assert_eq!(win.name, "HopperBuilder-nightly-2-bbb-windows-x64.zip");
        let linux = crate::assets::pick(&rels[0].assets, "linux", "x64", Some("Hopper-*-x86_64.AppImage")).unwrap();
        assert_eq!(linux.name, "Hopper-nightly-2-bbb-x86_64.AppImage");
        assert!(crate::assets::pick(&rels[0].assets, "macos", "arm64", None).is_none());
        assert!(own_download(repo, &win.browser_download_url, None) && own_download(repo, &linux.browser_download_url, None));
    }

    #[test]
    fn a_forgejo_file_has_no_second_address_to_download_from() {
        let mut v: Value = serde_json::from_str(FORGEJO_RELEASES).unwrap();
        forgejo_assets("forgejo:git.example.dev/Studio/Hopper", &mut v);
        let rels: Vec<Release> = serde_json::from_value(v).unwrap();
        assert!(rels.iter().flat_map(|r| &r.assets).all(|a| a.url.is_none()));
        // A GitHub list keeps its API addresses (the downloader's second way in).
        let mut gh: Value = serde_json::from_str(FORGEJO_RELEASES).unwrap();
        forgejo_assets("owner/game", &mut gh);
        assert!(gh[0]["assets"][1]["url"].is_string());
    }

    #[test]
    fn a_forgejo_entry_waits_for_the_launcher_version_the_catalog_names() {
        let entry = serde_json::json!({"id": "hopper", "host": "forgejo", "repo": "git.example.dev/Studio/Hopper"});
        let mut cat = serde_json::json!({"apps": [], "apps_min": [{"min": "99.0.0", "apps": [entry]}]});
        take_newer(&mut cat, "0.90.0-beta.43");
        assert!(cat["apps"].as_array().unwrap().is_empty());
        let mut lowered = serde_json::json!({"apps": [], "apps_min": [{"min": "0.90.0-beta.43", "apps": [entry]}]});
        take_newer(&mut lowered, "0.90.0-beta.43");
        mark_hosts(&mut lowered);
        assert_eq!(lowered["apps"][0]["repo"], "forgejo:git.example.dev/Studio/Hopper");
    }

    /// A one-answer server on this PC: the request it got, and the answer it gave.
    fn answer_once(status: &str, body: &'static str) -> (String, std::thread::JoinHandle<String>) {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", l.local_addr().unwrap());
        let status = status.to_string();
        let seen = std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = c.read(&mut buf).unwrap_or(0);
            let resp = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            let _ = c.write_all(resp.as_bytes());
            String::from_utf8_lossy(&buf[..n]).to_lowercase()
        });
        (origin, seen)
    }

    #[test]
    fn a_link_to_another_server_is_read_as_host_owner_and_name() {
        let got = |s: &str| forgejo_from_link(s).map(|(h, o, n)| format!("{h} {o} {n}"));
        assert_eq!(got("https://Git.Example.dev/Studio/Hopper").as_deref(), Some("git.example.dev Studio Hopper"));
        assert_eq!(got("https://git.example.dev/Studio/Hopper/releases").as_deref(), Some("git.example.dev Studio Hopper"));
        assert_eq!(got("https://git.example.dev/Studio/Hopper.git/").as_deref(), Some("git.example.dev Studio Hopper"));
        assert_eq!(got("http://git.example.dev/Studio/Hopper?tab=readme").as_deref(), Some("git.example.dev Studio Hopper"));
        // Not one: no project in it, GitHub's or GitLab's, a machine on the local network, no scheme.
        assert_eq!(got("https://www.example.com"), None);
        assert_eq!(got("https://example.com/news"), None);
        assert_eq!(got("https://github.com/Studio/Hopper"), None);
        assert_eq!(got("https://gitlab.com/Studio/Hopper"), None);
        assert_eq!(got("https://192.168.1.20/Studio/Hopper"), None);
        assert_eq!(got("https://localhost/Studio/Hopper"), None);
        assert_eq!(got("git.example.dev/Studio/Hopper"), None);
    }

    #[test]
    fn every_kind_of_project_has_its_own_page() {
        assert_eq!(repo_home("forgejo:git.example.dev/Studio/Hopper"), "https://git.example.dev/Studio/Hopper");
        assert_eq!(repo_home("gitlab:group/sub/game"), "https://gitlab.com/group/sub/game");
        assert_eq!(repo_home("sourceforge:gamebox"), "https://sourceforge.net/projects/gamebox");
        assert_eq!(repo_home("owner/game"), "https://github.com/owner/game");
    }

    #[test]
    fn a_server_that_answered_is_known_for_the_rest_of_the_session() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        FORGEJO_SERVERS.lock().unwrap().push("code.known-test.dev".into());
        // Answered from memory: nothing is asked (there's no such server).
        assert!(rt.block_on(forgejo_server(&reqwest::Client::new(), "code.known-test.dev")));
        // Never a machine on the player's network, known or not.
        FORGEJO_SERVERS.lock().unwrap().push("10.0.0.5".into());
        assert!(!rt.block_on(forgejo_server(&reqwest::Client::new(), "10.0.0.5")));
    }

    #[test]
    fn a_forgejo_server_says_its_version() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let http = reqwest::Client::new();
        let (origin, seen) = answer_once("200 OK", r#"{"version":"11.0.1+gitea-1.22.0"}"#);
        assert!(rt.block_on(forgejo_version(&http, &format!("{origin}/api/v1/version"))));
        let req = seen.join().unwrap();
        assert!(!req.contains("authorization") && !req.contains("cookie"), "{req}");
        let (origin, seen) = answer_once("200 OK", r#"<html>A shop</html>"#);
        assert!(!rt.block_on(forgejo_version(&http, &format!("{origin}/api/v1/version"))));
        seen.join().unwrap();
        let (origin, seen) = answer_once("404 Not Found", r#"{"message":"not found"}"#);
        assert!(!rt.block_on(forgejo_version(&http, &format!("{origin}/api/v1/version"))));
        seen.join().unwrap();
    }

    #[test]
    fn a_forgejo_projects_setup_file_is_read_from_its_repo() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let http = reqwest::Client::new();
        let (origin, seen) = answer_once("200 OK", r#"{"format":1,"game":{"kind":"iso_or_folder","label":"Your copy"},"start":{"program":["Hopper.AppImage"]}}"#);
        let url = repo_profile_url(&origin, "Studio", "Hopper");
        let got = rt.block_on(fetch_profile(&http, &url)).unwrap();
        let req = seen.join().unwrap();
        assert!(req.starts_with("get /api/v1/repos/studio/hopper/raw/recomplauncher.json http/1.1"), "{req}");
        assert!(!req.contains("authorization") && !req.contains("cookie"), "{req}");
        assert_eq!(got["format"], 1);
        // None in the project: kept as such, so it isn't asked again until the copy is old.
        let (origin, seen) = answer_once("404 Not Found", r#"{"message":"not found"}"#);
        let none = rt.block_on(fetch_profile(&http, &repo_profile_url(&origin, "Studio", "Hopper"))).unwrap();
        seen.join().unwrap();
        assert_eq!(none, serde_json::json!({"none": true}));
        assert!(!profile_ok(&none));
    }

    #[test]
    fn a_forgejo_entry_gets_its_projects_setup_file_from_disk() {
        let dir = std::env::temp_dir().join(format!("rl-repo-profile-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let repo = "forgejo:git.example.dev/Studio/Hopper";
        save(&repo_profile_file(&dir, repo), &serde_json::json!({"format": 1, "game": {"kind": "iso_or_folder"}}));
        let cat = serde_json::json!({"apps": [
            {"id": "hopper", "host": "forgejo", "repo": "git.example.dev/Studio/Hopper"},
            {"id": "own", "host": "forgejo", "repo": "git.example.dev/Studio/Hopper", "launcher": {"format": 1}},
            {"id": "gh", "repo": "owner/game"},
        ]});
        let cat = with_mine(&dir, cat);
        assert_eq!(cat["apps"][0]["launcher"]["game"]["kind"], "iso_or_folder");
        assert_eq!(cat["apps"][0]["launcher_from"], "repo");
        // The catalog's own file stays the catalog's; a GitHub entry gets nothing.
        assert!(cat["apps"][1].get("launcher_from").is_none());
        assert!(cat["apps"][2].get("launcher").is_none());
        // Just read: not due again (unless the player refreshes).
        assert!(repo_profiles_due(&dir, &cat, false).is_empty());
        assert_eq!(repo_profiles_due(&dir, &cat, true), vec![repo.to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_forgejo_release_list_is_asked_for_with_no_token() {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", l.local_addr().unwrap());
        let seen = std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = c.read(&mut buf).unwrap_or(0);
            let body = FORGEJO_RELEASES;
            let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            let _ = c.write_all(resp.as_bytes());
            String::from_utf8_lossy(&buf[..n]).to_lowercase()
        });
        let dir = std::env::temp_dir().join(format!("rl-forgejo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let url = forgejo_releases_url(&origin, "Studio", "Hopper");
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let f = rt.block_on(cached(&reqwest::Client::new(), &url, dir.join("r.json"), 0, Duration::from_secs(5), true, Value::is_array)).unwrap();
        let req = seen.join().unwrap();
        assert!(req.starts_with("get /api/v1/repos/studio/hopper/releases?limit=30 http/1.1"), "{req}");
        assert!(!req.contains("authorization") && !req.contains("token") && !req.contains("cookie"), "{req}");
        assert_eq!(f.data[0]["tag_name"], "nightly-2-bbb");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn repos() {
        assert!(repo_ok("Zelda64Recomp/Zelda64Recomp"));
        assert!(repo_ok("a-b/c.d_e"));
        assert!(!repo_ok("a/b/c"));
        assert!(!repo_ok("../etc"));
        assert!(!repo_ok("a/.."));
        assert!(!repo_ok("a/b?x=1"));
    }

    /// A tiny site: the list with an ETag, and a 304 when asked with that ETag.
    fn tiny_site() -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = format!("http://{}", l.local_addr().unwrap());
        let full = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let f2 = full.clone();
        std::thread::spawn(move || {
            for c in l.incoming().flatten().take(4) {
                let mut c = c;
                let mut buf = [0u8; 4096];
                let n = c.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_lowercase();
                let resp = if req.contains("if-none-match: w/\"v1\"") {
                    "HTTP/1.1 304 Not Modified\r\nETag: W/\"v1\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
                } else {
                    f2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let body = r#"{"apps":[{"id":"a"}]}"#;
                    format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nETag: W/\"v1\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
                };
                let _ = c.write_all(resp.as_bytes());
            }
        });
        (addr, full)
    }

    #[test]
    fn unchanged_list_comes_back_as_304() {
        let (site, full) = tiny_site();
        let dir = std::env::temp_dir().join(format!("rl-etag-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("list.json");
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let http = reqwest::Client::new();
        let url = format!("{site}/pc-catalog.json");
        let first = rt.block_on(cached(&http, &url, file.clone(), 0, Duration::from_secs(5), true, list_ok)).unwrap();
        assert_eq!(first.data["apps"][0]["id"], "a");
        assert!(tag_file(&file).is_file());
        // Asked again: only if it changed. It didn't, so the copy on disk is used and is fresh.
        let again = rt.block_on(cached(&http, &url, file.clone(), 0, Duration::from_secs(5), true, list_ok)).unwrap();
        assert_eq!(again.data["apps"][0]["id"], "a");
        assert!(!again.offline);
        assert_eq!(full.load(std::sync::atomic::Ordering::SeqCst), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn deck_scores_and_team_notes() {
        let scores = serde_json::json!({ "ok": true, "pc": {
            "a": { "score": 90, "n": 10, "sys": { "windows": { "score": 92, "n": 8 }, "deck": { "score": 85, "n": 3 } } },
            "b": { "score": 60, "n": 4, "sys": { "windows": { "score": 60, "n": 4 } } } } });
        let d = deck_scores(&scores);
        assert_eq!(d["a"], serde_json::json!({ "score": 85.0, "n": 3 }));
        assert!(d.get("b").is_none());
        let notes = notes_from(&serde_json::json!({ "ok": true, "notes": [
            { "id": 7, "body": "We just added Discord, join us", "link": "https://discord.gg/x", "linkText": "Join", "tone": "violet" },
            { "id": "n2", "body": "Maintenance tonight", "link": "http://example.com", "tone": "red" },
            { "id": "n3", "body": "" } ] }));
        assert_eq!(notes.len(), 2);
        assert_eq!((notes[0]["id"].as_str(), notes[0]["link"].as_str(), notes[0]["tone"].as_str()), (Some("7"), Some("https://discord.gg/x"), Some("violet")));
        // Only https links; an unknown color is cyan.
        assert_eq!((notes[1]["link"].as_str(), notes[1]["linkText"].as_str(), notes[1]["tone"].as_str()), (Some(""), Some(""), Some("cyan")));
        assert!(notes_from(&serde_json::json!({ "ok": true, "notes": [] })).is_empty());
        assert!(notes_from(&serde_json::json!({ "ok": false })).is_empty());
    }

    #[test]
    fn newest_stable_first() {
        let r = |t: &str, pre: bool, draft: bool| Release { tag_name: t.into(), name: None, published_at: None, prerelease: pre, draft, body: None, html_url: None, assets: vec![], project_id: None };
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

#[cfg(test)]
mod project_status_tests {
    use super::*;
    use std::io::{Read, Write};

    // Real HTTP responses, without contacting GitHub or the launcher service.
    fn server(metadata: &'static str, status: u16) -> (String, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let handle = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            let mut requests = 0;
            while requests < 2 && std::time::Instant::now() < deadline {
                let Ok((mut stream, _)) = listener.accept() else { std::thread::sleep(Duration::from_millis(2)); continue };
                stream.set_nonblocking(false).unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
                let mut bytes = [0; 4096];
                let n = stream.read(&mut bytes).unwrap();
                let request = String::from_utf8_lossy(&bytes[..n]);
                let (status, body) = if request.starts_with("GET /repos/owner/game/releases?") {
                    (200, r#"[{"tag_name":"v1","assets":[]}]"#)
                } else {
                    assert!(request.starts_with("GET /repos/owner/game HTTP/"), "{request}");
                    (status, metadata)
                };
                write!(stream, "HTTP/1.1 {status} Response\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                requests += 1;
            }
        });
        (url, handle)
    }

    fn dir() -> PathBuf {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!("recomp-project-status-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn fallback_keeps_archived_status_with_releases_and_catalog() {
        let dir = dir();
        let (api, server) = server(r#"{"full_name":"OWNER/Game","archived":true}"#, 200);
        let f = from_github_at(&reqwest::Client::new(), &dir, "owner/game", &api).await.unwrap();
        server.join().unwrap();
        assert_eq!(f.data[0]["tag_name"], "v1");
        assert_eq!(serde_json::to_value(f).unwrap()["archived"], true);
        let cached = releases_on_disk(&dir, "owner/game").unwrap();
        assert_eq!(serde_json::to_value(cached).unwrap()["archived"], true);
        let cat = with_mine(&dir, serde_json::json!({"apps":[{"id":"game","repo":"owner/game"},{"id":"other","repo":"owner/other"}]}));
        assert_eq!(cat["apps"][0]["github_archived"], true);
        assert!(cat["apps"][0].get("archived").is_none());
        assert!(cat["apps"][1].get("archived").is_none());
        let explicit = with_mine(&dir, serde_json::json!({"apps":[{"id":"game","repo":"owner/game","archived":false}]}));
        assert_eq!(explicit["apps"][0]["archived"], false);
        save_mine(&dir, &[serde_json::json!({"id":"mine","repo":"Owner/Game"})]).unwrap();
        let mine = with_mine(&dir, serde_json::json!({"apps":[]}));
        assert_eq!(mine["apps"][0]["github_archived"], true);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn optional_metadata_failures_preserve_releases_and_known_status() {
        let dir = dir();
        let http = reqwest::Client::new();
        for (body, status, want) in [
            (r#"{"message":"rate limited"}"#, 403, Value::Null),
            (r#"{"full_name":"owner/game","archived":true}"#, 200, serde_json::json!(true)),
            (r#"{"message":"rate limited"}"#, 403, serde_json::json!(true)),
            (r#"{"full_name":"other/game","archived":false}"#, 200, serde_json::json!(true)),
            (r#"{"full_name":"owner/game","archived":"false"}"#, 200, serde_json::json!(true)),
            (r#"{"full_name":"owner/game","archived":false}"#, 200, serde_json::json!(false)),
        ] {
            let (api, server) = server(body, status);
            let f = from_github_at(&http, &dir, "owner/game", &api).await.unwrap();
            server.join().unwrap();
            assert_eq!(f.data[0]["tag_name"], "v1");
            assert_eq!(serde_json::to_value(f).unwrap()["archived"], want);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}

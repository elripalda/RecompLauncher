//! Where the launcher's server is, and how gently it's asked.
//!
//! Every request to the RecompLauncher server goes through `send`. When a site answers with
//! trouble (no answer, a timeout, 403, 408, 429 or a 5xx), the launcher stops asking that site for
//! a while: 30 seconds at first, doubling up to an hour, with a random part so launchers don't all
//! come back at the same moment. A Retry-After from the site is respected. While a site is resting,
//! the launcher works from what it has on disk, and the window shows a small "Offline" mark.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/// The RecompLauncher server: the list, release info, accounts, cloud saves, logs and stats. Move
/// the API to its own address (api.recomplauncher.com, say) by changing this one line.
pub const API_BASE: &str = "https://recomplauncher.com";

/// The website people open in their browser (download page, links). Not used for requests.
pub const SITE: &str = "https://recomplauncher.com";

/// The server to ask: `API_BASE`, or RL_API for testing against a local copy of the site.
pub fn base() -> String {
    std::env::var("RL_API").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| API_BASE.into()).trim_end_matches('/').to_string()
}

/// How long a quick question to the server may take.
pub const QUICK: Duration = Duration::from_secs(10);
/// A bigger answer (the whole list).
pub const LONGER: Duration = Duration::from_secs(20);
/// A save going up or coming down, or a log being sent.
pub const TRANSFER: Duration = Duration::from_secs(60);

const FIRST_WAIT: u64 = 30;
const MOST_WAIT: u64 = 3600;
/// A request the player asked for themselves still waits this long after the last try.
const PLAYER_GAP: Duration = Duration::from_secs(30);

#[derive(Default)]
struct Gate {
    fails: u32,
    until: Option<Instant>,
    last_try: Option<Instant>,
    /// When the last failure was counted: requests already on their way then don't count again.
    last_fail: Option<Instant>,
}

static GATES: Mutex<Option<HashMap<String, Gate>>> = Mutex::new(None);
static OFFLINE: AtomicBool = AtomicBool::new(false);
static APP: OnceLock<AppHandle> = OnceLock::new();
static SALT: AtomicU64 = AtomicU64::new(0);

pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// Is the server resting (it had trouble and isn't being asked right now)?
pub fn offline() -> bool {
    OFFLINE.load(Ordering::Relaxed)
}

/// A random number from 0 up to (not including) 1.
pub fn random() -> f64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    h.write_u64(SALT.fetch_add(1, Ordering::Relaxed));
    (h.finish() >> 11) as f64 / (1u64 << 53) as f64
}

/// `secs`, stretched by up to `extra` (0.2 = up to 20% more) at random, so timers on different PCs
/// drift apart.
pub fn spread(secs: u64, extra: f64) -> Duration {
    Duration::from_secs_f64(secs as f64 * (1.0 + extra * random()))
}

fn host(url: &str) -> String {
    reqwest::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_ascii_lowercase)).unwrap_or_default()
}

/// What a failure puts to rest: the whole site, except release info (gh.php passes on GitHub's own
/// trouble with one repo, which says nothing about the site), which rests on its own.
fn key(url: &str) -> String {
    let h = host(url);
    let path = reqwest::Url::parse(url).map(|u| u.path().to_string()).unwrap_or_default();
    if path.ends_with("/api/gh.php") {
        format!("{h}/gh")
    } else {
        h
    }
}

fn is_api(k: &str) -> bool {
    k == host(&base())
}

/// How long to wait after the `fails`-th failure in a row: a random time between half the step and
/// the whole of it (never under 30 seconds, never over an hour; the first is 30 to 45 seconds).
/// Steps: 30 s, 60 s, 2 min ... 1 h.
fn wait_after(fails: u32, r: f64) -> u64 {
    let step = FIRST_WAIT.saturating_mul(1u64 << fails.saturating_sub(1).min(20)).min(MOST_WAIT);
    let lo = (step / 2).max(FIRST_WAIT);
    let hi = step.max(lo + 15).min(MOST_WAIT);
    lo + ((hi - lo) as f64 * r).round() as u64
}

/// Retry-After, in seconds or as a date. Capped at six hours.
fn retry_after(r: &reqwest::Response) -> Option<u64> {
    let v = r.headers().get(reqwest::header::RETRY_AFTER)?.to_str().ok()?.trim().to_string();
    let secs = match v.parse::<u64>() {
        Ok(s) => s,
        Err(_) => {
            let when = chrono::DateTime::parse_from_rfc2822(&v).ok()?;
            (when.timestamp() - chrono::Utc::now().timestamp()).max(0) as u64
        }
    };
    Some(secs.min(6 * 3600))
}

/// Statuses that mean "the site is having trouble, leave it alone for a bit".
pub fn trouble_status(code: u16) -> bool {
    matches!(code, 403 | 408 | 429) || (500..600).contains(&code)
}

fn set_offline(v: bool) {
    if OFFLINE.swap(v, Ordering::Relaxed) != v {
        if let Some(app) = APP.get() {
            let _ = app.emit("net", serde_json::json!({ "offline": v }));
        }
    }
}

fn gate_rests(g: &HashMap<String, Gate>, k: &str, player: bool) -> bool {
    let Some(gate) = g.get(k) else { return false };
    match gate.until {
        Some(u) if Instant::now() < u => !(player && gate.last_try.map(|t| t.elapsed() >= PLAYER_GAP).unwrap_or(true)),
        _ => false,
    }
}

/// Should a request to this address be skipped right now? `player`: the player asked for it
/// (a button, opening a page), which goes through once 30 seconds have passed since the last try.
pub fn resting(url: &str, player: bool) -> bool {
    let (h, k) = (host(url), key(url));
    let mut g = GATES.lock().unwrap();
    let g = g.get_or_insert_with(HashMap::new);
    gate_rests(g, &h, player) || (k != h && gate_rests(g, &k, player))
}

fn tried(url: &str) {
    let mut g = GATES.lock().unwrap();
    g.get_or_insert_with(HashMap::new).entry(key(url)).or_default().last_try = Some(Instant::now());
}

fn went_well(url: &str) {
    let (h, k) = (host(url), key(url));
    {
        let mut g = GATES.lock().unwrap();
        let g = g.get_or_insert_with(HashMap::new);
        // An answer means the site is up; release info's own rest ends only with an answer about it.
        for x in [&h, &k] {
            if let Some(gate) = g.get_mut(x) {
                gate.fails = 0;
                gate.until = None;
            }
        }
    }
    if is_api(&h) {
        set_offline(false);
    }
}

/// `started`: when the failed request was sent. Requests that were already on their way when an
/// earlier failure was counted don't count again (an outage is one failure, not ten).
fn went_badly(url: &str, after: Option<u64>, started: Instant) {
    let k = key(url);
    let secs = {
        let mut g = GATES.lock().unwrap();
        let gate = g.get_or_insert_with(HashMap::new).entry(k.clone()).or_default();
        let counted = gate.last_fail.map(|t| started < t).unwrap_or(false);
        if !counted {
            gate.fails = gate.fails.saturating_add(1);
            gate.last_fail = Some(Instant::now());
        }
        let mut secs = wait_after(gate.fails.max(1), random());
        if let Some(a) = after {
            // The site said when: then, plus a little so everyone doesn't come back together.
            secs = secs.max(a + (a as f64 * 0.25 * random()).min(300.0) as u64);
        }
        let until = Instant::now() + Duration::from_secs(secs);
        // Never shortened by a later failure of the same outage.
        if counted && gate.until.map(|u| u > until).unwrap_or(false) {
            return;
        }
        gate.until = Some(until);
        secs
    };
    crate::trail::note(&format!("{k} had trouble; asking again in {secs}s"));
    if is_api(&k) {
        set_offline(true);
    }
}

/// An answer came, but not one that makes sense (a hosting page instead of the list, say): treated
/// like the site having trouble.
pub fn bad_answer(url: &str) {
    went_badly(url, None, Instant::now());
}

/// Seconds until this address can be asked again (0 when it can be now).
pub fn wait_left(url: &str) -> u64 {
    let g = GATES.lock().unwrap();
    let left = |k: &str| g.as_ref().and_then(|m| m.get(k)).and_then(|gate| gate.until).map(|u| u.saturating_duration_since(Instant::now()).as_secs()).unwrap_or(0);
    left(&host(url)).max(left(&key(url)))
}

/// The one way to ask the server. A site that's resting isn't asked at all; trouble makes it rest.
/// The answer comes back whatever its status, for the caller to read.
pub async fn send(rb: reqwest::RequestBuilder, url: &str, player: bool) -> Result<reqwest::Response, crate::Error> {
    if resting(url, player) {
        return Err(crate::Error::Net(format!("Can't reach {} right now. Using what's saved on this PC.", host(url))));
    }
    tried(url);
    let started = Instant::now();
    match rb.send().await {
        Ok(r) => {
            let code = r.status().as_u16();
            if trouble_status(code) {
                went_badly(url, if matches!(code, 429 | 503) { retry_after(&r) } else { None }, started);
            } else {
                went_well(url);
            }
            Ok(r)
        }
        Err(e) => {
            went_badly(url, None, started);
            Err(e.into())
        }
    }
}

/// Pictures: not asked while the site rests, and only the site not answering at all (or saying
/// it's busy) makes it rest; one missing or refused picture doesn't.
pub async fn send_picture(rb: reqwest::RequestBuilder, url: &str) -> Option<reqwest::Response> {
    if resting(url, false) {
        return None;
    }
    let started = Instant::now();
    match rb.send().await {
        Ok(r) => {
            let code = r.status().as_u16();
            if matches!(code, 429 | 503) {
                went_badly(url, retry_after(&r), started);
            }
            Some(r)
        }
        Err(e) => {
            if e.is_connect() || e.is_timeout() {
                went_badly(url, None, started);
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_grow_and_stay_spread() {
        // First: 30 to 45 s. Then up to double each time, never over an hour.
        assert_eq!(wait_after(1, 0.0), 30);
        assert_eq!(wait_after(1, 1.0), 45);
        assert_eq!(wait_after(2, 0.0), 30);
        assert_eq!(wait_after(2, 1.0), 60);
        assert_eq!(wait_after(5, 1.0), 480);
        assert_eq!(wait_after(5, 0.0), 240);
        assert_eq!(wait_after(8, 1.0), 3600);
        assert_eq!(wait_after(40, 1.0), 3600);
        // At the top, launchers are still spread over half an hour.
        assert_eq!(wait_after(40, 0.0), 1800);
    }

    #[test]
    fn one_outage_counts_once() {
        let u = "https://outage-test.example/a";
        let before = Instant::now();
        went_badly(u, None, before);
        // Others that were already on their way when it failed.
        went_badly(u, None, before);
        went_badly(u, None, before);
        let fails = GATES.lock().unwrap().as_ref().unwrap().get("outage-test.example").unwrap().fails;
        assert_eq!(fails, 1);
        assert!(wait_left(u) <= 45);
        // A new try after that, failing too, counts.
        went_badly(u, None, Instant::now() + Duration::from_millis(5));
        let fails = GATES.lock().unwrap().as_ref().unwrap().get("outage-test.example").unwrap().fails;
        assert_eq!(fails, 2);
    }

    #[test]
    fn release_info_rests_on_its_own() {
        let gh = "https://keys-test.example/api/gh.php?repo=a/b";
        let list = "https://keys-test.example/pc-catalog.json";
        went_badly(gh, None, Instant::now());
        assert!(resting(gh, false));
        assert!(!resting(list, false));
        went_badly(list, None, Instant::now());
        assert!(resting("https://keys-test.example/api/gh.php?repo=c/d", false));
        // The site answering again doesn't end release info's own rest (GitHub may still be out).
        went_well(list);
        assert!(!resting(list, false));
        assert!(resting(gh, false));
        went_well(gh);
        assert!(!resting(gh, false));
    }

    #[test]
    fn trouble_is_trouble() {
        for c in [403, 408, 429, 500, 502, 503, 504] {
            assert!(trouble_status(c), "{c}");
        }
        for c in [200, 304, 400, 401, 404] {
            assert!(!trouble_status(c), "{c}");
        }
    }

    #[test]
    fn random_is_in_range() {
        for _ in 0..1000 {
            let r = random();
            assert!((0.0..1.0).contains(&r));
        }
        let a: Vec<f64> = (0..20).map(|_| random()).collect();
        assert!(a.windows(2).any(|w| w[0] != w[1]));
    }

    #[test]
    fn resting_sites_are_left_alone() {
        let u = "https://rest-test.example/api/x";
        assert!(!resting(u, false));
        went_badly(u, Some(120), Instant::now());
        assert!(resting(u, false));
        assert!(wait_left(u) >= 119);
        // A button press right after the failed try still waits.
        tried(u);
        assert!(resting(u, true));
        went_well(u);
        assert!(!resting(u, false));
    }
}

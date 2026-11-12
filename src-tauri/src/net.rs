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

/// Smaller copies of the site's pictures (made once on the server, cached by Cloudflare), as the
/// website's own thumb() helper asks for them: ?f=<file in /media>&w=<width>.
pub const THUMBS: &str = "https://recompstore.com/api/thumb.php";

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
/// The longest rest for release info, pictures, saves and usage.
const MOST_WAIT: u64 = 3600;
/// The longest the site itself rests (the list, the account, updates): five minutes, then it's
/// asked again (see probe), so an Offline mark never sticks for hours after the site is back.
const SITE_MOST_WAIT: u64 = 300;
/// A request the player asked for themselves still waits this long after the last try.
const PLAYER_GAP: Duration = Duration::from_secs(30);

#[derive(Default)]
struct Gate {
    fails: u32,
    until: Option<Instant>,
    last_try: Option<Instant>,
    /// When the last failure was counted: requests already on their way then don't count again.
    last_fail: Option<Instant>,
    /// A non-trouble answer cleared this cooldown. Older in-flight failures cannot reopen it.
    last_good: Option<Instant>,
}

static GATES: Mutex<Option<HashMap<String, Gate>>> = Mutex::new(None);
static OFFLINE: AtomicBool = AtomicBool::new(false);
/// The server answered, but with trouble (a 5xx or 429): it's busy, not this PC offline.
static BUSY: AtomicBool = AtomicBool::new(false);
static APP: OnceLock<AppHandle> = OnceLock::new();
static SALT: AtomicU64 = AtomicU64::new(0);

pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// Is the server resting (it had trouble and isn't being asked right now)?
pub fn offline() -> bool {
    OFFLINE.load(Ordering::Relaxed)
}

/// While resting: was it the server saying it's busy (it answered 5xx or 429), rather than no
/// answer at all? The window then says the website is busy and the launcher is retrying.
pub fn busy() -> bool {
    OFFLINE.load(Ordering::Relaxed) && BUSY.load(Ordering::Relaxed)
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
/// trouble with one repo, which says nothing about the site), which rests on its own, and pictures
/// (a page of covers asked for at once can meet the site's limit, which says nothing about the
/// rest of it: the list, updates and reports carry on).
///
/// Cloud saves going up or coming down rest on their own ("{host}/saves"): a big upload that's cut
/// off or too slow says nothing about the list or the account. So does usage and logs
/// ("{host}/stats"). Only the site's own gate marks it Offline.
fn key(url: &str) -> String {
    let h = host(url);
    let parsed = reqwest::Url::parse(url).ok();
    let path = parsed.as_ref().map(|u| u.path().to_string()).unwrap_or_default();
    let action = parsed.as_ref().and_then(|u| u.query_pairs().find(|(k, _)| k == "a").map(|(_, v)| v.to_string())).unwrap_or_default();
    if path.ends_with("/api/launcher.php") && matches!(action.as_str(), "save-put" | "save-get" | "save-delete") {
        format!("{h}/saves")
    } else if path.ends_with("/api/launcher-ping.php") || path.ends_with("/api/logs.php") || (path.ends_with("/api/launcher.php") && matches!(action.as_str(), "events" | "hello" | "report")) {
        format!("{h}/stats")
    } else if path.ends_with("/api/gh.php") {
        format!("{h}/gh")
    } else if path.ends_with("/api/gl.php") {
        format!("{h}/gl")
    } else if path.starts_with("/media/") || path.ends_with("/api/thumb.php") || url.starts_with(THUMBS) {
        format!("{h}/pictures")
    } else {
        h
    }
}

fn is_api(k: &str) -> bool {
    k == host(&base())
}

/// How long to wait after the `fails`-th failure in a row: a random time between half the step and
/// the whole of it (never under 30 seconds, never over `most`; the first is 30 to 45 seconds).
/// Steps: 30 s, 60 s, 2 min ... up to `most`.
fn wait_after(fails: u32, r: f64, most: u64) -> u64 {
    let step = FIRST_WAIT.saturating_mul(1u64 << fails.saturating_sub(1).min(20)).min(most);
    let lo = (step / 2).max(FIRST_WAIT).min(most);
    let hi = step.max(lo + 15).min(most);
    lo + ((hi.saturating_sub(lo)) as f64 * r).round() as u64
}

/// The longest a gate rests: five minutes for the site itself, an hour for the rest.
fn most_wait(k: &str) -> u64 {
    if is_api(k) { SITE_MOST_WAIT } else { MOST_WAIT }
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

/// Called once each time the site comes back after being Offline (lib.rs asks for the list again).
static ON_BACK: OnceLock<fn(&AppHandle)> = OnceLock::new();

pub fn on_back(f: fn(&AppHandle)) {
    let _ = ON_BACK.set(f);
}

fn set_offline(v: bool, answered: bool) {
    let busy_changed = v && BUSY.swap(answered, Ordering::Relaxed) != answered;
    let was = OFFLINE.swap(v, Ordering::Relaxed);
    if was != v || busy_changed {
        if was != v {
            crate::trail::note(if v { "note: the site is resting (Offline)" } else { "note: Online again" });
        }
        if let Some(app) = APP.get() {
            let _ = app.emit("net", serde_json::json!({ "offline": v, "busy": v && answered }));
            if was && !v {
                if let Some(f) = ON_BACK.get() {
                    f(app);
                }
            }
        }
    }
}

/// While Offline: once the site's rest is over, one quick question to it (the list's headers,
/// nothing downloaded). An answer clears Offline by itself, with nothing for the player to do;
/// no answer rests it again (five minutes at most).
pub async fn probe(http: &reqwest::Client) -> bool {
    let url = format!("{}/pc-catalog.json", base());
    if !offline() || wait_left(&url) > 0 {
        return false;
    }
    matches!(send(http.head(&url).timeout(QUICK), &url, false).await, Ok(r) if !trouble_status(r.status().as_u16()))
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
            let gate = g.entry(x.clone()).or_default();
            gate.fails = 0;
            gate.until = None;
            gate.last_good = Some(Instant::now());
        }
        // Keep the indicator and its event ordered with the gate transition on other threads.
        if is_api(&h) {
            set_offline(false, false);
        }
    }
}

/// `started`: when the failed request was sent. Requests that were already on their way when an
/// earlier failure was counted don't count again (an outage is one failure, not ten).
fn went_badly(url: &str, after: Option<u64>, started: Instant) {
    went_badly_as(url, after, started, false)
}

/// `answered`: the site sent a trouble status (busy), rather than no answer at all.
fn went_badly_as(url: &str, after: Option<u64>, started: Instant, answered: bool) {
    let k = key(url);
    let secs = {
        let mut g = GATES.lock().unwrap();
        let gate = g.get_or_insert_with(HashMap::new).entry(k.clone()).or_default();
        // A successful answer after this request started already established that the site
        // recovered. Discard the old failure before it changes the cooldown or Offline mark.
        if gate.last_good.map(|t| started < t).unwrap_or(false) {
            return;
        }
        let counted = gate.last_fail.map(|t| started < t).unwrap_or(false);
        if !counted {
            gate.fails = gate.fails.saturating_add(1);
            gate.last_fail = Some(Instant::now());
        }
        let mut secs = wait_after(gate.fails.max(1), random(), most_wait(&k));
        if let Some(a) = after {
            // The site said when: then, plus a little so everyone doesn't come back together.
            // Still never past the gate's longest rest for the site itself.
            secs = secs.max(a + (a as f64 * 0.25 * random()).min(300.0) as u64);
            if is_api(&k) {
                secs = secs.min(SITE_MOST_WAIT);
            }
        }
        let until = Instant::now() + Duration::from_secs(secs);
        // Never shortened by a later failure of the same outage.
        if counted && gate.until.map(|u| u > until).unwrap_or(false) {
            return;
        }
        gate.until = Some(until);
        // One hiccup does not mean Offline. Publish this while the gate is still locked so a
        // successful recovery cannot be followed by an older failure's delayed indicator update.
        if is_api(&k) && gate.fails >= 2 {
            set_offline(true, answered);
        }
        secs
    };
    crate::trail::note(&format!("note: {k} was slow or busy; resting {secs}s"));
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

tokio::task_local! {
    /// Set for what the player asked for straight away (Upload Now, Check Again): the site is asked
    /// even while it rests.
    static NOW: bool;
}

/// Runs `f` with every request in it sent straight away, resting or not: only for a button the
/// player pressed to try again now.
pub async fn now<F: std::future::Future>(f: F) -> F::Output {
    NOW.scope(true, f).await
}

/// Inside `now()`?
pub fn forced() -> bool {
    NOW.try_with(|v| *v).unwrap_or(false)
}

/// The one way to ask the server. A site that's resting isn't asked at all; trouble makes it rest.
/// The answer comes back whatever its status, for the caller to read.
pub async fn send(rb: reqwest::RequestBuilder, url: &str, player: bool) -> Result<reqwest::Response, crate::Error> {
    if !forced() && resting(url, player) {
        if busy() && is_api(&key(url)) {
            return Err(crate::Error::Net(format!("{} is busy, retrying in {}s. Using what's saved on this PC.", host(url), wait_left(url).max(1))));
        }
        return Err(crate::Error::Net(format!("Can't reach {} right now. Using what's saved on this PC.", host(url))));
    }
    tried(url);
    let started = Instant::now();
    match rb.send().await {
        Ok(r) => {
            let code = r.status().as_u16();
            if trouble_status(code) {
                went_badly_as(url, if matches!(code, 429 | 503) { retry_after(&r) } else { None }, started, code == 429 || code >= 500);
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
/// A picture that didn't come, and why, said once each time the launcher runs: a report about
/// missing covers then says what stopped them (a refusal, a blocked connection, a page instead).
pub(crate) fn picture_trouble(url: &str, why: &str) {
    static SAID: AtomicBool = AtomicBool::new(false);
    if !SAID.swap(true, Ordering::Relaxed) {
        crate::trail::note(&format!("note: pictures from {} didn't load ({why})", host(url)));
    }
}

fn picture_went_well(url: &str) {
    let mut g = GATES.lock().unwrap();
    let gate = g.get_or_insert_with(HashMap::new).entry(key(url)).or_default();
    gate.fails = 0;
    gate.until = None;
    gate.last_good = Some(Instant::now());
}

pub async fn send_picture(rb: reqwest::RequestBuilder, url: &str) -> Option<reqwest::Response> {
    if resting(url, false) {
        return None;
    }
    let started = Instant::now();
    match rb.send().await {
        Ok(r) => {
            let code = r.status().as_u16();
            if !r.status().is_success() {
                picture_trouble(url, &format!("the site answered {code}"));
            }
            if matches!(code, 429 | 503) {
                went_badly(url, retry_after(&r), started);
            } else if r.status().is_success() && key(url) != host(url) {
                // Pictures come again: their own rest is over (the site's isn't touched).
                picture_went_well(url);
            }
            Some(r)
        }
        Err(e) => {
            // What went wrong underneath too (a refused connection, a TLS failure).
            let mut why = String::new();
            let mut src = std::error::Error::source(&e);
            while let Some(x) = src {
                why = x.to_string();
                src = x.source();
            }
            let kind = if e.is_timeout() { "it took too long" } else if e.is_connect() { "couldn't connect" } else { "the request failed" };
            picture_trouble(url, &if why.is_empty() { kind.to_string() } else { format!("{kind} ({})", why.chars().take(160).collect::<String>()) });
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
        let m = MOST_WAIT;
        assert_eq!(wait_after(1, 0.0, m), 30);
        assert_eq!(wait_after(1, 1.0, m), 45);
        assert_eq!(wait_after(2, 0.0, m), 30);
        assert_eq!(wait_after(2, 1.0, m), 60);
        assert_eq!(wait_after(5, 1.0, m), 480);
        assert_eq!(wait_after(5, 0.0, m), 240);
        assert_eq!(wait_after(8, 1.0, m), 3600);
        assert_eq!(wait_after(40, 1.0, m), 3600);
        // At the top, launchers are still spread over half an hour.
        assert_eq!(wait_after(40, 0.0, m), 1800);
        // The site itself never rests more than five minutes.
        assert_eq!(wait_after(40, 1.0, SITE_MOST_WAIT), 300);
        assert_eq!(wait_after(40, 0.0, SITE_MOST_WAIT), 150);
        assert_eq!(wait_after(1, 1.0, SITE_MOST_WAIT), 45);
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
    fn a_recovered_site_is_not_put_back_to_rest_by_an_old_request() {
        let url = "https://recovered-test.example/api/launcher.php";
        let old_request = Instant::now() - Duration::from_secs(1);
        went_badly(url, None, old_request);
        assert!(resting(url, false));
        went_well(url);
        assert!(!resting(url, false));
        // Another request from the same outage finishes late, after recovery was confirmed.
        went_badly(url, None, old_request);
        assert!(!resting(url, false), "a stale failure reopened the site's cooldown");
        assert_eq!(wait_left(url), 0);
        // Requests made after recovery still count when they fail.
        went_badly(url, None, Instant::now());
        assert!(resting(url, false));
    }

    #[test]
    fn the_first_good_answer_also_rejects_older_failures() {
        let url = "https://first-good-test.example/api/launcher.php";
        let old_request = Instant::now() - Duration::from_secs(1);
        went_well(url);
        went_badly(url, None, old_request);
        assert!(!resting(url, false));
    }

    #[test]
    fn picture_recovery_is_remembered_without_clearing_an_api_outage() {
        let picture = "https://picture-recovered-test.example/media/a.jpg";
        let api = "https://picture-recovered-test.example/api/launcher.php";
        let old_request = Instant::now() - Duration::from_secs(1);
        went_badly(api, None, old_request);
        picture_went_well(picture);
        assert!(resting(api, false), "a picture must not clear the site's API outage");
        went_well(api);
        went_badly(picture, None, old_request);
        assert!(!resting(picture, false), "a stale picture failure reopened its cooldown");
    }

    /// The tests that read or change the site's Offline mark, one at a time.
    static SITE: Mutex<()> = Mutex::new(());

    #[test]
    fn saves_and_usage_rest_on_their_own() {
        let b = base();
        for a in ["save-put", "save-get", "save-delete"] {
            assert_eq!(key(&format!("{b}/api/launcher.php?a={a}&game=x")), "recomplauncher.com/saves", "{a}");
        }
        for a in ["events", "hello", "report"] {
            assert_eq!(key(&format!("{b}/api/launcher.php?a={a}")), "recomplauncher.com/stats", "{a}");
        }
        assert_eq!(key(&format!("{b}/api/launcher-ping.php")), "recomplauncher.com/stats");
        assert_eq!(key(&format!("{b}/api/logs.php")), "recomplauncher.com/stats");
        // The account, the saves list and the list of games are the site itself.
        for u in [format!("{b}/api/launcher.php?a=me"), format!("{b}/api/launcher.php?a=saves&game=x"), format!("{b}/api/launcher.php"), format!("{b}/pc-catalog.json")] {
            assert_eq!(key(&u), "recomplauncher.com", "{u}");
        }
    }

    #[test]
    fn a_failed_save_never_takes_the_site_offline() {
        let _one = SITE.lock().unwrap_or_else(|e| e.into_inner());
        let (save, list) = (format!("{}/api/launcher.php?a=save-put&game=g", base()), format!("{}/pc-catalog.json", base()));
        went_well(&list);
        for _ in 0..4 {
            went_badly_as(&save, None, Instant::now(), true);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(!offline(), "a cut-off upload isn't the site being down");
        assert!(!resting(&list, false), "the list, account and updates carry on");
        assert!(resting(&save, false), "uploads wait their own time");
        went_well(&save);
        assert!(!resting(&save, false));
    }

    #[test]
    fn the_site_rests_five_minutes_at_most() {
        let _one = SITE.lock().unwrap_or_else(|e| e.into_inner());
        let list = format!("{}/pc-catalog.json", base());
        went_well(&list);
        for _ in 0..12 {
            went_badly(&list, Some(6 * 3600), Instant::now());
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(offline());
        assert!(wait_left(&list) <= SITE_MOST_WAIT, "rests {}s", wait_left(&list));
        went_well(&list);
        assert!(!offline());
    }

    #[test]
    fn offline_state_stays_ordered_with_concurrent_gate_transitions() {
        let _one = SITE.lock().unwrap_or_else(|e| e.into_inner());
        let url = format!("{}/api/launcher.php", base());
        let round = std::sync::Barrier::new(3);
        let mismatches = AtomicU64::new(0);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                for _ in 0..3000 {
                    round.wait();
                    went_badly(&url, None, Instant::now());
                    went_badly(&url, None, Instant::now());
                    round.wait();
                }
            });
            scope.spawn(|| {
                for _ in 0..3000 {
                    round.wait();
                    went_well(&url);
                    round.wait();
                }
            });
            for _ in 0..3000 {
                went_well(&url);
                round.wait();
                round.wait();
                let gates = GATES.lock().unwrap();
                let failed = gates.as_ref().unwrap().get(&key(&url)).unwrap().fails >= 2;
                if offline() != failed {
                    mismatches.fetch_add(1, Ordering::Relaxed);
                }
            }
        });
        went_well(&url);
        assert_eq!(mismatches.load(Ordering::Relaxed), 0, "Offline disagreed with the latest completed gate transition");
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
    fn pictures_rest_on_their_own() {
        let pic = "https://pics-test.example/media/cover.jpg";
        let api = "https://pics-test.example/api/pc.php";
        went_badly(pic, Some(60), Instant::now());
        assert!(resting(pic, false));
        // The site's list and the rest carry on.
        assert!(!resting(api, false));
        went_well(pic);
        assert!(!resting(pic, false));
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

// ---------- the proxy (Settings → Network) ----------

/// Off, the system's (HTTPS_PROXY / ALL_PROXY / HTTP_PROXY), or the player's own address. Every
/// request the launcher makes (the list, GitHub, downloads, the website) goes through it.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Proxy {
    Off,
    #[default]
    System,
    Custom(reqwest::Url),
}

static PROXY: std::sync::RwLock<Option<Proxy>> = std::sync::RwLock::new(None);

/// Applies the player's setting: "off", "system" (or empty) or "custom" with its address and
/// optional user and password (put into the address, which is how proxies are told).
pub fn set_proxy(mode: &str, url: &str, user: &str, pass: &str) -> Result<(), String> {
    let p = parse_proxy(mode, url, user, pass)?;
    *PROXY.write().unwrap() = Some(p);
    Ok(())
}

pub fn parse_proxy(mode: &str, url: &str, user: &str, pass: &str) -> Result<Proxy, String> {
    match mode {
        "off" => Ok(Proxy::Off),
        "" | "system" => Ok(Proxy::System),
        "custom" => {
            let url = url.trim();
            let with_scheme = if url.contains("://") { url.to_string() } else { format!("http://{url}") };
            let mut u = reqwest::Url::parse(&with_scheme).map_err(|_| "That isn't a proxy address. Write it like http://127.0.0.1:7890 or socks5://host:1080.".to_string())?;
            if !matches!(u.scheme(), "http" | "https" | "socks5" | "socks5h") || u.host_str().is_none_or(str::is_empty) {
                return Err("The proxy has to be an http, https or socks5 address, like http://127.0.0.1:7890.".into());
            }
            if !user.is_empty() {
                let _ = u.set_username(user);
                let _ = u.set_password(Some(pass).filter(|p| !p.is_empty()));
            }
            Ok(Proxy::Custom(u))
        }
        _ => Err("Pick Off, System or Custom.".into()),
    }
}

/// The system's proxy, from the usual variables (the https one for https addresses).
fn system_proxy(target: &reqwest::Url) -> Option<reqwest::Url> {
    let var = |k: &str| std::env::var(k).ok().or_else(|| std::env::var(k.to_ascii_lowercase()).ok()).filter(|v| !v.trim().is_empty());
    let no = var("NO_PROXY").unwrap_or_default();
    let host = target.host_str().unwrap_or("");
    if no.split(',').map(str::trim).any(|n| n == "*" || (!n.is_empty() && (host == n.trim_start_matches('.') || host.ends_with(&format!(".{}", n.trim_start_matches('.')))))) {
        return None;
    }
    let raw = if target.scheme() == "https" { var("HTTPS_PROXY").or_else(|| var("ALL_PROXY")) } else { var("HTTP_PROXY").or_else(|| var("ALL_PROXY")) }?;
    let raw = if raw.contains("://") { raw } else { format!("http://{raw}") };
    reqwest::Url::parse(&raw).ok()
}

/// Where a request to `target` goes through, if anywhere: what the client asks for each request.
pub fn proxy_for(target: &reqwest::Url) -> Option<reqwest::Url> {
    if matches!(target.host_str(), Some("127.0.0.1" | "localhost" | "::1")) {
        return None;
    }
    match PROXY.read().unwrap().clone().unwrap_or_default() {
        Proxy::Off => None,
        Proxy::System => system_proxy(target),
        Proxy::Custom(u) => Some(u),
    }
}

/// The proxy in use, without its password, for messages ("Couldn't reach the proxy 1.2.3.4:7890").
pub fn proxy_shown() -> Option<String> {
    let u = match PROXY.read().unwrap().clone().unwrap_or_default() {
        Proxy::Off => return None,
        Proxy::System => system_proxy(&reqwest::Url::parse("https://github.com").ok()?)?,
        Proxy::Custom(u) => u,
    };
    Some(format!("{}:{}", u.host_str()?, u.port_or_known_default().unwrap_or(0)))
}

/// Every client the launcher makes asks this for its proxy.
pub fn with_proxy(b: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
    b.no_proxy().proxy(reqwest::Proxy::custom(proxy_for))
}

#[cfg(test)]
mod proxy_tests {
    use super::*;

    #[test]
    fn the_proxy_setting_reads_and_rejects_clearly() {
        assert_eq!(parse_proxy("off", "", "", ""), Ok(Proxy::Off));
        assert_eq!(parse_proxy("", "", "", ""), Ok(Proxy::System));
        let Ok(Proxy::Custom(u)) = parse_proxy("custom", "127.0.0.1:7890", "me", "pw") else { panic!() };
        assert_eq!(u.as_str(), "http://me:pw@127.0.0.1:7890/");
        let Ok(Proxy::Custom(u)) = parse_proxy("custom", "socks5://10.0.0.2:1080", "", "") else { panic!() };
        assert_eq!(u.scheme(), "socks5");
        assert!(parse_proxy("custom", "ftp://x:21", "", "").unwrap_err().contains("http, https or socks5"));
        assert!(parse_proxy("custom", "", "", "").is_err());
        assert!(parse_proxy("custom", "http://", "", "").is_err());
    }

    #[test]
    fn a_dead_proxy_fails_fast_with_its_address() {
        // Nothing listens on this port: the request through it fails at once (not a hang), and
        // the error names the proxy.
        let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = dead.local_addr().unwrap().port();
        drop(dead);
        set_proxy("custom", &format!("http://127.0.0.1:{port}"), "", "").unwrap();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let c = with_proxy(reqwest::Client::builder()).connect_timeout(Duration::from_secs(5)).build().unwrap();
        let started = Instant::now();
        let e = rt.block_on(c.get("https://example.invalid/").send()).unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(6));
        let msg = crate::Error::from(e).to_string();
        assert!(msg.contains(&format!("127.0.0.1:{port}")), "{msg}");
        set_proxy("system", "", "", "").unwrap();
        // Off: never one, whatever the variables say.
        set_proxy("off", "", "", "").unwrap();
        assert_eq!(proxy_for(&reqwest::Url::parse("https://github.com").unwrap()), None);
        set_proxy("system", "", "", "").unwrap();
    }
}

//! Downloading a build, checking it's the file GitHub has, and unpacking it into its folder.
//!
//! Nothing here knows about the window: progress goes to a callback, so the tests drive it
//! with plain files.

use crate::assets::{kind, Asset};
use crate::Error;
use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub id: String,
    /// download, verify, unpack, done, error, cancelled
    pub phase: &'static str,
    pub done: u64,
    pub total: u64,
    /// Bytes a second, while downloading.
    pub speed: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub msg: Option<String>,
}

impl Progress {
    pub fn new(id: &str, phase: &'static str, done: u64, total: u64) -> Self {
        Progress { id: id.into(), phase, done, total, speed: 0, msg: None }
    }
}

/// What the download turned out to be.
pub struct Downloaded {
    pub file: PathBuf,
    pub sha256: String,
    /// GitHub gave a checksum and the file matches it.
    pub verified: bool,
}

/// A big file from a site that sends parts of files: downloaded in small chunks over several
/// connections at once. Many sites hold each connection to a set speed, or send the first few
/// megabytes of a connection fast and then slow it right down; a fresh request for each chunk, and
/// a connection that falls far behind the others dropped and asked again, keeps it fast to the end.
/// `None` when the site doesn't send parts or the file is small: then it's downloaded the usual way.
pub async fn download_split(
    http: &reqwest::Client,
    url: &str,
    name: &str,
    dir: &Path,
    id: &str,
    cancel: Arc<AtomicBool>,
    report: &(dyn Fn(Progress) + Send + Sync),
) -> Result<Option<Downloaded>, Error> {
    use std::collections::VecDeque;
    use std::sync::atomic::AtomicU64;
    use std::sync::Mutex;
    const WORKERS: usize = 8;
    const CHUNK: u64 = 8 << 20;
    // Does it send parts, and how big is it? One byte asked for says both. Where it ends up after
    // redirects is where every chunk is asked for, so each one doesn't go through them again.
    let probe = http.get(url).header(reqwest::header::RANGE, "bytes=0-0").timeout(Duration::from_secs(30)).send().await;
    let Ok(probe) = probe else { return Ok(None) };
    if probe.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Ok(None);
    }
    let total = probe
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit('/').next())
        .and_then(|n| n.trim().parse::<u64>().ok())
        .unwrap_or(0);
    let landed = probe.url().to_string();
    drop(probe);
    if total < 48 << 20 {
        return Ok(None);
    }
    fs::create_dir_all(dir)?;
    let safe: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || "-_.+".contains(c) { c } else { '_' }).collect();
    let file = dir.join(&safe);
    let part = dir.join(format!("{safe}.split.part"));
    {
        let f = std::fs::File::create(&part)?;
        f.set_len(total)?;
    }
    let done = Arc::new(AtomicU64::new(0));
    // The best speed one connection has kept up (bytes a second), for spotting a slowed one.
    let best = Arc::new(AtomicU64::new(0));
    let queue: Arc<Mutex<VecDeque<(u64, u64)>>> = Arc::new(Mutex::new((0..total.div_ceil(CHUNK)).map(|i| (i * CHUNK, ((i + 1) * CHUNK).min(total))).collect()));
    let mut tasks = vec![];
    for _ in 0..WORKERS {
        let (http, first, url, part, done, cancel, queue, best) = (http.clone(), landed.clone(), url.to_string(), part.clone(), done.clone(), cancel.clone(), queue.clone(), best.clone());
        tasks.push(tokio::spawn(async move {
            use tokio::io::{AsyncSeekExt, AsyncWriteExt};
            let mut out = tokio::fs::OpenOptions::new().write(true).open(&part).await?;
            loop {
                let next = queue.lock().unwrap().pop_front();
                let Some((from, to)) = next else { break };
                let mut at = from;
                let (mut tries, mut redials) = (0u32, 0u32);
                // How fast it was when last dropped for being slow: a new connection no faster
                // means the site is just slow right now, and dropping more won't help.
                let mut was: Option<u64> = None;
                while at < to {
                    if cancel.load(Ordering::Relaxed) {
                        return Err(Error::Cancelled);
                    }
                    // The address it landed on, then the first one again if that stops working
                    // (a mirror's link can expire).
                    let ask = if tries < 2 { &first } else { &url };
                    let r = http.get(ask).header(reqwest::header::RANGE, format!("bytes={at}-{}", to - 1)).timeout(Duration::from_secs(60 * 30)).send().await;
                    let r = match r {
                        Ok(r) if r.status() == reqwest::StatusCode::PARTIAL_CONTENT => r,
                        other => {
                            tries += 1;
                            match other {
                                Ok(r) if tries >= 5 => return Err(Error::Msg(answered(r.status().as_u16(), &url, url.rsplit('/').next().unwrap_or("the file")))),
                                Err(e) if tries >= 5 => return Err(e.into()),
                                _ => {}
                            }
                            tokio::time::sleep(Duration::from_secs(2 * tries as u64)).await;
                            continue;
                        }
                    };
                    out.seek(std::io::SeekFrom::Start(at)).await?;
                    let mut stream = r.bytes_stream();
                    let started = Instant::now();
                    let (mut win, mut win_bytes) = (Instant::now(), 0u64);
                    let mut slowed = false;
                    loop {
                        if cancel.load(Ordering::Relaxed) {
                            return Err(Error::Cancelled);
                        }
                        match tokio::time::timeout(Duration::from_secs(30), stream.next()).await {
                            Ok(Some(Ok(b))) => {
                                let n = (b.len() as u64).min(to - at);
                                out.write_all(&b[..n as usize]).await?;
                                at += n;
                                win_bytes += n;
                                done.fetch_add(n, Ordering::Relaxed);
                                if at >= to {
                                    break;
                                }
                                // Every few seconds: this connection's speed against the best one.
                                // Far behind (a quarter of it) once it's been going a while: dropped
                                // and asked again from here, which gets a fresh, fast connection.
                                let secs = win.elapsed().as_secs_f64();
                                if secs >= 4.0 {
                                    let rate = (win_bytes as f64 / secs) as u64;
                                    (win, win_bytes) = (Instant::now(), 0);
                                    if let Some(w) = was.take() {
                                        if rate <= w.saturating_mul(2) {
                                            // Asking again didn't help: this is the speed now.
                                            best.store(rate, Ordering::Relaxed);
                                            redials = u32::MAX;
                                        }
                                    }
                                    let top = best.fetch_max(rate, Ordering::Relaxed).max(rate);
                                    if started.elapsed() >= Duration::from_secs(8) && rate < top / 4 && redials < 30 {
                                        was = Some(rate);
                                        slowed = true;
                                        break;
                                    }
                                }
                            }
                            // Ended early or stalled: asked again from where it got to.
                            _ => {
                                tries += 1;
                                break;
                            }
                        }
                    }
                    if slowed {
                        redials += 1;
                        continue;
                    }
                    if at < to && tries >= 8 {
                        return Err(Error::Msg(format!("The download of {} stopped. Try again.", url.rsplit('/').next().unwrap_or("the file"))));
                    }
                }
            }
            out.flush().await?;
            Ok::<(), Error>(())
        }));
    }
    // Progress while the pieces come in.
    let (mut mark, mut mark_bytes, mut speed) = (Instant::now(), 0u64, 0u64);
    let all = futures_util::future::try_join_all(tasks.into_iter().map(|t| async move { t.await.map_err(|e| Error::Msg(e.to_string()))? }));
    tokio::pin!(all);
    let res = loop {
        tokio::select! {
            r = &mut all => break r,
            // A few updates a second, so the Downloads graph has points to draw even on a short download.
            _ = tokio::time::sleep(Duration::from_millis(150)) => {
                let d = done.load(Ordering::Relaxed);
                if mark.elapsed() >= Duration::from_secs(1) {
                    speed = ((d - mark_bytes) as f64 / mark.elapsed().as_secs_f64()) as u64;
                    mark = Instant::now();
                    mark_bytes = d;
                }
                let mut p = Progress::new(id, "download", d, total);
                p.speed = speed;
                report(p);
            }
        }
    };
    if let Err(e) = res {
        cancel.store(true, Ordering::Relaxed);
        let _ = fs::remove_file(&part);
        return Err(e);
    }
    report(Progress::new(id, "verify", total, total));
    let p2 = part.clone();
    let sha256 = tokio::task::spawn_blocking(move || -> Result<String, Error> {
        let mut h = Sha256::new();
        let mut f = std::fs::File::open(&p2)?;
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = std::io::Read::read(&mut f, &mut buf)?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
        Ok(hex::encode(h.finalize()))
    })
    .await
    .map_err(|e| Error::Msg(e.to_string()))??;
    let _ = fs::remove_file(&file);
    fs::rename(&part, &file)?;
    Ok(Some(Downloaded { file, sha256, verified: false }))
}

/// Download `asset` into `dir`, reporting progress, then check it against GitHub's checksum.
/// Downloads the player paused: stopped where they got to, with the part file kept, so Install (or
/// Update) carries on from there.
static PAUSED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
/// The player's speed limit in bytes a second (0 for none), and whether downloads slow down while
/// a game is running.
static LIMIT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static SLOW_PLAYING: AtomicBool = AtomicBool::new(true);
/// While a game runs, downloads keep to this so the game's own reading and network come first.
const PLAYING_RATE: u64 = 4 << 20;
static PLAYING: std::sync::OnceLock<Box<dyn Fn() -> bool + Send + Sync>> = std::sync::OnceLock::new();

pub fn pause(id: &str) {
    let mut p = PAUSED.lock().unwrap();
    if !p.iter().any(|x| x == id) {
        p.push(id.to_string());
    }
}
pub fn paused(id: &str) -> bool {
    PAUSED.lock().unwrap().iter().any(|x| x == id)
}
pub fn unpause(id: &str) {
    PAUSED.lock().unwrap().retain(|x| x != id);
}
/// Where each paused download's part file is, so cancelling a paused one can throw it away.
static PARTS: std::sync::Mutex<Vec<(String, PathBuf)>> = std::sync::Mutex::new(Vec::new());
fn kept(id: &str, part: &Path) {
    let mut p = PARTS.lock().unwrap();
    p.retain(|(i, _)| i != id);
    p.push((id.to_string(), part.to_path_buf()));
}
/// A paused download carried on: its part file is in use again, not one to throw away.
pub fn resumed(id: &str) {
    PARTS.lock().unwrap().retain(|(i, _)| i != id);
}
/// Throws away what a paused download had got (the player cancelled it instead of resuming).
pub fn discard(id: &str) {
    let mut p = PARTS.lock().unwrap();
    for (_, f) in p.iter().filter(|(i, _)| i == id) {
        let _ = std::fs::remove_file(f);
    }
    p.retain(|(i, _)| i != id);
}
pub fn set_limits(bytes_per_sec: u64, slow_while_playing: bool) {
    LIMIT.store(bytes_per_sec, Ordering::Relaxed);
    SLOW_PLAYING.store(slow_while_playing, Ordering::Relaxed);
}
/// How the launcher tells whether a game is running (set once when it starts).
pub fn playing_probe(f: impl Fn() -> bool + Send + Sync + 'static) {
    let _ = PLAYING.set(Box::new(f));
}
/// The speed a download keeps to right now, in bytes a second (0 for as fast as it goes).
fn rate_now() -> u64 {
    let limit = LIMIT.load(Ordering::Relaxed);
    let playing = SLOW_PLAYING.load(Ordering::Relaxed) && PLAYING.get().map(|f| f()).unwrap_or(false);
    rate_for(limit, playing)
}
fn rate_for(limit: u64, playing: bool) -> u64 {
    match (limit, playing) {
        (0, false) => 0,
        (0, true) => PLAYING_RATE,
        (l, true) => l.min(PLAYING_RATE),
        (l, false) => l,
    }
}

pub async fn download(
    http: &reqwest::Client,
    asset: &Asset,
    dir: &Path,
    id: &str,
    cancel: Arc<AtomicBool>,
    report: &(dyn Fn(Progress) + Send + Sync),
) -> Result<Downloaded, Error> {
    let api = asset.url.as_deref().filter(|u| u.starts_with("https://api.github.com/"));
    download_with_api(http, asset, dir, id, cancel, report, api).await
}

async fn download_with_api(
    http: &reqwest::Client,
    asset: &Asset,
    dir: &Path,
    id: &str,
    cancel: Arc<AtomicBool>,
    report: &(dyn Fn(Progress) + Send + Sync),
    api: Option<&str>,
) -> Result<Downloaded, Error> {
    fs::create_dir_all(dir)?;
    let safe: String = asset.name.chars().map(|c| if c.is_ascii_alphanumeric() || "-_.+".contains(c) { c } else { '_' }).collect();
    let file = dir.join(&safe);
    // The part file is named for this exact file (its size and download address), so a stopped
    // download of another release with the same file name is never carried on from.
    let tag = &hex::encode(Sha256::digest(format!("{}|{}", asset.size, asset.browser_download_url).as_bytes()))[..10];
    let part = dir.join(format!("{safe}.{tag}.part"));
    // A download that stopped part way carries on from where it got to.
    let mut hash = Sha256::new();
    let mut have = 0u64;
    if let Ok(mut f) = std::fs::File::open(&part) {
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = std::io::Read::read(&mut f, &mut buf)?;
            if n == 0 {
                break;
            }
            hash.update(&buf[..n]);
            have += n as u64;
        }
    }
    // GitHub sometimes accepts the connection and then sends nothing. Waiting for the answer and for
    // each piece is limited, cancelling works while it waits, and a download that stalls part way
    // carries on from where it got to, a few times, before it gives up.
    let cancelled = || cancel.load(Ordering::Relaxed);
    // Stopped: a pause keeps what came so far; a cancel throws it away.
    let stop = |part: &Path| -> Error {
        if paused(id) {
            kept(id, part);
            return Error::Msg("PAUSED".into());
        }
        let _ = std::fs::remove_file(part);
        Error::Cancelled
    };
    let mut out: Option<tokio::fs::File> = None;
    let mut done = have;
    let mut total;
    let mut tries = 0u32;
    use tokio::io::AsyncWriteExt;
    // github.com's download link, or GitHub's API link to the same file once that keeps failing.
    let mut src = asset.browser_download_url.clone();
    let mut by_api = false;
    let mut api_tried = false;
    // The same alternate route helps when the edge drops a connection or never answers. It is
    // tried once per download, after repeated failures, with the byte offset and hash preserved.
    let mut try_api = |tries: u32, src: &mut String, by_api: &mut bool| {
        if tries < 3 || api_tried {
            return false;
        }
        let Some(api) = api else { return false };
        crate::trail::note(format!("{}: the download keeps failing; trying GitHub's API link", asset.name));
        *src = api.to_string();
        *by_api = true;
        api_tried = true;
        true
    };
    'attempt: loop {
        tries += 1;
        let mut req = http.get(&src).timeout(Duration::from_secs(60 * 60 * 6));
        if by_api {
            req = req.header(reqwest::header::ACCEPT, "application/octet-stream");
        }
        if done > 0 {
            req = req.header(reqwest::header::RANGE, format!("bytes={done}-"));
        }
        let send = req.send();
        tokio::pin!(send);
        let started = Instant::now();
        let r = loop {
            if cancelled() {
                if let Some(mut f) = out.take() { let _ = f.flush().await; }
                return Err(stop(&part));
            }
            match tokio::time::timeout(Duration::from_millis(500), &mut send).await {
                Ok(r) => break r,
                Err(_) if started.elapsed() < Duration::from_secs(30) => continue,
                Err(_) if try_api(tries, &mut src, &mut by_api) => continue 'attempt,
                Err(_) if tries < 4 => continue 'attempt,
                Err(_) => return Err(Error::Msg(format!("{} isn't answering right now. Try again in a minute; the download carries on from where it got to.", site_of(&asset.browser_download_url)))),
            }
        };
        let r = match r {
            Ok(r) => r,
            Err(e) if (e.is_timeout() || e.is_connect() || e.is_request()) && try_api(tries, &mut src, &mut by_api) => continue,
            Err(e) if tries < 4 && (e.is_timeout() || e.is_connect() || e.is_request()) => {
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        if !r.status().is_success() {
            // GitHub's busy moments (502, 503, 504, or "slow down"): wait a little longer each time and
            // ask again, from where it got to.
            // Still busy after a few tries: the same file through GitHub's API, which often answers
            // when the download link doesn't.
            if busy_answer(r.status().as_u16()) && try_api(tries, &mut src, &mut by_api) {
                continue;
            }
            // The API said no for another reason (its hourly limit for a PC that isn't signed in):
            // back to the usual link, which keeps its own tries.
            if by_api && !busy_answer(r.status().as_u16()) && tries < 8 {
                crate::trail::note(format!("{}: GitHub's API link answered {}; back to github.com", asset.name, r.status().as_u16()));
                src = asset.browser_download_url.clone();
                by_api = false;
                continue;
            }
            if busy_answer(r.status().as_u16()) && (tries < 5 || (by_api && tries < 8)) {
                let wait = Instant::now() + Duration::from_secs(2u64 << (tries - 1).min(4));
                while Instant::now() < wait {
                    if cancelled() {
                        if let Some(mut f) = out.take() { let _ = f.flush().await; }
                        return Err(stop(&part));
                    }
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
                continue;
            }
            if r.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
                done = 0;
                hash = Sha256::new();
                out = None;
                let _ = tokio::fs::remove_file(&part).await;
                if tries < 4 {
                    continue;
                }
            }
            if out.is_none() {
                let _ = tokio::fs::remove_file(&part).await;
            }
            return Err(Error::Msg(answered(r.status().as_u16(), &asset.browser_download_url, &asset.name)));
        }
        let resumed = done > 0 && r.status() == reqwest::StatusCode::PARTIAL_CONTENT;
        if !resumed {
            hash = Sha256::new();
            done = 0;
            out = None;
        }
        total = r.content_length().map(|n| n + done).unwrap_or(asset.size);
        if out.is_none() {
            out = Some(if resumed { tokio::fs::OpenOptions::new().append(true).open(&part).await? } else { tokio::fs::File::create(&part).await? });
        }
        let f = out.as_mut().unwrap();
        let mut stream = r.bytes_stream();
        let (mut last, mut mark, mut mark_bytes, mut speed) = (Instant::now() - Duration::from_secs(1), Instant::now(), done, 0u64);
        let mut quiet = Instant::now();
        let (mut pace_at, mut pace_bytes, mut pace_rate) = (Instant::now(), 0u64, 0u64);
        loop {
            if cancelled() {
                break 'attempt;
            }
            let next = match tokio::time::timeout(Duration::from_millis(500), stream.next()).await {
                Ok(n) => n,
                Err(_) if quiet.elapsed() < Duration::from_secs(40) => continue,
                // Nothing for 40 seconds: ask again from where it got to.
                Err(_) if tries < 6 => { f.flush().await?; try_api(tries, &mut src, &mut by_api); continue 'attempt; }
                Err(_) => return Err(Error::Msg(format!("The download of {} stopped. Press Install again; it carries on from where it got to.", asset.name))),
            };
            let Some(chunk) = next else { break 'attempt };
            let chunk = match chunk {
                Ok(c) => c,
                Err(_) if tries < 6 => { f.flush().await?; if !try_api(tries, &mut src, &mut by_api) { tokio::time::sleep(Duration::from_secs(2)).await; } continue 'attempt; }
                Err(e) => return Err(e.into()),
            };
            quiet = Instant::now();
            hash.update(&chunk);
            f.write_all(&chunk).await?;
            done += chunk.len() as u64;
            // Kept to the player's limit (and slower while a game is running): ahead of it, it waits
            // the difference, a little at a time so pausing and cancelling still answer at once.
            let rate = rate_now();
            if rate > 0 {
                if pace_rate != rate { pace_rate = rate; pace_at = Instant::now(); pace_bytes = 0; }
                pace_bytes += chunk.len() as u64;
                let due = Duration::from_secs_f64(pace_bytes as f64 / rate as f64);
                while pace_at.elapsed() < due && !cancelled() {
                    tokio::time::sleep(due.saturating_sub(pace_at.elapsed()).clamp(Duration::from_millis(1), Duration::from_millis(200))).await;
                }
                quiet = Instant::now();
                // A fresh window now and then, so a short burst after a pause isn't held against it.
                if pace_at.elapsed() > Duration::from_secs(4) { pace_at = Instant::now(); pace_bytes = 0; }
            } else {
                pace_rate = 0;
            }
            if mark.elapsed() >= Duration::from_millis(800) {
                speed = ((done - mark_bytes) as f64 / mark.elapsed().as_secs_f64()) as u64;
                mark = Instant::now();
                mark_bytes = done;
            }
            if last.elapsed() >= Duration::from_millis(150) {
                last = Instant::now();
                let mut p = Progress::new(id, "download", done, total);
                p.speed = speed;
                report(p);
            }
        }
    }
    if cancelled() {
        if let Some(mut f) = out.take() { let _ = f.flush().await; }
        return Err(stop(&part));
    }
    let Some(mut out) = out else { return Err(Error::Msg(format!("The download of {} didn't start. Try again.", asset.name))) };
    out.flush().await?;
    drop(out);
    report(Progress::new(id, "verify", done, total));
    let got = hex::encode(hash.finalize());
    let want = asset.digest.as_deref().and_then(|d| d.strip_prefix("sha256:")).map(str::to_lowercase);
    if let Some(w) = &want {
        if *w != got {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(Error::Msg(format!("{} didn't download right (its checksum doesn't match GitHub's). Try again.", asset.name)));
        }
    }
    tokio::fs::rename(&part, &file).await?;
    Ok(Downloaded { file, sha256: got, verified: want.is_some() })
}

/// A path inside an archive, made safe to join onto the folder: no absolute paths, no `..`.
fn inside(name: &str) -> Option<PathBuf> {
    let mut p = PathBuf::new();
    for c in Path::new(&name.replace('\\', "/")).components() {
        match c {
            Component::Normal(s) => p.push(s),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!p.as_os_str().is_empty()).then_some(p)
}

fn unzip(file: &Path, to: &Path) -> Result<(), Error> {
    let mut z = zip::ZipArchive::new(fs::File::open(file)?).map_err(|e| Error::Msg(format!("Couldn't open the zip: {e}")))?;
    #[cfg(unix)]
    let mut links = Vec::new();
    for i in 0..z.len() {
        let mut e = z.by_index(i).map_err(|e| Error::Msg(format!("Couldn't read the zip: {e}")))?;
        let Some(rel) = inside(e.name()) else { continue };
        // Finder's companion metadata is not part of the application or a packaging wrapper.
        if rel.components().any(|c| c.as_os_str() == "__MACOSX" || c.as_os_str() == ".DS_Store") {
            continue;
        }
        let dest = to.join(rel);
        #[cfg(unix)]
        if e.unix_mode().map(|m| m & 0o170000 == 0o120000).unwrap_or(false) {
            let mut target = String::new();
            io::Read::read_to_string(&mut io::Read::take(&mut e, 4097), &mut target)?;
            if target.len() > 4096 || !safe_link(to, &dest, &target) {
                return Err(Error::Msg("The archive contains a link outside its installation folder.".into()));
            }
            links.push((dest, target));
            continue;
        }
        if e.is_dir() {
            fs::create_dir_all(&dest)?;
            continue;
        }
        if let Some(d) = dest.parent() {
            fs::create_dir_all(d)?;
        }
        let mut out = fs::File::create(&dest)?;
        io::copy(&mut e, &mut out)?;
        #[cfg(unix)]
        if let Some(mode) = e.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&dest, fs::Permissions::from_mode(mode & 0o777 | 0o600));
        }
    }
    // Make every link's parent before installing any links. Otherwise a later link could
    // follow an earlier link chain while creating its parent, before final validation.
    #[cfg(unix)]
    for (dest, _) in &links {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
    }
    #[cfg(unix)]
    for (dest, target) in links {
        std::os::unix::fs::symlink(target, dest)?;
    }
    validate_links(to, to)?;
    Ok(())
}

#[cfg(unix)]
fn safe_link(base: &Path, link: &Path, target: &str) -> bool {
    if target.is_empty() || target.contains('\\') {
        return false;
    }
    let Some(parent) = link.parent().and_then(|p| p.strip_prefix(base).ok()) else { return false };
    let mut depth = parent.components().count();
    for part in Path::new(target).components() {
        match part {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {},
            Component::ParentDir if depth > 0 => depth -= 1,
            _ => return false,
        }
    }
    true
}

/// Also check resolved link chains and the final, unwrapped root. Never recurse through links.
fn validate_links(dir: &Path, base: &Path) -> Result<(), Error> {
    let root = fs::canonicalize(base)?;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let ty = entry.file_type()?;
            if ty.is_symlink() {
                // A link to a file the archive doesn't have, or one that loops (common inside Mac
                // apps: a framework's "Current" left from the developer's machine): it points at
                // nothing, so it's dropped and the rest installs.
                let Ok(target) = fs::canonicalize(entry.path()) else {
                    crate::trail::note(format!("Install: dropped a link that points nowhere ({})", entry.file_name().to_string_lossy()));
                    fs::remove_file(entry.path())?;
                    continue;
                };
                if !target.starts_with(&root) {
                    return Err(Error::Msg("The archive contains a link outside its installation folder.".into()));
                }
            } else if ty.is_dir() {
                stack.push(entry.path());
            }
        }
    }
    Ok(())
}

fn un7z(file: &Path, to: &Path) -> Result<(), Error> {
    sevenz_rust::decompress_file_with_extract_fn(file, to, |entry, reader, _default| {
        let Some(rel) = inside(entry.name()) else { return Ok(true) };
        let dest = to.join(rel);
        if entry.is_directory() {
            fs::create_dir_all(&dest)?;
        } else {
            if let Some(d) = dest.parent() {
                fs::create_dir_all(d)?;
            }
            let mut out = fs::File::create(&dest)?;
            io::copy(reader, &mut out)?;
        }
        Ok(true)
    })
    .map_err(|e| Error::Msg(format!("Couldn't open the 7z: {e}")))
}

/// A tarball, compressed with gzip, xz or zstd. tar skips entries that would land outside the folder.
fn untar(file: &Path, to: &Path, k: &str) -> Result<(), Error> {
    let f = io::BufReader::new(fs::File::open(file)?);
    let read: Box<dyn io::Read> = match k {
        // Several xz streams one after another (parallel compressors write them) are one file.
        "tar.xz" => Box::new(xz2::read::XzDecoder::new_multi_decoder(f)),
        "tar.zst" => Box::new(zstd::stream::read::Decoder::with_buffer(f)?),
        _ => Box::new(flate2::read::GzDecoder::new(f)),
    };
    let mut t = tar::Archive::new(read);
    t.set_preserve_permissions(true);
    t.unpack(to)?;
    Ok(())
}

/// The folder the files actually start in: an archive with one folder at the top is that folder.
fn root(dir: &Path) -> PathBuf {
    let items: Vec<_> = fs::read_dir(dir).map(|r| r.flatten().collect()).unwrap_or_default();
    if items.len() == 1 && items[0].file_type().map(|t| t.is_dir()).unwrap_or(false) && !crate::macos::is_bundle(&items[0].path()) {
        root(&items[0].path())
    } else {
        dir.to_path_buf()
    }
}

/// A file operation that Windows can refuse for a few seconds (access denied, or in use) while an
/// antivirus looks at a file that was just unpacked: tried again for up to about ten seconds.
pub(crate) fn patient<T>(mut op: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut last = None;
    for i in 0..20 {
        match op() {
            Ok(v) => return Ok(v),
            Err(e) if in_use(&e) => {
                last = Some(e);
                std::thread::sleep(std::time::Duration::from_millis(if i < 4 { 250 } else { 600 }));
            }
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| io::Error::other("busy")))
}

/// A file another program has open (a running game, an antivirus), or one the launcher may not change.
pub(crate) fn in_use(e: &io::Error) -> bool {
    matches!(e.raw_os_error(), Some(5) | Some(32) | Some(33)) || e.kind() == io::ErrorKind::PermissionDenied
}

/// Files unpacked in staging moved into `to`, also in staging, each replacing what has its name (an
/// AppImage's own recomplauncher.json wins over one beside it in a zip, as its manifest does).
/// Folders on both sides merge, except a Mac app: one signed unit, replaced whole.
#[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
fn merge_into(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for e in fs::read_dir(from)? {
        let e = e?;
        let dest = to.join(e.file_name());
        let there = fs::symlink_metadata(&dest).ok();
        let both_folders = e.file_type()?.is_dir() && there.as_ref().is_some_and(|m| m.is_dir()) && !crate::macos::is_bundle(&e.path());
        if both_folders {
            merge_into(&e.path(), &dest)?;
            continue;
        }
        match there {
            Some(m) if m.is_dir() => fs::remove_dir_all(&dest)?,
            Some(_) => fs::remove_file(&dest)?,
            None => {}
        }
        fs::rename(e.path(), &dest)?;
    }
    Ok(())
}

/// For an update: the files the previous release put in the game's folder that this release no
/// longer has, for the update to take away with the rest of the old release (see `swap`). Left
/// behind, a source file or library an old version had can break a build or load in place of the
/// new one (DKB Recomp 0.2.0's runtime/gfx_present.c broke 0.3.0's link with duplicate symbols). A
/// file changed since that install (settings the game rewrote, a file the player edited) and
/// `keep` (the program the player starts) stay.
pub fn dropped(target: &Path, old: &[PathBuf], new: &[PathBuf], installed_at: u64, keep: Option<&Path>) -> Vec<PathBuf> {
    // Case doesn't tell files apart on Windows and Mac: a file only renamed in case is still there.
    let fold = |p: &Path| p.to_string_lossy().replace('\\', "/").to_lowercase();
    let still_shipped: std::collections::HashSet<String> = new.iter().map(|p| fold(p)).collect();
    let kept = keep.map(fold);
    let mut gone = vec![];
    for rel in old {
        let name = fold(rel);
        if still_shipped.contains(&name) || kept.as_deref() == Some(name.as_str()) {
            continue;
        }
        if !rel.components().all(|c| matches!(c, Component::Normal(_))) {
            continue;
        }
        let path = target.join(rel);
        let Ok(meta) = fs::symlink_metadata(&path) else { continue };
        if !meta.is_file() {
            continue;
        }
        let since = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
        let changed = since.map(|d| d.as_secs() > installed_at).unwrap_or(true);
        if !changed {
            gone.push(rel.clone());
        }
    }
    gone
}

/// The program an update starts from (see updated_exe), or None to find it again once the update is
/// in: one the update takes away is gone. `picked`: the player chose it, so it stays while it's there.
pub fn kept_exe(exe: Option<PathBuf>, game: &Path, os: &str, dropped: &[PathBuf], picked: bool, names: &[&str]) -> Option<PathBuf> {
    let exe = exe?;
    if !game.join(&exe).is_file() || dropped.contains(&exe) {
        return None;
    }
    if picked {
        return Some(exe);
    }
    // A game imported as a Mac app starts what the new app's Info.plist names: a release can rename
    // its program, and the old one may stay on as a helper.
    if os == "macos" && crate::macos::is_bundle(game) {
        return None;
    }
    // One the launcher picked that's one of the game's tools (an older version's guess) isn't kept.
    (!looks_like_tool(&exe, names)).then_some(exe)
}

/// Keep the selected program through an update, following release renames and moves. Run before
/// the update's files go in. Also returns the old release's AppImage a differently named new one
/// replaces, for the update to take away.
pub fn updated_exe(id: &str, target: &Path, exe: Option<PathBuf>, files: &[PathBuf], kept: bool) -> (Option<PathBuf>, Option<PathBuf>) {
    let Some(old) = exe.clone() else { return (None, None) };
    // A player's program outside the install belongs to them. Neither a release's AppImage nor
    // a file with the same name replaces it, including paths through parent folders or links.
    if !old.components().all(|c| matches!(c, Component::Normal(_))) {
        return (exe, None);
    }
    let Ok(root) = fs::canonicalize(target) else { return (exe, None) };
    let Ok(path) = fs::canonicalize(target.join(&old)) else { return (exe, None) };
    if !path.starts_with(root) {
        return (exe, None);
    }
    let is_appimage = old.to_string_lossy().to_lowercase().ends_with(".appimage");
    let new_appimage = files.iter().find(|f| f.to_string_lossy().to_lowercase().ends_with(".appimage"));
    if let (true, Some(n)) = (is_appimage, new_appimage) {
        if *n != old {
            return (Some(n.clone()), Some(old));
        }
    }
    // The AppImage an older launcher installed, and the new release unpacked (on Linux every game
    // plays from a plain folder): its AppRun starts the game now, and the old AppImage goes.
    if let Some(start) = is_appimage.then(|| unpacked_start(files, &old)).flatten() {
        return (Some(start), Some(old));
    }
    // The program the player picked isn't one of the new release's files (it was moved inside the
    // release, say): the new release's file of the same name, so Play starts the update.
    if !kept || files.contains(&old) {
        return (exe, None);
    }
    let name = old.file_name().map(|n| n.to_ascii_lowercase());
    let Some(moved) = files.iter().find(|f| f.file_name().map(|n| n.to_ascii_lowercase()) == name) else { return (exe, None) };
    crate::trail::note(format!("Install {id}: the program you picked is at {} in this release now", moved.display()));
    (Some(moved.clone()), None)
}

/// The AppRun that starts an unpacked AppImage among a release's files: the one at the top, else
/// the one in the folder named after the AppImage (see unpack_appimages), else the only one.
pub fn unpacked_start(files: &[PathBuf], image: &Path) -> Option<PathBuf> {
    let top = PathBuf::from("AppRun");
    if files.contains(&top) {
        return Some(top);
    }
    let own = image.with_extension("").join("AppRun");
    if files.contains(&own) {
        return Some(own);
    }
    let one_down: Vec<&PathBuf> = files.iter().filter(|f| f.components().count() == 2 && f.file_name().is_some_and(|n| n == "AppRun")).collect();
    match one_down.as_slice() {
        [only] => Some((*only).clone()),
        _ => None,
    }
}

/// The folders above a removed file that it leaves empty, up to (not including) `top`.
pub(crate) fn remove_empty_parents(top: &Path, file: &Path) {
    let mut dir = file.parent();
    while let Some(d) = dir.filter(|d| *d != top && d.starts_with(top)) {
        if fs::remove_dir(d).is_err() {
            return;
        }
        dir = d.parent();
    }
}

/// Unpack (or place) the downloaded file into `target`. Returns the files it put there, relative.
/// A site's own name, not the server it handed the file to (files3.eu.example.com: example.com).
pub fn site_of(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.split('.').rev().take(2).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(".")))
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "The site".into())
}

/// What a download site's error means, naming the site it came from.
/// An answer that means the site is busy for a moment (worth asking again after a short wait).
pub fn busy_answer(code: u16) -> bool {
    matches!(code, 429 | 500 | 502 | 503 | 504)
}

pub fn answered(code: u16, url: &str, file: &str) -> String {
    let site = site_of(url);
    match code {
        401 | 403 => format!("{site} won't send {file} (error {code}). It may need you signed in, or it isn't shared."),
        404 | 410 => format!("{site} doesn't have {file} anymore (error {code})."),
        429 => format!("{site} is getting too many downloads right now (error {code}). Try again in a few minutes."),
        500..=599 => format!("{site} had a problem sending {file} (error {code}). That's on their side; try again in a bit."),
        _ => format!("{site} answered {code} for {file}."),
    }
}

pub fn unpack(file: &Path, target: &Path, staging: &Path) -> Result<Vec<PathBuf>, Error> {
    unpack_with(file, target, staging, true)
}

/// Like unpack, but keeps the archive's folders as they are (a mod's own layout), even a single
/// folder at the top.
pub fn unpack_as_is(file: &Path, target: &Path, staging: &Path) -> Result<Vec<PathBuf>, Error> {
    unpack_with(file, target, staging, false)
}

/// What the marker of a swap that isn't a game's release says instead of a download's SHA-256: no
/// game's record has it, so a launcher closed in the middle undoes it (see swap::recover).
const NOT_A_RELEASE: &str = "not-a-release";

/// Unpacked beside `target` first, then put in all at once (see swap): a mod, an installer or a
/// copy of a game goes in whole, or `target` stays as it was. Nothing keeps a list of what such an
/// unpack put there before, so none of what's there counts as its own: a file of it where `target`
/// has a folder (or a folder where it has a file) stops it, rather than setting the player's files
/// aside. A file over a file of the same name is replaced, as it always was.
fn unpack_with(file: &Path, target: &Path, staging: &Path, strip_top: bool) -> Result<Vec<PathBuf>, Error> {
    let staged = stage_with(file, staging, strip_top)?;
    let placed = crate::swap::put_in_place(target, &staged.root, &staged.files, NOT_A_RELEASE, &[], &[]);
    let _ = fs::remove_dir_all(staging);
    placed?.finish();
    Ok(staged.files)
}

/// A release unpacked in its staging folder, ready to go into the game's folder.
pub struct Staged {
    /// Where its files start, inside the staging folder.
    pub root: PathBuf,
    /// Its files, relative to `root`.
    pub files: Vec<PathBuf>,
}

impl Staged {
    /// For an update of a game whose folder is a Mac app itself (an app the player picked when
    /// importing it): the release's one app goes in, its files in place of the old app's, never
    /// nested inside it. Also every file of the old app's Contents the new one doesn't have, for the
    /// update to take away: a signed app must hold exactly what it was signed with. What the game
    /// keeps beside Contents ({data}) stays. Any other game: as it is, nothing more taken away.
    pub fn for_game(self, game: &Path, os: &str) -> Result<(Staged, Vec<PathBuf>), Error> {
        if os != "macos" || !crate::macos::is_bundle(game) || crate::macos::is_bundle(&self.root) {
            return Ok((self, vec![]));
        }
        let mut apps = vec![];
        for e in fs::read_dir(&self.root)? {
            let p = e?.path();
            if crate::macos::is_bundle(&p) {
                apps.push(p);
            }
        }
        let [app] = apps.as_slice() else {
            return Err(Error::Msg("This release doesn't have exactly one Mac app, so it can't replace the app the game was imported as. Nothing was changed.".into()));
        };
        let top = app.strip_prefix(&self.root).unwrap_or(app).to_path_buf();
        let files: Vec<PathBuf> = self.files.iter().filter_map(|f| f.strip_prefix(&top).ok()).map(Path::to_path_buf).collect();
        let shipped: std::collections::HashSet<&PathBuf> = files.iter().collect();
        let mut old = vec![];
        // Links too, never followed: a Mac app's frameworks link inside it.
        list(game, &game.join("Contents"), &mut old);
        let leftovers = old.into_iter().filter(|f| !shipped.contains(f)).collect();
        Ok((Staged { root: app.clone(), files }, leftovers))
    }
}

/// Unpack (or place) the downloaded file into `staging`, the way a game's release is: a single
/// folder at the top unwrapped. Nothing of the game's is touched (see `swap`).
pub fn stage(file: &Path, staging: &Path) -> Result<Staged, Error> {
    stage_with(file, staging, true)
}

fn stage_with(file: &Path, staging: &Path, strip_top: bool) -> Result<Staged, Error> {
    let name = file.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
    // A .rar (a plugin's download, a mod): unpacked the way mods are, then put in place below.
    let k = if name.to_lowercase().ends_with(".rar") { "rar" } else { kind(&name).ok_or_else(|| Error::Msg(format!("{name} isn't a kind of file the launcher can install")))? };
    // Windows Security and Search look at a fresh download first and can hold it for a moment.
    for _ in 0..20 {
        match fs::File::open(file) {
            Err(e) if matches!(e.raw_os_error(), Some(5) | Some(32) | Some(33)) => std::thread::sleep(std::time::Duration::from_millis(500)),
            _ => break,
        }
    }
    let _ = fs::remove_dir_all(staging);
    fs::create_dir_all(staging)?;
    match k {
        "zip" => unzip(file, staging)?,
        "7z" => un7z(file, staging)?,
        "rar" => crate::gamebanana::extract(file, staging)?,
        "tar.gz" | "tar.xz" | "tar.zst" => untar(file, staging, k)?,
        #[cfg(target_os = "macos")]
        "dmg" => undmg(file, staging)?,
        "appimage" | "exe" | "msi" => {
            fs::copy(file, staging.join(&name))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(staging.join(&name), fs::Permissions::from_mode(0o755))?;
            }
        }
        _ => return Err(Error::Msg(format!("{name} can't be installed on this PC"))),
    }
    // A .7z or .rar doesn't carry a file's executable bit the way a zip's entries do: a Mac program
    // out of one is found by its header and marked runnable.
    #[cfg(target_os = "macos")]
    if matches!(k, "7z" | "rar") {
        mark_mac_programs(staging);
    }
    let from = if strip_top { root(staging) } else { staging.to_path_buf() };
    #[cfg(target_os = "macos")]
    let from = if strip_top { unwrap_mac_zip(from, staging)? } else { from };
    // On Linux every game plays from a plain folder: the release's AppImages are unpacked here, so
    // their files are the game's files from now on.
    #[cfg(target_os = "linux")]
    unpack_appimages(&from, staging)?;
    validate_links(&from, &from)?;
    // Bundles must be self-contained when moved or replaced as a unit.
    for entry in fs::read_dir(&from)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() && crate::macos::is_bundle(&entry.path()) {
            validate_links(&entry.path(), &entry.path())?;
        }
    }
    let mut files = vec![];
    list(&from, &from, &mut files);
    Ok(Staged { root: from, files })
}

/// What an AppImage that wouldn't unpack says, before its name and why. The install then takes the
/// release's tarball when it has one (see lib.rs).
pub const APPIMAGE_UNPACK: &str = "APPIMAGE_UNPACK|";

/// Every AppImage in a release, unpacked in staging (see appimage::unpack) and the AppImage taken
/// out, so the launcher never runs one. The only one, at the top of the release (on its own or out
/// of a zip), becomes the top, its AppRun beside the rest. Any other unpacks into a folder named
/// after it. Err names the one that didn't unpack, and why.
#[cfg(target_os = "linux")]
pub(crate) fn unpack_appimages(dir: &Path, staging: &Path) -> Result<(), Error> {
    let images = appimages_in(dir);
    let alone_at_top = images.len() == 1 && images[0].parent() == Some(dir);
    let work = staging.with_extension("appdir");
    for image in &images {
        let name = image.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let into = if alone_at_top { dir.to_path_buf() } else { image.with_extension("") };
        let _ = fs::remove_dir_all(&work);
        let unpacked = unpack_one(image, &work, &into);
        let _ = fs::remove_dir_all(&work);
        if let Err(e) = unpacked {
            return Err(Error::Msg(format!("{APPIMAGE_UNPACK}{name}: {e}")));
        }
        crate::trail::note(format!("Install: unpacked {name} into a plain folder"));
    }
    Ok(())
}

/// An AppImage the player has (one they bring in, or one an older launcher left in a game's
/// folder) unpacked into `staging` as if it were a release: read, never run, and left as it is.
#[cfg(target_os = "linux")]
pub fn stage_appimage(image: &Path, staging: &Path) -> Result<Staged, Error> {
    let _ = fs::remove_dir_all(staging);
    let name = image.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let unpacked = crate::appimage::unpack(image, staging).and_then(|()| {
        let has_start = staging.join("AppRun").is_file();
        has_start.then_some(()).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "it has no AppRun"))
    });
    if let Err(e) = unpacked {
        let _ = fs::remove_dir_all(staging);
        return Err(Error::Msg(format!("{APPIMAGE_UNPACK}{name}: {e}")));
    }
    validate_links(staging, staging)?;
    let mut files = vec![];
    list(staging, staging, &mut files);
    Ok(Staged { root: staging.to_path_buf(), files })
}

/// One AppImage unpacked in `work`, then its files moved into `into` and the AppImage removed.
#[cfg(target_os = "linux")]
fn unpack_one(image: &Path, work: &Path, into: &Path) -> io::Result<()> {
    crate::appimage::unpack(image, work)?;
    // Every AppImage starts from its AppRun; without one there's nothing to play.
    if !work.join("AppRun").is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "it has no AppRun"));
    }
    fs::remove_file(image)?;
    merge_into(work, into)
}

/// The AppImages anywhere in `dir`: regular files named `….AppImage`, never through a link.
#[cfg(target_os = "linux")]
fn appimages_in(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).into_iter().flatten().flatten() {
            let Ok(ty) = e.file_type() else { continue };
            let path = e.path();
            if ty.is_dir() {
                stack.push(path);
                continue;
            }
            let named = path.extension().is_some_and(|x| x.eq_ignore_ascii_case("appimage"));
            if ty.is_file() && named {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// A Mac disk image: opened read-only where Finder doesn't show it, what's on it (the .app and
/// anything beside it, not the Applications shortcut or the window's background) copied out with
/// ditto so the bundle stays whole, then closed again.
#[cfg(target_os = "macos")]
pub(crate) fn undmg(file: &Path, staging: &Path) -> Result<(), Error> {
    let mount = staging.with_extension("mount");
    let _ = fs::remove_dir(&mount);
    fs::create_dir_all(&mount)?;
    let out = std::process::Command::new("/usr/bin/hdiutil")
        .args(["attach", "-nobrowse", "-readonly", "-noautoopen", "-noverify", "-mountpoint"])
        .arg(&mount)
        .arg(file)
        .stdin(std::process::Stdio::null())
        .output()?;
    if !out.status.success() {
        let _ = fs::remove_dir(&mount);
        return Err(Error::Msg(format!("Couldn't open the disk image: {}", String::from_utf8_lossy(&out.stderr).trim())));
    }
    let copied = (|| -> Result<(), Error> {
        for e in fs::read_dir(&mount)?.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if dmg_extra(&n) || e.file_type().map(|t| t.is_symlink()).unwrap_or(false) {
                continue;
            }
            let ok = std::process::Command::new("/usr/bin/ditto").arg(e.path()).arg(staging.join(&n)).status()?.success();
            if !ok {
                return Err(Error::Msg(format!("Couldn't copy {n} from the disk image")));
            }
        }
        Ok(())
    })();
    let detach = |force: bool| {
        let mut c = std::process::Command::new("/usr/bin/hdiutil");
        c.arg("detach").arg(&mount).arg("-quiet");
        if force {
            c.arg("-force");
        }
        c.status().map(|s| s.success()).unwrap_or(false)
    };
    if !detach(false) {
        std::thread::sleep(std::time::Duration::from_secs(1));
        detach(true);
    }
    let _ = fs::remove_dir(&mount);
    copied
}

/// What a disk image carries for its Finder window, not the game: the background picture, Finder's
/// own files, the volume icon, Spotlight's and the trash's folders.
#[cfg(any(test, target_os = "macos"))]
fn dmg_extra(name: &str) -> bool {
    name.starts_with('.') || matches!(name, "Applications" | "Icon\r" | "VolumeIcon.icns")
}

/// Mark every Mac executable under `dir` (by its Mach-O header) as runnable.
#[cfg(target_os = "macos")]
fn mark_mac_programs(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    for e in fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() {
            mark_mac_programs(&p);
        } else if t.is_file() && crate::macos::is_executable(&p, crate::assets::arch_now()) {
            let _ = fs::set_permissions(&p, fs::Permissions::from_mode(0o755));
        }
    }
}

/// A Mac download that is really a ZIP of the game: the one ZIP in the folder, with no application
/// or program beside it (a readme or a ROM may be). Unpacked in place, up to three layers deep.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn unwrap_mac_zip(mut from: PathBuf, staging: &Path) -> Result<PathBuf, Error> {
    for depth in 0..=3 {
        let entries = fs::read_dir(&from)?.collect::<Result<Vec<_>, _>>()?;
        let zips: Vec<_> = entries.iter().filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false) && kind(&e.file_name().to_string_lossy()) == Some("zip")).collect();
        let has_program = entries.iter().any(|e| crate::macos::is_bundle(&e.path()) || crate::macos::is_executable(&e.path(), crate::assets::arch_now()));
        if zips.len() != 1 || has_program {
            return Ok(from);
        }
        if depth == 3 {
            return Err(Error::Msg("The download has too many nested ZIP wrappers.".into()));
        }
        let layer = staging.join(format!(".mac-wrapper-{depth}"));
        fs::create_dir(&layer)?;
        unzip(&zips[0].path(), &layer)?;
        if entries.len() == 1 {
            from = root(&layer);
            continue;
        }
        // Files beside the ZIP stay with the game it holds.
        fs::remove_file(zips[0].path())?;
        merge_into(&root(&layer), &from)?;
        // The layer can sit inside `from` (a ZIP at the top of the download): not part of the game.
        let _ = fs::remove_dir_all(&layer);
    }
    unreachable!()
}

fn list(base: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            list(base, &p, out);
        } else if let Ok(r) = p.strip_prefix(base) {
            out.push(r.to_path_buf());
        }
    }
}

/// A name's words, lower case, CamelCase split: "Zelda64Recompiled" -> zelda64, recompiled.
pub fn name_tokens(s: &str) -> Vec<String> {
    tokens(s)
}

pub fn tokens(s: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let cs: Vec<char> = s.chars().collect();
    for (i, &c) in cs.iter().enumerate() {
        if !c.is_ascii_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        }
        // Split CamelCase: Zelda64Recompiled -> zelda64, recompiled; BotWRecomp -> bot, w, recomp
        // (an upper-case run ends where a new word starts); NFL08 stays whole.
        if c.is_ascii_uppercase() && !cur.is_empty() && i > 0 {
            let prev = cs[i - 1];
            let next_lower = cs.get(i + 1).map(|n| n.is_ascii_lowercase()).unwrap_or(false);
            if prev.is_ascii_lowercase() || prev.is_ascii_digit() || (prev.is_ascii_uppercase() && next_lower) {
                out.push(std::mem::take(&mut cur));
            }
        }
        cur.push(c.to_ascii_lowercase());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

// A game's own dedicated server (for hosting online play) is never the game, and neither is its
// installer.
// "instal" also covers other languages' installers ("Instalar", "Instalador", "Instalacja").
const NOT_THE_GAME: [&str; 18] = ["unins", "uninstall", "setup", "install", "instal", "crash", "crashpad", "redist", "vcredist", "helper", "updater", "update", "dxsetup", "vc", "elevate", "report", "server", "dedicated"];

/// Words that start a launch script's name: "Launch Madden NFL 08.bat", "Play.cmd".
const LAUNCH_WORDS: [&str; 5] = ["launch", "launcher", "play", "start", "run"];
/// Scripts that are for something else: debugging, a test, or playing without the launcher.
const NOT_A_LAUNCH: [&str; 11] = ["debug", "test", "safe", "sin", "without", "no", "build", "clean", "diagnostic", "diagnostics", "benchmark"];

/// Words at the end of a recomp's name that aren't the game's, and small words, for initials.
pub const TAIL_WORDS: [&str; 10] = ["recomp", "recompiled", "decomp", "decompiled", "pc", "port", "native", "online", "edition", "windows"];
const SMALL_WORDS: [&str; 6] = ["of", "the", "a", "and", "in", "to"];

/// A program kept for when the main one doesn't work: "inFAMOUS Second Son (classic).exe".
const OLD_WORDS: [&str; 4] = ["classic", "legacy", "old", "fallback"];
/// A program that skips the game's launcher: "Play inFAMOUS Second Son.exe".
const QUICK_WORDS: [&str; 3] = ["play", "quick", "direct"];

/// The version of find_exe's rules. Raise it when they pick better, and installs picked by older
/// rules get looked at again on their next Play (own_setup::pending).
pub const EXE_RULES: u8 = 21;

/// Words in the name of a program that comes with a game but is one of its tools: a translator,
/// shader builder, compiler, patcher or dedicated server (xcx_xlate, ultrs-shaders, xbe_recomp,
/// build_deps.sh, ttt2-server).
const TOOL_WORDS: [&str; 29] = ["config", "configure", "settings", "setting", "options", "xlate", "translate", "translator", "shader", "shaders", "compiler", "compile", "deps", "patcher", "patch", "extractor", "extract", "converter", "convert", "dumper", "packer", "unpacker", "server", "dedicated", "benchmark", "configurator", "devtool", "bktr", "keygen"];
/// Programs that build or unpack things, by their whole name: a release that builds the game on
/// the player's PC brings these along (bin/ninja.exe), and none of them is ever the game.
const BUILD_TOOLS: [&str; 66] = [
    "ninja", "cmake", "ctest", "cpack", "make", "mingw32-make", "nmake", "msbuild", "meson", "clang", "clang-cl", "clang++", "gcc", "g++", "cc", "c++",
    "tcc", "zig", "sccache", "ccache", "goalc",
    "ld", "lld", "lld-link", "ld.lld", "llvm-ar", "llvm-objcopy", "ar", "as", "nasm", "yasm", "rustc", "cargo", "python", "python3", "pythonw", "py",
    "node", "npm", "git", "7z", "7za", "7zr", "curl", "wget", "busybox", "unzip", "zstd", "xdelta3",
    // Disc and package converters a release brings along for the player's copy.
    "chd2gdi", "gdi2chd", "chdman", "maxcso", "extract-xiso", "xdvdfs", "hactool", "nsz",
    // Recompilers a release brings to build the game from the player's copy.
    "nxlift", "xbe_recomp", "xenonrecomp", "xenonanalyse", "n64recomp", "ps2recomp", "ffmpeg", "ffprobe", "ffplay",
];
/// Folders a release keeps its tools, or leftovers from building, in.
/// "Tools" in other languages too: ferramentas (Portuguese), herramientas (Spanish), outils
/// (French), werkzeuge (German), strumenti (Italian), narzedzia (Polish), gereedschap (Dutch).
pub const TOOL_DIRS: [&str; 32] = ["scripts", "compiler", "compilers", "deps", "dependencies", "tools", "tool", "temp", "tmp", "sdk", "redist", "_commonredist", "shaders", "llvm", "llvm-mingw", "toolchain", "toolchains", "busybox", "mingw32", "mingw64", "msys64", "msys2", "cmake", "w64devkit", "ferramentas", "herramientas", "outils", "werkzeuge", "strumenti", "narzedzia", "narzędzia", "gereedschap"];

/// Whether a program (relative to the game's folder) is one of the game's tools rather than the
/// game: by its name, or the folder it's in. A word that's in the game's own names doesn't count.
/// A runtime's or another program's installer (.NET, Visual C++, DirectX, WebView2...), never a game.
const RUNTIME_INSTALLERS: [&str; 17] = [
    "windowsdesktop-runtime", "dotnet-runtime", "dotnet-sdk", "aspnetcore-runtime", "dotnet-hosting", "vc_redist", "vcredist", "directx", "dxsetup", "dxwebsetup",
    "oalinst", "ue4prereq", "ue5prereq", "physx", "xnafx", "microsoftedgewebview2", "msedgewebview2",
];

/// A file name a browser gives a second download of the same file ("game (2).exe"): something the
/// player downloaded again, not the game's own program.
fn downloaded_copy(stem: &str) -> bool {
    let s = stem.trim_end();
    // "setup (2)", never "game (1993)": a browser counts up from 1, a year has four digits.
    s.ends_with(')') && s.rfind(" (").map(|i| { let n = &s[i + 2..s.len() - 1]; (1..=2).contains(&n.len()) && n.chars().all(|c| c.is_ascii_digit()) }).unwrap_or(false)
}

/// A program that belongs to Windows or to another app (Visual Studio's installer, a runtime's
/// setup, an uninstaller), never a game.
pub fn system_program(p: &Path) -> bool {
    let full = p.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
    let name = full.rsplit('/').next().unwrap_or_default().to_string();
    let places = ["/windows/system32/", "/windows/syswow64/", "/microsoft visual studio/", "/common files/", "/windowsapps/", "/microsoft/edge", "/dotnet/", "/nvidia corporation/", "/steam/steamapps/common/steamworks shared/"];
    let names = ["vs_", "vc_redist", "vcredist", "unins", "dxsetup", "dotnet", "msiexec", "explorer.exe", "cmd.exe", "powershell"];
    places.iter().any(|d| full.contains(d)) || names.iter().any(|n| name.starts_with(n))
}

pub fn looks_like_tool(rel: &Path, names: &[&str]) -> bool {
    let low = rel.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
    if RUNTIME_INSTALLERS.iter().any(|r| low.starts_with(r)) || downloaded_copy(&low) {
        return true;
    }
    let want: Vec<String> = names.iter().flat_map(|n| tokens(n)).collect();
    let stem = rel.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    // A build tool by its whole name (ninja.exe, cmake, python3.exe).
    let whole = rel.file_name().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
    let whole = whole.strip_suffix(".exe").unwrap_or(&whole);
    // A cross compiler names its target first: x86_64-w64-mingw32-clang++.exe.
    if BUILD_TOOLS.contains(&whole) || whole.starts_with("python3.") || whole.starts_with("llvm-") || whole.contains("clang") || whole.ends_with("-gcc") || whole.ends_with("-g++") {
        return true;
    }
    // A build or setup script (make_sysroot.sh, build.sh, configure, setup_deps.ps1): it makes the
    // game or what it needs, it isn't the game.
    let script = matches!(rel.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref(), Some("sh" | "bash" | "ps1" | "bat" | "cmd" | "command") | None);
    if script && ["make", "build", "configure", "bootstrap", "compile", "setup_", "setup-", "install", "gen_", "generate", "fetch", "download", "extract", "patch"].iter().any(|p| low.starts_with(p)) && !want_has_stem(&low, names) {
        return true;
    }
    // A tool word, or a name that ends in one (hactool, gt3tool), that isn't part of the game's name.
    if tokens(stem).iter().any(|t| !want.contains(t) && (TOOL_WORDS.contains(&t.as_str()) || t == "tool" || t == "tools" || t.ends_with("bktr") || (t.len() > 4 && (t.ends_with("tool") || t.ends_with("tools"))))) {
        return true;
    }
    // In a tools folder, a dedicated server's, or a hidden one (.tools, .sdk): leftovers from
    // building, or something that isn't the game (server/start.sh starts the server).
    rel.parent()
        .map(|d| {
            d.components().any(|c| {
                let n = c.as_os_str().to_string_lossy().to_lowercase();
                let hidden_or_tool = (n.starts_with('.') && n != "." && n != "..") || TOOL_DIRS.contains(&n.as_str());
                hidden_or_tool || n == "server" || n == "dedicated"
            })
        })
        .unwrap_or(false)
}

/// A script name that is the game's own name (a game called "Build" would be odd, but its name wins).
fn want_has_stem(low: &str, names: &[&str]) -> bool {
    names.iter().any(|n| tokens(n).iter().any(|t| t.len() > 3 && low == *t))
}

/// find_exe, but never a tool: when every program in it is one, there's no telling (None), and the
/// player is asked.
pub fn find_game_exe(dir: &Path, os: &str, names: &[&str]) -> Option<PathBuf> {
    find_exe(dir, os, names).filter(|e| !looks_like_tool(e, names))
}

/// Normalize a manual program selection to the executable stored in the library.
pub fn program_choice(path: &Path, os: &str) -> Option<PathBuf> {
    if os == "macos" && crate::macos::is_bundle(path) {
        return crate::macos::bundle_executable(path, crate::assets::arch_now());
    }
    path.is_file().then(|| path.to_path_buf())
}

/// A Windows script a release can start the game with (a .bat or .cmd).
pub fn is_script(p: &Path) -> bool {
    let n = p.file_name().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
    n.ends_with(".bat") || n.ends_with(".cmd")
}

/// A Linux start script ("run.sh", "launcher.sh", "Play.sh"): the release's own way in, which sets
/// what the program needs before starting it (DOTNET_ROOT for a .NET app, LD_LIBRARY_PATH, ...).
fn is_linux_script(p: &Path) -> bool {
    p.extension().map(|e| e.eq_ignore_ascii_case("sh")).unwrap_or(false)
}

/// Words in the name of a program that has to run once before the game: its installer, or the
/// first-time setup that unpacks the game's files from the player's copy.
const SETUP_WORDS: [&str; 9] = ["setup", "install", "installer", "unpack", "firstrun", "instalar", "instalador", "installation", "instalacja"];
/// Installers that come with a game but aren't for it: runtimes, drivers and uninstallers.
const NOT_A_SETUP: [&str; 14] = ["unins", "uninstall", "uninstaller", "vc", "vcredist", "redist", "dxsetup", "directx", "dotnet", "netfx", "webview2", "vulkan", "driver", "runtime"];

/// The program in a release that has to run before the game can, relative to `dir`. With no game
/// program (`game` None) it's the release's own installer (MaddenNFL08-Recompiled-Setup.exe), which
/// puts the game somewhere else. Beside a game program, it's a first-time setup made for that game
/// (Tenkaichi3Decomp-setup.exe next to Tenkaichi3Decomp.exe), or one named only "Setup".
/// A Windows Installer package (.msi).
pub fn is_msi(p: &Path) -> bool {
    p.extension().map(|e| e.eq_ignore_ascii_case("msi")).unwrap_or(false)
}

pub fn find_setup(dir: &Path, os: &str, game: Option<&Path>) -> Option<PathBuf> {
    // A release that's a Windows Installer package: that's its setup.
    if os == "windows" {
        let mut msi: Vec<PathBuf> = fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_file() && is_msi(p)).collect();
        msi.sort();
        if let Some(p) = msi.into_iter().next().and_then(|p| p.strip_prefix(dir).ok().map(Path::to_path_buf)) {
            return Some(p);
        }
    }
    let game_t: Vec<String> = game.and_then(|g| g.file_stem()).and_then(|s| s.to_str()).map(tokens).unwrap_or_default();
    let mut best: Option<(usize, PathBuf)> = None;
    let mut stack = vec![(dir.to_path_buf(), 0usize)];
    while let Some((d, depth)) = stack.pop() {
        let mut entries: Vec<_> = fs::read_dir(&d).into_iter().flatten().flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            if e.file_type().map(|t| t.is_symlink()).unwrap_or(true) || e.file_name() == crate::swap::FOLDER {
                continue;
            }
            if p.is_dir() {
                if depth < 1 {
                    stack.push((p, depth + 1));
                }
                continue;
            }
            if !(is_program(&p, os) || (os == "windows" && is_script(&p))) {
                continue;
            }
            let t = tokens(p.file_stem().and_then(|s| s.to_str()).unwrap_or(""));
            if !t.iter().any(|x| SETUP_WORDS.contains(&x.as_str())) || t.iter().any(|x| NOT_A_SETUP.iter().any(|n| x == n || x.starts_with("unins"))) {
                continue;
            }
            if game.is_some() {
                // Beside a game, only a setup named for that program (Tenkaichi3Decomp-setup next
                // to Tenkaichi3Decomp), or plain Setup. BotW-Native-Setup next to BotWLauncher
                // installs the same thing somewhere else: not needed.
                let only_setup = t.iter().all(|x| SETUP_WORDS.contains(&x.as_str()));
                let for_it = t.iter().filter(|x| !SETUP_WORDS.contains(&x.as_str())).all(|x| game_t.contains(x));
                if !only_setup && !for_it {
                    continue;
                }
            }
            if best.as_ref().map(|(b, _)| depth < *b).unwrap_or(true) {
                best = Some((depth, p));
            }
        }
    }
    best.and_then(|(_, p)| p.strip_prefix(dir).ok().map(Path::to_path_buf))
}

/// Whether a program's name says it's an installer or first-time setup (see find_setup).
pub fn looks_like_setup(p: &Path) -> bool {
    // A Windows Installer package is always one.
    if is_msi(p) {
        return true;
    }
    let t = tokens(p.file_stem().and_then(|s| s.to_str()).unwrap_or(""));
    t.iter().any(|x| SETUP_WORDS.contains(&x.as_str()))
}

pub fn is_program(p: &Path, os: &str) -> bool {
    let n = p.file_name().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
    if os == "windows" {
        return n.ends_with(".exe");
    }
    if os == "linux" && (n.ends_with(".appimage") || n.ends_with(".x86_64")) {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if os != "macos" && n.contains('.') && !n.ends_with(".sh") && !n.ends_with(".bin") {
            return false;
        }
        if !fs::metadata(p).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false) {
            return false;
        }
        // The run permission alone says little: on a memory card or a drive formatted for Windows
        // (exFAT, NTFS) every file has it. A program is a Linux one built for this PC's processor,
        // or a script that says what runs it.
        if n.ends_with(".sh") {
            return true;
        }
        let here = if crate::assets::arch_now() == "arm64" { "arm64" } else { "x64" };
        if os == "macos" {
            if crate::macos::is_executable(p, here) {
                return true;
            }
        } else if let Some(a) = crate::proton::elf_arch(p) {
            return a == here;
        }
        let mut head = [0u8; 2];
        return fs::File::open(p).and_then(|mut f| io::Read::read_exact(&mut f, &mut head)).map(|_| &head == b"#!").unwrap_or(false);
    }
    #[allow(unreachable_code)]
    false
}

/// A program that comes with a game but is never the game: an uninstaller, installer, crash
/// handler, runtime installer or one of its tools (crashpad_handler.exe, vc_redist.x64.exe,
/// x86_64-w64-mingw32-clang++.exe). A word that's in the game's own names doesn't count.
pub fn never_the_game(rel: &Path, names: &[&str]) -> bool {
    let want: Vec<String> = names.iter().flat_map(|n| tokens(n)).collect();
    let stem = rel.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    looks_like_setup(rel)
        || looks_like_tool(rel, names)
        || tokens(stem).iter().any(|x| !want.contains(x) && NOT_THE_GAME.iter().any(|n| x == n || (n.len() >= 5 && x.starts_with(n))))
}

/// The program to start, relative to `dir`: the one whose name looks most like the game's, near the
/// top of the folder, and big (the game is usually the biggest program in its folder).
pub fn find_exe(dir: &Path, os: &str, names: &[&str]) -> Option<PathBuf> {
    // A Mac app that is the game's folder itself: the program its Info.plist names, never a helper
    // inside it, and nothing when that program is missing or not for this Mac.
    if os == "macos" && crate::macos::is_bundle(dir) {
        let exe = crate::macos::bundle_executable(dir, crate::assets::arch_now())?;
        return exe.strip_prefix(dir).ok().map(Path::to_path_buf);
    }
    // An AppImage unpacked at install: its AppRun starts it, as the AppImage did. One of a release's
    // several AppImages unpacked into a folder named after it, when it's the only one.
    if os == "linux" && dir.join("AppRun").is_file() {
        return Some(PathBuf::from("AppRun"));
    }
    if os == "linux" {
        let one_down: Vec<PathBuf> = fs::read_dir(dir).into_iter().flatten().flatten().map(|e| Path::new(&e.file_name()).join("AppRun")).filter(|r| dir.join(r).is_file()).collect();
        if let [only] = one_down.as_slice() {
            return Some(only.clone());
        }
    }
    let mut want: Vec<String> = names.iter().flat_map(|n| tokens(n)).filter(|t| t.len() > 2 && t != "recomp" && t != "the").collect();
    // Initials too: VCSLauncher.exe is Vice City Stories' launcher, mcla.exe Midnight Club Los
    // Angeles.
    for n in names {
        let mut w = tokens(n);
        while w.last().map(|x| TAIL_WORDS.contains(&x.as_str())).unwrap_or(false) {
            w.pop();
        }
        if w.len() >= 2 {
            for skip_small in [false, true] {
                let a: String = w.iter().filter(|x| !(skip_small && SMALL_WORDS.contains(&x.as_str()))).filter_map(|x| x.chars().next()).collect();
                if a.len() >= 3 {
                    want.push(a);
                }
            }
        }
    }
    want.sort();
    want.dedup();
    let mut best: Option<(i64, PathBuf)> = None;
    let mut stack = vec![(dir.to_path_buf(), 0usize)];
    while let Some((d, depth)) = stack.pop() {
        for e in fs::read_dir(&d).into_iter().flatten().flatten() {
            let mut p = e.path();
            // The old release's program waits there while an update goes in: never the game.
            if e.file_type().map(|t| t.is_symlink()).unwrap_or(true) || e.file_name() == crate::swap::FOLDER {
                continue;
            }
            let bundle_name = if os == "macos" && crate::macos::is_bundle(&p) {
                let name = p.file_stem().and_then(|n| n.to_str()).unwrap_or("").to_string();
                let Some(exe) = crate::macos::bundle_executable(&p, crate::assets::arch_now()) else { continue };
                p = exe;
                Some(name)
            } else { None };
            if p.is_dir() {
                if depth < 4 {
                    stack.push((p, depth + 1));
                }
                continue;
            }
            let windows_script = os == "windows" && is_script(&p);
            if !windows_script && !is_program(&p, os) {
                continue;
            }
            let stem = bundle_name.as_deref().unwrap_or_else(|| p.file_stem().and_then(|s| s.to_str()).unwrap_or(""));
            let t = tokens(stem);
            // A word that's in the game's own name doesn't count: Crash Team Racing isn't a crash
            // handler.
            if t.iter().any(|x| !want.contains(x) && NOT_THE_GAME.iter().any(|n| x == n || (n.len() >= 5 && x.starts_with(n)))) {
                continue;
            }
            // Words of the game's names in this one's name, also run together (BotWLauncher).
            let squashed: String = stem.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
            let named = want.iter().filter(|w| t.iter().any(|x| x == *w || (x.len() > 3 && w.starts_with(x.as_str())) || (w.len() > 3 && x.starts_with(w.as_str()))) || (w.len() >= 4 && squashed.contains(w.as_str()))).count() as i64;
            let launch = t.iter().any(|x| LAUNCH_WORDS.contains(&x.as_str()));
            // A Linux start script ("run.sh") is the release's way in, like a Windows launch script.
            let linux_script = os == "linux" && is_linux_script(&p) && launch;
            let script = windows_script || linux_script;
            let mut s;
            if script {
                // A script the release starts the game with, named for it ("Launch Madden NFL
                // 08.bat"): it passes the options the game needs, so it goes ahead of the program.
                if !launch || t.iter().any(|x| NOT_A_LAUNCH.contains(&x.as_str())) {
                    continue;
                }
                // A Windows launch script has to be named for the game; a Linux one doesn't:
                // "run.sh" and "start.sh" set the runtime (.NET's DOTNET_ROOT) or the libraries the
                // program needs before starting it, so they go ahead of the program they start.
                let generic_linux = os == "linux";
                if !generic_linux && named == 0 {
                    continue;
                }
                s = if generic_linux {
                    130 - depth as i64 * 12 + named * 30
                } else {
                    44 - depth as i64 * 12 + named * 30 + 40
                };
            } else {
                let size = fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                s = (size.max(1) as f64).log2() as i64 * 2 - depth as i64 * 12 + named * 30;
                // The game's own launcher (BotWLauncher.exe) sets things up and starts the engine,
                // so it goes ahead of the engine. A bare Launcher.exe doesn't, and neither does a
                // quick-play program ("Play inFAMOUS Second Son.exe" skips the launcher it needs).
                if named > 0 && t.iter().any(|x| x == "launcher" || x == "launch") {
                    s += 40;
                }
                // The old version kept beside the new one.
                if t.iter().any(|x| OLD_WORDS.contains(&x.as_str())) {
                    s -= 15;
                }
                if t.first().map(|x| QUICK_WORDS.contains(&x.as_str())).unwrap_or(false) {
                    s -= 10;
                }
                // A command-line build beside the windowed one (eden-cli.exe next to eden.exe):
                // it needs options the player would have to type, so the windowed one goes first.
                if t.last().map(|x| x == "cli" || x == "cmd" || x == "console").unwrap_or(false) {
                    s -= 25;
                }
            }
            if t.iter().any(|x| x == "recompiled" || x == "recomp") {
                s += 10;
            }
            // One of its tools: behind every program that isn't.
            if p.strip_prefix(dir).map(|r| looks_like_tool(r, names)).unwrap_or(false) {
                s -= 1000;
            }
            if best.as_ref().map(|(b, _)| s > *b).unwrap_or(true) {
                best = Some((s, p));
            }
        }
    }
    // Only tools left (a compiler, BusyBox's programs): none of them is the game, so the player
    // is asked rather than a tool started.
    best.and_then(|(_, p)| p.strip_prefix(dir).ok().map(Path::to_path_buf)).filter(|r| !looks_like_tool(r, names))
}

#[cfg(test)]
mod tests {
    #[test]
    fn programs_from_windows_or_other_apps_are_told_apart() {
        assert!(super::system_program(std::path::Path::new(r"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vs_layout.exe")));
        assert!(super::system_program(std::path::Path::new(r"C:\Windows\System32\notepad.exe")));
        assert!(super::system_program(std::path::Path::new(r"D:\Games\X\unins000.exe")));
        assert!(super::system_program(std::path::Path::new(r"D:\Games\X\vc_redist.x64.exe")));
        assert!(!super::system_program(std::path::Path::new(r"C:\Recomp Library\hopper\Hopper.exe")));
        assert!(!super::system_program(std::path::Path::new("/home/me/Games/SkyRacer/SkyRacer.AppImage")));
    }

    #[test]
    fn a_settings_program_beside_the_game_is_never_the_game() {
        let d = std::env::temp_dir().join(format!("rl-settings-exe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("NightWatchSettings.exe"), vec![0u8; 900_000]).unwrap();
        std::fs::write(d.join("NightWatch.exe"), vec![0u8; 400_000]).unwrap();
        std::fs::write(d.join("GameOptions.exe"), vec![0u8; 900_000]).unwrap();
        let got = find_exe(&d, "windows", &["Harbor"]).map(|p| p.to_string_lossy().to_string());
        assert_eq!(got.as_deref(), Some("NightWatch.exe"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn a_linux_start_script_goes_ahead_of_the_program_it_sets_up() {
        use std::os::unix::fs::PermissionsExt;

        fn program(p: &Path, bytes: &[u8]) {
            fs::write(p, bytes).unwrap();
            fs::set_permissions(p, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let d = tmp("linux-dotnet");
        // A .NET app host and its managed program, beside the release's run.sh: the script finds
        // the player's .NET runtime (DOTNET_ROOT) and then starts the app host.
        let mut elf = vec![0u8; 78_192];
        elf[..4].copy_from_slice(b"\x7fELF");
        elf[18] = if crate::assets::arch_now() == "arm64" { 0xb7 } else { 0x3e };
        program(&d.join("sotn"), &elf);
        sparse(&d.join("sotn.dll"), 80_000_000);
        program(&d.join("run.sh"), b"#!/usr/bin/env bash\nexec \"$DIR/sotn\"\n");
        assert_eq!(find_exe(&d, "linux", &["Symphony Recomp", "symphonyrecomp"]).unwrap(), PathBuf::from("run.sh"));

        // A diagnostic script isn't how the game starts; the program still wins.
        let d = tmp("linux-diagnostic");
        program(&d.join("Game"), &elf);
        program(&d.join("run debug.sh"), b"#!/bin/sh\nexec ./Game\n");
        assert_eq!(find_exe(&d, "linux", &["Game"]).unwrap(), PathBuf::from("Game"));

        // A dedicated server's start script starts the server, not the game.
        let d = tmp("linux-server");
        program(&d.join("Game"), &elf);
        fs::create_dir_all(d.join("server")).unwrap();
        program(&d.join("server/start.sh"), b"#!/bin/sh\nexec ./game-server\n");
        assert_eq!(find_exe(&d, "linux", &["Game"]).unwrap(), PathBuf::from("Game"));
    }

    #[test]
    fn a_disk_images_window_dressing_stays_behind() {
        for n in [".background", ".DS_Store", ".fseventsd", "Applications", "VolumeIcon.icns", "Icon\r"] {
            assert!(super::dmg_extra(n), "{n}");
        }
        for n in ["LAS Recomp.app", "README.txt", "mods"] {
            assert!(!super::dmg_extra(n), "{n}");
        }
    }

    use super::*;

    #[test]
    fn download_speed_follows_the_limit_and_play() {
        assert_eq!(rate_for(0, false), 0);
        assert_eq!(rate_for(0, true), PLAYING_RATE);
        assert_eq!(rate_for(1 << 20, true), 1 << 20);
        assert_eq!(rate_for(50 << 20, true), PLAYING_RATE);
        assert_eq!(rate_for(50 << 20, false), 50 << 20);
        pause("x");
        assert!(paused("x") && !paused("y"));
        unpause("x");
        assert!(!paused("x"));
    }

    #[test]
    fn busy_answers_are_asked_again() {
        assert!(busy_answer(504) && busy_answer(503) && busy_answer(429));
        assert!(!busy_answer(404) && !busy_answer(403) && !busy_answer(416));
    }
    use std::io::Write as _;

    fn tmp(name: &str) -> PathBuf {
        // The same folder each run (cleared first), so test runs don't pile up in the temp folder.
        let d = std::env::temp_dir().join(format!("rl-test-{name}"));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// A time after every file a test just wrote: none of them changed since that install.
    fn later() -> u64 {
        crate::library::now() + 60
    }

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn updating_keeps_an_external_appimage() {
        let d = tmp("update-external-appimage");
        let target = d.join("managed");
        fs::create_dir_all(&target).unwrap();
        let old = d.join("My Game.AppImage");
        fs::write(&old, b"player's separate program").unwrap();
        fs::write(target.join("Game-v2.AppImage"), b"new release").unwrap();

        let (got, _) = updated_exe("game", &target, Some(old.clone()), &paths(&["Game-v2.AppImage"]), true);

        assert_eq!(fs::read(&old).unwrap(), b"player's separate program");
        assert_eq!(got, Some(old));
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn updating_keeps_an_external_exe_with_the_same_name() {
        let d = tmp("update-external-exe");
        let target = d.join("managed");
        fs::create_dir_all(target.join("bin")).unwrap();
        let old = d.join("Game.exe");
        fs::write(&old, b"player's separate program").unwrap();
        fs::write(target.join("bin/Game.exe"), b"new release").unwrap();

        let (got, _) = updated_exe("game", &target, Some(old.clone()), &paths(&["bin/Game.exe"]), true);

        assert_eq!(got, Some(old.clone()));
        assert_eq!(fs::read(old).unwrap(), b"player's separate program");
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn updating_keeps_an_appimage_reached_through_parent_components() {
        let d = tmp("update-parent-appimage");
        let target = d.join("managed");
        fs::create_dir_all(&target).unwrap();
        fs::write(d.join("My Game.AppImage"), b"player's separate program").unwrap();
        fs::write(target.join("Game-v2.AppImage"), b"new release").unwrap();
        let old = PathBuf::from("../My Game.AppImage");

        let (got, _) = updated_exe("game", &target, Some(old.clone()), &paths(&["Game-v2.AppImage"]), true);

        assert_eq!(fs::read(d.join("My Game.AppImage")).unwrap(), b"player's separate program");
        assert_eq!(got, Some(old));
        fs::remove_dir_all(d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn updating_keeps_an_appimage_in_an_external_linked_folder() {
        let d = tmp("update-linked-appimage");
        let target = d.join("managed");
        let external = d.join("external");
        fs::create_dir_all(&target).unwrap();
        fs::create_dir_all(&external).unwrap();
        fs::write(external.join("Game.AppImage"), b"player's separate program").unwrap();
        fs::write(target.join("Game-v2.AppImage"), b"new release").unwrap();
        std::os::unix::fs::symlink(&external, target.join("linked")).unwrap();
        let old = PathBuf::from("linked/Game.AppImage");

        let (got, _) = updated_exe("game", &target, Some(old.clone()), &paths(&["Game-v2.AppImage"]), true);

        assert_eq!(fs::read(external.join("Game.AppImage")).unwrap(), b"player's separate program");
        assert_eq!(got, Some(old));
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn updating_replaces_a_managed_appimage() {
        let target = tmp("update-managed-appimage");
        fs::write(target.join("Game-v1.AppImage"), b"old release").unwrap();
        fs::write(target.join("Game-v2.AppImage"), b"new release").unwrap();

        let (got, replaced) = updated_exe("game", &target, Some("Game-v1.AppImage".into()), &paths(&["Game-v2.AppImage"]), true);

        assert_eq!(got, Some("Game-v2.AppImage".into()));
        assert_eq!(replaced, Some("Game-v1.AppImage".into()));
        assert_eq!(fs::read(target.join("Game-v2.AppImage")).unwrap(), b"new release");
        fs::remove_dir_all(target).unwrap();
    }

    #[test]
    fn updating_follows_a_managed_program_into_its_new_folder() {
        let target = tmp("update-managed-exe");
        fs::create_dir_all(target.join("bin")).unwrap();
        fs::write(target.join("Game.exe"), b"old release").unwrap();
        fs::write(target.join("bin/Game.exe"), b"new release").unwrap();

        let (got, _) = updated_exe("game", &target, Some("Game.exe".into()), &paths(&["bin/Game.exe"]), true);

        assert_eq!(got, Some("bin/Game.exe".into()));
        assert_eq!(fs::read(target.join("bin/Game.exe")).unwrap(), b"new release");
        fs::remove_dir_all(target).unwrap();
    }

    #[test]
    fn an_update_takes_away_a_file_its_release_dropped() {
        let d = tmp("dropped-gone");
        fs::create_dir_all(d.join("runtime")).unwrap();
        fs::write(d.join("runtime/gfx_present.c"), b"old").unwrap();
        fs::write(d.join("runtime/gfx_window.c"), b"new").unwrap();
        let old = paths(&["runtime/gfx_present.c", "runtime/gfx_window.c"]);
        let new = paths(&["runtime/gfx_window.c"]);
        let gone = dropped(&d, &old, &new, later(), None);
        assert_eq!(gone, paths(&["runtime/gfx_present.c"]));
    }

    #[test]
    fn an_update_keeps_a_dropped_file_changed_since_the_install() {
        let d = tmp("dropped-changed");
        fs::write(d.join("game.ini"), b"edited by the player").unwrap();
        let installed_long_ago = crate::library::now() - 3600;
        let gone = dropped(&d, &paths(&["game.ini"]), &[], installed_long_ago, None);
        assert!(gone.is_empty());
        assert!(d.join("game.ini").exists());
    }

    #[test]
    fn an_update_keeps_the_program_the_player_starts() {
        let d = tmp("dropped-exe");
        fs::write(d.join("Game.exe"), b"exe").unwrap();
        let gone = dropped(&d, &paths(&["Game.exe"]), &[], later(), Some(Path::new("Game.exe")));
        assert!(gone.is_empty());
        assert!(d.join("Game.exe").exists());
    }

    #[test]
    fn an_update_keeps_a_file_only_renamed_in_case() {
        let d = tmp("dropped-case");
        fs::write(d.join("Engine.dll"), b"dll").unwrap();
        let gone = dropped(&d, &paths(&["Engine.dll"]), &paths(&["engine.DLL"]), later(), None);
        assert!(gone.is_empty());
        assert!(d.join("Engine.dll").exists());
    }

    #[test]
    fn an_update_never_removes_outside_the_game_folder() {
        let d = tmp("dropped-outside");
        fs::create_dir_all(d.join("game")).unwrap();
        fs::write(d.join("other.txt"), b"not the game's").unwrap();
        let gone = dropped(&d.join("game"), &paths(&["../other.txt"]), &[], later(), None);
        assert!(gone.is_empty());
        assert!(d.join("other.txt").exists());
    }

    fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, bytes) in files {
            z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(bytes).unwrap();
        }
        z.finish().unwrap().into_inner()
    }

    #[test]
    fn a_game_zip_beside_a_rom_is_unpacked_and_the_rom_kept() {
        // SMW Remastered Plus's Mac download: a folder with baserom.sfc and the game's ZIP.
        let d = tmp("mac-zip-beside");
        fs::create_dir_all(d.join("stage")).unwrap();
        let from = d.join("from");
        fs::create_dir_all(&from).unwrap();
        fs::write(from.join("baserom.sfc"), b"rom").unwrap();
        fs::write(from.join("Game.zip"), zip_of(&[("Game.app/Contents/Info.plist", b"info"), ("Game.app/Contents/MacOS/Game", b"program")])).unwrap();
        let out = unwrap_mac_zip(from.clone(), &d.join("stage")).unwrap();
        assert_eq!(out, from);
        assert!(from.join("Game.app/Contents/Info.plist").is_file());
        assert_eq!(fs::read(from.join("baserom.sfc")).unwrap(), b"rom");
        assert!(!from.join("Game.zip").exists());
        // The same files at the top of the download, with nothing around them: no unpacking layer left in.
        let stage = d.join("top");
        fs::create_dir_all(&stage).unwrap();
        fs::write(stage.join("baserom.sfc"), b"rom").unwrap();
        fs::write(stage.join("Game.zip"), zip_of(&[("Game.app/Contents/Info.plist", b"info")])).unwrap();
        let out = unwrap_mac_zip(stage.clone(), &stage).unwrap();
        let mut names: Vec<String> = fs::read_dir(&out).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, vec!["Game.app", "baserom.sfc"]);
    }

    #[test]
    fn a_zip_beside_a_program_is_game_data_and_stays() {
        let d = tmp("mac-zip-data");
        fs::create_dir_all(d.join("stage")).unwrap();
        let from = d.join("from");
        fs::create_dir_all(&from).unwrap();
        // A 64-bit Mach-O executable header for this processor.
        let mut h = [0u8; 32];
        h[..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
        h[4..8].copy_from_slice(&(if crate::assets::arch_now() == "arm64" { 0x0100000cu32 } else { 0x01000007u32 }).to_le_bytes());
        h[12..16].copy_from_slice(&2u32.to_le_bytes());
        fs::write(from.join("game"), h).unwrap();
        fs::write(from.join("data.zip"), zip_of(&[("level.bin", b"level")])).unwrap();
        let out = unwrap_mac_zip(from.clone(), &d.join("stage")).unwrap();
        assert!(out.join("data.zip").is_file());
        assert!(!out.join("level.bin").exists());
        // Two ZIPs and nothing else is no wrapper either.
        let two = d.join("two");
        fs::create_dir_all(&two).unwrap();
        fs::write(two.join("a.zip"), zip_of(&[("a", b"a")])).unwrap();
        fs::write(two.join("b.zip"), zip_of(&[("b", b"b")])).unwrap();
        assert!(unwrap_mac_zip(two.clone(), &d.join("stage")).unwrap().join("a.zip").is_file());
    }

    fn put(p: &Path, bytes: usize) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, vec![7u8; bytes]).unwrap();
    }

    #[test]
    fn the_windowed_program_goes_ahead_of_its_command_line_build() {
        let d = tmp("cli-build");
        put(&d.join("eden.exe"), 8_000_000);
        put(&d.join("eden-cli.exe"), 9_000_000);
        put(&d.join("crashpad_handler.exe"), 12_000_000);
        assert_eq!(find_exe(&d, "windows", &["Eden"]).unwrap(), PathBuf::from("eden.exe"));
        // Never the game, whatever its size; a word in the game's own name doesn't count.
        assert!(never_the_game(Path::new("crashpad_handler.exe"), &["NX2navi"]));
        assert!(never_the_game(Path::new("DKB-Setup.exe"), &["Donkey Kong"]));
        assert!(never_the_game(Path::new("bin/x86_64-w64-mingw32-clang++.exe"), &["DKB"]));
        assert!(!never_the_game(Path::new("NX2navi.exe"), &["NX2navi"]));
        assert!(!never_the_game(Path::new("CrashTagTeam.exe"), &["Crash Tag Team Racing"]));
        let _ = fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn links_that_point_nowhere_are_dropped() {
        let d = tmp("dead-links");
        fs::create_dir_all(d.join("Game.app/Contents/Frameworks/X.framework/Versions/A")).unwrap();
        fs::write(d.join("Game.app/Contents/Frameworks/X.framework/Versions/A/X"), b"lib").unwrap();
        std::os::unix::fs::symlink("A", d.join("Game.app/Contents/Frameworks/X.framework/Versions/Current")).unwrap();
        std::os::unix::fs::symlink("Versions/B/X", d.join("Game.app/Contents/Frameworks/X.framework/X")).unwrap();
        std::os::unix::fs::symlink("loop", d.join("Game.app/loop")).unwrap();
        validate_links(&d, &d).unwrap();
        assert!(fs::symlink_metadata(d.join("Game.app/Contents/Frameworks/X.framework/Versions/Current")).is_ok());
        assert!(fs::symlink_metadata(d.join("Game.app/Contents/Frameworks/X.framework/X")).is_err());
        assert!(fs::symlink_metadata(d.join("Game.app/loop")).is_err());
        // A link out of the folder still stops it.
        std::os::unix::fs::symlink("/etc", d.join("out")).unwrap();
        assert!(validate_links(&d, &d).is_err());
    }

    #[test]
    fn build_scripts_are_never_the_game() {
        for p in ["src/scripts/make_sysroot.sh", "build.sh", "configure", "setup_deps.ps1", "install.sh", "scripts/run.sh"] {
            assert!(looks_like_tool(Path::new(p), &["Harmony Recomp"]), "{p}");
        }
        assert!(!looks_like_tool(Path::new("harmony.sh"), &["Harmony Recomp"]));
    }

    #[test]
    fn a_tool_is_never_picked_as_the_game() {
        let d = tmp("only-tools");
        put(&d.join("toolchain/bin/clang-23.exe"), 4000);
        put(&d.join("busybox/bin/watch.exe"), 4000);
        assert_eq!(find_exe(&d, "windows", &["Racing 8 DX Recomp"]), None);
        put(&d.join("kartrecomp.exe"), 2000);
        assert_eq!(find_exe(&d, "windows", &["Racing 8 DX Recomp"]), Some(PathBuf::from("kartrecomp.exe")));
        assert!(looks_like_tool(Path::new("busybox/bin/watch.exe"), &["YW4 Recomp"]));
        assert!(looks_like_tool(Path::new("toolchain/bin/game.exe"), &["Game"]));
    }

    #[test]
    fn unsafe_names_are_dropped() {
        assert!(inside("../evil.dll").is_none());
        assert!(inside("/etc/passwd").is_none());
        assert!(inside("a/../../b").is_none());
        assert_eq!(inside("Game/./bin\\x.exe").unwrap(), PathBuf::from("Game/bin/x.exe"));
    }

    #[test]
    fn zip_with_one_folder_on_top_is_unwrapped_and_updates_keep_saves() {
        let d = tmp("zip");
        let zpath = d.join("Game-Windows.zip");
        {
            let mut z = zip::ZipWriter::new(fs::File::create(&zpath).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            z.add_directory("Game-v2/", o).unwrap();
            z.start_file("Game-v2/Game.exe", o).unwrap();
            z.write_all(b"new").unwrap();
            z.start_file("Game-v2/data/assets.bin", o).unwrap();
            z.write_all(b"data").unwrap();
            z.start_file("../escape.txt", o).unwrap();
            z.write_all(b"no").unwrap();
            z.finish().unwrap();
        }
        let target = d.join("lib/game");
        put(&target.join("Game.exe"), 3);
        put(&target.join("saves/slot1.sav"), 10);
        let files = unpack(&zpath, &target, &d.join("staging")).unwrap();
        assert_eq!(fs::read(target.join("Game.exe")).unwrap(), b"new");
        assert!(target.join("data/assets.bin").is_file());
        assert!(target.join("saves/slot1.sav").is_file(), "an update must not delete saves");
        assert!(!d.join("lib/escape.txt").exists() && !d.join("escape.txt").exists());
        assert_eq!(files.len(), 2);
        assert!(!d.join("staging").exists());
    }

    #[test]
    fn a_staged_update_goes_in_whole_and_keeps_saves() {
        let d = tmp("staged-update");
        let zpath = d.join("Game-v2.zip");
        fs::write(&zpath, zip_of(&[("Game-v2/Game.exe", b"new"), ("Game-v2/data/assets.bin", b"data")])).unwrap();
        let target = d.join("lib/game");
        fs::create_dir_all(target.join("saves")).unwrap();
        fs::write(target.join("Game.exe"), b"old").unwrap();
        fs::write(target.join("old-only.dll"), b"old").unwrap();
        fs::write(target.join("saves/slot1.sav"), b"save").unwrap();
        let staging = d.join("lib/.staging/game");

        let staged = stage(&zpath, &staging).unwrap();
        assert_eq!(fs::read(target.join("Game.exe")).unwrap(), b"old", "unpacking alone leaves the game as it is");
        let old = paths(&["Game.exe", "old-only.dll"]);
        let gone = dropped(&target, &old, &staged.files, later(), None);
        crate::swap::put_in_place(&target, &staged.root, &staged.files, "sha", &gone, &old).unwrap().finish();

        assert_eq!(fs::read(target.join("Game.exe")).unwrap(), b"new");
        assert_eq!(fs::read(target.join("data/assets.bin")).unwrap(), b"data");
        assert_eq!(fs::read(target.join("saves/slot1.sav")).unwrap(), b"save");
        assert!(!target.join("old-only.dll").exists());
        assert!(!target.join(crate::swap::FOLDER).exists());
    }

    #[test]
    fn the_old_release_waiting_in_an_update_is_never_the_game() {
        let target = tmp("swap-folder-not-the-game");
        put(&target.join(crate::swap::FOLDER).join("old/SkyRacer.exe"), 4096);
        put(&target.join("Readme.txt"), 10);
        assert_eq!(find_exe(&target, "windows", &["SkyRacer"]), None);
    }

    #[test]
    fn tar_gz_unpacks() {
        let d = tmp("tgz");
        let tpath = d.join("Game-Linux.tar.gz");
        {
            let gz = flate2::write::GzEncoder::new(fs::File::create(&tpath).unwrap(), flate2::Compression::fast());
            let mut t = tar::Builder::new(gz);
            let mut h = tar::Header::new_gnu();
            // A Linux program's header: ELF, built for x86-64 (or this PC's ARM64).
            let mut elf = [0u8; 20];
            elf[..4].copy_from_slice(b"\x7fELF");
            elf[18] = if crate::assets::arch_now() == "arm64" { 0xB7 } else { 0x3E };
            h.set_size(20);
            h.set_mode(0o755);
            h.set_cksum();
            t.append_data(&mut h, "Game/Game", &elf[..]).unwrap();
            let mut r = tar::Header::new_gnu();
            r.set_size(6);
            r.set_mode(0o755);
            r.set_cksum();
            t.append_data(&mut r, "Game/LICENSE", &b"MIT..."[..]).unwrap();
            t.into_inner().unwrap().finish().unwrap();
        }
        let target = d.join("lib/game");
        unpack(&tpath, &target, &d.join("st")).unwrap();
        assert!(target.join("Game").is_file());
        #[cfg(unix)]
        assert_eq!(find_exe(&target, "linux", &["Game"]).unwrap(), PathBuf::from("Game"));
        // An AppImage unpacked at install: AppRun starts it.
        #[cfg(unix)]
        {
            fs::write(target.join("AppRun"), b"#!/bin/sh\n").unwrap();
            assert_eq!(find_exe(&target, "linux", &["Game"]).unwrap(), PathBuf::from("AppRun"));
            fs::remove_file(target.join("AppRun")).unwrap();
        }
        #[cfg(unix)]
        assert!(!is_program(&target.join("LICENSE"), "linux"));
    }

    /// A tarball with one program in a folder, run permission and all.
    fn tarball_into(w: impl io::Write) {
        let mut t = tar::Builder::new(w);
        let mut h = tar::Header::new_gnu();
        h.set_size(10);
        h.set_mode(0o755);
        h.set_cksum();
        t.append_data(&mut h, "Game/run.sh", &b"#!/bin/sh\n"[..]).unwrap();
        t.finish().unwrap();
    }

    #[test]
    fn tar_xz_unpacks() {
        let d = tmp("txz");
        let tpath = d.join("Game-Linux.tar.xz");
        let mut xz = xz2::write::XzEncoder::new(fs::File::create(&tpath).unwrap(), 1);
        tarball_into(&mut xz);
        xz.finish().unwrap();
        let staged = stage(&tpath, &d.join("st")).unwrap();
        assert_eq!(staged.files, paths(&["run.sh"]));
        assert_eq!(fs::read(staged.root.join("run.sh")).unwrap(), b"#!/bin/sh\n");
    }

    #[test]
    fn tar_zst_unpacks() {
        let d = tmp("tzst");
        let tpath = d.join("Game-Linux.tar.zst");
        let mut zst = zstd::stream::write::Encoder::new(fs::File::create(&tpath).unwrap(), 1).unwrap();
        tarball_into(&mut zst);
        zst.finish().unwrap();
        let staged = stage(&tpath, &d.join("st")).unwrap();
        assert_eq!(staged.files, paths(&["run.sh"]));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_release_appimage_is_unpacked_and_never_kept() {
        let d = tmp("stage-appimage");
        let image = d.join("Game-1.0-x86_64.AppImage");
        fs::write(&image, crate::appimage::tests::game_image()).unwrap();
        let staged = stage(&image, &d.join("st")).unwrap();
        assert!(staged.files.contains(&PathBuf::from("AppRun")));
        assert!(staged.files.contains(&PathBuf::from("usr/bin/game")));
        assert!(!staged.files.iter().any(|f| f.to_string_lossy().ends_with(".AppImage")));
        assert_eq!(find_exe(&staged.root, "linux", &["Game"]), Some(PathBuf::from("AppRun")));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_release_dwarfs_appimage_is_unpacked_too() {
        let d = tmp("stage-dwarfs-appimage");
        let image = d.join("Game-anylinux-x86_64.AppImage");
        fs::write(&image, crate::appimage::tests::dwarfs_game_image()).unwrap();
        let staged = stage(&image, &d.join("st")).unwrap();
        assert!(staged.files.contains(&PathBuf::from("usr/bin/game")));
        assert!(!staged.files.iter().any(|f| f.to_string_lossy().ends_with(".AppImage")));
        assert_eq!(find_exe(&staged.root, "linux", &["Game"]), Some(PathBuf::from("AppRun")));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn an_appimage_in_a_zip_unpacks_beside_the_rest() {
        let d = tmp("stage-appimage-zip");
        let image = crate::appimage::tests::game_image();
        let zpath = d.join("Game-linux.zip");
        fs::write(&zpath, zip_of(&[("Game/Game-x86_64.AppImage", image.as_slice()), ("Game/README.txt", b"read me")])).unwrap();
        let staged = stage(&zpath, &d.join("st")).unwrap();
        assert!(staged.files.contains(&PathBuf::from("AppRun")));
        assert!(staged.files.contains(&PathBuf::from("README.txt")));
        assert!(!staged.files.contains(&PathBuf::from("Game-x86_64.AppImage")));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn several_appimages_each_unpack_into_a_folder_of_their_own() {
        let d = tmp("stage-appimages");
        let image = crate::appimage::tests::game_image();
        let zpath = d.join("Game-linux.zip");
        fs::write(&zpath, zip_of(&[("Game.AppImage", image.as_slice()), ("Editor.AppImage", image.as_slice())])).unwrap();
        let staged = stage(&zpath, &d.join("st")).unwrap();
        assert!(staged.files.contains(&PathBuf::from("Game/AppRun")));
        assert!(staged.files.contains(&PathBuf::from("Editor/AppRun")));
        assert!(!staged.files.iter().any(|f| f.to_string_lossy().ends_with(".AppImage")));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn an_appimage_that_wont_unpack_stops_the_install() {
        let d = tmp("stage-bad-appimage");
        let image = d.join("Game-x86_64.AppImage");
        fs::write(&image, b"#!/bin/sh\necho it's a script\n").unwrap();
        let Err(Error::Msg(m)) = stage(&image, &d.join("st")) else { panic!("it must not install") };
        assert!(m.starts_with(APPIMAGE_UNPACK), "{m}");
    }

    #[test]
    fn an_update_from_an_old_appimage_install_starts_its_apprun() {
        let target = tmp("update-unpacked");
        fs::write(target.join("Game-v1.AppImage"), b"old release").unwrap();
        let (got, replaced) = updated_exe("game", &target, Some("Game-v1.AppImage".into()), &paths(&["AppRun", "usr/bin/game"]), true);
        assert_eq!(got, Some("AppRun".into()));
        assert_eq!(replaced, Some("Game-v1.AppImage".into()));
    }

    #[test]
    fn the_apprun_of_an_unpacked_appimage_is_found_among_the_files() {
        assert_eq!(unpacked_start(&paths(&["AppRun", "Tool/AppRun"]), Path::new("Game.AppImage")), Some("AppRun".into()));
        assert_eq!(unpacked_start(&paths(&["Game/AppRun", "Tool/AppRun"]), Path::new("Game.AppImage")), Some("Game/AppRun".into()));
        assert_eq!(unpacked_start(&paths(&["Other/AppRun"]), Path::new("Game.AppImage")), Some("Other/AppRun".into()));
        assert_eq!(unpacked_start(&paths(&["A/AppRun", "B/AppRun"]), Path::new("Game.AppImage")), None);
    }

    #[test]
    fn a_mod_goes_into_the_game_whole_and_leaves_its_files() {
        let d = tmp("mod-whole");
        let target = d.join("game");
        fs::create_dir_all(target.join("mods")).unwrap();
        fs::write(target.join("mods/theirs.pak"), b"player's").unwrap();
        let zpath = d.join("mod.zip");
        fs::write(&zpath, zip_of(&[("mods/new.pak", b"new mod")])).unwrap();
        let files = unpack_as_is(&zpath, &target, &d.join("st")).unwrap();
        assert_eq!(files, paths(&["mods/new.pak"]));
        assert_eq!(fs::read(target.join("mods/new.pak")).unwrap(), b"new mod");
        assert_eq!(fs::read(target.join("mods/theirs.pak")).unwrap(), b"player's");
        assert!(!target.join(crate::swap::FOLDER).exists());
    }

    #[test]
    fn a_mod_never_sets_aside_a_folder_of_the_players_files() {
        let d = tmp("mod-folder-in-the-way");
        let target = d.join("game");
        fs::create_dir_all(target.join("config")).unwrap();
        fs::write(target.join("config/mine.ini"), b"player's").unwrap();
        let zpath = d.join("mod.zip");
        fs::write(&zpath, zip_of(&[("config", b"a file where the game has a folder")])).unwrap();
        assert!(unpack_as_is(&zpath, &target, &d.join("st")).is_err());
        assert_eq!(fs::read(target.join("config/mine.ini")).unwrap(), b"player's");
        assert!(!target.join(crate::swap::FOLDER).exists());
    }

    #[test]
    fn a_mod_isnt_put_in_while_an_update_waits_to_be_settled() {
        let d = tmp("mod-waits");
        let target = d.join("game");
        fs::create_dir_all(target.join(crate::swap::FOLDER)).unwrap();
        fs::write(target.join(crate::swap::FOLDER).join("update.json"), b"{}").unwrap();
        let zpath = d.join("mod.zip");
        fs::write(&zpath, zip_of(&[("new.pak", b"new mod")])).unwrap();
        assert!(unpack_as_is(&zpath, &target, &d.join("st")).is_err());
        assert!(!target.join("new.pak").exists());
    }

    #[test]
    fn picks_the_game_not_the_helpers() {
        let d = tmp("exe");
        put(&d.join("unins000.exe"), 900_000);
        put(&d.join("vc_redist.x64.exe"), 9_000_000);
        put(&d.join("crashpad_handler.exe"), 2_000_000);
        put(&d.join("Zelda64Recompiled.exe"), 30_000_000);
        put(&d.join("tools/converter.exe"), 40_000_000);
        assert_eq!(find_exe(&d, "windows", &["Zelda 64: Recompiled", "Zelda64Recomp"]).unwrap(), PathBuf::from("Zelda64Recompiled.exe"));
        let d = tmp("exe2");
        put(&d.join("bin/Launcher.exe"), 1_000_000);
        put(&d.join("bin/SonicUnleashedRecomp.exe"), 20_000_000);
        assert_eq!(find_exe(&d, "windows", &["Unleashed Recompiled", "UnleashedRecomp"]).unwrap(), PathBuf::from("bin/SonicUnleashedRecomp.exe"));
        assert!(find_exe(&tmp("empty"), "windows", &["x"]).is_none());
    }

    /// Against a site that slows each connection after its first few megabytes (a stand-in
    /// server, `RL_SPLIT_URL`): the file arrives whole, and quickly.
    #[tokio::test]
    #[ignore]
    async fn split_download_keeps_its_speed() {
        let Ok(url) = std::env::var("RL_SPLIT_URL") else { return };
        let dir = tmp("split");
        let http = reqwest::Client::new();
        let t = Instant::now();
        let got = download_split(&http, &url, "big.bin", &dir, "t", Arc::new(AtomicBool::new(false)), &|_| {}).await.unwrap().unwrap();
        eprintln!("split download: {:.1} s, sha256 {}", t.elapsed().as_secs_f64(), got.sha256);
        assert_eq!(got.sha256, std::env::var("RL_SPLIT_SHA").unwrap());
    }

    #[test]
    fn tools_are_not_the_game() {
        // A translator in bin, bigger than the game.
        let d = tmp("tool1");
        put(&d.join("bin/xcx_xlate.exe"), 60_000_000);
        put(&d.join("xcx.exe"), 20_000_000);
        assert_eq!(find_game_exe(&d, "windows", &["XCX Recomp", "xcx-recomp"]).unwrap(), PathBuf::from("xcx.exe"));
        // A shader builder in deps.
        let d = tmp("tool2");
        put(&d.join("deps/shaders/ultrs-shaders.exe"), 50_000_000);
        put(&d.join("UltraTears.exe"), 10_000_000);
        assert_eq!(find_game_exe(&d, "windows", &["UltraTears", "ultratears"]).unwrap(), PathBuf::from("UltraTears.exe"));
        // Only a compiler: no telling, the player picks.
        let d = tmp("tool3");
        put(&d.join("compiler/xbe_recomp.exe"), 5_000_000);
        assert!(find_game_exe(&d, "windows", &["007 Nightfire XBE Recomp", "007Nightfire-XBERecomp"]).is_none());
        // A dedicated server beside the game; a server whose name is the game's stays a candidate.
        let d = tmp("tool4");
        put(&d.join("ttt2-server.exe"), 9_000_000);
        put(&d.join("ttt2.exe"), 8_000_000);
        assert_eq!(find_game_exe(&d, "windows", &["TTT2 PC", "tekken-tag-tournament-2-pc"]).unwrap(), PathBuf::from("ttt2.exe"));
        assert!(!looks_like_tool(Path::new("ServerRecomp.exe"), &["Server Recomp"]));
        // A game kept in a folder called build isn't a tool; one in temp is.
        assert!(!looks_like_tool(Path::new("build/totk.exe"), &["TotK Recomp"]));
        assert!(looks_like_tool(Path::new("temp/zelda totk-1/build/totk.exe"), &["TotK Recomp"]));
        assert!(looks_like_tool(Path::new("hactool.exe"), &["XCX Recomp"]));
        // From players' logs: a patcher, a disc converter, a cross compiler.
        assert!(looks_like_tool(Path::new("Sf2Bktr.exe"), &["SF2Recomp"]));
        assert!(looks_like_tool(Path::new("chd2gdi.exe"), &["Street-Alpha-3-Dreamcast-Recomp"]));
        assert!(looks_like_tool(Path::new("x86_64-w64-mingw32-clang++.exe"), &["TotK Recomp"]));
        assert!(looks_like_tool(Path::new("windowsdesktop-runtime-10.0.14-win-x64 (2).exe"), &["Mario Kart WiiCompiled"]));
        assert!(looks_like_tool(Path::new("wps_wid.cid-642543927.1788466613 (8).exe"), &["ReStreet"]));
        // A year in the name is the game's, not a browser's count.
        assert!(!looks_like_tool(Path::new("Doom (1993).exe"), &["Doom"]));
        assert!(looks_like_tool(Path::new("VC_redist.x64.exe"), &["Any"]));
        assert!(!looks_like_tool(Path::new("Dragon Ball Z (Burst Limit).exe"), &["Dragon Ball Z"]));
        // A tools folder in Portuguese, and recompilers.
        assert!(looks_like_tool(Path::new("ferramentas/bin/unzbic.exe"), &["ocarina-recomp"]));
        assert!(looks_like_tool(Path::new("nxlift.exe"), &["pokopia-native"]));
        assert!(looks_like_tool(Path::new("xbe_recomp.exe"), &["Live Recompiled"]));

        assert!(looks_like_tool(Path::new("bin/ninja.exe"), &["XCX Recomp"]));
        assert!(looks_like_tool(Path::new("toolchain/python3.12"), &["Some Game"]));
        assert!(!looks_like_tool(Path::new("Ninja Gaiden.exe"), &["Ninja Gaiden Recomp"]));
        assert!(looks_like_tool(Path::new("gt3tool"), &["GT3 Revisited"]));
        assert!(looks_like_tool(Path::new("HacTool.exe"), &["XCX Recomp"]));
        assert!(looks_like_tool(Path::new(".tools/sdk/win-amd64/bin/rexglue.exe"), &["Table Tennis"]));
        assert!(!looks_like_tool(Path::new("gt3r"), &["GT3 Revisited"]));
        assert!(!looks_like_tool(Path::new("ToolKitty.exe"), &["Tool Kitty"]));
    }

    fn sparse(p: &Path, len: u64) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::File::create(p).unwrap().set_len(len).unwrap();
    }

    #[test]
    fn installers_in_other_languages_and_diagnostic_scripts() {
        // An installer named in Portuguese is the setup, not the game.
        let d = tmp("gowhd");
        sparse(&d.join("Instalar God of War Collection.exe"), 4_000_000);
        let names = ["God of War Collection (VitaRT)", "GOD-OF-WAR-HD-COLLECTION", "God of War Collection"];
        assert!(find_exe(&d, "windows", &names).is_none());
        assert_eq!(find_setup(&d, "windows", None).unwrap(), PathBuf::from("Instalar God of War Collection.exe"));
        assert!(looks_like_setup(Path::new("Instalador Juego.exe")));
        // A start script for diagnostics isn't how the game starts.
        let d = tmp("monado");
        sparse(&d.join("Play Monado (stutter diagnostics).bat"), 900);
        sparse(&d.join("Play Monado.bat"), 700);
        sparse(&d.join("monado.exe"), 60_000_000);
        assert_eq!(find_exe(&d, "windows", &["Monado"]).unwrap(), PathBuf::from("Play Monado.bat"));
    }

    #[test]
    fn a_windows_installer_package_is_the_setup() {
        let d = tmp("msi");
        sparse(&d.join("OpenGOAL-Launcher_2.11.2_x64_en-US.msi"), 9_000_000);
        assert_eq!(find_setup(&d, "windows", None).unwrap(), PathBuf::from("OpenGOAL-Launcher_2.11.2_x64_en-US.msi"));
        assert!(looks_like_setup(Path::new("OpenGOAL-Launcher_2.11.2_x64_en-US.msi")));
        assert!(find_game_exe(&d, "windows", &["OpenGOAL Launcher"]).is_none());
        assert_eq!(crate::assets::kind("OpenGOAL-Launcher_2.11.2_x64_en-US.msi"), Some("msi"));
        assert_eq!(crate::assets::describe("OpenGOAL-Launcher_2.11.2_x64_en-US.msi", "windows"), "Installer");
        // The compiler that comes with it is never the game.
        sparse(&d.join("goalc.exe"), 9_000_000);
        assert!(find_game_exe(&d, "windows", &["OpenGOAL Launcher"]).is_none());
    }

    #[test]
    fn picks_the_launcher_and_its_setup() {
        // CTR Nitro-Fueled: the launcher on top, the much bigger engine in bin.
        let d = tmp("octr");
        sparse(&d.join("bin/sacredfire.exe"), 165_566_464);
        sparse(&d.join("bin/bb-gpu-capabilities.exe"), 197_632);
        sparse(&d.join("Crash\u{2122} Team Racing Nitro-Fueled.exe"), 3_612_160);
        sparse(&d.join("Jugar sin launcher.bat"), 710);
        sparse(&d.join("python/python.exe"), 106_208);
        let names = ["CTR Nitro-Fueled Online", "OCTR-NitroFueled-PC", "Crash Team Racing Nitro-Fueled"];
        assert_eq!(find_exe(&d, "windows", &names).unwrap(), PathBuf::from("Crash\u{2122} Team Racing Nitro-Fueled.exe"));
        assert!(find_setup(&d, "windows", Some(Path::new("x.exe"))).is_none());
        // Tenkaichi 3: the game, and the setup that has to run first.
        let d = tmp("bt3");
        sparse(&d.join("Tenkaichi3Decomp.exe"), 7_492_097);
        sparse(&d.join("Tenkaichi3Decomp-setup.exe"), 1_550_430);
        let exe = find_exe(&d, "windows", &["Tenkaichi 3 Decomp", "Tenkaichi3Decomp"]).unwrap();
        assert_eq!(exe, PathBuf::from("Tenkaichi3Decomp.exe"));
        assert_eq!(find_setup(&d, "windows", Some(&exe)).unwrap(), PathBuf::from("Tenkaichi3Decomp-setup.exe"));
        // Madden NFL 08: only an installer. It's never the game, it's the setup.
        let d = tmp("madden");
        sparse(&d.join("MaddenNFL08-Recompiled-Setup.exe"), 3_000_000);
        let names = ["Madden NFL 08 Recompiled", "MaddenNFL08-Recompiled", "Madden NFL 08"];
        assert!(find_exe(&d, "windows", &names).is_none());
        assert_eq!(find_setup(&d, "windows", None).unwrap(), PathBuf::from("MaddenNFL08-Recompiled-Setup.exe"));
        // Where it installs: the launch script goes ahead of the program.
        let d = tmp("madden-dist");
        sparse(&d.join("dist/madden08.exe"), 90_000_000);
        sparse(&d.join("dist/Launch Madden NFL 08.bat"), 400);
        sparse(&d.join("setup.bat"), 4000);
        sparse(&d.join("unins000.exe"), 3_000_000);
        assert_eq!(find_exe(&d, "windows", &names).unwrap(), PathBuf::from("dist/Launch Madden NFL 08.bat"));
        // BotW Native: its launcher, not the 500 MB engine that needs to be told where the game is,
        // and not the installer of the same thing.
        let d = tmp("botw");
        sparse(&d.join("BotWRecomp.exe"), 498_748_619);
        sparse(&d.join("BotWLauncher.exe"), 1_146_767);
        sparse(&d.join("BotWLauncherClassic.exe"), 860_924);
        sparse(&d.join("BotW-Native-Setup.exe"), 3_519_943);
        let exe = find_exe(&d, "windows", &["BotW Native", "BotW-Native", "The Legend of Zelda: Breath of the Wild"]).unwrap();
        assert_eq!(exe, PathBuf::from("BotWLauncher.exe"));
        assert!(find_setup(&d, "windows", Some(&exe)).is_none());
        // inFAMOUS Second Son: its launcher, not the classic one or the quick-play program.
        let d = tmp("iss");
        sparse(&d.join("inFAMOUS Second Son (classic).exe"), 2_887_589);
        sparse(&d.join("inFAMOUS Second Son.exe"), 3_045_915);
        sparse(&d.join("Play inFAMOUS Second Son.exe"), 183_296);
        sparse(&d.join("out/tlg-probe.exe"), 14_285_824);
        assert_eq!(find_exe(&d, "windows", &["inFAMOUS Second Son PC", "iss_pc", "inFAMOUS Second Son"]).unwrap(), PathBuf::from("inFAMOUS Second Son.exe"));
        // Vice City Stories: VCSLauncher.exe, named by the game's initials, not the game itself.
        let d = tmp("vcs");
        sparse(&d.join("VCSNative.exe"), 77_908_480);
        sparse(&d.join("VCSLauncher.exe"), 80_896);
        sparse(&d.join("INSTALL_TEXTURE_PACK.bat"), 2055);
        assert_eq!(find_exe(&d, "windows", &["Vice City Stories Recomp", "vice-city-stories-recomp"]).unwrap(), PathBuf::from("VCSLauncher.exe"));
        // Runtimes that come along aren't the game's setup.
        let d = tmp("redist");
        sparse(&d.join("Game.exe"), 9_000_000);
        sparse(&d.join("vc_redist.x64.exe"), 9_000_000);
        sparse(&d.join("DirectX/DXSETUP.exe"), 500_000);
        assert!(find_setup(&d, "windows", Some(Path::new("Game.exe"))).is_none());
    }

    #[test]
    fn camel_case_tokens() {
        assert_eq!(tokens("Zelda64Recompiled"), vec!["zelda64", "recompiled"]);
        assert_eq!(tokens("unins000"), vec!["unins000"]);
        assert_eq!(tokens("vc_redist.x64"), vec!["vc", "redist", "x64"]);
        assert_eq!(tokens("BotWRecomp"), vec!["bot", "w", "recomp"]);
        assert_eq!(tokens("MaddenNFL08-Recompiled"), vec!["madden", "nfl08", "recompiled"]);
        assert_eq!(tokens("OCTRNitro"), vec!["octr", "nitro"]);
    }
}

#[cfg(test)]
mod answer_tests {
    #[test]
    fn errors_name_the_site() {
        let m = super::answered(500, "https://files3.eu.example.com/1/items/x/Pack.zip", "Pack.zip");
        assert!(m.starts_with("example.com had a problem"), "{m}");
        assert!(super::answered(404, "https://github.com/a/b/releases/download/v1/x.zip", "x.zip").starts_with("github.com doesn't have"));
    }
}

#[cfg(all(test, unix))]
mod mac_archive_tests {
    use super::*;
    use std::io::{Cursor, Write};
    use std::os::unix::fs::PermissionsExt;

    fn temp(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("rl-mac-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn archive(files: &[(&str, &[u8], u32)], links: &[(&str, &str)]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes, mode) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default().unix_permissions(*mode)).unwrap();
            zip.write_all(bytes).unwrap();
        }
        for (name, target) in links {
            zip.add_symlink(*name, *target, zip::write::SimpleFileOptions::default()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn a_disk_image_installs_its_app_and_leaves_the_applications_shortcut() {
        let d = temp("dmg");
        let src = d.join("volume");
        fs::create_dir_all(src.join("Game.app/Contents/MacOS")).unwrap();
        fs::write(src.join("Game.app/Contents/Info.plist"), b"metadata").unwrap();
        fs::write(src.join("Game.app/Contents/MacOS/Game"), b"program").unwrap();
        std::os::unix::fs::symlink("/Applications", src.join("Applications")).unwrap();
        let image = d.join("Game-macOS.dmg");
        let made = std::process::Command::new("hdiutil").args(["create", "-quiet", "-fs", "HFS+", "-volname", "Game", "-srcfolder"]).arg(&src).arg(&image).status().unwrap();
        assert!(made.success());
        let target = d.join("install");
        let files = unpack(&image, &target, &d.join("staging")).unwrap();
        assert!(files.iter().any(|f| f.ends_with("Game.app/Contents/MacOS/Game")));
        assert_eq!(fs::read(target.join("Game.app/Contents/Info.plist")).unwrap(), b"metadata");
        assert!(!target.join("Applications").exists());
        // Unmounted again, and a file that isn't an image is an error, not a hang.
        assert!(!d.join("staging.mount").exists());
        let junk = d.join("Junk.dmg");
        fs::write(&junk, b"not an image").unwrap();
        assert!(unpack(&junk, &d.join("install2"), &d.join("staging")).is_err());
        let _ = fs::remove_dir_all(&d);
    }

    fn app_zip(extra: &[(&str, &[u8], u32)]) -> Vec<u8> {
        let mut files: Vec<(&str, &[u8], u32)> = vec![
            ("Game.app/Contents/Info.plist", b"metadata", 0o644),
            ("Game.app/Contents/MacOS/Game", b"program", 0o755),
            ("Game.app/Contents/Frameworks/Engine.framework/Versions/A/Engine", b"library", 0o755),
        ];
        files.extend_from_slice(extra);
        archive(&files, &[
            ("Game.app/Contents/Frameworks/Engine.framework/Versions/Current", "A"),
            ("Game.app/Contents/Frameworks/Engine.framework/Engine", "Versions/Current/Engine"),
        ])
    }

    #[test]
    fn mac_app_keeps_bundle_and_framework_links() {
        let d = temp("bundle");
        fs::write(d.join("game.zip"), app_zip(&[])).unwrap();
        let target = d.join("installed");
        let files = unpack(&d.join("game.zip"), &target, &d.join("stage")).unwrap();
        assert_eq!(fs::read(target.join("Game.app/Contents/MacOS/Game")).unwrap(), b"program");
        assert_eq!(fs::metadata(target.join("Game.app/Contents/MacOS/Game")).unwrap().permissions().mode() & 0o111, 0o111);
        let link = target.join("Game.app/Contents/Frameworks/Engine.framework/Versions/Current");
        assert_eq!(fs::read_link(&link).unwrap(), PathBuf::from("A"));
        assert_eq!(fs::read(target.join("Game.app/Contents/Frameworks/Engine.framework/Engine")).unwrap(), b"library");
        assert!(files.contains(&PathBuf::from("Game.app/Contents/Frameworks/Engine.framework/Versions/Current")));
        assert!(!files.iter().any(|p| p.to_string_lossy().contains("Current/Engine")), "file inventory must not follow links");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn mac_nested_zip_wrapper_is_unpacked_but_game_data_is_not() {
        let d = temp("nested");
        let data = archive(&[("level.bin", b"level", 0o644)], &[]);
        let inner = app_zip(&[("Game.app/Contents/Resources/data.zip", &data, 0o644)]);
        let outer = archive(&[("release/game.zip", &inner, 0o644), ("__MACOSX/._game.zip", b"metadata", 0o644)], &[]);
        fs::write(d.join("outer.zip"), outer).unwrap();
        unpack(&d.join("outer.zip"), &d.join("installed"), &d.join("stage")).unwrap();
        assert!(d.join("installed/Game.app/Contents/MacOS/Game").is_file());
        assert_eq!(fs::read(d.join("installed/Game.app/Contents/Resources/data.zip")).unwrap(), data);
        assert!(!d.join("installed/Game.app/Contents/Resources/level.bin").exists());
    }

    #[test]
    fn mac_app_update_replaces_old_bundle_and_keeps_saves() {
        let d = temp("update");
        let target = d.join("installed");
        fs::write(d.join("game.zip"), app_zip(&[("Game.app/Contents/Resources/obsolete", b"old", 0o644)])).unwrap();
        unpack(&d.join("game.zip"), &target, &d.join("stage")).unwrap();
        fs::write(target.join("save.sav"), b"player progress").unwrap();
        fs::write(d.join("game.zip"), app_zip(&[])).unwrap();
        unpack(&d.join("game.zip"), &target, &d.join("stage")).unwrap();
        assert!(!target.join("Game.app/Contents/Resources/obsolete").exists());
        assert_eq!(fs::read(target.join("save.sav")).unwrap(), b"player progress");
        assert!(fs::read_link(target.join("Game.app/Contents/Frameworks/Engine.framework/Versions/Current")).is_ok());
    }

    /// A game imported as the app itself (its folder is Game.app), with a file the next release
    /// no longer has and a save in {data}.
    fn imported_app(d: &Path) -> PathBuf {
        fs::write(d.join("old.zip"), app_zip(&[("Game.app/Contents/Resources/obsolete", b"old", 0o644)])).unwrap();
        unpack(&d.join("old.zip"), &d.join("Applications"), &d.join("stage")).unwrap();
        let app = d.join("Applications/Game.app");
        fs::create_dir_all(app.join("recompdata")).unwrap();
        fs::write(app.join("recompdata/save.sav"), b"player progress").unwrap();
        app
    }

    /// What an update does with the release (see lib.rs): staged, then put in place.
    fn update_app(app: &Path, release: Vec<u8>, d: &Path) -> Result<Vec<PathBuf>, Error> {
        fs::write(d.join("new.zip"), release).unwrap();
        let staged = stage(&d.join("new.zip"), &d.join("stage"))?;
        let (staged, old_files) = staged.for_game(app, "macos")?;
        crate::swap::put_in_place(app, &staged.root, &staged.files, "sha", &old_files, &[])?.finish();
        Ok(staged.files)
    }

    #[test]
    fn mac_imported_app_update_goes_in_without_nesting() {
        let d = temp("imported-nest");
        let app = imported_app(&d);
        let files = update_app(&app, app_zip(&[]), &d).unwrap();
        assert!(!app.join("Game.app").exists());
        assert_eq!(fs::read(app.join("Contents/MacOS/Game")).unwrap(), b"program");
        assert!(files.contains(&PathBuf::from("Contents/MacOS/Game")), "the record's files are the app's own");
    }

    #[test]
    fn mac_imported_app_update_takes_away_what_the_new_app_lacks() {
        let d = temp("imported-obsolete");
        let app = imported_app(&d);
        update_app(&app, app_zip(&[]), &d).unwrap();
        assert!(!app.join("Contents/Resources/obsolete").exists());
    }

    #[test]
    fn mac_imported_app_update_keeps_the_games_data() {
        let d = temp("imported-data");
        let app = imported_app(&d);
        update_app(&app, app_zip(&[]), &d).unwrap();
        assert_eq!(fs::read(app.join("recompdata/save.sav")).unwrap(), b"player progress");
    }

    #[test]
    fn mac_imported_app_update_keeps_framework_links() {
        let d = temp("imported-links");
        let app = imported_app(&d);
        update_app(&app, app_zip(&[]), &d).unwrap();
        let link = app.join("Contents/Frameworks/Engine.framework/Versions/Current");
        assert_eq!(fs::read_link(link).unwrap(), PathBuf::from("A"));
    }

    #[test]
    fn mac_release_without_an_app_leaves_the_imported_app_as_it_was() {
        let d = temp("imported-no-app");
        let app = imported_app(&d);
        let loose = archive(&[("OtherGame", b"program", 0o755)], &[]);
        assert!(update_app(&app, loose, &d).is_err());
        assert_eq!(fs::read(app.join("Contents/Resources/obsolete")).unwrap(), b"old");
    }

    #[test]
    fn a_game_folder_that_is_not_an_app_is_staged_as_before() {
        let d = temp("not-imported-app");
        fs::write(d.join("new.zip"), app_zip(&[])).unwrap();
        let staged = stage(&d.join("new.zip"), &d.join("stage")).unwrap();
        let root = staged.root.clone();
        let (staged, old_files) = staged.for_game(&d.join("installed"), "macos").unwrap();
        assert_eq!(staged.root, root);
        assert!(old_files.is_empty());
    }

    #[test]
    fn an_app_named_game_folder_off_a_mac_is_staged_as_before() {
        let d = temp("app-folder-linux");
        let app = imported_app(&d);
        fs::write(d.join("new.zip"), app_zip(&[])).unwrap();
        let staged = stage(&d.join("new.zip"), &d.join("stage")).unwrap();
        let root = staged.root.clone();
        let (staged, old_files) = staged.for_game(&app, "linux").unwrap();
        assert_eq!(staged.root, root);
        assert!(old_files.is_empty());
    }

    #[test]
    fn mac_archive_rejects_links_outside_staging() {
        for (label, link) in [("absolute", "/tmp"), ("relative", "../../outside")] {
            let d = temp(label);
            let bytes = archive(&[("Game.app/Contents/Info.plist", b"info", 0o644)], &[("Game.app/escape", link)]);
            fs::write(d.join("game.zip"), bytes).unwrap();
            assert!(unpack(&d.join("game.zip"), &d.join("installed"), &d.join("stage")).is_err());
            assert!(!d.join("installed").exists(), "invalid archive must not be installed");
        }
    }

    #[test]
    fn mac_archive_does_not_write_through_archive_links() {
        let d = temp("link-child");
        let outside = d.join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("sentinel"), b"safe").unwrap();
        let bytes = archive(&[("link/sentinel", b"changed", 0o644)], &[("link", outside.to_str().unwrap())]);
        fs::write(d.join("game.zip"), bytes).unwrap();
        assert!(unpack(&d.join("game.zip"), &d.join("installed"), &d.join("stage")).is_err());
        assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"safe");
    }

    #[test]
    fn mac_archive_link_chains_cannot_create_paths_outside_staging() {
        let d = temp("link-chain-write");
        let bytes = archive(&[], &[("a", "."), ("b", "a/.."), ("b/escaped/probe", ".")]);
        fs::write(d.join("game.zip"), bytes).unwrap();
        fs::create_dir(d.join("stage")).unwrap();
        assert!(unzip(&d.join("game.zip"), &d.join("stage")).is_err());
        assert!(!d.join("escaped").exists(), "reject the chain before creating any outside path");
    }

    #[test]
    fn mac_update_does_not_follow_existing_destination_links() {
        let d = temp("destination-link");
        fs::create_dir_all(d.join("installed")).unwrap();
        fs::create_dir_all(d.join("outside")).unwrap();
        fs::write(d.join("outside/sentinel"), b"safe").unwrap();
        std::os::unix::fs::symlink(d.join("outside"), d.join("installed/data")).unwrap();
        let bytes = archive(&[("data/sentinel", b"new", 0o644), ("Game", b"program", 0o755)], &[]);
        fs::write(d.join("game.zip"), bytes).unwrap();
        unpack(&d.join("game.zip"), &d.join("installed"), &d.join("stage")).unwrap();
        assert_eq!(fs::read(d.join("outside/sentinel")).unwrap(), b"safe");
        assert_eq!(fs::read(d.join("installed/data/sentinel")).unwrap(), b"new");
    }
}

#[cfg(all(test, unix))]
mod mac_discovery_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn temp(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("rl-mac-detect-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn mach(cpu: u32, filetype: u32) -> Vec<u8> {
        let mut bytes = vec![0u8; 32];
        bytes[..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
        bytes[4..8].copy_from_slice(&cpu.to_le_bytes());
        bytes[12..16].copy_from_slice(&filetype.to_le_bytes());
        bytes
    }

    fn native_cpu() -> u32 {
        if crate::assets::arch_now() == "arm64" { 0x0100000c } else { 0x01000007 }
    }

    fn program(p: &Path, bytes: &[u8]) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
        fs::set_permissions(p, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn bundle(d: &Path, declared: &str) -> PathBuf {
        let app = d.join("My Game.app");
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        fs::write(app.join("Contents/Info.plist"), format!("<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>{declared}</string></dict></plist>")).unwrap();
        program(&app.join("Contents/MacOS/Engine"), &mach(native_cpu(), 2));
        let mut helper = mach(native_cpu(), 2);
        helper.resize(100_000, 0);
        program(&app.join("Contents/MacOS/MyGameHelper"), &helper);
        app
    }

    #[test]
    fn mac_detection_uses_bundle_metadata_instead_of_largest_helper() {
        let d = temp("metadata");
        bundle(&d, "Engine");
        assert_eq!(find_game_exe(&d, "macos", &["My Game"]), Some(PathBuf::from("My Game.app/Contents/MacOS/Engine")));
    }

    #[test]
    fn mac_manual_selection_resolves_bundle_and_keeps_file_choices() {
        let d = temp("manual");
        let app = bundle(&d, "Engine");
        assert_eq!(program_choice(&app, "macos"), Some(app.join("Contents/MacOS/Engine")));
        assert_eq!(program_choice(&app.join("Contents/MacOS/Engine"), "macos"), Some(app.join("Contents/MacOS/Engine")));
        assert!(program_choice(&app, "linux").is_none());
        assert!(program_choice(&d.join("Missing.app"), "macos").is_none());
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn mac_manual_setup_starts_the_selected_bundle() {
        let d = temp("manual-setup");
        let app = bundle(&d, "Engine");
        let g = crate::library::Installed { path: d.clone(), ..Default::default() };
        let manual = crate::library::ManualSetup { exe: Some(PathBuf::from("My Game.app")), how: "none".into(), ..Default::default() };
        let (exe, args) = crate::manual_plan(&g, &manual).unwrap().unwrap();
        assert_eq!(exe, app.join("Contents/MacOS/Engine"));
        assert!(args.is_empty());
    }

    #[test]
    fn mac_detection_does_not_guess_inside_invalid_bundle() {
        for declared in ["Missing", "../Engine", "/Engine"] {
            let d = temp(&format!("invalid-{}", declared.replace('/', "_")));
            bundle(&d, declared);
            assert!(find_game_exe(&d, "macos", &["My Game"]).is_none());
        }
    }

    #[test]
    fn mac_app_as_the_games_folder_starts_its_declared_program() {
        let app = bundle(&temp("app-root"), "Engine");
        assert_eq!(find_game_exe(&app, "macos", &["My Game"]), Some(PathBuf::from("Contents/MacOS/Engine")));
    }

    #[test]
    fn an_imported_apps_update_drops_the_saved_program_for_its_new_info_plist() {
        let app = bundle(&temp("app-update-renamed"), "Engine");
        let saved = Some(PathBuf::from("Contents/MacOS/Engine"));
        assert!(kept_exe(saved, &app, "macos", &[], false, &["My Game"]).is_none());
    }

    #[test]
    fn an_imported_apps_update_keeps_the_program_the_player_picked() {
        let app = bundle(&temp("app-update-picked"), "Engine");
        let saved = Some(PathBuf::from("Contents/MacOS/MyGameHelper"));
        assert_eq!(kept_exe(saved.clone(), &app, "macos", &[], true, &["My Game"]), saved);
    }

    #[test]
    fn a_picked_program_the_update_takes_away_is_not_kept() {
        let app = bundle(&temp("app-update-picked-gone"), "Engine");
        let saved = PathBuf::from("Contents/MacOS/Engine");
        assert!(kept_exe(Some(saved.clone()), &app, "macos", &[saved], true, &["My Game"]).is_none());
    }

    #[test]
    fn a_found_program_in_a_plain_folder_is_kept_through_an_update() {
        let d = temp("plain-update");
        bundle(&d, "Engine");
        let saved = Some(PathBuf::from("My Game.app/Contents/MacOS/Engine"));
        assert_eq!(kept_exe(saved.clone(), &d, "macos", &[], false, &["My Game"]), saved);
    }

    #[test]
    fn an_imported_app_finds_its_renamed_program_after_the_update() {
        let app = bundle(&temp("app-update-find"), "Engine2");
        program(&app.join("Contents/MacOS/Engine2"), &mach(native_cpu(), 2));
        let _ = fs::remove_file(app.join("Contents/MacOS/Engine"));
        assert_eq!(find_game_exe(&app, "macos", &["My Game"]), Some(PathBuf::from("Contents/MacOS/Engine2")));
    }

    #[test]
    fn mac_app_as_the_games_folder_with_a_bad_program_is_not_guessed_into() {
        let app = bundle(&temp("app-root-invalid"), "Missing");
        assert!(find_game_exe(&app, "macos", &["My Game"]).is_none());
    }

    #[test]
    fn mac_app_picked_as_the_folder_is_known_by_its_name() {
        let app = bundle(&temp("import-app"), "Engine");
        let entry = serde_json::json!({"name": "My Game"});
        let (root, exe, fits) = crate::adopt::check_folder(&app, &entry, &["My Game".to_string()], "macos");
        assert_eq!(root, app, "the app itself, never its Contents folder");
        assert_eq!(exe, Some(PathBuf::from("Contents/MacOS/Engine")));
        assert!(fits);
    }

    #[test]
    fn mac_folder_holding_only_the_app_stays_the_games_folder() {
        let d = temp("import-folder");
        bundle(&d, "Engine");
        let entry = serde_json::json!({"name": "My Game"});
        let (root, exe, fits) = crate::adopt::check_folder(&d, &entry, &["My Game".to_string()], "macos");
        assert_eq!(root, d);
        assert_eq!(exe, Some(PathBuf::from("My Game.app/Contents/MacOS/Engine")));
        assert!(fits);
    }

    #[test]
    fn mac_app_of_another_game_does_not_fit() {
        let app = bundle(&temp("import-other"), "Engine");
        let entry = serde_json::json!({"name": "Unrelated Title"});
        let (_, _, fits) = crate::adopt::check_folder(&app, &entry, &["Unrelated Title".to_string()], "macos");
        assert!(!fits);
    }

    #[test]
    fn mac_app_named_in_the_profile_fits() {
        let d = temp("import-profile-app");
        bundle(&d, "Engine");
        fs::write(d.join("recomplauncher.json"), r#"{"format":1,"start":{"program":["My Game.app"]}}"#).unwrap();
        let entry = serde_json::json!({"name": "Other Name"});
        let (_, exe, fits) = crate::adopt::check_folder(&d, &entry, &["Other Name".to_string()], "macos");
        assert_eq!(exe, Some(PathBuf::from("My Game.app")));
        assert!(fits);
    }

    #[test]
    fn mac_build_fits_when_the_profile_names_only_other_systems_programs() {
        let d = temp("import-profile-windows");
        bundle(&d, "Engine");
        fs::write(d.join("recomplauncher.json"), r#"{"format":1,"start":{"program":["MyGame.exe"]}}"#).unwrap();
        let entry = serde_json::json!({"name": "My Game"});
        let (_, exe, fits) = crate::adopt::check_folder(&d, &entry, &["My Game".to_string()], "macos");
        assert_eq!(exe, Some(PathBuf::from("My Game.app/Contents/MacOS/Engine")));
        assert!(fits);
    }

    #[test]
    fn mac_detection_accepts_native_mach_programs_with_dots() {
        let d = temp("native");
        let exe = d.join("Game.v1");
        program(&exe, &mach(native_cpu(), 2));
        assert!(is_program(&exe, "macos"));
        assert!(!is_program(&exe, "linux"));
        assert_eq!(find_game_exe(&d, "macos", &["Game"]), Some(PathBuf::from("Game.v1")));
    }

    #[test]
    fn mac_detection_accepts_universal_programs() {
        let d = temp("universal");
        let mut fat = vec![0u8; 112];
        fat[..4].copy_from_slice(&[0xca, 0xfe, 0xba, 0xbe]);
        fat[4..8].copy_from_slice(&2u32.to_be_bytes());
        for (entry, offset, cpu) in [(8, 48, 0x01000007u32), (28, 80, 0x0100000cu32)] {
            fat[entry..entry + 4].copy_from_slice(&cpu.to_be_bytes());
            fat[entry + 8..entry + 12].copy_from_slice(&(offset as u32).to_be_bytes());
            fat[entry + 12..entry + 16].copy_from_slice(&32u32.to_be_bytes());
            fat[offset..offset + 32].copy_from_slice(&mach(cpu, 2));
        }
        program(&d.join("Game"), &fat);
        assert!(is_program(&d.join("Game"), "macos"));
    }

    #[test]
    fn mac_detection_rejects_libraries_wrong_cpu_and_linux() {
        let d = temp("not-programs");
        for (name, bytes) in [
            ("Library", mach(native_cpu(), 6)),
            ("OtherCPU", mach(0x01000012, 2)),
            ("Truncated", vec![0xcf, 0xfa, 0xed, 0xfe]),
        ] {
            program(&d.join(name), &bytes);
            assert!(!is_program(&d.join(name), "macos"), "{name}");
        }
        let mut elf = vec![0u8; 20];
        elf[..4].copy_from_slice(b"\x7fELF");
        elf[18] = if crate::assets::arch_now() == "arm64" { 0xb7 } else { 0x3e };
        program(&d.join("LinuxGame"), &elf);
        assert!(!is_program(&d.join("LinuxGame"), "macos"));
        assert!(is_program(&d.join("LinuxGame"), "linux"));
    }
}

#[cfg(test)]
mod connection_fallback_tests {
    use super::*;
    use std::io::{Read, Write};

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!("rl-connection-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
    }

    const COMPLETE: &str = "HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nabcdef";

    fn download_site(replies: Vec<&'static str>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        download_site_until(replies, Arc::new(AtomicBool::new(false)))
    }

    fn download_site_until(replies: Vec<&'static str>, stop: Arc<AtomicBool>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/asset", listener.local_addr().unwrap());
        let thread = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for response in replies {
                let until = Instant::now() + Duration::from_secs(15);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((s, _)) => break s,
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock && stop.load(Ordering::Relaxed) => return requests,
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock && Instant::now() < until => std::thread::sleep(Duration::from_millis(5)),
                        Err(e) => panic!("the download route was never requested: {e}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut buf = [0; 1024];
                    let n = stream.read(&mut buf).unwrap();
                    assert!(n > 0);
                    request.extend_from_slice(&buf[..n]);
                }
                stream.write_all(response.as_bytes()).unwrap();
                requests.push(String::from_utf8(request).unwrap());
            }
            requests
        });
        (url, thread)
    }

    fn asset(download: String) -> Asset {
        Asset {
            name: "game.zip".into(), size: 6,
            digest: Some("sha256:bef57ec7f53a6d40beb640a780a639c83bc29ac8a9816f1fc6c5c6dcd93c4721".into()),
            browser_download_url: download, url: None,
        }
    }

    #[tokio::test]
    async fn interrupted_bodies_resume_through_the_api_with_the_same_hash() {
        let dir = TempDir::new();
        let broken = "HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nab";
        let stop = Arc::new(AtomicBool::new(false));
        let (original, primary) = download_site_until(vec![broken; 6], stop.clone());
        let (api, server) = download_site(vec!["HTTP/1.1 206 Partial Content\r\nContent-Length: 4\r\nContent-Range: bytes 2-5/6\r\nConnection: close\r\n\r\ncdef"]);
        let result = download_with_api(&reqwest::Client::new(), &asset(original), &dir.0, "body-test", Arc::new(AtomicBool::new(false)), &|_| {}, Some(&api)).await;
        stop.store(true, Ordering::Relaxed);
        let got = result.unwrap_or_else(|e| panic!("the asset API should resume an interrupted body: {e}"));
        assert_eq!(fs::read(&got.file).unwrap(), b"abcdef");
        assert!(got.verified);
        assert_eq!(got.sha256, "bef57ec7f53a6d40beb640a780a639c83bc29ac8a9816f1fc6c5c6dcd93c4721");
        assert_eq!(primary.join().unwrap().len(), 3);
        assert!(server.join().unwrap()[0].to_ascii_lowercase().contains("\r\nrange: bytes=2-\r\n"));
    }

    #[tokio::test]
    async fn response_timeouts_recover_through_the_asset_api() {
        let dir = TempDir::new();
        // Connections queue, but no server reads or answers them.
        let stalled = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let original = format!("http://{}/download", stalled.local_addr().unwrap());
        let (api, server) = download_site(vec![COMPLETE]);
        let http = reqwest::Client::builder().read_timeout(Duration::from_millis(100)).build().unwrap();
        let result = download_with_api(&http, &asset(original), &dir.0, "timeout-test", Arc::new(AtomicBool::new(false)), &|_| {}, Some(&api)).await;
        let got = result.unwrap_or_else(|e| panic!("a reachable asset API should recover a stalled download: {e}"));
        assert_eq!(fs::read(&got.file).unwrap(), b"abcdef");
        assert!(got.verified);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn a_fallback_still_rejects_a_mismatched_checksum() {
        let dir = TempDir::new();
        let stalled = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut release = asset(format!("http://{}/download", stalled.local_addr().unwrap()));
        release.digest = Some(format!("sha256:{}", "0".repeat(64)));
        let (api, server) = download_site(vec![COMPLETE]);
        let http = reqwest::Client::builder().read_timeout(Duration::from_millis(100)).build().unwrap();
        let result = download_with_api(&http, &release, &dir.0, "checksum-test", Arc::new(AtomicBool::new(false)), &|_| {}, Some(&api)).await;
        let error = match result { Ok(_) => panic!("the wrong bytes must not be accepted"), Err(e) => e.to_string() };
        assert!(error.contains("checksum doesn't match"), "{error}");
        assert!(!dir.0.join("game.zip").exists());
        server.join().unwrap();
    }

    #[tokio::test]
    async fn connection_failures_recover_through_the_asset_api_and_verify_the_bytes() {
        let dir = TempDir::new();
        // Pick an unused local port for a connection-refused response.
        let refused = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let original = format!("http://{}/download", refused.local_addr().unwrap());
        let (api, server) = download_site(vec![COMPLETE]);
        drop(refused);
        // A previously interrupted download resumes with Range; a full API response must reset
        // that partial hash instead of appending the file twice.
        let tag = &hex::encode(Sha256::digest(format!("6|{original}").as_bytes()))[..10];
        fs::write(dir.0.join(format!("game.zip.{tag}.part")), b"ab").unwrap();
        let result = download_with_api(&reqwest::Client::new(), &asset(original), &dir.0, "connection-test", Arc::new(AtomicBool::new(false)), &|_| {}, Some(&api)).await;
        let got = result.unwrap_or_else(|e| panic!("a reachable asset API should recover the download: {e}"));
        assert_eq!(fs::read(&got.file).unwrap(), b"abcdef");
        assert!(got.verified);
        assert_eq!(got.sha256, "bef57ec7f53a6d40beb640a780a639c83bc29ac8a9816f1fc6c5c6dcd93c4721");
        let request = server.join().unwrap()[0].to_ascii_lowercase();
        assert!(request.contains("\r\naccept: application/octet-stream\r\n"));
        assert!(request.contains("\r\nrange: bytes=2-\r\n"));
    }
}

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

/// Download `asset` into `dir`, reporting progress, then check it against GitHub's checksum.
pub async fn download(
    http: &reqwest::Client,
    asset: &Asset,
    dir: &Path,
    id: &str,
    cancel: Arc<AtomicBool>,
    report: &(dyn Fn(Progress) + Send + Sync),
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
    let mut out: Option<tokio::fs::File> = None;
    let mut done = have;
    let mut total;
    let mut tries = 0u32;
    use tokio::io::AsyncWriteExt;
    'attempt: loop {
        tries += 1;
        let mut req = http.get(&asset.browser_download_url).timeout(Duration::from_secs(60 * 60 * 6));
        if done > 0 {
            req = req.header(reqwest::header::RANGE, format!("bytes={done}-"));
        }
        let send = req.send();
        tokio::pin!(send);
        let started = Instant::now();
        let r = loop {
            if cancelled() {
                drop(out);
                let _ = tokio::fs::remove_file(&part).await;
                return Err(Error::Cancelled);
            }
            match tokio::time::timeout(Duration::from_millis(500), &mut send).await {
                Ok(r) => break r,
                Err(_) if started.elapsed() < Duration::from_secs(30) => continue,
                Err(_) if tries < 4 => continue 'attempt,
                Err(_) => return Err(Error::Msg("GitHub isn't answering right now. Try again in a minute; the download carries on from where it got to.".into())),
            }
        };
        let r = match r {
            Ok(r) => r,
            Err(e) if tries < 4 && (e.is_timeout() || e.is_connect() || e.is_request()) => {
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        if !r.status().is_success() {
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
            return Err(Error::Msg(format!("GitHub answered {} for {}", r.status().as_u16(), asset.name)));
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
        loop {
            if cancelled() {
                break 'attempt;
            }
            let next = match tokio::time::timeout(Duration::from_millis(500), stream.next()).await {
                Ok(n) => n,
                Err(_) if quiet.elapsed() < Duration::from_secs(40) => continue,
                // Nothing for 40 seconds: ask again from where it got to.
                Err(_) if tries < 6 => { f.flush().await?; continue 'attempt; }
                Err(_) => return Err(Error::Msg(format!("The download of {} stopped. Press Install again; it carries on from where it got to.", asset.name))),
            };
            let Some(chunk) = next else { break 'attempt };
            let chunk = match chunk {
                Ok(c) => c,
                Err(_) if tries < 6 => { f.flush().await?; tokio::time::sleep(Duration::from_secs(2)).await; continue 'attempt; }
                Err(e) => return Err(e.into()),
            };
            quiet = Instant::now();
            hash.update(&chunk);
            f.write_all(&chunk).await?;
            done += chunk.len() as u64;
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
        drop(out);
        let _ = tokio::fs::remove_file(&part).await;
        return Err(Error::Cancelled);
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
    for i in 0..z.len() {
        let mut e = z.by_index(i).map_err(|e| Error::Msg(format!("Couldn't read the zip: {e}")))?;
        let Some(rel) = inside(e.name()) else { continue };
        let dest = to.join(rel);
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

fn untar(file: &Path, to: &Path, xz: bool) -> Result<(), Error> {
    let f = fs::File::open(file)?;
    if xz {
        return Err(Error::Msg(".tar.xz isn't supported yet".into()));
    }
    let mut t = tar::Archive::new(flate2::read::GzDecoder::new(f));
    t.set_preserve_permissions(true);
    // tar skips entries that would land outside the folder.
    t.unpack(to)?;
    Ok(())
}

/// The folder the files actually start in: an archive with one folder at the top is that folder.
fn root(dir: &Path) -> PathBuf {
    let items: Vec<_> = fs::read_dir(dir).map(|r| r.flatten().collect()).unwrap_or_default();
    if items.len() == 1 && items[0].path().is_dir() {
        root(&items[0].path())
    } else {
        dir.to_path_buf()
    }
}

/// A file operation that Windows can refuse for a few seconds (access denied, or in use) while an
/// antivirus looks at a file that was just unpacked: tried again for up to about ten seconds.
fn patient<T>(mut op: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut last = None;
    for i in 0..20 {
        match op() {
            Ok(v) => return Ok(v),
            Err(e) if matches!(e.raw_os_error(), Some(5) | Some(32) | Some(33)) || e.kind() == io::ErrorKind::PermissionDenied => {
                last = Some(e);
                std::thread::sleep(std::time::Duration::from_millis(if i < 4 { 250 } else { 600 }));
            }
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| io::Error::other("busy")))
}

/// What to tell the player when a file can't be put in place.
fn denied(e: io::Error, what: &Path) -> io::Error {
    if matches!(e.raw_os_error(), Some(5) | Some(32) | Some(33)) || e.kind() == io::ErrorKind::PermissionDenied {
        io::Error::new(
            e.kind(),
            format!(
                "Windows wouldn't let the launcher write {}. Close the game if it's running, check that your antivirus isn't holding the folder, or pick a library folder outside Program Files, then try again.",
                what.display()
            ),
        )
    } else {
        e
    }
}

/// Move everything in `from` into `to`, replacing files of the same name and leaving the rest
/// (saves, settings, the game files people added) alone.
fn overlay(from: &Path, to: &Path) -> io::Result<()> {
    patient(|| fs::create_dir_all(to)).map_err(|e| denied(e, to))?;
    for e in fs::read_dir(from)? {
        let e = e?;
        let src = e.path();
        let dest = to.join(e.file_name());
        if src.is_dir() {
            if dest.is_file() {
                patient(|| fs::remove_file(&dest)).map_err(|e| denied(e, &dest))?;
            }
            overlay(&src, &dest)?;
        } else {
            if dest.is_dir() {
                patient(|| fs::remove_dir_all(&dest)).map_err(|e| denied(e, &dest))?;
            }
            if patient(|| fs::rename(&src, &dest)).is_err() {
                patient(|| fs::copy(&src, &dest)).map_err(|e| denied(e, &dest))?;
            }
        }
    }
    Ok(())
}

/// Unpack (or place) the downloaded file into `target`. Returns the files it put there, relative.
pub fn unpack(file: &Path, target: &Path, staging: &Path) -> Result<Vec<PathBuf>, Error> {
    unpack_with(file, target, staging, true)
}

/// Like unpack, but keeps the archive's folders as they are (a mod's own layout), even a single
/// folder at the top.
pub fn unpack_as_is(file: &Path, target: &Path, staging: &Path) -> Result<Vec<PathBuf>, Error> {
    unpack_with(file, target, staging, false)
}

fn unpack_with(file: &Path, target: &Path, staging: &Path, strip_top: bool) -> Result<Vec<PathBuf>, Error> {
    let name = file.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
    let k = kind(&name).ok_or_else(|| Error::Msg(format!("{name} isn't a kind of file the launcher can install")))?;
    // Windows Security and Search look at a fresh download first and can hold it for a moment.
    for _ in 0..20 {
        match fs::File::open(file) {
            Err(e) if matches!(e.raw_os_error(), Some(32) | Some(33)) => std::thread::sleep(std::time::Duration::from_millis(500)),
            _ => break,
        }
    }
    let _ = fs::remove_dir_all(staging);
    fs::create_dir_all(staging)?;
    match k {
        "zip" => unzip(file, staging)?,
        "7z" => un7z(file, staging)?,
        "tar.gz" => untar(file, staging, false)?,
        "tar.xz" => untar(file, staging, true)?,
        "appimage" | "exe" => {
            fs::copy(file, staging.join(&name))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(staging.join(&name), fs::Permissions::from_mode(0o755))?;
            }
        }
        _ => return Err(Error::Msg(format!("{name} can't be installed on this PC"))),
    }
    let from = if strip_top { root(staging) } else { staging.to_path_buf() };
    let mut files = vec![];
    list(&from, &from, &mut files);
    overlay(&from, target)?;
    let _ = fs::remove_dir_all(staging);
    Ok(files)
}

fn list(base: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
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

fn tokens(s: &str) -> Vec<String> {
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
const NOT_THE_GAME: [&str; 17] = ["unins", "uninstall", "setup", "install", "crash", "crashpad", "redist", "vcredist", "helper", "updater", "update", "dxsetup", "vc", "elevate", "report", "server", "dedicated"];

/// Words that start a launch script's name: "Launch Madden NFL 08.bat", "Play.cmd".
const LAUNCH_WORDS: [&str; 5] = ["launch", "launcher", "play", "start", "run"];
/// Scripts that are for something else: debugging, a test, or playing without the launcher.
const NOT_A_LAUNCH: [&str; 8] = ["debug", "test", "safe", "sin", "without", "no", "build", "clean"];

/// Words at the end of a recomp's name that aren't the game's, and small words, for initials.
const TAIL_WORDS: [&str; 10] = ["recomp", "recompiled", "decomp", "decompiled", "pc", "port", "native", "online", "edition", "windows"];
const SMALL_WORDS: [&str; 6] = ["of", "the", "a", "and", "in", "to"];

/// A program kept for when the main one doesn't work: "inFAMOUS Second Son (classic).exe".
const OLD_WORDS: [&str; 4] = ["classic", "legacy", "old", "fallback"];
/// A program that skips the game's launcher: "Play inFAMOUS Second Son.exe".
const QUICK_WORDS: [&str; 3] = ["play", "quick", "direct"];

/// The version of find_exe's rules. Raise it when they pick better, and installs picked by older
/// rules get looked at again on their next Play (own_setup::pending).
pub const EXE_RULES: u8 = 7;

/// A Windows script a release can start the game with (a .bat or .cmd).
pub fn is_script(p: &Path) -> bool {
    let n = p.file_name().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
    n.ends_with(".bat") || n.ends_with(".cmd")
}

/// Words in the name of a program that has to run once before the game: its installer, or the
/// first-time setup that unpacks the game's files from the player's copy.
const SETUP_WORDS: [&str; 5] = ["setup", "install", "installer", "unpack", "firstrun"];
/// Installers that come with a game but aren't for it: runtimes, drivers and uninstallers.
const NOT_A_SETUP: [&str; 14] = ["unins", "uninstall", "uninstaller", "vc", "vcredist", "redist", "dxsetup", "directx", "dotnet", "netfx", "webview2", "vulkan", "driver", "runtime"];

/// The program in a release that has to run before the game can, relative to `dir`. With no game
/// program (`game` None) it's the release's own installer (MaddenNFL08-Recompiled-Setup.exe), which
/// puts the game somewhere else. Beside a game program, it's a first-time setup made for that game
/// (Tenkaichi3Decomp-setup.exe next to Tenkaichi3Decomp.exe), or one named only "Setup".
pub fn find_setup(dir: &Path, os: &str, game: Option<&Path>) -> Option<PathBuf> {
    let game_t: Vec<String> = game.and_then(|g| g.file_stem()).and_then(|s| s.to_str()).map(tokens).unwrap_or_default();
    let mut best: Option<(usize, PathBuf)> = None;
    let mut stack = vec![(dir.to_path_buf(), 0usize)];
    while let Some((d, depth)) = stack.pop() {
        let mut entries: Vec<_> = fs::read_dir(&d).into_iter().flatten().flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            if e.file_type().map(|t| t.is_symlink()).unwrap_or(true) {
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
    let t = tokens(p.file_stem().and_then(|s| s.to_str()).unwrap_or(""));
    t.iter().any(|x| SETUP_WORDS.contains(&x.as_str()))
}

pub fn is_program(p: &Path, os: &str) -> bool {
    let n = p.file_name().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
    if os == "windows" {
        return n.ends_with(".exe");
    }
    if n.ends_with(".appimage") || n.ends_with(".x86_64") {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if n.contains('.') && !n.ends_with(".sh") && !n.ends_with(".bin") {
            return false;
        }
        return fs::metadata(p).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false);
    }
    #[allow(unreachable_code)]
    false
}

/// The program to start, relative to `dir`: the one whose name looks most like the game's, near the
/// top of the folder, and big (the game is usually the biggest program in its folder).
pub fn find_exe(dir: &Path, os: &str, names: &[&str]) -> Option<PathBuf> {
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
            let p = e.path();
            if p.is_dir() {
                if depth < 4 {
                    stack.push((p, depth + 1));
                }
                continue;
            }
            let script = os == "windows" && is_script(&p);
            if !script && !is_program(&p, os) {
                continue;
            }
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
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
            let mut s;
            if script {
                // A script the release starts the game with, named for it ("Launch Madden NFL
                // 08.bat"): it passes the options the game needs, so it goes ahead of the program.
                if named == 0 || !launch || t.iter().any(|x| NOT_A_LAUNCH.contains(&x.as_str())) {
                    continue;
                }
                s = 44 - depth as i64 * 12 + named * 30 + 40;
            } else {
                let size = e.metadata().map(|m| m.len()).unwrap_or(0);
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
            }
            if t.iter().any(|x| x == "recompiled" || x == "recomp") {
                s += 10;
            }
            if best.as_ref().map(|(b, _)| s > *b).unwrap_or(true) {
                best = Some((s, p));
            }
        }
    }
    best.and_then(|(_, p)| p.strip_prefix(dir).ok().map(Path::to_path_buf))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rl-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(p: &Path, bytes: usize) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, vec![7u8; bytes]).unwrap();
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
    fn tar_gz_unpacks() {
        let d = tmp("tgz");
        let tpath = d.join("Game-Linux.tar.gz");
        {
            let gz = flate2::write::GzEncoder::new(fs::File::create(&tpath).unwrap(), flate2::Compression::fast());
            let mut t = tar::Builder::new(gz);
            let mut h = tar::Header::new_gnu();
            h.set_size(4);
            h.set_mode(0o755);
            h.set_cksum();
            t.append_data(&mut h, "Game/Game", &b"elf!"[..]).unwrap();
            t.into_inner().unwrap().finish().unwrap();
        }
        let target = d.join("lib/game");
        unpack(&tpath, &target, &d.join("st")).unwrap();
        assert!(target.join("Game").is_file());
        #[cfg(unix)]
        assert_eq!(find_exe(&target, "linux", &["Game"]).unwrap(), PathBuf::from("Game"));
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

    fn sparse(p: &Path, len: u64) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::File::create(p).unwrap().set_len(len).unwrap();
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

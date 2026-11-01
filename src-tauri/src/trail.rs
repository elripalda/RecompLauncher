//! A short record of what the launcher did (installs, Steam, games starting, Looks), so a problem
//! report says what happened before it. Kept small: the newest part only.

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

static FILE: OnceLock<PathBuf> = OnceLock::new();
static LOCK: Mutex<()> = Mutex::new(());
const MAX: u64 = 300_000;

pub fn start(data: &std::path::Path) {
    let _ = FILE.set(data.join("launcher-trail.log"));
    note(format!("RecompLauncher {} started on {}", env!("CARGO_PKG_VERSION"), crate::assets::os_now()));
}

/// One line, with the time.
pub fn note(text: impl AsRef<str>) {
    let Some(f) = FILE.get() else { return };
    let _g = LOCK.lock();
    if std::fs::metadata(f).map(|m| m.len() > MAX).unwrap_or(false) {
        if let Ok(b) = std::fs::read(f) {
            let keep = &b[b.len() / 2..];
            let from = keep.iter().position(|c| *c == b'\n').map(|i| i + 1).unwrap_or(0);
            let _ = std::fs::write(f, &keep[from..]);
        }
    }
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    if let Ok(mut o) = std::fs::OpenOptions::new().create(true).append(true).open(f) {
        let line: String = text.as_ref().chars().take(600).collect();
        let _ = writeln!(o, "{h:02}:{m:02}:{s:02} UTC  {}", line.replace('\n', " "));
    }
}

/// The newest part, for a report.
pub fn tail(bytes: usize) -> Option<String> {
    let b = std::fs::read(FILE.get()?).ok()?;
    let part = &b[b.len().saturating_sub(bytes)..];
    let from = if part.len() < b.len() { part.iter().position(|c| *c == b'\n').map(|i| i + 1).unwrap_or(0) } else { 0 };
    Some(String::from_utf8_lossy(&part[from..]).to_string())
}

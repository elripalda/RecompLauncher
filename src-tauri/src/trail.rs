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

/// For a report about one game: its own lines first (the newest of them), under a heading with its
/// id, then the newest part of everything. A report's headline then comes from the right game.
pub fn tail_for(id: &str, bytes: usize) -> Option<String> {
    let all = tail(bytes)?;
    let b = std::fs::read(FILE.get()?).ok()?;
    let text = String::from_utf8_lossy(&b[b.len().saturating_sub(600_000)..]).to_string();
    let mine: Vec<&str> = text.lines().filter(|l| mentions(l, id)).collect();
    let mine = &mine[mine.len().saturating_sub(80)..];
    Some(format!("== {id} ==\n{}\n\n== Everything, newest ==\n{all}", mine.join("\n")))
}

/// The line is about this game: its id on its own, not inside another id.
fn mentions(line: &str, id: &str) -> bool {
    if id.is_empty() {
        return false;
    }
    let edge = |c: Option<char>| c.map(|c| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')).unwrap_or(true);
    line.match_indices(id).any(|(i, _)| edge(line[..i].chars().next_back()) && edge(line[i + id.len()..].chars().next()))
}

/// The newest part, for a report.
pub fn tail(bytes: usize) -> Option<String> {
    let b = std::fs::read(FILE.get()?).ok()?;
    let part = &b[b.len().saturating_sub(bytes)..];
    let from = if part.len() < b.len() { part.iter().position(|c| *c == b'\n').map(|i| i + 1).unwrap_or(0) } else { 0 };
    Some(String::from_utf8_lossy(&part[from..]).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_games_lines_are_its_own() {
        assert!(mentions("12:00:00 UTC  Play sos-ps5: SonsOfSparta-PS5.exe", "sos-ps5"));
        assert!(mentions("12:00:00 UTC  Add to Steam (sos-ps5): written", "sos-ps5"));
        assert!(!mentions("12:00:00 UTC  Play sos-ps5-demo: x.exe", "sos-ps5"));
        assert!(!mentions("12:00:00 UTC  Play octr-nitrofueled-pc: Crash.exe", "nitrofueled"));
    }
}

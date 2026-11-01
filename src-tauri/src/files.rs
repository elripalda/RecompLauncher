//! The launcher's own file browser: drives, the usual folders, and what's in a folder, for picking
//! a disc image, a game folder or where games go. The window draws it (in TV mode too), so it
//! works with a controller and can say which disc images are games that have recomps.

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct Place {
    /// "desktop", "documents", "downloads", "home", "drive", "games" (a folder of the player's
    /// games) or "library" (where recomps are installed).
    pub kind: String,
    pub name: String,
    pub path: PathBuf,
    /// Drives: space left and in all, in bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub free: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
}

fn place(kind: &str, name: &str, path: PathBuf) -> Place {
    Place { kind: kind.into(), name: name.into(), path, free: None, total: None }
}

/// The drives on this computer: C:\, D:\ and so on, or / and the mounted disks elsewhere.
pub fn drives() -> Vec<Place> {
    let mut out = vec![];
    let mut add = |p: PathBuf, name: String| {
        let free = fs2::available_space(&p).ok();
        let total = fs2::total_space(&p).ok().filter(|&t| t > 0);
        if total.is_some() {
            out.push(Place { kind: "drive".into(), name, path: p, free, total });
        }
    };
    if cfg!(windows) {
        // A network drive that's gone can take many seconds to answer, so every letter is asked
        // at once and the ones that haven't answered in a second and a half are left out. A and B
        // are floppy letters: asking about an empty one can stall too.
        let (tx, rx) = std::sync::mpsc::channel();
        for c in b'C'..=b'Z' {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let p = PathBuf::from(format!("{}:\\", c as char));
                let ok = p.exists() && fs2::total_space(&p).map(|t| t > 0).unwrap_or(false);
                let _ = tx.send((c, ok));
            });
        }
        drop(tx);
        let until = std::time::Instant::now() + std::time::Duration::from_millis(1500);
        let mut found = vec![];
        while let Ok((c, ok)) = rx.recv_timeout(until.saturating_duration_since(std::time::Instant::now())) {
            if ok {
                found.push(c);
            }
        }
        found.sort();
        for c in found {
            add(PathBuf::from(format!("{}:\\", c as char)), format!("{}:", c as char));
        }
    } else {
        add(PathBuf::from("/"), "/".into());
        let user = std::env::var("USER").unwrap_or_default();
        let roots = [PathBuf::from("/Volumes"), PathBuf::from("/media").join(&user), PathBuf::from("/run/media").join(&user), PathBuf::from("/mnt")];
        for r in roots {
            for e in std::fs::read_dir(&r).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() && p != Path::new("/Volumes/Macintosh HD") {
                    add(p.clone(), e.file_name().to_string_lossy().into());
                }
            }
        }
    }
    out
}

/// The folders people keep things in.
pub fn quick() -> Vec<Place> {
    let mut out = vec![];
    for (kind, name, dir) in [("desktop", "Desktop", dirs::desktop_dir()), ("documents", "Documents", dirs::document_dir()), ("downloads", "Downloads", dirs::download_dir()), ("home", "Home", dirs::home_dir())] {
        if let Some(d) = dir.filter(|d| d.is_dir()) {
            out.push(place(kind, name, d));
        }
    }
    out
}

pub fn named(kind: &str, list: &[PathBuf]) -> Vec<Place> {
    list.iter().filter(|p| p.is_dir()).map(|p| place(kind, &p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.display().to_string()), p.clone())).collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub dir: bool,
    pub size: u64,
    /// Seconds since 1970.
    pub modified: u64,
    /// "folder", "disc" (a disc image), "rom" (a cartridge ROM), "game" (a folder with a game in
    /// it, like an extracted Xbox 360 game), "program" or "file".
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Listing {
    pub path: PathBuf,
    pub parent: Option<PathBuf>,
    pub entries: Vec<Entry>,
    /// The folder couldn't be read (no permission): an empty list, and this says so.
    pub denied: bool,
    /// More than fit: the rest aren't shown.
    pub more: bool,
}

const SHOW: usize = 2000;

fn kind_of(p: &Path, dir: bool) -> &'static str {
    if dir {
        return if p.join("default.xex").is_file() { "game" } else { "folder" };
    }
    let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "iso" | "xiso" | "gcm" | "rvz" | "wbfs" | "ciso" | "chd" | "img" | "bin" | "cue" => "disc",
        "z64" | "n64" | "v64" => "rom",
        "exe" | "appimage" | "app" | "sh" => "program",
        _ => "file",
    }
}

/// What's in a folder: folders first, then files, by name. Hidden and system files are left out.
pub fn list(path: &Path) -> Listing {
    let parent = path.parent().map(Path::to_path_buf);
    let rd = match std::fs::read_dir(path) {
        Ok(rd) => rd,
        Err(_) => return Listing { path: path.to_path_buf(), parent, entries: vec![], denied: true, more: false },
    };
    let mut entries = vec![];
    let mut more = false;
    // Every name first, so the ones shown are the first in order, not whichever came first.
    let mut all: Vec<std::fs::DirEntry> = rd.flatten().filter(|e| !e.file_name().to_string_lossy().starts_with('.') && !hidden(e)).collect();
    if all.len() > SHOW {
        more = true;
        let dir_first = |e: &std::fs::DirEntry| !e.file_type().map(|t| t.is_dir() || t.is_symlink()).unwrap_or(false);
        all.sort_by(|a, b| dir_first(a).cmp(&dir_first(b)).then_with(|| natural(&a.file_name().to_string_lossy(), &b.file_name().to_string_lossy())));
        all.truncate(SHOW);
    }
    for e in all {
        let name = e.file_name().to_string_lossy().to_string();
        let p = e.path();
        let Ok(meta) = std::fs::metadata(&p) else { continue };
        let modified = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
        let dir = meta.is_dir();
        entries.push(Entry { kind: kind_of(&p, dir).into(), name, path: p, dir, size: if dir { 0 } else { meta.len() }, modified });
    }
    entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| natural(&a.name, &b.name)));
    Listing { path: path.to_path_buf(), parent, entries, denied: false, more }
}

/// Windows' system files ($Recycle.Bin, pagefile.sys, System Volume Information). Folders that are
/// only hidden (AppData, ProgramData) stay: emulators and games keep their files there.
#[cfg(windows)]
fn hidden(e: &std::fs::DirEntry) -> bool {
    use std::os::windows::fs::MetadataExt;
    let name = e.file_name().to_string_lossy().to_string();
    name.starts_with('$') || e.metadata().map(|m| m.file_attributes() & 0x4 != 0).unwrap_or(false)
}
#[cfg(not(windows))]
fn hidden(_e: &std::fs::DirEntry) -> bool {
    false
}

/// Names in the order people expect: "Disc 2" before "Disc 10", any case.
pub fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    let key = |s: &str| -> Vec<(u8, String, u64)> {
        let mut out = vec![];
        let mut num = String::new();
        let mut txt = String::new();
        for c in s.chars() {
            if c.is_ascii_digit() {
                if !txt.is_empty() {
                    out.push((1, std::mem::take(&mut txt), 0));
                }
                num.push(c);
            } else {
                if !num.is_empty() {
                    out.push((0, String::new(), num.parse().unwrap_or(u64::MAX)));
                    num.clear();
                }
                txt.extend(c.to_lowercase());
            }
        }
        if !num.is_empty() {
            out.push((0, String::new(), num.parse().unwrap_or(u64::MAX)));
        }
        if !txt.is_empty() {
            out.push((1, txt, 0));
        }
        out
    };
    key(a).cmp(&key(b)).then_with(|| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order() {
        let mut v = vec!["Disc 10.iso", "disc 2.iso", "Alpha", "Disc 1.iso"];
        v.sort_by(|a, b| natural(a, b));
        assert_eq!(v, vec!["Alpha", "Disc 1.iso", "disc 2.iso", "Disc 10.iso"]);
    }

    #[test]
    fn lists_folders_first() {
        let d = std::env::temp_dir().join(format!("rl-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("B folder")).unwrap();
        std::fs::create_dir_all(d.join("Halo 3")).unwrap();
        std::fs::write(d.join("Halo 3").join("default.xex"), b"x").unwrap();
        std::fs::write(d.join("a.iso"), b"x").unwrap();
        std::fs::write(d.join("mario.z64"), b"x").unwrap();
        std::fs::write(d.join(".hidden"), b"x").unwrap();
        let l = list(&d);
        let names: Vec<_> = l.entries.iter().map(|e| (e.name.as_str(), e.kind.as_str())).collect();
        assert_eq!(names, vec![("B folder", "folder"), ("Halo 3", "game"), ("a.iso", "disc"), ("mario.z64", "rom")]);
        assert!(!l.denied);
        assert!(list(&d.join("nope")).denied);
    }
}

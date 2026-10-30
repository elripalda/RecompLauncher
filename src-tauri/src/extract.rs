//! Unpacking a player's disc image for recomps that read extracted files: Xbox and Xbox 360 disc
//! images (XDVDFS, "XISO"), and PlayStation 2 / PSP ones (ISO 9660). Only reading: the image is
//! never changed. Names are checked so nothing lands outside the folder it unpacks into.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const SECTOR: u64 = 2048;

struct Entry {
    path: PathBuf,
    offset: u64,
    size: u64,
}

fn read_at(f: &mut File, at: u64, len: usize) -> io::Result<Vec<u8>> {
    f.seek(SeekFrom::Start(at))?;
    let mut b = vec![0u8; len];
    f.read_exact(&mut b)?;
    Ok(b)
}

/// A name from the disc that's safe as one part of a path.
fn safe(name: &str) -> Option<String> {
    let n = name.trim_end_matches(['.', ' ']);
    if n.is_empty() || n == "." || n == ".." || n.contains(['/', '\\', ':', '\0']) || n.chars().any(|c| (c as u32) < 32) {
        return None;
    }
    Some(n.to_string())
}

fn u32le(b: &[u8], i: usize) -> u64 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as u64
}

/// Where an Xbox or Xbox 360 image's file system starts (the game partition on 360 discs).
fn xdvdfs_base(f: &mut File) -> Option<(u64, u64, u64)> {
    for part in [0u64, 0x2080000, 0xFD90000, 0x18300000] {
        let Ok(d) = read_at(f, part + 32 * SECTOR, 28) else { continue };
        if &d[..20] == b"MICROSOFT*XBOX*MEDIA" {
            return Some((part, u32le(&d, 20), u32le(&d, 24)));
        }
    }
    None
}

fn xdvdfs_list(f: &mut File, base: u64, sector: u64, size: u64, dir: &Path, depth: u32, out: &mut Vec<Entry>) -> io::Result<()> {
    if depth > 32 || size == 0 || size > 64 << 20 {
        return Ok(());
    }
    let t = read_at(f, base + sector * SECTOR, size as usize)?;
    let mut pos = 0usize;
    while pos + 14 <= t.len() {
        if t[pos] == 0xFF && t[pos + 1] == 0xFF {
            pos = (pos / SECTOR as usize + 1) * SECTOR as usize;
            continue;
        }
        let nl = t[pos + 13] as usize;
        if nl == 0 || pos + 14 + nl > t.len() {
            break;
        }
        let (start, len, attr) = (u32le(&t, pos + 4), u32le(&t, pos + 8), t[pos + 12]);
        let name = String::from_utf8_lossy(&t[pos + 14..pos + 14 + nl]).to_string();
        if let Some(n) = safe(&name) {
            let p = dir.join(&n);
            if attr & 0x10 != 0 {
                xdvdfs_list(f, base, start, len, &p, depth + 1, out)?;
                out.push(Entry { path: p, offset: u64::MAX, size: 0 });
            } else {
                out.push(Entry { path: p, offset: base + start * SECTOR, size: len });
            }
        }
        pos = (pos + 14 + nl + 3) & !3;
    }
    Ok(())
}

fn iso_list(f: &mut File, lba: u64, size: u64, dir: &Path, depth: u32, out: &mut Vec<Entry>) -> io::Result<()> {
    if depth > 32 || size == 0 || size > 64 << 20 {
        return Ok(());
    }
    let d = read_at(f, lba * SECTOR, size as usize)?;
    let mut i = 0usize;
    while i < d.len() {
        let len = d[i] as usize;
        if len == 0 {
            i = (i / SECTOR as usize + 1) * SECTOR as usize;
            continue;
        }
        if i + 33 > d.len() || i + len > d.len() {
            break;
        }
        let nl = d[i + 32] as usize;
        let raw = &d[i + 33..(i + 33 + nl).min(d.len())];
        let (start, sz, flags) = (u32le(&d, i + 2), u32le(&d, i + 10), d[i + 25]);
        if !(nl == 1 && (raw[0] == 0 || raw[0] == 1)) {
            let name = String::from_utf8_lossy(raw).split(';').next().unwrap_or("").to_string();
            if let Some(n) = safe(&name) {
                let p = dir.join(&n);
                if flags & 0x02 != 0 {
                    iso_list(f, start, sz, &p, depth + 1, out)?;
                    out.push(Entry { path: p, offset: u64::MAX, size: 0 });
                } else {
                    out.push(Entry { path: p, offset: start * SECTOR, size: sz });
                }
            }
        }
        i += len;
    }
    Ok(())
}

/// What's on the image: every file and folder, by `kind` ("xdvdfs" or "iso9660").
fn list(image: &Path, kind: &str) -> io::Result<Vec<Entry>> {
    let mut f = File::open(image)?;
    let mut out = vec![];
    match kind {
        "xdvdfs" => {
            let (base, sector, size) = xdvdfs_base(&mut f).ok_or_else(|| io::Error::other("That isn't an Xbox disc image."))?;
            xdvdfs_list(&mut f, base, sector, size, Path::new(""), 0, &mut out)?;
        }
        "iso9660" => {
            let pvd = read_at(&mut f, 16 * SECTOR, 2048)?;
            if &pvd[1..6] != b"CD001" {
                return Err(io::Error::other("That isn't a disc image the launcher can unpack."));
            }
            let root = &pvd[156..190];
            iso_list(&mut f, u32le(root, 2), u32le(root, 10), Path::new(""), 0, &mut out)?;
        }
        _ => return Err(io::Error::other("The launcher can't unpack that kind of disc image.")),
    }
    Ok(out)
}

/// How much unpacking takes, for checking there's room first.
pub fn size(image: &Path, kind: &str) -> io::Result<u64> {
    Ok(list(image, kind)?.iter().map(|e| e.size).sum())
}

/// Unpacks the image into `dest`. `progress(done, total)` is called as it goes; `stop()` true
/// ends it early (with an error).
pub fn unpack(image: &Path, kind: &str, dest: &Path, progress: &dyn Fn(u64, u64), stop: &dyn Fn() -> bool) -> io::Result<u64> {
    let entries = list(image, kind)?;
    let total: u64 = entries.iter().map(|e| e.size).sum();
    let mut f = File::open(image)?;
    std::fs::create_dir_all(dest)?;
    let mut buf = vec![0u8; 4 << 20];
    let (mut done, mut last) = (0u64, std::time::Instant::now());
    for e in entries.iter().filter(|e| e.offset == u64::MAX) {
        std::fs::create_dir_all(dest.join(&e.path))?;
    }
    for e in entries.iter().filter(|e| e.offset != u64::MAX) {
        let to = dest.join(&e.path);
        if let Some(p) = to.parent() {
            std::fs::create_dir_all(p)?;
        }
        // Already there from an earlier unpack, the same size: kept.
        if std::fs::metadata(&to).map(|m| m.len() == e.size).unwrap_or(false) {
            done += e.size;
            continue;
        }
        let mut out = io::BufWriter::new(File::create(&to)?);
        f.seek(SeekFrom::Start(e.offset))?;
        let mut left = e.size;
        while left > 0 {
            if stop() {
                return Err(io::Error::other("Stopped."));
            }
            let n = (left as usize).min(buf.len());
            f.read_exact(&mut buf[..n])?;
            out.write_all(&buf[..n])?;
            left -= n as u64;
            done += n as u64;
            if last.elapsed().as_millis() >= 100 {
                last = std::time::Instant::now();
                progress(done, total);
            }
        }
        out.flush()?;
    }
    progress(total, total);
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny Xbox disc image: default.xex and media/a.bin.
    fn xiso(path: &Path) {
        let mut img = vec![0u8; 64 * 2048];
        let vd = 32 * 2048;
        img[vd..vd + 20].copy_from_slice(b"MICROSOFT*XBOX*MEDIA");
        img[vd + 20..vd + 24].copy_from_slice(&40u32.to_le_bytes());
        img[vd + 24..vd + 28].copy_from_slice(&2048u32.to_le_bytes());
        let ent = |img: &mut Vec<u8>, at: usize, start: u32, size: u32, attr: u8, name: &str| {
            img[at..at + 4].copy_from_slice(&[0, 0, 0, 0]);
            img[at + 4..at + 8].copy_from_slice(&start.to_le_bytes());
            img[at + 8..at + 12].copy_from_slice(&size.to_le_bytes());
            img[at + 12] = attr;
            img[at + 13] = name.len() as u8;
            img[at + 14..at + 14 + name.len()].copy_from_slice(name.as_bytes());
        };
        let root = 40 * 2048;
        for b in &mut img[root..root + 2048] {
            *b = 0xFF;
        }
        ent(&mut img, root, 44, 5, 0x20, "default.xex");
        ent(&mut img, root + 28, 41, 2048, 0x10, "media");
        ent(&mut img, root + 48, 44, 1, 0x20, "..");
        let sub = 41 * 2048;
        for b in &mut img[sub..sub + 2048] {
            *b = 0xFF;
        }
        ent(&mut img, sub, 45, 3, 0x20, "a.bin");
        img[44 * 2048..44 * 2048 + 5].copy_from_slice(b"XEX2!");
        img[45 * 2048..45 * 2048 + 3].copy_from_slice(b"abc");
        std::fs::write(path, img).unwrap();
    }

    #[test]
    fn unpacks_xbox_images() {
        let d = std::env::temp_dir().join(format!("rl-xiso-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let img = d.join("game.iso");
        xiso(&img);
        assert_eq!(size(&img, "xdvdfs").unwrap(), 8);
        let out = d.join("out");
        unpack(&img, "xdvdfs", &out, &|_, _| {}, &|| false).unwrap();
        assert_eq!(std::fs::read(out.join("default.xex")).unwrap(), b"XEX2!");
        assert_eq!(std::fs::read(out.join("media/a.bin")).unwrap(), b"abc");
        assert!(!d.join("out/..").join("x").exists());
        assert!(unpack(&img, "iso9660", &out, &|_, _| {}, &|| false).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn unpacks_iso9660() {
        let d = std::env::temp_dir().join(format!("rl-iso-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let mut img = vec![0u8; 24 * 2048];
        let pvd = 16 * 2048;
        img[pvd] = 1;
        img[pvd + 1..pvd + 6].copy_from_slice(b"CD001");
        let rec = |img: &mut Vec<u8>, at: usize, lba: u32, size: u32, flags: u8, name: &[u8]| -> usize {
            let len = 33 + name.len() + (name.len() + 1) % 2;
            img[at] = len as u8;
            img[at + 2..at + 6].copy_from_slice(&lba.to_le_bytes());
            img[at + 10..at + 14].copy_from_slice(&size.to_le_bytes());
            img[at + 25] = flags;
            img[at + 32] = name.len() as u8;
            img[at + 33..at + 33 + name.len()].copy_from_slice(name);
            len
        };
        rec(&mut img, pvd + 156, 18, 2048, 2, &[0]);
        let mut at = 18 * 2048;
        at += rec(&mut img, at, 18, 2048, 2, &[0]);
        at += rec(&mut img, at, 18, 2048, 2, &[1]);
        at += rec(&mut img, at, 20, 4, 0, b"SYSTEM.CNF;1");
        rec(&mut img, at, 19, 2048, 2, b"DATA");
        rec(&mut img, 19 * 2048, 21, 2, 0, b"X.BIN;1");
        img[20 * 2048..20 * 2048 + 4].copy_from_slice(b"BOOT");
        img[21 * 2048..21 * 2048 + 2].copy_from_slice(b"ok");
        let iso = d.join("ps2.iso");
        std::fs::write(&iso, img).unwrap();
        let out = d.join("out");
        unpack(&iso, "iso9660", &out, &|_, _| {}, &|| false).unwrap();
        assert_eq!(std::fs::read(out.join("SYSTEM.CNF")).unwrap(), b"BOOT");
        assert_eq!(std::fs::read(out.join("DATA/X.BIN")).unwrap(), b"ok");
        let _ = std::fs::remove_dir_all(&d);
    }
}

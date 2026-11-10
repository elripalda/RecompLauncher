//! The build ID of a Nintendo Switch or Switch 2 `.nup` package (or of a folder with `exefs/main`), for
//! `game.check.nup_build_id`. A content check only: no keys, no extraction, nothing is copied.
//!
//! Layout (little-endian), as far as it is needed: `NUP\x01` at 0; a TOC at 0x80 (`TOC\x01`,
//! u32 titles, u64 dirs, u64 files, u64 strings offset from 0x80, u64 strings size); titles at
//! 0xb0, 0x40 bytes each (u64 id, u32 root, u32 exefs dir, u32 romfs dir, u32 version); then dirs
//! (24 bytes: u32 name offset, u32 name length, u32 first child, u32 children, u64), files
//! (80 bytes: u32 name offset, u32 name length, u64 size, u64 data offset, u64 stored size, hash,
//! u8 codec at +64: 0 raw, 1 zstd), the child list (u32 nodes: below the dir count a dir, else a
//! file) and the names. The newest title's `exefs/main` is an NSO whose header holds the module
//! (build) ID at 0x40..0x60, the same for plain, LZ4 and ZBIC NSOs. Only the table rows on the way
//! to that file and its first 0x60 bytes are read.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const NONE: u32 = u32::MAX;
const ZSTD: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];
const NSO_HEAD: usize = 0x60;

fn at<R: Read + Seek>(r: &mut R, pos: u64, n: usize) -> Option<Vec<u8>> {
    r.seek(SeekFrom::Start(pos)).ok()?;
    let mut b = vec![0u8; n];
    r.read_exact(&mut b).ok()?;
    Some(b)
}

fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

fn u64le(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

/// Whether the file starts like a `.nup` package (by its magic, never by its name).
pub fn is_package(path: &Path) -> bool {
    std::fs::File::open(path).ok().and_then(|mut f| at(&mut f, 0, 4)).map(|h| h == b"NUP\x01").unwrap_or(false)
}

/// The 32-byte build ID, as lowercase hex, from an NSO's first 0x60 bytes.
fn id_of_header(h: &[u8]) -> Result<String, String> {
    if h.len() < NSO_HEAD || &h[..4] != b"NSO0" {
        return Err("The package's exefs/main isn't a game executable.".into());
    }
    Ok(hex::encode(&h[0x40..0x60]))
}

/// The build ID of a `.nup` package: its newest title's `exefs/main`.
pub fn build_id_of_package(path: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("That package can't be opened: {e}"))?;
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    build_id_in(&mut f, len)
}

fn build_id_in<R: Read + Seek>(r: &mut R, len: u64) -> Result<String, String> {
    let bad = || "That package can't be read: it may be incomplete or not a Switch .nup.".to_string();
    let head = at(r, 0, 0x80).ok_or_else(bad)?;
    if &head[..4] != b"NUP\x01" {
        return Err("That isn't a Nintendo Switch .nup package.".into());
    }
    let declared = u64le(&head, 0x40);
    if declared != 0 && len < declared {
        return Err(format!("That .nup file is incomplete: it is {} MB and its header says {} MB. Copy or download it again.", len >> 20, declared >> 20));
    }
    let toc = at(r, 0x80, 0x30).ok_or_else(bad)?;
    if &toc[..4] != b"TOC\x01" {
        return Err(bad());
    }
    let nt = u32le(&toc, 4) as u64;
    let (nd, nf, soff, ssz) = (u64le(&toc, 8), u64le(&toc, 16), u64le(&toc, 24), u64le(&toc, 32));
    if nt == 0 || nt > 64 || nd == 0 || nd > 1 << 24 || nf > 1 << 24 || soff > len || ssz > len {
        return Err(bad());
    }
    let titles = at(r, 0xb0, (nt * 0x40) as usize).ok_or_else(bad)?;
    // The newest version (an update over the base game); the first of equals.
    let mut newest: Option<(u32, u32)> = None;
    for i in 0..nt as usize {
        let (exefs, version) = (u32le(&titles, i * 0x40 + 12), u32le(&titles, i * 0x40 + 20));
        if newest.map(|(_, v)| version > v).unwrap_or(true) {
            newest = Some((exefs, version));
        }
    }
    let exefs = newest.map(|(e, _)| e).unwrap_or(NONE);
    if exefs == NONE || exefs as u64 >= nd {
        return Err("That package has no exefs, so it isn't a game package this recomp reads.".into());
    }
    let dirs_at = 0xb0 + 0x40 * nt;
    let files_at = dirs_at + 24 * nd;
    let kids_at = files_at + 80 * nf;
    let strings_at = 0x80 + soff;
    let dir = at(r, dirs_at + 24 * exefs as u64, 24).ok_or_else(bad)?;
    let (first, count) = (u32le(&dir, 8) as u64, u32le(&dir, 12) as u64);
    if count > 4096 || kids_at > strings_at || first + count > (strings_at - kids_at) / 4 {
        return Err(bad());
    }
    let kids = at(r, kids_at + 4 * first, (count * 4) as usize).ok_or_else(bad)?;
    for k in 0..count as usize {
        let node = u32le(&kids, k * 4) as u64;
        if node < nd || node >= nd + nf {
            continue;
        }
        let rec = at(r, files_at + 80 * (node - nd), 80).ok_or_else(bad)?;
        let (name_off, name_len) = (u32le(&rec, 0) as u64, u32le(&rec, 4) as u64);
        if name_len != 4 || name_off + name_len > ssz {
            continue;
        }
        if at(r, strings_at + name_off, 4).as_deref() != Some(b"main") {
            continue;
        }
        let (size, off, stored, codec) = (u64le(&rec, 8), u64le(&rec, 16), u64le(&rec, 24), rec[64]);
        return read_main(r, len, size, off, stored, codec);
    }
    Err("That package has no exefs/main.".into())
}

/// The first 0x60 bytes of the stored `main`: as is, or the start of a zstd frame (the package's
/// own compression, not the NSO's), decoded only as far as needed.
fn read_main<R: Read + Seek>(r: &mut R, len: u64, size: u64, off: u64, stored: u64, codec: u8) -> Result<String, String> {
    let bad = || "The package's exefs/main can't be read: the package may be incomplete.".to_string();
    if off >= len {
        return Err(bad());
    }
    let first = at(r, off, 4).ok_or_else(bad)?;
    if codec == 0 && first == b"NSO0" {
        if size < NSO_HEAD as u64 {
            return Err(bad());
        }
        return id_of_header(&at(r, off, NSO_HEAD).ok_or_else(bad)?);
    }
    if codec != 1 && first != ZSTD {
        return id_of_header(&first);
    }
    let limit = if codec == 1 { stored } else { size.saturating_add(1 << 16) }.min(len - off);
    r.seek(SeekFrom::Start(off)).map_err(|_| bad())?;
    let mut d = zstd::stream::read::Decoder::new(r.by_ref().take(limit)).map_err(|_| bad())?;
    let mut h = [0u8; NSO_HEAD];
    d.read_exact(&mut h).map_err(|_| bad())?;
    id_of_header(&h)
}

/// The build ID of a folder with `exefs/main` (an extracted package).
pub fn build_id_of_folder(dir: &Path) -> Result<String, String> {
    let main = dir.join("exefs").join("main");
    let mut f = std::fs::File::open(&main).map_err(|_| "That folder doesn't have exefs/main.".to_string())?;
    let h = at(&mut f, 0, NSO_HEAD).ok_or_else(|| "The folder's exefs/main isn't a game executable.".to_string())?;
    id_of_header(&h)
}

/// Whether a build ID (hex) is the one wanted: the wanted ID may be a prefix (8 hex digits is
/// what a recomp usually states). Err when the wanted ID isn't hex of at least 8 digits.
pub fn matches(have: &str, want: &str) -> Result<bool, String> {
    let w = want.trim().to_ascii_lowercase();
    if w.len() < 8 || !w.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("This recomp's nup_build_id \"{want}\" isn't a build ID (at least 8 hex digits)."));
    }
    Ok(have.starts_with(&w))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Cursor;

    const ID: [u8; 32] = [
        0xe0, 0xf3, 0xcc, 0xfa, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28,
    ];

    pub(crate) fn nso(id: &[u8; 32]) -> Vec<u8> {
        let mut n = vec![0u8; 0x100];
        n[..4].copy_from_slice(b"NSO0");
        n[0x40..0x60].copy_from_slice(id);
        n
    }

    /// A synthetic package: title versions with their `exefs` files (name, bytes, codec).
    pub(crate) fn package(titles: &[(u32, Vec<(&str, Vec<u8>, u8)>)]) -> Vec<u8> {
        // Nodes: dir 0 = root; per title one exefs dir. Files after the dirs.
        let nd = 1 + titles.len();
        let nf: usize = titles.iter().map(|t| t.1.len()).sum();
        let mut strings = Vec::new();
        let mut data_blobs: Vec<Vec<u8>> = Vec::new();
        let mut file_recs: Vec<(u32, u32, u64, u8)> = Vec::new(); // name off, len, size, codec
        let mut dir_kids: Vec<Vec<u32>> = Vec::new();
        let mut next_file = 0u32;
        for t in titles {
            let mut kids = Vec::new();
            for (name, bytes, codec) in &t.1 {
                file_recs.push((strings.len() as u32, name.len() as u32, bytes.len() as u64, *codec));
                strings.extend_from_slice(name.as_bytes());
                data_blobs.push(bytes.clone());
                kids.push(nd as u32 + next_file);
                next_file += 1;
            }
            dir_kids.push(kids);
        }
        let nt = titles.len();
        let kid_count: usize = dir_kids.iter().map(|k| k.len()).sum();
        let soff = 0x30 + 0x40 * nt + 24 * nd + 80 * nf + 4 * kid_count;
        let meta_end = 0x80 + soff + strings.len();
        let blob_start = (meta_end + 15) & !15;
        let mut out = vec![0u8; blob_start];
        out[..4].copy_from_slice(b"NUP\x01");
        out[0x80..0x84].copy_from_slice(b"TOC\x01");
        out[0x84..0x88].copy_from_slice(&(nt as u32).to_le_bytes());
        out[0x88..0x90].copy_from_slice(&(nd as u64).to_le_bytes());
        out[0x90..0x98].copy_from_slice(&(nf as u64).to_le_bytes());
        out[0x98..0xa0].copy_from_slice(&(soff as u64).to_le_bytes());
        out[0xa0..0xa8].copy_from_slice(&(strings.len() as u64).to_le_bytes());
        for (i, t) in titles.iter().enumerate() {
            let o = 0xb0 + 0x40 * i;
            out[o..o + 8].copy_from_slice(&(0x0100_0000_0000_0000u64 + i as u64).to_le_bytes());
            out[o + 8..o + 12].copy_from_slice(&0u32.to_le_bytes()); // root
            out[o + 12..o + 16].copy_from_slice(&(1 + i as u32).to_le_bytes()); // exefs
            out[o + 16..o + 20].copy_from_slice(&NONE.to_le_bytes()); // romfs
            out[o + 20..o + 24].copy_from_slice(&t.0.to_le_bytes());
        }
        let dirs_at = 0xb0 + 0x40 * nt;
        let mut first = 0u32;
        for (i, kids) in dir_kids.iter().enumerate() {
            let o = dirs_at + 24 * (1 + i);
            out[o + 8..o + 12].copy_from_slice(&first.to_le_bytes());
            out[o + 12..o + 16].copy_from_slice(&(kids.len() as u32).to_le_bytes());
            first += kids.len() as u32;
        }
        let files_at = dirs_at + 24 * nd;
        let mut pos = blob_start as u64;
        for (i, (no, nl, size, codec)) in file_recs.iter().enumerate() {
            let o = files_at + 80 * i;
            let stored = data_blobs[i].len() as u64;
            out[o..o + 4].copy_from_slice(&no.to_le_bytes());
            out[o + 4..o + 8].copy_from_slice(&nl.to_le_bytes());
            out[o + 8..o + 16].copy_from_slice(&size.to_le_bytes());
            out[o + 16..o + 24].copy_from_slice(&pos.to_le_bytes());
            out[o + 24..o + 32].copy_from_slice(&stored.to_le_bytes());
            out[o + 64] = *codec;
            pos += stored;
        }
        let kids_at = files_at + 80 * nf;
        for (j, k) in dir_kids.iter().flatten().enumerate() {
            out[kids_at + 4 * j..kids_at + 4 * j + 4].copy_from_slice(&k.to_le_bytes());
        }
        out[0x80 + soff..0x80 + soff + strings.len()].copy_from_slice(&strings);
        for b in &data_blobs {
            out.extend_from_slice(b);
        }
        let total = out.len() as u64;
        out[0x40..0x48].copy_from_slice(&total.to_le_bytes());
        out
    }

    fn read(bytes: &[u8]) -> Result<String, String> {
        build_id_in(&mut Cursor::new(bytes), bytes.len() as u64)
    }

    #[test]
    fn reads_a_raw_main() {
        let p = package(&[(0, vec![("main", nso(&ID), 0)])]);
        assert_eq!(read(&p).unwrap(), hex::encode(ID));
        assert!(matches(&read(&p).unwrap(), "E0F3CCFA").unwrap());
        assert!(!matches(&read(&p).unwrap(), "e0f3ccfb").unwrap());
        assert!(matches("ab", "xyz").is_err() && matches("ab", "abc").is_err());
    }

    #[test]
    fn reads_a_zstd_stored_main() {
        let z = zstd::encode_all(&nso(&ID)[..], 3).unwrap();
        let p = package(&[(0, vec![("rtld", nso(&[9; 32]), 0), ("main", z.clone(), 1)])]);
        assert_eq!(read(&p).unwrap(), hex::encode(ID));
        // The same frame under a raw codec byte is recognised by its magic.
        let p = package(&[(0, vec![("main", z, 0)])]);
        // (its listed size is the stored length here, which is at least the frame)
        assert_eq!(read(&p).unwrap(), hex::encode(ID));
    }

    #[test]
    fn uses_the_newest_title() {
        let mut newer = ID;
        newer[0] = 0x11;
        let p = package(&[(1, vec![("main", nso(&ID), 0)]), (5, vec![("main", nso(&newer), 0)])]);
        assert!(read(&p).unwrap().starts_with("11f3ccfa"));
    }

    #[test]
    fn refuses_what_is_not_a_package_or_a_game() {
        assert!(read(b"not a package at all, definitely not one, padded out to a header size........................................................................").unwrap_err().contains("isn't"));
        assert!(read(&package(&[(0, vec![("rtld", nso(&ID), 0)])])).unwrap_err().contains("no exefs/main"));
        assert!(read(&package(&[(0, vec![("main", vec![7u8; 0x100], 0)])])).unwrap_err().contains("game executable"));
        let p = package(&[(0, vec![("main", nso(&ID), 0)])]);
        assert!(read(&p[..p.len() - 100]).unwrap_err().contains("incomplete"));
        assert!(read(&p[..0x90]).is_err());
    }

    #[test]
    fn reads_a_folder_with_exefs_main() {
        let d = std::env::temp_dir().join(format!("rl-nup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        assert!(build_id_of_folder(&d).unwrap_err().contains("exefs/main"));
        std::fs::create_dir_all(d.join("exefs")).unwrap();
        std::fs::write(d.join("exefs").join("main"), nso(&ID)).unwrap();
        assert_eq!(build_id_of_folder(&d).unwrap(), hex::encode(ID));
        std::fs::write(d.join("exefs").join("main"), [0u8; 200]).unwrap();
        assert!(build_id_of_folder(&d).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}

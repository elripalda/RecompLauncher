//! An Xbox 360 game's own achievement table, read from the player's default.xex: titles,
//! descriptions and gamerscore, plus the title id. Nothing else from the game files is kept or
//! sent anywhere.
//!
//! XEX2: the optional headers give the file format (encryption, compression) and the execution
//! info (title id). A retail xex is AES-128 encrypted with a per-file key, itself encrypted with
//! the retail key. Disc games are mostly "basic" compressed (data, then zeros); XBLA games LZX.
//! In the image, the XDBF resource holds XACH (achievements) and XSTR (strings per language).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const RETAIL_KEY: [u8; 16] = [0x20, 0xB1, 0x85, 0xA5, 0x9D, 0x28, 0xFD, 0xC3, 0x40, 0x58, 0x3F, 0xBB, 0x08, 0x96, 0xBF, 0x91];
/// The newest default.xex is at most this big (disc games run to ~20 MB).
const MAX_XEX: u64 = 96 << 20;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Table {
    pub title_id: String,
    pub achievements: Vec<Achievement>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Achievement {
    pub id: u32,
    pub title: String,
    pub description: String,
    pub gamerscore: u32,
}

impl Table {
    /// As session-start's "native".
    pub fn native(&self) -> Value {
        json!(self.achievements)
    }
}

fn be32(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}
fn be16(d: &[u8], o: usize) -> Option<u16> {
    d.get(o..o + 2).map(|b| u16::from_be_bytes([b[0], b[1]]))
}

/// The game's achievement table, from a default.xex's bytes. Retail discs are encrypted with the
/// retail key; a few builds use the development key (all zeros): tried when the first gives nothing.
pub fn read(d: &[u8]) -> Result<Table, String> {
    match read_with(d, &RETAIL_KEY) {
        Ok(t) => Ok(t),
        Err(e) => read_with(d, &[0u8; 16]).map_err(|_| e),
    }
}

fn read_with(d: &[u8], key: &[u8; 16]) -> Result<Table, String> {
    if d.get(..4) != Some(b"XEX2") {
        return Err("not an XEX2 file".into());
    }
    let pe_off = be32(d, 8).ok_or("short header")? as usize;
    let sec_off = be32(d, 16).ok_or("short header")? as usize;
    let n = be32(d, 20).ok_or("short header")? as usize;
    let (mut fmt, mut exec) = (None, None);
    for i in 0..n.min(256) {
        let k = be32(d, 24 + i * 8).ok_or("short header")?;
        let v = be32(d, 28 + i * 8).ok_or("short header")?;
        match k {
            0x0000_03FF => fmt = Some(v as usize),
            0x0004_0006 => exec = Some(v as usize),
            _ => {}
        }
    }
    let title_id = exec.and_then(|e| be32(d, e + 12)).map(|t| format!("{t:08X}")).unwrap_or_default();
    let fi = fmt.ok_or("no file format header")?;
    let info_size = be32(d, fi).ok_or("bad format header")? as usize;
    let enc = be16(d, fi + 4).ok_or("bad format header")?;
    let comp = be16(d, fi + 6).ok_or("bad format header")?;
    let image_size = be32(d, sec_off + 4).ok_or("bad security info")? as usize;
    let payload = d.get(pe_off..).ok_or("no payload")?;
    let plain = if enc != 0 {
        let key_enc: [u8; 16] = d.get(sec_off + 0x150..sec_off + 0x160).ok_or("no key")?.try_into().unwrap();
        let session = aes_ecb_decrypt_block(key, key_enc);
        aes_cbc_decrypt(&session, &payload[..payload.len() / 16 * 16])
    } else {
        payload.to_vec()
    };
    let image = match comp {
        0 => plain,
        1 => {
            let mut img = Vec::with_capacity(image_size.min(256 << 20));
            let mut p = 0usize;
            for i in 0..info_size.saturating_sub(8) / 8 {
                let ds = be32(d, fi + 8 + i * 8).ok_or("bad block list")? as usize;
                let zs = be32(d, fi + 12 + i * 8).ok_or("bad block list")? as usize;
                img.extend_from_slice(plain.get(p..p + ds).ok_or("block past the end")?);
                p += ds;
                img.resize(img.len() + zs, 0);
                if img.len() > 512 << 20 {
                    return Err("image too big".into());
                }
            }
            img
        }
        2 => {
            let window = be32(d, fi + 8).ok_or("bad LZX header")?;
            let first = be32(d, fi + 12).ok_or("bad LZX header")? as usize;
            lzx_image(&plain, window, first, image_size)?
        }
        c => return Err(format!("compression {c} isn't read")),
    };
    let mut t = xdbf(&image)?;
    t.title_id = title_id;
    Ok(t)
}

fn aes_ecb_decrypt_block(key: &[u8; 16], block: [u8; 16]) -> [u8; 16] {
    use aes::cipher::{generic_array::GenericArray, BlockDecrypt, KeyInit};
    let c = aes::Aes128::new(GenericArray::from_slice(key));
    let mut b = GenericArray::clone_from_slice(&block);
    c.decrypt_block(&mut b);
    b.into()
}

/// AES-128-CBC with a zero IV.
fn aes_cbc_decrypt(key: &[u8; 16], data: &[u8]) -> Vec<u8> {
    use aes::cipher::{generic_array::GenericArray, BlockDecrypt, KeyInit};
    let c = aes::Aes128::new(GenericArray::from_slice(key));
    let mut out = data.to_vec();
    let mut prev = [0u8; 16];
    for chunk in out.chunks_exact_mut(16) {
        let ct: [u8; 16] = chunk.try_into().unwrap();
        let b = GenericArray::from_mut_slice(chunk);
        c.decrypt_block(b);
        for (x, p) in chunk.iter_mut().zip(prev.iter()) {
            *x ^= p;
        }
        prev = ct;
    }
    out
}

/// LZX: blocks (next block's size, a 20-byte hash, then u16-sized chunks until a 0), each chunk one
/// compressed frame of up to 32 KB.
fn lzx_image(plain: &[u8], window: u32, first: usize, image_size: usize) -> Result<Vec<u8>, String> {
    let ws = match window {
        0x8000 => lzxd::WindowSize::KB32,
        0x10000 => lzxd::WindowSize::KB64,
        0x20000 => lzxd::WindowSize::KB128,
        0x40000 => lzxd::WindowSize::KB256,
        0x80000 => lzxd::WindowSize::KB512,
        0x100000 => lzxd::WindowSize::MB1,
        0x200000 => lzxd::WindowSize::MB2,
        w => return Err(format!("LZX window {w:#x} isn't read")),
    };
    let mut chunks: Vec<&[u8]> = vec![];
    let (mut p, mut size) = (0usize, first);
    while size != 0 {
        let blk = plain.get(p..p + size).ok_or("LZX block past the end")?;
        let next = be32(blk, 0).ok_or("short LZX block")? as usize;
        let mut q = 24;
        while q + 2 <= blk.len() {
            let cs = be16(blk, q).unwrap() as usize;
            q += 2;
            if cs == 0 {
                break;
            }
            chunks.push(blk.get(q..q + cs).ok_or("LZX chunk past the end")?);
            q += cs;
        }
        p += size;
        size = next;
        if chunks.len() > 1 << 16 {
            return Err("too many LZX chunks".into());
        }
    }
    let mut lz = lzxd::Lzxd::new(ws);
    let mut out = Vec::with_capacity(image_size);
    for c in chunks {
        let want = (image_size - out.len()).min(0x8000);
        if want == 0 {
            break;
        }
        let got = lz.decompress_next(c, want).map_err(|e| format!("LZX: {e}"))?;
        out.extend_from_slice(got);
    }
    Ok(out)
}

/// The XDBF resource: entries, then XACH (36-byte records) and the English XSTR (id 1). The
/// letters "XDBF" can turn up elsewhere in a game's code too: every place is tried, the first real
/// one with achievements wins.
pub fn xdbf(img: &[u8]) -> Result<Table, String> {
    let mut first_err = None;
    let mut at = 0usize;
    while let Some(i) = img[at..].windows(4).position(|w| w == b"XDBF") {
        match xdbf_at(&img[at + i..]) {
            Ok(t) => return Ok(t),
            Err(e) => {
                first_err.get_or_insert(e);
            }
        }
        at += i + 4;
    }
    Err(first_err.unwrap_or_else(|| "no XDBF in the image".into()))
}

fn xdbf_at(x: &[u8]) -> Result<Table, String> {
    let table_len = be32(x, 8).ok_or("short XDBF")? as usize;
    let count = be32(x, 12).ok_or("short XDBF")? as usize;
    let free_len = be32(x, 16).ok_or("short XDBF")? as usize;
    let entries = 24usize;
    let data = entries + table_len * 18 + free_len * 8;
    let mut xach: Option<&[u8]> = None;
    let mut xstr: Option<&[u8]> = None;
    for i in 0..count.min(table_len).min(4096) {
        let e = entries + i * 18;
        let id = x.get(e + 2..e + 10).map(|b| u64::from_be_bytes(b.try_into().unwrap())).ok_or("short XDBF entry")?;
        let off = be32(x, e + 10).ok_or("short XDBF entry")? as usize;
        let len = be32(x, e + 14).ok_or("short XDBF entry")? as usize;
        let Some(v) = x.get(data + off..data + off + len) else { continue };
        match v.get(..4) {
            Some(b"XACH") if xach.is_none() => xach = Some(v),
            Some(b"XSTR") if id == 1 || xstr.is_none() => xstr = Some(v),
            _ => {}
        }
    }
    let xach = xach.ok_or("no achievements (XACH) in the game")?;
    let mut strings = std::collections::HashMap::new();
    if let Some(s) = xstr {
        let n = be16(s, 12).unwrap_or(0) as usize;
        let mut p = 14;
        for _ in 0..n {
            let (Some(sid), Some(ln)) = (be16(s, p), be16(s, p + 2)) else { break };
            p += 4;
            let Some(txt) = s.get(p..p + ln as usize) else { break };
            strings.insert(sid, String::from_utf8_lossy(txt).into_owned());
            p += ln as usize;
        }
    }
    let cnt = be16(xach, 12).ok_or("short XACH")? as usize;
    let mut out = vec![];
    for i in 0..cnt.min(1000) {
        let r = 14 + i * 36;
        let (Some(id), Some(label), Some(desc), Some(gs)) = (be16(xach, r), be16(xach, r + 2), be16(xach, r + 4), be16(xach, r + 12)) else { break };
        out.push(Achievement { id: id as u32, title: strings.get(&label).cloned().unwrap_or_default(), description: strings.get(&desc).cloned().unwrap_or_default(), gamerscore: gs as u32 });
    }
    Ok(Table { title_id: String::new(), achievements: out })
}

// ---------- finding it, once per game and version ----------

/// An Xbox disc image by its name (.iso or .xiso).
pub fn is_disc_image(p: &Path) -> bool {
    p.extension().map(|e| e.eq_ignore_ascii_case("iso") || e.eq_ignore_ascii_case("xiso")).unwrap_or(false)
}

fn is_default_xex(p: &Path) -> bool {
    p.file_name().map(|n| n.eq_ignore_ascii_case("default.xex")).unwrap_or(false)
}

/// Where the game's default.xex is, from the places its files can be: a default.xex, an Xbox disc
/// image (read straight from the image), or a folder (default.xex in it or a few levels down, else
/// a disc image at its top, where recomps like SmackDown vs. Raw 2007 look first).
pub fn find(roots: &[PathBuf]) -> Option<PathBuf> {
    for r in roots {
        if r.is_file() {
            if is_default_xex(r) || (is_disc_image(r) && crate::extract::xbox_root_file(r, "default.xex", MAX_XEX).ok().flatten().is_some()) {
                return Some(r.clone());
            }
            continue;
        }
        for direct in [r.join("default.xex"), r.join("game").join("default.xex")] {
            if direct.is_file() {
                return Some(direct);
            }
        }
        let mut stack = vec![(r.clone(), 0usize)];
        while let Some((d, depth)) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten().take(400) {
                let p = e.path();
                let ty = e.file_type();
                if ty.as_ref().map(|t| t.is_file()).unwrap_or(false) && is_default_xex(&p) {
                    return Some(p);
                }
                if depth < 4 && ty.map(|t| t.is_dir()).unwrap_or(false) {
                    stack.push((p, depth + 1));
                }
            }
        }
        for e in std::fs::read_dir(r).into_iter().flatten().flatten().take(400) {
            let p = e.path();
            if p.is_file() && is_disc_image(&p) && crate::extract::xbox_root_file(&p, "default.xex", MAX_XEX).ok().flatten().is_some() {
                return Some(p);
            }
        }
    }
    None
}

/// Like find, but only exact places: a default.xex, a disc image, or a folder with default.xex at its
/// top (or in game/). For paths read from settings or from what a running game has open, where
/// searching around could land on another game's copy.
pub fn find_exact(places: &[PathBuf]) -> Option<PathBuf> {
    for p in places {
        if p.is_file() {
            if is_default_xex(p) || (is_disc_image(p) && crate::extract::xbox_root_file(p, "default.xex", MAX_XEX).ok().flatten().is_some()) {
                return Some(p.clone());
            }
        } else {
            for direct in [p.join("default.xex"), p.join("game").join("default.xex")] {
                if direct.is_file() {
                    return Some(direct);
                }
            }
        }
    }
    None
}

/// The game a file belongs to, from any file a running game has open: a disc image or default.xex
/// itself, else the nearest folder above it with a default.xex (a few levels up at most).
pub fn game_for_open_file(p: &Path) -> Option<PathBuf> {
    if is_disc_image(p) || is_default_xex(p) {
        return Some(p.to_path_buf());
    }
    let mut d = p.parent();
    for _ in 0..8 {
        let dir = d?;
        if dir.join("default.xex").is_file() {
            return Some(dir.to_path_buf());
        }
        d = dir.parent();
    }
    None
}

/// The table from a place the player picked: a disc image, a default.xex, or a folder with one.
pub fn table_from_place(place: &Path) -> Result<(PathBuf, Table), String> {
    let src = find_exact(&[place.to_path_buf()]).or_else(|| find(&[place.to_path_buf()])).ok_or("no Xbox 360 game there")?;
    let bytes = xex_bytes(&src).ok_or("couldn't read default.xex")?;
    read(&bytes).map(|t| (src, t))
}

/// default.xex's bytes, from the file itself or from inside a disc image.
fn xex_bytes(src: &Path) -> Option<Vec<u8>> {
    if is_disc_image(src) {
        return crate::extract::xbox_root_file(src, "default.xex", MAX_XEX).ok().flatten();
    }
    std::fs::read(src).ok()
}

#[derive(Serialize, Deserialize)]
struct Cached {
    size: u64,
    modified: u64,
    sha256: String,
    table: Option<Table>,
    #[serde(default)]
    error: Option<String>,
}

const CACHE: &str = "recomplauncher-achievements.json";

/// The table for a game, from its cache next to the install when default.xex hasn't changed,
/// else read and cached. None when there's no default.xex or no table in it.
pub fn table_for(install: &Path, roots: &[PathBuf], exact: &[PathBuf]) -> Option<Table> {
    let xex = find(roots).or_else(|| find_exact(exact))?;
    let meta = std::fs::metadata(&xex).ok()?;
    if meta.len() > MAX_XEX && !is_disc_image(&xex) {
        return None;
    }
    let modified = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    let cache = install.join(CACHE);
    if let Some(c) = std::fs::read(&cache).ok().and_then(|b| serde_json::from_slice::<Cached>(&b).ok()) {
        // A read that failed before is tried again: the reader may know more now.
        if c.size == meta.len() && c.modified == modified && c.table.is_some() {
            return c.table;
        }
    }
    let bytes = xex_bytes(&xex)?;
    let sha256 = {
        use sha2::Digest;
        sha2::Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect::<String>()
    };
    if let Some(c) = std::fs::read(&cache).ok().and_then(|b| serde_json::from_slice::<Cached>(&b).ok()) {
        if c.sha256 == sha256 && c.table.is_some() {
            let _ = std::fs::write(&cache, serde_json::to_vec_pretty(&Cached { size: meta.len(), modified, ..c }).unwrap_or_default());
            return std::fs::read(&cache).ok().and_then(|b| serde_json::from_slice::<Cached>(&b).ok()).and_then(|c| c.table);
        }
    }
    let r = read(&bytes);
    let (table, error) = match r {
        Ok(t) => (Some(t), None),
        Err(e) => (None, Some(e)),
    };
    let _ = std::fs::write(&cache, serde_json::to_vec_pretty(&Cached { size: meta.len(), modified, sha256, table: table.clone(), error: error.clone() }).unwrap_or_default());
    if let Some(e) = error {
        crate::trail::note(format!("Achievements: couldn't read the game's achievement table ({e})"));
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The player's own game files aren't in the repo: set RL_TEST_XEX_DIR to a folder with
    /// fancypants.xex, mlb2k7.xex and bigs2.xex to check against them.
    fn game(name: &str) -> Option<Vec<u8>> {
        let d = std::env::var_os("RL_TEST_XEX_DIR")?;
        std::fs::read(PathBuf::from(d).join(name)).ok()
    }

    #[test]
    fn an_xbla_game_with_lzx() {
        let Some(d) = game("fancypants.xex") else { return };
        let t = read(&d).unwrap();
        assert_eq!(t.title_id, "58410A9F");
        assert_eq!(t.achievements.len(), 12);
        assert_eq!(t.achievements.iter().map(|a| a.id).min(), Some(13));
        assert_eq!(t.achievements.iter().map(|a| a.id).max(), Some(24));
        assert!(t.achievements.iter().all(|a| !a.title.is_empty()));
    }

    #[test]
    fn a_disc_game_with_basic_compression() {
        let Some(d) = game("mlb2k7.xex") else { return };
        let t = read(&d).unwrap();
        assert_eq!(t.title_id, "545407FA");
        assert_eq!(t.achievements.len(), 33);
    }

    #[test]
    fn the_bigs_2() {
        let Some(d) = game("bigs2.xex") else { return };
        let t = read(&d).unwrap();
        assert_eq!(t.title_id, "54540830");
        assert_eq!(t.achievements.len(), 37);
        assert_eq!(t.achievements.iter().map(|a| a.gamerscore).sum::<u32>(), 1000);
        let a = t.achievements.iter().find(|a| a.id == 21).unwrap();
        assert_eq!((a.title.as_str(), a.gamerscore), ("Taste of Power", 10));
    }

    #[test]
    fn a_made_up_xdbf_is_read() {
        // XDBF with one XACH entry (1 achievement) and one XSTR (id 1) with two strings.
        let mut xach = b"XACH".to_vec();
        xach.extend_from_slice(&[0, 0, 0, 1, 0, 0, 0, 0]);
        xach.extend_from_slice(&1u16.to_be_bytes());
        let mut rec = vec![0u8; 36];
        rec[0..2].copy_from_slice(&7u16.to_be_bytes());
        rec[2..4].copy_from_slice(&1u16.to_be_bytes());
        rec[4..6].copy_from_slice(&2u16.to_be_bytes());
        rec[12..14].copy_from_slice(&25u16.to_be_bytes());
        xach.extend_from_slice(&rec);
        let mut xstr = b"XSTR".to_vec();
        xstr.extend_from_slice(&[0, 0, 0, 1, 0, 0, 0, 0]);
        xstr.extend_from_slice(&2u16.to_be_bytes());
        for (id, s) in [(1u16, "Hello"), (2u16, "Do a thing")] {
            xstr.extend_from_slice(&id.to_be_bytes());
            xstr.extend_from_slice(&(s.len() as u16).to_be_bytes());
            xstr.extend_from_slice(s.as_bytes());
        }
        let mut x = b"junkXDBF".to_vec();
        let mut h = vec![0u8; 20];
        h[4..8].copy_from_slice(&2u32.to_be_bytes()); // table length
        h[8..12].copy_from_slice(&2u32.to_be_bytes()); // entries
        x.extend_from_slice(&h);
        let entry = |ns: u16, id: u64, off: u32, len: u32| { let mut e = ns.to_be_bytes().to_vec(); e.extend_from_slice(&id.to_be_bytes()); e.extend_from_slice(&off.to_be_bytes()); e.extend_from_slice(&len.to_be_bytes()); e };
        x.extend(entry(1, 0x58414348, 0, xach.len() as u32));
        x.extend(entry(2, 1, xach.len() as u32, xstr.len() as u32));
        x.extend(&xach);
        x.extend(&xstr);
        let t = xdbf(&x).unwrap();
        assert_eq!(t.achievements, vec![Achievement { id: 7, title: "Hello".into(), description: "Do a thing".into(), gamerscore: 25 }]);
    }

    #[test]
    fn default_xex_is_found_next_to_the_exe() {
        let d = std::env::temp_dir().join(format!("rl-xex-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("game")).unwrap();
        std::fs::write(d.join("game").join("default.xex"), b"XEX2").unwrap();
        assert_eq!(find(&[d.clone()]), Some(d.join("game").join("default.xex")));
        assert_eq!(find(&[d.join("nope")]), None);
        let _ = std::fs::remove_dir_all(&d);
    }
}

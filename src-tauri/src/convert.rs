//! A compressed copy of a game, made plain for a recomp that reads plain files: GameCube and Wii
//! images (RVZ, WIA, WBFS, GCZ, CISO, TGC, NKit) to .iso, CHD to .iso (a DVD) or .bin and .cue (a
//! CD), and PSP or PS2 CSO and ZSO to .iso. Everything happens here, on any system, with nothing to
//! download. The player's own file stays as it is; the plain one goes beside it.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Which reader a file needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    /// GameCube and Wii images.
    Nintendo,
    /// MAME's compressed hunks of data (CD or DVD).
    Chd,
    /// PSP and PS2 compressed ISOs (deflate or LZ4 blocks).
    Cso,
}

/// The reader a file needs, by its name; None for a plain file.
pub fn kind_of(p: &Path) -> Option<Kind> {
    let name = p.file_name()?.to_string_lossy().to_ascii_lowercase();
    if name.ends_with(".nkit.iso") || name.ends_with(".nkit.gcz") {
        return Some(Kind::Nintendo);
    }
    match name.rsplit('.').next()? {
        "rvz" | "wia" | "wbfs" | "gcz" | "ciso" | "tgc" => Some(Kind::Nintendo),
        "chd" => Some(Kind::Chd),
        "cso" | "zso" => Some(Kind::Cso),
        _ => None,
    }
}

/// Where the plain copy goes (beside the original, named after it) and about how big it gets.
pub fn plan(src: &Path) -> Result<(PathBuf, u64), String> {
    let kind = kind_of(src).ok_or("That file isn't one the launcher can convert.")?;
    let dir = src.parent().ok_or("That file has no folder.")?;
    let stem = {
        let n = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let low = n.to_ascii_lowercase();
        let cut = if low.ends_with(".nkit.iso") || low.ends_with(".nkit.gcz") { n.len() - 9 } else { n.rfind('.').unwrap_or(n.len()) };
        n[..cut].to_string()
    };
    match kind {
        Kind::Nintendo => {
            let disc = nod::Disc::new(src).map_err(|e| format!("Couldn't read it as a GameCube or Wii image: {e}"))?;
            Ok((dir.join(format!("{stem}.iso")), disc.disc_size()))
        }
        Kind::Chd => {
            let mut f = BufReader::new(File::open(src).map_err(|e| e.to_string())?);
            let mut chd = chd::Chd::open(&mut f, None).map_err(|e| format!("Couldn't read it as a CHD: {e}"))?;
            let tracks = cd_tracks(&mut chd);
            if tracks.is_empty() {
                Ok((dir.join(format!("{stem}.iso")), chd.header().logical_bytes()))
            } else {
                let frames: u64 = tracks.iter().map(|t| t.frames as u64).sum();
                Ok((dir.join(format!("{stem}.cue")), frames * 2352))
            }
        }
        Kind::Cso => {
            let h = cso_header(&mut File::open(src).map_err(|e| e.to_string())?)?;
            Ok((dir.join(format!("{stem}.iso")), h.total))
        }
    }
}

/// Make the plain copy. `progress(done, total)` as it goes; `stopped()` ends it, leaving nothing
/// behind. The finished file is written under another name first and renamed at the end, so a
/// half-written one is never taken for the real thing. Returns the file to pick (the .cue for a CD).
pub fn convert(src: &Path, progress: &dyn Fn(u64, u64), stopped: &dyn Fn() -> bool) -> Result<PathBuf, String> {
    let (dest, total) = plan(src)?;
    if dest.exists() {
        return Err(format!("There's already a {} beside it. Pick that one instead.", dest.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
    }
    if let Some(free) = crate::manage::free_space(dest.parent().unwrap_or(Path::new("."))) {
        if free < total + (256 << 20) {
            return Err(format!("The plain copy needs about {} free on that drive, and it has {}.", human(total), human(free)));
        }
    }
    let result = match kind_of(src) {
        Some(Kind::Nintendo) => nintendo(src, &dest, total, progress, stopped).map(|_| dest.clone()),
        Some(Kind::Chd) => chd_out(src, &dest, progress, stopped),
        Some(Kind::Cso) => cso(src, &dest, progress, stopped).map(|_| dest.clone()),
        None => Err("That file isn't one the launcher can convert.".into()),
    };
    if result.is_err() {
        let _ = std::fs::remove_file(part(&dest));
        let _ = std::fs::remove_file(part(&dest.with_extension("bin")));
    }
    result
}

fn part(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

fn human(b: u64) -> String {
    let gb = b as f64 / (1u64 << 30) as f64;
    if gb >= 1.0 { format!("{gb:.1} GB") } else { format!("{:.0} MB", b as f64 / (1u64 << 20) as f64) }
}

/// Copy from a reader into the file, in big pieces, saying how far it got.
fn pump(r: &mut dyn Read, w: &mut dyn Write, total: u64, progress: &dyn Fn(u64, u64), stopped: &dyn Fn() -> bool) -> Result<u64, String> {
    let mut buf = vec![0u8; 4 << 20];
    let mut done = 0u64;
    loop {
        if stopped() {
            return Err("Stopped.".into());
        }
        let n = r.read(&mut buf).map_err(|e| format!("Couldn't read it: {e}"))?;
        if n == 0 {
            break;
        }
        w.write_all(&buf[..n]).map_err(|e| format!("Couldn't write the plain copy: {e}"))?;
        done += n as u64;
        progress(done, total);
    }
    Ok(done)
}

fn nintendo(src: &Path, dest: &Path, total: u64, progress: &dyn Fn(u64, u64), stopped: &dyn Fn() -> bool) -> Result<(), String> {
    // Wii images kept without their encryption (RVZ, WIA) get it back, so the result is a real ISO.
    let opts = nod::OpenOptions { rebuild_encryption: true, ..Default::default() };
    let mut disc = nod::Disc::new_with_options(src, &opts).map_err(|e| format!("Couldn't read it as a GameCube or Wii image: {e}"))?;
    let tmp = part(dest);
    let mut out = BufWriter::new(File::create(&tmp).map_err(|e| format!("Couldn't write beside it: {e}"))?);
    pump(&mut disc, &mut out, total, progress, stopped)?;
    out.flush().map_err(|e| e.to_string())?;
    drop(out);
    std::fs::rename(&tmp, dest).map_err(|e| e.to_string())
}

// ---------- CHD ----------

/// One track of a CD kept in a CHD (its CHT2 or CHTR line).
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub kind: String,
    /// Frames stored, the pregap among them when it's kept ("V" pregap types).
    pub frames: u32,
    pub pregap: u32,
    pub pregap_stored: bool,
}

impl Track {
    /// Bytes of a frame that are the sector itself (the rest of the 2448 is padding or subcode),
    /// and the .cue's name for that kind of track. None for kinds a .cue can't describe.
    fn layout(&self) -> Option<(usize, &'static str)> {
        match self.kind.as_str() {
            "AUDIO" => Some((2352, "AUDIO")),
            "MODE1_RAW" => Some((2352, "MODE1/2352")),
            "MODE2_RAW" => Some((2352, "MODE2/2352")),
            "MODE1" => Some((2048, "MODE1/2048")),
            "MODE2" => Some((2336, "MODE2/2336")),
            _ => None,
        }
    }
}

/// A CHT2 (or older CHTR) line: "TRACK:1 TYPE:MODE2_RAW SUBTYPE:NONE FRAMES:1234 PREGAP:150 PGTYPE:VMODE2_RAW ...".
pub fn parse_track(line: &str) -> Option<Track> {
    let get = |k: &str| line.split_whitespace().find_map(|w| w.strip_prefix(&format!("{k}:")).map(str::to_string));
    Some(Track {
        kind: get("TYPE")?,
        frames: get("FRAMES")?.parse().ok()?,
        pregap: get("PREGAP").and_then(|v| v.parse().ok()).unwrap_or(0),
        pregap_stored: get("PGTYPE").map(|t| t.starts_with('V')).unwrap_or(false),
    })
}

fn cd_tracks<F: Read + Seek>(chd: &mut chd::Chd<F>) -> Vec<Track> {
    let Ok(metas): Result<Vec<chd::metadata::Metadata>, _> = chd.metadata_refs().try_into() else { return vec![] };
    let cht2 = u32::from_be_bytes(*b"CHT2");
    let chtr = u32::from_be_bytes(*b"CHTR");
    metas
        .iter()
        .filter(|m| m.metatag == cht2 || m.metatag == chtr)
        .filter_map(|m| parse_track(String::from_utf8_lossy(&m.value).trim_end_matches('\0')))
        .collect()
}

fn chd_out(src: &Path, dest: &Path, progress: &dyn Fn(u64, u64), stopped: &dyn Fn() -> bool) -> Result<PathBuf, String> {
    let mut f = BufReader::new(File::open(src).map_err(|e| e.to_string())?);
    let mut chd = chd::Chd::open(&mut f, None).map_err(|e| format!("Couldn't read it as a CHD: {e}"))?;
    if chd.header().has_parent() {
        return Err("That CHD needs another CHD it was made from. Use a complete one.".into());
    }
    let tracks = cd_tracks(&mut chd);
    let hunks = chd.header().hunk_count();
    let mut hunk_buf = chd.get_hunksized_buffer();
    let mut cmp = Vec::new();
    if tracks.is_empty() {
        // A DVD: its bytes, hunk after hunk, as far as the disc goes.
        let total = chd.header().logical_bytes();
        let tmp = part(dest);
        let mut out = BufWriter::new(File::create(&tmp).map_err(|e| format!("Couldn't write beside it: {e}"))?);
        let mut done = 0u64;
        for h in 0..hunks {
            if stopped() {
                return Err("Stopped.".into());
            }
            chd.hunk(h).and_then(|mut x| x.read_hunk_in(&mut cmp, &mut hunk_buf)).map_err(|e| format!("Part of it couldn't be read: {e}"))?;
            let take = ((total - done) as usize).min(hunk_buf.len());
            out.write_all(&hunk_buf[..take]).map_err(|e| format!("Couldn't write the plain copy: {e}"))?;
            done += take as u64;
            progress(done, total);
            if done >= total {
                break;
            }
        }
        out.flush().map_err(|e| e.to_string())?;
        drop(out);
        std::fs::rename(&tmp, dest).map_err(|e| e.to_string())?;
        return Ok(dest.to_path_buf());
    }
    // A CD: every track's sectors as raw 2352-byte ones in one .bin, and a .cue that lists them.
    // Frames are 2448 bytes in the CHD (sector, then subcode), each track padded to 4 frames, and
    // audio kept with its bytes swapped.
    const FRAME: usize = 2448;
    if let Some(t) = tracks.iter().find(|t| t.layout().is_none()) {
        return Err(format!("This CHD has a {} track, which the launcher can't make plain yet.", t.kind));
    }
    let bin = dest.with_extension("bin");
    let tmp = part(&bin);
    let mut out = BufWriter::new(File::create(&tmp).map_err(|e| format!("Couldn't write beside it: {e}"))?);
    let per_hunk = (hunk_buf.len() / FRAME).max(1);
    let total: u64 = tracks.iter().map(|t| t.frames as u64 * 2352).sum();
    let (mut frame_at, mut done) = (0u64, 0u64);
    let mut loaded: Option<u32> = None;
    let mut cue = format!("FILE \"{}\" BINARY\n", bin.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
    let mut sector_at = 0u64;
    for (i, t) in tracks.iter().enumerate() {
        let (size, cue_type) = t.layout().unwrap_or((2352, "MODE2/2352"));
        cue.push_str(&format!("  TRACK {:02} {}\n", i + 1, cue_type));
        let msf = |s: u64| format!("{:02}:{:02}:{:02}", s / (75 * 60), (s / 75) % 60, s % 75);
        if t.pregap_stored && t.pregap > 0 {
            cue.push_str(&format!("    INDEX 00 {}\n    INDEX 01 {}\n", msf(sector_at), msf(sector_at + t.pregap as u64)));
        } else {
            if t.pregap > 0 {
                cue.push_str(&format!("    PREGAP {}\n", msf(t.pregap as u64)));
            }
            cue.push_str(&format!("    INDEX 01 {}\n", msf(sector_at)));
        }
        for _ in 0..t.frames {
            if stopped() {
                return Err("Stopped.".into());
            }
            let h = (frame_at / per_hunk as u64) as u32;
            if h >= hunks {
                return Err("The CHD ends before its last track does.".into());
            }
            if loaded != Some(h) {
                chd.hunk(h).and_then(|mut x| x.read_hunk_in(&mut cmp, &mut hunk_buf)).map_err(|e| format!("Part of it couldn't be read: {e}"))?;
                loaded = Some(h);
            }
            let off = (frame_at as usize % per_hunk) * FRAME;
            let mut sector = [0u8; 2352];
            sector[..size].copy_from_slice(&hunk_buf[off..off + size]);
            if t.kind == "AUDIO" {
                for pair in sector.chunks_exact_mut(2) {
                    pair.swap(0, 1);
                }
            }
            out.write_all(&sector[..size]).map_err(|e| format!("Couldn't write the plain copy: {e}"))?;
            frame_at += 1;
            done += 2352;
            if done % (2352 * 512) == 0 {
                progress(done, total);
            }
        }
        sector_at += t.frames as u64;
        // The padding between tracks isn't part of the disc.
        frame_at += ((4 - t.frames % 4) % 4) as u64;
    }
    progress(total, total);
    out.flush().map_err(|e| e.to_string())?;
    drop(out);
    std::fs::rename(&tmp, &bin).map_err(|e| e.to_string())?;
    std::fs::write(dest, cue).map_err(|e| format!("Couldn't write the .cue: {e}"))?;
    Ok(dest.to_path_buf())
}

// ---------- CSO / ZSO ----------

#[derive(Debug, Clone, PartialEq)]
pub struct CsoHeader {
    pub lz4: bool,
    pub total: u64,
    pub block: u32,
    pub align: u8,
}

pub fn cso_header(f: &mut dyn Read) -> Result<CsoHeader, String> {
    let mut h = [0u8; 24];
    f.read_exact(&mut h).map_err(|_| "That file is too short to be a CSO.")?;
    let lz4 = match &h[0..4] {
        b"CISO" => false,
        b"ZISO" => true,
        _ => return Err("That isn't a CSO or ZSO file.".into()),
    };
    let total = u64::from_le_bytes(h[8..16].try_into().unwrap());
    let block = u32::from_le_bytes(h[16..20].try_into().unwrap());
    if block == 0 || block > (1 << 24) || total == 0 {
        return Err("That CSO's header doesn't make sense.".into());
    }
    Ok(CsoHeader { lz4, total, block, align: h[21] })
}

fn cso(src: &Path, dest: &Path, progress: &dyn Fn(u64, u64), stopped: &dyn Fn() -> bool) -> Result<(), String> {
    let mut f = BufReader::new(File::open(src).map_err(|e| e.to_string())?);
    let h = cso_header(&mut f)?;
    let blocks = h.total.div_ceil(h.block as u64) as usize;
    let mut idx = vec![0u32; blocks + 1];
    let mut raw = vec![0u8; (blocks + 1) * 4];
    f.read_exact(&mut raw).map_err(|_| "That CSO's block list is cut short.")?;
    for (i, c) in raw.chunks_exact(4).enumerate() {
        idx[i] = u32::from_le_bytes(c.try_into().unwrap());
    }
    let tmp = part(dest);
    let mut out = BufWriter::new(File::create(&tmp).map_err(|e| format!("Couldn't write beside it: {e}"))?);
    let mut packed = Vec::new();
    let mut plain = vec![0u8; h.block as usize];
    let mut done = 0u64;
    for i in 0..blocks {
        if stopped() {
            return Err("Stopped.".into());
        }
        let stored = idx[i] & 0x8000_0000 != 0;
        let at = ((idx[i] & 0x7FFF_FFFF) as u64) << h.align;
        let end = ((idx[i + 1] & 0x7FFF_FFFF) as u64) << h.align;
        let want = ((h.total - done) as usize).min(h.block as usize);
        packed.resize(end.saturating_sub(at) as usize, 0);
        f.seek(SeekFrom::Start(at)).map_err(|e| e.to_string())?;
        f.read_exact(&mut packed).map_err(|_| "Part of the CSO couldn't be read.")?;
        let data: &[u8] = if stored || packed.len() >= h.block as usize {
            &packed[..want.min(packed.len())]
        } else if h.lz4 {
            let n = lz4_flex::block::decompress_into(&packed, &mut plain).map_err(|e| format!("A block didn't unpack: {e}"))?;
            &plain[..n.min(want)]
        } else {
            let mut d = flate2::read::DeflateDecoder::new(&packed[..]);
            let mut n = 0;
            while n < want {
                let k = d.read(&mut plain[n..want]).map_err(|e| format!("A block didn't unpack: {e}"))?;
                if k == 0 {
                    break;
                }
                n += k;
            }
            &plain[..n]
        };
        out.write_all(data).map_err(|e| format!("Couldn't write the plain copy: {e}"))?;
        done += data.len() as u64;
        progress(done, h.total);
    }
    out.flush().map_err(|e| e.to_string())?;
    drop(out);
    std::fs::rename(&tmp, dest).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_are_told_apart_by_name() {
        assert_eq!(kind_of(Path::new("/g/Hopper.rvz")), Some(Kind::Nintendo));
        assert_eq!(kind_of(Path::new("/g/Hopper.nkit.iso")), Some(Kind::Nintendo));
        assert_eq!(kind_of(Path::new("/g/Hopper (USA).CHD")), Some(Kind::Chd));
        assert_eq!(kind_of(Path::new("/g/hopper.zso")), Some(Kind::Cso));
        assert_eq!(kind_of(Path::new("/g/hopper.iso")), None);
    }

    #[test]
    fn a_cd_track_line_is_read() {
        let t = parse_track("TRACK:2 TYPE:AUDIO SUBTYPE:NONE FRAMES:1500 PREGAP:150 PGTYPE:VAUDIO PGSUB:NONE POSTGAP:0").unwrap();
        assert_eq!(t, Track { kind: "AUDIO".into(), frames: 1500, pregap: 150, pregap_stored: true });
        assert_eq!(t.layout(), Some((2352, "AUDIO")));
        let d = parse_track("TRACK:1 TYPE:MODE2_RAW SUBTYPE:NONE FRAMES:200000").unwrap();
        assert!(!d.pregap_stored && d.layout() == Some((2352, "MODE2/2352")));
        assert_eq!(parse_track("TRACK:1 TYPE:MODE1 FRAMES:10").unwrap().layout(), Some((2048, "MODE1/2048")));
        assert_eq!(parse_track("TRACK:1 TYPE:MODE2_FORM2 FRAMES:10").unwrap().layout(), None);
        assert!(parse_track("nonsense").is_none());
    }

    #[test]
    fn a_cso_unpacks_to_the_plain_image() {
        use flate2::write::DeflateEncoder;
        let d = std::env::temp_dir().join(format!("rl-cso-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        // Three 2048-byte blocks: one compressed, one stored as it is, a short last one.
        let block = 2048usize;
        let plain: Vec<u8> = (0..block * 2 + 100).map(|i| (i % 251) as u8).collect();
        let mut enc = DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(&plain[..block]).unwrap();
        let b0 = enc.finish().unwrap();
        let b1 = plain[block..block * 2].to_vec();
        let b2 = plain[block * 2..].to_vec();
        let head = 24 + 4 * 4;
        let o0 = head as u32;
        let o1 = o0 + b0.len() as u32;
        let o2 = o1 + b1.len() as u32;
        let o3 = o2 + b2.len() as u32;
        let mut f = b"CISO".to_vec();
        f.extend(24u32.to_le_bytes());
        f.extend((plain.len() as u64).to_le_bytes());
        f.extend((block as u32).to_le_bytes());
        f.extend([1u8, 0, 0, 0]);
        for (o, stored) in [(o0, false), (o1, true), (o2, true), (o3, false)] {
            f.extend((o | if stored { 0x8000_0000 } else { 0 }).to_le_bytes());
        }
        f.extend(&b0);
        f.extend(&b1);
        f.extend(&b2);
        let src = d.join("Hopper.cso");
        std::fs::write(&src, &f).unwrap();
        let out = convert(&src, &|_, _| {}, &|| false).unwrap();
        assert_eq!(out, d.join("Hopper.iso"));
        assert_eq!(std::fs::read(&out).unwrap(), plain);
        // A second go finds it there and says so, instead of writing over it.
        assert!(convert(&src, &|_, _| {}, &|| false).unwrap_err().contains("already"));
        let _ = std::fs::remove_dir_all(&d);
    }
}

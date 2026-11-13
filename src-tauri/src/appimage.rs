//! Read a type-2 AppImage's root manifest without executing, mounting or extracting it.

use backhand::{FilesystemReader, InnerNode};
use std::fs::{self, File};
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub(crate) const MANIFEST: &str = "recomplauncher.json";
pub(crate) const MAX_MANIFEST: u64 = 1024 * 1024;

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn number(bytes: &[u8], at: usize, size: usize, little: bool) -> u64 {
    let mut value = 0u64;
    for i in 0..size {
        let shift = if little { i } else { size - 1 - i };
        value |= u64::from(bytes[at + i]) << (shift * 8);
    }
    value
}

/// The runtime ends after the ELF section table or its last file-backed section.
/// Do not scan for magic bytes: the runtime can contain unrelated SquashFS data.
fn payload_offset(file: &mut File) -> io::Result<u64> {
    let length = file.metadata()?.len();
    let mut header = [0u8; 64];
    file.read_exact(&mut header)?;
    if &header[..4] != b"\x7fELF" || &header[8..11] != b"AI\x02" {
        return Err(invalid("Not a type-2 AppImage"));
    }
    let little = match header[5] {
        1 => true,
        2 => false,
        _ => return Err(invalid("Unsupported ELF byte order")),
    };
    let (table, size, count, offset_field, size_field, word) = match header[4] {
        1 => (
            number(&header, 32, 4, little),
            number(&header, 46, 2, little),
            number(&header, 48, 2, little),
            16,
            20,
            4,
        ),
        2 => (
            number(&header, 40, 8, little),
            number(&header, 58, 2, little),
            number(&header, 60, 2, little),
            24,
            32,
            8,
        ),
        _ => return Err(invalid("Unsupported ELF class")),
    };
    let expected_size = if word == 4 { 40 } else { 64 };
    // Type-2 runtimes use ordinary ELF section tables. Bound every file access,
    // and reject extended/absurd counts rather than allocating from the header.
    if table < 64 || size != expected_size || count == 0 || count > 4096 {
        return Err(invalid("Invalid AppImage ELF section table"));
    }
    let mut end = table
        .checked_add(size * count)
        .filter(|end| *end <= length)
        .ok_or_else(|| invalid("Truncated AppImage ELF section table"))?;
    for i in 0..count {
        file.seek(SeekFrom::Start(table + i * size))?;
        let mut section = [0u8; 64];
        file.read_exact(&mut section[..size as usize])?;
        // SHT_NOBITS occupies memory, not bytes in the ELF file.
        if number(&section, 4, 4, little) == 8 {
            continue;
        }
        let section_end = number(&section, offset_field, word, little)
            .checked_add(number(&section, size_field, word, little))
            .filter(|end| *end <= length)
            .ok_or_else(|| invalid("Invalid AppImage ELF section extent"))?;
        end = end.max(section_end);
    }
    if end.checked_add(96).filter(|end| *end <= length).is_none() {
        return Err(invalid("Missing AppImage SquashFS payload"));
    }
    file.seek(SeekFrom::Start(end))?;
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic)?;
    if &magic != b"hsqs" {
        return Err(invalid("Unsupported AppImage filesystem"));
    }
    Ok(end)
}

/// Check a launcher update's container and native runtime before making it executable.
#[cfg(target_os = "linux")]
pub(crate) fn validate_runtime(path: &Path) -> io::Result<()> {
    let mut file = File::open(path)?;
    payload_offset(&mut file)?;
    file.seek(SeekFrom::Start(0))?;
    let mut header = [0u8; 20];
    file.read_exact(&mut header)?;
    let machine = match std::env::consts::ARCH {
        "x86_64" => 62,
        "aarch64" => 183,
        _ => return Err(invalid("Unsupported AppImage architecture")),
    };
    if header[4] != 2 || header[5] != 1 || number(&header, 18, 2, true) != machine {
        return Err(invalid(
            "The update's AppImage runtime is for a different architecture",
        ));
    }
    Ok(())
}

/// Only an unambiguous, regular AppImage at the install root is eligible.
/// Never follow a symlink or guess between several independent game builds.
pub(crate) fn installed_image(dir: &Path) -> Option<PathBuf> {
    let mut image = None;
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.eq_ignore_ascii_case("appimage"))
            != Some(true)
            || !entry.file_type().ok()?.is_file()
        {
            continue;
        }
        if image.is_some() {
            return None;
        }
        image = Some(path);
    }
    image
}

/// What was read from each AppImage (its size and modified time, and the manifest or why not),
/// so looking a game's profile up again doesn't open and walk a big image every time.
type Seen = (u64, Option<std::time::SystemTime>, u64, Result<Vec<u8>, (io::ErrorKind, String)>);

/// A quick fingerprint of a file: its first and last 64 KiB. A replaced image can keep the same
/// size and, on Windows, the same modified time; its start or end still differs.
fn fingerprint(path: &Path, len: u64) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    if let Ok(mut f) = File::open(path) {
        let mut buf = vec![0u8; 64 * 1024];
        let n = f.read(&mut buf).unwrap_or(0);
        buf[..n].hash(&mut h);
        if len > 128 * 1024 && f.seek(SeekFrom::End(-(64 * 1024))).is_ok() {
            let n = f.read(&mut buf).unwrap_or(0);
            buf[..n].hash(&mut h);
        } else {
            let mut rest = vec![];
            let _ = f.read_to_end(&mut rest);
            rest.hash(&mut h);
        }
    }
    h.finish()
}
static SEEN: std::sync::Mutex<Option<std::collections::HashMap<PathBuf, Seen>>> = std::sync::Mutex::new(None);

pub(crate) fn manifest(path: &Path) -> io::Result<Vec<u8>> {
    let meta = fs::metadata(path)?;
    let (len, modified) = (meta.len(), meta.modified().ok());
    let print = fingerprint(path, len);
    if let Some((l, m, fp, r)) = SEEN.lock().unwrap().as_ref().and_then(|c| c.get(path)) {
        if *l == len && *m == modified && modified.is_some() && *fp == print {
            return r.clone().map_err(|(k, msg)| io::Error::new(k, msg));
        }
    }
    let r = read_manifest(path);
    let kept = match &r {
        Ok(b) => Ok(b.clone()),
        Err(e) => Err((e.kind(), e.to_string())),
    };
    let mut c = SEEN.lock().unwrap();
    let c = c.get_or_insert_with(std::collections::HashMap::new);
    if c.len() > 256 {
        c.clear();
    }
    c.insert(path.to_path_buf(), (len, modified, print, kept));
    r
}

fn read_manifest(path: &Path) -> io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let offset = payload_offset(&mut file)?;
    // Malformed compressed blocks in third-party images must not unwind into
    // the UI. The same adjacent/catalog fallback applies to reader panics.
    std::panic::catch_unwind(move || {
        let filesystem = FilesystemReader::from_reader_with_offset(BufReader::new(file), offset)
            .map_err(|e| invalid(&format!("Cannot read AppImage filesystem: {e}")))?;
        let node = filesystem
            .files()
            .find(|node| node.fullpath == Path::new("/recomplauncher.json"))
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "No embedded recomplauncher.json")
            })?;
        let InnerNode::File(file) = &node.inner else {
            return Err(invalid("Embedded manifest is not a regular file"));
        };
        if file.file_len() as u64 > MAX_MANIFEST {
            return Err(invalid("Embedded manifest exceeds 1 MiB"));
        }
        let mut bytes = Vec::new();
        filesystem
            .file(file)
            .reader()
            .take(MAX_MANIFEST + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_MANIFEST {
            return Err(invalid("Embedded manifest exceeds 1 MiB"));
        }
        Ok(bytes)
    })
    .unwrap_or_else(|_| Err(invalid("Invalid AppImage filesystem")))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use backhand::compression::{CompressionOptions, Compressor, Lz4};
    use backhand::{FilesystemCompressor, FilesystemWriter, NodeHeader};
    use std::io::Cursor;
    use std::sync::atomic::{AtomicU64, Ordering};

    pub(crate) struct TempDir(pub PathBuf);
    impl TempDir {
        pub(crate) fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!(
                "rl-appimage-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Synthetic ELF runtime + real compressed SquashFS; never a runnable game.
    pub(crate) fn image(contents: Option<&[u8]>, compressor: Compressor) -> Vec<u8> {
        let mut fs = FilesystemWriter::default();
        let options = if compressor == Compressor::Lz4 {
            Some(CompressionOptions::Lz4(Lz4 {
                version: 1,
                flags: 0,
            }))
        } else {
            None
        };
        fs.set_compressor(FilesystemCompressor::new(compressor, options).unwrap());
        if let Some(contents) = contents {
            fs.push_file(
                Cursor::new(contents.to_vec()),
                "/recomplauncher.json",
                NodeHeader::default(),
            )
            .unwrap();
        }
        let mut squashfs = Cursor::new(Vec::new());
        fs.write(&mut squashfs).unwrap();
        let mut bytes = vec![0u8; 128];
        bytes[..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2; // ELF64
        bytes[5] = 1; // little endian
        bytes[8..11].copy_from_slice(b"AI\x02");
        bytes[40..48].copy_from_slice(&64u64.to_le_bytes());
        bytes[58..60].copy_from_slice(&64u16.to_le_bytes());
        bytes[60..62].copy_from_slice(&1u16.to_le_bytes());
        bytes.extend(squashfs.into_inner());
        bytes
    }

    #[test]
    fn reads_supported_compressors_without_extracting() {
        let dir = TempDir::new();
        let path = dir.0.join("A game.AppImage");
        for compressor in [
            Compressor::Gzip,
            Compressor::Xz,
            Compressor::Zstd,
            Compressor::Lz4,
        ] {
            fs::write(&path, image(Some(br#"{"format":1}"#), compressor)).unwrap();
            assert_eq!(manifest(&path).unwrap(), br#"{"format":1}"#);
            assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
        }
    }

    #[test]
    fn missing_truncated_and_non_type2_images_are_rejected() {
        let dir = TempDir::new();
        let path = dir.0.join("game.AppImage");
        for bytes in [
            b"not ELF".to_vec(),
            image(None, Compressor::Gzip),
            image(Some(b"{}"), Compressor::Gzip)[..160].to_vec(),
        ] {
            fs::write(&path, bytes).unwrap();
            assert!(manifest(&path).is_err());
        }
        let mut bytes = image(Some(b"{}"), Compressor::Gzip);
        bytes[10] = 1;
        fs::write(&path, bytes).unwrap();
        assert!(manifest(&path).is_err());
    }

    #[test]
    fn oversized_manifest_and_overflowing_elf_extents_are_rejected() {
        let dir = TempDir::new();
        let path = dir.0.join("game.AppImage");
        fs::write(
            &path,
            image(
                Some(&vec![b' '; MAX_MANIFEST as usize + 1]),
                Compressor::Gzip,
            ),
        )
        .unwrap();
        assert!(manifest(&path).is_err());
        let mut bytes = image(Some(b"{}"), Compressor::Gzip);
        bytes[40..48].copy_from_slice(&u64::MAX.to_le_bytes());
        fs::write(&path, bytes).unwrap();
        assert!(manifest(&path).is_err());
    }

    #[test]
    fn a_read_image_is_remembered_until_it_changes() {
        let dir = TempDir::new();
        let path = dir.0.join("Kept.AppImage");
        fs::write(&path, image(Some(br#"{"format":1}"#), Compressor::Gzip)).unwrap();
        assert_eq!(manifest(&path).unwrap(), br#"{"format":1}"#);
        // Changed on disk (another size): read again.
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(&path, image(Some(br#"{"format":1,"x":2}"#), Compressor::Gzip)).unwrap();
        assert_eq!(manifest(&path).unwrap(), br#"{"format":1,"x":2}"#);
    }

    #[test]
    fn discovers_only_one_regular_appimage() {
        let dir = TempDir::new();
        let a = dir.0.join("Renamed.APPIMAGE");
        fs::write(&a, []).unwrap();
        assert_eq!(installed_image(&dir.0), Some(a));
        fs::write(dir.0.join("another.AppImage"), []).unwrap();
        assert!(installed_image(&dir.0).is_none());
    }

    #[test]
    fn handles_elf32_and_big_endian_headers() {
        let dir = TempDir::new();
        let path = dir.0.join("game.AppImage");
        let original = image(Some(b"payload"), Compressor::Gzip);
        let mut bytes = vec![0u8; 104];
        bytes[..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 1;
        bytes[5] = 2; // big endian ELF32; the SquashFS stays little endian
        bytes[8..11].copy_from_slice(b"AI\x02");
        bytes[32..36].copy_from_slice(&64u32.to_be_bytes());
        bytes[46..48].copy_from_slice(&40u16.to_be_bytes());
        bytes[48..50].copy_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&original[128..]);
        fs::write(&path, bytes).unwrap();
        assert_eq!(manifest(&path).unwrap(), b"payload");
    }

    #[test]
    fn ignores_nobits_and_handles_file_data_after_the_section_table() {
        let dir = TempDir::new();
        let path = dir.0.join("game.AppImage");
        let original = image(Some(b"payload"), Compressor::Gzip);
        let mut bytes = original.clone();
        bytes[68..72].copy_from_slice(&8u32.to_le_bytes()); // SHT_NOBITS
        bytes[88..96].copy_from_slice(&u64::MAX.to_le_bytes());
        bytes[96..104].copy_from_slice(&u64::MAX.to_le_bytes());
        fs::write(&path, bytes).unwrap();
        assert_eq!(manifest(&path).unwrap(), b"payload");

        let mut bytes = original[..128].to_vec();
        bytes[68..72].copy_from_slice(&1u32.to_le_bytes());
        bytes[88..96].copy_from_slice(&200u64.to_le_bytes());
        bytes[96..104].copy_from_slice(&56u64.to_le_bytes());
        bytes.resize(256, 0);
        bytes.extend_from_slice(&original[128..]);
        fs::write(&path, bytes).unwrap();
        assert_eq!(manifest(&path).unwrap(), b"payload");
    }

    #[test]
    fn embedded_manifest_symlinks_are_not_followed() {
        let dir = TempDir::new();
        let path = dir.0.join("game.AppImage");
        let mut writer = FilesystemWriter::default();
        writer
            .push_symlink(
                "/outside.json",
                "/recomplauncher.json",
                NodeHeader::default(),
            )
            .unwrap();
        let mut payload = Cursor::new(Vec::new());
        writer.write(&mut payload).unwrap();
        let mut bytes = image(None, Compressor::Gzip)[..128].to_vec();
        bytes.extend(payload.into_inner());
        fs::write(&path, bytes).unwrap();
        assert_eq!(
            manifest(&path).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}

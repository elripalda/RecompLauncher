//! Read a type-2 AppImage without executing or mounting it: its root manifest, or (on Linux, where
//! every game plays from a plain folder) all of its files.

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

/// What an AppImage's files are packed in after its runtime. Most are SquashFS; the
/// "runs anywhere" tooling (pkgforge's Anylinux/uruntime, Tauri's portable AppImage) writes DwarFS.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Packed {
    SquashFs,
    DwarFs,
}

/// The runtime ends after the ELF section table or its last file-backed section.
/// Do not scan for magic bytes: the runtime can contain unrelated SquashFS data.
fn payload_offset(file: &mut File) -> io::Result<u64> {
    let (end, packed) = payload(file)?;
    if packed != Packed::SquashFs {
        return Err(invalid("Unsupported AppImage filesystem"));
    }
    Ok(end)
}

/// Where an AppImage's files start, and what they're packed in.
fn payload(file: &mut File) -> io::Result<(u64, Packed)> {
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
    let mut magic = [0u8; 6];
    file.read_exact(&mut magic)?;
    if &magic[..4] == b"hsqs" {
        return Ok((end, Packed::SquashFs));
    }
    if &magic == b"DWARFS" {
        return Ok((end, Packed::DwarFs));
    }
    Err(invalid("Unsupported AppImage filesystem"))
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

/// A file named `….AppImage` that is one: every AppImage (type 2 of either kind, or an old type 1)
/// starts like a Linux program. A start script some releases keep under an older AppImage name
/// doesn't, so it is a plain file. Only its first 4 bytes are read. A file whose start can't be read
/// (no permission, still being written) goes by its name, as it always did.
pub(crate) fn is_real_appimage(path: &Path) -> bool {
    use std::io::Read;
    if !path.extension().is_some_and(|x| x.eq_ignore_ascii_case("appimage")) {
        return false;
    }
    let mut magic = [0u8; 4];
    match fs::File::open(path).and_then(|mut f| f.read_exact(&mut magic)) {
        Ok(()) => &magic == b"\x7fELF",
        Err(_) => true,
    }
}

/// Only an unambiguous, regular AppImage at the install root is eligible.
/// Never follow a symlink or guess between several independent game builds.
pub(crate) fn installed_image(dir: &Path) -> Option<PathBuf> {
    let mut image = None;
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        let named = path.extension().is_some_and(|x| x.eq_ignore_ascii_case("appimage"));
        if !named || !entry.file_type().ok()?.is_file() || crate::install::mac_metadata(&path) || !is_real_appimage(&path) {
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
    let (offset, packed) = payload(&mut file)?;
    if packed == Packed::DwarFs {
        return std::panic::catch_unwind(move || dwarfs_manifest(file, offset)).unwrap_or_else(|_| Err(invalid("Invalid AppImage filesystem")));
    }
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

/// The manifest at the top of a DwarFS AppImage, as read_manifest reads a SquashFS one.
fn dwarfs_manifest(file: File, offset: u64) -> io::Result<Vec<u8>> {
    use dwarfs::archive::{Archive, ArchiveIndex, AsChunks, Config};
    let config = Config::default();
    let mut reader = dwarfs::section::SectionReader::new_with_offset(file, offset);
    let index = ArchiveIndex::new_with_config(&mut reader, &config).map_err(|e| invalid(&format!("Cannot read AppImage filesystem: {e}")))?;
    let inode = index.get_path([MANIFEST]).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No embedded recomplauncher.json"))?;
    let file = inode.as_file().ok_or_else(|| invalid("Embedded manifest is not a regular file"))?;
    if file.as_chunks().total_size() > MAX_MANIFEST {
        return Err(invalid("Embedded manifest exceeds 1 MiB"));
    }
    let mut archive = Archive::new_with_index_and_config(reader, &index, &config).map_err(|e| invalid(&format!("Cannot read AppImage filesystem: {e}")))?;
    file.read_to_vec(&mut archive)
}

/// Unpack a type-2 AppImage's files into `to` with the launcher's own readers (SquashFS, or DwarFS
/// for the "runs anywhere" kind): the AppImage is never run or mounted, so it works without FUSE
/// and inside the Flatpak. Programs keep their run permission. Links that would point outside `to`
/// are left out. Err when it isn't a type-2 AppImage (an old type-1 image), when it's damaged, or
/// when the drive hasn't room for it.
#[cfg(unix)]
pub(crate) fn unpack(image: &Path, to: &Path) -> io::Result<()> {
    let mut file = File::open(image)?;
    let (offset, packed) = payload(&mut file)?;
    let to = to.to_path_buf();
    // A damaged third-party image must not take the launcher down with it (as in read_manifest).
    let unpacked = std::panic::catch_unwind(move || match packed {
        Packed::SquashFs => unpack_squashfs(file, offset, &to),
        Packed::DwarFs => unpack_dwarfs(file, offset, &to),
    });
    unpacked.unwrap_or_else(|_| Err(invalid("The AppImage's files are damaged")))
}

#[cfg(unix)]
fn unpack_squashfs(file: File, offset: u64, to: &Path) -> io::Result<()> {
    let filesystem = FilesystemReader::from_reader_with_offset(BufReader::new(file), offset).map_err(|e| invalid(&format!("Cannot read AppImage filesystem: {e}")))?;
    let need: u64 = filesystem.files().filter_map(|n| if let InnerNode::File(f) = &n.inner { Some(f.file_len() as u64) } else { None }).sum();
    let mut out = Unpacking::start(to, need)?;
    for node in filesystem.files() {
        let Some(rel) = inside_image(&node.fullpath) else { continue };
        let mode = u32::from(node.header.permissions);
        match &node.inner {
            InnerNode::Dir(_) => out.folder(&rel, mode)?,
            InnerNode::File(f) => out.file(&rel, mode, &mut filesystem.file(f).reader())?,
            InnerNode::Symlink(l) => out.link(rel, l.link.clone()),
            // Devices, pipes and sockets aren't a game's files.
            _ => {}
        }
    }
    out.finish()
}

#[cfg(unix)]
fn unpack_dwarfs(file: File, offset: u64, to: &Path) -> io::Result<()> {
    use dwarfs::archive::{Archive, ArchiveIndex, AsChunks, Config, InodeKind};
    let config = Config::default();
    let mut reader = dwarfs::section::SectionReader::new_with_offset(file, offset);
    let index = ArchiveIndex::new_with_config(&mut reader, &config).map_err(|e| invalid(&format!("Cannot read AppImage filesystem: {e}")))?;
    let mut archive = Archive::new_with_index_and_config(reader, &index, &config).map_err(|e| invalid(&format!("Cannot read AppImage filesystem: {e}")))?;
    let need: u64 = index.inodes().filter_map(|i| i.as_file()).map(|f| f.as_chunks().total_size()).sum();
    let mut out = Unpacking::start(to, need)?;
    // Folders still to go through, each with its path in the image.
    let mut folders = vec![(index.root(), PathBuf::from("/"))];
    while let Some((dir, at)) = folders.pop() {
        for entry in dir.entries() {
            let full = at.join(entry.name());
            let Some(rel) = inside_image(&full) else { continue };
            let inode = entry.inode();
            let mode = inode.metadata().file_type_mode().mode_bits();
            match inode.classify() {
                InodeKind::Directory(sub) => {
                    out.folder(&rel, mode)?;
                    folders.push((sub, full));
                }
                InodeKind::File(f) => out.file(&rel, mode, &mut f.as_reader(&mut archive))?,
                InodeKind::Symlink(l) => out.link(rel, PathBuf::from(l.target())),
                // Devices, pipes and sockets aren't a game's files.
                _ => {}
            }
        }
    }
    out.finish()
}

/// The trail's line for links an unpack left out, naming the first few and where they pointed:
/// a game that then won't start for a missing file is found by its name.
#[cfg_attr(not(unix), allow(dead_code))]
fn left_out_note(links: &[String]) -> String {
    const SHOWN: usize = 5;
    let named = links.iter().take(SHOWN).cloned().collect::<Vec<_>>().join(", ");
    let more = links.len().saturating_sub(SHOWN);
    let rest = if more > 0 { format!(" and {more} more") } else { String::new() };
    format!("AppImage: {} links pointing outside the game's folder were left out: {named}{rest}", links.len())
}

/// Writing an AppImage's files out, whatever they were packed in: the same rules for both.
#[cfg(unix)]
struct Unpacking<'a> {
    to: &'a Path,
    links: Vec<(PathBuf, PathBuf)>,
    folders: Vec<(PathBuf, u32)>,
}

#[cfg(unix)]
impl<'a> Unpacking<'a> {
    /// Into `to`, when its drive has room for `need` bytes.
    fn start(to: &'a Path, need: u64) -> io::Result<Self> {
        fs::create_dir_all(to)?;
        if crate::manage::free_space(to).is_some_and(|free| free < need) {
            return Err(io::Error::new(io::ErrorKind::StorageFull, "The drive hasn't room for the unpacked game"));
        }
        Ok(Self { to, links: vec![], folders: vec![] })
    }

    fn folder(&mut self, rel: &Path, mode: u32) -> io::Result<()> {
        let dest = self.to.join(rel);
        fs::create_dir_all(&dest)?;
        self.folders.push((dest, (mode & 0o777) | 0o700));
        Ok(())
    }

    /// Never setuid, setgid or sticky: only the read, write and run bits come along.
    fn file(&mut self, rel: &Path, mode: u32, content: &mut dyn Read) -> io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let dest = self.to.join(rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = io::BufWriter::new(File::create(&dest)?);
        io::copy(content, &mut out)?;
        io::Write::flush(&mut out)?;
        fs::set_permissions(&dest, fs::Permissions::from_mode((mode & 0o777) | 0o600))
    }

    /// Made at the end, once every file is in, so no file is ever written through a link.
    fn link(&mut self, rel: PathBuf, target: PathBuf) {
        self.links.push((rel, target));
    }

    fn finish(self) -> io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let mut left_out = vec![];
        for (rel, target) in self.links {
            if !link_stays_inside(&rel, &target) {
                left_out.push(format!("{} -> {}", rel.display(), target.display()));
                continue;
            }
            let dest = self.to.join(&rel);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            std::os::unix::fs::symlink(&target, &dest)?;
        }
        if !left_out.is_empty() {
            crate::trail::note(left_out_note(&left_out));
        }
        // Folders' own permissions last: a read-only folder in the image would stop its files going in.
        for (dir, mode) in self.folders.into_iter().rev() {
            fs::set_permissions(&dir, fs::Permissions::from_mode(mode))?;
        }
        Ok(())
    }
}

/// A path in the image (`/usr/bin/game`) as one inside the folder it unpacks to, or None for the
/// image's root and anything that would leave the folder.
fn inside_image(full: &Path) -> Option<PathBuf> {
    let rel = full.strip_prefix("/").unwrap_or(full);
    let plain = rel.components().all(|c| matches!(c, std::path::Component::Normal(_)));
    (plain && !rel.as_os_str().is_empty()).then(|| rel.to_path_buf())
}

/// A link at `rel` whose `target` stays inside the folder, read without following any link: never
/// absolute, and no `..` past the top.
fn link_stays_inside(rel: &Path, target: &Path) -> bool {
    use std::path::Component;
    let mut depth = rel.parent().map(|p| p.components().count()).unwrap_or(0);
    for part in target.components() {
        match part {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir if depth > 0 => depth -= 1,
            _ => return false,
        }
    }
    !target.as_os_str().is_empty()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use backhand::compression::{CompressionOptions, Compressor, Lz4};
    use backhand::{FilesystemCompressor, FilesystemWriter, NodeHeader};
    use std::io::Cursor;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn links_left_out_are_named_with_where_they_pointed() {
        let note = left_out_note(&["usr/lib/libfoo.so -> /usr/lib/libfoo.so".into()]);
        assert_eq!(note, "AppImage: 1 links pointing outside the game's folder were left out: usr/lib/libfoo.so -> /usr/lib/libfoo.so");
    }

    #[test]
    fn only_the_first_five_links_left_out_are_named() {
        let links: Vec<String> = (1..=7).map(|i| format!("l{i} -> /x")).collect();
        let note = left_out_note(&links);
        assert!(note.starts_with("AppImage: 7 links"), "{note}");
        assert!(note.ends_with("l5 -> /x and 2 more"), "{note}");
    }

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

    /// A small AppImage of a game: AppRun linked to its program, a data file, a folder, and links
    /// that try to leave the folder.
    #[cfg(unix)]
    pub(crate) fn game_image() -> Vec<u8> {
        let mode = |permissions| NodeHeader { permissions, ..NodeHeader::default() };
        let mut fs = FilesystemWriter::default();
        fs.push_dir("/usr", mode(0o755)).unwrap();
        fs.push_dir("/usr/bin", mode(0o755)).unwrap();
        fs.push_file(Cursor::new(b"#!/bin/sh\necho game\n".to_vec()), "/usr/bin/game", mode(0o4755)).unwrap();
        fs.push_file(Cursor::new(b"data".to_vec()), "/usr/data.bin", mode(0o644)).unwrap();
        fs.push_symlink("usr/bin/game", "/AppRun", mode(0o777)).unwrap();
        fs.push_symlink("/etc/passwd", "/absolute", mode(0o777)).unwrap();
        fs.push_symlink("../../outside", "/usr/escape", mode(0o777)).unwrap();
        fs.push_file(Cursor::new(br#"{"format":1}"#.to_vec()), "/recomplauncher.json", mode(0o644)).unwrap();
        let mut squashfs = Cursor::new(Vec::new());
        fs.write(&mut squashfs).unwrap();
        let mut bytes = image(None, Compressor::Gzip)[..128].to_vec();
        bytes.extend(squashfs.into_inner());
        bytes
    }

    #[test]
    fn a_macs_metadata_file_beside_the_appimage_isnt_a_second_one() {
        let d = std::env::temp_dir().join(format!("rl-appledouble-image-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("Game.AppImage"), b"\x7fELF image").unwrap();
        std::fs::write(d.join("._Game.AppImage"), b"\x00\x05\x16\x07AppleDouble").unwrap();
        let got = installed_image(&d);
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!(got, Some(d.join("Game.AppImage")));
    }

    #[cfg(unix)]
    #[test]
    fn an_appimage_unpacks_into_a_plain_folder_without_running() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new();
        let image = dir.0.join("Game-x86_64.AppImage");
        fs::write(&image, game_image()).unwrap();
        let to = dir.0.join("game");
        unpack(&image, &to).unwrap();
        assert_eq!(fs::read(to.join("usr/data.bin")).unwrap(), b"data");
        assert_eq!(fs::read_link(to.join("AppRun")).unwrap(), Path::new("usr/bin/game"));
        assert_eq!(fs::read(to.join("recomplauncher.json")).unwrap(), br#"{"format":1}"#);
        // Its program still runs, but never as another user.
        let mode = fs::metadata(to.join("usr/bin/game")).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode, 0o755);
    }

    #[cfg(unix)]
    #[test]
    fn links_out_of_the_unpacked_folder_are_left_out() {
        let dir = TempDir::new();
        let image = dir.0.join("Game.AppImage");
        fs::write(&image, game_image()).unwrap();
        let to = dir.0.join("game");
        unpack(&image, &to).unwrap();
        assert!(fs::symlink_metadata(to.join("absolute")).is_err());
        assert!(fs::symlink_metadata(to.join("usr/escape")).is_err());
    }

    /// The same game as `game_image`, packed with DwarFS the way "runs anywhere" AppImages are.
    #[cfg(unix)]
    pub(crate) fn dwarfs_game_image() -> Vec<u8> {
        use dwarfs_enc::chunker::{BasicChunker, Chunker};
        use dwarfs_enc::metadata::{Builder, InodeMetadata};
        use dwarfs_enc::section::{CompressParam, Writer};
        let mut meta = Builder::new(&InodeMetadata::new(0o755));
        let compress = CompressParam::Zstd(3);
        let mut chunker = BasicChunker::new(Writer::new(Vec::new()).unwrap(), meta.block_size(), compress);
        let root = meta.root();
        let usr = meta.put_dir(root, "usr", &InodeMetadata::new(0o755)).unwrap();
        let bin = meta.put_dir(usr, "bin", &InodeMetadata::new(0o755)).unwrap();
        let game = chunker.put_bytes(b"#!/bin/sh\necho game\n").unwrap();
        // (dwarfs-enc can't write a setuid bit; the SquashFS test covers that it's dropped.)
        meta.put_file(bin, "game", &InodeMetadata::new(0o755), game).unwrap();
        let data = chunker.put_bytes(b"data").unwrap();
        meta.put_file(usr, "data.bin", &InodeMetadata::new(0o644), data).unwrap();
        meta.put_symlink(root, "AppRun", &InodeMetadata::new(0o777), "usr/bin/game").unwrap();
        meta.put_symlink(root, "absolute", &InodeMetadata::new(0o777), "/etc/passwd").unwrap();
        meta.put_symlink(usr, "escape", &InodeMetadata::new(0o777), "../../outside").unwrap();
        let manifest = chunker.put_bytes(br#"{"format":1}"#).unwrap();
        meta.put_file(root, "recomplauncher.json", &InodeMetadata::new(0o644), manifest).unwrap();
        let mut writer = chunker.finish().unwrap();
        writer.write_metadata_sections(&meta.finish().unwrap(), compress).unwrap();
        let mut bytes = image(None, Compressor::Gzip)[..128].to_vec();
        bytes.extend(writer.finish().unwrap());
        bytes
    }

    #[cfg(unix)]
    #[test]
    fn a_dwarfs_appimage_unpacks_into_a_plain_folder_too() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new();
        let image = dir.0.join("Game-anylinux-x86_64.AppImage");
        fs::write(&image, dwarfs_game_image()).unwrap();
        let to = dir.0.join("game");
        unpack(&image, &to).unwrap();
        assert_eq!(fs::read(to.join("usr/data.bin")).unwrap(), b"data");
        assert_eq!(fs::read_link(to.join("AppRun")).unwrap(), Path::new("usr/bin/game"));
        let mode = fs::metadata(to.join("usr/bin/game")).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode, 0o755);
    }

    #[cfg(unix)]
    #[test]
    fn a_dwarfs_appimage_keeps_its_links_inside_too() {
        let dir = TempDir::new();
        let image = dir.0.join("Game.AppImage");
        fs::write(&image, dwarfs_game_image()).unwrap();
        let to = dir.0.join("game");
        unpack(&image, &to).unwrap();
        assert!(fs::symlink_metadata(to.join("absolute")).is_err());
        assert!(fs::symlink_metadata(to.join("usr/escape")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_dwarfs_appimages_manifest_is_read_without_unpacking() {
        let dir = TempDir::new();
        let image = dir.0.join("Game.AppImage");
        fs::write(&image, dwarfs_game_image()).unwrap();
        assert_eq!(manifest(&image).unwrap(), br#"{"format":1}"#);
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn a_damaged_dwarfs_appimage_doesnt_unpack() {
        let dir = TempDir::new();
        let image = dir.0.join("Game.AppImage");
        let mut bytes = dwarfs_game_image();
        let half = 128 + (bytes.len() - 128) / 2;
        bytes.truncate(half);
        fs::write(&image, bytes).unwrap();
        assert!(unpack(&image, &dir.0.join("game")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_file_that_isnt_an_appimage_doesnt_unpack() {
        let dir = TempDir::new();
        let image = dir.0.join("Game.AppImage");
        fs::write(&image, b"#!/bin/sh\necho not an AppImage\n").unwrap();
        assert!(unpack(&image, &dir.0.join("game")).is_err());
    }

    #[test]
    fn a_link_stays_inside_only_without_leaving_the_top() {
        assert!(link_stays_inside(Path::new("AppRun"), Path::new("usr/bin/game")));
        assert!(link_stays_inside(Path::new("usr/lib/a.so"), Path::new("../share/./b")));
        assert!(!link_stays_inside(Path::new("usr/a"), Path::new("../../b")));
        assert!(!link_stays_inside(Path::new("a"), Path::new("/etc/passwd")));
        assert!(!link_stays_inside(Path::new("a"), Path::new("")));
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
    fn a_script_named_like_an_appimage_isnt_one() {
        let dir = TempDir::new();
        let real = dir.0.join("Game.AppImage");
        fs::write(&real, game_image()).unwrap();
        let dwarfs = dir.0.join("Game-anylinux.appimage");
        fs::write(&dwarfs, dwarfs_game_image()).unwrap();
        let script = dir.0.join("OldName.AppImage");
        fs::write(&script, b"#!/bin/sh\nexec ./Game.AppImage\n").unwrap();
        let type1 = dir.0.join("Old.AppImage");
        fs::write(&type1, b"\x7fELF\x02\x01\x01\x00AI\x01").unwrap();
        assert!(is_real_appimage(&real));
        assert!(is_real_appimage(&dwarfs));
        assert!(is_real_appimage(&type1), "a type-1 AppImage is still one, and still stops at unpack");
        assert!(!is_real_appimage(&script));
        assert!(!is_real_appimage(&dir.0.join("Game.sh")), "only a file named like one");
    }

    #[test]
    fn an_appimage_whose_start_cant_be_read_goes_by_its_name() {
        let dir = TempDir::new();
        // Shorter than a header (one still being written): its name says what it is.
        let short = dir.0.join("Partial.AppImage");
        fs::write(&short, b"\x7f").unwrap();
        assert!(is_real_appimage(&short));
        assert!(is_real_appimage(&dir.0.join("Gone.AppImage")));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let locked = dir.0.join("Locked.AppImage");
            fs::write(&locked, b"#!/bin/sh\n").unwrap();
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
            // Root reads it anyway, and then it is the script it is.
            if fs::File::open(&locked).is_err() {
                assert!(is_real_appimage(&locked));
            }
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o644)).unwrap();
        }
    }

    #[test]
    fn the_installed_image_is_the_real_one_beside_a_script() {
        let dir = TempDir::new();
        fs::write(dir.0.join("OldName.AppImage"), b"#!/bin/sh\nexec ./Game.AppImage\n").unwrap();
        assert_eq!(installed_image(&dir.0), None, "a script alone is no image");
        let real = dir.0.join("Game.AppImage");
        fs::write(&real, game_image()).unwrap();
        assert_eq!(installed_image(&dir.0), Some(real));
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

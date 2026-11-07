//! Mac application bundles and native executable identification. No application is executed to
//! inspect it; a bundle's declared entry point wins over its helpers and framework libraries.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

pub fn is_bundle(path: &Path) -> bool {
    path.extension().map(|s| s.eq_ignore_ascii_case("app")).unwrap_or(false) && path.is_dir()
}

pub fn bundle_executable(app: &Path, arch: &str) -> Option<PathBuf> {
    if !is_bundle(app) {
        return None;
    }
    let info = app.join("Contents/Info.plist");
    // Metadata should be small, including when supplied by an untrusted release.
    if fs::metadata(&info).ok()?.len() > 1024 * 1024 {
        return None;
    }
    let root = fs::canonicalize(app).ok()?;
    if !fs::canonicalize(&info).ok()?.starts_with(&root) {
        return None;
    }
    let value = plist::Value::from_file(info).ok()?;
    let name = value.as_dictionary()?.get("CFBundleExecutable")?.as_string()?;
    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() || name.contains('\\') {
        return None;
    }
    let exe = app.join("Contents/MacOS").join(name);
    if !fs::canonicalize(&exe).ok()?.starts_with(root) || !is_executable(&exe, arch) {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if fs::metadata(&exe).ok()?.permissions().mode() & 0o111 == 0 {
            return None;
        }
    }
    Some(exe)
}

fn word(bytes: &[u8], little: bool) -> u32 {
    let bytes: [u8; 4] = bytes.try_into().unwrap();
    if little { u32::from_le_bytes(bytes) } else { u32::from_be_bytes(bytes) }
}

fn thin_header(bytes: &[u8; 32], cpu: u32) -> bool {
    let little = match &bytes[..4] {
        [0xcf, 0xfa, 0xed, 0xfe] => true,
        [0xfe, 0xed, 0xfa, 0xcf] => false,
        _ => return false,
    };
    word(&bytes[4..8], little) == cpu && word(&bytes[12..16], little) == 2 // MH_EXECUTE, not a dylib
}

/// A 64-bit Mach-O executable for this processor, either thin or a slice of a universal file.
/// Check the slice's header too: a fat container can also hold libraries or malformed offsets.
pub fn is_executable(path: &Path, arch: &str) -> bool {
    let cpu = match arch {
        "arm64" => 0x0100000c,
        "x64" => 0x01000007,
        _ => return false,
    };
    let check = || -> Option<bool> {
        let mut file = fs::File::open(path).ok()?;
        let size = file.metadata().ok()?.len();
        let mut head = [0u8; 32];
        file.read_exact(&mut head).ok()?;
        if thin_header(&head, cpu) {
            return Some(true);
        }
        let (little, wide) = match &head[..4] {
            [0xca, 0xfe, 0xba, 0xbe] => (false, false),
            [0xbe, 0xba, 0xfe, 0xca] => (true, false),
            [0xca, 0xfe, 0xba, 0xbf] => (false, true),
            [0xbf, 0xba, 0xfe, 0xca] => (true, true),
            _ => return Some(false),
        };
        let count = word(&head[4..8], little) as u64;
        if count == 0 || count > 64 {
            return Some(false);
        }
        let entry_size = if wide { 32 } else { 20 };
        let table_end = 8 + count * entry_size;
        if table_end > size {
            return Some(false);
        }
        for i in 0..count {
            file.seek(SeekFrom::Start(8 + i * entry_size)).ok()?;
            let mut entry = [0u8; 32];
            file.read_exact(&mut entry[..entry_size as usize]).ok()?;
            if word(&entry[..4], little) != cpu {
                continue;
            }
            let long = |bytes: &[u8]| {
                let bytes: [u8; 8] = bytes.try_into().unwrap();
                if little { u64::from_le_bytes(bytes) } else { u64::from_be_bytes(bytes) }
            };
            let (offset, length) = if wide {
                (long(&entry[8..16]), long(&entry[16..24]))
            } else {
                (word(&entry[8..12], little) as u64, word(&entry[12..16], little) as u64)
            };
            if offset < table_end || length < 32 || offset.checked_add(length)? > size {
                return Some(false);
            }
            file.seek(SeekFrom::Start(offset)).ok()?;
            file.read_exact(&mut head).ok()?;
            if thin_header(&head, cpu) {
                return Some(true);
            }
        }
        Some(false)
    };
    check().unwrap_or(false)
}

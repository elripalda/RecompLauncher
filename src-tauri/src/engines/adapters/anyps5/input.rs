use crate::engines::{prepared, Console, GameIdentity};
use serde_json::Value;
use std::{collections::BTreeSet, fs, io::Read, path::{Component, Path, PathBuf}};

#[derive(Debug)]
pub struct Input {
    pub identity: GameIdentity,
    pub executable: PathBuf,
    pub module_directories: Vec<PathBuf>,
    pub executables: Vec<PathBuf>,
}

pub(super) fn files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = vec![];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            let path = entry.path();
            let relative = path.strip_prefix(root).map_err(|e| e.to_string())?;
            if kind.is_symlink() || (!kind.is_file() && !kind.is_dir()) { return Err(format!("Use a game folder with regular files, without links: {}", relative.display())); }
            if !safe_relative(relative) { return Err(format!("Unsupported game filename: {}", relative.display())); }
            if kind.is_dir() { pending.push(path); } else { files.push(relative.to_path_buf()); }
        }
    }
    files.sort();
    Ok(files)
}

pub(super) fn safe_relative(path: &Path) -> bool {
    if path.as_os_str().is_empty() { return false; }
    path.components().all(|part| match part {
        // Native Windows separators split components; a Unix filename containing one stays invalid.
        Component::Normal(name) => !name.to_string_lossy().contains(['\\', ':', '{', '}', '\n', '\r']),
        _ => false,
    })
}

fn kind(path: &Path) -> Result<Option<u16>, String> {
    let mut header = [0; 64];
    let count = fs::File::open(path).and_then(|mut f| f.read(&mut header)).map_err(|e| e.to_string())?;
    if count >= 4 && (header[..4] == [0x4f, 0x15, 0x3d, 0x1d] || header[..4] == [0x54, 0x14, 0xf5, 0xee]) {
        return Err(format!("This folder contains a SELF container. Supply clean decrypted ELF files: {}", path.display()));
    }
    if !header.starts_with(b"\x7fELF") { return Ok(None); }
    if count < 64 || header[4..7] != [2, 1, 1] || header[18..20] != [62, 0] {
        return Err(format!("Expected a complete x86-64 little-endian ELF: {}", path.display()));
    }
    Ok(Some(u16::from_le_bytes([header[16], header[17]])))
}

fn required<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key].as_str().filter(|s| !s.trim().is_empty()).ok_or_else(|| format!("sce_sys/param.json needs a nonempty {key}."))
}

pub fn inspect(root: &Path, expected: &str) -> Result<Input, String> {
    let metadata: Value = serde_json::from_slice(&fs::read(root.join("sce_sys/param.json")).map_err(|e| format!("Select the PS5 game folder containing sce_sys/param.json: {e}"))?).map_err(|e| e.to_string())?;
    let title = required(&metadata, "titleId")?;
    if title.len() != 9 || !title.starts_with("PPSA") || !title[4..].bytes().all(|c| c.is_ascii_digit()) || !title.eq_ignore_ascii_case(expected) {
        return Err("This folder does not match the selected PS5 title ID.".into());
    }
    let version = required(&metadata, "contentVersion")?;
    let localized = metadata["localizedParameters"].as_object().ok_or("Missing PS5 localizedParameters.")?;
    let language = localized.get("defaultLanguage").or_else(|| metadata.get("defaultLanguage")).and_then(Value::as_str);
    let text = language.and_then(|l| localized.get(l)).or_else(|| localized.get("en-US")).or_else(|| localized.values().find(|v| v.is_object()));
    required(text.ok_or("Missing PS5 title language metadata.")?, "titleName")?;
    if root.join("sce_module").exists() && root.join("sce_modules").exists() { return Err("Keep only the title's original sce_module or sce_modules directory; both are ambiguous.".into()); }
    let mut candidates = vec![];
    for path in files(root)? {
        if path.to_string_lossy().ends_with(".guest.prx") { return Err("Select the original game folder, without previously converted .guest.prx files.".into()); }
        let kind = kind(&root.join(&path))?;
        let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        if kind.is_none() && ["prx", "sprx", "elf"].contains(&extension) {
            return Err(format!("Expected a clean ELF executable or module: {}", path.display()));
        }
        if let Some(kind) = kind { candidates.push((path, kind)); }
    }
    let executable = candidates.iter().find(|(p, _)| p == Path::new("eboot.bin")).or_else(|| candidates.iter().find(|(p, _)| p == Path::new("eboot.elf"))).map(|(p, _)| p.clone()).ok_or("The game folder needs its clean ELF eboot.bin or eboot.elf.")?;
    let mut directories = BTreeSet::new();
    let mut executables = vec![];
    for (path, kind) in candidates {
        if path == executable {
            if !matches!(kind, 2 | 3 | 0xfe00 | 0xfe10) { return Err("The selected eboot is not an executable ELF.".into()); }
            continue;
        }
        match kind {
            3 | 0xfe18 => { directories.insert(path.parent().unwrap_or(Path::new("")).to_path_buf()); }
            2 | 0xfe00 | 0xfe10 => executables.push(path),
            _ => return Err(format!("Unrecognized guest ELF type in {}.", path.display())),
        }
    }
    let automatic: Vec<_> = ["sce_module", "sce_modules", "prx"].into_iter().map(PathBuf::from).filter(|p| root.join(p).is_dir()).collect();
    if automatic.is_empty() && directories.is_empty() { directories.insert(PathBuf::new()); }
    if executables.iter().any(|p| directories.contains(p.parent().unwrap()) || automatic.iter().any(|d| p.parent() == Some(d))) {
        return Err("An extra executable shares a module directory. This ambiguous layout needs upstream support.".into());
    }
    let module_directories = directories.into_iter().filter(|p| !automatic.contains(p)).map(|p| if p.as_os_str().is_empty() { PathBuf::from(".") } else { p }).collect();
    let identity = GameIdentity { console:Console::Other("ps5".into()), title_id:title.into(), version:version.into(), build_id:prepared::file_sha256(&root.join(&executable))? };
    identity.validate()?;
    Ok(Input { identity, executable, module_directories, executables })
}

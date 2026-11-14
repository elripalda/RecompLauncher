//! Named Easy Setup inputs. Scope is discs/DLC/game files, three shared key types, and NVIDIA DLSS
//! (nvidia.dlss, role runtime: a file the launcher fetches itself, approved by the maintainer).
//! BIOS, firmware, fonts and generic shared-resource expansion are deliberately out of scope.
use crate::{library::Installed, profile::Profile};
use serde::{Deserialize, Serialize};
use std::{collections::{BTreeMap, BTreeSet}, fs, path::{Component, Path, PathBuf}};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub id: String,
    pub label: String,
    /// game, disc, dlc, update, key or runtime (a file the launcher fetches itself: nvidia.dlss
    /// only). Only affects grouping, never content verification.
    #[serde(default = "game_role")]
    pub role: String,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub shared: String,
    #[serde(default)]
    pub formats: Vec<Format>,
    /// Installation-relative file destination. Empty: pass {input:id} to start/prepare.
    #[serde(default)]
    pub destination: String,
}
fn game_role() -> String { "game".into() }

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Format {
    /// file or folder; a file format may constrain extensions, without claiming encryption.
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub extensions: Vec<String>,
    /// IDs of shared key inputs to activate for this selected format.
    #[serde(default)]
    pub requires: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Row {
    pub id: String,
    pub label: String,
    pub role: String,
    pub optional: bool,
    pub shared: String,
    pub active: bool,
    pub path: Option<PathBuf>,
    pub file: bool,
    pub folder: bool,
    pub extensions: Vec<String>,
    /// A file the launcher fetches itself (NVIDIA DLSS): no file to choose.
    pub runtime: bool,
    /// Turned off for this game by the player.
    pub off: bool,
    /// The game's release has its own file at the destination: that one is used, never replaced.
    pub ships: bool,
}

const ROLES: [&str; 6] = ["game", "disc", "dlc", "update", "key", "runtime"];

/// The roles an input can have (the schema lists the same ones).
pub fn roles() -> &'static [&'static str] {
    &ROLES
}

/// A shared type or role this launcher knows.
fn known(i: &Input) -> bool {
    ROLES.contains(&i.role.as_str()) && (i.shared.is_empty() || key_name(&i.shared).is_some() || i.shared == crate::dlss::SHARED)
}

/// What this launcher can't or won't use, taken out before anything else reads the profile: an
/// optional input with a shared type or role it doesn't know (newer than this launcher), and DLSS
/// on a PC it doesn't apply to. Their {input:...} arguments are left out. A required input it
/// doesn't know stays, and fails validate as before.
pub fn prepare(p: &mut Profile, dlss_applies: bool) {
    let skip: Vec<String> = p.inputs.iter().filter(|i| i.optional && (!known(i) || i.shared == crate::dlss::SHARED && !dlss_applies)).map(|i| i.id.clone()).collect();
    for id in &skip {
        static SAID: std::sync::Mutex<Option<BTreeSet<String>>> = std::sync::Mutex::new(None);
        let i = p.inputs.iter().find(|i| &i.id == id).cloned().unwrap_or_default();
        if !known(&i) && SAID.lock().unwrap().get_or_insert_with(BTreeSet::new).insert(format!("{}|{}", i.shared, i.role)) {
            crate::trail::note(format!("Setup: an optional input this launcher doesn't know ({} {}) is skipped", i.role, i.shared));
        }
    }
    p.inputs.retain(|i| !skip.contains(&i.id));
    p.skipped_inputs.extend(skip);
}

pub fn key_name(shared: &str) -> Option<&'static str> {
    match shared {
        "switch.prod-keys" => Some("prod.keys"),
        "switch.title-keys" => Some("title.keys"),
        "wiiu.common-key" => Some("common.key"),
        _ => None,
    }
}
fn key_path(dir: &Path, shared: &str) -> Option<PathBuf> {
    Some(dir.join("shared-keys").join(shared).join(key_name(shared)?))
}
fn id_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
/// Managed inputs are not save data, including a secret staged under a custom filename.
pub fn excluded(g: &Installed, path: &Path) -> bool {
    g.input_staged.values().filter(|s| relative(s)).any(|s| {
        let target = g.path.join(s);
        target == path || target.canonicalize().ok().zip(path.canonicalize().ok()).is_some_and(|(a, b)| a == b)
    }) || path.file_name().and_then(|s| s.to_str()).is_some_and(|s| ["prod.keys", "title.keys", "common.key"].iter().any(|n| s.eq_ignore_ascii_case(n)))
}
pub fn release_files(g: &Installed) -> Vec<PathBuf> {
    g.files.iter().cloned().chain(g.input_staged.values().filter(|s| relative(s)).map(PathBuf::from)).collect()
}
pub fn relative(s: &str) -> bool {
    !s.is_empty() && !s.contains(['\\', ':', '{', '}']) && Path::new(s).components().all(|c| matches!(c, Component::Normal(_)))
        && !s.split('/').any(|c| c.is_empty() || c == "." || c == "..")
}

pub fn validate(p: &Profile) -> Result<(), String> {
    if p.inputs.len() > 32 { return Err("A profile can declare at most 32 setup inputs.".into()); }
    let mut ids = BTreeSet::new();
    let mut destinations = BTreeSet::new();
    let mut shares = BTreeSet::new();
    for i in p.inputs.iter().filter(|i| !(i.optional && !known(i))) {
        let dlss = i.shared == crate::dlss::SHARED;
        if !i.shared.is_empty() && i.optional && !dlss { return Err("Shared key requirements must be required or conditional, not optional features.".into()); }
        if !id_ok(&i.id) || ["game", "legacy-update"].contains(&i.id.as_str()) && p.game.is_some() || !ids.insert(&i.id) || i.label.trim().is_empty() {
            return Err("Setup inputs need unique IDs and a label; game is reserved when the legacy game field is used.".into());
        }
        if !ROLES.contains(&i.role.as_str()) { return Err("Unknown setup input role.".into()); }
        // NVIDIA DLSS: role runtime (a file the launcher fetches itself), no formats, may be optional.
        if dlss && (i.role != "runtime" || !i.formats.is_empty() || !shares.insert(&i.shared)) {
            return Err("nvidia.dlss uses role runtime, no formats, and is declared once.".into());
        }
        if i.role == "runtime" && !dlss { return Err("The runtime role is only for nvidia.dlss.".into()); }
        if !dlss && !i.shared.is_empty() && (key_name(&i.shared).is_none() || i.role != "key" || !shares.insert(&i.shared)) {
            return Err("Shared setup inputs support only switch.prod-keys, switch.title-keys, wiiu.common-key and nvidia.dlss, once each.".into());
        }
        if i.role == "key" && i.shared.is_empty() { return Err("Key inputs must declare a supported shared key type.".into()); }
        if i.shared.is_empty() && i.formats.is_empty() { return Err("A game input must declare its accepted formats.".into()); }
        if !i.destination.is_empty() && (!relative(&i.destination) || !destinations.insert(i.destination.to_ascii_lowercase())) {
            return Err("Input destinations must be unique files inside the installation.".into());
        }
        for f in &i.formats {
            if !["file", "folder"].contains(&f.kind.as_str()) || f.kind == "folder" && (!f.extensions.is_empty() || !i.destination.is_empty())
                || f.extensions.iter().any(|e| e.is_empty() || !e.bytes().all(|b| b.is_ascii_alphanumeric())) {
                return Err("Invalid setup input format. Folders are passed by path, not copied into the installation.".into());
            }
            if !i.shared.is_empty() { return Err("Shared key inputs do not declare formats.".into()); }
        }
        let mut extensions = BTreeSet::new();
        let files: Vec<_> = i.formats.iter().filter(|f| f.kind == "file").collect();
        if files.len() > 1 && files.iter().any(|f| f.extensions.is_empty()) || files.iter().flat_map(|f| &f.extensions).any(|e| !extensions.insert(e.to_ascii_lowercase())) {
            return Err("File formats cannot overlap; each extension must have one dependency rule.".into());
        }
    }
    for i in &p.inputs {
        for f in &i.formats {
            for dep in &f.requires {
                if !p.inputs.iter().any(|k| &k.id == dep && !k.shared.is_empty() && !k.optional && k.role == "key") {
                    return Err("Format dependencies must name a required shared key input in this profile.".into());
                }
            }
        }
    }
    let declarations: BTreeSet<_> = p.inputs.iter().map(|i| i.id.as_str()).chain(p.skipped_inputs.iter().map(String::as_str)).collect();
    let mut texts: Vec<&str> = p.start.iter().flat_map(|s| s.args.iter().chain(s.env.values()).map(String::as_str)).collect();
    texts.extend(p.prepare.iter().flat_map(|pr| pr.args.iter().map(String::as_str)));
    for text in texts {
        for suffix in text.split("{input:").skip(1) {
            let id = suffix.split_once('}').map(|(id, _)| id).ok_or("Unclosed input placeholder.")?;
            if !declarations.contains(id) { return Err(format!("Unknown input placeholder: {id}.")); }
        }
    }
    Ok(())
}

fn matches(f: &Format, path: &Path) -> bool {
    if path.is_dir() { return f.kind == "folder"; }
    path.is_file() && f.kind == "file" && (f.extensions.is_empty() || path.extension().and_then(|e| e.to_str()).is_some_and(|e| f.extensions.iter().any(|x| x.eq_ignore_ascii_case(e))))
}
fn selected<'a>(i: &Input, g: &'a Installed) -> Option<&'a PathBuf> {
    g.setup_inputs.get(&i.id).filter(|p| i.formats.iter().any(|f| matches(f, p)))
}
pub fn rows(p: &Profile, g: &Installed, dir: &Path) -> Result<Vec<Row>, String> {
    validate(p)?;
    let dependencies: BTreeSet<&str> = p.inputs.iter().filter(|i| i.shared.is_empty()).filter_map(|i| selected(i, g).map(|path| (i, path)))
        .flat_map(|(i, path)| i.formats.iter().filter(move |f| matches(f, path)).flat_map(|f| f.requires.iter().map(String::as_str))).collect();
    // Referenced keys are conditional; unreferenced required keys are unconditional.
    let referenced: BTreeSet<&str> = p.inputs.iter().flat_map(|i| &i.formats).flat_map(|f| f.requires.iter().map(String::as_str)).collect();
    Ok(p.inputs.iter().map(|i| {
        let active = i.shared.is_empty() || !referenced.contains(i.id.as_str()) || dependencies.contains(i.id.as_str());
        let runtime = i.role == "runtime";
        let off = runtime && g.runtime_off.contains(&i.id);
        // The game's release has its own file there (not the launcher's copy): it's used as it is.
        let ships = runtime && !i.destination.is_empty() && g.input_staged.get(&i.id) != Some(&i.destination) && g.path.join(&i.destination).is_file();
        let path = if runtime { (!off && !ships).then(|| crate::dlss::current(dir)).flatten() }
            else if i.shared.is_empty() { selected(i, g).cloned() } else if active { key_path(dir, &i.shared).filter(|p| p.is_file()) } else { None };
        Row { id: i.id.clone(), label: i.label.clone(), role: i.role.clone(), optional: i.optional, shared: i.shared.clone(), active, path,
            file: !runtime && (!i.shared.is_empty() || i.formats.iter().any(|f| f.kind == "file")), folder: i.formats.iter().any(|f| f.kind == "folder"),
            extensions: i.formats.iter().flat_map(|f| f.extensions.clone()).collect(), runtime, off, ships }
    }).collect())
}
pub fn resolve(p: &Profile, g: &Installed, dir: &Path) -> Result<BTreeMap<String, PathBuf>, String> {
    let mut paths = BTreeMap::new();
    for r in rows(p, g, dir)?.into_iter().filter(|r| r.active) {
        if let Some(path) = r.path { paths.insert(r.id, path); }
        else if !r.optional && !r.runtime { return Err(format!("Choose {} before continuing.", r.label)); }
    }
    Ok(paths)
}
pub fn available(p: &Profile, g: &Installed, dir: &Path) -> BTreeMap<String, PathBuf> {
    rows(p, g, dir).unwrap_or_default().into_iter().filter(|r| r.active).filter_map(|r| Some((r.id, r.path?))).collect()
}

/// Metadata fingerprint only. Disc/DLC checksums are explicitly deferred.
pub fn signature(p: &Profile, g: &Installed, dir: &Path) -> Result<String, String> {
    let paths = resolve(p, g, dir)?;
    let mut hash = Sha256::new();
    hash.update(serde_json::to_vec(&p.inputs).map_err(|_| "Invalid inputs")?);
    hash.update(serde_json::to_vec(&p.prepare).map_err(|_| "Invalid preparation")?);
    if p.game.is_some() {
        hash.update(serde_json::to_vec(&p.game).map_err(|_| "Invalid game copy")?);
        let copy = g.game_copy.as_ref().filter(|c| c.exists()).ok_or("Choose your copy of the game first.")?;
        hash.update(copy.to_string_lossy().as_bytes());
        if let Some(update) = p.game.as_ref().and_then(|c| c.update.as_ref()) {
            let path = g.update_copy.as_ref().filter(|p| p.exists()).cloned().or_else(|| update.in_place(&g.path));
            if let Some(path) = path { hash.update(path.to_string_lossy().as_bytes()); }
            else if !update.optional { return Err("Choose the game's update first.".into()); }
        }
    }
    // Files the launcher fetches itself (DLSS) don't count: one arriving later, or a newer
    // version, never asks for setup again.
    for (id, path) in paths.into_iter().filter(|(id, _)| !p.inputs.iter().any(|i| &i.id == id && i.role == "runtime")) {
        hash.update(id.as_bytes()); hash.update(path.to_string_lossy().as_bytes());
        let m = path.metadata().map_err(|_| "A selected input is no longer readable.")?;
        hash.update(m.len().to_le_bytes());
        hash.update(format!("{:?}", m.modified().ok()).as_bytes());
    }
    Ok(hex::encode(hash.finalize()))
}
pub fn ready(p: &Profile, g: &Installed, dir: &Path) -> bool {
    !g.inputs_ready.is_empty() && signature(p, g, dir).is_ok_and(|s| s == g.inputs_ready)
}
pub fn expand(t: &str, paths: &BTreeMap<String, PathBuf>, wine: bool) -> String {
    let mut text = t.to_string();
    for (id, path) in paths {
        let value = path.to_string_lossy();
        let value = if wine { crate::profile::wine_path(&value) } else { value.to_string() };
        text = text.replace(&format!("{{input:{id}}}"), &value);
    }
    text
}
pub fn arguments(args: &[String], paths: &BTreeMap<String, PathBuf>, wine: bool) -> Vec<String> {
    args.iter().filter_map(|a| {
        let expanded = expand(a, paths, wine);
        (!expanded.contains("{input:")).then_some(expanded)
    }).collect()
}

pub fn remove_key(p: &Profile, g: &Installed, dir: &Path, input: &str) -> Result<(), String> {
    validate(p)?;
    let i = p.inputs.iter().find(|i| i.id == input && !i.shared.is_empty()).ok_or("Unknown shared key input.")?;
    let path = key_path(dir, &i.shared).ok_or("Unsupported shared key type.")?;
    if path.exists() { fs::remove_file(path).map_err(|_| "Couldn't remove the shared keys file.")?; }
    // Also remove this project's managed delivery copy. Other projects cannot use their old
    // copy through this API: missing central keys invalidate their readiness before launch.
    if let Some(rel) = g.input_staged.get(input) {
        let target = destination(&g.path, rel)?;
        if target.is_file() { fs::remove_file(target).map_err(|_| "Couldn't remove the game's keys copy.")?; }
    }
    Ok(())
}

/// Only validates shape; game-side decryption determines generation/title compatibility.
fn check_key(shared: &str, bytes: &[u8]) -> Result<(), String> {
    if shared == "wiiu.common-key" && bytes.len() == 16 { return Ok(()); }
    let text = std::str::from_utf8(bytes).map_err(|_| "That isn't a supported keys file.")?;
    if shared == "wiiu.common-key" {
        return if text.trim().len() == 32 && text.trim().bytes().all(|b| b.is_ascii_hexdigit()) { Ok(()) } else { Err("The common key must be 16 bytes or 32 hexadecimal digits.".into()) };
    }
    let lines: Vec<(&str, &str)> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with(['#', ';']))
        .map(|l| l.split_once('=').map(|(n, v)| (n.trim(), v.trim())).unwrap_or(("", l)))
        .collect();
    if lines.is_empty() {
        return Err("That keys file is empty.".into());
    }
    let hex = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_hexdigit());
    // title.keys: a title ID (32 hex digits) = its key (32). prod.keys: named keys, whose values
    // can be much longer (eticket_rsa_keypair is 1056 digits, the kek sources 34), with header_key
    // among them, which is what a recomp's setup needs. Each one picked for the other is said so.
    let titles = lines.iter().all(|(n, v)| n.len() == 32 && hex(n) && v.len() == 32 && hex(v));
    let has_header = lines.iter().any(|(n, v)| n.eq_ignore_ascii_case("header_key") && v.len() == 64 && hex(v));
    match shared {
        "switch.title-keys" => {
            if titles {
                Ok(())
            } else if has_header {
                Err("That's prod.keys. Pick title.keys here.".into())
            } else {
                Err("That isn't a supported keys file.".into())
            }
        }
        "switch.prod-keys" => {
            let named = lines.iter().all(|(n, v)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') && hex(v) && v.len() % 2 == 0 && v.len() <= 8192);
            if named && has_header {
                Ok(())
            } else if titles {
                Err("That's title.keys. Pick prod.keys, the file with header_key in it.".into())
            } else if named {
                Err("That keys file has no header_key. Dump prod.keys from your console again.".into())
            } else {
                Err("That isn't a supported keys file.".into())
            }
        }
        _ => {
            let ok = lines.iter().all(|(n, v)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') && [32, 64].contains(&v.len()) && hex(v));
            if ok { Ok(()) } else { Err("That isn't a supported keys file.".into()) }
        }
    }
}
pub fn import_key(dir: &Path, shared: &str, source: &Path) -> Result<(), String> {
    let target = key_path(dir, shared).ok_or("Unsupported shared key type.")?;
    if !source.is_file() || source.metadata().map_err(|_| "Couldn't read that keys file.")?.len() > 1024 * 1024 { return Err("Choose a keys file smaller than 1 MB.".into()); }
    let bytes = fs::read(source).map_err(|_| "Couldn't read that keys file.")?;
    check_key(shared, &bytes)?;
    let parent = target.parent().ok_or("Invalid key destination.")?;
    fs::create_dir_all(parent).map_err(|_| "Couldn't create the private keys folder.")?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).map_err(|_| "Couldn't protect the keys folder.")?;
    }
    // Private copy, never a link to a user's original or another project's writable file.
    crate::secret::write_private(&target, &bytes).map_err(|_| "Couldn't store that keys file.".to_string())
}

/// Never follow a manifest destination through a symlink/junction outside the install.
fn destination(install: &Path, rel: &str) -> Result<PathBuf, String> {
    if !relative(rel) { return Err("Input destination must be inside the installation.".into()); }
    let root = install.canonicalize().map_err(|_| "Installation folder is missing.")?;
    let target = install.join(rel);
    let mut parent = target.parent().ok_or("Invalid input destination.")?.to_path_buf();
    while !parent.exists() { parent = parent.parent().ok_or("Invalid input destination.")?.to_path_buf(); }
    if !parent.canonicalize().map_err(|_| "Couldn't check input destination.")?.starts_with(&root) { return Err("Input destination leaves the installation.".into()); }
    Ok(target)
}
/// Files this big (disc images) are placed as a hard link when they can be; smaller ones (DLC,
/// small data files a game might write to) are always copied.
pub const LINK_AT_LEAST: u64 = 512 * 1024 * 1024;

pub fn stage(p: &Profile, g: &mut Installed, dir: &Path) -> Result<(), String> {
    stage_cancellable(p, g, dir, &|| false)
}
pub fn stage_cancellable(p: &Profile, g: &mut Installed, dir: &Path, stopped: &dyn Fn() -> bool) -> Result<(), String> {
    let paths = resolve(p, g, dir)?;
    // Optional files removed in the picker must not remain active in the game's folder.
    let obsolete: Vec<_> = g.input_staged.iter().filter(|(id, rel)| !p.inputs.iter().any(|i| &i.id == *id && &i.destination == *rel && paths.contains_key(&i.id))).map(|(k, v)| (k.clone(), v.clone())).collect();
    for (id, rel) in obsolete {
        let target = destination(&g.path, &rel)?;
        if target.symlink_metadata().is_ok() {
            if !target.is_file() { return Err("A managed input destination isn't a file.".into()); }
            fs::remove_file(target).map_err(|_| "Couldn't remove an unused managed input file.")?;
        }
        g.input_staged.remove(&id);
        g.input_stamps.remove(&id);
    }
    for i in &p.inputs {
        if stopped() { return Err("Stopped.".into()); }
        let Some(source) = paths.get(&i.id).filter(|_| !i.destination.is_empty()) else { continue };
        let target = destination(&g.path, &i.destination)?;
        if !source.is_file() { return Err("Only files can have an installation destination.".into()); }
        if source.canonicalize().ok() == target.canonicalize().ok() { continue; }
        // A DLSS file the game ships itself is never replaced.
        if i.role == "runtime" && target.symlink_metadata().is_ok() && g.input_staged.get(&i.id) != Some(&i.destination) { continue; }
        if target.symlink_metadata().is_ok() && g.input_staged.get(&i.id) != Some(&i.destination) { return Err(format!("{} already exists in the installation. Move it aside before setup.", i.destination)); }
        let metadata = source.metadata().map_err(|_| "Couldn't read the selected file.")?;
        let stamp = format!("{}|{}|{:?}", source.display(), metadata.len(), metadata.modified().ok());
        if (i.shared.is_empty() || i.role == "runtime") && g.input_stamps.get(&i.id) == Some(&stamp) && target.metadata().is_ok_and(|m| m.is_file() && m.len() == metadata.len()) { continue; }
        fs::create_dir_all(target.parent().unwrap()).map_err(|_| "Couldn't create the input folder.")?;
        // Recheck after directory creation, including existing destination symlinks.
        destination(&g.path, &i.destination)?;
        if target.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink() || m.is_dir()) { return Err("Input destination isn't a regular file.".into()); }
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let temp = target.parent().unwrap().join(format!(".recomplauncher-input-{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        // A hard link shares the player's file itself: a game writing to it would change their
        // original. Only big disc images (read, never written) are linked; anything smaller is
        // copied.
        // DLSS is read, never written: linked like game files where it can be.
        let link_ok = metadata.len() >= LINK_AT_LEAST || i.role == "runtime";
        let result = if i.shared.is_empty() || i.role == "runtime" {
            (if link_ok { fs::hard_link(source, &temp) } else { Err(std::io::Error::other("copy")) }).or_else(|_| {
                use std::io::{Read, Write};
                let mut src = fs::File::open(source)?;
                let mut dst = fs::OpenOptions::new().write(true).create_new(true).open(&temp)?;
                let mut buffer = vec![0; 1024 * 1024];
                loop {
                    if stopped() { return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "Stopped")); }
                    let n = src.read(&mut buffer)?;
                    if n == 0 { break; }
                    dst.write_all(&buffer[..n])?;
                }
                dst.sync_all()
            })
        } else { fs::read(source).and_then(|b| crate::secret::write_private(&temp, &b)) };
        if result.is_err() || stopped() { let _ = fs::remove_file(&temp); }
        result.map_err(|_| format!("Couldn't place {} in the game's folder.", i.label))?;
        if stopped() { return Err("Stopped.".into()); }
        if target.exists() { fs::remove_file(&target).map_err(|_| "Couldn't replace the managed input file.")?; }
        fs::rename(&temp, &target).map_err(|_| "Couldn't finish placing the input file.")?;
        g.input_staged.insert(i.id.clone(), i.destination.clone());
        g.input_stamps.insert(i.id.clone(), stamp);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dlss_profile(optional: bool, extra: serde_json::Value) -> Profile {
        let mut v = serde_json::json!({ "start": { "program": ["game.exe"], "args": ["--fullscreen", "--dlss={input:dlss}"] }, "inputs": [
            { "id": "dlss", "label": "NVIDIA DLSS", "role": "runtime", "shared": "nvidia.dlss", "optional": optional, "destination": "nvngx_dlss.dll" }
        ]});
        if let Some(o) = extra.as_object() { for (k, x) in o { v["inputs"][0][k] = x.clone(); } }
        serde_json::from_value(v).unwrap()
    }
    #[test]
    fn dlss_is_runtime_only_may_be_optional_and_has_no_formats() {
        assert!(validate(&dlss_profile(true, serde_json::json!({}))).is_ok());
        assert!(validate(&dlss_profile(false, serde_json::json!({}))).is_ok());
        assert!(validate(&dlss_profile(true, serde_json::json!({ "role": "key" }))).is_err());
        assert!(validate(&dlss_profile(true, serde_json::json!({ "formats": [{ "type": "file" }] }))).is_err());
        // Runtime is only for nvidia.dlss, and keys stay required.
        let p: Profile = serde_json::from_value(serde_json::json!({ "inputs": [{ "id": "x", "label": "X", "role": "runtime", "formats": [{ "type": "file" }] }] })).unwrap();
        assert!(validate(&p).is_err());
        let p: Profile = serde_json::from_value(serde_json::json!({ "inputs": [{ "id": "k", "label": "K", "role": "key", "shared": "switch.prod-keys", "optional": true }] })).unwrap();
        assert!(validate(&p).is_err());
        // Once per profile.
        let mut twice = dlss_profile(true, serde_json::json!({}));
        let mut second = twice.inputs[0].clone(); second.id = "dlss2".into(); second.destination = "b/nvngx_dlss.dll".into();
        twice.inputs.push(second);
        assert!(validate(&twice).is_err());
    }
    #[test]
    fn unknown_optional_inputs_are_skipped_and_their_arguments_left_out() {
        let p: Profile = serde_json::from_value(serde_json::json!({ "start": { "program": ["g.exe"], "args": ["--fw={input:fw}", "--x"] }, "inputs": [
            { "id": "fw", "label": "Future thing", "role": "firmware", "shared": "future.thing", "optional": true }
        ]})).unwrap();
        let mut p2 = p.clone();
        prepare(&mut p2, true);
        assert!(p2.inputs.is_empty());
        assert!(validate(&p2).is_ok());
        assert_eq!(arguments(&p2.start.as_ref().unwrap().args, &BTreeMap::new(), false), vec!["--x".to_string()]);
        // The raw file also passes the checker (an older launcher would have failed here).
        assert!(validate(&p).is_ok());
        // A required one still fails.
        let mut req = p.clone(); req.inputs[0].optional = false;
        prepare(&mut req, true);
        assert!(validate(&req).is_err());
    }
    #[test]
    fn dlss_is_skipped_where_it_doesnt_apply() {
        let mut p = dlss_profile(true, serde_json::json!({}));
        prepare(&mut p, false);
        assert!(p.inputs.is_empty());
        assert!(validate(&p).is_ok());
        assert_eq!(arguments(&p.start.as_ref().unwrap().args, &BTreeMap::new(), false), vec!["--fullscreen".to_string()]);
    }
    #[test]
    fn dlss_is_placed_never_over_a_shipped_file_and_goes_when_turned_off() {
        let (_, mut g, dir) = fixture();
        let p = dlss_profile(true, serde_json::json!({}));
        // Not fetched yet: no row path, setup isn't blocked, the argument is left out.
        assert!(rows(&p, &g, &dir).unwrap()[0].path.is_none());
        assert!(resolve(&p, &g, &dir).is_ok());
        let sig = signature(&p, &g, &dir).unwrap();
        let shared = dir.join("shared-runtime/nvidia.dlss/310.9.16");
        fs::create_dir_all(&shared).unwrap();
        fs::write(shared.join("nvngx_dlss.dll"), b"nvidia").unwrap();
        fs::write(dir.join("shared-runtime/nvidia.dlss/current.txt"), "310.9.16").unwrap();
        // Arriving later doesn't ask for setup again.
        assert_eq!(signature(&p, &g, &dir).unwrap(), sig);
        stage(&p, &mut g, &dir).unwrap();
        assert_eq!(fs::read(dir.join("nvngx_dlss.dll")).unwrap(), b"nvidia");
        assert!(excluded(&g, &dir.join("nvngx_dlss.dll")), "kept out of saves and backups");
        // Turned off: the managed copy goes.
        g.runtime_off.insert("dlss".into());
        assert!(rows(&p, &g, &dir).unwrap()[0].off);
        stage(&p, &mut g, &dir).unwrap();
        assert!(!dir.join("nvngx_dlss.dll").exists());
        // A file the game ships is left alone and used.
        g.runtime_off.clear();
        fs::write(dir.join("nvngx_dlss.dll"), b"the game's own").unwrap();
        assert!(rows(&p, &g, &dir).unwrap()[0].ships);
        stage(&p, &mut g, &dir).unwrap();
        assert_eq!(fs::read(dir.join("nvngx_dlss.dll")).unwrap(), b"the game's own");
        fs::remove_dir_all(dir).unwrap();
    }
    fn fixture() -> (Profile, Installed, PathBuf) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!("recomp-inputs-{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        fs::create_dir_all(&dir).unwrap();
        let p: Profile = serde_json::from_value(serde_json::json!({"inputs":[
            {"id":"disc1","label":"Disc 1","role":"disc","formats":[{"type":"file","extensions":["nsp"],"requires":["keys"]},{"type":"folder"}]},
            {"id":"keys","label":"Switch keys","role":"key","shared":"switch.prod-keys"},
            {"id":"dlc","label":"DLC","role":"dlc","optional":true,"formats":[{"type":"file"}]}
        ]})).unwrap();
        (p, Installed { path: dir.clone(), ..Default::default() }, dir)
    }
    #[test]
    fn keys_only_for_declared_selected_format_and_reused() {
        let (p, mut g, dir) = fixture();
        assert!(!rows(&p, &g, &dir).unwrap()[1].active);
        g.setup_inputs.insert("disc1".into(), dir.clone());
        assert!(resolve(&p, &g, &dir).is_ok());
        let nsp = dir.join("game.NSP"); fs::write(&nsp, b"fake package").unwrap();
        g.setup_inputs.insert("disc1".into(), nsp);
        assert!(resolve(&p, &g, &dir).unwrap_err().contains("Switch keys"));
        let key = dir.join("source.keys"); fs::write(&key, format!("header_key = {}", "a".repeat(64))).unwrap();
        import_key(&dir, "switch.prod-keys", &key).unwrap();
        assert!(resolve(&p, &g, &dir).is_ok());
        g.inputs_ready = signature(&p, &g, &dir).unwrap(); assert!(ready(&p, &g, &dir));
        fs::remove_file(key_path(&dir, "switch.prod-keys").unwrap()).unwrap();
        assert!(!ready(&p, &g, &dir));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn scope_and_paths_are_closed() {
        let (mut p, _, dir) = fixture();
        p.inputs[1].shared = "ps2.bios".into(); assert!(validate(&p).is_err());
        for path in ["../keys", "C:/keys", "/keys", "a/../keys", "a\\keys", "a/./keys"] { assert!(!relative(path), "{path}"); }
        assert!(check_key("switch.prod-keys", b"not keys").is_err());
        assert!(check_key("wiiu.common-key", &[0;16]).is_ok());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_real_prod_keys_file_is_taken_and_title_keys_in_its_place_is_named() {
        let h64 = "a".repeat(64);
        let prod = format!("aes_kek_generation_source = {}\nheader_key = {h64}\nmaster_kek_source_12 = {}\neticket_rsa_keypair = {}\nssl_rsa_key = {}\n", "b".repeat(32), "c".repeat(34), "d".repeat(1056), "e".repeat(512));
        assert!(check_key("switch.prod-keys", prod.as_bytes()).is_ok());
        let title = format!("{} = {}\n{} = {}\n", "0".repeat(32), "1".repeat(32), "2".repeat(32), "3".repeat(32));
        assert!(check_key("switch.title-keys", title.as_bytes()).is_ok());
        assert_eq!(check_key("switch.prod-keys", title.as_bytes()).unwrap_err(), "That's title.keys. Pick prod.keys, the file with header_key in it.");
        assert_eq!(check_key("switch.title-keys", prod.as_bytes()).unwrap_err(), "That's prod.keys. Pick title.keys here.");
        let no_header = format!("aes_kek_generation_source = {}\n", "b".repeat(32));
        assert_eq!(check_key("switch.prod-keys", no_header.as_bytes()).unwrap_err(), "That keys file has no header_key. Dump prod.keys from your console again.");
        // An odd number of digits isn't a key.
        assert!(check_key("switch.prod-keys", format!("header_key = {h64}\nbad = {}\n", "f".repeat(33)).as_bytes()).is_err());
    }
    #[test]
    fn four_discs_three_dlc_and_optional_arguments() {
        let p: Profile = serde_json::from_str(include_str!("../../docs/examples/multidisc-inputs.json")).unwrap();
        validate(&p).unwrap();
        let (_, mut g, dir) = fixture();
        assert_eq!(rows(&p, &g, &dir).unwrap().len(), 7);
        assert!(resolve(&p, &g, &dir).is_err());
        for n in 1..=4 { let path = dir.join(format!("Disc {n}.iso")); fs::write(&path, b"not hashed").unwrap(); g.setup_inputs.insert(format!("disc{n}"), path); }
        let paths = resolve(&p, &g, &dir).unwrap();
        assert_eq!(paths.len(), 4);
        let args = arguments(&p.prepare.as_ref().unwrap().args, &paths, false);
        assert_eq!(args.len(), 4);
        assert!(args[0].contains("Disc 1.iso"));
        assert!(!args.iter().any(|a| a.contains("dlc")));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn staged_keys_are_private_copies_and_not_saves() {
        let (mut p, mut g, dir) = fixture();
        p.inputs[1].destination = "keys/private.bin".into();
        let nsp = dir.join("game.nsp"); fs::write(&nsp, b"data").unwrap(); g.setup_inputs.insert("disc1".into(), nsp);
        let key = dir.join("source.keys"); fs::write(&key, format!("header_key = {}", "a".repeat(64))).unwrap();
        import_key(&dir, "switch.prod-keys", &key).unwrap();
        stage(&p, &mut g, &dir).unwrap();
        let delivered = dir.join("keys/private.bin");
        assert!(excluded(&g, &delivered));
        fs::write(&delivered, b"changed by game").unwrap();
        assert_ne!(fs::read(&delivered).unwrap(), fs::read(key_path(&dir, "switch.prod-keys").unwrap()).unwrap());
        remove_key(&p, &g, &dir, "keys").unwrap();
        assert!(!delivered.exists()); assert!(key.exists());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn refuse_unmanaged_destination_and_cleanup_removed_dlc() {
        let (mut p, mut g, dir) = fixture();
        p.inputs[2].destination = "dlc.bin".into();
        g.setup_inputs.insert("disc1".into(), dir.clone());
        let source = dir.join("original-dlc.bin"); fs::write(&source, b"DLC").unwrap();
        g.setup_inputs.insert("dlc".into(), source.clone());
        let target = dir.join("dlc.bin"); fs::write(&target, b"user file").unwrap();
        assert!(stage(&p, &mut g, &dir).is_err()); assert_eq!(fs::read(&target).unwrap(), b"user file");
        fs::remove_file(&target).unwrap(); stage(&p, &mut g, &dir).unwrap();
        g.setup_inputs.remove("dlc"); stage(&p, &mut g, &dir).unwrap();
        assert!(!target.exists()); assert!(source.exists());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn changing_a_disc_asks_to_set_up_again() {
        let p: Profile = serde_json::from_str(include_str!("../../docs/examples/multidisc-inputs.json")).unwrap();
        let (_, mut g, dir) = fixture();
        for n in 1..=4 { let path = dir.join(format!("Disc {n}.iso")); fs::write(&path, b"disc").unwrap(); g.setup_inputs.insert(format!("disc{n}"), path); }
        g.inputs_ready = signature(&p, &g, &dir).unwrap();
        assert!(ready(&p, &g, &dir));
        // Another file picked for disc 2: not ready until it's set up again.
        let other = dir.join("Disc 2 (Rev 1).iso"); fs::write(&other, b"another disc").unwrap();
        g.setup_inputs.insert("disc2".into(), other);
        assert!(!ready(&p, &g, &dir));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn a_game_writing_a_placed_file_leaves_the_players_original() {
        let (mut p, mut g, dir) = fixture();
        p.inputs[2].destination = "dlc.bin".into();
        g.setup_inputs.insert("disc1".into(), dir.clone());
        let source = dir.join("original-dlc.bin"); fs::write(&source, b"DLC").unwrap();
        g.setup_inputs.insert("dlc".into(), source.clone());
        stage(&p, &mut g, &dir).unwrap();
        fs::write(dir.join("dlc.bin"), b"written by the game").unwrap();
        assert_eq!(fs::read(&source).unwrap(), b"DLC");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn stage_cancellable_cleans_up_temp_file_when_stopped() {
        let (mut p, mut g, dir) = fixture();
        p.inputs[2].destination = "dlc.bin".into();
        g.setup_inputs.insert("disc1".into(), dir.clone());
        let source = dir.join("original-dlc.bin");
        fs::write(&source, vec![42u8; 2 * 1024 * 1024]).unwrap();
        g.setup_inputs.insert("dlc".into(), source.clone());
        let res = stage_cancellable(&p, &mut g, &dir, &|| true);
        assert!(res.is_err());
        assert!(!dir.join("dlc.bin").exists());
        let has_temp = fs::read_dir(&dir).unwrap().flatten().any(|e| {
            e.file_name().to_string_lossy().starts_with(".recomplauncher-input-")
        });
        assert!(!has_temp, "Temporary .recomplauncher-input file was not cleaned up!");
        fs::remove_dir_all(dir).unwrap();
    }
}

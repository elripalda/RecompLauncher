//! Preparation fingerprints are versioned in pending records; existing builds keep their rules.
use crate::{inputs, library::Installed, profile::Profile};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, time::UNIX_EPOCH};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputSignatureVersion {
    #[default]
    Legacy,
    TreeV1,
}

impl InputSignatureVersion {
    pub fn is_legacy(&self) -> bool { *self == Self::Legacy }
}

fn put(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}

fn tree(hash: &mut Sha256, root: &Path, stopped: &dyn Fn() -> bool) -> Result<(), String> {
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        if stopped() { return Err("Stopped.".into()); }
        let mut entries = fs::read_dir(&directory).map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            if stopped() { return Err("Stopped.".into()); }
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
            let relative = path.strip_prefix(root).map_err(|e| e.to_string())?;
            put(hash, relative.as_os_str().as_encoded_bytes());
            let kind = metadata.file_type();
            let tag = if kind.is_symlink() { b"link".as_slice() } else if kind.is_dir() { b"directory".as_slice() } else if kind.is_file() { b"file".as_slice() } else { return Err("A selected input folder contains an unsupported file type.".into()); };
            put(hash, tag);
            hash.update(metadata.len().to_le_bytes());
            let modified = metadata.modified().map_err(|e| e.to_string())?;
            let (negative, elapsed) = match modified.duration_since(UNIX_EPOCH) { Ok(time) => (false, time), Err(error) => (true, error.duration()) };
            hash.update([u8::from(negative)]);
            hash.update(elapsed.as_nanos().to_le_bytes());
            if kind.is_symlink() {
                // The target text is part of the snapshot; following it can escape or cycle.
                put(hash, fs::read_link(&path).map_err(|e| e.to_string())?.as_os_str().as_encoded_bytes());
            } else if kind.is_dir() { pending.push(path); }
        }
    }
    Ok(())
}

/// The first engine fingerprint. Saved work folders and completed results are named by it, so its
/// bytes must never change, even though the launcher's own setup fingerprints have since gone.
/// Engine profiles only have named inputs: the `null` is where a preparation step would be hashed.
fn selected_files_signature(profile: &Profile, game: &Installed, data: &Path) -> Result<String, String> {
    let paths = inputs::resolve(profile, game, data)?;
    let mut hash = Sha256::new();
    hash.update(serde_json::to_vec(&profile.inputs).map_err(|_| "Invalid inputs")?);
    hash.update(b"null");
    for (id, path) in paths {
        let runtime = profile.inputs.iter().any(|input| input.id == id && input.role == "runtime");
        if runtime { continue; }
        hash.update(id.as_bytes());
        hash.update(path.to_string_lossy().as_bytes());
        let meta = path.metadata().map_err(|_| "A selected input is no longer readable.")?;
        hash.update(meta.len().to_le_bytes());
        hash.update(format!("{:?}", meta.modified().ok()).as_bytes());
    }
    Ok(hex::encode(hash.finalize()))
}

pub fn input_signature(profile: &Profile, game: &Installed, data: &Path) -> Result<String, String> {
    input_signature_with_cancel(profile, game, data, &|| false)
}

pub fn input_signature_with_cancel(profile: &Profile, game: &Installed, data: &Path, stopped: &dyn Fn() -> bool) -> Result<String, String> {
    if stopped() { return Err("Stopped.".into()); }
    let saved = super::pending::read(game)?;
    let profile = saved.as_ref().and_then(|p| p.input_profile.as_ref()).unwrap_or(profile);
    let legacy = selected_files_signature(profile, game, data)?;
    let version = saved.as_ref().map(|p| p.input_signature_version).unwrap_or_default();
    if version == InputSignatureVersion::Legacy { return Ok(legacy); }
    let mut hash = Sha256::new();
    put(&mut hash, legacy.as_bytes());
    for (id, path) in inputs::resolve(profile, game, data)? {
        let runtime = profile.inputs.iter().any(|input| input.id == id && input.role == "runtime");
        if runtime || !path.metadata().map_err(|e| e.to_string())?.is_dir() { continue; }
        put(&mut hash, id.as_bytes());
        tree(&mut hash, &path, stopped)?;
    }
    Ok(format!("engine-tree-v1:{}", hex::encode(hash.finalize())))
}

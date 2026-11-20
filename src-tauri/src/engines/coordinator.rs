use crate::library::{Installed, SetupMode};
use std::{fs, path::Path, sync::{Arc, atomic::AtomicBool}};
/// Build from a private copy so generated files cannot mutate a verified package cache.
pub fn copy_tree(from: &Path, to: &Path, stopped: &dyn Fn() -> bool) -> Result<(), String> {
    if stopped() { return Err("Stopped.".into()); }
    fn destination(path: &Path) -> Result<std::path::PathBuf, String> {
        use std::path::{Component, PathBuf};
        let absolute = if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir().map_err(|e| e.to_string())?.join(path) };
        let mut resolved = PathBuf::new();
        for part in absolute.components() {
            match part {
                Component::CurDir => continue,
                Component::ParentDir => { resolved.pop(); }
                Component::Prefix(_) | Component::RootDir => resolved.push(part.as_os_str()),
                Component::Normal(name) => {
                    resolved.push(name);
                    match fs::symlink_metadata(&resolved) {
                        Ok(_) => resolved = resolved.canonicalize().map_err(|e| e.to_string())?,
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
                        Err(e) => return Err(e.to_string()),
                    }
                }
            }
        }
        Ok(resolved)
    }
    let base = from.canonicalize().map_err(|e| e.to_string())?;
    // Resolve existing parent links even when the destination does not exist yet.
    if destination(to)?.starts_with(&base) { return Err("A copy destination cannot be inside its source folder or replace the source itself.".into()); }
    fn copy(from: &Path, to: &Path, base: &Path, stopped: &dyn Fn() -> bool) -> Result<(), String> {
        if stopped() { return Err("Stopped.".into()); }
        let m = fs::symlink_metadata(from).map_err(|e| e.to_string())?;
        if m.file_type().is_symlink() {
            let dest = from.canonicalize().map_err(|e| e.to_string())?;
            if !dest.starts_with(base) { return Err("A component links outside its package.".into()); }
            #[cfg(unix)] { std::os::unix::fs::symlink(fs::read_link(from).map_err(|e| e.to_string())?, to).map_err(|e| e.to_string())?; }
            #[cfg(not(unix))] { return Err("This package needs symbolic link support.".into()); }
        } else if m.is_dir() {
            fs::create_dir_all(to).map_err(|e| e.to_string())?;
            for e in fs::read_dir(from).map_err(|e| e.to_string())? { let e = e.map_err(|e| e.to_string())?; copy(&e.path(), &to.join(e.file_name()), base, stopped)?; }
        } else if m.is_file() {
            fs::copy(from, to).map_err(|e| e.to_string())?;
        } else { return Err("A component contains an unsupported file type.".into()); }
        Ok(())
    }
    copy(from, to, &base, stopped)
}

pub(super) fn validate_tree(root: &Path) -> Result<(), String> {
    let base = root.canonicalize().map_err(|e| e.to_string())?;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in fs::read_dir(dir).map_err(|e| e.to_string())? {
            let e = e.map_err(|e| e.to_string())?;
            let kind = e.file_type().map_err(|e| e.to_string())?;
            if kind.is_symlink() {
                if !e.path().canonicalize().map_err(|e| e.to_string())?.starts_with(&base) { return Err("The prepared app links outside its installation.".into()); }
            } else if kind.is_dir() { stack.push(e.path()); }
        }
    }
    Ok(())
}


pub fn prepare(http: &reqwest::Client, data: &Path, game: &Installed, cancel: Arc<AtomicBool>, progress: &(dyn Fn(super::Progress) + Send + Sync)) -> Result<Installed, String> {
    let saved = super::pending::read(game)?.ok_or("This game is already prepared.")?;
    super::registry::get(&saved.package.engine)?.prepare(http, data, game, cancel, progress)
}

pub fn activate_output(current: &Installed, ready: &Installed, stopped: &dyn Fn() -> bool) -> Result<Installed, String> {
    let saved = super::pending::read(current)?.ok_or("This installation is already prepared.")?;
    let mut active = super::registry::get(&saved.package.engine)?.activate(current, ready, stopped)?;
    // Closing Easy Setup after success must keep the inputs for the activated build.
    active.setup_before = None;
    active.mode = SetupMode::Easy;
    Ok(active)
}

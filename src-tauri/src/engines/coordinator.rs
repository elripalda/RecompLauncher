use crate::library::Installed;
use std::{fs, path::Path, sync::{Arc, atomic::AtomicBool}};
/// Copy package sources separately: nxnative builds tools and generated files in its engine tree.
pub fn copy_tree(from: &Path, to: &Path, stopped: &dyn Fn() -> bool) -> Result<(), String> {
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
    copy(from, to, &from.canonicalize().map_err(|e| e.to_string())?, stopped)
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
    super::registry::get(&saved.package.engine)?.activate(current, ready, stopped)
}

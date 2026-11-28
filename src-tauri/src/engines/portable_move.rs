//! Moving a prepared game between drives. Its frozen tools link to each other inside the folder,
//! so unlike an ordinary game's move, a link that leaves the folder is refused before any copying.

use crate::manage::{checked_copy_root, copy_file, move_with_copy};
use crate::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Prepared engines must keep their frozen tools inside the installation after a copy.
/// Ordinary games may have external or optional links, so they use the normal move path.
pub fn move_portable_dir(from: &Path, to: &Path, report: &dyn Fn(u64, u64)) -> Result<(), Error> {
    move_with_copy(from, to, report, copy_portable_tree)
}

fn copy_portable_tree(from: &Path, to: &Path, done: &mut u64, total: u64, report: &dyn Fn(u64, u64)) -> io::Result<()> {
    let base = checked_copy_root(from, to)?;
    // Validate the complete link graph before writing. A framework may link through
    // Versions/Current; traversing those links during copying would duplicate it or loop.
    validate_copy_links(&base, &base, &mut Default::default(), &mut Default::default())?;
    copy_at(from, to, &base, done, total, report)?;
    // A relative path may pass through an alias before its .. segments. Recheck
    // in the new location so a copied link cannot still depend on the old folder.
    let copied = to.canonicalize()?;
    if let Err(error) = validate_copy_links(&copied, &copied, &mut Default::default(), &mut Default::default()) {
        let _ = fs::remove_dir_all(to);
        return Err(error);
    }
    Ok(())
}

fn copy_link_target(path: &Path, base: &Path) -> io::Result<PathBuf> {
    let target = fs::read_link(path)?;
    if target.is_absolute() || target.components().any(|part| matches!(part, std::path::Component::Prefix(_))) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "A game contains an absolute link that would not survive a move."));
    }
    let parent = path.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "A link has no parent folder."))?.canonicalize()?;
    let mut depth = parent.strip_prefix(base).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "A link is outside its installation folder."))?.components().count();
    for part in target.components() {
        match part {
            std::path::Component::ParentDir => {
                if depth == 0 { return Err(io::Error::new(io::ErrorKind::InvalidData, "A relative link leaves its installation folder before resolving.")); }
                depth -= 1;
            }
            std::path::Component::Normal(_) => depth += 1,
            _ => {}
        }
    }
    let resolved = path.canonicalize()?;
    if !resolved.starts_with(base) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "A game links outside its installation folder."));
    }
    Ok(resolved)
}

fn validate_copy_links(path: &Path, base: &Path, active: &mut std::collections::HashSet<PathBuf>, visited: &mut std::collections::HashSet<PathBuf>) -> io::Result<()> {
    if visited.contains(path) { return Ok(()); }
    if !active.insert(path.to_path_buf()) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "A game contains a directory link cycle."));
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = if kind.is_symlink() { copy_link_target(&entry.path(), base)? } else { entry.path() };
        let metadata = fs::metadata(&target)?;
        if metadata.is_dir() {
            validate_copy_links(&target, base, active, visited)?;
        } else if !metadata.is_file() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "A game contains an unsupported file type."));
        }
    }
    active.remove(path);
    visited.insert(path.to_path_buf());
    Ok(())
}

fn copy_at(from: &Path, to: &Path, base: &Path, done: &mut u64, total: u64, report: &dyn Fn(u64, u64)) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for e in fs::read_dir(from)? {
        let e = e?;
        let (src, dst) = (e.path(), to.join(e.file_name()));
        let kind = e.file_type()?;
        if kind.is_symlink() {
            let resolved = copy_link_target(&src, base)?;
            let target = fs::read_link(&src)?;
            #[cfg(unix)]
            { let _ = resolved; std::os::unix::fs::symlink(&target, &dst)?; }
            #[cfg(windows)]
            {
                if resolved.is_dir() { std::os::windows::fs::symlink_dir(&target, &dst)?; }
                else { std::os::windows::fs::symlink_file(&target, &dst)?; }
            }
            *done += fs::symlink_metadata(&src)?.len();
            report(*done, total);
            continue;
        }
        if kind.is_dir() {
            copy_at(&src, &dst, base, done, total, report)?;
            continue;
        }
        copy_file(&src, &dst, done, total, report)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manage::dir_size;

    fn tmp(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rl-portable-{n}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[cfg(unix)]
    #[test]
    fn cross_drive_copy_preserves_sdk_directory_and_file_links() {
        use std::os::unix::fs::symlink;
        let d = tmp("copy-sdk-links");
        let source = d.join("game");
        fs::create_dir_all(source.join("SDK/Framework/Versions/A/Headers")).unwrap();
        fs::write(source.join("SDK/Framework/Versions/A/Headers/api.h"), b"header").unwrap();
        symlink("A", source.join("SDK/Framework/Versions/Current")).unwrap();
        symlink("Versions/Current/Headers", source.join("SDK/Framework/Headers")).unwrap();
        symlink("Headers/api.h", source.join("SDK/Framework/api.h")).unwrap();
        let target = d.join("other drive ü/game");
        let mut done = 0;
        copy_portable_tree(&source, &target, &mut done, dir_size(&source), &|_, _| {}).unwrap();
        fs::remove_dir_all(&source).unwrap();
        assert_eq!(fs::read_link(target.join("SDK/Framework/Headers")).unwrap(), Path::new("Versions/Current/Headers"));
        assert_eq!(fs::read_link(target.join("SDK/Framework/api.h")).unwrap(), Path::new("Headers/api.h"));
        assert_eq!(fs::read(target.join("SDK/Framework/api.h")).unwrap(), b"header");
        assert_eq!(fs::read(target.join("SDK/Framework/Versions/Current/Headers/api.h")).unwrap(), b"header");
        fs::remove_dir_all(d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn cross_drive_copy_refuses_links_outside_the_game() {
        let d = tmp("copy-escaping-link");
        let source = d.join("game");
        fs::create_dir(&source).unwrap();
        fs::write(d.join("outside"), b"private").unwrap();
        std::os::unix::fs::symlink("../outside", source.join("escape")).unwrap();
        let target = d.join("copied");
        assert!(copy_portable_tree(&source, &target, &mut 0, 0, &|_, _| {}).is_err());
        assert!(!target.exists());
        assert_eq!(fs::read(d.join("outside")).unwrap(), b"private");
        fs::remove_dir_all(d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn cross_drive_copy_refuses_relative_links_that_leave_and_reenter_the_game() {
        let d = tmp("copy-reentering-link");
        let source = d.join("old-library/Game");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("real"), b"data").unwrap();
        std::os::unix::fs::symlink("../../old-library/Game/real", source.join("link")).unwrap();
        let target = d.join("new-library/Game");
        assert!(copy_portable_tree(&source, &target, &mut 0, 0, &|_, _| {}).is_err());
        assert!(!target.exists());
        assert_eq!(fs::read(source.join("real")).unwrap(), b"data");
        fs::remove_dir_all(d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn cross_drive_copy_refuses_aliases_that_resolve_through_the_old_library() {
        let d = tmp("copy-alias-reentry");
        let source = d.join("old-library/Game");
        fs::create_dir_all(source.join("d1/d2")).unwrap();
        fs::create_dir(source.join("shallow")).unwrap();
        fs::write(source.join("real"), b"data").unwrap();
        std::os::unix::fs::symlink("../../shallow", source.join("d1/d2/s")).unwrap();
        std::os::unix::fs::symlink("s/../../../old-library/Game/real", source.join("d1/d2/link")).unwrap();
        let target = d.join("new-library/Game");
        assert!(copy_portable_tree(&source, &target, &mut 0, 0, &|_, _| {}).is_err());
        assert!(!target.exists());
        assert_eq!(fs::read(source.join("real")).unwrap(), b"data");
        fs::remove_dir_all(d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn cross_drive_copy_refuses_absolute_links_even_inside_the_game() {
        let d = tmp("copy-absolute-link");
        let source = d.join("game");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("file"), b"data").unwrap();
        std::os::unix::fs::symlink(source.join("file"), source.join("absolute")).unwrap();
        let target = d.join("copied");
        assert!(copy_portable_tree(&source, &target, &mut 0, 0, &|_, _| {}).is_err());
        assert!(!target.exists());
        fs::remove_dir_all(d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn cross_drive_copy_refuses_directory_link_cycles_before_writing() {
        let d = tmp("copy-link-cycle");
        let source = d.join("game");
        fs::create_dir_all(source.join("first")).unwrap();
        fs::create_dir(source.join("second")).unwrap();
        std::os::unix::fs::symlink("../second", source.join("first/other")).unwrap();
        std::os::unix::fs::symlink("../first", source.join("second/other")).unwrap();
        let target = d.join("copied");
        assert!(copy_portable_tree(&source, &target, &mut 0, 0, &|_, _| {}).is_err());
        assert!(!target.exists());
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    #[ignore = "requires RL_COPY_TREE_FIXTURE pointing to a real tree with relative links"]
    fn cross_drive_copy_of_a_real_tree_preserves_all_relative_links() {
        let source = PathBuf::from(std::env::var_os("RL_COPY_TREE_FIXTURE").unwrap());
        let d = tmp("copy-real-tree");
        let target = d.join("moved tree ü");
        let mut done = 0;
        let total = dir_size(&source);
        copy_portable_tree(&source, &target, &mut done, total, &|_, _| {}).unwrap();
        assert_eq!(done, total);
        let base = target.canonicalize().unwrap();
        let mut directories = vec![source.clone()];
        let mut links = 0;
        while let Some(directory) = directories.pop() {
            for entry in fs::read_dir(directory).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                let kind = entry.file_type().unwrap();
                if kind.is_dir() { directories.push(path); continue; }
                if !kind.is_symlink() { continue; }
                let copied = target.join(path.strip_prefix(&source).unwrap());
                assert_eq!(fs::read_link(&copied).unwrap(), fs::read_link(&path).unwrap());
                assert!(copied.canonicalize().unwrap().starts_with(&base));
                links += 1;
            }
        }
        assert!(links > 0);
        fs::remove_dir_all(d).unwrap();
    }
}

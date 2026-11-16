//! Update backups belong to the launcher, so a folder-only game grant is enough to use them.

use crate::library::{folder_name, Installed};
use crate::{inputs, manage, Error, Res};
use std::fs;
use std::path::{Path, PathBuf};

pub fn directory(game: &Installed, app_dir: &Path) -> PathBuf {
    directory_for(app_dir, &game.id)
}

pub fn directory_for(app_dir: &Path, id: &str) -> PathBuf {
    app_dir.join("backups").join(folder_name(id))
}

pub fn rename_project(app_dir: &Path, old: &str, new: &str) -> Res<Option<(PathBuf, PathBuf)>> {
    let from = directory_for(app_dir, old);
    let to = directory_for(app_dir, new);
    if from == to || !from.try_exists()? {
        return Ok(None);
    }
    if to.try_exists()? {
        return Err(Error::Msg("The new project already has update backups. Keep both histories before changing its project.".into()));
    }
    fs::rename(&from, &to)?;
    Ok(Some((from, to)))
}

fn legacy_directory(game: &Installed) -> PathBuf {
    game.path.parent().unwrap_or(Path::new(".")).join(".backups").join(game.path.file_name().unwrap_or_default())
}

pub fn create(game: &Installed, app_dir: &Path) -> Res<Option<manage::Backup>> {
    manage::backup(&game.path, &inputs::release_files(game), &directory(game, app_dir), 5)
}

pub fn list(game: &Installed, app_dir: &Path) -> Vec<manage::Backup> {
    let mut backups = manage::list_backups(&directory(game, app_dir));
    for backup in manage::list_backups(&legacy_directory(game)) {
        if !backups.iter().any(|b| b.name == backup.name) {
            backups.push(backup);
        }
    }
    backups.sort_by(|a, b| b.when.cmp(&a.when));
    backups.truncate(5);
    backups
}

pub fn restore(game: &Installed, app_dir: &Path, name: &str) -> Res<usize> {
    let backup = list(game, app_dir).into_iter().find(|b| b.name == name)
        .ok_or_else(|| Error::Msg("That isn't one of its backups.".into()))?;
    manage::restore(&backup.file, &game.path)
}

/// Preserve readable legacy backups before Move changes their inferred location. Private backups
/// are keyed by game ID and stay in place. Old copies are left alone, including on read-only drives.
pub fn preserve_before_move(game: &Installed, app_dir: &Path) -> Res<()> {
    let backups = manage::list_backups(&legacy_directory(game));
    if backups.is_empty() {
        return Ok(());
    }
    let destination = directory(game, app_dir);
    fs::create_dir_all(&destination)?;
    for backup in backups {
        let target = destination.join(&backup.name);
        if target.exists() {
            continue;
        }
        let temporary = target.with_extension("importing");
        fs::copy(&backup.file, &temporary)?;
        fs::rename(temporary, target)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("recomp-backups-{}-{time}", std::process::id()));
        fs::create_dir(&root).unwrap();
        root
    }

    #[cfg(unix)]
    #[test]
    fn backup_list_and_restore_work_with_only_the_game_folder_accessible() {
        use std::os::unix::fs::PermissionsExt;
        let root = temp();
        let parent = root.join("restricted");
        let path = parent.join("game");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("save.sav"), b"save").unwrap();
        let game = Installed { id: "game".into(), path: path.clone(), ..Default::default() };
        let app_dir = root.join("data");
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o111)).unwrap();
        let result = (|| -> Res<()> {
            let backup = create(&game, &app_dir)?.unwrap();
            assert!(backup.file.starts_with(&app_dir));
            assert_eq!(list(&game, &app_dir).len(), 1);
            fs::write(path.join("save.sav"), b"changed")?;
            restore(&game, &app_dir, &backup.name)?;
            assert_eq!(fs::read(path.join("save.sav"))?, b"save");
            Ok(())
        })();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();
        result.unwrap();
        assert!(!parent.join(".backups").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn backups_remain_restorable_after_moving_the_game() {
        let root = temp();
        let path = root.join("game");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("save.sav"), b"save").unwrap();
        let mut game = Installed { id: "game".into(), path: path.clone(), ..Default::default() };
        let app_dir = root.join("data");
        let backup = create(&game, &app_dir).unwrap().unwrap();
        preserve_before_move(&game, &app_dir).unwrap();
        let moved = root.join("moved");
        fs::rename(path, &moved).unwrap();
        game.path = moved;
        fs::write(game.path.join("save.sav"), b"changed").unwrap();
        assert_eq!(directory(&game, &app_dir), backup.file.parent().unwrap());
        restore(&game, &app_dir, &backup.name).unwrap();
        assert_eq!(fs::read(game.path.join("save.sav")).unwrap(), b"save");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn readable_legacy_backups_are_preserved_before_move() {
        let root = temp();
        let path = root.join("game");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("save.sav"), b"legacy save").unwrap();
        let mut game = Installed { id: "game".into(), path, ..Default::default() };
        let app_dir = root.join("data");
        let backup = manage::backup(&game.path, &[], &legacy_directory(&game), 5).unwrap().unwrap();
        assert_eq!(list(&game, &app_dir).len(), 1);
        preserve_before_move(&game, &app_dir).unwrap();
        let moved = root.join("moved");
        fs::rename(&game.path, &moved).unwrap();
        game.path = moved;
        fs::write(game.path.join("save.sav"), b"changed").unwrap();
        restore(&game, &app_dir, &backup.name).unwrap();
        assert_eq!(fs::read(game.path.join("save.sav")).unwrap(), b"legacy save");
        assert!(backup.file.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn backup_write_errors_are_returned_to_the_update_caller() {
        let root = temp();
        let path = root.join("game");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("save.sav"), b"save").unwrap();
        let game = Installed { id: "game".into(), path, ..Default::default() };
        let app_dir = root.join("not-a-directory");
        fs::write(&app_dir, b"blocked").unwrap();
        assert!(create(&game, &app_dir).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}

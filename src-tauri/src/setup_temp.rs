//! What a setup step (a game's `prepare`) runs with: a temp folder of its own on the game's drive,
//! room on that drive, and plain words when Windows' path limit stops it. Only setup runs get the
//! temp folder: Play keeps the system's.

use std::path::{Path, PathBuf};

/// `<game>/.recomplauncher-tmp`: TMPDIR, TEMP and TMP for one setup run. On the game's drive, so a
/// builder unpacking gigabytes doesn't fill a small system drive (or the memory /tmp is on some
/// Linux systems), and taken away when the run ends.
pub const FOLDER: &str = ".recomplauncher-tmp";
/// The file inside that says the launcher made the folder, so the launcher may empty it. A folder
/// by that name without it (a release that ships one) is never touched.
const MARK: &str = ".made-by-recomplauncher";

/// Room a setup step needs on the game's drive before it starts.
pub const ROOM: u64 = 1 << 30;

pub const NO_ROOM: &str = "Setup needs at least 1 GB free on the game's drive. Free some up, or move the game to another drive, then try again.";
pub const NOT_OURS: &str = "The game's folder has a .recomplauncher-tmp folder the launcher didn't make, so setup won't use it. Move it out of the game's folder, then try again.";
pub const CANT_MAKE: &str = "Couldn't make a temp folder for setup in the game's folder. Check the drive isn't full or read-only, then try again.";
pub const TOO_LONG: &str = "A path in the game's folder is too long for Windows. Add a library folder with a shorter path in Settings → Games and Folders and move the game there, or turn on Windows' long paths setting (LongPathsEnabled), then try again. The launcher doesn't change that setting for you.";

/// One setup run's temp folder. Dropping it takes the folder away, whether the run worked, failed
/// or was stopped, unless it was left for programs the run started (`leave`).
pub struct SetupTemp {
    dir: PathBuf,
    left: bool,
}

impl SetupTemp {
    /// An empty `<game>/.recomplauncher-tmp` for this run. One left by a run that couldn't clean up
    /// (the launcher closed mid-setup) is emptied first, but only when the launcher made it.
    pub fn make(install: &Path) -> Result<SetupTemp, &'static str> {
        let dir = install.join(FOLDER);
        if std::fs::symlink_metadata(&dir).is_ok() {
            if !made_here(&dir) {
                return Err(NOT_OURS);
            }
            std::fs::remove_dir_all(&dir).map_err(|_| CANT_MAKE)?;
        }
        std::fs::create_dir_all(&dir).map_err(|_| CANT_MAKE)?;
        std::fs::write(dir.join(MARK), b"").map_err(|_| CANT_MAKE)?;
        Ok(SetupTemp { dir, left: false })
    }

    /// Kept when the run ends: a program the step started (a build it handed off to) still runs and
    /// still writes there. The next setup run empties it (`make`).
    pub fn leave(&mut self) {
        self.left = true;
    }

    pub fn path(&self) -> &Path {
        &self.dir
    }

    /// The names programs read their temp folder from, on Linux, Mac and Windows.
    pub fn env(&self) -> [(&'static str, &Path); 3] {
        [("TMPDIR", self.dir.as_path()), ("TEMP", self.dir.as_path()), ("TMP", self.dir.as_path())]
    }
}

impl Drop for SetupTemp {
    fn drop(&mut self) {
        if !self.left && made_here(&self.dir) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

/// A real folder (never a link to somewhere else) with the launcher's mark in it.
fn made_here(dir: &Path) -> bool {
    let link = std::fs::symlink_metadata(dir).map(|m| m.file_type().is_symlink()).unwrap_or(true);
    !link && dir.is_dir() && dir.join(MARK).is_file()
}

/// Whether a drive with `free` bytes left has room for a setup step. A drive that can't say how
/// much is free (some network folders) gets the benefit of the doubt.
pub fn room_for_setup(free: Option<u64>) -> bool {
    free.is_none_or(|f| f >= ROOM)
}

/// Words Windows programs (and their runtimes) print when its 260-character path limit stopped
/// them: .NET's PathTooLongException, Win32's ERROR_FILENAME_EXCED_RANGE (206), and plain ones.
pub fn says_too_long(said: &str) -> bool {
    // Not MAX_PATH: it's a constant's name, which a build can print in a warning that stopped nothing.
    const WORDS: [&str; 6] = ["path too long", "path is too long", "filename or extension is too long", "pathtoolongexception", "filename_exced_range", "os error 206"];
    let s = said.to_ascii_lowercase();
    WORDS.iter().any(|w| s.contains(w))
}

/// TOO_LONG, then what the step said last on a line of its own. A step can print a path warning
/// early and stop later for another reason: its last line keeps that reason in front of the
/// player. The UI translates the first line and leaves the step's own words as they are.
pub fn too_long_with(said: &str) -> String {
    if said.is_empty() {
        return TOO_LONG.to_string();
    }
    format!("{TOO_LONG}\n{said}")
}

/// An error from Windows' path limit (ERROR_FILENAME_EXCED_RANGE).
pub fn io_too_long(e: &std::io::Error) -> bool {
    cfg!(windows) && e.raw_os_error() == Some(206)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rl-setup-temp-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_temp_folder_is_made_for_a_run_and_gone_after() {
        let d = scratch("run");
        let t = SetupTemp::make(&d).unwrap();
        assert_eq!(t.path(), d.join(FOLDER));
        std::fs::write(t.path().join("unpacked.bin"), b"x").unwrap();
        drop(t);
        assert!(!d.join(FOLDER).exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn one_left_by_an_earlier_run_starts_empty() {
        let d = scratch("left");
        std::mem::forget(SetupTemp::make(&d).unwrap());
        std::fs::write(d.join(FOLDER).join("old.bin"), b"x").unwrap();
        let t = SetupTemp::make(&d).unwrap();
        assert!(!t.path().join("old.bin").exists());
        drop(t);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_folder_the_launcher_didnt_make_is_never_emptied() {
        let d = scratch("theirs");
        std::fs::create_dir_all(d.join(FOLDER)).unwrap();
        std::fs::write(d.join(FOLDER).join("release.txt"), b"theirs").unwrap();
        assert_eq!(SetupTemp::make(&d).err(), Some(NOT_OURS));
        assert_eq!(std::fs::read(d.join(FOLDER).join("release.txt")).unwrap(), b"theirs");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_file_by_that_name_is_never_removed() {
        let d = scratch("file");
        std::fs::write(d.join(FOLDER), b"a release file").unwrap();
        assert_eq!(SetupTemp::make(&d).err(), Some(NOT_OURS));
        assert_eq!(std::fs::read(d.join(FOLDER)).unwrap(), b"a release file");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn its_mark_taken_out_mid_run_keeps_the_folder() {
        // Something else replaced what the launcher made: it's not the launcher's to delete.
        let d = scratch("unmarked");
        let t = SetupTemp::make(&d).unwrap();
        std::fs::remove_file(t.path().join(MARK)).unwrap();
        drop(t);
        assert!(d.join(FOLDER).is_dir());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn it_points_all_three_temp_names_at_it() {
        let d = scratch("env");
        let t = SetupTemp::make(&d).unwrap();
        let names: Vec<&str> = t.env().iter().map(|(k, _)| *k).collect();
        assert_eq!(names, ["TMPDIR", "TEMP", "TMP"]);
        assert!(t.env().iter().all(|(_, v)| *v == t.path()));
        drop(t);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn setup_waits_for_a_gigabyte() {
        assert!(room_for_setup(None));
        assert!(room_for_setup(Some(ROOM)));
        assert!(!room_for_setup(Some(ROOM - 1)));
    }

    #[test]
    fn windows_path_limit_messages_are_recognised() {
        assert!(says_too_long("Unhandled exception. System.IO.PathTooLongException: The path is too long"));
        assert!(says_too_long("error: The filename or extension is too long. (os error 206)"));
        assert!(!says_too_long("error: file not found"));
    }

    #[test]
    fn a_mention_of_max_path_alone_isnt_the_path_limit() {
        assert!(!says_too_long("warning: MAX_PATH is 260 on this PC, long paths may not work"));
    }

    #[test]
    fn the_long_path_message_keeps_what_the_step_said_last() {
        assert_eq!(too_long_with("error: out of disk space"), format!("{TOO_LONG}\nerror: out of disk space"));
        assert_eq!(too_long_with(""), TOO_LONG);
    }

    #[test]
    fn a_deep_library_folder_takes_the_temp_folder() {
        // Past Windows' old 260-character limit: the launcher's own file work still goes through.
        let d = scratch("deep");
        let mut deep = d.clone();
        while deep.as_os_str().len() < 300 {
            deep = deep.join("a-library-folder-with-a-long-name");
        }
        std::fs::create_dir_all(&deep).unwrap();
        let t = SetupTemp::make(&deep).unwrap();
        std::fs::write(t.path().join("unpacked.bin"), b"x").unwrap();
        drop(t);
        assert!(!deep.join(FOLDER).exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}

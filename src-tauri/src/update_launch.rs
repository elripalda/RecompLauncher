//! Starting the Windows installer for an update or a visible retry.

use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

/// How the installer runs.
pub enum Install {
    /// No installer window; the launcher opens again after only when `reopen` is set.
    Quiet { reopen: bool },
    /// The installer's own window, which has its own button to open the launcher.
    Shown,
}

/// The launcher was asked to close for good (a quit from the tray or the window, or `--quit`).
static QUITTING: AtomicBool = AtomicBool::new(false);

pub fn note_quitting() {
    QUITTING.store(true, Ordering::Relaxed);
}

pub fn quitting() -> bool {
    QUITTING.load(Ordering::Relaxed)
}

/// Whether the launcher opens again once its update is in. An update takes a while, and a player
/// who closed the window meanwhile (to the tray, or quitting) is done with it: it stays closed.
pub fn reopen_after(window_shown: bool, quitting: bool) -> bool {
    window_shown && !quitting
}

pub fn installer_command(installer: &Path, portable_dir: Option<&Path>, how: Install) -> Command {
    let mut command = Command::new(installer);
    if let Install::Quiet { reopen } = how {
        // /S: no installer window at all (the passive one flashed its Back button as it finished);
        // the launcher says it is installing.
        command.args(["/S", "/UPDATE"]);
        // /R: RecompLauncher opens again after.
        if reopen {
            command.arg("/R");
        }
    }
    if let Some(dir) = portable_dir {
        install_dir(&mut command, dir);
    }
    command
}

fn install_dir(command: &mut Command, dir: &Path) {
    // NSIS reads /D= from the raw command-line tail. It must be last and unquoted, even when
    // the directory contains spaces: https://nsis.sourceforge.io/Docs/Chapter3.html#installerusage
    let mut argument = std::ffi::OsString::from("/D=");
    argument.push(dir.as_os_str());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.raw_arg(argument);
    }
    #[cfg(not(windows))]
    command.arg(argument);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn installed_updates_keep_their_flags_and_no_directory_override() {
        let command = installer_command(Path::new("installer.exe"), None, Install::Quiet { reopen: true });
        assert_eq!(command.get_program(), OsStr::new("installer.exe"));
        assert_eq!(command.get_args().collect::<Vec<_>>(), ["/S", "/UPDATE", "/R"]);
        let retry = installer_command(Path::new("installer.exe"), None, Install::Shown);
        assert_eq!(retry.get_args().count(), 0);
    }

    #[test]
    fn an_update_the_player_closed_during_installs_without_reopening() {
        let command = installer_command(Path::new("installer.exe"), None, Install::Quiet { reopen: false });
        assert_eq!(command.get_args().collect::<Vec<_>>(), ["/S", "/UPDATE"]);
    }

    #[test]
    fn portable_updates_and_retries_put_the_directory_last() {
        let dir = Path::new(r"C:\Portable Games\RecompLauncher");
        let quiet = installer_command(Path::new("installer.exe"), Some(dir), Install::Quiet { reopen: true });
        let quiet_args: Vec<_> = quiet.get_args().collect();
        assert_eq!(quiet_args, ["/S", "/UPDATE", "/R", r"/D=C:\Portable Games\RecompLauncher"]);
        let closed = installer_command(Path::new("installer.exe"), Some(dir), Install::Quiet { reopen: false });
        let closed_args: Vec<_> = closed.get_args().collect();
        assert_eq!(closed_args, ["/S", "/UPDATE", r"/D=C:\Portable Games\RecompLauncher"]);
        let shown = installer_command(Path::new("installer.exe"), Some(dir), Install::Shown);
        let shown_args: Vec<_> = shown.get_args().collect();
        assert_eq!(shown_args, [r"/D=C:\Portable Games\RecompLauncher"]);
    }

    #[test]
    fn the_launcher_reopens_only_when_its_window_is_still_open() {
        assert!(reopen_after(true, false));
    }

    #[test]
    fn closing_to_the_tray_during_an_update_keeps_it_closed() {
        assert!(!reopen_after(false, false));
    }

    #[test]
    fn quitting_during_an_update_keeps_it_closed() {
        assert!(!reopen_after(true, true));
        assert!(!reopen_after(false, true));
    }

    #[cfg(windows)]
    #[test]
    fn windows_passes_the_install_directory_without_quotes() {
        // NSIS consumes the raw command-line tail, not CRT argv. cmd's Unicode echo preserves
        // quotes in that tail, so this catches Command::arg inserting them around a spaced path.
        for dir in [r"C:\Portable Games\RecompLauncher", r"C:\Игры\Portable Launcher", r"\\server\shared games\Launcher"] {
            let mut command = Command::new("cmd.exe");
            command.args(["/D", "/U", "/C", "echo"]);
            install_dir(&mut command, Path::new(dir));
            let output = command.output().expect("run the Windows command-line probe");
            assert!(output.status.success(), "probe failed: {:?}", output.status);
            let words: Vec<_> = output.stdout.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
            let tail = String::from_utf16(&words).expect("Unicode command-line output");
            assert_eq!(tail.trim_end_matches(['\r', '\n']), format!("/D={dir}"));
        }
    }
}

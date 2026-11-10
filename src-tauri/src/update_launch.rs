//! Starting the Windows installer for an update or a visible retry.

use std::path::Path;
use std::process::Command;

pub fn installer_command(installer: &Path, portable_dir: Option<&Path>, background: bool) -> Command {
    let mut command = Command::new(installer);
    if background {
        command.args(["/P", "/UPDATE", "/R"]);
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
        let command = installer_command(Path::new("installer.exe"), None, true);
        assert_eq!(command.get_program(), OsStr::new("installer.exe"));
        assert_eq!(command.get_args().collect::<Vec<_>>(), ["/P", "/UPDATE", "/R"]);
        let retry = installer_command(Path::new("installer.exe"), None, false);
        assert_eq!(retry.get_args().count(), 0);
    }

    #[test]
    fn portable_updates_and_retries_put_the_directory_last() {
        let dir = Path::new(r"C:\Portable Games\RecompLauncher");
        for background in [true, false] {
            let command = installer_command(Path::new("installer.exe"), Some(dir), background);
            let args: Vec<_> = command.get_args().collect();
            assert_eq!(args.last().copied(), Some(OsStr::new(r"/D=C:\Portable Games\RecompLauncher")));
            if background {
                assert_eq!(&args[..args.len() - 1], ["/P", "/UPDATE", "/R"]);
            } else {
                assert_eq!(args.len(), 1);
            }
        }
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

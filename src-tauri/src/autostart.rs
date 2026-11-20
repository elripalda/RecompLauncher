//! Settings → Start with your PC: the launcher starts when the player signs in.
//!
//! The entry goes in the system's own list of programs that start at sign-in, so the player sees
//! it there and can turn it off there too: Startup apps on Windows (the Run key), Login Items on a
//! Mac (SMAppService), and the desktop's Autostart list on Linux (an XDG autostart file, which
//! Plasma and GNOME both list). A Flatpak can't write that file from its sandbox: it asks the
//! Background portal, which writes it for the app and may ask the player first.

use std::path::Path;

/// Started at sign-in: the window waits by the clock until the player opens it.
pub const ARG: &str = "--background";

/// The system can start the launcher at sign-in. A Mac before macOS 13 has no Login Items call
/// for it.
pub fn supported() -> bool {
    imp::supported()
}

/// What the system's own list says now. None where the launcher can't see the list (a Flatpak:
/// the portal keeps it), so the player's saved choice stands.
pub fn current() -> Option<bool> {
    imp::current()
}

/// Turns it on or off. Gives back whether it's on afterwards: the system can say no.
pub fn set(on: bool, data: &Path) -> Result<bool, String> {
    imp::set(on, data)
}

/// At start, the saved choice follows the system's list: the player may have turned it off (or
/// on) there since. An entry that's still there is written again, so it follows the launcher if it
/// moved (a portable folder, a renamed AppImage).
pub fn refresh(saved: bool, data: &Path) -> bool {
    let Some(now) = current() else {
        return saved;
    };
    if now && imp::REWRITES {
        if let Err(e) = set(true, data) {
            crate::trail::note(format!("Start with your PC: {e}"));
        }
    }
    now
}

/// Startup apps (Settings or Task Manager) turn an entry off without removing it, with a mark
/// whose first byte is odd.
#[cfg_attr(not(windows), allow(dead_code))]
fn marked_off(mark: &[u8]) -> bool {
    mark.first().is_some_and(|b| b & 1 == 1)
}

/// An autostart file the desktop starts: Plasma and GNOME each have a way to keep the file but
/// turn it off.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn entry_on(text: &str) -> bool {
    let off = |line: &str| line == "Hidden=true" || line == "X-GNOME-Autostart-enabled=false";
    !text.lines().map(str::trim).any(off)
}

/// One argument on a desktop file's Exec line. Anything with a space or a character the line
/// treats specially goes in quotes, with ", `, $ and \ escaped. The whole line is a desktop-file
/// string as well, so each backslash is then written twice, and % is written %%.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn exec_arg(arg: &str) -> String {
    let arg = arg.replace('%', "%%");
    let special = |c: char| c.is_whitespace() || "\"'\\><~|&;$*?#()`".contains(c);
    if !arg.is_empty() && !arg.contains(special) {
        return arg;
    }
    let mut quoted = String::from("\"");
    for c in arg.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    quoted.replace('\\', "\\\\")
}

/// The autostart file for a launcher that isn't sandboxed.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn entry_text(exec: &str, icon: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=RecompLauncher\nComment=Recompiled and decompiled PC games\nExec={exec}\nIcon={icon}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
    )
}

#[cfg(windows)]
mod imp {
    use crate::win::{self, HKCU};
    use std::path::Path;

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
    const NAME: &str = "RecompLauncher";
    pub const REWRITES: bool = true;

    pub fn supported() -> bool {
        true
    }

    pub fn current() -> Option<bool> {
        if win::reg_str(HKCU, RUN, NAME).is_none() {
            return Some(false);
        }
        Some(!super::marked_off(&win::reg_bytes(HKCU, APPROVED, NAME)))
    }

    pub fn set(on: bool, _data: &Path) -> Result<bool, String> {
        if !on {
            win::reg_delete(HKCU, RUN, NAME)?;
            return Ok(false);
        }
        let exe = crate::shortcuts::launcher_exe();
        let command = format!("\"{}\" {}", exe.display(), super::ARG);
        win::reg_set_str(HKCU, RUN, NAME, &command)?;
        // Turned off in Startup apps before: turning it on here turns it on there too.
        win::reg_delete(HKCU, APPROVED, NAME)?;
        Ok(true)
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::path::{Path, PathBuf};

    /// A Flatpak never gets here: its list can't be read (current is None), and asking the portal
    /// again at every start could bring back an entry the player removed.
    pub const REWRITES: bool = true;

    pub fn supported() -> bool {
        true
    }

    /// The same name as the app's own desktop file.
    fn entry_file() -> Option<PathBuf> {
        Some(dirs::config_dir()?.join("autostart").join("recomplauncher.desktop"))
    }

    pub fn current() -> Option<bool> {
        if crate::flatpak::sandboxed() {
            return None;
        }
        let text = entry_file().and_then(|f| std::fs::read_to_string(f).ok());
        Some(text.is_some_and(|t| super::entry_on(&t)))
    }

    pub fn set(on: bool, data: &Path) -> Result<bool, String> {
        if crate::flatpak::sandboxed() {
            return super::portal::request(on);
        }
        let file = entry_file().ok_or("Couldn't find your settings folder.")?;
        if !on {
            return match std::fs::remove_file(&file) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("Couldn't remove the startup entry: {e}")),
                _ => Ok(false),
            };
        }
        let (program, pre) = crate::shortcuts::start_command();
        let args = std::iter::once(program.to_string_lossy().to_string()).chain(pre).chain([super::ARG.to_string()]);
        let exec = args.map(|a| super::exec_arg(&a)).collect::<Vec<_>>().join(" ");
        let text = super::entry_text(&exec, &icon(data));
        let dir = file.parent().ok_or("Couldn't find your settings folder.")?;
        std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't make the autostart folder: {e}"))?;
        std::fs::write(&file, text).map_err(|e| format!("Couldn't write the startup entry: {e}"))?;
        Ok(true)
    }

    /// Installed (a .deb, an Arch package, Nix), the launcher's icon is in the PC's icon theme and
    /// the entry names it. Otherwise (an AppImage, whose own icon is gone once it closes, or a
    /// build run from its folder) a copy goes in the launcher's data folder: without one, the
    /// Autostart list shows a blank page for it.
    fn icon(data: &Path) -> String {
        if in_icon_theme() {
            return "recomplauncher".into();
        }
        let copy = data.join("recomplauncher.png");
        let png = include_bytes!("../icons/128x128@2x.png");
        if std::fs::write(&copy, png).is_err() {
            return "recomplauncher".into();
        }
        copy.to_string_lossy().to_string()
    }

    /// The icon theme has the launcher's icon at some size, in any of the folders the desktop
    /// looks in (XDG_DATA_HOME and XDG_DATA_DIRS).
    fn in_icon_theme() -> bool {
        let dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_default();
        let system = dirs.split(':').filter(|d| !d.is_empty()).map(PathBuf::from);
        let defaults = [PathBuf::from("/usr/local/share"), PathBuf::from("/usr/share")];
        let mut all = dirs::data_dir().into_iter().chain(system).chain(defaults);
        all.any(|d| has_icon(&d.join("icons/hicolor")))
    }

    fn has_icon(hicolor: &Path) -> bool {
        let Ok(sizes) = std::fs::read_dir(hicolor) else {
            return false;
        };
        sizes.flatten().any(|size| size.path().join("apps/recomplauncher.png").is_file())
    }
}

/// The Background portal: a Flatpak asks it to start the app at sign-in. The portal writes the
/// autostart file itself (with `flatpak run`), once the desktop allows it.
#[cfg(target_os = "linux")]
mod portal {
    use std::collections::HashMap;
    use zbus::blocking::{Connection, Proxy};
    use zbus::zvariant::{OwnedValue, Value};

    const DEST: &str = "org.freedesktop.portal.Desktop";
    const PATH: &str = "/org/freedesktop/portal/desktop";

    pub fn request(on: bool) -> Result<bool, String> {
        ask(on).map_err(|e| {
            crate::trail::note(format!("Background portal: {e}"));
            "Your desktop didn't answer the request to start RecompLauncher when you sign in.".to_string()
        })
    }

    fn ask(on: bool) -> zbus::Result<bool> {
        let conn = Connection::session()?;
        // The answer comes as a signal on a request object whose path is known up front: listening
        // before asking means a quick answer can't be missed.
        let sender = conn.unique_name().map(|n| n.trim_start_matches(':').replace('.', "_")).unwrap_or_default();
        let token = format!("recomplauncher_{}", std::process::id());
        let request_path = format!("{PATH}/request/{sender}/{token}");
        let request = Proxy::new(&conn, DEST, request_path.as_str(), "org.freedesktop.portal.Request")?;
        let mut answers = request.receive_signal("Response")?;
        let commandline = vec!["recomplauncher", super::ARG];
        let options: HashMap<&str, Value> = HashMap::from([
            ("handle_token", Value::from(token.as_str())),
            ("reason", Value::from("Start RecompLauncher when you sign in")),
            ("autostart", Value::from(on)),
            ("commandline", Value::from(commandline)),
            ("dbus-activatable", Value::from(false)),
        ]);
        let background = Proxy::new(&conn, DEST, PATH, "org.freedesktop.portal.Background")?;
        let _: zbus::zvariant::OwnedObjectPath = background.call("RequestBackground", &("", options))?;
        let Some(answer) = answers.next() else {
            return Err(zbus::Error::Failure("no answer".into()));
        };
        let (code, results): (u32, HashMap<String, OwnedValue>) = answer.body().deserialize()?;
        // 0 is yes; 1 the player said no, 2 the desktop couldn't.
        if code != 0 {
            return Ok(false);
        }
        let autostart = results.get("autostart").and_then(|v| v.downcast_ref::<bool>().ok());
        Ok(autostart.unwrap_or(on))
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use objc2_service_management::{SMAppService, SMAppServiceStatus};
    use std::path::Path;

    /// Login Items keep track of the app wherever it's moved.
    pub const REWRITES: bool = false;

    /// Login Items for the app itself came with macOS 13.
    pub fn supported() -> bool {
        let info = plist::Value::from_file("/System/Library/CoreServices/SystemVersion.plist").ok();
        let version = info.as_ref().and_then(|v| v.as_dictionary()?.get("ProductVersion")?.as_string().map(str::to_string));
        let major = version.and_then(|v| v.split('.').next()?.parse::<u32>().ok());
        major.is_some_and(|m| m >= 13)
    }

    pub fn current() -> Option<bool> {
        if !supported() {
            return Some(false);
        }
        let status = unsafe { SMAppService::mainAppService().status() };
        Some(status == SMAppServiceStatus::Enabled)
    }

    // A login item starts the app as a click on it does, with no arguments: on a Mac the window
    // opens at login instead of waiting by the clock.
    pub fn set(on: bool, _data: &Path) -> Result<bool, String> {
        if !supported() {
            return Err("Starting with your Mac needs macOS 13 or newer.".into());
        }
        let service = unsafe { SMAppService::mainAppService() };
        let listed = matches!(unsafe { service.status() }, SMAppServiceStatus::Enabled | SMAppServiceStatus::RequiresApproval);
        if !on && !listed {
            return Ok(false);
        }
        let done = if on { unsafe { service.registerAndReturnError() } } else { unsafe { service.unregisterAndReturnError() } };
        if let Err(e) = done {
            crate::trail::note(format!("Login item: {e:?}"));
            return Err("Your Mac didn't change RecompLauncher's login item.".into());
        }
        let status = unsafe { service.status() };
        if status == SMAppServiceStatus::RequiresApproval {
            // Turned off in Login Items before: only the player can allow it again, there.
            unsafe { SMAppService::openSystemSettingsLoginItems() };
            return Err("Allow RecompLauncher in System Settings under Login Items. It opens now.".into());
        }
        Ok(status == SMAppServiceStatus::Enabled)
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod imp {
    use std::path::Path;

    pub const REWRITES: bool = false;

    pub fn supported() -> bool {
        false
    }

    pub fn current() -> Option<bool> {
        Some(false)
    }

    pub fn set(_on: bool, _data: &Path) -> Result<bool, String> {
        Err("This system can't start RecompLauncher when you sign in.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_startup_app_turned_off_in_windows_is_off() {
        assert!(marked_off(&[0x03, 0, 0, 0]));
        assert!(marked_off(&[0x07]));
    }

    #[test]
    fn a_startup_app_without_a_mark_or_turned_on_is_on() {
        assert!(!marked_off(&[]));
        assert!(!marked_off(&[0x02, 0, 0, 0]));
        assert!(!marked_off(&[0x06]));
    }

    #[test]
    fn an_autostart_file_turned_off_in_plasma_is_off() {
        assert!(!entry_on("[Desktop Entry]\nExec=recomplauncher\nHidden=true\n"));
    }

    #[test]
    fn an_autostart_file_turned_off_in_gnome_is_off() {
        assert!(!entry_on("[Desktop Entry]\nExec=recomplauncher\nX-GNOME-Autostart-enabled=false\n"));
    }

    #[test]
    fn the_launchers_own_autostart_file_is_on() {
        assert!(entry_on(&entry_text("recomplauncher --background", "recomplauncher")));
    }

    #[test]
    fn a_plain_path_goes_on_the_exec_line_as_it_is() {
        assert_eq!(exec_arg("/usr/bin/recomplauncher"), "/usr/bin/recomplauncher");
    }

    #[test]
    fn a_path_with_a_space_is_quoted() {
        assert_eq!(exec_arg("/home/a/My Apps/RecompLauncher.AppImage"), "\"/home/a/My Apps/RecompLauncher.AppImage\"");
    }

    #[test]
    fn a_dollar_sign_is_escaped_inside_the_quotes() {
        assert_eq!(exec_arg("/home/a/$x"), "\"/home/a/\\\\$x\"");
    }

    #[test]
    fn a_percent_sign_is_written_twice() {
        assert_eq!(exec_arg("/home/a/100%"), "/home/a/100%%");
    }

    #[test]
    fn the_autostart_file_starts_in_the_background() {
        let text = entry_text("/usr/bin/recomplauncher --background", "recomplauncher");
        assert!(text.contains("\nExec=/usr/bin/recomplauncher --background\n"));
        assert!(text.contains("\nName=RecompLauncher\n"));
        assert!(text.contains("\nIcon=recomplauncher\n"));
    }
}

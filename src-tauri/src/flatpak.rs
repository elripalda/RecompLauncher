//! Builds released as a Flatpak bundle (a `.flatpak` file): Linux can't run the file itself, so
//! it's installed for this user the first time it's played, then started through Flatpak.

use std::io::Read;
use std::path::{Path, PathBuf};

/// The app a bundle holds (`io.github.hedge_dev.unleashedrecomp`), from its ref
/// (`app/<id>/<arch>/<branch>`), which a bundle keeps near its start.
pub fn app_id(bundle: &Path) -> Option<String> {
    let mut head = vec![0u8; 1 << 20];
    let n = std::fs::File::open(bundle).and_then(|mut f| f.read(&mut head)).ok()?;
    ref_in(&head[..n])
}

fn ref_in(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let re = regex_lite::Regex::new(r"app/([A-Za-z_][A-Za-z0-9_-]*(?:\.[A-Za-z0-9_-]+){2,})/(?:x86_64|aarch64|i386|arm)/[A-Za-z0-9_.-]+").ok()?;
    re.captures(&text).map(|c| c[1].to_string())
}

/// The launcher itself runs as a Flatpak (built from packaging/flatpak). Its sandbox has no `flatpak` of its own:
/// the PC's is reached through `flatpak-spawn --host`.
pub fn sandboxed() -> bool {
    cfg!(target_os = "linux") && (std::env::var_os("FLATPAK_ID").is_some() || Path::new("/.flatpak-info").exists())
}

/// GTK names the windows after the binary (`recomplauncher`), which matches no desktop file in
/// the Flatpak: X11 desktops and Wayland ones that don't ask Flatpak (Sway, Hyprland) then show a
/// generic icon. Named after the Flatpak's ID, they match its desktop file everywhere.
#[cfg(target_os = "linux")]
pub fn name_windows_after_app() {
    let Ok(id) = std::env::var("FLATPAK_ID") else {
        return;
    };
    webkit2gtk::glib::set_prgname(Some(&id));
}

/// A folder on disk for what a program unpacks to run (an AppImage), since the sandbox's own
/// /tmp is memory.
pub fn scratch_dir() -> Option<PathBuf> {
    let dir = dirs::cache_dir()?.join("appimage-run");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Where a game's AppImage is kept unpacked for the Flatpak: in the app's cache, not the game's
/// folder (an update would back it up as the player's own files).
fn unpacked_dir(id: &str) -> Option<PathBuf> {
    Some(dirs::cache_dir()?.join("appimage-run").join("unpacked").join(crate::library::folder_name(id)))
}

/// What says which AppImage a folder was unpacked from: its size and when it was written. An
/// update replaces the file, so it's unpacked again.
fn appimage_stamp(appimage: &Path) -> Option<String> {
    let m = std::fs::metadata(appimage).ok()?;
    let t = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    Some(format!("{} {t}", m.len()))
}

/// The game's AppImage unpacked once (there's no FUSE in the sandbox to mount it), the folder
/// with its AppRun: unpacked the first time it's played and after an update, kept after that,
/// so later starts don't unpack it again.
pub fn unpacked_appimage(appimage: &Path, id: &str) -> Result<PathBuf, String> {
    let base = unpacked_dir(id).ok_or("no cache folder")?;
    let stamp = appimage_stamp(appimage).ok_or("can't read the AppImage")?;
    let root = base.join("squashfs-root");
    if is_unpacked(appimage, id) {
        return Ok(root);
    }
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    // `--appimage-extract` writes squashfs-root into the folder it runs in; it needs no FUSE.
    let mut c = std::process::Command::new(appimage);
    c.arg("--appimage-extract").current_dir(&base).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    crate::sysenv::clean_std(&mut c);
    let ok = c.status().map(|s| s.success()).unwrap_or(false);
    if !ok || !root.join("AppRun").is_file() {
        let _ = std::fs::remove_dir_all(&base);
        return Err("it didn't unpack".into());
    }
    std::fs::write(base.join("stamp"), &stamp).map_err(|e| e.to_string())?;
    Ok(root)
}

/// This AppImage is unpacked already (the same file: no update since).
pub fn is_unpacked(appimage: &Path, id: &str) -> bool {
    let (Some(base), Some(stamp)) = (unpacked_dir(id), appimage_stamp(appimage)) else { return false };
    std::fs::read_to_string(base.join("stamp")).ok().as_deref() == Some(stamp.as_str()) && base.join("squashfs-root/AppRun").is_file()
}

/// A game's unpacked AppImage, once it's uninstalled.
pub fn forget_unpacked(id: &str) {
    if let Some(d) = unpacked_dir(id) {
        let _ = std::fs::remove_dir_all(d);
    }
}

fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(name)).find(|p| p.is_file())
}

/// The sandbox may talk to the PC's Flatpak (`flatpak override --user --talk-name=org.freedesktop.Flatpak <app>`):
/// the session bus rules in /.flatpak-info say so.
fn may_use_host_flatpak() -> bool {
    talks_to_flatpak(&std::fs::read_to_string("/.flatpak-info").unwrap_or_default())
}

fn talks_to_flatpak(info: &str) -> bool {
    let mut in_policy = false;
    for line in info.lines().map(str::trim) {
        if line.starts_with('[') {
            in_policy = line == "[Session Bus Policy]";
        } else if in_policy {
            if let Some(level) = line.strip_prefix("org.freedesktop.Flatpak=") {
                return matches!(level.trim(), "talk" | "own");
            }
        }
    }
    false
}

/// In the sandbox, and the PC's Flatpak is out of reach until the player allows it.
pub fn host_flatpak_blocked() -> bool {
    sandboxed() && !may_use_host_flatpak()
}

/// The program that runs `flatpak`, and what goes before its arguments.
fn flatpak() -> Option<(PathBuf, &'static [&'static str])> {
    if sandboxed() {
        return on_path("flatpak-spawn").filter(|_| may_use_host_flatpak()).map(|p| (p, &["--host", "flatpak"][..]));
    }
    on_path("flatpak").map(|p| (p, &[][..]))
}

/// Flatpak is on this PC.
pub fn available() -> bool {
    flatpak().is_some()
}

fn std_cmd() -> std::process::Command {
    let (program, prefix) = flatpak().unwrap_or_else(|| (PathBuf::from("flatpak"), &[]));
    let mut c = std::process::Command::new(program);
    c.args(prefix);
    crate::sysenv::clean_std(&mut c);
    c
}

/// Installed already, for this user or for everyone.
pub fn installed(id: &str) -> bool {
    std_cmd().args(["info", id]).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
}

/// Installs the bundle for this user, with what it needs from its usual place (Flathub, for most).
/// What Flatpak said last when it didn't.
pub fn install(bundle: &Path) -> Result<(), String> {
    let out = std_cmd().args(["install", "--user", "--noninteractive", "-y", "--bundle"]).arg(bundle).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        return Ok(());
    }
    let said = String::from_utf8_lossy(&out.stderr).lines().rev().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").chars().take(200).collect::<String>();
    Err(if said.is_empty() { format!("it stopped with code {}", out.status.code().unwrap_or(-1)) } else { said })
}

/// Starts it. Flatpak stays open until the game closes, so its exit code is the game's.
pub fn command(id: &str) -> tokio::process::Command {
    let (program, prefix) = flatpak().unwrap_or_else(|| (PathBuf::from("flatpak"), &[]));
    let mut c = tokio::process::Command::new(program);
    c.args(prefix).arg("run").arg(id);
    c
}

/// Taken off again with the game, when the launcher put it there.
pub fn remove(id: &str) {
    let _ = std_cmd().args(["uninstall", "--user", "--noninteractive", "-y", id]).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
}

/// A bundle file in the game's folder, and the app it holds.
pub fn bundle_in(dir: &Path) -> Option<(PathBuf, String)> {
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.extension().map(|x| x.eq_ignore_ascii_case("flatpak")).unwrap_or(false)).find_map(|p| app_id(&p).map(|id| (p, id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_flatpak_permission() {
        let info = "[Application]
name=a.b.C

[Session Bus Policy]
org.freedesktop.Flatpak=talk
org.kde.StatusNotifierWatcher=talk
";
        assert!(talks_to_flatpak(info));
        assert!(!talks_to_flatpak("[Session Bus Policy]
org.kde.StatusNotifierWatcher=talk
"));
        assert!(!talks_to_flatpak("[Session Bus Policy]
org.freedesktop.Flatpak=none
"));
        // Only the session bus rules count.
        assert!(!talks_to_flatpak("[System Bus Policy]
org.freedesktop.Flatpak=talk
"));
        assert!(!talks_to_flatpak(""));
    }

    #[test]
    fn the_app_in_a_bundle() {
        let mut b = b"\x00\x01flatpak\x00metadata\x00".to_vec();
        b.extend_from_slice(b"\x00\x00ref\x00app/io.github.hedge_dev.unleashedrecomp/x86_64/stable\x00\x00runtime=org.freedesktop.Platform/x86_64/24.08");
        assert_eq!(ref_in(&b).as_deref(), Some("io.github.hedge_dev.unleashedrecomp"));
        assert_eq!(ref_in(b"runtime/org.freedesktop.Platform/x86_64/24.08"), None);
        assert_eq!(ref_in(b"app/notanid/x86_64/stable"), None);
    }
}

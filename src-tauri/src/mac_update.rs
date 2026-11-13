//! The launcher's own update on a Mac: the new RecompLauncher.app comes out of the release's disk
//! image and takes the place of the running one, then opens again once this copy has quit.

use std::path::{Path, PathBuf};

pub enum Failed {
    /// The disk image holds another version than the one on offer (a file published by mistake).
    OtherVersion(String),
    Msg(String),
}

/// The .app this launcher runs from, when it runs from one (not from `cargo run`).
pub fn running_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    bundle_of(&exe)
}

/// Contents/MacOS/<program> inside X.app gives X.app.
fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos_dir = exe.parent()?;
    let contents = macos_dir.parent()?;
    let app = contents.parent()?;
    let in_bundle = macos_dir.file_name()? == "MacOS" && contents.file_name()? == "Contents";
    let is_app = app.extension().is_some_and(|e| e.eq_ignore_ascii_case("app"));
    (in_bundle && is_app).then(|| app.to_path_buf())
}

/// Whether this account can put a new copy next to the app and swap it in. Not when it runs
/// straight from the disk image, from a folder macOS moved it to (App Translocation, read only),
/// or from a folder only an administrator can change.
pub fn can_replace(bundle: &Path) -> bool {
    let Some(parent) = bundle.parent() else { return false };
    let probe = parent.join(format!(".recomplauncher-write-test-{}", std::process::id()));
    if std::fs::create_dir(&probe).is_err() {
        return false;
    }
    let _ = std::fs::remove_dir(&probe);
    true
}

/// The one .app the disk image held (next to it there's only the Applications link, left out).
#[cfg(target_os = "macos")]
fn app_in(dir: &Path) -> Option<PathBuf> {
    let mut apps = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| crate::macos::is_bundle(p));
    let app = apps.next()?;
    apps.next().is_none().then_some(app)
}

#[cfg(target_os = "macos")]
fn version_of(app: &Path) -> Option<String> {
    let v = plist::Value::from_file(app.join("Contents/Info.plist")).ok()?;
    v.as_dictionary()?.get("CFBundleShortVersionString")?.as_string().map(str::to_string)
}

#[cfg(target_os = "macos")]
fn run(program: &str, args: &[&str], path: &Path) -> bool {
    std::process::Command::new(program).args(args).arg(path).status().is_ok_and(|s| s.success())
}

#[cfg(target_os = "macos")]
fn signed(app: &Path) -> bool {
    run("/usr/bin/codesign", &["--verify", "--deep", "--strict"], app)
}

/// The previous version, kept next to the new one until the new one has started.
fn old_of(bundle: &Path) -> PathBuf {
    bundle.with_file_name(".RecompLauncher-old.app")
}

/// Copies the new app next to `bundle`, checks it and swaps it in, keeping the previous version
/// aside (see `start_and_watch` and `forget_old`). The running copy keeps working until it quits:
/// its files stay open after they're gone from the folder.
#[cfg(target_os = "macos")]
pub fn install(dmg: &Path, bundle: &Path, version: &str) -> Result<(), Failed> {
    // Next to the app, so the swap is a rename on the same disk and can't be left half done.
    let work = bundle.with_file_name(".RecompLauncher-update");
    let old = old_of(bundle);
    let _ = std::fs::remove_dir_all(&work);
    let _ = std::fs::remove_dir_all(&old);
    let put = swap_in(dmg, bundle, version, &work, &old);
    let _ = std::fs::remove_dir_all(&work);
    put
}

#[cfg(target_os = "macos")]
fn swap_in(dmg: &Path, bundle: &Path, version: &str, work: &Path, old: &Path) -> Result<(), Failed> {
    std::fs::create_dir_all(work).map_err(|e| Failed::Msg(format!("Couldn't make room for the new version next to this one: {e}")))?;
    crate::install::undmg(dmg, work).map_err(|e| Failed::Msg(e.to_string()))?;
    let new_app = app_in(work).ok_or_else(|| Failed::Msg("The new version's disk image has no RecompLauncher.app in it.".into()))?;
    let found = version_of(&new_app).unwrap_or_default();
    if !version.is_empty() && found != version {
        return Err(Failed::OtherVersion(found));
    }
    // Files this app downloads aren't quarantined, but a copy made by hand could be: macOS would
    // then ask about it, or refuse it, on the next start.
    run("/usr/bin/xattr", &["-dr", "com.apple.quarantine"], &new_app);
    // The release is signed ad hoc and the copy keeps that signature. One that doesn't verify was
    // changed after it was built, so it's refused, not signed again (that would let anything in).
    if !signed(&new_app) {
        return Err(Failed::Msg("The new version's signature doesn't check out, so it wasn't put in place. Download it from recomplauncher.com instead.".into()));
    }
    if crate::macos::bundle_executable(&new_app, crate::assets::arch_now()).is_none() {
        return Err(Failed::Msg("The new version has no program for this Mac in it, so it wasn't put in place.".into()));
    }
    std::fs::rename(bundle, old).map_err(|e| Failed::Msg(format!("Couldn't move this version aside for the new one: {e}")))?;
    if let Err(e) = std::fs::rename(&new_app, bundle) {
        let _ = std::fs::rename(old, bundle);
        return Err(Failed::Msg(format!("Couldn't put the new version in place: {e}")));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn install(_dmg: &Path, _bundle: &Path, _version: &str) -> Result<(), Failed> {
    Err(Failed::Msg("Only a Mac updates from a disk image.".into()))
}

/// The previous version isn't needed: the new one won't be started now (the player had closed the
/// window), so there's nothing to watch.
pub fn forget_old(bundle: &Path) {
    let _ = std::fs::remove_dir_all(old_of(bundle));
}

/// Waits for this process to quit, starts the new version and watches it for a while (see
/// mac_update.sh). If it crashes or macOS stops it, the previous version goes back and opens; the
/// next start then tells the player (update-pending.json, see update_check_last).
const WATCH: &str = include_str!("mac_update.sh");

pub fn start_and_watch(bundle: &Path) -> std::io::Result<()> {
    let program = crate::macos::bundle_executable(bundle, crate::assets::arch_now()).ok_or_else(|| std::io::Error::other("the new version has no program for this Mac"))?;
    let failed = bundle.with_file_name(".RecompLauncher-failed.app");
    let mut c = std::process::Command::new("/bin/sh");
    c.arg("-c").arg(WATCH).arg("sh");
    c.arg(std::process::id().to_string()).arg(bundle).arg(old_of(bundle)).arg(failed).arg(program).arg("/usr/bin/open");
    c.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    crate::sysenv::clean_std(&mut c);
    c.spawn().map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_of_finds_the_app_around_its_program() {
        let exe = Path::new("/Applications/RecompLauncher.app/Contents/MacOS/recomplauncher");
        assert_eq!(bundle_of(exe), Some(PathBuf::from("/Applications/RecompLauncher.app")));
    }

    #[test]
    fn bundle_of_is_none_outside_a_bundle() {
        assert_eq!(bundle_of(Path::new("/Users/me/RecompLauncher/target/debug/recomplauncher")), None);
    }

    #[test]
    fn can_replace_is_true_for_a_folder_this_account_writes() {
        let dir = std::env::temp_dir().join(format!("rl-can-replace-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(can_replace(&dir.join("RecompLauncher.app")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn can_replace_is_false_when_the_folder_is_missing() {
        assert!(!can_replace(Path::new("/no/such/folder/RecompLauncher.app")));
    }

    /// Runs mac_update.sh with a stand-in new version that exits with `code` straight away, next
    /// to an "app" and an "old" folder each holding a file saying which version it is. Returns the
    /// folder they're in.
    #[cfg(unix)]
    fn watch_with_exit(code: i32, name: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("rl-mac-watch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (app, old) = (dir.join("RecompLauncher.app"), dir.join(".RecompLauncher-old.app"));
        std::fs::create_dir_all(&app).unwrap();
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(app.join("which"), "new").unwrap();
        std::fs::write(old.join("which"), "old").unwrap();
        let program = dir.join("program");
        std::fs::write(&program, format!("#!/bin/sh\nexit {code}\n")).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        // A process that has already quit stands in for the version being replaced.
        let mut gone = std::process::Command::new("true").spawn().unwrap();
        gone.wait().unwrap();
        let status = std::process::Command::new("/bin/sh").arg("-c").arg(WATCH).arg("sh").arg(gone.id().to_string()).arg(&app).arg(&old).arg(dir.join(".RecompLauncher-failed.app")).arg(&program).arg("true").status().unwrap();
        assert!(status.success());
        dir
    }

    #[cfg(unix)]
    #[test]
    fn a_new_version_that_crashes_is_replaced_by_the_old_one() {
        let dir = watch_with_exit(3, "crash");
        assert_eq!(std::fs::read_to_string(dir.join("RecompLauncher.app/which")).unwrap(), "old");
        assert!(!dir.join(".RecompLauncher-old.app").exists());
        assert!(!dir.join(".RecompLauncher-failed.app").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_new_version_the_player_closes_soon_is_kept() {
        let dir = watch_with_exit(0, "closed");
        assert_eq!(std::fs::read_to_string(dir.join("RecompLauncher.app/which")).unwrap(), "new");
        assert!(!dir.join(".RecompLauncher-old.app").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

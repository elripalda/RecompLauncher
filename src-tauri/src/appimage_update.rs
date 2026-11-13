//! The launcher's Linux AppImage update. Stage beside the original so replacement is atomic;
//! a system-shell helper waits for this process (and its single-instance lock) to go away.

use crate::{Error, Res};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;
use tauri::{AppHandle, Emitter};

const HELPER: &str = include_str!("appimage_update.sh");
const LIMIT: u64 = 512 * 1024 * 1024;
const STAGING: &str = ".recomplauncher-update-";
static UPDATING: AtomicBool = AtomicBool::new(false);

/// The Linux build on offer, once its metadata checks out.
struct Offer {
    url: String,
    sha256: String,
    version: String,
}

fn metadata(entry: &Value, base: &str) -> Res<Offer> {
    let raw = crate::str_of(entry, "url");
    let is_appimage = reqwest::Url::parse(raw).is_ok_and(|url| url.path().to_ascii_lowercase().ends_with(".appimage"));
    if !is_appimage || !crate::trusted_update_url(raw, base) {
        return Err(Error::Msg(
            "This Linux update has no trusted HTTPS AppImage download. Try again later.".into(),
        ));
    }
    let version = crate::str_of(entry, "version");
    if version.is_empty()
        || crate::version_key(version) <= crate::version_key(env!("CARGO_PKG_VERSION"))
    {
        return Err(Error::Msg("RecompLauncher is already up to date.".into()));
    }
    let checksum = crate::str_of(entry, "sha256").to_ascii_lowercase();
    if checksum.len() != 64 || !checksum.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(Error::Msg(
            "This Linux update hasn't published a valid checksum yet. Try again later.".into(),
        ));
    }
    Ok(Offer {
        url: raw.into(),
        sha256: checksum,
        version: version.trim_start_matches('v').into(),
    })
}

/// Whether this account can put the update beside the AppImage and swap it in. A folder it can't
/// change (one only root owns, a read-only drive) can't take an in-place update at all.
fn can_replace(folder: &Path) -> bool {
    let probe = folder.join(format!("{STAGING}probe-{}", std::process::id()));
    if fs::create_dir(&probe).is_err() {
        return false;
    }
    let _ = fs::remove_dir(&probe);
    true
}

/// Staging folders an interrupted update left beside the AppImage (a logout or shutdown while the
/// helper waited). Only ones an hour old: a helper still at work finishes within about a minute.
pub fn clean_stale(image: &Path) {
    let Ok(target) = fs::canonicalize(image) else { return };
    let Some(folder) = target.parent() else { return };
    let Ok(entries) = fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        let ours = entry.file_name().to_string_lossy().starts_with(STAGING);
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        if !ours || !is_dir {
            continue;
        }
        let age = entry.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
        if age.is_some_and(|age| age.as_secs() > 3600) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

async fn download(
    http: &reqwest::Client,
    url: &str,
    checksum: &str,
    limit: u64,
    mut progress: impl FnMut(u64, u64),
) -> Res<Vec<u8>> {
    let mut response = crate::update_download::response(http, url).await?;
    if !response.status().is_success() {
        return Err(Error::Msg(format!(
            "The download answered {}",
            response.status().as_u16()
        )));
    }
    let total = response.content_length().unwrap_or(0);
    let too_large =
        || Error::Msg("The RecompLauncher update is too large. Try again later.".into());
    if total > limit {
        return Err(too_large());
    }
    progress(0, total);
    let mut bytes = Vec::with_capacity(total as usize);
    let mut last = Instant::now();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() as u64 + chunk.len() as u64 > limit {
            return Err(too_large());
        }
        bytes.extend_from_slice(&chunk);
        if last.elapsed().as_millis() > 200 {
            progress(bytes.len() as u64, total);
            last = Instant::now();
        }
    }
    if hex::encode(Sha256::digest(&bytes)) != checksum {
        return Err(Error::Msg(
            "The new version didn't download right (its checksum doesn't match). Try again.".into(),
        ));
    }
    Ok(bytes)
}

extern "C" {
    fn close_range(first: u32, last: u32, flags: i32) -> i32;
}
const CLOSE_RANGE_CLOEXEC: i32 = 4;

/// The AppImage runtime hands every program it starts two descriptors of its own: its mount (1023)
/// and the read end of a pipe it watches, staying up until no program holds that end any more. The
/// helper inherited both and passed them to the new launcher, so the old runtime kept running
/// beside it until it closed. The helper needs nothing past stdin, stdout and stderr, so all of
/// them close at its exec. (Close-on-exec rather than closing now: std's own pipe that reports a
/// failed exec stays usable until then.)
fn forget_old_mount(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    // close_range is async-signal-safe, as pre_exec requires (Linux 5.11 and later). If it ever
    // fails, the update still goes in; only the old runtime stays until the new launcher closes.
    unsafe {
        command.pre_exec(|| {
            close_range(3, u32::MAX, CLOSE_RANGE_CLOEXEC);
            Ok(())
        });
    }
}

struct Staged {
    work: PathBuf,
    target: PathBuf,
    handed_off: bool,
}

impl Drop for Staged {
    fn drop(&mut self) {
        if !self.handed_off {
            let _ = fs::remove_dir_all(&self.work);
        }
    }
}

impl Staged {
    fn prepare(target: &Path, bytes: &[u8]) -> Res<Self> {
        // Follow a user's AppImage symlink once, keeping the symlink and shortcuts intact.
        let target = fs::canonicalize(target)
            .map_err(|e| Error::Msg(format!("Couldn't find the running AppImage: {e}")))?;
        if !target.is_file() {
            return Err(Error::Msg(
                "Run RecompLauncher from its AppImage to update it.".into(),
            ));
        }
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let work = target.parent().unwrap().join(format!(
            "{STAGING}{}-{}-{}",
            std::process::id(),
            crate::now(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&work)
            .map_err(|e| Error::Msg(format!("Couldn't save the update next to the AppImage: {e}. Move RecompLauncher to a folder you can write to, then try again.")))?;
        let staged = Self {
            work,
            target,
            handed_off: false,
        };
        let next = staged.work.join("next.AppImage");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&next)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        crate::appimage::validate_runtime(&next)?;
        fs::set_permissions(&next, fs::Permissions::from_mode(0o755))?;
        // Prefer keeping the old inode; portable drives may not support hard links.
        let previous = staged.work.join("previous.AppImage");
        fs::hard_link(&staged.target, &previous)
            .or_else(|_| fs::copy(&staged.target, &previous).map(|_| ()))?;
        Ok(staged)
    }

    fn command(&self, data: &Path, big_picture: bool, reopen: bool) -> Command {
        let mut command = Command::new("/bin/sh");
        crate::sysenv::clean_std(&mut command);
        // Extraction is needed on systems without FUSE; retain an explicit runtime choice too.
        if std::env::var_os("APPIMAGE_EXTRACT_AND_RUN").is_some() || !crate::sysenv::has_fuse2() {
            command.env("APPIMAGE_EXTRACT_AND_RUN", "1");
        }
        command
            .args(["-c", HELPER, "recomplauncher-appimage-update"])
            .arg(std::process::id().to_string())
            .arg(&self.target)
            .arg(&self.work)
            // The helper outlives the launcher, so it notes what went wrong in the launcher's own
            // trail; the next start finds out from update-pending.json (see update_check_last).
            .arg(data.join("launcher-trail.log"))
            .arg(if reopen { "1" } else { "0" })
            .arg(if big_picture {
                "--bigpicture"
            } else {
                "--desktop"
            })
            .current_dir(self.target.parent().unwrap())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        forget_old_mount(&mut command);
        command
    }

    fn launch(mut self, data: &Path, big_picture: bool, reopen: bool) -> Res<()> {
        self.command(data, big_picture, reopen)
            .spawn()
            .map_err(|e| Error::Msg(format!("Couldn't start the AppImage update helper: {e}")))?;
        self.handed_off = true;
        Ok(())
    }
}

pub async fn update(
    app: &AppHandle,
    state: &crate::App,
    target: &Path,
    entry: &Value,
    big_picture: bool,
) -> Res<()> {
    if UPDATING.swap(true, Ordering::SeqCst) {
        return Err(Error::Msg("RecompLauncher is already updating.".into()));
    }
    struct Guard(bool);
    impl Drop for Guard {
        fn drop(&mut self) {
            if !self.0 {
                UPDATING.store(false, Ordering::SeqCst);
            }
        }
    }
    let mut guard = Guard(false);
    let offer = metadata(entry, &crate::catalog::base())?;
    // Checked before the download: a folder this account can't change can never take the update,
    // so the window says so and offers the download instead (see cantReplace in app.js).
    let image = fs::canonicalize(target)
        .map_err(|e| Error::Msg(format!("Couldn't find the running AppImage: {e}")))?;
    let folder = image.parent().unwrap_or(Path::new("/"));
    if !can_replace(folder) {
        return Err(Error::Msg(format!("CANT_REPLACE|{}", folder.display())));
    }
    let bytes = download(&state.http, &offer.url, &offer.sha256, LIMIT, |done, total| {
        let _ = app.emit("self-update", json!({"done": done, "total": total}));
    })
    .await?;
    let staged = tauri::async_runtime::spawn_blocking(move || Staged::prepare(&image, &bytes))
        .await
        .map_err(|e| Error::Msg(format!("Couldn't prepare the AppImage update: {e}")))??;
    let _ = app.emit("self-update", json!({"installing": true}));
    // Keep main's close-during-update behavior: quitting cancels the handoff, and a window
    // closed to the tray stays closed after replacement.
    if crate::update_launch::quitting() {
        return Ok(());
    }
    // Noted as on Windows and Mac: if this version is the one that starts next (the swap failed,
    // or the new one didn't start and this one was put back), that start tells the player, and
    // the second failure from the same file stops it being offered.
    let pending = state.dir.join("update-pending.json");
    let note = json!({"to": offer.version, "from": env!("CARGO_PKG_VERSION"), "at": crate::now(), "installer": "", "url": offer.url, "sha256": offer.sha256});
    let _ = fs::write(&pending, note.to_string());
    if let Err(e) = staged.launch(&state.dir, big_picture, crate::reopen_after_update(app)) {
        let _ = fs::remove_file(&pending);
        return Err(e);
    }
    guard.0 = true;
    app.exit(0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::appimage::tests::TempDir;

    fn image() -> Vec<u8> {
        let mut bytes = crate::appimage::tests::image(None, backhand::compression::Compressor::Xz);
        let machine: u16 = if cfg!(target_arch = "x86_64") {
            62
        } else {
            183
        };
        bytes[18..20].copy_from_slice(&machine.to_le_bytes());
        bytes
    }

    #[test]
    fn metadata_requires_linux_artifact_new_version_and_own_checksum() {
        let good = json!({"version":"999.0.0", "url":"https://github.com/owner/repo/releases/download/v999/Launcher.AppImage", "sha256":"A".repeat(64)});
        let offer = metadata(&good, "https://updates.example/api").unwrap();
        assert_eq!(offer.sha256, "a".repeat(64));
        assert_eq!(offer.version, "999.0.0");
        for url in [
            "http://github.com/file.AppImage",
            "https://github.com.evil/file.AppImage",
            "https://name@github.com/file.AppImage",
            "https://github.com/file.exe",
            "https://github.com/file.AppImage#bad",
            "https://updates.example/api-evil/file.AppImage",
        ] {
            let mut entry = good.clone();
            entry["url"] = json!(url);
            assert!(
                metadata(&entry, "https://updates.example/api").is_err(),
                "{url}"
            );
        }
        for checksum in ["", "abc", &"g".repeat(64)] {
            let mut entry = good.clone();
            entry["sha256"] = json!(checksum);
            assert!(metadata(&entry, "https://updates.example/api").is_err());
        }
        let mut entry = good;
        entry["version"] = json!(env!("CARGO_PKG_VERSION"));
        assert!(metadata(&entry, "https://updates.example/api").is_err());
    }

    #[test]
    fn staging_keeps_original_and_symlink_and_cleans_private_files() {
        let dir = TempDir::new();
        let target = dir.0.join("Renamed α; launcher.AppImage");
        let link = dir.0.join("shortcut.AppImage");
        fs::write(&target, b"old image").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let work;
        {
            let staged = Staged::prepare(&link, &image()).unwrap();
            work = staged.work.clone();
            assert_eq!(staged.target, target);
            assert_eq!(fs::read(&target).unwrap(), b"old image");
            assert_eq!(
                fs::read(work.join("previous.AppImage")).unwrap(),
                b"old image"
            );
            assert_eq!(
                fs::metadata(&work).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(work.join("next.AppImage"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o755
            );
            for (big, flag) in [(true, "--bigpicture"), (false, "--desktop")] {
                let command = staged.command(&dir.0, big, true);
                assert_eq!(command.get_program(), "/bin/sh");
                assert_eq!(command.get_args().last().unwrap(), flag);
            }
        }
        assert!(!work.exists());
        assert!(link.is_symlink());
        assert_eq!(fs::read(&link).unwrap(), b"old image");
    }

    #[test]
    fn helper_does_not_inherit_the_runtimes_descriptors() {
        use std::os::fd::AsRawFd;
        extern "C" {
            fn fcntl(fd: i32, cmd: i32, ...) -> i32;
        }
        const F_SETFD: i32 = 2;
        // Left inheritable, as the AppImage runtime leaves its mount and keepalive pipe.
        let file = fs::File::open("/dev/null").unwrap();
        let fd = file.as_raw_fd();
        assert_eq!(unsafe { fcntl(fd, F_SETFD, 0) }, 0);
        let mut command = Command::new("/bin/sh");
        command.args(["-c", &format!("[ -e /proc/self/fd/{fd} ] && echo open || echo closed")]);
        forget_old_mount(&mut command);
        let out = command.output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "closed");
    }

    #[test]
    fn can_replace_in_a_folder_this_account_writes() {
        let dir = TempDir::new();
        assert!(can_replace(&dir.0));
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 0);
    }

    #[test]
    fn can_replace_is_false_for_a_folder_that_is_not_there() {
        let dir = TempDir::new();
        assert!(!can_replace(&dir.0.join("missing")));
    }

    #[test]
    fn clean_stale_removes_only_old_staging_folders() {
        let dir = TempDir::new();
        let image = dir.0.join("Launcher.AppImage");
        fs::write(&image, b"image").unwrap();
        let old = dir.0.join(format!("{STAGING}1-1-0"));
        let fresh = dir.0.join(format!("{STAGING}2-2-0"));
        let other = dir.0.join(".other-folder");
        for folder in [&old, &fresh, &other] {
            fs::create_dir(folder).unwrap();
        }
        let two_hours_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
        for folder in [&old, &other] {
            fs::File::open(folder).unwrap().set_modified(two_hours_ago).unwrap();
        }
        clean_stale(&image);
        assert!(!old.exists());
        assert!(fresh.exists());
        assert!(other.exists());
        assert!(image.exists());
    }

    #[test]
    fn staging_rejects_wrong_files_and_architecture_without_touching_original() {
        let dir = TempDir::new();
        let target = dir.0.join("Launcher.AppImage");
        fs::write(&target, b"old image").unwrap();
        let mut other_arch = image();
        other_arch[18..20].copy_from_slice(&0u16.to_le_bytes());
        for bytes in [b"not an AppImage".to_vec(), other_arch] {
            assert!(Staged::prepare(&target, &bytes).is_err());
            assert_eq!(fs::read(&target).unwrap(), b"old image");
            assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
        }
    }

    fn site(headers: &str, body: &'static [u8]) -> (String, std::thread::JoinHandle<()>) {
        use std::io::Read;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let headers = headers.to_string();
        let thread = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut request = [0; 4096];
            stream.read(&mut request).unwrap();
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nConnection: close\r\n{headers}\r\n"
            );
            let _ = stream.write_all(body);
        });
        (url, thread)
    }

    #[tokio::test]
    async fn download_checks_digest_and_bounds_declared_and_streamed_sizes() {
        let http = reqwest::Client::new();
        let checksum = hex::encode(Sha256::digest(b"new image"));
        for (headers, body, sha, limit, ok) in [
            (
                "Content-Length: 9\r\n",
                b"new image".as_slice(),
                checksum.as_str(),
                10,
                true,
            ),
            (
                "Content-Length: 9\r\n",
                b"new image".as_slice(),
                "wrong",
                10,
                false,
            ),
            ("Content-Length: 1000\r\n", b"".as_slice(), "", 10, false),
            (
                "Transfer-Encoding: chunked\r\n",
                b"b\r\n12345678901\r\n0\r\n\r\n".as_slice(),
                "",
                10,
                false,
            ),
        ] {
            let (url, thread) = site(headers, body);
            assert_eq!(
                download(&http, &url, sha, limit, |_, _| {}).await.is_ok(),
                ok
            );
            thread.join().unwrap();
        }
    }
}

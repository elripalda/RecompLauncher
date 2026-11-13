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
static UPDATING: AtomicBool = AtomicBool::new(false);

fn metadata(entry: &Value, base: &str) -> Res<(String, String)> {
    let raw = crate::str_of(entry, "url");
    let url = reqwest::Url::parse(raw).ok();
    let allowed = url.as_ref().is_some_and(|url| {
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none()
            && url.path().to_ascii_lowercase().ends_with(".appimage")
            && ["https://github.com", crate::net::SITE, base]
                .iter()
                .any(|origin| {
                    reqwest::Url::parse(origin).ok().is_some_and(|origin| {
                        url.origin() == origin.origin()
                            && url
                                .path()
                                .starts_with(&format!("{}/", origin.path().trim_end_matches('/')))
                    })
                })
    });
    if !allowed {
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
    Ok((raw.into(), checksum))
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
            ".recomplauncher-update-{}-{}-{}",
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
            .arg(data.join("appimage-update-error.txt"))
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
    let (url, checksum) = metadata(entry, &crate::catalog::base())?;
    let bytes = download(&state.http, &url, &checksum, LIMIT, |done, total| {
        let _ = app.emit("self-update", json!({"done": done, "total": total}));
    })
    .await?;
    let target = target.to_path_buf();
    let staged = tauri::async_runtime::spawn_blocking(move || Staged::prepare(&target, &bytes))
        .await
        .map_err(|e| Error::Msg(format!("Couldn't prepare the AppImage update: {e}")))??;
    let _ = app.emit("self-update", json!({"installing": true}));
    // Keep main's close-during-update behavior: quitting cancels the handoff, and a window
    // closed to the tray stays closed after replacement.
    if crate::update_launch::quitting() {
        return Ok(());
    }
    staged.launch(&state.dir, big_picture, crate::reopen_after_update(app))?;
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
        assert_eq!(
            metadata(&good, "https://updates.example/api").unwrap().1,
            "a".repeat(64)
        );
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

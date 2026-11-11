//! macOS launcher updates. Download and inspect in the app data folder; launch only the system
//! shell, with commands embedded in this binary, after Tauri's event loop has shut down.

use crate::{App, Error, Res};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

const MAX_DOWNLOAD: u64 = 512 * 1024 * 1024;
const HELPER: &str = include_str!("self_update.sh");
static UPDATING: AtomicBool = AtomicBool::new(false);
static PENDING: Mutex<Option<Prepared>> = Mutex::new(None);

struct Work(PathBuf);
impl Drop for Work {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
impl Work {
    fn create(parent: &Path) -> Res<Self> {
        let parent = fs::canonicalize(parent)?;
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(".recomplauncher-update-{}-{stamp}-{serial}", std::process::id()));
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(&path).map_err(|e| Error::Msg(format!("Couldn't stage the RecompLauncher update: {e}. Check that its folder is writable.")))?;
        Ok(Self(path))
    }
}

fn bundle_for(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    (macos.file_name()? == "MacOS" && contents.file_name()? == "Contents" && crate::macos::is_bundle(bundle)).then(|| bundle.to_path_buf())
}

fn allowed_url(raw: &str, base: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(raw) else { return false };
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return false;
    }
    ["https://github.com", crate::net::SITE, base].iter().any(|origin| {
        let Ok(origin) = reqwest::Url::parse(origin) else { return false };
        url.origin() == origin.origin() && url.path().starts_with(&format!("{}/", origin.path().trim_end_matches('/')))
    })
}

fn metadata(entry: &Value, base: &str) -> Res<(String, String, String)> {
    let url = crate::str_of(entry, "url").to_string();
    if !allowed_url(&url, base) {
        return Err(Error::Msg("The Mac update has no trusted HTTPS download URL. Try again later.".into()));
    }
    let version = crate::str_of(entry, "version").trim_start_matches('v').to_string();
    if version.is_empty() || crate::version_key(&version) <= crate::version_key(env!("CARGO_PKG_VERSION")) {
        return Err(Error::Msg("RecompLauncher is already up to date.".into()));
    }
    let checksum = crate::str_of(entry, "sha256").to_lowercase();
    if checksum.len() != 64 || !checksum.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(Error::Msg("This Mac update hasn't published a valid checksum yet. Try again later.".into()));
    }
    Ok((url, version, checksum))
}

async fn download(http: &reqwest::Client, url: &str, checksum: &str, limit: u64, mut progress: impl FnMut(u64, u64)) -> Res<Vec<u8>> {
    let mut response = crate::update_download::response(http, url).await?;
    if !response.status().is_success() {
        return Err(Error::Msg(format!("The download answered {}", response.status().as_u16())));
    }
    let total = response.content_length().unwrap_or(0);
    if total > limit {
        return Err(Error::Msg("The RecompLauncher update is too large. Try again later.".into()));
    }
    progress(0, total);
    let mut bytes = Vec::with_capacity(total as usize);
    let mut last = Instant::now();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() as u64 + chunk.len() as u64 > limit {
            return Err(Error::Msg("The RecompLauncher update is too large. Try again later.".into()));
        }
        bytes.extend_from_slice(&chunk);
        if last.elapsed().as_millis() > 200 {
            progress(bytes.len() as u64, total);
            last = Instant::now();
        }
    }
    if hex::encode(Sha256::digest(&bytes)) != checksum {
        return Err(Error::Msg("The new version didn't download right (its checksum doesn't match). Try again.".into()));
    }
    Ok(bytes)
}

fn verify_bundle(bundle: &Path, version: &str) -> Res<()> {
    if crate::macos::bundle_executable(bundle, crate::assets::arch_now()).is_none() {
        return Err(Error::Msg("The update doesn't contain a RecompLauncher app for this Mac.".into()));
    }
    let info = plist::Value::from_file(bundle.join("Contents/Info.plist")).map_err(|e| Error::Msg(format!("Couldn't read the update's app information: {e}")))?;
    let field = |key| info.as_dictionary().and_then(|d| d.get(key)).and_then(plist::Value::as_string).unwrap_or("");
    if field("CFBundleIdentifier") != "com.recomplauncher.app" || field("CFBundleShortVersionString") != version.trim_start_matches('v') {
        return Err(Error::Msg("The downloaded app isn't the advertised RecompLauncher version. The current app hasn't changed.".into()));
    }
    let signature = Command::new("/usr/bin/codesign").args(["--verify", "--deep", "--strict"]).arg(bundle).output()?;
    if !signature.status.success() {
        return Err(Error::Msg(format!("The downloaded RecompLauncher app has an invalid signature. Try again. {}", String::from_utf8_lossy(&signature.stderr).trim())));
    }
    Ok(())
}

fn stage_mac(bytes: &[u8], url: &str, version: &str, work: &Path) -> Res<PathBuf> {
    let url = reqwest::Url::parse(url).map_err(|_| Error::Msg("The Mac update has an invalid download URL.".into()))?;
    let ext = Path::new(url.path()).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if !matches!(ext.as_str(), "zip" | "dmg") {
        return Err(Error::Msg("The Mac update must contain a ZIP or DMG application bundle.".into()));
    }
    let archive = work.join(format!("download.{ext}"));
    fs::write(&archive, bytes)?;
    let unpacked = work.join("unpacked");
    crate::install::unpack(&archive, &unpacked, &work.join("extract"))?;
    // Restore AppleDouble metadata from ZIPs without leaving ._ files inside the signed bundle.
    if ext == "zip" && !Command::new("/usr/sbin/dot_clean").arg("-m").arg(&unpacked).status()?.success() {
        return Err(Error::Msg("Couldn't restore the Mac update's application metadata. Try again.".into()));
    }
    let bundle = unpacked.join("RecompLauncher.app");
    verify_bundle(&bundle, version)?;
    Ok(bundle)
}

struct Prepared {
    target: PathBuf,
    next: PathBuf,
    data: Work,
    swap: Work,
    error: PathBuf,
    big_picture: bool,
}
impl Prepared {
    fn stage(bytes: &[u8], url: &str, version: &str, target: PathBuf, data_dir: &Path, big_picture: bool) -> Res<Self> {
        let data = Work::create(data_dir)?;
        let bundle = stage_mac(bytes, url, version, &data.0)?;
        // Only the final signed bundle goes beside the app, so the two renames are on the same
        // filesystem even when Application Support and the installed app are on different drives.
        let swap = Work::create(target.parent().ok_or_else(|| Error::Msg("Couldn't find RecompLauncher's folder.".into()))?)?;
        let next = swap.0.join("RecompLauncher.app");
        let copied = Command::new("/usr/bin/ditto").arg(bundle).arg(&next).output()?;
        if !copied.status.success() {
            return Err(Error::Msg(format!("Couldn't stage the new Mac app: {}", String::from_utf8_lossy(&copied.stderr).trim())));
        }
        verify_bundle(&next, version)?;
        Ok(Self { target, next, data, swap, error: data_dir.join("self-update-error.txt"), big_picture })
    }

    fn command(&self) -> Command {
        // No executable or helper script is written to a writable directory. The shell program is
        // the OS's /bin/sh; its fixed commands come from this installed binary, never from a URL.
        let mut command = Command::new("/bin/sh");
        command.args(["-c", HELPER, "recomplauncher-mac-update"])
            .arg(std::process::id().to_string()).arg(&self.target).arg(&self.next)
            .arg(&self.swap.0).arg(&self.data.0).arg(&self.error);
        if self.big_picture {
            command.arg("--bigpicture");
        }
        command.current_dir(self.target.parent().unwrap()).stdin(Stdio::null());
        command
    }

    fn launch(self) -> Res<()> {
        let _ = fs::remove_file(&self.error);
        let log = fs::File::create(self.error.with_file_name("self-update.log"))?;
        self.command().stdout(log.try_clone()?).stderr(log).spawn()?;
        // The system shell now owns cleanup. It waits for this process to exit before touching
        // the installed app, then verifies the staged signature once more before either rename.
        std::mem::forget(self.data);
        std::mem::forget(self.swap);
        Ok(())
    }
}

pub async fn update(app: &AppHandle, state: &App, entry: &Value, big_picture: bool) -> Res<()> {
    if UPDATING.swap(true, Ordering::SeqCst) {
        return Err(Error::Msg("RecompLauncher is already updating.".into()));
    }
    struct Updating(bool);
    impl Drop for Updating {
        fn drop(&mut self) {
            if self.0 { UPDATING.store(false, Ordering::SeqCst); }
        }
    }
    let mut updating = Updating(true);
    let (url, version, checksum) = metadata(entry, &crate::catalog::base())?;
    let exe = fs::canonicalize(std::env::current_exe()?)?;
    let target = bundle_for(&exe).ok_or_else(|| Error::Msg("Run RecompLauncher from its installed .app to update it.".into()))?;
    let bytes = download(&state.http, &url, &checksum, MAX_DOWNLOAD, |done, total| {
        let _ = app.emit("self-update", json!({"done": done, "total": total}));
    }).await?;
    let _ = app.emit("self-update", json!({"done": bytes.len(), "total": bytes.len(), "installing": true}));
    let data = state.dir.clone();
    let prepared = tauri::async_runtime::spawn_blocking(move || Prepared::stage(&bytes, &url, &version, target, &data, big_picture))
        .await.map_err(|e| Error::Msg(format!("Couldn't prepare the Mac update: {e}")))??;
    *PENDING.lock().unwrap() = Some(prepared);
    updating.0 = false;
    app.exit(0);
    Ok(())
}

/// Called only after run_return has shut down Tauri and its single-instance plugin. The helper
/// still waits for the original PID to disappear before swapping or opening either app bundle.
pub fn after_exit() {
    let Some(prepared) = PENDING.lock().unwrap().take() else { return };
    let error = prepared.error.clone();
    if let Err(e) = prepared.launch() {
        let message = format!("Couldn't finish the Mac update: {e}. The installed app hasn't changed. Open RecompLauncher again.");
        crate::trail::note(&message);
        let _ = fs::write(error, message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A real signed bundle exercises the same ZIP extraction, permission/link preservation and
    // signature verification as a published update, without replacing or launching this app.
    #[cfg(target_os = "macos")]
    fn fixture(work: &Path, id: &str, version: &str, corrupt: bool) -> Vec<u8> {
        let app = work.join("source/RecompLauncher.app");
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        fs::create_dir_all(app.join("Contents/Resources")).unwrap();
        fs::copy("/usr/bin/true", app.join("Contents/MacOS/recomplauncher")).unwrap();
        let mut info = plist::Dictionary::new();
        for (key, value) in [
            ("CFBundleIdentifier", id), ("CFBundleShortVersionString", version),
            ("CFBundleVersion", version), ("CFBundleExecutable", "recomplauncher"),
            ("CFBundlePackageType", "APPL"),
        ] {
            info.insert(key.into(), value.into());
        }
        plist::Value::Dictionary(info).to_file_xml(app.join("Contents/Info.plist")).unwrap();
        let resource = app.join("Contents/Resources/marker");
        fs::write(&resource, "signed resource").unwrap();
        std::os::unix::fs::symlink("marker", app.join("Contents/Resources/link")).unwrap();
        assert!(Command::new("/usr/bin/codesign").args(["--force", "--sign", "-"]).arg(&app).status().unwrap().success());
        assert!(Command::new("/usr/bin/codesign").args(["--verify", "--deep", "--strict"]).arg(&app).status().unwrap().success());
        if corrupt {
            fs::write(resource, "changed after signing").unwrap();
        }
        let archive = work.join("fixture.zip");
        assert!(Command::new("/usr/bin/ditto").args(["-c", "-k", "--keepParent"]).arg(app).arg(&archive).status().unwrap().success());
        fs::read(archive).unwrap()
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn mac_update_stages_a_complete_signed_bundle() {
        let work = Work::create(&std::env::temp_dir()).unwrap();
        let bytes = fixture(&work.0, "com.recomplauncher.app", "1.2.3-beta.4", false);
        let staged = stage_mac(&bytes, "https://github.com/owner/repo/update.zip", "v1.2.3-beta.4", &work.0).unwrap();
        assert!(crate::macos::bundle_executable(&staged, crate::assets::arch_now()).is_some());
        assert_eq!(fs::read_link(staged.join("Contents/Resources/link")).unwrap(), PathBuf::from("marker"));
        assert_eq!(bundle_for(&staged.join("Contents/MacOS/recomplauncher")), Some(staged));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn mac_update_refuses_the_wrong_app_version_or_broken_signature() {
        for (id, version, corrupt, message) in [
            ("another.app", "1.2.3", false, "isn't the advertised"),
            ("com.recomplauncher.app", "1.2.2", false, "isn't the advertised"),
            ("com.recomplauncher.app", "1.2.3", true, "invalid signature"),
        ] {
            let work = Work::create(&std::env::temp_dir()).unwrap();
            let bytes = fixture(&work.0, id, version, corrupt);
            let error = stage_mac(&bytes, "https://github.com/owner/repo/update.zip", "1.2.3", &work.0).unwrap_err();
            assert!(error.to_string().contains(message), "{error}");
        }
    }


    #[test]
    fn metadata_requires_a_new_version_trusted_https_url_and_sha256() {
        let base = "https://updates.example.test/api";
        let good = json!({"version":"999.1.0", "url":"https://github.com/owner/repo/releases/download/v999.1.0/app.zip", "sha256":"a".repeat(64)});
        assert!(metadata(&good, base).is_ok());
        for url in ["https://recomplauncher.com/launcher/app.dmg", "https://updates.example.test/api/app.zip"] {
            let mut entry = good.clone();
            entry["url"] = json!(url);
            assert!(metadata(&entry, base).is_ok(), "{url}");
        }
        for url in [
            "", "file:///tmp/app.zip", "http://github.com/owner/repo/app.zip",
            "https://github.com.evil.test/file", "https://github.com@evil.test/file",
            "https://name@github.com/file", "https://github.com:444/file",
            "https://updates.example.test/api-evil/file", "https://github.com/file#fragment",
        ] {
            let mut entry = good.clone();
            entry["url"] = json!(url);
            assert!(metadata(&entry, base).is_err(), "{url}");
        }
        for checksum in ["", "abc", &"x".repeat(64)] {
            let mut entry = good.clone();
            entry["sha256"] = json!(checksum);
            assert!(metadata(&entry, base).is_err());
        }
        let mut entry = good.clone();
        entry.as_object_mut().unwrap().remove("sha256");
        assert!(metadata(&entry, base).is_err());
        for version in ["", "0.1.0", env!("CARGO_PKG_VERSION")] {
            let mut entry = good.clone();
            entry["version"] = json!(version);
            assert!(metadata(&entry, base).is_err());
        }
    }

    #[test]
    fn staging_uses_app_data_and_never_writes_an_executable_helper() {
        let root = Work::create(&std::env::temp_dir()).unwrap();
        let bytes = fixture(&root.0, "com.recomplauncher.app", "999.1.0", false);
        let data = root.0.join("Application Support");
        let apps = root.0.join("Applications");
        fs::create_dir(&data).unwrap();
        fs::create_dir(&apps).unwrap();
        let target = apps.join("RecompLauncher.app");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("old resource"), "keep until exit").unwrap();
        let prepared = Prepared::stage(&bytes, "https://github.com/owner/repo/update.zip", "999.1.0", target.clone(), &data, true).unwrap();
        assert!(prepared.data.0.starts_with(fs::canonicalize(&data).unwrap()));
        assert_eq!(prepared.next.parent().unwrap(), prepared.swap.0);
        assert_eq!(fs::read_dir(&prepared.swap.0).unwrap().count(), 1, "only the signed replacement bundle is staged beside the app");
        assert_eq!(fs::read_to_string(target.join("old resource")).unwrap(), "keep until exit");
        let command = prepared.command();
        assert_eq!(command.get_program(), "/bin/sh");
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args[0], "-c");
        assert_eq!(args[1], HELPER);
        assert_eq!(args.last().unwrap(), &std::ffi::OsStr::new("--bigpicture"));
        let swap = prepared.swap.0.clone();
        drop(prepared);
        assert!(!swap.exists(), "cancelled staging removes the adjacent bundle");
        assert_eq!(fs::read_dir(data).unwrap().count(), 0, "cancelled staging removes app-data downloads");
        assert!(target.join("old resource").exists());
    }

    #[test]
    fn failed_staging_cleans_app_data_and_keeps_the_installed_app() {
        let root = Work::create(&std::env::temp_dir()).unwrap();
        let bytes = fixture(&root.0, "com.recomplauncher.app", "999.1.0", true);
        let data = root.0.join("Application Support");
        let apps = root.0.join("Applications");
        fs::create_dir(&data).unwrap();
        fs::create_dir(&apps).unwrap();
        let target = apps.join("RecompLauncher.app");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("old resource"), "unchanged").unwrap();
        assert!(Prepared::stage(&bytes, "https://github.com/owner/repo/update.zip", "999.1.0", target.clone(), &data, false).is_err());
        assert_eq!(fs::read_dir(data).unwrap().count(), 0);
        assert_eq!(fs::read_dir(apps).unwrap().count(), 1);
        assert_eq!(fs::read_to_string(target.join("old resource")).unwrap(), "unchanged");
    }

    fn response_site(headers: &str, body: &'static [u8]) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        use std::time::Duration;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let headers = headers.to_string();
        let server = std::thread::spawn(move || {
            let until = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < until => std::thread::sleep(Duration::from_millis(5)),
                    Err(e) => panic!("no download request: {e}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            write!(stream, "HTTP/1.1 200 OK\r\n{headers}\r\nConnection: close\r\n\r\n").unwrap();
            let _ = stream.write_all(body);
        });
        (url, server)
    }

    #[tokio::test]
    async fn download_requires_the_exact_digest() {
        let checksum = hex::encode(Sha256::digest(b"abc"));
        let (url, server) = response_site("Content-Length: 3", b"abc");
        assert_eq!(download(&reqwest::Client::new(), &url, &checksum, 16, |_, _| {}).await.unwrap(), b"abc");
        server.join().unwrap();
        let (url, server) = response_site("Content-Length: 3", b"abc");
        assert!(download(&reqwest::Client::new(), &url, &"0".repeat(64), 16, |_, _| {}).await.unwrap_err().to_string().contains("checksum"));
        server.join().unwrap();
    }

    #[tokio::test]
    async fn download_caps_both_content_length_and_chunked_bodies() {
        let (url, server) = response_site("Content-Length: 536870913", b"abc");
        assert!(download(&reqwest::Client::new(), &url, &"a".repeat(64), MAX_DOWNLOAD, |_, _| {}).await.unwrap_err().to_string().contains("too large"));
        server.join().unwrap();
        let (url, server) = response_site("Transfer-Encoding: chunked", b"3\r\nabc\r\n3\r\ndef\r\n0\r\n\r\n");
        assert!(download(&reqwest::Client::new(), &url, &"a".repeat(64), 4, |_, _| {}).await.unwrap_err().to_string().contains("too large"));
        server.join().unwrap();
    }
}

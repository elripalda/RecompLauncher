//! Verified immutable downloads shared by engine adapters. Catalog data never supplies commands.
use crate::{assets::Asset, install, Error};
use serde::Deserialize;
use sha2::{Digest, Sha256, Sha512};
use std::{fs, io::Read, path::{Path, PathBuf}, sync::{Arc, atomic::{AtomicBool, Ordering}}};

#[derive(Clone, Deserialize)]
pub struct Artifact {
    pub name: String,
    pub url: String,
    pub digest: String,
    pub size: u64,
    #[serde(default)] pub root: String,
    #[serde(default)] pub registry: String,
}

pub fn verify(file: &Path, digest: &str) -> Result<(), Error> {
    let mut f = fs::File::open(file)?;
    let mut b = [0u8; 64 * 1024];
    let mut sha = Sha256::new();
    let mut sha512 = Sha512::new();
    loop { let n = f.read(&mut b)?; if n == 0 { break; } sha.update(&b[..n]); sha512.update(&b[..n]); }
    let actual = if digest.len() == 128 { hex::encode(sha512.finalize()) } else { hex::encode(sha.finalize()) };
    if actual != digest { return Err(Error::Msg("The downloaded components failed verification. Retry the download.".into())); }
    Ok(())
}

/// Publish only a fully verified extraction. The caller owns the package lock.
pub fn install_archive(archive: &Path, destination: &Path, digest: &str) -> Result<(), Error> {
    verify(archive, digest)?;
    if destination.exists() { return Err(Error::Msg("That component package already exists.".into())); }
    let stage = destination.with_extension("extracting");
    let unpack = destination.with_extension("unpacking");
    let _ = fs::remove_dir_all(&stage);
    let result = (|| {
        install::unpack_as_is(archive, &stage, &unpack)?;
        fs::write(stage.join(".verified-package"), digest)?;
        fs::rename(&stage, destination)?;
        Ok(())
    })();
    let _ = fs::remove_dir_all(&stage);
    let _ = fs::remove_dir_all(&unpack);
    result
}

pub async fn lock(path: &Path, cancel: &AtomicBool) -> Result<fs::File, Error> {
    let file = fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(path)?;
    loop {
        if cancel.load(Ordering::Relaxed) { return Err(Error::Cancelled); }
        match fs2::FileExt::try_lock_exclusive(&file) {
            Ok(()) => return Ok(file),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => tokio::time::sleep(std::time::Duration::from_millis(150)).await,
            Err(e) => return Err(e.into()),
        }
    }
}

pub async fn download(http: &reqwest::Client, data: &Path, artifact: &Artifact, cancel: Arc<AtomicBool>, report: &(dyn Fn(f32, String) + Send + Sync)) -> Result<PathBuf, Error> {
    let downloads = data.join("engines/downloads").join(&artifact.digest);
    fs::create_dir_all(&downloads)?;
    let _lock = lock(&downloads.join("download.lock"), &cancel).await?;
    let file = downloads.join(&artifact.name);
    if file.is_file() && verify(&file, &artifact.digest).is_ok() { return Ok(file); }
    let _ = fs::remove_file(&file);
    if artifact.registry.is_empty() {
        let asset = Asset { name:artifact.name.clone(), size:artifact.size, digest:None, browser_download_url:artifact.url.clone(), url:None };
        install::download(http, &asset, &downloads, &artifact.digest, cancel.clone(), &|p| {
            report(if p.total > 0 { p.done as f32 / p.total as f32 } else { 0.0 }, "Downloading components".into());
        }).await?;
    } else {
        download_registry(http, "https://ghcr.io/token", artifact, &file, &cancel, report).await?;
    }
    if cancel.load(Ordering::Relaxed) { return Err(Error::Cancelled); }
    verify(&file, &artifact.digest)?;
    Ok(file)
}

pub async fn ensure(http: &reqwest::Client, data: &Path, artifact: &Artifact, cancel: Arc<AtomicBool>, report: &(dyn Fn(f32, String) + Send + Sync)) -> Result<PathBuf, Error> {
    let cache = data.join("engines/packages");
    fs::create_dir_all(&cache)?;
    let _lock = lock(&cache.join(format!("{}.lock",artifact.digest)), &cancel).await?;
    let target = cache.join(&artifact.digest);
    let root = target.join(&artifact.root);
    if root.is_dir() && fs::read_to_string(target.join(".verified-package")).ok().as_deref() == Some(&artifact.digest) { return Ok(root); }
    if target.exists() { return Err(Error::Msg("A component package is incomplete. Its folder was kept; remove it before retrying.".into())); }
    let file = download(http, data, artifact, cancel.clone(), report).await?;
    if cancel.load(Ordering::Relaxed) { return Err(Error::Cancelled); }
    report(0.0, "Checking and unpacking components".into());
    install_archive(&file, &target, &artifact.digest)?;
    if !root.is_dir() { return Err(Error::Msg("The component package is missing its recorded folder.".into())); }
    Ok(root)
}

async fn download_registry(http: &reqwest::Client, token_url: &str, artifact: &Artifact, file: &Path, cancel: &AtomicBool, report: &(dyn Fn(f32, String) + Send + Sync)) -> Result<(), Error> {
    // Public registry tokens authorize only pulling this immutable tool package.
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;
    let request = http.get(token_url)
        .query(&[("service", "ghcr.io"), ("scope", &format!("repository:{}:pull", artifact.registry))])
        .send();
    let response = network_wait(request, cancel).await??.error_for_status()?;
    let auth: serde_json::Value = network_wait(response.json(), cancel).await??;
    let token = auth["token"].as_str().ok_or_else(|| Error::Msg("The component registry did not authorize the download.".into()))?;
    let response = network_wait(http.get(&artifact.url).bearer_auth(token).send(), cancel).await??.error_for_status()?;
    let mut stream = response.bytes_stream();
    let mut out = tokio::fs::File::create(&file).await?;
    let mut done = 0u64;
    while let Some(bytes) = network_wait(stream.next(), cancel).await? {
        if cancel.load(Ordering::Relaxed) { return Err(Error::Cancelled); }
        let bytes = bytes?;
        done += bytes.len() as u64;
        if done > artifact.size { return Err(Error::Msg("The component download exceeds its recorded size.".into())); }
        out.write_all(&bytes).await?;
        report(done as f32 / artifact.size.max(1) as f32, "Downloading components".into());
    }
    out.flush().await?;
    Ok(())
}

/// Bound each network wait, including the gaps between body chunks. Stop must release
/// the caller's installation and cache locks even when the server remains connected.
async fn network_wait<T>(future: impl std::future::Future<Output=T>, cancel: &AtomicBool) -> Result<T, Error> {
    let stopped = async {
        loop {
            if cancel.load(Ordering::Relaxed) { break; }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    };
    tokio::select! {
        biased;
        _ = stopped => Err(Error::Cancelled),
        result = tokio::time::timeout(std::time::Duration::from_secs(30), future) =>
            result.map_err(|_| Error::Msg("The component download stopped responding. Retry Setup to continue.".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn registry_stop_interrupts_token_and_blob_headers_and_bodies() {
        for stage in 0..4 {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let (reached, waiting) = tokio::sync::oneshot::channel();
            let server = tokio::spawn(async move {
                let mut reached = Some(reached);
                for request in 0..2 {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut data = vec![];
                    while !data.ends_with(b"\r\n\r\n") {
                        let mut byte = [0]; socket.read_exact(&mut byte).await.unwrap(); data.push(byte[0]);
                    }
                    let stalled = (stage < 2 && request == 0) || request == 1;
                    if stalled {
                        if stage % 2 == 1 { socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n").await.unwrap(); }
                        reached.take().unwrap().send(()).unwrap();
                        std::future::pending::<()>().await;
                    } else {
                        let body = r#"{"token":"public-test-token"}"#;
                        socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
                    }
                }
            });
            let cancel = Arc::new(AtomicBool::new(false));
            let stop = cancel.clone();
            let stopper = tokio::spawn(async move { waiting.await.unwrap(); stop.store(true, Ordering::Relaxed); });
            let artifact = Artifact { name:"fixture".into(), url:format!("http://{address}/blob"), digest:"a".repeat(64), size:100, root:String::new(), registry:"fixture".into() };
            let path = std::env::temp_dir().join(format!("rl-registry-stop-{}-{stage}",std::process::id()));
            let client = reqwest::Client::new();
            let result = tokio::time::timeout(std::time::Duration::from_secs(1), download_registry(&client,&format!("http://{address}/token"),&artifact,&path,&cancel,&|_,_|{})).await;
            server.abort(); stopper.abort(); let _ = fs::remove_file(path);
            assert!(matches!(result, Ok(Err(Error::Cancelled))), "Stop did not interrupt registry stage {stage}: {result:?}");
        }
    }
}

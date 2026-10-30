//! Pictures: covers and banners kept on disk (so they show at once, and offline), and copying a
//! screenshot to the clipboard.
//!
//! The window asks for a picture as rlimg://<the picture's web address>; the first time, it's
//! downloaded and kept in the app's cache folder, after that it comes from there.

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub fn cache_file(dir: &Path, url: &str) -> PathBuf {
    dir.join(hex::encode(Sha256::digest(url.as_bytes()))[..32].to_string())
}

pub fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn mime(bytes: &[u8]) -> &'static str {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => "image/png",
        [0xFF, 0xD8, ..] => "image/jpeg",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "image/webp",
        [b'G', b'I', b'F', ..] => "image/gif",
        _ if bytes.starts_with(b"<svg") || bytes.starts_with(b"<?xml") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

/// The picture, from the cache or the web (then kept).
pub async fn get(http: &reqwest::Client, dir: &Path, url: &str) -> Option<Vec<u8>> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return None;
    }
    let file = cache_file(dir, url);
    if let Ok(b) = tokio::fs::read(&file).await {
        return Some(b);
    }
    let r = http.get(url).timeout(std::time::Duration::from_secs(30)).send().await.ok()?;
    if !r.status().is_success() {
        return None;
    }
    let b = r.bytes().await.ok()?.to_vec();
    if mime(&b).starts_with("image/") {
        let _ = tokio::fs::create_dir_all(dir).await;
        let tmp = file.with_extension("tmp");
        if tokio::fs::write(&tmp, &b).await.is_ok() {
            let _ = tokio::fs::rename(&tmp, &file).await;
        }
    }
    Some(b)
}

/// Fetch any of these not cached yet, six at a time, in the background.
pub async fn prefetch(http: reqwest::Client, dir: PathBuf, urls: Vec<String>) {
    let todo: Vec<String> = urls.into_iter().filter(|u| !cache_file(&dir, u).exists()).collect();
    futures_util::stream::iter(todo)
        .map(|u| {
            let (h, d) = (http.clone(), dir.clone());
            async move {
                let _ = get(&h, &d, &u).await;
            }
        })
        .buffer_unordered(6)
        .collect::<Vec<_>>()
        .await;
}

/// Put a PNG on the clipboard, to paste into Discord, X or anywhere.
pub fn copy_image(path: &Path) -> Result<(), String> {
    let img = image::open(path).map_err(|e| e.to_string())?.to_rgba8();
    let (w, h) = img.dimensions();
    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    cb.set_image(arboard::ImageData { width: w as usize, height: h as usize, bytes: std::borrow::Cow::Owned(img.into_raw()) }).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_addresses() {
        assert_eq!(decode("https%3A%2F%2Frecomplauncher.com%2Fmedia%2Fa%20b.jpg"), "https://recomplauncher.com/media/a b.jpg");
        assert_eq!(decode("100%"), "100%");
    }

    #[test]
    fn kinds() {
        assert_eq!(mime(&[0xFF, 0xD8, 0xFF]), "image/jpeg");
        assert_eq!(mime(b"\x89PNG\r\n"), "image/png");
    }
}

/// Base64, for pictures handed to the window as data addresses.
pub fn base64(b: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    s
}

#[cfg(test)]
mod b64_tests {
    #[test]
    fn base64() {
        assert_eq!(super::base64(b""), "");
        assert_eq!(super::base64(b"f"), "Zg==");
        assert_eq!(super::base64(b"fo"), "Zm8=");
        assert_eq!(super::base64(b"foobar"), "Zm9vYmFy");
    }
}

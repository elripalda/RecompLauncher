//! NVIDIA DLSS as a shared file (the "nvidia.dlss" input, role "runtime"). NVIDIA's
//! nvngx_dlss.dll isn't open source, so some recomps support DLSS but can't ship it. A recomp asks
//! for it in recomplauncher.json and the launcher downloads it from NVIDIA's own GitHub repo, once,
//! for every game that wants it. recomplauncher.com only says which version to use
//! (launcher/dlss.json); it never hosts the file.
//!
//! Only on an NVIDIA RTX card, on Windows or Linux (DLSS works through Proton). Never on macOS. A
//! file a game ships itself is never replaced: swapping versions is DLSS Swapper's job.

use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const SHARED: &str = "nvidia.dlss";
pub const FILE: &str = "nvngx_dlss.dll";
const MANIFEST: &str = "https://recomplauncher.com/launcher/dlss.json";
/// The manifest is asked again after a day; the last good copy is used when offline.
const MANIFEST_FRESH: u64 = 24 * 3600;
/// What the player sees when it couldn't be fetched or checked. The game plays on without it.
pub const FAILED: &str = "Couldn't get NVIDIA's DLSS file. The game runs without it.";

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Entry {
    pub name: String,
    #[serde(default)]
    pub os: String,
    pub url: String,
    pub size: u64,
    #[serde(default)]
    pub signer: String,
    #[serde(default)]
    pub sha256: String,
}

#[derive(Debug, Clone, Deserialize)]
struct Manifest {
    format: u32,
    version: String,
    #[serde(default)]
    files: Vec<Entry>,
}

/// The file to use from a manifest (format 1): its version and NVIDIA's Windows nvngx_dlss.dll.
pub fn parse(bytes: &[u8]) -> Option<(String, Entry)> {
    let m: Manifest = serde_json::from_slice(bytes).ok()?;
    if m.format != 1 || m.version.is_empty() || !m.version.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
        return None;
    }
    let e = m.files.into_iter().find(|f| f.name.eq_ignore_ascii_case(FILE) && (f.os.is_empty() || f.os == "windows"))?;
    (e.size > 0 && url_ok(&e.url)).then_some((m.version, e))
}

/// Only NVIDIA's own GitHub, whatever the manifest says.
pub fn url_ok(url: &str) -> bool {
    (url.starts_with("https://raw.githubusercontent.com/NVIDIA/") || url.starts_with("https://github.com/NVIDIA/")) && !url.contains("..") && !url.contains('@')
}

/// An NVIDIA RTX card (Turing and newer have the tensor cores DLSS needs; their names say RTX,
/// Quadro RTX and the RTX A-series included). GTX 16xx are Turing without them.
pub fn rtx_card(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("nvidia") && n.split(|c: char| !c.is_ascii_alphanumeric()).any(|w| w == "rtx")
}

/// Whether DLSS applies on this PC: Windows or Linux with an NVIDIA RTX card. Asked once.
pub fn applies(dir: &Path) -> bool {
    use std::sync::OnceLock;
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        if cfg!(target_os = "macos") {
            return false;
        }
        #[cfg(windows)]
        let names: Vec<String> = crate::win::gpus().into_iter().map(|(n, _)| n).collect();
        #[cfg(not(windows))]
        let names: Vec<String> = vec![crate::usage::pc_info(dir).gpu];
        let _ = dir;
        names.iter().any(|n| rtx_card(n))
    })
}

fn root(dir: &Path) -> PathBuf {
    dir.join("shared-runtime").join(SHARED)
}

/// The DLSS file games get now, when it's been fetched: the version named in current.txt.
pub fn current(dir: &Path) -> Option<PathBuf> {
    let v = std::fs::read_to_string(root(dir).join("current.txt")).ok()?;
    let v = v.trim();
    if v.is_empty() || v.contains(['/', '\\']) || v.contains("..") {
        return None;
    }
    Some(root(dir).join(v).join(FILE)).filter(|p| p.is_file())
}

/// Settings → Storage: the shared DLSS file goes (every game's own copy goes at its next setup or
/// launch). It's downloaded again when a game next needs it.
pub fn remove(dir: &Path) -> Result<(), String> {
    let r = root(dir);
    if r.exists() {
        std::fs::remove_dir_all(&r).map_err(|e| format!("Couldn't remove the DLSS file: {e}"))?;
    }
    Ok(())
}

/// Is the downloaded file what NVIDIA published: its size, then its signature (NVIDIA Corporation).
pub fn check(bytes: &[u8], e: &Entry, verify: &dyn Fn(&[u8], &str) -> bool) -> Result<(), &'static str> {
    if bytes.len() as u64 != e.size {
        return Err("size");
    }
    if !e.sha256.is_empty() {
        use sha2::{Digest, Sha256};
        if !hex::encode(Sha256::digest(bytes)).eq_ignore_ascii_case(&e.sha256) {
            return Err("checksum");
        }
    }
    let signer = if e.signer.is_empty() { "NVIDIA Corporation" } else { &e.signer };
    if !verify(bytes, signer) {
        return Err("signature");
    }
    Ok(())
}

/// The file's Authenticode signature: valid, and signed by `signer`. On Windows, Windows checks it
/// (Get-AuthenticodeSignature, which is WinVerifyTrust underneath). Elsewhere there's nothing to ask,
/// so the file has to carry a signature naming the signer (it came from NVIDIA's own repo, checked
/// by size).
pub fn verify_signature(bytes: &[u8], signer: &str) -> bool {
    if !has_signature(bytes, signer) {
        return false;
    }
    #[cfg(windows)]
    {
        let tmp = std::env::temp_dir().join(format!("rl-dlss-check-{}.dll", std::process::id()));
        if std::fs::write(&tmp, bytes).is_err() {
            return false;
        }
        let script = format!(
            "$s = Get-AuthenticodeSignature -LiteralPath '{}'; \"$($s.Status)|$($s.SignerCertificate.Subject)\"",
            tmp.to_string_lossy().replace('\'', "''")
        );
        let mut c = std::process::Command::new("powershell");
        c.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000);
        }
        let out = c.output().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
        let _ = std::fs::remove_file(&tmp);
        let (status, subject) = out.trim().split_once('|').unwrap_or(("", ""));
        return status == "Valid" && (subject.contains(&format!("O={signer}")) || subject.contains(&format!("CN={signer}")) || subject.contains(&format!("O=\"{signer}\"")));
    }
    #[allow(unreachable_code)]
    true
}

/// The PE file has an Authenticode certificate table, and the certificates in it name `signer`.
pub fn has_signature(bytes: &[u8], signer: &str) -> bool {
    let u32_at = |o: usize| bytes.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    let u16_at = |o: usize| bytes.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    if bytes.get(..2) != Some(b"MZ") {
        return false;
    }
    let Some(pe) = u32_at(0x3C) else { return false };
    if bytes.get(pe..pe + 4) != Some(b"PE\0\0") {
        return false;
    }
    let opt = pe + 24;
    let dirs = opt + if u16_at(opt) == Some(0x20B) { 112 } else { 96 };
    let (Some(at), Some(len)) = (u32_at(dirs + 4 * 8), u32_at(dirs + 4 * 8 + 4)) else { return false };
    let Some(table) = bytes.get(at..at.saturating_add(len)).filter(|t| len >= 8 && !t.is_empty()) else { return false };
    table.windows(signer.len()).any(|w| w == signer.as_bytes())
}

/// The manifest: the copy on disk while it's younger than a day, else asked for, else the last
/// good copy (offline).
async fn manifest(http: &reqwest::Client, dir: &Path) -> Option<(String, Entry)> {
    let file = root(dir).join("manifest.json");
    let age = std::fs::metadata(&file).ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).map(|d| d.as_secs());
    let saved = || std::fs::read(&file).ok().and_then(|b| parse(&b));
    if age.is_some_and(|a| a < MANIFEST_FRESH) {
        if let Some(m) = saved() {
            return Some(m);
        }
    }
    let got = async {
        let r = crate::net::send(http.get(MANIFEST).timeout(crate::net::QUICK), MANIFEST, false).await.ok()?;
        if !r.status().is_success() {
            return None;
        }
        r.bytes().await.ok()
    }
    .await;
    if let Some(b) = got.filter(|b| parse(b).is_some()) {
        let _ = std::fs::create_dir_all(root(dir));
        let _ = std::fs::write(&file, &b);
        return parse(&b);
    }
    saved()
}

/// The shared DLSS file, fetched when it isn't here yet (or the manifest moved to a newer
/// version). `progress` gets (done, total) while it downloads. Never called on a PC it doesn't
/// apply to.
pub async fn ensure(http: &reqwest::Client, dir: &Path, progress: &(dyn Fn(u64, u64) + Sync)) -> Result<PathBuf, String> {
    let Some((version, e)) = manifest(http, dir).await else {
        return current(dir).ok_or_else(|| FAILED.to_string());
    };
    let target = root(dir).join(&version).join(FILE);
    if target.is_file() && std::fs::metadata(&target).map(|m| m.len() == e.size).unwrap_or(false) {
        let _ = std::fs::write(root(dir).join("current.txt"), &version);
        return Ok(target);
    }
    let bytes = download(http, &e, progress).await.map_err(|why| {
        crate::trail::note(format!("DLSS: download failed ({why})"));
        FAILED.to_string()
    })?;
    if let Err(why) = check(&bytes, &e, &verify_signature) {
        crate::trail::note(format!("DLSS: the downloaded file failed its {why} check, so it wasn't kept"));
        return Err(FAILED.into());
    }
    let parent = target.parent().ok_or(FAILED)?;
    std::fs::create_dir_all(parent).map_err(|_| FAILED.to_string())?;
    let temp = parent.join(format!(".{FILE}.part"));
    std::fs::write(&temp, &bytes).and_then(|_| std::fs::rename(&temp, &target)).map_err(|_| {
        let _ = std::fs::remove_file(&temp);
        FAILED.to_string()
    })?;
    let _ = std::fs::write(root(dir).join("current.txt"), &version);
    // Older versions go once the new one is in place (games' own copies are separate files).
    for old in std::fs::read_dir(root(dir)).into_iter().flatten().flatten() {
        if old.path().is_dir() && old.file_name().to_string_lossy() != version {
            let _ = std::fs::remove_dir_all(old.path());
        }
    }
    crate::trail::note(format!("DLSS: NVIDIA DLSS {version} is ready"));
    Ok(target)
}

async fn download(http: &reqwest::Client, e: &Entry, progress: &(dyn Fn(u64, u64) + Sync)) -> Result<Vec<u8>, String> {
    if !url_ok(&e.url) {
        return Err("not NVIDIA's GitHub".into());
    }
    let mut r = http.get(&e.url).send().await.map_err(|x| x.to_string())?;
    // A redirect that left NVIDIA's GitHub (raw files are served from raw.githubusercontent.com).
    let end = r.url().as_str().to_string();
    if !(url_ok(&end) || end.starts_with("https://objects.githubusercontent.com/") || end.starts_with("https://media.githubusercontent.com/media/NVIDIA/")) {
        return Err(format!("redirected away to {end}"));
    }
    if !r.status().is_success() {
        return Err(format!("answered {}", r.status().as_u16()));
    }
    let mut bytes = Vec::with_capacity(e.size as usize);
    while let Some(chunk) = r.chunk().await.map_err(|x| x.to_string())? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() as u64 > e.size {
            return Err("bigger than NVIDIA's file".into());
        }
        progress(bytes.len() as u64, e.size);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{ "format": 1, "version": "310.9.16", "files": [
        { "name": "nvngx_dlss.dll", "os": "windows", "url": "https://raw.githubusercontent.com/NVIDIA/DLSS/v310.9.16/lib/Windows_x86_64/rel/nvngx_dlss.dll", "size": 58956912, "signer": "NVIDIA Corporation" } ] }"#;

    #[test]
    fn the_manifest_names_one_file_from_nvidia_only() {
        let (v, e) = parse(GOOD.as_bytes()).unwrap();
        assert_eq!(v, "310.9.16");
        assert_eq!(e.size, 58956912);
        // Anywhere but NVIDIA's own GitHub is refused, whatever the manifest says.
        for bad in ["https://recomplauncher.com/nvngx_dlss.dll", "https://raw.githubusercontent.com/someone/DLSS/x.dll", "http://github.com/NVIDIA/DLSS/x.dll", "https://github.com/NVIDIA/../x.dll", "https://github.com.evil/NVIDIA/x"] {
            assert!(!url_ok(bad), "{bad}");
            assert!(parse(GOOD.replace("https://raw.githubusercontent.com/NVIDIA/DLSS/v310.9.16/lib/Windows_x86_64/rel/nvngx_dlss.dll", bad).as_bytes()).is_none());
        }
        assert!(url_ok("https://github.com/NVIDIA/DLSS/raw/v310.9.16/lib/x.dll"));
        assert!(parse(GOOD.replace("\"format\": 1", "\"format\": 2").as_bytes()).is_none());
        assert!(parse(b"not json").is_none());
        assert!(parse(GOOD.replace("310.9.16\", \"files", "../x\", \"files").as_bytes()).is_none());
    }

    #[test]
    fn a_wrong_size_or_signature_isnt_kept() {
        let (_, e) = parse(GOOD.as_bytes()).unwrap();
        let small = Entry { size: 4, ..e.clone() };
        assert_eq!(check(b"abc", &small, &|_, _| true), Err("size"));
        assert_eq!(check(b"abcd", &small, &|_, _| false), Err("signature"));
        assert_eq!(check(b"abcd", &small, &|_, s| s == "NVIDIA Corporation"), Ok(()));
        let summed = Entry { sha256: "00".repeat(32), ..small };
        assert_eq!(check(b"abcd", &summed, &|_, _| true), Err("checksum"));
        // A file without a certificate table naming NVIDIA isn't signed by it.
        assert!(!has_signature(b"MZ not a real program", "NVIDIA Corporation"));
        assert!(!verify_signature(b"plain bytes", "NVIDIA Corporation"));
    }

    #[test]
    fn a_signed_pe_names_its_signer() {
        // A minimal PE32+ with a certificate table at its end that names NVIDIA.
        let mut b = vec![0u8; 0x200];
        b[..2].copy_from_slice(b"MZ");
        b[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        b[0x80..0x84].copy_from_slice(b"PE\0\0");
        let opt = 0x80 + 24;
        b[opt..opt + 2].copy_from_slice(&0x20Bu16.to_le_bytes());
        let cert = b"....0....CN=NVIDIA Corporation, O=NVIDIA Corporation....";
        let at = b.len();
        b.extend_from_slice(cert);
        let dir = opt + 112 + 4 * 8;
        b[dir..dir + 4].copy_from_slice(&(at as u32).to_le_bytes());
        b[dir + 4..dir + 8].copy_from_slice(&(cert.len() as u32).to_le_bytes());
        assert!(has_signature(&b, "NVIDIA Corporation"));
        assert!(!has_signature(&b, "Someone Else"));
    }

    #[test]
    fn only_rtx_cards_get_it() {
        for yes in ["NVIDIA GeForce RTX 3070", "NVIDIA GeForce RTX 4090 Laptop GPU", "NVIDIA Quadro RTX 4000", "NVIDIA RTX A2000", "NVIDIA TITAN RTX"] {
            assert!(rtx_card(yes), "{yes}");
        }
        for no in ["NVIDIA GeForce GTX 1660 SUPER", "NVIDIA GeForce GTX 1080 Ti", "AMD Radeon RX 7900 XTX", "Intel(R) Arc(TM) A770 Graphics", "Intel(R) UHD Graphics 630", "", "Microsoft Basic Render Driver", "AMD Radeon RTX-like"] {
            assert!(!rtx_card(no), "{no}");
        }
    }

    #[test]
    fn removing_it_clears_the_shared_copy() {
        let d = std::env::temp_dir().join(format!("rl-dlss-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        assert!(current(&d).is_none());
        std::fs::create_dir_all(root(&d).join("310.9.16")).unwrap();
        std::fs::write(root(&d).join("310.9.16").join(FILE), b"dll").unwrap();
        std::fs::write(root(&d).join("current.txt"), "310.9.16").unwrap();
        assert!(current(&d).is_some());
        remove(&d).unwrap();
        assert!(current(&d).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}

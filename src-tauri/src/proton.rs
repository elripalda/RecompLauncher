//! Windows recomps on Linux: started through Proton (Steam's, or a GE-Proton in
//! compatibilitytools.d), umu-run when it's installed, or Wine. Every game shares one prefix in
//! the launcher's own folder, so nothing is set up twice.

use std::path::{Path, PathBuf};

/// What starts a Windows program here.
#[derive(Debug, Clone, PartialEq)]
pub enum Runner {
    /// umu-run, with the Proton it should use when one was found.
    Umu(PathBuf, Option<PathBuf>),
    /// A Proton folder (with its `proton` script) and the Steam folder it belongs to.
    Proton(PathBuf, PathBuf),
    Wine(PathBuf),
}

impl Runner {
    pub fn name(&self) -> String {
        match self {
            Runner::Umu(_, Some(p)) | Runner::Proton(p, _) => p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Proton".into()),
            Runner::Umu(_, None) => "umu-run".into(),
            Runner::Wine(_) => "Wine".into(),
        }
    }
}

/// The launcher's data folder, where the shared prefix lives.
static DATA: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

pub fn set_data(dir: &Path) {
    let _ = DATA.set(dir.to_path_buf());
}

/// The Windows user folder inside the shared prefix: where a Windows game played here keeps its
/// AppData and Documents. Where Proton (or Wine) will make it when nothing has been played yet.
pub fn windows_user() -> Option<PathBuf> {
    windows_user_in(&DATA.get()?.join("proton"), runner().as_ref())
}

fn windows_user_in(base: &Path, r: Option<&Runner>) -> Option<PathBuf> {
    let pfx = base.join("pfx/drive_c/users/steamuser");
    if pfx.is_dir() {
        return Some(pfx);
    }
    let users = base.join("wine/drive_c/users");
    let me = std::env::var("USER").ok().filter(|u| !u.is_empty()).map(|u| users.join(u));
    if users.is_dir() {
        if let Some(m) = me.clone().filter(|m| m.is_dir()) {
            return Some(m);
        }
        let other = std::fs::read_dir(&users).into_iter().flatten().flatten().map(|e| e.path()).find(|p| p.is_dir() && p.file_name().map(|n| n != "Public").unwrap_or(false));
        if other.is_some() {
            return other;
        }
    }
    match r {
        Some(Runner::Wine(_)) => me,
        Some(_) => Some(pfx),
        None => None,
    }
}

fn on_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join(name)).find(|f| f.is_file()))
}

fn steam_roots() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else { return vec![] };
    [".steam/steam", ".local/share/Steam", ".var/app/com.valvesoftware.Steam/data/Steam", ".steam/root"]
        .iter()
        .map(|p| home.join(p))
        .filter(|p| p.is_dir())
        .collect()
}

/// How new a Proton is, from its folder's name: GE-Proton10-4 > Proton 9.0 (Beta) > Proton 8.0.
/// Experimental counts as newest of Valve's own.
pub fn rank(name: &str) -> (u32, u32, u32) {
    let n = name.to_lowercase();
    let nums: Vec<u32> = n.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect();
    let ge = n.contains("ge-proton") || n.starts_with("proton-ge") || n.contains("-ge-");
    let (a, b) = (nums.first().copied().unwrap_or(0), nums.get(1).copied().unwrap_or(0));
    if ge {
        (2, a, b)
    } else if n.contains("experimental") {
        (1, 999, 0)
    } else {
        (1, a, b)
    }
}

/// The processor a program was built for, from its ELF header: "x64", "arm64", or None when it
/// isn't a Linux program (or is for something else).
pub fn elf_arch(p: &Path) -> Option<&'static str> {
    use std::io::Read;
    let mut h = [0u8; 20];
    std::fs::File::open(p).and_then(|mut f| f.read_exact(&mut h)).ok()?;
    if &h[..4] != b"\x7fELF" {
        return None;
    }
    match u16::from_le_bytes([h[18], h[19]]) {
        0x3E => Some("x64"),
        0xB7 => Some("arm64"),
        _ => None,
    }
}

/// The processor a Proton folder's own programs are built for, when that can be read.
fn built_for(dir: &Path) -> Option<&'static str> {
    ["files/bin/wineserver", "files/bin/wine64", "files/bin/wine", "dist/bin/wineserver", "dist/bin/wine64", "dist/bin/wine"].iter().find_map(|b| elf_arch(&dir.join(b)))
}

/// A Proton built for another processor (an ARM64 one on a normal PC needs a runtime this PC can't
/// run): by the programs inside it, else its name or the Steam runtime its manifest asks for.
fn other_arch(dir: &Path, name: &str) -> bool {
    let here = if crate::assets::arch_now() == "arm64" { "arm64" } else { "x64" };
    if let Some(a) = built_for(dir) {
        return a != here;
    }
    let manifest = std::fs::read_to_string(dir.join("toolmanifest.vdf")).unwrap_or_default().to_lowercase();
    let arm = name.contains("arm64") || name.contains("aarch64") || manifest.contains("arm64") || manifest.contains("aarch64");
    arm != (here == "arm64")
}

/// Every Proton found, newest first, with its Steam folder.
pub fn protons() -> Vec<(PathBuf, PathBuf)> {
    let mut out = vec![];
    for root in steam_roots() {
        for parent in [root.join("steamapps/common"), root.join("compatibilitytools.d")] {
            for e in std::fs::read_dir(&parent).into_iter().flatten().flatten() {
                let p = e.path();
                let name = e.file_name().to_string_lossy().to_lowercase();
                if p.join("proton").is_file() && name.contains("proton") && !other_arch(&p, &name) {
                    out.push((p, root.clone()));
                }
            }
        }
    }
    out.sort_by(|a, b| rank(&b.0.file_name().unwrap_or_default().to_string_lossy()).cmp(&rank(&a.0.file_name().unwrap_or_default().to_string_lossy())));
    out.dedup_by(|a, b| a.0 == b.0);
    out
}

/// Where a downloaded Proton goes: the compatibilitytools.d of the Steam on this PC (the one
/// installed normally before the Flatpak one), or of a Steam folder made for it when there's none.
pub fn tools_dir() -> Option<PathBuf> {
    let roots = steam_roots();
    let real = roots.iter().find(|r| r.join("steamapps").is_dir() && !r.to_string_lossy().contains("/.var/app/")).or_else(|| roots.iter().find(|r| r.join("steamapps").is_dir())).or(roots.first());
    let root = real.cloned().or_else(|| dirs::home_dir().map(|h| h.join(".local/share/Steam")))?;
    Some(std::fs::canonicalize(&root).unwrap_or(root).join("compatibilitytools.d"))
}

/// The newest GE-Proton from its GitHub, unpacked where Steam and the launcher both find it.
/// Its name when it's in place (also when it already was).
pub async fn install_ge(
    http: &reqwest::Client,
    cache: &Path,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    report: &(dyn Fn(crate::install::Progress) + Send + Sync),
) -> Result<String, crate::Error> {
    use crate::Error;
    if crate::assets::arch_now() == "arm64" {
        return Err(Error::Msg("GE-Proton is only made for x86-64 PCs.".into()));
    }
    let to = tools_dir().ok_or_else(|| Error::Msg("Couldn't find your home folder.".into()))?;
    const REPO: &str = "https://github.com/GloriousEggroll/proton-ge-custom";
    // The newest one's name, from where GitHub's "latest release" page leads (not its API, which
    // turns players away after a few asks an hour).
    let ok_tag = |t: &str| t.starts_with("GE-Proton") && !t.contains("..") && t.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    let from_page = match http.get(format!("{REPO}/releases/latest")).timeout(std::time::Duration::from_secs(30)).send().await {
        Ok(r) if r.status().is_success() => r.url().path().rsplit('/').next().map(str::to_string).filter(|t| ok_tag(t)),
        _ => None,
    };
    // When that page doesn't answer: GitHub's API, once.
    let tag = match from_page {
        Some(t) => t,
        None => {
            let v: serde_json::Value = http
                .get("https://api.github.com/repos/GloriousEggroll/proton-ge-custom/releases/latest")
                .header("Accept", "application/vnd.github+json")
                .timeout(std::time::Duration::from_secs(30))
                .send()
                .await
                .map_err(|e| Error::Msg(format!("GitHub didn't answer about GE-Proton: {e}")))?
                .error_for_status()
                .map_err(|e| Error::Msg(format!("GitHub didn't answer about GE-Proton: {e}")))?
                .json()
                .await?;
            v.get("tag_name").and_then(|t| t.as_str()).unwrap_or("").to_string()
        }
    };
    if !ok_tag(&tag) {
        return Err(Error::Msg("GitHub's answer about GE-Proton didn't make sense. Try again later.".into()));
    }
    // Already here, and built for this PC: nothing to do. One built for another processor (an
    // earlier version of this button picked the ARM64 download) is replaced.
    if to.join(&tag).join("proton").is_file() && !other_arch(&to.join(&tag), &tag.to_lowercase()) {
        return Ok(tag);
    }
    // The x86-64 download, by the names GE-Proton gives its files (newer releases say so in the
    // name; older ones had only the one).
    let mut pick = None;
    for name in [format!("{tag}-x86_64"), tag.clone()] {
        let url = format!("{REPO}/releases/download/{tag}/{name}.tar.gz");
        let ok = http.head(&url).timeout(std::time::Duration::from_secs(30)).send().await.map(|r| r.status().is_success()).unwrap_or(false);
        if ok {
            pick = Some(name);
            break;
        }
    }
    let name = pick.ok_or_else(|| Error::Msg(format!("{tag} has no download for this PC yet. Try again later.")))?;
    // Its checksum, published beside it.
    let sum = http.get(format!("{REPO}/releases/download/{tag}/{name}.sha512sum")).timeout(std::time::Duration::from_secs(30)).send().await.ok().filter(|r| r.status().is_success());
    let want = match sum {
        Some(r) => r.text().await.ok().and_then(|t| t.split_whitespace().next().map(str::to_lowercase)).filter(|h| h.len() == 128),
        None => None,
    };
    let asset = crate::assets::Asset { name: format!("{name}.tar.gz"), size: 0, digest: None, browser_download_url: format!("{REPO}/releases/download/{tag}/{name}.tar.gz") };
    let dl = crate::install::download(http, &asset, &cache.join("ge-proton"), "ge-proton", cancel, report).await?;
    report(crate::install::Progress::new("ge-proton", "unpack", 0, 0));
    let (file, to2, tag2) = (dl.file.clone(), to.clone(), tag.clone());
    tokio::task::spawn_blocking(move || -> Result<(), Error> {
        if let Some(want) = want {
            use sha2::Digest;
            let mut h = sha2::Sha512::new();
            std::io::copy(&mut std::fs::File::open(&file)?, &mut h)?;
            if hex::encode(h.finalize()) != want {
                let _ = std::fs::remove_file(&file);
                return Err(Error::Msg("The GE-Proton download didn't match its checksum. Try again.".into()));
            }
        }
        std::fs::create_dir_all(&to2)?;
        // GE-Protons an earlier version of this button put here built for ARM64 under a normal
        // name: they can't run on this PC, so they go.
        for e in std::fs::read_dir(&to2).into_iter().flatten().flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            let p = e.path();
            if n.starts_with("GE-Proton") && !n.to_lowercase().contains("aarch64") && !n.to_lowercase().contains("arm64") && built_for(&p).map(|a| a == "arm64").unwrap_or(false) {
                crate::trail::note(format!("GE-Proton: {n} is built for ARM64, taken out"));
                let _ = std::fs::remove_dir_all(&p);
            }
        }
        // Unpacked beside where it goes, then moved in whole: Steam never sees half a Proton.
        let part = to2.join(".recomplauncher-part");
        let _ = std::fs::remove_dir_all(&part);
        std::fs::create_dir_all(&part)?;
        let mut t = tar::Archive::new(flate2::read::GzDecoder::new(std::fs::File::open(&file)?));
        t.set_preserve_permissions(true);
        t.unpack(&part)?;
        let top = part.join(&tag2);
        let top = if top.join("proton").is_file() { top } else { std::fs::read_dir(&part)?.flatten().map(|e| e.path()).find(|p| p.join("proton").is_file()).ok_or_else(|| Error::Msg("The GE-Proton download didn't have Proton in it.".into()))? };
        let _ = std::fs::remove_dir_all(to2.join(&tag2));
        std::fs::rename(&top, to2.join(&tag2))?;
        let _ = std::fs::remove_dir_all(&part);
        let _ = std::fs::remove_file(&file);
        Ok(())
    })
    .await
    .map_err(|e| Error::Msg(e.to_string()))??;
    Ok(tag)
}

/// The version an umu-run says it is ("umu-launcher version 1.2.9 (…)"), asked once.
fn umu_version(umu: &Path) -> Option<(u32, u32, u32)> {
    static V: std::sync::OnceLock<Option<(u32, u32, u32)>> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        let out = std::process::Command::new(umu).arg("--version").stdin(std::process::Stdio::null()).output().ok()?;
        version_in(&format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
    })
}

fn version_in(text: &str) -> Option<(u32, u32, u32)> {
    let i = text.find("version")?;
    let mut n = text[i + 7..].trim_start().split(|c: char| !c.is_ascii_digit() && c != '.').next()?.split('.').map(|x| x.parse::<u32>().ok());
    Some((n.next()??, n.next().flatten().unwrap_or(0), n.next().flatten().unwrap_or(0)))
}

/// A Proton built for a newer Steam runtime than Proton 9's ("soldier" and "sniper"): its
/// toolmanifest asks for another one (GE-Proton 11 asks for steamrt4).
fn newer_runtime(proton: &Path) -> bool {
    let Ok(m) = std::fs::read_to_string(proton.join("toolmanifest.vdf")) else { return false };
    let id = m.lines().find(|l| l.contains("require_tool_appid")).and_then(|l| l.split('"').filter(|x| !x.is_empty() && x.chars().all(|c| c.is_ascii_digit())).next().map(str::to_string));
    matches!(id.as_deref(), Some(x) if x != "1628350" && x != "1391110")
}

/// The best way to start a Windows program on this PC, if there's one.
pub fn runner() -> Option<Runner> {
    let proton = protons().into_iter().next();
    if let Some(umu) = on_path("umu-run") {
        // An umu-run older than 1.4 can't set up the runtime a newer Proton needs ("Failed to
        // match … with a container runtime"): that Proton runs on its own instead.
        let old = proton.as_ref().map(|p| newer_runtime(&p.0)).unwrap_or(false) && umu_version(&umu).map(|v| v < (1, 4, 0)).unwrap_or(false);
        if !old {
            return Some(Runner::Umu(umu, proton.map(|p| p.0)));
        }
        static SAID: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !SAID.swap(true, std::sync::atomic::Ordering::Relaxed) {
            crate::trail::note("Proton: this PC's umu-run is too old for the newest Proton, so Proton runs on its own");
        }
    }
    if let Some((p, root)) = proton {
        return Some(Runner::Proton(p, root));
    }
    on_path("wine").map(Runner::Wine)
}

/// Proton's controller helper for desktop windows (Xalia) crashes some games' start ("An item with
/// the same key has already been added"). Off, unless the player turned it on themselves.
fn no_xalia(c: &mut std::process::Command) {
    if std::env::var_os("PROTON_USE_XALIA").is_none() {
        c.env("PROTON_USE_XALIA", "0");
    }
}

/// The command that starts `exe` through `r`, with the shared prefix in `data`.
pub fn command(r: &Runner, data: &Path, exe: &Path) -> tokio::process::Command {
    tokio::process::Command::from(std_command(r, data, exe))
}

/// The same, as a plain (not async) command: for a game's setup program.
pub fn std_command(r: &Runner, data: &Path, exe: &Path) -> std::process::Command {
    let base = data.join("proton");
    let _ = std::fs::create_dir_all(&base);
    match r {
        Runner::Umu(umu, proton) => {
            let mut c = std::process::Command::new(umu);
            c.arg(exe).env("WINEPREFIX", base.join("pfx")).env("GAMEID", "umu-default");
            no_xalia(&mut c);
            if let Some(p) = proton {
                c.env("PROTONPATH", p);
            }
            c
        }
        Runner::Proton(p, root) => {
            let mut c = std::process::Command::new(p.join("proton"));
            c.arg("run").arg(exe).env("STEAM_COMPAT_DATA_PATH", &base).env("STEAM_COMPAT_CLIENT_INSTALL_PATH", root);
            no_xalia(&mut c);
            c
        }
        Runner::Wine(w) => {
            let mut c = std::process::Command::new(w);
            c.arg(exe).env("WINEPREFIX", base.join("wine"));
            c
        }
    }
}

/// The shared prefix's folder (its drive_c is inside), for the player to look in.
pub fn prefix_dir(r: &Runner, data: &Path) -> PathBuf {
    let base = data.join("proton");
    match r {
        Runner::Wine(_) => base.join("wine"),
        _ => base.join("pfx"),
    }
}

/// Winetricks on the shared prefix, with the Wine the games run with, so what the player adds
/// there is there for every Windows game played here. umu-run brings its own Winetricks; with
/// Proton or Wine, `winetricks` is the one on this PC (or the one the launcher keeps, `own`).
pub fn winetricks_command(r: &Runner, data: &Path, own: Option<&Path>) -> Result<std::process::Command, String> {
    let base = data.join("proton");
    let _ = std::fs::create_dir_all(&base);
    if !matches!(r, Runner::Umu(..)) && on_path("zenity").is_none() && on_path("kdialog").is_none() {
        return Err("Winetricks needs zenity or kdialog to show its window. Install zenity from your distro's packages, then try again.".into());
    }
    let tricks = || on_path("winetricks").or_else(|| own.filter(|p| p.is_file()).map(Path::to_path_buf)).ok_or_else(|| "Winetricks isn't on this PC. Install winetricks from your distro's packages, then try again.".to_string());
    match r {
        Runner::Umu(umu, proton) => {
            let mut c = std::process::Command::new(umu);
            c.arg("winetricks").arg("--gui").env("WINEPREFIX", base.join("pfx")).env("GAMEID", "umu-default");
            if let Some(p) = proton {
                c.env("PROTONPATH", p);
            }
            Ok(c)
        }
        Runner::Proton(p, _) => {
            // Proton makes the prefix the first time it runs something: before that there's nothing to change.
            if !base.join("pfx").is_dir() {
                return Err("Play a Windows game here once first: that makes the Wine prefix Winetricks works on.".into());
            }
            let bin = ["files/bin", "dist/bin"].iter().map(|b| p.join(b)).find(|b| b.join("wine").is_file()).ok_or("This Proton's own Wine couldn't be found.")?;
            let wine = if bin.join("wine64").is_file() { bin.join("wine64") } else { bin.join("wine") };
            // Proton's own Wine, set up the way Protontricks does it.
            let top = bin.parent().unwrap_or(&bin).to_path_buf();
            let dlls = std::env::join_paths([top.join("lib64/wine"), top.join("lib/wine")]).unwrap_or_default();
            let mut c = std::process::Command::new(tricks()?);
            c.arg("--gui").env("WINEPREFIX", base.join("pfx")).env("WINE", &wine).env("WINELOADER", &wine).env("WINESERVER", bin.join("wineserver")).env("WINEDLLPATH", dlls);
            Ok(c)
        }
        Runner::Wine(w) => {
            let mut c = std::process::Command::new(tricks()?);
            c.arg("--gui").env("WINEPREFIX", base.join("wine")).env("WINE", w);
            Ok(c)
        }
    }
}

/// The launcher's data folder (where the shared prefix is), once it's known.
pub fn data_dir() -> Option<PathBuf> {
    DATA.get().cloned()
}

/// A Windows program by its file (.exe, .bat, .cmd, .msi).
pub fn is_windows_program(p: &Path) -> bool {
    matches!(p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).as_deref(), Some("exe" | "bat" | "cmd" | "msi"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn umu_versions_and_runtimes() {
        assert_eq!(super::version_in("umu-launcher version 1.2.9 (3.14.2 (main, Dec 5 2025))"), Some((1, 2, 9)));
        assert_eq!(super::version_in("umu-launcher version 1.4.4"), Some((1, 4, 4)));
        assert_eq!(super::version_in("nothing here"), None);
        let d = std::env::temp_dir().join("rl-proton-manifest");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("toolmanifest.vdf"), "\"manifest\"\n{\n  \"version\" \"2\"\n  \"commandline\" \"/proton %verb%\"\n  \"require_tool_appid\" \"1628350\"\n}\n").unwrap();
        assert!(!super::newer_runtime(&d));
        std::fs::write(d.join("toolmanifest.vdf"), "\"manifest\"\n{\n  \"require_tool_appid\" \"3810310\"\n}\n").unwrap();
        assert!(super::newer_runtime(&d));
        std::fs::remove_dir_all(&d).unwrap();
    }

    use super::*;

    #[test]
    fn winetricks_works_on_the_shared_prefix() {
        let base = std::env::temp_dir().join(format!("rl-wt-{}", std::process::id()));
        let proton = base.join("GE-Proton10");
        std::fs::create_dir_all(proton.join("files/bin")).unwrap();
        std::fs::write(proton.join("files/bin/wine"), b"").unwrap();
        let r = Runner::Proton(proton.clone(), base.join("steam"));
        assert_eq!(prefix_dir(&r, &base), base.join("proton/pfx"));
        assert_eq!(prefix_dir(&Runner::Wine(PathBuf::from("/usr/bin/wine")), &base), base.join("proton/wine"));
        // umu-run brings its own Winetricks, on the same prefix the games use.
        if let Ok(c) = winetricks_command(&Runner::Umu(PathBuf::from("/usr/bin/umu-run"), Some(proton.clone())), &base, None) {
            let env: Vec<_> = c.get_envs().filter_map(|(k, v)| Some((k.to_string_lossy().to_string(), v?.to_string_lossy().to_string()))).collect();
            assert!(env.contains(&("WINEPREFIX".into(), base.join("proton").join("pfx").to_string_lossy().to_string())));
            assert_eq!(c.get_args().next().map(|a| a.to_string_lossy().to_string()).as_deref(), Some("winetricks"));
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn finds_the_windows_user_folder() {
        let base = std::env::temp_dir().join(format!("rl-pfx-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let proton = Runner::Proton(PathBuf::from("/p"), PathBuf::from("/s"));
        // Nothing played yet: where Proton will make it.
        assert_eq!(windows_user_in(&base, Some(&proton)), Some(base.join("pfx/drive_c/users/steamuser")));
        assert_eq!(windows_user_in(&base, None), None);
        std::fs::create_dir_all(base.join("pfx/drive_c/users/steamuser")).unwrap();
        assert_eq!(windows_user_in(&base, None), Some(base.join("pfx/drive_c/users/steamuser")));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn an_arm_proton_under_a_normal_name_is_skipped() {
        let d = std::env::temp_dir().join("rl-ge-arch");
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("files/bin")).unwrap();
        let mut elf = vec![0u8; 64];
        elf[..4].copy_from_slice(b"\x7fELF");
        elf[18] = 0xB7;
        std::fs::write(d.join("files/bin/wineserver"), &elf).unwrap();
        assert_eq!(super::elf_arch(&d.join("files/bin/wineserver")), Some("arm64"));
        if crate::assets::arch_now() != "arm64" {
            assert!(super::other_arch(&d, "ge-proton11-12"));
        }
        elf[18] = 0x3E;
        std::fs::write(d.join("files/bin/wineserver"), &elf).unwrap();
        if crate::assets::arch_now() != "arm64" {
            assert!(!super::other_arch(&d, "ge-proton11-12"));
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn newest_proton_first() {
        let mut v = vec!["Proton 8.0", "Proton - Experimental", "GE-Proton9-20", "Proton 9.0 (Beta)", "GE-Proton10-4"];
        v.sort_by(|a, b| rank(b).cmp(&rank(a)));
        assert_eq!(v, vec!["GE-Proton10-4", "GE-Proton9-20", "Proton - Experimental", "Proton 9.0 (Beta)", "Proton 8.0"]);
    }
}

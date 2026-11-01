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

/// Every Proton found, newest first, with its Steam folder.
pub fn protons() -> Vec<(PathBuf, PathBuf)> {
    let mut out = vec![];
    for root in steam_roots() {
        for parent in [root.join("steamapps/common"), root.join("compatibilitytools.d")] {
            for e in std::fs::read_dir(&parent).into_iter().flatten().flatten() {
                let p = e.path();
                let name = e.file_name().to_string_lossy().to_lowercase();
                if p.join("proton").is_file() && name.contains("proton") {
                    out.push((p, root.clone()));
                }
            }
        }
    }
    out.sort_by(|a, b| rank(&b.0.file_name().unwrap_or_default().to_string_lossy()).cmp(&rank(&a.0.file_name().unwrap_or_default().to_string_lossy())));
    out.dedup_by(|a, b| a.0 == b.0);
    out
}

/// The best way to start a Windows program on this PC, if there's one.
pub fn runner() -> Option<Runner> {
    let proton = protons().into_iter().next();
    if let Some(umu) = on_path("umu-run") {
        return Some(Runner::Umu(umu, proton.map(|p| p.0)));
    }
    if let Some((p, root)) = proton {
        return Some(Runner::Proton(p, root));
    }
    on_path("wine").map(Runner::Wine)
}

/// The command that starts `exe` through `r`, with the shared prefix in `data`.
pub fn command(r: &Runner, data: &Path, exe: &Path) -> tokio::process::Command {
    let base = data.join("proton");
    let _ = std::fs::create_dir_all(&base);
    match r {
        Runner::Umu(umu, proton) => {
            let mut c = tokio::process::Command::new(umu);
            c.arg(exe).env("WINEPREFIX", base.join("pfx")).env("GAMEID", "umu-default");
            if let Some(p) = proton {
                c.env("PROTONPATH", p);
            }
            c
        }
        Runner::Proton(p, root) => {
            let mut c = tokio::process::Command::new(p.join("proton"));
            c.arg("run").arg(exe).env("STEAM_COMPAT_DATA_PATH", &base).env("STEAM_COMPAT_CLIENT_INSTALL_PATH", root);
            c
        }
        Runner::Wine(w) => {
            let mut c = tokio::process::Command::new(w);
            c.arg(exe).env("WINEPREFIX", base.join("wine"));
            c
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_proton_first() {
        let mut v = vec!["Proton 8.0", "Proton - Experimental", "GE-Proton9-20", "Proton 9.0 (Beta)", "GE-Proton10-4"];
        v.sort_by(|a, b| rank(b).cmp(&rank(a)));
        assert_eq!(v, vec!["GE-Proton10-4", "GE-Proton9-20", "Proton - Experimental", "Proton 9.0 (Beta)", "Proton 8.0"]);
    }
}

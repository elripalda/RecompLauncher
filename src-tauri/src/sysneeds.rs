//! What a game needs from the PC (`system` in its recomplauncher.json), by system: the oldest
//! Windows or macOS it runs on, the oldest glibc for its Linux build, and its graphics API. The
//! launcher compares what it can (the system's version, glibc) with this PC, and says so when the
//! PC is too old. It never stops the game: the developer may be wrong, or the PC may manage.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `system`: one part for each system the game ships a build for. A part or a key left out is no
/// known need.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct SystemNeeds {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows: Option<OsNeeds>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linux: Option<OsNeeds>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub macos: Option<OsNeeds>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct OsNeeds {
    /// The oldest Windows ("10", "11", or a build like "10.0.19041") or macOS ("13", "14.4").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version: String,
    /// The oldest glibc its Linux build runs with ("2.35").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub glibc: String,
    /// The graphics API it draws with and its oldest version: {"vulkan": "1.3"}.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub graphics: BTreeMap<String, String>,
}

/// The graphics APIs `system.*.graphics` names.
pub const GRAPHICS: [&str; 6] = ["vulkan", "opengl", "d3d12", "d3d11", "d3d9", "metal"];

impl SystemNeeds {
    /// The part for a build for `os` ("windows", "linux", "macos"). A Windows build played
    /// through Proton has the Windows part, though nothing of it is checked there.
    pub fn for_build(&self, os: &str) -> Option<&OsNeeds> {
        match os {
            "windows" => self.windows.as_ref(),
            "linux" => self.linux.as_ref(),
            "macos" => self.macos.as_ref(),
            _ => None,
        }
    }
}

/// Something this PC has older than the game asks for: what ("windows", "macos", "glibc"), the
/// oldest the game runs on, and what the PC has.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TooOld {
    pub what: &'static str,
    pub want: String,
    pub have: String,
}

/// What of `needs` a PC on `pc_os` with these versions doesn't have. `build_os` is the build's own
/// system: a Windows build through Proton checks nothing (its Windows version means nothing there,
/// and Proton brings its own libraries). A version the PC can't tell is never counted against it.
pub fn too_old(needs: &OsNeeds, build_os: &str, pc_os: &str, os_version: Option<&str>, glibc: Option<&str>) -> Vec<TooOld> {
    if build_os != pc_os {
        return vec![];
    }
    // Linux has no one version to ask for: its games are checked by glibc.
    if pc_os == "linux" {
        let want = needs.glibc.trim();
        return match glibc {
            Some(have) if !want.is_empty() && !at_least(have, want) => vec![TooOld { what: "glibc", want: want.to_string(), have: have.to_string() }],
            _ => vec![],
        };
    }
    let want = needs.version.trim();
    let Some(have) = os_version.filter(|_| !want.is_empty()) else { return vec![] };
    let windows = pc_os == "windows";
    let ok = if windows { windows_at_least(have, want) } else { at_least(have, want) };
    if ok {
        return vec![];
    }
    vec![TooOld { what: if windows { "windows" } else { "macos" }, want: want.to_string(), have: have.to_string() }]
}

/// Whether version `have` is `want` or newer, part by part as numbers: 2.39 is newer than 2.4.
/// Parts that aren't numbers count as 0, and a missing part as 0 (14 is 14.0).
pub fn at_least(have: &str, want: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> { v.trim().split('.').map(|p| p.trim().parse().unwrap_or(0)).collect() };
    let (h, w) = (parts(have), parts(want));
    for i in 0..h.len().max(w.len()) {
        let (a, b) = (h.get(i).copied().unwrap_or(0), w.get(i).copied().unwrap_or(0));
        if a != b {
            return a > b;
        }
    }
    true
}

/// Windows says it's 10.0.<build> for 10 and 11 alike: Windows 11 is build 22000 and later.
pub fn windows_at_least(have: &str, want: &str) -> bool {
    let build = have.split('.').nth(2).and_then(|b| b.trim().parse::<u64>().ok()).unwrap_or(0);
    match want.trim() {
        "11" => at_least(have, "10") && build >= 22000,
        w => at_least(have, w),
    }
}

/// This PC's glibc ("2.39"), the one the launcher runs with: inside the Flatpak that's the
/// runtime's, which is also the one its games get.
pub fn glibc_version() -> Option<String> {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        extern "C" {
            fn gnu_get_libc_version() -> *const std::os::raw::c_char;
        }
        // SAFETY: glibc returns a pointer to a static, NUL-terminated string.
        let v = unsafe { std::ffi::CStr::from_ptr(gnu_get_libc_version()) };
        return Some(v.to_string_lossy().trim().to_string()).filter(|s| !s.is_empty());
    }
    #[allow(unreachable_code)]
    None
}

/// This PC's Windows ("10.0.22631") or macOS ("14.5") version; None on Linux, whose games are
/// checked by glibc instead.
pub fn os_version() -> Option<String> {
    #[cfg(windows)]
    {
        return Some(crate::win::os_version()).filter(|v| !v.is_empty());
    }
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("/usr/bin/sw_vers").arg("-productVersion").output().ok()?;
        return Some(String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|v| !v.is_empty());
    }
    #[allow(unreachable_code)]
    None
}

/// Values in `system` that don't read as a version, and graphics APIs it doesn't know: for the
/// developer tools.
pub fn bad_values(s: &SystemNeeds) -> Vec<String> {
    let looks_like_version = |v: &str| !v.trim().is_empty() && v.trim().split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    let mut out = vec![];
    for (os, part) in [("windows", &s.windows), ("linux", &s.linux), ("macos", &s.macos)] {
        let Some(part) = part else { continue };
        if !part.version.is_empty() && !looks_like_version(&part.version) {
            out.push(format!("system.{os}.version \"{}\"", part.version));
        }
        if !part.glibc.is_empty() && (os != "linux" || !looks_like_version(&part.glibc)) {
            out.push(format!("system.{os}.glibc \"{}\"", part.glibc));
        }
        if os == "linux" && !part.version.is_empty() {
            out.push("system.linux.version (Linux builds give glibc instead)".to_string());
        }
        for (api, v) in &part.graphics {
            if !GRAPHICS.contains(&api.as_str()) {
                out.push(format!("system.{os}.graphics.{api}"));
            } else if !v.is_empty() && !v.split(['.', '_']).all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())) {
                out.push(format!("system.{os}.graphics.{api} \"{v}\""));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn needs(json: &str) -> SystemNeeds {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn versions_compare_as_numbers_part_by_part() {
        assert!(at_least("2.39", "2.35"));
        assert!(at_least("2.35", "2.35"));
        assert!(!at_least("2.31", "2.35"));
        assert!(at_least("2.39", "2.4"), "39 is more than 4");
        assert!(at_least("14", "14.0"));
        assert!(!at_least("13.6", "14"));
    }

    #[test]
    fn windows_11_is_told_by_its_build() {
        assert!(windows_at_least("10.0.22631", "11"));
        assert!(!windows_at_least("10.0.19045", "11"));
        assert!(windows_at_least("10.0.19045", "10"));
        assert!(!windows_at_least("10.0.17763", "10.0.19041"));
    }

    #[test]
    fn an_old_glibc_is_named_with_both_versions() {
        let n = needs(r#"{"linux":{"glibc":"2.35","graphics":{"vulkan":"1.3"}}}"#);
        let got = too_old(n.for_build("linux").unwrap(), "linux", "linux", None, Some("2.31"));
        assert_eq!(got, vec![TooOld { what: "glibc", want: "2.35".into(), have: "2.31".into() }]);
        assert!(too_old(n.for_build("linux").unwrap(), "linux", "linux", None, Some("2.39")).is_empty());
    }

    #[test]
    fn an_old_windows_or_mac_is_named() {
        let n = needs(r#"{"windows":{"version":"11"},"macos":{"version":"14"}}"#);
        let win = too_old(n.for_build("windows").unwrap(), "windows", "windows", Some("10.0.19045"), None);
        assert_eq!(win[0].what, "windows");
        let mac = too_old(n.for_build("macos").unwrap(), "macos", "macos", Some("13.6"), None);
        assert_eq!(mac, vec![TooOld { what: "macos", want: "14".into(), have: "13.6".into() }]);
    }

    #[test]
    fn a_windows_build_through_proton_checks_nothing() {
        let n = needs(r#"{"windows":{"version":"11"}}"#);
        assert!(too_old(n.for_build("windows").unwrap(), "windows", "linux", None, Some("2.17")).is_empty());
    }

    #[test]
    fn what_the_pc_cant_tell_is_never_counted_against_it() {
        let n = needs(r#"{"linux":{"glibc":"2.35"},"windows":{"version":"11"}}"#);
        assert!(too_old(n.for_build("linux").unwrap(), "linux", "linux", None, None).is_empty());
        assert!(too_old(n.for_build("windows").unwrap(), "windows", "windows", None, None).is_empty());
    }

    #[test]
    fn a_part_left_out_is_no_need() {
        let n = needs(r#"{"linux":{"glibc":"2.35"}}"#);
        assert!(n.for_build("windows").is_none());
        assert!(n.for_build("macos").is_none());
    }

    #[test]
    fn this_pcs_glibc_reads_as_a_version() {
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        assert!(glibc_version().is_some_and(|v| at_least(&v, "2.17")));
        #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
        assert!(glibc_version().is_none());
    }

    #[test]
    fn values_that_arent_versions_or_apis_are_named() {
        let n = needs(r#"{"linux":{"glibc":"new","version":"22.04","graphics":{"vulkan":"1.3","dx12":"12"}},"windows":{"version":"11","graphics":{"d3d12":"12_0"}}}"#);
        let bad = bad_values(&n);
        assert!(bad.iter().any(|b| b.contains("glibc \"new\"")), "{bad:?}");
        assert!(bad.iter().any(|b| b.contains("system.linux.version")), "{bad:?}");
        assert!(bad.iter().any(|b| b.contains("graphics.dx12")), "{bad:?}");
        assert!(!bad.iter().any(|b| b.contains("d3d12") || b.contains("vulkan")), "{bad:?}");
    }
}

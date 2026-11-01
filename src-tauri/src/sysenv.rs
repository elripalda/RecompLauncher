//! Programs RecompLauncher starts (games, their setups, Proton, Steam) get the PC's own
//! environment, not the launcher's. Run as an AppImage, the launcher's environment points at the
//! libraries inside it (LD_LIBRARY_PATH and friends), and a game that inherits that loads the
//! launcher's OpenSSL or GTK instead of its own and quits at once.

use std::ffi::OsString;
use std::sync::OnceLock;

/// Variables an AppImage sets for itself: gone for the programs it starts.
const OWN: [&str; 5] = ["APPDIR", "APPIMAGE", "ARGV0", "OWD", "APPIMAGE_EXTRACT_AND_RUN"];
/// Lists of folders an AppImage adds its own to: those taken back out.
const LISTS: [&str; 17] = [
    "LD_LIBRARY_PATH", "LD_PRELOAD", "PATH", "XDG_DATA_DIRS", "XDG_CONFIG_DIRS", "GSETTINGS_SCHEMA_DIR", "GST_PLUGIN_SYSTEM_PATH", "GST_PLUGIN_SYSTEM_PATH_1_0",
    "GST_PLUGIN_PATH", "GST_PLUGIN_PATH_1_0", "GIO_MODULE_DIR", "GDK_PIXBUF_MODULE_FILE", "GTK_PATH", "QT_PLUGIN_PATH", "PYTHONPATH", "PERLLIB", "GI_TYPELIB_PATH",
];
/// Single paths an AppImage points into itself: gone when they do.
const SINGLE: [&str; 5] = ["PYTHONHOME", "GTK_EXE_PREFIX", "GTK_DATA_PREFIX", "GST_PLUGIN_SCANNER", "GTK_IM_MODULE_FILE"];

/// One change to make: a variable set to a value, or taken away.
type Change = (&'static str, Option<OsString>);

fn inside(entry: &str, appdir: &str) -> bool {
    (!appdir.is_empty() && entry.starts_with(appdir)) || entry.contains("/.mount_") || entry.contains("/tmp/appimage")
}

/// What has to change, worked out once. Empty when the launcher isn't an AppImage.
pub fn changes_from(get: &dyn Fn(&str) -> Option<String>) -> Vec<Change> {
    let appdir = get("APPDIR").unwrap_or_default();
    if appdir.is_empty() && get("APPIMAGE").is_none() {
        return vec![];
    }
    let appdir = appdir.trim_end_matches('/').to_string();
    let mut out: Vec<Change> = OWN.iter().filter(|k| get(k).is_some()).map(|k| (*k, None)).collect();
    for k in LISTS {
        let Some(v) = get(k) else { continue };
        let kept: Vec<&str> = v.split(':').filter(|e| !e.is_empty() && !inside(e, &appdir)).collect();
        if kept.len() == v.split(':').filter(|e| !e.is_empty()).count() {
            continue;
        }
        let mut kept: Vec<String> = kept.into_iter().map(str::to_string).collect();
        // Apps installed as Flatpaks (often the browser) are listed from these: kept in view.
        if k == "XDG_DATA_DIRS" {
            let home = get("HOME").unwrap_or_default();
            for d in [format!("{home}/.local/share/flatpak/exports/share"), "/var/lib/flatpak/exports/share".to_string(), "/usr/local/share".to_string(), "/usr/share".to_string()] {
                if !kept.contains(&d) && std::path::Path::new(&d).is_dir() && !home.is_empty() {
                    kept.push(d);
                }
            }
        }
        out.push((k, if kept.is_empty() { None } else { Some(OsString::from(kept.join(":"))) }));
    }
    for k in SINGLE {
        if get(k).map(|v| inside(&v, &appdir)).unwrap_or(false) {
            out.push((k, None));
        }
    }
    // PATH can't go altogether: the usual places, when the AppImage's were all there was.
    if let Some(i) = out.iter().position(|(k, v)| *k == "PATH" && v.is_none()) {
        out[i].1 = Some("/usr/local/bin:/usr/bin:/bin".into());
    }
    out
}

fn changes() -> &'static [Change] {
    static C: OnceLock<Vec<Change>> = OnceLock::new();
    C.get_or_init(|| if cfg!(target_os = "linux") { changes_from(&|k| std::env::var(k).ok()) } else { vec![] })
}

pub fn clean_std(c: &mut std::process::Command) {
    for (k, v) in changes() {
        match v {
            Some(v) => c.env(k, v),
            None => c.env_remove(k),
        };
    }
}

pub fn clean_tokio(c: &mut tokio::process::Command) {
    for (k, v) in changes() {
        match v {
            Some(v) => c.env(k, v),
            None => c.env_remove(k),
        };
    }
}

/// The launcher's own start on Linux, before any window: when Steam starts it (a Steam shortcut,
/// Gaming Mode on a Steam Deck), Steam hands it Steam's own libraries and overlay, which the
/// launcher's web view can crash on, so it starts itself again without them; and under Gaming
/// Mode's compositor the web view draws with a renderer that works there.
#[cfg(target_os = "linux")]
pub fn prepare_start() {
    let var = |k: &str| std::env::var(k).unwrap_or_default();
    let gamescope = var("XDG_CURRENT_DESKTOP").eq_ignore_ascii_case("gamescope") || std::env::var_os("GAMESCOPE_WAYLAND_DISPLAY").is_some();
    let from_steam = ["SteamGameId", "SteamAppId", "SteamClientLaunch", "SteamEnv"].iter().any(|k| std::env::var_os(k).is_some());
    if (gamescope || from_steam) && std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
    if std::env::var_os("RL_STEAM_CLEAN").is_some() {
        return;
    }
    let (preload, libs) = (var("LD_PRELOAD"), var("LD_LIBRARY_PATH"));
    let keep_pre = steam_free(&preload, ' ', &["gameoverlayrenderer"]);
    let keep_lib = steam_free(&libs, ':', &["steam-runtime", "ubuntu12_32", "ubuntu12_64", "/steamrt", "pressure-vessel"]);
    if keep_pre == preload && keep_lib == libs {
        return;
    }
    for (k, v) in [("LD_PRELOAD", keep_pre), ("LD_LIBRARY_PATH", keep_lib)] {
        if v.is_empty() {
            std::env::remove_var(k);
        } else {
            std::env::set_var(k, v);
        }
    }
    std::env::set_var("RL_STEAM_CLEAN", "1");
    if let Ok(exe) = std::env::current_exe() {
        use std::os::unix::process::CommandExt;
        // Only returns when it couldn't start again; then it carries on as it is.
        let _ = std::process::Command::new(exe).args(std::env::args_os().skip(1)).exec();
    }
}

/// A list of paths (or preloads) without Steam's own entries.
pub fn steam_free(list: &str, sep: char, drop: &[&str]) -> String {
    let sep_s = sep.to_string();
    list.split([sep, if sep == ' ' { ':' } else { sep }]).filter(|e| !e.is_empty() && !drop.iter().any(|d| e.contains(d))).collect::<Vec<_>>().join(&sep_s)
}

#[cfg(test)]
mod tests {
    #[test]
    fn steam_entries_are_taken_out() {
        assert_eq!(super::steam_free("/tmp/.mount_x/usr/lib:/home/deck/.steam/root/ubuntu12_32/steam-runtime/lib:/usr/lib", ':', &["steam-runtime", "ubuntu12_32"]), "/tmp/.mount_x/usr/lib:/usr/lib");
        assert_eq!(super::steam_free("/home/deck/.local/share/Steam/ubuntu12_64/gameoverlayrenderer.so", ' ', &["gameoverlayrenderer"]), "");
        assert_eq!(super::steam_free("/usr/lib", ':', &["steam-runtime"]), "/usr/lib");
    }
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn appimage_folders_are_taken_out() {
        let env: HashMap<&str, &str> = HashMap::from([
            ("APPDIR", "/tmp/.mount_RecompgbHmdl"),
            ("APPIMAGE", "/home/a/RecompLauncher.AppImage"),
            ("LD_LIBRARY_PATH", "/tmp/.mount_RecompgbHmdl/usr/lib:/tmp/.mount_RecompgbHmdl/usr/lib/x86_64-linux-gnu"),
            ("PATH", "/tmp/.mount_RecompgbHmdl/usr/bin:/usr/bin:/bin"),
            ("XDG_DATA_DIRS", "/usr/share"),
            ("PYTHONHOME", "/tmp/.mount_RecompgbHmdl/usr"),
        ]);
        let c = changes_from(&|k| env.get(k).map(|s| s.to_string()));
        let get = |k: &str| c.iter().find(|(n, _)| *n == k).map(|(_, v)| v.clone());
        assert_eq!(get("LD_LIBRARY_PATH"), Some(None));
        assert_eq!(get("PATH"), Some(Some(OsString::from("/usr/bin:/bin"))));
        assert_eq!(get("APPDIR"), Some(None));
        assert_eq!(get("PYTHONHOME"), Some(None));
        assert_eq!(get("XDG_DATA_DIRS"), None); // nothing of the AppImage's in it
        // Not an AppImage: nothing changes.
        assert!(changes_from(&|k| if k == "PATH" { Some("/usr/bin".into()) } else { None }).is_empty());
    }
}

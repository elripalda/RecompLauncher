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

// Marks the NVIDIA setting added by this launcher, so games keep the player's own sync policy.
const OWN_NV_SYNC: &str = "RL_WEBKIT_NV_SYNC";

fn inside(entry: &str, appdir: &str) -> bool {
    (!appdir.is_empty() && entry.starts_with(appdir)) || entry.contains("/.mount_") || entry.contains("/tmp/appimage")
}

/// AppImage paths and launcher-added graphics settings to remove from child programs.
pub fn changes_from(get: &dyn Fn(&str) -> Option<String>) -> Vec<Change> {
    let mut out = Vec::new();
    if get(OWN_NV_SYNC).as_deref() == Some("1") {
        out.extend([(OWN_NV_SYNC, None), ("__NV_DISABLE_EXPLICIT_SYNC", None)]);
    }
    let appdir = get("APPDIR").unwrap_or_default();
    if appdir.is_empty() && get("APPIMAGE").is_none() {
        return out;
    }
    let appdir = appdir.trim_end_matches('/').to_string();
    out.extend(OWN.iter().filter(|k| get(k).is_some()).map(|k| (*k, None)));
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
    C.get_or_init(|| if cfg!(any(test, target_os = "linux")) { changes_from(&|k| std::env::var(k).ok()) } else { vec![] })
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
pub fn prepare_start(data: Option<std::path::PathBuf>) {
    let var = |k: &str| std::env::var(k).unwrap_or_default();
    let gamescope = var("XDG_CURRENT_DESKTOP").eq_ignore_ascii_case("gamescope") || std::env::var_os("GAMESCOPE_WAYLAND_DISPLAY").is_some();
    let from_steam = ["SteamGameId", "SteamAppId", "SteamClientLaunch", "SteamEnv"].iter().any(|k| std::env::var_os(k).is_some());
    // Avoid forcing every NVIDIA Wayland window through the legacy renderer: at high display
    // scales even Settings can fall to ~4 fps. Disabling NVIDIA's explicit sync preserves the
    // accelerated path while avoiding the Wayland protocol error (Tauri #9394, WebKit #324551).
    let nvidia = std::path::Path::new("/proc/driver/nvidia/version").exists();
    let appimage = std::env::var_os("APPIMAGE").is_some();
    // The player's choice in Settings → Appearance wins over the guess; their own variables win over both.
    let mode = data.map(|d| graphics_mode(&d)).unwrap_or_default();
    let wayland = (std::env::var_os("WAYLAND_DISPLAY").is_some() || var("XDG_SESSION_TYPE").eq_ignore_ascii_case("wayland")) && !var("GDK_BACKEND").starts_with("x11");
    let graphics = graphics_workarounds(&mode, gamescope, from_steam, nvidia, wayland, appimage, has_hardware_rendering());
    if graphics.explicit_sync_off && std::env::var_os("__NV_DISABLE_EXPLICIT_SYNC").is_none() {
        std::env::set_var("__NV_DISABLE_EXPLICIT_SYNC", "1");
        std::env::set_var(OWN_NV_SYNC, "1");
    }
    if graphics.dmabuf_off && std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
    if graphics.compositing_off && std::env::var_os("WEBKIT_DISABLE_COMPOSITING_MODE").is_none() {
        std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
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
    // Steam's overlay, kept aside for the game: Steam finds the game's window through it (Steam
    // Link and Remote Play stream that window; without it they show the desktop, often black).
    let overlay = steam_only(&preload, &["gameoverlayrenderer"]);
    if !overlay.is_empty() {
        std::env::set_var("RL_STEAM_OVERLAY", &overlay);
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

/// The launcher's memory over a long session. glibc gives each thread that allocates its own
/// 64 MB "arena" and keeps the freed parts of it for the rest of the run, so a session that
/// opened a few game pages held tens of MB per worker thread for nothing. Cap the arenas (the
/// launcher is not allocation-bound, so a couple are plenty) and give the freed parts back to
/// the system now and then.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub fn tune_allocator() {
    // glibc's own names, from <malloc.h>: M_ARENA_MAX is -8.
    extern "C" {
        fn mallopt(param: i32, value: i32) -> i32;
        fn malloc_trim(pad: usize) -> i32;
    }
    const M_ARENA_MAX: i32 = -8;
    unsafe { mallopt(M_ARENA_MAX, 2) };
    // A game page asks the core for the catalog, and the burst it leaves in the arenas stays
    // until something trims them: once a minute is often enough to keep a session small.
    std::thread::Builder::new().name("malloc-trim".into()).spawn(|| loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
        unsafe { malloc_trim(0) };
    }).ok();
}

/// Windows and macOS give memory back on their own; glibc is the one that keeps it.
#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
pub fn tune_allocator() {}

/// Settings → Appearance → Graphics Mode, read from library.json before the window opens.
#[cfg(target_os = "linux")]
fn graphics_mode(data: &std::path::Path) -> String {
    std::fs::read(data.join("library.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v.pointer("/settings/linux_graphics").and_then(|m| m.as_str()).map(str::to_string))
        .unwrap_or_default()
}

#[cfg(any(test, target_os = "linux"))]
#[derive(Debug, Default, PartialEq)]
struct GraphicsWorkarounds {
    dmabuf_off: bool,
    compositing_off: bool,
    explicit_sync_off: bool,
}

/// Direct NVIDIA Wayland launches keep acceleration, with the narrower explicit-sync fix.
/// Steam/Gamescope, NVIDIA on X11, and AppImages without a render node retain their fallback.
/// Faster and Safer remain explicit choices; environment overrides are applied at startup.
#[cfg(any(test, target_os = "linux"))]
fn graphics_workarounds(mode: &str, gamescope: bool, from_steam: bool, nvidia: bool, wayland: bool, appimage: bool, hardware: bool) -> GraphicsWorkarounds {
    match mode {
        "fast" => GraphicsWorkarounds::default(),
        "safe" => GraphicsWorkarounds { dmabuf_off: true, compositing_off: true, explicit_sync_off: false },
        _ if nvidia && wayland && hardware && !gamescope && !from_steam => GraphicsWorkarounds {
            explicit_sync_off: true, ..Default::default()
        },
        _ => GraphicsWorkarounds {
            dmabuf_off: gamescope || from_steam || nvidia || (appimage && !hardware),
            compositing_off: nvidia && wayland,
            explicit_sync_off: false,
        },
    }
}

/// Whether the web view can play a sound. It plays through GStreamer: an AppImage (run as one or
/// unpacked) carries its own GStreamer core and none of the plugins, and the PC's newer plugins
/// can't load into it, so the web view stops the moment a sound starts. A PC without the base
/// plugins (appsink) is the same.
#[cfg(target_os = "linux")]
pub fn sound_works() -> bool {
    static WORKS: OnceLock<bool> = OnceLock::new();
    *WORKS.get_or_init(sound_check)
}

#[cfg(target_os = "linux")]
fn sound_check() -> bool {
    let bundled = std::env::var_os("APPIMAGE").is_some()
        || std::env::var_os("APPDIR").is_some()
        || std::env::current_exe().ok().is_some_and(|exe| bundled_gstreamer(&exe));
    let var = |k: &str| std::env::var(k).unwrap_or_default();
    let mut dirs: Vec<std::path::PathBuf> = ["GST_PLUGIN_SYSTEM_PATH_1_0", "GST_PLUGIN_SYSTEM_PATH", "GST_PLUGIN_PATH_1_0", "GST_PLUGIN_PATH"]
        .iter()
        .flat_map(|k| var(k).split(':').filter(|d| !d.is_empty()).map(std::path::PathBuf::from).collect::<Vec<_>>())
        .collect();
    for d in ["/usr/lib/gstreamer-1.0", "/usr/lib64/gstreamer-1.0", "/usr/lib/x86_64-linux-gnu/gstreamer-1.0", "/usr/lib/aarch64-linux-gnu/gstreamer-1.0", "/usr/local/lib/gstreamer-1.0"] {
        dirs.push(d.into());
    }
    !bundled && has_appsink(&dirs)
}

/// The launcher sits in an unpacked AppImage: <root>/usr/bin/<launcher> beside its own
/// usr/lib/libgstreamer-1.0.so.0, or with an AppRun at <root>.
#[cfg(any(test, target_os = "linux"))]
fn bundled_gstreamer(exe: &std::path::Path) -> bool {
    let Some(root) = exe.parent().filter(|b| b.ends_with("usr/bin")).and_then(|b| b.parent()).and_then(|u| u.parent()) else { return false };
    // /usr/bin itself is an ordinary install: on Arch and others the PC's own GStreamer is in /usr/lib.
    if root.parent().is_none() {
        return false;
    }
    root.join("AppRun").exists() || root.join("usr/lib/libgstreamer-1.0.so.0").exists()
}

#[cfg(any(test, target_os = "linux"))]
fn has_appsink(dirs: &[std::path::PathBuf]) -> bool {
    dirs.iter().any(|d| d.join("libgstapp.so").is_file())
}

/// A DRM render node means WebKit can draw through the GPU's DMA-BUF path. Without one (a VM,
/// a remote display, software rendering) the default renderer has nothing to bind to. A node
/// that can't be opened (a sandbox, a strict permission) is no use either.
#[cfg(target_os = "linux")]
fn has_hardware_rendering() -> bool {
    let software = |k: &str, bad: &[&str]| std::env::var(k).map(|v| bad.contains(&v.as_str())).unwrap_or(false);
    if software("LIBGL_ALWAYS_SOFTWARE", &["1", "true", "yes"]) || software("GALLIUM_DRIVER", &["llvmpipe", "softpipe", "swrast"]) {
        return false;
    }
    std::fs::read_dir("/dev/dri")
        .map(|dir| {
            dir.flatten().any(|e| {
                let name = e.file_name();
                name.to_string_lossy().starts_with("renderD") && std::fs::OpenOptions::new().read(true).write(true).open(e.path()).is_ok()
            })
        })
        .unwrap_or(false)
}

/// The entries of a preload list that are Steam's own (its overlay), space-separated.
pub fn steam_only(list: &str, keep: &[&str]) -> String {
    list.split([' ', ':']).filter(|e| !e.is_empty() && keep.iter().any(|d| e.contains(d))).collect::<Vec<_>>().join(" ")
}

/// Only Steam's launch identity and saved overlay cross the single-instance handoff. Library
/// paths, preload lists and unrelated variables still come from the receiving launcher's cleanup.
pub type SteamLaunch = std::collections::BTreeMap<String, String>;
const STEAM_VARS: [&str; 8] = [
    "RL_STEAM_OVERLAY", "SteamAppId", "SteamGameId", "SteamOverlayGameId", "SteamClientLaunch",
    "SteamEnv", "STEAM_COMPAT_CLIENT_INSTALL_PATH", "ENABLE_VK_LAYER_VALVE_steam_overlay_1",
];
const STEAM_ARG: &str = "--steam-launch=";

fn steam_launch_from(get: &dyn Fn(&str) -> Option<String>) -> Option<SteamLaunch> {
    let launch: SteamLaunch = STEAM_VARS.iter().filter_map(|k| {
        get(k).filter(|v| !v.is_empty() && !v.contains('\0')).map(|v| (k.to_string(), v))
    }).collect();
    // A graphics override alone doesn't make this a Steam shortcut.
    STEAM_VARS[..6].iter().any(|k| launch.contains_key(*k)).then_some(launch)
}

pub fn steam_launch_from_args(args: &[String]) -> Option<SteamLaunch> {
    if !args.iter().any(|a| a == "--handoff") {
        return None;
    }
    let payload = args.iter().find_map(|a| a.strip_prefix(STEAM_ARG))?;
    if payload.len() > 32768 {
        return None;
    }
    let values: SteamLaunch = serde_json::from_str(payload).ok()?;
    steam_launch_from(&|k| values.get(k).cloned())
}

/// The copy making a Steam shortcut hands its environment to the existing launcher as well as
/// its game ID. The receiving process may have been opened from the desktop, outside Steam.
pub fn play_handoff_args(args: &[String], get: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    let mut rest: Vec<String> = args.iter().skip(1).filter(|a| !a.starts_with(STEAM_ARG)).cloned().collect();
    rest.push("--handoff".into());
    if let Some(payload) = steam_launch_from(get).and_then(|s| serde_json::to_string(&s).ok()).filter(|s| s.len() <= 32768) {
        rest.push(format!("{STEAM_ARG}{payload}"));
    }
    rest
}

/// A game started from a Steam shortcut gets Steam's identity and overlay back (the launcher
/// itself runs without the overlay). A handed-off context applies to this child only.
#[cfg(any(target_os = "linux", all(test, unix)))]
pub fn steam_overlay_for_game(c: &mut tokio::process::Command, launch: Option<&SteamLaunch>) {
    if let Some(launch) = launch {
        for k in &STEAM_VARS[1..] {
            match launch.get(*k) {
                Some(v) => c.env(k, v),
                None => c.env_remove(k),
            };
        }
    }
    let inherited = || std::env::var("RL_STEAM_OVERLAY").ok();
    let Some(overlay) = launch.map(|s| s.get("RL_STEAM_OVERLAY").cloned()).unwrap_or_else(inherited).filter(|o| !o.trim().is_empty()) else { return };
    // Only the overlay's own files, and only ones that are there.
    let ok: Vec<&str> = overlay.split(' ').filter(|e| {
        let p = std::path::Path::new(e);
        p.is_absolute() && p.file_name().is_some_and(|n| n == "gameoverlayrenderer.so") && !e.contains("..") && p.is_file()
    }).collect();
    if ok.is_empty() {
        return;
    }
    // Respect the command's cleaned preload list, including an explicit removal. Reading the
    // launcher's original value here would put AppImage libraries back into the game.
    let cur = c.as_std().get_envs().find(|(k, _)| *k == "LD_PRELOAD")
        .map(|(_, v)| v.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default())
        .unwrap_or_else(|| std::env::var("LD_PRELOAD").unwrap_or_default());
    let mut list: Vec<String> = cur.split([' ', ':']).filter(|e| !e.is_empty()).map(str::to_string).collect();
    for e in ok {
        if !list.iter().any(|x| x == e) {
            list.push(e.to_string());
        }
    }
    c.env("LD_PRELOAD", list.join(" "));
    let layer = launch.map(|s| s.get("ENABLE_VK_LAYER_VALVE_steam_overlay_1").cloned())
        .unwrap_or_else(|| std::env::var("ENABLE_VK_LAYER_VALVE_steam_overlay_1").ok());
    if layer.is_none() {
        c.env("ENABLE_VK_LAYER_VALVE_steam_overlay_1", "1");
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
    fn steam_shortcut_handoff_carries_its_launch_environment() {
        let env = HashMap::from([
            ("RL_STEAM_OVERLAY", "/steam/ubuntu12_64/gameoverlayrenderer.so"),
            ("SteamAppId", "123"),
            ("SteamGameId", "456"),
            ("SteamOverlayGameId", "456"),
            ("ENABLE_VK_LAYER_VALVE_steam_overlay_1", "0"),
            ("LD_LIBRARY_PATH", "/tmp/.mount_launcher/usr/lib"),
            ("HOME", "/private/home"),
        ]);
        let args = ["launcher", "--play", "metroidprime2port"].map(str::to_string);
        let forwarded = play_handoff_args(&args, &|k| env.get(k).map(|s| s.to_string()));
        assert_eq!(&forwarded[..3], ["--play", "metroidprime2port", "--handoff"]);
        let payload = forwarded.iter().find_map(|a| a.strip_prefix("--steam-launch="))
            .expect("the existing launcher needs the Steam shortcut's environment");
        let values: serde_json::Value = serde_json::from_str(payload).unwrap();
        assert_eq!(values["RL_STEAM_OVERLAY"], "/steam/ubuntu12_64/gameoverlayrenderer.so");
        assert_eq!(values["SteamAppId"], "123");
        assert_eq!(values["SteamGameId"], "456");
        assert_eq!(values["SteamOverlayGameId"], "456");
        assert_eq!(values["ENABLE_VK_LAYER_VALVE_steam_overlay_1"], "0");
        assert!(values.get("HOME").is_none());
        assert!(values.get("LD_LIBRARY_PATH").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn handed_off_steam_environment_applies_only_to_its_game() {
        let root = std::env::temp_dir().join(format!("rl-steam-handoff-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let overlay = root.join("gameoverlayrenderer.so");
        std::fs::write(&overlay, b"").unwrap();
        let args = vec!["--play".into(), "metroidprime2port".into(), "--handoff".into(), format!("--steam-launch={}", serde_json::json!({
            "RL_STEAM_OVERLAY": overlay,
            "SteamAppId": "123",
            "SteamGameId": "456",
            "ENABLE_VK_LAYER_VALVE_steam_overlay_1": "0",
            "HOME": "/unwanted",
            "LD_LIBRARY_PATH": "/unwanted/lib",
        }))];
        let launch = steam_launch_from_args(&args).expect("Steam context must survive a single-instance handoff");
        let mut game = tokio::process::Command::new("game");
        game.env("LD_PRELOAD", "/user/libextra.so").env("SteamAppId", "old").env("SteamOverlayGameId", "old");
        steam_overlay_for_game(&mut game, Some(&launch));
        let env: HashMap<_, _> = game.as_std().get_envs().map(|(k, v)| (k.to_string_lossy().into_owned(), v.map(|s| s.to_string_lossy().into_owned()))).collect();
        assert_eq!(env["LD_PRELOAD"], Some(format!("/user/libextra.so {}", overlay.display())));
        assert_eq!(env["SteamAppId"].as_deref(), Some("123"));
        assert_eq!(env["SteamGameId"].as_deref(), Some("456"));
        assert_eq!(env["SteamOverlayGameId"], None);
        assert_eq!(env["ENABLE_VK_LAYER_VALVE_steam_overlay_1"].as_deref(), Some("0"));
        assert!(!env.contains_key("HOME"));
        assert!(!env.contains_key("LD_LIBRARY_PATH"));
        assert!(!launch.contains_key("HOME"));
        assert!(!launch.contains_key("LD_LIBRARY_PATH"));
        // An explicit cleanup stays removed; only the verified overlay is added back, once.
        let mut default_layer = launch.clone();
        default_layer.remove("ENABLE_VK_LAYER_VALVE_steam_overlay_1");
        let mut clean = tokio::process::Command::new("game");
        clean.env_remove("LD_PRELOAD");
        steam_overlay_for_game(&mut clean, Some(&default_layer));
        steam_overlay_for_game(&mut clean, Some(&default_layer));
        let env: HashMap<_, _> = clean.as_std().get_envs().map(|(k, v)| (k.to_string_lossy().into_owned(), v.map(|s| s.to_string_lossy().into_owned()))).collect();
        assert_eq!(env["LD_PRELOAD"].as_deref(), overlay.to_str());
        assert_eq!(env["ENABLE_VK_LAYER_VALVE_steam_overlay_1"].as_deref(), Some("1"));
        // A later desktop start has no inherited context from the previous shortcut.
        let desktop = play_handoff_args(&["launcher".into(), "--play".into(), "another".into()], &|_| None);
        assert!(steam_launch_from_args(&desktop).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_steam_handoffs_do_not_change_the_launch() {
        for args in [
            vec![r#"--steam-launch={"SteamGameId":"123"}"#.to_string()],
            vec!["--handoff".into(), "--steam-launch=not-json".into()],
            vec!["--handoff".into(), r#"--steam-launch={"HOME":"/unwanted"}"#.into()],
            vec!["--handoff".into(), r#"--steam-launch={"SteamGameId":"bad\u0000id"}"#.into()],
        ] {
            assert!(steam_launch_from_args(&args).is_none(), "{args:?}");
        }
    }

    #[test]
    fn steam_entries_are_taken_out() {
        assert_eq!(super::steam_free("/tmp/.mount_x/usr/lib:/home/deck/.steam/root/ubuntu12_32/steam-runtime/lib:/usr/lib", ':', &["steam-runtime", "ubuntu12_32"]), "/tmp/.mount_x/usr/lib:/usr/lib");
        assert_eq!(super::steam_free("/home/deck/.local/share/Steam/ubuntu12_64/gameoverlayrenderer.so", ' ', &["gameoverlayrenderer"]), "");
        // Steam's overlay, kept aside for the game.
        assert_eq!(super::steam_only("/s/ubuntu12_32/gameoverlayrenderer.so:/s/ubuntu12_64/gameoverlayrenderer.so /x/libother.so", &["gameoverlayrenderer"]), "/s/ubuntu12_32/gameoverlayrenderer.so /s/ubuntu12_64/gameoverlayrenderer.so");
        assert_eq!(super::steam_free("/usr/lib", ':', &["steam-runtime"]), "/usr/lib");
    }
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn auto_keeps_acceleration_on_direct_nvidia_wayland_launches() {
        let accelerated = GraphicsWorkarounds { explicit_sync_off: true, ..Default::default() };
        for appimage in [false, true] {
            assert_eq!(graphics_workarounds("", false, false, true, true, appimage, true), accelerated);
        }
        // Steam/Gamescope and a missing render node retain the old fallback, even on Wayland.
        let fallback = GraphicsWorkarounds { dmabuf_off: true, compositing_off: true, explicit_sync_off: false };
        assert_eq!(graphics_workarounds("", true, false, true, true, false, true), fallback);
        assert_eq!(graphics_workarounds("", false, true, true, true, false, true), fallback);
        assert_eq!(graphics_workarounds("", false, false, true, true, false, false), fallback);
    }

    #[test]
    fn explicit_graphics_modes_override_platform_detection() {
        for nvidia in [false, true] {
            for wayland in [false, true] {
                for steam in [false, true] {
                    assert_eq!(graphics_workarounds("fast", steam, steam, nvidia, wayland, true, true), GraphicsWorkarounds::default());
                    assert_eq!(graphics_workarounds("safe", steam, steam, nvidia, wayland, true, true), GraphicsWorkarounds {
                        dmabuf_off: true, compositing_off: true, explicit_sync_off: false,
                    });
                }
            }
        }
    }

    #[test]
    fn games_do_not_inherit_the_launchers_nvidia_sync_workaround() {
        let env = HashMap::from([(OWN_NV_SYNC, "1"), ("__NV_DISABLE_EXPLICIT_SYNC", "1")]);
        let changes = changes_from(&|k| env.get(k).map(|s| s.to_string()));
        assert!(changes.contains(&(OWN_NV_SYNC, None)));
        assert!(changes.contains(&("__NV_DISABLE_EXPLICIT_SYNC", None)));
        // An explicit player setting, without our marker, belongs to the game's environment.
        for value in ["0", "1"] {
            assert!(changes_from(&|k| (k == "__NV_DISABLE_EXPLICIT_SYNC").then(|| value.to_string())).is_empty());
        }
    }

    #[test]
    fn an_unpacked_appimage_plays_no_sound() {
        let d = std::env::temp_dir().join(format!("rl-gst-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let root = d.join("squashfs-root");
        std::fs::create_dir_all(root.join("usr/bin")).unwrap();
        std::fs::create_dir_all(root.join("usr/lib")).unwrap();
        let exe = root.join("usr/bin/recomp-launcher");
        assert!(!bundled_gstreamer(&exe));
        std::fs::write(root.join("usr/lib/libgstreamer-1.0.so.0"), b"").unwrap();
        assert!(bundled_gstreamer(&exe));
        // An ordinary install in /usr/bin isn't one.
        assert!(!bundled_gstreamer(std::path::Path::new("/usr/bin/recomp-launcher")));
        // Sounds need GStreamer's appsink plugin somewhere.
        let plugins = d.join("gst");
        std::fs::create_dir_all(&plugins).unwrap();
        assert!(!has_appsink(&[plugins.clone()]));
        std::fs::write(plugins.join("libgstapp.so"), b"").unwrap();
        assert!(has_appsink(&[d.join("none"), plugins]));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn auto_keeps_appimages_on_the_fast_path_when_a_gpu_is_there() {
        // AMD/Intel desktop AppImage: the default renderer is fine and much cheaper.
        assert_eq!(graphics_workarounds("", false, false, false, false, true, true), GraphicsWorkarounds::default());
        assert_eq!(graphics_workarounds("", false, false, false, true, true, true), GraphicsWorkarounds::default());
        // No render node (a VM, a remote display, software rendering): keep the safe renderer.
        let fallback = GraphicsWorkarounds { dmabuf_off: true, ..Default::default() };
        assert_eq!(graphics_workarounds("", false, false, false, false, true, false), fallback);
        // NVIDIA X11, Steam and Gaming Mode keep their existing transport workaround.
        assert_eq!(graphics_workarounds("", false, false, true, false, true, true), fallback);
        assert_eq!(graphics_workarounds("", false, true, false, false, false, true), fallback);
        assert_eq!(graphics_workarounds("", true, false, false, false, false, true), fallback);
    }

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

/// Whether this Linux has FUSE 2 (libfuse.so.2), which most AppImages need to mount themselves.
#[cfg(target_os = "linux")]
pub fn has_fuse2() -> bool {
    let dirs = ["/usr/lib", "/usr/lib64", "/lib", "/lib64", "/usr/lib/x86_64-linux-gnu", "/lib/x86_64-linux-gnu", "/usr/lib/aarch64-linux-gnu", "/run/host/usr/lib", "/run/host/usr/lib64"];
    dirs.iter().any(|d| std::path::Path::new(d).join("libfuse.so.2").exists())
}

//! Android: the games are Android apps. They're downloaded from the release the website's Android
//! list names (`android_tag`, `assets.android`), checked against what the list says (package name,
//! signing certificate), and installed, opened and uninstalled through Android itself
//! (`ApkPlugin.kt` in the Android project). The Library follows what's installed on the device.

use crate::catalog::{self, Release};
use crate::install::{self, Progress};
use crate::library::{self, Installed};
use crate::{str_of, App, Error, Res};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, OnceLock};
use tauri::{AppHandle, Emitter};

type Handle = tauri::plugin::PluginHandle<tauri::Wry>;
static APK: OnceLock<Handle> = OnceLock::new();
static SCREEN: OnceLock<Handle> = OnceLock::new();

/// The plugin that holds `ApkPlugin.kt`.
pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("apk")
        .setup(|_app, api| {
            let h = api.register_android_plugin("com.recomplauncher.app", "ApkPlugin")?;
            let _ = APK.set(h);
            Ok(())
        })
        .build()
}

/// The plugin that holds `ScreenPlugin.kt`: the launcher's own window.
pub fn screen_plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("screen")
        .setup(|_app, api| {
            let h = api.register_android_plugin("com.recomplauncher.app", "ScreenPlugin")?;
            let _ = SCREEN.set(h);
            Ok(())
        })
        .build()
}

/// Which way the window turns: Big Picture mode only sideways, the phone layout as the phone is
/// held (and as its rotation lock says).
pub fn orientation(landscape: bool) -> Res<()> {
    screen_call("orientation", json!({ "landscape": landscape }))
}

/// The phone layout draws under the clock bar and beside the camera hole; Big Picture mode doesn't.
pub fn edges(phone: bool, light: bool) -> Res<()> {
    screen_call("edges", json!({ "phone": phone, "light": light }))
}

/// The room the system takes at each edge of the window, as the page measures: `{top, right, bottom, left}`.
pub fn insets() -> Res<Value> {
    let h = SCREEN.get().ok_or_else(|| Error::Msg("The window isn't ready yet.".into()))?;
    h.run_mobile_plugin::<Value>("insets", json!({})).map_err(|e| Error::Msg(e.to_string()))
}

/// What kind of screen the launcher is on: `{sw, pad}`, the screen's smaller side in the page's
/// pixels (600 and up is a tablet) and whether it has a controller built in (a handheld).
pub fn device() -> Res<Value> {
    let h = SCREEN.get().ok_or_else(|| Error::Msg("The window isn't ready yet.".into()))?;
    h.run_mobile_plugin::<Value>("device", json!({})).map_err(|e| Error::Msg(e.to_string()))
}

/// A light tick under the finger.
pub fn haptic(kind: &str) -> Res<()> {
    screen_call("haptic", json!({ "kind": kind }))
}

/// Back on the phone layout's Home: the launcher goes to the background, as other apps do.
pub fn to_back() -> Res<()> {
    screen_call("toBack", json!({}))
}

fn screen_call(cmd: &str, args: Value) -> Res<()> {
    let h = SCREEN.get().ok_or_else(|| Error::Msg("The window isn't ready yet.".into()))?;
    h.run_mobile_plugin::<Value>(cmd, args).map(|_| ()).map_err(|e| Error::Msg(e.to_string()))
}

/// One of `ApkPlugin`'s commands. It waits for Android's answer, so never on the async threads.
fn call<T: serde::de::DeserializeOwned>(cmd: &str, args: Value) -> Res<T> {
    let h = APK.get().ok_or_else(|| Error::Msg("Android's installer isn't ready yet.".into()))?;
    h.run_mobile_plugin(cmd, args).map_err(|e| {
        let m = e.to_string();
        if m.contains("CANCELLED") {
            Error::Cancelled
        } else {
            Error::Msg(m)
        }
    })
}

async fn call_async<T: serde::de::DeserializeOwned + Send + 'static>(cmd: &'static str, args: Value) -> Res<T> {
    tokio::task::spawn_blocking(move || call(cmd, args)).await.map_err(|e| Error::Msg(e.to_string()))?
}

/// Which of these packages are installed, with their version code.
fn installed(pkgs: &[String]) -> HashMap<String, i64> {
    if pkgs.is_empty() {
        return HashMap::new();
    }
    let v: Value = call("installed", json!({ "pkgs": pkgs })).unwrap_or(Value::Null);
    v.as_object().map(|o| o.iter().map(|(k, x)| (k.clone(), x.get("versionCode").and_then(Value::as_i64).unwrap_or(0))).collect()).unwrap_or_default()
}

fn list_on_disk(st: &App) -> Value {
    catalog::list_on_disk(&st.dir).unwrap_or(Value::Null)
}

fn package_of(st: &App, id: &str) -> Res<String> {
    let cat = list_on_disk(st);
    catalog::app(&cat, id).map(|e| str_of(e, "android_package").to_string()).filter(|p| !p.is_empty()).ok_or_else(|| Error::Msg("The website's list has no Android app for this game.".into()))
}

/// Downloads the game's APK, checks it, and has Android install it (or update it). Called from
/// `do_install` in place of the PC's unpacking.
pub async fn install(app: &AppHandle, st: &App, id: &str, cancel: Arc<AtomicBool>) -> Res<()> {
    let report = |p: Progress| {
        let _ = app.emit("install", p);
    };
    report(Progress::new(id, "prepare", 0, 0));
    let cat = crate::getting_ready(catalog::catalog(&st.http, &st.dir, false), &cancel).await?.data;
    let entry = catalog::app(&cat, id).ok_or_else(|| Error::Msg("This game isn't listed anymore.".into()))?.clone();
    let repo = str_of(&entry, "repo").to_string();
    let tag = str_of(&entry, "tag").to_string();
    let pattern = entry.get("assets").and_then(|a| a.get("android")).and_then(Value::as_str).unwrap_or("").to_string();
    if pattern.is_empty() {
        return Err(Error::Msg("This game has no Android version yet.".into()));
    }
    crate::trail::note(format!("Install {id} (Android): {repo} {tag}"));
    let rels: Vec<Release> = serde_json::from_value(crate::getting_ready(catalog::releases_within(&st.http, &st.dir, &repo, 600), &cancel).await?.data)?;
    let rel = rels.iter().find(|r| crate::same_tag(&r.tag_name, &tag)).ok_or_else(|| Error::Msg(format!("Its release ({tag}) isn't on its project anymore.")))?;
    let asset = catalog::android_asset(&rel.assets, &pattern).ok_or_else(|| Error::Msg(format!("Its release ({tag}) has no Android app in it anymore.")))?.clone();
    let cache = st.dir.join("cache/apk");
    std::fs::create_dir_all(&cache)?;
    let got = install::download(&st.http, &asset, &cache, id, cancel.clone(), &report).await?;
    let cleanup = |e: Error| {
        let _ = std::fs::remove_file(&got.file);
        e
    };
    // What the file is, against what the list says: the same app, signed by the same developer.
    report(Progress::new(id, "verify", 0, 0));
    let path = got.file.to_string_lossy().to_string();
    let read: Value = call_async("read", json!({ "path": path })).await.map_err(cleanup)?;
    let (want_pkg, want_sig) = (str_of(&entry, "android_package"), str_of(&entry, "android_sig"));
    if let Some(why) = catalog::apk_mismatch(&read, want_pkg, want_sig) {
        crate::trail::note(format!("Install {id} (Android): {why}"));
        return Err(cleanup(Error::Msg("This download isn't the game the store checked (a different app, or signed by someone else). It wasn't installed.".into())));
    }
    let ok: Value = call_async("canInstall", json!({})).await.map_err(cleanup)?;
    if !ok.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        let _ = std::fs::remove_file(&got.file);
        return Err(Error::Msg("Allow RecompLauncher to install apps in the settings that opened, then press Install again.".into()));
    }
    crate::trail::note(format!("Install {id} (Android): asking Android to install {}", asset.name));
    let r: Res<Value> = call_async("install", json!({ "path": path })).await;
    let _ = std::fs::remove_file(&got.file);
    r?;
    // In the Library like any game: its folder holds nothing (Android keeps the app), so it's made
    // empty in the launcher's own library folder.
    let prev = st.lib.lock().unwrap().installed.get(id).cloned();
    let lib_dir = st.lib.lock().unwrap().settings.library_dir.clone();
    let folder = lib_dir.join(library::folder_name(id));
    let _ = std::fs::create_dir_all(&folder);
    let now = library::now();
    let rec = Installed {
        id: id.into(),
        name: str_of(&entry, "name").into(),
        repo,
        tag,
        version: str_of(&entry, "version").into(),
        path: folder,
        asset: asset.name.clone(),
        size: asset.size,
        sha256: got.sha256,
        verified: got.verified,
        installed_at: prev.as_ref().map(|p| p.installed_at).unwrap_or(now),
        updated_at: now,
        played_secs: prev.as_ref().map(|p| p.played_secs).unwrap_or(0),
        last_played: prev.as_ref().map(|p| p.last_played).unwrap_or(0),
        sessions: prev.as_ref().map(|p| p.sessions.clone()).unwrap_or_default(),
        setup_checked: true,
        ..Default::default()
    };
    {
        let mut l = st.lib.lock().unwrap();
        l.put(id, rec);
        l.settings.asked_location = true;
    }
    let _ = st.try_save();
    st.jobs.lock().unwrap().remove(id);
    let mut p = Progress::new(id, "done", 1, 1);
    p.msg = Some("Installed by Android".into());
    report(p);
    Ok(())
}

/// Opens the installed game. A game with save places in shared storage gets a session, finished
/// (its saves copied, and sent with cloud saves on) when the launcher comes back to the front:
/// Android doesn't say when another app closes.
pub fn open(st: &App, id: &str) -> Res<()> {
    let pkg = package_of(st, id)?;
    call::<Value>("open", json!({ "pkg": pkg }))?;
    let mut l = st.lib.lock().unwrap();
    let g = l.installed.get_mut(id).map(|g| {
        g.last_played = library::now();
        g.clone()
    });
    drop(l);
    st.save();
    if let Some(g) = g {
        if !crate::cloud::spots_of(st, id, &g).is_empty() {
            crate::cloud::set_after_play(&st.dir, id, true);
        }
    }
    Ok(())
}

/// Whether the player has allowed All files access, which cloud saves need for a game's folders in
/// shared storage: `{granted, root, device}`. Keeps the shared storage folder for the save code
/// (`saves::set_storage`) and the phone's name for its copies.
pub fn files_access() -> Res<Value> {
    let v: Value = call("filesAccess", json!({}))?;
    let granted = v.get("granted").and_then(Value::as_bool).unwrap_or(false);
    let root = v.get("root").and_then(Value::as_str).unwrap_or("");
    crate::saves::set_storage(if granted && !root.is_empty() { Some(root.into()) } else { None });
    crate::saves::set_phone_name(v.get("device").and_then(Value::as_str).unwrap_or(""));
    Ok(json!({ "granted": granted }))
}

/// Opens Android's All files access page for the launcher.
pub fn ask_files_access() -> Res<Value> {
    call("askFilesAccess", json!({}))
}

/// Android's own Uninstall question. The game leaves the Library now; if the player said no, the
/// next look at what's installed (`sync`) puts it back.
pub fn uninstall(st: &App, id: &str) -> Res<Value> {
    let pkg = package_of(st, id)?;
    call::<Value>("uninstall", json!({ "pkg": pkg }))?;
    if let Some(g) = st.lib.lock().unwrap().forget(id) {
        let _ = std::fs::remove_dir(&g.path);
    }
    st.save();
    Ok(json!({ "left": 0, "android": true }))
}

/// The Library as Android has it: a game installed from its list (by this launcher or not) is in
/// it, one uninstalled outside the launcher isn't. True when the Library changed.
pub fn sync(st: &App) -> bool {
    let cat = list_on_disk(st);
    let apps: Vec<&Value> = cat.get("apps").and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default();
    let by_pkg: HashMap<String, &Value> = apps.iter().filter_map(|e| Some((str_of(e, "android_package").to_string(), *e)).filter(|(p, _)| !p.is_empty())).collect();
    let pkgs: Vec<String> = by_pkg.keys().cloned().collect();
    if pkgs.is_empty() {
        return false;
    }
    let on_device = installed(&pkgs);
    let mut changed = false;
    let lib_dir = st.lib.lock().unwrap().settings.library_dir.clone();
    let mut l = st.lib.lock().unwrap();
    for (pkg, e) in &by_pkg {
        let id = str_of(e, "id");
        let here = on_device.contains_key(pkg);
        let listed = l.installed.contains_key(id);
        if here && !listed {
            // Installed outside the launcher (or an uninstall the player said no to).
            let folder = lib_dir.join(library::folder_name(id));
            let _ = std::fs::create_dir_all(&folder);
            let current = on_device.get(pkg).copied().unwrap_or(0) >= e.get("android_version_code").and_then(Value::as_i64).unwrap_or(i64::MAX);
            l.put(id, Installed {
                id: id.into(),
                name: str_of(e, "name").into(),
                repo: str_of(e, "repo").into(),
                // An older version than the list's shows as an update.
                tag: if current { str_of(e, "tag").into() } else { String::new() },
                version: if current { str_of(e, "version").into() } else { String::new() },
                path: folder,
                installed_at: library::now(),
                updated_at: library::now(),
                setup_checked: true,
                ..Default::default()
            });
            changed = true;
        } else if !here && listed && !st.jobs.lock().unwrap().contains_key(id) {
            if let Some(g) = l.forget(id) {
                let _ = std::fs::remove_dir(&g.path);
            }
            changed = true;
        }
    }
    drop(l);
    if changed {
        st.save();
    }
    changed
}


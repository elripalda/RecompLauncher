//! While a game runs: the hotkeys, the quick menu over the game, and screenshots. Controllers are
//! read in pads.rs. On the keyboard, F12 takes a screenshot and Ctrl+F12 opens the quick menu, held
//! only while a game is running.

use crate::{App, Error};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager};

/// Where a game's screenshots go: Pictures\RecompLauncher\<game>.
pub fn shots_dir(name: &str) -> PathBuf {
    let safe: String = name.chars().map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { ' ' } else { c }).collect();
    dirs::picture_dir().unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join("Pictures")).join("RecompLauncher").join(safe.trim())
}

/// A game's screenshots, newest first.
pub fn list_shots(name: &str) -> Vec<PathBuf> {
    let mut v: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(shots_dir(name))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().and_then(|x| x.to_str()).map(|x| x.eq_ignore_ascii_case("png")).unwrap_or(false))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v.into_iter().map(|(_, p)| p).collect()
}

/// Capture the game's window (or, failing that, the screen it's on) to a PNG.
#[cfg(windows)]
pub fn capture(pid: Option<u32>, name: &str) -> Result<PathBuf, Error> {
    use xcap::{Monitor, Window};
    let fail = |e: xcap::XCapError| Error::Msg(format!("Couldn't take the screenshot: {e}"));
    let windows = Window::all().map_err(fail)?;
    let ours = std::process::id();
    let pick = windows
        .iter()
        .filter(|w| !w.is_minimized().unwrap_or(true) && w.width().unwrap_or(0) > 200 && w.pid().ok() != Some(ours))
        .find(|w| pid.is_some() && w.pid().ok() == pid)
        .or_else(|| windows.iter().find(|w| w.is_focused().unwrap_or(false) && w.pid().ok() != Some(ours)));
    let img = match pick {
        Some(w) => match w.capture_image() {
            // Some games draw nothing a window capture can see: take that part of the screen instead.
            Ok(i) if i.pixels().any(|p| p[0] > 8 || p[1] > 8 || p[2] > 8) => i,
            _ => {
                let m = w.current_monitor().map_err(fail)?;
                let (mx, my) = (m.x().unwrap_or(0), m.y().unwrap_or(0));
                let x = (w.x().unwrap_or(0) - mx).max(0) as u32;
                let y = (w.y().unwrap_or(0) - my).max(0) as u32;
                let width = w.width().unwrap_or(1).min(m.width().unwrap_or(1).saturating_sub(x));
                let height = w.height().unwrap_or(1).min(m.height().unwrap_or(1).saturating_sub(y));
                m.capture_region(x, y, width.max(1), height.max(1)).map_err(fail)?
            }
        },
        None => Monitor::all().map_err(fail)?.into_iter().find(|m| m.is_primary().unwrap_or(false)).ok_or_else(|| Error::Msg("No screen to capture.".into()))?.capture_image().map_err(fail)?,
    };
    let dir = shots_dir(name);
    std::fs::create_dir_all(&dir)?;
    let file = dir.join(format!("{}.png", chrono::Local::now().format("%Y-%m-%d %H-%M-%S")));
    img.save(&file).map_err(|e| Error::Msg(format!("Couldn't save the screenshot: {e}")))?;
    Ok(file)
}

#[cfg(not(windows))]
pub fn capture(_pid: Option<u32>, _name: &str) -> Result<PathBuf, Error> {
    Err(Error::Msg("Screenshots come to Linux in a later build.".into()))
}

/// The game playing right now (the most recently started one).
fn current(app: &AppHandle) -> Option<(String, String, Option<u32>, u64)> {
    let st = app.state::<App>();
    let ids: Vec<String> = st.running.lock().unwrap().keys().cloned().collect();
    let started = st.started.lock().unwrap().clone();
    let id = ids.into_iter().max_by_key(|i| started.get(i).copied().unwrap_or(0))?;
    let pid = st.pids.lock().unwrap().get(&id).copied();
    let started = st.started.lock().unwrap().get(&id).copied().unwrap_or(0);
    let name = st.lib.lock().unwrap().installed.get(&id).map(|g| g.name.clone()).unwrap_or_else(|| id.clone());
    Some((id, name, pid, started))
}

pub fn take_screenshot(app: &AppHandle) {
    let Some((id, name, pid, _)) = current(app) else { return };
    let app2 = app.clone();
    std::thread::spawn(move || {
        // Out of the picture first, if the quick menu is up.
        let overlay_up = app2.state::<App>().overlay_open.load(Ordering::Relaxed);
        if overlay_up {
            hide_overlay(&app2);
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        let r = capture(pid, &name);
        match &r {
            Ok(file) => {
                let st = app2.state::<App>();
                if st.looked.lock().unwrap().contains(&id) {
                    crate::looks::note_shot(&st.dir, &id, file);
                }
                let _ = app2.emit("screenshot", serde_json::json!({"id": id, "file": file}));
                notify(&app2, "Screenshot saved", &format!("In {name}'s gallery"));
            }
            Err(e) => notify(&app2, "Screenshot didn't work", &e.to_string()),
        }
        if overlay_up {
            show_overlay(&app2);
        }
    });
}

pub fn notify(app: &AppHandle, title: &str, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    let _ = app.notification().builder().title(title).body(body).show();
}

pub fn show_overlay(app: &AppHandle) {
    let Some((id, name, _, started)) = current(app) else { return };
    let Some(w) = app.get_webview_window("overlay") else { return };
    if let Ok(Some(m)) = app.primary_monitor() {
        let _ = w.set_position(*m.position());
        let _ = w.set_size(*m.size());
    }
    let st = app.state::<App>();
    st.overlay_open.store(true, Ordering::Relaxed);
    let played = st.lib.lock().unwrap().installed.get(&id).map(|g| g.played_secs).unwrap_or(0);
    crate::rest(&w, false);
    let _ = w.show();
    let _ = w.set_always_on_top(true);
    let _ = w.set_focus();
    let _ = app.emit_to("overlay", "overlay-open", serde_json::json!({
        "id": id, "name": name, "session": crate::library::now().saturating_sub(started), "played": played,
        "shots": list_shots(&name).len()
    }));
}

pub fn hide_overlay(app: &AppHandle) {
    app.state::<App>().overlay_open.store(false, Ordering::Relaxed);
    if let Some(w) = app.get_webview_window("overlay") {
        let _ = w.hide();
        crate::rest(&w, true);
    }
}

pub fn toggle_overlay(app: &AppHandle) {
    if app.state::<App>().overlay_open.load(Ordering::Relaxed) {
        hide_overlay(app);
    } else {
        show_overlay(app);
    }
}

/// F12 and Ctrl+F12, only while a game runs, so they never get in the way otherwise.
pub fn set_hotkeys(app: &AppHandle, on: bool) {
    use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    if !on {
        return;
    }
    let shot = Shortcut::new(None, Code::F12);
    let menu = Shortcut::new(Some(Modifiers::CONTROL), Code::F12);
    let _ = gs.on_shortcut(shot, |app, _, e| {
        if e.state() == ShortcutState::Pressed {
            take_screenshot(app);
        }
    });
    let _ = gs.on_shortcut(menu, |app, _, e| {
        if e.state() == ShortcutState::Pressed {
            toggle_overlay(app);
        }
    });
}

pub fn shot_paths(name: &str) -> Vec<String> {
    list_shots(name).iter().map(|p: &PathBuf| p.to_string_lossy().to_string()).collect()
}

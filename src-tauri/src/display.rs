//! Monitor selection and window placement for the launcher and Big Picture mode.

use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

/// A connected monitor, identified by its name and position in the desktop layout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorInfo {
    pub id: String,
    pub name: String,
    pub is_primary: bool,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone)]
struct WindowState {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    maximized: bool,
    monitor_id: String,
}

static BIG_PICTURE_WINDOW: Mutex<Option<WindowState>> = Mutex::new(None);

/// Returns the monitors Tauri currently reports as connected.
pub fn enumerate_monitors(app: &AppHandle) -> Vec<MonitorInfo> {
    let Some(window) = app.get_webview_window("main") else {
        return Vec::new();
    };
    let Ok(mut monitors) = window.available_monitors() else {
        return Vec::new();
    };
    let primary = window.primary_monitor().ok().flatten();
    monitors.sort_by_key(|monitor| {
        let position = monitor.position();
        (
            position.x,
            position.y,
            monitor.name().cloned().unwrap_or_default(),
        )
    });

    let mut result: Vec<MonitorInfo> = monitors
        .into_iter()
        .enumerate()
        .map(|(index, monitor)| {
            let position = monitor.position();
            let size = monitor.size();
            let id = format!(
                "{}|{},{}",
                monitor.name().map(String::as_str).unwrap_or_default(),
                position.x,
                position.y
            );
            let is_primary = primary
                .as_ref()
                .map(|p| p.position() == position && p.size() == size)
                .unwrap_or(false);

            MonitorInfo {
                id,
                name: format!("Monitor {}", index + 1),
                is_primary,
                width: size.width,
                height: size.height,
                x: position.x,
                y: position.y,
            }
        })
        .collect();

    if !result.is_empty() && !result.iter().any(|monitor| monitor.is_primary) {
        result[0].is_primary = true;
    }
    result
}

/// Resolves a saved selection, falling back to the current primary monitor if it is unavailable.
pub fn resolve_target_monitor(
    monitors: &[MonitorInfo],
    preference: Option<&str>,
) -> Option<MonitorInfo> {
    if monitors.is_empty() {
        return None;
    }

    let preference = preference.unwrap_or("").trim();
    if !preference.is_empty()
        && !preference.eq_ignore_ascii_case("auto")
        && !preference.eq_ignore_ascii_case("primary")
    {
        if let Some(monitor) = monitors.iter().find(|monitor| {
            monitor.id.eq_ignore_ascii_case(preference)
                || monitor
                    .id
                    .split_once('|')
                    .map(|(native_name, _)| native_name.eq_ignore_ascii_case(preference))
                    .unwrap_or(false)
                || monitor.name.eq_ignore_ascii_case(preference)
        }) {
            return Some(monitor.clone());
        }
        crate::trail::note(format!(
            "Selected monitor '{preference}' is unavailable; using the primary display"
        ));
    }

    monitors
        .iter()
        .find(|monitor| monitor.is_primary)
        .cloned()
        .or_else(|| monitors.first().cloned())
}

/// Saves the normal window state once, moves the window, then lets the UI enable fullscreen.
pub fn prepare_tv_mode(app: &AppHandle, preference: Option<String>) -> Result<MonitorInfo, String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window not found".to_string())?;
    let monitors = enumerate_monitors(app);
    let target = resolve_target_monitor(&monitors, preference.as_deref())
        .ok_or_else(|| "No display monitors detected".to_string())?;

    let first_entry = {
        let mut active = BIG_PICTURE_WINDOW.lock().unwrap();
        let first_entry = active.is_none();
        if first_entry {
            remember_window_state(&mut active, capture_window_state(app, &window, &monitors)?);
        }
        first_entry
    };

    if let Err(error) = place_window_at_monitor(&window, &target) {
        if first_entry {
            let _ = leave_tv_mode(app);
        }
        return Err(error);
    }
    Ok(target)
}

/// Restores the window's monitor, size, and maximized state after Big Picture mode.
pub fn leave_tv_mode(app: &AppHandle) -> Result<(), String> {
    let Some(state) = BIG_PICTURE_WINDOW.lock().unwrap().clone() else {
        if let Some(window) = app.get_webview_window("main") {
            window
                .set_fullscreen(false)
                .map_err(|e| format!("Couldn't leave fullscreen mode: {e}"))?;
        }
        return Ok(());
    };
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window not found".to_string())?;
    window
        .set_fullscreen(false)
        .map_err(|e| format!("Couldn't leave fullscreen mode: {e}"))?;
    if window.is_maximized().unwrap_or(false) {
        window
            .unmaximize()
            .map_err(|e| format!("Couldn't restore the launcher window: {e}"))?;
    }

    let monitors = enumerate_monitors(app);
    let (x, y) = if monitors
        .iter()
        .any(|monitor| monitor.id == state.monitor_id)
    {
        (state.x, state.y)
    } else if let Some(monitor) = monitors
        .iter()
        .find(|monitor| monitor.is_primary)
        .or_else(|| monitors.first())
    {
        crate::trail::note(
            "The launcher's original monitor is unavailable; restoring on the primary display",
        );
        (
            monitor
                .x
                .saturating_add(monitor.width.saturating_sub(state.width) as i32 / 2),
            monitor
                .y
                .saturating_add(monitor.height.saturating_sub(state.height) as i32 / 2),
        )
    } else {
        (state.x, state.y)
    };

    window
        .set_size(PhysicalSize::new(state.width, state.height))
        .map_err(|e| format!("Couldn't restore the launcher size: {e}"))?;
    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|e| format!("Couldn't restore the launcher position: {e}"))?;
    if state.maximized {
        window
            .maximize()
            .map_err(|e| format!("Couldn't restore the maximized state: {e}"))?;
    }
    *BIG_PICTURE_WINDOW.lock().unwrap() = None;
    Ok(())
}

fn capture_window_state(
    app: &AppHandle,
    window: &WebviewWindow,
    monitors: &[MonitorInfo],
) -> Result<WindowState, String> {
    window
        .set_fullscreen(false)
        .map_err(|e| format!("Couldn't leave fullscreen mode: {e}"))?;
    let maximized = window.is_maximized().unwrap_or(false);
    let saved_place = maximized
        .then(|| {
            app.try_state::<crate::App>()
                .and_then(|state| std::fs::read(state.dir.join("window.json")).ok())
                .and_then(|bytes| serde_json::from_slice::<crate::WindowPlace>(&bytes).ok())
                .filter(|place| place.w >= 400 && place.h >= 300)
        })
        .flatten();
    if maximized {
        window
            .unmaximize()
            .map_err(|e| format!("Couldn't prepare the launcher window: {e}"))?;
        if saved_place.is_none() {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    let captured: Result<WindowState, String> = (|| {
        let (x, y, width, height) = if let Some(place) = saved_place {
            (place.x, place.y, place.w, place.h)
        } else {
            let position = window
                .outer_position()
                .map_err(|e| format!("Couldn't read the launcher position: {e}"))?;
            let size = window
                .outer_size()
                .map_err(|e| format!("Couldn't read the launcher size: {e}"))?;
            (position.x, position.y, size.width, size.height)
        };
        let monitor_id = monitor_at(monitors, x, y, width, height)
            .or_else(|| monitors.iter().find(|monitor| monitor.is_primary))
            .or_else(|| monitors.first())
            .map(|monitor| monitor.id.clone())
            .unwrap_or_default();

        Ok(WindowState {
            x,
            y,
            width,
            height,
            maximized,
            monitor_id,
        })
    })();
    if captured.is_err() && maximized {
        let _ = window.maximize();
    }
    captured
}

fn remember_window_state(active: &mut Option<WindowState>, candidate: WindowState) {
    if active.is_none() {
        *active = Some(candidate);
    }
}

fn place_window_at_monitor(window: &WebviewWindow, target: &MonitorInfo) -> Result<(), String> {
    if window.is_maximized().unwrap_or(false) {
        window
            .unmaximize()
            .map_err(|e| format!("Couldn't prepare the launcher window: {e}"))?;
    }
    window
        .set_position(PhysicalPosition::new(target.x, target.y))
        .map_err(|e| format!("Couldn't move the launcher window to {}: {e}", target.name))
}

fn monitor_at(
    monitors: &[MonitorInfo],
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Option<&MonitorInfo> {
    let center_x = x.saturating_add((width / 2) as i32);
    let center_y = y.saturating_add((height / 2) as i32);
    monitors.iter().find(|monitor| {
        center_x >= monitor.x
            && center_x < monitor.x.saturating_add(monitor.width as i32)
            && center_y >= monitor.y
            && center_y < monitor.y.saturating_add(monitor.height as i32)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitors() -> Vec<MonitorInfo> {
        vec![
            MonitorInfo {
                id: "Desk Display|0,0".into(),
                name: "Desk Display".into(),
                is_primary: true,
                width: 1920,
                height: 1080,
                x: 0,
                y: 0,
            },
            MonitorInfo {
                id: "Second Display|1920,0".into(),
                name: "Second Display".into(),
                is_primary: false,
                width: 2560,
                height: 1440,
                x: 1920,
                y: 0,
            },
        ]
    }

    #[test]
    fn saved_monitor_is_selected_by_stable_id() {
        let monitors = monitors();
        assert_eq!(
            resolve_target_monitor(&monitors, Some("Second Display|1920,0")).map(|m| m.name),
            Some("Second Display".into())
        );
    }

    #[test]
    fn previous_native_monitor_ids_still_resolve() {
        let mut monitors = monitors();
        monitors[1].id = "\\\\.\\DISPLAY2|1920,0".into();
        assert_eq!(
            resolve_target_monitor(&monitors, Some("\\\\.\\DISPLAY2")).map(|m| m.name),
            Some("Second Display".into())
        );
    }

    #[test]
    fn automatic_and_unavailable_selections_fall_back_to_primary() {
        let monitors = monitors();
        assert_eq!(
            resolve_target_monitor(&monitors, Some("auto")).map(|m| m.name.clone()),
            Some("Desk Display".into())
        );
        assert_eq!(
            resolve_target_monitor(&monitors, Some("disconnected monitor")).map(|m| m.name),
            Some("Desk Display".into())
        );
    }

    #[test]
    fn repeated_entries_keep_the_first_state_until_exit_then_capture_again() {
        let mut active = None;
        let original = WindowState {
            x: 25,
            y: 40,
            width: 1280,
            height: 800,
            maximized: false,
            monitor_id: "Desk Display|0,0".into(),
        };
        let intermediate = WindowState {
            x: 1920,
            y: 0,
            width: 2560,
            height: 1440,
            maximized: true,
            monitor_id: "Second Display|1920,0".into(),
        };
        remember_window_state(&mut active, original.clone());
        remember_window_state(&mut active, intermediate);
        assert_eq!(active.as_ref().map(|state| state.x), Some(original.x));
        assert_eq!(
            active.as_ref().map(|state| state.width),
            Some(original.width)
        );
        assert_eq!(
            active.as_ref().map(|state| state.maximized),
            Some(original.maximized)
        );

        active = None;
        let next_entry = WindowState { x: 72, ..original };
        remember_window_state(&mut active, next_entry.clone());
        assert_eq!(active.as_ref().map(|state| state.x), Some(next_entry.x));
    }
}

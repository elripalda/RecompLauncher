//! Android has no tray: the launcher is an app on the home screen. The window still calls these.

use std::collections::HashMap;
use tauri::AppHandle;

/// Big Picture's way out on Home.
#[tauri::command]
pub fn quit_launcher(app: AppHandle) {
    app.exit(0);
}

#[tauri::command(async)]
pub fn tray_words(_app: AppHandle, _words: HashMap<String, String>) {}

pub fn refresh(_app: &AppHandle) {}

pub fn start(_app: &AppHandle) -> tauri::Result<()> {
    Ok(())
}

pub fn hid(_app: &AppHandle) {}

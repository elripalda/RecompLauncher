//! Controllers on Android: the window reads them itself (navigator.getGamepads, the same
//! fallback the desktop window has), since Android hands controller input to the app on screen.
//! These keep the shape of pads.rs, which reads them through SDL on Windows, Linux and macOS.

use serde::Serialize;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use tauri::AppHandle;

#[derive(Debug, Clone, Serialize)]
pub struct Pad {
    pub name: String,
    pub kind: &'static str,
}

/// Always empty here: the window lists the controllers it sees.
pub static CONNECTED: Mutex<Vec<Pad>> = Mutex::new(Vec::new());

pub static FOCUSED: AtomicBool = AtomicBool::new(false);
pub static PLAYING: AtomicBool = AtomicBool::new(false);
pub static STICK: AtomicBool = AtomicBool::new(true);
pub static QUICK_MENU: AtomicBool = AtomicBool::new(true);

pub fn right_stick() -> [f32; 2] {
    [0.0, 0.0]
}

/// The window always reads controllers on Android.
pub fn window_stays_off() -> bool {
    false
}

pub fn stop() {}

pub fn start(_app: AppHandle) {}

pub fn connected() -> Vec<Pad> {
    CONNECTED.lock().unwrap().clone()
}

//! Android (and iOS): stand-ins for the desktop window calls Tauri only has on Windows, Linux and
//! macOS. On a phone the launcher is one full-screen window, so minimising, maximising, full
//! screen, always-on-top, click-through and frameless windows have nothing to do; these keep the
//! shared code the same on every system. Nothing here is built for the PC launcher.

use tauri::{Runtime, WebviewWindow, WebviewWindowBuilder};

/// The window calls a desktop has and a phone doesn't. Each does nothing and says it worked.
pub trait PhoneWindow {
    fn minimize(&self) -> tauri::Result<()> {
        Ok(())
    }
    fn unminimize(&self) -> tauri::Result<()> {
        Ok(())
    }
    fn maximize(&self) -> tauri::Result<()> {
        Ok(())
    }
    fn unmaximize(&self) -> tauri::Result<()> {
        Ok(())
    }
    fn set_fullscreen(&self, _on: bool) -> tauri::Result<()> {
        Ok(())
    }
    fn set_fullscreen_on_monitor(&self, _at: tauri::PhysicalPosition<f64>) -> tauri::Result<()> {
        Ok(())
    }
    fn set_always_on_top(&self, _on: bool) -> tauri::Result<()> {
        Ok(())
    }
    fn set_ignore_cursor_events(&self, _on: bool) -> tauri::Result<()> {
        Ok(())
    }
    /// There's no mouse pointer to follow on a touch screen.
    fn cursor_position(&self) -> tauri::Result<tauri::PhysicalPosition<f64>> {
        Err(tauri::Error::WindowNotFound)
    }
}

impl<R: Runtime> PhoneWindow for WebviewWindow<R> {}

/// Window builder options a phone doesn't have: every window is full screen without a frame.
pub trait PhoneWindowBuilder: Sized {
    fn decorations(self, _on: bool) -> Self {
        self
    }
    fn always_on_top(self, _on: bool) -> Self {
        self
    }
    fn center(self) -> Self {
        self
    }
    fn skip_taskbar(self, _on: bool) -> Self {
        self
    }
    fn shadow(self, _on: bool) -> Self {
        self
    }
    fn focused(self, _on: bool) -> Self {
        self
    }
}

impl<'a, R: Runtime, M: tauri::Manager<R>> PhoneWindowBuilder for WebviewWindowBuilder<'a, R, M> {}

/// No tray icon on a phone: `tray_by_id` always finds none, so "close to tray" never hides the window.
pub trait NoTray {
    fn tray_by_id(&self, _id: &str) -> Option<()> {
        None
    }
}

impl<R: Runtime> NoTray for tauri::AppHandle<R> {}
impl<R: Runtime> NoTray for tauri::App<R> {}

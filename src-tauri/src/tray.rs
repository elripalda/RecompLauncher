//! The icon by the clock: closing the window leaves the launcher running there, like Steam or
//! Discord. A click opens the window again; right-click lists recent games, a few places, and Quit.

use crate::{show_main, App};
use std::collections::HashMap;
use std::sync::RwLock;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

/// The menu's words in the launcher's language, sent by the window once its language is loaded.
static WORDS: RwLock<Option<HashMap<String, String>>> = RwLock::new(None);

fn word(key: &str) -> String {
    WORDS.read().unwrap().as_ref().and_then(|w| w.get(key).cloned()).unwrap_or_else(|| key.to_string())
}

/// The window's translations for the menu, keyed by the English.
#[tauri::command(async)]
pub fn tray_words(app: AppHandle, words: HashMap<String, String>) {
    *WORDS.write().unwrap() = Some(words);
    refresh(&app);
}

fn menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let st = app.state::<App>();
    let running: Vec<String> = st.running.lock().unwrap().keys().cloned().collect();
    let mut games: Vec<(String, String, u64)> = st
        .lib
        .lock()
        .unwrap()
        .installed
        .values()
        .filter(|g| g.exe.is_some())
        .map(|g| (g.id.clone(), g.name.clone(), g.last_played.max(g.installed_at)))
        .collect();
    games.sort_by(|a, b| b.2.cmp(&a.2));
    let m = Menu::new(app)?;
    m.append(&MenuItem::with_id(app, "open", word("Open RecompLauncher"), true, None::<&str>)?)?;
    if !games.is_empty() {
        m.append(&PredefinedMenuItem::separator(app)?)?;
        for (id, name, _) in games.into_iter().take(8) {
            let on = running.contains(&id);
            let label = if on { format!("{name} · {}", word("Playing")) } else { name };
            m.append(&MenuItem::with_id(app, format!("play:{id}"), label, !on, None::<&str>)?)?;
        }
    }
    m.append(&PredefinedMenuItem::separator(app)?)?;
    for (id, key) in [("go:library", "Library"), ("go:discover", "Discover"), ("go:shots", "Screenshots"), ("go:settings", "Settings")] {
        m.append(&MenuItem::with_id(app, id, word(key), true, None::<&str>)?)?;
    }
    m.append(&MenuItem::with_id(app, "bp", word("Big Picture mode"), true, None::<&str>)?)?;
    m.append(&PredefinedMenuItem::separator(app)?)?;
    m.append(&MenuItem::with_id(app, "quit", word("Quit RecompLauncher"), true, None::<&str>)?)?;
    Ok(m)
}

/// Builds the menu again: the games in it change as they're played and installed.
pub fn refresh(app: &AppHandle) {
    if let (Some(t), Ok(m)) = (app.tray_by_id("main"), menu(app)) {
        let _ = t.set_menu(Some(m));
    }
}

pub fn start(app: &AppHandle) -> tauri::Result<()> {
    let Some(icon) = app.default_window_icon().cloned() else { return Ok(()) };
    TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip("RecompLauncher")
        .menu(&menu(app)?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, ev| picked(app, ev.id().as_ref()))
        .on_tray_icon_event(|t, ev| match ev {
            TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } => show_main(t.app_handle()),
            TrayIconEvent::Enter { .. } => refresh(t.app_handle()),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

fn picked(app: &AppHandle, id: &str) {
    match id {
        "open" => show_main(app),
        "quit" => app.exit(0),
        "bp" => {
            show_main(app);
            let _ = app.emit("tray", "bp");
        }
        _ if id.starts_with("go:") => {
            show_main(app);
            let _ = app.emit("tray", id);
        }
        _ if id.starts_with("play:") => {
            let id = id["play:".len()..].to_string();
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                crate::ready_look(&app, &id).await;
                crate::cloud::sync_quiet(&app, &id).await;
                if let Err(e) = crate::start_game(&app, &id) {
                    show_main(&app);
                    let _ = app.emit("toast", e.to_string());
                }
                refresh(&app);
            });
        }
        _ => {}
    }
}

/// The window was closed with the tray on: it hides instead, and the first time a notification
/// says where it went.
pub fn hid(app: &AppHandle) {
    let st = app.state::<App>();
    let first = {
        let mut l = st.lib.lock().unwrap();
        let first = !l.settings.tray_told;
        if first {
            l.settings.tray_told = true;
            let _ = l.save(&st.dir);
        }
        first
    };
    if first {
        crate::ingame::notify(app, &word("RecompLauncher is still running"), &word("It's by the clock. Right-click its icon to quit."));
    }
}

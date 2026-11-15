//! The save destination is independent of the RecompStore account used by other launcher features.

use crate::{cloud, drive, App, Error};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    #[default]
    Recompstore,
    GoogleDrive,
    Disabled,
}

pub fn current(dir: &Path) -> Provider {
    match std::fs::read(dir.join("save-provider.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or(Provider::Disabled),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Provider::Recompstore,
        Err(_) => Provider::Disabled,
    }
}

pub fn ready(dir: &Path) -> bool {
    match current(dir) {
        Provider::Recompstore => cloud::signed_in(dir),
        Provider::GoogleDrive => drive::connected(dir),
        Provider::Disabled => false,
    }
}

pub fn scope(dir: &Path) -> String {
    match current(dir) {
        Provider::Recompstore => format!("recompstore:{}", cloud::account_scope(dir)),
        Provider::GoogleDrive => format!("google_drive:{}", drive::identity(dir)),
        Provider::Disabled => "disabled".into(),
    }
}

pub fn name(dir: &Path) -> &'static str {
    match current(dir) {
        Provider::Recompstore => "RecompStore",
        Provider::GoogleDrive => "Google Drive",
        Provider::Disabled => "Unavailable",
    }
}

pub fn state(app: &AppHandle) -> Value {
    let st = app.state::<App>();
    json!({"provider": current(&st.dir), "name": name(&st.dir), "connected": ready(&st.dir), "google": drive::state(&st.dir)})
}

#[tauri::command]
pub fn save_provider_state(app: AppHandle) -> Value {
    state(&app)
}

pub fn emit(app: &AppHandle) {
    let st = app.state::<App>();
    let _ = app.emit("save-provider", json!({"state": state(app), "installed": st.lib.lock().unwrap().installed}));
}

pub fn ensure_idle(st: &App) -> Result<(), Error> {
    if !st.running.lock().unwrap().is_empty() || !st.pids.lock().unwrap().is_empty() || !crate::STARTING.lock().unwrap().is_empty() {
        return Err(Error::Msg("Close your games before changing the cloud save account or provider.".into()));
    }
    Ok(())
}

/// A destination change needs fresh consent for each game. Local backups and remote history stay.
pub fn clear_consent(st: &App) -> Result<(), Error> {
    let mut library = st.lib.lock().unwrap();
    let mut next = library.clone();
    for game in next.installed.values_mut() {
        game.cloud = None;
    }
    next.save(&st.dir)?;
    *library = next;
    cloud::reset_provider_status();
    Ok(())
}

#[tauri::command]
pub async fn save_provider_set(app: AppHandle, provider: Provider) -> Result<Value, Error> {
    if provider == Provider::Disabled {
        return Err(Error::Msg("Choose RecompStore or Google Drive.".into()));
    }
    let _guard = cloud::BUSY.lock().await;
    let st = app.state::<App>();
    ensure_idle(&st)?;
    if provider == Provider::GoogleDrive && !drive::connected(&st.dir) {
        return Err(Error::Msg("Connect Google Drive first.".into()));
    }
    if current(&st.dir) != provider {
        clear_consent(&st)?;
        let temp = st.dir.join("save-provider.json.tmp");
        crate::secret::write_private(&temp, &serde_json::to_vec(&provider).unwrap())?;
        std::fs::rename(temp, st.dir.join("save-provider.json"))?;
    }
    emit(&app);
    Ok(json!({"state": state(&app), "installed": st.lib.lock().unwrap().installed}))
}

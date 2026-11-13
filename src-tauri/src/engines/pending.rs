use super::{catalog::EngineGame, PackagePin};
use crate::{library::Installed, profile::Profile};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pending {
    pub format: u32,
    #[serde(default)]
    pub generation: String,
    pub state: String,
    pub game: EngineGame,
    pub package: PackagePin,
}

pub fn read(g: &Installed) -> Result<Option<Pending>, String> {
    let Some(raw) = &g.engine_build else { return Ok(None) };
    if raw.get("state").and_then(|v| v.as_str()) != Some("pending") { return Ok(None); }
    let p: Pending = serde_json::from_value(raw.clone()).map_err(|_| "The saved preparation cannot be read.")?;
    p.package.validate()?;
    if p.format != 1 || p.game.id() != g.id || p.package.engine != p.game.engine || super::package_for(&p.game.engine,p.package.target).is_err() || !super::registry::get(&p.game.engine)?.supports_console(&p.game.console) {
        return Err("This unfinished preparation needs its original engine checkpoint. Its files have been kept.".into());
    }
    Ok(Some(p))
}

pub fn profile(g: &Installed) -> Result<Option<Profile>, String> { Ok(read(g)?.map(|p| p.game.setup_profile())) }

pub fn check_move(g: &Installed) -> Result<(), String> {
    if read(g)?.is_some() && g.path.join(".engine-prepared.json").exists() {
        return Err("Finish Setup before moving this game. Its completed build will be registered without rebuilding it.".into());
    }
    Ok(())
}

/// Only the existing Setup action calls this. Completed games and completed results keep
/// their original pins; an unfinished attempt can use the current adapter on explicit retry.
pub fn for_setup(g: &Installed) -> Result<Installed, String> {
    let mut p = read(g)?.ok_or("This game is already prepared. Applying a fix requires a separate action.")?;
    let recommended = super::package_for(&p.game.engine, p.package.target)?;
    if p.package == recommended || g.path.join(".engine-prepared.json").exists() { return Ok(g.clone()); }
    if super::registry::get(&p.game.engine)?.output_present(&g.path) { return Err("An existing game output has been kept. It must be recovered before preparing another version.".into()); }
    p.game = super::catalog::game(&g.id).ok_or("This game has no profile in the current engine checkpoint.")?;
    p.package = recommended;
    let label = super::package_label(&p.package);
    let mut next = g.clone();
    next.engine_build = Some(serde_json::to_value(p).map_err(|e| e.to_string())?);
    next.tag = label;
    next.inputs_ready.clear();
    Ok(next)
}

pub fn create(game: &EngineGame, path: &Path) -> Result<Installed, String> {
    create_for(game, super::Target::MacosArm64, path)
}

pub fn create_for(game: &EngineGame, target: super::Target, path: &Path) -> Result<Installed, String> {
    let package = super::package_for(&game.engine, target)?;
    let engine = super::registry::for_target(&game.engine, target)?;
    if !engine.supports_console(&game.console) { return Err("This engine does not support the selected console.".into()); }
    if path.exists() { return Err("That installation folder already exists. Choose another location.".into()); }
    fs::create_dir_all(path.parent().ok_or("Missing installation parent.")?).map_err(|e| e.to_string())?;
    fs::create_dir(path).map_err(|e| e.to_string())?;
    let label = super::package_label(&package);
    let p = Pending { generation:format!("{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()), format:1, state:"pending".into(), game:game.clone(), package };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    Ok(Installed { id:game.id(), name:game.name.clone(), game:game.name.clone(), repo:engine.repo().into(),
        path:path.to_path_buf(), tag:label, installed_at:now, pinned:true,
        engine_build:Some(serde_json::to_value(p).unwrap()), ..Default::default() })
}

pub fn same_installation(original: &Installed, current: &Installed) -> bool {
    original.id == current.id && original.path == current.path && original.installed_at == current.installed_at && original.engine_build == current.engine_build
}

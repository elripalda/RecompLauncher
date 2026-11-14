//! The launcher speaks this contract; engine names and platform rules live in adapter folders.
use super::{catalog::EngineGame, PackagePin, Progress, Target};
use crate::{library::Installed, profile::Profile};
use std::{path::Path, sync::{Arc, atomic::AtomicBool}};

pub trait Adapter: Sync {
    fn id(&self) -> &'static str;
    fn repo(&self) -> &'static str;
    fn targets(&self) -> &'static [Target];
    fn systems(&self) -> &'static [&'static str];
    fn supports_console(&self, console: &str) -> bool { self.systems().contains(&console) }
    fn games(&self) -> Vec<EngineGame>;
    fn artwork(&self, title: &str) -> serde_json::Value;
    fn status(&self) -> &'static str;
    fn profile(&self, game: &EngineGame) -> Profile;
    fn pin(&self, target: Target) -> Result<PackagePin, String>;
    fn preflight(&self) -> Result<(), String> { Ok(()) }
    /// An interrupted activation must be recovered before preparing another checkpoint.
    fn output_present(&self, install: &Path) -> bool;
    fn prepare(&self, http: &reqwest::Client, data: &Path, game: &Installed, cancel: Arc<AtomicBool>, progress: &(dyn Fn(Progress) + Send + Sync)) -> Result<Installed, String>;
    fn activate(&self, current: &Installed, ready: &Installed, stopped: &dyn Fn() -> bool) -> Result<Installed, String>;
}

pub fn get(id: &str) -> Result<&'static dyn Adapter, String> {
    super::ADAPTERS.iter().copied().find(|e| e.id() == id).ok_or_else(|| "This installation needs an engine adapter this launcher does not include. Its files have been kept.".into())
}

pub fn available(target: Target) -> impl Iterator<Item = &'static dyn Adapter> {
    super::ADAPTERS.iter().copied().filter(move |e| e.targets().contains(&target))
}

pub fn for_target(id: &str, target: Target) -> Result<&'static dyn Adapter, String> {
    available(target).find(|e| e.id() == id).ok_or_else(|| "This engine is not available for this computer.".into())
}

//! Versioned engine builds. Catalog refreshes describe possibilities; installed records own pins.

mod installed;
mod model;
mod policy;

pub use installed::*;
pub use model::*;
pub use policy::*;

pub mod catalog;
pub mod packages;
pub mod pending;
pub mod jobs;
#[cfg(windows)]
mod windows_job;
pub mod coordinator;
pub mod registry;
include!(concat!(env!("OUT_DIR"), "/engine-adapters.rs"));

pub fn package_for(engine: &str, target: Target) -> Result<PackagePin, String> {
    registry::for_target(engine, target)?.pin(target)
}

pub fn preflight(engine: &str, target: Target) -> Result<(), String> {
    if Target::current() != Some(target) { return Err("This engine build is for a different computer.".into()); }
    registry::for_target(engine, target)?.preflight()
}

pub fn package_label(pin: &PackagePin) -> String { format!("{} {}", pin.engine, &pin.upstream_revision[..12]) }

/// Work folders and prepared records written before setup fingerprints changed still use this hash.
pub fn input_signature(profile: &crate::profile::Profile, game: &crate::library::Installed, data: &std::path::Path) -> Result<String, String> {
    crate::inputs::signature_legacy(profile, game, data, false)
}

mod progress;
pub use progress::Progress;

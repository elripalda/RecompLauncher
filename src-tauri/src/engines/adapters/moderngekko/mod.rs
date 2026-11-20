//! Generic GameCube/Wii preparation; upstream owns game-specific compatibility.
use crate::engines::{catalog::EngineGame, packages::Artifact, prepared, PackagePin, Progress, Target};
use crate::{library::Installed, profile::Profile};
use serde::Deserialize;
use std::{path::Path, sync::{Arc, atomic::AtomicBool}};

#[derive(Deserialize)]
pub struct Source {
    pub path: String,
    pub revision: String,
    pub artifact: Artifact,
}

#[derive(Deserialize)]
pub struct Manifest {
    pub upstream_revision: String,
    pub runtime_revision: String,
    pub sources: Vec<Source>,
    pub cmake: Artifact,
    pub ninja: Artifact,
    pub python: Artifact,
}

pub fn manifest() -> Manifest { serde_json::from_str(include_str!("components.json")).expect("checked ModernGekko packages") }

pub fn patch_sha256() -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(include_bytes!("private-python.cmake")))
}

pub fn pin(target: Target) -> Result<PackagePin, String> {
    if target != Target::MacosArm64 { return Err("ModernGekko currently requires an Apple silicon Mac.".into()); }
    let manifest = manifest();
    Ok(PackagePin {
        engine:"moderngekko".into(), target, upstream_revision:manifest.upstream_revision,
        patch_sha256:patch_sha256(),
        toolchain_sha256:crate::engines::fingerprint(&(include_str!("components.json"), include_str!("toolchain.rs"), include_str!("pipeline.rs"))),
        archive_sha256:manifest.sources[0].artifact.digest.clone(),
        packaging_revision:"recomplauncher-moderngekko-macos-v1".into(), adapter_version:1,
    })
}

pub fn setup_profile() -> Profile {
    serde_json::from_value(serde_json::json!({"format":1,"inputs":[
        {"id":"base","label":"Your game disc or extracted game","formats":[{"type":"file","extensions":["iso"]},{"type":"folder"}]}
    ]})).expect("internal ModernGekko input profile")
}

fn titles() -> Vec<serde_json::Value> { serde_json::from_str(include_str!("titles.json")).expect("generated disc metadata") }

pub struct Engine;
pub static ENGINE: Engine = Engine;
impl crate::engines::registry::Adapter for Engine {
    fn id(&self) -> &'static str { "moderngekko" }
    fn repo(&self) -> &'static str { "ExpansionPak/ModernGekko" }
    fn targets(&self) -> &'static [Target] { &[Target::MacosArm64] }
    fn systems(&self) -> &'static [&'static str] { &["gc", "wii"] }
    fn input_signature_version(&self) -> crate::engines::InputSignatureVersion { crate::engines::InputSignatureVersion::TreeV1 }
    fn status(&self) -> &'static str { "unverified" }
    fn games(&self) -> Vec<EngineGame> {
        titles().into_iter().map(|t| EngineGame {
            engine:self.id().into(), title_id:t["title_id"].as_str().unwrap().into(), name:t["name"].as_str().unwrap().into(),
            console:t["console"].as_str().unwrap().into(), package:String::new(), version:String::new(), build_id:String::new(),
            notes:"Unverified ModernGekko compatibility. Imported upstream disc metadata does not establish successful gameplay. Setup identifies the selected disc revision; some games still need fixes upstream.".into(),
            evidence:t["source"].as_str().unwrap().into(),
        }).collect()
    }
    fn artwork(&self, title: &str) -> serde_json::Value { titles().into_iter().find(|t| t["title_id"] == title).unwrap_or_default() }
    fn profile(&self, _: &EngineGame) -> Profile { setup_profile() }
    fn pin(&self, target: Target) -> Result<PackagePin, String> { pin(target) }
    fn preflight(&self) -> Result<(), String> { toolchain::preflight() }
    fn output_present(&self, install: &Path) -> bool { prepared::output_present(install) }
    fn prepare(&self, http: &reqwest::Client, data: &Path, game: &Installed, cancel: Arc<AtomicBool>, progress: &(dyn Fn(Progress) + Send + Sync)) -> Result<Installed, String> {
        prepared::prepare(self, http, data, game, cancel, progress, &pipeline::build)
    }
    fn activate(&self, current: &Installed, ready: &Installed, stopped: &dyn Fn() -> bool) -> Result<Installed, String> { prepared::activate(current, ready, stopped) }
}

mod pipeline;
pub use pipeline::{read_identity, extract_command, compile_command, frozen_output};
mod toolchain;
pub use toolchain::configure_command;

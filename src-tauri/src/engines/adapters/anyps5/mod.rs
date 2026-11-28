//! Generic conversion of decrypted PS5 folders. Compatibility fixes remain upstream.
mod input;
mod pipeline;
pub use input::{inspect, Input};
pub use pipeline::{convert_command, output, validate_output, validate_runtime};
use crate::engines::{catalog::EngineGame, packages::Artifact, prepared, PackagePin, Progress, Target};
use crate::{library::Installed, profile::Profile};
use serde::Deserialize;
use std::{path::Path, sync::{Arc, atomic::AtomicBool}};

#[derive(Clone, Deserialize)]
pub struct Packages { pub relinker: Artifact, pub runtime: Artifact }
#[derive(Deserialize)]
struct Manifest { upstream_revision: String, windows: Packages, linux: Packages }
fn manifest() -> Manifest { serde_json::from_str(include_str!("components.json")).expect("checked AnyPS5 packages") }
pub fn packages(target: Target) -> Result<Packages, String> {
    let manifest = manifest();
    match target {
        Target::WindowsX64 => Ok(manifest.windows),
        Target::LinuxX64 => Ok(manifest.linux),
        _ => Err("AnyPS5's pinned runtime supports Windows and Linux x86-64 only.".into()),
    }
}

pub fn pin(target: Target) -> Result<PackagePin, String> {
    let packages = packages(target)?;
    Ok(PackagePin {
        engine:"anyps5".into(), target, upstream_revision:manifest().upstream_revision,
        patch_sha256:"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
        toolchain_sha256:crate::engines::fingerprint(&include_str!("components.json")),
        archive_sha256:packages.runtime.digest,
        packaging_revision:"recomplauncher-anyps5-desktop-v1".into(), adapter_version:1,
    })
}

pub fn setup_profile() -> Profile {
    serde_json::from_value(serde_json::json!({"format":1,"inputs":[
        {"id":"base","label":"Your decrypted PS5 game folder (clean ELF executables and modules)","formats":[{"type":"folder"}]}
    ]})).expect("internal AnyPS5 input profile")
}

pub fn check_glibc(version: &str) -> Result<(), String> {
    let version = version.trim().strip_prefix("glibc ").unwrap_or("");
    let mut parts = version.split('.').map(str::parse::<u32>);
    if let (Some(Ok(major)), Some(Ok(minor))) = (parts.next(), parts.next()) {
        if (major, minor) >= (2, 43) { return Ok(()); }
    }
    Err("The pinned AnyPS5 Linux runtime requires glibc 2.43 or newer and libstdc++ with GLIBCXX_3.4.31. This system cannot use that runtime; no system libraries have been changed.".into())
}

fn host_preflight() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let version = std::process::Command::new("getconf").arg("GNU_LIBC_VERSION").output().map_err(|e| format!("Could not check AnyPS5's glibc requirement: {e}"))?;
        if !version.status.success() { return Err("Could not determine this system's glibc version for AnyPS5.".into()); }
        check_glibc(&String::from_utf8_lossy(&version.stdout))?;
    }
    Ok(())
}

fn titles() -> Vec<serde_json::Value> { serde_json::from_str(include_str!("titles.json")).expect("generated AnyPS5 title metadata") }
pub struct Engine;
pub static ENGINE: Engine = Engine;
impl crate::engines::registry::Adapter for Engine {
    fn id(&self) -> &'static str { "anyps5" }
    fn repo(&self) -> &'static str { "boykopovar/AnyPS5" }
    fn targets(&self) -> &'static [Target] { &[Target::WindowsX64, Target::LinuxX64] }
    fn systems(&self) -> &'static [&'static str] { &["ps5"] }
    fn input_signature_version(&self) -> crate::engines::InputSignatureVersion { crate::engines::InputSignatureVersion::TreeV1 }
    fn status(&self) -> &'static str { "unverified" }
    fn games(&self) -> Vec<EngineGame> { titles().into_iter().map(|t| serde_json::from_value(t).expect("checked title metadata")).collect() }
    fn artwork(&self, title: &str) -> serde_json::Value {
        let records: Vec<serde_json::Value> = serde_json::from_str(include_str!("artwork.json")).expect("checked title artwork");
        records.into_iter().find(|t| t["title_id"] == title).unwrap_or_default()
    }
    fn profile(&self, _: &EngineGame) -> Profile { setup_profile() }
    fn pin(&self, target: Target) -> Result<PackagePin, String> { pin(target) }
    fn preflight(&self) -> Result<(), String> { host_preflight() }
    fn output_present(&self, install: &Path) -> bool { prepared::output_present(install) }
    fn prepare(&self, http: &reqwest::Client, data: &Path, game: &Installed, cancel: Arc<AtomicBool>, progress: &(dyn Fn(Progress) + Send + Sync)) -> Result<Installed, String> {
        prepared::prepare(self, http, data, game, cancel, progress, &pipeline::build)
    }
    fn activate(&self, current: &Installed, ready: &Installed, stopped: &dyn Fn() -> bool) -> Result<Installed, String> { prepared::activate(current, ready, stopped) }
}

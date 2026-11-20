//! PS2's generic upstream pipeline runs only during Setup; Play uses a frozen native runner.
use crate::engines::{catalog::EngineGame, jobs::CommandSpec, packages::Artifact, prepared, registry::Adapter, GameIdentity, PackagePin, Progress, Target};
use crate::{library::Installed, profile::{Profile, Start}};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, path::{Path, PathBuf}, process::Command, sync::{Arc, atomic::AtomicBool}};

const MANIFEST: &str = include_str!("components.json");
const BOOTSTRAP: &str = include_str!("bootstrap.py");
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

#[derive(Deserialize)]
struct Manifest { upstream_revision: String, artifacts: BTreeMap<String, Artifact>, licenses: BTreeMap<String, Artifact> }
fn manifest() -> Manifest { serde_json::from_str(MANIFEST).expect("checked PS2 component manifest") }

pub fn pin(target: Target) -> Result<PackagePin, String> {
    if target != Target::MacosArm64 { return Err("PS2 preparation is currently available on Apple silicon Mac only.".into()); }
    let manifest = manifest();
    Ok(PackagePin { engine:"ps2-native-recomp".into(), target, upstream_revision:manifest.upstream_revision,
        patch_sha256:EMPTY_SHA256.into(), toolchain_sha256:crate::engines::fingerprint(&(MANIFEST, BOOTSTRAP)),
        archive_sha256:manifest.artifacts["source"].digest.clone(), packaging_revision:"private-macos-v1".into(), adapter_version:1 })
}

pub fn setup_profile() -> Profile {
    serde_json::from_value(json!({"format":1,"inputs":[{"id":"base","label":"Your PlayStation 2 disc image",
        "formats":[{"type":"file","extensions":["iso"]}]}]})).expect("PS2 ISO input profile")
}

#[derive(Deserialize)]
struct ResultRecord {
    identity: GameIdentity,
    start: Start,
    saves: Vec<PathBuf>,
    files: Vec<PathBuf>,
    required_files: Vec<PathBuf>,
}

fn build(ctx: &prepared::Context<'_>) -> Result<prepared::Output, String> {
    let mut config = serde_json::Map::new();
    for (name, artifact) in manifest().artifacts {
        config.insert(name, json!(ctx.unpack(&artifact)?));
    }
    let mut licenses = serde_json::Map::new();
    for (name, artifact) in manifest().licenses {
        licenses.insert(name, json!(ctx.download(&artifact)?));
    }
    config.insert("licenses".into(), json!(licenses));
    let python_root = PathBuf::from(config["python"].as_str().ok_or("Missing private Python package.")?);
    let python = python_root.join("bin/python3");
    let base = ctx.paths.get("base").ok_or("Choose your PS2 ISO first.")?;
    config.insert("disc".into(), json!(base));
    config.insert("work".into(), json!(ctx.work));
    config.insert("output".into(), json!(ctx.output));
    config.insert("title_id".into(), json!(ctx.pending.game.title_id));
    config.insert("name".into(), json!(ctx.pending.game.name));
    let config_path = ctx.work.join("ps2-config.json");
    let script = ctx.work.join("bootstrap-ps2.py");
    fs::write(&script, BOOTSTRAP).map_err(|e| e.to_string())?;
    fs::write(&config_path, serde_json::to_vec_pretty(&config).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let mut command = CommandSpec::new(python, &ctx.work);
    command.remove_env = ["PYTHONPATH", "PYTHONHOME", "PYTHONUSERBASE", "CMAKE_PREFIX_PATH", "CMAKE_TOOLCHAIN_FILE", "CFLAGS", "CXXFLAGS", "LDFLAGS", "CPATH", "LIBRARY_PATH", "DYLD_LIBRARY_PATH"].iter().map(|name| (*name).into()).collect();
    command.args = vec!["-I".into(), script.display().to_string(), config_path.display().to_string()];
    ctx.run(&command, "Preparing the disc, private compiler, and native game")?;
    let record: ResultRecord = serde_json::from_slice(&fs::read(ctx.output.join("result.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(prepared::Output { identity:record.identity, start:record.start, saves:record.saves, files:record.files, required_files:record.required_files })
}

pub struct Engine;
pub static ENGINE: Engine = Engine;
impl Adapter for Engine {
    fn id(&self) -> &'static str { "ps2-native-recomp" }
    fn repo(&self) -> &'static str { "exoteric33/ps2-native-recomp" }
    fn targets(&self) -> &'static [Target] { &[Target::MacosArm64] }
    fn systems(&self) -> &'static [&'static str] { &["ps2"] }
    fn games(&self) -> Vec<EngineGame> { serde_json::from_str(include_str!("titles.json")).expect("checked PS2 title metadata") }
    fn artwork(&self, title: &str) -> Value {
        let titles: Vec<Value> = serde_json::from_str(include_str!("titles.json")).expect("checked PS2 artwork metadata");
        titles.into_iter().find(|row| row["title_id"] == title).unwrap_or_default()
    }
    fn input_signature_version(&self) -> crate::engines::InputSignatureVersion { crate::engines::InputSignatureVersion::TreeV1 }
    fn status(&self) -> &'static str { "experimental · Mac gameplay unverified" }
    fn profile(&self, _game: &EngineGame) -> Profile { setup_profile() }
    fn pin(&self, target: Target) -> Result<PackagePin, String> { pin(target) }
    fn preflight(&self) -> Result<(), String> {
        for args in [&["--find", "clang++"][..], &["--find", "ld"][..], &["--show-sdk-path"][..]] {
            let output = Command::new("/usr/bin/xcrun").args(args).output().map_err(|_| "PS2 Setup needs Xcode's C++ compiler and macOS SDK. Install Xcode and select its command-line tools.")?;
            if !output.status.success() { return Err("PS2 Setup needs Xcode's C++ compiler and macOS SDK. Open Xcode once, finish its setup, and select its command-line tools.".into()); }
        }
        Ok(())
    }
    fn output_present(&self, install: &Path) -> bool { prepared::output_present(install) }
    fn prepare(&self, http: &reqwest::Client, data: &Path, game: &Installed, cancel: Arc<AtomicBool>, progress: &(dyn Fn(Progress) + Send + Sync)) -> Result<Installed, String> {
        prepared::prepare(self, http, data, game, cancel, progress, &build)
    }
    fn activate(&self, current: &Installed, ready: &Installed, stopped: &dyn Fn() -> bool) -> Result<Installed, String> { prepared::activate(current, ready, stopped) }
}

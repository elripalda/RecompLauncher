//! The upstream generic folder-to-app pipeline; no title-specific preparation lives here.
use crate::engines::{catalog::EngineGame, jobs::CommandSpec, packages::Artifact, prepared, registry::Adapter, Console, GameIdentity, PackagePin, Progress, Target};
use crate::{library::Installed, profile::Profile};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, io::Read, path::Path, process::Command, sync::{Arc, atomic::AtomicBool}};

mod pipeline;
const MANIFEST: &str = include_str!("components.json");
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

#[derive(Deserialize)]
struct Manifest { upstream_revision: String, engine: Artifact, cmake: Artifact, freetype: Artifact, ffmpeg: Artifact, source: Artifact }
fn manifest() -> Manifest { serde_json::from_str(MANIFEST).expect("checked PS4 component manifest") }

pub fn setup_profile() -> Profile {
    serde_json::from_value(json!({"format":1,"inputs":[{"id":"base","label":"Your decrypted PlayStation 4 game folder",
        "formats":[{"type":"folder"}]}]})).expect("PS4 folder input profile")
}

fn sfo(bytes: &[u8]) -> Result<BTreeMap<String, String>, String> {
    let invalid = "The game has an invalid sce_sys/param.sfo.";
    if bytes.len() < 20 || &bytes[..4] != b"\0PSF" { return Err(invalid.into()); }
    let word = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let (keys, data, count) = (word(8), word(12), word(16));
    if count > (bytes.len() - 20) / 16 || keys < 20 + count * 16 || keys > data || data > bytes.len() { return Err(invalid.into()); }
    let mut fields = BTreeMap::new();
    for index in 0..count {
        let at = 20 + index * 16;
        let key_offset = u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) as usize;
        let key = keys.checked_add(key_offset).filter(|&key| key < data).ok_or(invalid)?;
        let end = bytes[key..data].iter().position(|&b| b == 0).ok_or(invalid)?;
        let name = std::str::from_utf8(&bytes[key..key + end]).map_err(|_| invalid)?;
        let value = data.checked_add(word(at + 12)).ok_or(invalid)?;
        let end = value.checked_add(word(at + 4)).filter(|&end| end <= bytes.len()).ok_or(invalid)?;
        if word(at + 4) > word(at + 8) { return Err(invalid.into()); }
        if !["TITLE_ID", "APP_VER"].contains(&name) { continue; }
        if bytes[at + 2..at + 4] != [4, 2] || value == end || bytes[end - 1] != 0 { return Err(invalid.into()); }
        let value = std::str::from_utf8(&bytes[value..end - 1]).map_err(|_| invalid)?;
        if value.contains('\0') || fields.insert(name.into(), value.into()).is_some() { return Err(invalid.into()); }
    }
    Ok(fields)
}

pub fn identify(root: &Path, selected_title: &str) -> Result<GameIdentity, String> {
    let metadata = root.join("sce_sys/param.sfo");
    if fs::metadata(&metadata).map_err(|_| "Choose the game folder containing sce_sys/param.sfo and eboot.bin.")?.len() > 1024 * 1024 { return Err("The game metadata is too large.".into()); }
    let fields = sfo(&fs::read(metadata).map_err(|e| e.to_string())?)?;
    let title = fields.get("TITLE_ID").ok_or("The game metadata has no title ID.")?;
    if title.len() != 9 || !title.starts_with("CUSA") || !title[4..].bytes().all(|b| b.is_ascii_digit()) || title != selected_title { return Err("The selected folder belongs to a different PS4 game or region.".into()); }
    let version = fields.get("APP_VER").filter(|v| !v.is_empty()).ok_or("The game metadata has no application version.")?;
    let executable = root.join("eboot.bin");
    let mut head = [0; 64];
    fs::File::open(&executable).and_then(|mut file| file.read_exact(&mut head)).map_err(|_| "The game has no complete eboot.bin.")?;
    if &head[..6] != b"\x7fELF\x02\x01" || head[18..20] != [62, 0] { return Err("PS4Recomp needs a decrypted x86-64 ELF eboot.bin; encrypted SELF files are not supported.".into()); }
    Ok(GameIdentity { console:Console::Other("ps4".into()), title_id:title.clone(), version:version.clone(), build_id:prepared::file_sha256(&executable)? })
}

pub fn command(program: &Path, work: &Path, input: &Path, output: &Path, tools: &Path) -> CommandSpec {
    let mut command = pipeline::isolated(program, work);
    command.args = vec!["build".into(), input.display().to_string(), "-o".into(), output.display().to_string(), "--standalone".into(), "--json".into()];
    command.env.insert("PATH".into(), format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", tools.join("bin").display()));
    command.env.insert("CPATH".into(), tools.join("ffmpeg/include").display().to_string());
    command.env.insert("LIBRARY_PATH".into(), tools.join("ffmpeg/lib").display().to_string());
    command
}

pub fn progress(event: &Value) -> Option<Progress> {
    if event["event"] != "progress" { return None; }
    let done = event["done"].as_f64().filter(|v| v.is_finite() && *v >= 0.0);
    let total = event["total"].as_f64().filter(|v| v.is_finite() && *v > 0.0);
    let fraction = done.zip(total).filter(|(d, t)| d <= t).map(|(d, t)| (d / t) as f32);
    Some(Progress { text:format!("{}: {}", event["stage"].as_str().unwrap_or("Preparing"), event["message"].as_str().unwrap_or("")), fraction, done, total,
        unit:event["unit"].as_str().unwrap_or("").into(), ..Default::default() })
}

pub struct Engine;
pub static ENGINE: Engine = Engine;
impl Adapter for Engine {
    fn id(&self) -> &'static str { "ps4recomp" }
    fn repo(&self) -> &'static str { "wie-project/PS4Recomp" }
    fn targets(&self) -> &'static [Target] { &[Target::MacosArm64] }
    fn systems(&self) -> &'static [&'static str] { &["ps4"] }
    fn games(&self) -> Vec<EngineGame> {
        let rows: Vec<Value> = serde_json::from_str(include_str!("titles.json")).expect("checked PS4 catalog");
        rows.into_iter().map(|row| EngineGame { engine:self.id().into(), title_id:row["title_id"].as_str().unwrap().into(), name:row["name"].as_str().unwrap().into(),
            console:"ps4".into(), package:"folder".into(), version:String::new(), build_id:String::new(), evidence:row["source"].as_str().unwrap().into(),
            notes:format!("Experimental; launcher gameplay unverified. Upstream reports {}: {}", row["upstream_status"].as_str().unwrap(), row["notes"].as_str().unwrap()) }).collect()
    }
    fn artwork(&self, title: &str) -> Value {
        let rows: Vec<Value> = serde_json::from_str(include_str!("titles.json")).expect("checked PS4 artwork");
        rows.into_iter().find(|row| row["title_id"] == title).unwrap_or_default()
    }
    fn input_signature_version(&self) -> crate::engines::InputSignatureVersion { crate::engines::InputSignatureVersion::TreeV1 }
    fn status(&self) -> &'static str { "experimental · gameplay unverified" }
    fn profile(&self, _game: &EngineGame) -> Profile { setup_profile() }
    fn pin(&self, target: Target) -> Result<PackagePin, String> {
        if target != Target::MacosArm64 { return Err("PS4Recomp currently prepares Apple silicon Mac games only.".into()); }
        let manifest = manifest();
        Ok(PackagePin { engine:self.id().into(), target, upstream_revision:manifest.upstream_revision, archive_sha256:manifest.engine.digest,
            patch_sha256:EMPTY_SHA256.into(), toolchain_sha256:crate::engines::fingerprint(&(MANIFEST, include_str!("pipeline.rs"))), packaging_revision:"standalone-macos-v1".into(), adapter_version:1 })
    }
    fn preflight(&self) -> Result<(), String> {
        for args in [&["--find", "clang"][..], &["--show-sdk-path"][..]] {
            let result = Command::new("/usr/bin/xcrun").args(args).output().map_err(|_| "PS4 Setup needs Xcode's compiler and macOS SDK.")?;
            if !result.status.success() { return Err("PS4 Setup needs Xcode's compiler and macOS SDK. Open Xcode and finish its command-line tools setup.".into()); }
        }
        Ok(())
    }
    fn output_present(&self, install: &Path) -> bool { prepared::output_present(install) }
    fn prepare(&self, http: &reqwest::Client, data: &Path, game: &Installed, cancel: Arc<AtomicBool>, progress: &(dyn Fn(Progress) + Send + Sync)) -> Result<Installed, String> {
        prepared::prepare(self, http, data, game, cancel, progress, &pipeline::build)
    }
    fn activate(&self, current: &Installed, ready: &Installed, stopped: &dyn Fn() -> bool) -> Result<Installed, String> { prepared::activate(current, ready, stopped) }
}

//! ScarletNX's driver is used during Setup only; Play uses the frozen game runtime.
use crate::engines::{jobs::{self, CommandSpec, Journal}, packages::{self, Artifact}, pending, GameIdentity, PackagePin, Progress, Target};
use crate::{inputs, library::Installed, profile::Profile};
use serde_json::json;
use std::{fs, path::{Path, PathBuf}, sync::{Arc, atomic::{AtomicBool, Ordering}}};

#[derive(serde::Deserialize)]
struct Manifest {
    upstream_revision: String,
    packages: std::collections::BTreeMap<String, Artifact>,
}

fn manifest() -> Manifest { serde_json::from_str(include_str!("scarletnx-components.json")).expect("checked ScarletNX manifest") }

pub fn artifact(target: Target) -> Result<Artifact, String> {
    let key = match target {
        Target::WindowsX64 => "windows-x64",
        Target::LinuxX64 => "linux-x64",
        _ => return Err("ScarletNX is enabled for Windows/Linux x86-64 only.".into()),
    };
    manifest().packages.remove(key).ok_or_else(|| "This engine package is unavailable.".into())
}

pub fn pin(target: Target) -> Result<PackagePin, String> {
    let package = artifact(target)?;
    Ok(PackagePin { engine:"scarletnx".into(), target, upstream_revision:manifest().upstream_revision,
        patch_sha256:"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
        toolchain_sha256:crate::engines::fingerprint(&include_str!("scarletnx-components.json")),
        archive_sha256:package.digest, packaging_revision:"recomplauncher-desktop-v1".into(), adapter_version:1 })
}

pub fn setup_profile() -> Profile {
    serde_json::from_value(json!({"format":1,"inputs":[
        {"id":"base","label":"Your game","formats":[{"type":"folder"},{"type":"file","extensions":["dnsp","dxci"]},{"type":"file","extensions":["nsp","xci"],"requires":["keys"]}]},
        {"id":"update","label":"Game update (if separate)","optional":true,"role":"update","formats":[{"type":"file","extensions":["dnsp","dxci"]},{"type":"file","extensions":["nsp","xci"],"requires":["keys"]}]},
        {"id":"keys","label":"Nintendo Switch prod.keys","role":"key","shared":"switch.prod-keys"}
    ]})).expect("internal ScarletNX input profile")
}

fn command(program: &Path, work: &Path) -> CommandSpec {
    let mut cmd = CommandSpec::new(program, work);
    // AppImage's own extraction mode avoids depending on FUSE, including inside Flatpak.
    cmd.env.insert("APPIMAGE_EXTRACT_AND_RUN".into(), "1".into());
    cmd.args = vec!["--data".into(), work.join("data").display().to_string()];
    cmd
}

pub fn extract_command(program: &Path, work: &Path, base: &Path, update: Option<&Path>, keys: Option<&Path>) -> CommandSpec {
    let mut cmd = command(program, work);
    cmd.args.extend(["-x".into(), "--out".into(), work.join("extracted").display().to_string()]);
    if let Some(update) = update { cmd.args.extend(["--update".into(), update.display().to_string()]); }
    if let Some(keys) = keys { cmd.args.extend(["--keys".into(), keys.display().to_string()]); }
    cmd.args.push(base.display().to_string());
    cmd
}

pub fn compile_command(program: &Path, work: &Path) -> CommandSpec {
    let mut cmd = command(program, work);
    cmd.args.extend(["-r".into(), "--extract-container".into(), "--outdir".into(), work.join("output").display().to_string(), work.join("extracted").display().to_string()]);
    cmd
}

pub fn read_identity(root: &Path, selected_title: &str) -> Result<GameIdentity, String> {
    use std::io::Read;
    let read = |path: &str, limit: u64| -> Result<Vec<u8>, String> {
        let mut bytes = Vec::new();
        fs::File::open(root.join(path)).map_err(|e| format!("Missing game metadata ({path}): {e}"))?
            .take(limit).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        Ok(bytes)
    };
    let npdm = read("exefs/main.npdm", 1024 * 1024)?;
    if npdm.len() < 0x80 || &npdm[..4] != b"META" { return Err("The game has invalid program metadata.".into()); }
    let offset = u32::from_le_bytes(npdm[0x70..0x74].try_into().unwrap()) as usize;
    let aci = npdm.get(offset..).filter(|v| v.len() >= 0x40 && &v[..4] == b"ACI0").ok_or("The game's title record is incomplete.")?;
    let title = format!("{:016X}", u64::from_le_bytes(aci[0x10..0x18].try_into().unwrap()));
    if !title.eq_ignore_ascii_case(selected_title) || !title.starts_with("01") { return Err("These files belong to a different game. Choose the selected Switch 1 title's files.".into()); }
    let nso = read("exefs/main", 128)?;
    if nso.len() < 96 || &nso[..4] != b"NSO0" || nso[64..96].iter().all(|b| *b == 0) { return Err("The game's executable has no valid build identity.".into()); }
    let nacp = read("control.nacp", 0x4000)?;
    let version = nacp.get(0x3060..0x3070).ok_or("The game has no version metadata.")?;
    let version = std::str::from_utf8(version.split(|b| *b == 0).next().unwrap()).map_err(|_| "The game version cannot be read.")?.trim().to_string();
    let identity = GameIdentity { console:crate::engines::Console::Switch, title_id:title, version, build_id:hex::encode(&nso[64..96]) };
    identity.validate()?;
    if !root.join("romfs").is_dir() { return Err("The extracted game is missing its RomFS folder.".into()); }
    Ok(identity)
}

pub async fn ensure_package(http: &reqwest::Client, data: &Path, target: Target, cancel: Arc<AtomicBool>, report: &(dyn Fn(f32, String) + Send + Sync)) -> Result<PathBuf, String> {
    let artifact = artifact(target)?;
    let base = if target == Target::WindowsX64 {
        packages::ensure(http, data, &artifact, cancel, report).await.map_err(|e| e.to_string())?
    } else {
        // Keep the image intact: the normal installer may unpack AppImages in Flatpak.
        // ScarletNX must deploy a complete copy of the same image into the game folder.
        let cache = data.join("engines/packages");
        fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
        let _lock = packages::lock(&cache.join(format!("{}.lock", artifact.digest)), &cancel).await.map_err(|e| e.to_string())?;
        let target = cache.join(&artifact.digest);
        let binary = target.join(&artifact.name);
        if target.exists() {
            packages::verify(&binary, &artifact.digest).map_err(|e| e.to_string())?;
        } else {
            let downloaded = packages::download(http, data, &artifact, cancel.clone(), report).await.map_err(|e| e.to_string())?;
            if cancel.load(Ordering::Relaxed) { return Err("Stopped.".into()); }
            let stage = target.with_extension("extracting");
            if stage.exists() { fs::remove_dir_all(&stage).map_err(|e| e.to_string())?; }
            fs::create_dir(&stage).map_err(|e| e.to_string())?;
            fs::copy(downloaded, stage.join(&artifact.name)).map_err(|e| e.to_string())?;
            #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; fs::set_permissions(stage.join(&artifact.name), fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?; }
            fs::write(stage.join(".verified-package"), &artifact.digest).map_err(|e| e.to_string())?;
            fs::rename(stage, &target).map_err(|e| e.to_string())?;
        }
        target
    };
    let program = base.join(if target == Target::WindowsX64 { "bin/scarletnx.exe" } else { &artifact.name });
    if !crate::engines::installed::native_binary(&program, target) { return Err("The engine package has no native executable for this computer.".into()); }
    Ok(program)
}

pub fn prepare(http: &reqwest::Client, data: &Path, game: &Installed, cancel: Arc<AtomicBool>, progress: &(dyn Fn(Progress) + Send + Sync)) -> Result<Installed, String> {
    let saved = pending::read(game)?.ok_or("This game is already prepared.")?;
    crate::engines::preflight("scarletnx", saved.package.target)?;
    let profile = saved.game.setup_profile();
    let paths = inputs::resolve(&profile, game, data)?;
    let signature = inputs::signature(&profile, game, data)?;
    if let Some(ready) = output::recover(game, &signature)? { return Ok(ready); }
    if saved.package != pin(saved.package.target)? { return Err("Retry Setup to use the current engine checkpoint for this unfinished installation.".into()); }
    let work = game.path.join(".engine-work").join(crate::engines::fingerprint(&(saved.package.identity(), &signature)));
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let stopped = || cancel.load(Ordering::Relaxed);
    let mut journal = Journal { state:"preparing".into(), installation:game.id.clone(), package:saved.package.identity(), inputs:signature.clone() };
    journal.save(&work)?;
    let result = (|| {
        let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
        let program = rt.block_on(ensure_package(http, data, saved.package.target, cancel.clone(), &|fraction, text| {
            progress(Progress { fraction:Some(fraction), text, ..Default::default() });
        }))?;
        if stopped() { return Err("Stopped.".into()); }
        let base = paths.get("base").ok_or("Choose your game files first.")?;
        let extracted = work.join("extracted");
        let extraction_stamp = work.join(".extracted");
        if fs::read_to_string(&extraction_stamp).ok().as_deref() != Some(&signature) {
            if extracted.exists() { fs::remove_dir_all(&extracted).map_err(|e| e.to_string())?; }
            progress(Progress::message("Extracting game files"));
            if base.is_dir() {
                if paths.contains_key("update") { return Err("For an extracted game, apply its update inside the selected folder first.".into()); }
                crate::engines::coordinator::copy_tree(base, &extracted, &stopped)?;
            } else {
                let cmd = extract_command(&program, &work, base, paths.get("update").map(PathBuf::as_path), paths.get("keys").map(PathBuf::as_path));
                jobs::run_process(&cmd, &work.join("extraction.log"), &stopped, &|_| {})?;
            }
            read_identity(&extracted, &saved.game.title_id)?;
            fs::write(&extraction_stamp, &signature).map_err(|e| e.to_string())?;
        }
        let identity = read_identity(&extracted, &saved.game.title_id)?;
        progress(Progress::message("Compiling the game"));
        jobs::run_process(&compile_command(&program, &work), &game.path.join("preparation.log"), &stopped, &|_| {})?;
        if stopped() { return Err("Stopped.".into()); }
        if inputs::signature(&profile, game, data)? != signature { return Err("Your selected files changed during preparation. Choose them again before retrying.".into()); }
        progress(Progress::message("Checking the prepared game"));
        output::register(game, &work, &signature, identity, &program)
    })();
    journal.state = if result.is_ok() { "complete" } else if stopped() { "cancelled" } else { "failed" }.into();
    let _ = journal.save(&work);
    result
}

pub mod output;
pub struct Engine;
pub static ENGINE: Engine = Engine;
fn titles() -> Vec<serde_json::Value> { serde_json::from_str(include_str!("titles.json")).expect("checked title metadata") }
impl crate::engines::registry::Adapter for Engine {
    fn id(&self) -> &'static str { "scarletnx" }
    fn repo(&self) -> &'static str { "scarletnx/ScarletNX" }
    fn targets(&self) -> &'static [Target] { &[Target::WindowsX64, Target::LinuxX64] }
    fn systems(&self) -> &'static [&'static str] { &["switch"] }
    fn games(&self) -> Vec<crate::engines::catalog::EngineGame> {
        titles().into_iter().map(|t| crate::engines::catalog::EngineGame {
            engine:self.id().into(), title_id:t["title_id"].as_str().unwrap().into(), name:t["name"].as_str().unwrap().into(),
            console:"switch".into(), package:String::new(), version:String::new(), build_id:String::new(),
            notes:"Unverified ScarletNX compatibility. This listing uses title metadata from the nxnative catalog; it is not a ScarletNX compatibility report. The selected files determine the installed version.".into(),
            evidence:t["source"].as_str().unwrap().into()
        }).collect()
    }
    fn artwork(&self, title: &str) -> serde_json::Value { titles().into_iter().find(|t| t["title_id"] == title).unwrap_or_default() }
    fn status(&self) -> &'static str { "unverified" }
    fn profile(&self, _game: &crate::engines::catalog::EngineGame) -> Profile { setup_profile() }
    fn pin(&self, target: Target) -> Result<PackagePin, String> { pin(target) }
    fn output_present(&self, install: &Path) -> bool { install.join(".scarletnx").exists() }
    fn prepare(&self, http: &reqwest::Client, data: &Path, game: &Installed, cancel: Arc<AtomicBool>, progress: &(dyn Fn(Progress) + Send + Sync)) -> Result<Installed, String> { prepare(http, data, game, cancel, progress) }
    fn activate(&self, current: &Installed, ready: &Installed, stopped: &dyn Fn() -> bool) -> Result<Installed, String> { output::activate(current, ready, stopped) }
}

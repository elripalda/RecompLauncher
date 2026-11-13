use crate::engines::{catalog::EngineGame, jobs::CommandSpec};
use std::path::Path;

pub fn command(game: &EngineGame, resources: &Path, work: &Path, keys: Option<&Path>, dotnet: Option<&Path>) -> CommandSpec {
    let python = resources.join("python/bin/python3");
    let engine = work.join("engine");
    let mut cmd = CommandSpec::new(python, &engine);
    cmd.args = vec!["-c".into(), include_str!("watch_parent.py").into(), engine.join("scripts/build_ipa_from_nsp.py").display().to_string(), work.join("inputs").display().to_string(),
        "--title-id".into(), game.title_id.clone(), "--package".into(), game.package.clone(), "--platform".into(), "mac-app".into(),
        "--progress".into(), "json".into(), "--work-dir".into(), work.join("build").display().to_string(),
        "-o".into(), work.join("output/Game.app").display().to_string(), "--keep-extracted".into()];
    if let Some(k) = keys { cmd.args.extend(["--keys".into(), k.display().to_string()]); }
    let path = format!("{}:{}:{}:/usr/bin:/bin:/usr/sbin:/sbin", resources.join("bin").display(), resources.join("python/bin").display(), dotnet.unwrap_or(Path::new("/usr/bin")).display());
    cmd.env.extend([("PATH".into(),path),("PYTHONNOUSERSITE".into(),"1".into()),("PYTHONDONTWRITEBYTECODE".into(),"1".into()),
        ("DOTNET_CLI_TELEMETRY_OPTOUT".into(),"1".into()), ("DOTNET_SKIP_FIRST_TIME_EXPERIENCE".into(),"1".into()),
        ("DOTNET_CLI_HOME".into(),work.join("dotnet-home").display().to_string()), ("LANG".into(),"en_US.UTF-8".into())]);
    if let Some(d) = dotnet { cmd.env.insert("DOTNET_ROOT".into(), d.display().to_string()); }
    cmd
}

pub fn preflight() -> Result<(), String> {
    if crate::engines::Target::current() != Some(crate::engines::Target::MacosArm64) { return Err("This engine build is available for Apple silicon Macs only.".into()); }
    for args in [vec!["--show-sdk-path"], vec!["-sdk", "macosx", "metal", "--version"]] {
        let output = std::process::Command::new("/usr/bin/xcrun").args(args).output().map_err(|e| e.to_string())?;
        if !output.status.success() { return Err("This game's preparation requires Xcode and its Metal compiler. They aren't available on this Mac.".into()); }
    }
    Ok(())
}

#[derive(serde::Deserialize)]
pub struct Manifest {
    pub upstream_revision: String,
    pub source: crate::engines::packages::Artifact,
    pub python: crate::engines::packages::Artifact,
    pub dotnet: crate::engines::packages::Artifact,
    pub bottles: Vec<crate::engines::packages::Artifact>,
    pub wheels: Vec<crate::engines::packages::Artifact>,
}
impl Manifest {
    pub fn components(&self) -> Vec<&crate::engines::packages::Artifact> {
        [&self.source, &self.python, &self.dotnet].into_iter().chain(self.bottles.iter()).chain(self.wheels.iter()).collect()
    }
}
pub fn manifest() -> Manifest { serde_json::from_str(include_str!("nxnative-components.json")).expect("checked nxnative component manifest") }

pub fn pin() -> crate::engines::PackagePin {
    let m = manifest();
    crate::engines::PackagePin { engine:"nxnative".into(), target:crate::engines::Target::MacosArm64, upstream_revision:m.upstream_revision,
        patch_sha256:"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
        toolchain_sha256:crate::engines::fingerprint(&(include_str!("nxnative-components.json"), include_str!("bootstrap_nxnative.py"))),
        archive_sha256:m.source.digest, packaging_revision:"recomplauncher-macos-v2".into(), adapter_version:1 }
}
pub fn label() -> String { format!("nxnative {}", &manifest().upstream_revision[..12]) }

pub fn preparation_error(game: &EngineGame, error: String) -> String {
    if error.contains("version to pack (-p / PACKAGE) is not in the folder") {
        return format!("{} needs version {} ({}). The selected files do not include it. Choose the matching update in Easy Setup, or a game file that already includes that version.",game.name,game.version,game.package);
    }
    error
}

pub struct Tools {
    pub source: std::path::PathBuf,
    pub private: std::path::PathBuf,
    pub dotnet: std::path::PathBuf,
}

pub async fn provision(http: &reqwest::Client, data: &Path, cancel: std::sync::Arc<std::sync::atomic::AtomicBool>, progress: &(dyn Fn(f32, String) + Send + Sync)) -> Result<Tools, crate::Error> {
    use crate::engines::{packages, jobs};
    use std::{fs, sync::atomic::Ordering};
    let m = manifest();
    let source = packages::ensure(http, data, &m.source, cancel.clone(), progress).await?;
    let dotnet = packages::ensure(http, data, &m.dotnet, cancel.clone(), progress).await?;
    let cache = data.join("engines/toolchains");
    fs::create_dir_all(&cache)?;
    let key = pin().toolchain_sha256;
    let _lock = packages::lock(&cache.join(format!("{key}.lock")), &cancel).await?;
    let target = cache.join(&key);
    if fs::read_to_string(target.join(".ready")).ok().as_deref() == Some(&key) { return Ok(Tools {source, private:target, dotnet}); }
    if target.exists() { return Err(crate::Error::Msg("The private build tools are incomplete; their folder was kept.".into())); }
    let python = packages::ensure(http, data, &m.python, cancel.clone(), progress).await?;
    let mut bottles = vec![];
    for b in &m.bottles { bottles.push(packages::ensure(http,data,b,cancel.clone(),progress).await?); }
    let mut wheels = vec![];
    for w in &m.wheels { wheels.push(packages::download(http,data,w,cancel.clone(),progress).await?); }
    if cancel.load(Ordering::Relaxed) { return Err(crate::Error::Cancelled); }
    let stage = cache.join(format!("{key}.preparing"));
    if stage.exists() { fs::remove_dir_all(&stage)?; }
    fs::create_dir_all(&stage)?;
    let config = stage.join("components.json");
    fs::write(&config, serde_json::to_vec(&serde_json::json!({"destination":stage,"python":python,"bottles":bottles,"wheels":wheels})).unwrap())?;
    let script = stage.join("bootstrap.py");
    fs::write(&script, include_str!("bootstrap_nxnative.py"))?;
    let mut cmd = jobs::CommandSpec::new(python.join("bin/python3"), &stage);
    cmd.args = vec!["-c".into(), include_str!("watch_parent.py").into(), script.display().to_string(), config.display().to_string()];
    cmd.env.extend([("PYTHONNOUSERSITE".into(),"1".into()),("PYTHONDONTWRITEBYTECODE".into(),"1".into())]);
    progress(0.0,"Preparing private build tools".into());
    jobs::run_process(&cmd,&cache.join(format!("{key}.log")),&||cancel.load(Ordering::Relaxed),&|_|{}).map_err(crate::Error::Msg)?;
    if cancel.load(Ordering::Relaxed) { return Err(crate::Error::Cancelled); }
    fs::write(stage.join(".ready"),&key)?;
    fs::rename(stage,&target)?;
    Ok(Tools {source, private:target, dotnet})
}

#[derive(Default)]
pub struct ProgressTracker { stages: Option<u64> }
impl ProgressTracker {
    pub fn event(&mut self, event: &serde_json::Value) -> Option<crate::engines::Progress> {
        let kind = event["ev"].as_str()?;
        if !matches!(kind, "stage" | "step" | "progress" | "stage_end") { return None; }
        if kind == "stage" { self.stages = event["n"].as_u64().filter(|n| *n > 0); }
        let number = |key: &str| event[key].as_f64().filter(|n| n.is_finite() && *n >= 0.0);
        let name = event["name"].as_str().unwrap_or("Preparing game").trim();
        let step = event["step"].as_str().unwrap_or("").trim();
        let text = if step.is_empty() { name.to_string() } else { format!("{name} · {step}") };
        let fraction = match kind {
            "stage_end" if event["ok"] == true => Some(1.0),
            "progress" => number("frac").filter(|n| *n <= 1.0).map(|n| n.min(0.99) as f32),
            _ => None,
        };
        // Each event replaces the measurements. A new substep must not retain the old ETA.
        Some(crate::engines::Progress { text:text.chars().take(200).collect(), fraction,
            stage:event["k"].as_u64().filter(|k| *k > 0), stages:self.stages,
            done:number("done"), total:number("total").filter(|n| *n > 0.0),
            unit:event["unit"].as_str().unwrap_or("").chars().take(40).collect(),
            estimated:event["est"].as_bool().unwrap_or(false),
            eta_seconds:if kind == "progress" { number("eta") } else { None }, elapsed_seconds:number("elapsed") })
    }
}

pub mod catalog;
pub mod pipeline;
pub struct Engine;
pub static ENGINE: Engine = Engine;
impl crate::engines::registry::Adapter for Engine {
    fn id(&self) -> &'static str { "nxnative" }
    fn repo(&self) -> &'static str { "seedds/nxnative" }
    fn targets(&self) -> &'static [crate::engines::Target] { &[crate::engines::Target::MacosArm64] }
    fn systems(&self) -> &'static [&'static str] { &["switch", "switch2"] }
    fn games(&self) -> Vec<EngineGame> { catalog::games() }
    fn artwork(&self, title: &str) -> serde_json::Value {
        let art: serde_json::Value = serde_json::from_str(include_str!("artwork-nxnative.json")).expect("checked artwork");
        art["games"][title].clone()
    }
    fn status(&self) -> &'static str { "experimental" }
    fn profile(&self, game: &EngineGame) -> crate::profile::Profile { catalog::setup_profile(game) }
    fn pin(&self, _target: crate::engines::Target) -> Result<crate::engines::PackagePin, String> { Ok(pin()) }
    fn preflight(&self) -> Result<(), String> { preflight() }
    fn output_present(&self, install: &Path) -> bool { install.join("Game.app").exists() }
    fn prepare(&self, http: &reqwest::Client, data: &Path, game: &crate::library::Installed, cancel: std::sync::Arc<std::sync::atomic::AtomicBool>, progress: &(dyn Fn(crate::engines::Progress) + Send + Sync)) -> Result<crate::library::Installed, String> { pipeline::prepare(http, data, game, cancel, progress) }
    fn activate(&self, current: &crate::library::Installed, ready: &crate::library::Installed, stopped: &dyn Fn() -> bool) -> Result<crate::library::Installed, String> { pipeline::activate_output(current, ready, stopped) }
}

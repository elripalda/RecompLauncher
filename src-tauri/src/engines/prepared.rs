//! Engine-independent preparation and publication. Adapters supply commands and output rules.
use super::{catalog::EngineGame, coordinator, jobs, packages, pending, registry::Adapter, BuildPin, Components, GameIdentity, Progress};
use crate::{inputs, library::Installed, profile::{Profile, Start}};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, io::Read, path::{Component, Path, PathBuf}, sync::{Arc, atomic::{AtomicBool, Ordering}}};

pub const DIRECTORY: &str = "Game";

/// Paths are relative to the output tree. Launch arguments/environment may use `{install}`.
pub struct Output {
    pub identity: GameIdentity,
    pub start: Start,
    pub saves: Vec<PathBuf>,
    pub files: Vec<PathBuf>,
    pub required_files: Vec<PathBuf>,
}

pub struct Context<'a> {
    pub http: &'a reqwest::Client,
    pub data: &'a Path,
    pub game: &'a Installed,
    pub pending: pending::Pending,
    pub paths: BTreeMap<String, PathBuf>,
    pub work: PathBuf,
    pub output: PathBuf,
    pub progress: &'a (dyn Fn(Progress) + Send + Sync),
    pub cancel: Arc<AtomicBool>,
}

impl Context<'_> {
    pub fn stopped(&self) -> bool { self.cancel.load(Ordering::Relaxed) }

    pub fn check_cancel(&self) -> Result<(), String> {
        if self.stopped() { return Err("Stopped.".into()); }
        Ok(())
    }

    pub fn unpack(&self, artifact: &packages::Artifact) -> Result<PathBuf, String> {
        self.check_cancel()?;
        let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
        runtime.block_on(packages::ensure(self.http, self.data, artifact, self.cancel.clone(), &|fraction, text| {
            (self.progress)(Progress { fraction: Some(fraction), text, ..Default::default() });
        })).map_err(|e| e.to_string())
    }

    pub fn download(&self, artifact: &packages::Artifact) -> Result<PathBuf, String> {
        self.check_cancel()?;
        let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
        runtime.block_on(packages::download(self.http, self.data, artifact, self.cancel.clone(), &|fraction, text| {
            (self.progress)(Progress { fraction: Some(fraction), text, ..Default::default() });
        })).map_err(|e| e.to_string())
    }

    pub fn run(&self, command: &jobs::CommandSpec, message: &str) -> Result<(), String> {
        self.check_cancel()?;
        (self.progress)(Progress::message(message));
        jobs::run_process(command, &self.work.join("preparation.log"), &|| self.stopped(), &|_| {})?;
        self.check_cancel()
    }
}

#[derive(Serialize, Deserialize)]
struct Prepared {
    pending: pending::Pending,
    ready: Installed,
}

pub fn output_present(install: &Path) -> bool { install.join(DIRECTORY).exists() }

pub fn work_path(game: &Installed, signature: &str) -> Result<PathBuf, String> {
    let saved = pending::read(game)?.ok_or("This game is already prepared.")?;
    Ok(game.path.join(".engine-work").join(super::fingerprint(&(saved.package.identity(), signature))))
}

pub fn prepare(
    adapter: &dyn Adapter, http: &reqwest::Client, data: &Path, game: &Installed,
    cancel: Arc<AtomicBool>, progress: &(dyn Fn(Progress) + Send + Sync),
    build: &dyn Fn(&Context<'_>) -> Result<Output, String>,
) -> Result<Installed, String> {
    let saved = pending::read(game)?.ok_or("This game is already prepared.")?;
    if saved.package.engine != adapter.id() { return Err("The preparation belongs to another engine.".into()); }
    let profile = saved.profile();
    let paths = inputs::resolve(&profile, game, data)?;
    let signature = super::input_signature_with_cancel(&profile, game, data, &|| cancel.load(Ordering::Relaxed))?;
    if cancel.load(Ordering::Relaxed) { return Err("Stopped.".into()); }
    // Recovery needs the saved output, not today's compiler or host build prerequisites.
    if let Some(ready) = recover(game, &signature)? { return Ok(ready); }
    if output_present(&game.path) { return Err("Existing game output has been kept. Recover it before preparing another build.".into()); }
    if saved.package != adapter.pin(saved.package.target)? {
        return Err("Retry Setup to use the current engine checkpoint for this unfinished installation.".into());
    }
    super::preflight(adapter.id(), saved.package.target)?;
    let work = work_path(game, &signature)?;
    let output = work.join("output");
    if output.exists() { fs::remove_dir_all(&output).map_err(|e| e.to_string())?; }
    fs::create_dir_all(&output).map_err(|e| e.to_string())?;
    let context = Context { http, data, game, pending: saved, paths, work, output, progress, cancel };
    let result = build(&context)?;
    context.check_cancel()?;
    if super::input_signature_with_cancel(&profile, game, data, &|| context.stopped())? != signature {
        return Err("The selected inputs changed during preparation. Select them again and retry Setup.".into());
    }
    register_with_cancel(game, &signature, result, &|| context.stopped())
}

fn relative(path: &Path) -> bool {
    let text = path.to_string_lossy();
    !text.is_empty() && !text.contains([':', '{', '}']) && (cfg!(windows) || !text.contains('\\'))
        && path.components().all(|part| matches!(part, Component::Normal(_)))
}

fn same_game(expected: &EngineGame, actual: &GameIdentity) -> bool {
    let console = serde_json::to_value(&actual.console).ok();
    console.as_ref().and_then(|v| v.as_str()) == Some(&expected.console)
        && expected.title_id.eq_ignore_ascii_case(&actual.title_id)
        && (expected.version.is_empty() || expected.version == actual.version)
        && (expected.build_id.is_empty() || expected.build_id.eq_ignore_ascii_case(&actual.build_id))
}

pub fn file_sha256(path: &Path) -> Result<String, String> {
    file_sha256_with_cancel(path, &|| false)
}

fn file_sha256_with_cancel(path: &Path, stopped: &dyn Fn() -> bool) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut source = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        if stopped() { return Err("Stopped.".into()); }
        let count = source.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 { break; }
        hash.update(&buffer[..count]);
    }
    Ok(hex::encode(hash.finalize()))
}

fn runtime_digest(root: &Path, program: &str, required: &[PathBuf], stopped: &dyn Fn() -> bool) -> Result<String, String> {
    let mut files: Vec<_> = required.iter().map(|path| path.to_string_lossy().replace('\\', "/")).collect();
    files.push(program.replace('\\', "/"));
    files.sort();
    files.dedup();
    let hashes = files.iter().map(|path| file_sha256_with_cancel(&root.join(path), stopped).map(|digest| (path, digest))).collect::<Result<Vec<_>, _>>()?;
    Ok(super::fingerprint(&hashes))
}

pub fn register(game: &Installed, signature: &str, output: Output) -> Result<Installed, String> {
    register_with_cancel(game, signature, output, &|| false)
}

pub fn register_with_cancel(game: &Installed, signature: &str, output: Output, stopped: &dyn Fn() -> bool) -> Result<Installed, String> {
    if stopped() { return Err("Stopped.".into()); }
    let saved = pending::read(game)?.ok_or("This game is already prepared.")?;
    let record = game.path.join(".engine-prepared.json");
    if record.exists() || output_present(&game.path) {
        return Err("Existing game output has been kept. Recover it before preparing another build.".into());
    }
    output.identity.validate()?;
    if !same_game(&saved.game, &output.identity) { return Err("The prepared files belong to a different game or revision.".into()); }
    if output.start.program.len() != 1 || !relative(Path::new(&output.start.program[0])) {
        return Err("An engine must provide one exact native output program.".into());
    }
    if output.files.iter().chain(&output.required_files).chain(&output.saves).any(|path| !relative(path)) {
        return Err("Prepared file and save paths must stay inside the output folder.".into());
    }
    let mut owned = output.files.clone();
    owned.extend(output.required_files.iter().cloned());
    owned.push(output.start.program[0].clone().into());
    owned.sort();
    owned.dedup();
    // A private compiler may have thousands of headers. Its owned parent already excludes
    // them from save backups; keep the full required-file list only in the frozen build pin.
    let mut roots: Vec<PathBuf> = vec![];
    for path in owned {
        if roots.last().is_some_and(|parent| path.starts_with(parent)) { continue; }
        roots.push(path);
    }
    let owned = roots;
    if output.saves.iter().any(|save| owned.iter().any(|data| save.starts_with(data) || data.starts_with(save))) {
        return Err("Game files and saves must have separate owned paths.".into());
    }
    let stage = work_path(game, signature)?.join("output");
    coordinator::validate_tree(&stage)?;
    let program = &output.start.program[0];
    let runtime_hash = runtime_digest(&stage, program, &output.required_files, stopped)?;
    let mut start = output.start.clone();
    start.program = vec![format!("{DIRECTORY}/{}", program.replace('\\', "/"))];
    let profile = Profile {
        format: 1, start: Some(start),
        saves: output.saves.iter().map(|p| format!("{{install}}/{DIRECTORY}/{}", p.to_string_lossy().replace('\\', "/"))).collect(),
        ..Default::default()
    };
    let recipe = super::fingerprint(&(saved.package.identity(), signature, &output.identity));
    let pin = BuildPin {
        format: 1, game: output.identity.clone(), package: saved.package.clone(), runtime: saved.package.clone(),
        components: Components { code: recipe.clone(), shaders: recipe, runtime: runtime_hash, data: super::fingerprint(&signature) },
        profile, required_files: output.required_files.iter().map(|p| PathBuf::from(format!("{DIRECTORY}/{}", p.to_string_lossy().replace('\\', "/")))).collect(),
    };
    pin.validate()?;
    let mut ready = game.clone();
    ready.exe = Some(Path::new(DIRECTORY).join(program));
    ready.engine_build = Some(serde_json::to_value(pin).map_err(|e| e.to_string())?);
    ready.version = output.identity.version;
    ready.tag = super::package_label(&saved.package);
    ready.sha256 = saved.package.archive_sha256.clone();
    ready.asset = "Prepared native game".into();
    ready.verified = true;
    ready.auto_update = false;
    ready.pinned = true;
    ready.inputs_ready = signature.into();
    ready.files = owned.iter().map(|p| Path::new(DIRECTORY).join(p)).collect();
    check_staged(&ready, &stage, stopped)?;
    if stopped() { return Err("Stopped.".into()); }
    let temp = game.path.join(".engine-prepared.json.tmp");
    let bytes = serde_json::to_vec_pretty(&Prepared { pending: saved, ready: ready.clone() }).map_err(|e| e.to_string())?;
    fs::write(&temp, bytes).map_err(|e| e.to_string())?;
    fs::OpenOptions::new().write(true).open(&temp).and_then(|file| file.sync_all()).map_err(|e| e.to_string())?;
    fs::rename(temp, record).map_err(|e| e.to_string())?;
    Ok(ready)
}

fn check_staged(ready: &Installed, stage: &Path, stopped: &dyn Fn() -> bool) -> Result<(), String> {
    let root = ready.path.canonicalize().map_err(|e| e.to_string())?;
    let directory = fs::symlink_metadata(stage).map_err(|e| e.to_string())?;
    if !directory.is_dir() || !stage.canonicalize().map_err(|e| e.to_string())?.starts_with(root) {
        return Err("The completed output folder must remain inside its original installation.".into());
    }
    let mut check = ready.clone();
    check.path = stage.into();
    let mut pin = super::read_build(ready)?.ok_or("Missing prepared pin.")?;
    let start = pin.profile.start.as_mut().ok_or("Missing prepared program.")?;
    start.program[0] = start.program[0].strip_prefix(&format!("{DIRECTORY}/")).ok_or("Invalid prepared program path.")?.into();
    for path in &mut pin.required_files { *path = path.strip_prefix(DIRECTORY).map_err(|e| e.to_string())?.into(); }
    let target = pin.package.target;
    let digest = runtime_digest(stage, &pin.profile.start.as_ref().unwrap().program[0], &pin.required_files, stopped)?;
    if digest != pin.components.runtime { return Err("The completed runtime changed after preparation. Its files have been kept; restore the original output before registering it.".into()); }
    check.engine_build = Some(serde_json::to_value(pin).map_err(|e| e.to_string())?);
    super::pinned_executable(&check, target)?;
    Ok(())
}

pub fn recover(game: &Installed, signature: &str) -> Result<Option<Installed>, String> {
    let path = game.path.join(".engine-prepared.json");
    if !path.exists() { return Ok(None); }
    let stored: Prepared = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let saved = pending::read(game)?.ok_or("This game is already prepared.")?;
    let ready = stored.ready;
    let pin = super::read_build(&ready)?.ok_or("Missing prepared pin.")?;
    let same = serde_json::to_value(&saved).unwrap() == serde_json::to_value(&stored.pending).unwrap();
    if !same || ready.id != game.id || ready.path != game.path || ready.installed_at != game.installed_at
        || ready.inputs_ready != signature || pin.package != saved.package {
        return Err("A completed preparation is waiting for its original installation and inputs. Its files have been kept.".into());
    }
    Ok(Some(ready))
}

pub fn activate(current: &Installed, ready: &Installed, stopped: &dyn Fn() -> bool) -> Result<Installed, String> {
    if stopped() { return Err("Stopped.".into()); }
    let stored = recover(current, &ready.inputs_ready)?.ok_or("Missing completed preparation record.")?;
    if stored.engine_build != ready.engine_build { return Err("The result does not match the recorded preparation.".into()); }
    let mut active = current.clone();
    active.engine_build = stored.engine_build;
    active.exe = stored.exe;
    active.version = stored.version;
    active.tag = stored.tag;
    active.asset = stored.asset;
    active.sha256 = stored.sha256;
    active.verified = stored.verified;
    active.inputs_ready = stored.inputs_ready;
    active.files = stored.files;
    active.auto_update = false;
    active.pinned = true;
    let stage = work_path(current, &active.inputs_ready)?.join("output");
    if output_present(&current.path) {
        if stage.exists() { return Err("An unrelated game output already exists. Its files have been kept.".into()); }
        let target = super::read_build(&active)?.unwrap().package.target;
        super::pinned_executable(&active, target)?;
        coordinator::validate_tree(&current.path.join(DIRECTORY))?;
        check_staged(&active, &current.path.join(DIRECTORY), stopped)?;
        return Ok(active);
    }
    coordinator::validate_tree(&stage)?;
    check_staged(&active, &stage, stopped)?;
    if stopped() { return Err("Stopped.".into()); }
    fs::rename(stage, current.path.join(DIRECTORY)).map_err(|e| e.to_string())?;
    Ok(active)
}

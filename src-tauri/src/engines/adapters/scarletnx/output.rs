//! Durable publication of a self-contained game. Recovery never invokes the compiler.
use crate::engines::{self, coordinator::validate_tree, pending, BuildPin, Components, GameIdentity, Target};
use crate::{library::Installed, profile::{Profile, Start}};
use serde::{Deserialize, Serialize};
use std::{fs, io::Read, path::{Path, PathBuf}};

#[derive(Serialize, Deserialize)]
struct Prepared { pending: pending::Pending, ready: Installed }

pub fn work_path(game: &Installed, signature: &str) -> Result<PathBuf, String> {
    let saved = pending::read(game)?.ok_or("This game is already prepared.")?;
    Ok(game.path.join(".engine-work").join(engines::fingerprint(&(saved.package.identity(), signature))))
}

fn digest(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new(); let mut buffer = [0; 65536];
    loop { let n = file.read(&mut buffer).map_err(|e| e.to_string())?; if n == 0 { break; } hash.update(&buffer[..n]); }
    Ok(hex::encode(hash.finalize()))
}

fn game_folder(output: &Path, title: &str) -> Result<PathBuf, String> {
    let folders = fs::read_dir(output).map_err(|e| e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e| e.to_string())?;
    let found: Vec<_> = folders.into_iter().filter(|f| f.file_name().to_string_lossy().eq_ignore_ascii_case(title)).collect();
    if found.len() != 1 || !found[0].path().is_dir() { return Err("The engine did not produce the selected game's folder.".into()); }
    Ok(found[0].path())
}

fn detach_source(settings: &Path) -> Result<(), String> {
    if !settings.exists() { return Ok(()); }
    let text = fs::read_to_string(settings).map_err(|e| e.to_string())?;
    let mut game_section = false;
    let lines: Vec<_> = text.lines().filter(|line| {
        let line = line.trim();
        if line.starts_with('[') { game_section = line.eq_ignore_ascii_case("[Game]"); }
        !(game_section && line.split_once('=').is_some_and(|(key,_)| key.trim().eq_ignore_ascii_case("source")))
    }).collect();
    fs::write(settings, format!("{}\n", lines.join("\n"))).map_err(|e| e.to_string())
}

pub fn register(game: &Installed, work: &Path, signature: &str, identity: GameIdentity, engine: &Path) -> Result<Installed, String> {
    let saved = pending::read(game)?.ok_or("This game is already prepared.")?;
    if work != work_path(game, signature)? || game.path.join(".scarletnx").exists() || game.path.join(".engine-prepared.json").exists() {
        return Err("Existing game output has been kept. Recover it before preparing another build.".into());
    }
    let output = work.join("output");
    validate_tree(&output)?;
    let folder = game_folder(&output, &saved.game.title_id)?;
    let name = folder.file_name().and_then(|s| s.to_str()).ok_or("Invalid output folder.")?;
    let data = folder.join(format!("{name}_data"));
    if !super::read_identity(&data, &saved.game.title_id)?.matches(&identity) { return Err("The prepared game has a different revision from the selected files.".into()); }
    let target = saved.package.target;
    let executable = if target == Target::WindowsX64 { format!("{name}.exe") } else { name.into() };
    let code = format!("{name}_code.{}", if target == Target::WindowsX64 { "dll" } else { "so" });
    if !engines::installed::native_binary(&folder.join(&code), target) || !engines::installed::native_binary(&folder.join(&executable), target) {
        return Err("The game has no compiled code and runtime for this computer.".into());
    }
    if digest(engine)? != digest(&folder.join(&executable))? { return Err("The prepared runtime does not match the selected engine package.".into()); }
    let stamp = fs::read_to_string(folder.join(format!("{name}.abi"))).map_err(|_| "The engine did not finish compiling this game.")?;
    if stamp.split_whitespace().next().and_then(|s| s.parse::<u32>().ok()).unwrap_or(0) == 0 { return Err("The game has no completed build stamp.".into()); }
    // Windows deploys the runtime's libraries beside the game; never fall back to a global PATH.
    if target == Target::WindowsX64 {
        for item in fs::read_dir(engine.parent().ok_or("Missing package folder.")?).map_err(|e| e.to_string())? {
            let item = item.map_err(|e| e.to_string())?;
            if item.path().extension().is_some_and(|s| s.eq_ignore_ascii_case("dll")) && digest(&item.path())? != digest(&folder.join(item.file_name()))? {
                return Err("A prepared runtime library does not match the pinned package.".into());
            }
        }
        if let Some(licenses) = engine.parent().and_then(Path::parent).map(|p| p.join("share/licenses")).filter(|p| p.is_dir()) {
            let dest = folder.join("licenses");
            if !dest.exists() { engines::coordinator::copy_tree(&licenses, &dest, &||false)?; }
        }
    }
    let settings = folder.join(format!("{name}.ini"));
    detach_source(&settings)?;
    // The portable root makes users and settings local too. Discard recorded absolute work paths.
    let folders = folder.join(format!("{name}.folders"));
    if folders.exists() { fs::remove_file(folders).map_err(|e|e.to_string())?; }
    if settings.exists() {
        let config = output.join("config").join(name);
        fs::create_dir_all(&config).map_err(|e|e.to_string())?;
        fs::rename(settings,config.join(format!("{name}.ini"))).map_err(|e|e.to_string())?;
    }
    let prefix = format!(".scarletnx/{name}");
    let program = format!("{prefix}/{executable}");
    // --data belongs to the preparation driver and prevents direct runtime launch.
    // The .scarletnx ancestor supplies portable config/users without driver-only flags.
    let profile = Profile { format:1, start:Some(Start { program:vec![program.clone()],
        args:vec![format!("--game={{install}}/{prefix}"), "--save_layout=game".into()],
        env:if target == Target::LinuxX64 { [("APPIMAGE_EXTRACT_AND_RUN".into(),"1".into())].into_iter().collect() } else { Default::default() } }),
        saves:vec![format!("{{install}}/{prefix}/{name}_data/save"), "{install}/.scarletnx/config".into(), "{install}/.scarletnx/users".into()], ..Default::default() };
    let mut required: Vec<PathBuf> = [code.as_str(), &format!("{name}.abi"), &format!("{name}_data/exefs/main"), &format!("{name}_data/exefs/main.npdm"), &format!("{name}_data/control.nacp")].into_iter().map(|p| PathBuf::from(format!("{prefix}/{p}"))).collect();
    let mut files: Vec<PathBuf> = [".scarletnx/cache", ".scarletnx/state", ".scarletnx/units"].into_iter().map(PathBuf::from).collect();
    for entry in fs::read_dir(&folder).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let file = entry.file_name().to_string_lossy().to_string();
        if file == format!("{name}_data") { continue; }
        let rel = PathBuf::from(format!("{prefix}/{file}"));
        if file.ends_with(".dll") { required.push(rel.clone()); }
        if !file.ends_with(".ini") { files.push(rel); }
    }
    for file in ["exefs","romfs","control.nacp"] { files.push(format!("{prefix}/{name}_data/{file}").into()); }
    let recipe = engines::fingerprint(&(saved.package.identity(),signature));
    let pin = BuildPin { format:1, game:identity.clone(), package:saved.package.clone(), runtime:saved.package.clone(),
        components:Components { code:recipe.clone(), shaders:recipe, runtime:digest(engine)?, data:engines::fingerprint(&signature) }, profile, required_files:required };
    pin.validate()?;
    let mut ready = game.clone();
    ready.engine_build = Some(serde_json::to_value(pin).map_err(|e| e.to_string())?);
    ready.exe = Some(program.into()); ready.version = identity.version; ready.tag = engines::package_label(&saved.package);
    ready.sha256 = saved.package.archive_sha256.clone(); ready.asset = "Prepared native game".into();
    ready.verified = true; ready.auto_update = false; ready.pinned = true; ready.inputs_ready = signature.into(); ready.files = files;
    // Validate the staged tree using the same relative layout Play will use after activation.
    check_staged(&ready, &output)?;
    let temp = game.path.join(".engine-prepared.json.tmp");
    fs::write(&temp, serde_json::to_vec_pretty(&Prepared { pending:saved, ready:ready.clone() }).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    fs::OpenOptions::new().write(true).open(&temp).and_then(|f| f.sync_all()).map_err(|e| e.to_string())?;
    fs::rename(temp, game.path.join(".engine-prepared.json")).map_err(|e| e.to_string())?;
    Ok(ready)
}

fn check_staged(ready: &Installed, output: &Path) -> Result<(), String> {
    let mut check = ready.clone(); check.path = output.to_path_buf();
    let mut pin = engines::read_build(ready)?.ok_or("Missing prepared pin.")?;
    let start = pin.profile.start.as_mut().unwrap();
    start.program[0] = start.program[0].strip_prefix(".scarletnx/").ok_or("Invalid prepared path.")?.into();
    for file in &mut pin.required_files { *file = file.strip_prefix(".scarletnx").map_err(|e|e.to_string())?.to_path_buf(); }
    let target = pin.package.target;
    check.engine_build = Some(serde_json::to_value(pin).unwrap());
    engines::pinned_executable(&check, target)?;
    Ok(())
}

pub fn recover(game: &Installed, signature: &str) -> Result<Option<Installed>, String> {
    let record = game.path.join(".engine-prepared.json");
    if !record.exists() { return Ok(None); }
    let stored: Prepared = serde_json::from_slice(&fs::read(record).map_err(|e| e.to_string())?).map_err(|e|e.to_string())?;
    let current = pending::read(game)?.ok_or("This game is already prepared.")?;
    let ready = stored.ready;
    let build = engines::read_build(&ready)?.ok_or("Missing prepared identity.")?;
    if serde_json::to_value(current).unwrap() != serde_json::to_value(stored.pending).unwrap() || ready.id != game.id || ready.path != game.path || ready.installed_at != game.installed_at || ready.inputs_ready != signature || build.package != pending::read(game)?.unwrap().package {
        return Err("A completed preparation is waiting for its original installation and inputs. Its files have been kept.".into());
    }
    Ok(Some(ready))
}

pub fn activate(current: &Installed, ready: &Installed, stopped: &dyn Fn() -> bool) -> Result<Installed, String> {
    if stopped() { return Err("Stopped.".into()); }
    let stored = recover(current,&ready.inputs_ready)?.ok_or("Missing completed preparation record.")?;
    if stored.engine_build != ready.engine_build { return Err("The result does not match the recorded preparation.".into()); }
    let mut active = current.clone();
    active.engine_build = stored.engine_build; active.exe = stored.exe; active.version = stored.version; active.tag = stored.tag;
    active.asset = stored.asset; active.sha256 = stored.sha256; active.verified = stored.verified;
    active.inputs_ready = stored.inputs_ready; active.files = stored.files; active.auto_update = false; active.pinned = true;
    let staged = work_path(current,&active.inputs_ready)?.join("output");
    let target = engines::read_build(&active)?.unwrap().package.target;
    if current.path.join(".scarletnx").exists() {
        if staged.exists() { return Err("An unrelated game output already exists. Its files have been kept.".into()); }
        engines::pinned_executable(&active,target)?;
        return Ok(active);
    }
    validate_tree(&staged)?;
    check_staged(&active,&staged)?;
    if stopped() { return Err("Stopped.".into()); }
    fs::rename(staged,current.path.join(".scarletnx")).map_err(|e|e.to_string())?;
    Ok(active)
}

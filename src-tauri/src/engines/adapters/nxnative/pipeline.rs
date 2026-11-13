use crate::engines::{catalog::EngineGame, jobs::{self, Journal}, nxnative, pending, BuildPin, Components, Console, GameIdentity, PackagePin, Target};
use crate::{inputs, library::Installed, profile::{Profile, Start}};
use std::{fs, io::Read, path::Path, sync::{Arc, atomic::{AtomicBool, Ordering}}};

use crate::engines::coordinator::{copy_tree, validate_tree};
pub fn stage_inputs(paths: &std::collections::BTreeMap<String, std::path::PathBuf>, to: &Path, stopped: &dyn Fn() -> bool) -> Result<(), String> {
    if to.exists() { fs::remove_dir_all(to).map_err(|e| e.to_string())?; }
    fs::create_dir_all(to).map_err(|e| e.to_string())?;
    let base = paths.get("base").ok_or("Choose your game files first.")?;
    if base.is_dir() {
        if paths.keys().any(|k| k != "base") { return Err("For an extracted folder, put the extracted updates inside that folder. Separate encrypted updates cannot be combined with it.".into()); }
        let flat = base.join("exefs/main.npdm").is_file() && base.join("romfs").is_dir();
        let nested = base.join("base/exefs/main.npdm").is_file();
        let updates = fs::read_dir(base.join("updates")).into_iter().flatten().flatten().any(|e| e.path().join("exefs/main.npdm").is_file() && e.path().join("romfs").is_dir());
        if !flat && !nested && !updates { return Err("Choose the base NSP/XCI file and its update separately, or an extracted game folder with exefs/main.npdm and romfs.".into()); }
        return copy_tree(base, to, stopped);
    }
    for (id, path) in paths.iter().filter(|(k, _)| k.as_str() != "keys") {
        if stopped() { return Err("Stopped.".into()); }
        let ext = path.extension().and_then(|v| v.to_str()).ok_or("The game file has no extension.")?;
        let destination = to.join(format!("{id}.{ext}"));
        // The engine treats inputs as read-only; all generated files have a separate work path.
        if fs::hard_link(path, &destination).is_err() { fs::copy(path, destination).map_err(|e| e.to_string())?; }
    }
    Ok(())
}

pub fn register_output(g: &Installed, game: &EngineGame, work: &Path, source: &str, package: PackagePin) -> Result<Installed, String> {
    if pending::read(g)?.is_none() { return Err("This game is already prepared. Applying a fix requires a separate action.".into()); }
    let app = work.join("output/Game.app");
    validate_tree(&app)?;
    let info = plist::Value::from_file(app.join("Contents/Info.plist")).map_err(|e| e.to_string())?;
    let info = info.as_dictionary().ok_or("The prepared app has no metadata.")?;
    let title = info.get("DMTitleID").and_then(plist::Value::as_string).unwrap_or("");
    let version = info.get("CFBundleShortVersionString").and_then(plist::Value::as_string).unwrap_or("");
    if !title.eq_ignore_ascii_case(&game.title_id) || version != game.version { return Err("The prepared game does not match the selected title and revision.".into()); }
    let mut header = [0; 96];
    fs::File::open(work.join(format!("build/extract/{}/ExeFS/main",game.package))).map_err(|e| e.to_string())?.read_exact(&mut header).map_err(|e| e.to_string())?;
    let build_id = hex::encode(&header[64..96]);
    if &header[..4] != b"NSO0" || !build_id.starts_with(&game.build_id.to_ascii_lowercase()) { return Err("These game files have a build the selected profile does not support.".into()); }
    let program = "Game.app/Contents/MacOS/nxnative";
    let profile = Profile { format:1, start:Some(Start { program:vec![program.into()], args:vec![], env:[("DM_LAUNCHER".into(),"0".into())].into_iter().collect() }),
        saves:vec![format!("{{home}}/Library/Application Support/nxnative/{}/save",game.title_id)], ..Default::default() };
    let digest = |v: &str| crate::engines::fingerprint(&v);
    let pin = BuildPin { format:1, game:GameIdentity { console:if game.console == "switch2" {Console::Switch2} else {Console::Switch}, title_id:game.title_id.clone(), version:version.into(), build_id },
        runtime:package.clone(), package:package.clone(), components:Components { code:digest(&format!("{}:{source}",package.identity())), shaders:digest(&format!("{}:{source}",package.identity())), runtime:package.archive_sha256.clone(), data:digest(source) },
        profile, required_files:vec!["Game.app/Contents/Info.plist".into()] };
    pin.validate()?;
    let mut result = g.clone();
    result.engine_build = Some(serde_json::to_value(&pin).map_err(|e| e.to_string())?);
    result.exe = Some(program.into());
    result.version = game.version.clone();
    result.tag = format!("{} {}", package.engine, &package.upstream_revision[..12]);
    result.verified = true;
    result.sha256 = package.archive_sha256;
    result.asset = "Prepared Mac app".into();
    result.inputs_ready = source.into();
    result.auto_update = false;
    // Validate the exact launch target before moving any output into the active installation.
    let mut check = result.clone(); check.path = work.join("output");
    crate::engines::pinned_executable(&check, Target::MacosArm64)?;
    if g.path.join("Game.app").exists() { return Err("A game output is already present; it has been kept.".into()); }
    // Persist the intent atomically before moving the output. Recovery can finish either side
    // of the rename without rebuilding, even if writing the library fails afterwards.
    let temp = g.path.join(".engine-prepared.json.tmp");
    fs::write(&temp, serde_json::to_vec_pretty(&result).unwrap()).map_err(|e| e.to_string())?;
    // Windows requires a writable handle to flush this recovery record to disk.
    fs::OpenOptions::new().write(true).open(&temp).and_then(|f| f.sync_all()).map_err(|e| e.to_string())?;
    fs::rename(temp,g.path.join(".engine-prepared.json")).map_err(|e| e.to_string())?;
    Ok(result)
}

/// Called with the installation reservation and the app's setup-run lock held. Cancellation
/// is decided before publication; a recovered result goes through the same boundary.
pub fn activate_output(current: &Installed, ready: &Installed, stopped: &dyn Fn() -> bool) -> Result<Installed, String> {
    if stopped() { return Err("Stopped.".into()); }
    let pending=pending::read(current)?.ok_or("This installation is already prepared.")?;
    if current.id != ready.id || current.path != ready.path || current.installed_at != ready.installed_at { return Err("This is a different installation. The result has been kept.".into()); }
    let mut output=current.clone();
    output.engine_build=ready.engine_build.clone();output.exe=ready.exe.clone();output.version=ready.version.clone();
    output.asset=ready.asset.clone();output.sha256=ready.sha256.clone();output.verified=ready.verified;
    output.inputs_ready=ready.inputs_ready.clone();output.auto_update=false;output.pinned=true;
    let work=current.path.join(".engine-work").join(crate::engines::fingerprint(&(pending.package.identity(), &ready.inputs_ready)));
    let staged=work.join("output/Game.app");
    if current.path.join("Game.app").exists() {
        if staged.exists() { return Err("An unrelated output already occupies the installation. Its files have been kept.".into()); }
        crate::engines::pinned_executable(&output,Target::MacosArm64)?;
        return Ok(output);
    }
    let mut check=output.clone();check.path=work.join("output");
    crate::engines::pinned_executable(&check,Target::MacosArm64)?;
    validate_tree(&staged)?;
    if stopped() { return Err("Stopped.".into()); }
    fs::rename(staged,current.path.join("Game.app")).map_err(|e|e.to_string())?;
    Ok(output)
}

pub fn prepare(http: &reqwest::Client, data: &Path, g: &Installed, cancel: Arc<AtomicBool>, progress: &(dyn Fn(crate::engines::Progress) + Send + Sync)) -> Result<Installed, String> {
    let pending = pending::read(g)?.ok_or("This game is already prepared.")?;
    nxnative::preflight()?;
    let p = pending.game.setup_profile();
    let paths = inputs::resolve(&p, g, data)?;
    let signature = inputs::signature(&p, g, data)?;
    let recovered = g.path.join(".engine-prepared.json");
    if recovered.is_file() {
        let ready: Installed = serde_json::from_slice(&fs::read(recovered).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        if ready.id != g.id || ready.path != g.path || ready.inputs_ready != signature { return Err("A completed preparation is waiting for its original inputs. Its files have been kept.".into()); }
        return Ok(ready);
    }
    if pending.package != nxnative::pin() { return Err("Retry Setup to prepare this unfinished installation with the current nxnative checkpoint.".into()); }
    let work = g.path.join(".engine-work").join(crate::engines::fingerprint(&(pending.package.identity(), &signature)));
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let mut journal = Journal { state:"preparing".into(), installation:g.id.clone(), package:pending.package.identity(), inputs:signature.clone() };
    journal.save(&work)?;
    let stopped = || cancel.load(Ordering::Relaxed);
    let result = (|| {
        progress(crate::engines::Progress::message("Downloading components"));
        let components = |fraction: f32, text: String| {
            let mut p = crate::engines::Progress::message(text);
            if fraction.is_finite() && fraction > 0.0 { p.fraction = Some(fraction.min(1.0)); }
            progress(p);
        };
        let tools = tauri::async_runtime::block_on(nxnative::provision(http, data, cancel.clone(), &components)).map_err(|e| e.to_string())?;
        if !work.join("engine/.ready").exists() {
            let _ = fs::remove_dir_all(work.join("engine"));
            progress(crate::engines::Progress::message("Preparing components"));
            copy_tree(&tools.source, &work.join("engine"), &stopped)?;
            fs::write(work.join("engine/.ready"),pending.package.identity()).map_err(|e| e.to_string())?;
        }
        progress(crate::engines::Progress::message("Checking selected game files"));
        stage_inputs(&paths,&work.join("inputs"),&stopped)?;
        fs::create_dir_all(work.join("output")).map_err(|e| e.to_string())?;
        let cmd = nxnative::command(&pending.game, &tools.private, &work, paths.get("keys").map(|p| p.as_path()), Some(&tools.dotnet));
        let tracker = std::sync::Mutex::new(nxnative::ProgressTracker::default());
        jobs::run_process(&cmd, &g.path.join("preparation.log"), &stopped, &|v| {
            if let Some(p) = tracker.lock().unwrap().event(&v) { progress(p); }
        }).map_err(|e| nxnative::preparation_error(&pending.game,e))?;
        if stopped() { return Err("Stopped.".into()); }
        if inputs::signature(&p,g,data)? != signature { return Err("Your selected files changed during preparation. Choose them again before retrying.".into()); }
        progress(crate::engines::Progress::message("Checking the prepared game"));
        #[cfg(target_os="macos")]
        {
            let status = std::process::Command::new("/usr/bin/codesign").args(["--verify","--deep","--strict"]).arg(work.join("output/Game.app")).status().map_err(|e| e.to_string())?;
            if !status.success() { return Err("The prepared app's signature could not be verified.".into()); }
        }
        if stopped() { return Err("Stopped.".into()); }
        let mut pin = pending.package.clone();
        let xcode = std::process::Command::new("/usr/bin/xcodebuild").arg("-version").output().map_err(|e|e.to_string())?;
        let metal = std::process::Command::new("/usr/bin/xcrun").args(["-sdk","macosx","metal","--version"]).output().map_err(|e|e.to_string())?;
        pin.toolchain_sha256 = crate::engines::fingerprint(&(pin.toolchain_sha256, nxnative::manifest().dotnet.digest, xcode.stdout, metal.stdout));
        register_output(g, &pending.game, &work, &signature, pin)
    })();
    journal.state = if result.is_ok() {"complete"} else if stopped() {"cancelled"} else {"failed"}.into();
    let _ = journal.save(&work);
    result
}

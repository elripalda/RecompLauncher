use super::{input, Input};
use crate::engines::{coordinator, installed, jobs::CommandSpec, prepared::{Context, Output}, GameIdentity, Progress, Target};
use crate::profile::Start;
use std::{collections::BTreeSet, fs, path::{Path, PathBuf}};

fn program(target: Target) -> Result<&'static str, String> {
    match target { Target::LinuxX64 => Ok("eboot.elf"), Target::WindowsX64 => Ok("eboot.exe"), _ => Err("No pinned AnyPS5 runtime for this platform.".into()) }
}

pub fn convert_command(relinker: &Path, source: &Path, output: &Path, input: &Input, target: Target) -> Result<CommandSpec, String> {
    let mut command = CommandSpec::new(relinker, output);
    if target == Target::WindowsX64 { command.args.push("--windows".into()); }
    command.args.extend(["--to-intel", "--registry", "--no-patches", "--sce-module-path"].map(str::to_string));
    command.args.push(source.display().to_string());
    for directory in &input.module_directories { command.args.extend(["--module-dir".into(), directory.to_string_lossy().replace('\\', "/")]); }
    for executable in &input.executables { command.args.extend(["--executable".into(), executable.to_string_lossy().replace('\\', "/")]); }
    command.args.extend([source.join(&input.executable).display().to_string(), output.join(program(target)?).display().to_string()]);
    command.env.insert("ANYPS5_OPEN_URLS".into(), "never".into());
    command.remove_env = ["LD_PRELOAD", "LD_LIBRARY_PATH", "DYLD_INSERT_LIBRARIES", "DYLD_LIBRARY_PATH"].map(str::to_string).to_vec();
    Ok(command)
}

fn native(root: &Path, file: &Path, target: Target) -> Result<(), String> {
    if !installed::native_binary(&root.join(file), target) { return Err(format!("AnyPS5 did not produce a native file for this platform: {}", file.display())); }
    Ok(())
}

fn dependency(root: &Path, name: &str, target: Target) -> Result<Option<PathBuf>, String> {
    if let Some(relative) = name.strip_prefix("$ORIGIN/") {
        let path = PathBuf::from(relative);
        if input::safe_relative(&path) && root.join(&path).is_file() { return Ok(Some(path)); }
        return Err(format!("AnyPS5 is missing the converted dependency: {name}"));
    }
    let name_path = Path::new(name);
    if !input::safe_relative(name_path) || name_path.components().count() != 1 { return Err(format!("AnyPS5 produced a nonportable dependency: {name}")); }
    let bundled = Path::new("libs").join(name);
    if root.join(&bundled).is_file() { return Ok(Some(bundled)); }
    let system = match target {
        Target::LinuxX64 => ["libc.so.6", "libm.so.6", "libstdc++.so.6", "libgcc_s.so.1", "libdl.so.2", "libpthread.so.0", "ld-linux-x86-64.so.2"].contains(&name),
        Target::WindowsX64 => ["kernel32.dll", "ntdll.dll", "user32.dll", "advapi32.dll", "ws2_32.dll"].contains(&name.to_ascii_lowercase().as_str()),
        _ => false,
    };
    if system { return Ok(None); }
    Err(format!("AnyPS5 is missing the runtime dependency: {name}. The original folder and completed output have been kept."))
}

pub fn validate_runtime(root: &Path, target: Target) -> Result<(), String> {
    let files = input::files(root)?;
    let libraries: Vec<_> = files.iter().filter(|p| p.extension().is_some_and(|e| e == "prx")).collect();
    if libraries.len() != 407 { return Err("The pinned AnyPS5 runtime must contain its 407 native PRX libraries.".into()); }
    for library in libraries { native(root, library, target)?; }
    let support: &[&str] = match target {
        Target::WindowsX64 => &["libgcc_s_seh-1.dll", "libstdc++-6.dll", "libwinpthread-1.dll", "mimalloc.dll", "mimalloc-redirect.dll"],
        Target::LinuxX64 => &["libmimalloc.so"],
        _ => return Err("No pinned AnyPS5 runtime for this platform.".into()),
    };
    for name in support { native(root, Path::new(name), target)?; }
    if !root.join("licenses").is_dir() { return Err("The AnyPS5 runtime is missing its third-party licenses.".into()); }
    Ok(())
}

pub fn validate_output(root: &Path, target: Target) -> Result<Vec<PathBuf>, String> {
    let program = PathBuf::from(program(target)?);
    native(root, &program, target)?;
    let files = input::files(root)?;
    let mut required = BTreeSet::from([program, PathBuf::from("app0/sce_sys/param.json")]);
    for name in ["eboot.registry.json", "eboot.needed.json"] {
        let bytes = fs::read(root.join(name)).map_err(|e| format!("Missing AnyPS5 conversion record {name}: {e}"))?;
        serde_json::from_slice::<serde_json::Value>(&bytes).map_err(|e| format!("Invalid AnyPS5 conversion record: {e}"))?;
        required.insert(name.into());
    }
    for file in files {
        let name = file.to_string_lossy();
        let extension = file.extension().and_then(|s| s.to_str()).unwrap_or("");
        let runtime = file.starts_with("libs") && ["prx", "dll", "so"].contains(&extension);
        let generated = name.ends_with(".guest.prx") || (file.parent() == Some(Path::new("")) && ["elf", "exe"].contains(&extension));
        if runtime || generated { native(root, &file, target)?; required.insert(file.clone()); }
        if !name.ends_with(".needed.json") { continue; }
        let names: Vec<String> = serde_json::from_slice(&fs::read(root.join(&file)).map_err(|e| e.to_string())?).map_err(|e| format!("Invalid dependency record: {e}"))?;
        for name in names {
            if let Some(path) = dependency(root, &name, target)? { native(root, &path, target)?; required.insert(path); }
        }
        required.insert(file);
    }
    for file in &required {
        if !root.join(file).is_file() { return Err(format!("Missing AnyPS5 output: {}", file.display())); }
    }
    Ok(required.into_iter().collect())
}

pub fn output(root: &Path, identity: GameIdentity, target: Target) -> Result<Output, String> {
    let required_files = validate_output(root, target)?;
    let start = Start {
        program:vec![program(target)?.into()], args:vec![],
        env:std::collections::BTreeMap::from([
            ("ANYPS5_OPEN_URLS".into(), "never".into()),
            ("ANYPS5_SHADER_CACHE_DIR".into(), "{install}/Game/shader_cache".into()),
        ]),
    };
    let saves = ["_sd", "_sd_mem", "_sd_backup", "_sd_backup_tmp", "_sd_backup_old", "_sd_restore_tmp", "_sd_restore_old", "_trophy", "anyps5-input.ini", "anyps5-entitlements.ini"].map(PathBuf::from).to_vec();
    let mut files = vec![PathBuf::from("app0"), PathBuf::from("libs"), PathBuf::from("shader_cache"), PathBuf::from("data"), PathBuf::from("download0")];
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_type().map_err(|e| e.to_string())?.is_file() { files.push(PathBuf::from(entry.file_name())); }
    }
    Ok(Output { identity, start, saves, files, required_files })
}

pub(super) fn build(context: &Context<'_>) -> Result<Output, String> {
    let source = context.paths.get("base").ok_or("Select your decrypted PS5 game folder.")?;
    let input = super::inspect(source, &context.pending.game.title_id)?;
    context.check_cancel()?;
    let target = context.pending.package.target;
    let packages = super::packages(target)?;
    let relinker = context.download(&packages.relinker)?;
    let runtime = context.unpack(&packages.runtime)?;
    validate_runtime(&runtime, target)?;
    let private_relinker = context.work.join(if target == Target::WindowsX64 { "relinker.exe" } else { "relinker" });
    fs::copy(relinker, &private_relinker).map_err(|e| e.to_string())?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&private_relinker, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    native(&context.work, private_relinker.file_name().unwrap().as_ref(), target)?;
    (context.progress)(Progress::message("Copying the original game files and pinned AnyPS5 runtime"));
    let app = context.output.join("app0");
    coordinator::copy_tree(source, &app, &|| context.stopped())?;
    let copied = super::inspect(&app, &context.pending.game.title_id)?;
    if !copied.identity.matches(&input.identity) { return Err("The game executable or metadata changed while being copied. Select the original folder again.".into()); }
    coordinator::copy_tree(&runtime, &context.output.join("libs"), &|| context.stopped())?;
    fs::write(context.output.join("LICENSE.AnyPS5"), include_str!("LICENSE.AnyPS5")).map_err(|e| e.to_string())?;
    let command = convert_command(&private_relinker, &app, &context.output, &input, target)?;
    context.run(&command, "Converting the executable and bundled modules with AnyPS5")?;
    if !app.join("eboot.bin").exists() { fs::copy(app.join(&input.executable), app.join("eboot.bin")).map_err(|e| e.to_string())?; }
    // The generated desktop shortcut embeds staging paths; Play uses the native executable.
    for entry in fs::read_dir(&context.output).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_some_and(|ext| ext == "desktop") { fs::remove_file(path).map_err(|e| e.to_string())?; }
    }
    context.check_cancel()?;
    output(&context.output, input.identity, target)
}

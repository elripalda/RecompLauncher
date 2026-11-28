use crate::engines::{Console, GameIdentity, coordinator, jobs::CommandSpec, prepared::{self, Output}};
use crate::profile::Start;
use std::{fs, io::Read, path::{Path, PathBuf}};

fn header(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    fs::File::open(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?.take(limit).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}
fn be32(bytes: &[u8], offset: usize) -> u32 { u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) }

pub fn read_identity(root: &Path, title: &str, system: &str) -> Result<GameIdentity, String> {
    let boot = header(&root.join("sys/boot.bin"), 0x440)?;
    if boot.len() < 0x60 || !boot[..6].iter().all(u8::is_ascii_alphanumeric) {
        return Err("The extracted disc has invalid boot metadata.".into());
    }
    let actual_system = match (be32(&boot, 0x18), be32(&boot, 0x1c)) {
        (0x5d1c9ea3, _) => "wii",
        (_, 0xc2339f3d) => "gc",
        _ => return Err("The selected files are not a GameCube or Wii disc.".into()),
    };
    let actual_title = std::str::from_utf8(&boot[..6]).map_err(|e| e.to_string())?;
    if actual_title != title || actual_system != system { return Err("These disc files belong to a different game or system.".into()); }
    if boot[6] != 0 { return Err("This adapter currently prepares the first disc only. Select disc 1.".into()); }
    let path = root.join("sys/main.dol");
    let dol = header(&path, 0x100)?;
    if dol.len() < 0x100 { return Err("The game's main DOL header is incomplete.".into()); }
    let size = fs::metadata(&path).map_err(|e| e.to_string())?.len();
    let entry = u64::from(be32(&dol, 0xe0));
    let mut executable_entry = false;
    for section in 0..18 {
        let offset = u64::from(be32(&dol, section * 4));
        let address = u64::from(be32(&dol, 0x48 + section * 4));
        let length = u64::from(be32(&dol, 0x90 + section * 4));
        if length == 0 { continue; }
        if offset < 0x100 || address == 0 || offset + length > size || address + length > 0x1_0000_0000 {
            return Err("The game's main DOL section table is invalid.".into());
        }
        executable_entry |= section < 7 && entry >= address && entry < address + length;
    }
    if !executable_entry || !root.join("files").is_dir() { return Err("The extracted disc is missing valid executable code or game files.".into()); }
    Ok(GameIdentity { console:Console::Other(system.into()), title_id:actual_title.into(), version:boot[7].to_string(), build_id:prepared::file_sha256(&path)? })
}

pub fn extract_command(program: &Path, work: &Path, base: &Path) -> CommandSpec {
    let mut command = CommandSpec::new(program, work);
    command.args = vec!["--extract".into(), base.display().to_string()];
    command.env.insert("XDG_DATA_HOME".into(), work.join("extract-user").display().to_string());
    command
}

pub fn compile_command(program: &Path, work: &Path, root: &Path) -> CommandSpec {
    let mut command = CommandSpec::new(program, work);
    command.args = vec!["build".into(), root.display().to_string(), "--backend".into(), "c".into(), "--toolchain".into(), "clang".into(), "--output".into(), work.join("modules").display().to_string()];
    command
}

fn file_list(root: &Path) -> Result<Vec<PathBuf>, String> {
    coordinator::validate_tree(root)?;
    let mut stack = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(path) = stack.pop() {
        for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.file_type().map_err(|e| e.to_string())?.is_dir() { stack.push(entry.path()); }
            else if entry.path().is_file() { files.push(entry.path().strip_prefix(root).unwrap().to_path_buf()); }
            else { return Err("The prepared output contains an unsupported file or directory link.".into()); }
        }
    }
    files.sort();
    Ok(files)
}

pub fn frozen_output(root: &Path, identity: GameIdentity) -> Result<Output, String> {
    let module = format!("Runtime/g{}_recomp.dylib", identity.title_id);
    let files = file_list(root)?;
    for file in ["Runtime/moderngekko-run", "Runtime/receipt.json", &module] {
        if !files.contains(&PathBuf::from(file)) { return Err(format!("Prepared output is missing {file}.")); }
    }
    let mut saves: Vec<PathBuf> = ["User/Config", "User/config.ini", "User/StateSaves", "User/Wii/shared2/sys/SYSCONF"].iter().map(PathBuf::from).collect();
    if identity.console == Console::Other("gc".into()) {
        for region in ["USA", "EUR", "JAP", "JPN", "DEV"] {
            saves.extend([format!("User/GC/{region}/Card A").into(), format!("User/GC/{region}/Card B").into()]);
        }
        saves.extend(["User/GC/SRAM.raw".into(), "User/GC/MemoryCardA.*.raw".into(), "User/GC/MemoryCardB.*.raw".into()]);
    } else {
        let tmd = header(&root.join("Disc/tmd.bin"), 0x1e4)?;
        if tmd.len() < 0x1e4 || be32(&tmd, 0) != 0x00010001 { return Err("The Wii disc is missing its partition title metadata.".into()); }
        let high = be32(&tmd, 0x18c);
        if ![0x00010000, 0x00010004].contains(&high) { return Err("The Wii partition is not a game title.".into()); }
        let low = be32(&tmd, 0x190);
        if low.to_be_bytes() != identity.title_id.as_bytes()[..4] { return Err("The Wii partition title does not match the disc.".into()); }
        saves.push(format!("User/Wii/title/{high:08x}/{low:08x}/data").into());
    }
    let start = Start {
        program:vec!["Runtime/moderngekko-run".into()],
        args:vec!["--game".into(), "{install}/Game/Disc".into(), "--module".into(), format!("{{install}}/Game/{module}"), "--user-dir".into(), "{install}/Game/User".into(), "--graphics".into(), "Metal".into()],
        ..Default::default()
    };
    // Save exclusion checks need only owned roots; the frozen pin still lists every required file.
    Ok(Output { identity, start, saves, required_files:files, files:vec!["Disc".into(), "Runtime".into()] })
}

fn copy_notices(source: &Path, destination: &Path, stopped: &dyn Fn() -> bool) -> Result<(), String> {
    for relative in file_list(source)? {
        if stopped() { return Err("Stopped.".into()); }
        let name = relative.to_string_lossy().to_ascii_lowercase();
        if !["license", "copying", "copyright", "notice", "provenance", "authors", "contributors"].iter().any(|s| name.contains(s)) { continue; }
        let target = destination.join(&relative);
        fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
        fs::copy(source.join(relative), target).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn native_module(bytes: &[u8]) -> bool {
    bytes.len() >= 32 && bytes[..4] == 0xfeedfacfu32.to_le_bytes()
        && bytes[4..8] == 0x0100000cu32.to_le_bytes() && bytes[12..16] == 6u32.to_le_bytes()
}

fn native_dependencies(path: &Path) -> Result<(), String> {
    let valid = if path.extension().is_some_and(|e| e == "dylib") {
        native_module(&header(path, 32)?)
    } else {
        crate::engines::installed::native_binary(path, crate::engines::Target::MacosArm64)
    };
    if !valid {
        return Err("The engine produced a program or module for the wrong computer.".into());
    }
    let inspected = std::process::Command::new("/usr/bin/otool").arg("-L").arg(path).output().map_err(|e| e.to_string())?;
    if !inspected.status.success() { return Err("The engine's native library dependencies could not be inspected.".into()); }
    for line in String::from_utf8_lossy(&inspected.stdout).lines().skip(1) {
        let dependency = line.trim().split(" (compatibility").next().unwrap_or_default();
        if dependency.starts_with("/usr/lib/") || dependency.starts_with("/System/Library/") { continue; }
        // otool prints a dylib's own install name before its dependencies.
        if path.extension().is_some_and(|e| e == "dylib") && Path::new(dependency).file_name() == path.file_name() { continue; }
        return Err(format!("The engine requires an unpackaged library: {dependency}. Its output has been kept."));
    }
    Ok(())
}

pub fn build(ctx: &prepared::Context<'_>) -> Result<Output, String> {
    ctx.check_cancel()?;
    let selected = &ctx.pending.game;
    if selected.title_id.len() != 6 || !selected.title_id.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err("The game does not have an exact upstream disc ID.".into());
    }
    let base = ctx.paths.get("base").ok_or("Choose your game disc or extracted game folder.")?;
    let disc = ctx.output.join("Disc");
    if base.is_dir() {
        read_identity(base, &selected.title_id, &selected.console)?;
        coordinator::copy_tree(base, &disc, &|| ctx.stopped())?;
    }
    let tools = super::toolchain::build_tools(ctx)?;
    if !base.is_dir() {
        let extraction = ctx.work.join("extract-user");
        if extraction.exists() { fs::remove_dir_all(&extraction).map_err(|e| e.to_string())?; }
        let mut command = extract_command(&tools.bin.join("ModernGekko"), &ctx.work, base);
        command.env.extend(tools.env.clone());
        command.remove_env = tools.remove_env.clone();
        ctx.run(&command, "Extracting your GameCube/Wii disc")?;
        let extracted = extraction.join("moderngekko/games").join(&selected.title_id);
        read_identity(&extracted, &selected.title_id, &selected.console)?;
        coordinator::copy_tree(&extracted, &disc, &|| ctx.stopped())?;
    }
    let identity = read_identity(&disc, &selected.title_id, &selected.console)?;
    let mut compile = compile_command(&tools.bin.join("moderngekko-port"), &ctx.work, &disc);
    compile.env = tools.env.clone();
    compile.remove_env = tools.remove_env.clone();
    ctx.run(&compile, "Recompiling your game into its native module")?;
    let active = ctx.work.join("modules").join(&identity.title_id).join("active-module.txt");
    let recorded = fs::read_to_string(active).map_err(|e| format!("The compiler did not publish its module: {e}"))?;
    let module = PathBuf::from(recorded.trim()).canonicalize().map_err(|e| e.to_string())?;
    let modules = ctx.work.join("modules").canonicalize().map_err(|e| e.to_string())?;
    if !module.starts_with(&modules) || !module.is_file() { return Err("The compiler's module is outside this preparation.".into()); }
    let manifest = fs::read_to_string(module.parent().unwrap().join("manifest.txt")).map_err(|e| e.to_string())?;
    let expected_id = format!("disc_id={}", identity.title_id);
    let expected_dol = format!("dol_sha256={}", identity.build_id);
    if !manifest.lines().any(|l| l == expected_id) || !manifest.lines().any(|l| l == expected_dol) { return Err("The compiled module does not match the selected disc revision.".into()); }
    let mut inspect = CommandSpec::new(tools.bin.join("moderngekko-module-info"), &ctx.work);
    inspect.args = vec![module.display().to_string(), identity.title_id.clone()];
    ctx.run(&inspect, "Checking the prepared native module")?;
    let runtime = ctx.output.join("Runtime");
    fs::create_dir_all(&runtime).map_err(|e| e.to_string())?;
    fs::copy(tools.bin.join("moderngekko-run"), runtime.join("moderngekko-run")).map_err(|e| e.to_string())?;
    let final_module = runtime.join(format!("g{}_recomp.dylib", identity.title_id));
    fs::copy(&module, &final_module).map_err(|e| e.to_string())?;
    coordinator::copy_tree(&tools.bin.join("Sys"), &runtime.join("Sys"), &|| ctx.stopped())?;
    copy_notices(&tools.source, &runtime.join("Licenses"), &|| ctx.stopped())?;
    fs::write(runtime.join("Licenses/Ninja-LICENSE"), include_bytes!("Ninja-LICENSE")).map_err(|e| e.to_string())?;
    fs::write(runtime.join("receipt.json"), serde_json::to_vec_pretty(&tools.receipt).unwrap()).map_err(|e| e.to_string())?;
    fs::write(runtime.join("module-manifest.txt"), manifest).map_err(|e| e.to_string())?;
    for native in [runtime.join("moderngekko-run"), final_module] {
        native_dependencies(&native)?;
        let mut sign = CommandSpec::new("/usr/bin/codesign", &ctx.work);
        sign.args = vec!["--force".into(), "--sign".into(), "-".into(), native.display().to_string()];
        ctx.run(&sign, "Signing the prepared native runtime")?;
    }
    frozen_output(&ctx.output, identity)
}

#[cfg(test)]
mod tests {
    #[test]
    fn cancellation_stops_before_input_copy_or_tool_provisioning() {
        use crate::engines::{catalog, pending::Pending, prepared::Context, Target};
        use std::sync::{Arc, atomic::AtomicBool};
        let title = catalog::game("moderngekko-gmse01").unwrap();
        let game = crate::library::Installed::default();
        let http = reqwest::Client::new();
        let root = std::env::temp_dir().join(format!("moderngekko-cancel-{}", std::process::id()));
        let ctx = Context {
            http:&http, data:&root, game:&game,
            pending:Pending { input_profile:None, input_signature_version:Default::default(), format:1, generation:"cancel-fixture".into(), state:"pending".into(), game:title, package:super::super::pin(Target::MacosArm64).unwrap() },
            paths:Default::default(), work:root.join("work"), output:root.join("output"),
            progress:&|_| panic!("Cancelled preparation must not provision tools"), cancel:Arc::new(AtomicBool::new(true)),
        };
        assert_eq!(super::build(&ctx).err().unwrap(), "Stopped.");
        assert!(!root.exists());
    }

    #[test]
    fn notices_include_spdx_license_directories() {
        use std::fs;
        let root = std::env::temp_dir().join(format!("moderngekko-licenses-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("source/LICENSES")).unwrap();
        fs::write(root.join("source/LICENSES/MIT.txt"), "SPDX license text").unwrap();
        super::copy_notices(&root.join("source"), &root.join("output"), &|| false).unwrap();
        assert!(root.join("output/LICENSES/MIT.txt").is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn native_module_requires_an_arm64_dylib() {
        let mut header = [0; 32];
        header[..4].copy_from_slice(&0xfeedfacfu32.to_le_bytes());
        header[4..8].copy_from_slice(&0x0100000cu32.to_le_bytes());
        header[12..16].copy_from_slice(&6u32.to_le_bytes());
        assert!(super::native_module(&header));
        header[12..16].copy_from_slice(&2u32.to_le_bytes());
        assert!(!super::native_module(&header));
        header[12..16].copy_from_slice(&6u32.to_le_bytes());
        header[4..8].copy_from_slice(&0x01000007u32.to_le_bytes());
        assert!(!super::native_module(&header));
    }
}

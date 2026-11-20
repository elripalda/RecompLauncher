use super::{command, identify, manifest, progress};
use crate::engines::{coordinator, jobs::{self, CommandSpec}, prepared};
use crate::profile::Start;
use serde_json::json;
use std::{collections::BTreeMap, fs, path::{Path, PathBuf}, process::Command};

pub(super) fn isolated(program: impl Into<PathBuf>, cwd: &Path) -> CommandSpec {
    let mut spec = CommandSpec::new(program, cwd);
    spec.remove_env = ["CPATH", "C_INCLUDE_PATH", "CPLUS_INCLUDE_PATH", "LIBRARY_PATH", "DYLD_LIBRARY_PATH", "DYLD_FRAMEWORK_PATH", "CFLAGS", "CXXFLAGS", "CPPFLAGS", "LDFLAGS", "SDKROOT", "MACOSX_DEPLOYMENT_TARGET", "CMAKE_PREFIX_PATH", "CMAKE_TOOLCHAIN_FILE", "PKG_CONFIG_PATH", "PKG_CONFIG_LIBDIR", "CC", "CXX"].iter().map(|s| (*s).into()).collect();
    spec.env.insert("PATH".into(), "/usr/bin:/bin:/usr/sbin:/sbin".into());
    spec
}

fn run(ctx: &prepared::Context<'_>, program: impl Into<PathBuf>, cwd: &Path, args: Vec<String>, text: &str) -> Result<(), String> {
    let mut spec = isolated(program, cwd);
    spec.args = args;
    ctx.run(&spec, text)
}

fn tool_output(program: &str, args: &[&str]) -> Result<String, String> {
    let result = Command::new(program).args(args).output().map_err(|e| e.to_string())?;
    if !result.status.success() { return Err(format!("Could not inspect the build tool: {program}")); }
    Ok(String::from_utf8_lossy(&result.stdout).trim().into())
}

fn tools(ctx: &prepared::Context<'_>) -> Result<PathBuf, String> {
    let manifest = manifest();
    let root = ctx.work.join("tools");
    let receipt = json!({"manifest":super::MANIFEST, "clang":tool_output("/usr/bin/xcrun", &["clang", "--version"])?, "sdk":tool_output("/usr/bin/xcrun", &["--show-sdk-version"])?});
    let marker = root.join("receipt.json");
    if fs::read(&marker).ok().and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok()).as_ref() == Some(&receipt) { return Ok(root); }
    if root.exists() { fs::remove_dir_all(&root).map_err(|e| e.to_string())?; }
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let engine = ctx.unpack(&manifest.engine)?;
    coordinator::copy_tree(&engine, &root, &|| ctx.stopped())?;
    let cmake = ctx.unpack(&manifest.cmake)?.join("bin/cmake");
    let ft = ctx.unpack(&manifest.freetype)?;
    let ft_build = root.join("freetype-build");
    let ft_install = root.join("3rdparty/freetype");
    run(ctx, &cmake, &root, vec!["-S".into(), ft.display().to_string(), "-B".into(), ft_build.display().to_string(), "-G".into(), "Unix Makefiles".into(),
        format!("-DCMAKE_INSTALL_PREFIX={}", ft_install.display()), "-DCMAKE_BUILD_TYPE=Release".into(), "-DBUILD_SHARED_LIBS=OFF".into(), "-DFT_DISABLE_HARFBUZZ=ON".into(), "-DFT_DISABLE_BROTLI=ON".into(), "-DFT_DISABLE_PNG=ON".into(), "-DFT_DISABLE_ZLIB=ON".into(), "-DFT_DISABLE_BZIP2=ON".into()], "Configuring private font rendering tools")?;
    run(ctx, &cmake, &root, vec!["--build".into(), ft_build.display().to_string(), "--parallel".into(), workers()], "Building private font rendering tools")?;
    run(ctx, &cmake, &root, vec!["--install".into(), ft_build.display().to_string()], "Installing private font rendering tools")?;
    let ff = ctx.unpack(&manifest.ffmpeg)?;
    let ff_source = root.join("ffmpeg-source");
    coordinator::copy_tree(&ff, &ff_source, &|| ctx.stopped())?;
    let ff_install = root.join("ffmpeg");
    run(ctx, ff_source.join("configure"), &ff_source, vec![format!("--prefix={}", ff_install.display()), "--cc=/usr/bin/clang".into(), "--cxx=/usr/bin/clang++".into(),
        "--disable-programs".into(), "--disable-doc".into(), "--disable-debug".into(), "--disable-shared".into(), "--enable-static".into(), "--disable-autodetect".into(), "--disable-network".into(),
        "--disable-videotoolbox".into(), "--disable-audiotoolbox".into(), "--disable-avfoundation".into(), "--disable-coreimage".into(), "--disable-securetransport".into(), "--disable-iconv".into()], "Configuring private media libraries")?;
    run(ctx, "/usr/bin/make", &ff_source, vec![format!("-j{}", workers())], "Building private media libraries")?;
    run(ctx, "/usr/bin/make", &ff_source, vec!["install".into()], "Installing private media libraries")?;
    fs::create_dir_all(root.join("bin")).map_err(|e| e.to_string())?;
    // The upstream CLI splits pkg-config output on whitespace. Search-path environment
    // variables keep private paths containing spaces intact, without global pkg-config.
    let pkg_config = root.join("bin/pkg-config");
    fs::write(&pkg_config, "#!/bin/sh\ncase \"$1\" in\n  --cflags) exit 0 ;;\n  --libs) printf '%s\\n' '-lavformat -lavcodec -lswscale -lswresample -lavutil -lm -pthread' ;;\n  *) exit 1 ;;\nesac\n").map_err(|e| e.to_string())?;
    #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; fs::set_permissions(pkg_config, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?; }
    fs::write(marker, serde_json::to_vec_pretty(&receipt).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(root)
}

fn workers() -> String { std::thread::available_parallelism().map_or(4, |n| n.get().min(8)).to_string() }

fn resources(source: &Path, copied: &Path) -> Result<(), String> {
    let mut directories = vec![source.to_path_buf()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name.starts_with('.') || name == "eboot.bin" || name.ends_with(".prx") || name.ends_with(".elf") { continue; }
            if path.parent() == Some(source) && name == "prx" { continue; }
            if entry.file_type().map_err(|e| e.to_string())?.is_dir() { directories.push(path); continue; }
            let relative = path.strip_prefix(source).map_err(|e| e.to_string())?;
            let target = copied.join(relative);
            if !target.is_file() || fs::metadata(&target).map_err(|e| e.to_string())?.len() != fs::metadata(&path).map_err(|e| e.to_string())?.len() {
                return Err(format!("The built application is missing a complete resource: {}", relative.display()));
            }
        }
    }
    Ok(())
}

fn notices(source: &Path, output: &Path, ctx: &prepared::Context<'_>) -> Result<(), String> {
    let mut directories = vec![source.to_path_buf()];
    while let Some(directory) = directories.pop() {
        ctx.check_cancel()?;
        for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_dir() { directories.push(path); continue; }
            if !kind.is_file() { continue; }
            let name = entry.file_name().to_string_lossy().to_lowercase();
            let relative = path.strip_prefix(source).map_err(|e| e.to_string())?;
            let license_directory = relative.components().any(|part| part.as_os_str().to_string_lossy().eq_ignore_ascii_case("licenses"));
            if !license_directory && !["license", "copying", "notice", "copyright"].iter().any(|prefix| name.starts_with(prefix)) { continue; }
            let destination = output.join(relative);
            fs::create_dir_all(destination.parent().unwrap()).map_err(|e| e.to_string())?;
            fs::copy(path, destination).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Static private dependencies mean every load command must resolve to an OS library.
pub fn system_dependencies(text: &str) -> bool {
    text.lines().skip(1).filter(|line| !line.trim().is_empty()).all(|line| {
        let dependency = line.trim().split(" (compatibility version").next().unwrap_or("");
        dependency.starts_with("/usr/lib/") || dependency.starts_with("/System/Library/")
    })
}

pub(super) fn build(ctx: &prepared::Context<'_>) -> Result<prepared::Output, String> {
    let input = ctx.paths.get("base").ok_or("Choose your decrypted PS4 game folder.")?;
    let identity = identify(input, &ctx.pending.game.title_id)?;
    let source = ctx.work.join("input");
    if source.exists() { fs::remove_dir_all(&source).map_err(|e| e.to_string())?; }
    coordinator::copy_tree(input, &source, &|| ctx.stopped())?;
    if identify(&source, &identity.title_id)? != identity { return Err("The game changed while its files were copied.".into()); }
    let tools = tools(ctx)?;
    let built = ctx.work.join("compiled");
    if built.exists() { fs::remove_dir_all(&built).map_err(|e| e.to_string())?; }
    let spec = command(&tools.join("ps4-recomp"), &ctx.work, &source, &built, &tools);
    jobs::run_process(&spec, &ctx.work.join("ps4-build.log"), &|| ctx.stopped(), &|event| { if let Some(p) = progress(&event) { (ctx.progress)(p); } })?;
    ctx.check_cancel()?;
    let mut bundles = fs::read_dir(&built).map_err(|e| e.to_string())?.filter_map(Result::ok).map(|entry| entry.path()).filter(|path| crate::macos::is_bundle(path));
    let bundle = bundles.next().ok_or("PS4Recomp did not produce a complete native application.")?;
    if bundles.next().is_some() { return Err("PS4Recomp produced more than one application.".into()); }
    let executable = crate::macos::bundle_executable(&bundle, "arm64").ok_or("PS4Recomp did not produce a native ARM64 application.")?;
    let dependencies = tool_output("/usr/bin/otool", &["-L", executable.to_str().ok_or("Invalid output path.")?])?;
    if !system_dependencies(&dependencies) { return Err("The built app still depends on libraries outside its frozen runtime.".into()); }
    let app_name = bundle.file_name().ok_or("Missing app name.")?;
    let app = ctx.output.join(app_name);
    coordinator::copy_tree(&bundle, &app, &|| ctx.stopped())?;
    resources(&source, &app.join("Contents/SharedSupport"))?;
    run(ctx, "/usr/bin/codesign", &ctx.work, vec!["--verify".into(), "--deep".into(), "--strict".into(), app.display().to_string()], "Verifying the native application signature")?;
    let program = executable.strip_prefix(&built).map_err(|e| e.to_string())?.to_path_buf();
    let guest = Path::new(app_name).join("Contents/SharedSupport/guest_image.bin");
    if fs::metadata(ctx.output.join(&guest)).map_err(|_| "The application is missing its guest image.")?.len() == 0 { return Err("The application has an empty guest image.".into()); }
    if prepared::file_sha256(&ctx.output.join(&guest))? != prepared::file_sha256(&built.join("guest_image.bin"))? { return Err("The packaged guest image does not match the compiled output.".into()); }
    fs::copy(tools.join("receipt.json"), ctx.output.join("toolchain.json")).map_err(|e| e.to_string())?;
    let source_engine = ctx.unpack(&manifest().source)?;
    let licenses = ctx.output.join("licenses");
    fs::create_dir_all(&licenses).map_err(|e| e.to_string())?;
    notices(&source_engine, &licenses.join("PS4Recomp"), ctx)?;
    let ft = ctx.unpack(&manifest().freetype)?;
    notices(&ft, &licenses.join("FreeType"), ctx)?;
    let ff = ctx.unpack(&manifest().ffmpeg)?;
    for name in ["COPYING.LGPLv2.1", "COPYING.LGPLv3", "LICENSE.md"] { fs::copy(ff.join(name), licenses.join(name)).map_err(|e| e.to_string())?; }
    let root = format!("{{install}}/Game/{}", app_name.to_string_lossy());
    let env = BTreeMap::from([("PS4_APP_DIR".into(), format!("{root}/Contents/SharedSupport")), ("PS4_SAVEDATA_DIR".into(), "{install}/Game/state/saves".into()), ("PS4RECOMP_DATA_DIR".into(), "{install}/Game/state".into())]);
    let start = Start { program:vec![program.to_string_lossy().into()], args:vec![], env, ..Default::default() };
    Ok(prepared::Output { identity, start, saves:vec!["state/saves".into(), "state/settings".into()], files:vec![app_name.into(), "licenses".into(), "toolchain.json".into(), "state/download0".into(), "state/temp0".into()],
        required_files:vec![guest, Path::new(app_name).join("Contents/Info.plist"), "toolchain.json".into()] })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omitted_resources_cannot_register_a_successful_build() {
        let root = std::env::temp_dir().join(format!("rl-ps4-resources-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let source = root.join("source");
        let copied = root.join("copied");
        fs::create_dir_all(source.join("data")).unwrap();
        fs::create_dir_all(copied.join("data")).unwrap();
        fs::write(source.join("data/resource.bin"), b"required resource").unwrap();
        fs::write(source.join("eboot.bin"), b"compiled separately").unwrap();
        assert!(resources(&source, &copied).is_err());
        fs::write(copied.join("data/resource.bin"), b"required resource").unwrap();
        resources(&source, &copied).unwrap();
        fs::write(copied.join("data/resource.bin"), b"truncated").unwrap();
        assert!(resources(&source, &copied).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_build_must_not_depend_on_the_preparation_folder_or_homebrew() {
        assert!(system_dependencies("app:\n /usr/lib/libSystem.B.dylib (compatibility version 1.0)\n"));
        assert!(!system_dependencies("app:\n /opt/homebrew/lib/libavcodec.dylib (compatibility version 1.0)\n"));
        assert!(!system_dependencies("app:\n @rpath/liboutside.dylib (compatibility version 1.0)\n"));
    }
}

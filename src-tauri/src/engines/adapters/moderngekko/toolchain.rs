use crate::engines::{coordinator, jobs::CommandSpec, prepared::Context};
use std::{collections::BTreeMap, fs, path::{Path, PathBuf}, process::Command};

pub struct Tools {
    pub bin: PathBuf,
    pub source: PathBuf,
    pub env: BTreeMap<String, String>,
    pub receipt: serde_json::Value,
    pub remove_env: Vec<String>,
}

fn apple(args: &[&str]) -> Result<String, String> {
    let output = Command::new("/usr/bin/xcrun").args(args).output().map_err(|e| format!("Install Xcode with its macOS SDK before Setup: {e}"))?;
    if !output.status.success() { return Err("Xcode's macOS compiler/SDK is unavailable. Select an installed Xcode in Developer settings and retry Setup.".into()); }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub fn preflight() -> Result<(), String> {
    apple(&["--find", "clang"])?;
    apple(&["--find", "clang++"])?;
    apple(&["--show-sdk-path"])?;
    Ok(())
}

pub fn configure_command(program: &Path, work: &Path, title_id: &str) -> CommandSpec {
    let mut command = CommandSpec::new(program, work);
    command.args = vec!["-S".into(), work.join("sources/ModernGekko").display().to_string(), "-B".into(), work.join("runtime").display().to_string(), "-G".into(), "Ninja".into()];
    command.args.extend([
        "-DCMAKE_BUILD_TYPE=Release", "-DHAVE_PIPE2=OFF", "-DCMAKE_OSX_ARCHITECTURES=arm64", "-DBUILD_TESTING=OFF",
        "-DDOLRECOMP_ENABLE_LLVM=OFF", "-DUSE_SYSTEM_LIBS=OFF", "-DBUILD_SHARED_LIBS=OFF",
        "-DENABLE_VULKAN=OFF", "-DUSE_BUNDLED_MOLTENVK=OFF", "-DUSE_SANITIZERS=OFF",
        "-DBUILD_RUST_LIBS=OFF", "-DBUNDLE_SPEEX=ON", "-DFETCHCONTENT_FULLY_DISCONNECTED=ON",
        "-DCMAKE_DISABLE_FIND_PACKAGE_Git=ON", "-DPKG_CONFIG_EXECUTABLE=/usr/bin/false",
        "-DCMAKE_C_FLAGS=", "-DCMAKE_CXX_FLAGS=", "-DCMAKE_OBJC_FLAGS=", "-DCMAKE_OBJCXX_FLAGS=",
        "-DCMAKE_EXE_LINKER_FLAGS=", "-DDOLPHIN_WC_BRANCH=launcher-pinned",
    ].iter().map(|s| s.to_string()));
    // The upstream extractor rejects unbranded builds; every title uses this same metadata option.
    command.args.push(format!("-DMODERNGEKKO_REQUIRED_DISC_ID={title_id}"));
    let revision = super::manifest().runtime_revision;
    command.args.push(format!("-DDOLPHIN_WC_REVISION={revision}"));
    command.args.push(format!("-DDOLPHIN_WC_DESCRIBE={revision}"));
    command
}

fn module_python_override(original: &str) -> Result<String, String> {
    let expected = "find_package(Python3 REQUIRED COMPONENTS Interpreter)";
    if original.matches(expected).count() != 1 {
        return Err("The pinned module template changed; its private Python override needs review.".into());
    }
    Ok(original.replace(expected, include_str!("private-python.cmake").trim()))
}

pub fn build_tools(ctx: &Context<'_>) -> Result<Tools, String> {
    ctx.check_cancel()?;
    let manifest = super::manifest();
    let cmake = ctx.unpack(&manifest.cmake)?.join("bin/cmake");
    let ninja = ctx.unpack(&manifest.ninja)?;
    let python = ctx.unpack(&manifest.python)?;
    let source = ctx.work.join("sources/ModernGekko");
    let stamp = ctx.work.join("sources/complete");
    if !stamp.is_file() {
        let sources = ctx.work.join("sources");
        if sources.exists() { fs::remove_dir_all(&sources).map_err(|e| e.to_string())?; }
        for package in &manifest.sources {
            let downloaded = ctx.unpack(&package.artifact)?;
            coordinator::copy_tree(&downloaded, &sources.join(&package.path), &|| ctx.stopped())?;
        }
        let template = source.join("vendor/dolphin/module-template/CMakeLists.txt");
        let original = fs::read_to_string(&template).map_err(|e| e.to_string())?;
        fs::write(template, module_python_override(&original)?).map_err(|e| e.to_string())?;
        fs::write(&stamp, ctx.pending.package.identity()).map_err(|e| e.to_string())?;
    }
    let compiler = apple(&["--find", "clang"])?;
    let cxx = apple(&["--find", "clang++"])?;
    let sdk = apple(&["--show-sdk-path"])?;
    let compiler_dir = Path::new(&compiler).parent().ok_or("Xcode compiler path is invalid.")?;
    let search = std::env::join_paths([cmake.parent().unwrap(), ninja.as_path(), python.join("bin").as_path(), compiler_dir, Path::new("/usr/bin"), Path::new("/bin"), Path::new("/usr/sbin"), Path::new("/sbin")]).map_err(|e| e.to_string())?;
    let mut env = BTreeMap::new();
    env.insert("PATH".into(), search.to_string_lossy().into_owned());
    env.insert("SDKROOT".into(), sdk.clone());
    env.insert("CC".into(), compiler.clone());
    env.insert("CXX".into(), cxx.clone());
    env.insert("PYTHONNOUSERSITE".into(), "1".into());
    env.insert("MODERNGEKKO_PYTHON".into(), python.join("bin/python3").display().to_string());
    let remove_env: Vec<String> = ["PYTHONPATH", "PYTHONHOME", "CFLAGS", "CXXFLAGS", "CPPFLAGS", "LDFLAGS", "PKG_CONFIG_PATH", "CMAKE_PREFIX_PATH", "CMAKE_TOOLCHAIN_FILE", "CPATH", "C_INCLUDE_PATH", "CPLUS_INCLUDE_PATH", "LIBRARY_PATH", "DYLD_LIBRARY_PATH", "DYLD_FALLBACK_LIBRARY_PATH", "DYLD_INSERT_LIBRARIES", "MACOSX_DEPLOYMENT_TARGET", "DOLRECOMP_C_CHUNK_INSTRUCTIONS", "DOLRECOMP_LLVM_CHUNK_INSTRUCTIONS", "DOLRECOMP_LLVM_CPU", "DOLRECOMP_LLVM_FEATURES", "DOLRECOMP_LLVM_TARGET", "DOLRECOMP_DISPATCH_LOOKUP", "DOLRECOMP_UNSAFE_DIRECT_CALLS", "DOLRECOMP_LLVM_WRITE_JOURNAL"].iter().map(|s| s.to_string()).collect();
    let mut configure = configure_command(&cmake, &ctx.work, &ctx.pending.game.title_id);
    configure.env = env.clone();
    configure.remove_env = remove_env.clone();
    configure.args.extend([format!("-DCMAKE_C_COMPILER={compiler}"), format!("-DCMAKE_CXX_COMPILER={cxx}"), format!("-DCMAKE_OSX_SYSROOT={sdk}"), format!("-DPython3_EXECUTABLE={}", python.join("bin/python3").display())]);
    ctx.run(&configure, "Configuring the private GameCube/Wii engine")?;
    let mut build = CommandSpec::new(&cmake, &ctx.work);
    build.env = env.clone();
    build.remove_env = remove_env.clone();
    let jobs = std::thread::available_parallelism().map_or(2, |n| n.get()).min(8);
    build.args = vec!["--build".into(), ctx.work.join("runtime").display().to_string(), "--target".into(), "moderngekko-port".into(), "moderngekko-launcher".into(), "moderngekko-module-info".into(), format!("-j{jobs}")];
    ctx.run(&build, "Building the private GameCube/Wii engine")?;
    let receipt = serde_json::json!({
        "packages":serde_json::from_str::<serde_json::Value>(include_str!("components.json")).unwrap(),
        "compiler":apple(&["clang", "--version"])?, "sdk":apple(&["--show-sdk-version"])?,
        "configuration":configure.args, "private_python_override_sha256":super::patch_sha256(),
    });
    Ok(Tools { bin:ctx.work.join("runtime"), source, env, receipt, remove_env })
}

#[cfg(test)]
mod tests {
    #[test]
    fn module_python_override_is_explicit_and_rejects_changed_upstream_context() {
        let original = "cmake_minimum_required(VERSION 3.18)\nfind_package(Python3 REQUIRED COMPONENTS Interpreter)\n";
        let patched = super::module_python_override(original).unwrap();
        assert!(patched.contains("set(Python3_EXECUTABLE \"$ENV{MODERNGEKKO_PYTHON}\")"));
        assert!(super::module_python_override("new upstream configuration").is_err());
    }
}

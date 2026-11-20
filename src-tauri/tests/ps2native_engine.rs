use recomplauncher_lib::engines::{registry, Target};

#[test]
fn ps2native_is_discovered_for_mac_without_claiming_unimplemented_routes() {
    let adapter = registry::get("ps2-native-recomp").expect("the PS2 adapter must be discovered");
    assert_eq!(adapter.systems(), &["ps2"]);
    assert_eq!(adapter.input_signature_version(), recomplauncher_lib::engines::InputSignatureVersion::TreeV1);
    assert_eq!(adapter.targets(), &[Target::MacosArm64]);
    assert!(adapter.pin(Target::MacosArm64).is_ok());
    assert!(adapter.pin(Target::WindowsX64).is_err());
    assert!(adapter.pin(Target::LinuxX64).is_err());
}

#[test]
fn ps2_catalog_keeps_upstream_compatibility_separate_from_identity_and_artwork() {
    let adapter = registry::get("ps2-native-recomp").unwrap();
    let games = adapter.games();
    assert!(games.iter().any(|game| game.title_id == "SCUS-97481"));
    assert!(games.iter().any(|game| game.title_id == "SCUS-97399"));
    for game in games {
        assert_eq!(game.console, "ps2");
        assert!(game.version.is_empty() && game.build_id.is_empty());
        assert!(game.evidence.contains("exoteric33/ps2-native-recomp/blob/"));
        let art = adapter.artwork(&game.title_id);
        assert!(art["identity_source"].as_str().unwrap().contains("PCSX2/pcsx2/blob/"));
        assert!(art["cover"].as_str().unwrap().starts_with("https://"));
        assert!(art["hero"].as_str().unwrap().starts_with("https://"));
    }
}

#[test]
fn ps2_setup_collects_iso_without_console_keys_or_preparation_commands() {
    let adapter = registry::get("ps2-native-recomp").unwrap();
    let profile = adapter.profile(&adapter.games().remove(0));
    recomplauncher_lib::inputs::validate(&profile).unwrap();
    let value = serde_json::to_value(profile).unwrap();
    assert_eq!(value["inputs"][0]["id"], "base");
    assert_eq!(value["inputs"][0]["formats"][0]["extensions"], serde_json::json!(["iso"]));
    assert!(value["prepare"].is_null());
    assert!(value["start"].is_null());
    assert!(value["inputs"].as_array().unwrap().iter().all(|input| input["shared"].as_str().unwrap_or_default().is_empty()));
}

#[cfg(target_os = "macos")]
#[test]
fn ps2_identity_and_catalog_python_contracts() {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/engines/adapters/ps2native");
    for name in ["bootstrap_test.py", "catalog_test.py"] {
        let output = std::process::Command::new("python3").arg(folder.join(name)).output().unwrap();
        assert!(output.status.success(), "{name}: {}", String::from_utf8_lossy(&output.stderr));
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "requires checksum-verified extracted PS2 components and Xcode; builds a native fixture"]
fn ps2_native_package_compiles_cold_and_moves_without_host_tools() {
    let components = std::env::var_os("RL_PS2_COMPONENT_ROOTS").expect("set RL_PS2_COMPONENT_ROOTS to verified component roots JSON");
    let python = std::env::var_os("RL_PS2_PRIVATE_PYTHON").expect("set RL_PS2_PRIVATE_PYTHON to the pinned standalone interpreter");
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let work = std::env::temp_dir().join(format!("PS2 package smoke {}-{nonce}", std::process::id()));
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/engines/adapters/ps2native/native_smoke.py");
    let output = std::process::Command::new(&python).args(["-I"]).arg(script)
        .arg("--components").arg(components).arg("--work").arg(&work)
        .output().unwrap();
    assert!(output.status.success(), "{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    check_registered_fixture(&work.join("moved package ü"), &python);
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "requires a disposable real PS2 synthetic package; validates its actual registration and move"]
fn ps2_real_package_registers_recovers_activates_and_moves() {
    let fixture = std::path::PathBuf::from(std::env::var_os("RL_PS2_PREPARED_FIXTURE").expect("set RL_PS2_PREPARED_FIXTURE to the synthetic package"));
    let python = std::env::var_os("RL_PS2_PRIVATE_PYTHON").expect("set RL_PS2_PRIVATE_PYTHON to the pinned standalone interpreter");
    check_registered_fixture(&fixture, &python);
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn check_registered_fixture(fixture: &std::path::Path, python: &std::ffi::OsStr) {
    use recomplauncher_lib::{engines::{self, coordinator, pending, prepared}, manage};
    use std::{fs, path::PathBuf};
    #[derive(serde::Deserialize)]
    struct Record {
        identity: engines::GameIdentity,
        start: recomplauncher_lib::profile::Start,
        saves: Vec<PathBuf>,
        files: Vec<PathBuf>,
        required_files: Vec<PathBuf>,
    }
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!("PS2 registered smoke {nonce}"));
    let adapter = registry::get("ps2-native-recomp").unwrap();
    let title = adapter.games().into_iter().find(|game| game.title_id == "SCUS-97481").unwrap();
    let pending = pending::create_for(&title, Target::MacosArm64, &root.join("installation")).unwrap();
    let stage = prepared::work_path(&pending, "synthetic-fixture").unwrap().join("output");
    fs::create_dir_all(stage.parent().unwrap()).unwrap();
    coordinator::copy_tree(&fixture, &stage, &|| false).unwrap();
    let record: Record = serde_json::from_slice(&fs::read(stage.join("result.json")).unwrap()).unwrap();
    let output = prepared::Output { identity:record.identity, start:record.start, saves:record.saves, files:record.files, required_files:record.required_files };
    let ready = prepared::register(&pending, "synthetic-fixture", output).unwrap();
    assert!(prepared::recover(&pending, "synthetic-fixture").unwrap().is_some());
    let active = prepared::activate(&pending, &ready, &|| false).unwrap();
    let recovered = prepared::recover(&pending, "synthetic-fixture").unwrap().unwrap();
    let again = prepared::activate(&pending, &recovered, &|| false).unwrap();
    assert_eq!(active.engine_build, again.engine_build);
    let before = engines::read_build(&active).unwrap().unwrap();
    let profile = root.join("saved-profile.json");
    fs::write(&profile, serde_json::to_vec_pretty(&before.profile).unwrap()).unwrap();
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/engines/adapters/ps2native/native_smoke.py");
    let run = |installation: &std::path::Path| {
        let result = std::process::Command::new(&python).arg("-I").arg(&script).arg("--exercise-registered")
            .arg(installation).arg("--profile").arg(&profile).output().unwrap();
        assert!(result.status.success(), "{}\n{}", String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));
    };
    run(&active.path);
    let moved = root.join("moved library ü/game");
    manage::move_dir(&active.path, &moved, &|_, _| {}).unwrap();
    let mut active = active;
    active.path = moved;
    assert!(engines::pinned_executable(&active, Target::MacosArm64).unwrap().is_some());
    assert_eq!(engines::read_build(&active).unwrap().unwrap().components.runtime, before.components.runtime);
    run(&active.path);
    fs::remove_dir_all(root).unwrap();
}

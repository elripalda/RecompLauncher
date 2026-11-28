use recomplauncher_lib::engines::{anyps5, catalog, pending, prepared, registry::Adapter, Target};
use std::{fs, path::{Path, PathBuf}};

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("rl-anyps5-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn elf(path: &Path, kind: u16) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut bytes = vec![0; 256];
    bytes[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    bytes[16..18].copy_from_slice(&kind.to_le_bytes());
    bytes[18..20].copy_from_slice(&62u16.to_le_bytes());
    fs::write(path, bytes).unwrap();
}

fn source(root: &Path) {
    fs::create_dir_all(root.join("sce_sys")).unwrap();
    fs::write(root.join("sce_sys/param.json"), r#"{"titleId":"PPSA04181","contentVersion":"01.000.002","localizedParameters":{"defaultLanguage":"en-US","en-US":{"titleName":"Hoa"}}}"#).unwrap();
    elf(&root.join("eboot.bin"), 0xfe10);
    elf(&root.join("sce_module/game.prx"), 0xfe18);
}

#[test]
fn anyps5_pins_exact_separate_desktop_packages() {
    for target in [Target::WindowsX64, Target::LinuxX64] {
        let pin = anyps5::pin(target).unwrap();
        pin.validate().unwrap();
        assert_eq!(pin.upstream_revision, "62bd7dd61e2b8a7a59262c4c145edff549bae5b9");
        let package = anyps5::packages(target).unwrap();
        assert!(package.relinker.url.contains("/v0.1.5/"));
        assert_eq!(package.runtime.root, "libs");
        assert_eq!(package.runtime.digest.len(), 64);
    }
    assert_ne!(anyps5::pin(Target::WindowsX64).unwrap().identity(), anyps5::pin(Target::LinuxX64).unwrap().identity());
    assert!(anyps5::pin(Target::MacosArm64).is_err());
}

#[test]
fn selected_folder_identity_uses_original_metadata_and_executable_bytes() {
    let root = scratch("identity"); source(&root);
    let input = anyps5::inspect(&root, "PPSA04181").unwrap();
    assert_eq!(input.identity.version, "01.000.002");
    assert_eq!(input.identity.build_id, prepared::file_sha256(&root.join("eboot.bin")).unwrap());
    assert!(anyps5::inspect(&root, "PPSA00000").is_err());
    fs::write(root.join("sce_sys/param.json"), r#"{"titleId":"PPSA04181","localizedParameters":{"en-US":{"titleName":"Hoa"}}}"#).unwrap();
    assert!(anyps5::inspect(&root, "PPSA04181").is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn encrypted_or_wrong_architecture_modules_cannot_be_silently_skipped() {
    let root = scratch("self"); source(&root);
    fs::write(root.join("sce_module/game.prx"), b"\x4f\x15\x3d\x1dSELF").unwrap();
    assert!(anyps5::inspect(&root, "PPSA04181").unwrap_err().contains("SELF"));
    elf(&root.join("sce_module/game.prx"), 0xfe18);
    let mut module = fs::read(root.join("sce_module/game.prx")).unwrap(); module[18] = 183;
    fs::write(root.join("sce_module/game.prx"), module).unwrap();
    assert!(anyps5::inspect(&root, "PPSA04181").is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn all_module_directories_are_included_without_title_specific_recipes() {
    let root = scratch("extra modules"); source(&root);
    elf(&root.join("extra modules/nested/extra.prx"), 0xfe18);
    let input = anyps5::inspect(&root, "PPSA04181").unwrap();
    let command = anyps5::convert_command(Path::new("/private tools/relinker"), &root, Path::new("/output game"), &input, Target::WindowsX64).unwrap();
    assert!(command.args.windows(2).any(|p| p == ["--module-dir", "extra modules/nested"]));
    assert!(command.args.contains(&"--windows".into()));
    assert!(command.args.contains(&"--no-patches".into()));
    assert!(command.args.contains(&"--to-intel".into()));
    assert!(command.args.contains(&"--registry".into()));
    assert!(!command.args.contains(&"--autorun".into()));
    assert_eq!(Path::new(command.args.last().unwrap()), Path::new("/output game").join("eboot.exe"));
    assert_eq!(command.cwd, Path::new("/output game"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn linux_host_requirement_matches_the_published_runtime() {
    assert!(anyps5::check_glibc("glibc 2.43").is_ok());
    assert!(anyps5::check_glibc("glibc 2.44\n").is_ok());
    assert!(anyps5::check_glibc("glibc 2.42").unwrap_err().contains("2.43"));
    assert!(anyps5::check_glibc("musl 1.2.5").is_err());
}

#[test]
fn metadata_candidates_do_not_claim_the_reported_revision_is_supported() {
    let games = anyps5::ENGINE.games(); assert_eq!(games.len(), 3);
    for game in games {
        assert_eq!(game.console, "ps5");
        assert!(game.version.is_empty() && game.build_id.is_empty());
        assert!(game.notes.contains("Unverified"));
        assert!(game.evidence.starts_with("https://github.com/boykopovar/AnyPS5/discussions/"));
        let art = anyps5::ENGINE.artwork(&game.title_id);
        assert!(art["cover"].as_str().unwrap().starts_with("https://image.api.playstation.com/"));
        assert!(art["source"].as_str().unwrap().starts_with("https://store.playstation.com/"));
        assert_eq!(art["title_id"], game.title_id);
    }
    let profile = anyps5::setup_profile();
    recomplauncher_lib::inputs::validate(&profile).unwrap();
    assert_eq!(profile.inputs.len(), 1);
}

#[test]
fn native_outputs_and_dependencies_are_validated_before_publication() {
    let root = scratch("output"); source(&root.join("app0"));
    elf(&root.join("eboot.elf"), 2);
    elf(&root.join("libs/libc.prx"), 3);
    elf(&root.join("app0/sce_module/game.prx.guest.prx"), 3);
    fs::write(root.join("eboot.registry.json"), "[]").unwrap();
    fs::write(root.join("eboot.needed.json"), r#"["libc.prx","$ORIGIN/app0/sce_module/game.prx.guest.prx"]"#).unwrap();
    let required = anyps5::validate_output(&root, Target::LinuxX64).unwrap();
    assert!(required.contains(&PathBuf::from("libs/libc.prx")));
    assert!(anyps5::validate_output(&root, Target::WindowsX64).is_err());
    fs::remove_file(root.join("libs/libc.prx")).unwrap();
    assert!(anyps5::validate_output(&root, Target::LinuxX64).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn portable_game_uses_frozen_output_and_keeps_saves_separate_after_move() {
    let root = scratch("portable");
    let game = catalog::game("anyps5-ppsa04181").unwrap();
    let pending = pending::create_for(&game, Target::LinuxX64, &root.join("install")).unwrap();
    assert_eq!(pending.engine_build.as_ref().unwrap()["input_signature_version"], "tree-v1");
    assert!(pending.engine_build.as_ref().unwrap()["input_profile"].is_object());
    let stage = prepared::work_path(&pending, "inputs").unwrap().join("output");
    source(&stage.join("app0")); elf(&stage.join("eboot.elf"), 2); elf(&stage.join("libs/libc.prx"), 3);
    fs::write(stage.join("eboot.registry.json"), "[]").unwrap();
    fs::write(stage.join("eboot.needed.json"), r#"["libc.prx"]"#).unwrap();
    let identity = anyps5::inspect(&stage.join("app0"), "PPSA04181").unwrap().identity;
    let output = anyps5::output(&stage, identity, Target::LinuxX64).unwrap();
    let ready = prepared::register(&pending, "inputs", output).unwrap();
    assert!(prepared::activate(&pending, &ready, &|| true).is_err());
    let mut active = prepared::activate(&pending, &ready, &|| false).unwrap();
    let pin = recomplauncher_lib::engines::read_build(&active).unwrap().unwrap();
    let start = pin.profile.start.as_ref().unwrap();
    assert_eq!(start.program, ["Game/eboot.elf"]);
    assert!(start.args.is_empty());
    assert_eq!(start.env["ANYPS5_OPEN_URLS"], "never");
    assert!(pin.profile.saves.contains(&"{install}/Game/_sd".into()));
    assert!(recomplauncher_lib::inputs::excluded(&active, &active.path.join("Game/app0/data.bin")));
    assert!(!recomplauncher_lib::inputs::excluded(&active, &active.path.join("Game/_sd/save.bin")));
    fs::rename(&active.path, root.join("moved game")).unwrap(); active.path = root.join("moved game");
    assert!(recomplauncher_lib::engines::pinned_executable(&active, Target::LinuxX64).unwrap().is_some());
    assert_eq!(recomplauncher_lib::engines::read_build(&active).unwrap().unwrap().package, pin.package);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn malformed_or_ambiguous_input_layouts_are_rejected_before_conversion() {
    let root = scratch("ambiguous"); source(&root);
    fs::create_dir_all(root.join("sce_modules")).unwrap();
    assert!(anyps5::inspect(&root, "PPSA04181").is_err());
    fs::remove_dir(root.join("sce_modules")).unwrap();
    elf(&root.join("sce_module/extra.elf"), 2);
    assert!(anyps5::inspect(&root, "PPSA04181").is_err());
    fs::remove_file(root.join("sce_module/extra.elf")).unwrap();
    fs::write(root.join("eboot.bin"), b"truncated").unwrap();
    assert!(anyps5::inspect(&root, "PPSA04181").is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn extra_executables_are_explicitly_converted_and_original_patch_files_are_disabled() {
    let root = scratch("extra executable"); source(&root);
    elf(&root.join("tools/second.elf"), 0xfe10);
    fs::write(root.join("relinker-patches.toml"), "not an authorized patch").unwrap();
    let input = anyps5::inspect(&root, "PPSA04181").unwrap();
    let command = anyps5::convert_command(Path::new("/relinker"), &root, Path::new("/output"), &input, Target::LinuxX64).unwrap();
    assert!(command.args.windows(2).any(|p| p == ["--executable", "tools/second.elf"]));
    assert!(!command.args.contains(&"--patches".into()));
    assert!(command.args.contains(&"--no-patches".into()));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn published_runtime_requires_both_native_libraries_and_bundled_windows_dependencies() {
    let root = scratch("runtime");
    elf(&root.join("libc.prx"), 3);
    assert!(anyps5::validate_runtime(&root, Target::LinuxX64).is_err());
    assert!(anyps5::validate_runtime(&root, Target::WindowsX64).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "Reads and verifies downloaded release archives; set RL_ANYPS5_PACKAGE_DIR"]
fn actual_release_archives_match_the_pins_and_native_layout() {
    let downloads = PathBuf::from(std::env::var_os("RL_ANYPS5_PACKAGE_DIR").expect("RL_ANYPS5_PACKAGE_DIR"));
    let root = scratch("archives");
    for target in [Target::LinuxX64, Target::WindowsX64] {
        let packages = anyps5::packages(target).unwrap();
        recomplauncher_lib::engines::packages::verify(&downloads.join(&packages.relinker.name), &packages.relinker.digest).unwrap();
        let output = root.join(format!("{target:?}"));
        recomplauncher_lib::engines::packages::install_archive(&downloads.join(&packages.runtime.name), &output, &packages.runtime.digest).unwrap();
        anyps5::validate_runtime(&output.join("libs"), target).unwrap();
        assert!(output.join("libs/licenses").is_dir());
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "Downloads and executes the pinned relinker on native Windows/Linux only"]
fn pinned_native_relinker_reports_its_version() {
    use recomplauncher_lib::engines::{jobs::{CommandSpec, run_process}, packages};
    use std::sync::{Arc, atomic::AtomicBool};
    let target = Target::current().unwrap();
    let package = anyps5::packages(target).expect("Run on a native Windows/Linux x86-64 host");
    let root = scratch("native-cli");
    let rt = tokio::runtime::Runtime::new().unwrap();
    let tool = rt.block_on(packages::download(&reqwest::Client::new(), &root, &package.relinker, Arc::new(AtomicBool::new(false)), &|_,_| {})).unwrap();
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut command = CommandSpec::new(tool, &root); command.args = vec!["--version".into()];
    run_process(&command, &root.join("version.log"), &|| false, &|_| {}).unwrap();
    assert!(fs::read_to_string(root.join("version.log")).unwrap().contains("relinker "));
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "Converts an authorized decrypted game on native Windows/Linux; set RL_ANYPS5_FIXTURE"]
fn real_game_preparation_and_recovery() {
    use std::sync::{Arc, atomic::AtomicBool};
    let fixture: serde_json::Value = serde_json::from_slice(&fs::read(std::env::var_os("RL_ANYPS5_FIXTURE").expect("RL_ANYPS5_FIXTURE")).unwrap()).unwrap();
    let root = PathBuf::from(fixture["root"].as_str().unwrap());
    assert!(!root.exists(), "Use a fresh disposable fixture root");
    fs::create_dir_all(&root).unwrap();
    let id = format!("anyps5-{}", fixture["title_id"].as_str().unwrap().to_lowercase());
    let game = catalog::game(&id).unwrap();
    let target = Target::current().unwrap();
    let mut install = pending::create_for(&game, target, &root.join("game")).unwrap();
    install.setup_inputs.insert("base".into(), PathBuf::from(fixture["base"].as_str().unwrap()));
    let http = reqwest::Client::new();
    let ready = anyps5::ENGINE.prepare(&http, &root.join("data"), &install, Arc::new(AtomicBool::new(false)), &|p| println!("{}",p.text)).unwrap();
    let recovered = anyps5::ENGINE.prepare(&http, &root.join("data"), &install, Arc::new(AtomicBool::new(false)), &|_| {}).unwrap();
    assert_eq!(ready.engine_build, recovered.engine_build);
    let active = anyps5::ENGINE.activate(&install, &recovered, &|| false).unwrap();
    assert!(recomplauncher_lib::engines::pinned_executable(&active, target).unwrap().is_some());
    println!("Prepared fixture retained at {}. Gameplay was not tested.", root.display());
}

#[test]
fn malformed_named_prx_is_not_treated_as_an_optional_resource() {
    let root = scratch("truncated module"); source(&root);
    fs::write(root.join("sce_module/game.prx"), b"broken").unwrap();
    assert!(anyps5::inspect(&root, "PPSA04181").is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_directory_separators_are_valid_but_literal_separator_characters_are_not_filenames() {
    let root = scratch("native paths"); source(&root);
    let nested = Path::new("extra").join("modules").join("game.prx");
    elf(&root.join(&nested), 0xfe18);
    let input = anyps5::inspect(&root, "PPSA04181").unwrap();
    let command = anyps5::convert_command(Path::new("tool"), &root, Path::new("output"), &input, Target::WindowsX64).unwrap();
    assert!(command.args.windows(2).any(|pair| pair == ["--module-dir", "extra/modules"]));
    #[cfg(unix)] {
        elf(&root.join("literal\\backslash.prx"), 0xfe18);
        assert!(anyps5::inspect(&root, "PPSA04181").is_err());
    }
    fs::remove_dir_all(root).unwrap();
}

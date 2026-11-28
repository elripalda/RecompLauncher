use recomplauncher_lib::engines::{catalog, registry, Target};

#[test]
fn moderngekko_discovery_exposes_only_its_implemented_host_and_game_systems() {
    let adapter = registry::get("moderngekko").expect("generic GameCube/Wii adapter is registered");
    assert_eq!(adapter.targets(), &[Target::MacosArm64]);
    assert_eq!(adapter.input_signature_version(), recomplauncher_lib::engines::InputSignatureVersion::TreeV1);
    assert_eq!(adapter.systems(), &["gc", "wii"]);
    assert!(registry::for_target("moderngekko", Target::WindowsX64).is_err());
    let mut catalog = serde_json::json!({"apps":[]});
    catalog::merge(&mut catalog, Some(Target::MacosArm64));
    let games: Vec<_> = catalog["apps"].as_array().unwrap().iter().filter(|g| g["engine"] == "moderngekko").collect();
    assert!(!games.is_empty());
    assert!(games.iter().all(|g| g["experimental"] == true));
    assert!(games.iter().all(|g| g["description"].as_str().unwrap().contains("Unverified")));
}

use recomplauncher_lib::engines::moderngekko;
use std::{fs, path::{Path, PathBuf}};

fn scratch(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("rl-moderngekko-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

fn disc(root: &Path, id: &[u8; 6], wii: bool) {
    fs::create_dir_all(root.join("sys")).unwrap();
    fs::create_dir_all(root.join("files")).unwrap();
    let mut boot = vec![0; 0x440];
    boot[..6].copy_from_slice(id);
    boot[7] = 2;
    if wii { boot[0x18..0x1c].copy_from_slice(&0x5d1c9ea3u32.to_be_bytes()); }
    else { boot[0x1c..0x20].copy_from_slice(&0xc2339f3du32.to_be_bytes()); }
    fs::write(root.join("sys/boot.bin"), boot).unwrap();
    let mut dol = vec![0; 0x120];
    dol[..4].copy_from_slice(&0x100u32.to_be_bytes());
    dol[0x48..0x4c].copy_from_slice(&0x80003100u32.to_be_bytes());
    dol[0x90..0x94].copy_from_slice(&0x20u32.to_be_bytes());
    dol[0xe0..0xe4].copy_from_slice(&0x80003100u32.to_be_bytes());
    fs::write(root.join("sys/main.dol"), dol).unwrap();
    fs::write(root.join("files/content.bin"), b"synthetic content").unwrap();
}

#[test]
fn identity_comes_from_disc_metadata_and_valid_dol_not_filename() {
    let root = scratch("identity");
    disc(&root, b"GABE01", false);
    let identity = moderngekko::read_identity(&root, "GABE01", "gc").unwrap();
    assert_eq!(identity.title_id, "GABE01");
    assert_eq!(identity.version, "2");
    assert_eq!(identity.build_id.len(), 64);
    assert!(moderngekko::read_identity(&root, "GXYZ01", "gc").is_err());
    assert!(moderngekko::read_identity(&root, "GABE01", "wii").is_err());
    fs::write(root.join("sys/main.dol"), vec![0; 0x120]).unwrap();
    assert!(moderngekko::read_identity(&root, "GABE01", "gc").is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn preparing_commands_keep_paths_separate_and_never_use_the_run_driver() {
    let work = Path::new("/private/work with spaces");
    let extract = moderngekko::extract_command(Path::new("/tools/ModernGekko"), work, Path::new("/My Games/a.iso"));
    assert_eq!(extract.args, ["--extract", "/My Games/a.iso"]);
    assert_eq!(Path::new(&extract.env["XDG_DATA_HOME"]), Path::new("/private/work with spaces").join("extract-user"));
    assert!(!extract.env.contains_key("HOME"));
    let compile = moderngekko::compile_command(Path::new("/tools/moderngekko-port"), work, Path::new("/private/disc data"));
    assert_eq!(compile.args[0], "build");
    assert_eq!(compile.args[1], "/private/disc data");
    assert!(compile.args.windows(2).any(|a| a == ["--backend", "c"]));
    assert!(!compile.args.iter().any(|a| a == "run"));
}

#[test]
fn frozen_output_separates_game_data_runtime_and_player_saves() {
    let root = scratch("output");
    disc(&root.join("Disc"), b"GABE01", false);
    fs::create_dir_all(root.join("Runtime/Sys")).unwrap();
    fs::write(root.join("Runtime/moderngekko-run"), b"synthetic executable").unwrap();
    fs::write(root.join("Runtime/gGABE01_recomp.dylib"), b"synthetic module").unwrap();
    fs::write(root.join("Runtime/receipt.json"), b"pinned dependency identity").unwrap();
    let identity = moderngekko::read_identity(&root.join("Disc"), "GABE01", "gc").unwrap();
    let output = moderngekko::frozen_output(&root, identity).unwrap();
    assert_eq!(output.start.program, ["Runtime/moderngekko-run"]);
    assert!(output.start.args.windows(2).any(|a| a == ["--game", "{install}/Game/Disc"]));
    assert!(output.start.args.windows(2).any(|a| a == ["--user-dir", "{install}/Game/User"]));
    assert!(output.saves.contains(&PathBuf::from("User/GC/USA/Card A")));
    assert!(output.saves.contains(&PathBuf::from("User/Wii/shared2/sys/SYSCONF")));
    assert!(!output.saves.contains(&PathBuf::from("User")));
    assert_eq!(output.files, [PathBuf::from("Disc"), PathBuf::from("Runtime")]);
    assert!(output.required_files.contains(&PathBuf::from("Disc/files/content.bin")));
    assert!(output.required_files.contains(&PathBuf::from("Runtime/receipt.json")));
    assert!(output.files.iter().all(|p| !p.starts_with("User")));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn all_source_artifacts_have_immutable_upstream_identity() {
    let manifest = moderngekko::manifest();
    moderngekko::pin(Target::MacosArm64).unwrap().validate().unwrap();
    assert!(moderngekko::pin(Target::LinuxX64).is_err());
    for source in manifest.sources {
        assert!(source.artifact.url.contains(&source.revision));
        assert_eq!(source.revision.len(), 40);
        assert_eq!(source.artifact.digest.len(), 64);
        assert!(source.artifact.size > 0);
        assert!(!source.path.contains(".."));
    }
    recomplauncher_lib::inputs::validate(&moderngekko::setup_profile()).unwrap();
}

#[test]
fn wii_save_scope_uses_the_partition_title_and_excludes_ticket_content() {
    let root = scratch("wii-saves");
    disc(&root.join("Disc"), b"SABE01", true);
    let mut tmd = vec![0; 0x1e4];
    tmd[..4].copy_from_slice(&0x00010001u32.to_be_bytes());
    tmd[0x18c..0x194].copy_from_slice(&0x0001000453414245u64.to_be_bytes());
    fs::write(root.join("Disc/tmd.bin"), tmd).unwrap();
    fs::create_dir_all(root.join("Runtime")).unwrap();
    for file in ["moderngekko-run", "gSABE01_recomp.dylib", "receipt.json"] { fs::write(root.join("Runtime").join(file), b"fixture").unwrap(); }
    let identity = moderngekko::read_identity(&root.join("Disc"), "SABE01", "wii").unwrap();
    let output = moderngekko::frozen_output(&root, identity).unwrap();
    assert!(output.saves.contains(&PathBuf::from("User/Wii/title/00010004/53414245/data")));
    assert!(!output.saves.iter().any(|p| p == Path::new("User/Wii") || p.starts_with("Disc")));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn gamecube_save_scope_includes_both_memory_card_formats() {
    let root = scratch("gc-saves");
    disc(&root.join("Disc"), b"GABE01", false);
    fs::create_dir_all(root.join("Runtime")).unwrap();
    for file in ["moderngekko-run", "gGABE01_recomp.dylib", "receipt.json"] { fs::write(root.join("Runtime").join(file), b"fixture").unwrap(); }
    let identity = moderngekko::read_identity(&root.join("Disc"), "GABE01", "gc").unwrap();
    let output = moderngekko::frozen_output(&root, identity).unwrap();
    assert!(output.saves.contains(&PathBuf::from("User/GC/JPN/Card A")));
    assert!(output.saves.contains(&PathBuf::from("User/GC/MemoryCardA.*.raw")));
    assert!(!output.saves.contains(&PathBuf::from("User/GC")));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn configure_uses_private_tools_metal_and_bundled_libraries_without_fetching() {
    let command = moderngekko::configure_command(Path::new("/private/CMake/bin/cmake"), Path::new("/job with spaces"), "GABE01");
    assert!(command.args.contains(&"-DMODERNGEKKO_REQUIRED_DISC_ID=GABE01".into()));
    assert!(command.args.contains(&"-DENABLE_VULKAN=OFF".into()));
    assert!(command.args.contains(&"-DFETCHCONTENT_FULLY_DISCONNECTED=ON".into()));
    assert!(command.args.contains(&"-DUSE_SYSTEM_LIBS=OFF".into()));
    assert!(command.args.contains(&"-DPKG_CONFIG_EXECUTABLE=/usr/bin/false".into()));
    let source = Path::new("/job with spaces").join("sources/ModernGekko");
    assert!(command.args.windows(2).any(|a| a[0] == "-S" && Path::new(&a[1]) == source));
}

#[test]
fn prepared_registration_owns_two_roots_but_pins_individual_game_and_runtime_files() {
    use recomplauncher_lib::engines::{pending, prepared, read_build};
    let root = scratch("owned-roots");
    let title = catalog::game("moderngekko-gmse01").unwrap();
    let game = pending::create_for(&title, Target::MacosArm64, &root.join("game")).unwrap();
    let stage = prepared::work_path(&game, "inputs").unwrap().join("output");
    disc(&stage.join("Disc"), b"GMSE01", false);
    fs::create_dir_all(stage.join("Runtime/Sys")).unwrap();
    let mut executable = [0; 32];
    executable[..4].copy_from_slice(&0xfeedfacfu32.to_le_bytes());
    executable[4..8].copy_from_slice(&0x0100000cu32.to_le_bytes());
    executable[12..16].copy_from_slice(&2u32.to_le_bytes());
    fs::write(stage.join("Runtime/moderngekko-run"), executable).unwrap();
    for file in ["gGMSE01_recomp.dylib", "receipt.json", "Sys/resource.bin"] {
        fs::write(stage.join("Runtime").join(file), b"synthetic runtime data").unwrap();
    }
    let identity = moderngekko::read_identity(&stage.join("Disc"), "GMSE01", "gc").unwrap();
    let output = moderngekko::frozen_output(&stage, identity).unwrap();
    let ready = prepared::register(&game, "inputs", output).unwrap();
    assert_eq!(ready.files, [PathBuf::from("Game/Disc"), PathBuf::from("Game/Runtime")]);
    let pin = read_build(&ready).unwrap().unwrap();
    assert!(pin.required_files.contains(&PathBuf::from("Game/Disc/files/content.bin")));
    assert!(pin.required_files.contains(&PathBuf::from("Game/Runtime/Sys/resource.bin")));
    assert!(recomplauncher_lib::inputs::excluded(&ready, &ready.path.join("Game/Disc/files/content.bin")));
    assert!(!recomplauncher_lib::inputs::excluded(&ready, &ready.path.join("Game/User/GC/SRAM.raw")));
    fs::remove_dir_all(root).unwrap();
}

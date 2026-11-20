use recomplauncher_lib::engines::{ps4recomp, registry::Adapter, Target};
use std::{fs, path::{Path, PathBuf}};

fn folder(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("rl-ps4-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(path.join("sce_sys")).unwrap();
    path
}

fn metadata(path: &Path, title: &str, version: &str) {
    let keys = b"TITLE_ID\0APP_VER\0";
    let mut sfo = vec![0; 20 + 32];
    sfo[..4].copy_from_slice(b"\0PSF");
    sfo[4..8].copy_from_slice(&0x101u32.to_le_bytes());
    sfo[8..12].copy_from_slice(&52u32.to_le_bytes());
    sfo[12..16].copy_from_slice(&(52u32 + keys.len() as u32).to_le_bytes());
    sfo[16..20].copy_from_slice(&2u32.to_le_bytes());
    let mut values = vec![];
    for (i, (key, value)) in [(0u16, title), (9u16, version)].iter().enumerate() {
        let entry = 20 + i * 16;
        sfo[entry..entry+2].copy_from_slice(&key.to_le_bytes());
        sfo[entry+2..entry+4].copy_from_slice(&0x204u16.to_le_bytes());
        sfo[entry+4..entry+8].copy_from_slice(&(value.len() as u32 + 1).to_le_bytes());
        sfo[entry+8..entry+12].copy_from_slice(&(value.len() as u32 + 1).to_le_bytes());
        sfo[entry+12..entry+16].copy_from_slice(&(values.len() as u32).to_le_bytes());
        values.extend_from_slice(value.as_bytes());
        values.push(0);
    }
    sfo.extend_from_slice(keys);
    sfo.extend(values);
    fs::write(path.join("sce_sys/param.sfo"), sfo).unwrap();
    let mut elf = vec![0u8; 64];
    elf[..6].copy_from_slice(b"\x7fELF\x02\x01");
    elf[18..20].copy_from_slice(&62u16.to_le_bytes());
    fs::write(path.join("eboot.bin"), elf).unwrap();
}

#[test]
fn ps4_identifies_the_actual_title_version_and_elf() {
    let path = folder("identity");
    metadata(&path, "CUSA05070", "01.05");
    let identity = ps4recomp::identify(&path, "CUSA05070").unwrap();
    assert_eq!(identity.version, "01.05");
    assert_eq!(identity.build_id.len(), 64);
    assert!(ps4recomp::identify(&path, "CUSA00001").is_err());
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn ps4_rejects_encrypted_executables_and_malformed_metadata() {
    let path = folder("invalid");
    metadata(&path, "CUSA05070", "01.05");
    fs::write(path.join("eboot.bin"), b"SELF encrypted").unwrap();
    assert!(ps4recomp::identify(&path, "CUSA05070").is_err());
    metadata(&path, "CUSA05070", "01.05");
    let mut bad = fs::read(path.join("sce_sys/param.sfo")).unwrap();
    bad[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
    fs::write(path.join("sce_sys/param.sfo"), bad).unwrap();
    assert!(ps4recomp::identify(&path, "CUSA05070").is_err());
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn ps4_setup_preserves_folder_selection_and_has_no_console_downloads() {
    let profile = ps4recomp::setup_profile();
    recomplauncher_lib::inputs::validate(&profile).unwrap();
    assert_eq!(profile.inputs.len(), 1);
    assert_eq!(profile.inputs[0].id, "base");
    assert!(profile.inputs[0].shared.is_empty());
    assert!(profile.prepare.is_none());
}

#[test]
fn ps4_build_uses_explicit_paths_and_never_autoruns_or_applies_title_patches() {
    let command = ps4recomp::command(Path::new("/private engine/ps4-recomp"), Path::new("/work folder"), Path::new("/game folder"), Path::new("/output folder"), Path::new("/private tools"));
    assert!(command.args.contains(&"/game folder".into()));
    assert!(command.args.contains(&"/output folder".into()));
    assert!(command.args.contains(&"--standalone".into()));
    assert!(command.args.contains(&"--json".into()));
    assert!(!command.args.iter().any(|arg| ["-r", "--run", "--patch", "--patches"].contains(&arg.as_str())));
}

#[test]
fn ps4_reports_measured_work_without_inventing_an_eta() {
    let event = serde_json::json!({"event":"progress", "stage":"compile", "done":5, "total":10, "unit":"files"});
    let progress = ps4recomp::progress(&event).unwrap();
    assert_eq!(progress.fraction, Some(0.5));
    assert_eq!(progress.done, Some(5.0));
    assert_eq!(progress.eta_seconds, None);
}

#[test]
fn ps4_catalog_and_package_declare_only_the_implemented_host() {
    let engine = &ps4recomp::ENGINE;
    assert_eq!(engine.input_signature_version(), recomplauncher_lib::engines::InputSignatureVersion::TreeV1);
    assert_eq!(engine.targets(), &[Target::MacosArm64]);
    assert_eq!(engine.systems(), &["ps4"]);
    engine.pin(Target::MacosArm64).unwrap().validate().unwrap();
    assert!(engine.pin(Target::LinuxX64).is_err());
    assert!(!engine.games().is_empty());
    for game in engine.games() {
        assert!(game.evidence.contains(&engine.pin(Target::MacosArm64).unwrap().upstream_revision));
        assert!(game.version.is_empty());
        assert!(game.build_id.is_empty());
        let art = engine.artwork(&game.title_id);
        assert!(art["cover"].as_str().unwrap().starts_with("https://"));
    }
}

#[test]
#[ignore = "Builds a copyright-free ELF fixture with the pinned real tools; needs Apple silicon and Xcode"]
fn native_synthetic_folder_builds_and_registers_a_movable_app() {
    use recomplauncher_lib::engines::{self, pending};
    use std::sync::{Arc, atomic::AtomicBool};
    let root = PathBuf::from(std::env::var_os("RL_PS4_SMOKE_ROOT").expect("Set RL_PS4_SMOKE_ROOT to an isolated test folder"));
    let source = root.join("synthetic input");
    if !source.exists() {
        fs::create_dir_all(source.join("sce_sys")).unwrap();
        metadata(&source, "CUSA05070", "01.00");
        let mut elf = vec![0u8; 4096 + 16];
        elf[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        elf[16..18].copy_from_slice(&2u16.to_le_bytes());
        elf[18..20].copy_from_slice(&62u16.to_le_bytes());
        elf[20..24].copy_from_slice(&1u32.to_le_bytes());
        elf[24..32].copy_from_slice(&0x400000u64.to_le_bytes());
        elf[32..40].copy_from_slice(&64u64.to_le_bytes());
        elf[52..54].copy_from_slice(&64u16.to_le_bytes());
        elf[54..56].copy_from_slice(&56u16.to_le_bytes());
        elf[56..58].copy_from_slice(&1u16.to_le_bytes());
        elf[64..68].copy_from_slice(&1u32.to_le_bytes());
        elf[68..72].copy_from_slice(&5u32.to_le_bytes());
        elf[72..80].copy_from_slice(&4096u64.to_le_bytes());
        elf[80..88].copy_from_slice(&0x400000u64.to_le_bytes());
        elf[96..104].copy_from_slice(&16u64.to_le_bytes());
        elf[104..112].copy_from_slice(&16u64.to_le_bytes());
        elf[112..120].copy_from_slice(&4096u64.to_le_bytes());
        elf[4096..4099].copy_from_slice(b"\x31\xc0\xc3");
        fs::write(source.join("eboot.bin"), elf).unwrap();
        fs::write(source.join("fixture.txt"), b"copyright-free synthetic resource").unwrap();
    }
    let pending_file = root.join("pending.json");
    let mut game = if pending_file.exists() { serde_json::from_slice(&fs::read(&pending_file).unwrap()).unwrap() } else {
        let title = ps4recomp::ENGINE.games().into_iter().find(|g| g.title_id == "CUSA05070").unwrap();
        pending::create_for(&title, Target::MacosArm64, &root.join("installed game")).unwrap()
    };
    game.setup_inputs.insert("base".into(), source);
    fs::write(pending_file, serde_json::to_vec(&game).unwrap()).unwrap();
    let http = reqwest::Client::new();
    let ready = ps4recomp::ENGINE.prepare(&http, &root.join("data"), &game, Arc::new(AtomicBool::new(false)), &|p| eprintln!("{} {:?}", p.text, p.fraction)).unwrap();
    let active = ps4recomp::ENGINE.activate(&game, &ready, &|| false).unwrap();
    assert!(engines::pinned_executable(&active, Target::MacosArm64).is_ok());
    let mut moved = active.clone();
    moved.path = root.join("moved game");
    fs::rename(&active.path, &moved.path).unwrap();
    assert!(engines::pinned_executable(&moved, Target::MacosArm64).is_ok());
    assert!(moved.pinned && !moved.auto_update);
    let profile = engines::read_build(&moved).unwrap().unwrap().profile;
    assert_eq!(profile.saves, ["{install}/Game/state/saves", "{install}/Game/state/settings"]);
    for (folder, filename) in [("saves", "slot.dat"), ("settings", "settings.cfg"), ("download0", "download.dat"), ("temp0", "temporary.dat")] {
        let destination = moved.path.join("Game/state").join(folder);
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join(filename), b"synthetic player state or excluded content").unwrap();
    }
    let spots: Vec<_> = profile.saves.into_iter().map(|template| recomplauncher_lib::saves::Spot { template, from:"engine" }).collect();
    let entries = recomplauncher_lib::saves::collect(&spots, &moved);
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().any(|entry| entry.rel == "slot.dat"));
    let broad = [recomplauncher_lib::saves::Spot { template:"{install}/Game/state".into(), from:"player" }];
    assert_eq!(recomplauncher_lib::saves::collect(&broad, &moved).len(), 2);
}

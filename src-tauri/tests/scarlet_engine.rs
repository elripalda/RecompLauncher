use recomplauncher_lib::engines::{scarletnx, catalog, pending, Target};
use std::{fs, path::{Path, PathBuf}};

fn scratch(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("rl-scarlet-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn extraction(root: &Path, title: u64) {
    fs::create_dir_all(root.join("exefs")).unwrap();
    fs::create_dir_all(root.join("romfs")).unwrap();
    let mut npdm = vec![0; 0xc0];
    npdm[..4].copy_from_slice(b"META");
    npdm[0x70..0x74].copy_from_slice(&0x80u32.to_le_bytes());
    npdm[0x74..0x78].copy_from_slice(&0x40u32.to_le_bytes());
    npdm[0x80..0x84].copy_from_slice(b"ACI0");
    npdm[0x90..0x98].copy_from_slice(&title.to_le_bytes());
    fs::write(root.join("exefs/main.npdm"), npdm).unwrap();
    let mut nso = vec![0; 128];
    nso[..4].copy_from_slice(b"NSO0");
    nso[64..96].fill(0xab);
    fs::write(root.join("exefs/main"), nso).unwrap();
    let mut nacp = vec![0; 0x4000];
    nacp[0x3060..0x3065].copy_from_slice(b"1.2.3");
    fs::write(root.join("control.nacp"), nacp).unwrap();
}

#[test]
fn scarlet_packages_select_only_their_pinned_desktop_target() {
    let win = scarletnx::pin(Target::WindowsX64).unwrap();
    let linux = scarletnx::pin(Target::LinuxX64).unwrap();
    win.validate().unwrap(); linux.validate().unwrap();
    assert_ne!(win.identity(), linux.identity());
    assert!(scarletnx::pin(Target::MacosArm64).is_err());
    assert!(scarletnx::artifact(Target::WindowsX64).unwrap().url.contains("/nightly-2026-11-13-2d5ec80/"));
}

#[test]
fn scarlet_commands_extract_then_compile_without_starting_a_game() {
    let root = Path::new("/work with spaces");
    let base = Path::new("/games/My Game.nsp");
    let cmd = scarletnx::extract_command(Path::new("/engine"), root, base, Some(Path::new("/games/update.nsp")), Some(Path::new("/keys/prod.keys")));
    assert!(cmd.args.contains(&"-x".into()));
    assert!(!cmd.args.contains(&"-r".into()));
    assert!(cmd.args.windows(2).any(|p| p == ["--update", "/games/update.nsp"]));
    assert!(cmd.args.windows(2).any(|p| p == ["--keys", "/keys/prod.keys"]));
    assert_eq!(cmd.args.last().unwrap(), "/games/My Game.nsp");
    let build = scarletnx::compile_command(Path::new("/engine"), root);
    assert!(build.args.contains(&"-r".into()));
    assert!(!build.args.contains(&"--update".into()));
    assert!(build.args.windows(2).any(|p| p[0] == "--outdir" && Path::new(&p[1]) == Path::new("/work with spaces/output")));
}

#[test]
fn extracted_game_must_match_the_selected_title_and_record_its_actual_revision() {
    let root = scratch("identity");
    extraction(&root, 0x0100000000000000);
    let game = scarletnx::read_identity(&root, "0100000000000000").unwrap();
    assert_eq!(game.version, "1.2.3");
    assert_eq!(game.build_id, "ab".repeat(32));
    assert!(scarletnx::read_identity(&root, "0100000000000001").is_err());
    fs::write(root.join("exefs/main.npdm"), b"truncated").unwrap();
    assert!(scarletnx::read_identity(&root, "0100000000000000").is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn desktop_catalog_is_switch_one_only_and_explicitly_unverified() {
    for target in [Target::WindowsX64, Target::LinuxX64] {
        let mut cat = serde_json::json!({"apps":[]});
        catalog::merge(&mut cat, Some(target));
        let apps = cat["apps"].as_array().unwrap();
        assert!(!apps.is_empty());
        for game in apps {
            assert_eq!(game["engine"], "scarletnx");
            assert_eq!(game["origin"], "switch");
            assert_eq!(game["experimental"], true);
            assert!(game["description"].as_str().unwrap().contains("Unverified"));
        }
    }
}

#[test]
fn pending_scarlet_install_keeps_its_target_and_requires_no_new_setup_steps() {
    let root = scratch("pending");
    let game = catalog::game("scarletnx-0100633007d48000").unwrap();
    let install = pending::create_for(&game, Target::WindowsX64, &root.join("game")).unwrap();
    let saved = pending::read(&install).unwrap().unwrap();
    assert_eq!(saved.package.target, Target::WindowsX64);
    let profile = pending::profile(&install).unwrap().unwrap();
    recomplauncher_lib::inputs::validate(&profile).unwrap();
    assert_eq!(profile.inputs.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["base", "update", "keys"]);
    assert!(profile.inputs[0].label.contains("game"));
    assert!(pending::create_for(&game, Target::MacosArm64, &root.join("wrong-target")).is_err());
    assert!(!root.join("wrong-target").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn registry_routes_by_declared_platform_and_rejects_unknown_engines() {
    use recomplauncher_lib::engines::registry;
    let mac: Vec<_> = registry::available(Target::MacosArm64).map(|e| e.id()).collect();
    assert_eq!(mac, ["nxnative"]);
    for target in [Target::WindowsX64, Target::LinuxX64] {
        assert_eq!(registry::available(target).map(|e| e.id()).collect::<Vec<_>>(), ["scarletnx"]);
    }
    assert!(registry::get("unknown").is_err());
    assert!(registry::get("scarletnx").unwrap().supports_console("switch"));
    assert!(!registry::get("scarletnx").unwrap().supports_console("switch2"));
}

#[path = "../engine_registry.rs"]
mod discovery;

#[test]
fn new_adapter_folder_is_discovered_without_a_registration_list() {
    let root = scratch("discovery");
    fs::create_dir_all(root.join("future_engine")).unwrap();
    fs::write(root.join("future_engine/mod.rs"), "// Engine's implementation").unwrap();
    fs::create_dir_all(root.join("notes")).unwrap();
    let generated = discovery::generate(&root).unwrap();
    assert!(generated.contains("pub mod future_engine"));
    assert!(generated.contains("&future_engine::ENGINE"));
    assert!(!generated.contains("notes"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn engine_identity_can_preserve_a_non_switch_system_and_opaque_game_id() {
    use recomplauncher_lib::engines::GameIdentity;
    let raw = serde_json::json!({"console":"gamecube", "title_id":"GZLE01", "version":"1.0", "build_id":"revision-one"});
    let identity: GameIdentity = serde_json::from_value(raw.clone()).unwrap();
    identity.validate().unwrap();
    assert_eq!(serde_json::to_value(&identity).unwrap(), raw);
}

fn native(path: &Path, target: Target) {
    let mut header = vec![0; 256];
    match target {
        Target::WindowsX64 => { header[..2].copy_from_slice(b"MZ"); header[60..64].copy_from_slice(&128u32.to_le_bytes()); header[128..134].copy_from_slice(b"PE\0\0\x64\x86"); }
        Target::LinuxX64 => { header[..5].copy_from_slice(b"\x7fELF\x02"); header[5] = 1; header[18] = 62; }
        _ => panic!("desktop fixture"),
    }
    fs::write(path, header).unwrap();
}

fn prepared_fixture(label: &str, target: Target) -> (PathBuf, recomplauncher_lib::library::Installed, PathBuf, PathBuf) {
    let root = scratch(label);
    let game = catalog::game("scarletnx-0100633007d48000").unwrap();
    let g = pending::create_for(&game, target, &root.join("game")).unwrap();
    let work = scarletnx::output::work_path(&g, "signature").unwrap();
    let title = &game.title_id;
    let output = work.join("output").join(title);
    fs::create_dir_all(&output).unwrap();
    extraction(&output.join(format!("{title}_data")), u64::from_str_radix(title,16).unwrap());
    let exe = if target == Target::WindowsX64 { format!("{title}.exe") } else { title.clone() };
    native(&output.join(exe), target);
    native(&output.join(format!("{title}_code.{}", if target == Target::WindowsX64 { "dll" } else { "so" })), target);
    fs::write(output.join(format!("{title}.abi")), "123\n").unwrap();
    fs::write(output.join(format!("{title}.ini")), "[Game]\nsource=/old/private/extracted\n[Video]\nscale=1\n").unwrap();
    let engine = root.join("runtime");
    native(&engine, target);
    (root,g,work,engine)
}

#[test]
fn prepared_desktop_game_recovers_and_moves_without_a_builder_or_source_files() {
    use recomplauncher_lib::engines::{coordinator, pinned_profile, pinned_executable, read_build};
    for target in [Target::WindowsX64, Target::LinuxX64] {
        let (root,g,work,engine) = prepared_fixture(&format!("output-{target:?}"), target);
        let title = "0100633007D48000";
        let identity = scarletnx::read_identity(&work.join(format!("output/{title}/{title}_data")),title).unwrap();
        let ready = scarletnx::output::register(&g,&work,"signature",identity,&engine).unwrap();
        assert!(!g.path.join(".scarletnx").exists());
        assert!(pending::check_move(&g).is_err());
        assert!(coordinator::activate_output(&g,&ready,&||true).is_err());
        let recovered = scarletnx::output::recover(&g,"signature").unwrap().unwrap();
        assert!(scarletnx::output::recover(&g,"changed-inputs").is_err());
        let active = coordinator::activate_output(&g,&recovered,&||false).unwrap();
        assert!(coordinator::activate_output(&g,&recovered,&||false).is_ok());
        assert!(pinned_executable(&active,target).unwrap().is_some());
        let start = pinned_profile(&active).unwrap().unwrap().start.unwrap();
        assert!(start.args.contains(&format!("--game={{install}}/.scarletnx/{title}")));
        assert!(!start.args.iter().any(|s| ["-r","-x","--outdir"].contains(&s.as_str())));
        // Upstream's driver rejects --data/--repo/--build-dir without a dump or preparation mode.
        assert!(!start.args.iter().any(|s| ["--data", "--repo", "--build-dir"].iter().any(|flag| s == flag || s.starts_with(&format!("{flag}=")))));
        let settings = fs::read_to_string(active.path.join(format!(".scarletnx/config/{title}/{title}.ini"))).unwrap();
        assert!(!settings.contains("source="));
        assert!(settings.contains("scale=1"));
        let pin = read_build(&active).unwrap().unwrap();
        assert_eq!(pin.package,pending::read(&g).unwrap().unwrap().package);
        assert!(recomplauncher_lib::inputs::excluded(&active,&active.path.join(format!(".scarletnx/{title}/{title}_data/romfs/file"))));
        assert!(!recomplauncher_lib::inputs::excluded(&active,&active.path.join(format!(".scarletnx/{title}/{title}_data/save/player.sav"))));
        let mut moved = active.clone(); moved.path = root.join("moved game");
        fs::rename(&active.path,&moved.path).unwrap();
        assert!(pinned_executable(&moved,target).unwrap().is_some());
        assert_eq!(read_build(&moved).unwrap().unwrap().package,pin.package);
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn incomplete_or_mismatched_desktop_output_never_becomes_ready() {
    let (root,g,work,engine) = prepared_fixture("invalid-output", Target::LinuxX64);
    let title = "0100633007D48000";
    let identity = scarletnx::read_identity(&work.join(format!("output/{title}/{title}_data")),title).unwrap();
    fs::remove_file(work.join(format!("output/{title}/{title}.abi"))).unwrap();
    assert!(scarletnx::output::register(&g,&work,"signature",identity,&engine).is_err());
    assert!(!g.path.join(".engine-prepared.json").exists());
    fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[test]
fn cancelling_preparation_terminates_owned_windows_descendants() {
    use recomplauncher_lib::engines::jobs::{CommandSpec, run_process};
    use std::time::{Duration, Instant};
    let root = scratch("cancel-process-tree");
    fs::write(root.join("child.ps1"), "Set-Content child-started yes\nStart-Sleep -Seconds 3\nSet-Content escaped yes").unwrap();
    fs::write(root.join("parent.ps1"), "Start-Process powershell -ArgumentList '-NoProfile -File child.ps1' -NoNewWindow\nStart-Sleep -Seconds 30").unwrap();
    let mut spec = CommandSpec::new("powershell.exe",&root);
    spec.args = vec!["-NoProfile".into(),"-File".into(),"parent.ps1".into()];
    let start = Instant::now();
    let result = run_process(&spec,&root.join("log"),&|| root.join("child-started").exists() || start.elapsed()>Duration::from_secs(20),&|_| {});
    assert!(result.unwrap_err().contains("Stopped"));
    assert!(root.join("child-started").exists(),"fixture child never started");
    std::thread::sleep(Duration::from_secs(4));
    assert!(!root.join("escaped").exists(),"a compiler descendant survived Stop");
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "Downloads and executes the pinned native package on a Windows/Linux test machine"]
fn pinned_desktop_package_starts_its_cli() {
    use recomplauncher_lib::engines::jobs::{CommandSpec, run_process};
    use std::sync::{Arc, atomic::AtomicBool};
    let target = Target::current().unwrap();
    assert!(matches!(target,Target::WindowsX64 | Target::LinuxX64));
    let root = scratch("package-smoke");
    let rt = tokio::runtime::Runtime::new().unwrap();
    let program = rt.block_on(scarletnx::ensure_package(&reqwest::Client::new(),&root,target,Arc::new(AtomicBool::new(false)),&|_,_| {})).unwrap();
    let mut spec = CommandSpec::new(program,&root);
    spec.args = vec!["--help".into()];
    spec.env.insert("APPIMAGE_EXTRACT_AND_RUN".into(),"1".into());
    run_process(&spec,&root.join("help.log"),&||false,&|_| {}).unwrap();
    let help = fs::read_to_string(root.join("help.log")).unwrap();
    assert!(help.contains("--game") && help.contains("--extract"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn every_discovered_adapter_declares_consistent_systems_targets_and_catalogs() {
    use recomplauncher_lib::engines::ADAPTERS;
    let mut ids = std::collections::BTreeSet::new();
    for engine in ADAPTERS {
        assert!(ids.insert(engine.id()),"duplicate engine ID");
        assert!(!engine.systems().is_empty());
        assert!(!engine.targets().is_empty());
        for target in engine.targets() {
            let pin = engine.pin(*target).unwrap();
            pin.validate().unwrap();
            assert_eq!(pin.engine,engine.id());
            assert_eq!(pin.target,*target);
        }
        let mut games = std::collections::BTreeSet::new();
        for game in engine.games() {
            assert_eq!(game.engine,engine.id());
            assert!(engine.supports_console(&game.console));
            assert!(games.insert(game.id()),"duplicate game route");
            assert!(!game.name.is_empty() && !game.title_id.is_empty());
            recomplauncher_lib::inputs::validate(&engine.profile(&game)).unwrap();
        }
    }
}

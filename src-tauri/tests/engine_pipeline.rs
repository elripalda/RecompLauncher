use recomplauncher_lib::engines::{packages, Target};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

fn scratch(label: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("rl-engine-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn engine_catalog_preserves_profile_console_and_exact_supported_build() {
    let text = "# Game profile (Switch 2, update 1.2)\ntitle_id\t0100000000000000\nname\tTest Game\npackage\tv65536\nbuild\tv65536\t1.2\tmain=0123456789abcdef\n";
    let game = recomplauncher_lib::engines::nxnative::catalog::parse(text, "Experimental upstream notes.", "https://github.com/example/engine").unwrap();
    assert_eq!(game.console, "switch2");
    assert_eq!(game.version, "1.2");
    assert_eq!(game.build_id, "0123456789abcdef");
    assert!(recomplauncher_lib::engines::nxnative::catalog::parse(&text.replace("main=0123456789abcdef", "sdk=0123456789abcdef"), "", "").is_none());
}

#[test]
fn engine_catalog_merge_is_idempotent_and_does_not_claim_other_targets() {
    use recomplauncher_lib::engines::catalog;
    let mut cat = json!({"apps":[{"id":"existing", "name":"Existing"}]});
    catalog::merge(&mut cat, Some(Target::MacosArm64));
    let first = cat.clone();
    catalog::merge(&mut cat, Some(Target::MacosArm64));
    assert_eq!(cat, first);
    assert!(cat["apps"].as_array().unwrap().len() > 2);
    let mut windows = json!({"apps":[]});
    catalog::merge(&mut windows, Some(Target::WindowsX64));
    assert!(windows["apps"].as_array().unwrap().iter().all(|a| a["engine"] != "nxnative"));
}

#[test]
fn engine_discovery_entries_include_cover_and_background_artwork() {
    let mut cat = json!({"apps":[]});
    recomplauncher_lib::engines::catalog::merge(&mut cat, Some(Target::MacosArm64));
    let apps = cat["apps"].as_array().unwrap();
    assert!(!apps.is_empty());
    for app in apps {
        for field in ["cover", "hero"] {
            let link = app[field].as_str().unwrap_or_default();
            assert!(link.starts_with("https://"), "{} has no {field}", app["name"]);
        }
    }
}

#[test]
fn engine_artwork_uses_title_identity_without_replacing_other_catalog_art() {
    let original = json!({"id":"other", "name":"Another game", "game":"Hollow Knight", "cover":"https://example.com/custom.jpg"});
    let mut cat = json!({"apps":[original.clone()]});
    recomplauncher_lib::engines::catalog::merge(&mut cat, Some(Target::MacosArm64));
    let apps = cat["apps"].as_array().unwrap();
    assert_eq!(apps[0], original);
    let hollow = apps.iter().find(|a| a["id"] == "nxnative-0100633007d48000").unwrap();
    assert!(hollow["hero"].as_str().unwrap_or_default().contains("HollowKnight"));
    assert_ne!(hollow["cover"], original["cover"]);
}

#[test]
fn wrong_engine_archive_never_unpacks_or_marks_ready() {
    let root = scratch("checksum");
    let archive = root.join("engine.zip");
    std::fs::write(&archive, b"not an engine").unwrap();
    assert!(packages::install_archive(&archive, &root.join("package"), &"a".repeat(64)).is_err());
    assert!(!root.join("package").exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn verified_package_retains_relative_symlinks_and_rejects_escape() {
    use std::io::Write;
    let root = scratch("zip");
    let archive = root.join("engine.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
    zip.start_file("bin/python3.13", zip::write::SimpleFileOptions::default().unix_permissions(0o755)).unwrap();
    zip.write_all(b"fixture executable").unwrap();
    zip.add_symlink("bin/python3", "python3.13", zip::write::SimpleFileOptions::default()).unwrap();
    zip.finish().unwrap();
    let digest = hex::encode(Sha256::digest(std::fs::read(&archive).unwrap()));
    packages::install_archive(&archive, &root.join("package"), &digest).unwrap();
    assert_eq!(std::fs::read(root.join("package/bin/python3")).unwrap(), b"fixture executable");
    let bad = root.join("bad.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&bad).unwrap());
    zip.add_symlink("escape", "../../outside", zip::write::SimpleFileOptions::default()).unwrap();
    zip.finish().unwrap();
    let digest = hex::encode(Sha256::digest(std::fs::read(&bad).unwrap()));
    assert!(packages::install_archive(&bad, &root.join("bad"), &digest).is_err());
    assert!(!root.join("bad").exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn pending_engine_install_has_setup_inputs_but_cannot_launch() {
    use recomplauncher_lib::engines::{pending, executable_for_host};
    let game = recomplauncher_lib::engines::nxnative::catalog::games().into_iter().find(|g| g.console == "switch").unwrap();
    let root = scratch("pending");
    let g = pending::create(&game, &root.join("game")).unwrap();
    let profile = pending::profile(&g).unwrap().unwrap();
    recomplauncher_lib::inputs::validate(&profile).unwrap();
    assert!(profile.inputs.iter().any(|i| i.id == "base"));
    assert!(profile.inputs.iter().any(|i| i.shared == "switch.prod-keys"));
    assert!(executable_for_host(&g).is_err());
    assert!(!recomplauncher_lib::engines::release_updates_allowed(&g));
    assert!(pending::create(&game, &g.path).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn pending_retry_adopts_direct_engine_without_changing_the_saved_record() {
    use recomplauncher_lib::engines::{pending, nxnative};
    let root=scratch("pending-retry");
    let mut old=pending::create(&recomplauncher_lib::engines::nxnative::catalog::games().remove(0),&root.join("game")).unwrap();
    old.engine_build.as_mut().unwrap()["package"]["archive_sha256"]=json!("b".repeat(64));
    old.engine_build.as_mut().unwrap()["package"]["packaging_revision"]=json!("nx2apple-v1.4.0-rl1");
    old.setup_inputs.insert("base".into(),PathBuf::from("/my/game.nsp"));
    let before=old.clone();
    assert!(pending::profile(&old).unwrap().is_some());
    let retry=pending::for_setup(&old).unwrap();
    assert_eq!(pending::read(&retry).unwrap().unwrap().package,nxnative::pin());
    assert_eq!(old.engine_build,before.engine_build);
    assert_eq!(retry.setup_inputs,old.setup_inputs);
    // A completed output awaiting registration is recovered with its original pin.
    std::fs::write(old.path.join(".engine-prepared.json"),b"fixture").unwrap();
    assert_eq!(pending::for_setup(&old).unwrap().engine_build,old.engine_build);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn nxnative_command_uses_pinned_local_tools_and_explicit_work_output() {
    use recomplauncher_lib::engines::nxnative;
    let game = recomplauncher_lib::engines::nxnative::catalog::games().remove(0);
    let cmd = nxnative::command(&game, std::path::Path::new("/package/Resources"), std::path::Path::new("/job with spaces"), Some(std::path::Path::new("/private/keys")), Some(std::path::Path::new("/dotnet")));
    assert!(cmd.program.ends_with("python/bin/python3"));
    assert!(cmd.args.windows(2).any(|a| a == ["--platform", "mac-app"]));
    assert!(cmd.args.windows(2).any(|a| a == ["--progress", "json"]));
    assert!(cmd.args.windows(2).any(|a| a[0] == "--work-dir" && std::path::Path::new(&a[1]) == std::path::Path::new("/job with spaces/build")));
    assert!(cmd.args.windows(2).any(|a| a[0] == "-o" && std::path::Path::new(&a[1]) == std::path::Path::new("/job with spaces/output/Game.app")));
    assert_eq!(cmd.env["PYTHONNOUSERSITE"], "1");
    assert_eq!(cmd.env["DOTNET_ROOT"], "/dotnet");
    assert!(!cmd.env.contains_key("SPIRV_CROSS"));
    assert!(!cmd.args.iter().any(|a| a == "--allow-damaged" || a == "--allow-other-metal"));
}

#[test]
fn direct_engine_manifest_pins_its_own_source_and_independent_dependencies() {
    let manifest = recomplauncher_lib::engines::nxnative::manifest();
    assert!(manifest.source.url.starts_with("https://codeload.github.com/seedds/nxnative/tar.gz/"));
    assert!(manifest.python.url.starts_with("https://github.com/astral-sh/python-build-standalone/releases/download/"));
    for component in manifest.components() {
        assert!(!component.url.to_lowercase().contains("nx2apple"));
        assert!([64,128].contains(&component.digest.len()));
        assert!(component.digest.bytes().all(|c|c.is_ascii_hexdigit()));
    }
}

#[test]
fn missing_update_error_names_the_required_game_version_and_input() {
    use recomplauncher_lib::engines::{catalog,nxnative};
    let game=catalog::game("nxnative-0100633007d48000").unwrap();
    let message=nxnative::preparation_error(&game,"[RED] the version to pack (-p / PACKAGE) is not in the folder, which holds v0".into());
    assert!(message.contains("1.5.12620"));
    assert!(message.contains("matching update"));
    assert_eq!(nxnative::preparation_error(&game,"disk full".into()),"disk full");
}

#[cfg(unix)]
#[test]
fn engine_process_checks_exit_status_even_after_success_progress() {
    use recomplauncher_lib::engines::jobs::{run_process, CommandSpec};
    let root = scratch("exit");
    let mut cmd = CommandSpec::new("/bin/sh", &root);
    cmd.args = vec!["-c".into(), "echo '{\"ev\":\"stage_end\",\"ok\":true}'; echo 'fixture failed' >&2; exit 9".into()];
    let result = run_process(&cmd, &root.join("log"), &|| false, &|_| {});
    assert!(result.unwrap_err().contains("9"));
    assert!(std::fs::read_to_string(root.join("log")).unwrap().contains("fixture failed"));
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn cancelling_engine_process_stops_its_children() {
    use recomplauncher_lib::engines::jobs::{run_process, CommandSpec};
    let root = scratch("cancel");
    let mut cmd = CommandSpec::new("/bin/sh", &root);
    cmd.args = vec!["-c".into(), "(sleep 1; touch survived) & wait".into()];
    let start = std::time::Instant::now();
    assert!(run_process(&cmd, &root.join("log"), &|| start.elapsed().as_millis() > 100, &|_| {}).is_err());
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert!(!root.join("survived").exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn prepared_output_must_match_selected_title_and_build_before_registration() {
    use recomplauncher_lib::engines::{pending, coordinator};
    let game = recomplauncher_lib::engines::nxnative::catalog::games().into_iter().find(|g| g.console == "switch").unwrap();
    let root = scratch("output");
    let g = pending::create(&game, &root.join("game")).unwrap();
    let work_id = hex::encode(Sha256::digest(serde_json::to_vec(&(recomplauncher_lib::engines::nxnative::pin().identity(), "source")).unwrap()));
    let work = g.path.join(".engine-work").join(work_id);
    std::fs::create_dir_all(work.join("output/Game.app/Contents/MacOS")).unwrap();
    let mut binary = vec![0;32]; binary[..4].copy_from_slice(&[0xcf,0xfa,0xed,0xfe]); binary[4..8].copy_from_slice(&0x100000cu32.to_le_bytes()); binary[12..16].copy_from_slice(&2u32.to_le_bytes());
    std::fs::write(work.join("output/Game.app/Contents/MacOS/nxnative"),binary).unwrap();
    let info = plist::Value::Dictionary([("DMTitleID".to_string(), plist::Value::String("0100000000000000".into())),("CFBundleShortVersionString".to_string(),plist::Value::String(game.version.clone()))].into_iter().collect());
    info.to_file_xml(work.join("output/Game.app/Contents/Info.plist")).unwrap();
    let nso = work.join(format!("build/extract/{}/ExeFS",game.package));
    std::fs::create_dir_all(&nso).unwrap();
    let mut header = vec![0;128]; header[..4].copy_from_slice(b"NSO0");
    let build = hex::decode(&game.build_id).unwrap(); header[64..64+build.len()].copy_from_slice(&build);
    std::fs::write(nso.join("main"),header).unwrap();
    assert!(recomplauncher_lib::engines::nxnative::pipeline::register_output(&g, &game, &work, "source", recomplauncher_lib::engines::nxnative::pin()).is_err());
    assert!(!g.path.join("Game.app").exists());
    let mut info = info; info.as_dictionary_mut().unwrap().insert("DMTitleID".to_string(), plist::Value::String(game.title_id.clone()));
    info.to_file_xml(work.join("output/Game.app/Contents/Info.plist")).unwrap();
    let ready = recomplauncher_lib::engines::nxnative::pipeline::register_output(&g, &game, &work, "source", recomplauncher_lib::engines::nxnative::pin()).unwrap();
    assert_eq!(ready.exe.as_deref(), Some(std::path::Path::new("Game.app/Contents/MacOS/nxnative")));
    assert!(!ready.path.join("Game.app").exists());
    assert!(ready.path.join(".engine-prepared.json").exists());
    assert!(coordinator::activate_output(&g,&ready,&||true).is_err());
    assert!(!ready.path.join("Game.app").exists());
    // A cancelled registration keeps a durable result at this location. Moving must wait
    // for Setup to register that result, otherwise its absolute recovery path is stranded.
    assert!(pending::check_move(&g).is_err());
    let mut current=g.clone();current.name="Player name".into();current.cloud=Some(false);
    let active=coordinator::activate_output(&current,&ready,&||false).unwrap();
    assert_eq!(active.name,"Player name");assert_eq!(active.cloud,Some(false));
    assert!(active.path.join("Game.app").exists());
    // A crash after the rename is recovered from the already-persisted intent, not rebuilt.
    assert!(coordinator::activate_output(&current,&ready,&||false).is_ok());
    assert!(recomplauncher_lib::engines::pending::read(&ready).unwrap().is_none());
    assert!(recomplauncher_lib::engines::nxnative::pipeline::register_output(&g, &game, &work, "source", recomplauncher_lib::engines::nxnative::pin()).is_err());
    pending::check_move(&active).unwrap();
    let moved_path=root.join("moved/game");
    recomplauncher_lib::manage::move_dir(&active.path,&moved_path,&|_,_|{}).unwrap();
    let mut moved=active.clone();moved.path=moved_path;
    assert!(recomplauncher_lib::engines::pinned_executable(&moved, Target::MacosArm64).unwrap().unwrap().is_file());
    if Target::current() == Some(Target::MacosArm64) {
        assert!(recomplauncher_lib::engines::executable_for_host(&moved).unwrap().unwrap().is_file());
    } else {
        assert!(recomplauncher_lib::engines::executable_for_host(&moved).is_err());
    }
    assert_eq!(moved.engine_build,active.engine_build);
    std::fs::remove_dir_all(root).unwrap();
}

/// Opt-in acceptance runner: all state stays under the fixture's root, never the live library.
#[test]
#[ignore = "requires a local game fixture and downloads the pinned engine/toolchain"]
fn real_nsp_preparation() {
    use recomplauncher_lib::{engines::{coordinator, pending}, inputs};
    use std::sync::{Arc, atomic::AtomicBool};
    let config = std::env::var("RL_ENGINE_FIXTURE").expect("set RL_ENGINE_FIXTURE to a JSON fixture description");
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(config).unwrap()).unwrap();
    let root = PathBuf::from(v["root"].as_str().unwrap());
    std::fs::create_dir_all(&root).unwrap();
    let data = root.join("data"); std::fs::create_dir_all(&data).unwrap();
    let package = data.join("engines/packages").join(recomplauncher_lib::engines::nxnative::manifest().source.digest);
    if let Some(archive) = v["archive"].as_str().filter(|_| !package.exists()) {
        packages::install_archive(std::path::Path::new(archive),&package,&recomplauncher_lib::engines::nxnative::manifest().source.digest).unwrap();
    }
    let game = recomplauncher_lib::engines::nxnative::catalog::games().into_iter().find(|g| g.title_id == v["title_id"].as_str().unwrap()).unwrap();
    let mut g = if root.join("pending.json").exists() {
        serde_json::from_slice(&std::fs::read(root.join("pending.json")).unwrap()).unwrap()
    } else { pending::create(&game,&root.join("game")).unwrap() };
    g.setup_inputs.insert("base".into(),v["base"].as_str().unwrap().into());
    if let Some(up) = v["update"].as_str() { g.setup_inputs.insert("update".into(),up.into()); }
    if let Some(keys) = v["keys"].as_str().filter(|_| !data.join("shared-keys/switch.prod-keys/prod.keys").is_file()) { inputs::import_key(&data,"switch.prod-keys",std::path::Path::new(keys)).unwrap(); }
    std::fs::write(root.join("pending.json"),serde_json::to_vec_pretty(&g).unwrap()).unwrap();
    let http = reqwest::Client::builder().user_agent("RecompLauncher local engine acceptance").build().unwrap();
    let shown = std::sync::Mutex::new(String::new());
    let ready = coordinator::prepare(&http,&data,&g,Arc::new(AtomicBool::new(false)),&|p| {
        let text = p.text;
        let mut last = shown.lock().unwrap(); if *last != text { println!("{text}"); *last=text; }
    }).unwrap();
    let ready = coordinator::activate_output(&g,&ready,&||false).unwrap();
    assert!(recomplauncher_lib::engines::executable_for_host(&ready).unwrap().unwrap().is_file());
    std::fs::write(root.join("ready.json"),serde_json::to_vec_pretty(&ready).unwrap()).unwrap();
}

#[test]
fn importing_prod_keys_accepts_extended_hex_records_for_the_engine_to_validate() {
    let root = scratch("extended-key-shape");
    let source = root.join("prod.keys");
    let text = format!("header_key = {}\neticket_rsa_keypair = {}\nmaster_kek_source_06 = {}\nssl_rsa_key = {}\n", "a".repeat(64), "b".repeat(1056), "c".repeat(34), "d".repeat(512));
    std::fs::write(&source,text).unwrap();
    recomplauncher_lib::inputs::import_key(&root.join("data"), "switch.prod-keys", &source).unwrap();
    std::fs::write(&source,format!("{} = {}", "a".repeat(32), "b".repeat(34))).unwrap();
    assert!(recomplauncher_lib::inputs::import_key(&root.join("data"), "switch.title-keys", &source).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn backup_enumeration_excludes_engine_work_and_native_bundle() {
    use recomplauncher_lib::{inputs, manage, engines::{pending}};
    let root=scratch("owned-files");
    let g=pending::create(&recomplauncher_lib::engines::nxnative::catalog::games().remove(0),&root.join("game")).unwrap();
    for file in [".engine-work/job/inputs/update.nsp","Game.app/Contents/Resources/config.json",".engine-prepared.json","player.cfg"] {
        let p=g.path.join(file);std::fs::create_dir_all(p.parent().unwrap()).unwrap();std::fs::write(p,b"fixture").unwrap();
    }
    assert_eq!(manage::player_files(&g.path,&inputs::release_files(&g)),vec![PathBuf::from("player.cfg")]);
    assert!(inputs::excluded(&g,&g.path.join(".engine-work/job/inputs/update.nsp")));
    assert!(inputs::excluded(&g,&g.path.join("Game.app/Contents/Resources/config.json")));
    let spots = [recomplauncher_lib::saves::Spot { template:"{install}".into(),from:"game" }];
    let saves = recomplauncher_lib::saves::collect(&spots,&g);
    assert_eq!(saves.iter().map(|s|s.rel.as_str()).collect::<Vec<_>>(),vec!["player.cfg"]);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn extracted_folder_staging_accepts_base_and_update_only_layouts() {
    let root=scratch("folder-layout");
    for folder in ["base/exefs", "base/romfs"] { std::fs::create_dir_all(root.join("source").join(folder)).unwrap(); }
    std::fs::write(root.join("source/base/exefs/main.npdm"),b"fixture").unwrap();
    let mut inputs=std::collections::BTreeMap::from([("base".to_string(),root.join("source"))]);
    recomplauncher_lib::engines::nxnative::pipeline::stage_inputs(&inputs,&root.join("staged"),&||false).unwrap();
    assert!(root.join("staged/base/exefs/main.npdm").exists());
    std::fs::create_dir_all(root.join("other/updates/65536/exefs")).unwrap();
    std::fs::create_dir_all(root.join("other/updates/65536/romfs")).unwrap();
    std::fs::write(root.join("other/updates/65536/exefs/main.npdm"),b"fixture").unwrap();
    inputs.insert("base".into(),root.join("other"));
    recomplauncher_lib::engines::nxnative::pipeline::stage_inputs(&inputs,&root.join("staged"),&||false).unwrap();
    assert!(root.join("staged/updates/65536/exefs/main.npdm").exists());
    inputs.insert("update".into(),root.join("separate.nsp"));
    assert!(recomplauncher_lib::engines::nxnative::pipeline::stage_inputs(&inputs,&root.join("staged"),&||false).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

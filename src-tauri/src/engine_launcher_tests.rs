//! How the launcher treats engine games: pins, launch, moves, saves and setup recovery.

mod engine_setup_completion_tests {
    use super::super::*;
    use super::super::engine_setup::*;

    fn assert_run_lock_available() {
        let start = std::time::Instant::now();
        // Other fixtures briefly publish under this shared lock; this calling thread must not own it.
        while SETUP_RUN.try_lock().is_err() {
            assert!(start.elapsed() < std::time::Duration::from_millis(250), "worker retained the global setup lock");
            std::thread::yield_now();
        }
    }

    struct Fixture { app: App, game: Installed, signature: String, run: u64 }
    impl Fixture {
        fn new(title: &str) -> Self {
            let root = std::env::temp_dir().join(format!("rl-engine-completion-{title}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            let mut title_record = engines::catalog::game("scarletnx-0100633007d48000").unwrap();
            title_record.title_id = format!("0100000000{title}");
            let mut game = engines::pending::create_for(&title_record, engines::Target::WindowsX64, &root.join("game")).unwrap();
            let mut pending = engines::pending::read(&game).unwrap().unwrap();
            pending.input_signature_version = engines::InputSignatureVersion::TreeV1;
            pending.input_profile = Some(serde_json::from_value(json!({"format":1,"inputs":[
                {"id":"base","label":"Owned fixture folder","formats":[{"type":"folder"}]}
            ]})).unwrap());
            game.engine_build = Some(serde_json::to_value(pending).unwrap());
            std::fs::create_dir_all(root.join("source")).unwrap();
            std::fs::write(root.join("source/content.bin"), b"owned fixture").unwrap();
            game.setup_inputs.insert("base".into(), root.join("source"));
            let profile = engines::pending::profile(&game).unwrap().unwrap();
            let signature = engines::input_signature(&profile, &game, &root).unwrap();
            let mut library = Library::default();
            library.put(&game.id, game.clone());
            let app = tests::app_in(&root, library);
            let run = SETUP_NEXT.fetch_add(1, Ordering::Relaxed);
            SETUP_RUN.lock().unwrap().get_or_insert_with(HashMap::new).insert(game.id.clone(), run);
            Self { app, game, signature, run }
        }
        fn ready(&self) -> Installed {
            let mut ready = self.game.clone();
            ready.engine_build = Some(json!({"frozen_fixture":true}));
            ready.exe = Some("Game/runtime".into());
            ready.files = vec!["Game/Runtime".into()];
            ready.inputs_ready = self.signature.clone();
            ready.version = "prepared-version".into();
            ready.pinned = true;
            ready.auto_update = false;
            ready.mode = library::SetupMode::Easy;
            ready
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            if let Some(runs) = SETUP_RUN.lock().unwrap().as_mut() { runs.remove(&self.game.id); }
            let _ = std::fs::remove_dir_all(&self.app.dir);
        }
    }

    #[test]
    fn activation_releases_global_locks_and_preserves_concurrent_preferences() {
        let f = Fixture::new("000001");
        let ready = f.ready();
        finish_engine_setup(&f.app, f.run, &f.game, &f.signature, &ready, &AtomicBool::new(false), &|_, result, _| {
            assert_run_lock_available();
            let mut library = f.app.lib.try_lock().expect("activation must not hold the library lock");
            let latest = library.installed.get_mut(&f.game.id).unwrap();
            latest.name = "My renamed game".into();
            latest.args = "--my-launch-option".into();
            latest.picks.insert("resolution".into(), 2);
            latest.played_secs = 42;
            Ok(result.clone())
        }).unwrap();
        let library = f.app.lib.lock().unwrap();
        let actual = &library.installed[&f.game.id];
        assert_eq!(actual.name, "My renamed game");
        assert_eq!(actual.args, "--my-launch-option");
        assert_eq!(actual.picks["resolution"], 2);
        assert_eq!(actual.played_secs, 42);
        assert_eq!(actual.engine_build, ready.engine_build);
        assert_eq!(actual.exe, ready.exe);
        assert_eq!(actual.inputs_ready, f.signature);
    }

    #[test]
    fn changed_input_selection_after_activation_keeps_result_without_publishing() {
        let f = Fixture::new("000002");
        let marker = f.game.path.join("completed-result");
        let result = finish_engine_setup(&f.app, f.run, &f.game, &f.signature, &f.ready(), &AtomicBool::new(false), &|_, ready, _| {
            std::fs::write(&marker, b"completed work").unwrap();
            let mut library = f.app.lib.lock().unwrap();
            library.installed.get_mut(&f.game.id).unwrap().setup_inputs.insert("base".into(), f.app.dir.join("new-choice"));
            Ok(ready.clone())
        });
        assert!(result.unwrap_err().contains("changed"));
        assert!(marker.is_file());
        assert_eq!(f.app.lib.lock().unwrap().installed[&f.game.id].engine_build, f.game.engine_build);
    }

    #[test]
    fn replacement_generation_during_activation_prevents_publication() {
        let f = Fixture::new("000004");
        let result = finish_engine_setup(&f.app, f.run, &f.game, &f.signature, &f.ready(), &AtomicBool::new(false), &|_, ready, _| {
            let mut library = f.app.lib.lock().unwrap();
            library.installed.get_mut(&f.game.id).unwrap().engine_build.as_mut().unwrap()["generation"] = json!("replacement");
            Ok(ready.clone())
        });
        assert!(result.unwrap_err().contains("changed"));
        assert_eq!(f.app.lib.lock().unwrap().installed[&f.game.id].engine_build.as_ref().unwrap()["generation"], "replacement");
    }

    #[test]
    fn cancellation_precedes_initial_input_filesystem_checks() {
        let f = Fixture::new("000005");
        std::fs::remove_dir_all(f.app.dir.join("source")).unwrap();
        let result = refresh_engine_setup(&f.app, f.run, &f.game, &AtomicBool::new(true));
        assert_eq!(result.err().unwrap(), "Stopped.");
        assert!(f.app.lib.try_lock().is_ok());
        assert_run_lock_available();
    }

    #[test]
    fn stop_during_activation_prevents_publication() {
        let f = Fixture::new("000003");
        let result = finish_engine_setup(&f.app, f.run, &f.game, &f.signature, &f.ready(), &AtomicBool::new(false), &|_, ready, _| {
            assert_run_lock_available();
            setup_cancel(f.game.id.clone());
            Ok(ready.clone())
        });
        assert_eq!(result.unwrap_err(), "Stopped.");
        assert_eq!(f.app.lib.lock().unwrap().installed[&f.game.id].engine_build, f.game.engine_build);
    }
}

mod engine_pin_tests {
    use super::super::*;
    use super::super::tests::app_in;
    use super::super::engine_setup::*;

    #[test]
    fn pending_engine_uses_named_setup_without_catalog_override() {
        let game = crate::engines::nxnative::catalog::games().remove(0);
        let root = std::env::temp_dir().join(format!("rl-pending-profile-{}",std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let g = crate::engines::pending::create(&game,&root).unwrap();
        let p = installation_profile(&g,&serde_json::json!({"launcher":{"format":1,"start":{"program":["wrong"]}}})).unwrap();
        assert!(p.inputs.iter().any(|i|i.id == "base"));
        assert!(p.start.is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    fn engine_record() -> Value {
        let package = json!({"engine": "nxnative", "target": "macos-arm64", "upstream_revision": "a".repeat(40),
            "patch_sha256": "b".repeat(64), "toolchain_sha256": "c".repeat(64), "archive_sha256": "d".repeat(64),
            "packaging_revision": "1", "adapter_version": 1});
        json!({"format": 1, "game": {"console": "switch", "title_id": "0100000000000001", "version": "1", "build_id": "abc"},
            "package": package, "runtime": package, "components": {"code": "1".repeat(64), "shaders": "2".repeat(64), "runtime": "3".repeat(64), "data": "4".repeat(64)},
            "profile": {"format": 1, "start": {"program": ["game-bin"]}, "saves": ["{install}/pinned-saves"]}, "required_files": []})
    }

    fn engine_app(name: &str, mut entry: Value) -> (PathBuf, App) {
        let dir = std::env::temp_dir().join(format!("rl-engine-{name}-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("cache")).unwrap();
        entry["id"] = json!("game");
        std::fs::write(dir.join("cache/pc-catalog.json"), serde_json::to_vec(&json!({"apps": [entry]})).unwrap()).unwrap();
        let mut lib = Library::default();
        lib.installed.insert("game".into(), Installed { id: "game".into(), path: dir.join("game"), engine_build: Some(engine_record()), ..Default::default() });
        let app = app_in(&dir, lib);
        (dir, app)
    }

    #[test]
    fn a_new_release_never_asks_a_prepared_engine_game_to_build_again() {
        let (dir, app) = engine_app("new-release", json!({}));
        let mut g = app.lib.lock().unwrap().installed["game"].clone();
        g.tag = "v2".into();
        g.set_up = "v1".into();
        let profile: profile::Profile = serde_json::from_value(json!({"format": 1, "inputs": [
            {"id": "base", "label": "Game", "formats": [{"type": "folder"}]}
        ]})).unwrap();
        let status = game_status(&app, &g, &profile, None);
        assert!(!status.changed);
        assert_ne!(status.reason, "rebuild");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn engine_dispatch_never_calls_script_or_installation_handlers() {
        for name in ["game.cmd", "game.bat", "game.sh", "game.py", "game.exe", "game.AppImage", "game.flatpak"] {
            let (exe, ext, command) = pinned_command(Path::new(name));
            assert_eq!(exe, Path::new(name));
            assert!(ext.is_empty());
            assert_eq!(command.as_std().get_program(), name);
            assert_eq!(command.as_std().get_args().count(), 0);
        }
    }

    #[test]
    fn engine_setup_cannot_be_skipped_by_the_global_switch() {
        let (dir, app) = engine_app("skip-setup", json!({}));
        app.lib.lock().unwrap().settings.ui = json!({"easy_skip_all": true});
        assert!(!player_sets_up(&app, "game"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn engine_launch_does_not_use_a_player_program_override() {
        let (dir, app) = engine_app("program-override", json!({}));
        let mut g = app.lib.lock().unwrap().installed["game"].clone();
        std::fs::create_dir_all(&g.path).unwrap();
        std::fs::write(g.path.join("other-program"), b"other").unwrap();
        g.exe = Some("other-program".into());
        g.exe_rules = u8::MAX;
        assert!(player_program(&app, "game", &g).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn engine_launch_does_not_append_arguments_that_override_its_game_data() {
        let g = Installed { engine_build: Some(engine_record()), args: "--game=/another/build".into(), ..Default::default() };
        assert!(player_launch_args(&g).is_empty());
        let ordinary = Installed { engine_build: None, ..g };
        assert_eq!(player_launch_args(&ordinary), ["--game=/another/build"]);
    }

    #[test]
    fn engine_move_does_not_adopt_another_folder_with_the_same_program_name() {
        let (dir, app) = engine_app("move-existing", json!({}));
        let mut g = app.lib.lock().unwrap().installed["game"].clone();
        g.exe = Some("game-bin".into());
        let other = dir.join("other");
        for folder in [&g.path, &other] {
            std::fs::create_dir_all(folder).unwrap();
            std::fs::write(folder.join("game-bin"), b"different builds can use the same filename").unwrap();
        }
        assert!(!holds_same_game(&g, &other));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn engine_locate_cannot_retarget_a_saved_pin_by_program_name() {
        let (dir, app) = engine_app("locate-existing", json!({}));
        let mut g = app.lib.lock().unwrap().installed["game"].clone();
        g.exe = Some("game-bin".into());
        let recorded_path = g.path.clone();
        let other = dir.join("other");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("game-bin"), b"another build").unwrap();
        assert!(locate_installed(&mut g, other.clone()).is_err());
        assert_eq!(g.path, recorded_path);
        g.engine_build = None;
        assert!(locate_installed(&mut g, other.clone()).is_ok());
        assert_eq!(g.path, other);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_stale_input_selection_cannot_replace_an_activated_engine_pin() {
        let (dir, app) = engine_app("stale-pick", json!({}));
        let pin = engine_record();
        app.lib.lock().unwrap().installed.get_mut("game").unwrap().engine_build = Some(pin.clone());
        let mut selected = app.lib.lock().unwrap().installed["game"].clone();
        selected.engine_build = Some(json!({"state": "pending"}));
        selected.setup_inputs.insert("base".into(), "/game.nsp".into());
        assert!(save_picks(&app, "game", &selected).is_err());
        assert_eq!(app.lib.lock().unwrap().installed["game"].engine_build, Some(pin));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn pending_engine_is_not_ready_just_because_its_inputs_are_ready() {
        let (dir, app) = engine_app("pending-status", json!({}));
        let game = engines::nxnative::catalog::games().remove(0);
        let g = engines::pending::create(&game, &dir.join("pending")).unwrap();
        let p = engines::pending::profile(&g).unwrap().unwrap();
        app.lib.lock().unwrap().installed.insert(g.id.clone(), g.clone());
        let status = easy_status(&app, &g.id, &g, &p, None);
        assert!(!status.ready);
        assert_eq!(status.reason, "files");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn engine_saves_follow_the_pin_instead_of_the_catalog() {
        let (dir, app) = engine_app("saves", json!({"launcher": {"format": 1, "saves": ["{install}/new-saves"]}, "saves": ["{install}/catalog-saves"]}));
        let g = app.lib.lock().unwrap().installed["game"].clone();
        let spots = cloud::spots_of(&app, "game", &g);
        assert_eq!(spots.iter().map(|s| s.template.as_str()).collect::<Vec<_>>(), ["{install}/pinned-saves"]);
        let mut unknown = g;
        unknown.engine_build = Some(json!({"format": 99}));
        assert!(cloud::spots_of(&app, "game", &unknown).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn changing_projects_keeps_engine_installations_and_their_pins() {
        let (dir, app) = engine_app("project-pin", json!({"repo":"other/game"}));
        let cat = json!({"apps":[{"id":"other","repo":"other/game"}]});
        for record in [engine_record(), json!({"state":"pending"}), Value::Null] {
            app.lib.lock().unwrap().installed.get_mut("game").unwrap().engine_build = Some(record);
            let before = serde_json::to_value(&app.lib.lock().unwrap().installed["game"]).unwrap();
            for project in ["other", ""] {
                assert!(adopt::change_project(&app,"game",project,&cat).is_err());
                assert_eq!(serde_json::to_value(&app.lib.lock().unwrap().installed["game"]).unwrap(),before);
            }
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn associating_a_legacy_installation_cannot_create_an_unprepared_engine_route() {
        let (dir, app) = engine_app("project-route", json!({"repo":"other/game"}));
        app.lib.lock().unwrap().installed.get_mut("game").unwrap().engine_build = None;
        let before = serde_json::to_value(&app.lib.lock().unwrap().installed["game"]).unwrap();
        let mut cat = json!({"apps":[]});
        engines::catalog::merge(&mut cat,Some(engines::Target::MacosArm64));
        let target = cat["apps"][0]["id"].as_str().unwrap();
        assert!(adopt::change_project(&app,"game",target,&cat).is_err());
        assert_eq!(serde_json::to_value(&app.lib.lock().unwrap().installed["game"]).unwrap(),before);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn engine_games_cannot_gain_children_from_a_changed_catalog() {
        let (dir, app) = engine_app("children", json!({"games": [{"id": "one", "name": "One", "args": ["--rebuild"]}, {"id": "two", "name": "Two"}]}));
        let g = app.lib.lock().unwrap().installed["game"].clone();
        assert!(games_of(&app, "game", &g).is_empty());
        assert!(game_of(&app, "game--one").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unknown_engine_records_never_fall_back_to_catalog_profiles() {
        let entry = json!({"launcher": {"format": 1, "start": {"program": ["new-driver"]}}});
        let mut g = Installed { engine_build: Some(json!({"format": 99})), ..Default::default() };
        assert!(installation_profile(&g, &entry).is_none());
        g.engine_build = None;
        assert_eq!(installation_profile(&g, &entry).unwrap().start.unwrap().program, ["new-driver"]);
    }

    #[test]
    fn engine_pins_are_not_offered_ordinary_release_updates() {
        let g = Installed {
            tag: "v1".into(), engine_build: Some(json!({"format": 99})), ..Default::default()
        };
        let releases: Vec<Release> = serde_json::from_value(json!([
            {"tag_name": "v2", "assets": [
                {"name": "game-linux-x64.tar.gz", "browser_download_url": "https://example.com/linux", "size": 10},
                {"name": "game-windows-x64.zip", "browser_download_url": "https://example.com/windows", "size": 10},
                {"name": "game-macos-arm64.zip", "browser_download_url": "https://example.com/mac", "size": 10}
            ]}
        ])).unwrap();
        assert_eq!(offered_update(&g, None, &releases), None);
    }

    #[test]
    fn engine_catalog_does_not_reassign_a_personal_repo_entry_to_an_arbitrary_game() {
        let d = std::env::temp_dir().join(format!("rl-engine-merge-{}",std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        catalog::save_mine(&d,&[json!({"id":"mine-nxnative","repo":"seedds/nxnative"})]).unwrap();
        let st = app_in(&d,Library::default());
        let mut cat=json!({"apps":[]});
        engines::catalog::merge(&mut cat,Some(engines::Target::MacosArm64));
        assert!(merge_mine(&st,&cat).is_empty());
        assert_eq!(catalog::mine(&d)[0]["id"],"mine-nxnative");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn preparation_reservation_blocks_other_installation_mutations() {
        let st=app_in(Path::new(""),Library::default());
        let first=InstallationMutation::acquire(&st,"game").unwrap();
        assert!(InstallationMutation::acquire(&st,"game").is_err());
        drop(first);
        assert!(InstallationMutation::acquire(&st,"game").is_ok());
    }

    #[test]
    fn engine_commit_requires_same_installation_generation_and_path() {
        let game=engines::nxnative::catalog::games().remove(0);
        let root=std::env::temp_dir().join(format!("rl-generation-{}",std::process::id()));
        let _=std::fs::remove_dir_all(&root);
        let first=engines::pending::create(&game,&root).unwrap();
        std::fs::remove_dir(&root).unwrap();
        let second=engines::pending::create(&game,&root).unwrap();
        assert!(!engines::pending::same_installation(&first,&second));
        let mut moved=first.clone();moved.path=root.join("moved");
        assert!(!engines::pending::same_installation(&first,&moved));
        std::fs::remove_dir_all(root).unwrap();
    }
}

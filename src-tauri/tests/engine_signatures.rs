use recomplauncher_lib::{engines::{self, catalog, pending, prepared, registry, Target}, library::Installed, profile::Profile};
use std::{fs, path::PathBuf};

struct Fixture { root: PathBuf, game: Installed, profile: Profile }
impl Fixture {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("rl-engine-signature-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("source/nested")).unwrap();
        fs::write(root.join("source/nested/module.bin"), b"original").unwrap();
        let title = catalog::game("scarletnx-0100633007d48000").unwrap();
        let mut game = pending::create_for(&title, Target::LinuxX64, &root.join("install")).unwrap();
        game.setup_inputs.insert("base".into(), root.join("source"));
        let profile: Profile = serde_json::from_value(serde_json::json!({"format":1,"inputs":[
            {"id":"base","label":"Original game folder","formats":[{"type":"folder"}]}
        ]})).unwrap();
        game.engine_build.as_mut().unwrap()["input_signature_version"] = "tree-v1".into();
        game.engine_build.as_mut().unwrap()["input_profile"] = serde_json::to_value(&profile).unwrap();
        Self { root, game, profile }
    }
    fn signature(&self) -> String { engines::input_signature(&self.profile, &self.game, &self.root).unwrap() }
}
impl Drop for Fixture { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.root); } }

#[test]
fn new_tree_preparations_detect_nested_content_changes_without_root_metadata_changes() {
    let fixture = Fixture::new("edit");
    let before = fixture.signature();
    let root_time = fs::metadata(fixture.root.join("source")).unwrap().modified().unwrap();
    fs::write(fixture.root.join("source/nested/module.bin"), b"changed content with a different length").unwrap();
    assert_eq!(root_time, fs::metadata(fixture.root.join("source")).unwrap().modified().unwrap());
    assert!(before.starts_with("engine-tree-v1:"));
    assert_ne!(before, fixture.signature());
}

#[test]
fn nested_additions_removals_and_renames_change_the_tree_signature() {
    let fixture = Fixture::new("entries");
    let original = fixture.signature();
    fs::write(fixture.root.join("source/nested/another"), "new").unwrap();
    let added = fixture.signature(); assert_ne!(original, added);
    fs::rename(fixture.root.join("source/nested/another"), fixture.root.join("source/nested/renamed")).unwrap();
    let renamed = fixture.signature(); assert_ne!(added, renamed);
    fs::remove_file(fixture.root.join("source/nested/renamed")).unwrap();
    assert_ne!(renamed, fixture.signature());
}

#[test]
fn legacy_pending_records_keep_their_original_signature_and_work_directory() {
    let mut fixture = Fixture::new("legacy");
    fixture.game.engine_build.as_mut().unwrap().as_object_mut().unwrap().remove("input_signature_version");
    fixture.game.engine_build.as_mut().unwrap().as_object_mut().unwrap().remove("input_profile");
    let before = fixture.signature();
    let work = prepared::work_path(&fixture.game, &before).unwrap();
    assert!(!before.contains(':'));
    let saved = pending::read(&fixture.game).unwrap().unwrap();
    assert_eq!(serde_json::to_value(saved).unwrap(), fixture.game.engine_build.clone().unwrap());
    fs::write(fixture.root.join("source/nested/module.bin"), b"legacy metadata behavior remains compatible").unwrap();
    assert_eq!(before, fixture.signature());
    assert_eq!(work, prepared::work_path(&fixture.game, &before).unwrap());
    let retried = pending::for_setup(&fixture.game).unwrap();
    assert_eq!(retried.engine_build, fixture.game.engine_build);
}

#[test]
fn opted_in_adapters_snapshot_their_input_contract_when_creating_a_pending_record() {
    for adapter in engines::ADAPTERS.iter().filter(|a| a.input_signature_version() == engines::InputSignatureVersion::TreeV1) {
        let title = adapter.games().remove(0);
        let root = std::env::temp_dir().join(format!("rl-new-signature-{}-{}", adapter.id(), std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let game = pending::create_for(&title, adapter.targets()[0], &root).unwrap();
        assert_eq!(game.engine_build.as_ref().unwrap()["input_signature_version"], "tree-v1");
        assert_eq!(game.engine_build.as_ref().unwrap()["input_profile"], serde_json::to_value(adapter.profile(&title)).unwrap());
        fs::remove_dir_all(root).unwrap();
    }
    for name in ["nxnative", "scarletnx"] {
        assert_eq!(serde_json::to_value(registry::get(name).unwrap().input_signature_version()).unwrap(), "legacy");
    }
}

#[cfg(unix)]
#[test]
fn input_tree_links_are_recorded_without_following_cycles() {
    let fixture = Fixture::new("links");
    std::os::unix::fs::symlink("..", fixture.root.join("source/nested/cycle")).unwrap();
    let before = fixture.signature();
    fs::remove_file(fixture.root.join("source/nested/cycle")).unwrap();
    std::os::unix::fs::symlink("module.bin", fixture.root.join("source/nested/cycle")).unwrap();
    assert_ne!(before, fixture.signature());
}

fn native_output(fixture: &Fixture, signature: &str) -> prepared::Output {
    let output = prepared::work_path(&fixture.game, signature).unwrap().join("output");
    fs::create_dir_all(&output).unwrap();
    let mut elf = vec![0; 64]; elf[..6].copy_from_slice(b"\x7fELF\x02\x01"); elf[18] = 62;
    fs::write(output.join("game.elf"), elf).unwrap();
    prepared::Output {
        identity:engines::GameIdentity { console:engines::Console::Switch, title_id:"0100633007D48000".into(), version:"1.0".into(), build_id:"fixture".into() },
        start:recomplauncher_lib::profile::Start { program:vec!["game.elf".into()], ..Default::default() },
        saves:vec!["saves".into()], files:vec!["game.elf".into()], required_files:vec![],
    }
}

#[test]
fn legacy_completed_results_recover_with_the_same_pin_without_adopting_tree_rules() {
    let mut fixture = Fixture::new("legacy-completed");
    fixture.game.engine_build.as_mut().unwrap().as_object_mut().unwrap().remove("input_signature_version");
    fixture.game.engine_build.as_mut().unwrap().as_object_mut().unwrap().remove("input_profile");
    let signature = fixture.signature();
    let ready = prepared::register(&fixture.game, &signature, native_output(&fixture, &signature)).unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(fixture.game.path.join(".engine-prepared.json")).unwrap()).unwrap();
    assert!(saved["pending"].get("input_signature_version").is_none());
    let retried = pending::for_setup(&fixture.game).unwrap();
    assert_eq!(retried.engine_build, fixture.game.engine_build);
    let recovered = prepared::recover(&retried, &fixture.signature()).unwrap().unwrap();
    assert_eq!(ready.engine_build, recovered.engine_build);
    prepared::activate(&retried, &recovered, &|| false).unwrap();
}

#[test]
fn prepared_tree_games_launch_from_frozen_output_after_external_inputs_are_removed() {
    let fixture = Fixture::new("frozen");
    let signature = fixture.signature();
    let ready = prepared::register(&fixture.game, &signature, native_output(&fixture, &signature)).unwrap();
    let active = prepared::activate(&fixture.game, &ready, &|| false).unwrap();
    fs::remove_dir_all(fixture.root.join("source")).unwrap();
    assert!(engines::pinned_executable(&active, Target::LinuxX64).unwrap().is_some());
    assert_eq!(engines::read_build(&active).unwrap().unwrap().package, pending::read(&fixture.game).unwrap().unwrap().package);
}

#[test]
fn checkpoint_retry_preserves_an_existing_pending_signature_version() {
    let mut fixture = Fixture::new("retry");
    let record = fixture.game.engine_build.as_mut().unwrap();
    record.as_object_mut().unwrap().remove("input_signature_version");
    record.as_object_mut().unwrap().remove("input_profile");
    record["package"]["packaging_revision"] = "old-recipe".into();
    let retried = pending::for_setup(&fixture.game).unwrap();
    assert!(retried.engine_build.as_ref().unwrap().get("input_signature_version").is_none());
    assert_ne!(retried.engine_build, fixture.game.engine_build);
}

#[test]
fn a_stopped_fingerprint_does_not_walk_the_input_tree() {
    let fixture = Fixture::new("cancel");
    let error = engines::input_signature_with_cancel(&fixture.profile, &fixture.game, &fixture.root, &|| true).unwrap_err();
    assert_eq!(error, "Stopped.");
}

#[test]
fn pending_profile_and_signature_keep_the_saved_labels_and_formats() {
    let fixture = Fixture::new("profile");
    let saved = pending::profile(&fixture.game).unwrap().unwrap();
    assert_eq!(serde_json::to_value(saved).unwrap(), serde_json::to_value(&fixture.profile).unwrap());
    let current = registry::get("scarletnx").unwrap().profile(&pending::read(&fixture.game).unwrap().unwrap().game);
    assert_ne!(current.inputs[0].label, fixture.profile.inputs[0].label);
    assert_eq!(fixture.signature(), engines::input_signature(&current, &fixture.game, &fixture.root).unwrap());
}

#[test]
fn a_completed_preparation_recovers_after_the_current_input_declaration_changes() {
    use std::sync::{Arc, atomic::AtomicBool};
    let mut fixture = Fixture::new("profile-recovery");
    fixture.game.engine_build.as_mut().unwrap()["package"]["packaging_revision"] = "previous-recipe".into();
    let signature = fixture.signature();
    let ready = prepared::register(&fixture.game, &signature, native_output(&fixture, &signature)).unwrap();
    let recovered = prepared::prepare(registry::get("scarletnx").unwrap(), &reqwest::Client::new(), &fixture.root, &fixture.game,
        Arc::new(AtomicBool::new(false)), &|_| {}, &|_| panic!("A completed result must never rebuild")).unwrap();
    assert_eq!(ready.engine_build, recovered.engine_build);
    assert_eq!(pending::for_setup(&fixture.game).unwrap().engine_build, fixture.game.engine_build);
}

#[test]
fn only_an_explicit_checkpoint_retry_refreshes_a_tree_records_input_profile() {
    let mut fixture = Fixture::new("profile-retry");
    let unchanged = pending::for_setup(&fixture.game).unwrap();
    assert_eq!(unchanged.engine_build, fixture.game.engine_build);
    fixture.game.engine_build.as_mut().unwrap()["package"]["packaging_revision"] = "previous-recipe".into();
    let retried = pending::for_setup(&fixture.game).unwrap();
    let title = pending::read(&retried).unwrap().unwrap().game;
    let current = title.setup_profile();
    assert_eq!(retried.engine_build.as_ref().unwrap()["input_profile"], serde_json::to_value(current).unwrap());
    assert_eq!(retried.engine_build.as_ref().unwrap()["input_signature_version"], "tree-v1");
}

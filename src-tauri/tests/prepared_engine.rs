use recomplauncher_lib::{engines::{self, catalog, pending, prepared, Console, GameIdentity, Target}, profile::Start};
use std::{fs, path::PathBuf};

struct Fixture {
    root: PathBuf,
    game: recomplauncher_lib::library::Installed,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!("rl-prepared-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let title = catalog::game("scarletnx-0100633007d48000").unwrap();
        let game = pending::create_for(&title, Target::WindowsX64, &root.join("game")).unwrap();
        let output = prepared::work_path(&game, "inputs").unwrap().join("output");
        fs::create_dir_all(output.join("data")).unwrap();
        let mut header = vec![0; 256];
        header[..2].copy_from_slice(b"MZ");
        header[60..64].copy_from_slice(&128u32.to_le_bytes());
        header[128..134].copy_from_slice(b"PE\0\0\x64\x86");
        fs::write(output.join("runtime.exe"), header).unwrap();
        fs::write(output.join("data/game.bin"), b"owned game content").unwrap();
        Self { root, game }
    }

    fn output(&self) -> prepared::Output {
        prepared::Output {
            identity: GameIdentity { console: Console::Switch, title_id: "0100633007D48000".into(), version: "1.0".into(), build_id: "fixture-code".into() },
            start: Start { program: vec!["runtime.exe".into()], args: vec!["--data={install}/Game/data".into()], env: Default::default() },
            saves: vec!["saves".into()],
            files: vec!["runtime.exe".into(), "data".into()],
            required_files: vec!["data/game.bin".into()],
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) { let _ = fs::remove_dir_all(&self.root); }
}

#[test]
fn generic_preparation_registers_a_frozen_native_output() {
    let f = Fixture::new("register");
    let ready = prepared::register(&f.game, "inputs", f.output()).unwrap();
    let build = engines::read_build(&ready).unwrap().unwrap();
    assert_eq!(build.profile.start.unwrap().program, ["Game/runtime.exe"]);
    assert_eq!(build.profile.saves, ["{install}/Game/saves"]);
    assert!(ready.files.contains(&PathBuf::from("Game/data")));
    assert!(!ready.files.contains(&PathBuf::from("Game/saves")));
    assert!(ready.pinned);
    assert!(!ready.auto_update);
}

#[test]
fn backup_ownership_keeps_parent_roots_instead_of_every_runtime_header() {
    let f = Fixture::new("owned-roots");
    let mut output = f.output();
    output.required_files.push("data/game.bin".into());
    let ready = prepared::register(&f.game, "inputs", output).unwrap();
    assert!(ready.files.contains(&PathBuf::from("Game/data")));
    assert!(!ready.files.contains(&PathBuf::from("Game/data/game.bin")));
    assert!(engines::read_build(&ready).unwrap().unwrap().required_files.contains(&PathBuf::from("Game/data/game.bin")));
}

#[test]
fn frozen_paths_use_portable_separators_after_native_directory_traversal() {
    let f = Fixture::new("portable-paths");
    let stage = prepared::work_path(&f.game, "inputs").unwrap().join("output");
    let native_path = fs::read_dir(stage.join("data")).unwrap().next().unwrap().unwrap().path();
    let mut output = f.output();
    output.required_files = vec![native_path.strip_prefix(stage).unwrap().into()];
    let ready = prepared::register(&f.game, "inputs", output).unwrap();
    let pin = engines::read_build(&ready).unwrap().unwrap();
    assert_eq!(pin.required_files[0].to_str().unwrap(), "Game/data/game.bin");
    assert_eq!(pin.profile.start.unwrap().program[0], "Game/runtime.exe");
}

#[test]
fn recovery_rejects_changed_required_runtime_bytes_before_publication() {
    let f = Fixture::new("changed-staged-runtime");
    let mut output = f.output();
    output.required_files.push("data/game.bin".into());
    let ready = prepared::register(&f.game, "inputs", output).unwrap();
    let stage = prepared::work_path(&f.game, "inputs").unwrap().join("output");
    fs::write(stage.join("data/game.bin"), b"different runtime data").unwrap();
    assert!(prepared::activate(&f.game, &ready, &|| false).is_err());
    assert!(!prepared::output_present(&f.game.path));
}

#[test]
fn recovery_rejects_another_native_program_after_publication() {
    let f = Fixture::new("changed-published-runtime");
    let ready = prepared::register(&f.game, "inputs", f.output()).unwrap();
    prepared::activate(&f.game, &ready, &|| false).unwrap();
    let program = f.game.path.join("Game/runtime.exe");
    let mut bytes = fs::read(&program).unwrap();
    bytes.push(42);
    fs::write(program, bytes).unwrap();
    assert!(prepared::activate(&f.game, &ready, &|| false).is_err());
}

#[test]
fn stopping_runtime_hashing_does_not_publish_a_completed_record() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let f = Fixture::new("stop-hash");
    let checks = AtomicUsize::new(0);
    let stopped = || checks.fetch_add(1, Ordering::Relaxed) > 1;
    assert_eq!(prepared::register_with_cancel(&f.game, "inputs", f.output(), &stopped).unwrap_err(), "Stopped.");
    assert!(!f.game.path.join(".engine-prepared.json").exists());
}

#[cfg(unix)]
#[test]
fn recovery_rejects_a_staged_root_replaced_with_an_external_link() {
    let f = Fixture::new("stage-root-link");
    let ready = prepared::register(&f.game, "inputs", f.output()).unwrap();
    let stage = prepared::work_path(&f.game, "inputs").unwrap().join("output");
    let outside = f.root.join("outside");
    fs::rename(&stage, &outside).unwrap();
    std::os::unix::fs::symlink(&outside, &stage).unwrap();
    assert!(prepared::activate(&f.game, &ready, &|| false).is_err());
    assert!(!prepared::output_present(&f.game.path));
}

#[cfg(unix)]
#[test]
fn recovery_rejects_a_published_root_replaced_with_an_external_link() {
    let f = Fixture::new("published-root-link");
    let ready = prepared::register(&f.game, "inputs", f.output()).unwrap();
    prepared::activate(&f.game, &ready, &|| false).unwrap();
    let published = f.game.path.join("Game");
    let outside = f.root.join("outside");
    fs::rename(&published, &outside).unwrap();
    std::os::unix::fs::symlink(&outside, &published).unwrap();
    assert!(prepared::activate(&f.game, &ready, &|| false).is_err());
}

#[test]
fn generic_activation_recovers_both_sides_of_the_rename() {
    let f = Fixture::new("recover");
    let ready = prepared::register(&f.game, "inputs", f.output()).unwrap();
    let mut current = f.game.clone();
    current.name = "My preferred name".into();
    let active = prepared::activate(&current, &ready, &|| false).unwrap();
    let recovered = prepared::recover(&current, "inputs").unwrap().unwrap();
    let again = prepared::activate(&current, &recovered, &|| false).unwrap();
    assert_eq!(active.engine_build, again.engine_build);
    assert_eq!(again.name, "My preferred name");
    assert!(engines::pinned_executable(&again, Target::WindowsX64).unwrap().is_some());
}

#[test]
fn generic_recovery_rejects_another_generation() {
    let f = Fixture::new("generation");
    prepared::register(&f.game, "inputs", f.output()).unwrap();
    let mut current = f.game.clone();
    current.engine_build.as_mut().unwrap()["generation"] = "another-attempt".into();
    assert!(prepared::recover(&current, "inputs").is_err());
    assert!(prepared::recover(&f.game, "changed-inputs").is_err());
}

#[test]
fn generic_publication_keeps_completed_output_when_cancelled() {
    let f = Fixture::new("cancel");
    let ready = prepared::register(&f.game, "inputs", f.output()).unwrap();
    assert!(prepared::activate(&f.game, &ready, &|| true).is_err());
    assert!(prepared::recover(&f.game, "inputs").unwrap().is_some());
    assert!(!prepared::output_present(&f.game.path));
}

#[test]
fn generic_registration_rejects_another_title() {
    let f = Fixture::new("title");
    let mut output = f.output();
    output.identity.title_id = "another-game".into();
    assert!(prepared::register(&f.game, "inputs", output).is_err());
    assert!(!f.game.path.join(".engine-prepared.json").exists());
}

#[test]
fn generic_registration_rejects_a_wrong_target_binary() {
    let f = Fixture::new("binary");
    let output = prepared::work_path(&f.game, "inputs").unwrap().join("output");
    fs::write(output.join("runtime.exe"), b"#!/bin/sh\nrebuild game").unwrap();
    assert!(prepared::register(&f.game, "inputs", f.output()).is_err());
}

#[test]
fn generic_registration_rejects_missing_runtime_data() {
    let f = Fixture::new("missing");
    let output = prepared::work_path(&f.game, "inputs").unwrap().join("output");
    fs::remove_file(output.join("data/game.bin")).unwrap();
    assert!(prepared::register(&f.game, "inputs", f.output()).is_err());
}

#[test]
fn generic_registration_rejects_save_and_game_data_overlap() {
    let f = Fixture::new("saves");
    let mut output = f.output();
    output.saves = vec!["data/saves".into()];
    assert!(prepared::register(&f.game, "inputs", output).is_err());
}

#[test]
fn generic_registration_preserves_an_existing_output() {
    let f = Fixture::new("existing");
    fs::create_dir(f.game.path.join("Game")).unwrap();
    fs::write(f.game.path.join("Game/keep"), b"keep").unwrap();
    assert!(prepared::register(&f.game, "inputs", f.output()).is_err());
    assert_eq!(fs::read(f.game.path.join("Game/keep")).unwrap(), b"keep");
}

#[cfg(unix)]
#[test]
fn generic_registration_rejects_a_link_outside_the_output() {
    let f = Fixture::new("symlink");
    let output = prepared::work_path(&f.game, "inputs").unwrap().join("output");
    std::os::unix::fs::symlink(&f.root, output.join("escape")).unwrap();
    assert!(prepared::register(&f.game, "inputs", f.output()).is_err());
}

#[test]
fn required_runtime_files_participate_in_the_frozen_fingerprint() {
    let first = Fixture::new("fingerprint-a");
    let second = Fixture::new("fingerprint-b");
    let path = prepared::work_path(&second.game, "inputs").unwrap().join("output/data/game.bin");
    fs::write(path, b"a different frozen dependency").unwrap();
    let a = prepared::register(&first.game, "inputs", first.output()).unwrap();
    let b = prepared::register(&second.game, "inputs", second.output()).unwrap();
    assert_ne!(engines::read_build(&a).unwrap().unwrap().components.runtime, engines::read_build(&b).unwrap().unwrap().components.runtime);
}

#[test]
fn required_runtime_files_are_always_excluded_from_save_discovery() {
    let f = Fixture::new("required-owned");
    let mut output = f.output();
    output.files.clear();
    let ready = prepared::register(&f.game, "inputs", output).unwrap();
    assert!(ready.files.contains(&PathBuf::from("Game/runtime.exe")));
    assert!(ready.files.contains(&PathBuf::from("Game/data/game.bin")));
}

#[cfg(unix)]
#[test]
fn adapters_choose_which_inherited_environment_to_remove() {
    let f = Fixture::new("environment");
    let mut command = engines::jobs::CommandSpec::new("/usr/bin/env", &f.root);
    command.remove_env = vec!["PATH".into()];
    command.env.insert("RL_EXPLICIT".into(), "kept".into());
    let log = f.root.join("environment.log");
    engines::jobs::run_process(&command, &log, &|| false, &|_| {}).unwrap();
    let text = fs::read_to_string(log).unwrap();
    assert!(!text.lines().any(|line| line.starts_with("PATH=")));
    assert!(text.lines().any(|line| line == "RL_EXPLICIT=kept"));
}

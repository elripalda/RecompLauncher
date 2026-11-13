use recomplauncher_lib::{
    engines::*,
    library::{Installed, Library},
};
use serde_json::{json, Value};

fn game() -> GameIdentity {
    serde_json::from_value(json!({
        "console": "switch", "title_id": "0100000000000001",
        "version": "1.0.0", "build_id": "1234567890abcdef"
    }))
    .unwrap()
}

fn package(engine: &str, target: Target, revision: char) -> PackagePin {
    serde_json::from_value(json!({
        "engine": engine, "target": target, "upstream_revision": revision.to_string().repeat(40),
        "patch_sha256": "b".repeat(64), "toolchain_sha256": "c".repeat(64),
        "archive_sha256": "d".repeat(64), "packaging_revision": "1", "adapter_version": 1
    }))
    .unwrap()
}

fn route(id: &str, rank: u32, target: Target) -> Route {
    Route {
        id: id.into(),
        game: game(),
        package: package("nxnative", target, 'a'),
        rank,
        tested: true,
        evidence: "https://github.com/example/engine/issues/1".into(),
    }
}

fn build(target: Target) -> Value {
    json!({
        "format": 1, "game": game(), "package": package("nxnative", target, 'a'),
        "runtime": package("nxnative", target, 'a'),
        "components": {"code": "1".repeat(64), "shaders": "2".repeat(64), "runtime": "3".repeat(64), "data": "4".repeat(64)},
        "profile": {"format": 1, "start": {"program": ["game-bin"], "args": ["--no-launcher"]}},
        "required_files": ["data/game.dat"]
    })
}

fn installed() -> Installed {
    serde_json::from_value(json!({
        "id": "my-game", "name": "My game", "tag": "v1", "repo": "example/engine",
        "path": "/unused/game", "engine_build": build(Target::MacosArm64)
    }))
    .unwrap()
}

#[test]
fn package_identity_includes_patches_toolchain_and_packaging() {
    let original = package("nxnative", Target::MacosArm64, 'a');
    let mut changed = original.clone();
    changed.patch_sha256 = "e".repeat(64);
    assert_ne!(original.identity(), changed.identity());
    changed = original.clone();
    changed.toolchain_sha256 = "e".repeat(64);
    assert_ne!(original.identity(), changed.identity());
    changed = original.clone();
    changed.packaging_revision = "2".into();
    assert_ne!(original.identity(), changed.identity());
}

#[test]
fn mutable_branch_names_are_not_engine_checkpoints() {
    let mut p = package("nxnative", Target::MacosArm64, 'a');
    p.upstream_revision = "main".into();
    assert!(p.validate().is_err());
}

#[test]
fn multiple_compatible_routes_choose_a_default_without_prompting() {
    let choices = vec![route("second", 2, Target::MacosArm64), route("first", 1, Target::MacosArm64)];
    assert_eq!(select_route(&choices, &game(), Target::MacosArm64, None).unwrap().id, "first");
    assert_eq!(select_route(&choices, &game(), Target::MacosArm64, Some("second")).unwrap().id, "second");
}

#[test]
fn recommendation_ties_are_independent_of_catalog_order() {
    let mut choices = vec![route("z", 1, Target::MacosArm64), route("a", 1, Target::MacosArm64)];
    assert_eq!(select_route(&choices, &game(), Target::MacosArm64, None).unwrap().id, "a");
    choices.reverse();
    assert_eq!(select_route(&choices, &game(), Target::MacosArm64, None).unwrap().id, "a");
}

#[test]
fn unsupported_target_or_game_revision_is_not_offered() {
    let choices = vec![route("mac", 1, Target::MacosArm64)];
    assert!(select_route(&choices, &game(), Target::WindowsX64, None).is_err());
    let mut other = game();
    other.version = "1.0.1".into();
    assert!(select_route(&choices, &other, Target::MacosArm64, None).is_err());
    other = game();
    other.console = Console::Switch2;
    assert!(select_route(&choices, &other, Target::MacosArm64, None).is_err());
}

#[test]
fn an_incompatible_explicit_choice_never_silently_falls_back() {
    let choices = vec![route("mac", 1, Target::MacosArm64), route("windows", 1, Target::WindowsX64)];
    assert!(select_route(&choices, &game(), Target::MacosArm64, Some("windows")).is_err());
}

#[test]
fn an_upstream_listing_without_validation_is_not_a_tested_route() {
    let mut r = route("profile-only", 1, Target::MacosArm64);
    r.tested = false;
    assert!(select_route(&[r], &game(), Target::MacosArm64, None).is_err());
}

#[test]
fn conflicting_route_ids_are_rejected_instead_of_depending_on_catalog_order() {
    let first = route("same-id", 1, Target::MacosArm64);
    let mut second = first.clone();
    second.package.upstream_revision = "f".repeat(40);
    assert!(select_route(&[first, second], &game(), Target::MacosArm64, None).is_err());
}

#[test]
fn engine_metadata_survives_library_round_trip_and_old_games_stay_unmanaged() {
    let data = json!({"installed": {
        "legacy": {"id": "legacy", "name": "Legacy", "tag": "v1", "path": "/old"},
        "my-game": installed()
    }});
    let lib = Library::lenient(&data);
    let encoded = serde_json::to_value(&lib).unwrap();
    assert_eq!(encoded["installed"]["my-game"]["engine_build"], build(Target::MacosArm64));
    assert!(encoded["installed"]["legacy"].get("engine_build").is_none());
}

#[test]
fn installed_pins_and_future_metadata_survive_saving_and_reopening_the_library() {
    let dir = TestFolder::new();
    let mut lib = Library::default();
    let g = installed();
    lib.installed.insert(g.id.clone(), g);
    let mut future = installed();
    future.id = "future".into();
    future.engine_build = Some(json!({"format": 99, "unknown": [1, 2, 3]}));
    lib.installed.insert(future.id.clone(), future);
    lib.save(&dir.0).unwrap();
    let loaded = Library::load(&dir.0);
    assert_eq!(loaded.installed["my-game"].engine_build, Some(build(Target::MacosArm64)));
    assert_eq!(loaded.installed["future"].engine_build, Some(json!({"format": 99, "unknown": [1, 2, 3]})));
}

#[test]
fn future_pin_metadata_is_preserved_but_cannot_launch_or_update() {
    let mut data = serde_json::to_value(installed()).unwrap();
    data["engine_build"] = json!({"format": 99, "future": "keep this"});
    let g: Installed = serde_json::from_value(data).unwrap();
    assert_eq!(serde_json::to_value(&g).unwrap()["engine_build"]["future"], "keep this");
    assert!(pinned_profile(&g).is_err());
    assert!(ensure_legacy_install(Some(&g)).is_err());
    assert!(!release_updates_allowed(&g));
}

#[test]
fn explicit_null_is_a_malformed_pin_not_a_legacy_installation() {
    let mut data = serde_json::to_value(installed()).unwrap();
    data["engine_build"] = Value::Null;
    let g: Installed = serde_json::from_value(data).unwrap();
    assert_eq!(g.engine_build, Some(Value::Null));
    assert!(serde_json::to_value(&g).unwrap().get("engine_build").is_some());
    assert!(!release_updates_allowed(&g));
    assert!(pinned_profile(&g).is_err());
    assert!(ensure_legacy_install(Some(&g)).is_err());
}

#[test]
fn a_pinned_game_cannot_use_ordinary_release_install_or_setup() {
    let g = installed();
    assert!(ensure_legacy_install(Some(&g)).is_err());
    assert!(ensure_legacy_setup(&g).is_err());
    assert!(!release_updates_allowed(&g));
    assert!(ensure_legacy_install(None).is_ok());
    assert!(ensure_legacy_setup(&Installed::default()).is_ok());
}

#[test]
fn pinned_launch_profiles_cannot_recompile_or_choose_another_program() {
    let mut g = installed();
    let frozen = pinned_profile(&g).unwrap().unwrap();
    assert_eq!(frozen.start.unwrap().program, ["game-bin"]);
    g.engine_build.as_mut().unwrap()["profile"]["prepare"] = json!({"program": ["build"]});
    assert!(pinned_profile(&g).is_err());
    g = installed();
    g.engine_build.as_mut().unwrap()["profile"]["start"]["program"] = json!(["game-bin", "latest-driver"]);
    assert!(pinned_profile(&g).is_err());
}

#[test]
fn pinned_paths_cannot_leave_the_game_folder() {
    for path in ["../driver", "/absolute/driver", "C:\\driver.exe", "..\\driver", "{install}/driver"] {
        let mut g = installed();
        g.engine_build.as_mut().unwrap()["profile"]["start"]["program"] = json!([path]);
        assert!(pinned_profile(&g).is_err(), "{path}");
    }
}

#[test]
fn unchanged_components_do_not_rebuild_for_an_engine_commit_change() {
    let g = installed();
    let current = read_build(&g).unwrap().unwrap();
    let mut replacement = current.clone();
    replacement.package.upstream_revision = "e".repeat(40);
    assert_eq!(current.components.work_for(&replacement.components), Work::default());
    replacement.components.runtime = "e".repeat(64);
    let work = current.components.work_for(&replacement.components);
    assert!(work.runtime);
    assert!(!work.code && !work.shaders && !work.data);
}

fn fix() -> Fix {
    let current = read_build(&installed()).unwrap().unwrap();
    let mut next = current.components.clone();
    next.code = "e".repeat(64);
    Fix {
        id: "fix-audio".into(),
        game: game(),
        from: current.package,
        to: package("nxnative", Target::MacosArm64, 'e'),
        components: next,
        evidence: "https://github.com/example/engine/issues/42".into(),
        summary: "Fixes missing audio in this game revision".into(),
    }
}

#[test]
fn applying_a_fix_requires_consent_for_the_exact_installation_and_replacement() {
    let g = installed();
    let f = fix();
    let consent = FixConsent::for_fix(&g, &f).unwrap();
    assert!(consent.authorize(&g, &f).unwrap().code);
    let mut other = g.clone();
    other.id = "another-installation".into();
    assert!(consent.authorize(&other, &f).is_err());
    other = g.clone();
    other.path = "/a-different-installation".into();
    assert!(consent.authorize(&other, &f).is_err());
    let mut other_fix = f.clone();
    other_fix.to.packaging_revision = "2".into();
    assert!(consent.authorize(&g, &other_fix).is_err());
    other = g.clone();
    other.engine_build.as_mut().unwrap()["components"]["runtime"] = json!("f".repeat(64));
    assert!(consent.authorize(&other, &f).is_err());
}

struct TestFolder(std::path::PathBuf);

impl TestFolder {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("rl-engine-{}-{id}", std::process::id()));
        std::fs::create_dir_all(path.join("data")).unwrap();
        std::fs::write(path.join("data/game.dat"), "prepared game data").unwrap();
        Self(path)
    }
}

impl Drop for TestFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn native_game_file(path: &std::path::Path) {
    // A minimal ARM64 Mach-O header, enough to identify an output without executing it.
    let mut header = vec![0u8; 32];
    header[..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
    header[4..8].copy_from_slice(&0x0100000cu32.to_le_bytes());
    header[12..16].copy_from_slice(&2u32.to_le_bytes());
    std::fs::write(path, header).unwrap();
}

#[test]
fn play_resolves_only_the_recorded_program_and_requires_its_runtime_files() {
    let dir = TestFolder::new();
    let mut g = installed();
    g.path = dir.0.clone();
    native_game_file(&g.path.join("game-bin"));
    g.exe = Some("newer-driver".into());
    assert_eq!(pinned_executable(&g, Target::MacosArm64).unwrap().unwrap(), g.path.join("game-bin").canonicalize().unwrap());
    std::fs::remove_file(g.path.join("data/game.dat")).unwrap();
    assert!(pinned_executable(&g, Target::MacosArm64).is_err());
}

#[test]
fn a_build_script_cannot_be_used_as_a_prepared_native_game() {
    let dir = TestFolder::new();
    let mut g = installed();
    g.path = dir.0.clone();
    std::fs::write(g.path.join("game-bin"), "#!/bin/sh\necho rebuilding\n").unwrap();
    assert!(pinned_executable(&g, Target::MacosArm64).is_err());
}

#[cfg(unix)]
#[test]
fn symlinked_programs_outside_the_installation_are_not_launched() {
    let dir = TestFolder::new();
    let outside = TestFolder::new();
    native_game_file(&outside.0.join("game-bin"));
    std::os::unix::fs::symlink(outside.0.join("game-bin"), dir.0.join("game-bin")).unwrap();
    let mut g = installed();
    g.path = dir.0.clone();
    assert!(pinned_executable(&g, Target::MacosArm64).is_err());
}

#[test]
fn a_prepared_game_for_another_target_does_not_fall_back_to_an_installed_program() {
    let dir = TestFolder::new();
    let mut g = installed();
    g.path = dir.0.clone();
    native_game_file(&g.path.join("game-bin"));
    assert!(pinned_executable(&g, Target::WindowsX64).is_err());
}

#[test]
fn unknown_or_unrelated_fix_does_not_authorize_a_rebuild() {
    let g = installed();
    let mut f = fix();
    f.game.version = "2.0.0".into();
    assert!(FixConsent::for_fix(&g, &f).is_err());
    f = fix();
    f.evidence.clear();
    assert!(FixConsent::for_fix(&g, &f).is_err());
    f = fix();
    f.components = read_build(&g).unwrap().unwrap().components;
    assert!(FixConsent::for_fix(&g, &f).is_err());
}

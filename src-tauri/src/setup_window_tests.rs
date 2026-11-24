//! Easy Setup's window: what's picked in it waits until a setup with it works, and nothing else
//! of the game changes before then.

use super::*;
use std::path::PathBuf;

/// An app with one game, set up from a copy. Each test has its own game
/// id: the picks waiting in windows and the running setups are shared by every test.
struct Fixture(App);

impl Fixture {
    fn new(id: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("rl-setup-window-{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut lib = Library::default();
        let mut g = Installed { id: id.into(), name: "Hopper".into(), path: dir.join(id), tag: "v2".into(), set_up: "v2".into(), played_secs: 60, ..Default::default() };
        g.setup_inputs.insert("game".into(), PathBuf::from("/games/hopper.nup"));
        lib.installed.insert(id.into(), g);
        Self(App {
            dir,
            http: reqwest::Client::new(),
            lib: Mutex::new(lib),
            jobs: Mutex::default(),
            running: Mutex::default(),
            pids: Mutex::default(),
            started: Mutex::default(),
            download_slot: Arc::new(tokio::sync::Semaphore::new(1)),
            moving: Mutex::default(),
            overlay_open: AtomicBool::new(false),
            looked: Mutex::default(),
            headless: AtomicBool::new(false),
        })
    }

    fn game(&self, id: &str) -> Installed {
        self.0.lib.lock().unwrap().installed.get(id).cloned().unwrap()
    }

    fn as_it_was(&self, id: &str) -> bool {
        let g = self.game(id);
        g.setup_inputs.get("game") == Some(&PathBuf::from("/games/hopper.nup")) && g.set_up == "v2"
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0.dir);
    }
}

#[test]
fn a_pick_in_easy_setup_waits_and_closing_the_window_drops_it() {
    let f = Fixture::new("sw-close");
    pend("sw-close", "game", Some(PathBuf::from("/games/other.nup")));
    // The library has what was set up; the window shows what it picked.
    assert!(f.as_it_was("sw-close"));
    let shown = with_pending("sw-close", f.game("sw-close"));
    assert_eq!(shown.setup_inputs["game"], PathBuf::from("/games/other.nup"));
    clear_pending("sw-close");
    assert!(f.as_it_was("sw-close"));
    assert_eq!(with_pending("sw-close", f.game("sw-close")).setup_inputs["game"], PathBuf::from("/games/hopper.nup"));
    // Play time and everything else stay as they are.
    assert_eq!(f.game("sw-close").played_secs, 60);
}

#[test]
fn a_file_cleared_in_easy_setup_is_cleared_only_in_the_window() {
    let f = Fixture::new("sw-clear");
    pend("sw-clear", "game", None);
    assert!(!with_pending("sw-clear", f.game("sw-clear")).setup_inputs.contains_key("game"));
    assert!(f.as_it_was("sw-clear"));
    clear_pending("sw-clear");
}

#[test]
fn picks_waiting_in_one_game_never_show_in_another() {
    let f = Fixture::new("sw-one");
    let _other = Fixture::new("sw-two");
    pend("sw-one", "game", Some(PathBuf::from("/games/other.nup")));
    assert_eq!(with_pending("sw-two", f.game("sw-one")).setup_inputs["game"], PathBuf::from("/games/hopper.nup"));
    clear_pending("sw-one");
}

/// A game that ships its own setup (HopperSetup.exe beside it), set up and playable, whose last
/// output asked for that setup again.
fn own_setup_game(f: &Fixture, id: &str) -> Installed {
    let dir = f.0.dir.join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("hopper.exe"), b"x").unwrap();
    std::fs::write(dir.join("HopperSetup.exe"), b"x").unwrap();
    let g = Installed {
        id: id.into(),
        name: "Hopper".into(),
        path: dir,
        exe: Some(PathBuf::from("hopper.exe")),
        exe_rules: install::EXE_RULES,
        setup: Some(PathBuf::from("HopperSetup.exe")),
        setup_done: true,
        setup_checked: true,
        ..Default::default()
    };
    f.0.lib.lock().unwrap().installed.insert(id.into(), g.clone());
    let out = game_output_path(&f.0.dir, id);
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    std::fs::write(&out, "Run HopperSetup.exe first.\n").unwrap();
    g
}

#[test]
fn a_games_own_setup_asked_for_again_is_never_written_down() {
    let f = Fixture::new("os-asked");
    let g = own_setup_game(&f, "os-asked-game");
    // A shortcut or the page asking leaves it for Play: asked again, as often as they look.
    assert!(own_setup::pending(&f.0, "os-asked-game", &g).is_some());
    assert!(own_setup::pending(&f.0, "os-asked-game", &g).is_some());
    // Play runs it, once for what the game said.
    assert!(own_setup::pending_for_play(&f.0, "os-asked-game", &g).is_some());
    // Closed before it finished: nothing was recorded, so the version that played plays again.
    assert!(f.game("os-asked-game").setup_done);
    assert!(own_setup::pending_for_play(&f.0, "os-asked-game", &g).is_none());
}

/// A game built from the player's copy (its manifest in its folder), set up on release v2.
fn built_game(f: &Fixture, id: &str) -> Installed {
    let dir = f.0.dir.join(id);
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = r#"{"format":1,"game":{"kind":"iso","label":"Your copy"},"prepare":{"program":["builder"],"version":"2","done":["{install}/built.bin"]},"start":{"program":["game"]}}"#;
    std::fs::write(dir.join("recomplauncher.json"), manifest).unwrap();
    std::fs::write(dir.join("built.bin"), b"built").unwrap();
    std::fs::write(dir.join("game"), b"x").unwrap();
    let copy = f.0.dir.join("copy.bin");
    std::fs::write(&copy, b"copy").unwrap();
    let mut g = Installed { id: id.into(), name: "Hopper".into(), path: dir, exe: Some(PathBuf::from("game")), tag: "v2".into(), set_up: "v2".into(), ..Default::default() };
    g.setup_inputs.insert("game".into(), copy);
    g
}

#[test]
fn what_starts_a_game_without_the_launcher_asks_the_same_answer_as_play() {
    let f = Fixture::new("steam-ready");
    let g = built_game(&f, "steam-ready-game");
    assert!(plays_as_set_up(&f.0, "steam-ready-game", &g, None));
    // A new release: built again first, so not ready, with its copy right there.
    let newer = Installed { tag: "v3".into(), ..g.clone() };
    assert!(!plays_as_set_up(&f.0, "steam-ready-game", &newer, None));
    // Built, and its copy gone since: it plays without it.
    std::fs::remove_file(&g.setup_inputs["game"]).unwrap();
    assert!(plays_as_set_up(&f.0, "steam-ready-game", &g, None));
    // Its build gone too: not ready.
    std::fs::remove_file(g.path.join("built.bin")).unwrap();
    assert!(!plays_as_set_up(&f.0, "steam-ready-game", &g, None));
}

#[test]
fn a_copy_picked_on_the_game_page_is_kept_and_sets_the_game_up_again() {
    let f = Fixture::new("page-pick");
    let g = built_game(&f, "page-pick-game");
    f.0.lib.lock().unwrap().installed.insert("page-pick-game".into(), g.clone());
    let other = f.0.dir.join("other.iso");
    std::fs::write(&other, b"another copy").unwrap();
    pick_file(&f.0, "page-pick-game", None, Some(other.display().to_string()), false).unwrap();
    let now = f.game("page-pick-game");
    assert_eq!(now.setup_inputs["game"], other);
    // Built from another copy: Easy Setup builds it again before it plays.
    assert!(now.set_up.is_empty());
    assert!(!plays_as_set_up(&f.0, "page-pick-game", &now, None));
}

#[test]
fn a_copy_picked_in_easy_setup_changes_nothing_until_it_is_set_up() {
    let f = Fixture::new("window-pick");
    let g = built_game(&f, "window-pick-game");
    f.0.lib.lock().unwrap().installed.insert("window-pick-game".into(), g.clone());
    let other = f.0.dir.join("other.iso");
    std::fs::write(&other, b"another copy").unwrap();
    pick_file(&f.0, "window-pick-game", None, Some(other.display().to_string()), true).unwrap();
    let now = f.game("window-pick-game");
    assert_eq!(now.setup_inputs, g.setup_inputs);
    assert_eq!(now.set_up, "v2");
    assert_eq!(with_pending("window-pick-game", now).setup_inputs["game"], other);
    clear_pending("window-pick-game");
}

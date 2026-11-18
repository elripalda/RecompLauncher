//! Easy Setup's window as a transaction: a game is set up as it was before the window opened
//! unless a setup in it worked.

use super::*;
use std::path::PathBuf;

/// An app with one game, set up from the player's own build and a copy. Each test has its own game
/// id: the open windows and the running setups are shared by every test.
struct Fixture(App);

impl Fixture {
    fn new(id: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("rl-setup-window-{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut lib = Library::default();
        let g = Installed {
            id: id.into(),
            name: "Hopper".into(),
            path: dir.join(id),
            game_copy: Some(PathBuf::from("/games/hopper.nup")),
            build_dir: Some(PathBuf::from("/builds/hopper")),
            inputs_ready: "v2:abc".into(),
            prepare_version: "2".into(),
            played_secs: 60,
            ..Default::default()
        };
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

    /// What a window or a run changes before it has worked: a new copy, the player's build let go,
    /// the fingerprint cleared.
    fn change(&self, id: &str) {
        let mut l = self.0.lib.lock().unwrap();
        let x = l.installed.get_mut(id).unwrap();
        x.game_copy = Some(PathBuf::from("/games/other.nup"));
        x.build_dir = None;
        x.inputs_ready.clear();
    }

    fn as_it_was(&self, id: &str) -> bool {
        let g = self.game(id);
        g.game_copy == Some(PathBuf::from("/games/hopper.nup")) && g.build_dir == Some(PathBuf::from("/builds/hopper")) && g.inputs_ready == "v2:abc"
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0.dir);
    }
}

#[test]
fn closing_easy_setup_without_a_setup_that_worked_puts_the_game_back() {
    let f = Fixture::new("sw-close");
    begin_setup(&f.0, "sw-close");
    f.change("sw-close");
    end_setup(&f.0, "sw-close");
    assert!(f.as_it_was("sw-close"));
    assert!(f.game("sw-close").setup_before.is_none());
    // Play time and everything else stay as they are.
    assert_eq!(f.game("sw-close").played_secs, 60);
}

#[test]
fn a_setup_that_worked_keeps_what_it_made() {
    let f = Fixture::new("sw-worked");
    begin_setup(&f.0, "sw-worked");
    f.change("sw-worked");
    // What setup_copy and setup_inputs_run do when they finish.
    f.0.lib.lock().unwrap().installed.get_mut("sw-worked").unwrap().setup_before = None;
    end_setup(&f.0, "sw-worked");
    assert_eq!(f.game("sw-worked").game_copy, Some(PathBuf::from("/games/other.nup")));
}

#[test]
fn a_run_left_in_the_background_puts_the_game_back_when_it_fails() {
    let f = Fixture::new("sw-background");
    begin_setup(&f.0, "sw-background");
    f.change("sw-background");
    // Running: closing the window leaves it to the run.
    SETUP_RUN.lock().unwrap().get_or_insert_with(HashMap::new).insert("sw-background".into(), 1);
    end_setup(&f.0, "sw-background");
    assert!(!f.as_it_was("sw-background"));
    SETUP_RUN.lock().unwrap().as_mut().unwrap().remove("sw-background");
    setup_run_ended(&f.0, "sw-background");
    assert!(f.as_it_was("sw-background"));
}

#[test]
fn a_run_that_fails_with_its_window_open_leaves_the_picks_for_another_try() {
    let f = Fixture::new("sw-retry");
    begin_setup(&f.0, "sw-retry");
    f.change("sw-retry");
    setup_run_ended(&f.0, "sw-retry");
    assert!(!f.as_it_was("sw-retry"));
    end_setup(&f.0, "sw-retry");
    assert!(f.as_it_was("sw-retry"));
}

#[test]
fn a_window_drawn_again_keeps_what_it_kept_when_it_opened() {
    let f = Fixture::new("sw-again");
    begin_setup(&f.0, "sw-again");
    f.change("sw-again");
    begin_setup(&f.0, "sw-again");
    end_setup(&f.0, "sw-again");
    assert!(f.as_it_was("sw-again"));
}

#[test]
fn a_setup_open_when_the_launcher_closed_is_put_back_at_start() {
    let f = Fixture::new("sw-start");
    begin_setup(&f.0, "sw-start");
    f.change("sw-start");
    // The launcher closed: no window is open when it starts again.
    SETUP_WINDOWS.lock().unwrap().retain(|x| x != "sw-start");
    put_back_unfinished_setups(&f.0);
    assert!(f.as_it_was("sw-start"));
}

#[test]
fn a_choice_made_outside_easy_setup_drops_a_leftover() {
    let f = Fixture::new("sw-outside");
    let mut g = f.game("sw-outside");
    g.setup_before = Some(Box::new(library::SetupChoices::of(&g)));
    settle_outside_setup(&mut g);
    assert!(g.setup_before.is_none());
}

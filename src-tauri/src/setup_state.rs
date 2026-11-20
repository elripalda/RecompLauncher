//! Is a game set up, and if not, why: one answer for Play, the game's page and Big Picture.
//!
//! It's worked out from what the player chose and what's on disk, and it never changes the
//! library. A reason to set up again is said here; it's never made by forgetting the player's
//! copy, their own build or the program they picked, so a setup that's cancelled or fails leaves
//! the game as it was.

use crate::library::Installed;
use crate::profile::Profile;
use serde::Serialize;
use std::path::PathBuf;

/// The answer. `reason` is one word the page turns into its button and its line.
///
/// Ready: "ready" (set up with the copy), "built" (its build is here and it starts without the
/// copy), "own_build", "own_program", "by_hand", "skipped".
///
/// Not ready: "copy" (no copy yet; detail: what to pick), "copy_wrong" (detail: why), "files"
/// (named files to choose), "update" (detail: what to pick), "rebuild" (its setup step changed;
/// detail "kept": the copy's gone, and the step runs again from what it kept, prepare.keeps_copy),
/// "build_missing" (detail: the folder that's gone), "own_build_missing" (detail: where its setup
/// builds it), "way" (set up by hand, but it has to be built from the copy first).
///
/// `changed`: it's built again because its setup step changed since it was built (a new
/// `prepare.version`, which comes with an update). Play then says it's finishing the update, not
/// setting the game up from the start.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub ready: bool,
    pub reason: &'static str,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub detail: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub changed: bool,
}

/// What the answer is worked out from. The caller gathers it: reading the library, the settings
/// and the player's copy is its business.
pub struct Facts<'a> {
    /// The game's profile, scoped to one game of several when it's that game's answer.
    pub profile: &'a Profile,
    pub game: &'a Installed,
    /// The named files still match what they were set up with.
    pub inputs_ready: bool,
    /// The copy to use: the one picked, still there, or the game's files already where it reads
    /// them.
    pub copy: Option<PathBuf>,
    /// Why the copy picked can't be used (it's another game, or another version).
    pub copy_wrong: Option<String>,
    /// The program the player picked (Set Game Executable) starts instead of the profile's.
    pub own_program: bool,
    /// Easy Setup is skipped for this game.
    pub skipped: bool,
}

fn ready(reason: &'static str) -> Status {
    Status { ready: true, reason, detail: String::new(), changed: false }
}

fn needs(reason: &'static str, detail: impl Into<String>) -> Status {
    Status { ready: false, reason, detail: detail.into(), changed: false }
}

/// A build needed because the setup step changed since the last one (an update).
fn needs_after_update(reason: &'static str, detail: impl Into<String>) -> Status {
    Status { changed: true, ..needs(reason, detail) }
}

pub fn of(f: &Facts) -> Status {
    let (p, g) = (f.profile, f.game);
    // Built from the copy before it starts: only the launcher's way does that.
    let built_from_copy = p.prepare.is_some() && p.start.is_some();
    if g.hand().is_some() {
        if built_from_copy {
            return needs("way", "");
        }
        return ready("by_hand");
    }
    if f.own_program {
        return ready("own_program");
    }
    let here = g.prebuilt && p.build_folder_here(&g.path).is_some();
    let built = p.build_done(&g.path) == Some(true) || here;
    if f.skipped {
        // Skipped on a game built into a folder of its own: it starts the player's own build.
        let Some(home) = p.build_home(&g.path) else { return ready("skipped") };
        let theirs = g.build_dir.as_ref().is_some_and(|d| d.is_dir()) || here;
        if !theirs {
            return needs("own_build_missing", home.display().to_string());
        }
        return ready("own_build");
    }
    if !p.inputs.is_empty() && !f.inputs_ready {
        return needs("files", "");
    }
    let Some(need) = p.game.as_ref() else { return ready("ready") };
    // Settings only (the game finds its copy itself), or a copy only its own setup takes.
    if p.start.is_none() {
        if need.for_setup && f.copy.is_none() {
            return needs("copy", need.label.clone());
        }
        return ready("ready");
    }
    let uses_copy = p.start_uses_game();
    // Built, and it doesn't take the copy when it starts: the copy isn't needed to play.
    let plays_built = built && !uses_copy;
    let built_reason = if g.build_dir.is_some() { "own_build" } else { "built" };
    let launcher_built = !g.prebuilt && g.build_dir.is_none();
    let changed = launcher_built && p.prepare_version_changed(&g.prepare_version);
    // Its finished files are listed and one isn't there (one that lists none can't tell).
    let unfinished = p.build_done(&g.path) == Some(false);
    match (&f.copy_wrong, &f.copy) {
        (Some(_), _) if plays_built => return ready(built_reason),
        (Some(why), _) => return needs("copy_wrong", why.clone()),
        // The copy's gone, and its setup step kept what it needs: built again from that.
        (None, None) if changed && builds_from_kept(p, g) => return needs_after_update("rebuild", "kept"),
        (None, None) if unfinished && builds_from_kept(p, g) => return needs("rebuild", "kept"),
        (None, None) if plays_built => return ready(built_reason),
        (None, None) => return needs("copy", need.label.clone()),
        (None, Some(_)) => {}
    }
    // Built by the launcher from the copy, but its setup step changed since: built again.
    if changed {
        return needs_after_update("rebuild", "");
    }
    // Built from the copy into a folder of its own, and that folder is gone.
    if let Some(gone) = p.built_folder_missing(&g.path) {
        return needs("build_missing", gone.display().to_string());
    }
    if let Some(up) = need.update.as_ref() {
        let have = g.update_copy.as_ref().is_some_and(|u| u.exists());
        if !have && !up.optional && up.in_place(&g.path).is_none() {
            return needs("update", up.label.clone());
        }
    }
    ready("ready")
}

/// Its setup step can run again without the copy (`prepare.keeps_copy`): the launcher built it,
/// what the step kept is still there, and starting it doesn't take the copy either.
pub fn builds_from_kept(p: &Profile, g: &Installed) -> bool {
    let launcher_built = !g.prebuilt && g.build_dir.is_none();
    launcher_built && !p.start_uses_game() && p.kept_from_copy(&g.path).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn dir(test: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rl-setup-state-{}-{test}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn profile(v: serde_json::Value) -> Profile {
        serde_json::from_value(v).unwrap()
    }

    /// Hookshot's shape (its recomplauncher.json): built from the copy into a folder inside the
    /// game's, started from there.
    fn built_from_copy() -> Profile {
        profile(serde_json::json!({
            "game": { "kind": "iso_or_folder", "label": "Your copy" },
            "prepare": { "program": ["Hookshot.AppImage"], "args": ["--game", "{game}", "--out", "{install}/Hookshot"], "version": "2" },
            "start": { "program": ["Hookshot.AppImage"], "args": ["--play", "{install}/Hookshot"] },
            "folders": { "game": [{ "path": "{install}/Hookshot" }] }
        }))
    }

    fn facts<'a>(p: &'a Profile, g: &'a Installed) -> Facts<'a> {
        Facts { profile: p, game: g, inputs_ready: false, copy: g.game_copy.clone().filter(|c| c.exists()), copy_wrong: None, own_program: false, skipped: false }
    }

    fn set_up(d: &Path) -> Installed {
        let copy = d.join("game.nup");
        fs::write(&copy, b"nup").unwrap();
        fs::create_dir_all(d.join("Hookshot")).unwrap();
        Installed { id: "hookshot".into(), path: d.to_path_buf(), game_copy: Some(copy), prepare_version: "2".into(), ..Default::default() }
    }

    #[test]
    fn a_game_built_from_its_copy_is_ready() {
        let d = dir("ready");
        let g = set_up(&d);
        let p = built_from_copy();
        assert_eq!(of(&facts(&p, &g)), ready("ready"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn no_copy_yet_asks_for_one() {
        let d = dir("no-copy");
        let mut g = set_up(&d);
        g.game_copy = None;
        let _ = fs::remove_dir_all(d.join("Hookshot"));
        let p = built_from_copy();
        assert_eq!(of(&facts(&p, &g)), needs("copy", "Your copy"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_changed_setup_step_asks_to_build_again_and_keeps_the_copy() {
        let d = dir("rebuild");
        let mut g = set_up(&d);
        g.prepare_version = "1".into();
        let p = built_from_copy();
        // (changed: an update's new prepare.version, so Play says it's finishing the update)
        assert_eq!(of(&facts(&p, &g)), needs_after_update("rebuild", ""));
        assert!(g.game_copy.is_some());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_build_folder_that_is_gone_asks_to_build_again() {
        let d = dir("gone");
        let g = set_up(&d);
        fs::remove_dir_all(d.join("Hookshot")).unwrap();
        let p = built_from_copy();
        let s = of(&facts(&p, &g));
        assert_eq!((s.ready, s.reason), (false, "build_missing"));
        assert!(s.detail.ends_with("Hookshot"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_build_that_starts_without_the_copy_is_ready_without_it() {
        let d = dir("built");
        let mut g = set_up(&d);
        g.game_copy = None;
        let p = profile(serde_json::json!({
            "game": { "kind": "iso_or_folder", "label": "Your copy" },
            "prepare": { "program": ["Builder.exe"], "done": [d.join("Hookshot").join("image.bin").display().to_string()] },
            "start": { "program": ["Hookshot/bin/hookshot"] }
        }));
        fs::write(d.join("Hookshot").join("image.bin"), b"built").unwrap();
        assert_eq!(of(&facts(&p, &g)), ready("built"));
        // A wrong copy doesn't stop it either: the copy isn't read when it starts.
        let mut f = facts(&p, &g);
        f.copy_wrong = Some("That's another game.".into());
        assert_eq!(of(&f), ready("built"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_wrong_copy_is_said_and_kept() {
        let d = dir("wrong");
        let g = set_up(&d);
        let p = built_from_copy();
        let mut f = facts(&p, &g);
        f.copy_wrong = Some("That's another game.".into());
        assert_eq!(of(&f), needs("copy_wrong", "That's another game."));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn the_players_own_program_is_ready() {
        let d = dir("own-program");
        let mut g = set_up(&d);
        g.game_copy = None;
        let p = built_from_copy();
        let mut f = facts(&p, &g);
        f.own_program = true;
        assert_eq!(of(&f), ready("own_program"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn skipped_on_a_game_built_into_its_own_folder_needs_the_players_build() {
        let d = dir("skipped");
        let mut g = set_up(&d);
        g.game_copy = None;
        let out = d.join("elsewhere").join("Hookshot").display().to_string();
        let p = profile(serde_json::json!({
            "game": { "kind": "iso_or_folder", "label": "Your copy" },
            "prepare": { "program": ["Builder.exe"], "args": ["--out", out] },
            "start": { "program": ["Builder.exe"], "args": ["--play", out] },
            "folders": { "game": [{ "path": out }] }
        }));
        let mut f = facts(&p, &g);
        f.skipped = true;
        assert_eq!(of(&f).reason, "own_build_missing");
        let mine = d.join("mine");
        fs::create_dir_all(&mine).unwrap();
        g.build_dir = Some(mine);
        let mut f = facts(&p, &g);
        f.skipped = true;
        assert_eq!(of(&f), ready("own_build"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn set_up_by_hand_is_ready_unless_it_is_built_from_the_copy() {
        let d = dir("by-hand");
        let mut g = set_up(&d);
        g.manual = Some(Default::default());
        g.mode = crate::library::SetupMode::ByHand;
        let p = built_from_copy();
        assert_eq!(of(&facts(&p, &g)), needs("way", ""));
        let plain = profile(serde_json::json!({ "game": { "kind": "iso", "label": "Your disc" }, "start": { "program": ["game.exe"], "args": ["{game}"] } }));
        assert_eq!(of(&facts(&plain, &g)), ready("by_hand"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn named_files_not_chosen_yet_are_asked_for() {
        let d = dir("files");
        let g = set_up(&d);
        let p = profile(serde_json::json!({
            "inputs": [{ "id": "base", "label": "Base game", "role": "game", "formats": [{ "type": "file" }] }],
            "start": { "program": ["game.exe"] }
        }));
        assert_eq!(of(&facts(&p, &g)), needs("files", ""));
        let mut f = facts(&p, &g);
        f.inputs_ready = true;
        assert_eq!(of(&f), ready("ready"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_missing_update_is_asked_for_unless_it_is_optional() {
        let d = dir("update");
        let g = set_up(&d);
        let p = |optional: bool| profile(serde_json::json!({
            "game": { "kind": "iso", "label": "Your disc", "update": { "into": "update", "label": "The 1.02 update", "optional": optional } },
            "start": { "program": ["game.exe"], "args": ["{game}"] }
        }));
        let needed = p(false);
        assert_eq!(of(&facts(&needed, &g)), needs("update", "The 1.02 update"));
        let optional = p(true);
        assert_eq!(of(&facts(&optional, &g)), ready("ready"));
        let _ = fs::remove_dir_all(&d);
    }

    /// Hookshot's shape once its builder keeps what it needs from the copy (prepare.keeps_copy):
    /// one --game={game} argument, and the finished game listed in done.
    fn keeps_copy() -> Profile {
        profile(serde_json::json!({
            "game": { "kind": "iso_or_folder", "label": "Your copy" },
            "prepare": { "program": ["Hookshot.AppImage"], "args": ["--game={game}", "--out", "{install}/Hookshot"], "version": "2", "done": ["{install}/Hookshot/hookshot"], "keeps_copy": true },
            "start": { "program": ["Hookshot.AppImage"], "args": ["--play", "{install}/Hookshot"] },
            "folders": { "game": [{ "path": "{install}/Hookshot" }] }
        }))
    }

    /// Set up, built, and the copy deleted since (to save space).
    fn built_copy_gone(d: &Path) -> Installed {
        let mut g = set_up(d);
        fs::write(d.join("Hookshot").join("hookshot"), b"game").unwrap();
        fs::remove_file(g.game_copy.as_ref().unwrap()).unwrap();
        g.game_copy = None;
        g
    }

    #[test]
    fn a_changed_step_that_kept_the_copy_builds_again_without_it() {
        let d = dir("kept-changed");
        let mut g = built_copy_gone(&d);
        g.prepare_version = "1".into();
        assert_eq!(of(&facts(&keeps_copy(), &g)), needs_after_update("rebuild", "kept"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_finished_file_gone_on_a_step_that_kept_the_copy_builds_again_without_it() {
        let d = dir("kept-unfinished");
        let g = built_copy_gone(&d);
        fs::remove_file(d.join("Hookshot").join("hookshot")).unwrap();
        fs::write(d.join("Hookshot").join(".hookshot_exefs"), b"kept").unwrap();
        // (an unfinished build, not an update: Play keeps Easy Setup's Build It Again for it)
        assert_eq!(of(&facts(&keeps_copy(), &g)), needs("rebuild", "kept"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_step_that_kept_the_copy_and_is_built_plays_without_it() {
        let d = dir("kept-built");
        let g = built_copy_gone(&d);
        assert_eq!(of(&facts(&keeps_copy(), &g)), ready("built"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_build_folder_that_is_gone_took_what_it_kept_and_asks_for_the_copy() {
        let d = dir("kept-gone");
        let mut g = built_copy_gone(&d);
        g.prepare_version = "1".into();
        fs::remove_dir_all(d.join("Hookshot")).unwrap();
        assert_eq!(of(&facts(&keeps_copy(), &g)), needs("copy", "Your copy"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn without_keeps_copy_a_changed_step_with_the_copy_gone_plays_as_built() {
        let d = dir("not-kept");
        let mut g = built_copy_gone(&d);
        g.prepare_version = "1".into();
        let mut p = keeps_copy();
        p.prepare.as_mut().unwrap().keeps_copy = false;
        assert_eq!(of(&facts(&p, &g)), ready("built"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_players_own_build_is_never_built_again_from_what_the_step_kept() {
        let d = dir("kept-own");
        let mut g = built_copy_gone(&d);
        g.prepare_version = "1".into();
        g.prebuilt = true;
        assert!(!builds_from_kept(&keeps_copy(), &g));
        let _ = fs::remove_dir_all(&d);
    }
}

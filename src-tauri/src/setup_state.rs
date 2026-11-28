//! Is a game set up, and if not, why: one answer for Play, the game's page and Big Picture.
//!
//! One rule for every file a game asks for: named inputs (a manifest's `game` is read as the input
//! `game`). It's worked out from what the player picked, what's on disk and the release the last
//! setup that worked finished on, and it never changes the library. Files are checked when they're
//! picked, never here: a launcher update that checks more never turns away a game that played.

use crate::inputs::Row;
use crate::library::Installed;
use crate::profile::Profile;
use serde::Serialize;

/// The answer. `reason` is one word the page turns into its button and its line.
///
/// Ready: "ready" (set up), "built" (its build is here and it starts without the player's copy),
/// "own_program", "skipped".
///
/// Not ready: "copy" (the game's main file isn't picked; detail: what to pick), "files" (another
/// file isn't, or the files are picked and Easy Setup hasn't set them up yet; detail: what to
/// pick, when it's one), "rebuild" (a new release, or its build isn't all there; detail "kept": the
/// copy's gone, and the step runs again from what it kept, prepare.keeps_copy), "build_missing"
/// (detail: the folder that's gone).
///
/// `changed`: it's built again because a new release was installed since it was built. Play then
/// says it's finishing the update, not setting the game up from the start.
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
/// and the player's files is its business.
pub struct Facts<'a> {
    /// The game's profile, scoped to one game of several when it's that game's answer.
    pub profile: &'a Profile,
    pub game: &'a Installed,
    /// Its files as Easy Setup lists them (inputs::rows): each one's path when it's picked and
    /// there, or already where the game reads it.
    pub rows: &'a [Row],
    /// The release its last setup that worked finished on: `Installed::set_up`, or one game's own.
    pub set_up: &'a str,
    /// Its last build was cut short (`Installed::build_cut`, or one game's own): not finished,
    /// whatever files it left.
    pub cut: bool,
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

/// Files picked in an Easy Setup window and not set up yet: its Set Up keeps them.
pub fn picked_not_set_up() -> Status {
    needs("files", "")
}

/// A build needed because a new release was installed since the last one.
fn needs_after_update(reason: &'static str, detail: impl Into<String>) -> Status {
    Status { changed: true, ..needs(reason, detail) }
}

/// Whether setting it up does something Play doesn't: a setup step, or a disc to unpack. Only then
/// does the release its setup finished on count; any other game is set up once its files are
/// picked, since Play puts them where the game reads them each time.
pub fn needs_run(p: &Profile) -> bool {
    p.prepare.is_some() || p.inputs.iter().any(|i| !i.extract.is_empty())
}

/// Whether the game reads this file when it starts: its start names it (or `{game}` for the main
/// file), or it's put in the game's folder before each start. A built game needs only these.
pub fn start_needs(p: &Profile, id: &str) -> bool {
    let input = p.inputs.iter().find(|i| i.id == id);
    let placed = input.is_some_and(|i| !i.destination.is_empty() || (!i.into.trim().is_empty() && i.extract.is_empty()));
    let main = p.main_input().is_some_and(|m| m.id == id);
    let token = format!("{{input:{id}}}");
    let names = |a: &String| a.contains(&token) || (main && a.contains("{game}"));
    let in_start = p.start.as_ref().is_some_and(|s| s.args.iter().chain(s.env.values()).any(names));
    let in_games = crate::games::of(p.games.as_ref(), &serde_json::Value::Null).iter().any(|g| g.args.iter().any(names));
    placed || in_start || in_games
}

/// Set up on the installed release, by the launcher or (brought in with its build, `prepare.done`
/// all there) by another launcher.
fn set_up_on_this_release(f: &Facts) -> bool {
    let g = f.game;
    let brought_built = g.files_in_place && f.set_up.is_empty() && f.profile.build_done(&g.path) == Some(true);
    brought_built || (!f.set_up.is_empty() && f.set_up == g.tag)
}

pub fn of(f: &Facts) -> Status {
    let (p, g) = (f.profile, f.game);
    if f.own_program {
        return ready("own_program");
    }
    // Skip Easy Setup: Play runs the program as it is, so nothing is asked for.
    if f.skipped {
        return ready("skipped");
    }
    let main = p.main_input().map(|i| i.id.as_str());
    let missing: Vec<&Row> = f.rows.iter().filter(|r| r.active && !r.optional && !r.runtime && r.path.is_none()).collect();
    // Settings only (the game finds its copy itself): only the files the manifest names as inputs,
    // and a copy only its own setup takes, are asked for.
    if p.start.is_none() {
        let asked = |r: &&&Row| p.inputs.iter().any(|i| i.id == r.id && (!i.lifted || i.for_setup));
        return match missing.iter().find(asked) {
            Some(r) if Some(r.id.as_str()) == main => needs("copy", r.label.clone()),
            Some(r) => needs("files", r.label.clone()),
            None => ready("ready"),
        };
    }
    let run = needs_run(p);
    // A build cut short may have left every file `prepare.done` lists, half written.
    let finished = !f.cut && p.build_done(&g.path) != Some(false);
    let here = run && set_up_on_this_release(f);
    let changed = run && !f.set_up.is_empty() && f.set_up != g.tag;
    // Set up on this release and its build is all there: only the files its start reads are needed.
    let built = here && finished && p.built_folder_missing(&g.path).is_none();
    for r in missing {
        if built && !start_needs(p, &r.id) {
            continue;
        }
        let is_main = Some(r.id.as_str()) == main;
        // The copy's gone, and its setup step kept what it needs: built again from that.
        if is_main && (changed || !finished) && builds_from_kept(p, g) {
            return if changed { needs_after_update("rebuild", "kept") } else { needs("rebuild", "kept") };
        }
        return needs(if is_main { "copy" } else { "files" }, r.label.clone());
    }
    if !run {
        return ready("ready");
    }
    if changed {
        return needs_after_update("rebuild", "");
    }
    // Picked, and Easy Setup hasn't set them up yet: its Set Up does.
    if !here {
        return needs("files", "");
    }
    // Built from the copy into a folder of its own, and that folder is gone.
    if let Some(gone) = p.built_folder_missing(&g.path) {
        return needs("build_missing", gone.display().to_string());
    }
    if !finished {
        // "cut": its last build was stopped partway, which Play and the page say.
        return needs("rebuild", if f.cut { "cut" } else { "" });
    }
    let main_gone = main.is_some_and(|m| f.rows.iter().any(|r| r.id == m && r.path.is_none()));
    ready(if main_gone { "built" } else { "ready" })
}

/// Its setup step can run again without the copy (`prepare.keeps_copy`): what the step kept is
/// still there, and starting it doesn't take the copy either.
pub fn builds_from_kept(p: &Profile, g: &Installed) -> bool {
    !p.start_uses_game() && p.kept_from_copy(&g.path).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn dir(test: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rl-setup-state-{}-{test}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn profile(v: serde_json::Value) -> Profile {
        let mut p: Profile = serde_json::from_value(v).unwrap();
        p.lift_game();
        p
    }

    /// The answer for a game as Play and the page get it: its rows read from what's picked.
    fn answer(p: &Profile, g: &Installed, own_program: bool, skipped: bool) -> Status {
        let rows = crate::inputs::rows(p, g, &g.path).unwrap();
        of(&Facts { profile: p, game: g, rows: &rows, set_up: &g.set_up, cut: g.build_cut, own_program, skipped })
    }

    fn status(p: &Profile, g: &Installed) -> Status {
        answer(p, g, false, false)
    }

    /// Hookshot's shape (its recomplauncher.json): built from the copy into a folder inside the
    /// game's, started from there.
    fn built_from_copy() -> Profile {
        profile(serde_json::json!({
            "game": { "kind": "iso_or_folder", "label": "Your copy" },
            "prepare": { "program": ["Hookshot.AppImage"], "args": ["--game", "{game}", "--out", "{install}/Hookshot"] },
            "start": { "program": ["Hookshot.AppImage"], "args": ["--play", "{install}/Hookshot"] },
            "folders": { "game": [{ "path": "{install}/Hookshot" }] }
        }))
    }

    /// Set up with its copy on release v2.
    fn set_up(d: &Path) -> Installed {
        let copy = d.join("game.nup");
        fs::write(&copy, b"nup").unwrap();
        fs::create_dir_all(d.join("Hookshot")).unwrap();
        let mut g = Installed { id: "hookshot".into(), path: d.to_path_buf(), tag: "v2".into(), set_up: "v2".into(), ..Default::default() };
        g.setup_inputs.insert("game".into(), copy);
        g
    }

    #[test]
    fn a_game_built_from_its_copy_is_ready() {
        let d = dir("ready");
        let g = set_up(&d);
        assert_eq!(status(&built_from_copy(), &g), ready("ready"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn no_copy_yet_asks_for_one() {
        let d = dir("no-copy");
        let mut g = set_up(&d);
        g.setup_inputs.clear();
        g.set_up.clear();
        assert_eq!(status(&built_from_copy(), &g), needs("copy", "Your copy"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn picked_but_not_set_up_yet_asks_for_easy_setup() {
        let d = dir("not-run");
        let mut g = set_up(&d);
        g.set_up.clear();
        assert_eq!(status(&built_from_copy(), &g), needs("files", ""));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_new_release_asks_to_build_again_and_keeps_the_copy() {
        let d = dir("rebuild");
        let mut g = set_up(&d);
        g.tag = "v3".into();
        // (changed: a new release, so Play says it's finishing the update)
        assert_eq!(status(&built_from_copy(), &g), needs_after_update("rebuild", ""));
        assert!(g.setup_inputs.contains_key("game"));
        let _ = fs::remove_dir_all(&d);
    }

    /// Built on this release, then built again (Set Up Again) and stopped partway: the files its
    /// build lists are all there, half written.
    #[test]
    fn a_build_cut_short_on_the_same_release_builds_again() {
        let d = dir("cut");
        let mut g = set_up(&d);
        let mut p = built_from_copy();
        p.prepare.as_mut().unwrap().done = vec!["{install}/Hookshot/image.bin".into()];
        fs::write(d.join("Hookshot").join("image.bin"), b"half").unwrap();
        g.build_cut = true;
        assert_eq!(status(&p, &g), needs("rebuild", "cut"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_build_cut_short_after_an_update_still_finishes_the_update() {
        let d = dir("cut-update");
        let mut g = set_up(&d);
        g.tag = "v3".into();
        g.build_cut = true;
        assert_eq!(status(&built_from_copy(), &g), needs_after_update("rebuild", ""));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_version_the_manifest_still_writes_changes_nothing() {
        let d = dir("version");
        let g = set_up(&d);
        let mut p = built_from_copy();
        p.prepare.as_mut().unwrap().version = "7".into();
        assert_eq!(status(&p, &g), ready("ready"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_build_folder_that_is_gone_asks_to_build_again() {
        let d = dir("gone");
        let g = set_up(&d);
        fs::remove_dir_all(d.join("Hookshot")).unwrap();
        let s = status(&built_from_copy(), &g);
        assert_eq!((s.ready, s.reason), (false, "build_missing"));
        assert!(s.detail.ends_with("Hookshot"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_build_that_starts_without_the_copy_is_ready_without_it() {
        let d = dir("built");
        let mut g = set_up(&d);
        g.setup_inputs.clear();
        let p = profile(serde_json::json!({
            "game": { "kind": "iso_or_folder", "label": "Your copy" },
            "prepare": { "program": ["Builder.exe"], "done": [d.join("Hookshot").join("image.bin").display().to_string()] },
            "start": { "program": ["Hookshot/bin/hookshot"] }
        }));
        fs::write(d.join("Hookshot").join("image.bin"), b"built").unwrap();
        assert_eq!(status(&p, &g), ready("built"));
        // A new release builds it again, and that needs the copy.
        g.tag = "v3".into();
        assert_eq!(status(&p, &g), needs("copy", "Your copy"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_build_brought_in_with_the_game_plays_without_a_setup() {
        let d = dir("brought");
        let mut g = set_up(&d);
        g.setup_inputs.clear();
        g.set_up.clear();
        g.files_in_place = true;
        let p = profile(serde_json::json!({
            "game": { "kind": "folder", "label": "Your copy" },
            "prepare": { "program": ["Builder.exe"], "done": ["{install}/Hookshot/image.bin"] },
            "start": { "program": ["Hookshot/bin/hookshot"] }
        }));
        fs::write(d.join("Hookshot").join("image.bin"), b"built").unwrap();
        assert_eq!(status(&p, &g), ready("built"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_game_without_a_setup_step_is_ready_once_its_copy_is_picked() {
        let d = dir("no-step");
        let mut g = set_up(&d);
        g.set_up.clear();
        let p = profile(serde_json::json!({
            "game": { "kind": "iso", "label": "Your ROM" },
            "start": { "program": ["game.exe"], "args": ["--rom", "{game}"] }
        }));
        assert_eq!(status(&p, &g), ready("ready"));
        g.setup_inputs.clear();
        assert_eq!(status(&p, &g), needs("copy", "Your ROM"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn the_players_own_program_and_skip_easy_setup_are_ready() {
        let d = dir("own-program");
        let mut g = set_up(&d);
        g.setup_inputs.clear();
        let p = built_from_copy();
        assert_eq!(answer(&p, &g, true, false), ready("own_program"));
        assert_eq!(answer(&p, &g, false, true), ready("skipped"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn named_files_not_chosen_yet_are_asked_for() {
        let d = dir("files");
        let mut g = set_up(&d);
        g.setup_inputs.clear();
        let p = profile(serde_json::json!({
            "inputs": [
                { "id": "base", "label": "Base game", "role": "game", "formats": [{ "type": "file" }] },
                { "id": "dlc", "label": "DLC", "role": "dlc", "optional": true, "formats": [{ "type": "file" }] }
            ],
            "start": { "program": ["game.exe"], "args": ["--game={input:base}"] }
        }));
        assert_eq!(status(&p, &g), needs("copy", "Base game"));
        let file = d.join("base.bin");
        fs::write(&file, b"base").unwrap();
        g.setup_inputs.insert("base".into(), file);
        assert_eq!(status(&p, &g), ready("ready"));
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
        assert_eq!(status(&p(false), &g), needs("files", "The 1.02 update"));
        assert_eq!(status(&p(true), &g), ready("ready"));
        // One the player put in its folder counts.
        fs::create_dir_all(d.join("update")).unwrap();
        fs::write(d.join("update").join("tu.bin"), b"tu").unwrap();
        assert_eq!(status(&p(false), &g), ready("ready"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_settings_only_profile_asks_only_for_a_copy_its_own_setup_takes() {
        let d = dir("settings-only");
        let mut g = set_up(&d);
        g.setup_inputs.clear();
        let p = profile(serde_json::json!({ "game": { "kind": "folder", "label": "Your copy" } }));
        assert_eq!(status(&p, &g), ready("ready"));
        let mut p = p;
        p.inputs[0].for_setup = true;
        assert_eq!(status(&p, &g), needs("copy", "Your copy"));
        let _ = fs::remove_dir_all(&d);
    }

    /// Hookshot's shape once its builder keeps what it needs from the copy (prepare.keeps_copy):
    /// one --game={game} argument, and the finished game listed in done.
    fn keeps_copy() -> Profile {
        profile(serde_json::json!({
            "game": { "kind": "iso_or_folder", "label": "Your copy" },
            "prepare": { "program": ["Hookshot.AppImage"], "args": ["--game={game}", "--out", "{install}/Hookshot"], "done": ["{install}/Hookshot/hookshot"], "keeps_copy": true },
            "start": { "program": ["Hookshot.AppImage"], "args": ["--play", "{install}/Hookshot"] },
            "folders": { "game": [{ "path": "{install}/Hookshot" }] }
        }))
    }

    /// Set up, built, and the copy deleted since (to save space).
    fn built_copy_gone(d: &Path) -> Installed {
        let mut g = set_up(d);
        fs::write(d.join("Hookshot").join("hookshot"), b"game").unwrap();
        fs::remove_file(&g.setup_inputs["game"]).unwrap();
        g.setup_inputs.clear();
        g
    }

    #[test]
    fn a_new_release_that_kept_the_copy_builds_again_without_it() {
        let d = dir("kept-changed");
        let mut g = built_copy_gone(&d);
        g.tag = "v3".into();
        assert_eq!(status(&keeps_copy(), &g), needs_after_update("rebuild", "kept"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_finished_file_gone_on_a_step_that_kept_the_copy_builds_again_without_it() {
        let d = dir("kept-unfinished");
        let g = built_copy_gone(&d);
        fs::remove_file(d.join("Hookshot").join("hookshot")).unwrap();
        fs::write(d.join("Hookshot").join(".hookshot_exefs"), b"kept").unwrap();
        // (an unfinished build, not an update: Play keeps Easy Setup's Build It Again for it)
        assert_eq!(status(&keeps_copy(), &g), needs("rebuild", "kept"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_step_that_kept_the_copy_and_is_built_plays_without_it() {
        let d = dir("kept-built");
        let g = built_copy_gone(&d);
        assert_eq!(status(&keeps_copy(), &g), ready("built"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_build_folder_that_is_gone_took_what_it_kept_and_asks_for_the_copy() {
        let d = dir("kept-gone");
        let mut g = built_copy_gone(&d);
        g.tag = "v3".into();
        fs::remove_dir_all(d.join("Hookshot")).unwrap();
        assert_eq!(status(&keeps_copy(), &g), needs("copy", "Your copy"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn without_keeps_copy_a_new_release_with_the_copy_gone_asks_for_it() {
        let d = dir("not-kept");
        let mut g = built_copy_gone(&d);
        g.tag = "v3".into();
        let mut p = keeps_copy();
        p.prepare.as_mut().unwrap().keeps_copy = false;
        assert_eq!(status(&p, &g), needs("copy", "Your copy"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn changed_is_sent_to_the_page_only_when_an_update_did_it() {
        let unfinished = serde_json::to_value(needs("rebuild", "kept")).unwrap();
        assert!(unfinished.get("changed").is_none());
        let update = serde_json::to_value(needs_after_update("rebuild", "kept")).unwrap();
        assert_eq!(update["changed"], true);
        assert_eq!(update["reason"], "rebuild");
    }
}

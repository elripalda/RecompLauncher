//! Discord status: "Playing <game>" with the game's cover and the time played, while a game
//! runs, and "Playing RecompLauncher" while the launcher's window is open without one (not while
//! it waits in the tray). Talks to the Discord app on this PC; nothing happens when it isn't open.
//!
//! It needs RecompLauncher's Discord application id: the catalog's top-level `"discord"`, or
//! the one built in.

#[cfg(desktop)]
use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
#[cfg(desktop)]
use std::sync::mpsc::{channel, Sender};
#[cfg(desktop)]
use std::sync::{Mutex, OnceLock};

/// RecompLauncher's application on discord.com/developers (empty until it's made). The catalog's
/// top-level `"discord"` overrides it.
pub const APP_ID: &str = "1565871388504367154";
/// The launcher's logo, shown small in the corner of the game's cover.
#[cfg(desktop)]
const LOGO: &str = "https://recomplauncher.com/assets/logo-512.png";

pub enum Msg {
    Playing { app_id: String, game: String, recomp: String, cover: String, start: u64 },
    /// The game's own line (Recomp Achievements' rich presence) in place of "via RecompLauncher".
    State(String),
    Clear,
    /// The launcher is open (app id, since when): "RecompLauncher" shows while no game is running.
    /// None when the Discord status is turned off.
    Idle(Option<(String, u64)>),
    /// The launcher's window came into sight (true) or went to the tray (false). Hidden, the
    /// launcher's line goes; a game's status stays either way.
    Shown(bool),
}

#[cfg(desktop)]
static TX: OnceLock<Mutex<Sender<Msg>>> = OnceLock::new();

/// What Discord is shown: a game, or the launcher itself.
#[cfg(desktop)]
enum Show<'a> {
    Game { game: &'a str, recomp: &'a str, cover: &'a str, start: u64, line: &'a str },
    Launcher { start: u64 },
}

#[cfg(desktop)]
fn put(client: &mut Option<(String, DiscordIpcClient)>, app_id: &str, show: Show) {
    if app_id.is_empty() {
        return;
    }
    if client.as_ref().map(|c| c.0 != app_id).unwrap_or(true) {
        if let Some((_, mut c)) = client.take() {
            let _ = c.close();
        }
        let mut c = DiscordIpcClient::new(app_id);
        if c.connect().is_err() {
            return;
        }
        *client = Some((app_id.to_string(), c));
    }
    let Some((_, c)) = client.as_mut() else { return };
    let make = || match &show {
        // "Playing <game>" in the member list and on the profile (the game's name shown in place of
        // the app's), its cover, the time played, and a button to the launcher.
        Show::Game { game, recomp, cover, start, line } => {
            let (game, recomp, cover, line) = (*game, *recomp, *cover, *line);
            // No cover (a game the player added): the launcher's logo instead.
            let assets = if cover.starts_with("https://") {
                activity::Assets::new().large_image(cover).large_text(recomp).small_image(LOGO).small_text("RecompLauncher")
            } else {
                activity::Assets::new().large_image(LOGO).large_text(recomp)
            };
            activity::Activity::new()
                .name(game)
                .activity_type(activity::ActivityType::Playing)
                .status_display_type(activity::StatusDisplayType::Details)
                .details(game)
                .state(if line.is_empty() { "via RecompLauncher" } else { line })
                .timestamps(activity::Timestamps::new().start(*start as i64 * 1000))
                .assets(assets)
                .buttons(vec![activity::Button::new("Get RecompLauncher", "https://recomplauncher.com")])
        }
        // The launcher open, no game: "Playing RecompLauncher" with its logo.
        Show::Launcher { start } => activity::Activity::new()
            .activity_type(activity::ActivityType::Playing)
            .details("In the launcher")
            .timestamps(activity::Timestamps::new().start(*start as i64 * 1000))
            .assets(activity::Assets::new().large_image(LOGO).large_text("RecompLauncher"))
            .buttons(vec![activity::Button::new("Get RecompLauncher", "https://recomplauncher.com")]),
    };
    if c.set_activity(make()).is_err() {
        // Discord was closed and opened again: one more try.
        if c.reconnect().is_ok() {
            let _ = c.set_activity(make());
        }
    }
}

#[cfg(desktop)]
fn tx() -> &'static Mutex<Sender<Msg>> {
    TX.get_or_init(|| {
        let (tx, rx) = channel::<Msg>();
        std::thread::spawn(move || {
            let mut client: Option<(String, DiscordIpcClient)> = None;
            // What's showing, to show again with a new state line.
            let mut last: Option<(String, String, String, String, u64)> = None;
            let mut state_line = String::new();
            // The launcher open with the status on: what shows when no game runs.
            let mut idle: Option<(String, u64)> = None;
            // The window starts hidden (it shows once its page is drawn, or never when a shortcut
            // only starts a game), so the launcher's line waits for it.
            let mut shown = false;
            for m in rx {
                match m {
                    Msg::State(text) => {
                        if let Some((app_id, game, recomp, cover, start)) = &last {
                            if text != state_line {
                                state_line = text;
                                put(&mut client, app_id, Show::Game { game, recomp, cover, start: *start, line: &state_line });
                            }
                        }
                    }
                    Msg::Playing { app_id, game, recomp, cover, start } => {
                        if last.as_ref().map(|l| l.1 != game).unwrap_or(true) {
                            state_line.clear();
                        }
                        put(&mut client, &app_id, Show::Game { game: &game, recomp: &recomp, cover: &cover, start, line: &state_line });
                        last = Some((app_id, game, recomp, cover, start));
                    }
                    Msg::Clear => {
                        last = None;
                        state_line.clear();
                        show_idle(&mut client, idle_status(false, idle.as_ref(), shown));
                    }
                    Msg::Idle(on) => {
                        idle = on;
                        show_idle(&mut client, idle_status(last.is_some(), idle.as_ref(), shown));
                    }
                    Msg::Shown(now) => {
                        // Focusing the window says "shown" again and again; Discord only takes a
                        // few updates a minute, so only a real change goes out.
                        if now == shown {
                            continue;
                        }
                        shown = now;
                        show_idle(&mut client, idle_status(last.is_some(), idle.as_ref(), shown));
                    }
                }
            }
        });
        Mutex::new(tx)
    })
}

/// What the status shows after the launcher's own line changed (turned on or off, the window
/// hidden or shown, a game closed).
#[cfg_attr(mobile, allow(dead_code))]
#[derive(Debug, PartialEq)]
enum Idle<'a> {
    /// A game is showing: it stays as it is.
    Game,
    /// "In the launcher" (app id, since when).
    Launcher(&'a str, u64),
    /// Nothing at all.
    Nothing,
}

/// "In the launcher" only while the player can see it: closed to the tray, the launcher is just
/// waiting by the clock, and saying they're in it would be wrong.
#[cfg_attr(mobile, allow(dead_code))]
fn idle_status(playing: bool, idle: Option<&(String, u64)>, shown: bool) -> Idle<'_> {
    if playing {
        return Idle::Game;
    }
    let Some((app_id, start)) = idle else {
        return Idle::Nothing;
    };
    if !shown {
        return Idle::Nothing;
    }
    Idle::Launcher(app_id, *start)
}

#[cfg(desktop)]
fn show_idle(client: &mut Option<(String, DiscordIpcClient)>, what: Idle) {
    match what {
        Idle::Game => {}
        Idle::Launcher(app_id, start) => put(client, app_id, Show::Launcher { start }),
        Idle::Nothing => {
            if let Some((_, c)) = client.as_mut() {
                let _ = c.clear_activity();
            }
        }
    }
}

/// There's no Discord app for the launcher to talk to on a phone.
#[cfg(mobile)]
pub fn send(_m: Msg) {}

#[cfg(desktop)]
pub fn send(m: Msg) {
    let _ = tx().lock().unwrap().send(m);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on() -> Option<(String, u64)> {
        Some(("123".to_string(), 1000))
    }

    #[test]
    fn launcher_shows_while_its_window_is_in_sight() {
        assert_eq!(idle_status(false, on().as_ref(), true), Idle::Launcher("123", 1000));
    }

    #[test]
    fn launcher_goes_while_its_window_is_in_the_tray() {
        assert_eq!(idle_status(false, on().as_ref(), false), Idle::Nothing);
    }

    #[test]
    fn a_game_stays_when_the_window_goes_to_the_tray() {
        assert_eq!(idle_status(true, on().as_ref(), false), Idle::Game);
    }

    #[test]
    fn nothing_shows_with_the_status_off() {
        assert_eq!(idle_status(false, None, true), Idle::Nothing);
    }
}

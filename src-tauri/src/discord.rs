//! Discord status: "Playing <game>" with the game's cover and the time played, while a game
//! runs, and "Playing RecompLauncher" while the launcher is open without one. Talks to the
//! Discord app on this PC; nothing happens when it isn't open.
//!
//! It needs RecompLauncher's Discord application id: the catalog's top-level `"discord"`, or
//! the one built in.

use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};

/// RecompLauncher's application on discord.com/developers (empty until it's made). The catalog's
/// top-level `"discord"` overrides it.
pub const APP_ID: &str = "1565871388504367154";
/// The launcher's logo, shown small in the corner of the game's cover.
const LOGO: &str = "https://recomplauncher.com/assets/logo-512.png";

pub enum Msg {
    Playing { app_id: String, game: String, recomp: String, cover: String, start: u64 },
    /// The game's own line (Recomp Achievements' rich presence) in place of "via RecompLauncher".
    State(String),
    Clear,
    /// The launcher is open (app id, since when): "RecompLauncher" shows while no game is running.
    /// None when the Discord status is turned off.
    Idle(Option<(String, u64)>),
}

static TX: OnceLock<Mutex<Sender<Msg>>> = OnceLock::new();

/// What Discord is shown: a game, or the launcher itself.
enum Show<'a> {
    Game { game: &'a str, recomp: &'a str, cover: &'a str, start: u64, line: &'a str },
    Launcher { start: u64 },
}

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
                        match &idle {
                            Some((app_id, start)) => put(&mut client, app_id, Show::Launcher { start: *start }),
                            None => {
                                if let Some((_, c)) = client.as_mut() {
                                    let _ = c.clear_activity();
                                }
                            }
                        }
                    }
                    Msg::Idle(on) => {
                        idle = on;
                        if last.is_none() {
                            match &idle {
                                Some((app_id, start)) => put(&mut client, app_id, Show::Launcher { start: *start }),
                                None => {
                                    if let Some((_, c)) = client.as_mut() {
                                        let _ = c.clear_activity();
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });
        Mutex::new(tx)
    })
}

pub fn send(m: Msg) {
    let _ = tx().lock().unwrap().send(m);
}

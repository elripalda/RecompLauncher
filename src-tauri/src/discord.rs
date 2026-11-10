//! Discord status: "Playing NBA Street Vol. 2" with the game's cover and the time played, while a
//! game runs. Talks to the Discord app on this PC; nothing happens when it isn't open.
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
}

static TX: OnceLock<Mutex<Sender<Msg>>> = OnceLock::new();

fn tx() -> &'static Mutex<Sender<Msg>> {
    TX.get_or_init(|| {
        let (tx, rx) = channel::<Msg>();
        std::thread::spawn(move || {
            let mut client: Option<(String, DiscordIpcClient)> = None;
            // What's showing, to show again with a new state line.
            let mut last: Option<(String, String, String, String, u64)> = None;
            let mut state_line = String::new();
            for m in rx {
                let m = match m {
                    Msg::State(text) => match &last {
                        Some((app_id, game, recomp, cover, start)) if text != state_line => {
                            state_line = text;
                            Msg::Playing { app_id: app_id.clone(), game: game.clone(), recomp: recomp.clone(), cover: cover.clone(), start: *start }
                        }
                        _ => continue,
                    },
                    Msg::Playing { app_id, game, recomp, cover, start } => {
                        if last.as_ref().map(|l| l.1 != game).unwrap_or(true) {
                            state_line.clear();
                        }
                        last = Some((app_id.clone(), game.clone(), recomp.clone(), cover.clone(), start));
                        Msg::Playing { app_id, game, recomp, cover, start }
                    }
                    Msg::Clear => {
                        last = None;
                        state_line.clear();
                        Msg::Clear
                    }
                };
                match m {
                    Msg::State(_) => {}
                    Msg::Playing { app_id, game, recomp, cover, start } => {
                        if app_id.is_empty() {
                            continue;
                        }
                        if client.as_ref().map(|c| c.0 != app_id).unwrap_or(true) {
                            if let Some((_, mut c)) = client.take() {
                                let _ = c.close();
                            }
                            let mut c = DiscordIpcClient::new(&app_id);
                            if c.connect().is_err() {
                                continue;
                            }
                            client = Some((app_id.clone(), c));
                        }
                        let Some((_, c)) = client.as_mut() else { continue };
                        // "Playing <game>" in the member list and on the profile (the game's name
                        // shown in place of the app's), its cover, the time played, and a button to
                        // the launcher.
                        let make = || {
                            // No cover (a game the player added): the launcher's logo instead.
                            let assets = if cover.starts_with("https://") {
                                activity::Assets::new().large_image(cover.as_str()).large_text(recomp.as_str()).small_image(LOGO).small_text("RecompLauncher")
                            } else {
                                activity::Assets::new().large_image(LOGO).large_text(recomp.as_str())
                            };
                            activity::Activity::new()
                                .name(game.as_str())
                                .activity_type(activity::ActivityType::Playing)
                                .status_display_type(activity::StatusDisplayType::Details)
                                .details(game.as_str())
                                .state(if state_line.is_empty() { "via RecompLauncher" } else { state_line.as_str() })
                                .timestamps(activity::Timestamps::new().start(start as i64 * 1000))
                                .assets(assets)
                                .buttons(vec![activity::Button::new("Get RecompLauncher", "https://recomplauncher.com")])
                        };
                        if c.set_activity(make()).is_err() {
                            // Discord was closed and opened again: one more try.
                            if c.reconnect().is_ok() {
                                let _ = c.set_activity(make());
                            }
                        }
                    }
                    Msg::Clear => {
                        if let Some((_, c)) = client.as_mut() {
                            let _ = c.clear_activity();
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

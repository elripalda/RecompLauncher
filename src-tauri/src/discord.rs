//! Discord status: "Playing NBA Street Vol. 2" with the game's cover and the time played, while a
//! game runs. Talks to the Discord app on this PC; nothing happens when it isn't open.
//!
//! It needs RecompLauncher's Discord application id: the catalog's top-level `"discord"`, or
//! the one built in.

use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};

/// RecompLauncher's application on discord.com/developers (empty until it's made).
pub const APP_ID: &str = "";

pub enum Msg {
    Playing { app_id: String, game: String, recomp: String, cover: String, start: u64 },
    Clear,
}

static TX: OnceLock<Mutex<Sender<Msg>>> = OnceLock::new();

fn tx() -> &'static Mutex<Sender<Msg>> {
    TX.get_or_init(|| {
        let (tx, rx) = channel::<Msg>();
        std::thread::spawn(move || {
            let mut client: Option<(String, DiscordIpcClient)> = None;
            for m in rx {
                match m {
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
                        let mut assets = activity::Assets::new().large_text(recomp.as_str());
                        if cover.starts_with("https://") {
                            assets = assets.large_image(cover.as_str());
                        }
                        let a = activity::Activity::new()
                            .details(game.as_str())
                            .state("via RecompLauncher")
                            .timestamps(activity::Timestamps::new().start(start as i64 * 1000))
                            .assets(assets);
                        if c.set_activity(a).is_err() {
                            // Discord was closed and opened again: one more try.
                            if c.reconnect().is_ok() {
                                let _ = c.set_activity(activity::Activity::new().details(game.as_str()).state("via RecompLauncher").timestamps(activity::Timestamps::new().start(start as i64 * 1000)));
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

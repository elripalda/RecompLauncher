//! When a window's web engine (WebView2) dies. The main window used to stay one flat colour, its
//! background, until the launcher was quit from the tray. The usual cause is a program that draws
//! over games hooking into the engine: RivaTuner Statistics Server crashed it a few seconds after
//! the launcher started, more than once.
//!
//! A page that died is loaded again. An engine that died takes its windows with it, so the main
//! window is made again; the launcher itself (downloads, a game being played) goes on as it was.
//! Only a few times, though: an engine that dies again straight away gets a message, not a loop.
//!
//! Only WebView2 (Windows) tells the launcher when its engine dies. The rest is built everywhere
//! all the same, so its tests run on every system.
#![cfg_attr(not(windows), allow(dead_code))]

#[cfg(mobile)]
#[allow(unused_imports)]
use crate::mobile::*;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use crate::trail;

/// What died.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lost {
    /// The page's own process: the engine is fine, and the page can be loaded again.
    Page,
    /// The engine itself: every window it drew is gone with it.
    Engine,
}

#[derive(Debug, PartialEq, Eq)]
enum Act {
    Recover,
    GiveUp,
}

/// Two recoveries closer than this: the engine dies again as soon as it's back, so no more tries.
const TOO_SOON: Duration = Duration::from_secs(120);
/// At most this many recoveries in one run of the launcher.
const MOST: usize = 3;

/// When the main window was recovered in this run.
static RECOVERED: Mutex<Vec<Instant>> = Mutex::new(Vec::new());
/// The main window is being made again, or was given up on.
static MAIN_GONE: AtomicBool = AtomicBool::new(false);
/// The main window was open when its engine died during play, and came back minimized so as not
/// to cover the game. It opens again when the game ends.
static MINIMIZED_FOR_PLAY: AtomicBool = AtomicBool::new(false);

fn next(past: &[Instant], now: Instant) -> Act {
    if past.len() >= MOST {
        return Act::GiveUp;
    }
    let Some(last) = past.last() else { return Act::Recover };
    if now.duration_since(*last) < TOO_SOON {
        return Act::GiveUp;
    }
    Act::Recover
}

pub fn set_main_gone(gone: bool) {
    MAIN_GONE.store(gone, Ordering::Relaxed);
}

/// The main window is gone because its engine died (not because the player closed it). The
/// launcher mustn't close for having no window then: the tray, or starting it again, brings the
/// window back.
pub fn main_gone() -> bool {
    MAIN_GONE.load(Ordering::Relaxed)
}

pub fn set_minimized_for_play() {
    MINIMIZED_FOR_PLAY.store(true, Ordering::Relaxed);
}

/// The game ended: was the main window minimized only because it was made again during play?
pub fn take_minimized_for_play() -> bool {
    MINIMIZED_FOR_PLAY.swap(false, Ordering::Relaxed)
}

/// Starts watching a window's engine.
#[cfg(windows)]
pub fn watch(w: &tauri::WebviewWindow) {
    let app = w.app_handle().clone();
    let label = w.label().to_string();
    let _ = w.with_webview(move |wv| unsafe {
        use webview2_com::Microsoft::Web::WebView2::Win32::*;
        use webview2_com::ProcessFailedEventHandler;
        let Ok(core) = wv.controller().CoreWebView2() else { return };
        let window = label.clone();
        let handler = ProcessFailedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
            args.ProcessFailedKind(&mut kind)?;
            let lost = match kind {
                COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED => Lost::Engine,
                COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED => Lost::Page,
                // A page that's slow to answer may come back by itself, and the engine starts its
                // other helpers (drawing, a frame, the network) again on its own.
                _ => return Ok(()),
            };
            let why = why_it_stopped(&args);
            let (app, label) = (app.clone(), window.clone());
            // Not from inside the engine's own call: letting go of the window there would free the
            // engine while it's still calling us.
            tauri::async_runtime::spawn(async move { recover(app, label, lost, why).await });
            Ok(())
        }));
        let mut token = 0i64;
        if let Err(e) = core.add_ProcessFailed(&handler, &mut token) {
            trail::note(format!("Window: can't watch the {label} window's engine ({e})"));
        }
    });
}

#[cfg(not(windows))]
pub fn watch(_w: &tauri::WebviewWindow) {}

/// The engine's reason, in words, with the exit code that tells a crash apart.
#[cfg(windows)]
fn why_it_stopped(args: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2ProcessFailedEventArgs) -> String {
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    use windows_core::Interface;
    let Ok(more) = args.cast::<ICoreWebView2ProcessFailedEventArgs2>() else { return "no reason given".into() };
    let mut reason = COREWEBVIEW2_PROCESS_FAILED_REASON::default();
    let mut code = 0i32;
    // SAFETY: plain reads into the two locals, from the engine's own event.
    unsafe {
        let _ = more.Reason(&mut reason);
        let _ = more.ExitCode(&mut code);
    }
    let words = match reason {
        COREWEBVIEW2_PROCESS_FAILED_REASON_CRASHED => "crashed",
        COREWEBVIEW2_PROCESS_FAILED_REASON_TERMINATED => "ended by another program",
        COREWEBVIEW2_PROCESS_FAILED_REASON_OUT_OF_MEMORY => "out of memory",
        COREWEBVIEW2_PROCESS_FAILED_REASON_LAUNCH_FAILED => "didn't start",
        COREWEBVIEW2_PROCESS_FAILED_REASON_PROFILE_DELETED => "its data folder was deleted",
        COREWEBVIEW2_PROCESS_FAILED_REASON_UNRESPONSIVE => "stopped answering",
        _ => "stopped",
    };
    format!("{words}, exit code {:#010x}", code as u32)
}

/// RivaTuner, named in the log and the message when it's running: it's the known cause.
fn rivatuner_running() -> bool {
    #[cfg(windows)]
    return crate::win::is_running("RTSS.exe");
    #[cfg(not(windows))]
    false
}

async fn recover(app: AppHandle, label: String, lost: Lost, why: String) {
    let Some(w) = app.get_webview_window(&label) else { return };
    let what = match lost {
        Lost::Page => "page",
        Lost::Engine => "web engine",
    };
    let rtss = if rivatuner_running() { "; RivaTuner Statistics Server is running, a known cause" } else { "" };
    // The quick menu and the achievement toast are made the next time they're needed.
    if label != "main" {
        trail::note(format!("Window {label}: its {what} stopped ({why}{rtss}); made again when it's next needed"));
        if label == "overlay" {
            app.state::<crate::App>().overlay_open.store(false, Ordering::Relaxed);
        }
        let _ = w.destroy();
        return;
    }
    let act = {
        let mut past = RECOVERED.lock().unwrap();
        let act = next(&past, Instant::now());
        if act == Act::Recover {
            past.push(Instant::now());
        }
        act
    };
    if act == Act::GiveUp {
        trail::note(format!("Window: its {what} stopped again ({why}{rtss}); not opened again until the player asks"));
        set_main_gone(true);
        let _ = w.destroy();
        tell_player(&app);
        return;
    }
    match lost {
        Lost::Page => {
            trail::note(format!("Window: its page stopped ({why}{rtss}); loaded again"));
            let _ = w.reload();
        }
        Lost::Engine => {
            trail::note(format!("Window: its web engine stopped ({why}{rtss}); window made again"));
            crate::remake_main(&app, shown(&w)).await;
        }
    }
}

/// How the dead window was showing, for the one made in its place.
fn shown(w: &tauri::WebviewWindow) -> crate::Shown {
    if !w.is_visible().unwrap_or(false) {
        return crate::Shown::Hidden;
    }
    if w.is_minimized().unwrap_or(false) {
        return crate::Shown::Minimized;
    }
    crate::Shown::Open
}

/// Why the window went away, and how to bring it back. The engine is dead, so a plain Windows
/// message, not the launcher's own.
fn tell_player(app: &AppHandle) {
    let cause = if rivatuner_running() {
        "RivaTuner Statistics Server (it comes with MSI Afterburner) is running, and it's the known cause: it hooks into the window's web engine and crashes it. In RivaTuner, add msedgewebview2.exe and set its Application detection level to None."
    } else {
        "This usually comes from a program that draws over games hooking into the window's web engine, like RivaTuner Statistics Server (MSI Afterburner). In RivaTuner, add msedgewebview2.exe and set its Application detection level to None."
    };
    let back = if app.tray_by_id("main").is_some() { "Open RecompLauncher from its tray icon to try again." } else { "Start RecompLauncher again to try again." };
    let text = format!("RecompLauncher's window kept stopping, so it wasn't opened again. Downloads and games keep going.\n\n{cause}\n\n{back}");
    #[cfg(windows)]
    std::thread::spawn(move || {
        use windows::core::{HSTRING, PCWSTR};
        use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONWARNING, MB_OK, MB_SETFOREGROUND};
        let (text, caption) = (HSTRING::from(text), HSTRING::from("RecompLauncher"));
        // SAFETY: both strings end with a zero and live until the call returns.
        unsafe { MessageBoxW(None, PCWSTR(text.as_ptr()), PCWSTR(caption.as_ptr()), MB_OK | MB_ICONWARNING | MB_SETFOREGROUND) };
    });
    #[cfg(not(windows))]
    let _ = text;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_crash_is_recovered() {
        assert_eq!(next(&[], Instant::now()), Act::Recover);
    }

    #[test]
    fn a_crash_soon_after_a_recovery_gives_up() {
        let now = Instant::now();
        let past = [now - Duration::from_secs(15)];
        assert_eq!(next(&past, now), Act::GiveUp);
    }

    #[test]
    fn a_crash_long_after_a_recovery_is_recovered() {
        let now = Instant::now();
        let past = [now - Duration::from_secs(600)];
        assert_eq!(next(&past, now), Act::Recover);
    }

    #[test]
    fn three_recoveries_in_one_run_are_the_most() {
        let now = Instant::now();
        let past = [now - Duration::from_secs(3000), now - Duration::from_secs(2000), now - Duration::from_secs(1000)];
        assert_eq!(next(&past, now), Act::GiveUp);
    }
}

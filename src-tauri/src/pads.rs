//! Controllers: Xbox (and anything else Windows sees as an Xbox pad) through XInput, PlayStation
//! controllers (DualSense, DualSense Edge, DualShock 4) read directly over USB or Bluetooth.
//! Both are read even while a game has focus, so the in-game combos work:
//! View + RB (Create + R1) takes a screenshot, holding View (Create) opens the quick menu. Otherwise buttons go to the launcher's window as `pad` events:
//! {"b": a | b | x | y | lb | rb | view | menu | guide | up | down | left | right, "kind": xbox | ps}.

use crate::{ingame, App};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Debug, Clone, Serialize)]
pub struct Pad {
    pub name: String,
    pub kind: &'static str,
}

/// What's plugged in, for Settings.
pub static CONNECTED: Mutex<Vec<Pad>> = Mutex::new(Vec::new());

enum Ev {
    Down(&'static str, &'static str),
    Up(&'static str),
}

// ---------- PlayStation reports ----------

pub const SONY: u16 = 0x054C;
pub const DUALSENSE: [u16; 2] = [0x0CE6, 0x0DF2];
pub const DUALSHOCK4: [u16; 3] = [0x05C4, 0x09CC, 0x0BA0];

/// Buttons held in one input report, as names, plus the left stick.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SonyState {
    pub held: Vec<&'static str>,
    pub lx: u8,
    pub ly: u8,
}

/// Read one input report. `dualsense` tells the two layouts apart over USB.
pub fn parse_sony(r: &[u8], dualsense: bool) -> Option<SonyState> {
    // Where the first button byte is, for each kind of report.
    let (sticks, b0) = match (r.first()?, r.len()) {
        (0x01, n) if dualsense && n >= 11 && n > 20 => (1, 8), // DualSense over USB
        (0x31, n) if n >= 12 => (2, 9),                       // DualSense over Bluetooth
        (0x11, n) if n >= 10 => (3, 7),                       // DualShock 4 over Bluetooth
        (0x01, n) if n >= 8 => (1, 5),                        // DualShock 4 over USB, or a DualSense's short Bluetooth report
        _ => return None,
    };
    let (btn0, btn1, btn2) = (*r.get(b0)?, *r.get(b0 + 1)?, *r.get(b0 + 2)?);
    let mut held = vec![];
    match btn0 & 0x0F {
        0 => held.push("up"),
        1 => held.extend(["up", "right"]),
        2 => held.push("right"),
        3 => held.extend(["down", "right"]),
        4 => held.push("down"),
        5 => held.extend(["down", "left"]),
        6 => held.push("left"),
        7 => held.extend(["up", "left"]),
        _ => {}
    }
    for (bit, name) in [(0x10, "x"), (0x20, "a"), (0x40, "b"), (0x80, "y")] {
        if btn0 & bit != 0 {
            held.push(name);
        }
    }
    for (bit, name) in [(0x01, "lb"), (0x02, "rb"), (0x10, "view"), (0x20, "menu")] {
        if btn1 & bit != 0 {
            held.push(name);
        }
    }
    if btn2 & 0x01 != 0 {
        held.push("guide");
    }
    Some(SonyState { held, lx: *r.get(sticks)?, ly: *r.get(sticks + 1)? })
}

/// The stick, pushed past half way, as a direction.
#[cfg_attr(not(windows), allow(dead_code))]
fn stick_dirs(lx: u8, ly: u8) -> Vec<&'static str> {
    let mut v = vec![];
    if lx < 50 {
        v.push("left");
    } else if lx > 205 {
        v.push("right");
    }
    if ly < 50 {
        v.push("up");
    } else if ly > 205 {
        v.push("down");
    }
    v
}

#[cfg(windows)]
fn read_sony(tx: Sender<Ev>) {
    std::thread::spawn(move || {
        let Ok(mut api) = hidapi::HidApi::new() else { return };
        let mut open: std::collections::HashSet<String> = Default::default();
        loop {
            let _ = api.refresh_devices();
            for d in api.device_list() {
                let (vid, pid) = (d.vendor_id(), d.product_id());
                if vid != SONY || !(DUALSENSE.contains(&pid) || DUALSHOCK4.contains(&pid)) {
                    continue;
                }
                let path = d.path().to_string_lossy().to_string();
                if open.contains(&path) {
                    continue;
                }
                let Ok(dev) = d.open_device(&api) else { continue };
                open.insert(path.clone());
                let name = if DUALSENSE.contains(&pid) { "DualSense" } else { "DualShock 4" };
                CONNECTED.lock().unwrap().push(Pad { name: name.into(), kind: "ps" });
                let tx = tx.clone();
                let dualsense = DUALSENSE.contains(&pid);
                std::thread::spawn(move || {
                    let mut buf = [0u8; 128];
                    let mut prev: Vec<&'static str> = vec![];
                    // The stick counts once it has been seen near the middle.
                    let mut rested = false;
                    loop {
                        match dev.read_timeout(&mut buf, 250) {
                            Ok(0) => continue,
                            Ok(n) => {
                                let Some(s) = parse_sony(&buf[..n], dualsense) else { continue };
                                let mut now = s.held.clone();
                                if (90..=166).contains(&s.lx) && (90..=166).contains(&s.ly) {
                                    rested = true;
                                }
                                for d in if rested { stick_dirs(s.lx, s.ly) } else { vec![] } {
                                    if !now.contains(&d) {
                                        now.push(d);
                                    }
                                }
                                for b in &now {
                                    if !prev.contains(b) {
                                        let _ = tx.send(Ev::Down(b, "ps"));
                                    }
                                }
                                for b in &prev {
                                    if !now.contains(b) {
                                        let _ = tx.send(Ev::Up(b));
                                    }
                                }
                                prev = now;
                            }
                            Err(_) => break, // unplugged
                        }
                    }
                    let mut c = CONNECTED.lock().unwrap();
                    if let Some(i) = c.iter().position(|p| p.kind == "ps") {
                        c.remove(i);
                    }
                });
            }
            // Forget devices that went away so they can be opened again when they come back.
            let live: std::collections::HashSet<String> = api.device_list().map(|d| d.path().to_string_lossy().to_string()).collect();
            open.retain(|p| live.contains(p));
            // Looked for less often while nothing needs a controller.
            std::thread::sleep(std::time::Duration::from_secs(if idle() { 6 } else { 2 }));
        }
    });
}

#[cfg(not(windows))]
fn read_sony(_tx: Sender<Ev>) {
    // On Linux the kernel's driver already makes PlayStation controllers ordinary gamepads.
}

fn read_gilrs(tx: Sender<Ev>) {
    std::thread::spawn(move || {
        use gilrs::{Axis, Button, EventType, Gilrs};
        let Ok(mut g) = Gilrs::new() else { return };
        let kind_of = |name: &str| {
            let n = name.to_lowercase();
            if n.contains("sony") || n.contains("dualsense") || n.contains("playstation") || n.contains("wireless controller") || n.contains("dualshock") {
                "ps"
            } else {
                "xbox"
            }
        };
        let refresh = |g: &Gilrs| {
            let mut c = CONNECTED.lock().unwrap();
            c.retain(|p| p.kind == "ps" && cfg!(windows));
            for (_, pad) in g.gamepads() {
                c.push(Pad { name: pad.name().to_string(), kind: kind_of(pad.name()) });
            }
        };
        refresh(&g);
        let mut stick: std::collections::HashMap<gilrs::GamepadId, (i8, i8)> = Default::default();
        let mut rested: std::collections::HashMap<(gilrs::GamepadId, u32), bool> = Default::default();
        loop {
            while let Some(ev) = g.next_event() {
                let kind = kind_of(g.gamepad(ev.id).name());
                let name = |b: Button| match b {
                    Button::South => Some("a"),
                    Button::East => Some("b"),
                    Button::West => Some("x"),
                    Button::North => Some("y"),
                    Button::LeftTrigger => Some("lb"),
                    Button::RightTrigger => Some("rb"),
                    Button::Select => Some("view"),
                    Button::Start => Some("menu"),
                    Button::Mode => Some("guide"),
                    Button::DPadUp => Some("up"),
                    Button::DPadDown => Some("down"),
                    Button::DPadLeft => Some("left"),
                    Button::DPadRight => Some("right"),
                    _ => None,
                };
                match ev.event {
                    EventType::Connected | EventType::Disconnected => refresh(&g),
                    EventType::ButtonPressed(b, _) => {
                        if let Some(n) = name(b) {
                            let _ = tx.send(Ev::Down(n, kind));
                        }
                    }
                    EventType::ButtonReleased(b, _) => {
                        if let Some(n) = name(b) {
                            let _ = tx.send(Ev::Up(n));
                        }
                    }
                    EventType::AxisChanged(axis, v, _) => {
                        // An axis counts once it has been seen resting near the middle: a wheel,
                        // pedal or worn stick that sits pushed all the way never scrolls on its own.
                        let a = rested.entry((ev.id, axis as u32)).or_insert(false);
                        if v.abs() < 0.3 {
                            *a = true;
                        }
                        if !*a {
                            continue;
                        }
                        let s = stick.entry(ev.id).or_default();
                        let dir = if v > 0.6 { 1 } else if v < -0.6 { -1 } else { 0 };
                        match axis {
                            Axis::LeftStickX if dir != s.0 => {
                                if s.0 != 0 {
                                    let _ = tx.send(Ev::Up(if s.0 > 0 { "right" } else { "left" }));
                                }
                                s.0 = dir;
                                if dir != 0 {
                                    let _ = tx.send(Ev::Down(if dir > 0 { "right" } else { "left" }, kind));
                                }
                            }
                            Axis::LeftStickY if dir != s.1 => {
                                if s.1 != 0 {
                                    let _ = tx.send(Ev::Up(if s.1 > 0 { "up" } else { "down" }));
                                }
                                s.1 = dir;
                                if dir != 0 {
                                    let _ = tx.send(Ev::Down(if dir > 0 { "up" } else { "down" }, kind));
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(if idle() { 60 } else { 10 }));
        }
    });
}

/// Start reading controllers. One place decides where each press goes.
pub fn start(app: AppHandle) {
    let (tx, rx) = channel::<Ev>();
    read_gilrs(tx.clone());
    read_sony(tx);
    std::thread::spawn(move || {
        use std::time::{Duration, Instant};
        let mut view_held = false;
        // Holding View on its own (0.7 s) opens the quick menu while a game runs. Steam and the
        // Xbox Game Bar keep the Guide button and View + Menu for themselves.
        let mut view_since: Option<Instant> = None;
        let mut view_used = false;
        // Holding Menu (Options) for a second, on the launcher's own window, switches to TV mode.
        // A tap does nothing, so a controller can't flip the launcher into TV mode by accident.
        let mut menu_since: Option<(Instant, &'static str)> = None;
        let mut last_dir = Instant::now();
        let mut last_name = "";
        // A direction held down repeats, like a keyboard: after 0.4 s, then every 0.12 s.
        let mut held: Option<(&'static str, &'static str, Instant, Instant)> = None;
        loop {
            let ev = match rx.recv_timeout(Duration::from_millis(30)) {
                Ok(ev) => ev,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if let Some((t, k)) = menu_since {
                        if t.elapsed() > Duration::from_millis(900) {
                            menu_since = None;
                            send(&app, "menu-hold", k);
                        }
                    }
                    if let Some(t) = view_since {
                        if !view_used && t.elapsed() > Duration::from_millis(700) {
                            view_used = true;
                            if !app.state::<App>().running.lock().unwrap().is_empty() {
                                ingame::toggle_overlay(&app);
                            }
                        }
                    }
                    if let Some((n, k, since, last)) = held {
                        if since.elapsed() > Duration::from_millis(400) && last.elapsed() > Duration::from_millis(120) {
                            held = Some((n, k, since, Instant::now()));
                            send(&app, n, k);
                        }
                    }
                    continue;
                }
                Err(_) => return,
            };
            let (name, kind) = match ev {
                Ev::Up("view") => {
                    view_held = false;
                    view_since = None;
                    continue;
                }
                Ev::Up(n) => {
                    if n == "menu" {
                        menu_since = None;
                    }
                    if held.map(|h| h.0 == n).unwrap_or(false) {
                        held = None;
                    }
                    continue;
                }
                Ev::Down(n, k) => (n, k),
            };
            // Menu with View held is a chord other programs use, not a hold.
            if name == "menu" && !view_held {
                menu_since = Some((Instant::now(), kind));
            }
            if name == "view" {
                menu_since = None;
                view_held = true;
                view_since = Some(Instant::now());
                view_used = false;
            } else if view_held {
                view_used = true;
            }
            let is_dir = ["up", "down", "left", "right"].contains(&name);
            // The same press can arrive twice: the d-pad and stick together, or a PlayStation
            // controller read directly and again through Steam Input's stand-in Xbox controller.
            // Nobody presses one button twice within 70 ms, so the second is dropped.
            if name == last_name && last_dir.elapsed().as_millis() < 70 {
                continue;
            }
            last_name = name;
            last_dir = Instant::now();
            held = if is_dir { Some((name, kind, Instant::now(), Instant::now())) } else { None };
            let playing = !app.state::<App>().running.lock().unwrap().is_empty();
            if playing && view_held && name == "rb" {
                ingame::take_screenshot(&app);
                continue;
            }
            // The quick menu is up: View closes it again.
            if playing && name == "view" && app.state::<App>().overlay_open.load(Ordering::Relaxed) {
                view_used = true;
                ingame::toggle_overlay(&app);
                continue;
            }
            send(&app, name, kind);
        }
    });
}

/// The quick menu gets the controller while it's up, focused or not (Windows may not let it take
/// focus from a game); otherwise the launcher's window, when it has focus.
fn send(app: &AppHandle, name: &str, kind: &str) {
    let payload = serde_json::json!({"b": name, "kind": kind});
    // emit_to, not a window's emit: in Tauri 2 that one reaches every window, so a press meant for
    // the launcher would also move (and press) the hidden quick menu, and the other way round.
    if app.state::<App>().overlay_open.load(Ordering::Relaxed) {
        if app.get_webview_window("overlay").is_some() {
            let _ = app.emit_to("overlay", "pad", payload);
        }
    } else if FOCUSED.load(Ordering::Relaxed) {
        if app.get_webview_window("main").is_some() {
            let _ = app.emit_to("main", "pad", payload);
        }
    }
}

/// The launcher's window has focus (window events keep this up to date).
pub static FOCUSED: AtomicBool = AtomicBool::new(false);
/// A game is running (its quick menu needs the controller).
pub static PLAYING: AtomicBool = AtomicBool::new(false);

fn idle() -> bool {
    !FOCUSED.load(Ordering::Relaxed) && !PLAYING.load(Ordering::Relaxed)
}

pub fn connected() -> Vec<Pad> {
    CONNECTED.lock().unwrap().clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dualsense_usb() {
        let mut r = vec![0u8; 64];
        r[0] = 0x01;
        r[1] = 128;
        r[2] = 128;
        r[8] = 0x20 | 0x02; // cross, d-pad right
        r[9] = 0x10 | 0x02; // create, R1
        r[10] = 0x01; // PS
        let s = parse_sony(&r, true).unwrap();
        for b in ["right", "a", "view", "rb", "guide"] {
            assert!(s.held.contains(&b), "{b} missing: {:?}", s.held);
        }
        assert!(!s.held.contains(&"up"));
    }

    #[test]
    fn dualsense_bluetooth_and_dualshock() {
        let mut bt = vec![0u8; 78];
        bt[0] = 0x31;
        bt[9] = 0x08 | 0x40; // no d-pad, circle
        bt[10] = 0x20; // options
        let s = parse_sony(&bt, true).unwrap();
        assert_eq!(s.held, vec!["b", "menu"]);
        let mut ds4 = vec![0u8; 64];
        ds4[0] = 0x01;
        ds4[5] = 0x04 | 0x80; // down, triangle
        ds4[6] = 0x01; // L1
        let s = parse_sony(&ds4, false).unwrap();
        assert_eq!(s.held, vec!["down", "y", "lb"]);
        let mut ds4bt = vec![0u8; 78];
        ds4bt[0] = 0x11;
        ds4bt[7] = 0x08 | 0x10; // square
        assert_eq!(parse_sony(&ds4bt, false).unwrap().held, vec!["x"]);
    }

    #[test]
    fn sticks() {
        assert_eq!(stick_dirs(0, 128), vec!["left"]);
        assert_eq!(stick_dirs(255, 255), vec!["right", "down"]);
        assert!(stick_dirs(128, 128).is_empty());
    }
}

//! Controllers, through SDL 3's gamepad support on every system: Xbox pads, PlayStation
//! (DualSense, DualShock 4), Switch Pro and Joy-Cons, 8BitDo and the hundreds of others SDL knows,
//! plus any mappings in `gamecontrollerdb.txt` in the launcher's folder.
//! They're read even while a game has focus, so the in-game combos work:
//! View + RB (Create + R1) takes a screenshot, holding View (Create) opens the quick menu. Otherwise buttons go to the launcher's window as `pad` events:
//! {"b": a | b | x | y | lb | rb | lt | rt | ls | rs | view | menu | guide | up | down | left | right, "kind": xbox | ps | nintendo | steam}.
//!
//! SDL runs on one thread of its own, which starts it and makes every SDL call: SDL takes the
//! thread that starts its events for its main thread.

use crate::{ingame, App};
use serde::Serialize;
use sdl3_sys::everything as sdl;
use std::collections::HashMap;
use std::ffi::{c_char, CStr, CString};
use std::path::{Path, PathBuf};
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

#[derive(Debug, PartialEq)]
enum Ev {
    Down(&'static str, &'static str),
    Up(&'static str),
}

/// Valve: the Steam Controller and the Steam Deck. SDL reads a Steam Controller itself; the
/// Deck's controls come through Steam Input.
pub const VALVE: u16 = 0x28DE;

/// Which button icons a controller gets.
fn kind_of(ty: sdl::SDL_GamepadType, vendor: u16) -> &'static str {
    use sdl::SDL_GamepadType as T;
    match ty {
        T::PS3 | T::PS4 | T::PS5 => "ps",
        T::NINTENDO_SWITCH_PRO | T::NINTENDO_SWITCH_JOYCON_LEFT | T::NINTENDO_SWITCH_JOYCON_RIGHT | T::NINTENDO_SWITCH_JOYCON_PAIR | T::GAMECUBE => "nintendo",
        T::STEAM => "steam",
        _ if vendor == VALVE => "steam",
        _ => "xbox",
    }
}

/// A button's name for the launcher. The face buttons go by what's printed on them (SDL knows,
/// per controller), so a Nintendo pad's A is `a` though it sits on the right, and Cross is `a`.
fn button_name(button: sdl::SDL_GamepadButton, label: sdl::SDL_GamepadButtonLabel) -> Option<&'static str> {
    use sdl::SDL_GamepadButton as B;
    use sdl::SDL_GamepadButtonLabel as L;
    let face = match label {
        L::A | L::CROSS => Some("a"),
        L::B | L::CIRCLE => Some("b"),
        L::X | L::SQUARE => Some("x"),
        L::Y | L::TRIANGLE => Some("y"),
        _ => None,
    };
    match button {
        B::SOUTH => face.or(Some("a")),
        B::EAST => face.or(Some("b")),
        B::WEST => face.or(Some("x")),
        B::NORTH => face.or(Some("y")),
        B::BACK => Some("view"),
        B::START => Some("menu"),
        B::GUIDE => Some("guide"),
        B::LEFT_STICK => Some("ls"),
        B::RIGHT_STICK => Some("rs"),
        B::LEFT_SHOULDER => Some("lb"),
        B::RIGHT_SHOULDER => Some("rb"),
        B::DPAD_UP => Some("up"),
        B::DPAD_DOWN => Some("down"),
        B::DPAD_LEFT => Some("left"),
        B::DPAD_RIGHT => Some("right"),
        _ => None,
    }
}

/// The left stick and the triggers of one controller, as presses.
#[derive(Default)]
struct Axes {
    /// An axis counts once it has been seen resting: a wheel, pedal or worn stick that sits
    /// pushed all the way never scrolls on its own.
    rested: [bool; 6],
    /// Where each axis is pushed: -1, 0 or 1.
    dir: [i8; 6],
}

impl Axes {
    /// An axis moved (SDL's -32768..32767, or 0..32767 for a trigger): what's pressed or let go.
    /// `true` is a press.
    fn moved(&mut self, axis: sdl::SDL_GamepadAxis, value: i16) -> Vec<(bool, &'static str)> {
        use sdl::SDL_GamepadAxis as A;
        // Which name each way is: SDL's stick Y is positive downwards.
        let (i, names): (usize, [&'static str; 2]) = match axis {
            // Settings → Controllers can leave the stick out: let go of whatever it was holding.
            A::LEFTX | A::LEFTY if !STICK.load(Ordering::Relaxed) => {
                let i = if axis == A::LEFTX { 0 } else { 1 };
                let held = std::mem::take(&mut self.dir[i]);
                let names = if i == 0 { ["left", "right"] } else { ["up", "down"] };
                return if held == 0 { vec![] } else { vec![(false, if held > 0 { names[1] } else { names[0] })] };
            }
            A::LEFTX => (0, ["left", "right"]),
            A::LEFTY => (1, ["up", "down"]),
            A::LEFT_TRIGGER => (4, ["", "lt"]),
            A::RIGHT_TRIGGER => (5, ["", "rt"]),
            _ => return vec![],
        };
        let v = value as f32 / 32767.0;
        if v.abs() < 0.3 {
            self.rested[i] = true;
        }
        if !self.rested[i] {
            return vec![];
        }
        let dir = if v > 0.6 { 1 } else if v < -0.6 && !names[0].is_empty() { -1 } else { 0 };
        let was = self.dir[i];
        if dir == was {
            return vec![];
        }
        self.dir[i] = dir;
        let name = |d: i8| if d > 0 { names[1] } else { names[0] };
        let mut out = vec![];
        if was != 0 {
            out.push((false, name(was)));
        }
        if dir != 0 {
            out.push((true, name(dir)));
        }
        out
    }
}

struct Open {
    pad: *mut sdl::SDL_Gamepad,
    name: String,
    kind: &'static str,
    axes: Axes,
}

fn set_hint(name: *const c_char, value: &CStr) {
    // SAFETY: both are NUL-terminated strings that SDL copies.
    unsafe {
        sdl::SDL_SetHint(name, value.as_ptr());
    }
}

fn list(open: &HashMap<sdl::SDL_JoystickID, Open>) {
    let mut ids: Vec<_> = open.keys().copied().collect();
    ids.sort();
    let mut c = CONNECTED.lock().unwrap();
    c.clear();
    c.extend(ids.iter().map(|id| Pad { name: open[id].name.clone(), kind: open[id].kind }));
}

/// Start SDL for controllers, on the thread that will make every SDL call from now on.
fn init(mappings: &Path) -> bool {
    // Hints, not environment variables, so the games the launcher starts don't get them.
    // Keep reading while a game is in front.
    set_hint(sdl::SDL_HINT_JOYSTICK_ALLOW_BACKGROUND_EVENTS, c"1");
    // Leave PlayStation and Switch pads in the mode they started in. Enhanced reports break
    // DirectInput for games that don't use SDL, and a PlayStation pad keeps them until it's
    // switched off.
    set_hint(sdl::SDL_HINT_JOYSTICK_ENHANCED_REPORTS, c"0");
    // DirectInput opens a controller for itself alone, which would keep it from a game.
    // XInput and SDL's own drivers cover the controllers the launcher has always read.
    set_hint(sdl::SDL_HINT_JOYSTICK_DIRECTINPUT, c"0");
    // SDL's Steam Deck driver turns the Deck's trackpad mouse off for as long as it's open.
    // Started from Steam, the Deck's controls come through Steam Input instead.
    // SDL's Steam Controller drivers stay on: they read the controller without Steam. Its
    // trackpad mouse is off while it's open here, and (on the first model) back when it closes.
    set_hint(sdl::SDL_HINT_JOYSTICK_HIDAPI_STEAMDECK, c"0");
    // Closing the launcher (Ctrl+C, the system's own "quit") must still close it.
    set_hint(sdl::SDL_HINT_NO_SIGNAL_HANDLERS, c"1");
    // SAFETY: SDL is started here and only ever called from this thread.
    if !unsafe { sdl::SDL_Init(sdl::SDL_INIT_GAMEPAD) } {
        // SAFETY: SDL_GetError always returns a NUL-terminated string.
        eprintln!("No controllers: {}", unsafe { CStr::from_ptr(sdl::SDL_GetError()) }.to_string_lossy());
        return false;
    }
    // Events nothing here reads, so SDL doesn't queue them: "update complete" comes with
    // every report a controller sends. The joystick-level button, axis and hat events stay:
    // SDL makes the gamepad events from them.
    for t in [
        sdl::SDL_EVENT_JOYSTICK_UPDATE_COMPLETE,
        sdl::SDL_EVENT_GAMEPAD_UPDATE_COMPLETE,
        sdl::SDL_EVENT_GAMEPAD_TOUCHPAD_DOWN,
        sdl::SDL_EVENT_GAMEPAD_TOUCHPAD_MOTION,
        sdl::SDL_EVENT_GAMEPAD_TOUCHPAD_UP,
        sdl::SDL_EVENT_GAMEPAD_SENSOR_UPDATE,
        sdl::SDL_EVENT_JOYSTICK_BATTERY_UPDATED,
        sdl::SDL_EVENT_JOYSTICK_BALL_MOTION,
    ] {
        // SAFETY: SDL is running on this thread.
        unsafe { sdl::SDL_SetEventEnabled(t.0, false) };
    }
    // The player's own mappings, for a controller SDL doesn't know.
    if mappings.is_file() {
        if let Some(p) = mappings.to_str().and_then(|p| CString::new(p).ok()) {
            // SAFETY: a NUL-terminated UTF-8 path, as SDL wants.
            if unsafe { sdl::SDL_AddGamepadMappingsFromFile(p.as_ptr()) } < 0 {
                // SAFETY: SDL_GetError always returns a NUL-terminated string.
                eprintln!("Couldn't read {}: {}", mappings.display(), unsafe { CStr::from_ptr(sdl::SDL_GetError()) }.to_string_lossy());
            }
        }
    }
    true
}

/// The controllers SDL has open, and what they're doing.
#[derive(Default)]
struct Reader {
    open: HashMap<sdl::SDL_JoystickID, Open>,
}

impl Reader {
    /// Close every controller. SDL puts back what it changed when it opened one (a Steam
    /// Controller's trackpad mouse, for one).
    fn close(&mut self) {
        for (_, o) in self.open.drain() {
            // SAFETY: each pad was opened once, and is closed once.
            unsafe { sdl::SDL_CloseGamepad(o.pad) };
        }
        list(&self.open);
    }

    /// Everything SDL has seen since last time, as presses.
    fn poll(&mut self, tx: &Sender<Ev>) {
        let open = &mut self.open;
        // SAFETY: an SDL_Event is plain data SDL fills in; the union member read is the one
        // its type says.
        let mut ev: sdl::SDL_Event = unsafe { std::mem::zeroed() };
        while unsafe { sdl::SDL_PollEvent(&mut ev) } {
            let ty = sdl::SDL_EventType(unsafe { ev.r#type });
            match ty {
                sdl::SDL_EVENT_GAMEPAD_ADDED => {
                    let id = unsafe { ev.gdevice.which };
                    if open.contains_key(&id) {
                        continue;
                    }
                    // SAFETY: the id is one SDL just announced; a null pad is skipped.
                    let pad = unsafe { sdl::SDL_OpenGamepad(id) };
                    if pad.is_null() {
                        continue;
                    }
                    let name = unsafe { sdl::SDL_GetGamepadName(pad) };
                    let name = if name.is_null() { "Controller".to_string() } else { unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned() };
                    let kind = kind_of(unsafe { sdl::SDL_GetGamepadType(pad) }, unsafe { sdl::SDL_GetGamepadVendor(pad) });
                    open.insert(id, Open { pad, name, kind, axes: Axes::default() });
                    list(open);
                }
                sdl::SDL_EVENT_GAMEPAD_REMOVED => {
                    if let Some(o) = open.remove(&unsafe { ev.gdevice.which }) {
                        // SAFETY: opened above, closed once.
                        unsafe { sdl::SDL_CloseGamepad(o.pad) };
                        list(open);
                    }
                }
                sdl::SDL_EVENT_GAMEPAD_BUTTON_DOWN | sdl::SDL_EVENT_GAMEPAD_BUTTON_UP => {
                    let b = unsafe { ev.gbutton };
                    let Some(o) = open.get(&b.which) else { continue };
                    let button = sdl::SDL_GamepadButton(b.button as i32);
                    // SAFETY: the pad is open.
                    let label = unsafe { sdl::SDL_GetGamepadButtonLabel(o.pad, button) };
                    if let Some(n) = button_name(button, label) {
                        let _ = tx.send(if b.down { Ev::Down(n, o.kind) } else { Ev::Up(n) });
                    }
                }
                sdl::SDL_EVENT_GAMEPAD_AXIS_MOTION => {
                    let a = unsafe { ev.gaxis };
                    let Some(o) = open.get_mut(&a.which) else { continue };
                    for (down, n) in o.axes.moved(sdl::SDL_GamepadAxis(a.axis as i32), a.value) {
                        let _ = tx.send(if down { Ev::Down(n, o.kind) } else { Ev::Up(n) });
                    }
                }
                _ => {}
            }
        }
    }
}

fn read_sdl(tx: Sender<Ev>, mappings: PathBuf) {
    RUNNING.store(true, Ordering::Relaxed);
    let spawned = std::thread::Builder::new().name("pads".into()).spawn(move || {
        if !init(&mappings) {
            RUNNING.store(false, Ordering::Relaxed);
            return;
        }
        let mut reader = Reader::default();
        while !STOP.load(Ordering::Relaxed) {
            reader.poll(&tx);
            // During a game the quick menu and the screenshot combo only wait for their buttons: a
            // check every 25 ms is plenty, and leaves the processor to the game. Not slower, even
            // with the quick menu off: SDL reads a controller's state when it's asked, so a quick
            // tap between two checks would be missed.
            let playing = PLAYING.load(Ordering::Relaxed);
            std::thread::sleep(std::time::Duration::from_millis(if idle() { 60 } else if playing && !FOCUSED.load(Ordering::Relaxed) { 25 } else { 10 }));
        }
        reader.close();
        // SAFETY: the last SDL call, on SDL's thread.
        unsafe { sdl::SDL_Quit() };
        RUNNING.store(false, Ordering::Relaxed);
    });
    if let Err(e) = spawned {
        RUNNING.store(false, Ordering::Relaxed);
        eprintln!("No controllers: {e}");
    }
}

/// SDL's thread is running (or starting).
static RUNNING: AtomicBool = AtomicBool::new(false);
/// The launcher is closing: SDL's thread closes the controllers and stops.
static STOP: AtomicBool = AtomicBool::new(false);

/// The launcher is closing: close the controllers properly, so SDL can put back what it changed,
/// waiting half a second at most.
pub fn stop() {
    STOP.store(true, Ordering::Relaxed);
    let t = std::time::Instant::now();
    while RUNNING.load(Ordering::Relaxed) && t.elapsed() < std::time::Duration::from_millis(500) {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Start reading controllers. One place decides where each press goes.
pub fn start(app: AppHandle) {
    let (tx, rx) = channel::<Ev>();
    read_sdl(tx, app.state::<App>().dir.join("gamecontrollerdb.txt"));
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
                            if QUICK_MENU.load(Ordering::Relaxed) && !app.state::<App>().running.lock().unwrap().is_empty() {
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
/// Settings → Controllers: whether the left stick moves around the launcher.
pub static STICK: AtomicBool = AtomicBool::new(true);
/// The player wants the quick menu over games (Settings, While Playing).
pub static QUICK_MENU: AtomicBool = AtomicBool::new(true);

fn idle() -> bool {
    !FOCUSED.load(Ordering::Relaxed) && !PLAYING.load(Ordering::Relaxed)
}

pub fn connected() -> Vec<Pad> {
    CONNECTED.lock().unwrap().clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdl::SDL_GamepadAxis as A;
    use sdl::SDL_GamepadButton as B;
    use sdl::SDL_GamepadButtonLabel as L;
    use sdl::SDL_GamepadType as T;

    #[test]
    fn kinds() {
        assert_eq!(kind_of(T::PS5, 0x054C), "ps");
        assert_eq!(kind_of(T::PS4, 0x054C), "ps");
        assert_eq!(kind_of(T::PS3, 0x054C), "ps");
        assert_eq!(kind_of(T::NINTENDO_SWITCH_PRO, 0x057E), "nintendo");
        assert_eq!(kind_of(T::NINTENDO_SWITCH_JOYCON_PAIR, 0x057E), "nintendo");
        assert_eq!(kind_of(T::STEAM, VALVE), "steam");
        // Steam's own controllers show as Steam's even when SDL calls them an Xbox pad.
        assert_eq!(kind_of(T::XBOX360, VALVE), "steam");
        assert_eq!(kind_of(T::XBOXONE, 0x045E), "xbox");
        assert_eq!(kind_of(T::STANDARD, 0x2DC8), "xbox");
        assert_eq!(kind_of(T::UNKNOWN, 0), "xbox");
    }

    #[test]
    fn face_buttons_go_by_what_is_printed() {
        // Xbox: A at the bottom.
        assert_eq!(button_name(B::SOUTH, L::A), Some("a"));
        assert_eq!(button_name(B::NORTH, L::Y), Some("y"));
        // PlayStation: Cross, Circle, Square, Triangle.
        assert_eq!(button_name(B::SOUTH, L::CROSS), Some("a"));
        assert_eq!(button_name(B::EAST, L::CIRCLE), Some("b"));
        assert_eq!(button_name(B::WEST, L::SQUARE), Some("x"));
        assert_eq!(button_name(B::NORTH, L::TRIANGLE), Some("y"));
        // Nintendo: A on the right, B at the bottom, X on top, Y on the left.
        assert_eq!(button_name(B::EAST, L::A), Some("a"));
        assert_eq!(button_name(B::SOUTH, L::B), Some("b"));
        assert_eq!(button_name(B::NORTH, L::X), Some("x"));
        assert_eq!(button_name(B::WEST, L::Y), Some("y"));
        // No label known: by position, like an Xbox pad.
        assert_eq!(button_name(B::SOUTH, L::UNKNOWN), Some("a"));
        assert_eq!(button_name(B::EAST, L::UNKNOWN), Some("b"));
    }

    #[test]
    fn other_buttons() {
        let n = |b| button_name(b, L::UNKNOWN);
        assert_eq!(n(B::BACK), Some("view"));
        assert_eq!(n(B::START), Some("menu"));
        assert_eq!(n(B::GUIDE), Some("guide"));
        assert_eq!(n(B::LEFT_SHOULDER), Some("lb"));
        assert_eq!(n(B::RIGHT_SHOULDER), Some("rb"));
        assert_eq!(n(B::LEFT_STICK), Some("ls"));
        assert_eq!(n(B::RIGHT_STICK), Some("rs"));
        assert_eq!(n(B::DPAD_UP), Some("up"));
        assert_eq!(n(B::DPAD_DOWN), Some("down"));
        assert_eq!(n(B::DPAD_LEFT), Some("left"));
        assert_eq!(n(B::DPAD_RIGHT), Some("right"));
        // Paddles, the touchpad and the share/capture button aren't the launcher's.
        assert_eq!(n(B::TOUCHPAD), None);
        assert_eq!(n(B::MISC1), None);
        assert_eq!(n(B::RIGHT_PADDLE1), None);
    }

    #[test]
    fn stick() {
        let mut a = Axes::default();
        // Not seen resting yet: a stick that starts pushed does nothing.
        assert!(a.moved(A::LEFTX, 32767).is_empty());
        assert!(a.moved(A::LEFTX, 0).is_empty());
        assert_eq!(a.moved(A::LEFTX, 30000), vec![(true, "right")]);
        // Still right: nothing new.
        assert!(a.moved(A::LEFTX, 32767).is_empty());
        // Straight across: right is let go, then left pressed.
        assert_eq!(a.moved(A::LEFTX, -32768), vec![(false, "right"), (true, "left")]);
        // Half way back counts as let go.
        assert_eq!(a.moved(A::LEFTX, -15000), vec![(false, "left")]);
        // SDL's Y is positive downwards.
        assert!(a.moved(A::LEFTY, 0).is_empty());
        assert_eq!(a.moved(A::LEFTY, -32768), vec![(true, "up")]);
        assert_eq!(a.moved(A::LEFTY, 32767), vec![(false, "up"), (true, "down")]);
        // The right stick isn't read.
        assert!(a.moved(A::RIGHTX, 0).is_empty());
        assert!(a.moved(A::RIGHTX, 32767).is_empty());
    }

    #[test]
    fn each_axis_rests_on_its_own() {
        let mut a = Axes::default();
        assert!(a.moved(A::LEFTX, 0).is_empty());
        // X resting doesn't make a Y that sits pushed count.
        assert!(a.moved(A::LEFTY, 32767).is_empty());
        assert_eq!(a.moved(A::LEFTX, 32767), vec![(true, "right")]);
    }

    /// The whole way through SDL, with virtual controllers: SDL starts with the launcher's
    /// settings, makes gamepad events, and the launcher names them. The only test that starts
    /// SDL, so nothing else shares its state.
    #[test]
    fn through_sdl() {
        assert!(init(Path::new("")));
        let (tx, rx) = channel();
        let mut r = Reader::default();
        let attach = |vendor: u16, product: u16| {
            let mut d = sdl::SDL_VirtualJoystickDesc::default();
            d.r#type = sdl::SDL_JOYSTICK_TYPE_GAMEPAD.0 as u16;
            d.vendor_id = vendor;
            d.product_id = product;
            d.naxes = sdl::SDL_GAMEPAD_AXIS_COUNT.0 as u16;
            d.nbuttons = sdl::SDL_GAMEPAD_BUTTON_COUNT.0 as u16;
            d.name = c"Test pad".as_ptr();
            // SAFETY: SDL runs on this test's thread; the description outlives the call.
            let id = unsafe { sdl::SDL_AttachVirtualJoystick(&d) };
            assert_ne!(id.0, 0);
            let j = unsafe { sdl::SDL_OpenJoystick(id) };
            assert!(!j.is_null());
            (id, j)
        };
        // SAFETY (for the calls below): the joysticks are open, on this thread.
        let press = |j, b: B, down| unsafe { assert!(sdl::SDL_SetJoystickVirtualButton(j, b.0, down)) };
        let axis = |j, a: A, v| unsafe { assert!(sdl::SDL_SetJoystickVirtualAxis(j, a.0, v)) };

        // An Xbox One pad.
        let (xid, x) = attach(0x045E, 0x02EA);
        r.poll(&tx);
        assert_eq!(connected().iter().map(|p| p.kind).collect::<Vec<_>>(), vec!["xbox"]);
        press(x, B::SOUTH, true);
        press(x, B::BACK, true);
        r.poll(&tx);
        press(x, B::SOUTH, false);
        axis(x, A::LEFTX, 0);
        r.poll(&tx);
        axis(x, A::LEFTX, 32767);
        r.poll(&tx);
        let got: Vec<Ev> = rx.try_iter().collect();
        assert_eq!(got, vec![Ev::Down("a", "xbox"), Ev::Down("view", "xbox"), Ev::Up("a"), Ev::Down("right", "xbox")]);

        // A Switch Pro Controller: its A is on the right.
        let (nid, n) = attach(0x057E, 0x2009);
        r.poll(&tx);
        assert!(connected().iter().any(|p| p.kind == "nintendo"), "{:?}", connected());
        press(n, B::EAST, true);
        r.poll(&tx);
        press(n, B::SOUTH, true);
        r.poll(&tx);
        let got: Vec<Ev> = rx.try_iter().collect();
        assert_eq!(got, vec![Ev::Down("a", "nintendo"), Ev::Down("b", "nintendo")]);

        // Unplugged: gone from the list.
        unsafe {
            sdl::SDL_CloseJoystick(x);
            assert!(sdl::SDL_DetachVirtualJoystick(xid));
        }
        r.poll(&tx);
        assert_eq!(connected().iter().map(|p| p.kind).collect::<Vec<_>>(), vec!["nintendo"]);
        // Closing the launcher closes the rest.
        r.close();
        assert!(connected().is_empty(), "{:?}", connected());
        unsafe {
            sdl::SDL_CloseJoystick(n);
            assert!(sdl::SDL_DetachVirtualJoystick(nid));
            sdl::SDL_Quit();
        }
    }

    #[test]
    fn triggers() {
        let mut a = Axes::default();
        // A pedal that sits pressed doesn't count until it's been let go.
        assert!(a.moved(A::LEFT_TRIGGER, 32767).is_empty());
        assert!(a.moved(A::LEFT_TRIGGER, 0).is_empty());
        assert!(a.moved(A::LEFT_TRIGGER, 15000).is_empty());
        assert_eq!(a.moved(A::LEFT_TRIGGER, 25000), vec![(true, "lt")]);
        assert_eq!(a.moved(A::LEFT_TRIGGER, 5000), vec![(false, "lt")]);
        assert!(a.moved(A::RIGHT_TRIGGER, 0).is_empty());
        assert_eq!(a.moved(A::RIGHT_TRIGGER, 32767), vec![(true, "rt")]);
        assert_eq!(a.moved(A::RIGHT_TRIGGER, 0), vec![(false, "rt")]);
    }
}

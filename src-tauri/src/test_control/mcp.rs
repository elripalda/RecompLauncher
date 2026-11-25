//! MCP (Model Context Protocol) over JSON-RPC 2.0, served at `POST /mcp`: `initialize`, `ping`,
//! `tools/list` and `tools/call`. The message handling follows the one in
//! elripalda/RecompLauncher#10 by Alex Southwell; the tools are test control's own.
//!
//! The ui_* tools, launcher_command and the window's console run in the window (page.rs and
//! bridge.js), so they do exactly what a click or the window's own code would. The others read
//! the launcher's state and files, or press controller buttons the way pads.rs does.

use super::{page, Ctx};
use serde_json::{json, Value};
use std::time::Duration;
use tauri::{Emitter, Manager};

pub const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
pub const DEFAULT_PROTOCOL: &str = "2025-06-18";

/// How long a step in the window may take unless the call says otherwise.
const STEP_WAIT: Duration = Duration::from_secs(20);
/// The longest any call may ask to wait (an install, a long setup).
const MAX_WAIT_MS: u64 = 600_000;
/// What a log read gives at most.
const MAX_LOG_LINES: usize = 2000;
const PAD_BUTTONS: &[&str] = &["a", "b", "x", "y", "lb", "rb", "lt", "rt", "view", "menu", "ls", "rs", "up", "down", "left", "right", "guide"];
const PAGES: &[&str] = &["home", "discover", "library", "achievements", "game", "downloads", "settings", "shots", "soon", "saves", "back", "bigpicture", "desktop"];

const INSTRUCTIONS: &str = "Drives a RecompLauncher test build, as a tester would. Start with launcher_status and ui_snapshot. ui_snapshot lists what can be pressed in what's in front (a dialog, Big Picture mode or the page), each with a ref; ui_click, ui_type and ui_key take a ref, a CSS selector or the words on the element. Refs last until the page is drawn again: take a new snapshot after a click changes the page. ui_navigate opens a page (bigpicture and desktop switch modes); Big Picture mode is moved around with controller_press. ui_wait_for waits for text, an element or a page. launcher_command calls one of the launcher's own commands the way the window does (installs, setup, Play). read_logs reads the launcher's trail, a game's output and the window's console; recent_events follows downloads, setups and games starting. screenshot shows the window (Windows only). This is a real launcher on a real PC: installs, uninstalls and settings changes really happen.";

fn rpc_ok(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn rpc_err(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

pub fn parse_error(detail: &str) -> Value {
    rpc_err(&Value::Null, -32700, &format!("Parse error: {detail}"))
}

fn obj(props: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": props, "required": required, "additionalProperties": false})
}

/// Naming an element: a ref from ui_snapshot, a CSS selector, or the words on it.
fn target_props(extra: Value) -> Value {
    let mut p = json!({
        "ref": {"type": "string", "description": "An element's ref from ui_snapshot"},
        "selector": {"type": "string", "description": "A CSS selector (the first visible match, or the one at index)"},
        "text": {"type": "string", "description": "The words on the element, as ui_snapshot shows its label (an exact match first, else one that contains them)"},
        "index": {"type": "integer", "minimum": 0, "description": "Which match, when several match"},
    });
    if let (Some(p), Some(e)) = (p.as_object_mut(), extra.as_object()) {
        p.extend(e.clone());
    }
    p
}

fn tool(name: &str, title: &str, description: &str, schema: Value, read_only: bool) -> Value {
    json!({"name": name, "title": title, "description": description, "inputSchema": schema, "annotations": {"readOnlyHint": read_only, "openWorldHint": false}})
}

pub fn tools() -> Value {
    let wait_ms = json!({"type": "integer", "minimum": 0, "maximum": MAX_WAIT_MS});
    json!([
        tool("launcher_status", "Launcher status", "The launcher's version, OS and data folder, whether its window is shown, the installed games (id, name, version, folder, whether the folder is there), the games running (with their process ids), and downloads and setups in progress.", obj(json!({}), &[]), true),
        tool("ui_snapshot", "What's on screen", "What's in front now (the top dialog, Big Picture mode or the page): the page, its headings and messages, and what can be pressed or typed in, each with a ref for ui_click, ui_type and ui_key. On-screen elements come first. text: true adds the visible text.", obj(json!({
            "max": {"type": "integer", "minimum": 1, "maximum": 600, "description": "How many elements at most (150 if not given)"},
            "text": {"type": "boolean", "description": "Also give the visible text"},
            "text_chars": {"type": "integer", "minimum": 100, "maximum": 60000},
        }), &[]), true),
        tool("ui_click", "Click", "Clicks an element, as a mouse would, then waits a moment for the page to settle. Answers with the page after it.", obj(target_props(json!({
            "settle_ms": {"type": "integer", "minimum": 0, "maximum": 10000, "description": "How long to wait after the click (350 if not given)"},
        })), &[]), false),
        tool("ui_type", "Type", "Puts text in a box (a search, a field, a select's value) and tells the page it changed. submit: true presses Enter after.", obj(target_props(json!({
            "value": {"type": "string"},
            "clear": {"type": "boolean", "description": "false adds to what's there instead of replacing it"},
            "submit": {"type": "boolean"},
        })), &["value"]), false),
        tool("ui_key", "Press a key", "Presses a key on the focused element, or on the one named: Enter, Escape, Tab, ArrowUp/Down/Left/Right, Backspace, a letter, with ctrl, shift, alt or meta.", obj(target_props(json!({
            "key": {"type": "string"},
            "ctrl": {"type": "boolean"}, "shift": {"type": "boolean"}, "alt": {"type": "boolean"}, "meta": {"type": "boolean"},
            "settle_ms": {"type": "integer", "minimum": 0, "maximum": 10000},
        })), &["key"]), false),
        tool("ui_navigate", "Open a page", "Opens one of the launcher's pages, as the sidebar would: home, discover, library, achievements, game (with id, and tab), downloads, settings (with tab), shots, soon, saves, plug:<plugin id>, back, bigpicture to start Big Picture mode, or desktop to leave it.", obj(json!({
            "page": {"type": "string", "description": PAGES.join(", ")},
            "id": {"type": "string", "description": "The game's id, for page game"},
            "tab": {"type": "string", "description": "A game page's tab, or Settings' section"},
        }), &["page"]), false),
        tool("ui_wait_for", "Wait for", "Waits until text shows anywhere in the window, an element matching a selector is visible, or a page is open (all that are given), or with gone: true until it isn't. Answers met: false when the time runs out.", obj(json!({
            "text": {"type": "string"},
            "selector": {"type": "string"},
            "page": {"type": "string"},
            "gone": {"type": "boolean"},
            "timeout_ms": {"type": "integer", "minimum": 100, "maximum": 120000, "description": "10000 if not given"},
        }), &[]), true),
        tool("ui_eval", "Run JavaScript", "Runs JavaScript in the launcher's window, as the body of an async function: use await, and return a value that can be turned into JSON. The window's globals are there (S, the window's state; go; invoke). For checks the other tools can't make.", obj(json!({
            "code": {"type": "string"},
            "args": {"type": "object", "description": "Passed to the code as args"},
            "timeout_ms": wait_ms,
        }), &["code"]), false),
        tool("launcher_command", "Launcher command", "Calls one of the launcher's own commands the way its window does (the names in lib.rs's invoke handler), and answers with what it returned. wait: false starts it and answers at once (for a long install; follow it with recent_events or launcher_status).", obj(json!({
            "name": {"type": "string"},
            "args": {"type": "object"},
            "wait": {"type": "boolean"},
            "timeout_ms": wait_ms,
        }), &["name"]), false),
        tool("controller_press", "Press a controller button", "Presses a controller button in the launcher's window, as a real controller would (it moves around Big Picture mode and the desktop window's controller highlight).", obj(json!({
            "button": {"type": "string", "enum": PAD_BUTTONS},
            "times": {"type": "integer", "minimum": 1, "maximum": 20, "description": "Presses in a row (1 if not given)"},
            "kind": {"type": "string", "enum": ["xbox", "ps", "nintendo", "steam"], "description": "The controller's kind, for the button pictures (xbox if not given)"},
        }), &["button"]), false),
        tool("read_logs", "Read logs", "The newest lines of a log: launcher (the launcher's trail: installs, setups, games starting, errors), game (what a game printed on its last start; needs id), or console (the window's errors and warnings since test control started).", obj(json!({
            "log": {"type": "string", "enum": ["launcher", "game", "console"]},
            "id": {"type": "string"},
            "lines": {"type": "integer", "minimum": 1, "maximum": MAX_LOG_LINES, "description": "200 if not given"},
        }), &["log"]), true),
        tool("recent_events", "Recent events", "What the launcher told its window since an event number (0 for all kept): download progress (install), setups (setup), games starting and closing (game), messages (toast) and more. Answers with last, the number to ask from next time.", obj(json!({
            "since": {"type": "integer", "minimum": 0},
        }), &[]), true),
        tool("screenshot", "Screenshot", "A picture of the launcher's window, or of the whole main screen with screen: true (to see a game). Windows only.", obj(json!({
            "screen": {"type": "boolean"},
            "max_width": {"type": "integer", "minimum": 200, "maximum": 3840, "description": "Scaled down to this width (1280 if not given)"},
        }), &[]), true),
        tool("launcher_window", "Launcher window", "Shows and focuses the launcher's window (some things only happen while it has focus), or minimizes it.", obj(json!({
            "action": {"type": "string", "enum": ["show", "minimize"]},
        }), &["action"]), false),
    ])
}

/// One message. None: nothing to answer (a notification, or a response from the client).
pub(crate) async fn handle(ctx: &Ctx, msg: Value) -> Option<Value> {
    // Batches were dropped from MCP: one call per request keeps every call through one path.
    if msg.is_array() {
        return Some(rpc_err(&Value::Null, -32600, "Invalid Request: send one message at a time"));
    }
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str);
    let is_rpc = msg.get("jsonrpc").and_then(Value::as_str) == Some("2.0");
    let Some(method) = method.filter(|_| is_rpc) else {
        if msg.get("result").is_some() || msg.get("error").is_some() {
            return None;
        }
        return Some(rpc_err(&id.unwrap_or(Value::Null), -32600, "Invalid Request"));
    };
    // Notifications (initialized, cancelled...): nothing to answer.
    let id = id?;
    let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));
    let answer = match method {
        "initialize" => {
            let want = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(DEFAULT_PROTOCOL);
            let version = if PROTOCOL_VERSIONS.contains(&want) { want } else { DEFAULT_PROTOCOL };
            rpc_ok(&id, json!({
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "recomplauncher-test-control", "title": "RecompLauncher test control", "version": env!("CARGO_PKG_VERSION")},
                "instructions": INSTRUCTIONS,
            }))
        }
        "ping" => rpc_ok(&id, json!({})),
        "tools/list" => rpc_ok(&id, json!({"tools": tools()})),
        "tools/call" => call(ctx, &id, &params).await,
        _ => rpc_err(&id, -32601, &format!("Method not found: {method}")),
    };
    Some(answer)
}

fn text_result(structured: Value, is_error: bool) -> Value {
    let text = serde_json::to_string_pretty(&structured).unwrap_or_default();
    json!({"content": [{"type": "text", "text": text}], "structuredContent": structured, "isError": is_error})
}

fn error_result(message: &str) -> Value {
    text_result(json!({"error": message}), true)
}

/// A whole number from the arguments, kept between `min` and `max`.
fn num(args: &Value, key: &str, default: u64, min: u64, max: u64) -> u64 {
    args.get(key).and_then(Value::as_u64).unwrap_or(default).clamp(min, max)
}

async fn call(ctx: &Ctx, id: &Value, params: &Value) -> Value {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return rpc_err(id, -32602, "Invalid params: `name` is required");
    };
    let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
    if !args.is_object() {
        return rpc_err(id, -32602, "Invalid params: `arguments` must be an object");
    }
    let app = &ctx.app;
    let in_page = |op: &'static str, wait: Duration| {
        let a = args.clone();
        async move { page::run(app, op, &a, None, wait).await }
    };
    let answer: Result<Value, String> = match name {
        "launcher_status" => Ok(status(app)),
        "ui_snapshot" => in_page("snapshot", STEP_WAIT).await,
        "ui_click" => in_page("click", STEP_WAIT).await,
        "ui_type" => in_page("type", STEP_WAIT).await,
        "ui_key" => in_page("key", STEP_WAIT).await,
        "ui_navigate" => in_page("navigate", STEP_WAIT).await,
        "ui_wait_for" => {
            let wait = Duration::from_millis(num(&args, "timeout_ms", 10_000, 100, 120_000)) + STEP_WAIT;
            in_page("wait", wait).await
        }
        "ui_eval" => {
            let Some(code) = args.get("code").and_then(Value::as_str) else {
                return rpc_ok(id, error_result("`code` is required."));
            };
            let wait = Duration::from_millis(num(&args, "timeout_ms", 20_000, 100, MAX_WAIT_MS));
            let passed = args.get("args").cloned().unwrap_or_else(|| json!({}));
            page::run(app, "eval", &passed, Some(code), wait).await
        }
        "launcher_command" => {
            let wait = Duration::from_millis(num(&args, "timeout_ms", 60_000, 100, MAX_WAIT_MS));
            in_page("command", wait).await
        }
        "recent_events" => in_page("events", STEP_WAIT).await,
        "controller_press" => press(app, &args).await,
        "read_logs" => logs(app, &args).await,
        "launcher_window" => window(app, &args),
        "screenshot" => {
            let screen = args.get("screen").and_then(Value::as_bool).unwrap_or(false);
            let width = num(&args, "max_width", 1280, 200, 3840) as u32;
            return rpc_ok(id, screenshot(app, screen, width));
        }
        _ => return rpc_err(id, -32602, &format!("Unknown tool: {name}")),
    };
    let result = match answer {
        Ok(v) => text_result(v, false),
        Err(e) => error_result(&e),
    };
    rpc_ok(id, result)
}

fn status(app: &tauri::AppHandle) -> Value {
    let st = app.state::<crate::App>();
    let games: Vec<crate::library::Installed> = st.lib.lock().unwrap().installed.values().cloned().collect();
    let installed: Vec<Value> = games
        .iter()
        .map(|g| json!({"id": g.id, "name": g.name, "version": g.version, "tag": g.tag, "repo": g.repo, "folder": g.path, "folder_found": g.path.exists()}))
        .collect();
    let pids = st.pids.lock().unwrap().clone();
    let started = st.started.lock().unwrap().clone();
    let mut running: Vec<Value> = st.running.lock().unwrap().keys().map(|id| json!({"id": id, "pid": pids.get(id), "started": started.get(id)})).collect();
    running.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    let mut downloads: Vec<String> = st.jobs.lock().unwrap().keys().cloned().collect();
    downloads.sort();
    let setups: Vec<Value> = crate::SETUP_STATUS
        .lock()
        .unwrap()
        .as_ref()
        .map(|m| m.values().map(|e| json!({"id": e.id, "step": e.step, "pct": e.pct, "text": e.text})).collect())
        .unwrap_or_default();
    let window = app.get_webview_window("main").map(|w| {
        json!({"visible": w.is_visible().unwrap_or(false), "minimized": w.is_minimized().unwrap_or(false), "focused": w.is_focused().unwrap_or(false)})
    });
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "os": crate::assets::os_now(),
        "data_folder": st.dir,
        "window": window,
        "installed": installed,
        "running": running,
        "downloads": downloads,
        "setups": setups,
    })
}

async fn press(app: &tauri::AppHandle, args: &Value) -> Result<Value, String> {
    let button = args.get("button").and_then(Value::as_str).unwrap_or("");
    if !PAD_BUTTONS.contains(&button) {
        return Err(format!("No button called {button:?}. Buttons: {}.", PAD_BUTTONS.join(", ")));
    }
    let kind = args.get("kind").and_then(Value::as_str).unwrap_or("xbox");
    let times = num(args, "times", 1, 1, 20);
    if app.get_webview_window("main").is_none() {
        return Err("The launcher's window isn't there.".into());
    }
    for i in 0..times {
        if i > 0 {
            tokio::time::sleep(Duration::from_millis(180)).await;
        }
        app.emit_to("main", "pad", json!({"b": button, "kind": kind})).map_err(|e| e.to_string())?;
    }
    // Long enough for the highlight to move and a page to start drawing.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let page = page::run(app, "ping", &json!({}), None, STEP_WAIT).await.ok();
    Ok(json!({"pressed": button, "times": times, "page": page.and_then(|p| p.get("page").cloned())}))
}

/// The last `n` lines of a log's text.
pub fn last_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// The newest part of a file (a log can be large; the end is what matters).
fn read_end(path: &std::path::Path) -> Result<String, String> {
    use std::io::{Read, Seek, SeekFrom};
    const MAX: u64 = 1024 * 1024;
    let mut f = std::fs::File::open(path).map_err(|e| format!("Couldn't open {}: {e}", path.display()))?;
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = f.seek(SeekFrom::Start(len.saturating_sub(MAX)));
    let mut b = Vec::new();
    f.read_to_end(&mut b).map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&b).to_string())
}

async fn logs(app: &tauri::AppHandle, args: &Value) -> Result<Value, String> {
    let n = num(args, "lines", 200, 1, MAX_LOG_LINES as u64) as usize;
    let dir = app.state::<crate::App>().dir.clone();
    match args.get("log").and_then(Value::as_str).unwrap_or("") {
        "launcher" => {
            let path = dir.join("launcher-trail.log");
            Ok(json!({"file": path, "text": last_lines(&read_end(&path)?, n)}))
        }
        "game" => {
            let Some(id) = args.get("id").and_then(Value::as_str).filter(|s| !s.is_empty()) else {
                return Err("A game's log needs the game's id.".into());
            };
            let path = crate::game_output_path(&dir, id);
            if !path.exists() {
                return Err(format!("No output kept for {id} (it hasn't started since it was installed, or printed nothing)."));
            }
            Ok(json!({"file": path, "text": last_lines(&read_end(&path)?, n)}))
        }
        "console" => page::run(app, "logs", &json!({"lines": n}), None, STEP_WAIT).await,
        other => Err(format!("No log called {other:?}: launcher, game or console.")),
    }
}

fn window(app: &tauri::AppHandle, args: &Value) -> Result<Value, String> {
    let Some(w) = app.get_webview_window("main") else {
        return Err("The launcher's window isn't there.".into());
    };
    match args.get("action").and_then(Value::as_str).unwrap_or("") {
        "show" => crate::show_main(app),
        "minimize" => w.minimize().map_err(|e| e.to_string())?,
        other => return Err(format!("No window action {other:?}: show or minimize.")),
    }
    Ok(json!({"visible": w.is_visible().unwrap_or(false), "minimized": w.is_minimized().unwrap_or(false), "focused": w.is_focused().unwrap_or(false)}))
}

/// A picture as PNG, scaled down to `max_width` when it's wider.
#[cfg_attr(not(windows), allow(dead_code))]
fn png(img: image::RgbaImage, max_width: u32) -> Result<Vec<u8>, String> {
    let img = if img.width() > max_width {
        let h = ((img.height() as u64 * max_width as u64) / img.width().max(1) as u64).max(1) as u32;
        image::imageops::resize(&img, max_width, h, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img).write_to(&mut out, image::ImageFormat::Png).map_err(|e| format!("Couldn't make the picture: {e}"))?;
    Ok(out.into_inner())
}

#[cfg(windows)]
fn capture(app: &tauri::AppHandle, screen: bool, max_width: u32) -> Result<Vec<u8>, String> {
    use xcap::Monitor;
    let fail = |e: xcap::XCapError| format!("Couldn't take the screenshot: {e}");
    if screen {
        let monitors = Monitor::all().map_err(fail)?;
        let main = monitors.into_iter().find(|m| m.is_primary().unwrap_or(false)).ok_or("No screen to capture.")?;
        return png(main.capture_image().map_err(fail)?, max_width);
    }
    // xcap's window list leaves out the windows of the process asking (reading their titles from
    // their own process can hang), so the launcher's window is never in it. Its part of the screen
    // is taken instead, from where the window says it is.
    let Some(w) = app.get_webview_window("main") else {
        return Err("The launcher's window isn't there.".into());
    };
    let shown = w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(true);
    if !shown {
        return Err("The launcher's window is minimized or hidden: show it first (launcher_window).".into());
    }
    let pos = w.inner_position().map_err(|e| e.to_string())?;
    let size = w.inner_size().map_err(|e| e.to_string())?;
    let (wx, wy, ww, wh) = (pos.x, pos.y, size.width as i32, size.height as i32);
    let m = Monitor::from_point(wx + ww / 2, wy + wh / 2).map_err(fail)?;
    let (mx, my) = (m.x().map_err(fail)?, m.y().map_err(fail)?);
    let (mw, mh) = (m.width().map_err(fail)? as i32, m.height().map_err(fail)? as i32);
    let Some((x, y, cw, ch)) = clip((wx, wy, ww, wh), (mx, my, mw, mh)) else {
        return Err("The launcher's window is off the screen.".into());
    };
    png(m.capture_region(x, y, cw, ch).map_err(fail)?, max_width)
}

/// The part of a window (x, y, width, height on the desktop) on its screen, in that screen's own
/// coordinates. None when none of it is on the screen.
#[cfg_attr(not(windows), allow(dead_code))]
fn clip(win: (i32, i32, i32, i32), screen: (i32, i32, i32, i32)) -> Option<(u32, u32, u32, u32)> {
    let (wx, wy, ww, wh) = win;
    let (sx, sy, sw, sh) = screen;
    let left = wx.max(sx);
    let top = wy.max(sy);
    let right = (wx + ww).min(sx + sw);
    let bottom = (wy + wh).min(sy + sh);
    if right <= left || bottom <= top {
        return None;
    }
    Some(((left - sx) as u32, (top - sy) as u32, (right - left) as u32, (bottom - top) as u32))
}

#[cfg(not(windows))]
fn capture(_app: &tauri::AppHandle, _screen: bool, _max_width: u32) -> Result<Vec<u8>, String> {
    Err("Screenshots work on Windows only for now. On Linux or a Mac, use the system's own screenshot tool, or ui_snapshot to read the window.".into())
}

fn screenshot(app: &tauri::AppHandle, screen: bool, max_width: u32) -> Value {
    use base64::Engine;
    match capture(app, screen, max_width) {
        Ok(bytes) => json!({"content": [{"type": "image", "data": base64::engine::general_purpose::STANDARD.encode(bytes), "mimeType": "image/png"}], "isError": false}),
        Err(e) => error_result(&e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_a_closed_object_schema() {
        for t in tools().as_array().unwrap() {
            let s = &t["inputSchema"];
            assert_eq!(s["type"], "object", "{}", t["name"]);
            assert_eq!(s["additionalProperties"], false, "{}", t["name"]);
        }
    }

    #[test]
    fn tool_names_are_unique() {
        let names: Vec<String> = tools().as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        let set: std::collections::BTreeSet<&String> = names.iter().collect();
        assert_eq!(set.len(), names.len());
    }

    #[test]
    fn last_lines_keeps_the_end() {
        assert_eq!(last_lines("a\nb\nc\n", 2), "b\nc");
        assert_eq!(last_lines("a", 5), "a");
        assert_eq!(last_lines("", 5), "");
    }

    #[test]
    fn a_wide_picture_is_scaled_down() {
        let img = image::RgbaImage::new(2000, 1000);
        let bytes = png(img, 1000).unwrap();
        let back = image::load_from_memory(&bytes).unwrap();
        assert_eq!((back.width(), back.height()), (1000, 500));
    }

    #[test]
    fn a_window_inside_its_screen_is_taken_whole() {
        assert_eq!(clip((100, 50, 800, 600), (0, 0, 1920, 1080)), Some((100, 50, 800, 600)));
    }

    #[test]
    fn a_window_on_a_second_screen_is_in_that_screens_coordinates() {
        assert_eq!(clip((2000, 100, 800, 600), (1920, 0, 2560, 1440)), Some((80, 100, 800, 600)));
    }

    #[test]
    fn a_window_partly_off_its_screen_is_cut_to_it() {
        assert_eq!(clip((-50, -20, 800, 600), (0, 0, 1920, 1080)), Some((0, 0, 750, 580)));
    }

    #[test]
    fn a_window_off_the_screen_gives_nothing() {
        assert_eq!(clip((3000, 0, 800, 600), (0, 0, 1920, 1080)), None);
    }

    #[test]
    fn a_small_picture_keeps_its_size() {
        let bytes = png(image::RgbaImage::new(300, 200), 1280).unwrap();
        let back = image::load_from_memory(&bytes).unwrap();
        assert_eq!((back.width(), back.height()), (300, 200));
    }
}

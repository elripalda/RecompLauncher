//! What the control API can call: every command registered with `tauri::generate_handler!`, as
//! read from the source at build time (build/control_registry.rs), plus a few notes the source
//! can't say by itself (which ones only read, which report progress by event, which can be
//! cancelled and how). The notes are checked against the registry by the tests.

use serde_json::{json, Map, Value};
use std::sync::OnceLock;

#[derive(Debug)]
pub struct CommandSpec {
    /// The name the command is invoked by.
    pub name: &'static str,
    /// Where it's registered (`plugins::plugin_list`).
    pub path: &'static str,
    /// The source file it's in.
    pub module: &'static str,
    pub doc: &'static str,
    pub is_async: bool,
    /// What Tauri fills in itself (State, AppHandle, WebviewWindow...).
    pub injected: &'static [&'static str],
    pub params: &'static [ParamSpec],
    pub returns: &'static str,
    /// False when the build couldn't find the function (its parameters are then unknown).
    pub signature_found: bool,
}

#[derive(Debug)]
pub struct ParamSpec {
    /// The key it's passed as (Tauri's lowerCamelCase of the Rust name).
    pub key: &'static str,
    pub rust_name: &'static str,
    pub rust_type: &'static str,
    pub required: bool,
    /// JSON Schema, as JSON text.
    pub schema: &'static str,
}

/// The real app's commands and events.
pub mod generated {
    use super::{CommandSpec, ParamSpec};
    include!(concat!(env!("OUT_DIR"), "/control_registry.rs"));
}

/// An operation that reports progress through an event.
#[derive(Debug, Clone, Copy)]
pub struct Track {
    pub event: &'static str,
    /// The payload field naming what it's about, compared with the call's `arg`. Empty: any.
    pub key_field: &'static str,
    pub arg: &'static str,
    /// The payload field with the step/phase/state.
    pub state_field: &'static str,
    pub done: &'static [&'static str],
    pub failed: &'static [&'static str],
    pub cancelled: &'static [&'static str],
    /// The payload field with a message when it fails.
    pub message_field: &'static str,
    /// The command returns at once and the work goes on in the background: the job ends with the
    /// event. Otherwise the job ends when the command returns, and events are only progress.
    pub completes_on_event: bool,
    /// For background work: the field of `get_state` listing what's still going (by `arg`). Work
    /// no longer listed there, with no closing event, is taken as finished.
    pub active_in: &'static str,
    /// The payload field naming one run of the work, compared with what the command answered (a
    /// run number, say). Empty: events are told apart by `key_field` only. With it, a newer run
    /// for the same key (the same game set up again) doesn't end or report into an older job.
    pub run_field: &'static str,
}

/// How a job is cancelled: another registered command, given the same `arg`. The cancel is sent
/// only once the job's command has answered (asked earlier, it waits for that): before then its
/// work may not have started (the command can be waiting for a lock), so a cancel would find
/// nothing to stop and the work would start after it. So a cancellable command must answer as
/// soon as its work is under way (install, setup_copy and account_start do).
#[derive(Debug, Clone, Copy)]
pub struct Cancel {
    pub command: &'static str,
    /// The argument copied from the job's call (empty: none).
    pub arg: &'static str,
    /// The cancel command's argument given the job's run (the track's `run_field` value the
    /// command answered with), so the cancel stops that run only, never a newer one. Empty: none.
    pub run_arg: &'static str,
    /// The work stops without a closing event: the job is marked cancelled once the cancel is in.
    pub immediate: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Note {
    pub read_only: bool,
    pub track: Option<Track>,
    pub cancel: Option<Cancel>,
    pub remark: &'static str,
}

/// A set of commands and events the API serves (the app's, or a test's).
pub struct Registry {
    pub commands: &'static [CommandSpec],
    pub events: &'static [&'static str],
    pub read_only: &'static [&'static str],
    pub tracks: &'static [(&'static str, Track)],
    pub cancels: &'static [(&'static str, Cancel)],
    pub remarks: &'static [(&'static str, &'static str)],
    schemas: OnceLock<Vec<Value>>,
}

impl Registry {
    pub const fn new(
        commands: &'static [CommandSpec],
        events: &'static [&'static str],
        read_only: &'static [&'static str],
        tracks: &'static [(&'static str, Track)],
        cancels: &'static [(&'static str, Cancel)],
        remarks: &'static [(&'static str, &'static str)],
    ) -> Self {
        Registry { commands, events, read_only, tracks, cancels, remarks, schemas: OnceLock::new() }
    }

    /// The events the API keeps: the ones the source emits, and its own change notice.
    pub fn listened_events(&self) -> Vec<&'static str> {
        let mut v: Vec<&'static str> = self.events.to_vec();
        if !v.contains(&super::CHANGED_EVENT) {
            v.push(super::CHANGED_EVENT);
        }
        v
    }

    pub fn find(&self, name: &str) -> Option<&'static CommandSpec> {
        self.commands.iter().find(|c| c.name == name)
    }

    pub fn note(&self, name: &str) -> Note {
        Note {
            read_only: self.read_only.contains(&name),
            track: self.tracks.iter().find(|(n, _)| *n == name).map(|(_, t)| *t),
            cancel: self.cancels.iter().find(|(n, _)| *n == name).map(|(_, c)| *c),
            remark: self.remarks.iter().find(|(n, _)| *n == name).map(|(_, r)| *r).unwrap_or(""),
        }
    }

    /// Each command's arguments as one JSON Schema object (same order as `commands`).
    pub fn input_schemas(&self) -> &Vec<Value> {
        self.schemas.get_or_init(|| self.commands.iter().map(input_schema).collect())
    }

    pub fn input_schema(&self, name: &str) -> Option<&Value> {
        let i = self.commands.iter().position(|c| c.name == name)?;
        self.input_schemas().get(i)
    }

    /// The machine-readable description of one command.
    pub fn describe(&self, c: &CommandSpec) -> Value {
        let n = self.note(c.name);
        let mut v = json!({
            "name": c.name,
            "path": c.path,
            "module": c.module,
            "doc": c.doc,
            "async": c.is_async,
            "injected": c.injected,
            "returns": c.returns,
            "read_only": n.read_only,
            "params": c.params.iter().map(|p| json!({"key": p.key, "rust_name": p.rust_name, "rust_type": p.rust_type, "required": p.required, "schema": parse_schema(p.schema)})).collect::<Vec<_>>(),
            "input_schema": self.input_schema(c.name).cloned().unwrap_or(Value::Null),
            "signature_found": c.signature_found,
        });
        if let Some(t) = n.track {
            v["progress"] = json!({"event": t.event, "key_field": t.key_field, "completes_on_event": t.completes_on_event});
        }
        if let Some(k) = n.cancel {
            v["cancel"] = json!({"command": k.command, "arg": k.arg, "run_arg": k.run_arg});
        }
        if !n.remark.is_empty() {
            v["remark"] = Value::String(n.remark.into());
        }
        v
    }

    /// A short line about a command for listings.
    pub fn summary(&self, c: &CommandSpec) -> Value {
        let n = self.note(c.name);
        json!({
            "name": c.name,
            "module": c.module,
            "summary": first_sentence(c.doc),
            "params": c.params.iter().map(|p| if p.required { p.key.to_string() } else { format!("{}?", p.key) }).collect::<Vec<_>>(),
            "read_only": n.read_only,
            "background": n.track.map(|t| t.completes_on_event).unwrap_or(false),
            "cancellable": n.cancel.is_some(),
        })
    }
}

fn parse_schema(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or_else(|_| json!({}))
}

fn input_schema(c: &CommandSpec) -> Value {
    let mut props = Map::new();
    let mut required = vec![];
    for p in c.params {
        let mut s = parse_schema(p.schema);
        if let Some(o) = s.as_object_mut() {
            let d = format!("Rust `{}: {}`", p.rust_name, p.rust_type);
            match o.get("description").and_then(Value::as_str) {
                Some(have) => {
                    let joined = format!("{have} ({d})");
                    o.insert("description".into(), Value::String(joined));
                }
                None => {
                    o.insert("description".into(), Value::String(d));
                }
            }
        }
        if p.required {
            required.push(Value::String(p.key.into()));
        }
        props.insert(p.key.into(), s);
    }
    let mut v = json!({"type": "object", "properties": props, "additionalProperties": !c.signature_found});
    if !required.is_empty() {
        v["required"] = Value::Array(required);
    }
    v
}

pub fn first_sentence(doc: &str) -> String {
    let d = doc.trim();
    let end = d.find(". ").map(|i| i + 1).unwrap_or(d.len());
    let s = &d[..end];
    if s.chars().count() > 200 {
        s.chars().take(197).collect::<String>() + "..."
    } else {
        s.to_string()
    }
}

/// snake_case to lowerCamelCase, the way Tauri names arguments.
pub fn camel(s: &str) -> String {
    let mut out = String::new();
    let mut up = false;
    for (i, ch) in s.chars().enumerate() {
        if ch == '_' {
            up = i > 0;
            continue;
        }
        if up {
            out.extend(ch.to_uppercase());
            up = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Checks a call's arguments against the command's parameters and puts them in the form Tauri
/// reads: snake_case keys are accepted for camelCase parameters. With `validate`, unknown keys,
/// missing required ones and plainly wrong types are refused here, with what was expected;
/// without it they go through as they are and Tauri's own argument reading answers.
pub fn normalize_args(c: &CommandSpec, args: Value, validate: bool) -> Result<Value, (String, Value)> {
    let obj = match args {
        Value::Null => Map::new(),
        Value::Object(o) => o,
        other => return Err((format!("Arguments for `{}` must be a JSON object, not {}.", c.name, kind(&other)), json!({"expected": "object"}))),
    };
    let mut out = Map::new();
    let mut unknown = vec![];
    for (k, v) in obj {
        let key = if c.params.iter().any(|p| p.key == k) {
            k
        } else {
            let cc = camel(&k);
            if c.params.iter().any(|p| p.key == cc) {
                cc
            } else {
                unknown.push(k.clone());
                k
            }
        };
        if out.contains_key(&key) {
            return Err((format!("`{key}` was given twice (as snake_case and camelCase)."), json!({"param": key})));
        }
        out.insert(key, v);
    }
    if validate && c.signature_found {
        if !unknown.is_empty() {
            return Err((
                format!("`{}` has no parameter {}.", c.name, unknown.iter().map(|u| format!("`{u}`")).collect::<Vec<_>>().join(", ")),
                json!({"unknown": unknown, "accepted": c.params.iter().map(|p| p.key).collect::<Vec<_>>()}),
            ));
        }
        // Required means given: a null that the parameter's type takes (a serde_json::Value, say,
        // as in plugin_store_set's `value`) is a real value. A null it doesn't take is refused
        // below as the wrong type.
        let missing: Vec<&str> = c.params.iter().filter(|p| p.required && !out.contains_key(p.key)).map(|p| p.key).collect();
        if !missing.is_empty() {
            return Err((format!("`{}` needs {}.", c.name, missing.iter().map(|m| format!("`{m}`")).collect::<Vec<_>>().join(", ")), json!({"missing": missing})));
        }
        for p in c.params {
            let Some(v) = out.get(p.key) else { continue };
            // An optional parameter given as null is the same as left out (Tauri reads None).
            if v.is_null() && !p.required {
                continue;
            }
            let schema = parse_schema(p.schema);
            if !type_ok(&schema, v) {
                let w = schema.get("type").map(|t| match t {
                    Value::Array(a) => a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" or "),
                    other => other.as_str().unwrap_or("").to_string(),
                });
                let w = w.unwrap_or_default();
                return Err((format!("`{}` should be {w} ({}), not {}.", p.key, p.rust_type, kind(v)), json!({"param": p.key, "expected": schema.get("type").cloned().unwrap_or(Value::Null), "rust_type": p.rust_type})));
            }
        }
    }
    Ok(Value::Object(out))
}

/// Whether a value has one of the types a (generated) schema allows: `type` as one name or a
/// list (Option<T> is [T, "null"]), or `anyOf`. No type: anything.
pub fn type_ok(schema: &Value, v: &Value) -> bool {
    let is = |t: &str| match t {
        "string" => v.is_string(),
        "boolean" => v.is_boolean(),
        "integer" => v.is_i64() || v.is_u64(),
        "number" => v.is_number(),
        "array" => v.is_array(),
        "object" => v.is_object(),
        "null" => v.is_null(),
        _ => true,
    };
    match schema.get("type") {
        Some(Value::String(t)) => is(t),
        Some(Value::Array(ts)) => ts.iter().filter_map(Value::as_str).any(is),
        _ => match schema.get("anyOf") {
            Some(Value::Array(alts)) => alts.iter().any(|s| type_ok(s, v)),
            _ => true,
        },
    }
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

// ---------- the app's notes ----------

/// Commands that only look (they may refresh a cache or reach the internet to look). After any
/// other command succeeds, the window is told to read the state again. Not here: `saves_check`,
/// which puts a newer cloud save in place of the game's saves (cloud::take_cloud) when it finds one;
/// `check_updates`, which saves the version it works out for a game added without one (its tag in
/// the Library); and `art_source`, which keeps the picture it's given in the covers folder.
pub const READ_ONLY: &[&str] = &[
    "get_state", "get_catalog", "get_releases", "net_state", "launcher_update", "launcher_notes", "libraries", "game_sizes", "backups", "pc_check",
    "rom_matches", "fs_places", "fs_list", "recent_log", "screenshots", "all_screenshots", "controllers", "versions", "build_check", "news", "game_info", "mods_state",
    "mods_online", "game_programs", "game_root", "boot_builtin", "game_setup", "looks_info", "ui_data", "account_state", "saves_info", "saves_overview",
    "download_history", "report_counts", "play_reports", "data_ids", "plugin_list", "plugin_inspect", "plugin_store_get", "plugin_library", "plugin_game_files",
    "dev_check", "release_builds", "manual_defaults", "portable_info", "is_handheld", "proton_now", "repo_readme", "github_lookup", "github_art", "art_search", "art_for",
    "art_open", "gb_mods", "gb_mod", "adopt_scan", "inbox", "betas", "troubleshoot", "look_picture",
];

#[allow(clippy::too_many_arguments)]
const fn track(event: &'static str, key_field: &'static str, arg: &'static str, state_field: &'static str, done: &'static [&'static str], failed: &'static [&'static str], cancelled: &'static [&'static str], message_field: &'static str, completes_on_event: bool) -> Track {
    Track { event, key_field, arg, state_field, done, failed, cancelled, message_field, completes_on_event, active_in: "", run_field: "" }
}

/// Operations that report progress by event (see the `emit` calls in their commands).
pub const TRACKS: &[(&str, Track)] = &[
    // install() returns at once; do_install reports `install` events with phase done/error/cancelled.
    // Its id stays in get_state.jobs while it runs.
    ("install", Track { active_in: "jobs", ..track("install", "id", "id", "phase", &["done"], &["error"], &["cancelled"], "msg", true) }),
    // setup_copy() returns its run number at once; its thread reports `setup` events with that
    // `run`. Every run ends with one closing step: done, error, or cancelled (setup_cancel, from
    // the window or a call, or a newer setup_copy for the same game replacing it).
    ("setup_copy", Track { run_field: "run", ..track("setup", "id", "id", "step", &["done"], &["error"], &["cancelled"], "text", true) }),
    ("move_game", track("move", "id", "id", "", &[], &[], &[], "", false)),
    ("gb_install", track("mod-progress", "", "", "", &[], &[], &[], "", false)),
    ("ge_proton_install", track("ge_progress", "", "", "phase", &[], &[], &[], "msg", false)),
    ("update_launcher", track("self-update", "", "", "", &[], &[], &[], "", false)),
    ("plugin_install_zip", track("plugin-progress", "", "", "phase", &[], &[], &[], "msg", false)),
    ("play", track("game", "id", "id", "state", &[], &[], &[], "", false)),
];

/// How tracked jobs are cancelled, through the app's own cancel commands.
pub const CANCELS: &[(&str, Cancel)] = &[
    ("install", Cancel { command: "cancel", arg: "id", run_arg: "", immediate: false }),
    // Bound to the job's run: setup_cancel { id, run } stops that run only, and its own
    // `cancelled` event ends the job (nothing happens when that run has already ended).
    ("setup_copy", Cancel { command: "setup_cancel", arg: "id", run_arg: "run", immediate: false }),
    ("account_start", Cancel { command: "account_cancel", arg: "", run_arg: "", immediate: true }),
];

const SENDS: &str = "Sends data to recomplauncher.com (or another site), as the window does when the player uses it.";
const OPENS: &str = "Opens something on the desktop (a folder, a browser, a program or a window); the result says only whether it was started.";

pub const REMARKS: &[(&str, &str)] = &[
    ("usage_hello", SENDS),
    ("usage_event", SENDS),
    ("usage_open", SENDS),
    ("run_report", SENDS),
    ("send_report", SENDS),
    ("beta_feedback", SENDS),
    ("inbox_done", SENDS),
    ("beta_join", SENDS),
    ("beta_leave", SENDS),
    ("account_start", "Starts a sign-in that the player finishes in a browser; not useful unattended."),
    ("account_poll", "Part of the browser sign-in (account_start)."),
    ("account_signout", SENDS),
    ("saves_cloud", "Turns cloud saves on or off for a game (uploads saves when signed in)."),
    ("plugin_fetch", "Fetches a URL on a plugin's behalf, within the sites the player allowed for it."),
    ("open_folder", OPENS),
    ("open_url", OPENS),
    ("open_shots", OPENS),
    ("open_mods", OPENS),
    ("open_app_control", OPENS),
    ("saves_open", OPENS),
    ("show_screenshot", OPENS),
    ("run_program", OPENS),
    ("play", "Starts the game. Progress: `game` events (state running, then stopped). Running games: get_state.running. Stop with `stop`."),
    ("stop", "Stops a running game started by the launcher."),
    ("update_launcher", "Downloads and installs a new RecompLauncher, then restarts it: this API goes away with the old process."),
    ("update_retry", "Runs the update installer the usual way and closes the launcher."),
    ("uninstall", "Removes a game from the Library; with deleteFiles true its folder is deleted too."),
    ("overlay_action", "Acts on the in-game quick menu (the overlay window)."),
    ("take_setup_asked", "Takes (and clears) the game a shortcut asked to set up; the window normally does this at start."),
    ("save_settings", "Replaces all settings: send the whole object from get_state.settings with your changes, or use rlapi_update_settings (patch_settings) for a partial change. A whole-object save made from settings read earlier overwrites changes made since."),
    ("patch_settings", "Changes only the given settings (JSON merge patch: objects merge, null removes a setting, which then goes back to its default), read, merged and saved under the app's own lock, so changes made at the same time aren't lost. Answers with the settings as saved."),
];

/// The registry the app serves.
pub static APP: Registry = Registry::new(generated::COMMANDS, generated::EVENTS, READ_ONLY, TRACKS, CANCELS, REMARKS);

/// What the API can't do, and what to use instead.
pub fn limitations() -> Value {
    json!({
        "native_dialogs": {
            "summary": "File and folder pickers and the ask/message boxes are opened by the window's JavaScript through the dialog plugin (plugin:dialog|open, ask, message). They need a person, so the API doesn't open them. Every backend command they lead to takes a path or a choice directly instead.",
            "equivalents": {
                "pick a folder to adopt (installed-game scan)": "adopt_scan (known places) or adopt_pick { path }, then adopt_add { items }",
                "pick a game's program": "set_exe { id, path } or locate_game { id, path }",
                "pick the player's game files (ROM / disc / folder)": "scan_roms, rom_matches, then setup_copy { id, path } (validates and sets up) or set_game_copy { id, path }",
                "pick an update's files": "set_update_copy { id, path }",
                "pick a library or ROM folder": "rlapi_update_settings { patch: { library_dirs / rom_dirs / library_dir } }",
                "pick a cover/background": "set_cover { id, path, hero }, art_source / art_save",
                "pick a plugin zip or folder": "plugin_inspect { path }, plugin_install { path }",
                "pick a portable target": "make_portable { target }",
                "browse files": "fs_places, fs_list { path }",
                "confirm boxes (ask)": "call the command the confirmation leads to directly",
            }
        },
        "window_only": [
            "Window chrome (dragging, resizing, maximize/minimize buttons) is the core window plugin, used by the page: the API has rlapi_app show / hide / focus / status / quit instead.",
            "Flows the page strings together (first-run setup, the setup wizard, the mod browser) are the same backend commands called in order: call them in the same order.",
            "Plugin pages (rlplug://) run in the window; their backend commands (plugin_*) are available.",
            "Keyboard, controller and Big Picture navigation are the page's; the commands they lead to are available.",
        ],
        "not_exposed": [
            "Tauri plugin commands (plugin:*) other than through the backend commands above.",
            "The IPC invoke key: the API calls commands inside the app process and never hands it out.",
        ],
        "platform": [
            "macOS and Linux only: the endpoint file (with the token) is mode 0600 in a 0700 folder.",
            "Windows: the API doesn't start (it logs why). The endpoint file can't yet be limited to this user there (no private ACL is set), and a portable data folder may be on a shared drive.",
        ],
        "settings": [
            "rlapi_update_settings / POST /v1/settings use patch_settings: each patch is read, merged and saved under the app's own lock, atomic with the app's other settings changes in this process.",
            "save_settings (the window's own save, or a call) replaces every setting with the object it's given: one made from settings read before a patch puts the old values back. The window reads the settings again after each API change (control-api-changed), which narrows but doesn't close that gap.",
        ]
    })
}

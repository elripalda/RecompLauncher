//! The control API end to end on Tauri's mock runtime: a real loopback server, the real token
//! checks, and commands dispatched through `Webview::on_message` with injected State, AppHandle
//! and window, plus checks of the registry generated from the app's own handler list.

use super::registry::{self, Cancel, CommandSpec, ParamSpec, Registry, Track, APP};
use super::*;
use std::io::{Read, Write};
use std::sync::atomic::AtomicU64;
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};

// ---------- test commands ----------

#[derive(Default)]
struct Counter(AtomicU64);

#[derive(Default)]
struct Store(Mutex<Value>);

#[derive(Default)]
struct Stops(Mutex<std::collections::HashSet<String>>);

/// Stands for the app's Library lock, which setup_copy takes before it starts its run.
#[derive(Default)]
struct Gate(Mutex<()>);

#[tauri::command]
fn echo(text: String, times: Option<u32>) -> String {
    text.repeat(times.unwrap_or(1) as usize)
}

#[tauri::command]
fn count(st: tauri::State<'_, Counter>) -> u64 {
    st.0.fetch_add(1, Ordering::SeqCst) + 1
}

#[tauri::command]
fn fail(reason: String) -> Result<(), String> {
    Err(reason)
}

#[tauri::command]
fn whoami<R: Runtime>(window: tauri::WebviewWindow<R>, app: AppHandle<R>) -> String {
    format!("{}@{}", window.label(), app.package_info().name)
}

#[tauri::command]
async fn sleepy(ms: u64) -> Result<u64, String> {
    tokio::time::sleep(Duration::from_millis(ms)).await;
    Ok(ms)
}

/// Background work like install: answers at once, reports `work` events, ends with done or cancelled.
#[tauri::command]
fn slow_job<R: Runtime>(app: AppHandle<R>, id: String, steps: u32) {
    std::thread::spawn(move || {
        for i in 0..steps {
            std::thread::sleep(Duration::from_millis(60));
            if app.state::<Stops>().0.lock().unwrap().remove(&id) {
                let _ = app.emit("work", json!({"id": id, "phase": "cancelled"}));
                return;
            }
            let _ = app.emit("work", json!({"id": id, "phase": "step", "done": i + 1, "total": steps}));
        }
        let _ = app.emit("work", json!({"id": id, "phase": "done"}));
    });
}

#[tauri::command]
fn stop_job(st: tauri::State<'_, Stops>, id: String) {
    st.0.lock().unwrap().insert(id);
}

#[tauri::command]
fn get_state(st: tauri::State<'_, Store>) -> Value {
    json!({"settings": st.0.lock().unwrap().clone(), "running": [], "jobs": []})
}

#[tauri::command]
fn save_settings(st: tauri::State<'_, Store>, settings: Value) {
    *st.0.lock().unwrap() = settings;
}

/// Like the app's: read, merge and save under the store's lock (with a pause in the middle, where
/// a read-then-save done from outside would let another change in and lose it).
#[tauri::command]
fn patch_settings(st: tauri::State<'_, Store>, patch: Value) -> Value {
    let mut g = st.0.lock().unwrap();
    let mut v = g.clone();
    std::thread::sleep(Duration::from_millis(5));
    merge_patch(&mut v, &patch);
    *g = v.clone();
    v
}

/// A required serde_json::Value parameter, like plugin_store_set's `value`: null is a value.
#[tauri::command]
fn keep(st: tauri::State<'_, Store>, key: String, value: Value) -> Value {
    let mut g = st.0.lock().unwrap();
    g["kept"] = json!({ key: value });
    g["kept"].clone()
}

// Runs told apart by number, like setup_copy: a newer run for the same id replaces the older one,
// a cancel command stops one, and every run ends with one closing event.
static RUNS: Mutex<Option<std::collections::HashMap<String, u64>>> = Mutex::new(None);
static RUN_NEXT: AtomicU64 = AtomicU64::new(1);

fn run_end(id: &str, run: Option<u64>) -> Option<u64> {
    let mut g = RUNS.lock().unwrap();
    let m = g.as_mut()?;
    let now = *m.get(id)?;
    if run.is_some_and(|r| r != now) {
        return None;
    }
    m.remove(id)
}

#[tauri::command]
fn run_job<R: Runtime>(app: AppHandle<R>, id: String, steps: u32, fail_early: Option<bool>) -> u64 {
    start_run(app, id, steps, fail_early)
}

/// Like setup_copy: takes the (Library) lock first, so its run starts only once it has it.
#[tauri::command]
fn gated_run_job<R: Runtime>(app: AppHandle<R>, gate: tauri::State<'_, Gate>, id: String, steps: u32) -> u64 {
    let _g = gate.0.lock().unwrap();
    start_run(app, id, steps, None)
}

fn start_run<R: Runtime>(app: AppHandle<R>, id: String, steps: u32, fail_early: Option<bool>) -> u64 {
    let run = RUN_NEXT.fetch_add(1, Ordering::SeqCst);
    let old = RUNS.lock().unwrap().get_or_insert_with(Default::default).insert(id.clone(), run);
    if let Some(old) = old {
        let _ = app.emit("run", json!({"id": id, "run": old, "phase": "cancelled", "msg": "replaced"}));
    }
    // Reports (and may even end) before it answers.
    let _ = app.emit("run", json!({"id": id, "run": run, "phase": "start"}));
    if fail_early == Some(true) {
        run_end(&id, Some(run));
        let _ = app.emit("run", json!({"id": id, "run": run, "phase": "error", "msg": "not there"}));
        return run;
    }
    std::thread::spawn(move || {
        for i in 0..steps {
            std::thread::sleep(Duration::from_millis(40));
            if RUNS.lock().unwrap().as_ref().and_then(|m| m.get(&id).copied()) != Some(run) {
                return;
            }
            let _ = app.emit("run", json!({"id": id, "run": run, "phase": "step", "done": i + 1}));
        }
        if run_end(&id, Some(run)).is_some() {
            let _ = app.emit("run", json!({"id": id, "run": run, "phase": "done"}));
        }
    });
    run
}

/// Like setup_cancel: with `run`, only that run.
#[tauri::command]
fn run_cancel<R: Runtime>(app: AppHandle<R>, id: String, run: Option<u64>) -> bool {
    match run_end(&id, run) {
        Some(run) => {
            let _ = app.emit("run", json!({"id": id, "run": run, "phase": "cancelled", "msg": "Cancelled."}));
            true
        }
        None => false,
    }
}

const S: &str = r#"{"type":"string"}"#;
const U: &str = r#"{"type":"integer","minimum":0}"#;

macro_rules! spec {
    ($name:expr, [$($p:expr),*]) => {
        CommandSpec { name: $name, path: $name, module: "tests", doc: "A test command. It does one thing.", is_async: false, injected: &[], params: &[$($p),*], returns: "", signature_found: true }
    };
}
const fn p(key: &'static str, schema: &'static str, required: bool) -> ParamSpec {
    ParamSpec { key, rust_name: key, rust_type: "test", required, schema }
}

static TEST_COMMANDS: &[CommandSpec] = &[
    spec!("echo", [p("text", S, true), p("times", U, false)]),
    spec!("count", []),
    spec!("fail", [p("reason", S, true)]),
    spec!("whoami", []),
    spec!("sleepy", [p("ms", U, true)]),
    spec!("slow_job", [p("id", S, true), p("steps", U, true)]),
    spec!("stop_job", [p("id", S, true)]),
    spec!("get_state", []),
    spec!("save_settings", [p("settings", r#"{"type":"object"}"#, true)]),
    spec!("patch_settings", [p("patch", r#"{"type":"object"}"#, true)]),
    spec!("keep", [p("key", S, true), p("value", "{}", true)]),
    spec!("run_job", [p("id", S, true), p("steps", U, true), p("failEarly", r#"{"type":["boolean","null"]}"#, false)]),
    spec!("gated_run_job", [p("id", S, true), p("steps", U, true)]),
    spec!("run_cancel", [p("id", S, true), p("run", r#"{"type":["integer","null"]}"#, false)]),
    // Listed but not registered with the handler: Tauri's own "not found" comes back.
    spec!("ghost", []),
];
static TEST_TRACKS: &[(&str, Track)] = &[
    ("slow_job", Track { event: "work", key_field: "id", arg: "id", state_field: "phase", done: &["done"], failed: &["error"], cancelled: &["cancelled"], message_field: "msg", completes_on_event: true, active_in: "", run_field: "" }),
    ("run_job", RUN_TRACK),
    ("gated_run_job", RUN_TRACK),
];
const RUN_TRACK: Track = Track { event: "run", key_field: "id", arg: "id", state_field: "phase", done: &["done"], failed: &["error"], cancelled: &["cancelled"], message_field: "msg", completes_on_event: true, active_in: "", run_field: "run" };
const RUN_CANCEL: Cancel = Cancel { command: "run_cancel", arg: "id", run_arg: "run", immediate: false };
static TEST_CANCELS: &[(&str, Cancel)] = &[("slow_job", Cancel { command: "stop_job", arg: "id", run_arg: "", immediate: false }), ("run_job", RUN_CANCEL), ("gated_run_job", RUN_CANCEL)];
static TEST_REG: Registry = Registry::new(TEST_COMMANDS, &["work", "run", CHANGED_EVENT], &["get_state", "whoami"], TEST_TRACKS, TEST_CANCELS, &[]);

struct Harness {
    _app: tauri::App<MockRuntime>,
    ctl: Arc<Ctl<MockRuntime>>,
    dir: PathBuf,
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn opts(dir: &Path) -> Options<MockRuntime> {
    Options::<MockRuntime> { data_dir: dir.to_path_buf(), registry: &TEST_REG, label: "main", app_version: "0.0.0-test", port: 0, show: |_| {}, hide: |_| {}, log: |_| {} }
}

fn mock_app() -> tauri::App<MockRuntime> {
    let app = mock_builder()
        .manage(Counter::default())
        .manage(Store(Mutex::new(json!({"library_dir": "/games", "ui": {"lang": "en", "ui_scale": 1.0}}))))
        .manage(Stops::default())
        .manage(Gate::default())
        .invoke_handler(tauri::generate_handler![echo, count, fail, whoami, sleepy, slow_job, stop_job, get_state, save_settings, patch_settings, keep, run_job, gated_run_job, run_cancel])
        .build(mock_context(noop_assets()))
        .expect("mock app");
    tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().expect("main window");
    app
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rl-control-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn harness(tag: &str) -> Harness {
    let app = mock_app();
    let dir = temp_dir(tag);
    let ctl = start(app.handle(), opts(&dir)).expect("control api starts");
    Harness { _app: app, ctl, dir }
}

struct Reply {
    status: u16,
    head: String,
    body: Value,
}

fn raw(port: u16, request: &[u8]) -> Reply {
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    s.write_all(request).unwrap();
    let mut out = vec![];
    let _ = s.read_to_end(&mut out);
    let text = String::from_utf8_lossy(&out).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    Reply { status, head: head.to_string(), body: serde_json::from_str(body).unwrap_or(Value::Null) }
}

impl Harness {
    fn token(&self) -> String {
        let v: Value = serde_json::from_slice(&std::fs::read(endpoint_file(&self.dir)).unwrap()).unwrap();
        v["token"].as_str().unwrap().to_string()
    }
    fn get(&self, path: &str) -> Reply {
        raw(self.ctl.port, format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\n\r\n", self.ctl.port, self.token()).as_bytes())
    }
    fn post(&self, path: &str, body: &Value) -> Reply {
        let b = body.to_string();
        raw(self.ctl.port, format!("POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{b}", self.ctl.port, self.token(), b.len()).as_bytes())
    }
    fn invoke(&self, command: &str, args: Value) -> Reply {
        self.post("/v1/invoke", &json!({"command": command, "args": args}))
    }
    fn mcp(&self, msg: Value) -> Reply {
        self.post("/mcp", &msg)
    }
}

// ---------- transport and auth ----------

#[test]
fn endpoint_file_is_private_and_requests_need_the_token() {
    let h = harness("auth");
    let ep = endpoint_file(&h.dir);
    let info: Value = serde_json::from_slice(&std::fs::read(&ep).unwrap()).unwrap();
    assert_eq!(info["schema"], ENDPOINT_SCHEMA);
    assert_eq!(info["port"], h.ctl.port);
    assert_eq!(info["pid"], std::process::id());
    assert_eq!(info["token"].as_str().unwrap().len(), 64);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&ep).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(control_dir(&h.dir)).unwrap().permissions().mode() & 0o777, 0o700);
    }
    let port = h.ctl.port;
    let tok = h.token();
    assert_eq!(h.get("/v1/status").status, 200);
    let none = raw(port, format!("GET /v1/status HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n").as_bytes());
    assert_eq!((none.status, none.body["error"]["code"].as_str()), (401, Some("unauthorized")));
    assert!(none.head.contains("WWW-Authenticate: Bearer"));
    let wrong = raw(port, format!("GET /v1/status HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {}x\r\n\r\n", &tok[..63]).as_bytes());
    assert_eq!(wrong.status, 401);
    // A page in a browser: Origin (or Sec-Fetch-*) is refused even with the token.
    let page = raw(port, format!("POST /v1/invoke HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: https://evil.example\r\nAuthorization: Bearer {tok}\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{{}}").as_bytes());
    assert_eq!((page.status, page.body["error"]["code"].as_str()), (403, Some("browser_request_forbidden")));
    let rebinding = raw(port, format!("GET /v1/status HTTP/1.1\r\nHost: attacker.example:{port}\r\nAuthorization: Bearer {tok}\r\n\r\n").as_bytes());
    assert_eq!(rebinding.body["error"]["code"], "bad_host");
    let form = raw(port, format!("POST /v1/invoke HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {tok}\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\n{{}}").as_bytes());
    assert_eq!(form.status, 415);
    let big = raw(port, format!("POST /v1/invoke HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {tok}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n", http::MAX_BODY + 1).as_bytes());
    // The whole oversized body sent anyway: the refusal still arrives (no reset before it's read).
    let mut huge = format!("POST /v1/invoke HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {tok}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n", http::MAX_BODY + 1).into_bytes();
    huge.extend(std::iter::repeat(b'x').take(http::MAX_BODY + 1));
    assert_eq!(raw(port, &huge).status, 413);
    assert_eq!(big.status, 413);
    assert!(!big.head.to_ascii_lowercase().contains("access-control"));
    assert!(h.ctl.auth_failures.load(Ordering::Relaxed) >= 2);
    assert_eq!(h.get("/v1/nothing").status, 404);
    assert_eq!(h.post("/v1/status", &json!({})).status, 405);
    // At exit the endpoint file goes.
    stop(h.ctl.app.app_handle());
    assert!(!ep.exists());
}

// ---------- dispatch through Tauri's IPC ----------

#[test]
fn commands_run_through_the_real_dispatch_with_injected_context() {
    let h = harness("dispatch");
    let r = h.invoke("echo", json!({"text": "ab", "times": 3}));
    assert_eq!((r.status, r.body["result"].as_str()), (200, Some("ababab")), "{:?}", r.body);
    // State<'_, T> injected: the same managed counter each time.
    assert_eq!(h.invoke("count", json!({})).body["result"], 1);
    assert_eq!(h.invoke("count", Value::Null).body["result"], 2);
    // WebviewWindow and AppHandle injected.
    let who = h.invoke("whoami", json!({})).body;
    assert!(who["result"].as_str().unwrap().starts_with("main@"), "{who:?}");
    // The command's own error, structured.
    let f = h.invoke("fail", json!({"reason": "disk full"}));
    assert_eq!((f.status, f.body["error"]["code"].as_str(), f.body["error"]["message"].as_str()), (422, Some("command_failed"), Some("disk full")));
    // The API's argument checks.
    let missing = h.invoke("echo", json!({}));
    assert_eq!((missing.status, missing.body["error"]["code"].as_str()), (400, Some("invalid_args")));
    assert_eq!(missing.body["error"]["details"]["missing"], json!(["text"]));
    let unknown_arg = h.invoke("echo", json!({"text": "x", "colour": 1}));
    assert_eq!(unknown_arg.body["error"]["details"]["unknown"], json!(["colour"]));
    let wrong_type = h.invoke("echo", json!({"text": 5}));
    assert_eq!(wrong_type.body["error"]["details"]["expected"], "string");
    // Without them, Tauri's own argument reading answers (proof it reached the command's wrapper).
    let tauri_says = h.post("/v1/invoke", &json!({"command": "echo", "args": {}, "validate": false}));
    assert_eq!((tauri_says.status, tauri_says.body["error"]["code"].as_str()), (400, Some("invalid_args")), "{:?}", tauri_says.body);
    assert!(tauri_says.body["error"]["message"].as_str().unwrap().contains("text"));
    // Not a command at all / listed but not registered.
    assert_eq!(h.invoke("rm_rf", json!({})).body["error"]["code"], "unknown_command");
    let ghost = h.invoke("ghost", json!({}));
    assert_eq!((ghost.status, ghost.body["error"]["code"].as_str()), (404, Some("unknown_command")), "{:?}", ghost.body);
}

#[test]
fn long_calls_become_jobs_and_background_work_follows_events() {
    let h = harness("jobs");
    // Answers with the job when it isn't done in time, then finishes.
    let r = h.post("/v1/invoke", &json!({"command": "sleepy", "args": {"ms": 800}, "timeout_ms": 50}));
    assert_eq!((r.status, r.body["state"].as_str()), (202, Some("running")), "{:?}", r.body);
    let id = r.body["job_id"].as_str().unwrap().to_string();
    let done = h.get(&format!("/v1/jobs/{id}?wait_ms=10000"));
    assert_eq!((done.body["job"]["state"].as_str(), done.body["job"]["result"].as_u64()), (Some("succeeded"), Some(800)));
    // Background work: the command answers at once, the job ends with its event.
    let r = h.post("/v1/invoke", &json!({"command": "slow_job", "args": {"id": "game-a", "steps": 3}, "async": true}));
    let id = r.body["job_id"].as_str().unwrap().to_string();
    let j = h.get(&format!("/v1/jobs/{id}?wait_ms=10000")).body["job"].clone();
    assert_eq!(j["state"], "succeeded", "{j:?}");
    assert_eq!(j["progress"]["phase"], "done");
    assert!(j["recent_events"].as_array().unwrap().len() >= 3);
    // Events: numbered, filterable, long-polled.
    let ev = h.get("/v1/events?after=0&names=work").body;
    assert!(ev["events"].as_array().unwrap().iter().all(|e| e["name"] == "work"));
    assert!(ev["events"].as_array().unwrap().iter().any(|e| e["payload"]["phase"] == "done"));
    let after = ev["last_seq"].as_u64().unwrap();
    let t = std::time::Instant::now();
    let empty = h.get(&format!("/v1/events?after={after}&names=work&wait_ms=300")).body;
    assert!(empty["events"].as_array().unwrap().is_empty());
    assert!(t.elapsed() >= Duration::from_millis(250));
    // A change was announced to the window for the commands that change things.
    let changed = h.get(&format!("/v1/events?after=0&names={CHANGED_EVENT}")).body;
    assert!(changed["events"].as_array().unwrap().iter().any(|e| e["payload"]["command"] == "slow_job"));
    // Cancel through the app's own cancel command.
    let r = h.post("/v1/invoke", &json!({"command": "slow_job", "args": {"id": "game-b", "steps": 200}, "async": true}));
    let id = r.body["job_id"].as_str().unwrap().to_string();
    std::thread::sleep(Duration::from_millis(150));
    let c = h.post(&format!("/v1/jobs/{id}/cancel"), &json!({}));
    assert_eq!(c.status, 200, "{:?}", c.body);
    let j = h.get(&format!("/v1/jobs/{id}?wait_ms=10000")).body["job"].clone();
    assert_eq!(j["state"], "cancelled", "{j:?}");
    // Finished: can't be cancelled again; plain commands can't be cancelled at all.
    assert_eq!(h.post(&format!("/v1/jobs/{id}/cancel"), &json!({})).body["error"]["code"], "already_finished");
    let r = h.post("/v1/invoke", &json!({"command": "sleepy", "args": {"ms": 400}, "async": true}));
    let c = h.post(&format!("/v1/jobs/{}/cancel", r.body["job_id"].as_str().unwrap()), &json!({}));
    assert_eq!((c.status, c.body["error"]["code"].as_str()), (409, Some("not_cancellable")));
    assert_eq!(h.get("/v1/jobs/job-999999").status, 404);
    assert!(h.get("/v1/jobs").body["jobs"].as_array().unwrap().len() >= 4);
}

#[test]
fn settings_patch_and_window_actions() {
    let h = harness("settings");
    let r = h.post("/v1/settings", &json!({"patch": {"ui": {"ui_scale": 1.25, "lang": null}, "close_to_tray": true}}));
    assert_eq!(r.status, 200, "{:?}", r.body);
    let st = h.invoke("get_state", json!({})).body["result"]["settings"].clone();
    assert_eq!(st, json!({"library_dir": "/games", "ui": {"ui_scale": 1.25}, "close_to_tray": true}));
    assert_eq!(h.post("/v1/settings", &json!({"patch": 3})).status, 400);
    let w = h.post("/v1/app", &json!({"action": "status"})).body;
    assert_eq!(w["window"]["label"], "main");
    assert_eq!(h.post("/v1/app", &json!({"action": "explode"})).status, 400);
    let s = h.get("/v1/status").body;
    assert_eq!(s["api"]["commands"], TEST_COMMANDS.len());
    assert_eq!(s["launcher"]["running"], json!([]));
    assert_eq!(h.get("/v1/commands/echo").body["command"]["input_schema"]["required"], json!(["text"]));
    assert_eq!(h.get("/v1/commands?q=echo").body["count"], 1);
    assert!(h.get("/v1/limitations").body["limitations"]["native_dialogs"]["equivalents"].is_object());
}

// ---------- MCP ----------

#[test]
fn mcp_initialize_list_and_call() {
    let h = harness("mcp");
    let init = h.mcp(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}}));
    assert_eq!(init.body["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init.body["result"]["serverInfo"]["name"], "recomplauncher");
    assert!(init.body["result"]["capabilities"]["tools"].is_object());
    let old = h.mcp(json!({"jsonrpc": "2.0", "id": 2, "method": "initialize", "params": {"protocolVersion": "1999-01-01"}}));
    assert_eq!(old.body["result"]["protocolVersion"], mcp::DEFAULT_PROTOCOL);
    // Notifications get no answer.
    let n = h.mcp(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    assert_eq!((n.status, n.body.clone()), (202, Value::Null));
    let list = h.mcp(json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list"})).body;
    let tools = list["result"]["tools"].as_array().unwrap();
    let echo_tool = tools.iter().find(|t| t["name"] == "rl_echo").expect("rl_echo listed");
    assert_eq!(echo_tool["inputSchema"]["required"], json!(["text"]));
    assert!(tools.iter().any(|t| t["name"] == "rlapi_job"));
    let meta = h.post("/mcp?tools=meta", &json!({"jsonrpc": "2.0", "id": 4, "method": "tools/list"})).body;
    assert!(meta["result"]["tools"].as_array().unwrap().iter().all(|t| t["name"].as_str().unwrap().starts_with("rlapi_")));
    let call = h.mcp(json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "rl_echo", "arguments": {"text": "hi"}}})).body;
    assert_eq!(call["result"]["isError"], false);
    assert_eq!(call["result"]["structuredContent"]["result"], "hi");
    let text: Value = serde_json::from_str(call["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text["result"], "hi");
    let bad = h.mcp(json!({"jsonrpc": "2.0", "id": 6, "method": "tools/call", "params": {"name": "rl_fail", "arguments": {"reason": "nope"}}})).body;
    assert_eq!(bad["result"]["isError"], true);
    assert_eq!(bad["result"]["structuredContent"]["error"]["code"], "command_failed");
    let generic = h.mcp(json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": {"name": "rlapi_invoke", "arguments": {"command": "count"}}})).body;
    assert_eq!(generic["result"]["structuredContent"]["result"], 1);
    let job = h.mcp(json!({"jsonrpc": "2.0", "id": 8, "method": "tools/call", "params": {"name": "rlapi_invoke", "arguments": {"command": "slow_job", "args": {"id": "g", "steps": 2}, "async": true}}})).body;
    let jid = job["result"]["structuredContent"]["job_id"].as_str().unwrap().to_string();
    let jw = h.mcp(json!({"jsonrpc": "2.0", "id": 9, "method": "tools/call", "params": {"name": "rlapi_job", "arguments": {"id": jid, "wait_ms": 10000}}})).body;
    assert_eq!(jw["result"]["structuredContent"]["job"]["state"], "succeeded");
    assert_eq!(h.mcp(json!({"jsonrpc": "2.0", "id": 10, "method": "tools/call", "params": {"name": "rl_nope"}})).body["error"]["code"], -32602);
    assert_eq!(h.mcp(json!({"jsonrpc": "2.0", "id": 11, "method": "resources/list"})).body["error"]["code"], -32601);
    assert_eq!(h.mcp(json!({"id": 12, "method": "ping"})).body["error"]["code"], -32600);
    let parse = raw(h.ctl.port, format!("POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: 5\r\n\r\n{{oops", h.ctl.port, h.token()).as_bytes());
    assert_eq!(parse.body["error"]["code"], -32700);
    let batch = h.mcp(json!([{"jsonrpc": "2.0", "id": 13, "method": "ping"}, {"jsonrpc": "2.0", "method": "notifications/cancelled"}])).body;
    assert_eq!(batch.as_array().unwrap().len(), 1);
}

// ---------- the app's own registry ----------

/// The handler list read straight from lib.rs text, independently of the build step.
fn handler_names_from_source() -> Vec<String> {
    let src = include_str!("../lib.rs");
    let start = src.find("tauri::generate_handler![").expect("handler list") + "tauri::generate_handler![".len();
    let end = start + src[start..].find(']').unwrap();
    src[start..end].split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).map(|s| s.rsplit("::").next().unwrap().to_string()).collect()
}

#[test]
fn the_app_registry_is_every_registered_command_with_its_parameters() {
    let from_source = handler_names_from_source();
    let names: Vec<&str> = APP.commands.iter().map(|c| c.name).collect();
    assert_eq!(names.len(), from_source.len(), "every generate_handler! entry is in the API");
    for n in &from_source {
        assert!(names.contains(&n.as_str()), "{n} is missing from the API registry");
    }
    let mut uniq = names.clone();
    uniq.sort();
    uniq.dedup();
    assert_eq!(uniq.len(), names.len(), "command names are unique");
    for c in APP.commands {
        assert!(c.signature_found, "{}: the build found its #[tauri::command] function", c.name);
        for p in c.params {
            let s: Value = serde_json::from_str(p.schema).unwrap_or_else(|_| panic!("{}.{} schema parses", c.name, p.key));
            assert!(s.is_object());
            assert!(!p.key.contains('_') || p.key.starts_with('_'), "{}.{} is camelCase as Tauri expects", c.name, p.key);
        }
        assert!(!c.injected.iter().any(|i| c.params.iter().any(|p| p.rust_type == *i)));
    }
    // A few signatures, as written in the source.
    let install = APP.find("install").unwrap();
    assert_eq!(APP.input_schema("install").unwrap()["required"], json!(["id"]));
    assert!(install.injected.iter().any(|i| i.starts_with("AppHandle")) && install.injected.iter().any(|i| i.starts_with("State")));
    assert_eq!(APP.find("uninstall").unwrap().params.iter().map(|p| p.key).collect::<Vec<_>>(), vec!["id", "deleteFiles"]);
    assert_eq!(APP.find("play").unwrap().params.iter().map(|p| p.key).collect::<Vec<_>>(), vec!["id", "replace", "bigPicture", "program"]);
    let settings = APP.input_schema("save_settings").unwrap();
    assert_eq!(settings["properties"]["settings"]["properties"]["library_dir"]["type"], "string");
    assert!(settings["properties"]["settings"]["required"].as_array().unwrap().contains(&json!("library_dir")));
    // snake_case keys are accepted for camelCase parameters, but not both at once.
    let un = APP.find("uninstall").unwrap();
    assert_eq!(registry::normalize_args(un, json!({"id": "x", "delete_files": true}), true).unwrap(), json!({"id": "x", "deleteFiles": true}));
    assert!(registry::normalize_args(un, json!({"id": "x", "delete_files": true, "deleteFiles": false}), true).is_err());
    let adopt = APP.input_schema("adopt_add").unwrap();
    assert_eq!(adopt["properties"]["items"]["items"]["required"], json!(["id", "path"]));
    // Notes refer to real commands and events.
    for n in registry::READ_ONLY.iter().chain(registry::REMARKS.iter().map(|(n, _)| n)) {
        assert!(APP.find(n).is_some(), "note for unknown command {n}");
    }
    for (n, t) in registry::TRACKS {
        assert!(APP.find(n).is_some(), "track for unknown command {n}");
        assert!(APP.events.contains(&t.event), "{n}: the source emits `{}`", t.event);
    }
    for (n, c) in registry::CANCELS {
        assert!(APP.find(n).is_some() && APP.find(c.command).is_some(), "cancel {n} -> {}", c.command);
    }
    for e in ["install", "setup", "game", "toast", "move", CHANGED_EVENT] {
        assert!(APP.listened_events().contains(&e), "event {e} is listened to");
    }
    // MCP: one tool per command plus the rlapi_* ones, valid unique names, object schemas.
    let tools = mcp::tools(&APP, false);
    assert_eq!(tools.len(), APP.commands.len() + mcp::tools(&APP, true).len());
    let mut seen = std::collections::HashSet::new();
    for t in &tools {
        let n = t["name"].as_str().unwrap();
        assert!(n.len() <= 64 && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'), "tool name {n}");
        assert!(seen.insert(n.to_string()), "duplicate tool {n}");
        assert_eq!(t["inputSchema"]["type"], "object", "{n}");
    }
}

// ---------- review regressions ----------

/// RECOMPLAUNCHER_CONTROL_API=0 keeps the API off even when `--control-api` is handed to the
/// launcher already running (the single-instance path goes through start_requested).
#[test]
fn the_environment_keeps_it_off_however_it_is_asked_for() {
    let d = temp_dir("env-off");
    for off in ["0", " off ", "FALSE", "no"] {
        assert!(!wanted_with(Some(off), &d, &["recomplauncher".into(), ARG.into()]), "{off:?}");
    }
    assert!(wanted_with(Some("1"), &d, &["recomplauncher".into()]));
    assert!(wanted_with(None, &d, &["recomplauncher".into(), ARG.into()]));
    assert!(!wanted_with(Some("maybe"), &d, &["recomplauncher".into()]));
    let app = mock_app();
    let kept = start_requested(app.handle(), opts(&d), Some("0")).expect("no error");
    assert!(kept.is_none(), "kept off");
    assert!(!endpoint_file(&d).exists(), "no endpoint file, no token written");
    assert!(app.try_state::<Arc<Ctl<MockRuntime>>>().is_none(), "nothing listening");
    let on = start_requested(app.handle(), opts(&d), None).expect("starts").expect("running");
    assert!(endpoint_file(&d).exists());
    stop(on.app.app_handle());
    let _ = std::fs::remove_dir_all(&d);
}

/// Concurrent patches through the API all land: each is merged and saved in one step in the app.
#[test]
fn concurrent_settings_patches_are_not_lost() {
    let h = harness("patch-race");
    let (port, tok) = (h.ctl.port, h.token());
    let threads: Vec<_> = (0..12)
        .map(|i| {
            let tok = tok.clone();
            std::thread::spawn(move || {
                let b = json!({"patch": {"ui": {format!("k{i}"): i}}}).to_string();
                raw(port, format!("POST /v1/settings HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {tok}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{b}", b.len()).as_bytes()).status
            })
        })
        .collect();
    for t in threads {
        assert_eq!(t.join().unwrap(), 200);
    }
    let ui = h.invoke("get_state", json!({})).body["result"]["settings"]["ui"].clone();
    for i in 0..12 {
        assert_eq!(ui[format!("k{i}")], i, "patch {i} kept: {ui}");
    }
    assert_eq!(ui["lang"], "en", "settings not in any patch kept");
}

/// The app's own patch_settings: atomic with the app's other settings writers (which change
/// settings under the same Library lock), null goes back to the default, bad changes change nothing.
#[test]
fn app_patch_settings_is_atomic_with_other_writers() {
    let dir = temp_dir("app-patch");
    let lib = Arc::new(Mutex::new(crate::library::Library::default()));
    let mut threads = vec![];
    for i in 0..16 {
        let (lib, dir) = (lib.clone(), dir.clone());
        threads.push(std::thread::spawn(move || crate::patch_settings_in(&lib, &dir, &json!({"ui": {format!("k{i}"): i}})).map(|_| ()).map_err(|e| e.to_string())));
    }
    for _ in 0..16 {
        let lib = lib.clone();
        // Like menu_tips += 1 or asked_location = true elsewhere in the app.
        threads.push(std::thread::spawn(move || {
            lib.lock().unwrap().settings.menu_tips += 1;
            Ok(())
        }));
    }
    for t in threads {
        t.join().unwrap().unwrap();
    }
    let s = lib.lock().unwrap().settings.clone();
    assert_eq!(s.menu_tips, 16, "no other writer's change lost");
    for i in 0..16 {
        assert_eq!(s.ui[format!("k{i}")], i, "patch {i} kept");
    }
    let on_disk: Value = serde_json::from_slice(&std::fs::read(dir.join("library.json")).unwrap()).unwrap();
    assert_eq!(on_disk["settings"]["ui"].as_object().unwrap().len(), 16, "the last save has every patch");
    // Option settings: set, then null takes them back to not set.
    assert_eq!(crate::patch_settings_in(&lib, &dir, &json!({"lighter": true, "minimize_playing": false})).unwrap().lighter, Some(true));
    let back = crate::patch_settings_in(&lib, &dir, &json!({"lighter": null})).unwrap();
    assert_eq!((back.lighter, back.minimize_playing), (None, Some(false)));
    // Refused changes leave everything as it was.
    let before = serde_json::to_value(&lib.lock().unwrap().settings).unwrap();
    assert!(crate::patch_settings_in(&lib, &dir, &json!({"library_dir": "relative/path"})).is_err());
    assert!(crate::patch_settings_in(&lib, &dir, &json!({"library_dir": null})).is_err(), "library_dir has no default");
    assert!(crate::patch_settings_in(&lib, &dir, &json!({"menu_tips": "lots"})).is_err());
    assert!(crate::patch_settings_in(&lib, &dir, &json!(["not", "an", "object"])).is_err());
    assert_eq!(serde_json::to_value(&lib.lock().unwrap().settings).unwrap(), before);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Required means given: a required serde_json::Value takes null; a required String doesn't;
/// an optional one given null is left out.
#[test]
fn required_null_is_a_value_when_the_type_takes_it() {
    let h = harness("nulls");
    let r = h.invoke("keep", json!({"key": "a", "value": null}));
    assert_eq!(r.status, 200, "{:?}", r.body);
    assert_eq!(r.body["result"], json!({"a": null}));
    let missing = h.invoke("keep", json!({"key": "a"}));
    assert_eq!(missing.body["error"]["details"]["missing"], json!(["value"]));
    let wrong = h.invoke("echo", json!({"text": null}));
    assert_eq!((wrong.status, wrong.body["error"]["details"]["param"].as_str()), (400, Some("text")), "{:?}", wrong.body);
    assert_eq!(h.invoke("echo", json!({"text": "x", "times": null})).body["result"], "x");
    // The app's own plugin_store_set: value null passes the API's checks; leaving it out doesn't.
    let pss = APP.find("plugin_store_set").expect("plugin_store_set registered");
    assert!(registry::normalize_args(pss, json!({"id": "p", "key": "k", "value": null}), true).is_ok());
    let err = registry::normalize_args(pss, json!({"id": "p", "key": "k"}), true).unwrap_err();
    assert_eq!(err.1["missing"], json!(["value"]));
    assert!(registry::normalize_args(pss, json!({"id": null, "key": "k", "value": 1}), true).is_err(), "a required String isn't null");
}

/// Option<T> is optional and nullable in the generated schemas, top level and inside structs.
#[test]
fn option_schemas_take_null_and_stay_optional() {
    let settings = &APP.input_schema("save_settings").unwrap()["properties"]["settings"];
    for f in ["lighter", "minimize_playing"] {
        assert_eq!(settings["properties"][f]["type"], json!(["boolean", "null"]), "{f}");
        assert!(!settings["required"].as_array().unwrap().contains(&json!(f)), "{f} stays optional");
        assert!(registry::type_ok(&settings["properties"][f], &Value::Null));
        assert!(registry::type_ok(&settings["properties"][f], &json!(true)));
        assert!(!registry::type_ok(&settings["properties"][f], &json!("yes")));
    }
    assert_eq!(settings["properties"]["library_dir"]["type"], "string", "non-Option stays as it was");
    let play = APP.input_schema("play").unwrap();
    assert_eq!(play["properties"]["program"]["type"], json!(["string", "null"]));
    assert!(!play["required"].as_array().unwrap().contains(&json!("program")));
    // get_state.settings as the app serializes it (None as null) fits save_settings's schema.
    let s = serde_json::to_value(crate::library::Settings::default()).unwrap();
    assert!(s["lighter"].is_null() && s["minimize_playing"].is_null());
    for (k, v) in s.as_object().unwrap() {
        if let Some(fs) = settings["properties"].get(k) {
            assert!(registry::type_ok(fs, v), "settings.{k} = {v} fits {fs}");
        }
    }
}

/// Runs told apart: a direct cancel (not through the job) ends the job, a newer run for the same
/// id ends the older job as cancelled without finishing or reporting into the newer one, and a
/// run that ends before its command answers still ends its job.
#[test]
fn run_identity_ends_jobs_on_cancel_replacement_and_early_end() {
    let h = harness("runs");
    // Ends before it answers.
    let r = h.post("/v1/invoke", &json!({"command": "run_job", "args": {"id": "rx-early", "steps": 1, "fail_early": true}, "timeout_ms": 5000}));
    assert_eq!((r.status, r.body["state"].as_str()), (422, Some("failed")), "{:?}", r.body);
    assert_eq!(r.body["error"]["message"], "not there");
    // Replaced by a newer run for the same id.
    let a = h.post("/v1/invoke", &json!({"command": "run_job", "args": {"id": "rx-same", "steps": 200}, "async": true})).body;
    let a_id = a["job_id"].as_str().unwrap().to_string();
    std::thread::sleep(Duration::from_millis(120));
    let b = h.post("/v1/invoke", &json!({"command": "run_job", "args": {"id": "rx-same", "steps": 3}, "timeout_ms": 10000})).body;
    assert_eq!(b["state"], "succeeded", "{b:?}");
    let a = h.get(&format!("/v1/jobs/{a_id}?wait_ms=5000")).body["job"].clone();
    assert_eq!((a["state"].as_str(), a["note"].as_str()), (Some("cancelled"), Some("replaced")), "{a:?}");
    let (ra, rb) = (a["run"].clone(), b["job"]["run"].clone());
    assert_ne!(ra, rb);
    assert!(a["recent_events"].as_array().unwrap().iter().all(|e| e["payload"]["run"] == ra), "only its own run's events: {a:?}");
    assert!(b["job"]["recent_events"].as_array().unwrap().iter().all(|e| e["payload"]["run"] == rb), "{b:?}");
    // Cancelled by calling the cancel command directly (as the window does).
    let c = h.post("/v1/invoke", &json!({"command": "run_job", "args": {"id": "rx-cancel", "steps": 200}, "async": true})).body;
    let c_id = c["job_id"].as_str().unwrap().to_string();
    std::thread::sleep(Duration::from_millis(120));
    assert_eq!(h.invoke("run_cancel", json!({"id": "rx-cancel"})).status, 200);
    let c = h.get(&format!("/v1/jobs/{c_id}?wait_ms=5000")).body["job"].clone();
    assert_eq!(c["state"], "cancelled", "{c:?}");
    // And through the job's cancel: ends with the run's cancelled event.
    let d = h.post("/v1/invoke", &json!({"command": "run_job", "args": {"id": "rx-jobcancel", "steps": 200}, "async": true})).body;
    let d_id = d["job_id"].as_str().unwrap().to_string();
    std::thread::sleep(Duration::from_millis(120));
    assert_eq!(h.post(&format!("/v1/jobs/{d_id}/cancel"), &json!({})).status, 200);
    assert_eq!(h.get(&format!("/v1/jobs/{d_id}?wait_ms=5000")).body["job"]["state"], "cancelled");
    assert_eq!(h.get("/v1/jobs").body["active"], 0, "no job left running");
}

/// Background work started through the API (setup_copy, install) changes things only when it
/// ends, after the change notice sent as its command answered: its closing event (done, error or
/// cancelled) brings another `control-api-changed`, naming the job and how it ended, so the
/// window reads the state again then.
#[test]
fn background_jobs_announce_their_end_to_the_window() {
    let h = harness("ended");
    let changed_after = |seq: u64, job: &str| -> Vec<Value> {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let ev = h.get(&format!("/v1/events?after={seq}&names={CHANGED_EVENT}&wait_ms=200")).body;
            let mine: Vec<Value> = ev["events"].as_array().unwrap().iter().filter(|e| e["payload"]["job"] == job).cloned().collect();
            if !mine.is_empty() || std::time::Instant::now() > deadline {
                return mine;
            }
        }
    };
    let closing_seq = |id: &str, phase: &str| -> u64 {
        let ev = h.get("/v1/events?after=0&names=run&limit=500").body;
        ev["events"].as_array().unwrap().iter().find(|e| e["payload"]["id"] == id && e["payload"]["phase"] == phase).and_then(|e| e["seq"].as_u64()).unwrap_or_else(|| panic!("{id} {phase}: {ev:?}"))
    };
    // Done.
    let r = h.post("/v1/invoke", &json!({"command": "run_job", "args": {"id": "end-done", "steps": 3}, "timeout_ms": 10000})).body;
    assert_eq!(r["state"], "succeeded", "{r:?}");
    let job = r["job_id"].as_str().unwrap().to_string();
    let done = closing_seq("end-done", "done");
    let after = changed_after(done, &job);
    assert_eq!(after.len(), 1, "one notice for its end: {after:?}");
    assert_eq!((after[0]["payload"]["command"].as_str(), after[0]["payload"]["state"].as_str()), (Some("run_job"), Some("succeeded")), "{after:?}");
    // Cancelled (through the job's cancel, which ends with the run's own `cancelled`).
    let d = h.post("/v1/invoke", &json!({"command": "run_job", "args": {"id": "end-cancel", "steps": 200}, "async": true})).body;
    let job = d["job_id"].as_str().unwrap().to_string();
    std::thread::sleep(Duration::from_millis(120));
    assert_eq!(h.post(&format!("/v1/jobs/{job}/cancel"), &json!({})).status, 200);
    assert_eq!(h.get(&format!("/v1/jobs/{job}?wait_ms=5000")).body["job"]["state"], "cancelled");
    let after = changed_after(closing_seq("end-cancel", "cancelled"), &job);
    assert_eq!(after.len(), 1, "{after:?}");
    assert_eq!(after[0]["payload"]["state"], "cancelled");
    // Failed before its command even answered: the notice sent as it answered already comes
    // after its end.
    let e = h.post("/v1/invoke", &json!({"command": "run_job", "args": {"id": "end-early", "steps": 1, "fail_early": true}, "timeout_ms": 5000})).body;
    assert_eq!(e["state"], "failed", "{e:?}");
    let err = closing_seq("end-early", "error");
    let notices = h.get(&format!("/v1/events?after={err}&names={CHANGED_EVENT}&wait_ms=2000")).body;
    assert!(notices["events"].as_array().unwrap().iter().any(|n| n["payload"]["command"] == "run_job"), "{notices:?}");
    // Plain commands end when they answer: only the one notice, as before.
    let before = h.get(&format!("/v1/events?after=0&names={CHANGED_EVENT}")).body["last_seq"].as_u64().unwrap();
    assert_eq!(h.invoke("count", json!({})).status, 200);
    std::thread::sleep(Duration::from_millis(150));
    let n = h.get(&format!("/v1/events?after={before}&names={CHANGED_EVENT}")).body;
    let counts: Vec<&Value> = n["events"].as_array().unwrap().iter().filter(|e| e["payload"]["command"] == "count").collect();
    assert_eq!(counts.len(), 1, "{n:?}");
}

/// The app's setup runs: numbered, a newer one replaces the older, cancel ends the current one.
#[test]
fn app_setup_runs_are_numbered_and_replaced() {
    let id = format!("rl-test-setup-{}", std::process::id());
    let (r1, old) = crate::setup_begin(&id);
    assert_eq!(old, None);
    let (r2, old) = crate::setup_begin(&id);
    assert_eq!(old, Some(r1), "the newer run names the one it replaces");
    assert!(crate::setup_stopped(&id, r1) && !crate::setup_stopped(&id, r2));
    assert_eq!(crate::setup_end(&id, Some(r1)), None, "an old run can't end the current one");
    assert_eq!(crate::setup_end(&id, None), Some(r2), "cancel ends whichever is going");
    assert!(crate::setup_stopped(&id, r2));
    assert_eq!(crate::setup_end(&id, None), None, "nothing to cancel: no cancelled event");
}

#[test]
fn turning_it_on() {
    let d = std::env::temp_dir().join(format!("rl-control-wanted-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    if std::env::var(ENV).is_err() {
        assert!(!wanted(&d, &["recomplauncher".into()]));
        assert!(wanted(&d, &["recomplauncher".into(), ARG.into()]));
        std::fs::create_dir_all(control_dir(&d)).unwrap();
        std::fs::write(control_dir(&d).join("enabled"), "").unwrap();
        assert!(wanted(&d, &["recomplauncher".into()]));
    }
    let _ = std::fs::remove_dir_all(&d);
    let mut t = json!({"a": {"b": 1, "c": 2}, "d": 1});
    merge_patch(&mut t, &json!({"a": {"b": null, "e": 3}, "d": [1]}));
    assert_eq!(t, json!({"a": {"c": 2, "e": 3}, "d": [1]}));
}

// ---------- review regressions, round 2 ----------

/// App::save (snapshot and write under the Library lock) against patch_settings and the other
/// writers that save under that lock: the file only ever moves forward (an older snapshot never
/// lands over a newer save, and two saves never mix in library.json.tmp), and ends as the Library is.
#[test]
fn app_save_and_settings_patches_persist_in_order() {
    use crate::library::{Installed, Library};
    let dir = temp_dir("app-save-order");
    let mut lib = Library::default();
    lib.settings.library_dir = dir.join("games");
    // A Library big enough that writing it takes a while (the window an old snapshot used).
    for i in 0..400 {
        let id = format!("game-{i}");
        lib.installed.insert(id.clone(), Installed { id: id.clone(), name: format!("A game with a long enough name {i}"), path: dir.join("games").join(&id), ..Default::default() });
    }
    let app = Arc::new(crate::App::for_tests(dir.clone(), lib));
    app.save();
    let file = dir.join("library.json");
    let stop = Arc::new(AtomicBool::new(false));
    // Reads the file the whole time: it must always parse, and never lose a patch it had.
    let watcher = {
        let (file, stop) = (file.clone(), stop.clone());
        std::thread::spawn(move || -> Result<usize, String> {
            let (mut most, mut reads) = (0usize, 0usize);
            while !stop.load(Ordering::SeqCst) {
                let Ok(b) = std::fs::read(&file) else { continue };
                let v: Value = serde_json::from_slice(&b).map_err(|e| format!("library.json didn't parse after {reads} reads: {e}"))?;
                let n = v["settings"]["ui"].as_object().map(|o| o.keys().filter(|k| k.starts_with('p')).count()).unwrap_or(0);
                if n < most {
                    return Err(format!("library.json went back from {most} patches to {n}: an older snapshot landed over a newer save"));
                }
                most = n;
                reads += 1;
            }
            Ok(reads)
        })
    };
    let savers: Vec<_> = (0..4)
        .map(|_| {
            let (app, stop) = (app.clone(), stop.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    app.save();
                    std::thread::sleep(Duration::from_millis(1));
                }
            })
        })
        .collect();
    let patchers: Vec<_> = (0..4)
        .map(|t| {
            let app = app.clone();
            std::thread::spawn(move || {
                let mut errors = vec![];
                for i in 0..15 {
                    if let Err(e) = crate::patch_settings_in(&app.lib, &app.dir, &json!({"ui": {format!("p{t}-{i}"): i}})) {
                        errors.push(e.to_string());
                    }
                }
                errors
            })
        })
        .collect();
    let patch_errors: Vec<String> = patchers.into_iter().flat_map(|p| p.join().unwrap()).collect();
    stop.store(true, Ordering::SeqCst);
    for s in savers {
        s.join().unwrap();
    }
    let watched = watcher.join().unwrap();
    assert!(patch_errors.is_empty(), "patches failed to save (a save shared library.json.tmp with them): {patch_errors:?}; the file meanwhile: {watched:?}");
    let reads = watched.unwrap_or_else(|e| panic!("{e}"));
    assert!(reads > 10, "the file was read while it was being saved ({reads} reads)");
    let on_disk: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    let in_memory = serde_json::to_value(&*app.lib.lock().unwrap()).unwrap();
    assert_eq!(on_disk["settings"], in_memory["settings"], "the last save is the Library as it is");
    assert_eq!(on_disk["settings"]["ui"].as_object().unwrap().len(), 60, "every patch saved");
    assert!(!dir.join("library.json.tmp").exists(), "no save left its temporary file behind");
    let _ = std::fs::remove_dir_all(&dir);
}

/// saves_check can put a newer cloud save in place of the game's saves (cloud::take_cloud), so
/// it isn't listed as read-only: no readOnlyHint for its MCP tool, read_only false in listings.
#[test]
fn saves_check_is_not_read_only() {
    assert!(!registry::READ_ONLY.contains(&"saves_check"));
    assert!(!APP.note("saves_check").read_only);
    let tools = mcp::tools(&APP, false);
    let tool = |n: &str| tools.iter().find(|t| t["name"] == format!("{}{n}", mcp::COMMAND_PREFIX)).unwrap_or_else(|| panic!("{n} listed")).clone();
    let sc = tool("saves_check");
    assert_ne!(sc["annotations"]["readOnlyHint"], json!(true), "{sc}");
    assert_eq!(APP.describe(APP.find("saves_check").unwrap())["read_only"], false);
    assert_eq!(APP.summary(APP.find("saves_check").unwrap())["read_only"], false);
    // A command that only reads still says so.
    assert_eq!(tool("get_state")["annotations"]["readOnlyHint"], true);
    assert_eq!(tool("saves_info")["annotations"]["readOnlyHint"], true);
}

/// check_updates saves the version it works out for a game added without one, and art_source
/// keeps the picture it's given in the covers folder: neither is listed as read-only (no
/// readOnlyHint for their MCP tools, read_only false in listings), so the window is told after them.
#[test]
fn commands_that_write_are_not_read_only() {
    let tools = mcp::tools(&APP, false);
    let tool = |n: &str| tools.iter().find(|t| t["name"] == format!("{}{n}", mcp::COMMAND_PREFIX)).unwrap_or_else(|| panic!("{n} listed")).clone();
    for name in ["check_updates", "art_source", "saves_check"] {
        assert!(!registry::READ_ONLY.contains(&name), "{name}");
        assert!(!APP.note(name).read_only, "{name}");
        let t = tool(name);
        assert_ne!(t["annotations"]["readOnlyHint"], json!(true), "{name}: {t}");
        assert_eq!(APP.describe(APP.find(name).unwrap())["read_only"], false, "{name}");
        assert_eq!(APP.summary(APP.find(name).unwrap())["read_only"], false, "{name}");
    }
    // Their neighbours that only read still say so.
    for name in ["art_open", "art_for", "get_releases", "launcher_update"] {
        assert_eq!(tool(name)["annotations"]["readOnlyHint"], true, "{name}");
    }
}

/// A cancel asked for at once, while the command is still waiting for the lock it needs before
/// it starts its run: it isn't sent then (it would find nothing to stop and the run would start
/// after it); the job is `cancelling`, the answer says pending, and the cancel goes in as soon as
/// the command answers, bound to its run. The run never finishes, and the job ends cancelled.
#[test]
fn a_cancel_before_the_command_answers_waits_for_its_run() {
    let h = harness("cancel-early");
    let gate = h._app.state::<Gate>();
    let held = gate.0.lock().unwrap();
    let r = h.post("/v1/invoke", &json!({"command": "gated_run_job", "args": {"id": "gx", "steps": 6}, "async": true})).body;
    let id = r["job_id"].as_str().unwrap().to_string();
    // No pause: the command is still waiting for the gate.
    let c = h.post(&format!("/v1/jobs/{id}/cancel"), &json!({}));
    assert_eq!((c.status, c.body["pending"].as_bool()), (202, Some(true)), "{:?}", c.body);
    assert_eq!(c.body["job"]["state"], "cancelling", "not claimed cancelled before it is: {:?}", c.body);
    assert_eq!(c.body["job"]["returned"], false);
    // Asking again doesn't send a second one.
    let again = h.post(&format!("/v1/jobs/{id}/cancel"), &json!({}));
    assert_eq!((again.status, again.body["pending"].as_bool()), (202, Some(true)), "{:?}", again.body);
    // Nothing was stopped yet: no run is going for gx.
    assert!(RUNS.lock().unwrap().as_ref().map(|m| !m.contains_key("gx")).unwrap_or(true));
    drop(held);
    let j = h.get(&format!("/v1/jobs/{id}?wait_ms=10000")).body["job"].clone();
    assert_eq!(j["state"], "cancelled", "{j:?}");
    let run = j["run"].clone();
    assert!(run.is_u64(), "{j:?}");
    // Give a run that wasn't stopped the time it needs to finish (6 steps of 40 ms), then look:
    // its events end with its cancel, and it never got to done.
    std::thread::sleep(Duration::from_millis(500));
    let ev = h.get("/v1/events?after=0&names=run&limit=500").body;
    let mine: Vec<&Value> = ev["events"].as_array().unwrap().iter().map(|e| &e["payload"]).filter(|p| p["id"] == "gx" && p["run"] == run).collect();
    assert!(mine.iter().all(|p| p["phase"] != "done"), "the run never finished: {mine:?}");
    assert_eq!(mine.iter().filter(|p| p["phase"] == "cancelled").count(), 1, "one cancel, for its own run: {mine:?}");
    assert!(RUNS.lock().unwrap().as_ref().map(|m| !m.contains_key("gx")).unwrap_or(true), "no run left going");
    assert_eq!(h.get("/v1/jobs").body["active"], 0);
}

/// The job's cancel is bound to its run: the cancel command gets the run the job's command
/// answered with, and a cancel for a run that has ended doesn't stop a newer one.
#[test]
fn job_cancels_are_bound_to_their_run() {
    let h = harness("cancel-bound");
    let returned_run = |id: &str| {
        for _ in 0..200 {
            let j = h.get(&format!("/v1/jobs/{id}")).body["job"].clone();
            if j["returned"] == true {
                return j["run"].clone();
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("{id} never answered");
    };
    let a = h.post("/v1/invoke", &json!({"command": "run_job", "args": {"id": "bx", "steps": 200}, "async": true})).body;
    let a_id = a["job_id"].as_str().unwrap().to_string();
    let a_run = returned_run(&a_id);
    // A newer run for the same id: A is replaced.
    let b = h.post("/v1/invoke", &json!({"command": "run_job", "args": {"id": "bx", "steps": 200}, "async": true})).body;
    let b_id = b["job_id"].as_str().unwrap().to_string();
    let b_run = returned_run(&b_id);
    assert_ne!(a_run, b_run);
    assert_eq!(h.get(&format!("/v1/jobs/{a_id}?wait_ms=5000")).body["job"]["state"], "cancelled");
    // A cancel for A's run (now ended) is refused by the command and leaves B's run going.
    let stale = h.invoke("run_cancel", json!({"id": "bx", "run": a_run}));
    assert_eq!(stale.body["result"], false, "{:?}", stale.body);
    assert_eq!(RUNS.lock().unwrap().as_ref().unwrap().get("bx").copied(), b_run.as_u64(), "B's run still going");
    // B's job is cancelled through its own cancel, which names B's run.
    let c = h.post(&format!("/v1/jobs/{b_id}/cancel"), &json!({}));
    assert_eq!((c.status, c.body["pending"].as_bool()), (200, Some(false)), "{:?}", c.body);
    let j = h.get(&format!("/v1/jobs/{b_id}?wait_ms=5000")).body["job"].clone();
    assert_eq!(j["state"], "cancelled", "{j:?}");
    assert_eq!(j["progress"]["run"], b_run);
    // The app's own setup_copy cancel is the same: run-bound, with setup_cancel's `run`.
    let c = APP.note("setup_copy").cancel.unwrap();
    assert_eq!((c.command, c.arg, c.run_arg), ("setup_cancel", "id", "run"));
    assert!(APP.find("setup_cancel").unwrap().params.iter().any(|p| p.key == "run" && !p.required));
    // Every cancel here is sent once its command has answered: each cancellable command answers
    // as soon as its work is under way, and run-bound ones name their run.
    for (n, c) in registry::CANCELS {
        let t = APP.note(n).track;
        assert!(c.immediate || t.is_some_and(|t| t.completes_on_event), "{n}: its job ends with the work's closing event or at once");
        assert!(c.run_arg.is_empty() || t.is_some_and(|t| !t.run_field.is_empty()), "{n}: a run-bound cancel needs the run");
    }
}

// ---------- the app's setup runs: finishing ----------

struct SetupFixture {
    dir: PathBuf,
    id: String,
    lib: Mutex<crate::library::Library>,
    g: crate::library::Installed,
    p: crate::profile::Profile,
    copy: PathBuf,
}

static SETUP_FIXTURES: AtomicU64 = AtomicU64::new(0);

fn setup_fixture(tag: &str) -> SetupFixture {
    let n = SETUP_FIXTURES.fetch_add(1, Ordering::SeqCst);
    let dir = temp_dir(&format!("setup-{tag}-{n}"));
    let id = format!("rl-test-setup-{tag}-{n}-{}", std::process::id());
    let install = dir.join("install");
    let copy = dir.join("copy");
    std::fs::create_dir_all(&install).unwrap();
    std::fs::create_dir_all(copy.join("sub")).unwrap();
    std::fs::write(copy.join("game.bin"), vec![7u8; 4096]).unwrap();
    std::fs::write(copy.join("sub").join("data.bin"), vec![9u8; 1024]).unwrap();
    // A settings file the setup writes, with where the copy is.
    let p: crate::profile::Profile = serde_json::from_value(json!({"config": [{"format": "json", "file": [{"path": "{install}/settings.json"}], "game_path": "game"}]})).unwrap();
    let g = crate::library::Installed { id: id.clone(), name: "Test".into(), path: install, ..Default::default() };
    let mut lib = crate::library::Library::default();
    lib.settings.library_dir = dir.clone();
    lib.installed.insert(id.clone(), g.clone());
    SetupFixture { dir, id, lib: Mutex::new(lib), g, p, copy }
}

impl SetupFixture {
    fn game_copy(&self) -> Option<PathBuf> {
        self.lib.lock().unwrap().installed[&self.id].game_copy.clone()
    }
    fn saved_copy(&self) -> Value {
        std::fs::read(self.dir.join("library.json")).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).map(|v| v["installed"][&self.id]["game_copy"].clone()).unwrap_or(Value::Null)
    }
    fn settings_written(&self) -> bool {
        self.g.path.join("settings.json").exists()
    }
    /// Runs setup_copy's thread (setup_thread: the real run, and its recovery when it panics)
    /// with `hook` seeing each event as it's sent; answers every event sent, including any the
    /// hook adds.
    fn work(&self, run: u64, hook: impl Fn(&crate::SetupEv, &Mutex<Vec<crate::SetupEv>>)) -> Vec<crate::SetupEv> {
        let sent = Mutex::new(vec![]);
        let send = |ev: crate::SetupEv| {
            sent.lock().unwrap().push(ev.clone());
            hook(&ev, &sent);
        };
        crate::setup_thread(&self.lib, &self.dir, &self.id, run, &self.g, &self.p, self.copy.clone(), &send);
        sent.into_inner().unwrap()
    }
}

impl Drop for SetupFixture {
    fn drop(&mut self) {
        crate::setup_end(&self.id, None);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn closing(evs: &[crate::SetupEv], run: u64) -> Vec<String> {
    evs.iter().filter(|e| e.run == run && ["done", "error", "cancelled"].contains(&e.step.as_str())).map(|e| e.step.clone()).collect()
}

/// The real run end to end: it ends with one `done`, and only then are the settings written and
/// the copy remembered (in the Library and in library.json).
#[test]
fn a_setup_run_that_gets_through_is_done_once_and_remembered() {
    let f = setup_fixture("done");
    let (run, _) = crate::setup_begin(&f.id);
    let evs = f.work(run, |_, _| {});
    assert_eq!(closing(&evs, run), vec!["done"], "{evs:?}");
    assert_eq!(evs.last().unwrap().step, "done");
    assert!(evs.iter().all(|e| e.id == f.id && e.run == run));
    assert_eq!(f.game_copy().as_deref(), Some(f.copy.as_path()));
    assert_eq!(f.saved_copy(), json!(f.copy), "saved");
    let settings: Value = serde_json::from_slice(&std::fs::read(f.g.path.join("settings.json")).unwrap()).unwrap();
    assert_eq!(settings["game"], json!(f.copy.to_string_lossy()), "its settings point at the copy");
    assert!(crate::setup_stopped(&f.id, run), "the run is over");
    assert_eq!(crate::setup_end(&f.id, None), None, "nothing left for a cancel to stop: no `cancelled` after `done`");
}

/// Cancelled (as setup_cancel does) as it reaches its last step: the run sends nothing more,
/// writes no settings and doesn't remember the copy; its one closing event is the cancel's.
#[test]
fn a_setup_run_cancelled_at_its_last_step_writes_nothing() {
    let f = setup_fixture("cancel");
    let (run, _) = crate::setup_begin(&f.id);
    let evs = f.work(run, |ev, sent| {
        if ev.step == "setup" {
            let r = crate::setup_end(&ev.id, None).expect("the run was going");
            sent.lock().unwrap().push(crate::SetupEv { id: ev.id.clone(), run: r, step: "cancelled".into(), ..Default::default() });
        }
    });
    assert_eq!(closing(&evs, run), vec!["cancelled"], "{evs:?}");
    assert_eq!(f.game_copy(), None);
    assert_eq!(f.saved_copy(), Value::Null);
    assert!(!f.settings_written(), "no settings written for a cancelled run");
}

/// Replaced by a newer run for the same game while it checks the copy: the old run ends without
/// a closing event of its own and without writing, and the newer run is left going.
#[test]
fn a_replaced_setup_run_leaves_the_newer_one_alone() {
    let f = setup_fixture("replace");
    let (run, _) = crate::setup_begin(&f.id);
    let newer = Mutex::new(None);
    let evs = f.work(run, |ev, sent| {
        if ev.step == "verify" && ev.pct >= 1.0 && newer.lock().unwrap().is_none() {
            let (r2, old) = crate::setup_begin(&ev.id);
            *newer.lock().unwrap() = Some(r2);
            sent.lock().unwrap().push(crate::SetupEv { id: ev.id.clone(), run: old.expect("it replaced the run"), step: "cancelled".into(), ..Default::default() });
        }
    });
    let r2 = newer.into_inner().unwrap().expect("replaced");
    assert_eq!(closing(&evs, run), vec!["cancelled"], "{evs:?}");
    assert_eq!(f.game_copy(), None);
    assert!(!f.settings_written());
    assert!(!crate::setup_stopped(&f.id, r2), "the newer run goes on");
}

/// A copy that isn't there: one `error`, and the run is over.
#[test]
fn a_failed_setup_run_says_so_once() {
    let mut f = setup_fixture("fail");
    f.copy = f.dir.join("not-there.iso");
    let (run, _) = crate::setup_begin(&f.id);
    let evs = f.work(run, |_, _| {});
    assert_eq!(closing(&evs, run), vec!["error"], "{evs:?}");
    assert!(crate::setup_stopped(&f.id, run));
    assert_eq!(f.game_copy(), None);
}

/// setup_commit, which setup_work ends with: in one step against setup_cancel and a newer run.
#[test]
fn setup_commit_is_one_step_with_cancel_and_replacement() {
    let f = setup_fixture("commit");
    let copy = f.copy.clone();
    let wrote = AtomicBool::new(false);
    let write = || {
        wrote.store(true, Ordering::SeqCst);
        Ok(())
    };
    // Replaced first: nothing written, the newer run untouched.
    let (r1, _) = crate::setup_begin(&f.id);
    let (r2, old) = crate::setup_begin(&f.id);
    assert_eq!(old, Some(r1));
    assert_eq!(crate::setup_commit(&f.lib, &f.dir, &f.id, r1, &copy, write), Ok(false));
    assert!(!wrote.load(Ordering::SeqCst) && f.game_copy().is_none() && f.saved_copy().is_null());
    assert!(!crate::setup_stopped(&f.id, r2));
    // Cancelled first: the same.
    assert_eq!(crate::setup_end(&f.id, None), Some(r2));
    assert_eq!(crate::setup_commit(&f.lib, &f.dir, &f.id, r2, &copy, || Ok(())), Ok(false));
    assert!(f.game_copy().is_none());
    // A cancel and a newer run arriving while it writes wait for it, and then find the run over:
    // the run's one closing event is its `done`, never a `cancelled` after it.
    for late in ["cancel", "replace"] {
        let (run, _) = crate::setup_begin(&f.id);
        let (writing_tx, writing_rx) = std::sync::mpsc::channel();
        let finished = Arc::new(AtomicBool::new(false));
        let (other, other_done) = std::thread::scope(|s| {
            let commit = s.spawn(|| {
                let r = crate::setup_commit(&f.lib, &f.dir, &f.id, run, &copy, || {
                    writing_tx.send(()).unwrap();
                    std::thread::sleep(Duration::from_millis(300));
                    Ok(())
                });
                finished.store(true, Ordering::SeqCst);
                r
            });
            writing_rx.recv().unwrap();
            let fin = finished.clone();
            let id = f.id.clone();
            let other = s.spawn(move || {
                let r = if late == "cancel" { crate::setup_end(&id, None) } else { crate::setup_begin(&id).1 };
                (r, fin.load(Ordering::SeqCst))
            });
            assert_eq!(commit.join().unwrap(), Ok(true), "{late}");
            other.join().unwrap()
        });
        assert!(other_done, "{late}: waited for the commit to finish");
        assert_eq!(other, None, "{late}: found the run already over (no `cancelled` for it)");
        assert_eq!(f.game_copy().as_deref(), Some(copy.as_path()));
        assert_eq!(f.saved_copy(), json!(copy));
        crate::setup_end(&f.id, None);
    }
    // Writing its settings failed: the error is the run's end, nothing remembered.
    let g = setup_fixture("commit-err");
    let (run, _) = crate::setup_begin(&g.id);
    assert_eq!(crate::setup_commit(&g.lib, &g.dir, &g.id, run, &g.copy, || Err("disk full".into())), Err("disk full".to_string()));
    assert!(crate::setup_stopped(&g.id, run));
    assert!(g.game_copy().is_none());
}

/// A panic while the run commits (writing its settings, or remembering the copy) is the run's
/// one `error`: the run is ended, its closing event still comes from the side that ended it, and
/// the next setup for the game starts as usual. setup_commit itself, with `write` panicking:
#[test]
fn a_panic_while_committing_is_the_runs_one_error() {
    let f = setup_fixture("commit-panic");
    let (run, _) = crate::setup_begin(&f.id);
    let r = crate::setup_commit(&f.lib, &f.dir, &f.id, run, &f.copy, || panic!("writing its settings panicked"));
    assert_eq!(r, Err("The setup stopped before it finished.".to_string()));
    assert!(crate::setup_stopped(&f.id, run), "the run is over");
    assert_eq!(crate::setup_end(&f.id, Some(run)), None, "nothing left for the thread's recovery to report again");
    assert!(f.game_copy().is_none() && f.saved_copy().is_null(), "nothing remembered");
    let (again, old) = crate::setup_begin(&f.id);
    assert_eq!(old, None, "no stale run left behind");
    crate::setup_end(&f.id, Some(again));
}

/// The whole run, as setup_copy's thread runs it, panicking in the commit for real: the Library's
/// lock is poisoned, so taking it to remember the copy panics after the settings are written. It
/// ends with exactly one `error`.
#[test]
fn a_setup_run_that_panics_while_committing_ends_with_one_error() {
    let g = setup_fixture("commit-panic-run");
    let _ = std::thread::scope(|s| {
        s.spawn(|| {
            let _l = g.lib.lock().unwrap();
            panic!("poisoning the Library lock for the test");
        })
        .join()
    });
    assert!(g.lib.is_poisoned());
    let (run, _) = crate::setup_begin(&g.id);
    let evs = g.work(run, |_, _| {});
    assert_eq!(closing(&evs, run), vec!["error"], "exactly one closing event: {evs:?}");
    let last = evs.last().unwrap();
    assert_eq!((last.step.as_str(), last.text.as_str()), ("error", "The setup stopped before it finished."), "{evs:?}");
    assert!(evs.iter().any(|e| e.step == "setup"), "it got as far as the commit: {evs:?}");
    assert!(g.settings_written(), "the panic came after writing, in the commit");
    assert!(crate::setup_stopped(&g.id, run));
    assert_eq!(crate::setup_end(&g.id, None), None, "no run left going: a cancel finds nothing, no `cancelled` after the error");
    let (next, old) = crate::setup_begin(&g.id);
    assert_eq!(old, None);
    assert!(!crate::setup_stopped(&g.id, next), "the next setup for the game starts as usual");
    assert_eq!(g.lib.lock().unwrap_or_else(|e| e.into_inner()).installed[&g.id].game_copy, None);
}

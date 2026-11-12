//! End-to-end: the real server on a loopback port, with a stand-in launcher that records every
//! Play it's asked for. Each test has its own data folder under the system's temp folder.

use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// A launcher with one installed game, `alpha`, that remembers what it was asked to play.
#[derive(Default)]
struct Fake {
    plays: Mutex<Vec<String>>,
}

impl Launcher for Fake {
    fn status(&self) -> Value {
        json!({"games": [{"id": "alpha", "name": "Alpha", "running": false}], "running": []})
    }

    fn play(&self, id: String, _off: Off) -> PlayResult {
        self.plays.lock().unwrap().push(id.clone());
        Box::pin(async move {
            if id != "alpha" {
                return Err(ApiError::new(404, "not_installed", "That game isn't installed."));
            }
            Ok(json!({"ok": true, "id": id, "started": true}))
        })
    }
}

impl Fake {
    fn plays(&self) -> Vec<String> {
        self.plays.lock().unwrap().clone()
    }
}

struct Running {
    server: Server,
    fake: Arc<Fake>,
    token: String,
    dir: PathBuf,
}

async fn running(name: &str) -> Running {
    let dir = std::env::temp_dir().join(format!("rl-control-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let fake = Arc::new(Fake::default());
    let server = start(&dir, "9.9.9-test", fake.clone()).await.unwrap();
    let info: Value = serde_json::from_slice(&std::fs::read(endpoint_file(&dir)).unwrap()).unwrap();
    let token = info["token"].as_str().unwrap().to_string();
    Running { server, fake, token, dir }
}

impl Running {
    async fn finish(self) {
        assert!(self.server.stop().await.is_ok(), "the server didn't end");
        let _ = std::fs::remove_dir_all(&self.dir);
    }

    fn auth(&self) -> String {
        format!("Bearer {}", self.token)
    }

    async fn get(&self, path: &str, auth: Option<&str>) -> (u16, Value) {
        send(self.server.port, "GET", path, auth, &[], None).await
    }

    async fn post(&self, path: &str, auth: Option<&str>, body: &str) -> (u16, Value) {
        send(self.server.port, "POST", path, auth, &[("Content-Type", "application/json")], Some(body)).await
    }

    async fn mcp(&self, msg: Value) -> (u16, Value) {
        let auth = self.auth();
        self.post("/mcp", Some(&auth), &msg.to_string()).await
    }
}

/// One raw HTTP request; the status and the JSON body (Null when there's none).
async fn send(port: u16, method: &str, path: &str, auth: Option<&str>, headers: &[(&str, &str)], body: Option<&str>) -> (u16, Value) {
    let mut raw = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n");
    if let Some(a) = auth {
        raw.push_str(&format!("Authorization: {a}\r\n"));
    }
    for (k, v) in headers {
        raw.push_str(&format!("{k}: {v}\r\n"));
    }
    let body = body.unwrap_or("");
    if method == "POST" {
        raw.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    raw.push_str("\r\n");
    raw.push_str(body);
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(raw.as_bytes()).await.unwrap();
    let mut out = vec![];
    let _ = s.read_to_end(&mut out).await;
    let text = String::from_utf8_lossy(&out).to_string();
    let (head, rest) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head.split(' ').nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    (status, serde_json::from_str(rest).unwrap_or(Value::Null))
}

#[tokio::test]
async fn the_endpoint_file_is_private_and_points_at_loopback() {
    let r = running("endpoint").await;
    let file = endpoint_file(&r.dir);
    assert!(private::is_private(&file).is_ok());
    assert!(private::is_private(&control_dir(&r.dir)).is_ok());
    let info: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(info["url"], format!("http://127.0.0.1:{}", r.server.port));
    assert_eq!(info["pid"], std::process::id());
    assert_eq!(r.token.len(), 64);
    r.finish().await;
}

#[tokio::test]
async fn status_answers_with_the_token() {
    let r = running("status").await;
    let (status, body) = r.get("/v1/status", Some(&r.auth())).await;
    assert_eq!(status, 200);
    assert_eq!(body["ok"], true);
    assert_eq!(body["api_version"], API_VERSION);
    assert_eq!(body["games"][0]["id"], "alpha");
    r.finish().await;
}

#[tokio::test]
async fn play_starts_an_installed_game() {
    let r = running("play").await;
    let (status, body) = r.post("/v1/play", Some(&r.auth()), r#"{"id":"alpha"}"#).await;
    assert_eq!((status, body["started"].clone()), (200, json!(true)));
    assert_eq!(r.fake.plays(), vec!["alpha".to_string()]);
    r.finish().await;
}

#[tokio::test]
async fn play_of_a_game_that_isnt_installed_is_refused_by_the_launcher() {
    let r = running("notinstalled").await;
    let (status, body) = r.post("/v1/play", Some(&r.auth()), r#"{"id":"beta"}"#).await;
    assert_eq!((status, body["error"]["code"].clone()), (404, json!("not_installed")));
    r.finish().await;
}

#[tokio::test]
async fn no_token_gets_nothing_on_any_route() {
    let r = running("notoken").await;
    assert_eq!(r.get("/v1/status", None).await.0, 401);
    assert_eq!(r.post("/v1/play", None, r#"{"id":"alpha"}"#).await.0, 401);
    let call = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "play_game", "arguments": {"id": "alpha"}}});
    assert_eq!(r.post("/mcp", None, &call.to_string()).await.0, 401);
    assert_eq!(r.post("/mcp", Some("Bearer not-the-token"), &call.to_string()).await.0, 401);
    assert!(r.fake.plays().is_empty());
    r.finish().await;
}

#[tokio::test]
async fn web_pages_cant_play_even_with_the_token() {
    let r = running("browser").await;
    let auth = r.auth();
    let port = r.server.port;
    let page = [("Content-Type", "application/json"), ("Origin", "http://evil.example")];
    assert_eq!(send(port, "POST", "/v1/play", Some(&auth), &page, Some(r#"{"id":"alpha"}"#)).await.0, 403);
    let fetch = [("Content-Type", "application/json"), ("Sec-Fetch-Site", "cross-site")];
    assert_eq!(send(port, "POST", "/v1/play", Some(&auth), &fetch, Some(r#"{"id":"alpha"}"#)).await.0, 403);
    // A simple form post, which a page can send without asking first.
    let form = [("Content-Type", "text/plain")];
    assert_eq!(send(port, "POST", "/v1/play", Some(&auth), &form, Some(r#"{"id":"alpha"}"#)).await.0, 415);
    assert!(r.fake.plays().is_empty());
    r.finish().await;
}

#[tokio::test]
async fn another_host_name_is_refused() {
    let r = running("host").await;
    let raw = format!("GET /v1/status HTTP/1.1\r\nHost: evil.example:{}\r\nAuthorization: {}\r\n\r\n", r.server.port, r.auth());
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", r.server.port)).await.unwrap();
    s.write_all(raw.as_bytes()).await.unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out).await;
    assert!(out.starts_with("HTTP/1.1 403"), "{out}");
    r.finish().await;
}

#[tokio::test]
async fn every_other_command_is_absent_even_with_the_token() {
    let r = running("absent").await;
    let auth = r.auth();
    // The old API's routes, and what the deferred work would need: none of them exist.
    for path in ["/v1/invoke", "/v1/commands", "/v1/jobs", "/v1/events", "/v1/app", "/v1/settings", "/v1/install", "/v1/uninstall", "/v1/limitations"] {
        let body = r#"{"command":"uninstall","args":{"id":"alpha"},"action":"quit","patch":{"control_api":true}}"#;
        assert_eq!(r.post(path, Some(&auth), body).await.0, 404, "{path}");
        assert_eq!(r.get(path, Some(&auth)).await.0, 404, "{path}");
    }
    assert_eq!(r.get("/v1/play", Some(&auth)).await.0, 405);
    assert_eq!(r.post("/v1/status", Some(&auth), "{}").await.0, 405);
    assert!(r.fake.plays().is_empty());
    r.finish().await;
}

#[tokio::test]
async fn play_takes_only_an_id() {
    let r = running("playargs").await;
    let auth = r.auth();
    // Another program, Big Picture or closing other games: not this API's to ask for.
    for body in [r#"{"id":"alpha","program":"evil.exe"}"#, r#"{"id":"alpha","replace":true}"#, r#"{"id":"alpha","bigPicture":true}"#, r#"{}"#, r#"{"id":7}"#, r#"["alpha"]"#, "not json"] {
        assert_eq!(r.post("/v1/play", Some(&auth), body).await.0, 400, "{body}");
    }
    let long = json!({"id": "a".repeat(MAX_ID_LEN + 1)}).to_string();
    assert_eq!(r.post("/v1/play", Some(&auth), &long).await.0, 400);
    assert_eq!(r.post("/v1/play", Some(&auth), r#"{"id":"al\npha"}"#).await.0, 400);
    assert_eq!(r.post("/v1/play", Some(&auth), r#"{"id":"   "}"#).await.0, 400);
    assert!(r.fake.plays().is_empty());
    r.finish().await;
}

#[tokio::test]
async fn mcp_lists_only_status_and_play() {
    let r = running("mcplist").await;
    let (status, init) = r.mcp(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}})).await;
    assert_eq!(status, 200);
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    let (_, list) = r.mcp(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).await;
    let names: Vec<&str> = list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, vec![mcp::STATUS_TOOL, mcp::PLAY_TOOL]);
    r.finish().await;
}

#[tokio::test]
async fn mcp_status_and_play_work() {
    let r = running("mcpcall").await;
    let (_, st) = r.mcp(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "launcher_status", "arguments": {}}})).await;
    assert_eq!(st["result"]["isError"], false);
    assert_eq!(st["result"]["structuredContent"]["games"][0]["id"], "alpha");
    let (_, play) = r.mcp(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "play_game", "arguments": {"id": "alpha"}}})).await;
    assert_eq!(play["result"]["isError"], false);
    assert_eq!(r.fake.plays(), vec!["alpha".to_string()]);
    r.finish().await;
}

#[tokio::test]
async fn mcp_refuses_every_other_tool_and_extra_arguments() {
    let r = running("mcpdeny").await;
    for name in ["install", "uninstall", "save_settings", "rl_play", "rlapi_invoke", "rlapi_app", "rlapi_update_settings"] {
        let (_, v) = r.mcp(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": {"id": "alpha"}}})).await;
        assert_eq!(v["error"]["code"], -32602, "{name}");
    }
    let (_, v) = r.mcp(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "play_game", "arguments": {"id": "alpha", "program": "evil.exe"}}})).await;
    assert_eq!(v["result"]["isError"], true);
    let (_, v) = r.mcp(json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "launcher_status", "arguments": {"command": "uninstall"}}})).await;
    assert_eq!(v["result"]["isError"], true);
    let (_, v) = r.mcp(json!({"jsonrpc": "2.0", "id": 4, "method": "resources/list"})).await;
    assert_eq!(v["error"]["code"], -32601);
    let (_, v) = r.mcp(json!([{"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "play_game", "arguments": {"id": "alpha"}}}])).await;
    assert_eq!(v["error"]["code"], -32600);
    assert!(r.fake.plays().is_empty());
    r.finish().await;
}

#[tokio::test]
async fn mcp_notifications_get_no_answer() {
    let r = running("mcpnote").await;
    let (status, body) = r.mcp(json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).await;
    assert_eq!((status, body), (202, Value::Null));
    r.finish().await;
}

#[tokio::test]
async fn stopping_closes_the_port_and_removes_the_endpoint_file() {
    let r = running("stop").await;
    let (file, dir) = (endpoint_file(&r.dir), r.dir.clone());
    assert!(file.is_file());
    assert!(r.server.stop().await.is_ok(), "the server didn't end");
    assert!(!file.exists());
    // The listener ended with the server. (Another test may already have been given the same
    // free port, so connecting to it can't be the check.)
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn stopping_cancels_an_incomplete_connection_without_waiting_for_its_body() {
    let r = running("stop-incomplete").await;
    let mut sock = tokio::net::TcpStream::connect(("127.0.0.1", r.server.port)).await.unwrap();
    sock.write_all(b"POST /v1/play HTTP/1.1\r\n").await.unwrap();
    assert!(r.server.stop_within(std::time::Duration::from_secs(1)).await.is_ok());
    let mut out = Vec::new();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), sock.read_to_end(&mut out)).await.unwrap();
    assert!(r.fake.plays().is_empty());
    let _ = std::fs::remove_dir_all(r.dir);
}

#[tokio::test]
async fn a_request_after_halt_is_refused() {
    let r = running("halt").await;
    let port = r.server.port;
    // A connection made before the API was turned off, whose request arrives after.
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    r.server.halt();
    let body = r#"{"id":"alpha"}"#;
    let raw = format!("POST /v1/play HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", r.auth(), body.len());
    let _ = s.write_all(raw.as_bytes()).await;
    let mut out = String::new();
    let _ = s.read_to_string(&mut out).await;
    assert!(out.is_empty() || out.starts_with("HTTP/1.1 503"), "{out}");
    assert!(r.fake.plays().is_empty());
    r.finish().await;
}

/// A launcher whose Play waits (as a shader download or a cloud save can) before its game would
/// start. Like the real shader wait, it can safely be cancelled through `off`.
#[derive(Default)]
struct Slow {
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
    launched: AtomicBool,
    offs: Mutex<Vec<Off>>,
}

impl Launcher for Arc<Slow> {
    fn status(&self) -> Value {
        json!({"games": [], "running": []})
    }

    fn play(&self, _id: String, off: Off) -> PlayResult {
        self.offs.lock().unwrap().push(off.clone());
        let me = self.clone();
        Box::pin(async move {
            me.entered.notify_one();
            if off.while_on(me.release.notified()).await.is_none() {
                return Err(ApiError::new(503, "stopped", "The control API was turned off."));
            }
            me.launched.store(true, Ordering::SeqCst);
            Ok(json!({"ok": true, "started": true}))
        })
    }
}

async fn start_slow(dir: &Path, slow: &Arc<Slow>) -> (Server, String) {
    let server = start(dir, "9.9.9-test", Arc::new(slow.clone())).await.unwrap();
    let info: Value = serde_json::from_slice(&std::fs::read(endpoint_file(dir)).unwrap()).unwrap();
    (server, info["token"].as_str().unwrap().to_string())
}

#[tokio::test(flavor = "multi_thread")]
async fn turning_it_off_cancels_a_play_still_getting_ready() {
    let dir = std::env::temp_dir().join(format!("rl-control-cancel-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let slow = Arc::new(Slow::default());
    let (server, token) = start_slow(&dir, &slow).await;
    let port = server.port;
    let auth = format!("Bearer {token}");
    let asked = tokio::spawn(async move { send(port, "POST", "/v1/play", Some(&auth), &[("Content-Type", "application/json")], Some(r#"{"id":"alpha"}"#)).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), slow.entered.notified()).await.expect("Play didn't start getting ready");
    assert!(server.stop().await.is_ok(), "the server didn't end");
    // Whatever it was waiting on finishes now: the game must not start.
    slow.release.notify_waiters();
    let (status, _) = asked.await.unwrap();
    assert_ne!(status, 200);
    // Turned on again: the Play from before still doesn't come back to life.
    let (again, _) = start_slow(&dir, &slow).await;
    slow.release.notify_waiters();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(!slow.launched.load(Ordering::SeqCst), "a game started after the API was turned off");
    assert!(again.stop().await.is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn each_start_of_the_api_has_its_own_off_switch() {
    let dir = std::env::temp_dir().join(format!("rl-control-offs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let slow = Arc::new(Slow::default());
    let (first, token) = start_slow(&dir, &slow).await;
    let (port, auth) = (first.port, format!("Bearer {token}"));
    let asked = tokio::spawn(async move { send(port, "POST", "/v1/play", Some(&auth), &[("Content-Type", "application/json")], Some(r#"{"id":"alpha"}"#)).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), slow.entered.notified()).await.unwrap();
    let first_off = slow.offs.lock().unwrap()[0].clone();
    assert!(!first_off.is_off());
    assert!(first.stop().await.is_ok());
    let _ = asked.await;
    // What the launcher looks at right before a game starts says off for good.
    assert!(first_off.is_off());
    let (second, token) = start_slow(&dir, &slow).await;
    let (port, auth) = (second.port, format!("Bearer {token}"));
    let asked = tokio::spawn(async move { send(port, "POST", "/v1/play", Some(&auth), &[("Content-Type", "application/json")], Some(r#"{"id":"alpha"}"#)).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), slow.entered.notified()).await.unwrap();
    let second_off = slow.offs.lock().unwrap()[1].clone();
    // On again: a new switch, while the old one stays off.
    assert!(!second_off.is_off() && first_off.is_off());
    slow.release.notify_waiters();
    assert_eq!(asked.await.unwrap().0, 200);
    assert!(second.stop().await.is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A launcher whose Play gets stuck in work that can't be interrupted (unpacking an AppImage
/// waits on its process), after its last look at `off`, as the launcher's own Play is. Its game
/// then starts through [`Off::launch`], the gate the launcher's real start goes through.
#[derive(Default)]
pub(crate) struct Stuck {
    pub(crate) entered: tokio::sync::Notify,
    release: (std::sync::Mutex<bool>, std::sync::Condvar),
    attempts: std::sync::atomic::AtomicUsize,
    launched: std::sync::atomic::AtomicUsize,
}

impl Stuck {
    pub(crate) fn let_go(&self) {
        *self.release.0.lock().unwrap() = true;
        self.release.1.notify_all();
    }

    /// Plays that reached the start: (asked to start, started).
    pub(crate) fn starts(&self) -> (usize, usize) {
        (self.attempts.load(Ordering::SeqCst), self.launched.load(Ordering::SeqCst))
    }
}

/// Lets a [`Stuck`] go when dropped: a test that fails while it's stuck ends instead of waiting
/// on it for ever.
pub(crate) struct LetGo(pub(crate) Arc<Stuck>);

impl Drop for LetGo {
    fn drop(&mut self) {
        self.0.let_go();
    }
}

impl Launcher for Arc<Stuck> {
    fn status(&self) -> Value {
        json!({"games": [], "running": []})
    }

    fn play(&self, _id: String, off: Off) -> PlayResult {
        let me = self.clone();
        Box::pin(async move {
            if off.is_off() {
                return Err(ApiError::new(503, "stopped", "The control API was turned off."));
            }
            me.entered.notify_one();
            // Blocks this thread, not just this task: aborting the task can't end it.
            let mut released = me.release.0.lock().unwrap();
            while !*released {
                released = me.release.1.wait(released).unwrap();
            }
            drop(released);
            me.attempts.fetch_add(1, Ordering::SeqCst);
            let started = off.launch(|| me.launched.fetch_add(1, Ordering::SeqCst));
            match started {
                Some(_) => Ok(json!({"ok": true, "started": true})),
                None => Err(ApiError::new(503, "stopped", "The control API was turned off.")),
            }
        })
    }
}

pub(crate) async fn play_on(server: &Server, dir: &Path) -> tokio::task::JoinHandle<(u16, Value)> {
    let info: Value = serde_json::from_slice(&std::fs::read(endpoint_file(dir)).unwrap()).unwrap();
    let (port, auth) = (server.port, format!("Bearer {}", info["token"].as_str().unwrap()));
    tokio::spawn(async move { send(port, "POST", "/v1/play", Some(&auth), &[("Content-Type", "application/json")], Some(r#"{"id":"alpha"}"#)).await })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_play_stuck_past_the_stop_wait_never_starts_its_game_even_once_on_again() {
    // The review's case: Play stuck longer than turning it off waits, the API turned on again,
    // then the old Play let go.
    let dir = std::env::temp_dir().join(format!("rl-control-stuck-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let stuck = Arc::new(Stuck::default());
    let _let_go = LetGo(stuck.clone());
    let first = start(&dir, "9.9.9-test", Arc::new(stuck.clone())).await.unwrap();
    let asked = play_on(&first, &dir).await;
    tokio::time::timeout(std::time::Duration::from_secs(5), stuck.entered.notified()).await.expect("Play didn't start getting ready");
    // A short wait stands in for the real five seconds.
    let ending = first.stop_within(std::time::Duration::from_millis(300)).await.expect_err("a stuck Play ended");
    assert!(!ending.ended(), "the stuck Play is still tracked, not let go");
    let second = start(&dir, "9.9.9-test", Arc::new(stuck.clone())).await.unwrap();
    stuck.let_go();
    // The old Play reaches the start, and the start refuses it.
    assert!(ending.wait(std::time::Duration::from_secs(5)).await.is_ok(), "the old server never ended");
    assert_eq!(stuck.starts(), (1, 0), "a game started after the API was turned off");
    assert_ne!(asked.await.unwrap().0, 200);
    // The API turned on again starts games as usual.
    let (status, _) = play_on(&second, &dir).await.await.unwrap();
    assert_eq!(status, 200);
    assert_eq!(stuck.starts(), (2, 1));
    assert!(second.stop().await.is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn turning_it_off_waits_for_a_start_under_way_then_refuses_any_more() {
    let off = Off::default();
    let (inside, leave) = (Arc::new(std::sync::Barrier::new(2)), Arc::new(std::sync::Barrier::new(2)));
    let (off2, inside2, leave2) = (off.clone(), inside.clone(), leave.clone());
    let starting = std::thread::spawn(move || {
        off2.launch(|| {
            inside2.wait();
            leave2.wait();
        })
    });
    inside.wait();
    let turning = std::thread::spawn({
        let off = off.clone();
        move || off.turn_off()
    });
    std::thread::sleep(std::time::Duration::from_millis(100));
    // The start under way holds the gate: turning off waits for it.
    assert!(!turning.is_finished());
    leave.wait();
    assert!(starting.join().unwrap().is_some());
    turning.join().unwrap();
    assert!(off.launch(|| ()).is_none());
}

/// The stdio adapter carries MCP to this server and nothing more: the same two tools, the same
/// refusals. Skipped where there's no python3.
#[tokio::test(flavor = "multi_thread")]
async fn the_stdio_adapter_reaches_only_status_and_play() {
    let python = if cfg!(windows) { "python" } else { "python3" };
    // Windows' "python" can be a Store link that only says how to install it.
    let works = std::process::Command::new(python).arg("--version").output().map(|o| o.status.success()).unwrap_or(false);
    if !works {
        eprintln!("no {python}: skipped");
        return;
    }
    let r = running("stdio").await;
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tools/control-api/recomplauncher_mcp.py");
    let endpoint = endpoint_file(&r.dir);
    let lines = [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "install", "arguments": {"id": "alpha"}}}),
        json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "play_game", "arguments": {"id": "alpha"}}}),
    ];
    let input: String = lines.iter().map(|l| format!("{l}\n")).collect();
    let out = tokio::task::spawn_blocking(move || {
        use std::io::Write;
        let mut child = std::process::Command::new(python)
            .arg("-I")
            .arg(&script)
            .arg("--endpoint")
            .arg(&endpoint)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        child.wait_with_output().unwrap()
    })
    .await
    .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(!stdout.contains(&r.token) && !stderr.contains(&r.token), "the token leaked");
    let answers: Vec<Value> = stdout.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    // The notification gets no line: four answers for five messages.
    assert_eq!(answers.len(), 4, "{stdout}\n{stderr}");
    assert_eq!(answers[0]["result"]["serverInfo"]["name"], "recomplauncher");
    assert_eq!(answers[1]["result"]["tools"].as_array().unwrap().len(), 2);
    assert_eq!(answers[2]["error"]["code"], -32602);
    assert_eq!(answers[3]["result"]["isError"], false);
    assert_eq!(r.fake.plays(), vec!["alpha".to_string()]);
    r.finish().await;
}

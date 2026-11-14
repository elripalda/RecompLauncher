//! Update checks use a local site and disposable cache, without an installed game.

use super::*;
use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::AtomicUsize;
use std::time::Duration;

const CHILD: &str = "RL_TEST_UPDATE_CATALOG";
const REPO: &str = "sourceforge:approval-probe";

fn isolated(name: &str) -> bool {
    if std::env::var(CHILD).as_deref() == Ok(name) {
        return false;
    }
    // The catalog's once-per-launch flag, network gates and RL_API are process-wide.
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env(CHILD, name)
        .output().unwrap();
    assert!(out.status.success(), "{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    true
}

struct Site {
    app: App,
    approved: Arc<AtomicUsize>,
    catalogs: Arc<AtomicUsize>,
    stop: std::sync::mpsc::Sender<()>,
    server: Option<std::thread::JoinHandle<()>>,
}

impl Site {
    fn new(approved: usize) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        std::env::set_var("RL_API", format!("http://{}", listener.local_addr().unwrap()));
        let approved = Arc::new(AtomicUsize::new(approved));
        let catalogs = Arc::new(AtomicUsize::new(0));
        let version = approved.clone();
        let requests = catalogs.clone();
        let (stop, stopped) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || loop {
            if stopped.try_recv().is_ok() {
                return;
            }
            let (mut stream, _) = match listener.accept() {
                Ok(s) => s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                }
                Err(e) => panic!("{e}"),
            };
            stream.set_nonblocking(false).unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut request = String::new();
            BufReader::new(&mut stream).read_line(&mut request).unwrap();
            let approved = version.load(Ordering::SeqCst);
            let body = if request.starts_with("GET /pc-catalog.json ") {
                requests.fetch_add(1, Ordering::SeqCst);
                json!({"apps": [{"id": "game", "repo": REPO, "tag": format!("v{approved}")}]})
            } else {
                assert!(request.starts_with("GET /api/sf.php?repo=approval-probe "), "{request}");
                let files = ["game-windows-x64.zip", "game-linux-x64.tar.gz", "game-linux-arm64.tar.gz", "game-macos-arm64.zip", "game-macos-x64.zip"];
                json!([3, 2, 1].map(|v| json!({"tag_name": format!("v{v}"), "assets": files.map(|name| json!({"name": name, "browser_download_url": "https://example.invalid/game.zip", "size": 10}))})))
            };
            let status = if approved == 0 { "503 Service Unavailable" } else { "200 OK" };
            let body = body.to_string();
            write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        });
        let dir = std::env::temp_dir().join(format!("rl-update-catalog-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut lib = Library::default();
        // Site-only releases prevent the offline case from contacting GitHub's fallback API.
        lib.installed.insert("game".into(), Installed { id: "game".into(), repo: REPO.into(), tag: "v1".into(), path: dir.join("game"), ..Default::default() });
        let app = App {
            dir, http: reqwest::Client::builder().no_proxy().build().unwrap(), lib: Mutex::new(lib),
            jobs: Mutex::new(HashMap::new()), running: Mutex::new(HashMap::new()), pids: Mutex::new(HashMap::new()), started: Mutex::new(HashMap::new()),
            download_slot: Arc::new(tokio::sync::Semaphore::new(1)), moving: Mutex::new(HashSet::new()), overlay_open: AtomicBool::new(false), looked: Mutex::new(HashSet::new()), headless: AtomicBool::new(false),
        };
        Self { app, approved, catalogs, stop, server: Some(server) }
    }
}

impl Drop for Site {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(server) = self.server.take() { let _ = server.join(); }
        let _ = std::fs::remove_dir_all(&self.app.dir);
    }
}

#[test]
fn manual_checks_refresh_the_approved_release() {
    if isolated("update_check_tests::manual_checks_refresh_the_approved_release") { return; }
    let site = Site::new(1);
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        assert!(check_updates_for(&site.app, true).await.unwrap().is_empty());
        assert_eq!(site.catalogs.load(Ordering::SeqCst), 1);
        site.approved.store(2, Ordering::SeqCst);
        // Background checks keep using the session's cached approval.
        assert!(check_updates_for(&site.app, false).await.unwrap().is_empty());
        assert_eq!(site.catalogs.load(Ordering::SeqCst), 1);
        let updates = check_updates_for(&site.app, true).await.unwrap();
        // v3 is already released too, but the site has only approved v2.
        assert_eq!(updates.get("game").map(String::as_str), Some("v2"));
        assert_eq!(site.catalogs.load(Ordering::SeqCst), 2);
    });
}

#[test]
fn manual_checks_keep_cached_approval_when_the_site_fails() {
    if isolated("update_check_tests::manual_checks_keep_cached_approval_when_the_site_fails") { return; }
    let site = Site::new(2);
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        assert_eq!(check_updates_for(&site.app, true).await.unwrap().get("game").map(String::as_str), Some("v2"));
        site.approved.store(0, Ordering::SeqCst);
        assert_eq!(check_updates_for(&site.app, true).await.unwrap().get("game").map(String::as_str), Some("v2"));
        assert_eq!(site.catalogs.load(Ordering::SeqCst), 2);
    });
}

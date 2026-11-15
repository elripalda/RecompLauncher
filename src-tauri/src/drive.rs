//! Direct Google Drive storage. Immutable snapshots retain their parents so simultaneous uploads
//! remain visible as conflicts, even when the PCs disagree about the time.

use crate::{cloud, save_provider, saves, App, Error};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex, OnceLock,
};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

type Res<T> = Result<T, Error>;
const API: &str = "https://www.googleapis.com/drive/v3";
const TOKEN: &str = "https://oauth2.googleapis.com/token";
const SCOPE: &str = "https://www.googleapis.com/auth/drive.file";
pub const MAX_ARCHIVE: usize = 95 * 1024 * 1024;
const CREDENTIAL: &str = "google-drive";
static NEEDS_RECONNECT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const RECONNECT: &str = "Google Drive needs you to connect again in Settings → Account.";
static GENERATION: AtomicU64 = AtomicU64::new(0);
static CONNECT: Mutex<Option<Connection>> = Mutex::new(None);
static ACCESS: tokio::sync::Mutex<Option<Access>> = tokio::sync::Mutex::const_new(None);

#[derive(Clone, Serialize, Deserialize)]
struct Account {
    id: String,
    email: String,
    refresh: String,
    client_id: String,
    client_secret: String,
}

struct Access {
    account: String,
    token: String,
    until: Instant,
}

struct Connection {
    generation: u64,
    task: Option<tokio::task::AbortHandle>,
    result: Value,
}

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(300))
            .build()
            .expect("Google HTTP client")
    })
}

fn load(dir: &Path) -> Option<Account> {
    serde_json::from_slice(&crate::secret::load_named(dir, CREDENTIAL)?).ok()
}

fn store(dir: &Path, account: &Account) -> Res<()> {
    crate::secret::save_named(
        dir,
        CREDENTIAL,
        &serde_json::to_vec(account).map_err(|_| Error::Msg("Couldn't save the Google sign-in.".into()))?,
    )?;
    Ok(())
}

pub fn identity(dir: &Path) -> String {
    load(dir).map(|a| a.id).unwrap_or_default()
}

pub fn connected(dir: &Path) -> bool {
    load(dir).is_some()
}

fn desktop_credentials() -> (&'static str, &'static str) {
    (option_env!("RL_GOOGLE_CLIENT_ID").unwrap_or(""), option_env!("RL_GOOGLE_CLIENT_SECRET").unwrap_or(""))
}

fn device_credentials() -> (&'static str, &'static str) {
    (option_env!("RL_GOOGLE_DEVICE_CLIENT_ID").unwrap_or(""), option_env!("RL_GOOGLE_DEVICE_CLIENT_SECRET").unwrap_or(""))
}

pub fn state(dir: &Path) -> Value {
    let a = load(dir);
    json!({"connected": a.is_some(), "email": a.map(|a| a.email), "configured": !desktop_credentials().0.is_empty(), "deviceConfigured": !device_credentials().0.is_empty(), "needsReconnect": NEEDS_RECONNECT.load(Ordering::Relaxed)})
}

fn transport(_: reqwest::Error) -> Error {
    // Upload session URLs and OAuth responses may contain credentials. Never put them in logs.
    Error::Net("Google Drive couldn't be reached. Your saves stay on this PC and will retry.".into())
}

async fn token_form(form: &[(&str, &str)]) -> Res<Value> {
    let r = client().post(TOKEN).form(form).timeout(Duration::from_secs(20)).send().await.map_err(transport)?;
    let status = r.status().as_u16();
    let value: Value = r.json().await.map_err(transport)?;
    if status == 429 || status >= 500 {
        return Err(Error::Net("Google sign-in is busy. Try again later.".into()));
    }
    if status >= 400 || value.get("error").is_some() {
        return Err(Error::Msg(RECONNECT.into()));
    }
    Ok(value)
}

async fn access(app: &AppHandle, force: bool) -> Res<String> {
    let mut cached = ACCESS.lock().await;
    let st = app.state::<App>();
    let mut a = load(&st.dir).ok_or_else(|| Error::Msg(RECONNECT.into()))?;
    if !force {
        if let Some(c) = cached.as_ref().filter(|c| c.account == a.id && c.until > Instant::now()) {
            return Ok(c.token.clone());
        }
    }
    let refreshed = token_form(&[
        ("client_id", &a.client_id),
        ("client_secret", &a.client_secret),
        ("refresh_token", &a.refresh),
        ("grant_type", "refresh_token"),
    ])
    .await;
    if matches!(&refreshed, Err(Error::Msg(message)) if message == RECONNECT) {
        NEEDS_RECONNECT.store(true, Ordering::Relaxed);
    }
    let v = refreshed?;
    NEEDS_RECONNECT.store(false, Ordering::Relaxed);
    let token = v["access_token"].as_str().filter(|s| !s.is_empty()).ok_or_else(|| Error::Msg(RECONNECT.into()))?.to_string();
    if let Some(refresh) = v["refresh_token"].as_str().filter(|s| !s.is_empty()) {
        a.refresh = refresh.to_string();
        store(&st.dir, &a)?;
    }
    let seconds = v["expires_in"].as_u64().unwrap_or(3600).saturating_sub(60).min(3600);
    *cached = Some(Access { account: a.id, token: token.clone(), until: Instant::now() + Duration::from_secs(seconds) });
    Ok(token)
}

fn api_error(status: u16, body: &Value) -> Error {
    let reason = body.pointer("/error/errors/0/reason").and_then(Value::as_str).unwrap_or("");
    if reason == "storageQuotaExceeded" {
        return Error::Msg("Google Drive storage is full. Free space in Drive, then press Sync Now. Your saves stay on this PC.".into());
    }
    if status == 401 {
        return Error::Msg(RECONNECT.into());
    }
    if status == 429 || status >= 500 || matches!(reason, "rateLimitExceeded" | "userRateLimitExceeded") {
        return Error::Net("Google Drive is busy. Your saves stay on this PC and will retry.".into());
    }
    Error::Msg(format!("Google Drive refused the request ({status}). Check its connection in Settings → Account."))
}

async fn send(app: &AppHandle, request: reqwest::RequestBuilder) -> Res<reqwest::Response> {
    let url = request
        .try_clone()
        .ok_or_else(|| Error::Msg("Couldn't prepare the Drive request.".into()))?
        .build()
        .map_err(transport)?
        .url()
        .to_string();
    let token = access(app, false).await?;
    let retry = request.try_clone();
    let mut r = crate::net::send(request.bearer_auth(token), &url, true).await.map_err(|_| transport_error())?;
    if r.status().as_u16() == 401 {
        let token = access(app, true).await?;
        let request = retry.ok_or_else(|| Error::Msg(RECONNECT.into()))?;
        r = request.bearer_auth(token).send().await.map_err(transport)?;
    }
    let status = r.status().as_u16();
    if status < 400 {
        return Ok(r);
    }
    let body = r.json().await.unwrap_or(Value::Null);
    Err(api_error(status, &body))
}

fn transport_error() -> Error {
    Error::Net("Google Drive couldn't be reached. Your saves stay on this PC and will retry.".into())
}

async fn json_response(r: reqwest::Response) -> Res<Value> {
    r.json().await.map_err(transport)
}

async fn about_token(token: &str) -> Res<Value> {
    let r = client()
        .get(format!("{API}/about"))
        .query(&[("fields", "user(permissionId,emailAddress),storageQuota")])
        .bearer_auth(token)
        .send()
        .await
        .map_err(transport)?;
    let status = r.status().as_u16();
    let v = r.json().await.map_err(transport)?;
    if status >= 400 {
        return Err(api_error(status, &v));
    }
    Ok(v)
}

pub async fn space(app: &AppHandle) -> Res<Value> {
    let request = client().get(format!("{API}/about")).query(&[("fields", "storageQuota")]).timeout(Duration::from_secs(20));
    let v = json_response(send(app, request).await?).await?;
    let number = |key: &str| v.pointer(key).and_then(Value::as_str).and_then(|s| s.parse::<u64>().ok());
    Ok(json!({"used": number("/storageQuota/usage"), "quota": number("/storageQuota/limit")}))
}

fn random_text() -> Res<String> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|_| Error::Msg("Couldn't start Google sign-in securely.".into()))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn callback(request: &str, state: &str) -> Res<Option<String>> {
    let target = request.lines().next().and_then(|line| line.strip_prefix("GET ")).and_then(|line| line.strip_suffix(" HTTP/1.1"));
    let Some(target) = target else { return Ok(None) };
    let url = reqwest::Url::parse(&format!("http://127.0.0.1{target}")).map_err(|_| Error::Msg("Invalid Google callback.".into()))?;
    if url.path() != "/oauth2callback" {
        return Ok(None);
    }
    let pairs: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    if pairs.get("state").map(String::as_str) != Some(state) {
        return Ok(None);
    }
    if pairs.contains_key("error") {
        return Err(Error::Msg("Google Drive connection was declined.".into()));
    }
    Ok(pairs.get("code").filter(|c| !c.is_empty()).cloned())
}

async fn browser_code(listener: tokio::net::TcpListener, state: String) -> Res<String> {
    loop {
        let (mut stream, _) = listener.accept().await?;
        let request = tokio::time::timeout(Duration::from_secs(5), async {
            let mut bytes = Vec::new();
            let mut buffer = [0u8; 1024];
            while bytes.len() < 8192 {
                let n = stream.read(&mut buffer).await?;
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&buffer[..n]);
                if bytes.windows(4).any(|s| s == b"\r\n\r\n") {
                    break;
                }
            }
            Ok::<_, std::io::Error>(String::from_utf8_lossy(&bytes).to_string())
        })
        .await;
        let Ok(Ok(request)) = request else { continue };
        let result = callback(&request, &state);
        let accepted = !matches!(&result, Ok(None));
        let body = if accepted {
            "Return to RecompLauncher to finish connecting Google Drive. You can close this page."
        } else {
            "This isn't the Google sign-in this launcher is waiting for."
        };
        let status = if accepted { "200 OK" } else { "400 Bad Request" };
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = tokio::time::timeout(Duration::from_secs(2), stream.write_all(response.as_bytes())).await;
        if let Some(code) = result? {
            return Ok(code);
        }
    }
}

async fn finish(app: &AppHandle, generation: u64, tokens: Value, client_id: String, client_secret: String) -> Res<()> {
    let token = tokens["access_token"].as_str().ok_or_else(|| Error::Msg("Google didn't return a sign-in token.".into()))?;
    let refresh = tokens["refresh_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Error::Msg("Google didn't grant offline access. Connect again and approve access to Drive.".into()))?;
    let about = about_token(token).await?;
    let id = about
        .pointer("/user/permissionId")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Error::Msg("Google didn't identify the connected account.".into()))?;
    let email = about.pointer("/user/emailAddress").and_then(Value::as_str).unwrap_or("Google Drive");
    let _busy = cloud::BUSY.lock().await;
    let st = app.state::<App>();
    save_provider::ensure_idle(&st)?;
    *ACCESS.lock().await = None;
    let connection = CONNECT.lock().unwrap();
    if connection.as_ref().map(|c| c.generation) != Some(generation) {
        return Err(Error::Msg("Google sign-in was cancelled.".into()));
    }
    if identity(&st.dir) != id && save_provider::current(&st.dir) == save_provider::Provider::GoogleDrive {
        save_provider::clear_consent(&st)?;
    }
    store(&st.dir, &Account { id: id.into(), email: email.into(), refresh: refresh.into(), client_id, client_secret })?;
    NEEDS_RECONNECT.store(false, Ordering::Relaxed);
    Ok(())
}

fn launch_connection(
    app: AppHandle,
    generation: u64,
    future: impl std::future::Future<Output = Res<(Value, String, String)>> + Send + 'static,
) {
    let task = tauri::async_runtime::spawn(async move {
        let result = match tokio::time::timeout(Duration::from_secs(600), future).await {
            Ok(Ok((tokens, id, secret))) => finish(&app, generation, tokens, id, secret).await,
            Ok(Err(e)) => Err(e),
            Err(_) => Err(Error::Msg("Google sign-in expired. Try connecting again.".into())),
        };
        {
            let mut current = CONNECT.lock().unwrap();
            if let Some(c) = current.as_mut().filter(|c| c.generation == generation) {
                c.result = match result {
                    Ok(()) => json!({"status": "ok"}),
                    Err(e) => json!({"status": "error", "error": e.to_string()}),
                };
                c.task = None;
            }
        }
        save_provider::emit(&app);
        cloud::upload_pending(&app).await;
    });
    // Tauri's handle supports cancellation without keeping this command open.
    let mut current = CONNECT.lock().unwrap();
    if let Some(c) = current.as_mut().filter(|c| c.generation == generation && c.result["status"] == "pending") {
        c.task = Some(task.inner().abort_handle());
    }
}

#[tauri::command]
pub fn drive_connect_cancel() {
    GENERATION.fetch_add(1, Ordering::SeqCst);
    let mut current = CONNECT.lock().unwrap();
    if let Some(c) = current.take() {
        if let Some(task) = c.task {
            task.abort();
        }
    }
}

#[tauri::command]
pub fn drive_connect_poll() -> Value {
    CONNECT.lock().unwrap().as_ref().map(|c| c.result.clone()).unwrap_or(json!({"status": "cancelled"}))
}

#[tauri::command]
pub async fn drive_connect_start(app: AppHandle, tv: bool) -> Res<Value> {
    save_provider::ensure_idle(&app.state::<App>())?;
    drive_connect_cancel();
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    if tv && !device_credentials().0.is_empty() {
        return start_device(app, generation).await;
    }
    let (id, secret) = desktop_credentials();
    if id.is_empty() {
        return Err(Error::Msg("Google Drive isn't configured in this launcher build.".into()));
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let redirect = format!("http://127.0.0.1:{}/oauth2callback", listener.local_addr()?.port());
    let state = random_text()?;
    let verifier = random_text()?;
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut url = reqwest::Url::parse("https://accounts.google.com/o/oauth2/v2/auth").unwrap();
    url.query_pairs_mut().extend_pairs(&[
        ("client_id", id),
        ("redirect_uri", &redirect),
        ("response_type", "code"),
        ("scope", SCOPE),
        ("state", &state),
        ("code_challenge", &challenge),
        ("code_challenge_method", "S256"),
        ("access_type", "offline"),
        ("prompt", "consent select_account"),
    ]);
    {
        let mut current = CONNECT.lock().unwrap();
        if GENERATION.load(Ordering::SeqCst) != generation {
            return Err(Error::Msg("Google sign-in was cancelled.".into()));
        }
        *current = Some(Connection { generation, task: None, result: json!({"status": "pending"}) });
    }
    launch_connection(app, generation, async move {
        let code = browser_code(listener, state).await?;
        let tokens = token_form(&[
            ("client_id", id),
            ("client_secret", secret),
            ("code", &code),
            ("code_verifier", &verifier),
            ("redirect_uri", &redirect),
            ("grant_type", "authorization_code"),
        ])
        .await?;
        Ok((tokens, id.into(), secret.into()))
    });
    Ok(json!({"url": url.as_str(), "device": false, "expires": 600}))
}

async fn start_device(app: AppHandle, generation: u64) -> Res<Value> {
    let (id, secret) = device_credentials();
    let r = client()
        .post("https://oauth2.googleapis.com/device/code")
        .form(&[("client_id", id), ("scope", SCOPE)])
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(transport)?;
    let status = r.status().as_u16();
    let value: Value = r.json().await.map_err(transport)?;
    if status >= 400 {
        return Err(Error::Msg("Google couldn't start device sign-in. Use the browser on this PC.".into()));
    }
    let code = value["device_code"].as_str().ok_or_else(|| Error::Msg("Google didn't return a device code.".into()))?.to_string();
    let url = value["verification_url"].as_str().unwrap_or("https://www.google.com/device");
    let mut interval = value["interval"].as_u64().unwrap_or(5).clamp(5, 60);
    let expires = value["expires_in"].as_u64().unwrap_or(600).min(600);
    let end = Instant::now() + Duration::from_secs(expires);
    {
        let mut current = CONNECT.lock().unwrap();
        if GENERATION.load(Ordering::SeqCst) != generation {
            return Err(Error::Msg("Google sign-in was cancelled.".into()));
        }
        *current = Some(Connection { generation, task: None, result: json!({"status": "pending"}) });
    }
    launch_connection(app, generation, async move {
        while Instant::now() < end {
            tokio::time::sleep(Duration::from_secs(interval)).await;
            let r = client()
                .post(TOKEN)
                .form(&[
                    ("client_id", id),
                    ("client_secret", secret),
                    ("device_code", &code),
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ])
                .timeout(Duration::from_secs(20))
                .send()
                .await
                .map_err(transport)?;
            let v: Value = r.json().await.map_err(transport)?;
            match v["error"].as_str() {
                Some("authorization_pending") => continue,
                Some("slow_down") => {
                    interval = (interval + 5).min(60);
                    continue;
                }
                Some(_) => return Err(Error::Msg("Google device sign-in was declined or expired. Try again.".into())),
                None => return Ok((v, id.into(), secret.into())),
            }
        }
        Err(Error::Msg("Google device sign-in expired. Try again.".into()))
    });
    Ok(json!({"url": url, "code": value["user_code"], "qr": cloud::qr_svg(url), "device": true, "expires": expires}))
}

#[tauri::command]
pub async fn drive_disconnect(app: AppHandle) -> Res<Value> {
    drive_connect_cancel();
    let _busy = cloud::BUSY.lock().await;
    let st = app.state::<App>();
    save_provider::ensure_idle(&st)?;
    if save_provider::current(&st.dir) == save_provider::Provider::GoogleDrive {
        save_provider::clear_consent(&st)?;
    }
    crate::secret::forget_named(&st.dir, CREDENTIAL)?;
    *ACCESS.lock().await = None;
    NEEDS_RECONNECT.store(false, Ordering::Relaxed);
    save_provider::emit(&app);
    Ok(json!({"state": save_provider::state(&app), "installed": st.lib.lock().unwrap().installed}))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Snapshot {
    format: u32,
    game: String,
    hash: String,
    made: u64,
    device: String,
    played: u64,
    generation: u64,
    parents: Vec<String>,
}

#[derive(Clone)]
struct Remote {
    id: String,
    size: u64,
    snapshot: Snapshot,
}

fn heads(files: &[Remote]) -> Vec<&Remote> {
    let parents: HashSet<&str> = files.iter().flat_map(|f| f.snapshot.parents.iter().map(String::as_str)).collect();
    files.iter().filter(|f| !parents.contains(f.id.as_str())).collect()
}

fn revision(files: &[Remote]) -> String {
    let mut ids: Vec<_> = heads(files).iter().map(|f| f.id.as_str()).collect();
    ids.sort();
    hex::encode(Sha256::digest(ids.join("\n").as_bytes()))
}

fn copies(files: &[Remote]) -> Vec<cloud::CloudCopy> {
    let heads = heads(files);
    let hashes: HashSet<&str> = heads.iter().map(|f| f.snapshot.hash.as_str()).collect();
    let conflict = hashes.len() > 1;
    let head_ids: HashSet<&str> = heads.iter().map(|f| f.id.as_str()).collect();
    let mut ordered: Vec<_> = files.iter().collect();
    ordered.sort_by(|a, b| {
        head_ids
            .contains(b.id.as_str())
            .cmp(&head_ids.contains(a.id.as_str()))
            .then_with(|| b.snapshot.generation.cmp(&a.snapshot.generation))
            .then_with(|| a.id.cmp(&b.id))
    });
    ordered
        .into_iter()
        .map(|f| cloud::CloudCopy {
            v: f.id.clone(),
            hash: f.snapshot.hash.clone(),
            size: f.size,
            made: f.snapshot.made,
            device: f.snapshot.device.clone(),
            played: f.snapshot.played,
            conflict,
            revision: revision(files),
            head: head_ids.contains(f.id.as_str()),
        })
        .collect()
}

fn game_key(id: &str) -> String {
    hex::encode(Sha256::digest(id.as_bytes()))
}

async fn query(app: &AppHandle, q: &str) -> Res<Vec<Value>> {
    let mut page = String::new();
    let mut out = Vec::new();
    loop {
        let request = client().get(format!("{API}/files")).timeout(Duration::from_secs(20)).query(&[
            ("q", q),
            ("fields", "nextPageToken,incompleteSearch,files(id,size,description)"),
            ("pageSize", "1000"),
            ("pageToken", &page),
        ]);
        let value = json_response(send(app, request).await?).await?;
        if value["incompleteSearch"] == true {
            return Err(Error::Msg("Google Drive returned an incomplete save list. Try again later.".into()));
        }
        let files = value["files"].as_array().ok_or_else(|| Error::Msg("Google Drive returned an incomplete save list.".into()))?;
        out.extend(files.iter().cloned());
        if out.len() > 20_000 {
            return Err(Error::Msg("Too many Drive snapshots to sync safely. Review your Drive save folder.".into()));
        }
        page = value["nextPageToken"].as_str().unwrap_or("").to_string();
        if page.is_empty() {
            return Ok(out);
        }
    }
}

async fn remote(app: &AppHandle, id: &str) -> Res<Vec<Remote>> {
    let key = game_key(id);
    let q = format!(
        "trashed = false and appProperties has {{ key='rl' and value='save-v1' }} and appProperties has {{ key='game' and value='{key}' }}"
    );
    let values = query(app, &q).await?;
    let mut files = Vec::new();
    for value in values {
        let snapshot: Snapshot = value["description"]
            .as_str()
            .and_then(|s| serde_json::from_str(s).ok())
            .ok_or_else(|| Error::Msg("A Google Drive save has invalid metadata. Review its save folder before syncing.".into()))?;
        if snapshot.format != 1 || snapshot.game != id || snapshot.hash.len() != 64 {
            return Err(Error::Msg("A Google Drive save has incompatible metadata.".into()));
        }
        let file_id = value["id"].as_str().filter(|s| valid_id(s)).ok_or_else(|| Error::Msg("Invalid Google Drive file ID.".into()))?;
        let size =
            value["size"].as_str().and_then(|s| s.parse::<u64>().ok()).ok_or_else(|| Error::Msg("Incomplete Google Drive save.".into()))?;
        files.push(Remote { id: file_id.into(), size, snapshot });
    }
    let generations: std::collections::HashMap<_, _> = files.iter().map(|f| (f.id.as_str(), f.snapshot.generation)).collect();
    for file in &files {
        let snapshot = &file.snapshot;
        let bad_parent = snapshot
            .parents
            .iter()
            .any(|p| !valid_id(p) || p == &file.id || generations.get(p.as_str()).map(|&n| n >= snapshot.generation).unwrap_or(false));
        if snapshot.generation == 0 || bad_parent {
            return Err(Error::Msg("Google Drive save history is inconsistent. Review its save folder before syncing.".into()));
        }
    }
    if !files.is_empty() && heads(&files).is_empty() {
        return Err(Error::Msg("Google Drive save history is inconsistent. Review its save folder before syncing.".into()));
    }
    Ok(files)
}

pub async fn list(app: &AppHandle, id: &str) -> Res<(Vec<cloud::CloudCopy>, Value)> {
    Ok((copies(&remote(app, id).await?), space(app).await?))
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 200 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

async fn folder(app: &AppHandle, kind: &str, name: &str, parent: Option<&str>) -> Res<String> {
    let q = format!(
        "trashed = false and mimeType = 'application/vnd.google-apps.folder' and appProperties has {{ key='rl' and value='{kind}' }}"
    );
    let mut files = query(app, &q).await?;
    files.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    if let Some(id) = files.first().and_then(|v| v["id"].as_str()).filter(|s| valid_id(s)) {
        return Ok(id.into());
    }
    let mut metadata = json!({"name": name, "mimeType": "application/vnd.google-apps.folder", "appProperties": {"rl": kind}});
    if let Some(parent) = parent {
        metadata["parents"] = json!([parent]);
    }
    let value =
        json_response(send(app, client().post(format!("{API}/files")).timeout(Duration::from_secs(20)).json(&metadata)).await?).await?;
    value["id"]
        .as_str()
        .filter(|s| valid_id(s))
        .map(String::from)
        .ok_or_else(|| Error::Msg("Google couldn't create the save folder.".into()))
}

#[derive(Serialize, Deserialize)]
struct Ticket {
    id: String,
    archive_hash: String,
    snapshot: Snapshot,
    url: Option<String>,
}

fn ticket_path(dir: &Path, id: &str) -> PathBuf {
    dir.join("drive-uploads").join(game_key(&format!("{}:{id}", identity(dir))))
}

fn save_ticket(path: &Path, ticket: &Ticket) -> Res<()> {
    std::fs::create_dir_all(path.parent().unwrap())?;
    let temp = path.with_extension("tmp");
    crate::secret::write_private(&temp, &serde_json::to_vec(ticket).unwrap())?;
    std::fs::rename(temp, path)?;
    Ok(())
}

fn valid_session(url: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(url) else { return false };
    url.scheme() == "https"
        && url.host_str() == Some("www.googleapis.com")
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && url.path() == "/upload/drive/v3/files"
}

async fn complete(app: &AppHandle, ticket: &Ticket, size: usize) -> Res<bool> {
    let token = access(app, false).await?;
    let r = client()
        .get(format!("{API}/files/{}", ticket.id))
        .timeout(Duration::from_secs(20))
        .query(&[("fields", "id,size,sha256Checksum")])
        .bearer_auth(token)
        .send()
        .await
        .map_err(transport)?;
    if r.status().as_u16() == 404 {
        return Ok(false);
    }
    let status = r.status().as_u16();
    let v: Value = r.json().await.map_err(transport)?;
    if status >= 400 {
        return Err(api_error(status, &v));
    }
    Ok(v["size"].as_str().and_then(|s| s.parse::<usize>().ok()) == Some(size) && v["sha256Checksum"].as_str() == Some(&ticket.archive_hash))
}

async fn upload_bytes(app: &AppHandle, ticket_path: &Path, ticket: &mut Ticket, bytes: Vec<u8>) -> Res<()> {
    if complete(app, ticket, bytes.len()).await? {
        return Ok(());
    }
    let mut offset = 0;
    if let Some(url) = ticket.url.as_deref().filter(|u| valid_session(u)) {
        let token = access(app, false).await?;
        let r = client()
            .put(url)
            .timeout(Duration::from_secs(20))
            .bearer_auth(token)
            .header("Content-Length", "0")
            .header("Content-Range", format!("bytes */{}", bytes.len()))
            .send()
            .await
            .map_err(transport)?;
        match r.status().as_u16() {
            200 | 201 => {
                return if complete(app, ticket, bytes.len()).await? { Ok(()) } else { Err(transport_error()) };
            }
            308 => offset = resume_offset(r.headers(), bytes.len())?,
            404 | 410 => ticket.url = None,
            status => return Err(api_error(status, &r.json().await.unwrap_or(Value::Null))),
        }
    } else {
        ticket.url = None;
    }
    if ticket.url.is_none() {
        let root = folder(app, "root-v1", "RecompLauncher", None).await?;
        let parent = folder(app, "saves-v1", "Saves", Some(&root)).await?;
        let metadata = json!({"id": ticket.id, "name": format!("{}-{}.zip", crate::library::folder_name(&ticket.snapshot.game), ticket.snapshot.made), "parents": [parent], "mimeType": "application/zip", "description": serde_json::to_string(&ticket.snapshot).unwrap(), "appProperties": {"rl": "save-v1", "game": game_key(&ticket.snapshot.game)}});
        let request = client()
            .post("https://www.googleapis.com/upload/drive/v3/files")
            .query(&[("uploadType", "resumable")])
            .header("X-Upload-Content-Type", "application/zip")
            .header("X-Upload-Content-Length", bytes.len())
            .json(&metadata);
        let r = send(app, request).await?;
        let url = r
            .headers()
            .get("Location")
            .and_then(|v| v.to_str().ok())
            .filter(|u| valid_session(u))
            .ok_or_else(|| Error::Msg("Google returned an invalid upload session.".into()))?;
        ticket.url = Some(url.into());
        save_ticket(ticket_path, ticket)?;
    }
    if offset >= bytes.len() {
        return Err(transport_error());
    }
    let total = bytes.len();
    let request = client()
        .put(ticket.url.as_ref().unwrap())
        .header("Content-Type", "application/zip")
        .header("Content-Range", format!("bytes {offset}-{}/{total}", total - 1))
        .body(bytes[offset..].to_vec());
    let r = send(app, request).await?;
    if !r.status().is_success() {
        return Err(transport_error());
    }
    if !complete(app, ticket, total).await? {
        return Err(Error::Msg("Google Drive's uploaded copy didn't match. Your local save is kept.".into()));
    }
    Ok(())
}

fn resume_offset(headers: &reqwest::header::HeaderMap, total: usize) -> Res<usize> {
    let Some(range) = headers.get("Range") else { return Ok(0) };
    let last = range.to_str().ok().and_then(|s| s.strip_prefix("bytes=0-")).and_then(|s| s.parse::<usize>().ok());
    let offset = last.and_then(|n| n.checked_add(1)).filter(|&n| n <= total);
    offset.ok_or_else(|| Error::Msg("Google returned an invalid upload position.".into()))
}

pub async fn upload(app: &AppHandle, id: &str, path: &Path, resolution: Option<&str>) -> Res<()> {
    let bytes = tokio::fs::read(path).await?;
    if bytes.len() > MAX_ARCHIVE {
        return Err(Error::Msg("This save is too big for cloud saves (over 95 MB). It's kept on this PC.".into()));
    }
    let man = saves::manifest(path).ok_or_else(|| Error::Msg("That save copy is gone.".into()))?;
    let files = remote(app, id).await?;
    let path = ticket_path(&app.state::<App>().dir, id);
    let archive_hash = hex::encode(Sha256::digest(&bytes));
    let old: Option<Ticket> = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok());
    if resolution.map(|expected| revision(&files) != expected).unwrap_or(false) {
        return Err(Error::Msg("The saves changed. Try syncing again.".into()));
    }
    let current_heads: HashSet<_> = heads(&files).iter().map(|f| f.id.as_str()).collect();
    let mut ticket = match old.filter(|t| {
        let parents: HashSet<_> = t.snapshot.parents.iter().map(String::as_str).collect();
        let reusable =
            resolution.is_none() || parents == current_heads || (current_heads.len() == 1 && current_heads.contains(t.id.as_str()));
        t.archive_hash == archive_hash && valid_id(&t.id) && reusable
    }) {
        Some(ticket) => ticket,
        None => {
            let value = json_response(
                send(
                    app,
                    client()
                        .get(format!("{API}/files/generateIds"))
                        .timeout(Duration::from_secs(20))
                        .query(&[("count", "1"), ("space", "drive")]),
                )
                .await?,
            )
            .await?;
            let file_id = value["ids"][0]
                .as_str()
                .filter(|s| valid_id(s))
                .ok_or_else(|| Error::Msg("Google couldn't allocate a save copy.".into()))?;
            let parents = heads(&files).iter().map(|f| f.id.clone()).collect();
            let generation = files.iter().map(|f| f.snapshot.generation).max().unwrap_or(0).saturating_add(1);
            let snapshot = Snapshot {
                format: 1,
                game: id.into(),
                hash: man.hash,
                made: man.made,
                device: man.device,
                played: man.played,
                generation,
                parents,
            };
            Ticket { id: file_id.into(), archive_hash, snapshot, url: None }
        }
    };
    let synced = saves::state(&app.state::<App>().dir, id).synced_hash;
    if resolution.is_none() && heads(&files).iter().any(|f| f.snapshot.hash != synced && f.snapshot.hash != ticket.snapshot.hash) {
        return Err(Error::Msg(cloud::SYNC_CONFLICT.into()));
    }
    save_ticket(&path, &ticket)?;
    upload_bytes(app, &path, &mut ticket, bytes).await?;
    let current = remote(app, id).await?;
    if copies(&current).first().map(|c| c.conflict).unwrap_or(false) {
        return Err(Error::Msg(cloud::SYNC_CONFLICT.into()));
    }
    let _ = std::fs::remove_file(&path);
    prune(app, &current).await;
    Ok(())
}

async fn prune(app: &AppHandle, files: &[Remote]) {
    // Conflicting branches and their ancestry are retained until the player resolves them.
    if heads(files).len() != 1 {
        return;
    }
    let mut ordered: Vec<_> = files.iter().collect();
    ordered.sort_by_key(|f| std::cmp::Reverse(f.snapshot.generation));
    for file in ordered.into_iter().skip(5) {
        let request = client().patch(format!("{API}/files/{}", file.id)).timeout(Duration::from_secs(20)).json(&json!({"trashed": true}));
        if send(app, request).await.is_err() {
            break;
        }
    }
}

pub async fn download(app: &AppHandle, id: &str, copy: &cloud::CloudCopy) -> Res<Vec<u8>> {
    if !valid_id(&copy.v) || copy.size > MAX_ARCHIVE as u64 {
        return Err(Error::Msg("That Google Drive save is too large or invalid.".into()));
    }
    let files = remote(app, id).await?;
    if !files.iter().any(|f| f.id == copy.v && f.snapshot.hash == copy.hash) {
        return Err(Error::Msg("The saves changed. Try syncing again.".into()));
    }
    let mut r = send(app, client().get(format!("{API}/files/{}", copy.v)).query(&[("alt", "media")])).await?;
    let mut bytes = Vec::new();
    while let Some(chunk) = r.chunk().await.map_err(transport)? {
        if bytes.len().saturating_add(chunk.len()) > MAX_ARCHIVE {
            return Err(Error::Msg("That Google Drive save is too large.".into()));
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.len() as u64 != copy.size {
        return Err(Error::Msg("The Google Drive save didn't arrive whole.".into()));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(id: &str, hash: &str, generation: u64, parents: &[&str]) -> Remote {
        Remote {
            id: id.into(),
            size: 32,
            snapshot: Snapshot {
                format: 1,
                game: "game".into(),
                hash: hash.into(),
                made: 100,
                device: id.into(),
                played: 60,
                generation,
                parents: parents.iter().map(|p| p.to_string()).collect(),
            },
        }
    }

    #[test]
    fn simultaneous_saves_are_conflicts_even_with_equal_times() {
        let files = vec![file("base", "old", 1, &[]), file("pc", "pc-save", 2, &["base"]), file("deck", "deck-save", 2, &["base"])];
        assert_eq!(heads(&files).len(), 2);
        assert!(copies(&files).iter().all(|c| c.conflict));
    }

    #[test]
    fn resolution_covers_both_branches() {
        let files = vec![
            file("pc", "pc-save", 2, &["base"]),
            file("deck", "deck-save", 2, &["base"]),
            file("resolved", "pc-save", 3, &["pc", "deck"]),
        ];
        let copies = copies(&files);
        assert_eq!(copies[0].v, "resolved");
        assert!(!copies[0].conflict);
    }

    #[test]
    fn identical_saves_from_two_devices_do_not_conflict() {
        let files = vec![file("pc", "same", 2, &["base"]), file("deck", "same", 2, &["base"])];
        assert!(!copies(&files)[0].conflict);
    }

    #[test]
    fn clock_skew_cannot_make_an_ancestor_current() {
        let mut old = file("old", "old", 1, &[]);
        old.snapshot.made = u64::MAX;
        let files = vec![old, file("new", "new", 2, &["old"])];
        assert_eq!(copies(&files)[0].v, "new");
    }

    #[test]
    fn trimming_old_ancestors_keeps_the_current_head() {
        let files = vec![file("four", "four", 4, &["three"]), file("five", "five", 5, &["four"])];
        assert_eq!(heads(&files).len(), 1);
        assert_eq!(copies(&files)[0].v, "five");
    }

    #[test]
    fn revision_changes_when_another_device_uploads() {
        let files = vec![file("pc", "one", 2, &["base"])];
        let mut next = files.clone();
        next.push(file("deck", "two", 2, &["base"]));
        assert_ne!(revision(&files), revision(&next));
    }

    #[test]
    fn revisions_ignore_listing_order() {
        let files = vec![file("pc", "one", 2, &["base"]), file("deck", "two", 2, &["base"])];
        let mut reordered = files.clone();
        reordered.reverse();
        assert_eq!(revision(&files), revision(&reordered));
    }

    #[test]
    fn callback_requires_the_original_state() {
        assert_eq!(callback("GET /oauth2callback?code=secret&state=other HTTP/1.1\r\n\r\n", "expected").unwrap(), None);
    }

    #[test]
    fn callback_requires_the_expected_path() {
        assert_eq!(callback("GET /favicon.ico?code=secret&state=expected HTTP/1.1\r\n\r\n", "expected").unwrap(), None);
    }

    #[test]
    fn callback_decodes_the_authorization_code() {
        let result = callback("GET /oauth2callback?code=abc%2Fdef&state=expected HTTP/1.1\r\n\r\n", "expected").unwrap();
        assert_eq!(result.as_deref(), Some("abc/def"));
    }

    #[test]
    fn consent_denial_does_not_return_a_code() {
        assert!(callback("GET /oauth2callback?error=access_denied&state=expected HTTP/1.1\r\n\r\n", "expected").is_err());
    }

    #[tokio::test]
    async fn browser_sign_in_accepts_the_loopback_callback() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(browser_code(listener, "expected".into()));
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        stream.write_all(b"GET /oauth2callback?code=approved&state=expected HTTP/1.1\r\nHost: localhost\r\n\r\n").await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert_eq!(server.await.unwrap().unwrap(), "approved");
    }

    #[test]
    fn resumable_upload_continues_after_the_last_received_byte() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("Range", "bytes=0-262143".parse().unwrap());
        assert_eq!(resume_offset(&headers, 524288).unwrap(), 262144);
    }

    #[test]
    fn resumable_upload_rejects_an_offset_past_the_archive() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("Range", "bytes=0-999999".parse().unwrap());
        assert!(resume_offset(&headers, 100).is_err());
    }

    #[test]
    fn upload_sessions_cannot_send_tokens_to_another_host() {
        assert!(valid_session("https://www.googleapis.com/upload/drive/v3/files?upload_id=abc"));
        assert!(!valid_session("https://www.googleapis.com.attacker.example/upload/drive/v3/files"));
        assert!(!valid_session("http://www.googleapis.com/upload/drive/v3/files"));
        assert!(!valid_session("https://www.googleapis.com:444/upload/drive/v3/files"));
    }

    #[test]
    fn rate_limit_errors_retry_without_pausing_the_website() {
        let e = api_error(403, &json!({"error": {"errors": [{"reason": "userRateLimitExceeded"}]}}));
        assert!(matches!(e, Error::Net(_)));
    }

    #[test]
    fn full_drive_gives_a_storage_error() {
        let e = api_error(403, &json!({"error": {"errors": [{"reason": "storageQuotaExceeded"}]}}));
        assert!(e.to_string().contains("Google Drive storage is full"));
    }
}

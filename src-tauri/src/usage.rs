//! What the launcher tells recomplauncher.com, when "Share usage and diagnostics" is on (Settings,
//! General): the PC it runs on, once a day; game sessions, games that close right after starting,
//! and installs that fail. And "How did it run?" reports, which the player sends themselves. Each
//! launcher has a random install ID; a signed-in launcher sends its account token too.
//! recompstore.com/privacy says all of this.

use crate::{catalog, App, Error};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use tauri::{AppHandle, Manager};

type Res<T> = Result<T, Error>;

fn api() -> String {
    format!("{}/api/launcher.php", catalog::base())
}

/// This launcher's random ID, made the first time it's needed.
pub fn install_id(dir: &Path) -> String {
    let f = dir.join("install.id");
    if let Ok(s) = std::fs::read_to_string(&f) {
        let s = s.trim().to_string();
        if s.len() == 32 && s.chars().all(|c| c.is_ascii_hexdigit()) {
            return s;
        }
    }
    let id: String = (0..16).map(|_| format!("{:02x}", rand_byte())).collect();
    let _ = std::fs::write(&f, &id);
    id
}

pub fn rand_byte() -> u8 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    (h.finish() & 0xff) as u8
}

/// The PC, as reports and usage describe it.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PcInfo {
    #[serde(default)]
    pub osver: String,
    #[serde(default)]
    pub cpu: String,
    #[serde(default)]
    pub cores: u32,
    #[serde(default)]
    pub ram_gb: u32,
    #[serde(default)]
    pub gpu: String,
    #[serde(default)]
    pub gpu_driver: String,
}

#[cfg(windows)]
fn read_pc() -> PcInfo {
    use std::os::windows::process::CommandExt;
    const PS: &str = r#"$o=@{};$os=Get-CimInstance Win32_OperatingSystem;$dv=(Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' -ErrorAction SilentlyContinue).DisplayVersion;$o.osver=(($os.Caption -replace '^Microsoft ','')+' '+$dv+' ('+$os.BuildNumber+')').Trim();$o.ram_gb=[int][math]::Round($os.TotalVisibleMemorySize/1MB);$c=Get-CimInstance Win32_Processor|Select-Object -First 1;$o.cpu=($c.Name -replace '\s+',' ').Trim();$o.cores=[int]$c.NumberOfLogicalProcessors;$g=@(Get-CimInstance Win32_VideoController|Where-Object{$_.Name -notmatch 'Basic Display|Remote|Virtual|Parsec|Meta|Mirage|DisplayLink'});$m=($g|Where-Object{$_.Name -match 'NVIDIA'}|Select-Object -First 1);if(-not $m){$m=($g|Where-Object{$_.Name -match 'Radeon RX|Arc'}|Select-Object -First 1)};if(-not $m){$m=$g|Select-Object -First 1};$o.gpu=[string]$m.Name;$o.gpu_driver=[string]$m.DriverVersion;$o|ConvertTo-Json -Compress"#;
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", PS])
        .creation_flags(0x0800_0000)
        .output();
    out.ok().and_then(|o| serde_json::from_slice::<PcInfo>(&o.stdout).ok()).unwrap_or_default()
}

#[cfg(not(windows))]
fn read_pc() -> PcInfo {
    let cpu = std::fs::read_to_string("/proc/cpuinfo").ok().and_then(|s| s.lines().find(|l| l.starts_with("model name")).and_then(|l| l.split(':').nth(1)).map(|x| x.trim().to_string())).unwrap_or_default();
    let ram_gb = std::fs::read_to_string("/proc/meminfo").ok().and_then(|s| s.lines().next().and_then(|l| l.split_whitespace().nth(1)).and_then(|k| k.parse::<u64>().ok())).map(|k| ((k as f64) / 1048576.0).round() as u32).unwrap_or(0);
    PcInfo { osver: std::env::consts::OS.into(), cpu, cores: std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(0), ram_gb, ..Default::default() }
}

/// The PC (asked of Windows once a day, kept in cache/pcinfo.json).
pub fn pc_info(dir: &Path) -> PcInfo {
    let f = dir.join("cache/pcinfo.json");
    let fresh = std::fs::metadata(&f).ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).map(|e| e.as_secs() < 86400).unwrap_or(false);
    if fresh {
        if let Some(p) = std::fs::read(&f).ok().and_then(|b| serde_json::from_slice::<PcInfo>(&b).ok()) {
            return p;
        }
    }
    let p = read_pc();
    if !p.cpu.is_empty() || !p.gpu.is_empty() {
        let _ = std::fs::create_dir_all(dir.join("cache"));
        let _ = std::fs::write(&f, serde_json::to_vec(&p).unwrap_or_default());
    }
    p
}

fn sharing(st: &App) -> bool {
    st.lib.lock().unwrap().settings.share_usage
}

fn token(dir: &Path) -> Option<String> {
    std::fs::read(dir.join("account.json")).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|v| v.get("token").and_then(Value::as_str).map(String::from)).filter(|t| !t.is_empty())
}

async fn post(st: &App, a: &str, body: Value) -> Res<Value> {
    let mut req = st.http.post(api()).query(&[("a", a)]).json(&body).timeout(std::time::Duration::from_secs(20));
    // Only a player's own report carries their account (it shows under their name). Usage and
    // diagnostics never do: "No names", as Settings says.
    if a == "report" {
        if let Some(t) = token(&st.dir) {
            req = req.header("X-RL-Token", t);
        }
    }
    let r = req.send().await.map_err(|e| Error::Net(e.to_string()))?;
    let code = r.status().as_u16();
    let v: Value = r.json().await.unwrap_or(Value::Null);
    if code >= 400 {
        return Err(Error::Msg(v.get("error").and_then(Value::as_str).unwrap_or("The website didn't take it. Try again later.").to_string()));
    }
    Ok(v)
}

/// Once a launch (at most once a day): the PC and how the launcher is used. Things that didn't get
/// through last time go too.
#[tauri::command]
pub async fn usage_hello(app: AppHandle, lang: String, screen: String) -> Res<()> {
    let st = app.state::<App>();
    if !sharing(&st) {
        return Ok(());
    }
    let mark = st.dir.join("cache/hello.day");
    let today = (crate::library::now() / 86400).to_string();
    if std::fs::read_to_string(&mark).map(|s| s == today).unwrap_or(false) {
        return send_pending(&st).await;
    }
    let dir = st.dir.clone();
    let pc = tokio::task::spawn_blocking(move || pc_info(&dir)).await.unwrap_or_default();
    let (games, bp) = {
        let l = st.lib.lock().unwrap();
        (l.installed.len(), l.settings.start_in_tv || l.settings.ui.get("bp_used").and_then(Value::as_bool).unwrap_or(false))
    };
    let pads: Vec<&str> = crate::pads::connected().iter().map(|p| if p.kind == "ps" { "playstation" } else { "xbox" }).collect();
    let mut kinds = pads.clone();
    kinds.sort();
    kinds.dedup();
    post(&st, "hello", json!({
        "install": install_id(&st.dir), "version": env!("CARGO_PKG_VERSION"), "os": std::env::consts::OS, "osver": pc.osver, "cpu": pc.cpu, "cores": pc.cores,
        "ram_gb": pc.ram_gb, "gpu": pc.gpu, "gpu_driver": pc.gpu_driver, "screen": screen, "lang": lang, "pads": kinds.join("+"), "games": games, "bigpicture": bp,
    }))
    .await?;
    let _ = std::fs::write(&mark, today);
    send_pending(&st).await
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Event {
    pub kind: String,
    pub at: u64,
    #[serde(default)]
    pub game: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub secs: u64,
    #[serde(default)]
    pub code: Option<i64>,
    #[serde(default)]
    pub detail: String,
}

fn pending_file(st: &App) -> std::path::PathBuf {
    st.dir.join("cache/events.json")
}

/// Something worth knowing happened: sent now, or kept for later (up to 200).
pub async fn event(app: &AppHandle, e: Event) {
    let st = app.state::<App>();
    if !sharing(&st) {
        return;
    }
    let f = pending_file(&st);
    let mut list: Vec<Event> = std::fs::read(&f).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    list.push(e);
    let n = list.len();
    if n > 200 {
        list.drain(..n - 200);
    }
    let _ = std::fs::create_dir_all(st.dir.join("cache"));
    let _ = std::fs::write(&f, serde_json::to_vec(&list).unwrap_or_default());
    let _ = send_pending(&st).await;
}

async fn send_pending(st: &App) -> Res<()> {
    let f = pending_file(st);
    let list: Vec<Event> = std::fs::read(&f).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    if list.is_empty() {
        return Ok(());
    }
    post(st, "events", json!({"install": install_id(&st.dir), "events": list})).await?;
    let _ = std::fs::remove_file(&f);
    Ok(())
}

/// From the window: an install or update that finished or failed, an uninstall.
#[tauri::command]
pub async fn usage_event(app: AppHandle, kind: String, game: String, version: Option<String>, detail: Option<String>) -> Res<()> {
    if !["install", "update", "uninstall", "error", "setup"].contains(&kind.as_str()) {
        return Ok(());
    }
    event(&app, Event { kind, at: crate::library::now(), game, version: version.unwrap_or_default(), detail: detail.unwrap_or_default().chars().take(250).collect(), ..Default::default() }).await;
    Ok(())
}

/// "How did it run?": the player's word, with the PC it was played on.
#[tauri::command]
pub async fn run_report(app: AppHandle, id: String, verdict: String, note: Option<String>, mins: Option<u64>) -> Res<()> {
    let st = app.state::<App>();
    let version = st.lib.lock().unwrap().installed.get(&id).map(|g| g.tag.clone()).unwrap_or_default();
    let dir = st.dir.clone();
    let pc = tokio::task::spawn_blocking(move || pc_info(&dir)).await.unwrap_or_default();
    post(&st, "report", json!({
        "install": install_id(&st.dir), "app": id, "verdict": verdict, "sys": std::env::consts::OS, "version": version, "note": note.unwrap_or_default(),
        "mins": mins.unwrap_or(0), "gpu": pc.gpu, "cpu": pc.cpu, "ram_gb": pc.ram_gb, "osver": pc.osver,
    }))
    .await?;
    Ok(())
}

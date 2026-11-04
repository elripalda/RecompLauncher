//! What the launcher tells recomplauncher.com, when "Share usage and diagnostics" is on (Settings,
//! General): the PC it runs on, once a day; game sessions, games that close right after starting,
//! and installs that fail, sent together every ten minutes or so rather than one by one. And "How did it run?" reports, which the player sends themselves. Each
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

/// The two random IDs (usage and diagnostics; anonymous stats), as Settings shows them so a player
/// can quote one when asking for its data to be removed.
#[tauri::command(async)]
pub fn data_ids(app: AppHandle) -> Value {
    let dir = app.state::<App>().dir.clone();
    json!({"usage": install_id(&dir), "stats": crate::ping::install(&dir)})
}

/// New IDs: whatever was sent before can't be tied to what's sent from now on.
#[tauri::command(async)]
pub fn reset_ids(app: AppHandle) -> Value {
    let dir = app.state::<App>().dir.clone();
    let _ = std::fs::remove_file(dir.join("install.id"));
    let _ = std::fs::remove_file(dir.join("stats.id"));
    data_ids(app)
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
    use crate::win;
    // The card games run on: not a remote-desktop or virtual one; NVIDIA, then a Radeon RX or Arc, then the first.
    let skip = ["basic display", "remote", "virtual", "parsec", "meta", "mirage", "displaylink"];
    let cards: Vec<(String, String)> = win::gpus().into_iter().filter(|(n, _)| !skip.iter().any(|s| n.to_lowercase().contains(s))).collect();
    let card = cards
        .iter()
        .find(|(n, _)| n.contains("NVIDIA"))
        .or_else(|| cards.iter().find(|(n, _)| n.contains("Radeon RX") || n.contains("Arc")))
        .or_else(|| cards.first())
        .cloned()
        .unwrap_or_default();
    PcInfo {
        osver: win::os_name(),
        cpu: win::cpu_name(),
        cores: std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(0),
        ram_gb: win::ram_gb(),
        gpu: card.0,
        gpu_driver: card.1,
    }
}

#[cfg(not(windows))]
fn read_pc() -> PcInfo {
    let cpu = std::fs::read_to_string("/proc/cpuinfo").ok().and_then(|s| s.lines().find(|l| l.starts_with("model name")).and_then(|l| l.split(':').nth(1)).map(|x| x.trim().to_string())).unwrap_or_default();
    let ram_gb = std::fs::read_to_string("/proc/meminfo").ok().and_then(|s| s.lines().next().and_then(|l| l.split_whitespace().nth(1)).and_then(|k| k.parse::<u64>().ok())).map(|k| ((k as f64) / 1048576.0).round() as u32).unwrap_or(0);
    // The system's own name ("SteamOS 3.7", "Ubuntu 24.04 LTS"), not just "linux".
    let osver = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|s| s.lines().find_map(|l| l.strip_prefix("PRETTY_NAME=").map(|v| v.trim().trim_matches('"').to_string())))
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| std::env::consts::OS.into());
    let (gpu, gpu_driver) = linux_gpu();
    PcInfo { osver, cpu, cores: std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(0), ram_gb, gpu, gpu_driver }
}

/// The graphics card on Linux, from /sys (no tools needed): NVIDIA, then AMD, then Intel, then
/// any other, skipping a virtual machine's. Named from the system's PCI list when it has one.
#[cfg(not(windows))]
fn linux_gpu() -> (String, String) {
    let rd = |p: &Path| std::fs::read_to_string(p).map(|s| s.trim().to_string()).unwrap_or_default();
    let mut cards: Vec<(u32, String, String, String)> = vec![];
    for e in std::fs::read_dir("/sys/class/drm").into_iter().flatten().flatten() {
        let n = e.file_name().to_string_lossy().to_string();
        if !(n.starts_with("card") && n[4..].chars().all(|c| c.is_ascii_digit()) && n.len() > 4) {
            continue;
        }
        let dev = e.path().join("device");
        let (vendor, device) = (rd(&dev.join("vendor")), rd(&dev.join("device")));
        let hex = |s: &str| s.trim_start_matches("0x").to_ascii_lowercase();
        let (vendor, device) = (hex(&vendor), hex(&device));
        if vendor.is_empty() {
            continue;
        }
        let driver = std::fs::read_link(dev.join("driver")).ok().and_then(|l| l.file_name().map(|f| f.to_string_lossy().to_string())).unwrap_or_default();
        let rank = match vendor.as_str() { "10de" => 0, "1002" => 1, "8086" => 2, "1af4" | "15ad" | "1234" | "80ee" | "1b36" => 9, _ => 3 };
        if !cards.iter().any(|c| c.1 == vendor && c.2 == device) {
            cards.push((rank, vendor, device, driver));
        }
    }
    cards.sort_by_key(|c| c.0);
    let Some((_, vendor, device, driver)) = cards.into_iter().next() else { return (String::new(), String::new()) };
    let maker = match vendor.as_str() { "10de" => "NVIDIA", "1002" => "AMD", "8086" => "Intel", _ => "" };
    // NVIDIA's own driver names the card and its version.
    let nv_model = || {
        std::fs::read_dir("/proc/driver/nvidia/gpus").ok()?.flatten().find_map(|g| {
            rd(&g.path().join("information")).lines().find_map(|l| l.strip_prefix("Model:").map(|m| m.trim().to_string())).filter(|m| !m.is_empty())
        })
    };
    let name = (vendor == "10de").then(nv_model).flatten().or_else(|| pci_name(&vendor, &device).map(|n| if maker.is_empty() || n.to_lowercase().contains(&maker.to_lowercase()) { n } else { format!("{maker} {n}") })).unwrap_or_else(|| if maker.is_empty() { format!("GPU {vendor}:{device}") } else { format!("{maker} GPU ({device})") });
    let version = if driver == "nvidia" {
        rd(Path::new("/proc/driver/nvidia/version")).lines().next().and_then(|l| l.split_whitespace().find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()) && w.contains('.'))).map(str::to_string).unwrap_or_default()
    } else {
        String::new()
    };
    (name, [driver, version].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" "))
}

/// A card's name from the system's PCI list ("VanGogh [AMD Custom GPU 0405]" → "AMD Custom GPU 0405").
#[cfg(not(windows))]
fn pci_name(vendor: &str, device: &str) -> Option<String> {
    let list = ["/usr/share/hwdata/pci.ids", "/usr/share/misc/pci.ids", "/usr/share/pci.ids"].iter().find_map(|p| std::fs::read_to_string(p).ok())?;
    pci_lookup(&list, vendor, device)
}

#[cfg(any(not(windows), test))]
fn pci_lookup(list: &str, vendor: &str, device: &str) -> Option<String> {
    let mut inside = false;
    for l in list.lines() {
        if l.starts_with('#') || l.is_empty() {
            continue;
        }
        if !l.starts_with('\t') {
            if inside {
                return None;
            }
            inside = l.get(..4).is_some_and(|v| v.eq_ignore_ascii_case(vendor));
        } else if inside && !l.starts_with("\t\t") {
            let l = &l[1..];
            if l.get(..4).is_some_and(|d| d.eq_ignore_ascii_case(device)) {
                let n = l[4..].trim();
                // The marketing name is in brackets when there is one.
                let shown = n.rfind('[').and_then(|a| n[a + 1..].find(']').map(|b| n[a + 1..a + 1 + b].to_string())).filter(|x| !x.trim().is_empty()).unwrap_or_else(|| n.to_string());
                return Some(shown.trim().to_string());
            }
        }
    }
    None
}

/// The PC (asked of Windows once a day, kept in cache/pcinfo.json).
pub fn pc_info(dir: &Path) -> PcInfo {
    let f = dir.join("cache/pcinfo.json");
    let fresh = std::fs::metadata(&f).ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).map(|e| e.as_secs() < 86400).unwrap_or(false);
    if fresh {
        // Linux didn't know its graphics card before, so a saved one without it is asked again.
        if let Some(p) = std::fs::read(&f).ok().and_then(|b| serde_json::from_slice::<PcInfo>(&b).ok()).filter(|p| !p.gpu.is_empty() || !cfg!(target_os = "linux")) {
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
    let s = &st.lib.lock().unwrap().settings;
    s.data_asked && s.share_usage
}

fn token(dir: &Path) -> Option<String> {
    crate::cloud::token(dir)
}

async fn post(st: &App, a: &str, body: Value) -> Res<Value> {
    let mut req = st.http.post(api()).query(&[("a", a)]).json(&body).timeout(crate::net::QUICK);
    // Only a player's own report carries their account (it shows under their name). Usage and
    // diagnostics never do: "No names", as Settings says.
    if a == "report" {
        if let Some(t) = token(&st.dir) {
            req = req.header("X-RL-Token", t);
        }
    }
    let url = api();
    let r = crate::net::send(req, &url, a == "report").await?;
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

/// A send of what's waiting is planned.
static FLUSH_PLANNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// What's waiting goes in one go, ten to fifteen minutes after the first of it (or with the next
/// launch's hello, whichever is first).
fn plan_flush(app: &AppHandle) {
    if FLUSH_PLANNED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(crate::net::spread(600, 0.5)).await;
        FLUSH_PLANNED.store(false, std::sync::atomic::Ordering::SeqCst);
        let _ = send_pending(&app.state::<App>()).await;
    });
}

/// Something worth knowing happened: kept, and sent with others a little later (up to 200 kept).
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
    plan_flush(app);
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

#[cfg(test)]
mod tests {
    use super::pci_lookup;

    #[test]
    fn a_graphics_card_is_named_from_the_pci_list() {
        let list = "# comment\n1002  Advanced Micro Devices, Inc. [AMD/ATI]\n\t163f  VanGogh [AMD Custom GPU 0405]\n\t\t1002 0123  Some subsystem\n\t73df  Navi 22 [Radeon RX 6700/6700 XT/6750 XT / 6800M/6850M XT]\n10de  NVIDIA Corporation\n\t2504  GA106 [GeForce RTX 3060 Lite Hash Rate]\n\t1234  Plain Name\n8086  Intel Corporation\n\t163f  Not this one\n";
        assert_eq!(pci_lookup(list, "1002", "163f").as_deref(), Some("AMD Custom GPU 0405"));
        assert_eq!(pci_lookup(list, "1002", "73DF").as_deref(), Some("Radeon RX 6700/6700 XT/6750 XT / 6800M/6850M XT"));
        assert_eq!(pci_lookup(list, "10de", "2504").as_deref(), Some("GeForce RTX 3060 Lite Hash Rate"));
        assert_eq!(pci_lookup(list, "10de", "1234").as_deref(), Some("Plain Name"));
        assert_eq!(pci_lookup(list, "1002", "0123"), None);
        assert_eq!(pci_lookup(list, "8086", "9999"), None);
    }
}

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

/// This PC's system, processor, memory and graphics card, for a crash report the player writes.
#[tauri::command(async)]
pub fn pc_specs(app: AppHandle) -> PcInfo {
    pc_info(&app.state::<App>().dir)
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

/// A Mac: sysctl for the processor and memory, sw_vers for the system, and the graphics from
/// System Information (Apple silicon's GPU is the chip's own).
#[cfg(target_os = "macos")]
fn read_pc() -> PcInfo {
    let out = |cmd: &str, args: &[&str]| std::process::Command::new(cmd).args(args).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let cpu = out("/usr/sbin/sysctl", &["-n", "machdep.cpu.brand_string"]);
    let ram_gb = out("/usr/sbin/sysctl", &["-n", "hw.memsize"]).parse::<u64>().map(|b| ((b as f64) / 1073741824.0).round() as u32).unwrap_or(0);
    let ver = out("/usr/bin/sw_vers", &["-productVersion"]);
    let osver = if ver.is_empty() { "macOS".to_string() } else { format!("macOS {ver}") };
    let (gpu, gpu_driver) = mac_gpu(&out("/usr/sbin/system_profiler", &["SPDisplaysDataType", "-json"]));
    let gpu = if gpu.is_empty() && cpu.starts_with("Apple") { format!("{cpu} GPU") } else { gpu };
    PcInfo { osver, cpu, cores: std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(0), ram_gb, gpu, gpu_driver }
}

/// The first graphics card in System Information's JSON, with its core count or Metal support.
#[cfg(any(target_os = "macos", test))]
fn mac_gpu(json: &str) -> (String, String) {
    let v: Value = serde_json::from_str(json).unwrap_or(Value::Null);
    let Some(card) = v.get("SPDisplaysDataType").and_then(Value::as_array).and_then(|a| a.first()) else { return (String::new(), String::new()) };
    let s = |k: &str| card.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let name = [s("sppci_model"), s("_name")].into_iter().find(|x| !x.is_empty()).unwrap_or_default();
    let cores = s("sppci_cores");
    let detail = if !cores.is_empty() { format!("{cores} cores") } else { s("spdisplays_mtlgpufamilysupport").replace("spdisplays_", "") };
    (name, detail)
}

#[cfg(all(not(windows), not(target_os = "macos")))]
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
#[cfg(all(not(windows), not(target_os = "macos")))]
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
#[cfg(all(not(windows), not(target_os = "macos")))]
fn pci_name(vendor: &str, device: &str) -> Option<String> {
    let list = ["/usr/share/hwdata/pci.ids", "/usr/share/misc/pci.ids", "/usr/share/pci.ids"].iter().find_map(|p| std::fs::read_to_string(p).ok())?;
    pci_lookup(&list, vendor, device)
}

#[cfg(any(all(not(windows), not(target_os = "macos")), test))]
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
    // Gated by its action: usage and reports rest on their own and never mark the site Offline.
    let url = format!("{}?a={a}", api());
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

/// How a game's profile did, for the admin (only when the player allows sharing): which version of
/// the profile (its fingerprint), the step (`files`: Easy Setup, `settings`, `start`), whether it
/// worked, and the error in short. No paths or names.
pub async fn profile_event(app: &AppHandle, id: &str, step: &str, ok: bool, error: &str) {
    let st = app.state::<App>();
    let cat = crate::cached_catalog(&st);
    let Some(entry) = crate::catalog::app(&cat, id) else { return };
    let hash = crate::boot::hash_of(entry);
    if hash.is_empty() {
        return;
    }
    let version = st.lib.lock().unwrap().installed.get(id).map(|g| g.tag.clone()).unwrap_or_default();
    let detail = json!({"profile_hash": hash, "step": step, "ok": ok, "error": scrub(error)}).to_string();
    event(app, Event { kind: "profile".into(), at: crate::library::now(), game: id.into(), version, detail, ..Default::default() }).await;
}

/// An error as it goes to the site: anything that looks like a path (or carries the player's name)
/// left out, and short.
fn scrub(e: &str) -> String {
    let user = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_default();
    let words: Vec<String> = e
        .split_whitespace()
        .map(|w| if w.contains('/') || w.contains('\\') || (w.len() > 1 && w.as_bytes()[1] == b':') || (!user.is_empty() && w.to_lowercase().contains(&user.to_lowercase())) { "…".to_string() } else { w.to_string() })
        .collect();
    words.join(" ").chars().take(160).collect()
}

/// From the window: how an Easy Setup or a settings change went.
#[tauri::command]
pub async fn profile_report(app: AppHandle, id: String, step: String, ok: bool, error: Option<String>) -> Res<()> {
    if !["files", "settings", "start"].contains(&step.as_str()) {
        return Ok(());
    }
    profile_event(&app, &id, &step, ok, error.as_deref().unwrap_or("")).await;
    Ok(())
}

// ---------- "How did it run?": reports developers can act on ----------

/// The site's PC store API: reports, the player's own reports, answering a developer.
fn pc_api() -> String {
    format!("{}/api/pc.php", catalog::base())
}

/// A log goes with a report as its last 2 MB.
pub const LOG_MAX: usize = 2 << 20;

/// The PC in one short line, as the player sees it before sending: "Windows 11 23H2 · Ryzen 7
/// 5800X · RTX 3070 (driver 566.36) · 32 GB".
pub fn specs_line(pc: &PcInfo) -> String {
    let tidy = |x: &str, drop: &[&str]| {
        let mut s = x.replace("(R)", "").replace("(TM)", "").replace("(tm)", "");
        for d in drop {
            s = s.replace(d, " ");
        }
        let s = regex_lite::Regex::new(r"\s*@\s*[\d.]+\s*GHz|\b\d+-Core\b|\bProcessor\b|\bCPU\b|\bwith Radeon Graphics\b").unwrap().replace_all(&s, " ").to_string();
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    let cpu = tidy(&pc.cpu, &["AMD ", "Intel "]);
    let gpu = tidy(&pc.gpu, &["NVIDIA ", "GeForce ", "AMD ", "Intel "]);
    let driver = pc.gpu_driver.split_whitespace().last().unwrap_or("").to_string();
    let gpu = if gpu.is_empty() || driver.is_empty() || !driver.chars().next().is_some_and(|c| c.is_ascii_digit()) { gpu } else { format!("{gpu} (driver {driver})") };
    let ram = if pc.ram_gb > 0 { format!("{} GB", pc.ram_gb) } else { String::new() };
    [pc.osver.trim().to_string(), cpu, gpu, ram].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" · ")
}

/// What it's played on, as the site names it.
fn sys_now() -> &'static str {
    if crate::shortcuts::is_steam_deck() {
        "deck"
    } else {
        std::env::consts::OS
    }
}

/// A log as it goes with a report: its last 2 MB (from the start of a line), as text.
pub fn log_tail(bytes: &[u8], max: usize) -> String {
    let cut = if bytes.len() > max {
        let from = bytes.len() - max;
        let at = bytes[from..].iter().position(|b| *b == b'\n').map(|i| from + i + 1).filter(|i| *i < bytes.len()).unwrap_or(from);
        &bytes[at..]
    } else {
        bytes
    };
    let mut text = String::from_utf8_lossy(cut).replace('\0', " ");
    // The lossy text can come out a little longer (a broken character becomes three bytes).
    while text.len() > max {
        let mut i = text.len() - max;
        while !text.is_char_boundary(i) {
            i += 1;
        }
        text = text[i..].to_string();
    }
    text
}

/// A report as the site takes it: the fields, and the log when there's one. Refused here the same
/// way the site would: a bad report says what happened.
#[derive(Debug)]
pub struct ReportForm {
    pub fields: Vec<(&'static str, String)>,
    pub log: Option<(String, String)>,
}

pub fn report_form(id: &str, verdict: &str, sys: &str, note: &str, version: &str, specs: &str, log: Option<(String, String)>) -> Result<ReportForm, String> {
    if !["runs", "issues", "broken"].contains(&verdict) {
        return Err("Pick how it plays.".into());
    }
    let note = note.trim();
    if verdict != "runs" && note.chars().count() < 15 {
        return Err("Tell the developer what happened and when.".into());
    }
    Ok(ReportForm {
        fields: vec![
            ("a", "feedback".into()),
            ("id", id.into()),
            ("verdict", verdict.into()),
            ("sys", sys.into()),
            ("note", note.chars().take(1000).collect()),
            ("version", version.chars().take(40).collect()),
            ("specs", specs.chars().take(600).collect()),
        ],
        log: log.filter(|(_, t)| !t.trim().is_empty()),
    })
}

/// A report from Linux or a Steam Deck on a Windows build played through Proton says so, next to
/// where it was played.
pub fn mark_proton(form: &mut ReportForm, proton: bool) {
    if proton && !form.fields.iter().any(|(k, _)| *k == "proton") {
        let at = form.fields.iter().position(|(k, _)| *k == "sys").map(|i| i + 1).unwrap_or(form.fields.len());
        form.fields.insert(at, ("proton", "true".into()));
    }
}

/// The report as multipart/form-data.
pub fn multipart(form: &ReportForm, boundary: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for (k, v) in &form.fields {
        out.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n").as_bytes());
    }
    if let Some((name, text)) = &form.log {
        let name: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')).collect();
        out.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"log\"; filename=\"{}\"\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n", if name.is_empty() { "game.log" } else { &name }).as_bytes());
        out.extend(text.as_bytes());
        out.extend(b"\r\n");
    }
    out.extend(format!("--{boundary}--\r\n").as_bytes());
    out
}

/// The game's newest log, for a report: its file name and last 2 MB.
fn report_log(st: &App, id: &str) -> Option<(String, String)> {
    let p = crate::recent_log_path(st, id, 0)?;
    let bytes = std::fs::read(&p).ok()?;
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "game.log".into());
    Some((name, log_tail(&bytes, LOG_MAX)))
}

/// What goes with a report, shown to the player before they send it: their PC in a line, the
/// version they have, and the log the launcher would attach (its name and size), if it has one.
#[tauri::command]
pub async fn report_preview(app: AppHandle, id: String) -> Res<Value> {
    let st = app.state::<App>();
    let version = st.lib.lock().unwrap().installed.get(&id).map(|g| g.tag.clone()).unwrap_or_default();
    let dir = st.dir.clone();
    let pc = tokio::task::spawn_blocking(move || pc_info(&dir)).await.unwrap_or_default();
    let log = crate::recent_log_path(&st, &id, 0).and_then(|p| {
        let size = std::fs::metadata(&p).ok()?.len();
        Some(json!({"name": p.file_name()?.to_string_lossy(), "size": size.min(LOG_MAX as u64)}))
    });
    let device = crate::shortcuts::handheld_name(&pc.cpu);
    Ok(json!({"specs": specs_line(&pc), "version": version, "sys": sys_now(), "log": log, "device": device}))
}

/// "How did it run?": the player's word, the version they have and their PC in a line, with the
/// game's log when they leave it on. Signed in, it's tied to their account so they hear back.
#[tauri::command]
pub async fn run_report(app: AppHandle, id: String, verdict: String, note: Option<String>, mins: Option<u64>, log: Option<bool>) -> Res<Value> {
    let _ = mins;
    let st = app.state::<App>();
    let version = st.lib.lock().unwrap().installed.get(&id).map(|g| g.tag.clone()).unwrap_or_default();
    let dir = st.dir.clone();
    let pc = tokio::task::spawn_blocking(move || pc_info(&dir)).await.unwrap_or_default();
    let attach = if log.unwrap_or(false) { report_log(&st, &id) } else { None };
    let mut form = report_form(&id, &verdict, sys_now(), note.as_deref().unwrap_or(""), &version, &specs_line(&pc), attach).map_err(Error::Msg)?;
    // On Linux, a Windows build: it ran through Proton.
    let proton = !cfg!(windows) && st.lib.lock().unwrap().installed.get(&id).map(|g| g.proton).unwrap_or(false);
    mark_proton(&mut form, proton);
    // Played on a handheld: which one, so the site can tell which games run well on them.
    if let Some(d) = crate::shortcuts::handheld_name(&pc.cpu) {
        form.fields.push(("device", d.into()));
    }
    let boundary = format!("----RecompLauncher{}", (0..12).map(|_| format!("{:02x}", rand_byte())).collect::<String>());
    let url = pc_api();
    let mut req = st
        .http
        .post(&url)
        .header("Content-Type", format!("multipart/form-data; boundary={boundary}"))
        .body(multipart(&form, &boundary))
        .timeout(std::time::Duration::from_secs(60));
    if let Some(t) = token(&st.dir) {
        req = req.header("X-RL-Token", t);
    }
    let r = crate::net::send(req, &url, true).await?;
    let code = r.status().as_u16();
    let v: Value = r.json().await.unwrap_or(Value::Null);
    if code >= 400 {
        return Err(Error::Msg(v.get("error").and_then(Value::as_str).unwrap_or("The website didn't take it. Try again later.").to_string()));
    }
    Ok(v)
}

/// The Coming Soon games the signed-in player asked to hear about (one email when each comes out).
/// Empty when they aren't signed in.
#[tauri::command]
pub async fn notifies(app: AppHandle) -> Res<Vec<String>> {
    let st = app.state::<App>();
    let Some(t) = token(&st.dir) else { return Ok(vec![]) };
    let url = pc_api();
    let req = st.http.get(&url).query(&[("a", "notifies")]).bearer_auth(&t).header("X-RL-Token", t.as_str()).timeout(crate::net::QUICK);
    let r = crate::net::send(req, &url, true).await?;
    let v: Value = r.json().await.unwrap_or(Value::Null);
    Ok(v.get("apps").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default())
}

/// Notify Me on a Coming Soon game: on (one email when it comes out) or off. The site's own words
/// when it says no (an email that isn't confirmed yet, say).
#[tauri::command]
pub async fn notify(app: AppHandle, id: String, on: bool) -> Res<bool> {
    let st = app.state::<App>();
    let t = token(&st.dir).ok_or_else(|| Error::Msg("Sign in first.".into()))?;
    if id.is_empty() || id.len() > 120 || !id.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)) {
        return Err(Error::Msg("That isn't a recomp.".into()));
    }
    let url = pc_api();
    let body = format!("a=notify&app={id}&on={}", if on { 1 } else { 0 });
    let req = st.http.post(&url).header("Content-Type", "application/x-www-form-urlencoded").body(body).bearer_auth(&t).header("X-RL-Token", t.as_str()).timeout(crate::net::QUICK);
    let r = crate::net::send(req, &url, true).await?;
    let v: Value = r.json().await.unwrap_or(Value::Null);
    if !v.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        return Err(Error::Msg(v.get("error").and_then(Value::as_str).unwrap_or("The website didn't take it. Try again later.").to_string()));
    }
    Ok(v.get("on").and_then(|x| x.as_bool().or_else(|| x.as_i64().map(|n| n != 0))).unwrap_or(on))
}

/// The signed-in player's reports, with what developers answered.
#[tauri::command]
pub async fn my_reports(app: AppHandle) -> Res<Value> {
    let st = app.state::<App>();
    let t = token(&st.dir).ok_or_else(|| Error::Msg("Sign in to see your reports.".into()))?;
    let url = pc_api();
    let req = st.http.get(&url).query(&[("a", "my-reports")]).header("X-RL-Token", t).timeout(crate::net::QUICK);
    let r = crate::net::send(req, &url, true).await?;
    let code = r.status().as_u16();
    let v: Value = r.json().await.unwrap_or(Value::Null);
    if code >= 400 {
        return Err(Error::Msg(v.get("error").and_then(Value::as_str).unwrap_or("Your reports couldn't be loaded. Try again later.").to_string()));
    }
    Ok(v)
}

/// The player answers a developer on one of their reports.
#[tauri::command]
pub async fn report_reply(app: AppHandle, id: i64, text: String) -> Res<Value> {
    let st = app.state::<App>();
    let t = token(&st.dir).ok_or_else(|| Error::Msg("Sign in to answer.".into()))?;
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(Error::Msg("Write your answer first.".into()));
    }
    let url = pc_api();
    let req = st.http.post(&url).json(&json!({"a": "report-reply", "id": id, "text": text})).header("X-RL-Token", t).timeout(crate::net::QUICK);
    let r = crate::net::send(req, &url, true).await?;
    let code = r.status().as_u16();
    let v: Value = r.json().await.unwrap_or(Value::Null);
    if code >= 400 {
        return Err(Error::Msg(v.get("error").and_then(Value::as_str).unwrap_or("Your answer didn't go through. Try again later.").to_string()));
    }
    Ok(v)
}

// ---------- Request a recomp: a project link the player wants in the launcher ----------

/// The longest link and note a request carries. Anything longer is a paste gone wrong.
const REQUEST_URL_MAX: usize = 300;
const REQUEST_NOTE_MAX: usize = 500;

/// A project link as the player typed it, made into one the site can open: https, a real host, and
/// a path to the project itself (github.com alone says nothing). A bare "github.com/owner/repo"
/// gets its https; a trailing slash or ".git" goes, so the same project reads the same twice.
/// The project has to be on GitHub or GitLab, as for submissions on the website.
pub fn project_link(input: &str) -> Result<String, String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err("Paste the project's link first.".into());
    }
    if raw.len() > REQUEST_URL_MAX || raw.chars().any(char::is_whitespace) {
        return Err("That doesn't look like a project link.".into());
    }
    let with_scheme = if raw.contains("://") { raw.to_string() } else { format!("https://{raw}") };
    let Ok(mut u) = reqwest::Url::parse(&with_scheme) else {
        return Err("That doesn't look like a project link.".into());
    };
    if u.scheme() == "http" {
        let _ = u.set_scheme("https");
    }
    if u.scheme() != "https" {
        return Err("Use the project's web link (https://…).".into());
    }
    // A sign-in tucked into the link would go to the site in the open.
    if !u.username().is_empty() || u.password().is_some() {
        return Err("Leave the sign-in out of the link.".into());
    }
    let host = u.host_str().unwrap_or("").to_ascii_lowercase();
    if !host.contains('.') || host.starts_with('.') || host.ends_with('.') {
        return Err("That doesn't look like a project link.".into());
    }
    let host = host.strip_prefix("www.").unwrap_or(&host).to_string();
    if host != "github.com" && host != "gitlab.com" {
        return Err("Request a project on GitHub or GitLab.".into());
    }
    let _ = u.set_host(Some(&host));
    let path = u.path().trim_end_matches('/').trim_end_matches(".git").trim_end_matches('/').to_string();
    if path.is_empty() {
        return Err("Link to the project itself, not just the site.".into());
    }
    u.set_path(&path);
    u.set_fragment(None);
    Ok(u.to_string())
}

/// Asks recomplauncher.com for a recomp the launcher doesn't list yet: its project link, and what
/// the player wants to say about it. A signed-in player's request goes under their name, so the
/// site can tell them when it's added; anyone else's goes with the install's random id only.
#[tauri::command]
pub async fn recomp_request(app: AppHandle, url: String, note: Option<String>) -> Res<Value> {
    let st = app.state::<App>();
    let link = project_link(&url).map_err(Error::Msg)?;
    let note: String = note.unwrap_or_default().trim().chars().take(REQUEST_NOTE_MAX).collect();
    let version = app.package_info().version.to_string();
    let body = json!({"a": "recomp-request", "install": install_id(&st.dir), "url": link, "note": note, "version": version});
    let api = pc_api();
    let mut req = st.http.post(&api).query(&[("a", "recomp-request")]).json(&body).timeout(crate::net::QUICK);
    if let Some(t) = token(&st.dir) {
        req = req.header("X-RL-Token", t);
    }
    let r = crate::net::send(req, &api, true).await?;
    let code = r.status().as_u16();
    let v: Value = r.json().await.unwrap_or(Value::Null);
    if code >= 400 || !v.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        return Err(Error::Msg(v.get("error").and_then(Value::as_str).unwrap_or("The website didn't take the request. Try again later.").to_string()));
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_macs_graphics_come_from_system_information() {
        let j = r#"{"SPDisplaysDataType":[{"_name":"kHW_AppleM2Item","sppci_model":"Apple M2","sppci_cores":"10"}]}"#;
        assert_eq!(mac_gpu(j), ("Apple M2".to_string(), "10 cores".to_string()));
        let intel = r#"{"SPDisplaysDataType":[{"_name":"Intel Iris Plus","spdisplays_mtlgpufamilysupport":"spdisplays_metal3"}]}"#;
        assert_eq!(mac_gpu(intel), ("Intel Iris Plus".to_string(), "metal3".to_string()));
        assert_eq!(mac_gpu("not json"), (String::new(), String::new()));
    }

    #[test]
    fn profile_errors_leave_paths_out() {
        let e = scrub("Couldn't open C:\\Users\\Ruben\\game.iso: not found at /home/ruben/x");
        assert!(!e.contains("Users") && !e.contains("/home") && e.contains("not found"));
        assert!(scrub(&"x ".repeat(300)).chars().count() <= 160);
    }

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

    #[test]
    fn a_report_says_what_happened_and_always_carries_specs_and_version() {
        // Has problems / Won't run need a note of 15 characters or more; Plays great doesn't.
        assert!(report_form("g", "issues", "windows", "", "v1.2", "Windows 11", None).is_err());
        assert!(report_form("g", "broken", "windows", "it crashed", "v1.2", "Windows 11", None).is_err());
        assert!(report_form("g", "issues", "windows", "Crashes after the intro on level one.", "v1.2", "Windows 11", None).is_ok());
        assert!(report_form("g", "runs", "windows", "", "v1.2", "Windows 11", None).is_ok());
        assert!(report_form("g", "great", "windows", "", "v1.2", "Windows 11", None).is_err());
        // Version and specs go every time, even empty.
        let mut p = report_form("g", "runs", "deck", "", "", "", None).unwrap();
        mark_proton(&mut p, true);
        let names: Vec<&str> = p.fields.iter().map(|(k, _)| *k).collect();
        assert_eq!(names[names.iter().position(|k| *k == "sys").unwrap() + 1], "proton");
        assert!(String::from_utf8(multipart(&p, "b")).unwrap().contains("name=\"proton\"\r\n\r\ntrue\r\n"));
        let mut n = report_form("g", "runs", "linux", "", "", "", None).unwrap();
        mark_proton(&mut n, false);
        assert!(!n.fields.iter().any(|(k, _)| *k == "proton"));
        let f = report_form("g", "runs", "deck", "", "", "", None).unwrap();
        let names: Vec<&str> = f.fields.iter().map(|(k, _)| *k).collect();
        assert!(names.contains(&"version") && names.contains(&"specs") && names.contains(&"sys") && names.contains(&"note"));
        let f = report_form("g", "broken", "windows", "Black screen right after the logo.", "v2.0", "Windows 11 · RTX 3070", Some(("game.log".into(), "line 1\nline 2\n".into()))).unwrap();
        let body = String::from_utf8(multipart(&f, "XYZ")).unwrap();
        assert!(body.contains("name=\"a\"\r\n\r\nfeedback\r\n"));
        assert!(body.contains("name=\"version\"\r\n\r\nv2.0\r\n"));
        assert!(body.contains("name=\"specs\"\r\n\r\nWindows 11 · RTX 3070\r\n"));
        assert!(body.contains("name=\"log\"; filename=\"game.log\"\r\nContent-Type: text/plain"));
        assert!(body.ends_with("--XYZ--\r\n"));
    }

    #[test]
    fn a_long_log_goes_as_its_last_2_mb() {
        let mut big = Vec::new();
        let mut n = 0u32;
        while big.len() < 3 * 1024 * 1024 {
            big.extend(format!("line {n} of the game's log\n").as_bytes());
            n += 1;
        }
        let tail = log_tail(&big, LOG_MAX);
        assert!(tail.len() <= LOG_MAX && tail.len() > LOG_MAX - 100);
        assert!(tail.starts_with("line "), "starts on a whole line");
        assert!(tail.ends_with(&format!("line {} of the game's log\n", n - 1)));
        // A short one goes whole; nothing that isn't text gets through.
        assert_eq!(log_tail(b"ok\0fine\n", LOG_MAX), "ok fine\n");
    }

    #[test]
    fn the_pc_in_one_short_line() {
        let pc = PcInfo { osver: "Windows 11 23H2".into(), cpu: "AMD Ryzen 7 5800X 8-Core Processor".into(), cores: 16, ram_gb: 32, gpu: "NVIDIA GeForce RTX 3070".into(), gpu_driver: "566.36".into() };
        assert_eq!(specs_line(&pc), "Windows 11 23H2 · Ryzen 7 5800X · RTX 3070 (driver 566.36) · 32 GB");
        let pc = PcInfo { osver: "SteamOS 3.7.13".into(), cpu: "AMD Custom APU 0405".into(), ram_gb: 15, gpu: "AMD Custom GPU 0405".into(), gpu_driver: "amdgpu".into(), ..Default::default() };
        assert_eq!(specs_line(&pc), "SteamOS 3.7.13 · Custom APU 0405 · Custom GPU 0405 · 15 GB");
        let pc = PcInfo { osver: "Windows 10".into(), cpu: "Intel(R) Core(TM) i7-9700K CPU @ 3.60GHz".into(), ..Default::default() };
        assert_eq!(specs_line(&pc), "Windows 10 · Core i7-9700K");
    }

    #[test]
    fn a_requested_project_link_keeps_its_forge() {
        assert_eq!(project_link("https://github.com/owner/repo").unwrap(), "https://github.com/owner/repo");
        assert_eq!(project_link("https://gitlab.com/group/sub/repo").unwrap(), "https://gitlab.com/group/sub/repo");
        assert_eq!(project_link("https://www.github.com/owner/repo").unwrap(), "https://github.com/owner/repo");
    }

    #[test]
    fn a_requested_project_is_on_github_or_gitlab() {
        assert!(project_link("https://codeberg.org/owner/repo").is_err());
        assert!(project_link("https://example.com/owner/repo").is_err());
        assert!(project_link("https://github.com.example.com/owner/repo").is_err());
    }

    #[test]
    fn a_requested_project_link_is_tidied() {
        assert_eq!(project_link("  github.com/owner/repo/ ").unwrap(), "https://github.com/owner/repo");
        assert_eq!(project_link("http://github.com/owner/repo.git").unwrap(), "https://github.com/owner/repo");
        assert_eq!(project_link("https://github.com/owner/repo#readme").unwrap(), "https://github.com/owner/repo");
    }

    #[test]
    fn a_requested_project_link_must_point_at_a_project() {
        assert!(project_link("").is_err());
        assert!(project_link("https://github.com/").is_err());
        assert!(project_link("not a link").is_err());
        assert!(project_link("localhost/repo").is_err());
        assert!(project_link("ftp://example.com/repo").is_err());
        assert!(project_link("https://user:pass@github.com/owner/repo").is_err());
        assert!(project_link(&format!("https://github.com/{}", "a".repeat(400))).is_err());
    }
}

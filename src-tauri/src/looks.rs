//! Looks: filters the player picks for a game (Sharp, Vivid, Film, CRT, Studio monitor).
//!
//! They're our own ReShade effects (looks/*.fx). ReShade itself is downloaded once from its
//! developer and kept up to date. Nothing is installed into Windows: when a game starts with a look,
//! ReShade goes next to the game as dxgi.dll (Direct3D) or is added as a Vulkan layer for that one
//! process, and the dxgi.dll is taken away again when the game closes. Its settings and log live in
//! the launcher's own folder (looks/<game>), found through RESHADE_BASE_PATH_OVERRIDE.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The effects, built into the launcher.
const SHADERS: &[(&str, &str)] = &[
    ("RL.fxh", include_str!("../looks/RL.fxh")),
    ("RL_Sharp.fx", include_str!("../looks/RL_Sharp.fx")),
    ("RL_Vivid.fx", include_str!("../looks/RL_Vivid.fx")),
    ("RL_Film.fx", include_str!("../looks/RL_Film.fx")),
    ("RL_CRT.fx", include_str!("../looks/RL_CRT.fx")),
];

/// The looks and how their amount maps to each effect's settings. The window reads the same file
/// for its previews.
pub const LOOKS_JSON: &str = include_str!("../../ui/looks.json");

/// ReShade's own releases. The first tag is the newest (ReShade's own update check reads it the same way).
const TAGS_URL: &str = "https://api.github.com/repos/crosire/reshade/tags";
/// Used when GitHub can't be reached and there's no ReShade yet.
const KNOWN_VERSION: &str = "6.8.0";
/// How often to look for a newer ReShade.
const CHECK_EVERY: u64 = 3 * 24 * 3600;

#[derive(Debug, Clone, Deserialize)]
pub struct LookDef {
    pub id: String,
    pub name: String,
    pub fx: String,
    pub technique: String,
    #[serde(default)]
    pub lines: bool,
    #[serde(default)]
    pub amount: BTreeMap<String, [f32; 2]>,
    #[serde(default)]
    pub fixed: BTreeMap<String, f32>,
}

#[derive(Debug, Deserialize)]
struct Defs {
    looks: Vec<LookDef>,
    lines: BTreeMap<String, u32>,
    default_lines: u32,
}

fn defs() -> &'static Defs {
    static D: OnceLock<Defs> = OnceLock::new();
    D.get_or_init(|| serde_json::from_str(LOOKS_JSON).expect("ui/looks.json"))
}

pub fn find(id: &str) -> Option<&'static LookDef> {
    defs().looks.iter().find(|l| l.id == id)
}

/// How many lines the console sent to the TV, for the CRT looks.
pub fn lines_for(origin: &str) -> u32 {
    defs().lines.get(&origin.to_ascii_lowercase()).copied().unwrap_or(defs().default_lines)
}

/// What a profile can say about looks (`"looks"` in recomplauncher.json).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProfileLooks {
    /// false: this game has its own filters, or ReShade doesn't work with it.
    #[serde(default = "yes")]
    pub allowed: bool,
    /// Why not, shown to the player.
    #[serde(default)]
    pub reason: String,
    /// "d3d12", "d3d11", "d3d9", "opengl" or "vulkan". Left out: worked out from the program.
    #[serde(default)]
    pub graphics: String,
    /// A settings key that holds the graphics API the game will use (in the config named by `in`).
    #[serde(default)]
    pub graphics_key: String,
    #[serde(default = "main")]
    pub r#in: String,
    /// Lines for the CRT looks, when the console's usual number is wrong for this game.
    #[serde(default)]
    pub lines: u32,
}

fn yes() -> bool {
    true
}
fn main() -> String {
    "main".into()
}

// ---------- the graphics API ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Api {
    /// Direct3D 10, 11 or 12, through dxgi.dll.
    Dxgi,
    D3d9,
    Opengl,
    Vulkan,
    /// Either Direct3D or Vulkan, chosen by the game at start: both ways in, one ReShade.
    Both,
}

impl Api {
    pub fn from_name(s: &str) -> Option<Api> {
        let s = s.to_ascii_lowercase();
        Some(match () {
            _ if s.contains("vulkan") || s == "vk" => Api::Vulkan,
            _ if s.contains("12") || s.contains("11") || s.contains("10") || s == "dx" || s == "d3d" || s == "dxgi" => Api::Dxgi,
            _ if s.contains('9') => Api::D3d9,
            _ if s.contains("gl") => Api::Opengl,
            _ => return None,
        })
    }
    fn proxy(self) -> Option<&'static str> {
        match self {
            Api::Dxgi | Api::Both => Some("dxgi.dll"),
            Api::D3d9 => Some("d3d9.dll"),
            Api::Opengl => Some("opengl32.dll"),
            Api::Vulkan => None,
        }
    }
    fn layer(self) -> bool {
        matches!(self, Api::Vulkan | Api::Both)
    }
}

fn has(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle))
}

/// What a Windows program's header says: 64-bit or not, and the DLLs it imports (delay-loaded
/// ones too). None when it isn't a Windows program.
pub struct Pe {
    pub x64: bool,
    pub imports: Vec<String>,
}

pub fn read_pe(b: &[u8]) -> Option<Pe> {
    let u16_at = |o: usize| b.get(o..o + 2).map(|x| u16::from_le_bytes([x[0], x[1]]));
    let u32_at = |o: usize| b.get(o..o + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]));
    if !b.starts_with(b"MZ") {
        return None;
    }
    let pe = u32_at(0x3C)? as usize;
    if b.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    let coff = pe + 4;
    let machine = u16_at(coff)?;
    let sections = u16_at(coff + 2)? as usize;
    let opt_size = u16_at(coff + 16)? as usize;
    let opt = coff + 20;
    let magic = u16_at(opt)?;
    let dirs = match magic {
        0x20b => opt + 112,
        0x10b => opt + 96,
        _ => return None,
    };
    let count = u32_at(dirs - 4)? as usize;
    let sect = opt + opt_size;
    let at = |rva: u32| -> Option<usize> {
        (0..sections.min(96)).find_map(|i| {
            let h = sect + i * 40;
            let (vsize, va, raw_size, raw) = (u32_at(h + 8)?, u32_at(h + 12)?, u32_at(h + 16)?, u32_at(h + 20)?);
            let size = vsize.max(raw_size);
            if rva >= va && rva < va.saturating_add(size) { (rva - va).checked_add(raw).map(|x| x as usize) } else { None }
        })
    };
    let name_at = |rva: u32| -> Option<String> {
        // An odd or packed program can point anywhere: never past the end of the file.
        let s = b.get(at(rva)?..)?;
        let end = s.iter().take(260).position(|&c| c == 0)?;
        Some(String::from_utf8_lossy(&s[..end]).to_ascii_lowercase())
    };
    let mut imports = vec![];
    // Ordinary imports: 20-byte entries, the DLL's name at +12.
    if count > 1 {
        if let Some(mut o) = u32_at(dirs + 8).filter(|&r| r != 0).and_then(at) {
            for _ in 0..512 {
                let Some(n) = u32_at(o + 12) else { break };
                if n == 0 {
                    break;
                }
                if let Some(name) = name_at(n) {
                    imports.push(name);
                }
                o += 20;
            }
        }
    }
    // Delay-loaded imports: 32-byte entries, the DLL's name at +4.
    if count > 13 {
        if let Some(mut o) = u32_at(dirs + 13 * 8).filter(|&r| r != 0).and_then(at) {
            for _ in 0..512 {
                let Some(n) = u32_at(o + 4) else { break };
                if n == 0 {
                    break;
                }
                if let Some(name) = name_at(n) {
                    imports.push(name);
                }
                o += 32;
            }
        }
    }
    Some(Pe { x64: machine == 0x8664, imports })
}

/// Which API a program draws with: the graphics DLLs it imports, and failing that the ones it names
/// anywhere (a program can load them itself).
pub fn detect(exe: &Path) -> Api {
    use std::io::Read;
    let mut bytes = vec![];
    let Ok(f) = std::fs::File::open(exe) else { return Api::Both };
    let _ = f.take(96 << 20).read_to_end(&mut bytes);
    let imp = read_pe(&bytes).map(|p| p.imports).unwrap_or_default();
    let imports = |n: &[&str]| imp.iter().any(|i| n.contains(&i.as_str()));
    let d3d_imp = imports(&["d3d12.dll", "d3d11.dll", "dxgi.dll", "d3d10.dll", "d3d10_1.dll"]);
    let vk_named = has(&bytes, b"vulkan-1.dll");
    if d3d_imp {
        return if vk_named { Api::Both } else { Api::Dxgi };
    }
    if imports(&["vulkan-1.dll"]) {
        return Api::Vulkan;
    }
    if imports(&["opengl32.dll"]) {
        return Api::Opengl;
    }
    if imports(&["d3d9.dll"]) {
        return Api::D3d9;
    }
    // Nothing imported: whatever it names (it loads the library itself).
    let d3d = has(&bytes, b"d3d12.dll") || has(&bytes, b"d3d11.dll") || has(&bytes, b"dxgi.dll");
    match (d3d, vk_named) {
        (true, true) => Api::Both,
        (true, false) => Api::Dxgi,
        (false, true) => Api::Vulkan,
        _ if has(&bytes, b"d3d9.dll") => Api::D3d9,
        _ if has(&bytes, b"opengl32.dll") => Api::Opengl,
        _ => Api::Both,
    }
}

// A game that can draw with either Direct3D or Vulkan gets ReShade both ways in at first. When
// that crashes it right at the start (some graphics drivers present Vulkan through Direct3D, and
// ReShade then sees both), the next start tries Vulkan alone, then Direct3D alone, and the way that
// works is kept for that game.

fn route_file(data: &Path, id: &str) -> PathBuf {
    data.join("looks").join(id).join("route.txt")
}

/// The way in for this start, and the name of the route tried (for a game that can go either way).
pub fn route_for(data: &Path, id: &str, api: Api) -> (Api, Option<&'static str>) {
    if api != Api::Both {
        return (api, None);
    }
    match std::fs::read_to_string(route_file(data, id)).unwrap_or_default().trim() {
        "layer" => (Api::Vulkan, Some("layer")),
        "proxy" => (Api::Dxgi, Some("proxy")),
        _ => (Api::Both, Some("both")),
    }
}

/// After a game with a look closes: one that went down within seconds tries the next way in.
pub fn after_run(data: &Path, id: &str, route: &str, secs: u64, code: Option<i32>) {
    let crashed = secs < 20 && code.map(|c| c != 0).unwrap_or(false);
    if !crashed {
        return;
    }
    let next = match route {
        "both" => "layer",
        "layer" => "proxy",
        _ => "both",
    };
    let f = route_file(data, id);
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(f, next);
}

/// 64-bit (true), 32-bit (false), or unknown (None).
pub fn is_64bit(exe: &Path) -> Option<bool> {
    use std::io::Read;
    let mut head = vec![];
    std::fs::File::open(exe).ok()?.take(4096).read_to_end(&mut head).ok()?;
    let pe = u32::from_le_bytes(head.get(0x3C..0x40)?.try_into().ok()?) as usize;
    if !head.starts_with(b"MZ") || head.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    Some(u16::from_le_bytes(head.get(pe + 4..pe + 6)?.try_into().ok()?) == 0x8664)
}

// ---------- ReShade itself ----------

pub fn reshade_dir(data: &Path) -> PathBuf {
    data.join("reshade")
}

/// The ReShade the launcher has, if any.
pub fn have_version(data: &Path) -> Option<String> {
    let dir = reshade_dir(data);
    if !dir.join("ReShade64.dll").is_file() {
        return None;
    }
    let v = std::fs::read_to_string(dir.join("version.txt")).ok()?;
    v.lines().next().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn checked_at(data: &Path) -> u64 {
    std::fs::read_to_string(reshade_dir(data).join("version.txt")).ok().and_then(|v| v.lines().nth(1).and_then(|s| s.trim().parse().ok())).unwrap_or(0)
}

/// Where to get ReShade: the catalog can name a copy (with its checksum) for when reshade.me can't
/// be reached; otherwise ReShade's own site.
#[derive(Debug, Clone, Default)]
pub struct Source {
    pub version: String,
    pub url: String,
    pub sha256: String,
}

fn parse_version(tag: &str) -> Option<String> {
    let v = tag.trim().trim_start_matches(['v', 'V']);
    let ok = !v.is_empty() && v.split('.').count() >= 2 && v.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    ok.then(|| v.to_string())
}

fn newer(a: &str, b: &str) -> bool {
    let n = |s: &str| s.split('.').map(|p| p.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>();
    n(a) > n(b)
}

async fn latest(http: &reqwest::Client) -> Option<String> {
    let r = http.get(TAGS_URL).timeout(std::time::Duration::from_secs(10)).send().await.ok()?;
    let v: serde_json::Value = r.error_for_status().ok()?.json().await.ok()?;
    v.as_array()?.iter().filter_map(|t| t.get("name")?.as_str()).filter_map(parse_version).next()
}

/// The ReShade DLLs (64-bit, 32-bit) from a ReShade setup program: the setup carries a zip after
/// its own code.
pub fn dll_from_setup(setup: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    use std::io::Read;
    let bad = || "That download wasn't a ReShade setup.".to_string();
    // Same as ReShade's own setup: the zip starts on a 512-byte boundary with a local file header.
    let start = (0..setup.len())
        .step_by(512)
        .find(|&i| setup[i..].starts_with(b"PK\x03\x04") && setup[i + 4..(i + 30).min(setup.len())].iter().any(|&b| b != 0))
        .ok_or_else(bad)?;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&setup[start..])).map_err(|_| bad())?;
    let mut get = |name: &str| -> Result<Vec<u8>, String> {
        let mut f = zip.by_name(name).map_err(|_| bad())?;
        let mut out = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut out).map_err(|_| bad())?;
        if !out.starts_with(b"MZ") || out.len() < 256 * 1024 {
            return Err(bad());
        }
        Ok(out)
    };
    Ok((get("ReShade64.dll")?, get("ReShade32.dll")?))
}

fn reshade_url(v: &str) -> String {
    format!("https://reshade.me/downloads/ReShade_Setup_{v}.exe")
}

/// One download at a time (two shortcuts started together, say).
static ENSURE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Make sure there's a ReShade, and that it's the newest (checked every few days). `say` is told
/// what's happening, for the window. Keeps an older ReShade when a newer one can't be had.
pub async fn ensure(http: &reqwest::Client, data: &Path, mirror: Option<Source>, say: &(dyn Fn(&str) + Send + Sync)) -> Result<String, String> {
    let _one = ENSURE.lock().await;
    let dir = reshade_dir(data);
    let have = have_version(data);
    if let Some(h) = have.as_ref().filter(|_| crate::library::now().saturating_sub(checked_at(data)) < CHECK_EVERY) {
        return Ok(h.clone());
    }
    let stamp = |v: &str| {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("version.txt"), format!("{v}\n{}\n", crate::library::now()));
    };
    // Where to look, in order: the catalog's copy, the newest on reshade.me, the one this launcher
    // was made with.
    let mut sources: Vec<Source> = vec![];
    if let Some(m) = mirror.filter(|m| !m.url.is_empty() && parse_version(&m.version).is_some()) {
        sources.push(m);
    }
    if let Some(v) = latest(http).await {
        sources.push(Source { url: reshade_url(&v), version: v, sha256: String::new() });
    }
    sources.push(Source { url: reshade_url(KNOWN_VERSION), version: KNOWN_VERSION.into(), sha256: String::new() });
    let mut last_err = String::new();
    for src in sources {
        if let Some(h) = &have {
            if !newer(&src.version, h) {
                continue;
            }
        }
        say(&format!("Getting ReShade {}", src.version));
        let got: Result<(Vec<u8>, Vec<u8>), String> = async {
            let r = http.get(&src.url).timeout(std::time::Duration::from_secs(120)).send().await.map_err(|e| e.to_string())?;
            let r = r.error_for_status().map_err(|e| e.to_string())?;
            let b = r.bytes().await.map_err(|e| e.to_string())?.to_vec();
            if !src.sha256.is_empty() && !hex::encode(Sha256::digest(&b)).eq_ignore_ascii_case(&src.sha256) {
                return Err("The download didn't match its checksum.".into());
            }
            dll_from_setup(&b)
        }
        .await;
        let (dll64, dll32) = match got {
            Ok(d) => d,
            Err(e) => {
                last_err = e;
                continue;
            }
        };
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let tag = format!("{}-{}", std::process::id(), crate::library::now());
        let saved = [("ReShade64.dll", &dll64), ("ReShade32.dll", &dll32)].iter().all(|(name, bytes)| {
            let tmp = dir.join(format!("{name}.{tag}.new"));
            // A game using it as a Vulkan layer right now keeps the file busy: the new one is
            // used next time.
            let ok = std::fs::write(&tmp, bytes).is_ok() && std::fs::rename(&tmp, dir.join(name)).is_ok();
            let _ = std::fs::remove_file(&tmp);
            ok
        });
        if !saved {
            last_err = "Couldn't save ReShade.".into();
            continue;
        }
        stamp(&src.version);
        return Ok(src.version);
    }
    match have {
        // Nothing newer to be had right now: keep this one, and look again in a few days.
        Some(h) => {
            stamp(&h);
            Ok(h)
        }
        None => Err(format!("Couldn't get ReShade: {last_err}")),
    }
}

// ---------- a game's look ----------

/// The value a setting gets at this amount (0 to 100).
pub fn values(look: &LookDef, amount: u8) -> BTreeMap<String, f32> {
    let t = amount.min(100) as f32 / 100.0;
    let mut v: BTreeMap<String, f32> = look.fixed.clone();
    for (k, [a, b]) in &look.amount {
        v.insert(k.clone(), a + (b - a) * t);
    }
    v
}

/// A ReShade preset with just this look.
pub fn preset(look: &LookDef, amount: u8, lines: u32) -> String {
    let tech = format!("{}@{}", look.technique, look.fx);
    let mut s = String::new();
    if look.lines {
        s += &format!("PreprocessorDefinitions=RL_LINES={lines}\n");
    }
    s += &format!("Techniques={tech}\nTechniqueSorting={tech}\n\n[{}]\n", look.fx);
    for (k, v) in values(look, amount) {
        // Settings that are whole numbers (the mask type) are written as whole numbers.
        if k == "MaskType" {
            s += &format!("{k}={}\n", v.round() as i32);
        } else {
            s += &format!("{k}={v:.6}\n");
        }
    }
    s
}

/// Set keys in an INI file's text, keeping everything else (ReShade writes its own settings back).
pub fn ini_set(text: &str, sets: &[(&str, &str, String)]) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    for (section, key, value) in sets {
        let head = format!("[{section}]");
        let start = lines.iter().position(|l| l.trim().eq_ignore_ascii_case(&head));
        let Some(start) = start else {
            if lines.last().map(|l| !l.trim().is_empty()).unwrap_or(false) {
                lines.push(String::new());
            }
            lines.push(head);
            lines.push(format!("{key}={value}"));
            continue;
        };
        let end = lines[start + 1..].iter().position(|l| l.trim_start().starts_with('[')).map(|i| start + 1 + i).unwrap_or(lines.len());
        let at = lines[start + 1..end].iter().position(|l| l.split('=').next().map(|k| k.trim().eq_ignore_ascii_case(key)).unwrap_or(false));
        match at {
            Some(i) => lines[start + 1 + i] = format!("{key}={value}"),
            None => {
                // After the section's last setting, before any blank lines.
                let mut i = end;
                while i > start + 1 && lines[i - 1].trim().is_empty() {
                    i -= 1;
                }
                lines.insert(i, format!("{key}={value}"));
            }
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

fn write_if_changed(path: &Path, text: &str) -> std::io::Result<()> {
    if std::fs::read_to_string(path).map(|t| t == text).unwrap_or(false) {
        return Ok(());
    }
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(path, text)
}

/// Our own name for the Vulkan layer, so the loader can't mistake it for a ReShade installed for
/// the whole PC.
const LAYER: &str = "VK_LAYER_recomplauncher_reshade";

/// What starting a game with a look needs: environment for the process, and a file to take away
/// when it closes.
#[derive(Debug, Default)]
pub struct Run {
    pub env: Vec<(String, String)>,
    pub placed: Option<PathBuf>,
}

/// What the launcher noted when it put ReShade in a game's folder: the file and its checksum.
fn read_note(note: &Path) -> Option<(PathBuf, String)> {
    let t = std::fs::read_to_string(note).ok()?;
    let mut it = t.lines();
    Some((PathBuf::from(it.next()?.trim()), it.next()?.trim().to_ascii_lowercase()))
}

fn sum_of(file: &Path) -> Option<String> {
    std::fs::read(file).ok().map(|b| hex::encode(Sha256::digest(&b)))
}

/// A file in a game's folder is ours only when the launcher noted putting exactly that file there.
/// Anything else (the player's own ReShade, even the same version) is left alone.
fn ours(note: &Path, file: &Path) -> bool {
    match (read_note(note), sum_of(file)) {
        (Some((at, sum)), Some(have)) => at == file && sum == have,
        _ => false,
    }
}

/// Get a game ready to start with a look: effects, settings, preset, and ReShade where the game
/// will load it.
pub fn prepare(data: &Path, id: &str, exe: &Path, look: &LookDef, amount: u8, lines: u32, api: Api) -> Result<Run, String> {
    let rs = reshade_dir(data);
    // A 32-bit game needs the 32-bit ReShade.
    let x64 = is_64bit(exe).unwrap_or(true);
    let dll_path = rs.join(if x64 { "ReShade64.dll" } else { "ReShade32.dll" });
    let dll = std::fs::read(&dll_path).map_err(|_| "ReShade isn't downloaded yet.".to_string())?;
    let shaders = rs.join("shaders");
    for (name, text) in SHADERS {
        write_if_changed(&shaders.join(name), text).map_err(|e| e.to_string())?;
    }
    let base = data.join("looks").join(id);
    std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;

    // The preset: written again only when the look changes, so tweaks made in ReShade's own menu
    // (Home, in the game) stay until the player picks something else here.
    let preset_path = base.join(format!("{}.ini", look.id));
    let key = format!("{} {} {} {}", look.id, amount, lines, hex::encode(&Sha256::digest(LOOKS_JSON.as_bytes())[..6]));
    let stamp = base.join("look.txt");
    if !preset_path.is_file() || std::fs::read_to_string(&stamp).map(|t| t.trim() != key).unwrap_or(true) {
        std::fs::write(&preset_path, preset(look, amount, lines)).map_err(|e| e.to_string())?;
        std::fs::write(&stamp, &key).map_err(|e| e.to_string())?;
    }

    let ini_path = base.join("ReShade.ini");
    let text = std::fs::read_to_string(&ini_path).unwrap_or_default();
    let p = |x: &Path| x.display().to_string();
    // ReShade's INI splits values at commas; a doubled comma is a comma.
    let v = |x: &Path| p(x).replace(',', ",,");
    let ini = ini_set(
        &text,
        &[
            ("GENERAL", "EffectSearchPaths", v(&shaders)),
            ("GENERAL", "TextureSearchPaths", v(&shaders)),
            ("GENERAL", "PresetPath", v(&preset_path)),
            ("GENERAL", "IntermediateCachePath", v(&rs.join("cache"))),
            ("GENERAL", "SkipLoadingDisabledEffects", "1".into()),
            ("GENERAL", "NoReloadOnInit", "0".into()),
            // The launcher takes screenshots itself (F12).
            ("INPUT", "KeyScreenshot", "0,0,0,0".into()),
            ("OVERLAY", "TutorialProgress", "4".into()),
        ],
    );
    write_if_changed(&ini_path, &ini).map_err(|e| e.to_string())?;
    let _ = std::fs::create_dir_all(rs.join("cache"));

    let mut run = Run { env: vec![("RESHADE_BASE_PATH_OVERRIDE".into(), p(&base))], placed: None };
    let note = base.join("placed.txt");
    let mut layer_lib = dll_path.clone();
    if let Some(name) = api.proxy() {
        let dir = exe.parent().ok_or("No game folder.")?;
        let target = dir.join(name);
        if target.exists() && !ours(&note, &target) {
            return Err(format!("The game's folder already has a {name}, maybe your own ReShade or another mod, so the look is off. Take it out of the folder to use looks."));
        }
        if dir.join("ReShade.ini").exists() && !target.exists() {
            return Err("The game's folder has its own ReShade settings, so the launcher's look is off. Take ReShade out of the folder to use looks.".into());
        }
        std::fs::write(&target, &dll).map_err(|e| format!("Couldn't put ReShade in the game's folder: {e}"))?;
        std::fs::write(&note, format!("{}\n{}\n", p(&target), hex::encode(Sha256::digest(&dll)))).map_err(|e| e.to_string())?;
        run.placed = Some(target.clone());
        layer_lib = target;
    }
    if api.layer() {
        // A Vulkan layer for this process only. A ReShade layer installed for the whole PC is
        // switched off for it, so the look isn't applied twice.
        let manifest = serde_json::json!({
            "file_format_version": "1.0.0",
            "layer": {
                "name": LAYER,
                "type": "GLOBAL",
                "library_path": p(&layer_lib),
                "api_version": "1.3.268",
                "implementation_version": "1",
                "description": "ReShade, for RecompLauncher looks"
            }
        });
        let vk = base.join("vk");
        write_if_changed(&vk.join("ReShade.json"), &serde_json::to_string_pretty(&manifest).unwrap()).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(vk.join("ReShade64.json"));
        let add = |k: &str, v: &str, sep: char| {
            let cur = std::env::var(k).unwrap_or_default();
            if cur.is_empty() {
                v.to_string()
            } else {
                format!("{v}{sep}{cur}")
            }
        };
        let list_sep = if cfg!(windows) { ';' } else { ':' };
        // VK_LAYER_PATH is read by every Vulkan loader, old and new.
        run.env.push(("VK_LAYER_PATH".into(), add("VK_LAYER_PATH", &p(&vk), list_sep)));
        run.env.push(("VK_INSTANCE_LAYERS".into(), add("VK_INSTANCE_LAYERS", LAYER, list_sep)));
        run.env.push(("VK_LOADER_LAYERS_ENABLE".into(), add("VK_LOADER_LAYERS_ENABLE", LAYER, ',')));
        run.env.push(("DISABLE_VK_LAYER_reshade_1".into(), "1".into()));
    }
    Ok(run)
}

// ---------- Linux: vkBasalt ----------
//
// On Linux the same effects run through vkBasalt, a Vulkan layer most distros package. It reads
// ReShade effects but not ReShade presets, so each game gets its own copy of the look's effect
// with the player's amount written in as the settings' starting values.

/// Where Vulkan looks for layers that are on by default, in the order the loader reads them.
fn implicit_layer_dirs() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = vec![];
    if let Some(d) = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| dirs::home_dir().map(|h| h.join(".local/share"))) {
        v.push(d.join("vulkan/implicit_layer.d"));
    }
    for d in std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into()).split(':').filter(|d| !d.is_empty()) {
        v.push(Path::new(d).join("vulkan/implicit_layer.d"));
    }
    for d in ["/etc/vulkan/implicit_layer.d", "/usr/share/vulkan/implicit_layer.d", "/usr/local/share/vulkan/implicit_layer.d", "/usr/lib/extensions/vulkan/vkBasalt/share/vulkan/implicit_layer.d"] {
        v.push(PathBuf::from(d));
    }
    v
}

/// Is vkBasalt installed?
pub fn vkbasalt_installed() -> bool {
    implicit_layer_dirs().iter().any(|d| {
        std::fs::read_dir(d).into_iter().flatten().flatten().any(|e| e.file_name().to_string_lossy().to_lowercase().contains("vkbasalt"))
    })
}

/// An effect with the look's settings written in as their starting values (and the number of
/// lines for the CRT look).
pub fn bake(fx: &str, values: &BTreeMap<String, f32>, lines: Option<u32>) -> String {
    let mut out = fx.to_string();
    for (k, v) in values {
        let re = regex_lite::Regex::new(&format!(r"(?s)(uniform\s+(float|int)\s+{}\s*<.*?>\s*=\s*)([^;]+)(;)", regex_lite::escape(k))).unwrap();
        out = re
            .replace(&out, |c: &regex_lite::Captures| {
                let val = if &c[2] == "int" { format!("{}", v.round() as i32) } else { format!("{v:.6}") };
                format!("{}{}{}", &c[1], val, &c[4])
            })
            .into_owned();
    }
    match lines {
        Some(n) => format!("#define RL_LINES {n}\n{out}"),
        None => out,
    }
}

/// Get a game ready to start with a look through vkBasalt: the effect, its settings and
/// vkBasalt's own settings file, all in the launcher's folder. Nothing goes into the game's folder.
pub fn prepare_vkbasalt(data: &Path, id: &str, look: &LookDef, amount: u8, lines: u32) -> Result<Run, String> {
    let dir = data.join("looks").join(id).join("vkbasalt");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for (name, text) in SHADERS {
        if *name == look.fx {
            write_if_changed(&dir.join(name), &bake(text, &values(look, amount), look.lines.then_some(lines))).map_err(|e| e.to_string())?;
        } else if name.ends_with(".fxh") {
            write_if_changed(&dir.join(name), text).map_err(|e| e.to_string())?;
        }
    }
    let p = |x: &Path| x.display().to_string();
    let conf = format!(
        "# Written by RecompLauncher for the {} look. It's written again when the game starts.\neffects = rl\nrl = {}\nreshadeIncludePath = {}\nreshadeTexturePath = {}\ndepthCapture = off\ntoggleKey = Home\nenableOnLaunch = True\n",
        look.name,
        p(&dir.join(&look.fx)),
        p(&dir),
        p(&dir)
    );
    let file = dir.join("vkBasalt.conf");
    write_if_changed(&file, &conf).map_err(|e| e.to_string())?;
    Ok(Run { env: vec![("ENABLE_VKBASALT".into(), "1".into()), ("VKBASALT_CONFIG_FILE".into(), p(&file))], placed: None })
}

/// After the game closes: take ReShade back out of its folder. A file that's still in use is
/// tried again for a few seconds; whatever's left is tidied when the launcher next opens, or the
/// game next starts.
pub fn finish(data: &Path, id: &str, placed: &Path) {
    for _ in 0..20 {
        if !placed.exists() || std::fs::remove_file(placed).is_ok() {
            let _ = std::fs::remove_file(data.join("looks").join(id).join("placed.txt"));
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
}

/// Take away anything the launcher left in a game's folder (it closed while the game ran, or the
/// file was busy). Only the exact file it noted putting there.
pub fn tidy(data: &Path, id: &str) {
    let note = data.join("looks").join(id).join("placed.txt");
    let Some((file, _)) = read_note(&note) else { return };
    let named = file.file_name().and_then(|n| n.to_str()).map(|n| ["dxgi.dll", "d3d9.dll", "opengl32.dll"].contains(&n.to_ascii_lowercase().as_str())).unwrap_or(false);
    if named && ours(&note, &file) && std::fs::remove_file(&file).is_err() {
        return; // still busy: try again later
    }
    let _ = std::fs::remove_file(&note);
}

/// Screenshots taken while a look was on (the window shows the others as the preview, so the look
/// isn't drawn over itself).
pub fn note_shot(data: &Path, id: &str, file: &Path) {
    use std::io::Write;
    let path = data.join("looks").join(id).join("shots.txt");
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{}", file.file_name().and_then(|n| n.to_str()).unwrap_or_default());
    }
}

pub fn shots_with_look(data: &Path, id: &str) -> Vec<String> {
    std::fs::read_to_string(data.join("looks").join(id).join("shots.txt")).map(|t| t.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rl-looks-{n}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn looks_file_reads_and_every_look_has_its_effect() {
        assert!(defs().looks.len() >= 5);
        for l in &defs().looks {
            assert!(SHADERS.iter().any(|(n, _)| *n == l.fx), "{}", l.fx);
            let fx = SHADERS.iter().find(|(n, _)| *n == l.fx).unwrap().1;
            assert!(fx.contains(&format!("technique {}", l.technique)), "{}", l.technique);
            for k in l.amount.keys().chain(l.fixed.keys()) {
                assert!(fx.contains(&format!("uniform float {k}")) || fx.contains(&format!("uniform int {k}")), "{} has no {k}", l.fx);
            }
        }
        assert_eq!(lines_for("n64"), 240);
        assert_eq!(lines_for("N64"), 240);
        assert_eq!(lines_for("something"), 480);
    }

    #[test]
    fn a_crashing_look_tries_the_next_way_in() {
        let d = tmp("route");
        assert_eq!(route_for(&d, "g", Api::Dxgi), (Api::Dxgi, None));
        assert_eq!(route_for(&d, "g", Api::Both), (Api::Both, Some("both")));
        after_run(&d, "g", "both", 120, Some(1)); // played a while: nothing changes
        assert_eq!(route_for(&d, "g", Api::Both).1, Some("both"));
        after_run(&d, "g", "both", 5, Some(-2147483645));
        assert_eq!(route_for(&d, "g", Api::Both), (Api::Vulkan, Some("layer")));
        after_run(&d, "g", "layer", 3, Some(-1073741819));
        assert_eq!(route_for(&d, "g", Api::Both), (Api::Dxgi, Some("proxy")));
        after_run(&d, "g", "proxy", 300, Some(0));
        assert_eq!(route_for(&d, "g", Api::Both).1, Some("proxy"));
    }

    #[test]
    fn baked_for_vkbasalt() {
        let fx = "uniform float Curvature <\n\tui_type = \"slider\";\n> = 0.5;\nuniform int MaskType < ui_min = 0; > = 2;\nuniform float Other < > = 1.0;\n";
        let mut v = BTreeMap::new();
        v.insert("Curvature".to_string(), 0.25f32);
        v.insert("MaskType".to_string(), 1.0f32);
        let out = bake(fx, &v, Some(240));
        assert!(out.starts_with("#define RL_LINES 240\n"));
        assert!(out.contains("> = 0.250000;"), "{out}");
        assert!(out.contains("ui_min = 0; > = 1;"), "{out}");
        assert!(out.contains("Other < > = 1.0;"));
        // Every built-in look bakes without losing its technique.
        for l in &defs().looks {
            let text = SHADERS.iter().find(|(n, _)| *n == l.fx).unwrap().1;
            let b = bake(text, &values(l, 60), l.lines.then_some(224));
            assert!(b.contains(&format!("technique {}", l.technique)));
            for k in values(l, 60).keys() {
                assert!(text.contains(&format!(" {k} ")) || text.contains(&format!(" {k}<")) || text.contains(&format!(" {k}\n")), "{} has no {k}", l.fx);
            }
        }
    }

    #[test]
    fn presets() {
        let crt = find("crt").unwrap();
        let p = preset(crt, 50, 240);
        assert!(p.starts_with("PreprocessorDefinitions=RL_LINES=240\nTechniques=RL_CRT@RL_CRT.fx\n"));
        assert!(p.contains("[RL_CRT.fx]\n"));
        assert!(p.contains("MaskType=1\n"));
        let v = values(crt, 0);
        assert_eq!(v["Curvature"], crt.amount["Curvature"][0]);
        let v = values(crt, 100);
        assert!((v["Curvature"] - crt.amount["Curvature"][1]).abs() < 1e-6);
        assert!(!preset(find("sharp").unwrap(), 50, 240).contains("RL_LINES"));
    }

    #[test]
    fn ini_keeps_what_reshade_wrote() {
        let t = "[GENERAL]\nEffectSearchPaths=old\nPerformanceMode=0\n\n[INPUT]\nKeyOverlay=36,0,0,0\n";
        let out = ini_set(t, &[("GENERAL", "EffectSearchPaths", "new".into()), ("GENERAL", "PresetPath", "p.ini".into()), ("OVERLAY", "TutorialProgress", "4".into())]);
        assert_eq!(out, "[GENERAL]\nEffectSearchPaths=new\nPerformanceMode=0\nPresetPath=p.ini\n\n[INPUT]\nKeyOverlay=36,0,0,0\n\n[OVERLAY]\nTutorialProgress=4\n");
        assert_eq!(ini_set("", &[("A", "b", "c".into())]), "[A]\nb=c\n");
    }

    /// A small Windows program header: 64-bit or not, importing `imports` and delay-loading `delayed`.
    fn fake_pe(x64: bool, imports: &[&str], delayed: &[&str]) -> Vec<u8> {
        let mut b = vec![0u8; 0x2000];
        b[0..2].copy_from_slice(b"MZ");
        b[0x3C..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        b[0x40..0x44].copy_from_slice(b"PE\0\0");
        let coff = 0x44;
        b[coff..coff + 2].copy_from_slice(&(if x64 { 0x8664u16 } else { 0x14c }).to_le_bytes());
        b[coff + 2..coff + 4].copy_from_slice(&1u16.to_le_bytes());
        let opt = coff + 20;
        let (magic, dirs, opt_size) = if x64 { (0x20bu16, opt + 112, 240u16) } else { (0x10b, opt + 96, 224) };
        b[coff + 16..coff + 18].copy_from_slice(&opt_size.to_le_bytes());
        b[opt..opt + 2].copy_from_slice(&magic.to_le_bytes());
        b[dirs - 4..dirs].copy_from_slice(&16u32.to_le_bytes());
        // One section: file 0x400.. is RVA 0x1000..
        let sect = opt + opt_size as usize;
        b[sect + 8..sect + 12].copy_from_slice(&0x1000u32.to_le_bytes());
        b[sect + 12..sect + 16].copy_from_slice(&0x1000u32.to_le_bytes());
        b[sect + 16..sect + 20].copy_from_slice(&0x1000u32.to_le_bytes());
        b[sect + 20..sect + 24].copy_from_slice(&0x400u32.to_le_bytes());
        let rva = |off: usize| (off - 0x400 + 0x1000) as u32;
        let mut names = 0x800;
        let mut put_name = |b: &mut Vec<u8>, n: &str| {
            b[names..names + n.len()].copy_from_slice(n.as_bytes());
            let r = rva(names);
            names += n.len() + 1;
            r
        };
        b[dirs + 8..dirs + 12].copy_from_slice(&rva(0x400).to_le_bytes());
        for (i, n) in imports.iter().enumerate() {
            let r = put_name(&mut b, n);
            let o = 0x400 + i * 20 + 12;
            b[o..o + 4].copy_from_slice(&r.to_le_bytes());
        }
        b[dirs + 13 * 8..dirs + 13 * 8 + 4].copy_from_slice(&rva(0x600).to_le_bytes());
        for (i, n) in delayed.iter().enumerate() {
            let r = put_name(&mut b, n);
            let o = 0x600 + i * 32 + 4;
            b[o..o + 4].copy_from_slice(&r.to_le_bytes());
        }
        b
    }

    #[test]
    fn api_from_the_program() {
        let d = tmp("api");
        let f = d.join("game.exe");
        // Imports decide, whatever else the program names (a static SDL names them all).
        let mut pe = fake_pe(true, &["KERNEL32.dll", "OPENGL32.dll"], &[]);
        pe.extend_from_slice(b"d3d11.dll dxgi.dll vulkan-1.dll");
        std::fs::write(&f, &pe).unwrap();
        assert_eq!(detect(&f), Api::Opengl);
        assert_eq!(is_64bit(&f), Some(true));
        let mut pe = fake_pe(true, &["D3D12.dll"], &[]);
        pe.extend_from_slice(b"vulkan-1.dll");
        std::fs::write(&f, &pe).unwrap();
        assert_eq!(detect(&f), Api::Both);
        std::fs::write(&f, fake_pe(true, &["user32.dll"], &["d3d11.dll"])).unwrap();
        assert_eq!(detect(&f), Api::Dxgi);
        std::fs::write(&f, fake_pe(false, &["vulkan-1.dll"], &[])).unwrap();
        assert_eq!(detect(&f), Api::Vulkan);
        assert_eq!(is_64bit(&f), Some(false));
        // Nothing graphical imported: what it names.
        std::fs::write(&f, b"MZ....D3D12.dll....vulkan-1.dll").unwrap();
        assert_eq!(detect(&f), Api::Both);
        std::fs::write(&f, b"MZ....opengl32.dll").unwrap();
        assert_eq!(detect(&f), Api::Opengl);
        assert_eq!(is_64bit(&f), None);
        assert_eq!(Api::from_name("d3d12"), Some(Api::Dxgi));
        assert_eq!(Api::from_name("\"vulkan\""), Some(Api::Vulkan));
        assert_eq!(Api::from_name("opengl"), Some(Api::Opengl));
    }

    #[test]
    fn dll_comes_out_of_a_setup_program() {
        use std::io::Write;
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let o = zip::write::SimpleFileOptions::default();
        let mut d32 = b"MZ".to_vec();
        d32.resize(280 * 1024, 3);
        z.start_file("ReShade32.dll", o).unwrap();
        z.write_all(&d32).unwrap();
        z.start_file("ReShade64.dll", o).unwrap();
        let mut dll = b"MZ".to_vec();
        dll.resize(300 * 1024, 7);
        z.write_all(&dll).unwrap();
        let zip = z.finish().unwrap().into_inner();
        let mut setup = vec![0x4Du8; 512 * 9];
        setup.extend_from_slice(&zip);
        assert_eq!(dll_from_setup(&setup).unwrap(), (dll, d32));
        assert!(dll_from_setup(&setup[..512 * 9]).is_err());
    }

    #[test]
    fn versions() {
        assert_eq!(parse_version("v6.8.0").as_deref(), Some("6.8.0"));
        assert_eq!(parse_version("nightly"), None);
        assert!(newer("6.10.0", "6.9.1"));
        assert!(!newer("6.8.0", "6.8.0"));
    }

    #[test]
    fn placed_and_taken_away() {
        let data = tmp("place");
        let game = data.join("game");
        std::fs::create_dir_all(&game).unwrap();
        let exe = game.join("game.exe");
        std::fs::write(&exe, b"MZ d3d12.dll").unwrap();
        std::fs::create_dir_all(reshade_dir(&data)).unwrap();
        let mut dll = b"MZ".to_vec();
        dll.resize(300 * 1024, 1);
        std::fs::write(reshade_dir(&data).join("ReShade64.dll"), &dll).unwrap();
        let run = prepare(&data, "g", &exe, find("crt").unwrap(), 40, 240, detect(&exe)).unwrap();
        let placed = run.placed.clone().unwrap();
        assert_eq!(placed, game.join("dxgi.dll"));
        assert!(run.env.iter().any(|(k, v)| k == "RESHADE_BASE_PATH_OVERRIDE" && v.ends_with("g")));
        let ini = std::fs::read_to_string(data.join("looks/g/ReShade.ini")).unwrap();
        assert!(ini.contains("TutorialProgress=4") && ini.contains("crt.ini"));
        assert!(std::fs::read_to_string(data.join("looks/g/crt.ini")).unwrap().contains("RL_LINES=240"));
        assert!(reshade_dir(&data).join("shaders/RL_CRT.fx").is_file());
        // A tweak made in ReShade's menu survives the next start with the same look.
        std::fs::write(data.join("looks/g/crt.ini"), "tweaked").unwrap();
        prepare(&data, "g", &exe, find("crt").unwrap(), 40, 240, Api::Dxgi).unwrap();
        assert_eq!(std::fs::read_to_string(data.join("looks/g/crt.ini")).unwrap(), "tweaked");
        // ...and goes when the amount changes.
        prepare(&data, "g", &exe, find("crt").unwrap(), 60, 240, Api::Dxgi).unwrap();
        assert_ne!(std::fs::read_to_string(data.join("looks/g/crt.ini")).unwrap(), "tweaked");
        finish(&data, "g", &placed);
        assert!(!placed.exists());
        // Someone else's dxgi.dll is left alone, and the look stays off.
        std::fs::write(game.join("dxgi.dll"), b"MZ not ours").unwrap();
        assert!(prepare(&data, "g", &exe, find("sharp").unwrap(), 50, 240, Api::Dxgi).is_err());
        assert_eq!(std::fs::read(game.join("dxgi.dll")).unwrap(), b"MZ not ours");
        tidy(&data, "g");
        assert!(game.join("dxgi.dll").exists());
        // Even when it's the very same ReShade: the player put it there, not the launcher.
        std::fs::write(game.join("dxgi.dll"), &dll).unwrap();
        assert!(prepare(&data, "g", &exe, find("sharp").unwrap(), 50, 240, Api::Dxgi).is_err());
        tidy(&data, "g");
        assert!(game.join("dxgi.dll").exists());
        // Their own ReShade settings next to the game: hands off too.
        std::fs::remove_file(game.join("dxgi.dll")).unwrap();
        std::fs::write(game.join("ReShade.ini"), "[GENERAL]").unwrap();
        assert!(prepare(&data, "g", &exe, find("sharp").unwrap(), 50, 240, Api::Dxgi).is_err());
        std::fs::remove_file(game.join("ReShade.ini")).unwrap();
        // Left over from a start that didn't finish: tidied away.
        prepare(&data, "g", &exe, find("sharp").unwrap(), 50, 240, Api::Dxgi).unwrap();
        assert!(game.join("dxgi.dll").exists());
        tidy(&data, "g");
        assert!(!game.join("dxgi.dll").exists());
        assert!(!data.join("looks/g/placed.txt").exists());
        // A 32-bit game gets the 32-bit ReShade.
        let mut d32 = b"MZ".to_vec();
        d32.resize(280 * 1024, 2);
        std::fs::write(reshade_dir(&data).join("ReShade32.dll"), &d32).unwrap();
        std::fs::write(&exe, fake_pe(false, &["d3d9.dll"], &[])).unwrap();
        let run = prepare(&data, "g", &exe, find("vivid").unwrap(), 50, 240, detect(&exe)).unwrap();
        assert_eq!(run.placed.as_deref(), Some(game.join("d3d9.dll").as_path()));
        assert_eq!(std::fs::read(game.join("d3d9.dll")).unwrap(), d32);
        finish(&data, "g", &game.join("d3d9.dll"));
        // Vulkan: nothing in the game's folder.
        std::fs::write(&exe, b"MZ d3d12.dll").unwrap();
        let run = prepare(&data, "g", &exe, find("film").unwrap(), 50, 240, Api::Vulkan).unwrap();
        assert!(run.placed.is_none());
        assert!(run.env.iter().any(|(k, v)| k == "VK_INSTANCE_LAYERS" && v.starts_with(LAYER)));
        let m = std::fs::read_to_string(data.join("looks/g/vk/ReShade.json")).unwrap();
        assert!(m.contains("ReShade64.dll") && m.contains(LAYER));
        // Commas in paths are doubled, the way ReShade reads them.
        assert_eq!(ini_set("", &[("A", "b", "x,,y".into())]), "[A]\nb=x,,y\n");
    }
}

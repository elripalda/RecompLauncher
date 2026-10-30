//! Mods from GameBanana, for the recomps that have a page there. GameBanana's API is public and
//! needs no account. Each recomp reads mods its own way, so installing depends on the game:
//!
//! - `nrm`: N64: Recompiled games (.nrm mods and .rtz texture packs in the game's mods folder, the
//!   same place Thunderstore mods go).
//! - `otr`: Harbour Masters ports (Ship of Harkinian, Starship, SpaghettiKart, Ghostship): .otr and
//!   .o2r packs in the mods folder next to the program.
//! - `hmm`: Unleashed Recompiled and Marathon Recompiled: a folder with mod.ini, listed in the
//!   ModsDB.ini that the game's built-in mod loader reads (the same files Hedge Mod Manager writes,
//!   so the two can be used together).

use crate::Error;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

const API: &str = "https://gamebanana.com/apiv11";

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Loader {
    /// N64: Recompiled: the data folder's name (as in mods::Source).
    Nrm { data: String },
    Otr,
    /// The game's user folder's name (in AppData\Roaming on Windows).
    Hmm { user: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct Source {
    /// The game's id on GameBanana.
    pub game: u64,
    pub loader: Loader,
}

fn ok_name(s: &str) -> bool {
    !s.is_empty() && s.len() < 80 && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_ .".contains(c)) && !s.contains("..")
}

/// Recomps with a GameBanana page the launcher knows how to install from. The catalog can name
/// others: `"mods": {"gamebanana": 12345, "loader": "otr"}` (with `"data"` for nrm, `"user"` for hmm).
pub fn source(id: &str, entry: &Value) -> Option<Source> {
    if let Some(m) = entry.get("mods").filter(|m| m.get("gamebanana").is_some()) {
        let game = m.get("gamebanana").and_then(Value::as_u64)?;
        let s = |k: &str| m.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let loader = match s("loader").as_str() {
            "otr" => Loader::Otr,
            "hmm" if ok_name(&s("user")) => Loader::Hmm { user: s("user") },
            "nrm" if ok_name(&s("data")) => Loader::Nrm { data: s("data") },
            _ => return None,
        };
        return Some(Source { game, loader });
    }
    let (game, loader) = match id {
        "unleashedrecomp" => (21975, Loader::Hmm { user: "UnleashedRecomp".into() }),
        "marathonrecomp" => (23176, Loader::Hmm { user: "MarathonRecomp".into() }),
        "zelda64recomp" => (21763, Loader::Nrm { data: "Zelda64Recompiled".into() }),
        "banjorecomp" => (24169, Loader::Nrm { data: "BanjoRecompiled".into() }),
        "shipwright" => (16121, Loader::Otr),
        "starship" => (21612, Loader::Otr),
        "spaghettikart" => (22970, Loader::Otr),
        "ghostship" => (24131, Loader::Otr),
        _ => return None,
    };
    Some(Source { game, loader })
}

// ---------- browsing ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mod {
    pub id: u64,
    pub name: String,
    pub author: String,
    /// A screenshot, about 530 pixels wide.
    pub image: String,
    pub likes: u64,
    pub views: u64,
    /// Seconds since 1970.
    pub updated: u64,
    pub category: String,
    pub page: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page {
    pub mods: Vec<Mod>,
    /// Mods in all (for "load more").
    pub total: u64,
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}
fn n(v: &Value, k: &str) -> u64 {
    v.get(k).and_then(Value::as_u64).unwrap_or(0)
}

/// A screenshot's address from GameBanana's preview media, at the size asked (220, 530 or 800).
fn image_of(v: &Value, size: &str) -> String {
    let Some(img) = v.get("_aPreviewMedia").and_then(|p| p.get("_aImages")).and_then(Value::as_array).and_then(|a| a.first()) else { return String::new() };
    let base = s(img, "_sBaseUrl");
    let file = [format!("_sFile{size}"), "_sFile530".into(), "_sFile220".into(), "_sFile".into()].iter().map(|k| s(img, k)).find(|f| !f.is_empty()).unwrap_or_default();
    if base.starts_with("https://") && !file.is_empty() && !file.contains('/') {
        format!("{base}/{file}")
    } else {
        String::new()
    }
}

pub fn parse_list(v: &Value) -> Page {
    let total = v.get("_aMetadata").map(|m| n(m, "_nRecordCount")).unwrap_or(0);
    let mods = v
        .get("_aRecords")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        // Mods only, not ones marked for mature content or replaced by newer ones.
        .filter(|r| s(r, "_sModelName") == "Mod" && !r.get("_bHasContentRatings").and_then(Value::as_bool).unwrap_or(false) && !r.get("_bIsObsolete").and_then(Value::as_bool).unwrap_or(false))
        .map(|r| Mod {
            id: n(r, "_idRow"),
            name: s(r, "_sName"),
            author: r.get("_aSubmitter").map(|x| s(x, "_sName")).unwrap_or_default(),
            image: image_of(r, "530"),
            likes: n(r, "_nLikeCount"),
            views: n(r, "_nViewCount"),
            updated: n(r, "_tsDateModified").max(n(r, "_tsDateAdded")),
            category: r.get("_aRootCategory").map(|x| s(x, "_sName")).unwrap_or_default(),
            page: s(r, "_sProfileUrl"),
        })
        .filter(|m| m.id > 0)
        .collect();
    Page { mods, total }
}

/// A page of a game's mods: "popular", "new" or "liked", or a search.
pub async fn list(http: &reqwest::Client, game: u64, sort: &str, page: u32, q: &str) -> Result<Page, Error> {
    let q = q.trim();
    let url = if q.is_empty() {
        let sort = match sort {
            "new" => "Generic_Newest",
            "liked" => "Generic_MostLiked",
            _ => "Generic_MostDownloaded",
        };
        format!("{API}/Mod/Index?_nPerpage=30&_aFilters[Generic_Game]={game}&_sSort={sort}&_nPage={}", page.max(1))
    } else {
        let q: String = q.chars().take(60).collect();
        let enc: String = q.bytes().map(|b| if b.is_ascii_alphanumeric() { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
        format!("{API}/Util/Search/Results?_sModelName=Mod&_sSearchString={enc}&_idGameRow={game}&_nPage={}", page.max(1))
    };
    let r = http.get(&url).timeout(Duration::from_secs(25)).send().await?;
    if !r.status().is_success() {
        return Err(Error::Msg(format!("GameBanana answered {}", r.status().as_u16())));
    }
    Ok(parse_list(&r.json::<Value>().await?))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct File {
    pub id: u64,
    pub name: String,
    pub size: u64,
    pub version: String,
    pub desc: String,
    pub added: u64,
    pub downloads: u64,
    /// Passed GameBanana's virus scan and comes as a zip, 7z or rar the launcher can open.
    pub ok: bool,
    /// Why it can't be installed from here, when it can't.
    pub why: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Detail {
    pub id: u64,
    pub name: String,
    pub author: String,
    /// The description as plain text.
    pub text: String,
    pub images: Vec<String>,
    pub files: Vec<File>,
    pub page: String,
    pub likes: u64,
    pub downloads: u64,
}

/// Plain text from GameBanana's HTML: line breaks kept, tags gone, the usual entities decoded.
pub fn plain(html: &str) -> String {
    let mut out = String::new();
    let mut tag = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => {
                in_tag = true;
                tag.clear();
            }
            '>' if in_tag => {
                in_tag = false;
                let t = tag.trim_start_matches('/').split_whitespace().next().unwrap_or("").to_ascii_lowercase();
                if ["br", "p", "div", "li", "h1", "h2", "h3", "h4", "tr"].contains(&t.as_str()) && !out.ends_with('\n') {
                    out.push('\n');
                }
            }
            _ if in_tag => tag.push(c),
            _ => out.push(c),
        }
    }
    let out = out.replace("&nbsp;", " ").replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&#039;", "'");
    let mut lines: Vec<&str> = out.lines().map(str::trim).collect();
    lines.dedup_by(|a, b| a.is_empty() && b.is_empty());
    lines.join("\n").trim().chars().take(4000).collect()
}

fn kind_of_name(name: &str) -> Option<&'static str> {
    let n = name.to_ascii_lowercase();
    if n.ends_with(".zip") {
        Some("zip")
    } else if n.ends_with(".7z") {
        Some("7z")
    } else if n.ends_with(".rar") {
        Some("rar")
    } else {
        None
    }
}

pub fn parse_detail(v: &Value) -> Detail {
    let files = v
        .get("_aFiles")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|f| {
            let name = s(f, "_sFile");
            let av = s(f, "_sAvResult");
            let why = if kind_of_name(&name).is_none() {
                "It isn't a zip, 7z or rar.".to_string()
            } else if av != "clean" {
                "GameBanana's virus scan hasn't passed it.".to_string()
            } else {
                String::new()
            };
            File { id: n(f, "_idRow"), size: n(f, "_nFilesize"), version: s(f, "_sVersion"), desc: s(f, "_sDescription"), added: n(f, "_tsDateAdded"), downloads: n(f, "_nDownloadCount"), ok: why.is_empty(), why, name }
        })
        .collect();
    let images = v
        .get("_aPreviewMedia")
        .and_then(|p| p.get("_aImages"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(8)
        .filter_map(|img| {
            let base = s(img, "_sBaseUrl");
            let file = ["_sFile800", "_sFile530", "_sFile"].iter().map(|k| s(img, k)).find(|f| !f.is_empty())?;
            (base.starts_with("https://") && !file.contains('/')).then(|| format!("{base}/{file}"))
        })
        .collect();
    Detail {
        id: n(v, "_idRow"),
        name: s(v, "_sName"),
        author: v.get("_aSubmitter").map(|x| s(x, "_sName")).unwrap_or_default(),
        text: plain(&s(v, "_sText")),
        images,
        files,
        page: s(v, "_sProfileUrl"),
        likes: n(v, "_nLikeCount"),
        downloads: n(v, "_nDownloadCount"),
    }
}

pub async fn detail(http: &reqwest::Client, id: u64) -> Result<Detail, Error> {
    let r = http.get(format!("{API}/Mod/{id}/ProfilePage")).timeout(Duration::from_secs(25)).send().await?;
    if !r.status().is_success() {
        return Err(Error::Msg(format!("GameBanana answered {}", r.status().as_u16())));
    }
    let v = r.json::<Value>().await?;
    if v.get("_bHasContentRatings").and_then(Value::as_bool).unwrap_or(false) || v.get("_bIsTrashed").and_then(Value::as_bool).unwrap_or(false) || v.get("_bIsWithheld").and_then(Value::as_bool).unwrap_or(false) {
        return Err(Error::Msg("That mod isn't available here.".into()));
    }
    Ok(parse_detail(&v))
}

/// Download one of a mod's files to `to`. `progress` hears (done, total).
pub async fn download(http: &reqwest::Client, file: &File, to: &Path, progress: &(dyn Fn(u64, u64) + Send + Sync)) -> Result<(), Error> {
    use futures_util::StreamExt;
    if !file.ok {
        return Err(Error::Msg(file.why.clone()));
    }
    let r = http.get(format!("https://gamebanana.com/dl/{}", file.id)).timeout(Duration::from_secs(60 * 30)).send().await?;
    if !r.status().is_success() {
        return Err(Error::Msg(format!("GameBanana answered {}", r.status().as_u16())));
    }
    let total = r.content_length().unwrap_or(file.size);
    if total > 8 << 30 {
        return Err(Error::Msg("That file is too big to install from here.".into()));
    }
    if let Some(p) = to.parent() {
        std::fs::create_dir_all(p)?;
    }
    let mut out = std::fs::File::create(to)?;
    let mut done = 0u64;
    let mut last = std::time::Instant::now();
    let mut body = r.bytes_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk?;
        done += chunk.len() as u64;
        if done > 8 << 30 {
            return Err(Error::Msg("That file is too big to install from here.".into()));
        }
        out.write_all(&chunk)?;
        if last.elapsed() > Duration::from_millis(150) {
            last = std::time::Instant::now();
            progress(done, total);
        }
    }
    progress(done, total.max(done));
    Ok(())
}

// ---------- unpacking ----------

/// A path inside an archive, made safe: relative, no "..", no drive letters.
fn safe(p: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(x) => out.push(x),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

/// The most an unpacked mod may take, and the most files: an archive that unpacks to more is
/// refused rather than filling the disk.
const UNPACK_MAX: u64 = 16 << 30;
const UNPACK_FILES: usize = 100_000;

fn too_big() -> Error {
    Error::Msg("This mod unpacks to more than the launcher will put on your disk.".into())
}

/// Copy at most `left` bytes; past that, the mod is too big.
fn copy_capped(r: &mut dyn Read, dest: &Path, left: &mut u64) -> std::io::Result<bool> {
    let mut f = std::fs::File::create(dest)?;
    let n = std::io::copy(&mut r.take(*left + 1), &mut f)?;
    if n > *left {
        drop(f);
        let _ = std::fs::remove_file(dest);
        return Ok(false);
    }
    *left -= n;
    Ok(true)
}

/// Unpack a zip, 7z or rar (told apart by their first bytes) into `to`.
pub fn extract(archive: &Path, to: &Path) -> Result<(), Error> {
    let mut head = [0u8; 8];
    std::fs::File::open(archive)?.read_exact(&mut head).map_err(|_| Error::Msg("The download was empty.".into()))?;
    std::fs::create_dir_all(to)?;
    let bad = |e: String| Error::Msg(format!("Couldn't unpack the mod: {e}"));
    let mut left = UNPACK_MAX;
    if head.starts_with(b"PK") {
        let mut z = zip::ZipArchive::new(std::fs::File::open(archive)?).map_err(|e| bad(e.to_string()))?;
        if z.len() > UNPACK_FILES {
            return Err(too_big());
        }
        for i in 0..z.len() {
            let mut e = z.by_index(i).map_err(|e| bad(e.to_string()))?;
            let Some(rel) = e.enclosed_name().and_then(|p| safe(&p)) else { continue };
            let dest = to.join(rel);
            if e.is_dir() {
                std::fs::create_dir_all(&dest)?;
                continue;
            }
            if let Some(p) = dest.parent() {
                std::fs::create_dir_all(p)?;
            }
            if !copy_capped(&mut e, &dest, &mut left)? {
                return Err(too_big());
            }
        }
        Ok(())
    } else if head.starts_with(b"7z\xBC\xAF\x27\x1C") {
        let mut count = 0usize;
        let mut over = false;
        let io = |e: std::io::Error| sevenz_rust::Error::other(e.to_string());
        let r = sevenz_rust::decompress_file_with_extract_fn(archive, to, |entry, reader, _| {
            // Every entry is read through, even one that's skipped: in a solid archive the next
            // file's bytes follow on from this one's.
            let rel = safe(Path::new(entry.name()));
            count += 1;
            if over || count > UNPACK_FILES || rel.is_none() || entry.is_directory() {
                if let Some(rel) = rel.filter(|_| entry.is_directory() && !over) {
                    std::fs::create_dir_all(to.join(rel)).map_err(io)?;
                }
                over |= count > UNPACK_FILES;
                std::io::copy(reader, &mut std::io::sink()).map_err(io)?;
                return Ok(true);
            }
            let dest = to.join(rel.unwrap_or_default());
            if let Some(p) = dest.parent() {
                std::fs::create_dir_all(p).map_err(io)?;
            }
            if !copy_capped(reader, &dest, &mut left).map_err(io)? {
                over = true;
                std::io::copy(reader, &mut std::io::sink()).map_err(io)?;
            }
            Ok(true)
        });
        if over {
            return Err(too_big());
        }
        r.map_err(|e| bad(e.to_string()))
    } else if head.starts_with(b"Rar!\x1A\x07") {
        let mut a = unrar::Archive::new(archive).open_for_processing().map_err(|e| bad(e.to_string()))?;
        let mut count = 0usize;
        while let Some(h) = a.read_header().map_err(|e| bad(e.to_string()))? {
            let entry = h.entry();
            count += 1;
            if count > UNPACK_FILES || entry.unpacked_size > left {
                return Err(too_big());
            }
            let rel = safe(&entry.filename);
            a = match rel {
                Some(rel) if !entry.is_directory() => {
                    left -= entry.unpacked_size;
                    let dest = to.join(rel);
                    if let Some(p) = dest.parent() {
                        std::fs::create_dir_all(p)?;
                    }
                    h.extract_to(&dest).map_err(|e| bad(e.to_string()))?
                }
                _ => h.skip().map_err(|e| bad(e.to_string()))?,
            };
        }
        Ok(())
    } else {
        Err(Error::Msg("That download isn't a zip, 7z or rar.".into()))
    }
}

/// The files under a folder. Links are left alone, so a link back up can't loop.
fn walk(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 12 {
        return;
    }
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() {
            walk(&p, out, depth + 1);
        } else if t.is_file() {
            out.push(p);
        }
    }
}

fn copy_dir(from: &Path, to: &Path, depth: usize) -> std::io::Result<()> {
    if depth > 12 {
        return Ok(());
    }
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let p = e.path();
        let d = to.join(e.file_name());
        let t = e.file_type()?;
        if t.is_dir() {
            copy_dir(&p, &d, depth + 1)?;
        } else if t.is_file() {
            std::fs::copy(&p, &d)?;
        }
    }
    Ok(())
}

/// A name for a mod's folder, from its name on GameBanana.
pub fn folder_name(name: &str, id: u64) -> String {
    let clean: String = name.chars().map(|c| if c.is_alphanumeric() || " -_.()[]".contains(c) { c } else { ' ' }).collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    let clean = clean.trim_matches(|c: char| c == '.' || c == ' ').chars().take(60).collect::<String>();
    let clean = clean.trim_end_matches(|c: char| c == '.' || c == ' ').to_string();
    if clean.is_empty() {
        format!("GameBanana {id}")
    } else {
        clean
    }
}

/// Where a game's mods go, for this loader.
pub fn mods_dir(loader: &Loader, install: &Path, exe_dir: &Path) -> PathBuf {
    match loader {
        Loader::Nrm { data } => crate::mods::mods_dir(install, data),
        Loader::Otr | Loader::Hmm { .. } => exe_dir.join("mods"),
    }
}

/// The folders in an unpacked Hedge Mod Manager mod that hold a mod.ini (not ones inside
/// another), each with its mod.ini's own name, and a name for it the player can pick by.
pub fn hmm_roots(unpacked: &Path) -> Vec<(PathBuf, String, String)> {
    let mut files = vec![];
    walk(unpacked, &mut files, 0);
    let mut roots: Vec<(PathBuf, String)> = files.iter().filter_map(|f| {
        let n = f.file_name()?.to_string_lossy().to_string();
        n.eq_ignore_ascii_case("mod.ini").then(|| (f.parent().unwrap_or(unpacked).to_path_buf(), n))
    }).collect();
    roots.sort_by(|a, b| a.0.cmp(&b.0));
    let all: Vec<PathBuf> = roots.iter().map(|r| r.0.clone()).collect();
    roots.retain(|(r, _)| !all.iter().any(|o| o != r && r.starts_with(o)));
    roots.into_iter().map(|(r, n)| {
        let label = r.strip_prefix(unpacked).ok().map(|p| p.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect::<Vec<_>>().join(" / ")).filter(|s| !s.is_empty()).unwrap_or_else(|| ".".into());
        (r, n, label)
    }).collect()
}

/// Put an unpacked mod where the game reads it. Returns what to remember for taking it out again:
/// file names in the mods folder, or "dir:<folder>" for a mod that's a folder. `owned` is what an
/// earlier copy of the same mod put there (it may be replaced); anything else already there is
/// another mod's and isn't overwritten. A Hedge Mod Manager mod that comes in versions (several
/// folders with a mod.ini) needs `variant`, one of their names: without it the error is
/// "variants:" and the names as JSON, for the window to ask which.
pub fn install(loader: &Loader, unpacked: &Path, install: &Path, exe_dir: &Path, name: &str, id: u64, owned: &[String], variant: Option<&str>) -> Result<Vec<String>, Error> {
    let mods = mods_dir(loader, install, exe_dir);
    std::fs::create_dir_all(&mods)?;
    match loader {
        Loader::Nrm { .. } | Loader::Otr => {
            let mut files = vec![];
            walk(unpacked, &mut files, 0);
            let exts: &[&str] = if matches!(loader, Loader::Otr) { &[".otr", ".o2r"] } else { &[".nrm", ".rtz", if cfg!(windows) { ".dll" } else if cfg!(target_os = "macos") { ".dylib" } else { ".so" }] };
            let picked: Vec<(PathBuf, String)> = files.into_iter().filter_map(|f| {
                let fname = f.file_name()?.to_string_lossy().to_string();
                let low = fname.to_lowercase();
                exts.iter().any(|e| low.ends_with(e)).then_some((f, fname))
            }).collect();
            if picked.is_empty() {
                return Err(Error::Msg("This mod has nothing the game reads by itself. Its page on GameBanana says how to set it up.".into()));
            }
            if let Some((_, f)) = picked.iter().find(|(_, f)| mods.join(f).exists() && !owned.contains(f)) {
                return Err(Error::Msg(format!("{f} is already in the game's mods folder, from another mod. Take that one out first.")));
            }
            let mut out = vec![];
            for (f, fname) in picked {
                std::fs::copy(&f, mods.join(&fname)).map_err(|e| crate::defender_msg(e, &fname))?;
                if !out.contains(&fname) {
                    out.push(fname);
                }
            }
            Ok(out)
        }
        Loader::Hmm { user } => {
            let roots = hmm_roots(unpacked);
            if roots.is_empty() {
                return Err(Error::Msg("This mod has no mod.ini, so the game's mod loader can't use it. Its page on GameBanana says how to set it up.".into()));
            }
            let (root, ini, label) = if roots.len() == 1 {
                roots[0].clone()
            } else {
                match variant.and_then(|v| roots.iter().find(|r| r.2 == v)) {
                    Some(r) => r.clone(),
                    None => return Err(Error::Msg(format!("variants:{}", serde_json::to_string(&roots.iter().map(|r| &r.2).collect::<Vec<_>>()).unwrap_or_default()))),
                }
            };
            let base = if roots.len() == 1 || label == "." { folder_name(name, id) } else { folder_name(&format!("{name} - {}", root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()), id) };
            let mut folder = base.clone();
            let mut k = 2;
            while mods.join(&folder).exists() {
                folder = format!("{base} ({k})");
                k += 1;
            }
            let dest = mods.join(&folder);
            let done = (|| -> Result<(), Error> {
                copy_dir(&root, &dest, 0)?;
                let _g = DB_LOCK.lock().unwrap_or_else(|e| e.into_inner());
                let db = hmm_db(user, exe_dir)?;
                // An earlier version of this mod comes off first, so nothing it had stays on.
                hmm_disable(&db, &format!("rl_gb_{id}_"))?;
                hmm_enable(&db, &format!("rl_gb_{id}_0"), &dest.join(&ini))
            })();
            if let Err(e) = done {
                let _ = std::fs::remove_dir_all(&dest);
                return Err(e);
            }
            Ok(vec![format!("dir:{folder}")])
        }
    }
}

/// Take a mod out again. `keep` is what other mods use: those files stay.
pub fn remove(loader: &Loader, install: &Path, exe_dir: &Path, id: u64, files: &[String], keep: &[String]) {
    let mods = mods_dir(loader, install, exe_dir);
    for f in files.iter().filter(|f| !keep.contains(f)) {
        if let Some(d) = f.strip_prefix("dir:") {
            if !d.is_empty() && !d.contains(['/', '\\']) && d != "." && d != ".." {
                let _ = std::fs::remove_dir_all(mods.join(d));
            }
        } else if !f.is_empty() && !f.contains(['/', '\\']) && f != "." && f != ".." {
            let _ = std::fs::remove_file(mods.join(f));
        }
    }
    if let Loader::Hmm { user } = loader {
        let _g = DB_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(db) = hmm_db_now(user, exe_dir) {
            let _ = hmm_disable(&db, &format!("rl_gb_{id}_"));
        }
    }
}

/// ModsDB.ini is read, changed and written whole: one change at a time.
static DB_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

// ---------- Hedge Mod Manager's files ----------

/// A small INI: sections in order, keys in order, values without their quotes.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Ini(pub Vec<(String, Vec<(String, String)>)>);

impl Ini {
    pub fn parse(text: &str) -> Ini {
        let mut out: Vec<(String, Vec<(String, String)>)> = vec![];
        for line in text.trim_start_matches('\u{feff}').lines() {
            let l = line.trim();
            if l.is_empty() || l.starts_with(';') || l.starts_with('#') {
                continue;
            }
            if l.starts_with('[') && l.ends_with(']') {
                out.push((l[1..l.len() - 1].to_string(), vec![]));
                continue;
            }
            let Some((k, v)) = l.split_once('=') else { continue };
            let v = v.trim();
            let v = v.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(v);
            if out.is_empty() {
                out.push((String::new(), vec![]));
            }
            out.last_mut().unwrap().1.push((k.trim().to_string(), v.to_string()));
        }
        Ini(out)
    }
    pub fn get(&self, sec: &str, key: &str) -> Option<&str> {
        self.0.iter().find(|(s, _)| s.eq_ignore_ascii_case(sec))?.1.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str())
    }
    pub fn set(&mut self, sec: &str, key: &str, value: &str) {
        let i = match self.0.iter().position(|(s, _)| s.eq_ignore_ascii_case(sec)) {
            Some(i) => i,
            None => {
                self.0.push((sec.to_string(), vec![]));
                self.0.len() - 1
            }
        };
        let kv = &mut self.0[i].1;
        match kv.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(key)) {
            Some(x) => x.1 = value.to_string(),
            None => kv.push((key.to_string(), value.to_string())),
        }
    }
    pub fn remove(&mut self, sec: &str, key: &str) {
        if let Some((_, kv)) = self.0.iter_mut().find(|(s, _)| s.eq_ignore_ascii_case(sec)) {
            kv.retain(|(k, _)| !k.eq_ignore_ascii_case(key));
        }
    }
    /// Written the way Hedge Mod Manager writes it: every value in quotes.
    pub fn text(&self) -> String {
        let mut s = String::new();
        for (sec, kv) in &self.0 {
            if !sec.is_empty() {
                if !s.is_empty() {
                    s.push('\n');
                }
                s += &format!("[{sec}]\n");
            }
            for (k, v) in kv {
                s += &format!("{k}=\"{v}\"\n");
            }
        }
        s
    }
}

/// The game's user folder: next to the program when it runs portable, else its folder in
/// AppData\Roaming (~/.config on Linux, Application Support on a Mac).
pub fn hmm_user(user: &str, exe_dir: &Path) -> PathBuf {
    if exe_dir.join("portable.txt").is_file() {
        return exe_dir.to_path_buf();
    }
    dirs::config_dir().unwrap_or_else(|| exe_dir.to_path_buf()).join(user)
}

/// The ModsDB.ini the game reads: the one cpkredir.ini names (Hedge Mod Manager's, when it's been
/// used), or a new one in the mods folder, named in a new cpkredir.ini.
fn cpkredir(user: &str, exe_dir: &Path) -> PathBuf {
    let dir = hmm_user(user, exe_dir);
    [dir.join("cpkredir.ini"), exe_dir.join("cpkredir.ini")].into_iter().find(|p| p.is_file()).unwrap_or_else(|| dir.join("cpkredir.ini"))
}

/// The ModsDB.ini in use, when there is one; nothing is made or changed.
pub fn hmm_db_now(user: &str, exe_dir: &Path) -> Option<PathBuf> {
    let cfg = Ini::parse(&std::fs::read_to_string(cpkredir(user, exe_dir)).ok()?);
    let db = cfg.get("CPKREDIR", "ModsDbIni").map(PathBuf::from).filter(|p| !p.as_os_str().is_empty())?;
    db.is_file().then_some(db)
}

pub fn hmm_db(user: &str, exe_dir: &Path) -> Result<PathBuf, Error> {
    let cfg_path = cpkredir(user, exe_dir);
    let mut cfg = Ini::parse(&std::fs::read_to_string(&cfg_path).unwrap_or_default());
    let named = cfg.get("CPKREDIR", "ModsDbIni").map(PathBuf::from).filter(|p| !p.as_os_str().is_empty());
    let db = named.clone().unwrap_or_else(|| exe_dir.join("mods").join("ModsDB.ini"));
    if named.is_none() || cfg.get("CPKREDIR", "Enabled").map(|v| v == "0" || v.eq_ignore_ascii_case("false")).unwrap_or(false) {
        cfg.set("CPKREDIR", "Enabled", "1");
        cfg.set("CPKREDIR", "ModsDbIni", &db.display().to_string());
        if let Some(p) = cfg_path.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::write(&cfg_path, cfg.text())?;
    }
    if !db.is_file() {
        if let Some(p) = db.parent() {
            std::fs::create_dir_all(p)?;
        }
        let mut d = Ini::default();
        d.set("Main", "ActiveModCount", "0");
        d.0.push(("Mods".into(), vec![]));
        std::fs::write(&db, d.text())?;
    }
    Ok(db)
}

fn active(db: &Ini) -> Vec<String> {
    let n: usize = db.get("Main", "ActiveModCount").and_then(|v| v.parse().ok()).unwrap_or(0);
    (0..n).filter_map(|i| db.get("Main", &format!("ActiveMod{i}")).map(str::to_string)).collect()
}

fn set_active(db: &mut Ini, list: &[String]) {
    let old: usize = db.get("Main", "ActiveModCount").and_then(|v| v.parse().ok()).unwrap_or(0);
    for i in 0..old.max(list.len()) {
        db.remove("Main", &format!("ActiveMod{i}"));
    }
    db.set("Main", "ActiveModCount", &list.len().to_string());
    for (i, m) in list.iter().enumerate() {
        db.set("Main", &format!("ActiveMod{i}"), m);
    }
}

/// Turn a mod on: listed in [Mods] and added to the active ones.
pub fn hmm_enable(db_path: &Path, id: &str, mod_ini: &Path) -> Result<(), Error> {
    let mut db = Ini::parse(&std::fs::read_to_string(db_path).unwrap_or_default());
    db.set("Mods", id, &mod_ini.display().to_string());
    let mut list = active(&db);
    if !list.iter().any(|m| m == id) {
        list.push(id.to_string());
    }
    set_active(&mut db, &list);
    std::fs::write(db_path, db.text())?;
    Ok(())
}

/// Take the launcher's mods with this id prefix out of ModsDB.ini.
pub fn hmm_disable(db_path: &Path, prefix: &str) -> Result<(), Error> {
    let mut db = Ini::parse(&std::fs::read_to_string(db_path).unwrap_or_default());
    let keys: Vec<String> = db.0.iter().find(|(s, _)| s.eq_ignore_ascii_case("Mods")).map(|(_, kv)| kv.iter().map(|(k, _)| k.clone()).filter(|k| k.starts_with(prefix)).collect()).unwrap_or_default();
    for k in &keys {
        db.remove("Mods", k);
    }
    let list: Vec<String> = active(&db).into_iter().filter(|m| !m.starts_with(prefix)).collect();
    set_active(&mut db, &list);
    std::fs::write(db_path, db.text())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rl-gb-{n}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn sources() {
        assert_eq!(source("unleashedrecomp", &Value::Null).unwrap().game, 21975);
        assert_eq!(source("shipwright", &Value::Null).unwrap().loader, Loader::Otr);
        assert!(source("fifa", &Value::Null).is_none());
        let e = serde_json::json!({"mods": {"gamebanana": 99, "loader": "hmm", "user": "MyRecomp"}});
        assert_eq!(source("x", &e).unwrap().loader, Loader::Hmm { user: "MyRecomp".into() });
        let bad = serde_json::json!({"mods": {"gamebanana": 99, "loader": "hmm", "user": "../x"}});
        assert!(source("x", &bad).is_none());
    }

    #[test]
    fn parses_lists_and_pages() {
        let v = serde_json::json!({"_aMetadata": {"_nRecordCount": 632}, "_aRecords": [
            {"_idRow": 1, "_sModelName": "Mod", "_sName": "Miku", "_aSubmitter": {"_sName": "GPF"}, "_nLikeCount": 790, "_tsDateModified": 5,
             "_aPreviewMedia": {"_aImages": [{"_sBaseUrl": "https://images.gamebanana.com/img/ss/mods", "_sFile": "a.jpg", "_sFile530": "530-90_a.jpg"}]}, "_aRootCategory": {"_sName": "Skins"}},
            {"_idRow": 2, "_sModelName": "Mod", "_sName": "Adult", "_bHasContentRatings": true},
            {"_idRow": 3, "_sModelName": "Wip", "_sName": "Not a mod"}]});
        let p = parse_list(&v);
        assert_eq!(p.total, 632);
        assert_eq!(p.mods.len(), 1);
        assert_eq!(p.mods[0].image, "https://images.gamebanana.com/img/ss/mods/530-90_a.jpg");
        let d = parse_detail(&serde_json::json!({"_idRow": 1, "_sName": "Miku", "_sText": "Hi<br>there &amp; <b>you</b>", "_aFiles": [
            {"_idRow": 9, "_sFile": "miku.7z", "_nFilesize": 10, "_sAvResult": "clean"},
            {"_idRow": 10, "_sFile": "miku.exe", "_sAvResult": "clean"},
            {"_idRow": 11, "_sFile": "miku.rar", "_sAvResult": "pending"}]}));
        assert_eq!(d.text, "Hi\nthere & you");
        assert!(d.files[0].ok && !d.files[1].ok && !d.files[2].ok);
    }

    #[test]
    fn unpacks_zip_and_7z_safely() {
        let d = tmp("x");
        let zip = d.join("m.zip");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&zip).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            for n in ["Cool Mod/mod.ini", "Cool Mod/disk/a.ar", "../evil.txt"] {
                z.start_file(n, o).unwrap();
                z.write_all(b"x").unwrap();
            }
            z.finish().unwrap();
        }
        let out = d.join("out");
        extract(&zip, &out).unwrap();
        assert!(out.join("Cool Mod/mod.ini").is_file());
        assert!(!d.join("evil.txt").exists());
        // 7z: made with the same crate.
        let src = d.join("src");
        std::fs::create_dir_all(src.join("pack")).unwrap();
        std::fs::write(src.join("pack/cool.o2r"), b"o2r").unwrap();
        let sz = d.join("m.7z");
        sevenz_rust::compress_to_path(&src, &sz).unwrap();
        let out7 = d.join("out7");
        extract(&sz, &out7).unwrap();
        let mut found = vec![];
        walk(&out7, &mut found, 0);
        assert!(found.iter().any(|f| f.ends_with("cool.o2r")));
        let bogus = d.join("x.bin");
        std::fs::write(&bogus, b"hello world").unwrap();
        assert!(extract(&bogus, &d.join("o3")).is_err());
    }

    #[test]
    fn installs_for_each_loader() {
        let d = tmp("inst");
        let game = d.join("game");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("portable.txt"), b"").unwrap();
        let un = d.join("un");
        std::fs::create_dir_all(un.join("Pack/textures")).unwrap();
        std::fs::write(un.join("Pack/hd.o2r"), b"x").unwrap();
        std::fs::write(un.join("Pack/readme.txt"), b"x").unwrap();
        // otr
        let f = install(&Loader::Otr, &un, &game, &game, "HD Pack", 5, &[], None).unwrap();
        assert_eq!(f, vec!["hd.o2r"]);
        assert!(game.join("mods/hd.o2r").is_file());
        // Another mod with the same file can't overwrite it.
        assert!(install(&Loader::Otr, &un, &game, &game, "HD Pack", 6, &[], None).is_err());
        assert!(install(&Loader::Otr, &un, &game, &game, "HD Pack", 5, &f, None).is_ok());
        remove(&Loader::Otr, &game, &game, 5, &f, &[]);
        assert!(!game.join("mods/hd.o2r").exists());
        // hmm: no mod.ini, no install
        let hmm = Loader::Hmm { user: "UnleashedRecomp".into() };
        assert!(install(&hmm, &un, &game, &game, "HD Pack", 5, &[], None).is_err());
        std::fs::create_dir_all(un.join("Pack/Miku")).unwrap();
        std::fs::write(un.join("Pack/Miku/mod.ini"), b"[Main]\nIncludeDirCount=1\nIncludeDir0=\".\"\n").unwrap();
        // Someone's Hedge Mod Manager setup already: kept, ours added.
        std::fs::write(game.join("cpkredir.ini"), "[CPKREDIR]\nEnabled=\"1\"\nModsDbIni=\"".to_string() + &game.join("hmm/ModsDB.ini").display().to_string() + "\"\n").unwrap();
        std::fs::create_dir_all(game.join("hmm")).unwrap();
        std::fs::write(game.join("hmm/ModsDB.ini"), "[Main]\nActiveModCount=\"1\"\nActiveMod0=\"theirs\"\n\n[Mods]\ntheirs=\"C:\\x\\mod.ini\"\n").unwrap();
        let f = install(&hmm, &un, &game, &game, "Hatsune Miku over Chip", 7, &[], None).unwrap();
        assert_eq!(f, vec!["dir:Hatsune Miku over Chip"]);
        assert!(game.join("mods/Hatsune Miku over Chip/mod.ini").is_file());
        let db = Ini::parse(&std::fs::read_to_string(game.join("hmm/ModsDB.ini")).unwrap());
        assert_eq!(db.get("Main", "ActiveModCount"), Some("2"));
        assert_eq!(db.get("Main", "ActiveMod0"), Some("theirs"));
        assert_eq!(db.get("Main", "ActiveMod1"), Some("rl_gb_7_0"));
        assert!(db.get("Mods", "rl_gb_7_0").unwrap().ends_with("mod.ini"));
        remove(&hmm, &game, &game, 7, &f, &[]);
        assert!(!game.join("mods/Hatsune Miku over Chip").exists());
        let db = Ini::parse(&std::fs::read_to_string(game.join("hmm/ModsDB.ini")).unwrap());
        assert_eq!(db.get("Main", "ActiveModCount"), Some("1"));
        assert_eq!(db.get("Mods", "rl_gb_7_0"), None);
        assert_eq!(db.get("Mods", "theirs"), Some("C:\\x\\mod.ini"));
        // No setup yet: a new cpkredir.ini (in the portable game's folder) and ModsDB.ini.
        std::fs::remove_file(game.join("cpkredir.ini")).unwrap();
        install(&hmm, &un, &game, &game, "Miku", 8, &[], None).unwrap();
        let cfg = Ini::parse(&std::fs::read_to_string(game.join("cpkredir.ini")).unwrap());
        assert!(cfg.get("CPKREDIR", "ModsDbIni").unwrap().ends_with("ModsDB.ini"));
        assert!(game.join("mods/ModsDB.ini").is_file());
        // A mod in versions: the window is asked which, and only that one goes on. A mod.ini
        // inside another mod's folder isn't a version of its own.
        std::fs::create_dir_all(un.join("Pack/Blue/extra")).unwrap();
        std::fs::write(un.join("Pack/Blue/Mod.ini"), b"[Main]\n").unwrap();
        std::fs::write(un.join("Pack/Blue/extra/mod.ini"), b"[Main]\n").unwrap();
        let e = install(&hmm, &un, &game, &game, "Colors", 9, &[], None).unwrap_err().to_string();
        assert!(e.contains("variants:") && e.contains("Pack / Blue") && e.contains("Pack / Miku") && !e.contains("extra"), "{e}");
        let f = install(&hmm, &un, &game, &game, "Colors", 9, &[], Some("Pack / Blue")).unwrap();
        assert_eq!(f, vec!["dir:Colors - Blue"]);
        let db = Ini::parse(&std::fs::read_to_string(game.join("mods/ModsDB.ini")).unwrap());
        assert!(db.get("Mods", "rl_gb_9_0").unwrap().ends_with("Mod.ini"));
        // Again (an update): still one entry.
        let f2 = install(&hmm, &un, &game, &game, "Colors", 9, &f, Some("Pack / Blue")).unwrap();
        let db = Ini::parse(&std::fs::read_to_string(game.join("mods/ModsDB.ini")).unwrap());
        assert_eq!(db.get("Main", "ActiveModCount"), Some("2"));
        remove(&hmm, &game, &game, 9, &f2, &[]);
        // Taking a mod out doesn't make any files.
        let bare = d.join("bare");
        std::fs::create_dir_all(&bare).unwrap();
        std::fs::write(bare.join("portable.txt"), b"").unwrap();
        remove(&hmm, &bare, &bare, 1, &["dir:x".into()], &[]);
        assert!(!bare.join("cpkredir.ini").exists());
        // A byte order mark doesn't hide the first section.
        assert_eq!(Ini::parse("\u{feff}[Main]\nA=\"1\"\n").get("Main", "A"), Some("1"));
    }

    #[test]
    fn unpacks_rar() {
        // A sample from the unrar crate itself, when it's in the local registry.
        let Some(src) = std::fs::read_dir(dirs::home_dir().unwrap().join(".cargo/registry/src")).ok().and_then(|mut r| r.find_map(|e| {
            let p = e.ok()?.path().join("unrar-0.5.8/data/solid.rar");
            p.is_file().then_some(p)
        })) else { return };
        let d = tmp("rar");
        extract(&src, &d.join("out")).unwrap();
        let mut found = vec![];
        walk(&d.join("out"), &mut found, 0);
        assert!(!found.is_empty());
    }

    #[test]
    fn folder_names() {
        assert_eq!(folder_name("Hatsune Miku: over Chip!", 1), "Hatsune Miku over Chip");
        assert_eq!(folder_name("../..", 4), "GameBanana 4");
        assert_eq!(plain("<p>One</p><p>Two</p>"), "One\nTwo");
    }
}

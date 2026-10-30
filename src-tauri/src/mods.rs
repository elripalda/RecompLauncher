//! Mods, one click: the recomps that have a Thunderstore community (Zelda 64: Recompiled,
//! Banjo: Recompiled and others like them) list their mods on the game's page. Installing puts the
//! mod's files (.nrm, texture packs, and the library for this system) in the game's mods folder and
//! remembers them, so they can be taken out again.

use crate::Error;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Where a game's mods come from and where they go.
#[derive(Debug, Clone, Serialize)]
pub struct Source {
    /// The Thunderstore community, as in thunderstore.io/c/<community>.
    pub community: String,
    /// The recomp's data folder name (in AppData\Local on Windows, ~/.config on Linux).
    pub data: String,
}

/// Recomps known to take Thunderstore mods. The catalog can name others with
/// `"mods": {"thunderstore": "...", "data": "..."}`.
pub fn source(id: &str, entry: &Value) -> Option<Source> {
    if let Some(m) = entry.get("mods") {
        let c = m.get("thunderstore").and_then(Value::as_str).unwrap_or("");
        let d = m.get("data").and_then(Value::as_str).unwrap_or("");
        if ok_name(c) && ok_name(d) {
            return Some(Source { community: c.into(), data: d.into() });
        }
    }
    let (c, d) = match id {
        "zelda64recomp" => ("zelda-64-recompiled", "Zelda64Recompiled"),
        "banjorecomp" => ("banjo-recompiled", "BanjoRecompiled"),
        _ => return None,
    };
    Some(Source { community: c.into(), data: d.into() })
}

fn ok_name(s: &str) -> bool {
    !s.is_empty() && s.len() < 80 && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_ .".contains(c)) && !s.contains("..")
}

/// The game's mods folder: beside the game when it runs portable (portable.txt), otherwise in
/// its data folder.
pub fn mods_dir(game: &Path, data: &str) -> PathBuf {
    if game.join("portable.txt").is_file() {
        return game.join("mods");
    }
    let base = if cfg!(windows) {
        dirs::data_local_dir()
    } else if cfg!(target_os = "macos") {
        dirs::data_dir()
    } else {
        dirs::config_dir()
    };
    base.unwrap_or_else(|| game.to_path_buf()).join(data).join("mods")
}

/// One mod on Thunderstore, its newest version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pkg {
    /// Owner-Name, the package's id.
    pub full: String,
    pub name: String,
    pub owner: String,
    pub desc: String,
    pub icon: String,
    pub version: String,
    pub url: String,
    pub page: String,
    pub downloads: u64,
    pub rating: u64,
    pub updated: String,
    pub size: u64,
    /// Owner-Name of the mods it needs.
    pub deps: Vec<String>,
    pub categories: Vec<String>,
}

/// A mod the launcher installed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModRec {
    pub full: String,
    pub name: String,
    pub version: String,
    /// The files it put in the mods folder (names only).
    pub files: Vec<String>,
    /// Put in because another mod needed it.
    #[serde(default)]
    pub dep: bool,
}

fn tidy(s: &str) -> String {
    s.replace('_', " ")
}

fn parse(list: &Value) -> Vec<Pkg> {
    let mut out = vec![];
    for p in list.as_array().into_iter().flatten() {
        if p.get("is_deprecated").and_then(Value::as_bool).unwrap_or(false) || p.get("has_nsfw_content").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        let Some(v) = p.get("versions").and_then(Value::as_array).and_then(|v| v.first()) else { continue };
        let s = |x: &Value, k: &str| x.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let downloads = p.get("versions").and_then(Value::as_array).map(|vs| vs.iter().filter_map(|x| x.get("downloads")?.as_u64()).sum()).unwrap_or(0);
        let deps = v
            .get("dependencies")
            .and_then(Value::as_array)
            .map(|d| {
                d.iter()
                    .filter_map(Value::as_str)
                    // Owner-Name-1.2.3 -> Owner-Name
                    .map(|x| x.rsplitn(2, '-').nth(1).unwrap_or(x).to_string())
                    .collect()
            })
            .unwrap_or_default();
        out.push(Pkg {
            full: s(p, "full_name"),
            name: tidy(&s(p, "name")),
            owner: s(p, "owner"),
            desc: s(v, "description"),
            icon: s(v, "icon"),
            version: s(v, "version_number"),
            url: s(v, "download_url"),
            page: s(p, "package_url"),
            downloads,
            rating: p.get("rating_score").and_then(Value::as_u64).unwrap_or(0),
            updated: s(p, "date_updated"),
            size: v.get("file_size").and_then(Value::as_u64).unwrap_or(0),
            deps,
            categories: p.get("categories").and_then(Value::as_array).map(|c| c.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default(),
        });
    }
    out
}

/// Every mod in the community, most downloaded first. Kept on disk for an hour, and used as is
/// when Thunderstore can't be reached.
pub async fn list(http: &reqwest::Client, dir: &Path, community: &str) -> Result<Vec<Pkg>, Error> {
    if !ok_name(community) {
        return Err(Error::Msg("That isn't a mod community.".into()));
    }
    let file = dir.join("cache/mods").join(format!("{community}.json"));
    let age = std::fs::metadata(&file).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).map(|d| d.as_secs());
    let saved = || std::fs::read(&file).ok().and_then(|b| serde_json::from_slice::<Vec<Pkg>>(&b).ok());
    if age.map(|a| a < 3600).unwrap_or(false) {
        if let Some(v) = saved() {
            return Ok(v);
        }
    }
    let url = format!("https://thunderstore.io/c/{community}/api/v1/package/");
    let fresh = async {
        let r = http.get(&url).timeout(Duration::from_secs(40)).send().await?;
        if !r.status().is_success() {
            return Err(Error::Msg(format!("Thunderstore answered {}", r.status().as_u16())));
        }
        Ok::<Value, Error>(r.json::<Value>().await?)
    }
    .await;
    match fresh {
        Ok(v) => {
            let mut pkgs = parse(&v);
            pkgs.sort_by(|a, b| b.downloads.cmp(&a.downloads));
            let _ = std::fs::create_dir_all(file.parent().unwrap());
            let _ = std::fs::write(&file, serde_json::to_vec(&pkgs).unwrap_or_default());
            Ok(pkgs)
        }
        Err(e) => saved().ok_or(e),
    }
}

/// The files of a mod's zip that go in the mods folder: mods, texture packs, and the native
/// library for this system. Anything else (readme, icon, manifest) stays out.
fn wanted(name: &str) -> bool {
    let n = name.to_lowercase();
    let lib = if cfg!(windows) {
        ".dll"
    } else if cfg!(target_os = "macos") {
        ".dylib"
    } else {
        ".so"
    };
    n.ends_with(".nrm") || n.ends_with(".rtz") || n.ends_with(lib)
}

/// Put a downloaded mod zip's files in the mods folder. Returns the file names.
pub fn unpack(zip_bytes: &[u8], mods: &Path) -> Result<Vec<String>, Error> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).map_err(|e| Error::Msg(format!("Couldn't open the mod: {e}")))?;
    std::fs::create_dir_all(mods)?;
    let mut out = vec![];
    for i in 0..z.len() {
        let mut e = z.by_index(i).map_err(|e| Error::Msg(format!("Couldn't read the mod: {e}")))?;
        if e.is_dir() {
            continue;
        }
        let Some(name) = e.enclosed_name().and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string())) else { continue };
        if !wanted(&name) {
            continue;
        }
        let mut buf = vec![];
        e.read_to_end(&mut buf)?;
        let dest = mods.join(&name);
        let tmp = mods.join(format!(".{name}.part"));
        std::fs::write(&tmp, &buf)?;
        std::fs::rename(&tmp, &dest).map_err(|e| crate::defender_msg(e, &name))?;
        out.push(name);
    }
    if out.is_empty() {
        return Err(Error::Msg("This mod has no files the game reads by itself. Its page says how to set it up.".into()));
    }
    Ok(out)
}

/// Take a mod's files out of the mods folder.
pub fn remove(mods: &Path, files: &[String]) {
    for f in files {
        if ok_file(f) {
            let _ = std::fs::remove_file(mods.join(f));
        }
    }
}

fn ok_file(f: &str) -> bool {
    !f.is_empty() && !f.contains('/') && !f.contains('\\') && f != "." && f != ".."
}

/// Mods in the folder the launcher didn't put there (added by hand).
pub fn others(mods: &Path, known: &[ModRec]) -> Vec<String> {
    let mine: Vec<&String> = known.iter().flat_map(|m| m.files.iter()).collect();
    let mut out: Vec<String> = std::fs::read_dir(mods)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| (n.to_lowercase().ends_with(".nrm") || n.to_lowercase().ends_with(".rtz")) && !mine.contains(&n))
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn known_games_and_catalog_override() {
        assert_eq!(source("zelda64recomp", &Value::Null).unwrap().community, "zelda-64-recompiled");
        assert!(source("fifa", &Value::Null).is_none());
        let e = serde_json::json!({"mods": {"thunderstore": "dk64-recompiled", "data": "DK64Recompiled"}});
        assert_eq!(source("dk64", &e).unwrap().data, "DK64Recompiled");
        let bad = serde_json::json!({"mods": {"thunderstore": "../x", "data": "a"}});
        assert!(source("dk64", &bad).is_none());
    }

    #[test]
    fn parses_the_list() {
        let v = serde_json::json!([
            {"full_name": "A-Cool_Mod", "name": "Cool_Mod", "owner": "A", "package_url": "https://thunderstore.io/x", "date_updated": "2026", "rating_score": 3,
             "versions": [{"description": "d", "icon": "i", "version_number": "1.1.0", "download_url": "u", "downloads": 10, "file_size": 5, "dependencies": ["B-Lib-2.0.0"]}, {"downloads": 5}]},
            {"full_name": "Old", "is_deprecated": true, "versions": [{}]}
        ]);
        let p = parse(&v);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].name, "Cool Mod");
        assert_eq!(p[0].downloads, 15);
        assert_eq!(p[0].deps, vec!["B-Lib"]);
    }

    #[test]
    fn unpacks_only_mod_files() {
        let mut buf = std::io::Cursor::new(vec![]);
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            let o = zip::write::SimpleFileOptions::default();
            for n in ["manifest.json", "icon.png", "mods/cool.nrm", "pack.rtz", "../evil.nrm"] {
                z.start_file(n, o).unwrap();
                z.write_all(b"x").unwrap();
            }
            z.finish().unwrap();
        }
        let d = std::env::temp_dir().join(format!("rl-mods-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let mut got = unpack(buf.get_ref(), &d).unwrap();
        got.sort();
        assert_eq!(got, vec!["cool.nrm", "pack.rtz"]);
        assert!(d.join("cool.nrm").is_file());
        assert_eq!(others(&d, &[]), vec!["cool.nrm", "pack.rtz"]);
        remove(&d, &got);
        assert!(!d.join("cool.nrm").exists());
    }
}

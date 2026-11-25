//! LaunchBox on this PC: the pictures and descriptions a player's own LaunchBox has already saved,
//! offered in Customize next to SteamGridDB.
//!
//! LaunchBox has no online API and no keys, so nothing is asked of LaunchBox itself. A player who
//! uses it already has its folder: `Data/Platforms/<console>.xml` lists their games (title, console,
//! description...), and `Images/<console>/<kind>/[<region>/]<title>-01.jpg` holds the pictures
//! LaunchBox downloaded for them. That folder is only read, never written, and a picked picture goes
//! through `art_source` like the player's own file.

use crate::{Error, Res};
use base64::Engine as _;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// A Platforms file bigger than this isn't read (a big library's is around 50 MB).
const MOST_XML: u64 = 400 * 1024 * 1024;
/// A picture bigger than this gets no thumbnail (art_source takes 30 MB at most anyway).
const MOST_PIC: u64 = 30 * 1024 * 1024;
const THUMB: u32 = 240;
const MOST_PICS: usize = 30;
const MOST_HITS: usize = 12;

/// LaunchBox's picture folders for each kind Customize frames, best first. An icon is cut from
/// the box art or the logo in Customize's frame, since LaunchBox keeps no square icons by default.
fn folders_for(kind: &str) -> &'static [&'static str] {
    match kind {
        "cover" => &["Box - Front", "Box - Front - Reconstructed", "Fanart - Box - Front", "Box - 3D"],
        "hero" => &["Fanart - Background", "Screenshot - Game Title", "Screenshot - Gameplay"],
        "logo" => &["Clear Logo"],
        "icon" => &["Square", "Icon", "Box - Front", "Clear Logo"],
        _ => &[],
    }
}

/// LaunchBox's console names for the store's console keys, so a game on its own console comes
/// first. The same names the website's LaunchBox source uses.
fn platforms_for(console: &str) -> &'static [&'static str] {
    match console {
        "n64" => &["Nintendo 64"],
        "x360" => &["Microsoft Xbox 360"],
        "xbox" => &["Microsoft Xbox"],
        "xseries" => &["Microsoft Xbox Series X/S"],
        "ps1" => &["Sony Playstation"],
        "ps2" => &["Sony Playstation 2"],
        "ps3" => &["Sony Playstation 3"],
        "ps4" => &["Sony Playstation 4"],
        "ps5" => &["Sony Playstation 5"],
        "gc" => &["Nintendo GameCube"],
        "wii" => &["Nintendo Wii"],
        "wiiu" => &["Nintendo Wii U"],
        "snes" => &["Super Nintendo Entertainment System"],
        "gba" => &["Nintendo Game Boy Advance"],
        "gb" => &["Nintendo Game Boy"],
        "gbc" => &["Nintendo Game Boy Color"],
        "ds" => &["Nintendo DS"],
        "3ds" => &["Nintendo 3DS"],
        "dc" => &["Sega Dreamcast"],
        "saturn" => &["Sega Saturn"],
        "psp" => &["Sony PSP"],
        "switch" => &["Nintendo Switch"],
        "switch2" => &["Nintendo Switch 2"],
        "nes" => &["Nintendo Entertainment System", "Nintendo Famicom Disk System"],
        "vb" => &["Nintendo Virtual Boy"],
        "genesis" => &["Sega Genesis", "Sega CD"],
        "s32x" => &["Sega 32X", "Sega CD 32X"],
        "gg" => &["Sega Game Gear"],
        "arcade" => &["Arcade"],
        "cdi" => &["Philips CD-i"],
        "amiga" => &["Commodore Amiga", "Commodore Amiga CD32"],
        "pc" => &["Windows", "MS-DOS"],
        _ => &[],
    }
}

/// One game from the player's LaunchBox.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LbGame {
    pub title: String,
    pub platform: String,
    pub notes: String,
    pub released: String,
    pub developer: String,
    pub publisher: String,
}

/// A name as lowercase letters and numbers only: "Assassin's Creed: II" and LaunchBox's file name
/// for it, "Assassin_s Creed_ II", come out the same.
pub fn key(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// The words of a search, lowercase, letters and numbers only.
fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect()
}

/// XML's escapes back to text (LaunchBox writes &amp; &lt; &gt; &quot; &apos; and numbered ones).
fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';').filter(|&e| e <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..end];
        let named = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => None,
        };
        let numbered = name.strip_prefix("#x").map(|h| u32::from_str_radix(h, 16)).or_else(|| name.strip_prefix('#').map(str::parse::<u32>));
        let ch = named.or_else(|| numbered.and_then(Result::ok).and_then(char::from_u32));
        match ch {
            Some(c) => out.push(c),
            None => out.push_str(&rest[..=end]),
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

/// The text of `<tag>…</tag>` inside one game's block, or "" (also for an empty `<tag />`).
fn field(block: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let Some(start) = block.find(&open).map(|i| i + open.len()) else { return String::new() };
    let Some(len) = block[start..].find(&close) else { return String::new() };
    unescape(block[start..start + len].trim())
}

/// The games in one of LaunchBox's Platforms files. LaunchBox writes these itself, one flat
/// `<Game>` element per game with one child per field, so each block is cut out and read by its
/// fields; other elements (`<AdditionalApplication>`, `<CustomField>`...) are passed over.
pub fn games_in(xml: &str) -> Vec<LbGame> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find("<Game>") {
        let after = &rest[i + "<Game>".len()..];
        let Some(end) = after.find("</Game>") else { break };
        let block = &after[..end];
        rest = &after[end + "</Game>".len()..];
        let title = field(block, "Title");
        if title.is_empty() {
            continue;
        }
        let released = field(block, "ReleaseDate").chars().take(10).collect();
        out.push(LbGame {
            title,
            platform: field(block, "Platform"),
            notes: field(block, "Notes"),
            released,
            developer: field(block, "Developer"),
            publisher: field(block, "Publisher"),
        });
    }
    out
}

/// Is this LaunchBox's own folder (the one with LaunchBox.exe, Data and Images)?
pub fn is_root(p: &Path) -> bool {
    p.join("Data").join("Platforms").is_dir()
}

/// Where LaunchBox usually is: its installer puts it in the user's folder; people also keep it at
/// the top of a drive or under Games. The user's folder is looked at first, and only then the
/// PC's own drives. -> the first one that's really LaunchBox's folder.
fn guess() -> Option<PathBuf> {
    let mut maybe = Vec::new();
    if let Some(h) = dirs::home_dir() {
        maybe.push(h.join("LaunchBox"));
        maybe.push(h.join("Documents").join("LaunchBox"));
        maybe.push(h.join("Games").join("LaunchBox"));
    }
    for drive in fixed_drives() {
        maybe.push(drive.join("LaunchBox"));
        maybe.push(drive.join("Games").join("LaunchBox"));
    }
    maybe.into_iter().find(|p| is_root(p))
}

/// The PC's own drives (C:\, D:\...). Network, removable and CD drives are left out: a mapped
/// drive that's disconnected can take seconds to answer.
#[cfg(windows)]
fn fixed_drives() -> Vec<PathBuf> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    const DRIVE_FIXED: u32 = 3;
    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let root = format!("{}:\\", (b'A' + i as u8) as char);
        let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
        let kind = unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) };
        if kind == DRIVE_FIXED {
            out.push(PathBuf::from(root));
        }
    }
    out
}
#[cfg(not(windows))]
fn fixed_drives() -> Vec<PathBuf> {
    Vec::new()
}

/// Every Platforms file with when it last changed, so the games read from them are read again
/// only after LaunchBox changes one.
fn platform_files(root: &Path) -> Vec<(PathBuf, SystemTime)> {
    let Ok(rd) = std::fs::read_dir(root.join("Data").join("Platforms")) else { return Vec::new() };
    let mut files: Vec<(PathBuf, SystemTime)> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("xml")))
        .filter_map(|p| {
            let m = std::fs::metadata(&p).ok()?;
            (m.len() <= MOST_XML).then(|| (p, m.modified().unwrap_or(SystemTime::UNIX_EPOCH)))
        })
        .collect();
    files.sort();
    files
}

type Read = (PathBuf, Vec<(PathBuf, SystemTime)>, Arc<Vec<LbGame>>);
static READ: Mutex<Option<Read>> = Mutex::new(None);

/// The player's LaunchBox games, read once and kept until a Platforms file changes.
fn games(root: &Path) -> Arc<Vec<LbGame>> {
    let files = platform_files(root);
    if let Some((r, f, g)) = READ.lock().unwrap().as_ref() {
        if r == root && *f == files {
            return g.clone();
        }
    }
    let mut all = Vec::new();
    for (p, _) in &files {
        let Ok(bytes) = std::fs::read(p) else { continue };
        all.extend(games_in(&String::from_utf8_lossy(&bytes)));
    }
    let all = Arc::new(all);
    *READ.lock().unwrap() = Some((root.to_path_buf(), files, all.clone()));
    all
}

/// How well a game fits a search: every word in its title, its own console first, then the same
/// title, then the closest in length. None: not every word is in it.
fn score(g: &LbGame, want: &[String], want_key: &str, plats: &[&str]) -> Option<i64> {
    let title = words(&g.title);
    if !want.iter().all(|w| title.iter().any(|t| t.starts_with(w.as_str()))) {
        return None;
    }
    let k = key(&g.title);
    let mut s = 0i64;
    if k == want_key {
        s += 100;
    } else if k.starts_with(want_key) {
        s += 30;
    }
    // The console counts most: a recomp of the N64 game wants the N64 game's pictures, even when
    // the SNES game has the name typed exactly.
    if plats.contains(&g.platform.as_str()) {
        s += 120;
    }
    Some(s - (k.len() as i64 - want_key.len() as i64).abs().min(40))
}

/// The player's games that fit a search, best first.
pub fn find(all: &[LbGame], term: &str, console: &str) -> Vec<LbGame> {
    let want = words(term);
    if want.is_empty() {
        return Vec::new();
    }
    let want_key = key(term);
    let plats = platforms_for(console);
    let mut hits: Vec<(i64, &LbGame)> = all.iter().filter_map(|g| score(g, &want, &want_key, plats).map(|s| (s, g))).collect();
    hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.title.cmp(&b.1.title)));
    let mut out: Vec<LbGame> = Vec::new();
    for (_, g) in hits {
        // The same game listed twice (two copies of a Platforms entry) shows once.
        if out.iter().any(|o| o.title == g.title && o.platform == g.platform) {
            continue;
        }
        out.push(g.clone());
        if out.len() == MOST_HITS {
            break;
        }
    }
    out
}

/// A folder under `parent` whose name is `name`, matched by letters and numbers only (a console
/// LaunchBox saved as "Sony Playstation" or with characters Windows doesn't allow in names).
/// Only a plain name is joined on: one with a path in it ("..", "C:\\x", "a/b") is only ever
/// compared with the folders that are really there, so nothing outside `parent` is looked at.
fn child(parent: &Path, name: &str) -> Option<PathBuf> {
    let plain = !name.contains(['/', '\\', ':']) && name != "." && name != "..";
    let exact = parent.join(name);
    if plain && !name.is_empty() && exact.is_dir() {
        return Some(exact);
    }
    let want = key(name);
    if want.is_empty() {
        return None;
    }
    std::fs::read_dir(parent).ok()?.flatten().map(|e| e.path()).find(|p| p.is_dir() && p.file_name().is_some_and(|n| key(&n.to_string_lossy()) == want))
}

/// Is this file a picture of that game? LaunchBox names them "<title>-01.jpg", "<title>-02.png"...
/// with the characters Windows doesn't allow in names (and apostrophes) as "_".
pub fn is_picture_of(file: &Path, title_key: &str) -> bool {
    let ext = file.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
    if !matches!(ext.as_str(), "png" | "jpg" | "jpeg") {
        return false;
    }
    let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let base = match stem.rsplit_once('-') {
        Some((b, n)) if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) => b,
        _ => stem.as_str(),
    };
    !title_key.is_empty() && key(base) == title_key
}

/// One game's pictures for a kind: (file, LaunchBox's kind, region), best kind first, then no
/// region or North America before other regions.
pub fn pictures(root: &Path, platform: &str, title: &str, kind: &str) -> Vec<(PathBuf, String, String)> {
    let Some(images) = child(&root.join("Images"), platform) else { return Vec::new() };
    let title_key = key(title);
    let mut out = Vec::new();
    for folder in folders_for(kind) {
        let Some(dir) = child(&images, folder) else { continue };
        let mut found: Vec<(u8, PathBuf, String)> = Vec::new();
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_file() {
                if is_picture_of(&p, &title_key) {
                    found.push((0, p, String::new()));
                }
                continue;
            }
            // A region's own folder: Images/<console>/Box - Front/North America/...
            let region = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let rank = match region.as_str() {
                "North America" | "United States" | "World" => 1,
                "Europe" | "United Kingdom" | "Australia" => 2,
                _ => 3,
            };
            for f in std::fs::read_dir(&p).into_iter().flatten().flatten() {
                let fp = f.path();
                if fp.is_file() && is_picture_of(&fp, &title_key) {
                    found.push((rank, fp, region.clone()));
                }
            }
        }
        found.sort();
        out.extend(found.into_iter().map(|(_, p, r)| (p, folder.to_string(), r)));
        if out.len() >= MOST_PICS {
            break;
        }
    }
    out.truncate(MOST_PICS);
    out
}

/// A small copy of a picture for the list, as a `data:` address (the picture itself stays where
/// LaunchBox keeps it until it's picked). None: too big or not a PNG or JPEG.
fn thumb(p: &Path) -> Option<String> {
    if std::fs::metadata(p).ok()?.len() > MOST_PIC {
        return None;
    }
    let img = image::open(p).ok()?.thumbnail(THUMB, THUMB);
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img.to_rgb8()).write_to(&mut out, image::ImageFormat::Jpeg).ok()?;
    Some(format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(out.into_inner())))
}

/// The folder the window gave, if it's LaunchBox's.
fn root_of(path: &str) -> Res<PathBuf> {
    let p = PathBuf::from(path.trim());
    if path.trim().is_empty() || !is_root(&p) {
        return Err(Error::Msg("That isn't LaunchBox's folder. Pick the folder LaunchBox is in (it has Data and Images in it).".into()));
    }
    Ok(p)
}

/// LaunchBox's folder: the one given, checked, or (none given) the one found where LaunchBox
/// usually is. -> { path } or null.
#[tauri::command]
pub async fn launchbox_find(path: Option<String>) -> Res<Value> {
    let given = path.filter(|p| !p.trim().is_empty());
    let found = tokio::task::spawn_blocking(move || match given {
        Some(p) => root_of(&p).map(Some),
        None => Ok(guess()),
    })
    .await
    .map_err(|e| Error::Msg(e.to_string()))??;
    Ok(found.map(|p| json!({ "path": p })).unwrap_or(Value::Null))
}

/// The player's LaunchBox games that fit a search, the game's own console first:
/// [{ title, platform, year, released, developer, publisher, notes }].
#[tauri::command]
pub async fn launchbox_search(path: String, term: String, console: Option<String>) -> Res<Value> {
    let root = root_of(&path)?;
    let console = console.unwrap_or_default();
    let hits = tokio::task::spawn_blocking(move || find(&games(&root), &term, &console)).await.map_err(|e| Error::Msg(e.to_string()))?;
    let list: Vec<Value> = hits
        .into_iter()
        .map(|g| {
            let year: String = g.released.chars().take(4).collect();
            json!({ "title": g.title, "platform": g.platform, "year": year, "released": g.released,
                "developer": g.developer, "publisher": g.publisher, "notes": g.notes })
        })
        .collect();
    Ok(Value::Array(list))
}

/// One game's pictures for a kind, with small copies to pick from: [{ path, thumb, type, region }].
#[tauri::command]
pub async fn launchbox_pictures(path: String, platform: String, title: String, kind: String) -> Res<Value> {
    let root = root_of(&path)?;
    if folders_for(&kind).is_empty() {
        return Err(Error::Msg("That isn't a kind of picture.".into()));
    }
    let list = tokio::task::spawn_blocking(move || {
        pictures(&root, &platform, &title, &kind)
            .into_iter()
            .filter_map(|(p, kind, region)| {
                let small = thumb(&p)?;
                Some(json!({ "path": p, "thumb": small, "type": kind, "region": region }))
            })
            .collect::<Vec<Value>>()
    })
    .await
    .map_err(|e| Error::Msg(e.to_string()))?;
    Ok(Value::Array(list))
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<?xml version="1.0" standalone="yes"?>
<LaunchBox>
  <Game>
    <Title>Assassin&apos;s Creed: II</Title>
    <Platform>Windows</Platform>
    <ReleaseDate>2009-11-17T00:00:00-08:00</ReleaseDate>
    <Developer>Ubisoft Montreal</Developer>
    <Notes>Ezio &amp; friends.</Notes>
  </Game>
  <AdditionalApplication><Title>Not a game</Title></AdditionalApplication>
  <Game>
    <Title>Star Fox 64</Title>
    <Platform>Nintendo 64</Platform>
    <Notes />
  </Game>
  <Game><Title></Title></Game>
  <GameControllerSupport><Title>Also not a game</Title></GameControllerSupport>
</LaunchBox>"#;

    #[test]
    fn reads_only_game_blocks() {
        let g = games_in(XML);
        assert_eq!(g.len(), 2);
    }

    #[test]
    fn reads_fields_and_unescapes() {
        let g = &games_in(XML)[0];
        assert_eq!(g.title, "Assassin's Creed: II");
        assert_eq!(g.released, "2009-11-17");
        assert_eq!(g.notes, "Ezio & friends.");
        assert_eq!(g.developer, "Ubisoft Montreal");
    }

    #[test]
    fn empty_field_is_empty() {
        assert_eq!(games_in(XML)[1].notes, "");
    }

    #[test]
    fn unescapes_numbered() {
        assert_eq!(unescape("Pok&#233;mon &#x26; &bogus; a&b"), "Pokémon & &bogus; a&b");
    }

    #[test]
    fn file_names_match_titles() {
        let k = key("Assassin's Creed: II");
        assert!(is_picture_of(Path::new("Assassin_s Creed_ II-01.jpg"), &k));
        assert!(is_picture_of(Path::new("Assassin_s Creed_ II-12.png"), &k));
        assert!(!is_picture_of(Path::new("Assassin_s Creed_ II-01.webp"), &k));
        assert!(!is_picture_of(Path::new("Assassin_s Creed III-01.jpg"), &k));
    }

    #[test]
    fn search_puts_own_console_first() {
        let all = vec![
            LbGame { title: "Star Fox".into(), platform: "Super Nintendo Entertainment System".into(), ..Default::default() },
            LbGame { title: "Star Fox 64".into(), platform: "Nintendo 64".into(), ..Default::default() },
        ];
        let hits = find(&all, "star fox", "n64");
        assert_eq!(hits[0].title, "Star Fox 64");
    }

    #[test]
    fn search_needs_every_word() {
        let all = vec![LbGame { title: "Star Fox 64".into(), platform: "Nintendo 64".into(), ..Default::default() }];
        assert!(find(&all, "star wars", "").is_empty());
    }

    #[test]
    fn thumbnail_is_small_jpeg() {
        let p = std::env::temp_dir().join(format!("rl-lb-thumb-{}.png", std::process::id()));
        image::RgbImage::new(900, 1200).save(&p).unwrap();
        let t = thumb(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        assert!(t.starts_with("data:image/jpeg;base64,"));
    }

    #[test]
    fn a_console_name_never_leaves_the_folder() {
        let root = std::env::temp_dir().join(format!("rl-lb-out-{}", std::process::id()));
        std::fs::create_dir_all(root.join("Images").join("Nintendo 64")).unwrap();
        std::fs::create_dir_all(root.join("Outside")).unwrap();
        let images = root.join("Images");
        assert!(child(&images, "..").is_none());
        assert!(child(&images, "../Outside").is_none());
        assert_eq!(child(&images, "Nintendo 64"), Some(images.join("Nintendo 64")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn pictures_by_kind_and_region() {
        let root = std::env::temp_dir().join(format!("rl-lb-test-{}", std::process::id()));
        let front = root.join("Images").join("Nintendo 64").join("Box - Front");
        std::fs::create_dir_all(front.join("Europe")).unwrap();
        std::fs::create_dir_all(front.join("North America")).unwrap();
        std::fs::create_dir_all(root.join("Data").join("Platforms")).unwrap();
        std::fs::write(front.join("Europe").join("Star Fox 64-01.jpg"), b"x").unwrap();
        std::fs::write(front.join("North America").join("Star Fox 64-01.jpg"), b"x").unwrap();
        std::fs::write(front.join("Star Fox 64-02.png"), b"x").unwrap();
        std::fs::write(front.join("Star Fox-01.png"), b"x").unwrap();
        let pics = pictures(&root, "Nintendo 64", "Star Fox 64", "cover");
        let regions: Vec<&str> = pics.iter().map(|p| p.2.as_str()).collect();
        assert!(is_root(&root));
        assert_eq!(regions, ["", "North America", "Europe"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}

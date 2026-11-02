//! Customize: the player's own pictures for a game (cover, background, logo, icon), from
//! SteamGridDB or their own files, framed in the window and kept in the launcher's covers folder.
//!
//! The window frames a picture on a canvas and sends back the finished one. A picture from another
//! place (a file, SteamGridDB) can't be cut on the window's canvas, so it comes through here as a
//! `data:` address. The picture as it came is kept beside the finished one, with how it was framed,
//! so Customize can open it again just as it was left.

use crate::library::{folder_name, now, Installed};
use crate::{App, Error, Res};
use base64::Engine as _;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tauri::State;

pub const KINDS: [&str; 4] = ["cover", "hero", "logo", "icon"];
const MOST: u64 = 30 * 1024 * 1024;

fn covers(st: &App) -> PathBuf {
    st.dir.join("covers")
}

fn ext_for(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => "img",
    }
}

fn mime_of(b: &[u8]) -> &'static str {
    match b {
        [b'B', b'M', ..] => "image/bmp",
        _ => crate::media::mime(b),
    }
}

fn data_url(b: &[u8]) -> String {
    format!("data:{};base64,{}", mime_of(b), base64::engine::general_purpose::STANDARD.encode(b))
}

/// A file in the covers folder (never anything else is deleted).
fn remove_ours(st: &App, p: &Path) {
    if p.starts_with(covers(st)) {
        let _ = std::fs::remove_file(p);
    }
}

fn field<'a>(g: &'a mut Installed, kind: &str) -> Option<&'a mut Option<PathBuf>> {
    match kind {
        "cover" => Some(&mut g.cover_file),
        "hero" => Some(&mut g.hero_file),
        "logo" => Some(&mut g.logo_file),
        "icon" => Some(&mut g.icon_file),
        _ => None,
    }
}

/// A picture to frame: from the player's file (`path`) or the web (`url`, SteamGridDB's or the
/// site's). Kept in the covers folder as it came; returns { src, data }.
#[tauri::command]
pub async fn art_source(st: State<'_, App>, id: String, kind: String, path: Option<String>, url: Option<String>) -> Res<Value> {
    if !KINDS.contains(&kind.as_str()) {
        return Err(Error::Msg("That isn't a kind of picture.".into()));
    }
    let bytes = match (path.filter(|p| !p.trim().is_empty()), url.filter(|u| !u.trim().is_empty())) {
        (Some(p), _) => {
            let p = PathBuf::from(p);
            if std::fs::metadata(&p).map(|m| m.len() > MOST).unwrap_or(true) {
                return Err(Error::Msg("That picture is too big (30 MB at most).".into()));
            }
            tokio::fs::read(&p).await?
        }
        (None, Some(u)) => {
            if !u.starts_with("https://") {
                return Err(Error::Msg("That picture isn't on a secure address.".into()));
            }
            let r = st.http.get(&u).timeout(std::time::Duration::from_secs(40)).send().await?;
            if !r.status().is_success() {
                return Err(Error::Msg(format!("The picture's site answered {}.", r.status().as_u16())));
            }
            if r.content_length().unwrap_or(0) > MOST {
                return Err(Error::Msg("That picture is too big (30 MB at most).".into()));
            }
            r.bytes().await?.to_vec()
        }
        _ => return Err(Error::Msg("Pick a picture first.".into())),
    };
    let mime = mime_of(&bytes);
    if !mime.starts_with("image/") || mime == "image/svg+xml" {
        return Err(Error::Msg("Pick a picture: PNG, JPG, WebP, GIF or BMP.".into()));
    }
    std::fs::create_dir_all(covers(&st))?;
    let src = covers(&st).join(format!("{}-{kind}-src-{}.{}", folder_name(&id), now(), if mime == "image/bmp" { "bmp" } else { ext_for(mime) }));
    tokio::fs::write(&src, &bytes).await?;
    Ok(json!({ "src": src, "data": data_url(&bytes) }))
}

/// What Customize needs to open a picture the player framed before: { src, data, x, y, zoom }.
#[tauri::command]
pub async fn art_open(st: State<'_, App>, id: String, kind: String) -> Res<Value> {
    let edit = st.lib.lock().unwrap().installed.get(&id).and_then(|g| g.art_edit.get(&kind).cloned());
    let Some(mut e) = edit else { return Ok(Value::Null) };
    let src = e.get("src").and_then(Value::as_str).map(PathBuf::from).filter(|p| p.is_file());
    let Some(src) = src else { return Ok(Value::Null) };
    let bytes = tokio::fs::read(&src).await?;
    e["data"] = Value::String(data_url(&bytes));
    Ok(e)
}

/// The finished picture from Customize (`data`, a PNG or JPEG `data:` address), with the picture it
/// was framed from (`src`) and how (`frame`: x, y, zoom). No `data`: back to the usual picture.
#[tauri::command]
pub async fn art_save(st: State<'_, App>, id: String, kind: String, data: Option<String>, src: Option<String>, frame: Option<Value>) -> Res<Installed> {
    if !KINDS.contains(&kind.as_str()) {
        return Err(Error::Msg("That isn't a kind of picture.".into()));
    }
    let (old_out, old_src) = {
        let mut l = st.lib.lock().unwrap();
        let g = l.installed.get_mut(&id).ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
        let out = field(g, &kind).and_then(|f| f.clone());
        let src = g.art_edit.get(&kind).and_then(|e| e.get("src")).and_then(Value::as_str).map(PathBuf::from);
        (out, src)
    };
    let new_out = match data.as_deref().filter(|d| !d.is_empty()) {
        Some(d) => {
            let (head, body) = d.split_once(',').ok_or_else(|| Error::Msg("That picture didn't come through.".into()))?;
            let ext = if head.contains("image/png") { "png" } else if head.contains("image/jpeg") { "jpg" } else { return Err(Error::Msg("That picture didn't come through.".into())) };
            let bytes = base64::engine::general_purpose::STANDARD.decode(body.trim()).map_err(|_| Error::Msg("That picture didn't come through.".into()))?;
            if bytes.len() as u64 > MOST {
                return Err(Error::Msg("That picture is too big (30 MB at most).".into()));
            }
            std::fs::create_dir_all(covers(&st))?;
            let stamp = now();
            let file = covers(&st).join(format!("{}-{kind}-{stamp}.{ext}", folder_name(&id)));
            std::fs::write(&file, &bytes)?;
            // An icon as an .ico too: what Windows shortcuts take (a PNG inside, as Windows reads
            // since Vista).
            if kind == "icon" && ext == "png" {
                let _ = std::fs::write(file.with_extension("ico"), ico_from_png(&bytes));
            }
            Some(file)
        }
        None => None,
    };
    let new_src = src.map(PathBuf::from).filter(|p| p.starts_with(covers(&st)) && p.is_file());
    // What isn't used any more goes.
    if let Some(o) = old_out.as_ref().filter(|o| Some(*o) != new_out.as_ref()) {
        remove_ours(&st, o);
        if kind == "icon" {
            remove_ours(&st, &o.with_extension("ico"));
        }
    }
    if let Some(o) = old_src.as_ref().filter(|o| Some(*o) != new_src.as_ref()) {
        remove_ours(&st, o);
    }
    if new_out.is_none() {
        if let Some(s) = new_src.as_ref() {
            remove_ours(&st, s);
        }
    }
    let g = {
        let mut l = st.lib.lock().unwrap();
        let g = l.installed.get_mut(&id).ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
        if let Some(f) = field(g, &kind) {
            *f = new_out.clone();
        }
        match (&new_out, &new_src) {
            (Some(_), Some(s)) => {
                let mut e = json!({ "src": s });
                if let Some(Value::Object(fr)) = frame {
                    for k in ["x", "y", "zoom"] {
                        if let Some(v) = fr.get(k).filter(|v| v.is_number()) {
                            e[k] = v.clone();
                        }
                    }
                }
                g.art_edit.insert(kind.clone(), e);
            }
            _ => {
                g.art_edit.remove(&kind);
            }
        }
        g.clone()
    };
    st.save();
    Ok(g)
}

/// An .ico holding one PNG (Windows reads these since Vista).
pub fn ico_from_png(png: &[u8]) -> Vec<u8> {
    // Width and height from the PNG's header (0 in an icon entry means 256).
    let dim = |at: usize| -> u8 {
        let v = png.get(at..at + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]])).unwrap_or(0);
        if v >= 256 || v == 0 { 0 } else { v as u8 }
    };
    let (w, h) = (dim(16), dim(20));
    let mut out = Vec::with_capacity(22 + png.len());
    out.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
    out.extend_from_slice(&[w, h, 0, 0, 1, 0, 32, 0]);
    out.extend_from_slice(&(png.len() as u32).to_le_bytes());
    out.extend_from_slice(&22u32.to_le_bytes());
    out.extend_from_slice(png);
    out
}

/// SteamGridDB games matching a name, to pick the right one: [{ id, name, year }].
#[tauri::command]
pub async fn art_search(st: State<'_, App>, term: String, key: String) -> Res<Value> {
    crate::ghinfo::search(&st.http, key.trim(), &term).await.map_err(Error::Msg)
}

/// Every cover, background, logo and icon SteamGridDB has for one of its games.
#[tauri::command]
pub async fn art_for(st: State<'_, App>, sgdb: u64, key: String) -> Res<Value> {
    Ok(crate::ghinfo::art_of(&st.http, key.trim(), sgdb).await)
}

#[cfg(test)]
mod tests {
    #[test]
    fn icons_hold_their_png() {
        // A 1x1 PNG's header: width and height at 16 and 20.
        let mut png = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10, 0, 0, 0, 13, b'I', b'H', b'D', b'R'];
        png.extend_from_slice(&64u32.to_be_bytes());
        png.extend_from_slice(&64u32.to_be_bytes());
        png.extend_from_slice(&[8, 6, 0, 0, 0]);
        let ico = super::ico_from_png(&png);
        assert_eq!(&ico[..6], &[0, 0, 1, 0, 1, 0]);
        assert_eq!(ico[6], 64);
        assert_eq!(ico[7], 64);
        assert_eq!(u32::from_le_bytes([ico[14], ico[15], ico[16], ico[17]]) as usize, png.len());
        assert_eq!(u32::from_le_bytes([ico[18], ico[19], ico[20], ico[21]]), 22);
        assert_eq!(&ico[22..], &png[..]);
    }
}

//! Settings > About > Troubleshoot > Export Logs: every log the launcher keeps, in one zip on the
//! desktop, for a player who'd rather attach them by hand (Discord, GitHub, an email) than send a
//! report from the launcher.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use tauri::{AppHandle, State};

use crate::{App, Error};

type Res<T> = Result<T, Error>;

/// A game's log can grow without end; its newest part is what says what went wrong, and the zip
/// stays small enough to attach.
const MAX_FILE: u64 = 4_000_000;

#[tauri::command(async)]
pub fn export_logs(app: AppHandle, st: State<'_, App>) -> Res<String> {
    let path = destination()?;
    let files = gather(&st);
    write_zip(&path, &files).map_err(|e| Error::Msg(format!("Couldn't save the logs: {e}")))?;
    // The zip is there even when the file manager doesn't open, so that isn't an error.
    let _ = reveal(&app, &path);
    Ok(path.to_string_lossy().into())
}

/// The desktop, where a player finds it to attach; Downloads when there's no desktop folder.
fn destination() -> Res<PathBuf> {
    let desktop = dirs::desktop_dir().or_else(|| dirs::home_dir().map(|h| h.join("Desktop")));
    let folder = desktop.filter(|d| d.is_dir()).or_else(|| dirs::download_dir().filter(|d| d.is_dir()));
    let folder = folder.ok_or_else(|| Error::Msg("Couldn't find your desktop folder.".into()))?;
    let stamp = chrono::Local::now().format("%Y-%m-%d-%H%M%S");
    Ok(folder.join(format!("RecompLauncher-logs-{stamp}.zip")))
}

/// The files that go in the zip, as (name in the zip, contents).
fn gather(st: &App) -> Vec<(String, Vec<u8>)> {
    let pc = crate::usage::pc_info(&st.dir);
    let mut out = vec![("launcher-and-pc.txt".to_string(), crate::launcher_and_pc(st, &pc).into_bytes())];
    if let Some(b) = newest_part(&st.dir.join("launcher-trail.log")) {
        out.push(("launcher-trail.log".into(), b));
    }
    let ids: Vec<String> = st.lib.lock().unwrap().installed.keys().cloned().collect();
    for id in ids {
        out.extend(game_files(st, &id));
    }
    out
}

/// One game's logs, under games/<id>/: how the launcher last started it, what it printed, ReShade's
/// log and the newest log the game wrote itself.
fn game_files(st: &App, id: &str) -> Vec<(String, Vec<u8>)> {
    let folder = format!("games/{}", safe(id));
    let mut out = vec![];
    if let Some(run) = crate::last_run(&st.dir, id) {
        out.push((format!("{folder}/launcher-details.txt"), run.into_bytes()));
    }
    let printed = crate::game_output_path(&st.dir, id);
    let reshade = st.dir.join("looks").join(id).join("ReShade.log");
    // The game's own log, unless the newest one is what it printed or ReShade's (already in).
    let own = crate::recent_log_path(st, id, 0).filter(|p| p != &printed && p != &reshade);
    let mut files = vec![(printed, "game-output.log".to_string()), (reshade, "ReShade.log".to_string())];
    if let Some(p) = own {
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "game.log".into());
        files.push((p, name));
    }
    for (path, name) in files {
        if let Some(b) = newest_part(&path) {
            out.push((format!("{folder}/{name}"), b));
        }
    }
    out
}

/// A game id as a folder name in the zip.
fn safe(id: &str) -> String {
    id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' }).collect()
}

/// The newest MAX_FILE bytes of a file, read from the end so a huge log isn't loaded whole.
/// Nothing for a missing or empty file.
fn newest_part(p: &Path) -> Option<Vec<u8>> {
    let mut f = std::fs::File::open(p).ok()?;
    let len = f.metadata().ok().filter(|m| m.is_file())?.len();
    if len == 0 {
        return None;
    }
    f.seek(SeekFrom::Start(len.saturating_sub(MAX_FILE))).ok()?;
    let mut b = vec![];
    f.read_to_end(&mut b).ok()?;
    Some(b)
}

fn write_zip(path: &Path, files: &[(String, Vec<u8>)]) -> zip::result::ZipResult<()> {
    let mut z = zip::ZipWriter::new(std::fs::File::create(path)?);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in files {
        z.start_file(name.as_str(), opts)?;
        z.write_all(bytes)?;
    }
    z.finish()?;
    Ok(())
}

/// The zip, selected in the file manager (Linux opens its folder: there's no common way to select).
fn reveal(app: &AppHandle, p: &Path) -> Res<()> {
    #[cfg(target_os = "linux")]
    return crate::open_thing(app, &p.parent().unwrap_or(p).to_string_lossy(), false);
    #[cfg(not(target_os = "linux"))]
    {
        use tauri_plugin_opener::OpenerExt;
        app.opener().reveal_item_in_dir(p).map_err(|e| Error::Msg(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rl-logs-zip-{n}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_zip_holds_each_file_under_its_name() {
        let d = tmp("names");
        let zip_path = d.join("logs.zip");
        let files = vec![("launcher-trail.log".to_string(), b"started".to_vec()), ("games/sm64/game-output.log".to_string(), b"printed".to_vec())];
        write_zip(&zip_path, &files).unwrap();
        let mut z = zip::ZipArchive::new(std::fs::File::open(&zip_path).unwrap()).unwrap();
        let mut text = String::new();
        z.by_name("games/sm64/game-output.log").unwrap().read_to_string(&mut text).unwrap();
        assert_eq!(z.len(), 2);
        assert_eq!(text, "printed");
    }

    #[test]
    fn a_long_log_keeps_its_newest_part() {
        let d = tmp("long");
        let p = d.join("big.log");
        let mut body = vec![b'a'; MAX_FILE as usize];
        body.extend_from_slice(b"newest");
        std::fs::write(&p, &body).unwrap();
        let part = newest_part(&p).unwrap();
        assert_eq!(part.len() as u64, MAX_FILE);
        assert!(part.ends_with(b"newest"));
    }

    #[test]
    fn an_empty_or_missing_log_is_left_out() {
        let d = tmp("empty");
        std::fs::write(d.join("empty.log"), b"").unwrap();
        assert!(newest_part(&d.join("empty.log")).is_none());
        assert!(newest_part(&d.join("missing.log")).is_none());
        assert!(newest_part(&d).is_none());
    }

    #[test]
    fn a_game_id_is_a_plain_folder_name() {
        assert_eq!(safe("sos-ps5"), "sos-ps5");
        assert_eq!(safe("../x/y"), ".._x_y");
    }
}

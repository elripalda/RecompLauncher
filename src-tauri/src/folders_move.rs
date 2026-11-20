//! Folder Structure, the part that touches the disk: the designer's commands, the plan for
//! Reorganize Existing Folders, the moves themselves with a journal, recovery from a move the
//! launcher was closed in the middle of, and Undo. The layout and path rules are folders.rs.
//!
//! Safety rules, all enforced here:
//! - Nothing moves until the player confirms the plan, and only the games they left ticked.
//! - A game is never moved onto a folder that has files in it, into another game's folder, or
//!   into its own; a running, updating or local-build game is never moved.
//! - A game in a folder outside the libraries (one the player brought in from elsewhere) is listed
//!   but unticked: it moves only if the player ticks it.
//! - A move is a rename on the same drive; across drives the copy is finished and checked before
//!   the old folder is removed (manage::move_dir). Each move is written to the journal first, so
//!   one the launcher was closed in the middle of is finished or undone at the next start.
//! - The game's saves, settings and anything else inside its folder go with it; its backups
//!   (.backups beside it) follow; paths the Library keeps inside the folder are updated.

use crate::folders::{self, Layout, Meta, Source};
use crate::library::{self, Installed};
use crate::{catalog, manage, trail, App, Error};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, State};

type Res<T> = Result<T, Error>;

fn journal_file(dir: &Path) -> PathBuf {
    dir.join("folders-journal.json")
}

/// One game's move, as the journal keeps it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Moved {
    pub id: String,
    pub from: PathBuf,
    pub to: PathBuf,
    /// "moving" (started, not finished), "done", "failed", "undone".
    pub state: String,
    /// The game's size before the move: a copy across drives is whole when it's this big.
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Journal {
    pub when: u64,
    pub moves: Vec<Moved>,
}

fn read_journal(dir: &Path) -> Journal {
    std::fs::read(journal_file(dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn write_journal(dir: &Path, j: &Journal) {
    let Ok(body) = serde_json::to_vec_pretty(j) else { return };
    let tmp = journal_file(dir).with_extension("json.tmp");
    if std::fs::write(&tmp, body).is_ok() {
        let _ = std::fs::rename(tmp, journal_file(dir));
    }
}

/// The library a game's folder is in (the innermost, if libraries nest), or None for a folder
/// outside all of them.
fn library_of(path: &Path, libs: &[PathBuf]) -> Option<PathBuf> {
    libs.iter().filter(|l| path != l.as_path() && library::inside(path, l)).max_by_key(|l| l.components().count()).cloned()
}

/// Every path the Library keeps inside a game's folder, moved along with it.
fn rebase_paths(g: &mut Installed, from: &Path, to: &Path) {
    let fix = |p: &mut PathBuf| {
        if let Ok(rest) = p.strip_prefix(from) {
            *p = to.join(rest);
        }
    };
    g.path = to.to_path_buf();
    for p in [g.exe.as_mut(), g.game_copy.as_mut(), g.update_copy.as_mut(), g.build_dir.as_mut(), g.setup.as_mut()].into_iter().flatten() {
        fix(p);
    }
    for p in g.setup_inputs.values_mut() {
        fix(p);
    }
    if let Some(m) = g.manual.as_mut() {
        for p in [m.exe.as_mut(), m.copy.as_mut()].into_iter().flatten() {
            fix(p);
        }
    }
    if let Some(c) = g.copy_check.as_mut() {
        fix(&mut c.path);
    }
}

/// Folders left empty by a move or an uninstall, from `start` up to (not including) its library.
/// Only empty ones: remove_dir refuses anything with a file in it.
pub fn prune_empty(start: &Path, libs: &[PathBuf]) {
    let Some(lib) = library_of(start, libs) else { return };
    let mut dir = start.parent().map(Path::to_path_buf);
    while let Some(d) = dir {
        if library::same_path(&d, &lib) || !library::inside(&d, &lib) {
            break;
        }
        if std::fs::remove_dir(&d).is_err() {
            break;
        }
        dir = d.parent().map(Path::to_path_buf);
    }
}

/// A game the launcher put in a folder of the structure, still there: uninstall deletes that
/// folder as it would one it made the usual way.
pub fn placed_by_us(dir: &Path, id: &str, path: &Path, libs: &[PathBuf]) -> bool {
    let layout = folders::load(dir);
    let placed = layout.placed.get(id).is_some_and(|p| library::same_path(p, path));
    placed && library_of(path, libs).is_some()
}

/// After an uninstall: the game is no longer placed, and the structure's folders it left empty go.
pub fn forget(dir: &Path, id: &str, path: &Path, libs: &[PathBuf]) {
    let mut layout = folders::load(dir);
    if layout.placed.remove(id).is_some() {
        let _ = folders::save(dir, &layout);
        prune_empty(path, libs);
    }
}

fn source_for<'a>(g: &'a Installed, cat: &'a Value) -> Source<'a> {
    let version = if g.version.is_empty() { g.tag.as_str() } else { g.version.as_str() };
    Source { id: &g.id, name: &g.name, game: &g.game, repo: &g.repo, version, local: g.local, copy: g.game_copy.as_deref(), entry: catalog::app(cat, &g.id), cat }
}

/// A folder that has something in it.
fn has_files(p: &Path) -> bool {
    std::fs::read_dir(p).map(|mut r| r.next().is_some()).unwrap_or(p.exists())
}

/// Where a new install goes with "Automatically use my folder structure" on: a folder of the
/// structure in `lib_dir`, never one another game or the player's files already use. None when
/// the setting is off (the launcher's usual folder is used).
pub fn new_target(st: &App, lib_dir: &Path, id: &str, entry: &Value, cat: &Value) -> Option<PathBuf> {
    let layout = folders::load(&st.dir);
    if !layout.apply_new {
        return None;
    }
    let others: Vec<PathBuf> = st.lib.lock().unwrap().installed.values().filter(|g| g.id != id).map(|g| g.path.clone()).collect();
    let name = entry.get("name").and_then(Value::as_str).unwrap_or(id);
    let game = entry.get("game").and_then(Value::as_str).unwrap_or("");
    let repo = entry.get("repo").and_then(Value::as_str).unwrap_or("");
    let version = entry.get("version").and_then(Value::as_str).unwrap_or("");
    let src = Source { id, name, game, repo, version, local: false, copy: None, entry: Some(entry), cat };
    let meta = folders::resolve(&src, &layout);
    let rel = folders::relative(&meta, &layout);
    let taken = |p: &Path| has_files(p) || others.iter().any(|o| library::same_path(o, p) || library::inside(p, o) || library::inside(o, p));
    Some(folders::unique(lib_dir, &rel, &meta, &taken))
}

/// Remember that a new install went into a folder of the structure.
pub fn note_placed(dir: &Path, id: &str, path: &Path) {
    let mut layout = folders::load(dir);
    layout.placed.insert(id.to_string(), path.to_path_buf());
    let _ = folders::save(dir, &layout);
}

// ---------- the designer ----------

#[tauri::command(async)]
pub fn folders_get(st: State<'_, App>) -> Value {
    let presets: Vec<Value> = folders::PRESETS.iter().map(|(id, name, _)| json!({ "id": id, "name": name })).collect();
    let consoles: Vec<Value> = folders::consoles().into_iter().map(|(id, maker, full, short)| json!({ "id": id, "maker": maker, "full": full, "short": short })).collect();
    let fallbacks: serde_json::Map<String, Value> = folders::FIELDS.iter().map(|f| (f.to_string(), serde_json::to_value(folders::default_fallback(f)).unwrap_or(Value::Null))).collect();
    let undo = read_journal(&st.dir).moves.iter().any(|m| m.state == "done");
    json!({ "layout": folders::load(&st.dir), "presets": presets, "consoles": consoles, "fields": folders::FIELDS, "fallbacks": fallbacks, "undo": undo })
}

#[tauri::command(async)]
pub fn folders_preset(id: String) -> Res<Layout> {
    folders::preset(&id).ok_or_else(|| Error::Msg("That isn't one of the ready-made structures.".into()))
}

/// Saves the structure. Changing it never moves a game: Reorganize Existing Folders does that.
#[tauri::command(async)]
pub fn folders_save(st: State<'_, App>, layout: Layout) -> Res<Layout> {
    if layout.levels.is_empty() {
        return Err(Error::Msg("A structure needs at least one folder level.".into()));
    }
    let bad = layout.levels.iter().flat_map(|l| &l.parts).find(|p| !folders::FIELDS.contains(&p.field.as_str()));
    if let Some(p) = bad {
        return Err(Error::Msg(format!("{} isn't a folder field.", p.field)));
    }
    // What the launcher placed is its own record, never the page's.
    let mut out = layout;
    out.placed = folders::load(&st.dir).placed;
    folders::save(&st.dir, &out)?;
    Ok(out)
}

#[derive(Serialize)]
pub struct PreviewRow {
    pub id: String,
    pub name: String,
    /// The library it's in (empty for an example).
    pub library: String,
    pub folders: Vec<String>,
    pub example: bool,
}

/// The catalog as the launcher already has it. Asking the website could keep Reorganize and the
/// designer waiting for a minute on a slow connection, so the network is used only when there's
/// no copy on disk yet.
async fn catalog_data(st: &App) -> Value {
    if let Some(cat) = catalog::catalog_on_disk(&st.dir) {
        return cat;
    }
    catalog::catalog(&st.http, &st.dir, false).await.map(|f| f.data).unwrap_or_else(|_| json!({}))
}

/// The tree the structure would make: the Library's games (each in its own library), and made-up
/// examples while the Library has fewer than three.
#[tauri::command]
pub async fn folders_preview(st: State<'_, App>, layout: Layout) -> Res<Vec<PreviewRow>> {
    let cat = catalog_data(&st).await;
    let (games, libs) = {
        let l = st.lib.lock().unwrap();
        (l.installed.values().filter(|g| !g.local).cloned().collect::<Vec<_>>(), l.settings.all_libraries())
    };
    let mut rows: Vec<PreviewRow> = games.iter().map(|g| {
        let meta = folders::resolve(&source_for(g, &cat), &layout);
        let lib = library_of(&g.path, &libs).unwrap_or_else(|| libs[0].clone());
        PreviewRow { id: g.id.clone(), name: g.name.clone(), library: lib.to_string_lossy().to_string(), folders: folders::relative(&meta, &layout), example: false }
    }).collect();
    if rows.len() < 3 {
        for m in folders::examples() {
            let meta = with_layout(m, &layout);
            rows.push(PreviewRow { id: meta.id.clone(), name: meta.values.get("recomp").cloned().unwrap_or_default(), library: String::new(), folders: folders::relative(&meta, &layout), example: true });
        }
    }
    rows.sort_by(|a, b| a.folders.cmp(&b.folders));
    Ok(rows)
}

/// What the catalog and the Library already know about each installed game, before the player's
/// own values: the designer fills the per-game boxes with it, so only the gaps need typing.
#[tauri::command]
pub async fn folders_known(st: State<'_, App>, layout: Layout) -> Res<BTreeMap<String, BTreeMap<String, String>>> {
    let cat = catalog_data(&st).await;
    let games: Vec<Installed> = st.lib.lock().unwrap().installed.values().filter(|g| !g.local).cloned().collect();
    let mut bare = layout;
    bare.games.clear();
    Ok(games.iter().map(|g| (g.id.clone(), folders::resolve(&source_for(g, &cat), &bare).values)).collect())
}

/// An example game seen through the layout's own choices (its console names, its per-game values).
fn with_layout(mut m: Meta, layout: &Layout) -> Meta {
    if let Some(console) = m.values.get("console").cloned() {
        if let Some(c) = folders::console_id(&console).and_then(|id| folders::consoles().into_iter().find(|c| c.0 == id)) {
            let name = layout.consoles.get(c.0).cloned().unwrap_or_else(|| (if layout.short_consoles { c.3 } else { c.2 }).to_string());
            m.values.insert("console".into(), name);
        }
    }
    m
}

// ---------- Reorganize Existing Folders ----------

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PlanRow {
    pub id: String,
    pub name: String,
    pub from: PathBuf,
    pub to: PathBuf,
    /// "move", "same" (already there), "outside" (a folder outside the libraries: moves only if
    /// ticked) or "blocked" (can't move; `note` says why).
    pub state: String,
    pub note: String,
    /// Ticked to begin with.
    pub on: bool,
}

/// What Reorganize Existing Folders would do with each game, with the saved structure.
fn plan(st: &App, cat: &Value, layout: &Layout) -> Vec<PlanRow> {
    let (games, libs, default_lib) = {
        let l = st.lib.lock().unwrap();
        (l.installed.values().cloned().collect::<Vec<_>>(), l.settings.all_libraries(), l.settings.library_dir.clone())
    };
    let running = st.running.lock().unwrap().keys().cloned().collect::<Vec<_>>();
    let busy = st.jobs.lock().unwrap().keys().cloned().chain(st.moving.lock().unwrap().iter().cloned()).collect::<Vec<_>>();
    plan_rows(games, &libs, &default_lib, &running, &busy, cat, layout)
}

/// The plan, from the Library as it is: games in id order, so the same Library always gives the
/// same plan (and the same game gets the plain name when two would share one).
fn plan_rows(mut games: Vec<Installed>, libs: &[PathBuf], default_lib: &Path, running: &[String], busy: &[String], cat: &Value, layout: &Layout) -> Vec<PlanRow> {
    games.sort_by(|a, b| a.id.cmp(&b.id));
    let libs = libs.to_vec();
    let default_lib = default_lib.to_path_buf();
    let mut planned: Vec<PathBuf> = vec![];
    let mut rows = vec![];
    for g in &games {
        let blocked = |note: &str| PlanRow { id: g.id.clone(), name: g.name.clone(), from: g.path.clone(), to: g.path.clone(), state: "blocked".into(), note: note.into(), on: false };
        if g.local {
            rows.push(blocked("A local build stays in its own folder."));
            continue;
        }
        if running.contains(&g.id) {
            rows.push(blocked("It's running. Close it to move it."));
            continue;
        }
        if busy.contains(&g.id) {
            rows.push(blocked("It's being installed, updated or moved."));
            continue;
        }
        if !g.path.is_dir() {
            rows.push(blocked("Its folder isn't there (a drive that isn't connected?)."));
            continue;
        }
        if library::shared_folder(&g.path, &libs) {
            rows.push(blocked("Its folder holds more than the game, so it stays where it is."));
            continue;
        }
        let lib = library_of(&g.path, &libs);
        let base = lib.clone().unwrap_or_else(|| default_lib.clone());
        let meta = folders::resolve(&source_for(g, cat), layout);
        let rel = folders::relative(&meta, layout);
        let others: Vec<&Installed> = games.iter().filter(|o| o.id != g.id).collect();
        let me = g.path.clone();
        let taken = |p: &Path| {
            if library::same_path(p, &me) {
                return false;
            }
            let other_there = others.iter().any(|o| library::same_path(&o.path, p) || library::inside(p, &o.path) || library::inside(&o.path, p));
            other_there || planned.iter().any(|q| library::same_path(q, p)) || has_files(p)
        };
        let to = folders::unique(&base, &rel, &meta, &taken);
        if library::same_path(&to, &g.path) {
            rows.push(PlanRow { id: g.id.clone(), name: g.name.clone(), from: g.path.clone(), to, state: "same".into(), note: String::new(), on: false });
            continue;
        }
        if library::inside(&to, &g.path) {
            rows.push(blocked("Its new folder would be inside its current one."));
            continue;
        }
        planned.push(to.clone());
        let (state, note, on) = match lib {
            Some(_) => ("move", String::new(), true),
            None => ("outside", format!("Its folder is outside your libraries, so it moves only if you tick it (into {}).", base.display()), false),
        };
        rows.push(PlanRow { id: g.id.clone(), name: g.name.clone(), from: g.path.clone(), to, state: state.into(), note, on });
    }
    rows
}

#[tauri::command]
pub async fn folders_plan(st: State<'_, App>) -> Res<Vec<PlanRow>> {
    let began = std::time::Instant::now();
    recover(&st);
    let cat = catalog_data(&st).await;
    let read = began.elapsed().as_millis();
    let layout = folders::load(&st.dir);
    let rows = plan(&st, &cat, &layout);
    trail::note(format!("Folder Structure: planned {} games in {} ms (catalog {} ms)", rows.len(), began.elapsed().as_millis(), read));
    Ok(rows)
}

#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    pub id: String,
    pub name: String,
    pub ok: bool,
    pub to: PathBuf,
    pub note: String,
    /// The library the game ended up in, so Open Folder can show the new structure.
    pub library: PathBuf,
}

/// The game starts from its new folder: its program is there (when the launcher knows it), or
/// at least the folder has its files.
fn looks_whole(g: &Installed, at: &Path) -> bool {
    match g.exe.as_ref().filter(|e| e.is_relative() && !e.as_os_str().is_empty()) {
        Some(exe) => at.join(exe).is_file(),
        None => has_files(at),
    }
}

/// Move one game to `to` and update the Library. On any trouble the game stays (or is put back)
/// where it was.
async fn move_one(app: &AppHandle, st: &App, row: &PlanRow, journal: &mut Journal) -> Outcome {
    let libs = st.lib.lock().unwrap().settings.all_libraries();
    let out = |ok: bool, to: &Path, note: String| {
        let library = library_of(to, &libs).unwrap_or_else(|| to.parent().unwrap_or(to).to_path_buf());
        Outcome { id: row.id.clone(), name: row.name.clone(), ok, to: to.to_path_buf(), note, library }
    };
    let Some(g) = st.lib.lock().unwrap().installed.get(&row.id).cloned() else { return out(false, &row.from, "It isn't in the Library anymore.".into()) };
    if !library::same_path(&g.path, &row.from) {
        return out(false, &g.path, "It moved since the list was made. Look again.".into());
    }
    if st.running.lock().unwrap().contains_key(&row.id) || st.jobs.lock().unwrap().contains_key(&row.id) {
        return out(false, &g.path, "It's running or busy. Try again once it's done.".into());
    }
    if has_files(&row.to) {
        return out(false, &g.path, format!("{} already has files in it, so nothing was moved.", row.to.display()));
    }
    if !st.moving.lock().unwrap().insert(row.id.clone()) {
        return out(false, &g.path, "It's already being moved.".into());
    }
    crate::looks::tidy(&st.dir, &row.id);
    let size = manage::dir_size(&g.path);
    journal.moves.retain(|m| m.id != row.id);
    journal.moves.push(Moved { id: row.id.clone(), from: g.path.clone(), to: row.to.clone(), state: "moving".into(), size, note: String::new() });
    write_journal(&st.dir, journal);
    let (from, to, app2, id2) = (g.path.clone(), row.to.clone(), app.clone(), row.id.clone());
    let moved = tokio::task::spawn_blocking(move || {
        // A few updates a second are plenty for the percentage (a copy reports every megabyte).
        let last = std::cell::Cell::new(std::time::Instant::now());
        manage::move_dir(&from, &to, &|done, total| {
            if last.get().elapsed().as_millis() > 150 || done == total {
                last.set(std::time::Instant::now());
                let _ = app2.emit("folders-move", json!({ "id": id2, "done": done, "total": total }));
            }
        })
    })
    .await
    .map_err(|e| Error::Msg(e.to_string()))
    .and_then(|r| r);
    let state = |j: &mut Journal, s: &str, note: &str| {
        if let Some(m) = j.moves.iter_mut().find(|m| m.id == row.id) {
            m.state = s.into();
            m.note = note.into();
        }
    };
    if let Err(e) = moved {
        st.moving.lock().unwrap().remove(&row.id);
        state(journal, "failed", &e.to_string());
        write_journal(&st.dir, journal);
        return out(false, &g.path, format!("Not moved: {e}"));
    }
    if !looks_whole(&g, &row.to) {
        // Put back where it was: the game must still start.
        let back = manage::move_dir(&row.to, &g.path, &|_, _| {});
        st.moving.lock().unwrap().remove(&row.id);
        let note = match back {
            Ok(()) => "Its program wasn't there after the move, so it was put back.".to_string(),
            Err(e) => format!("Its program wasn't there after the move, and putting it back failed ({e}). Its files are in {}.", row.to.display()),
        };
        state(journal, "failed", &note);
        write_journal(&st.dir, journal);
        return out(false, &g.path, note);
    }
    // Its backups go with it (they sit beside the game's folder).
    let old_b = crate::backups_dir(&g);
    let mut next = g.clone();
    rebase_paths(&mut next, &g.path, &row.to);
    if old_b.is_dir() {
        let _ = manage::move_dir(&old_b, &crate::backups_dir(&next), &|_, _| {});
    }
    st.lib.lock().unwrap().installed.insert(row.id.clone(), next);
    st.save();
    note_placed(&st.dir, &row.id, &row.to);
    prune_empty(&g.path, &libs);
    prune_empty(&old_b, &libs);
    st.moving.lock().unwrap().remove(&row.id);
    state(journal, "done", "");
    write_journal(&st.dir, journal);
    trail::note(format!("Folder Structure: {} moved from {} to {}", row.id, g.path.display(), row.to.display()));
    out(true, &row.to, String::new())
}

/// Moves the ticked games (`ids`) to their folders in the saved structure, one at a time.
#[tauri::command]
pub async fn folders_apply(app: AppHandle, st: State<'_, App>, ids: Vec<String>) -> Res<Vec<Outcome>> {
    recover(&st);
    let cat = catalog_data(&st).await;
    let layout = folders::load(&st.dir);
    let rows = plan(&st, &cat, &layout);
    let mut journal = Journal { when: library::now(), moves: vec![] };
    let mut done = vec![];
    for id in &ids {
        let Some(row) = rows.iter().find(|r| &r.id == id) else { continue };
        if row.state != "move" && row.state != "outside" {
            continue;
        }
        let _ = app.emit("folders-move", json!({ "id": row.id, "done": 0, "total": 0 }));
        done.push(move_one(&app, &st, row, &mut journal).await);
        // Every game counts toward the percentage, moved or not.
        let _ = app.emit("folders-move", json!({ "id": row.id, "finished": true }));
    }
    Ok(done)
}

/// Puts the games the last reorganize moved back where they were (newest first), each only if its
/// old folder is still free.
#[tauri::command]
pub async fn folders_undo(app: AppHandle, st: State<'_, App>) -> Res<Vec<Outcome>> {
    recover(&st);
    let mut journal = read_journal(&st.dir);
    let todo: Vec<Moved> = journal.moves.iter().rev().filter(|m| m.state == "done").cloned().collect();
    let mut done = vec![];
    for m in todo {
        let name = st.lib.lock().unwrap().installed.get(&m.id).map(|g| g.name.clone()).unwrap_or_else(|| m.id.clone());
        let row = PlanRow { id: m.id.clone(), name, from: m.to.clone(), to: m.from.clone(), state: "move".into(), note: String::new(), on: true };
        let mut scratch = Journal::default();
        let r = move_one(&app, &st, &row, &mut scratch).await;
        if r.ok {
            if let Some(x) = journal.moves.iter_mut().find(|x| x.id == m.id) {
                x.state = "undone".into();
            }
            // Back in its usual folder: not one of the structure's anymore.
            let mut layout = folders::load(&st.dir);
            layout.placed.remove(&m.id);
            let _ = folders::save(&st.dir, &layout);
        }
        done.push(r);
    }
    write_journal(&st.dir, &journal);
    Ok(done)
}

/// A move the launcher was closed in the middle of, settled: finished when the game's folder is
/// whole at its new place, else undone (only the half copy the move made is removed; the game's
/// own folder is never deleted here).
pub fn recover(st: &App) {
    let mut journal = read_journal(&st.dir);
    let mut changed = false;
    for m in journal.moves.iter_mut().filter(|m| m.state == "moving") {
        changed = true;
        let Some(g) = st.lib.lock().unwrap().installed.get(&m.id).cloned() else {
            m.state = "failed".into();
            continue;
        };
        if let Some(next) = settle(m, &g) {
            st.lib.lock().unwrap().installed.insert(m.id.clone(), next);
            st.save();
            note_placed(&st.dir, &m.id, &m.to);
        }
        trail::note(format!("Folder Structure: {} {}", m.id, m.note));
    }
    if changed {
        write_journal(&st.dir, &journal);
    }
}

/// One interrupted move, settled on disk. Some(the game's record at its new folder) when the move
/// had got far enough to finish; None when the game stays where it was.
fn settle(m: &mut Moved, g: &Installed) -> Option<Installed> {
    let at_new = m.to.is_dir() && looks_whole(g, &m.to);
    let whole = at_new && manage::dir_size(&m.to) >= m.size;
    if whole {
        let mut next = g.clone();
        rebase_paths(&mut next, &m.from, &m.to);
        m.state = "done".into();
        m.note = if m.from.exists() { format!("finished after a restart; the old folder {} is left for you to delete.", m.from.display()) } else { "finished after a restart.".into() };
        return Some(next);
    }
    // The game's own folder is still whole where it was: only the partial copy goes.
    if m.from.is_dir() && m.to.is_dir() {
        let _ = std::fs::remove_dir_all(&m.to);
    }
    m.state = "failed".into();
    m.note = "was stopped by a restart; the game stayed where it was.".into();
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_inside_the_game_folder_follow_it() {
        let mut g = Installed { id: "g".into(), path: "/lib/g".into(), exe: Some("bin/game.exe".into()), game_copy: Some("/lib/g/rom.z64".into()), update_copy: Some("/elsewhere/u.bin".into()), ..Default::default() };
        rebase_paths(&mut g, Path::new("/lib/g"), Path::new("/lib/N64/Game"));
        assert_eq!(g.path, PathBuf::from("/lib/N64/Game"));
        assert_eq!(g.exe, Some(PathBuf::from("bin/game.exe")));
        assert_eq!(g.game_copy, Some(PathBuf::from("/lib/N64/Game/rom.z64")));
        assert_eq!(g.update_copy, Some(PathBuf::from("/elsewhere/u.bin")));
    }

    #[test]
    fn a_game_belongs_to_the_innermost_library_holding_it() {
        let libs = vec![PathBuf::from("/games"), PathBuf::from("/games/more")];
        assert_eq!(library_of(Path::new("/games/more/N64/x"), &libs), Some(PathBuf::from("/games/more")));
        assert_eq!(library_of(Path::new("/games/x"), &libs), Some(PathBuf::from("/games")));
        assert_eq!(library_of(Path::new("/other/x"), &libs), None);
        assert_eq!(library_of(Path::new("/games"), &libs), None);
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rl-fm-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn game_at(dir: &Path) -> Installed {
        std::fs::create_dir_all(dir.join("saves")).unwrap();
        std::fs::write(dir.join("game.exe"), b"exe").unwrap();
        std::fs::write(dir.join("saves/slot1.sav"), b"save").unwrap();
        Installed { id: "soh".into(), name: "Ship of Harkinian".into(), path: dir.to_path_buf(), exe: Some("game.exe".into()), ..Default::default() }
    }

    fn moving(g: &Installed, to: &Path) -> Moved {
        Moved { id: g.id.clone(), from: g.path.clone(), to: to.to_path_buf(), state: "moving".into(), size: manage::dir_size(&g.path), note: String::new() }
    }

    #[test]
    fn a_rename_that_finished_before_a_restart_is_kept() {
        let d = tmp("renamed");
        let g = game_at(&d.join("soh"));
        let mut m = moving(&g, &d.join("N64/Ocarina of Time/Ship of Harkinian"));
        std::fs::create_dir_all(m.to.parent().unwrap()).unwrap();
        std::fs::rename(&g.path, &m.to).unwrap();
        let next = settle(&mut m, &g).unwrap();
        assert_eq!(m.state, "done");
        assert_eq!(next.path, m.to);
        assert!(m.to.join("saves/slot1.sav").is_file());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_half_copy_across_drives_is_removed_and_the_game_stays() {
        let d = tmp("half");
        let g = game_at(&d.join("soh"));
        let mut m = moving(&g, &d.join("other-drive/Ship of Harkinian"));
        // The copy got the program but not the saves before the launcher closed.
        std::fs::create_dir_all(&m.to).unwrap();
        std::fs::write(m.to.join("game.exe"), b"exe").unwrap();
        assert!(settle(&mut m, &g).is_none());
        assert_eq!(m.state, "failed");
        assert!(!m.to.exists());
        assert!(g.path.join("saves/slot1.sav").is_file());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_whole_copy_whose_old_folder_wasnt_removed_yet_is_kept_and_the_old_one_left() {
        let d = tmp("copied");
        let g = game_at(&d.join("soh"));
        let mut m = moving(&g, &d.join("N64/Ship of Harkinian"));
        manage::move_dir(&g.path, &m.to, &|_, _| {}).unwrap();
        game_at(&g.path);
        let next = settle(&mut m, &g).unwrap();
        assert_eq!(next.path, m.to);
        assert!(g.path.is_dir(), "the old folder is the player's to delete");
        assert!(m.note.contains("left for you"));
        let _ = std::fs::remove_dir_all(&d);
    }

    fn installed(id: &str, name: &str, path: &Path) -> Installed {
        std::fs::create_dir_all(path).unwrap();
        std::fs::write(path.join("game.exe"), b"exe").unwrap();
        Installed { id: id.into(), name: name.into(), path: path.to_path_buf(), exe: Some("game.exe".into()), ..Default::default() }
    }

    fn catalog() -> Value {
        json!({ "origins": [{ "id": "n64", "name": "Nintendo 64" }], "apps": [
            { "id": "soh", "name": "Ship of Harkinian", "game": "Ocarina of Time", "origin": "n64" },
            { "id": "oot-recomp", "name": "OoT Recomp", "game": "Ocarina of Time", "origin": "n64" },
            { "id": "mm", "name": "Zelda64Recomp", "game": "Majora's Mask", "origin": "n64" }
        ] })
    }

    #[test]
    fn the_plan_moves_library_games_and_asks_about_the_rest() {
        let d = tmp("plan");
        let lib = d.join("lib");
        let games = vec![
            installed("soh", "Ship of Harkinian", &lib.join("soh")),
            installed("oot-recomp", "OoT Recomp", &lib.join("oot-recomp")),
            installed("mm", "Zelda64Recomp", &d.join("elsewhere/mm")),
            Installed { id: "mine".into(), name: "Mine".into(), path: d.join("src/mine"), local: true, ..Default::default() },
        ];
        let layout = folders::preset("year-console-game").unwrap();
        let rows = plan_rows(games, std::slice::from_ref(&lib), &lib, &[], &[], &catalog(), &layout);
        let row = |id: &str| rows.iter().find(|r| r.id == id).unwrap().clone();
        // Two recomps of one game: each gets its own folder, the second with its name added.
        assert_eq!(row("oot-recomp").to, lib.join("Unknown Year/N64/Ocarina of Time"));
        assert_eq!(row("soh").to, lib.join("Unknown Year/N64/Ocarina of Time - Ship of Harkinian"));
        assert!(row("soh").on && row("soh").state == "move");
        // A folder outside the libraries is listed, unticked.
        assert_eq!(row("mm").state, "outside");
        assert!(!row("mm").on);
        assert_eq!(row("mine").state, "blocked");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_plan_never_points_at_a_folder_with_files_or_at_a_running_game() {
        let d = tmp("taken");
        let lib = d.join("lib");
        let games = vec![installed("soh", "Ship of Harkinian", &lib.join("soh")), installed("mm", "Zelda64Recomp", &lib.join("mm"))];
        // The player's own files sit where the structure would put Ship of Harkinian.
        std::fs::create_dir_all(lib.join("Ocarina of Time")).unwrap();
        std::fs::write(lib.join("Ocarina of Time/notes.txt"), b"mine").unwrap();
        let layout = folders::preset("game-recomp").unwrap();
        let mut layout = layout;
        layout.levels.pop();
        let rows = plan_rows(games, std::slice::from_ref(&lib), &lib, &["mm".to_string()], &[], &catalog(), &layout);
        let soh = rows.iter().find(|r| r.id == "soh").unwrap();
        assert_eq!(soh.to, lib.join("Ocarina of Time - Ship of Harkinian"));
        assert_eq!(rows.iter().find(|r| r.id == "mm").unwrap().state, "blocked");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn only_empty_folders_up_to_the_library_are_pruned() {
        let root = std::env::temp_dir().join(format!("rl-prune-{}", std::process::id()));
        let lib = root.join("lib");
        std::fs::create_dir_all(lib.join("Nintendo/N64/Game")).unwrap();
        std::fs::create_dir_all(lib.join("Nintendo/SNES/Other")).unwrap();
        std::fs::remove_dir(lib.join("Nintendo/N64/Game")).unwrap();
        prune_empty(&lib.join("Nintendo/N64/Game"), std::slice::from_ref(&lib));
        assert!(!lib.join("Nintendo/N64").exists());
        assert!(lib.join("Nintendo/SNES/Other").exists());
        assert!(lib.exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}

//! Settings → Keep Steam in Sync: every installed game that's ready to play is a non-Steam game in
//! Steam, with its art, in a RecompLauncher collection, and leaves Steam when it's uninstalled.
//!
//! Steam reads its list of non-Steam games (shortcuts.vdf) and its collections only when it
//! starts, and writes them back when it closes. So changes are written only while Steam is closed:
//! a check every couple of minutes catches it closed, and Sync Now closes and reopens it.
//!
//! Only entries the launcher made are ever taken out: they start the launcher with `--play <id>`,
//! or one of its Linux launch scripts. Games the player added to Steam themselves stay.

use crate::shortcuts::{self, Node};
use crate::{games, library::Installed, trail, App, Error, Res};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Manager, State};

/// The tag on the launcher's entries in shortcuts.vdf (shortcuts::add_shortcut writes it).
const TAG: &str = "RecompLauncher";
/// The collection's id in Steam. Fixed, so the launcher finds its own collection again on every
/// PC the player signs in on (collections go with their Steam account).
const COLLECTION_ID: &str = "rl-recomplauncher";
const COLLECTION_NAME: &str = "RecompLauncher";
/// How often the launcher looks for Steam being closed, to write what changed.
const CHECK_EVERY: std::time::Duration = std::time::Duration::from_secs(120);

/// A game that should be in Steam, as the launcher writes it.
#[derive(Debug, Clone, PartialEq)]
pub struct Want {
    pub id: String,
    pub name: String,
    /// The program Steam starts, without quotes.
    pub exe: String,
    pub launch: String,
}

impl Want {
    pub fn app_id(&self) -> u32 {
        shortcuts::steam_app_id(&format!("\"{}\"", self.exe), &self.name)
    }
}

/// One of the launcher's entries already in a shortcuts.vdf.
#[derive(Debug, Clone, PartialEq)]
pub struct Owned {
    /// Its key in the file ("0", "1", ...).
    pub key: String,
    /// The game it plays.
    pub id: String,
    pub app_id: u32,
    pub name: String,
    pub exe: String,
    pub launch: String,
}

/// What has to change in one account's shortcuts.vdf.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    pub add: Vec<Want>,
    pub remove: Vec<Owned>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.add.is_empty() && self.remove.is_empty()
    }
}

/// The game a `--play <id>` launch option plays.
pub fn played_id(launch: &str) -> Option<String> {
    let (_, rest) = launch.rsplit_once("--play ")?;
    let id = rest.trim().trim_matches(['"', '\'']);
    if id.is_empty() || id.contains(char::is_whitespace) {
        return None;
    }
    Some(id.to_string())
}

/// Made by the launcher: it carries the launcher's tag, or starts the launcher itself.
fn made_by_launcher(e: &Node, exe: &str, launch: &str) -> bool {
    let tagged = match e.get("tags") {
        Some(Node::Map(tags)) => tags.iter().any(|(_, t)| matches!(t, Node::Str(s) if s == TAG)),
        _ => false,
    };
    let starts_launcher = exe.to_lowercase().contains("recomplauncher") || launch.to_lowercase().contains("recomplauncher");
    tagged || starts_launcher
}

/// The launcher's game entries in a shortcuts.vdf's list. `script_game` reads which game one of
/// its Linux launch scripts plays. The launcher's own entry (Big Picture mode) isn't one.
pub fn owned(list: &[(String, Node)], script_game: &dyn Fn(&str) -> Option<String>) -> Vec<Owned> {
    let mut out = vec![];
    for (key, e) in list {
        let exe = e.str("Exe").trim_matches('"').to_string();
        let launch = e.str("LaunchOptions").to_string();
        let id = match played_id(&launch) {
            Some(id) if made_by_launcher(e, &exe, &launch) => Some(id),
            Some(_) => None,
            None => script_game(&exe),
        };
        let Some(id) = id else { continue };
        let app_id = match e.get("appid") {
            Some(Node::Int(n)) => *n as u32,
            _ => shortcuts::steam_app_id(e.str("Exe"), e.str("AppName")),
        };
        out.push(Owned { key: key.clone(), id, app_id, name: e.str("AppName").to_string(), exe, launch });
    }
    out
}

/// What changes for Steam to have exactly `wants` of the launcher's games. An entry for a game
/// that's installed but not ready to play yet (`keep`) stays: the player may have added it with
/// Add to Steam on purpose. An entry that plays a wanted game the old way (another name, the
/// launcher somewhere else) is replaced.
pub fn plan(owned: &[Owned], wants: &[Want], keep: &BTreeSet<String>) -> Plan {
    let mut matched = BTreeSet::new();
    let mut add = vec![];
    for w in wants {
        let found = owned.iter().find(|o| !matched.contains(&o.key) && o.exe == w.exe && o.launch == w.launch && o.name == w.name);
        match found {
            Some(o) => {
                matched.insert(o.key.clone());
            }
            None => add.push(w.clone()),
        }
    }
    let remove = owned.iter().filter(|o| !matched.contains(&o.key) && !keep.contains(&o.id)).cloned().collect();
    Plan { add, remove }
}

/// A shortcuts.vdf's content without the entries under `keys`. None when it can't be read.
pub fn without(old: &[u8], keys: &BTreeSet<String>) -> Option<Vec<u8>> {
    let mut top = shortcuts::parse_vdf(old)?;
    for (k, v) in top.iter_mut() {
        if !k.eq_ignore_ascii_case("shortcuts") {
            continue;
        }
        let Node::Map(list) = v else { continue };
        list.retain(|(key, _)| !keys.contains(key));
        // Steam numbers its entries from 0, without gaps.
        for (n, (key, _)) in list.iter_mut().enumerate() {
            *key = n.to_string();
        }
    }
    Some(shortcuts::write_vdf(&top))
}

/// The list of entries in a shortcuts.vdf's content (empty for a new file).
fn entries(bytes: Option<&[u8]>) -> Option<Vec<(String, Node)>> {
    let Some(b) = bytes.filter(|b| !b.is_empty()) else { return Some(vec![]) };
    let top = shortcuts::parse_vdf(b)?;
    let list = top.into_iter().find(|(k, _)| k.eq_ignore_ascii_case("shortcuts")).map(|(_, v)| v);
    match list {
        Some(Node::Map(l)) => Some(l),
        _ => Some(vec![]),
    }
}

// ---------- Steam's collections ----------

/// Steam's collections file (cloudstorage/cloud-storage-namespace-1.json, a list of [key, record])
/// with the RecompLauncher collection holding `add` and no longer `remove`. Games the player put
/// in the collection themselves stay. Steam's other collections and settings stay as they are.
/// The changed key comes back too, for the file of changes Steam still has to upload. None when
/// nothing changes, or the file isn't the list Steam writes.
pub fn with_collection(file: &str, add: &[u32], remove: &[u32], now: u64) -> Option<(String, String)> {
    let mut list: Vec<Value> = serde_json::from_str(file).ok()?;
    // Records are versioned by Steam Cloud; a new one goes in at the newest version.
    let version = list.iter().filter_map(|x| x.get(1)?.get("version")?.as_str()?.parse::<u64>().ok()).max().unwrap_or(0);
    let pos = collection_at(&list);
    let key = match pos {
        Some(i) => list[i][0].as_str()?.to_string(),
        None => format!("user-collections.{COLLECTION_ID}"),
    };
    let old: Value = pos.and_then(|i| list[i][1].get("value")?.as_str().and_then(|v| serde_json::from_str(v).ok())).unwrap_or(Value::Null);
    let id = old.get("id").and_then(Value::as_str).unwrap_or(COLLECTION_ID).to_string();
    let had: Vec<u32> = old.get("added").and_then(Value::as_array).map(|a| a.iter().filter_map(|n| n.as_u64()).map(|n| n as u32).collect()).unwrap_or_default();
    let mut apps: Vec<u32> = had.iter().copied().filter(|a| !remove.contains(a)).collect();
    for a in add {
        if !apps.contains(a) {
            apps.push(*a);
        }
    }
    let was_removed: Vec<u64> = old.get("removed").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
    let removed: Vec<u64> = was_removed.iter().copied().filter(|r| !apps.contains(&(*r as u32))).collect();
    if pos.is_some() && apps == had && removed == was_removed {
        return None;
    }
    let value = json!({ "id": id, "name": COLLECTION_NAME, "added": apps, "removed": removed }).to_string();
    let record = json!([key, { "key": key, "timestamp": now, "value": value, "version": version.to_string() }]);
    match pos {
        Some(i) => list[i] = record,
        None => list.push(record),
    }
    Some((serde_json::to_string(&list).ok()?, key))
}

/// Where the launcher's collection is in the list: its own id, or one the player named
/// RecompLauncher. A collection deleted in Steam counts as gone (a new one is made).
fn collection_at(list: &[Value]) -> Option<usize> {
    let own_key = format!("user-collections.{COLLECTION_ID}");
    let live = |x: &Value| x.get(1).map(|r| !r.get("is_deleted").and_then(Value::as_bool).unwrap_or(false)).unwrap_or(false);
    let named = |x: &Value| {
        let key = x.get(0).and_then(Value::as_str).unwrap_or("");
        let value = x.get(1).and_then(|r| r.get("value")).and_then(Value::as_str).unwrap_or("");
        key.starts_with("user-collections.") && serde_json::from_str::<Value>(value).ok().and_then(|v| v.get("name").and_then(Value::as_str).map(|n| n == COLLECTION_NAME)).unwrap_or(false)
    };
    let own = list.iter().position(|x| x.get(0).and_then(Value::as_str) == Some(own_key.as_str()));
    if let Some(i) = own.filter(|i| live(&list[*i])) {
        return Some(i);
    }
    if let Some(i) = list.iter().position(|x| live(x) && named(x)) {
        return Some(i);
    }
    // Ours, deleted: written over, so it doesn't sit there twice.
    own
}

/// Steam's list of records changed on this PC and not uploaded yet, with `key` in it: Steam Cloud
/// then takes the new collection instead of putting the old one back.
pub fn with_modified(file: &str, key: &str) -> Option<String> {
    let mut keys: Vec<Value> = if file.trim().is_empty() { vec![] } else { serde_json::from_str(file).ok()? };
    if keys.iter().any(|k| k.as_str() == Some(key)) {
        return None;
    }
    keys.push(Value::String(key.to_string()));
    serde_json::to_string(&keys).ok()
}

/// Write a file through a temporary copy beside it, so Steam never finds half of one.
fn write_whole(file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = file.with_extension("recomplauncher-tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, file)
}

/// Steam's collections file as it was before the launcher first wrote it stays beside it, like the
/// shortcuts file's backup, so the player's collections can always be put back by hand.
fn keep_collections_backup(dir: &Path, old: &str) {
    let backup = dir.join("cloud-storage-namespace-1.json.recomplauncher-backup");
    if !backup.exists() {
        let _ = fs::write(backup, old);
    }
}

/// Put the collection right in one account's cloud storage folder. Ok(false): nothing to change.
fn sync_collection(cfg: &Path, add: &[u32], remove: &[u32]) -> std::io::Result<bool> {
    let dir = cfg.join("cloudstorage");
    let file = dir.join("cloud-storage-namespace-1.json");
    // No file yet (an account that hasn't opened the library on this PC): Steam makes it, and the
    // next sync after that fills the collection.
    let Ok(old) = fs::read_to_string(&file) else { return Ok(false) };
    let Some((new, key)) = with_collection(&old, add, remove, crate::library::now()) else { return Ok(false) };
    let modified = dir.join("cloud-storage-namespace-1.modified.json");
    let old_mod = fs::read_to_string(&modified).unwrap_or_default();
    keep_collections_backup(&dir, &old);
    write_whole(&file, new.as_bytes())?;
    if let Some(m) = with_modified(&old_mod, &key) {
        write_whole(&modified, m.as_bytes())?;
    }
    Ok(true)
}

// ---------- one account ----------

/// One Steam account's shortcuts and what has to change in them.
struct Account {
    cfg: PathBuf,
    old: Option<Vec<u8>>,
    plan: Plan,
    /// The app ids of the launcher's games that stay or come in: what the collection holds.
    apps: Vec<u32>,
}

fn account(user: &Path, data: &Path, wants: &[Want], keep: &BTreeSet<String>) -> Option<Account> {
    let cfg = user.join("config");
    let old = fs::read(cfg.join("shortcuts.vdf")).ok();
    // Something in it the launcher can't read: the player's shortcuts are left alone.
    let list = entries(old.as_deref())?;
    let mine = owned(&list, &|exe| shortcuts::script_game(data, exe));
    let plan = plan(&mine, wants, keep);
    let gone: BTreeSet<u32> = plan.remove.iter().map(|o| o.app_id).collect();
    let mut apps: Vec<u32> = mine.iter().map(|o| o.app_id).filter(|a| !gone.contains(a)).collect();
    apps.extend(plan.add.iter().map(Want::app_id));
    Some(Account { cfg, old, plan, apps })
}

/// The collection doesn't hold the launcher's games yet (or still holds ones that left).
fn collection_behind(a: &Account) -> bool {
    let removed: Vec<u32> = a.plan.remove.iter().map(|o| o.app_id).collect();
    let file = fs::read_to_string(a.cfg.join("cloudstorage").join("cloud-storage-namespace-1.json"));
    file.map(|f| with_collection(&f, &a.apps, &removed, 0).is_some()).unwrap_or(false)
}

/// The art and icon of each game coming in, by id.
type Arts = BTreeMap<String, (Vec<(&'static str, Vec<u8>, &'static str)>, String)>;

fn apply(a: &Account, arts: &Arts) -> Result<(), Error> {
    let file = a.cfg.join("shortcuts.vdf");
    if let Some(o) = &a.old {
        shortcuts::keep_backup(&a.cfg, o);
    }
    let keys: BTreeSet<String> = a.plan.remove.iter().map(|o| o.key.clone()).collect();
    let mut bytes = match &a.old {
        Some(o) if !o.is_empty() => without(o, &keys).ok_or_else(|| Error::Msg("Steam's shortcuts couldn't be read".into()))?,
        _ => vec![],
    };
    for w in &a.plan.add {
        let dir = Path::new(&w.exe).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
        let (art, icon) = arts.get(&w.id).cloned().unwrap_or_default();
        let (next, app_id) = shortcuts::add_shortcut(Some(bytes.as_slice()), &w.name, &w.exe, &dir, &w.launch, &icon);
        bytes = next;
        shortcuts::save_art(&a.cfg, app_id, &art)?;
    }
    fs::create_dir_all(&a.cfg)?;
    write_whole(&file, &bytes)?;
    // Pictures of games that left, unless the same game comes back under the same id.
    let staying: BTreeSet<u32> = a.apps.iter().copied().collect();
    for o in a.plan.remove.iter().filter(|o| !staying.contains(&o.app_id)) {
        shortcuts::remove_art(&a.cfg, o.app_id);
    }
    let removed: Vec<u32> = a.plan.remove.iter().map(|o| o.app_id).filter(|id| !staying.contains(id)).collect();
    sync_collection(&a.cfg, &a.apps, &removed)?;
    Ok(())
}

// ---------- which games ----------

/// Ready to play from Steam: installed, its own first-time setup run, and its copy of the game and
/// Easy Setup files picked. Otherwise Steam would open the launcher to set it up, not the game.
fn ready(st: &App, recomp: &str, g: &Installed, game: Option<(&games::Game, &[games::Game])>) -> bool {
    let ui = st.lib.lock().unwrap().settings.ui.clone();
    if crate::catalog::needs_setup_first(&st.dir, recomp, &ui) {
        return false;
    }
    if g.exe.is_none() || !g.installer_setup.is_empty() {
        return false;
    }
    if g.setup.is_some() && !g.setup_done {
        return false;
    }
    // Set Up By Hand: the player says how it starts and gets its files.
    if g.manual.is_some() {
        return true;
    }
    let Some(whole) = crate::profile_of(st, recomp, g) else { return true };
    let has_copy = g.game_copy.as_ref().map(|c| c.exists()).unwrap_or(false) || whole.already_in_place(&g.path).is_some();
    if whole.game.is_some() && !has_copy {
        return false;
    }
    if whole.inputs.is_empty() {
        return true;
    }
    let Some((game, all)) = game else { return crate::inputs::ready(&whole, g, &st.dir) };
    let mut scoped = whole.clone();
    games::scope(&mut scoped, game, all);
    scoped.inputs.is_empty() || crate::game_inputs_ready(&scoped, &whole, g, game, &st.dir)
}

/// The games that should be in Steam, and the ones installed but not ready yet (their entries
/// stay as they are).
async fn wants(st: &App) -> (Vec<Want>, BTreeSet<String>) {
    let (list, aliases) = {
        let l = st.lib.lock().unwrap();
        (l.installed.iter().map(|(k, v)| (k.clone(), v.clone())).collect::<Vec<_>>(), l.project_aliases.clone())
    };
    let mut wants = vec![];
    let mut keep = BTreeSet::new();
    for (id, g) in list {
        // A recomp with several games is a tile, and a Steam entry, per game.
        let all = crate::games_of(st, &id, &g);
        let tiles: Vec<(String, Option<&games::Game>)> = if all.is_empty() { vec![(id.clone(), None)] } else { all.iter().map(|x| (games::full(&id, &x.id), Some(x))).collect() };
        for (tile, game) in tiles {
            if !ready(st, &id, &g, game.map(|x| (x, all.as_slice()))) {
                keep.insert(tile);
                continue;
            }
            let name = crate::shown_name(st, &tile, &g).await;
            let start = shortcuts::game_start(&st.dir, &tile);
            wants.push(Want { id: tile, name, exe: start.exe.to_string_lossy().to_string(), launch: start.launch });
        }
    }
    // An old id kept for shortcuts after a project match was corrected plays the game it now
    // points to: kept while that game is.
    for (old, new) in aliases {
        if keep.contains(&new) {
            keep.insert(old);
        }
    }
    (wants, keep)
}

/// Linux from the AppImage: each game's launch script starts the AppImage where it is now (it may
/// have moved since). Steam reads the script at Play, so this needs no restart.
fn refresh_scripts(st: &App, wants: &[Want]) {
    for w in wants {
        let start = shortcuts::game_start(&st.dir, &w.id);
        let Some(appimage) = start.appimage else { continue };
        let _ = shortcuts::write_steam_script(&st.dir, &w.name, &w.id, &appimage);
    }
}

fn accounts(steam: &Path, data: &Path, wants: &[Want], keep: &BTreeSet<String>) -> Vec<Account> {
    shortcuts::steam_users(steam).iter().filter_map(|u| account(u, data, wants, keep)).collect()
}

fn behind(accts: &[Account]) -> bool {
    accts.iter().any(|a| !a.plan.is_empty() || collection_behind(a))
}

// ---------- syncing ----------

/// One sync at a time: the timer and Sync Now could otherwise both write.
static BUSY: AtomicBool = AtomicBool::new(false);

struct Busy;
impl Busy {
    fn take() -> Option<Busy> {
        BUSY.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).ok().map(|_| Busy)
    }
}
impl Drop for Busy {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::Release);
    }
}

/// Bring Steam in line. With Steam open and `restart` false nothing is written and the answer is
/// "steam_running", so the window can ask first; with `restart`, Steam is closed, written to and
/// opened again. "in_sync" when nothing had to change, "synced:<added>:<removed>" after writing.
async fn sync(st: &App, restart: bool) -> Res<String> {
    let steam = shortcuts::steam_dir().ok_or_else(|| Error::Msg("Steam isn't installed on this PC.".into()))?;
    let Some(_busy) = Busy::take() else { return Ok("busy".into()) };
    let (wants, keep) = wants(st).await;
    refresh_scripts(st, &wants);
    if !behind(&accounts(&steam, &st.dir, &wants, &keep)) {
        return Ok("in_sync".into());
    }
    let was_running = shortcuts::steam_running();
    if was_running && !restart {
        return Ok("steam_running".into());
    }
    if was_running {
        if let Some(why) = shortcuts::cant_restart_steam() {
            return Err(Error::Msg(why.into()));
        }
        trail::note("Steam sync: closing Steam");
        shortcuts::close_steam(&steam).await.inspect_err(|e| trail::note(format!("Steam sync: {e}")))?;
    }
    // Read again now Steam is closed: it writes its list back as it closes.
    let accts = accounts(&steam, &st.dir, &wants, &keep);
    let coming: BTreeSet<String> = accts.iter().flat_map(|a| a.plan.add.iter().map(|w| w.id.clone())).collect();
    let mut arts = Arts::new();
    for id in coming {
        let Ok(g) = crate::installed_or_game(st, &id) else { continue };
        arts.insert(id.clone(), crate::steam_art(st, &id, &g).await);
    }
    let (mut added, mut removed) = (0, 0);
    let mut failed = None;
    for a in &accts {
        if !a.plan.is_empty() || collection_behind(a) {
            match apply(a, &arts) {
                Ok(()) => {
                    added += a.plan.add.len();
                    removed += a.plan.remove.len();
                }
                Err(e) => failed = Some(e),
            }
        }
    }
    if was_running {
        shortcuts::start_steam(&steam);
    }
    if let Some(e) = failed {
        trail::note(format!("Steam sync: {e}"));
        if added + removed == 0 {
            return Err(e);
        }
    }
    trail::note(format!("Steam sync: {added} added, {removed} removed, for {} account(s)", accts.len()));
    Ok(format!("synced:{added}:{removed}"))
}

/// Settings → Keep Steam in Sync → Sync Now (and turning it on).
#[tauri::command]
pub async fn steam_sync_now(st: State<'_, App>, restart: bool) -> Res<String> {
    sync(&st, restart).await
}

/// For the setting's line: how many games are waiting to go in or out of Steam.
#[tauri::command]
pub async fn steam_sync_state(st: State<'_, App>) -> Res<Value> {
    let Some(steam) = shortcuts::steam_dir() else { return Ok(json!({ "steam": false })) };
    let (wants, keep) = wants(&st).await;
    let accts = accounts(&steam, &st.dir, &wants, &keep);
    let add: BTreeSet<String> = accts.iter().flat_map(|a| a.plan.add.iter().map(|w| w.id.clone())).collect();
    let remove: BTreeSet<String> = accts.iter().flat_map(|a| a.plan.remove.iter().map(|o| o.key.clone() + &o.id)).collect();
    Ok(json!({ "steam": true, "games": wants.len(), "add": add.len(), "remove": remove.len(), "collection": accts.iter().any(collection_behind), "running": shortcuts::steam_running() }))
}

/// While the setting is on: write what changed whenever Steam is found closed.
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
        // The same problem every two minutes goes in the log once.
        let mut told = String::new();
        loop {
            let st = app.state::<App>();
            let on = st.lib.lock().unwrap().settings.steam_sync;
            if on && shortcuts::steam_dir().is_some() && !shortcuts::steam_running() {
                let why = sync(&st, false).await.err().map(|e| e.to_string()).unwrap_or_default();
                if !why.is_empty() && why != told {
                    trail::note(format!("Steam sync: {why}"));
                }
                told = why;
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn want(id: &str, name: &str) -> Want {
        Want { id: id.into(), name: name.into(), exe: r"C:\RL\recomplauncher.exe".into(), launch: format!("--play {id}") }
    }

    fn file_with(wants: &[Want]) -> Vec<u8> {
        let mut bytes = vec![];
        for w in wants {
            bytes = shortcuts::add_shortcut(Some(bytes.as_slice()), &w.name, &w.exe, r"C:\RL", &w.launch, "").0;
        }
        bytes
    }

    fn owned_in(bytes: &[u8]) -> Vec<Owned> {
        owned(&entries(Some(bytes)).unwrap(), &|_| None)
    }

    #[test]
    fn reads_the_game_a_launch_option_plays() {
        assert_eq!(played_id("--play some-recomp").as_deref(), Some("some-recomp"));
        assert_eq!(played_id("'run' 'io.github.RecompLauncher' --play cogbox--gow2").as_deref(), Some("cogbox--gow2"));
        assert_eq!(played_id("--bigpicture"), None);
    }

    #[test]
    fn the_launchers_own_entry_and_other_programs_are_not_its_games() {
        let other = vec![("0".to_string(), Node::Map(vec![("AppName".into(), Node::Str("Dolphin".into())), ("Exe".into(), Node::Str("\"dolphin.exe\"".into())), ("LaunchOptions".into(), Node::Str("--play x".into()))]))];
        let mut bytes = shortcuts::write_vdf(&[("shortcuts".to_string(), Node::Map(other))]);
        bytes = shortcuts::add_shortcut(Some(bytes.as_slice()), "RecompLauncher", r"C:\RL\recomplauncher.exe", r"C:\RL", "--bigpicture", "").0;
        assert!(owned_in(&bytes).is_empty());
    }

    #[test]
    fn a_ready_game_missing_from_steam_is_added() {
        let p = plan(&[], &[want("banjo", "Banjo")], &BTreeSet::new());
        assert_eq!(p.add.len(), 1);
        assert!(p.remove.is_empty());
    }

    #[test]
    fn a_game_already_in_steam_the_same_way_is_left_alone() {
        let w = want("banjo", "Banjo");
        let bytes = file_with(std::slice::from_ref(&w));
        assert!(plan(&owned_in(&bytes), &[w], &BTreeSet::new()).is_empty());
    }

    #[test]
    fn an_uninstalled_games_entry_is_removed() {
        let bytes = file_with(&[want("banjo", "Banjo")]);
        let p = plan(&owned_in(&bytes), &[], &BTreeSet::new());
        assert_eq!(p.remove.len(), 1);
        assert_eq!(p.remove[0].id, "banjo");
    }

    #[test]
    fn a_game_not_ready_yet_keeps_its_entry() {
        let bytes = file_with(&[want("banjo", "Banjo")]);
        let keep = BTreeSet::from(["banjo".to_string()]);
        assert!(plan(&owned_in(&bytes), &[], &keep).is_empty());
    }

    #[test]
    fn a_renamed_game_is_replaced() {
        let bytes = file_with(&[want("banjo", "Banjo")]);
        let p = plan(&owned_in(&bytes), &[want("banjo", "Banjo-Kazooie")], &BTreeSet::new());
        assert_eq!(p.add[0].name, "Banjo-Kazooie");
        assert_eq!(p.remove[0].name, "Banjo");
    }

    #[test]
    fn removing_keeps_other_entries_numbered_from_zero() {
        let bytes = file_with(&[want("a", "A"), want("b", "B"), want("c", "C")]);
        let out = without(&bytes, &BTreeSet::from(["1".to_string()])).unwrap();
        let list = entries(Some(out.as_slice())).unwrap();
        let names: Vec<(&str, &str)> = list.iter().map(|(k, e)| (k.as_str(), e.str("AppName"))).collect();
        assert_eq!(names, vec![("0", "A"), ("1", "C")]);
    }

    #[test]
    fn a_new_collection_goes_in_at_the_newest_version() {
        let file = r#"[["user-collections.favorite",{"key":"user-collections.favorite","timestamp":1,"value":"{\"id\":\"favorite\",\"name\":\"Favorites\",\"added\":[],\"removed\":[]}","version":"5085"}]]"#;
        let (out, key) = with_collection(file, &[3890861067], &[], 99).unwrap();
        assert_eq!(key, "user-collections.rl-recomplauncher");
        let list: Vec<Value> = serde_json::from_str(&out).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[1][1]["version"], "5085");
        let value: Value = serde_json::from_str(list[1][1]["value"].as_str().unwrap()).unwrap();
        assert_eq!(value, json!({ "id": "rl-recomplauncher", "name": "RecompLauncher", "added": [3890861067u32], "removed": [] }));
    }

    #[test]
    fn the_collection_keeps_games_the_player_put_in_it() {
        let value = json!({ "id": "rl-recomplauncher", "name": "RecompLauncher", "added": [5, 7], "removed": [] }).to_string();
        let file = json!([["user-collections.rl-recomplauncher", { "key": "user-collections.rl-recomplauncher", "timestamp": 1, "value": value, "version": "10" }]]).to_string();
        let (out, _) = with_collection(&file, &[9], &[7], 2).unwrap();
        let list: Vec<Value> = serde_json::from_str(&out).unwrap();
        let value: Value = serde_json::from_str(list[0][1]["value"].as_str().unwrap()).unwrap();
        assert_eq!(value["added"], json!([5, 9]));
    }

    #[test]
    fn an_up_to_date_collection_is_not_written() {
        let value = json!({ "id": "rl-recomplauncher", "name": "RecompLauncher", "added": [5], "removed": [] }).to_string();
        let file = json!([["user-collections.rl-recomplauncher", { "key": "user-collections.rl-recomplauncher", "timestamp": 1, "value": value, "version": "10" }]]).to_string();
        assert_eq!(with_collection(&file, &[5], &[], 2), None);
    }

    #[test]
    fn a_collection_the_player_named_recomplauncher_is_used() {
        let value = json!({ "id": "uc-abc", "name": "RecompLauncher", "added": [], "removed": [] }).to_string();
        let file = json!([["user-collections.uc-abc", { "key": "user-collections.uc-abc", "timestamp": 1, "value": value, "version": "10" }]]).to_string();
        let (out, key) = with_collection(&file, &[5], &[], 2).unwrap();
        assert_eq!(key, "user-collections.uc-abc");
        assert_eq!(serde_json::from_str::<Vec<Value>>(&out).unwrap().len(), 1);
    }

    #[test]
    fn the_collections_file_is_backed_up_once_before_the_first_write() {
        let d = std::env::temp_dir().join(format!("rl-steam-backup-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        keep_collections_backup(&d, "the player's own");
        keep_collections_backup(&d, "after the launcher wrote");
        assert_eq!(fs::read_to_string(d.join("cloud-storage-namespace-1.json.recomplauncher-backup")).unwrap(), "the player's own");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn the_changed_collection_is_marked_for_upload_once() {
        let once = with_modified("[]", "user-collections.rl-recomplauncher").unwrap();
        assert_eq!(once, r#"["user-collections.rl-recomplauncher"]"#);
        assert_eq!(with_modified(&once, "user-collections.rl-recomplauncher"), None);
    }
}

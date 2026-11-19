//! Online and local network play: what a recomp says it supports (`network` in its
//! recomplauncher.json), and the dedicated server its release carries, which the launcher can start
//! for the player (Host a Server) and stop again. The launcher adds no netcode of its own: it runs
//! the developer's program with the port the player picked, and says the address to share.

use crate::{App, Error, Res};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::net::{IpAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::State;

/// The ways a recomp can be played with others that the launcher shows.
pub const MODES: [&str; 2] = ["online", "lan"];

/// Below this, Linux and macOS only let an administrator open a port.
const LOWEST_PORT: u16 = 1024;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Network {
    /// "online", "lan" or both. Other words are left out, so a newer file still reads.
    #[serde(default, deserialize_with = "modes_lenient")]
    pub modes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<Server>,
}

/// The dedicated server in the release, started with Host a Server.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Server {
    /// The program, first one that exists (relative to the game's folder).
    pub program: Vec<String>,
    /// Its arguments: `{port}` is the port the player picked, paths fill in like everywhere else.
    #[serde(default)]
    pub args: Vec<String>,
    /// The port it listens on unless the player picks another.
    pub port: u16,
    /// What the game page calls it ("Dedicated server").
    #[serde(default)]
    pub label: String,
}

fn modes_lenient<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    let v = Value::deserialize(d)?;
    let words = v.as_array().map(|l| l.iter().filter_map(Value::as_str).map(str::to_lowercase).collect::<Vec<_>>()).unwrap_or_default();
    let mut out: Vec<String> = Vec::new();
    for w in words {
        if MODES.contains(&w.as_str()) && !out.contains(&w) {
            out.push(w);
        }
    }
    Ok(out)
}

/// `network` as written, when it can be read: a block with a mistake in it is left out (the
/// developer check names it), never the whole file, so the game still plays.
pub fn lenient<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Network>, D::Error> {
    let v = Value::deserialize(d)?;
    Ok(serde_json::from_value::<Network>(v).ok())
}

impl Server {
    /// Its program in the game's folder, found the way a setup step's is.
    pub fn program_in(&self, install: &Path) -> Option<PathBuf> {
        let named = self.program.iter().find_map(|p| crate::profile::named_program(install.join(p)));
        if named.is_some() || cfg!(windows) {
            return named;
        }
        self.program.iter().find_map(|p| crate::profile::native_twin(install, p))
    }

    /// Its arguments for this start, with the port and paths filled in.
    pub fn args_for(&self, install: &Path, port: u16) -> Vec<String> {
        let port = port.to_string();
        self.args.iter().map(|a| crate::profile::fill(&a.replace("{port}", &port), install, None)).collect()
    }

    /// The port to use: the one the player asked for when it's one they can open, else the
    /// developer's.
    pub fn port_for(&self, asked: Option<u16>) -> u16 {
        asked.filter(|p| *p >= LOWEST_PORT).unwrap_or(self.port)
    }
}

/// This PC's address on the local network, for other players to join. Connecting a UDP socket
/// sends nothing; it only makes the system pick the network it would use.
pub fn lan_address() -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    // A documentation-only address (TEST-NET-1): never answered, and nothing goes out anyway.
    socket.connect("192.0.2.1:9").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

struct Hosted {
    child: std::process::Child,
    port: u16,
    log: PathBuf,
}

/// Servers this launcher started, by game id.
static RUNNING: Mutex<BTreeMap<String, Hosted>> = Mutex::new(BTreeMap::new());

/// A server running now, as the game page shows it.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Hosting {
    pub port: u16,
    /// Where other players connect: this PC's local address and the port. None when the PC isn't
    /// on a network.
    pub address: Option<String>,
    pub log: PathBuf,
}

/// What the game page shows about playing with others.
#[derive(Serialize, Clone, Debug)]
pub struct View {
    pub modes: Vec<String>,
    pub server: Option<ServerView>,
    pub hosting: Option<Hosting>,
}

#[derive(Serialize, Clone, Debug)]
pub struct ServerView {
    pub label: String,
    pub port: u16,
}

fn hosting_of(h: &Hosted) -> Hosting {
    Hosting { port: h.port, address: lan_address().map(|ip| format!("{ip}:{}", h.port)), log: h.log.clone() }
}

/// The server for this game, when it's still running. One that closed by itself is forgotten.
fn running(id: &str) -> Option<Hosting> {
    let mut all = RUNNING.lock().unwrap();
    let still = all.get_mut(id).map(|h| matches!(h.child.try_wait(), Ok(None)))?;
    if !still {
        all.remove(id);
        return None;
    }
    all.get(id).map(hosting_of)
}

pub fn view(p: &crate::profile::Profile, id: &str) -> Option<View> {
    let n = p.network.as_ref()?;
    let server = n.server.as_ref().map(|s| ServerView { label: s.label.trim().to_string(), port: s.port });
    let hosting = server.as_ref().and_then(|_| running(id));
    Some(View { modes: n.modes.clone(), server, hosting })
}

/// The command that starts the server: a Windows program on Linux goes through Proton, as Play
/// does for the game.
fn command_for(st: &App, exe: &Path) -> Res<std::process::Command> {
    let ext = exe.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let windows_program = matches!(ext.as_str(), "exe" | "bat" | "cmd");
    if cfg!(windows) || !windows_program {
        return Ok(std::process::Command::new(exe));
    }
    let runner = crate::proton::runner().ok_or_else(|| Error::Msg("This server is a Windows program, and it needs Proton on Linux. Settings → General → GE-Proton installs it in one press.".into()))?;
    Ok(crate::proton::std_command(&runner, &st.dir, exe))
}

/// Host a Server: starts the game's dedicated server on `port` (or the developer's), and keeps
/// what it prints in a log beside the launcher's data. Already running: says where.
#[tauri::command(async)]
pub fn server_start(st: State<'_, App>, id: String, port: Option<u16>) -> Res<Hosting> {
    if let Some(h) = running(&id) {
        return Ok(h);
    }
    let g = st.lib.lock().unwrap().installed.get(&id).cloned().ok_or_else(|| Error::Msg("It isn't installed.".into()))?;
    let p = crate::profile_of(&st, &id, &g).ok_or_else(|| Error::Msg("This game doesn't say how to start its server.".into()))?;
    let s = p.network.and_then(|n| n.server).ok_or_else(|| Error::Msg("This game doesn't have a server to host.".into()))?;
    let exe = s.program_in(&g.path).ok_or_else(|| Error::Msg("Its server program is missing. Reinstall the game.".into()))?;
    let port = s.port_for(port);
    let c = command_for(&st, &exe)?;
    let cwd = exe.parent().unwrap_or(&g.path).to_path_buf();
    launch(&id, c, &cwd, s.args_for(&g.path, port), &st.dir.join("servers"), port)
}

/// Starts the server's command, its output in `<logs>/<id>.log`, and remembers it for Stop.
fn launch(id: &str, mut c: std::process::Command, cwd: &Path, args: Vec<String>, logs: &Path, port: u16) -> Res<Hosting> {
    std::fs::create_dir_all(logs)?;
    let log = logs.join(format!("{id}.log"));
    let out = std::fs::File::create(&log)?;
    let err = out.try_clone()?;
    crate::sysenv::clean_std(&mut c);
    c.args(args).current_dir(cwd).stdin(std::process::Stdio::null()).stdout(out).stderr(err);
    // Its output goes to the log, so no console window of its own.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    // A group of its own: Stop ends whatever the server started too.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        c.process_group(0);
    }
    let child = c.spawn().map_err(|e| Error::Msg(format!("Couldn't start its server: {e}")))?;
    crate::trail::note(format!("Server {id}: started on port {port}"));
    let hosted = Hosted { child, port, log };
    let now = hosting_of(&hosted);
    RUNNING.lock().unwrap().insert(id.to_string(), hosted);
    Ok(now)
}

/// Stops the game's server, if this launcher started one.
#[tauri::command]
pub fn server_stop(id: String) -> Res<()> {
    let Some(mut h) = RUNNING.lock().unwrap().remove(&id) else { return Ok(()) };
    crate::kill_process_tree(&mut h.child);
    crate::trail::note(format!("Server {id}: stopped"));
    Ok(())
}

/// The game's server, while it runs.
#[tauri::command]
pub fn server_now(id: String) -> Option<Hosting> {
    running(&id)
}

/// Opens the folder with the game's server log: what it printed explains a server that closed.
#[tauri::command(async)]
pub fn server_log(app: tauri::AppHandle, st: State<'_, App>) -> Res<()> {
    let logs = st.dir.join("servers");
    std::fs::create_dir_all(&logs)?;
    crate::open_thing(&app, &logs.to_string_lossy(), false)
}

/// The launcher closing: its servers close with it, so none keeps a port open unseen.
pub fn stop_all() {
    let all = std::mem::take(&mut *RUNNING.lock().unwrap());
    for (_, mut h) in all {
        crate::kill_process_tree(&mut h.child);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn network(json: &str) -> Network {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn modes_keep_only_the_known_ones_once() {
        let n = network(r#"{"modes":["LAN","online","splitscreen","lan"]}"#);
        assert_eq!(n.modes, vec!["lan", "online"]);
    }

    #[test]
    fn modes_that_arent_a_list_read_as_none() {
        let n = network(r#"{"modes":"online"}"#);
        assert!(n.modes.is_empty());
    }

    #[test]
    fn a_server_needs_its_program_and_port() {
        assert!(serde_json::from_str::<Server>(r#"{"program":["Server.exe"]}"#).is_err());
        assert!(serde_json::from_str::<Server>(r#"{"port":27015}"#).is_err());
    }

    #[test]
    fn the_port_fills_into_the_arguments() {
        let s: Server = serde_json::from_str(r#"{"program":["Server.exe"],"args":["--port","{port}","--name=Game {port}"],"port":27015}"#).unwrap();
        assert_eq!(s.args_for(Path::new("/games/g"), 28000), vec!["--port", "28000", "--name=Game 28000"]);
    }

    #[test]
    fn install_fills_into_the_arguments() {
        let s: Server = serde_json::from_str(r#"{"program":["Server.exe"],"args":["--config={install}/server.ini"],"port":27015}"#).unwrap();
        let args = s.args_for(Path::new("/games/g"), 27015);
        assert_eq!(args, vec![format!("--config={}/server.ini", Path::new("/games/g").to_string_lossy())]);
    }

    #[test]
    fn the_developers_port_is_used_unless_the_player_picks_one() {
        let s = Server { program: vec!["Server.exe".into()], port: 27015, ..Default::default() };
        assert_eq!(s.port_for(None), 27015);
        assert_eq!(s.port_for(Some(28000)), 28000);
    }

    #[test]
    fn a_port_only_an_administrator_can_open_is_not_used() {
        let s = Server { program: vec!["Server.exe".into()], port: 27015, ..Default::default() };
        assert_eq!(s.port_for(Some(80)), 27015);
        assert_eq!(s.port_for(Some(0)), 27015);
    }

    #[test]
    fn the_server_program_is_found_in_the_games_folder() {
        let dir = std::env::temp_dir().join(format!("rl-server-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(dir.join("bin/GameServer"), b"").unwrap();
        let s = Server { program: vec!["Missing".into(), "bin/GameServer".into()], port: 27015, ..Default::default() };
        assert_eq!(s.program_in(&dir), Some(dir.join("bin/GameServer")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_network_block_with_a_mistake_leaves_the_rest_of_the_file() {
        let p: crate::profile::Profile = serde_json::from_str(r#"{"format":1,"saves":["{install}/saves"],"network":{"server":{"program":"Server.exe"}}}"#).unwrap();
        assert!(p.network.is_none());
        assert_eq!(p.saves, vec!["{install}/saves"]);
    }

    #[test]
    fn a_game_without_a_network_block_has_nothing_to_show() {
        let p = crate::profile::Profile::default();
        assert!(view(&p, "game").is_none());
    }

    #[test]
    fn a_server_that_isnt_running_shows_no_address() {
        let p: crate::profile::Profile = serde_json::from_str(r#"{"network":{"modes":["lan"],"server":{"program":["S"],"port":27015,"label":"Dedicated server"}}}"#).unwrap();
        let v = view(&p, "not-running").unwrap();
        assert_eq!(v.modes, vec!["lan"]);
        assert_eq!(v.server.unwrap().label, "Dedicated server");
        assert!(v.hosting.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn a_hosted_server_runs_with_its_port_and_stops() {
        let dir = std::env::temp_dir().join(format!("rl-hosted-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("server.sh");
        std::fs::write(&script, "echo listening on $2\nexec sleep 30\n").unwrap();
        let mut c = std::process::Command::new("sh");
        c.arg(&script);
        let id = "hosted-test";
        let h = launch(id, c, &dir, vec!["--port".into(), "28123".into()], &dir.join("servers"), 28123).unwrap();
        assert_eq!(h.port, 28123);
        let log = dir.join("servers/hosted-test.log");
        let said = (0..50).find_map(|_| {
            std::thread::sleep(std::time::Duration::from_millis(50));
            std::fs::read_to_string(&log).ok().filter(|t| t.contains("listening"))
        });
        assert_eq!(said.as_deref().map(str::trim), Some("listening on 28123"));
        assert!(running(id).is_some());
        server_stop(id.into()).unwrap();
        assert!(running(id).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_server_that_closed_by_itself_is_forgotten() {
        let dir = std::env::temp_dir().join(format!("rl-closed-{}", std::process::id()));
        let id = "closed-test";
        launch(id, std::process::Command::new("true"), &std::env::temp_dir(), vec![], &dir, 28124).unwrap();
        let gone = (0..50).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(50));
            running(id).is_none()
        });
        assert!(gone);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

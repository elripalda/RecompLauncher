//! A game started from a Steam shortcut while the launcher is already open (#424).
//!
//! On Windows Steam puts its overlay and Steam Input into the program it started and the programs
//! that one starts. The open launcher wasn't started by Steam, so a game it starts gets neither. So
//! the copy Steam started (`--play`) waits here, and the open launcher, once it has the game ready
//! (cloud saves, its Look, its settings) as for any Play, asks that copy to start the program. That
//! copy keeps the game as its child until it closes; the open launcher watches it by its process,
//! for play time, the cloud save after it and the game showing as playing.
//!
//! Only on loopback, and only for the one-time token the copy Steam started handed over with its
//! `--play`: nothing else on the PC can ask it to start a program.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::Path;
use std::time::Duration;

const ARG: &str = "--play-host=";
/// A request is a program, its arguments and its environment changes: far below this.
const MAX_REQUEST: u64 = 1 << 20;

/// Whether the copy Steam started starts the game itself instead of leaving it to the launcher.
/// Only on Windows: on Linux Steam finds the game by the launch context the handoff carries (see
/// sysenv::play_handoff_args), and a Steam-started copy there may run in another AppImage mount or
/// Flatpak sandbox than the launcher that set the game up.
pub fn hosts_game(windows: bool, from_steam: bool) -> bool {
    windows && from_steam
}

/// Where the copy Steam started waits, as the handoff tells the launcher.
#[derive(Clone, Debug, PartialEq)]
pub struct Host {
    pub port: u16,
    pub token: String,
}

impl Host {
    pub fn arg(&self) -> String {
        format!("{ARG}{}:{}", self.port, self.token)
    }

    /// Only a handoff names a host: a `--play-host` typed on a shortcut of its own is ignored.
    pub fn from_args(args: &[String]) -> Option<Host> {
        if !args.iter().any(|a| a == "--handoff") {
            return None;
        }
        let (port, token) = args.iter().find_map(|a| a.strip_prefix(ARG))?.split_once(':')?;
        let port = port.parse::<u16>().ok().filter(|p| *p != 0)?;
        let well_formed = token.len() == 32 && token.bytes().all(|b| b.is_ascii_hexdigit());
        well_formed.then(|| Host { port, token: token.to_string() })
    }
}

/// The game's program as the launcher made it ready, for the copy Steam started to start.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Request {
    pub token: String,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    /// What the launcher changed in the environment: a value, or None for taken out. The rest comes
    /// from the copy Steam started, Steam's own variables included.
    pub env: Vec<(String, Option<String>)>,
    /// The file what the game prints goes to (made fresh by the launcher already).
    pub output: Option<String>,
}

impl Request {
    /// None when part of it isn't text that can be sent (a path only Windows can spell): the
    /// launcher starts the game itself then.
    pub fn of(cmd: &std::process::Command, output: Option<&Path>, token: &str) -> Option<Request> {
        let text = |s: &std::ffi::OsStr| s.to_str().map(str::to_string);
        let mut env = vec![];
        for (k, v) in cmd.get_envs() {
            let value = match v {
                Some(v) => Some(text(v)?),
                None => None,
            };
            env.push((text(k)?, value));
        }
        let args = cmd.get_args().map(text).collect::<Option<Vec<_>>>()?;
        let cwd = match cmd.get_current_dir() {
            Some(d) => Some(text(d.as_os_str())?),
            None => None,
        };
        let output = match output {
            Some(p) => Some(text(p.as_os_str())?),
            None => None,
        };
        Some(Request { token: token.to_string(), program: text(cmd.get_program())?, args, cwd, env, output })
    }

    /// The program again, as the launcher would have started it.
    pub fn command(&self) -> std::process::Command {
        let mut c = std::process::Command::new(&self.program);
        c.args(&self.args);
        if let Some(d) = &self.cwd {
            c.current_dir(d);
        }
        for (k, v) in &self.env {
            match v {
                Some(v) => c.env(k, v),
                None => c.env_remove(k),
            };
        }
        let out = self.output.as_ref().and_then(|p| std::fs::OpenOptions::new().append(true).open(p).ok());
        if let Some(f) = out {
            if let Ok(f2) = f.try_clone() {
                c.stdout(f).stderr(f2);
            }
        }
        c
    }
}

/// What the copy Steam started answers: the game's process id, or why it didn't start.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub enum Reply {
    Started(u32),
    Failed { code: Option<i32>, not_found: bool, msg: String },
}

impl Reply {
    fn of(r: &std::io::Result<std::process::Child>) -> Reply {
        match r {
            Ok(c) => Reply::Started(c.id()),
            Err(e) => Reply::Failed { code: e.raw_os_error(), not_found: e.kind() == std::io::ErrorKind::NotFound, msg: e.to_string() },
        }
    }

    /// The same error a start in the launcher would have had, so it says the same to the player.
    /// A missing program has no OS code on Windows (std looks the program up itself), so whether
    /// it was "not found" travels on its own.
    pub fn error(code: Option<i32>, not_found: bool, msg: &str) -> std::io::Error {
        if let Some(c) = code {
            return std::io::Error::from_raw_os_error(c);
        }
        let kind = if not_found { std::io::ErrorKind::NotFound } else { std::io::ErrorKind::Other };
        std::io::Error::new(kind, msg.to_string())
    }
}

/// The copy Steam started, waiting for the launcher's request.
pub struct Listening {
    listener: TcpListener,
    host: Host,
}

pub fn listen() -> Option<Listening> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).ok()?;
    let port = listener.local_addr().ok()?.port();
    Some(Listening { listener, host: Host { port, token: token()? } })
}

/// 128 bits from the system's random source. Without one, no host: the game is handed over as before.
fn token() -> Option<String> {
    let mut bytes = [0u8; 16];
    os_random(&mut bytes)?;
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(windows)]
fn os_random(bytes: &mut [u8]) -> Option<()> {
    use windows::Win32::Security::Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG};
    unsafe { BCryptGenRandom(None, bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG) }.ok().ok()
}

// The host only runs on Windows; this is for the tests elsewhere.
#[cfg(not(windows))]
fn os_random(bytes: &mut [u8]) -> Option<()> {
    use std::io::Read;
    std::fs::File::open("/dev/urandom").ok()?.read_exact(bytes).ok()
}

impl Listening {
    pub fn host(&self) -> &Host {
        &self.host
    }

    /// Starts the game for the first request with the right token, then keeps it as its own
    /// child until it closes. Anything else that connects is dropped.
    pub fn serve(self) {
        for conn in self.listener.incoming() {
            let Ok(stream) = conn else { continue };
            let Some(mut game) = answer(stream, &self.host.token) else { continue };
            drop(self.listener);
            let _ = game.wait();
            return;
        }
    }
}

fn answer(stream: TcpStream, token: &str) -> Option<std::process::Child> {
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut line = String::new();
    reader.by_ref().take(MAX_REQUEST).read_line(&mut line).ok()?;
    let req: Request = serde_json::from_str(&line).ok()?;
    if req.token != token {
        return None;
    }
    let started = req.command().spawn();
    let reply = serde_json::to_string(&Reply::of(&started)).ok()?;
    let mut out = stream;
    let _ = out.write_all(format!("{reply}\n").as_bytes());
    let child = started.ok()?;
    // The game's process id stays this game's while its handle is open here: kept until the
    // launcher says it holds the game too, so it can't open another process by the same number.
    let _ = reader.get_ref().set_read_timeout(Some(Duration::from_secs(30)));
    let mut held = String::new();
    let _ = reader.read_line(&mut held);
    Some(child)
}

/// Sends the request and reads the answer. The connection stays open for the "held" after it.
pub fn ask(host: &Host, req: &Request) -> std::io::Result<(Reply, TcpStream)> {
    let mut stream = TcpStream::connect_timeout(&(Ipv4Addr::LOCALHOST, host.port).into(), Duration::from_secs(3))?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    let line = serde_json::to_string(req).map_err(std::io::Error::other)?;
    stream.write_all(format!("{line}\n").as_bytes())?;
    let mut reply = String::new();
    BufReader::new(stream.try_clone()?).take(MAX_REQUEST).read_line(&mut reply)?;
    let reply: Reply = serde_json::from_str(&reply).map_err(std::io::Error::other)?;
    Ok((reply, stream))
}

/// A game the launcher is playing: started by itself, or by the copy Steam started.
pub enum Game {
    Own(tokio::process::Child),
    #[cfg(windows)]
    Hosted(hosted::Hosted),
}

impl Game {
    pub fn id(&self) -> Option<u32> {
        match self {
            Game::Own(c) => c.id(),
            #[cfg(windows)]
            Game::Hosted(h) => Some(h.pid),
        }
    }

    #[cfg(windows)]
    pub fn raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
        match self {
            Game::Own(c) => c.raw_handle(),
            Game::Hosted(h) => Some(h.raw_handle()),
        }
    }

    /// Its exit code once it closes.
    pub async fn wait(&mut self) -> Option<i32> {
        match self {
            Game::Own(c) => c.wait().await.ok().and_then(|s| s.code()),
            #[cfg(windows)]
            Game::Hosted(h) => h.wait().await,
        }
    }

    pub async fn kill(&mut self) {
        match self {
            Game::Own(c) => {
                let _ = c.kill().await;
            }
            #[cfg(windows)]
            Game::Hosted(h) => h.kill(),
        }
    }
}

/// Asks the copy Steam started to start the game. `Err(None)`: it can't be reached (it gave up
/// waiting, or the request can't be sent), so the launcher starts the game itself as before.
/// `Err(Some(e))`: the program didn't start there, for the reason a start here would give.
#[cfg(windows)]
pub fn start_through(host: &Host, cmd: &std::process::Command, output: Option<&Path>) -> Result<Game, Option<std::io::Error>> {
    let req = Request::of(cmd, output, &host.token).ok_or(None)?;
    let (reply, mut stream) = ask(host, &req).map_err(|_| None)?;
    let pid = match reply {
        Reply::Started(pid) => pid,
        Reply::Failed { code, not_found, msg } => return Err(Some(Reply::error(code, not_found, &msg))),
    };
    // The host holds the game's process open until "held", so this finds the game even when it
    // closed at once.
    let game = hosted::Hosted::open(pid);
    let _ = stream.write_all(b"held\n");
    game.map(Game::Hosted).map_err(Some)
}

#[cfg(windows)]
pub mod hosted {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
    use std::sync::Arc;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, TerminateProcess, WaitForSingleObject, INFINITE, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE};

    /// The game's process, opened by its id: waited on and stopped like a child of the launcher.
    pub struct Hosted {
        pub pid: u32,
        handle: Arc<OwnedHandle>,
    }

    fn api(h: &OwnedHandle) -> HANDLE {
        HANDLE(h.as_raw_handle() as *mut _)
    }

    impl Hosted {
        pub fn open(pid: u32) -> std::io::Result<Hosted> {
            let access = PROCESS_SYNCHRONIZE | PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION;
            let h = unsafe { OpenProcess(access, false, pid) }.map_err(|e| std::io::Error::from_raw_os_error(e.code().0 & 0xFFFF))?;
            let handle = unsafe { OwnedHandle::from_raw_handle(h.0 as RawHandle) };
            Ok(Hosted { pid, handle: Arc::new(handle) })
        }

        pub fn raw_handle(&self) -> RawHandle {
            self.handle.as_raw_handle()
        }

        pub async fn wait(&self) -> Option<i32> {
            let h = self.handle.clone();
            let wait = move || {
                unsafe { WaitForSingleObject(api(&h), INFINITE) };
                let mut code = 0u32;
                unsafe { GetExitCodeProcess(api(&h), &mut code) }.ok()?;
                Some(code as i32)
            };
            tokio::task::spawn_blocking(wait).await.ok().flatten()
        }

        pub fn kill(&self) {
            let _ = unsafe { TerminateProcess(api(&self.handle), 1) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_copy_steam_started_starts_the_game_only_on_windows() {
        assert!(hosts_game(true, true));
        assert!(!hosts_game(true, false), "a desktop shortcut hands the game over as before");
        assert!(!hosts_game(false, true), "Linux keeps forwarding Steam's launch context");
        assert!(!hosts_game(false, false));
    }

    #[test]
    fn the_handoff_names_the_host() {
        let host = Host { port: 51234, token: "0123456789abcdef0123456789abcdef".into() };
        let mut args = crate::sysenv::play_handoff_args(&["launcher".into(), "--play".into(), "some-recomp".into()], &|_| None);
        args.push(host.arg());
        assert_eq!(&args[..3], ["--play", "some-recomp", "--handoff"]);
        assert_eq!(Host::from_args(&args), Some(host));
    }

    #[test]
    fn a_host_outside_a_handoff_or_malformed_is_ignored() {
        let good = "--play-host=51234:0123456789abcdef0123456789abcdef".to_string();
        assert!(Host::from_args(&["--play".into(), "x".into(), good]).is_none());
        for bad in ["--play-host=0:0123456789abcdef0123456789abcdef", "--play-host=99999:0123456789abcdef0123456789abcdef", "--play-host=51234:short", "--play-host=51234:0123456789abcdef0123456789abcdeg", "--play-host=51234"] {
            assert!(Host::from_args(&["--handoff".into(), bad.into()]).is_none(), "{bad}");
        }
    }

    #[test]
    fn each_host_has_its_own_token_the_handoff_accepts() {
        let (a, b) = (listen().unwrap(), listen().unwrap());
        assert_ne!(a.host().token, b.host().token);
        let args = ["--handoff".to_string(), a.host().arg()];
        assert_eq!(Host::from_args(&args).as_ref(), Some(a.host()));
    }

    #[test]
    fn a_request_carries_the_program_as_the_launcher_made_it() {
        let mut cmd = std::process::Command::new("game.exe");
        cmd.args(["--fullscreen", "two words"]).current_dir("C:/Games/Game").env("RECOMPLAUNCHER", "1").env_remove("PYTHONHOME");
        let req = Request::of(&cmd, Some(Path::new("C:/data/game-output/game.log")), "tok").unwrap();
        assert_eq!(req.program, "game.exe");
        assert_eq!(req.args, ["--fullscreen", "two words"]);
        assert_eq!(req.cwd.as_deref(), Some("C:/Games/Game"));
        assert!(req.env.contains(&("RECOMPLAUNCHER".into(), Some("1".into()))));
        assert!(req.env.contains(&("PYTHONHOME".into(), None)));
        assert_eq!(req.output.as_deref(), Some("C:/data/game-output/game.log"));
        let again = req.command();
        assert_eq!(again.get_program(), "game.exe");
        assert_eq!(again.get_args().collect::<Vec<_>>(), ["--fullscreen", "two words"]);
        assert_eq!(again.get_current_dir(), Some(Path::new("C:/Games/Game")));
        let envs: Vec<_> = again.get_envs().collect();
        assert!(envs.contains(&(std::ffi::OsStr::new("RECOMPLAUNCHER"), Some(std::ffi::OsStr::new("1")))));
        assert!(envs.contains(&(std::ffi::OsStr::new("PYTHONHOME"), None)));
        let line = serde_json::to_string(&req).unwrap();
        assert_eq!(serde_json::from_str::<Request>(&line).unwrap(), req);
    }

    /// A program that prints RL_HOST_TEST and closes with code 3.
    fn printer() -> std::process::Command {
        if cfg!(windows) {
            let mut c = std::process::Command::new("cmd");
            c.args(["/c", "echo %RL_HOST_TEST%& exit 3"]);
            c
        } else {
            let mut c = std::process::Command::new("sh");
            c.args(["-c", "echo $RL_HOST_TEST; exit 3"]);
            c
        }
    }

    #[test]
    fn the_host_starts_the_game_only_for_its_token() {
        let dir = std::env::temp_dir().join(format!("rl-play-host-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("game.log");
        std::fs::write(&log, b"").unwrap();
        let listening = listen().unwrap();
        let host = listening.host().clone();
        let serving = std::thread::spawn(move || listening.serve());
        let mut cmd = printer();
        cmd.env("RL_HOST_TEST", "from the launcher");
        let wrong = Request::of(&cmd, Some(&log), "ffffffffffffffffffffffffffffffff").unwrap();
        assert!(ask(&host, &wrong).is_err(), "a wrong token gets no answer");
        let right = Request::of(&cmd, Some(&log), &host.token).unwrap();
        let (reply, mut stream) = ask(&host, &right).unwrap();
        assert!(matches!(reply, Reply::Started(pid) if pid != 0), "{reply:?}");
        stream.write_all(b"held\n").unwrap();
        // The host keeps the game until it closes, then stops listening.
        serving.join().unwrap();
        assert_eq!(std::fs::read_to_string(&log).unwrap().trim(), "from the launcher");
        assert!(ask(&host, &right).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_program_that_doesnt_start_says_why() {
        let listening = listen().unwrap();
        let host = listening.host().clone();
        std::thread::spawn(move || listening.serve());
        let cmd = std::process::Command::new("rl-no-such-program-424");
        let (reply, _) = ask(&host, &Request::of(&cmd, None, &host.token).unwrap()).unwrap();
        let Reply::Failed { code, not_found, msg } = reply else { panic!("{reply:?}") };
        assert_eq!(Reply::error(code, not_found, &msg).kind(), std::io::ErrorKind::NotFound);
    }

    #[cfg(windows)]
    #[test]
    fn a_game_the_host_started_is_watched_like_the_launchers_own() {
        let listening = listen().unwrap();
        let host = listening.host().clone();
        std::thread::spawn(move || listening.serve());
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut game = start_through(&host, &printer(), None).unwrap_or_else(|e| panic!("{e:?}"));
        assert!(game.id().is_some_and(|p| p != 0));
        assert!(game.raw_handle().is_some());
        assert_eq!(rt.block_on(game.wait()), Some(3));
    }

    #[cfg(windows)]
    #[test]
    fn an_unreachable_host_leaves_the_start_to_the_launcher() {
        let gone = listen().unwrap().host().clone();
        let r = start_through(&gone, &printer(), None);
        assert!(matches!(r, Err(None)));
    }
}

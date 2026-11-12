//! A game's desktop shortcut, and adding it to Steam as a non-Steam game with its art.
//!
//! Both start RecompLauncher with `--play <id>`, so play time, backups and the quick menu still work.
//! Steam keeps non-Steam games in a binary file (userdata/<user>/config/shortcuts.vdf) that it only
//! reads when it starts, so it has to be closed while the launcher adds to it.

use crate::Error;
use std::fs;
use std::path::{Path, PathBuf};

pub fn launcher_exe() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("recomplauncher"))
}

fn safe_name(s: &str) -> String {
    let s: String = s.chars().map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { ' ' } else { c }).collect();
    s.trim().trim_end_matches('.').to_string()
}

// ---------- desktop ----------

/// A shortcut on the desktop that plays the game. `icon` is the game's program, for its icon.
/// With `direct`, the shortcut starts that program itself (with those arguments), the way a front
/// end like RetroBat or Playnite expects; without it, it goes through the launcher (`--play`).
/// `menu`: in the app menu only (the Start menu on Windows), not on the desktop.
pub fn desktop_shortcut(id: &str, name: &str, icon: Option<&Path>, direct: Option<(&Path, &[String])>, menu: bool) -> Result<PathBuf, Error> {
    let desk = if menu && cfg!(windows) {
        dirs::data_dir().map(|d| d.join(r"Microsoft\Windows\Start Menu\Programs")).ok_or_else(|| Error::Msg("Couldn't find your Start menu folder.".into()))?
    } else {
        dirs::desktop_dir().or_else(|| dirs::home_dir().map(|h| h.join("Desktop"))).ok_or_else(|| Error::Msg("Couldn't find your desktop folder.".into()))?
    };
    // On Linux an AppImage runs from a folder that's gone once it closes: the shortcut starts the AppImage.
    let (launcher, pre) = start_command();
    let (target, args): (PathBuf, String) = match direct {
        Some((exe, a)) => (exe.to_path_buf(), a.iter().map(|x| quote_arg(x)).collect::<Vec<_>>().join(" ")),
        None => (launcher.clone(), with_start_args(&pre, &format!("--play {id}"))),
    };
    #[cfg(windows)]
    {
        let _ = std::fs::create_dir_all(&desk);
        let lnk = desk.join(format!("{}.lnk", safe_name(name)));
        let dir = target.parent().map(Path::to_path_buf).unwrap_or_default();
        let description = if direct.is_some() { format!("Play {name}") } else { format!("Play {name} with RecompLauncher") };
        crate::win::make_shortcut(crate::win::Shortcut { file: lnk.clone(), target: target.clone(), args, dir, icon: icon.map(Path::to_path_buf), description })
            .map_err(|e| Error::Msg(format!("Windows couldn't make the shortcut: {e}")))?;
        Ok(lnk)
    }
    #[cfg(not(windows))]
    {
        let file_name = format!("{}.desktop", safe_name(name).replace(' ', "-"));
        let file = desk.join(&file_name);
        let dir = target.parent().map(Path::to_path_buf).unwrap_or_default();
        // A program isn't an icon on Linux: only a picture is used, else the usual games icon.
        let icon = icon
            .filter(|i| matches!(i.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref(), Some("png" | "svg" | "xpm" | "ico")))
            .map(|i| i.display().to_string())
            .unwrap_or_else(|| "applications-games".into());
        let body = format!(
            "[Desktop Entry]\nType=Application\nName={name}\nExec=\"{}\" {}\nPath={}\nIcon={icon}\nCategories=Game;\nTerminal=false\n",
            target.display(),
            args,
            dir.display(),
        );
        // In the applications menu (where launchers and Steam's Add a Game look), and on the desktop
        // unless it's for the menu only.
        // As a Flatpak, data_dir() is the sandbox's own: the PC's menu reads the real one.
        let apps = if crate::flatpak::sandboxed() { dirs::home_dir().map(|h| h.join(".local/share")) } else { dirs::data_dir() }.map(|d| d.join("applications"));
        if let Some(apps) = &apps {
            if fs::create_dir_all(apps).is_ok() {
                let _ = fs::write(apps.join(&file_name), &body);
            }
        }
        if menu {
            return apps.map(|a| a.join(&file_name)).filter(|p| p.is_file()).ok_or_else(|| Error::Msg("Couldn't add it to your app menu.".into()));
        }
        fs::create_dir_all(&desk)?;
        fs::write(&file, &body)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&file, fs::Permissions::from_mode(0o755))?;
        }
        Ok(file)
    }
}

/// One argument as it goes on a shortcut's command line: in quotes when it has spaces.
fn quote_arg(a: &str) -> String {
    if a.is_empty() || a.contains([' ', '\t']) {
        format!("\"{}\"", a.replace('"', "\\\""))
    } else {
        a.to_string()
    }
}

// ---------- Steam's binary VDF ----------

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Map(Vec<(String, Node)>),
    Str(String),
    Int(i32),
    U64(u64),
}

impl Node {
    pub fn get(&self, k: &str) -> Option<&Node> {
        match self {
            Node::Map(v) => v.iter().find(|(n, _)| n.eq_ignore_ascii_case(k)).map(|(_, n)| n),
            _ => None,
        }
    }
    pub fn str(&self, k: &str) -> &str {
        match self.get(k) {
            Some(Node::Str(s)) => s,
            _ => "",
        }
    }
}

fn read_cstr(b: &[u8], i: &mut usize) -> Option<String> {
    let start = *i;
    while *i < b.len() && b[*i] != 0 {
        *i += 1;
    }
    let s = String::from_utf8_lossy(b.get(start..*i)?).into_owned();
    *i += 1;
    Some(s)
}

fn read_map(b: &[u8], i: &mut usize) -> Option<Vec<(String, Node)>> {
    let mut out = vec![];
    loop {
        let t = *b.get(*i)?;
        *i += 1;
        if t == 0x08 {
            return Some(out);
        }
        let key = read_cstr(b, i)?;
        let val = match t {
            0x00 => Node::Map(read_map(b, i)?),
            0x01 => Node::Str(read_cstr(b, i)?),
            0x02 => {
                let v = i32::from_le_bytes(b.get(*i..*i + 4)?.try_into().ok()?);
                *i += 4;
                Node::Int(v)
            }
            0x07 => {
                let v = u64::from_le_bytes(b.get(*i..*i + 8)?.try_into().ok()?);
                *i += 8;
                Node::U64(v)
            }
            _ => return None,
        };
        out.push((key, val));
    }
}

/// The whole file: one map ("shortcuts") at the top.
pub fn parse_vdf(b: &[u8]) -> Option<Vec<(String, Node)>> {
    let mut i = 0;
    let mut out = vec![];
    while i < b.len() {
        let t = b[i];
        i += 1;
        if t == 0x08 {
            break;
        }
        if t != 0x00 {
            return None;
        }
        let key = read_cstr(b, &mut i)?;
        out.push((key, Node::Map(read_map(b, &mut i)?)));
    }
    Some(out)
}

fn write_map(out: &mut Vec<u8>, m: &[(String, Node)]) {
    for (k, v) in m {
        let put_key = |out: &mut Vec<u8>, t: u8| {
            out.push(t);
            out.extend_from_slice(k.as_bytes());
            out.push(0);
        };
        match v {
            Node::Map(c) => {
                put_key(out, 0x00);
                write_map(out, c);
            }
            Node::Str(s) => {
                put_key(out, 0x01);
                out.extend_from_slice(s.as_bytes());
                out.push(0);
            }
            Node::Int(n) => {
                put_key(out, 0x02);
                out.extend_from_slice(&n.to_le_bytes());
            }
            Node::U64(n) => {
                put_key(out, 0x07);
                out.extend_from_slice(&n.to_le_bytes());
            }
        }
    }
    out.push(0x08);
}

pub fn write_vdf(top: &[(String, Node)]) -> Vec<u8> {
    let mut out = vec![];
    write_map(&mut out, top);
    out
}

/// The id Steam gives a non-Steam game, the same way Steam ROM Manager works it out: its art files
/// are named after it.
pub fn steam_app_id(exe: &str, name: &str) -> u32 {
    crc32fast::hash(format!("{exe}{name}").as_bytes()) | 0x8000_0000
}

/// Add (or update) one game in a shortcuts.vdf's content. Returns the new content and the app id.
pub fn add_shortcut(old: Option<&[u8]>, name: &str, exe: &str, start_dir: &str, launch: &str, icon: &str) -> (Vec<u8>, u32) {
    let mut top = old.and_then(parse_vdf).unwrap_or_default();
    if !top.iter().any(|(k, _)| k.eq_ignore_ascii_case("shortcuts")) {
        top = vec![("shortcuts".into(), Node::Map(vec![]))];
    }
    let quoted = format!("\"{exe}\"");
    let id = steam_app_id(&quoted, name);
    let entry = Node::Map(vec![
        ("appid".into(), Node::Int(id as i32)),
        ("AppName".into(), Node::Str(name.into())),
        ("Exe".into(), Node::Str(quoted.clone())),
        ("StartDir".into(), Node::Str(format!("\"{start_dir}\""))),
        ("icon".into(), Node::Str(icon.into())),
        ("ShortcutPath".into(), Node::Str(String::new())),
        ("LaunchOptions".into(), Node::Str(launch.into())),
        ("IsHidden".into(), Node::Int(0)),
        ("AllowDesktopConfig".into(), Node::Int(1)),
        ("AllowOverlay".into(), Node::Int(1)),
        ("OpenVR".into(), Node::Int(0)),
        ("Devkit".into(), Node::Int(0)),
        ("DevkitGameID".into(), Node::Str(String::new())),
        ("DevkitOverrideAppID".into(), Node::Int(0)),
        ("LastPlayTime".into(), Node::Int(0)),
        ("FlatpakAppID".into(), Node::Str(String::new())),
        ("tags".into(), Node::Map(vec![("0".into(), Node::Str("RecompLauncher".into()))])),
    ]);
    for (k, v) in top.iter_mut() {
        if !k.eq_ignore_ascii_case("shortcuts") {
            continue;
        }
        if let Node::Map(list) = v {
            // Already there (same program and options): replace it so the name and art stay right.
            // Or the same game added the older way (the launcher with --play, before its launch
            // script on Linux): replaced too, so it isn't there twice.
            let same_game = |e: &Node| e.str("AppName") == name && e.str("LaunchOptions").contains("--play ") && launch.is_empty();
            if let Some(slot) = list.iter_mut().find(|(_, e)| (e.str("Exe") == quoted && e.str("LaunchOptions") == launch) || same_game(e)) {
                slot.1 = entry;
            } else {
                let next = list.iter().filter_map(|(k, _)| k.parse::<usize>().ok()).max().map(|n| n + 1).unwrap_or(0);
                list.push((next.to_string(), entry));
            }
        }
        break;
    }
    (write_vdf(&top), id)
}

// ---------- finding and restarting Steam ----------

pub fn steam_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if let Some(p) = crate::win::reg_str(crate::win::HKCU, r"Software\Valve\Steam", "SteamPath") {
            let p = PathBuf::from(p.trim().replace('/', "\\"));
            if p.join("userdata").is_dir() {
                return Some(p);
            }
        }
        let p = PathBuf::from(r"C:\Program Files (x86)\Steam");
        return p.join("userdata").is_dir().then_some(p);
    }
    #[cfg(not(windows))]
    {
        let home = dirs::home_dir()?;
        [".steam/steam", ".local/share/Steam", ".var/app/com.valvesoftware.Steam/data/Steam"].iter().map(|p| home.join(p)).find(|p| p.join("userdata").is_dir())
    }
}

/// The Steam accounts that have signed in on this PC (folders under userdata).
pub fn steam_users(steam: &Path) -> Vec<PathBuf> {
    fs::read_dir(steam.join("userdata"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.file_name().and_then(|n| n.to_str()).map(|n| n.chars().all(|c| c.is_ascii_digit()) && n != "0").unwrap_or(false))
        .collect()
}

pub fn steam_running() -> bool {
    #[cfg(windows)]
    {
        crate::win::is_running("steam.exe")
    }
    #[cfg(not(windows))]
    {
        // Steam's own process (named "steam", or its ubuntu12_32/steam program, as SteamOS starts
        // it), or its pid file pointing at a process that's still there. Any of them still open
        // means Steam can still write its shortcuts back over the new one on the way out.
        // The sandbox sees none of the PC's processes. Steam keeps its command pipe open to read
        // from while it runs, so a write end opens only then (no reader: "no such device").
        #[cfg(target_os = "linux")]
        if crate::flatpak::sandboxed() {
            let home = dirs::home_dir().unwrap_or_default();
            return steam_pipes(&home).iter().any(|p| pipe_has_reader(p));
        }
        let pid_live = dirs::home_dir()
            .and_then(|h| fs::read_to_string(h.join(".steam/steam.pid")).ok())
            .and_then(|p| p.trim().parse::<u32>().ok())
            .map(|p| fs::read_to_string(format!("/proc/{p}/cmdline")).map(|c| steam_process("", &c)).unwrap_or(false))
            .unwrap_or(false);
        pid_live
            || fs::read_dir("/proc").into_iter().flatten().flatten().any(|e| {
                let name = e.file_name();
                let Some(n) = name.to_str().filter(|n| n.bytes().all(|b| b.is_ascii_digit())) else { return false };
                let comm = fs::read_to_string(format!("/proc/{n}/comm")).unwrap_or_default();
                let cmd = fs::read_to_string(format!("/proc/{n}/cmdline")).unwrap_or_default();
                steam_process(comm.trim(), &cmd)
            })
    }
}

/// A named pipe somebody is reading from. Opened for writing without waiting, which a pipe nobody
/// reads refuses (ENXIO) and nothing is written.
#[cfg(target_os = "linux")]
fn pipe_has_reader(pipe: &Path) -> bool {
    use std::os::unix::fs::OpenOptionsExt;
    // Linux's O_NONBLOCK (the same on x86_64 and aarch64).
    const O_NONBLOCK: i32 = 0o4000;
    // Anything but a reader being there (no pipe, no reader, no access) counts as Steam not running.
    fs::OpenOptions::new().write(true).custom_flags(O_NONBLOCK).open(pipe).is_ok()
}

/// Steam's command pipe: the PC's own Steam, and Steam from Flathub (its home is its ~/.var/app folder).
#[cfg(any(target_os = "linux", test))]
fn steam_pipes(home: &Path) -> [PathBuf; 2] {
    [home.join(".steam/steam.pipe"), home.join(".var/app/com.valvesoftware.Steam/.steam/steam.pipe")]
}

/// Is this process Steam itself (its comm name and command line)? Not the launcher's own children,
/// helpers or the steam-runtime that games run in.
#[cfg(not(windows))]
fn steam_process(comm: &str, cmdline: &str) -> bool {
    let prog = cmdline.split('\0').next().unwrap_or("");
    comm == "steam" || prog.ends_with("ubuntu12_32/steam") || prog.ends_with("/steam/steam") && !prog.contains("steam-runtime")
}

#[cfg(windows)]
fn steam_exe(steam: &Path) -> PathBuf {
    steam.join("steam.exe")
}

/// Steam can't be closed and opened again from here: the launcher was started by Steam (closing
/// Steam would take the launcher's window down with it), or it's a Steam Deck's Gaming Mode
/// (Steam is the whole screen there and starts again at once). What to do instead, then.
pub fn cant_restart_steam() -> Option<&'static str> {
    #[cfg(target_os = "linux")]
    {
        let gaming_mode = std::env::var("XDG_CURRENT_DESKTOP").map(|d| d.eq_ignore_ascii_case("gamescope")).unwrap_or(false) || std::env::var_os("GAMESCOPE_WAYLAND_DISPLAY").is_some() && std::env::var_os("SteamDeck").is_some();
        if gaming_mode {
            return Some("Steam can't restart in Gaming Mode. Switch to Desktop Mode, open RecompLauncher from the desktop, then press Add to Steam.");
        }
        if started_by_steam() {
            return Some("RecompLauncher was opened from Steam, so closing Steam would close it too. Open RecompLauncher from your desktop or app menu instead, then press Add to Steam.");
        }
        None
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// Is this launcher a program Steam started (its process, or one above it, is Steam's)?
#[cfg(target_os = "linux")]
fn started_by_steam() -> bool {
    if ["SteamGameId", "SteamAppId", "SteamClientLaunch", "STEAM_COMPAT_CLIENT_INSTALL_PATH"].iter().any(|k| std::env::var_os(k).is_some()) {
        return true;
    }
    let mut pid = std::process::id();
    for _ in 0..30 {
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else { break };
        // pid (comm) state ppid ...: the name can hold spaces, so after the last ')'.
        let Some(after) = stat.rfind(')').map(|i| &stat[i + 1..]) else { break };
        let Some(ppid) = after.split_whitespace().nth(1).and_then(|p| p.parse::<u32>().ok()) else { break };
        if ppid <= 1 {
            break;
        }
        let comm = std::fs::read_to_string(format!("/proc/{ppid}/comm")).unwrap_or_default();
        let comm = comm.trim();
        if comm == "steam" || comm == "reaper" || comm == "steamwebhelper" || comm.starts_with("steam-runtime") || comm == "pressure-vessel" {
            return true;
        }
        pid = ppid;
    }
    false
}

/// Ask Steam to close, and wait (up to 30 s) until it has.
/// On Linux, how to talk to Steam itself: its own command (the Steam Deck, most installs), the
/// Flatpak, or failing both its steam:// link. The link alone can land in another program (a
/// software center) on some desktops, and then Steam never closes.
#[cfg(not(windows))]
fn steam_cmd(steam: &Path, args: &[&str], link: &str) -> std::process::Command {
    let flatpak = steam.to_string_lossy().contains("/.var/app/com.valvesoftware.Steam");
    let on_path = |name: &str| std::env::var_os("PATH").map(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file())).unwrap_or(false);
    let mut c = if flatpak && on_path("flatpak") {
        let mut c = std::process::Command::new("flatpak");
        c.args(["run", "com.valvesoftware.Steam"]).args(args);
        c
    } else if on_path("steam") {
        let mut c = std::process::Command::new("steam");
        c.args(args);
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(link);
        c
    };
    crate::sysenv::clean_std(&mut c);
    // Its own process group: Steam keeps running when the launcher closes.
    use std::os::unix::process::CommandExt;
    c.process_group(0);
    c.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    c
}

pub async fn close_steam(steam: &Path) -> Result<(), Error> {
    if !steam_running() {
        return Ok(());
    }
    #[cfg(windows)]
    let mut c = {
        let mut c = std::process::Command::new(steam_exe(steam));
        crate::sysenv::clean_std(&mut c);
        c.arg("-shutdown");
        c
    };
    #[cfg(not(windows))]
    let mut c = steam_cmd(steam, &["-shutdown"], "steam://exit");
    if let Ok(mut ch) = c.spawn() {
        std::thread::spawn(move || {
            let _ = ch.wait();
        });
    }
    for _ in 0..60 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        if !steam_running() {
            // Steam writes its files on the way out; give it a moment.
            tokio::time::sleep(std::time::Duration::from_millis(3000)).await;
            return Ok(());
        }
    }
    Err(Error::Msg("Steam didn't close. Close it yourself, then try again.".into()))
}

pub fn start_steam(steam: &Path) {
    #[cfg(windows)]
    let mut c = {
        let mut c = std::process::Command::new(steam_exe(steam));
        crate::sysenv::clean_std(&mut c);
        c
    };
    #[cfg(not(windows))]
    let mut c = steam_cmd(steam, &[], "steam://open/main");
    if let Ok(mut ch) = c.spawn() {
        std::thread::spawn(move || {
            let _ = ch.wait();
        });
    }
}

/// Write the game into every Steam account's shortcuts and art folder. `art` is (suffix, bytes,
/// extension): "p" the poster, "_hero" the wide banner, "" the small wide header, "_logo".
pub fn write_to_steam(steam: &Path, name: &str, id: &str, icon: &str, art: &[(&str, Vec<u8>, &str)], data: &Path) -> Result<usize, Error> {
    // On Linux from the AppImage, Steam starts a small script for the game instead (see
    // steam_script): the AppImage would drop what Steam's overlay and capture need.
    if let Some(script) = steam_script(data, name, id) {
        return write_entry_with(steam, name, (script.clone(), vec![]), "", icon, art);
    }
    write_entry(steam, name, &format!("--play {id}"), icon, art)
}

/// Linux, AppImage: the AppImage runtime drops LD_PRELOAD and Vulkan's layer variables on its way
/// in, so Steam's overlay never reaches the game, and Steam Link and Remote Play find no window to
/// stream (a black screen). The shortcut runs this script, which keeps Steam's overlay aside in
/// RL_STEAM_OVERLAY (the launcher hands it to the game) and then asks the launcher to play it.
/// None elsewhere (Windows, a Flatpak, a .deb run): the shortcut starts the launcher as before.
pub fn steam_script(data: &Path, name: &str, id: &str) -> Option<PathBuf> {
    let appimage = steam_appimage()?;
    write_steam_script(data, name, id, &appimage).ok()
}

/// The AppImage a game's launch script starts, when Steam starts games through one (see
/// steam_script).
fn steam_appimage() -> Option<PathBuf> {
    if !cfg!(target_os = "linux") || crate::flatpak::sandboxed() {
        return None;
    }
    std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|p| p.is_file())
}

pub fn steam_script_path(data: &Path, id: &str) -> PathBuf {
    data.join("steam-launch").join(format!("{}.sh", crate::library::folder_name(id)))
}

/// The game a launch script in the launcher's data folder plays. None for any other program.
pub fn script_game(data: &Path, exe: &str) -> Option<String> {
    let path = Path::new(exe);
    if !path.starts_with(data.join("steam-launch")) {
        return None;
    }
    let body = fs::read_to_string(path).ok()?;
    script_body_game(&body)
}

fn script_body_game(body: &str) -> Option<String> {
    let exec = body.lines().find(|l| l.starts_with("exec "))?;
    let (_, after) = exec.split_once(" --play '")?;
    let (id, _) = after.split_once('\'')?;
    (!id.is_empty()).then(|| id.to_string())
}

/// How Steam starts one game, as write_to_steam adds it: the program, its launch options, and on
/// Linux the AppImage its launch script starts.
pub struct GameStart {
    pub exe: PathBuf,
    pub launch: String,
    pub appimage: Option<PathBuf>,
}

pub fn game_start(data: &Path, id: &str) -> GameStart {
    if let Some(appimage) = steam_appimage() {
        return GameStart { exe: steam_script_path(data, id), launch: String::new(), appimage: Some(appimage) };
    }
    let (exe, pre) = start_command();
    GameStart { exe, launch: with_start_args(&pre, &format!("--play {id}")), appimage: None }
}

pub fn write_steam_script(data: &Path, name: &str, id: &str, appimage: &Path) -> std::io::Result<PathBuf> {
    let file = steam_script_path(data, id);
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    let q = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    let body = format!(
        "#!/bin/sh\n# {name} from Steam, through RecompLauncher. Made by the launcher; it's made again when the game\n# is added to Steam again.\n# The AppImage drops Steam's LD_PRELOAD and Vulkan layer variables on its way in, so Steam's\n# overlay is kept aside here for the game (Steam Link and Remote Play find its window through it).\nRL_STEAM_OVERLAY=\"$(printf '%s' \"$LD_PRELOAD\" | tr ': ' '\\n\\n' | grep gameoverlayrenderer | tr '\\n' ' ')\"\nexport RL_STEAM_OVERLAY\nexec {} --play {} \"$@\"\n",
        q(&appimage.to_string_lossy()),
        q(id),
        name = name.replace(['\n', '\r'], " "),
    );
    fs::write(&file, body)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o755))?;
    }
    Ok(file)
}

/// RecompLauncher itself in Steam, opening in Big Picture mode (for a Steam Deck's Gaming Mode, or
/// Steam's own Big Picture).
pub fn launcher_to_steam(steam: &Path, art: &[(&str, Vec<u8>, &str)]) -> Result<usize, Error> {
    write_entry(steam, "RecompLauncher", "--bigpicture", &launcher_icon(), art)
}

/// How a shortcut starts the launcher: the program and the arguments that go before its own.
/// Run as a Flatpak, the program inside the sandbox isn't on the PC at all: a shortcut starts it
/// with `flatpak run <app id>`.
fn start_command() -> (PathBuf, Vec<String>) {
    if crate::flatpak::sandboxed() {
        if let Ok(id) = std::env::var("FLATPAK_ID") {
            return (PathBuf::from("/usr/bin/flatpak"), vec!["run".into(), id]);
        }
    }
    (start_path(), vec![])
}

/// `args` after the arguments `start_command` puts first, as one command line.
fn with_start_args(pre: &[String], args: &str) -> String {
    pre.iter().map(|a| quote_arg(a)).chain((!args.is_empty()).then(|| args.to_string())).collect::<Vec<_>>().join(" ")
}

/// The launcher's picture for Steam. As a Flatpak, the copy the PC's own icon folder holds (the
/// sandbox's /app doesn't exist outside it).
fn launcher_icon() -> String {
    if crate::flatpak::sandboxed() {
        if let Some(icon) = flatpak_icon() {
            return icon;
        }
    }
    launcher_exe().to_string_lossy().to_string()
}

fn flatpak_icon() -> Option<String> {
    let id = std::env::var("FLATPAK_ID").ok()?;
    let info = fs::read_to_string("/.flatpak-info").ok()?;
    icon_in(&info, &id)
}

/// `app-path` in /.flatpak-info is <installation>/app/<id>/<arch>/<branch>/<commit>/files; the
/// installation's exports folder has the icon.
fn icon_in(info: &str, id: &str) -> Option<String> {
    let app_path = info.lines().find_map(|l| l.trim().strip_prefix("app-path="))?;
    let root = app_path.split(&format!("/app/{id}/")).next().filter(|r| r.len() < app_path.len())?;
    Some(format!("{root}/exports/share/icons/hicolor/512x512/apps/{id}.png"))
}

/// The program Steam should start: the AppImage itself on Linux when it's one, else this program.
fn start_path() -> PathBuf {
    std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|p| p.is_file()).unwrap_or_else(launcher_exe)
}

/// Is this a Steam Deck? Its firmware says so (Jupiter is the LCD model, Galileo the OLED).
pub fn is_steam_deck() -> bool {
    if !cfg!(target_os = "linux") {
        return false;
    }
    let read = |f: &str| fs::read_to_string(format!("/sys/devices/virtual/dmi/id/{f}")).unwrap_or_default().trim().to_lowercase();
    matches!(read("product_name").as_str(), "jupiter" | "galileo") || read("board_vendor") == "valve"
}

/// The handheld this is, by name, from the maker's product name (DMI on Linux, the BIOS keys in the
/// registry on Windows): None on a normal PC. `cpu` catches handheld chips under another name.
pub fn handheld_name(cpu: &str) -> Option<&'static str> {
    let (product, vendor) = dmi();
    let named = device_of(&product, &vendor);
    if named.is_some() {
        return named;
    }
    is_handheld(cpu).then_some("Other handheld")
}

fn device_of(product: &str, vendor: &str) -> Option<&'static str> {
    let p = product.to_lowercase();
    let v = vendor.to_lowercase();
    Some(match () {
        _ if p == "jupiter" => "Steam Deck LCD",
        _ if p == "galileo" => "Steam Deck OLED",
        _ if v == "valve" => "Steam Deck",
        _ if p.contains("rc72l") || p.contains("ally x") => "ROG Ally X",
        _ if p.contains("rc71l") || p.contains("rog ally") => "ROG Ally",
        // Lenovo names them by machine type: 83E1 is the Legion Go, 83L3/83N6/83Q2/83Q3 the Go S.
        _ if ["83l3", "83n6", "83q2", "83q3"].iter().any(|m| p.starts_with(m)) || p.contains("legion go s") => "Legion Go S",
        _ if p.starts_with("83e1") || p.contains("legion go") => "Legion Go",
        _ if p.contains("claw") && (v.contains("micro-star") || v.contains("msi")) => "MSI Claw",
        _ => return None,
    })
}

#[cfg(target_os = "linux")]
fn dmi() -> (String, String) {
    let read = |f: &str| fs::read_to_string(format!("/sys/devices/virtual/dmi/id/{f}")).unwrap_or_default().trim().to_string();
    let vendor = read("board_vendor");
    (read("product_name"), if vendor.is_empty() { read("sys_vendor") } else { vendor })
}

#[cfg(windows)]
fn dmi() -> (String, String) {
    use crate::win::{reg_str, HKLM};
    let get = |n: &str| reg_str(HKLM, r"HARDWARE\DESCRIPTION\System\BIOS", n).unwrap_or_default();
    let vendor = get("BaseBoardManufacturer");
    (get("SystemProductName"), if vendor.is_empty() { get("SystemManufacturer") } else { vendor })
}

#[cfg(not(any(windows, target_os = "linux")))]
fn dmi() -> (String, String) {
    (String::new(), String::new())
}

/// A handheld PC: a Steam Deck, or one of the handheld chips (ROG Ally, Legion Go, MSI Claw A8).
pub fn is_handheld(cpu: &str) -> bool {
    let c = cpu.to_lowercase();
    is_steam_deck() || c.contains("custom apu 0405") || c.contains("custom apu 0932") || c.contains(" z1") || c.contains(" z2")
}

fn write_entry(steam: &Path, name: &str, launch: &str, icon: &str, art: &[(&str, Vec<u8>, &str)]) -> Result<usize, Error> {
    write_entry_with(steam, name, start_command(), launch, icon, art)
}

fn write_entry_with(steam: &Path, name: &str, start: (PathBuf, Vec<String>), launch: &str, icon: &str, art: &[(&str, Vec<u8>, &str)]) -> Result<usize, Error> {
    let (exe, pre) = start;
    let exe_s = exe.to_string_lossy().to_string();
    let dir = exe.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    let launch = with_start_args(&pre, launch);
    let launch = launch.as_str();
    let users = steam_users(steam);
    if users.is_empty() {
        return Err(Error::Msg("Sign in to Steam once on this PC first.".into()));
    }
    let mut done = 0;
    for u in &users {
        let cfg = u.join("config");
        fs::create_dir_all(&cfg)?;
        let file = cfg.join("shortcuts.vdf");
        let old = fs::read(&file).ok();
        if let Some(o) = &old {
            // Something in it the launcher can't read: leave the player's shortcuts alone rather
            // than write a file with only this game in it.
            if !o.is_empty() && parse_vdf(o).is_none() {
                continue;
            }
            keep_backup(&cfg, o);
        }
        let (bytes, app_id) = add_shortcut(old.as_deref(), name, &exe_s, &dir, launch, icon);
        fs::write(&file, bytes)?;
        save_art(&cfg, app_id, art)?;
        done += 1;
    }
    if done == 0 {
        return Err(Error::Msg("Steam's list of non-Steam games has something in it the launcher can't read, so it left it as it is. Add the game from Steam instead: Add a Game, then Add a Non-Steam Game.".into()));
    }
    Ok(done)
}

/// The shortcuts file as it was before the launcher first touched it stays as the backup.
pub fn keep_backup(cfg: &Path, old: &[u8]) {
    let backup = cfg.join("shortcuts.vdf.recomplauncher-backup");
    if !backup.exists() {
        let _ = fs::write(backup, old);
    }
}

const ART_EXTS: [&str; 4] = ["png", "jpg", "jpeg", "webp"];
const ART_SUFFIXES: [&str; 4] = ["p", "_hero", "", "_logo"];

/// A game's pictures in an account's grid folder, named after its app id.
pub fn save_art(cfg: &Path, app_id: u32, art: &[(&str, Vec<u8>, &str)]) -> std::io::Result<()> {
    let grid = cfg.join("grid");
    fs::create_dir_all(&grid)?;
    for (suffix, data, ext) in art {
        if data.is_empty() {
            continue;
        }
        // The same picture saved before in another format would be the one Steam shows.
        for old in ART_EXTS {
            let _ = fs::remove_file(grid.join(format!("{app_id}{suffix}.{old}")));
        }
        fs::write(grid.join(format!("{app_id}{suffix}.{ext}")), data)?;
    }
    Ok(())
}

/// The pictures save_art put there, for a game that left Steam.
pub fn remove_art(cfg: &Path, app_id: u32) {
    let grid = cfg.join("grid");
    for suffix in ART_SUFFIXES {
        for ext in ART_EXTS {
            let _ = fs::remove_file(grid.join(format!("{app_id}{suffix}.{ext}")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flatpak_starts_with_flatpak_run() {
        let pre = vec!["run".to_string(), "com.recomplauncher.RecompLauncher".to_string()];
        assert_eq!(with_start_args(&pre, "--play abc"), "run com.recomplauncher.RecompLauncher --play abc");
        assert_eq!(with_start_args(&[], "--bigpicture"), "--bigpicture");
        assert_eq!(with_start_args(&pre, ""), "run com.recomplauncher.RecompLauncher");
    }

    #[test]
    fn the_launcher_icon_outside_the_sandbox() {
        let id = "com.recomplauncher.RecompLauncher";
        let user = "[Instance]
app-path=/home/a/.local/share/flatpak/app/com.recomplauncher.RecompLauncher/x86_64/master/0f3a/files
";
        assert_eq!(icon_in(user, id).as_deref(), Some("/home/a/.local/share/flatpak/exports/share/icons/hicolor/512x512/apps/com.recomplauncher.RecompLauncher.png"));
        let system = "[Instance]
app-path=/var/lib/flatpak/app/com.recomplauncher.RecompLauncher/x86_64/stable/9/files
";
        assert!(icon_in(system, id).unwrap().starts_with("/var/lib/flatpak/exports/"));
        assert_eq!(icon_in("[Instance]
", id), None);
        assert_eq!(icon_in("app-path=/somewhere/else/files
", id), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn no_pipe_no_reader() {
        assert!(!pipe_has_reader(Path::new("/nonexistent/steam.pipe")));
    }

    #[test]
    fn both_steams_pipes() {
        let [own, flathub] = steam_pipes(Path::new("/home/a"));
        assert_eq!(own, Path::new("/home/a/.steam/steam.pipe"));
        assert_eq!(flathub, Path::new("/home/a/.var/app/com.valvesoftware.Steam/.steam/steam.pipe"));
    }

    #[test]
    fn knows_handhelds_by_name() {
        assert_eq!(device_of("Jupiter", "Valve"), Some("Steam Deck LCD"));
        assert_eq!(device_of("Galileo", "Valve"), Some("Steam Deck OLED"));
        assert_eq!(device_of("ROG Ally RC71L_RC71L", "ASUSTeK COMPUTER INC."), Some("ROG Ally"));
        assert_eq!(device_of("ROG Ally X RC72LA_RC72LA", "ASUSTeK COMPUTER INC."), Some("ROG Ally X"));
        assert_eq!(device_of("83E1", "LENOVO"), Some("Legion Go"));
        assert_eq!(device_of("83L3", "LENOVO"), Some("Legion Go S"));
        assert_eq!(device_of("Claw A1M", "Micro-Star International Co., Ltd."), Some("MSI Claw"));
        assert_eq!(device_of("MS-7C91", "Micro-Star International Co., Ltd."), None);
        assert_eq!(device_of("To Be Filled By O.E.M.", "ASRock"), None);
    }

    #[test]
    fn vdf_round_trip_and_add() {
        let (first, id) = add_shortcut(None, "Zelda 64: Recompiled", r"C:\RL\recomplauncher.exe", r"C:\RL", "--play zelda64recomp", "");
        let top = parse_vdf(&first).unwrap();
        assert_eq!(write_vdf(&top), first);
        let Node::Map(list) = &top[0].1 else { panic!() };
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].1.str("AppName"), "Zelda 64: Recompiled");
        assert!(id & 0x8000_0000 != 0);
        // A second game goes in as entry "1"; adding the first again doesn't duplicate it.
        let (two, _) = add_shortcut(Some(&first), "Banjo", r"C:\RL\recomplauncher.exe", r"C:\RL", "--play banjo", "");
        let (again, _) = add_shortcut(Some(&two), "Zelda 64: Recompiled", r"C:\RL\recomplauncher.exe", r"C:\RL", "--play zelda64recomp", "");
        let Node::Map(list) = &parse_vdf(&again).unwrap()[0].1 else { panic!() };
        assert_eq!(list.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), vec!["0", "1"]);
    }

    #[test]
    fn keeps_other_programs_entries() {
        // Entries written by Steam itself, with a 64-bit field the launcher doesn't use.
        let other = vec![("shortcuts".to_string(), Node::Map(vec![("0".into(), Node::Map(vec![("appid".into(), Node::Int(-5)), ("AppName".into(), Node::Str("Other".into())), ("Exe".into(), Node::Str("\"x.exe\"".into())), ("Big".into(), Node::U64(1 << 40))]))]))];
        let bytes = write_vdf(&other);
        let (out, _) = add_shortcut(Some(&bytes), "Banjo", "rl.exe", ".", "--play banjo", "");
        let Node::Map(list) = &parse_vdf(&out).unwrap()[0].1 else { panic!() };
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].1.str("AppName"), "Other");
        assert_eq!(list[0].1.get("Big"), Some(&Node::U64(1 << 40)));
    }

    #[test]
    fn a_launch_script_says_which_game_it_plays() {
        let d = std::env::temp_dir().join(format!("rl-steam-script-game-{}", std::process::id()));
        let f = write_steam_script(&d, "Game", "cogbox--gow2", Path::new("/apps/RL.AppImage")).unwrap();
        assert_eq!(script_game(&d, &f.to_string_lossy()).as_deref(), Some("cogbox--gow2"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_program_outside_the_launch_scripts_folder_is_no_game() {
        assert_eq!(script_game(Path::new("/data"), "/usr/bin/dolphin-emu"), None);
    }

    #[test]
    fn steam_starts_a_script_that_keeps_its_overlay_for_the_game() {
        let d = std::env::temp_dir().join(format!("rl-steam-script-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        let f = write_steam_script(&d, "It's a Game", "metroidprime2port", Path::new("/home/me/Apps/Recomp Launcher's.AppImage")).unwrap();
        let body = fs::read_to_string(&f).unwrap();
        assert!(body.starts_with("#!/bin/sh\n"));
        assert!(body.contains("grep gameoverlayrenderer"));
        assert!(body.contains("export RL_STEAM_OVERLAY"));
        // Quoted for the shell, even with a space and an apostrophe in the path.
        assert!(body.contains("exec '/home/me/Apps/Recomp Launcher'\\''s.AppImage' --play 'metroidprime2port' \"$@\""), "{body}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o755);
            // It runs: the overlay out of a mixed preload list is what reaches the program.
            let fake = d.join("fake.sh");
            fs::write(&fake, "#!/bin/sh\necho \"$RL_STEAM_OVERLAY|$1|$2\"\n").unwrap();
            fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
            let s2 = write_steam_script(&d, "Game", "g", &fake).unwrap();
            let out = std::process::Command::new(&s2).env("LD_PRELOAD", "/x/libfoo.so:/steam/ubuntu12_64/gameoverlayrenderer.so").output().unwrap();
            assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "/steam/ubuntu12_64/gameoverlayrenderer.so |--play|g");
        }
        // Added again the new way, the older entry for the same game is replaced, not doubled.
        let (old, _) = add_shortcut(None, "Game", "/apps/RL.AppImage", "/apps", "--play g", "");
        let (both, _) = add_shortcut(Some(&old), "Game", &f.to_string_lossy(), "/x", "", "");
        let top = parse_vdf(&both).unwrap();
        let Node::Map(list) = &top[0].1 else { panic!() };
        assert_eq!(list.len(), 1);
        let _ = fs::remove_dir_all(&d);
    }
}

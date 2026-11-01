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
    let launcher = start_path();
    let (target, args): (PathBuf, String) = match direct {
        Some((exe, a)) => (exe.to_path_buf(), a.iter().map(|x| quote_arg(x)).collect::<Vec<_>>().join(" ")),
        None => (launcher.clone(), format!("--play {id}")),
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
        let apps = dirs::data_dir().map(|d| d.join("applications"));
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
            if let Some(slot) = list.iter_mut().find(|(_, e)| e.str("Exe") == quoted && e.str("LaunchOptions") == launch) {
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
        let mut c = std::process::Command::new("pgrep");
        crate::sysenv::clean_std(&mut c);
        c.args(["-x", "steam"]).output().map(|o| o.status.success()).unwrap_or(false)
    }
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
            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
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
pub fn write_to_steam(steam: &Path, name: &str, id: &str, icon: &str, art: &[(&str, Vec<u8>, &str)]) -> Result<usize, Error> {
    write_entry(steam, name, &format!("--play {id}"), icon, art)
}

/// RecompLauncher itself in Steam, opening in Big Picture mode (for a Steam Deck's Gaming Mode, or
/// Steam's own Big Picture).
pub fn launcher_to_steam(steam: &Path, art: &[(&str, Vec<u8>, &str)]) -> Result<usize, Error> {
    let icon = launcher_exe().to_string_lossy().to_string();
    write_entry(steam, "RecompLauncher", "--bigpicture", &icon, art)
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

/// A handheld PC: a Steam Deck, or one of the handheld chips (ROG Ally, Legion Go, MSI Claw A8).
pub fn is_handheld(cpu: &str) -> bool {
    let c = cpu.to_lowercase();
    is_steam_deck() || c.contains("custom apu 0405") || c.contains("custom apu 0932") || c.contains(" z1") || c.contains(" z2")
}

fn write_entry(steam: &Path, name: &str, launch: &str, icon: &str, art: &[(&str, Vec<u8>, &str)]) -> Result<usize, Error> {
    let exe = start_path();
    let exe_s = exe.to_string_lossy().to_string();
    let dir = exe.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
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
            // The file as it was before the launcher first touched it stays as the backup.
            let backup = cfg.join("shortcuts.vdf.recomplauncher-backup");
            if !backup.exists() {
                let _ = fs::write(backup, o);
            }
        }
        let (bytes, app_id) = add_shortcut(old.as_deref(), name, &exe_s, &dir, launch, icon);
        fs::write(&file, bytes)?;
        let grid = cfg.join("grid");
        fs::create_dir_all(&grid)?;
        for (suffix, data, ext) in art {
            if !data.is_empty() {
                fs::write(grid.join(format!("{app_id}{suffix}.{ext}")), data)?;
            }
        }
        done += 1;
    }
    if done == 0 {
        return Err(Error::Msg("Steam's list of non-Steam games has something in it the launcher can't read, so it left it as it is. Add the game from Steam instead: Add a Game, then Add a Non-Steam Game.".into()));
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

//! Where a game's own files are (the player's copy), for Recomp Achievements: from the game's own
//! settings, and from what the running game has open. Recomps keep the player's copy wherever the
//! player put it: next to the exe, picked in the game's own file picker and saved in its settings
//! (a .toml, .ini, .json, even a .txt), or handed over on the command line. Once the game runs,
//! whatever it is, it has that copy open: a ReXGlue game maps the disc image (or each file it reads
//! from a folder) into memory, other engines open the files. Only paths are looked at, never file
//! contents beyond the settings files' text; nothing here is sent anywhere.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const SETTINGS_EXT: [&str; 10] = ["toml", "ini", "json", "txt", "cfg", "conf", "yaml", "yml", "xml", "config"];
const SETTINGS_MAX: u64 = 1 << 20;

// ---------- the game's own settings ----------

/// Paths written in a settings file's text: drive paths (C:\..., C:/...), and on Linux and Mac
/// absolute paths, with the doubled backslashes of TOML and JSON strings undone.
pub fn paths_in_text(text: &str) -> Vec<PathBuf> {
    let mut out = vec![];
    let b = text.as_bytes();
    // Commas are fine in names ("(USA, Europe).iso"): a path ends at a quote, a line end, or a
    // character no Windows path can hold.
    let ends = |c: u8| matches!(c, b'"' | b'\'' | b'\r' | b'\n' | b'<' | b'>' | b'|' | b'*' | b'?' | b'\t' | 0);
    let mut i = 0;
    while i + 3 <= b.len() {
        let drive = b[i].is_ascii_alphabetic() && b[i + 1] == b':' && (b[i + 2] == b'\\' || b[i + 2] == b'/') && (i == 0 || !b[i - 1].is_ascii_alphanumeric());
        let unix = cfg!(not(windows)) && b[i] == b'/' && b[i + 1].is_ascii_alphanumeric() && (i == 0 || matches!(b[i - 1], b'"' | b'\'' | b'=' | b' ' | b'\n' | b'\t'));
        if drive || unix {
            let mut j = i;
            while j < b.len() && !ends(b[j]) {
                j += 1;
            }
            let raw = text[i..j].trim_end();
            let clean = raw.replace("\\\\", "\\");
            if clean.len() > 3 {
                out.push(PathBuf::from(clean));
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    out
}

fn settings_files_in(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten().take(500) {
        let p = e.path();
        let Ok(ty) = e.file_type() else { continue };
        if ty.is_dir() {
            if depth > 0 {
                settings_files_in(&p, depth - 1, out);
            }
        } else if ty.is_file() {
            let ext = p.extension().map(|x| x.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            let small = e.metadata().map(|m| m.len() <= SETTINGS_MAX).unwrap_or(false);
            if small && SETTINGS_EXT.contains(&ext.as_str()) && !p.file_name().map(|n| n == "recomplauncher-achievements.json").unwrap_or(false) {
                out.push(p);
            }
        }
    }
}

/// Paths to the player's copy from the game's own settings: every settings file in its folder (and
/// a level down), and in the per-user folder named after its program (%APPDATA%\thebigs2, say).
pub fn from_settings(install: &Path) -> Vec<PathBuf> {
    let mut files = vec![];
    settings_files_in(install, 1, &mut files);
    let mut names: BTreeSet<String> = BTreeSet::new();
    for e in std::fs::read_dir(install).into_iter().flatten().flatten() {
        let p = e.path();
        let ext = p.extension().map(|x| x.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if p.is_file() && (ext == "exe" || ext == "appimage" || ext.is_empty()) {
            if let Some(s) = p.file_stem() {
                names.insert(s.to_string_lossy().to_string());
            }
        }
    }
    let mut bases: Vec<PathBuf> = vec![];
    for k in ["APPDATA", "LOCALAPPDATA"] {
        if let Some(v) = std::env::var_os(k) {
            bases.push(PathBuf::from(v));
        }
    }
    if let Some(h) = dirs::home_dir() {
        bases.push(h.join(".config"));
        bases.push(h.join(".local").join("share"));
        bases.push(h.join("Library").join("Application Support"));
    }
    for n in names.iter().take(20) {
        for b in &bases {
            let d = b.join(n);
            if d.is_dir() {
                settings_files_in(&d, 1, &mut files);
            }
        }
    }
    let mut out: Vec<PathBuf> = vec![];
    for f in files.iter().take(400) {
        let Ok(text) = std::fs::read_to_string(f) else { continue };
        for p in paths_in_text(&text) {
            if !out.contains(&p) && p.exists() {
                out.push(p);
            }
        }
    }
    out
}

// ---------- what the running game has open ----------

/// The game's files from what it (and anything it started) has open or mapped right now: each one
/// turned into the disc image or folder it belongs to.
pub fn from_process(pid: u32) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = vec![];
    for f in open_files(pid) {
        if let Some(g) = crate::xex::game_for_open_file(&f) {
            if !out.contains(&g) {
                out.push(g);
            }
        }
    }
    out
}

/// The paths a process tree has open or mapped (the process and its children, a few levels).
pub fn open_files(pid: u32) -> Vec<PathBuf> {
    let mut pids = vec![pid];
    let mut i = 0;
    while i < pids.len() && pids.len() < 64 {
        for c in children(pids[i]) {
            if !pids.contains(&c) {
                pids.push(c);
            }
        }
        i += 1;
    }
    let mut seen = BTreeSet::new();
    for p in pids {
        for f in files_of(p) {
            seen.insert(f);
        }
    }
    seen.into_iter().collect()
}

#[cfg(target_os = "linux")]
fn children(pid: u32) -> Vec<u32> {
    let mut out = vec![];
    for e in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        let Ok(c) = e.file_name().to_string_lossy().parse::<u32>() else { continue };
        if let Ok(stat) = std::fs::read_to_string(format!("/proc/{c}/stat")) {
            // pid (comm) state ppid ...: comm can hold spaces and parentheses, so after the last ')'.
            if let Some(rest) = stat.rfind(')').map(|k| &stat[k + 1..]) {
                if rest.split_whitespace().nth(1).and_then(|x| x.parse::<u32>().ok()) == Some(pid) {
                    out.push(c);
                }
            }
        }
    }
    out
}

#[cfg(target_os = "linux")]
fn files_of(pid: u32) -> Vec<PathBuf> {
    let mut out = vec![];
    if let Ok(maps) = std::fs::read_to_string(format!("/proc/{pid}/maps")) {
        for line in maps.lines() {
            // address perms offset dev inode path: the path is everything from the 6th field on.
            let mut parts = line.splitn(6, ' ');
            if let Some(path) = parts.nth(5).map(str::trim) {
                if path.starts_with('/') {
                    out.push(PathBuf::from(path.trim_end_matches(" (deleted)")));
                }
            }
        }
    }
    for e in std::fs::read_dir(format!("/proc/{pid}/fd")).into_iter().flatten().flatten() {
        if let Ok(t) = std::fs::read_link(e.path()) {
            if t.is_absolute() {
                out.push(t);
            }
        }
    }
    out
}

#[cfg(target_os = "macos")]
fn children(pid: u32) -> Vec<u32> {
    std::process::Command::new("pgrep").args(["-P", &pid.to_string()]).output().map(|o| String::from_utf8_lossy(&o.stdout).lines().filter_map(|l| l.trim().parse().ok()).collect()).unwrap_or_default()
}

#[cfg(target_os = "macos")]
fn files_of(pid: u32) -> Vec<PathBuf> {
    // lsof lists open and mapped files alike; -Fn prints one "n<path>" line each.
    std::process::Command::new("lsof").args(["-n", "-P", "-Fn", "-p", &pid.to_string()]).output().map(|o| String::from_utf8_lossy(&o.stdout).lines().filter_map(|l| l.strip_prefix('n')).filter(|p| p.starts_with('/')).map(PathBuf::from).collect()).unwrap_or_default()
}

#[cfg(windows)]
fn children(pid: u32) -> Vec<u32> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
    let mut out = vec![];
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                if e.th32ParentProcessID == pid && e.th32ProcessID != pid {
                    out.push(e.th32ProcessID);
                }
                if Process32NextW(snap, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    out
}

/// \Device\HarddiskVolume3\Games\x.iso -> C:\Games\x.iso, by asking each drive letter what device
/// it is.
#[cfg(windows)]
fn dos_path(device_path: &str) -> Option<PathBuf> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::QueryDosDeviceW;
    for letter in b'A'..=b'Z' {
        let drive: Vec<u16> = format!("{}:", letter as char).encode_utf16().chain([0]).collect();
        let mut buf = vec![0u16; 1024];
        let n = unsafe { QueryDosDeviceW(PCWSTR(drive.as_ptr()), Some(&mut buf)) } as usize;
        if n == 0 {
            continue;
        }
        let target = String::from_utf16_lossy(&buf[..buf[..n].iter().position(|&c| c == 0).unwrap_or(n)]);
        if !target.is_empty() && device_path.len() > target.len() && device_path[..target.len()].eq_ignore_ascii_case(&target) && device_path.as_bytes()[target.len()] == b'\\' {
            return Some(PathBuf::from(format!("{}:{}", letter as char, &device_path[target.len()..])));
        }
    }
    None
}

#[cfg(windows)]
fn files_of(pid: u32) -> Vec<PathBuf> {
    let mut out = mapped_files(pid);
    out.extend(open_handles(pid));
    out
}

/// Files mapped into the process: a ReXGlue game maps its disc image, and each file it reads from
/// an extracted folder.
#[cfg(windows)]
fn mapped_files(pid: u32) -> Vec<PathBuf> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Memory::{VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_MAPPED};
    use windows::Win32::System::ProcessStatus::GetMappedFileNameW;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
    let mut out: BTreeSet<PathBuf> = BTreeSet::new();
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid) else { return vec![] };
        let mut addr: usize = 0;
        let mut names: BTreeSet<String> = BTreeSet::new();
        for _ in 0..200_000 {
            let mut mbi = MEMORY_BASIC_INFORMATION::default();
            if VirtualQueryEx(h, Some(addr as *const _), &mut mbi, std::mem::size_of::<MEMORY_BASIC_INFORMATION>()) == 0 {
                break;
            }
            if mbi.Type == MEM_MAPPED {
                let mut buf = vec![0u16; 1024];
                let n = GetMappedFileNameW(h, mbi.BaseAddress, &mut buf) as usize;
                if n > 0 {
                    names.insert(String::from_utf16_lossy(&buf[..n]));
                }
            }
            let next = (mbi.BaseAddress as usize).saturating_add(mbi.RegionSize);
            if next <= addr {
                break;
            }
            addr = next;
        }
        let _ = CloseHandle(h);
        for n in names {
            if let Some(p) = dos_path(&n) {
                out.insert(p);
            }
        }
    }
    out.into_iter().collect()
}

/// Files the process has open, for engines that read their files the usual way. Only handles of
/// the File kind are looked at, and only ones on disk, so a pipe can never hold it up; and it all
/// runs on its own thread with a time limit.
#[cfg(windows)]
fn open_handles(pid: u32) -> Vec<PathBuf> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(open_handles_now(pid));
    });
    rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap_or_default()
}

#[cfg(windows)]
#[repr(C)]
#[derive(Clone, Copy)]
struct HandleEntry {
    object: usize,
    pid: usize,
    handle: usize,
    access: u32,
    creator_back_trace_index: u16,
    object_type_index: u16,
    attributes: u32,
    reserved: u32,
}

#[cfg(windows)]
#[link(name = "ntdll")]
extern "system" {
    fn NtQuerySystemInformation(class: u32, info: *mut core::ffi::c_void, len: u32, ret: *mut u32) -> i32;
}

#[cfg(windows)]
fn handle_table() -> Vec<HandleEntry> {
    // SystemExtendedHandleInformation: a count and a reserved word, then the entries.
    const CLASS: u32 = 64;
    let mut size: usize = 1 << 20;
    for _ in 0..8 {
        let mut buf = vec![0u8; size];
        let mut ret = 0u32;
        let status = unsafe { NtQuerySystemInformation(CLASS, buf.as_mut_ptr().cast(), size as u32, &mut ret) };
        if status == 0 {
            let head = std::mem::size_of::<usize>() * 2;
            let count = usize::from_ne_bytes(buf[..std::mem::size_of::<usize>()].try_into().unwrap());
            let each = std::mem::size_of::<HandleEntry>();
            let count = count.min((buf.len() - head) / each);
            let mut out = Vec::with_capacity(count);
            for i in 0..count {
                let e: HandleEntry = unsafe { std::ptr::read_unaligned(buf.as_ptr().add(head + i * each).cast()) };
                out.push(e);
            }
            return out;
        }
        // STATUS_INFO_LENGTH_MISMATCH: a bigger buffer.
        size = (ret as usize).max(size * 2) + (1 << 16);
        if size > 512 << 20 {
            break;
        }
    }
    vec![]
}

#[cfg(windows)]
fn open_handles_now(pid: u32) -> Vec<PathBuf> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::{CloseHandle, DuplicateHandle, DUPLICATE_SAME_ACCESS, HANDLE};
    use windows::Win32::Storage::FileSystem::{GetFileType, GetFinalPathNameByHandleW, FILE_NAME_NORMALIZED, FILE_TYPE_DISK};
    use windows::Win32::System::Threading::{GetCurrentProcess, GetCurrentProcessId, OpenProcess, PROCESS_DUP_HANDLE};
    let mut out: BTreeSet<PathBuf> = BTreeSet::new();
    // Which object type is "File" on this Windows: the type of a file this process has open.
    let probe = std::env::current_exe().ok().and_then(|p| std::fs::File::open(p).ok());
    let Some(probe) = probe else { return vec![] };
    let me = unsafe { GetCurrentProcessId() } as usize;
    let table = handle_table();
    let Some(file_type) = table.iter().find(|e| e.pid == me && e.handle == probe.as_raw_handle() as usize).map(|e| e.object_type_index) else { return vec![] };
    drop(probe);
    unsafe {
        let Ok(src) = OpenProcess(PROCESS_DUP_HANDLE, false, pid) else { return vec![] };
        for e in table.iter().filter(|e| e.pid == pid as usize && e.object_type_index == file_type).take(20_000) {
            let mut dup = HANDLE::default();
            if DuplicateHandle(src, HANDLE(e.handle as *mut _), GetCurrentProcess(), &mut dup, 0, false, DUPLICATE_SAME_ACCESS).is_err() {
                continue;
            }
            if GetFileType(dup) == FILE_TYPE_DISK {
                let mut buf = vec![0u16; 1024];
                let n = GetFinalPathNameByHandleW(dup, &mut buf, FILE_NAME_NORMALIZED) as usize;
                if n > 0 && n < buf.len() {
                    let s = String::from_utf16_lossy(&buf[..n]);
                    let s = s.strip_prefix(r"\\?\UNC\").map(|x| format!(r"\\{x}")).unwrap_or_else(|| s.strip_prefix(r"\\?\").unwrap_or(&s).to_string());
                    out.insert(PathBuf::from(s));
                }
            }
            let _ = CloseHandle(dup);
        }
        let _ = CloseHandle(src);
    }
    out.into_iter().collect()
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn children(_pid: u32) -> Vec<u32> {
    vec![]
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn files_of(_pid: u32) -> Vec<PathBuf> {
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_paths_out_of_settings_text() {
        let toml = "iso_path = \"D:\\\\Games\\\\SVR 2007.iso\"\nother = 3\n";
        assert_eq!(paths_in_text(toml), vec![PathBuf::from(r"D:\Games\SVR 2007.iso")]);
        let txt = r"C:\Users\Ruben\Documents\Roms\WWE SmackDown vs. Raw 2007 (USA, Europe).iso";
        assert_eq!(paths_in_text(txt), vec![PathBuf::from(txt)]);
        let json = r#"{"game": "E:/360/fifa14", "n": 1}"#;
        assert_eq!(paths_in_text(json), vec![PathBuf::from("E:/360/fifa14")]);
        assert!(paths_in_text("version = 1.2:3\nurl = \"https://x.y/z\"").iter().all(|p| !p.to_string_lossy().starts_with("s:")));
    }

    /// A tiny Xbox disc image with a default.xex, for the tests below.
    fn xiso(path: &Path, xex: &[u8]) {
        let mut img = vec![0u8; 64 * 2048];
        let vd = 32 * 2048;
        img[vd..vd + 20].copy_from_slice(b"MICROSOFT*XBOX*MEDIA");
        img[vd + 20..vd + 24].copy_from_slice(&40u32.to_le_bytes());
        img[vd + 24..vd + 28].copy_from_slice(&2048u32.to_le_bytes());
        let root = 40 * 2048;
        for b in &mut img[root..root + 2048] {
            *b = 0xFF;
        }
        img[root..root + 4].copy_from_slice(&[0, 0, 0, 0]);
        img[root + 4..root + 8].copy_from_slice(&44u32.to_le_bytes());
        img[root + 8..root + 12].copy_from_slice(&(xex.len() as u32).to_le_bytes());
        img[root + 12] = 0x20;
        img[root + 13] = 11;
        img[root + 14..root + 25].copy_from_slice(b"default.xex");
        img.resize(img.len().max(44 * 2048 + xex.len()), 0);
        img[44 * 2048..44 * 2048 + xex.len()].copy_from_slice(xex);
        std::fs::write(path, img).unwrap();
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rl-gamefiles-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// SmackDown vs. Raw 2007's way: the disc image picked in its own picker, the path saved in
    /// svr2007-game.txt next to the exe. Found from the settings, read straight from the image.
    #[test]
    fn finds_a_disc_image_named_in_a_txt_next_to_the_exe() {
        let d = temp("txt");
        let install = d.join("install");
        let roms = d.join("Roms");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::create_dir_all(&roms).unwrap();
        std::fs::write(install.join("svr2007.exe"), b"MZ").unwrap();
        let iso = roms.join("WWE SmackDown vs. Raw 2007 (USA, Europe).iso");
        xiso(&iso, b"XEX2-real");
        std::fs::write(install.join("svr2007-game.txt"), iso.to_string_lossy().as_bytes()).unwrap();
        let found = from_settings(&install);
        assert!(found.contains(&iso), "{found:?}");
        assert_eq!(crate::xex::find(&[install.clone()]), None, "not in the install");
        assert_eq!(crate::xex::find_exact(&found), Some(iso.clone()));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The Bigs 2's way: iso_path in its .toml, with TOML's doubled backslashes on Windows.
    #[test]
    fn finds_a_disc_image_in_a_toml_setting() {
        let d = temp("toml");
        let install = d.join("install");
        std::fs::create_dir_all(&install).unwrap();
        let iso = d.join("bigs2.iso");
        xiso(&iso, b"XEX2-real");
        let written = iso.to_string_lossy().replace('\\', "\\\\");
        std::fs::write(install.join("thebigs2.toml"), format!("# Game disc\niso_path = \"{written}\"\nresolution_scale = 1\n")).unwrap();
        assert_eq!(crate::xex::find_exact(&from_settings(&install)), Some(iso));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A disc image right next to the exe (SmackDown looks there first).
    #[test]
    fn finds_a_disc_image_next_to_the_exe() {
        let d = temp("next");
        xiso(&d.join("game.iso"), b"XEX2-real");
        std::fs::write(d.join("notes.iso"), b"not a disc").unwrap();
        assert_eq!(crate::xex::find(&[d.clone()]), Some(d.join("game.iso")));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// From what a running game has open: a file deep in an extracted folder leads to the folder with
    /// default.xex; a disc image or default.xex is itself.
    #[test]
    fn an_open_file_leads_to_its_game() {
        let d = temp("open");
        let game = d.join("FIFA");
        std::fs::create_dir_all(game.join("data/ui")).unwrap();
        std::fs::write(game.join("default.xex"), b"XEX2").unwrap();
        std::fs::write(game.join("data/ui/main.big"), b"x").unwrap();
        assert_eq!(crate::xex::game_for_open_file(&game.join("data/ui/main.big")), Some(game.clone()));
        assert_eq!(crate::xex::game_for_open_file(&d.join("x.iso")), Some(d.join("x.iso")));
        assert_eq!(crate::xex::game_for_open_file(&d.join("elsewhere.dll")), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The real thing: a child process that has a file open in a game folder (and one mapped on
    /// Windows and Linux, the way ReXGlue maps a disc image) is found from its pid.
    #[test]
    fn finds_what_a_running_process_has_open() {
        let d = temp("proc");
        let game = d.join("Game Folder");
        std::fs::create_dir_all(game.join("media")).unwrap();
        std::fs::write(game.join("default.xex"), b"XEX2").unwrap();
        let held = game.join("media").join("held.bin");
        std::fs::write(&held, vec![7u8; 1 << 16]).unwrap();
        #[cfg(windows)]
        let mut child = {
            // PowerShell keeps the file open (and mapped) until it's stopped.
            let script = format!(
                "$f=[System.IO.File]::Open('{}','Open','Read','Read'); $m=[System.IO.MemoryMappedFiles.MemoryMappedFile]::CreateFromFile($f,$null,0,'Read',$null,'None',$true); $v=$m.CreateViewAccessor(0,0,'Read'); Start-Sleep -Seconds 60",
                held.display()
            );
            std::process::Command::new("powershell").args(["-NoProfile", "-Command", &script]).spawn().unwrap()
        };
        #[cfg(not(windows))]
        let mut child = std::process::Command::new("sh").arg("-c").arg(format!("exec 3<'{}'; sleep 60", held.display())).spawn().unwrap();
        let mut found = vec![];
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            found = from_process(child.id());
            if found.contains(&game) {
                break;
            }
        }
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&d);
        assert!(found.iter().any(|p| p.ends_with("Game Folder")), "{found:?}");
    }
}

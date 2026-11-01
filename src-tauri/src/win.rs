//! Windows asked directly, through its own calls: the registry, the programs that are running,
//! shortcuts, folder links, Credential Manager and the administrator prompt. Nothing here starts
//! PowerShell, cmd, reg or tasklist.

/// Splits a command line the way Windows keeps one (an uninstaller, say) into its program and the
/// rest: a quoted program, else the first `.exe` followed by a space, else the first word.
pub fn split_command(cmd: &str) -> (String, String) {
    let c = cmd.trim();
    if let Some(rest) = c.strip_prefix('"') {
        if let Some(i) = rest.find('"') {
            return (rest[..i].to_string(), rest[i + 1..].trim().to_string());
        }
    }
    let low = c.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = low[from..].find(".exe") {
        let end = from + i + 4;
        if end == c.len() || c[end..].starts_with(' ') {
            return (c[..end].to_string(), c[end..].trim().to_string());
        }
        from = end;
    }
    match c.split_once(' ') {
        Some((a, b)) => (a.to_string(), b.trim().to_string()),
        None => (c.to_string(), String::new()),
    }
}

/// What a folder link (a junction) points at, as Windows stores it.
pub fn junction_data(target: &str) -> Vec<u8> {
    let subst: Vec<u16> = format!("\\??\\{target}").encode_utf16().collect();
    let print: Vec<u16> = target.encode_utf16().collect();
    let (sl, pl) = (subst.len() as u16 * 2, print.len() as u16 * 2);
    let path_bytes = sl + 2 + pl + 2;
    let mut b = Vec::with_capacity(16 + path_bytes as usize);
    b.extend_from_slice(&0xA000_0003u32.to_le_bytes()); // IO_REPARSE_TAG_MOUNT_POINT
    b.extend_from_slice(&(8 + path_bytes).to_le_bytes());
    b.extend_from_slice(&0u16.to_le_bytes());
    b.extend_from_slice(&0u16.to_le_bytes()); // where the target starts
    b.extend_from_slice(&sl.to_le_bytes());
    b.extend_from_slice(&(sl + 2).to_le_bytes()); // where the name shown starts
    b.extend_from_slice(&pl.to_le_bytes());
    for c in subst.iter().chain(&[0]).chain(print.iter()).chain(&[0]) {
        b.extend_from_slice(&c.to_le_bytes());
    }
    b
}

#[cfg(windows)]
pub use imp::*;

#[cfg(windows)]
mod imp {
    use std::path::{Path, PathBuf};
    use windows::core::{Interface, HSTRING, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE};
    pub use windows::Win32::System::Registry::{HKEY, HKEY_CURRENT_USER as HKCU, HKEY_LOCAL_MACHINE as HKLM};
    use windows::Win32::System::Registry::*;

    fn text(b: &[u16]) -> String {
        let n = b.iter().position(|&c| c == 0).unwrap_or(b.len());
        String::from_utf16_lossy(&b[..n])
    }

    // ---------- registry ----------

    /// A text value from the registry.
    pub fn reg_str(root: HKEY, key: &str, name: &str) -> Option<String> {
        let (k, n) = (HSTRING::from(key), HSTRING::from(name));
        let flags = REG_ROUTINE_FLAGS(RRF_RT_REG_SZ.0 | RRF_RT_REG_EXPAND_SZ.0);
        let mut len = 0u32;
        if unsafe { RegGetValueW(root, &k, &n, flags, None, None, Some(&mut len)) } != ERROR_SUCCESS {
            return None;
        }
        let mut buf = vec![0u16; len as usize / 2 + 2];
        let mut len = (buf.len() * 2) as u32;
        if unsafe { RegGetValueW(root, &k, &n, flags, None, Some(buf.as_mut_ptr().cast()), Some(&mut len)) } != ERROR_SUCCESS {
            return None;
        }
        Some(text(&buf))
    }

    /// A number from the registry.
    pub fn reg_dword(root: HKEY, key: &str, name: &str) -> Option<u32> {
        let mut v = 0u32;
        let mut len = 4u32;
        let r = unsafe { RegGetValueW(root, &HSTRING::from(key), &HSTRING::from(name), RRF_RT_REG_DWORD, None, Some((&mut v as *mut u32).cast()), Some(&mut len)) };
        (r == ERROR_SUCCESS).then_some(v)
    }

    /// The names of a registry key's subkeys.
    pub fn reg_subkeys(root: HKEY, key: &str) -> Vec<String> {
        let mut h = HKEY::default();
        if unsafe { RegOpenKeyExW(root, &HSTRING::from(key), None, KEY_READ, &mut h) } != ERROR_SUCCESS {
            return vec![];
        }
        let mut out = vec![];
        for i in 0..10_000u32 {
            let mut buf = [0u16; 256];
            let mut len = buf.len() as u32;
            if unsafe { RegEnumKeyExW(h, i, Some(PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None) } != ERROR_SUCCESS {
                break;
            }
            out.push(String::from_utf16_lossy(&buf[..len as usize]));
        }
        unsafe {
            let _ = RegCloseKey(h);
        }
        out
    }

    /// "10.0.22631"
    pub fn os_version() -> String {
        let k = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
        let major = reg_dword(HKLM, k, "CurrentMajorVersionNumber").unwrap_or(10);
        let minor = reg_dword(HKLM, k, "CurrentMinorVersionNumber").unwrap_or(0);
        let build = reg_str(HKLM, k, "CurrentBuild").unwrap_or_default();
        format!("{major}.{minor}.{}", build.trim())
    }

    /// "Windows 11 Pro 24H2 (26100)"
    pub fn os_name() -> String {
        let k = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
        let build = reg_str(HKLM, k, "CurrentBuild").unwrap_or_default();
        let mut name = reg_str(HKLM, k, "ProductName").unwrap_or_else(|| "Windows".into());
        // Windows 11 still calls itself Windows 10 here.
        if build.trim().parse::<u32>().unwrap_or(0) >= 22000 {
            name = name.replace("Windows 10", "Windows 11");
        }
        let dv = reg_str(HKLM, k, "DisplayVersion").unwrap_or_default();
        format!("{name} {dv} ({})", build.trim()).replace("  ", " ")
    }

    // ---------- programs that are running ----------

    /// The programs running now: (process id, program file name).
    pub fn processes() -> Vec<(u32, String)> {
        use windows::Win32::System::Diagnostics::ToolHelp::*;
        let Ok(snap) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else { return vec![] };
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut out = vec![];
        let mut ok = unsafe { Process32FirstW(snap, &mut e) }.is_ok();
        while ok {
            out.push((e.th32ProcessID, text(&e.szExeFile)));
            ok = unsafe { Process32NextW(snap, &mut e) }.is_ok();
        }
        unsafe {
            let _ = CloseHandle(snap);
        }
        out
    }

    /// Is a program with this file name running?
    pub fn is_running(file_name: &str) -> bool {
        processes().iter().any(|(_, n)| n.eq_ignore_ascii_case(file_name))
    }

    fn open_process(pid: u32) -> Option<HANDLE> {
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()
    }

    /// A running program's file.
    pub fn process_file(pid: u32) -> Option<PathBuf> {
        use windows::Win32::System::Threading::{QueryFullProcessImageNameW, PROCESS_NAME_WIN32};
        let h = open_process(pid)?;
        let mut buf = vec![0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = unsafe { QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) }.is_ok();
        unsafe {
            let _ = CloseHandle(h);
        }
        ok.then(|| PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])))
    }

    #[link(name = "ntdll")]
    extern "system" {
        fn NtQueryInformationProcess(h: *mut core::ffi::c_void, class: u32, info: *mut core::ffi::c_void, len: u32, ret: *mut u32) -> i32;
    }

    /// A running program's command line (what it was started with).
    pub fn command_line(pid: u32) -> Option<String> {
        const COMMAND_LINE: u32 = 60;
        let h = open_process(pid)?;
        let mut buf = vec![0u64; 1024];
        let mut ret = 0u32;
        let mut st = unsafe { NtQueryInformationProcess(h.0, COMMAND_LINE, buf.as_mut_ptr().cast(), (buf.len() * 8) as u32, &mut ret) };
        if st != 0 && ret as usize > buf.len() * 8 {
            buf = vec![0u64; ret as usize / 8 + 1];
            st = unsafe { NtQueryInformationProcess(h.0, COMMAND_LINE, buf.as_mut_ptr().cast(), (buf.len() * 8) as u32, &mut ret) };
        }
        unsafe {
            let _ = CloseHandle(h);
        }
        if st != 0 {
            return None;
        }
        // A UNICODE_STRING: its length in bytes, then a pointer to the text (inside buf).
        let len = (buf[0] & 0xFFFF) as usize / 2;
        let ptr = buf[1] as *const u16;
        let start = buf.as_ptr() as usize;
        let end = start + buf.len() * 8;
        if ptr.is_null() || (ptr as usize) < start || (ptr as usize) + len * 2 > end {
            return None;
        }
        Some(String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(ptr, len) }))
    }

    // ---------- shortcuts ----------

    /// Runs `f` on a thread of its own with COM ready, the way Windows' shell objects want it.
    fn with_com<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
        use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
        std::thread::spawn(move || {
            let ready = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
            let r = f();
            if ready {
                unsafe { CoUninitialize() };
            }
            r
        })
        .join()
        .ok()
    }

    pub struct Shortcut {
        pub file: PathBuf,
        pub target: PathBuf,
        pub args: String,
        pub dir: PathBuf,
        pub icon: Option<PathBuf>,
        pub description: String,
    }

    /// Makes a .lnk shortcut.
    pub fn make_shortcut(s: Shortcut) -> Result<(), String> {
        with_com(move || -> Result<(), String> {
            use windows::Win32::System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER};
            use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
            let go = || -> windows::core::Result<()> {
                let sl: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)? };
                unsafe {
                    sl.SetPath(&HSTRING::from(s.target.as_path()))?;
                    sl.SetArguments(&HSTRING::from(s.args.as_str()))?;
                    sl.SetWorkingDirectory(&HSTRING::from(s.dir.as_path()))?;
                    if let Some(i) = &s.icon {
                        sl.SetIconLocation(&HSTRING::from(i.as_path()), 0)?;
                    }
                    sl.SetDescription(&HSTRING::from(s.description.as_str()))?;
                    sl.cast::<IPersistFile>()?.Save(&HSTRING::from(s.file.as_path()), true)
                }
            };
            go().map_err(|e| e.message().to_string())
        })
        .unwrap_or_else(|| Err("Windows couldn't make the shortcut.".into()))
    }

    /// What some .lnk shortcuts start: (shortcut, program, its options). Ones that can't be read
    /// are left out.
    pub fn read_shortcuts(files: Vec<PathBuf>) -> Vec<(PathBuf, PathBuf, String)> {
        with_com(move || {
            use windows::Win32::System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER, STGM_READ};
            use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
            let one = |f: &Path| -> windows::core::Result<(PathBuf, String)> {
                unsafe {
                    let sl: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
                    sl.cast::<IPersistFile>()?.Load(&HSTRING::from(f), STGM_READ)?;
                    let mut p = vec![0u16; 1024];
                    sl.GetPath(&mut p, std::ptr::null_mut(), 0)?;
                    let mut a = vec![0u16; 4096];
                    sl.GetArguments(&mut a)?;
                    Ok((PathBuf::from(text(&p)), text(&a)))
                }
            };
            files.iter().filter_map(|f| one(f).ok().map(|(p, a)| (f.clone(), p, a))).collect()
        })
        .unwrap_or_default()
    }

    // ---------- folder links ----------

    /// A junction at `link` to the folder `target` (no administrator rights needed).
    pub fn junction(target: &Path, link: &Path) -> std::io::Result<()> {
        use windows::Win32::Foundation::GENERIC_WRITE;
        use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_MODE, OPEN_EXISTING};
        use windows::Win32::System::Ioctl::FSCTL_SET_REPARSE_POINT;
        use windows::Win32::System::IO::DeviceIoControl;
        let abs = std::path::absolute(target)?;
        let s = abs.to_string_lossy();
        let s = s.strip_prefix(r"\\?\").unwrap_or(&s).to_string();
        let data = crate::win::junction_data(&s);
        std::fs::create_dir(link)?;
        let fail = |e: windows::core::Error| {
            let _ = std::fs::remove_dir(link);
            std::io::Error::from_raw_os_error(e.code().0 & 0xFFFF)
        };
        let h = unsafe { CreateFileW(&HSTRING::from(link), GENERIC_WRITE.0, FILE_SHARE_MODE(0), None, OPEN_EXISTING, FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS, None) }.map_err(fail)?;
        let mut ret = 0u32;
        let r = unsafe { DeviceIoControl(h, FSCTL_SET_REPARSE_POINT, Some(data.as_ptr().cast()), data.len() as u32, None, 0, Some(&mut ret), None) };
        unsafe {
            let _ = CloseHandle(h);
        }
        r.map_err(fail)
    }

    // ---------- the sign-in, kept by Windows ----------

    const CRED: &str = "RecompLauncher/account";

    /// Keeps a secret in Windows' Credential Manager, for this Windows user only.
    pub fn cred_write(secret: &str) -> bool {
        use windows::Win32::Security::Credentials::{CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC};
        let mut target: Vec<u16> = CRED.encode_utf16().chain([0]).collect();
        let mut user: Vec<u16> = "RecompLauncher".encode_utf16().chain([0]).collect();
        let mut blob = secret.as_bytes().to_vec();
        let c = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: PWSTR(target.as_mut_ptr()),
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            UserName: PWSTR(user.as_mut_ptr()),
            ..Default::default()
        };
        unsafe { CredWriteW(&c, 0) }.is_ok()
    }

    pub fn cred_read() -> Option<String> {
        use windows::Win32::Security::Credentials::{CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC};
        let mut p: *mut CREDENTIALW = std::ptr::null_mut();
        unsafe { CredReadW(&HSTRING::from(CRED), CRED_TYPE_GENERIC, None, &mut p) }.ok()?;
        if p.is_null() {
            return None;
        }
        let v = unsafe {
            let c = &*p;
            let v = if c.CredentialBlob.is_null() { vec![] } else { std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize).to_vec() };
            CredFree(p as *const _);
            v
        };
        String::from_utf8(v).ok().filter(|s| !s.is_empty())
    }

    pub fn cred_delete() {
        use windows::Win32::Security::Credentials::{CredDeleteW, CRED_TYPE_GENERIC};
        let _ = unsafe { CredDeleteW(&HSTRING::from(CRED), CRED_TYPE_GENERIC, None) };
    }

    // ---------- the administrator prompt ----------

    /// A program started with administrator rights (after Windows' prompt).
    pub struct Elevated(isize);

    impl Elevated {
        /// Waits for it to close: its exit code.
        pub fn wait(self) -> Option<i32> {
            use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
            if self.0 == 0 {
                return None;
            }
            let h = HANDLE(self.0 as *mut _);
            let mut code = 0u32;
            let ok = unsafe {
                WaitForSingleObject(h, INFINITE);
                let ok = GetExitCodeProcess(h, &mut code).is_ok();
                let _ = CloseHandle(h);
                ok
            };
            ok.then_some(code as i32)
        }
    }

    /// Starts a program the way "Run as administrator" does.
    pub fn run_as_admin(exe: &Path, args: &str, dir: &Path) -> std::io::Result<Elevated> {
        use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let (f, p, d) = (HSTRING::from(exe), HSTRING::from(args), HSTRING::from(dir));
        let mut i = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
            lpVerb: windows::core::w!("runas"),
            lpFile: PCWSTR(f.as_ptr()),
            lpParameters: if args.is_empty() { PCWSTR::null() } else { PCWSTR(p.as_ptr()) },
            lpDirectory: PCWSTR(d.as_ptr()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        unsafe { ShellExecuteExW(&mut i) }.map_err(|e| std::io::Error::from_raw_os_error(e.code().0 & 0xFFFF))?;
        Ok(Elevated(i.hProcess.0 as isize))
    }

    // ---------- this PC ----------

    pub fn ram_gb() -> u32 {
        use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut m = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
        if unsafe { GlobalMemoryStatusEx(&mut m) }.is_err() {
            return 0;
        }
        (m.ullTotalPhys as f64 / 1_073_741_824.0).round() as u32
    }

    pub fn cpu_name() -> String {
        let n = reg_str(HKLM, r"HARDWARE\DESCRIPTION\System\CentralProcessor\0", "ProcessorNameString").unwrap_or_default();
        n.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// The graphics cards Windows is using now: (name, driver version).
    pub fn gpus() -> Vec<(String, String)> {
        use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
        let mut names: Vec<String> = vec![];
        if let Ok(f) = unsafe { CreateDXGIFactory1::<IDXGIFactory1>() } {
            for i in 0..16 {
                let Ok(a) = (unsafe { f.EnumAdapters1(i) }) else { break };
                if let Ok(d) = unsafe { a.GetDesc1() } {
                    if d.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) == 0 {
                        let n = text(&d.Description).trim().to_string();
                        if !n.is_empty() && !names.contains(&n) {
                            names.push(n);
                        }
                    }
                }
            }
        }
        // Each card's driver, from its entry among the display adapters.
        let class = r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";
        let drivers: Vec<(String, String)> = reg_subkeys(HKLM, class)
            .into_iter()
            .filter(|k| k.len() == 4 && k.chars().all(|c| c.is_ascii_digit()))
            .filter_map(|k| {
                let key = format!(r"{class}\{k}");
                Some((reg_str(HKLM, &key, "DriverDesc")?, reg_str(HKLM, &key, "DriverVersion").unwrap_or_default()))
            })
            .collect();
        names.into_iter().map(|n| {
            let v = drivers.iter().find(|(d, _)| d.trim().eq_ignore_ascii_case(&n)).map(|(_, v)| v.clone()).unwrap_or_default();
            (n, v)
        }).collect()
    }

    /// Visual Studio's C++ build tools, in any Visual Studio or Build Tools install.
    pub fn has_build_tools() -> bool {
        let has_msvc = |p: &Path| std::fs::read_dir(p.join(r"VC\Tools\MSVC")).map(|mut d| d.next().is_some()).unwrap_or(false);
        // Each install leaves a note of where it is.
        let pd = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into());
        let notes = Path::new(&pd).join(r"Microsoft\VisualStudio\Packages\_Instances");
        for e in std::fs::read_dir(notes).into_iter().flatten().flatten() {
            let path = std::fs::read(e.path().join("state.json")).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()).and_then(|v| v.get("installationPath").and_then(|p| p.as_str()).map(PathBuf::from));
            if path.map(|p| has_msvc(&p)).unwrap_or(false) {
                return true;
            }
        }
        // Else where it installs by default.
        for pf in [std::env::var("ProgramFiles").ok(), std::env::var("ProgramFiles(x86)").ok()].into_iter().flatten() {
            let vs = Path::new(&pf).join("Microsoft Visual Studio");
            for year in std::fs::read_dir(&vs).into_iter().flatten().flatten() {
                for edition in std::fs::read_dir(year.path()).into_iter().flatten().flatten() {
                    if has_msvc(&edition.path()) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Is the Visual C++ runtime installed?
    pub fn has_vcredist() -> bool {
        [r"SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64", r"SOFTWARE\WOW6432Node\Microsoft\VisualStudio\14.0\VC\Runtimes\x64"]
            .iter()
            .any(|k| reg_dword(HKLM, k, "Installed") == Some(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uninstall_commands_split_like_windows_does() {
        assert_eq!(split_command(r#""C:\Games\X\unins000.exe" /SILENT"#), (r"C:\Games\X\unins000.exe".into(), "/SILENT".into()));
        assert_eq!(split_command("MsiExec.exe /X{1234}"), ("MsiExec.exe".into(), "/X{1234}".into()));
        assert_eq!(split_command(r"C:\Program Files\Game\uninstall.exe --quiet"), (r"C:\Program Files\Game\uninstall.exe".into(), "--quiet".into()));
        assert_eq!(split_command(r"C:\Games\Y\remove.exe"), (r"C:\Games\Y\remove.exe".into(), String::new()));
    }

    #[test]
    fn a_junction_points_at_its_folder() {
        let d = junction_data(r"C:\Games\Copy");
        assert_eq!(&d[0..4], &0xA000_0003u32.to_le_bytes());
        let subst = r"\??\C:\Games\Copy".len() as u16 * 2;
        let print = r"C:\Games\Copy".len() as u16 * 2;
        assert_eq!(u16::from_le_bytes([d[4], d[5]]) as usize, d.len() - 8);
        assert_eq!(u16::from_le_bytes([d[10], d[11]]), subst);
        assert_eq!(u16::from_le_bytes([d[12], d[13]]), subst + 2);
        assert_eq!(u16::from_le_bytes([d[14], d[15]]), print);
        let words: Vec<u16> = d[16..].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        assert_eq!(String::from_utf16_lossy(&words[..subst as usize / 2]), r"\??\C:\Games\Copy");
    }
}

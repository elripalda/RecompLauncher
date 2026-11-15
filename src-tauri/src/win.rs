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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MonitorBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[cfg(windows)]
pub use imp::*;

#[cfg(windows)]
mod imp {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::{Duration, Instant};
    use windows::core::{Interface, HSTRING, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, DuplicateHandle, DUPLICATE_SAME_ACCESS, ERROR_SUCCESS, HANDLE, WAIT_TIMEOUT};
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

    struct ProcessInstance {
        pid: u32,
        created: u64,
        handle: OwnedHandle,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct ProcessIdentity {
        pid: u32,
        created: u64,
    }

    impl ProcessInstance {
        fn identity(&self) -> ProcessIdentity {
            ProcessIdentity { pid: self.pid, created: self.created }
        }
    }

    struct GameProcesses {
        processes: Vec<ProcessInstance>,
        // Keep the last confirmed screen while the game window is minimized or rebuilding.
        last_monitor: Option<super::MonitorBounds>,
        cancelled: bool,
    }

    static GAME_PROCESSES: OnceLock<Mutex<HashMap<String, Arc<Mutex<GameProcesses>>>>> = OnceLock::new();

    fn game_processes() -> &'static Mutex<HashMap<String, Arc<Mutex<GameProcesses>>>> {
        GAME_PROCESSES.get_or_init(|| Mutex::new(HashMap::new()))
    }

    fn game_processes_for(game: &str) -> Option<Arc<Mutex<GameProcesses>>> {
        game_processes().lock().unwrap().get(game).cloned()
    }

    pub fn track_game_process(game: &str, root: RawHandle) -> bool {
        let Some(root) = duplicate_process(root).and_then(process_instance) else { return false };
        let state = Arc::new(Mutex::new(GameProcesses { processes: vec![root], last_monitor: None, cancelled: false }));
        let previous = game_processes().lock().unwrap().insert(game.to_string(), state);
        if let Some(previous) = previous {
            previous.lock().unwrap().cancelled = true;
        }
        true
    }

    pub fn forget_game_process(game: &str) {
        let state = game_processes().lock().unwrap().remove(game);
        if let Some(state) = state {
            state.lock().unwrap().cancelled = true;
        }
    }

    fn api_handle(handle: &OwnedHandle) -> HANDLE {
        HANDLE(handle.as_raw_handle() as *mut _)
    }

    fn creation_time(handle: HANDLE) -> Option<u64> {
        use windows::Win32::Foundation::FILETIME;
        use windows::Win32::System::Threading::GetProcessTimes;
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) }.ok()?;
        Some((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
    }

    fn duplicate_process(raw: RawHandle) -> Option<OwnedHandle> {
        use windows::Win32::System::Threading::GetCurrentProcess;
        let mut duplicate = HANDLE::default();
        unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                HANDLE(raw as *mut _),
                GetCurrentProcess(),
                &mut duplicate,
                0,
                false,
                DUPLICATE_SAME_ACCESS,
            )
        }
        .ok()?;
        Some(unsafe { OwnedHandle::from_raw_handle(duplicate.0 as RawHandle) })
    }

    fn process_instance(handle: OwnedHandle) -> Option<ProcessInstance> {
        use windows::Win32::System::Threading::GetProcessId;
        let raw = api_handle(&handle);
        let pid = unsafe { GetProcessId(raw) };
        let created = creation_time(raw)?;
        (pid != 0).then_some(ProcessInstance { pid, created, handle })
    }

    fn is_active_instance(process: &ProcessInstance) -> bool {
        use windows::Win32::System::Threading::{GetProcessId, WaitForSingleObject};
        let tracked = api_handle(&process.handle);
        if unsafe { GetProcessId(tracked) } != process.pid
            || creation_time(tracked) != Some(process.created)
            || unsafe { WaitForSingleObject(tracked, 0) } != WAIT_TIMEOUT
        {
            return false;
        }
        let Some(current) = current_process_handle(process.pid) else { return false };
        let current = api_handle(&current);
        let same = unsafe { GetProcessId(current) } == process.pid
            && creation_time(current) == Some(process.created)
            && unsafe { WaitForSingleObject(current, 0) } == WAIT_TIMEOUT;
        same
    }

    fn current_process_handle(pid: u32) -> Option<OwnedHandle> {
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_ACCESS_RIGHTS, PROCESS_QUERY_LIMITED_INFORMATION};
        // The Windows crate exposes this standard access bit as a file right, not a process right.
        const PROCESS_SYNCHRONIZE: PROCESS_ACCESS_RIGHTS = PROCESS_ACCESS_RIGHTS(0x0010_0000);
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, false, pid) }.ok()?;
        Some(unsafe { OwnedHandle::from_raw_handle(handle.0 as RawHandle) })
    }

    #[derive(Clone, Copy)]
    struct ProcessEntry {
        pid: u32,
        parent_pid: u32,
    }

    fn active_identities(processes: &[ProcessInstance]) -> Vec<ProcessIdentity> {
        processes.iter().filter(|p| is_active_instance(p)).map(ProcessInstance::identity).collect()
    }

    fn child_discovery_authorized(
        entry: ProcessEntry,
        tracked: &[ProcessIdentity],
        active: &[ProcessIdentity],
        own_pid: u32,
    ) -> bool {
        entry.pid != 0
            && entry.pid != own_pid
            && active.iter().any(|parent| parent.pid == entry.parent_pid && tracked.contains(parent))
    }

    fn tracked_active_process(pid: u32, tracked: &[ProcessIdentity], active: &[ProcessIdentity]) -> Option<ProcessIdentity> {
        active.iter().copied().find(|identity| identity.pid == pid && tracked.contains(identity))
    }

    fn snapshot_parent_matches(pid: u32, parent_pid: u32, entries: &[ProcessEntry]) -> bool {
        entries.iter().any(|entry| entry.pid == pid && entry.parent_pid == parent_pid)
    }

    fn process_snapshot() -> Vec<ProcessEntry> {
        use windows::Win32::System::Diagnostics::ToolHelp::*;
        let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else { return Vec::new() };
        struct Snapshot(HANDLE);
        impl Drop for Snapshot {
            fn drop(&mut self) { unsafe { let _ = CloseHandle(self.0); } }
        }
        let _snapshot = Snapshot(snapshot);
        let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut out = Vec::new();
        let mut next = unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok();
        while next {
            out.push(ProcessEntry { pid: entry.th32ProcessID, parent_pid: entry.th32ParentProcessID });
            next = unsafe { Process32NextW(snapshot, &mut entry) }.is_ok();
        }
        out
    }

    fn discover_children(processes: &mut Vec<ProcessInstance>, entries: &[ProcessEntry]) {
        let tracked: Vec<ProcessIdentity> = processes.iter().map(ProcessInstance::identity).collect();
        let parents = active_identities(processes);
        for entry in entries {
            if !child_discovery_authorized(*entry, &tracked, &parents, std::process::id()) {
                continue;
            }
            if processes.iter().any(|p| p.pid == entry.pid && is_active_instance(p)) {
                continue;
            }
            let Some(handle) = current_process_handle(entry.pid) else { continue };
            let Some(child) = process_instance(handle) else { continue };
            if !is_active_instance(&child) {
                continue;
            }
            // The snapshot may have outlived this PID. Confirm the same parent relation again
            // while the retained child handle still identifies the process instance we opened.
            if !snapshot_parent_matches(child.pid, entry.parent_pid, &process_snapshot()) || !is_active_instance(&child) {
                continue;
            }
            // The parent must still be the same live process when the child identity is recorded.
            if child_discovery_authorized(*entry, &tracked, &active_identities(processes), std::process::id()) {
                processes.push(child);
            }
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct WindowKey {
        hwnd: isize,
        pid: u32,
        created: u64,
    }

    #[derive(Clone, Copy)]
    struct WindowCandidate {
        key: WindowKey,
        rect: windows::Win32::Foundation::RECT,
        style: i32,
    }

    fn eligible_window(hwnd: windows::Win32::Foundation::HWND, pid: u32) -> Option<WindowCandidate> {
        use windows::Win32::UI::WindowsAndMessaging::*;
        if !unsafe { IsWindow(Some(hwnd)) }.as_bool()
            || !unsafe { IsWindowVisible(hwnd) }.as_bool()
            || unsafe { IsIconic(hwnd) }.as_bool()
            || unsafe { GetAncestor(hwnd, GA_ROOT) } != hwnd
        {
            return None;
        }
        // GetWindow returns null for a valid unowned window; the windows crate wraps that as Err.
        let has_owner = unsafe { GetWindow(hwnd, GW_OWNER) }.is_ok_and(|owner| !owner.0.is_null());
        let mut current_pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut current_pid as *mut u32)) };
        if has_owner || !unsafe { IsWindow(Some(hwnd)) }.as_bool() || current_pid != pid {
            return None;
        }
        let style = window_long(hwnd, GWL_STYLE)?;
        let ex_style = window_long(hwnd, GWL_EXSTYLE)?;
        if style & 0x4000_0000 != 0 || ex_style & 0x0000_0080 != 0 || ex_style & 0x0800_0000 != 0 {
            return None;
        }
        let mut rect = windows::Win32::Foundation::RECT::default();
        unsafe { GetWindowRect(hwnd, &mut rect) }.ok()?;
        if rect.right.saturating_sub(rect.left) < 160 || rect.bottom.saturating_sub(rect.top) < 120 {
            return None;
        }
        Some(WindowCandidate { key: WindowKey { hwnd: hwnd.0 as isize, pid, created: 0 }, rect, style })
    }

    fn window_long(hwnd: windows::Win32::Foundation::HWND, index: windows::Win32::UI::WindowsAndMessaging::WINDOW_LONG_PTR_INDEX) -> Option<i32> {
        use windows::Win32::Foundation::{GetLastError, SetLastError, ERROR_SUCCESS};
        use windows::Win32::UI::WindowsAndMessaging::GetWindowLongW;
        unsafe { SetLastError(ERROR_SUCCESS) };
        let value = unsafe { GetWindowLongW(hwnd, index) };
        (value != 0 || unsafe { GetLastError() } == ERROR_SUCCESS).then_some(value)
    }

    struct WindowList(Vec<(windows::Win32::Foundation::HWND, u32)>);

    unsafe extern "system" fn collect_window(hwnd: windows::Win32::Foundation::HWND, data: windows::Win32::Foundation::LPARAM) -> windows::core::BOOL {
        let list = unsafe { &mut *(data.0 as *mut WindowList) };
        let mut pid = 0u32;
        unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32)) };
        if pid != 0 {
            list.0.push((hwnd, pid));
        }
        true.into()
    }

    fn candidates(processes: &[ProcessInstance]) -> Vec<WindowCandidate> {
        use windows::Win32::UI::WindowsAndMessaging::EnumWindows;
        let mut list = WindowList(Vec::new());
        let _ = unsafe { EnumWindows(Some(collect_window), windows::Win32::Foundation::LPARAM(&mut list as *mut _ as isize)) };
        let tracked: Vec<ProcessIdentity> = processes.iter().map(ProcessInstance::identity).collect();
        let active = active_identities(processes);
        list.0.into_iter().filter_map(|(hwnd, pid)| {
            let identity = tracked_active_process(pid, &tracked, &active)?;
            let mut candidate = eligible_window(hwnd, pid)?;
            candidate.key.created = identity.created;
            Some(candidate)
        }).collect()
    }

    fn unique_candidate(candidates: &[WindowCandidate]) -> Option<WindowCandidate> {
        (candidates.len() == 1).then(|| candidates[0])
    }

    fn verify_window(candidate: WindowCandidate, processes: &[ProcessInstance]) -> bool {
        use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
        let hwnd = windows::Win32::Foundation::HWND(candidate.key.hwnd as *mut _);
        let Some(process) = processes.iter().find(|p| p.pid == candidate.key.pid && p.created == candidate.key.created) else { return false };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32)) };
        pid == candidate.key.pid && is_active_instance(process)
            && eligible_window(hwnd, pid).is_some_and(|current| current.key.hwnd == candidate.key.hwnd)
    }

    fn placement_for(rect: windows::Win32::Foundation::RECT, style: i32, on_monitor_bounds: bool, target: super::MonitorBounds) -> (i32, i32, i32, i32) {
        let width = rect.right.saturating_sub(rect.left);
        let height = rect.bottom.saturating_sub(rect.top);
        let popup = style & 0x8000_0000u32 as i32 != 0;
        let framed = style & (0x00c0_0000 | 0x0004_0000) != 0;
        let borderless = popup && !framed && on_monitor_bounds;
        let width = if borderless { target.width.min(i32::MAX as u32) as i32 } else { width };
        let height = if borderless { target.height.min(i32::MAX as u32) as i32 } else { height };
        let x = centered(target.x, target.width, width);
        let y = centered(target.y, target.height, height);
        (x, y, width, height)
    }

    fn placement(candidate: WindowCandidate, target: super::MonitorBounds) -> (i32, i32, i32, i32) {
        let monitor = unsafe { windows::Win32::Graphics::Gdi::MonitorFromWindow(
            windows::Win32::Foundation::HWND(candidate.key.hwnd as *mut _),
            windows::Win32::Graphics::Gdi::MONITOR_DEFAULTTONEAREST,
        ) };
        let mut info = windows::Win32::Graphics::Gdi::MONITORINFO { cbSize: std::mem::size_of::<windows::Win32::Graphics::Gdi::MONITORINFO>() as u32, ..Default::default() };
        let on_monitor_bounds = unsafe { windows::Win32::Graphics::Gdi::GetMonitorInfoW(monitor, &mut info) }.as_bool()
            && rect_matches_monitor(candidate.rect, info.rcMonitor);
        placement_for(candidate.rect, candidate.style, on_monitor_bounds, target)
    }

    fn rect_matches_monitor(rect: windows::Win32::Foundation::RECT, monitor: windows::Win32::Foundation::RECT) -> bool {
        rect.left.abs_diff(monitor.left) <= 2
            && rect.top.abs_diff(monitor.top) <= 2
            && rect.right.abs_diff(monitor.right) <= 2
            && rect.bottom.abs_diff(monitor.bottom) <= 2
    }

    fn centered(origin: i32, extent: u32, window: i32) -> i32 {
        let origin = i64::from(origin);
        let extent = i64::from(extent.min(i32::MAX as u32));
        let window = i64::from(window.max(1));
        let centered = origin + (extent - window) / 2;
        centered.max(origin).min(origin + (extent - window).max(0)) as i32
    }

    fn on_target_monitor(hwnd: windows::Win32::Foundation::HWND, target: super::MonitorBounds) -> bool {
        monitor_bounds_for_window(hwnd).is_some_and(|bounds| bounds == target)
    }

    fn monitor_bounds_for_window(hwnd: windows::Win32::Foundation::HWND) -> Option<super::MonitorBounds> {
        use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
        let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            return None;
        }
        let width = u32::try_from(i64::from(info.rcMonitor.right) - i64::from(info.rcMonitor.left)).ok()?;
        let height = u32::try_from(i64::from(info.rcMonitor.bottom) - i64::from(info.rcMonitor.top)).ok()?;
        Some(super::MonitorBounds { x: info.rcMonitor.left, y: info.rcMonitor.top, width, height })
    }

    #[cfg(test)]
    fn monitor_matches_target(rect: windows::Win32::Foundation::RECT, target: super::MonitorBounds) -> bool {
        rect.left == target.x
            && rect.top == target.y
            && i64::from(rect.right) - i64::from(rect.left) == i64::from(target.width)
            && i64::from(rect.bottom) - i64::from(rect.top) == i64::from(target.height)
    }

    fn current_or_last_monitor(current: Option<super::MonitorBounds>, last: Option<super::MonitorBounds>) -> Option<super::MonitorBounds> {
        current.or(last)
    }

    fn move_candidate(candidate: WindowCandidate, processes: &[ProcessInstance], target: super::MonitorBounds) -> bool {
        use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOZORDER};
        let current = candidates(processes);
        if current.len() != 1 || current[0].key != candidate.key || !verify_window(candidate, processes) {
            return false;
        }
        let hwnd = windows::Win32::Foundation::HWND(candidate.key.hwnd as *mut _);
        let (x, y, width, height) = placement(candidate, target);
        if unsafe { SetWindowPos(hwnd, None, x, y, width, height, SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_NOZORDER) }.is_err() {
            return false;
        }
        let current = candidates(processes);
        current.len() == 1
            && current[0].key == candidate.key
            && verify_window(candidate, processes)
            && on_target_monitor(hwnd, target)
    }

    pub fn game_monitor(game: &str) -> Option<super::MonitorBounds> {
        let state = game_processes_for(game)?;
        let snapshot = process_snapshot();
        let mut state = state.lock().unwrap();
        if state.cancelled {
            return None;
        }
        discover_children(&mut state.processes, &snapshot);
        let candidates = candidates(&state.processes);
        let current = unique_candidate(&candidates)
            .filter(|candidate| verify_window(*candidate, &state.processes))
            .and_then(|candidate| monitor_bounds_for_window(windows::Win32::Foundation::HWND(candidate.key.hwnd as *mut _)));
        if let Some(monitor) = current {
            state.last_monitor = Some(monitor);
        }
        current_or_last_monitor(current, state.last_monitor)
    }

    pub fn watch_game_window(game: String, target: super::MonitorBounds) {
        let Some(state) = game_processes_for(&game) else {
            crate::trail::note(format!("{game}: no retained game process for monitor placement"));
            return;
        };
        std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(45);
            let mut candidate_key = None;
            let mut candidate_since = Instant::now();
            let mut moved_key = None;
            let mut positioned_since = None;
            while Instant::now() < deadline {
                let snapshot = process_snapshot();
                let mut state = state.lock().unwrap();
                if state.cancelled {
                    break;
                }
                discover_children(&mut state.processes, &snapshot);
                let windows = candidates(&state.processes);
                // Waiting filters short-lived startup windows; Windows has no generic signal that
                // distinguishes a game's main window from a launcher's only stable main window.
                let unique = unique_candidate(&windows);
                if let Some(candidate) = unique {
                    let hwnd = windows::Win32::Foundation::HWND(candidate.key.hwnd as *mut _);
                    if let Some(monitor) = monitor_bounds_for_window(hwnd) {
                        state.last_monitor = Some(monitor);
                    }
                    if candidate_key != Some(candidate.key) {
                        candidate_key = Some(candidate.key);
                        candidate_since = Instant::now();
                    }
                    if moved_key == Some(candidate.key) && on_target_monitor(hwnd, target) {
                        if positioned_since.get_or_insert_with(Instant::now).elapsed() >= Duration::from_secs(2) {
                            crate::trail::note(format!("{game}: its window is on the selected Big Picture monitor"));
                            return;
                        }
                    } else if candidate_since.elapsed() >= Duration::from_secs(2) {
                        if move_candidate(candidate, &state.processes, target) {
                            moved_key = Some(candidate.key);
                            positioned_since = Some(Instant::now());
                            state.last_monitor = monitor_bounds_for_window(hwnd).or(Some(target));
                        } else {
                            moved_key = None;
                            positioned_since = None;
                        }
                    }
                } else {
                    candidate_key = None;
                    moved_key = None;
                    positioned_since = None;
                }
                let running = state.processes.iter().any(is_active_instance);
                drop(state);
                if !running {
                    break;
                }
                std::thread::sleep(Duration::from_millis(500));
            }
            if moved_key.is_none() && !state.lock().unwrap().cancelled {
                crate::trail::note(format!("{game}: no unique game window appeared for Big Picture monitor placement"));
            }
        });
    }

    #[cfg(test)]
    mod targeting_tests {
        use super::*;

        fn id(pid: u32, created: u64) -> ProcessIdentity {
            ProcessIdentity { pid, created }
        }

        fn entry(pid: u32, parent_pid: u32) -> ProcessEntry {
            ProcessEntry { pid, parent_pid }
        }

        #[test]
        fn active_tracked_parent_authorizes_a_child() {
            let parent = id(10, 100);
            assert!(child_discovery_authorized(entry(20, 10), &[parent], &[parent], 1));
        }

        #[test]
        fn exited_parent_and_reused_pid_do_not_authorize_a_child() {
            let original = id(10, 100);
            let replacement = id(10, 200);
            assert!(!child_discovery_authorized(entry(30, 10), &[original], &[], 1));
            assert!(!child_discovery_authorized(entry(30, 10), &[original], &[replacement], 1));
        }

        #[test]
        fn verified_child_remains_usable_after_its_parent_exits() {
            let root = id(10, 100);
            let child = id(20, 200);
            let tracked = [root, child];
            let active = [child];
            assert_eq!(tracked_active_process(child.pid, &tracked, &active), Some(child));
            assert!(child_discovery_authorized(entry(30, 20), &tracked, &active, 1));
        }

        #[test]
        fn historical_parent_without_a_live_verified_instance_is_rejected() {
            let historical = id(10, 100);
            assert!(!child_discovery_authorized(entry(20, 10), &[historical], &[], 1));
            assert!(!child_discovery_authorized(entry(20, 99), &[historical], &[historical], 1));
        }

        #[test]
        fn child_pid_reuse_with_a_different_parent_fails_snapshot_revalidation() {
            let original_snapshot = [entry(20, 10)];
            let replacement_snapshot = [entry(20, 99)];
            assert!(snapshot_parent_matches(20, 10, &original_snapshot));
            assert!(!snapshot_parent_matches(20, 10, &replacement_snapshot));
        }

        #[test]
        fn launcher_process_cannot_become_its_own_descendant() {
            let root = id(10, 100);
            assert!(!child_discovery_authorized(entry(10, 10), &[root], &[root], 10));
            assert!(!child_discovery_authorized(entry(0, 10), &[root], &[root], 1));
        }

        fn window(hwnd: isize) -> WindowCandidate {
            WindowCandidate {
                key: WindowKey { hwnd, pid: 10, created: 100 },
                rect: windows::Win32::Foundation::RECT { left: 0, top: 0, right: 800, bottom: 600 },
                style: 0x00cf_0000,
            }
        }

        #[test]
        fn candidate_selection_requires_exactly_one_window() {
            let first = window(1);
            assert_eq!(unique_candidate(&[first]).map(|c| c.key), Some(first.key));
            assert!(unique_candidate(&[first, window(2)]).is_none());
            assert!(unique_candidate(&[]).is_none());
        }

        #[test]
        fn windowed_placement_preserves_size_and_handles_negative_monitor_coordinates() {
            let target = super::super::MonitorBounds { x: -1920, y: -200, width: 1920, height: 1080 };
            let rect = windows::Win32::Foundation::RECT { left: 100, top: 100, right: 900, bottom: 700 };
            assert_eq!(placement_for(rect, 0x00cf_0000, false, target), (-1360, 40, 800, 600));
        }

        #[test]
        fn borderless_fullscreen_matches_selected_monitor_bounds() {
            let target = super::super::MonitorBounds { x: -1920, y: 0, width: 1920, height: 1080 };
            let rect = windows::Win32::Foundation::RECT { left: 0, top: 0, right: 3840, bottom: 2160 };
            assert_eq!(placement_for(rect, 0x8000_0000u32 as i32, true, target), (-1920, 0, 1920, 1080));
            assert_eq!(placement_for(rect, 0x8000_0000u32 as i32, false, target), (-1920, 0, 3840, 2160));
        }

        #[test]
        fn monitor_verification_preserves_negative_origin_and_full_dimensions() {
            let target = super::super::MonitorBounds { x: -1920, y: -200, width: 1920, height: 1080 };
            assert!(monitor_matches_target(windows::Win32::Foundation::RECT { left: -1920, top: -200, right: 0, bottom: 880 }, target));
            assert!(!monitor_matches_target(windows::Win32::Foundation::RECT { left: 0, top: 0, right: 1920, bottom: 1080 }, target));
        }

        #[test]
        fn live_game_monitor_replaces_cached_position_and_unavailable_windows_use_last_monitor() {
            let previous = super::super::MonitorBounds { x: 0, y: 0, width: 1920, height: 1080 };
            let current = super::super::MonitorBounds { x: -3840, y: 0, width: 3840, height: 2160 };
            assert_eq!(current_or_last_monitor(Some(current), Some(previous)), Some(current));
            assert_eq!(current_or_last_monitor(None, Some(previous)), Some(previous));
            assert_eq!(current_or_last_monitor(None, None), None);
        }
    }

    // ---------- a game's window icon ----------

    /// Puts the player's icon (PNG bytes) on the windows of the programs running from `dir` (the
    /// game's folder, or the folder its program is in): their title bar and taskbar button. Some
    /// games set their own icon again afterwards. Returns how many windows got it.
    pub fn set_window_icon(dir: &Path, png: &[u8]) -> usize {
        use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::*;
        let d = dir.to_string_lossy().trim_end_matches('\\').to_lowercase() + "\\";
        let mine = std::process::id();
        let pids: Vec<u32> = processes()
            .into_iter()
            .map(|(p, _)| p)
            .filter(|&p| p != mine && p != 0 && process_file(p).map(|f| f.to_string_lossy().to_lowercase().starts_with(&d)).unwrap_or(false))
            .collect();
        if pids.is_empty() {
            return 0;
        }
        let big = unsafe { CreateIconFromResourceEx(png, true, 0x0003_0000, 256, 256, LR_DEFAULTCOLOR) };
        let small = unsafe { CreateIconFromResourceEx(png, true, 0x0003_0000, 32, 32, LR_DEFAULTCOLOR) };
        let (Ok(big), Ok(small)) = (big, small) else { return 0 };
        struct Want {
            pids: Vec<u32>,
            found: Vec<HWND>,
        }
        unsafe extern "system" fn each(h: HWND, l: LPARAM) -> windows::core::BOOL {
            let w = unsafe { &mut *(l.0 as *mut Want) };
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(h, Some(&mut pid as *mut u32)) };
            // The program's own windows: shown, and not owned by another window (a dialog).
            if w.pids.contains(&pid) && unsafe { IsWindowVisible(h) }.as_bool() && unsafe { GetWindow(h, GW_OWNER) }.is_err() {
                w.found.push(h);
            }
            true.into()
        }
        let mut want = Want { pids, found: vec![] };
        let _ = unsafe { EnumWindows(Some(each), LPARAM(&mut want as *mut Want as isize)) };
        for h in &want.found {
            for (which, icon) in [(ICON_BIG, big), (ICON_SMALL, small)] {
                unsafe { SendMessageTimeoutW(*h, WM_SETICON, WPARAM(which as usize), LPARAM(icon.0 as isize), SMTO_ABORTIFHUNG, 500, None) };
            }
        }
        want.found.len()
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
        pub fn wait(mut self) -> Option<i32> {
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
            self.0 = 0;
            ok.then_some(code as i32)
        }
    }

    impl Elevated {
        /// Waits for it to close, or until `stop` is set: Some(exit code) when it closed, None
        /// when the wait was stopped (the program keeps running).
        pub fn wait_or(&self, stop: &std::sync::atomic::AtomicBool) -> Option<Option<i32>> {
            use windows::Win32::Foundation::WAIT_OBJECT_0;
            use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
            if self.0 == 0 {
                return Some(None);
            }
            let h = HANDLE(self.0 as *mut _);
            loop {
                if unsafe { WaitForSingleObject(h, 400) } == WAIT_OBJECT_0 {
                    let mut code = 0u32;
                    let ok = unsafe { GetExitCodeProcess(h, &mut code).is_ok() };
                    return Some(ok.then_some(code as i32));
                }
                if stop.load(std::sync::atomic::Ordering::Relaxed) {
                    return None;
                }
            }
        }
    }
    impl Drop for Elevated {
        fn drop(&mut self) {
            // wait() closes the handle itself and leaves 0 behind.
            if self.0 != 0 {
                let _ = unsafe { CloseHandle(HANDLE(self.0 as *mut _)) };
            }
        }
    }

    /// Starts a program the way double-clicking it does (ShellExecute): one that needs
    /// administrator rights gets Windows' prompt instead of failing with error 740.
    pub fn shell_open(exe: &Path, args: &str, dir: &Path) -> std::io::Result<Elevated> {
        use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let (f, p, d) = (HSTRING::from(exe), HSTRING::from(args), HSTRING::from(dir));
        let mut i = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
            lpFile: PCWSTR(f.as_ptr()),
            lpParameters: if args.is_empty() { PCWSTR::null() } else { PCWSTR(p.as_ptr()) },
            lpDirectory: PCWSTR(d.as_ptr()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        unsafe { ShellExecuteExW(&mut i) }.map_err(|e| std::io::Error::from_raw_os_error(e.code().0 & 0xFFFF))?;
        Ok(Elevated(i.hProcess.0 as isize))
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

    /// Every Visual Studio or Build Tools install's folder (each leaves a note of where it is),
    /// then the default places.
    fn vs_installs() -> Vec<PathBuf> {
        let mut out = vec![];
        let pd = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into());
        let notes = Path::new(&pd).join(r"Microsoft\VisualStudio\Packages\_Instances");
        for e in std::fs::read_dir(notes).into_iter().flatten().flatten() {
            if let Some(p) = std::fs::read(e.path().join("state.json")).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()).and_then(|v| v.get("installationPath").and_then(|p| p.as_str()).map(PathBuf::from)) {
                out.push(p);
            }
        }
        for pf in [std::env::var("ProgramFiles").ok(), std::env::var("ProgramFiles(x86)").ok()].into_iter().flatten() {
            let vs = Path::new(&pf).join("Microsoft Visual Studio");
            for year in std::fs::read_dir(&vs).into_iter().flatten().flatten() {
                for edition in std::fs::read_dir(year.path()).into_iter().flatten().flatten() {
                    out.push(edition.path());
                }
            }
        }
        out
    }

    /// Visual Studio's own Clang ("C++ Clang tools for Windows"), or an LLVM install.
    pub fn has_clang() -> bool {
        if vs_installs().iter().any(|p| p.join(r"VC\Tools\Llvm\x64\bin\clang-cl.exe").is_file() || p.join(r"VC\Tools\Llvm\bin\clang-cl.exe").is_file()) {
            return true;
        }
        let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
        Path::new(&pf).join(r"LLVM\bin\clang.exe").is_file() || on_path("clang.exe").is_some()
    }

    /// A program on the PATH. Windows' "App execution alias" stubs (WindowsApps), which open the
    /// Store instead, don't count.
    fn on_path(name: &str) -> Option<PathBuf> {
        std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).filter(|d| !d.to_string_lossy().to_lowercase().contains("windowsapps")).map(|d| d.join(name)).find(|f| f.is_file()))
    }

    /// The newest Python 3 there is ("3.12.4"), from the py launcher or a python.exe on the PATH.
    pub fn python_version() -> Option<String> {
        use std::os::windows::process::CommandExt;
        let run = |exe: &Path, args: &[&str]| -> Option<String> {
            let o = std::process::Command::new(exe).args(args).creation_flags(0x0800_0000).output().ok()?;
            let text = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
            text.split_whitespace().skip_while(|w| *w != "Python").nth(1).filter(|v| v.starts_with('3')).map(String::from)
        };
        let sys = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let py = Path::new(&sys).join("py.exe");
        let py = if py.is_file() { Some(py) } else { on_path("py.exe") };
        py.and_then(|p| run(&p, &["-3", "--version"])).or_else(|| on_path("python.exe").and_then(|p| run(&p, &["--version"])))
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

    /// The installed Visual C++ runtime's version (major, minor, build), when Windows says.
    pub fn vcredist_version() -> Option<(u32, u32, u32)> {
        [r"SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64", r"SOFTWARE\WOW6432Node\Microsoft\VisualStudio\14.0\VC\Runtimes\x64"]
            .iter()
            .find_map(|k| Some((reg_dword(HKLM, k, "Major")?, reg_dword(HKLM, k, "Minor")?, reg_dword(HKLM, k, "Bld").unwrap_or(0))))
    }

    const GPU_PREFS: &str = r"Software\Microsoft\DirectX\UserGpuPreferences";

    /// Windows' own graphics choice for one program (Settings → System → Display → Graphics):
    /// this program on the faster graphics card. For a laptop with two.
    pub fn prefer_fast_gpu(exe: &Path) -> std::io::Result<()> {
        use std::os::windows::process::CommandExt;
        let ok = std::process::Command::new("reg")
            .args(["add", &format!(r"HKCU\{GPU_PREFS}"), "/v"])
            .arg(win_path(exe))
            .args(["/t", "REG_SZ", "/d", "GpuPreference=2;", "/f"])
            .creation_flags(0x0800_0000)
            .status()?
            .success();
        if ok { Ok(()) } else { Err(std::io::Error::other("Windows didn't save the choice")) }
    }

    /// The program is set to the faster graphics card already.
    pub fn prefers_fast_gpu(exe: &Path) -> bool {
        reg_str(HKCU, GPU_PREFS, &win_path(exe)).map(|v| v.contains("GpuPreference=2")).unwrap_or(false)
    }

    /// The path as Windows writes it (a profile's program can come with forward slashes), which is
    /// how Windows matches the program to the choice.
    fn win_path(p: &Path) -> String {
        p.to_string_lossy().replace('/', "\\")
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

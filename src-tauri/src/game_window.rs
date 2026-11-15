//! Tracks game processes and positions their verified top-level windows on a selected monitor.

use crate::win::MonitorBounds;
use std::collections::HashMap;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{CloseHandle, DuplicateHandle, DUPLICATE_SAME_ACCESS, HANDLE, WAIT_TIMEOUT};
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
    last_monitor: Option<MonitorBounds>,
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

fn active_process(pid: u32, active: &[ProcessIdentity]) -> Option<ProcessIdentity> {
    active.iter().copied().find(|identity| identity.pid == pid)
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
    let active = active_identities(processes);
    list.0.into_iter().filter_map(|(hwnd, pid)| {
        let identity = active_process(pid, &active)?;
        let mut candidate = eligible_window(hwnd, pid)?;
        candidate.key.created = identity.created;
        Some(candidate)
    }).collect()
}

fn unique_candidate(candidates: &[WindowCandidate]) -> Option<WindowCandidate> {
    (candidates.len() == 1).then(|| candidates[0])
}

fn verify_window(candidate: WindowCandidate, processes: &[ProcessInstance]) -> bool {
    let hwnd = windows::Win32::Foundation::HWND(candidate.key.hwnd as *mut _);
    let Some(process) = processes.iter().find(|p| p.pid == candidate.key.pid && p.created == candidate.key.created) else { return false };
    is_active_instance(process)
        && eligible_window(hwnd, candidate.key.pid).is_some_and(|current| current.key.hwnd == candidate.key.hwnd)
}

fn placement_for(rect: windows::Win32::Foundation::RECT, style: i32, on_monitor_bounds: bool, target: MonitorBounds) -> (i32, i32, i32, i32) {
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

fn placement(candidate: WindowCandidate, target: MonitorBounds) -> (i32, i32, i32, i32) {
    let hwnd = windows::Win32::Foundation::HWND(candidate.key.hwnd as *mut _);
    let on_monitor_bounds = monitor_info_for_window(hwnd)
        .is_some_and(|info| rect_matches_monitor(candidate.rect, info.rcMonitor));
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

fn on_target_monitor(hwnd: windows::Win32::Foundation::HWND, target: MonitorBounds) -> bool {
    monitor_bounds_for_window(hwnd).is_some_and(|bounds| bounds == target)
}

fn monitor_bounds_for_window(hwnd: windows::Win32::Foundation::HWND) -> Option<MonitorBounds> {
    let info = monitor_info_for_window(hwnd)?;
    let width = u32::try_from(i64::from(info.rcMonitor.right) - i64::from(info.rcMonitor.left)).ok()?;
    let height = u32::try_from(i64::from(info.rcMonitor.bottom) - i64::from(info.rcMonitor.top)).ok()?;
    Some(MonitorBounds { x: info.rcMonitor.left, y: info.rcMonitor.top, width, height })
}

fn monitor_info_for_window(hwnd: windows::Win32::Foundation::HWND) -> Option<windows::Win32::Graphics::Gdi::MONITORINFO> {
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool().then_some(info)
}

#[cfg(test)]
fn monitor_matches_target(rect: windows::Win32::Foundation::RECT, target: MonitorBounds) -> bool {
    rect.left == target.x
        && rect.top == target.y
        && i64::from(rect.right) - i64::from(rect.left) == i64::from(target.width)
        && i64::from(rect.bottom) - i64::from(rect.top) == i64::from(target.height)
}

fn current_or_last_monitor(current: Option<MonitorBounds>, last: Option<MonitorBounds>) -> Option<MonitorBounds> {
    current.or(last)
}

fn move_candidate(candidate: WindowCandidate, processes: &[ProcessInstance], target: MonitorBounds) -> bool {
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

pub fn game_monitor(game: &str) -> Option<MonitorBounds> {
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

pub fn watch_game_window(game: String, target: MonitorBounds) {
    let Some(state) = game_processes_for(&game) else {
        crate::trail::note(format!("{game}: no retained game process for monitor placement"));
        return;
    };
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(45);
        let mut candidate_state: Option<(WindowKey, Instant)> = None;
        let mut positioned: Option<(WindowKey, Instant)> = None;
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
            if let Some(window) = unique {
                let hwnd = windows::Win32::Foundation::HWND(window.key.hwnd as *mut _);
                if let Some(monitor) = monitor_bounds_for_window(hwnd) {
                    state.last_monitor = Some(monitor);
                }
                if candidate_state.map(|(key, _)| key) != Some(window.key) {
                    candidate_state = Some((window.key, Instant::now()));
                    positioned = None;
                }
                let on_target_since = positioned
                    .filter(|(key, _)| *key == window.key && on_target_monitor(hwnd, target))
                    .map(|(_, since)| since);
                if let Some(since) = on_target_since {
                    if since.elapsed() >= Duration::from_secs(2) {
                        crate::trail::note(format!("{game}: its window is on the selected Big Picture monitor"));
                        return;
                    }
                } else if candidate_state.is_some_and(|(key, since)| key == window.key && since.elapsed() >= Duration::from_secs(2)) {
                    if move_candidate(window, &state.processes, target) {
                        positioned = Some((window.key, Instant::now()));
                        state.last_monitor = monitor_bounds_for_window(hwnd).or(Some(target));
                    } else {
                        positioned = None;
                    }
                }
            } else {
                candidate_state = None;
                positioned = None;
            }
            let running = state.processes.iter().any(is_active_instance);
            drop(state);
            if !running {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        if positioned.is_none() && !state.lock().unwrap().cancelled {
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
        assert_eq!(active_process(child.pid, &active), Some(child));
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
        let target = MonitorBounds { x: -1920, y: -200, width: 1920, height: 1080 };
        let rect = windows::Win32::Foundation::RECT { left: 100, top: 100, right: 900, bottom: 700 };
        assert_eq!(placement_for(rect, 0x00cf_0000, false, target), (-1360, 40, 800, 600));
    }

    #[test]
    fn borderless_fullscreen_matches_selected_monitor_bounds() {
        let target = MonitorBounds { x: -1920, y: 0, width: 1920, height: 1080 };
        let rect = windows::Win32::Foundation::RECT { left: 0, top: 0, right: 3840, bottom: 2160 };
        assert_eq!(placement_for(rect, 0x8000_0000u32 as i32, true, target), (-1920, 0, 1920, 1080));
        assert_eq!(placement_for(rect, 0x8000_0000u32 as i32, false, target), (-1920, 0, 3840, 2160));
    }

    #[test]
    fn monitor_verification_preserves_negative_origin_and_full_dimensions() {
        let target = MonitorBounds { x: -1920, y: -200, width: 1920, height: 1080 };
        assert!(monitor_matches_target(windows::Win32::Foundation::RECT { left: -1920, top: -200, right: 0, bottom: 880 }, target));
        assert!(!monitor_matches_target(windows::Win32::Foundation::RECT { left: 0, top: 0, right: 1920, bottom: 1080 }, target));
    }

    #[test]
    fn live_game_monitor_replaces_cached_position_and_unavailable_windows_use_last_monitor() {
        let previous = MonitorBounds { x: 0, y: 0, width: 1920, height: 1080 };
        let current = MonitorBounds { x: -3840, y: 0, width: 3840, height: 2160 };
        assert_eq!(current_or_last_monitor(Some(current), Some(previous)), Some(current));
        assert_eq!(current_or_last_monitor(None, Some(previous)), Some(previous));
        assert_eq!(current_or_last_monitor(None, None), None);
    }
}

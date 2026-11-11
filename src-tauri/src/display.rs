//! Multi-monitor detection and Big Picture display management.
//!
//! Enumerates connected monitors with their human-friendly names (via Windows CCD / EDID APIs on
//! Windows, or Tauri window monitors on other platforms). Manages temporary switching of the
//! Windows primary display when entering Big Picture mode so games start on the chosen TV/display,
//! and cleanly restores the original display configuration and launcher window placement upon exit.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize};

/// Information about a connected display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorInfo {
    /// Unique identifier: target hardware device path or GDI device name
    pub id: String,
    /// System device name (e.g. "\\\\.\\DISPLAY1")
    pub device_name: String,
    /// Friendly monitor model name (e.g. "Generic Monitor", "Monitor A", "Display 1")
    pub name: String,
    /// Full human-readable label (e.g. "Display 1 · Generic Monitor (1920×1080 @ 165Hz)")
    pub label: String,
    /// Whether this monitor is currently the primary display
    pub is_primary: bool,
    /// Resolution width
    pub width: u32,
    /// Resolution height
    pub height: u32,
    /// Refresh rate in Hz
    pub refresh_rate: u32,
    /// Virtual screen coordinate X
    pub x: i32,
    /// Virtual screen coordinate Y
    pub y: i32,
    /// DPI scaling factor
    pub scale_factor: f64,
}

/// Normal desktop window state saved before entering Big Picture mode.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PreTvWindowState {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

/// Active Big Picture display state.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ActiveTvState {
    pub window: PreTvWindowState,
    pub display_switched: bool,
    pub target_device: String,
    pub original_primary_device: String,
    pub original_positions: Vec<OriginalMonitorPos>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OriginalMonitorPos {
    pub device_name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub is_primary: bool,
}

static ACTIVE_TV: Mutex<Option<ActiveTvState>> = Mutex::new(None);

const SWITCH_MARKER_FILE: &str = "tv_display_switched.json";

/// Returns all connected monitors.
pub fn enumerate_monitors(app: &AppHandle) -> Vec<MonitorInfo> {
    #[cfg(windows)]
    {
        win_display::list_monitors(app)
    }
    #[cfg(not(windows))]
    {
        fallback_list_monitors(app)
    }
}

/// Fallback monitor enumeration using Tauri's cross-platform window API.
#[allow(dead_code)]
fn fallback_list_monitors(app: &AppHandle) -> Vec<MonitorInfo> {
    let Some(w) = app.get_webview_window("main") else {
        return Vec::new();
    };
    let Ok(monitors) = w.available_monitors() else {
        return Vec::new();
    };
    let primary = w.primary_monitor().ok().flatten();
    let primary_name = primary.as_ref().and_then(|p| p.name()).map(|s| s.to_string());

    monitors
        .into_iter()
        .enumerate()
        .map(|(idx, m)| {
            let m_name = m.name().unwrap_or("").to_string();
            let is_prim = primary_name.as_ref().map(|pn| pn == &m_name).unwrap_or(idx == 0);
            let pos = m.position();
            let size = m.size();
            let num = idx + 1;
            let friendly = if m_name.is_empty() {
                format!("Display {num}")
            } else {
                m_name.clone()
            };
            let label = if is_prim {
                format!("{friendly} ({}×{}) · Primary", size.width, size.height)
            } else {
                format!("{friendly} ({}×{})", size.width, size.height)
            };
            MonitorInfo {
                id: if m_name.is_empty() { format!("display_{num}") } else { m_name.clone() },
                device_name: m_name,
                name: friendly,
                label,
                is_primary: is_prim,
                width: size.width,
                height: size.height,
                refresh_rate: 60,
                x: pos.x,
                y: pos.y,
                scale_factor: m.scale_factor(),
            }
        })
        .collect()
}

/// Matches user monitor preference against currently connected monitors.
/// Falls back to the primary display if not found or if pref is empty/auto.
pub fn resolve_target_monitor(monitors: &[MonitorInfo], pref: Option<&str>) -> Option<MonitorInfo> {
    if monitors.is_empty() {
        return None;
    }

    let pref = pref.unwrap_or("").trim();
    if pref.is_empty() || pref.eq_ignore_ascii_case("auto") || pref.eq_ignore_ascii_case("primary") {
        return monitors.iter().find(|m| m.is_primary).cloned().or_else(|| monitors.first().cloned());
    }

    // 1. Match by exact ID (device path or unique ID)
    if let Some(m) = monitors.iter().find(|m| m.id.eq_ignore_ascii_case(pref)) {
        return Some(m.clone());
    }

    // 2. Match by device name (e.g. \\.\DISPLAY2)
    if let Some(m) = monitors.iter().find(|m| m.device_name.eq_ignore_ascii_case(pref)) {
        return Some(m.clone());
    }

    // 3. Match by friendly name (e.g. "Monitor A")
    if let Some(m) = monitors.iter().find(|m| m.name.eq_ignore_ascii_case(pref)) {
        return Some(m.clone());
    }

    // 4. Fallback to primary monitor if preferred monitor is disconnected/unavailable
    crate::trail::note(format!("Target monitor '{pref}' not found; falling back to primary display"));
    monitors.iter().find(|m| m.is_primary).cloned().or_else(|| monitors.first().cloned())
}

/// Prepares the window and display configuration for entering Big Picture mode.
///
/// 1. Captures the current desktop window position, size, and maximized state.
/// 2. Resolves the target monitor based on user settings (or primary fallback).
/// 3. If target monitor is not the primary display, temporarily sets it as primary on Windows.
/// 4. Positions the launcher window on the target monitor ready for fullscreen.
pub fn prepare_tv_mode(app: &AppHandle, monitor_pref: Option<String>) -> Result<MonitorInfo, String> {
    let w = app.get_webview_window("main").ok_or_else(|| "Main window not found".to_string())?;

    let data_dir = app_data_dir(app);
    let existing_state = ACTIVE_TV.lock().unwrap().take();

    let pre_window = if let Some(ref prev) = existing_state {
        // If display was previously switched, restore to baseline before switching to new target
        if prev.display_switched {
            #[cfg(windows)]
            {
                let _ = win_display::restore_primary(&prev.original_primary_device, &prev.original_positions, &data_dir);
            }
        }
        prev.window.clone()
    } else {
        // Capture desktop window placement before TV mode modifications
        let pos = w.outer_position().unwrap_or_default();
        let size = w.outer_size().unwrap_or_default();
        let maximized = w.is_maximized().unwrap_or(false);
        PreTvWindowState {
            x: pos.x,
            y: pos.y,
            width: size.width,
            height: size.height,
            maximized,
        }
    };

    let monitors = enumerate_monitors(app);
    let target = resolve_target_monitor(&monitors, monitor_pref.as_deref())
        .ok_or_else(|| "No display monitors detected".to_string())?;

    let mut state = ActiveTvState {
        window: pre_window,
        display_switched: false,
        target_device: target.device_name.clone(),
        original_primary_device: monitors.iter().find(|m| m.is_primary).map(|m| m.device_name.clone()).unwrap_or_default(),
        original_positions: monitors.iter().map(|m| OriginalMonitorPos {
            device_name: m.device_name.clone(),
            x: m.x,
            y: m.y,
            width: m.width,
            height: m.height,
            is_primary: m.is_primary,
        }).collect(),
    };

    #[cfg(windows)]
    {
        if !target.is_primary && !target.device_name.is_empty() {
            crate::trail::note(format!("Big Picture: switching primary display to {}", target.device_name));
            match win_display::switch_primary(&target.device_name, &state.original_positions, &data_dir) {
                Ok(true) => {
                    state.display_switched = true;
                    crate::trail::note("Big Picture: primary display switched successfully");
                }
                Ok(false) => {
                    crate::trail::note("Big Picture: target monitor is already primary");
                }
                Err(err) => {
                    crate::trail::note(format!("Big Picture: display switch failed ({err}), proceeding with window move"));
                }
            }
        }
    }

    // Position window on target monitor
    if state.display_switched {
        // After switching, the target monitor is at origin (0, 0)
        let _ = w.set_position(PhysicalPosition::new(0, 0));
    } else {
        let _ = w.set_position(PhysicalPosition::new(target.x, target.y));
    }
    let _ = w.set_size(PhysicalSize::new(target.width, target.height));

    *ACTIVE_TV.lock().unwrap() = Some(state);
    Ok(target)
}

/// Restores the original display configuration and returns the launcher window
/// to its original desktop monitor and placement.
pub fn leave_tv_mode(app: &AppHandle) -> Result<(), String> {
    let state = ACTIVE_TV.lock().unwrap().take();
    let data_dir = app_data_dir(app);

    // 1. Restore Windows primary display configuration if it was switched
    if let Some(ref st) = state {
        if st.display_switched {
            #[cfg(windows)]
            {
                crate::trail::note("Big Picture: restoring original primary display configuration");
                if let Err(e) = win_display::restore_primary(&st.original_primary_device, &st.original_positions, &data_dir) {
                    crate::trail::note(format!("Big Picture: restore display failed: {e}"));
                }
            }
        }
    }

    // 2. Restore normal desktop launcher window placement
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_fullscreen(false);

        if let Some(st) = state {
            let win = st.window;
            if win.width >= 400 && win.height >= 300 {
                let _ = w.unmaximize();
                let _ = w.set_size(PhysicalSize::new(win.width, win.height));
                let _ = w.set_position(PhysicalPosition::new(win.x, win.y));
                if win.maximized {
                    let _ = w.maximize();
                }
            }
        }
    }

    Ok(())
}

/// Emergency cleanup hook invoked on process exit or unexpected shutdown.
pub fn on_app_exit(app: &AppHandle) {
    let state = ACTIVE_TV.lock().unwrap().take();
    let data_dir = app_data_dir(app);

    if let Some(st) = state {
        if st.display_switched {
            #[cfg(windows)]
            {
                let _ = win_display::restore_primary(&st.original_primary_device, &st.original_positions, &data_dir);
            }
        }
    }
}

/// Checks on launcher startup if a previous session crashed while in Big Picture mode
/// and restores the display configuration if needed.
pub fn recover_switched_display_on_startup(dir: &Path) {
    #[cfg(windows)]
    {
        win_display::recover_from_marker(dir);
    }
}

fn app_data_dir(app: &AppHandle) -> PathBuf {
    app.try_state::<crate::App>()
        .map(|s| s.dir.clone())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
}

// ---------------------------------------------------------------------------
// Windows display implementation using Win32 GDI & CCD APIs
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod win_display {
    use super::*;
    use std::collections::HashMap;
    use std::mem::size_of;

    #[repr(C)]
    #[derive(Copy, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
    pub struct LUID {
        pub LowPart: u32,
        pub HighPart: i32,
    }

    #[repr(C)]
    #[derive(Copy, Clone, Default)]
    pub struct DISPLAYCONFIG_RATIONAL {
        pub Numerator: u32,
        pub Denominator: u32,
    }

    #[repr(C)]
    #[derive(Copy, Clone, Default)]
    pub struct DISPLAYCONFIG_PATH_SOURCE_INFO {
        pub adapterId: LUID,
        pub id: u32,
        pub modeInfoIdx: u32,
        pub statusFlags: u32,
    }

    #[repr(C)]
    #[derive(Copy, Clone, Default)]
    pub struct DISPLAYCONFIG_PATH_TARGET_INFO {
        pub adapterId: LUID,
        pub id: u32,
        pub modeInfoIdx: u32,
        pub outputTechnology: u32,
        pub rotation: u32,
        pub scaling: u32,
        pub refreshRate: DISPLAYCONFIG_RATIONAL,
        pub scanLineOrdering: u32,
        pub targetAvailable: u32,
        pub statusFlags: u32,
    }

    #[repr(C)]
    #[derive(Copy, Clone, Default)]
    pub struct DISPLAYCONFIG_PATH_INFO {
        pub sourceInfo: DISPLAYCONFIG_PATH_SOURCE_INFO,
        pub targetInfo: DISPLAYCONFIG_PATH_TARGET_INFO,
        pub flags: u32,
    }

    #[repr(C)]
    #[derive(Copy, Clone)]
    pub struct DISPLAYCONFIG_MODE_INFO {
        pub infoType: u32,
        pub id: u32,
        pub adapterId: LUID,
        pub modeInfo: [u8; 48],
    }

    #[repr(C)]
    #[derive(Copy, Clone)]
    pub struct DISPLAYCONFIG_TARGET_DEVICE_NAME {
        pub header_type: u32,
        pub header_size: u32,
        pub adapterId: LUID,
        pub id: u32,
        pub flags: u32,
        pub outputTechnology: u32,
        pub edidManufactureId: u16,
        pub edidProductCodeId: u16,
        pub connectorInstance: u32,
        pub monitorFriendlyDeviceName: [u16; 64],
        pub monitorDevicePath: [u16; 128],
    }

    #[repr(C)]
    #[derive(Copy, Clone)]
    pub struct DISPLAYCONFIG_SOURCE_DEVICE_NAME {
        pub header_type: u32,
        pub header_size: u32,
        pub adapterId: LUID,
        pub id: u32,
        pub viewGdiDeviceName: [u16; 32],
    }

    #[repr(C)]
    #[derive(Copy, Clone, Default, Serialize, Deserialize)]
    pub struct POINTL {
        pub x: i32,
        pub y: i32,
    }

    #[repr(C)]
    #[derive(Copy, Clone)]
    pub struct DEVMODEW {
        pub dmDeviceName: [u16; 32],
        pub dmSpecVersion: u16,
        pub dmDriverVersion: u16,
        pub dmSize: u16,
        pub dmDriverExtra: u16,
        pub dmFields: u32,
        pub dmPosition: POINTL,
        pub dmDisplayOrientation: u32,
        pub dmDisplayFixedOutput: u32,
        pub dmColor: i16,
        pub dmDuplex: i16,
        pub dmYResolution: i16,
        pub dmTTOption: i16,
        pub dmCollate: i16,
        pub dmFormName: [u16; 32],
        pub dmLogPixels: u16,
        pub dmBitsPerPel: u32,
        pub dmPelsWidth: u32,
        pub dmPelsHeight: u32,
        pub dmDisplayFlags: u32,
        pub dmDisplayFrequency: u32,
        pub dmICMMethod: u32,
        pub dmICMIntent: u32,
        pub dmMediaType: u32,
        pub dmDitherType: u32,
        pub dmReserved1: u32,
        pub dmReserved2: u32,
        pub dmPanningWidth: u32,
        pub dmPanningHeight: u32,
    }

    #[repr(C)]
    #[derive(Copy, Clone)]
    pub struct DISPLAY_DEVICEW {
        pub cb: u32,
        pub DeviceName: [u16; 32],
        pub DeviceString: [u16; 128],
        pub StateFlags: u32,
        pub DeviceID: [u16; 128],
        pub DeviceKey: [u16; 128],
    }

    const QDC_ONLY_SPECIFIED: u32 = 0x00000002;
    const ENUM_CURRENT_SETTINGS: i32 = -1;
    const CDS_UPDATEREGISTRY: u32 = 0x00000001;
    const CDS_SET_PRIMARY: u32 = 0x00000010;
    const CDS_NORESET: u32 = 0x10000000;
    const DM_POSITION: u32 = 0x00000020;
    const DISP_CHANGE_SUCCESSFUL: i32 = 0;

    #[link(name = "user32")]
    extern "system" {
        fn GetDisplayConfigBufferSizes(flags: u32, numPathArrayElements: *mut u32, numModeInfoArrayElements: *mut u32) -> i32;
        fn QueryDisplayConfig(flags: u32, numPathArrayElements: *mut u32, pathArray: *mut DISPLAYCONFIG_PATH_INFO, numModeInfoArrayElements: *mut u32, modeInfoArray: *mut DISPLAYCONFIG_MODE_INFO, currentTopologyId: *mut core::ffi::c_void) -> i32;
        fn DisplayConfigGetDeviceInfo(requestPacket: *mut core::ffi::c_void) -> i32;
        fn EnumDisplayDevicesW(lpDevice: *const u16, iDevNum: u32, lpDisplayDevice: *mut DISPLAY_DEVICEW, dwFlags: u32) -> i32;
        fn EnumDisplaySettingsW(lpszDeviceName: *const u16, iModeNum: i32, lpDevMode: *mut DEVMODEW) -> i32;
        fn ChangeDisplaySettingsExW(lpszDeviceName: *const u16, lpDevMode: *const DEVMODEW, hwnd: *mut core::ffi::c_void, dwflags: u32, lParam: *mut core::ffi::c_void) -> i32;
    }

    fn to_wstring(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn text(b: &[u16]) -> String {
        let n = b.iter().position(|&c| c == 0).unwrap_or(b.len());
        String::from_utf16_lossy(&b[..n]).trim().to_string()
    }

    /// Queries the Connecting and Configuring Displays (CCD) API for friendly monitor names
    /// (e.g. "Generic Monitor", "Monitor A") and hardware paths keyed by GDI device name ("\\\\.\\DISPLAY1").
    fn query_ccd_names() -> HashMap<String, (String, String)> {
        let mut map = HashMap::new();
        unsafe {
            let mut path_count = 0u32;
            let mut mode_count = 0u32;
            if GetDisplayConfigBufferSizes(QDC_ONLY_SPECIFIED, &mut path_count, &mut mode_count) != 0 || path_count == 0 {
                return map;
            }
            let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
            let mut modes = vec![DISPLAYCONFIG_MODE_INFO { infoType: 0, id: 0, adapterId: LUID::default(), modeInfo: [0u8; 48] }; mode_count as usize];
            if QueryDisplayConfig(QDC_ONLY_SPECIFIED, &mut path_count, paths.as_mut_ptr(), &mut mode_count, modes.as_mut_ptr(), std::ptr::null_mut()) != 0 {
                return map;
            }

            for path in &paths[..path_count as usize] {
                // Get GDI source device name
                let mut src = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
                    header_type: 1, // DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME
                    header_size: size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
                    adapterId: path.sourceInfo.adapterId,
                    id: path.sourceInfo.id,
                    viewGdiDeviceName: [0u16; 32],
                };
                if DisplayConfigGetDeviceInfo(&mut src as *mut _ as *mut core::ffi::c_void) != 0 {
                    continue;
                }
                let gdi = text(&src.viewGdiDeviceName);
                if gdi.is_empty() {
                    continue;
                }

                // Get target monitor friendly name
                let mut tgt = DISPLAYCONFIG_TARGET_DEVICE_NAME {
                    header_type: 2, // DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME
                    header_size: size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32,
                    adapterId: path.targetInfo.adapterId,
                    id: path.targetInfo.id,
                    flags: 0,
                    outputTechnology: 0,
                    edidManufactureId: 0,
                    edidProductCodeId: 0,
                    connectorInstance: 0,
                    monitorFriendlyDeviceName: [0u16; 64],
                    monitorDevicePath: [0u16; 128],
                };
                if DisplayConfigGetDeviceInfo(&mut tgt as *mut _ as *mut core::ffi::c_void) == 0 {
                    let friendly = text(&tgt.monitorFriendlyDeviceName);
                    let dev_path = text(&tgt.monitorDevicePath);
                    map.insert(gdi, (friendly, dev_path));
                }
            }
        }
        map
    }

    /// Enumerates all active display monitors with real friendly names, resolutions, and refresh rates.
    pub fn list_monitors(app: &AppHandle) -> Vec<MonitorInfo> {
        let ccd = query_ccd_names();
        let mut list = Vec::new();

        // Get scale factor mapping from Tauri window monitors if possible
        let tauri_monitors = app.get_webview_window("main").and_then(|w| w.available_monitors().ok()).unwrap_or_default();

        unsafe {
            let mut i = 0u32;
            loop {
                let mut dd = DISPLAY_DEVICEW {
                    cb: size_of::<DISPLAY_DEVICEW>() as u32,
                    DeviceName: [0u16; 32],
                    DeviceString: [0u16; 128],
                    StateFlags: 0,
                    DeviceID: [0u16; 128],
                    DeviceKey: [0u16; 128],
                };
                if EnumDisplayDevicesW(std::ptr::null(), i, &mut dd, 0) == 0 {
                    break;
                }
                i += 1;

                // DISPLAY_DEVICE_ATTACHED_TO_DESKTOP = 0x00000001
                if (dd.StateFlags & 0x00000001) == 0 {
                    continue;
                }

                let gdi = text(&dd.DeviceName);
                let is_primary = (dd.StateFlags & 0x00000004) != 0; // DISPLAY_DEVICE_PRIMARY_DEVICE

                // Read current display resolution and refresh rate
                let w_gdi = to_wstring(&gdi);
                let mut dm = DEVMODEW {
                    dmDeviceName: [0u16; 32],
                    dmSpecVersion: 0,
                    dmDriverVersion: 0,
                    dmSize: size_of::<DEVMODEW>() as u16,
                    dmDriverExtra: 0,
                    dmFields: 0,
                    dmPosition: POINTL { x: 0, y: 0 },
                    dmDisplayOrientation: 0,
                    dmDisplayFixedOutput: 0,
                    dmColor: 0,
                    dmDuplex: 0,
                    dmYResolution: 0,
                    dmTTOption: 0,
                    dmCollate: 0,
                    dmFormName: [0u16; 32],
                    dmLogPixels: 0,
                    dmBitsPerPel: 0,
                    dmPelsWidth: 0,
                    dmPelsHeight: 0,
                    dmDisplayFlags: 0,
                    dmDisplayFrequency: 0,
                    dmICMMethod: 0,
                    dmICMIntent: 0,
                    dmMediaType: 0,
                    dmDitherType: 0,
                    dmReserved1: 0,
                    dmReserved2: 0,
                    dmPanningWidth: 0,
                    dmPanningHeight: 0,
                };
                let has_settings = EnumDisplaySettingsW(w_gdi.as_ptr(), ENUM_CURRENT_SETTINGS, &mut dm) != 0;

                // Query monitor attached to this adapter
                let mut mon_dd = DISPLAY_DEVICEW {
                    cb: size_of::<DISPLAY_DEVICEW>() as u32,
                    DeviceName: [0u16; 32],
                    DeviceString: [0u16; 128],
                    StateFlags: 0,
                    DeviceID: [0u16; 128],
                    DeviceKey: [0u16; 128],
                };
                let _ = EnumDisplayDevicesW(w_gdi.as_ptr(), 0, &mut mon_dd, 0);
                let mon_id = text(&mon_dd.DeviceID);

                // Determine friendly name: CCD model name > monitor device ID > "Display N"
                let display_num = gdi.trim_start_matches(r"\\.\DISPLAY").parse::<u32>().unwrap_or(list.len() as u32 + 1);
                let (ccd_friendly, ccd_path) = ccd.get(&gdi).cloned().unwrap_or_default();

                let friendly = if !ccd_friendly.is_empty() {
                    ccd_friendly
                } else if !mon_id.is_empty() && !mon_id.contains("Default") {
                    mon_id.split('\\').nth(1).unwrap_or(&format!("Display {display_num}")).to_string()
                } else {
                    format!("Display {display_num}")
                };

                let unique_id = if !ccd_path.is_empty() {
                    ccd_path
                } else if !mon_id.is_empty() {
                    mon_id
                } else {
                    gdi.clone()
                };

                let width = if has_settings { dm.dmPelsWidth } else { 1920 };
                let height = if has_settings { dm.dmPelsHeight } else { 1080 };
                let refresh = if has_settings { dm.dmDisplayFrequency } else { 60 };
                let x = if has_settings { dm.dmPosition.x } else { 0 };
                let y = if has_settings { dm.dmPosition.y } else { 0 };

                // Match scale factor from Tauri monitors by position / device name
                let scale = tauri_monitors.iter()
                    .find(|tm| {
                        tm.name().map(|n| n == gdi).unwrap_or(false)
                            || (tm.position().x == x && tm.position().y == y)
                    })
                    .map(|tm| tm.scale_factor())
                    .unwrap_or(1.0);

                let label = if is_primary {
                    format!("Display {display_num} · {friendly} ({width}×{height} @ {refresh}Hz) · Primary")
                } else {
                    format!("Display {display_num} · {friendly} ({width}×{height} @ {refresh}Hz)")
                };

                list.push(MonitorInfo {
                    id: unique_id,
                    device_name: gdi,
                    name: friendly,
                    label,
                    is_primary,
                    width,
                    height,
                    refresh_rate: refresh,
                    x,
                    y,
                    scale_factor: scale,
                });
            }
        }

        // Primary display first, then by display number
        list.sort_by(|a, b| {
            b.is_primary.cmp(&a.is_primary)
                .then_with(|| a.device_name.cmp(&b.device_name))
        });

        list
    }

    /// Switches the Windows primary display so that `target_device` is at (0, 0)
    /// and all other active displays are shifted by `(-target.x, -target.y)`.
    pub fn switch_primary(target_device: &str, originals: &[OriginalMonitorPos], dir: &Path) -> Result<bool, String> {
        let target_pos = originals.iter().find(|m| m.device_name.eq_ignore_ascii_case(target_device))
            .ok_or_else(|| format!("Display device '{target_device}' not found in current layout"))?;

        if target_pos.is_primary {
            return Ok(false);
        }

        let dx = -target_pos.x;
        let dy = -target_pos.y;

        unsafe {
            // 1. Move target display to (0, 0) and set as primary
            let mut dm_target = zero_devmode();
            dm_target.dmFields = DM_POSITION;
            dm_target.dmPosition.x = 0;
            dm_target.dmPosition.y = 0;
            let w_target = to_wstring(&target_pos.device_name);

            let r_target = ChangeDisplaySettingsExW(
                w_target.as_ptr(),
                &dm_target,
                std::ptr::null_mut(),
                CDS_SET_PRIMARY | CDS_UPDATEREGISTRY | CDS_NORESET,
                std::ptr::null_mut(),
            );
            if r_target != DISP_CHANGE_SUCCESSFUL {
                return Err(format!("ChangeDisplaySettingsExW for target failed: {r_target}"));
            }

            // 2. Shift other displays to preserve relative layout
            for other in originals {
                if other.device_name.eq_ignore_ascii_case(&target_pos.device_name) {
                    continue;
                }
                let mut dm_other = zero_devmode();
                dm_other.dmFields = DM_POSITION;
                dm_other.dmPosition.x = other.x + dx;
                dm_other.dmPosition.y = other.y + dy;
                let w_other = to_wstring(&other.device_name);

                let r_other = ChangeDisplaySettingsExW(
                    w_other.as_ptr(),
                    &dm_other,
                    std::ptr::null_mut(),
                    CDS_UPDATEREGISTRY | CDS_NORESET,
                    std::ptr::null_mut(),
                );
                if r_other != DISP_CHANGE_SUCCESSFUL {
                    // Rollback on partial failure
                    let _ = ChangeDisplaySettingsExW(std::ptr::null(), std::ptr::null(), std::ptr::null_mut(), 0, std::ptr::null_mut());
                    return Err(format!("ChangeDisplaySettingsExW for {} failed: {r_other}", other.device_name));
                }
            }

            // 3. Commit the change
            let r_commit = ChangeDisplaySettingsExW(
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
            );
            if r_commit != DISP_CHANGE_SUCCESSFUL {
                return Err(format!("ChangeDisplaySettingsExW commit failed: {r_commit}"));
            }
        }

        // Write marker file for crash recovery
        let marker = dir.join(SWITCH_MARKER_FILE);
        if let Ok(bytes) = serde_json::to_vec_pretty(originals) {
            let _ = std::fs::write(marker, bytes);
        }

        Ok(true)
    }

    /// Restores the original Windows primary display and relative screen positions.
    pub fn restore_primary(orig_primary_device: &str, originals: &[OriginalMonitorPos], dir: &Path) -> Result<bool, String> {
        let prim = originals.iter().find(|m| m.is_primary || m.device_name.eq_ignore_ascii_case(orig_primary_device))
            .ok_or_else(|| "Original primary display not found in captured state".to_string())?;

        unsafe {
            // 1. Restore original primary display to (0, 0)
            let mut dm_prim = zero_devmode();
            dm_prim.dmFields = DM_POSITION;
            dm_prim.dmPosition.x = 0;
            dm_prim.dmPosition.y = 0;
            let w_prim = to_wstring(&prim.device_name);

            let r_prim = ChangeDisplaySettingsExW(
                w_prim.as_ptr(),
                &dm_prim,
                std::ptr::null_mut(),
                CDS_SET_PRIMARY | CDS_UPDATEREGISTRY | CDS_NORESET,
                std::ptr::null_mut(),
            );
            if r_prim != DISP_CHANGE_SUCCESSFUL {
                return Err(format!("Restore ChangeDisplaySettingsExW primary failed: {r_prim}"));
            }

            // 2. Restore all other displays to their original coordinates
            for other in originals {
                if other.device_name.eq_ignore_ascii_case(&prim.device_name) {
                    continue;
                }
                let mut dm_other = zero_devmode();
                dm_other.dmFields = DM_POSITION;
                dm_other.dmPosition.x = other.x;
                dm_other.dmPosition.y = other.y;
                let w_other = to_wstring(&other.device_name);

                let _ = ChangeDisplaySettingsExW(
                    w_other.as_ptr(),
                    &dm_other,
                    std::ptr::null_mut(),
                    CDS_UPDATEREGISTRY | CDS_NORESET,
                    std::ptr::null_mut(),
                );
            }

            // 3. Commit
            let r_commit = ChangeDisplaySettingsExW(
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
            );
            if r_commit != DISP_CHANGE_SUCCESSFUL {
                return Err(format!("Restore ChangeDisplaySettingsExW commit failed: {r_commit}"));
            }
        }

        // Clean up recovery marker file
        let _ = std::fs::remove_file(dir.join(SWITCH_MARKER_FILE));

        Ok(true)
    }

    /// Recovers original display configuration if an earlier session crashed.
    pub fn recover_from_marker(dir: &Path) {
        let marker = dir.join(SWITCH_MARKER_FILE);
        let Ok(bytes) = std::fs::read(&marker) else {
            return;
        };
        let Ok(originals) = serde_json::from_slice::<Vec<OriginalMonitorPos>>(&bytes) else {
            let _ = std::fs::remove_file(&marker);
            return;
        };
        if let Some(prim) = originals.iter().find(|m| m.is_primary) {
            crate::trail::note("Display recovery: restoring display configuration from previous session");
            let _ = restore_primary(&prim.device_name, &originals, dir);
        }
        let _ = std::fs::remove_file(&marker);
    }

    fn zero_devmode() -> DEVMODEW {
        DEVMODEW {
            dmDeviceName: [0u16; 32],
            dmSpecVersion: 0,
            dmDriverVersion: 0,
            dmSize: size_of::<DEVMODEW>() as u16,
            dmDriverExtra: 0,
            dmFields: 0,
            dmPosition: POINTL { x: 0, y: 0 },
            dmDisplayOrientation: 0,
            dmDisplayFixedOutput: 0,
            dmColor: 0,
            dmDuplex: 0,
            dmYResolution: 0,
            dmTTOption: 0,
            dmCollate: 0,
            dmFormName: [0u16; 32],
            dmLogPixels: 0,
            dmBitsPerPel: 0,
            dmPelsWidth: 0,
            dmPelsHeight: 0,
            dmDisplayFlags: 0,
            dmDisplayFrequency: 0,
            dmICMMethod: 0,
            dmICMIntent: 0,
            dmMediaType: 0,
            dmDitherType: 0,
            dmReserved1: 0,
            dmReserved2: 0,
            dmPanningWidth: 0,
            dmPanningHeight: 0,
        }
    }
}

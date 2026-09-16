use actuate::Result;
use serde::Serialize;
use windows_api::{
    Win32::{
        Foundation::{HWND, LPARAM, RECT},
        UI::{
            HiDpi::{
                DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
                SetThreadDpiAwarenessContext,
            },
            WindowsAndMessaging::*,
        },
    },
    core::BOOL,
};

/// Physical desktop pixels, including negative virtual-desktop origins.
#[derive(Debug, Clone, Serialize)]
pub struct Window {
    pub window_id: u64,
    pub pid: u32,
    pub title: String,
    pub visible: bool,
    pub minimized: bool,
    pub bounds: Option<Bounds>,
    pub bounds_error: Option<String>,
}
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub(crate) struct DpiGuard(DPI_AWARENESS_CONTEXT);
impl DpiGuard {
    pub fn new() -> Result<Self> {
        let previous =
            unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        if previous.0.is_null() {
            return Err(super::error(
                "dpi_context",
                "Cannot establish physical-pixel coordinates",
            ));
        }
        Ok(Self(previous))
    }
}
impl Drop for DpiGuard {
    fn drop(&mut self) {
        unsafe {
            SetThreadDpiAwarenessContext(self.0);
        }
    }
}

pub fn discover_windows() -> Result<Vec<Window>> {
    let _dpi = DpiGuard::new()?;
    unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> BOOL {
        // EnumWindows calls synchronously; the vector outlives every callback.
        let output = unsafe { &mut *(data.0 as *mut Vec<Window>) };
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
        }
        let len = unsafe { GetWindowTextLengthW(hwnd) }.max(0) as usize;
        let mut title = vec![0; len + 1];
        let copied = unsafe { GetWindowTextW(hwnd, &mut title) }.max(0) as usize;
        let mut rect = RECT::default();
        let result = unsafe { GetWindowRect(hwnd, &mut rect) };
        output.push(Window {
            window_id: hwnd.0 as usize as u64,
            pid,
            title: String::from_utf16_lossy(&title[..copied]),
            visible: unsafe { IsWindowVisible(hwnd) }.as_bool(),
            minimized: unsafe { IsIconic(hwnd) }.as_bool(),
            bounds: result.as_ref().ok().map(|_| Bounds {
                x: rect.left,
                y: rect.top,
                width: rect.right - rect.left,
                height: rect.bottom - rect.top,
            }),
            bounds_error: result.err().map(|e| e.to_string()),
        });
        true.into()
    }
    let mut output = Vec::new();
    unsafe {
        EnumWindows(
            Some(visit),
            LPARAM(&mut output as *mut Vec<Window> as isize),
        )
    }
    .map_err(super::native)?;
    Ok(output)
}

pub fn virtual_desktop() -> Result<Bounds> {
    let _dpi = DpiGuard::new()?;
    let bounds = unsafe {
        Bounds {
            x: GetSystemMetrics(SM_XVIRTUALSCREEN),
            y: GetSystemMetrics(SM_YVIRTUALSCREEN),
            width: GetSystemMetrics(SM_CXVIRTUALSCREEN),
            height: GetSystemMetrics(SM_CYVIRTUALSCREEN),
        }
    };
    if bounds.width <= 0 || bounds.height <= 0 {
        return Err(super::error("no_desktop", "No interactive virtual desktop"));
    }
    Ok(bounds)
}

/// Creation time distinguishes a reused process identifier from the original target.
pub(crate) fn process_generation(pid: u32) -> Result<u64> {
    use windows_api::Win32::{
        Foundation::{CloseHandle, FILETIME},
        System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
        .map_err(super::native)?;
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    let result =
        unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) };
    unsafe {
        let _ = CloseHandle(process);
    }
    result.map_err(super::native)?;
    Ok((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
}

pub fn displays() -> Result<serde_json::Value> {
    use windows_api::Win32::Graphics::Gdi::*;
    let _dpi = DpiGuard::new()?;
    unsafe extern "system" fn visit(
        monitor: HMONITOR,
        _dc: HDC,
        _rect: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let output = unsafe { &mut *(data.0 as *mut Vec<serde_json::Value>) };
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if !unsafe { GetMonitorInfoW(monitor, &mut info.monitorInfo) }.as_bool() {
            output.push(serde_json::json!({"display_id":monitor.0 as usize as u64,"error":"GetMonitorInfoW failed"}));
            return true.into();
        }
        let rect = info.monitorInfo.rcMonitor;
        let work = info.monitorInfo.rcWork;
        let end = info
            .szDevice
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(info.szDevice.len());
        output.push(serde_json::json!({"display_id":monitor.0 as usize as u64,"name":String::from_utf16_lossy(&info.szDevice[..end]),"primary":info.monitorInfo.dwFlags & 1 != 0,"bounds":{"x":rect.left,"y":rect.top,"width":rect.right-rect.left,"height":rect.bottom-rect.top},"work_area":{"x":work.left,"y":work.top,"width":work.right-work.left,"height":work.bottom-work.top},"coordinate_space":"physical_desktop_pixels"}));
        true.into()
    }
    let mut monitors = Vec::<serde_json::Value>::new();
    if !unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(visit),
            LPARAM(&mut monitors as *mut Vec<serde_json::Value> as isize),
        )
    }
    .as_bool()
    {
        return Err(super::error(
            "display_enumeration",
            "EnumDisplayMonitors failed",
        ));
    }
    Ok(
        serde_json::json!({"displays":monitors,"virtual_desktop":virtual_desktop()?,"coordinate_space":"physical_desktop_pixels"}),
    )
}

#[derive(Default)]
pub struct WindowDiscovery;
impl actuate::Discover for WindowDiscovery {
    fn discover(&mut self) -> actuate::Result<serde_json::Value> {
        let windows = discover_windows()?;
        let mut applications = std::collections::BTreeMap::<u32, serde_json::Value>::new();
        for window in &windows {
            let app = applications.entry(window.pid).or_insert_with(|| serde_json::json!({"pid":window.pid,"name":window.title,"windows":[],"visible_window_ids":[]}));
            app["windows"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!(window));
            if window.visible && !window.minimized {
                app["visible_window_ids"]
                    .as_array_mut()
                    .unwrap()
                    .push(serde_json::json!(window.window_id));
            }
        }
        let mut active_pid = 0;
        unsafe {
            windows_api::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(
                windows_api::Win32::UI::WindowsAndMessaging::GetForegroundWindow(),
                Some(&mut active_pid),
            );
        }
        Ok(
            serde_json::json!({"applications":applications.into_values().collect::<Vec<_>>(),"active_pid":active_pid,"coordinate_space":"physical_desktop_pixels"}),
        )
    }
}

//! Explicit Windows shell operations. Taskbar settings affect the shared shell.
use serde::Serialize;
use unimation::Result;
use windows_api::{
    Win32::{
        Foundation::RECT,
        UI::{
            Shell::{ABM_GETSTATE, ABM_SETSTATE, APPBARDATA, SHAppBarMessage},
            WindowsAndMessaging::{FindWindowW, GetWindowRect},
        },
    },
    core::w,
};

#[derive(Debug, Serialize)]
pub struct TaskbarState {
    pub window_id: u64,
    pub flags: u32,
    pub auto_hide: bool,
    pub bounds: super::Bounds,
}

pub fn taskbar_state() -> Result<TaskbarState> {
    let _dpi = super::windows::DpiGuard::new()?;
    unsafe {
        let hwnd = FindWindowW(w!("Shell_TrayWnd"), None).map_err(super::native)?;
        let mut data = APPBARDATA {
            cbSize: std::mem::size_of::<APPBARDATA>() as u32,
            hWnd: hwnd,
            ..Default::default()
        };
        let flags = SHAppBarMessage(ABM_GETSTATE, &mut data) as u32;
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect).map_err(super::native)?;
        Ok(TaskbarState {
            window_id: hwnd.0 as usize as u64,
            flags,
            auto_hide: flags & 1 != 0,
            bounds: super::Bounds {
                x: rect.left,
                y: rect.top,
                width: rect.right - rect.left,
                height: rect.bottom - rect.top,
            },
        })
    }
}

/// Set only the auto-hide bit, preserving the shell's other taskbar flags.
/// Returns the observed state; animation completion must be checked separately.
pub fn set_taskbar_auto_hide(enabled: bool) -> Result<TaskbarState> {
    let before = taskbar_state()?;
    unsafe {
        let mut data = APPBARDATA {
            cbSize: std::mem::size_of::<APPBARDATA>() as u32,
            hWnd: windows_api::Win32::Foundation::HWND(before.window_id as usize as *mut _),
            lParam: windows_api::Win32::Foundation::LPARAM(
                ((before.flags & !1) | u32::from(enabled)) as isize,
            ),
            ..Default::default()
        };
        SHAppBarMessage(ABM_SETSTATE, &mut data);
    }
    taskbar_state().map_err(|mut error| {
        error.effect = unimation::Effect::Unknown;
        error
    })
}

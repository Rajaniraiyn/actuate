//! Win32 presentation only. This module never synthesizes keyboard or pointer input.
//! Coordinates are physical pixels in the Windows virtual desktop coordinate system.
use overlay::{CursorCommand, CursorScope};
use std::{
    io::{self, BufRead},
    sync::mpsc,
    time::Duration,
};
use windows_api::{
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM},
        Graphics::{
            Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute},
            Gdi::*,
        },
        System::{Com::*, LibraryLoader::GetModuleHandleW},
        UI::{
            HiDpi::*,
            Shell::{IVirtualDesktopManager, VirtualDesktopManager},
            WindowsAndMessaging::*,
        },
    },
    core::{Error, Result, w},
};

// Binary entry point. Native resources stay on the COM/UI thread.
pub fn run() {
    if std::env::args().nth(1).as_deref() == Some("--cursor-visibility-guard") {
        if let Err(error) = cursor_guard::run() {
            eprintln!("Cursor visibility guard: {error}");
            std::process::exit(1);
        }
        return;
    }
    if let Err(error) = run_native() {
        eprintln!("Windows cursor renderer: {error}");
        std::process::exit(1);
    }
}

struct Environment {
    previous_dpi: DPI_AWARENESS_CONTEXT,
}
impl Environment {
    unsafe fn new() -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
            let previous_dpi =
                SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            if previous_dpi.0.is_null() {
                CoUninitialize();
                return Err(Error::from_thread());
            }
            Ok(Self { previous_dpi })
        }
    }
}
impl Drop for Environment {
    fn drop(&mut self) {
        unsafe {
            SetThreadDpiAwarenessContext(self.previous_dpi);
            CoUninitialize();
        }
    }
}

struct Window(HWND);
impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
unsafe fn create_window() -> Result<Window> {
    unsafe {
        let instance = HINSTANCE(GetModuleHandleW(None)?.0);
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: w!("ActuateCursorOverlay"),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(Error::from_thread());
        }
        Ok(Window(CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            class.lpszClassName,
            w!("Actuate cursor"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(instance),
            None,
        )?))
    }
}

/// RAII for a top-down BGRA DIB selected into its own memory DC.
struct Bitmap {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    width: u32,
    height: u32,
}
impl Bitmap {
    unsafe fn new(width: u32, height: u32) -> Result<Self> {
        unsafe {
            let dc = CreateCompatibleDC(None);
            if dc.0.is_null() {
                return Err(Error::from_thread());
            }
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width as i32,
                    biHeight: -(height as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bitmap = match CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
            {
                Ok(bitmap) => bitmap,
                Err(error) => {
                    let _ = DeleteDC(dc);
                    return Err(error);
                }
            };
            let old = SelectObject(dc, HGDIOBJ(bitmap.0));
            if old.0.is_null() || old.0 as isize == -1 {
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
                let _ = DeleteDC(dc);
                return Err(Error::from_thread());
            }
            Ok(Self {
                dc,
                bitmap,
                old,
                bits: bits.cast(),
                width,
                height,
            })
        }
    }
    unsafe fn upload(&mut self, rgba: &[u8], origin: POINT, clip: Option<RECT>) {
        assert_eq!(rgba.len(), self.width as usize * self.height as usize * 4);
        unsafe {
            let output = std::slice::from_raw_parts_mut(self.bits, rgba.len());
            for (i, (source, target)) in rgba
                .as_chunks::<4>()
                .0
                .iter()
                .zip(output.as_chunks_mut::<4>().0.iter_mut())
                .enumerate()
            {
                let x = origin.x as i64 + (i % self.width as usize) as i64;
                let y = origin.y as i64 + (i / self.width as usize) as i64;
                let outside = clip.is_some_and(|rect| {
                    x < rect.left as i64
                        || x >= rect.right as i64
                        || y < rect.top as i64
                        || y >= rect.bottom as i64
                });
                if outside {
                    target.fill(0);
                } else {
                    target.copy_from_slice(&[source[2], source[1], source[0], source[3]]);
                }
            }
        }
    }
    unsafe fn present(&self, hwnd: HWND, origin: POINT) -> Result<()> {
        let size = SIZE {
            cx: self.width as i32,
            cy: self.height as i32,
        };
        let source = POINT::default();
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
            ..Default::default()
        };
        unsafe {
            UpdateLayeredWindow(
                hwnd,
                None,
                Some(&origin),
                Some(&size),
                Some(self.dc),
                Some(&source),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            )
        }
    }
}
impl Drop for Bitmap {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.dc);
        }
    }
}

struct Placement {
    after: HWND,
    bounds: Option<RECT>,
}
unsafe fn placement(
    scope: CursorScope,
    overlay: HWND,
    desktops: Option<&IVirtualDesktopManager>,
) -> Option<Placement> {
    unsafe {
        let CursorScope::Window { window_id, pid } = scope else {
            return Some(Placement {
                after: HWND_TOPMOST,
                bounds: None,
            });
        };
        let raw = usize::try_from(window_id).ok()?;
        let target = HWND(raw as *mut _);
        if !IsWindow(Some(target)).as_bool()
            || !IsWindowVisible(target).as_bool()
            || IsIconic(target).as_bool()
            || GetAncestor(target, GA_ROOT) != target
        {
            return None;
        }
        let mut owner = 0;
        if GetWindowThreadProcessId(target, Some(&mut owner)) == 0 || owner != pid as u32 {
            return None;
        }
        if !desktops?
            .IsWindowOnCurrentVirtualDesktop(target)
            .ok()?
            .as_bool()
        {
            return None;
        }
        let mut cloaked = 0u32;
        DwmGetWindowAttribute(
            target,
            DWMWA_CLOAKED,
            (&mut cloaked as *mut u32).cast(),
            std::mem::size_of::<u32>() as u32,
        )
        .ok()?;
        if cloaked != 0 {
            return None;
        }
        let mut bounds = RECT::default();
        DwmGetWindowAttribute(
            target,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&mut bounds as *mut RECT).cast(),
            std::mem::size_of::<RECT>() as u32,
        )
        .ok()?;
        if bounds.right <= bounds.left || bounds.bottom <= bounds.top {
            return None;
        }
        // Insert immediately above the target, never above its occluders. Only
        // our HWND changes order. Do not make another application's window owner.
        let mut predecessor = GetWindow(target, GW_HWNDPREV).ok();
        if predecessor == Some(overlay) {
            predecessor = GetWindow(overlay, GW_HWNDPREV).ok();
        }
        let topmost = GetWindowLongW(target, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 != 0;
        let after = predecessor.unwrap_or(if topmost { HWND_TOPMOST } else { HWND_TOP });
        Some(Placement {
            after,
            bounds: Some(bounds),
        })
    }
}

mod cursor_guard;
mod visual;
fn guard_update(guard: &mut Option<cursor_guard::Guard>, hide: bool) -> Result<()> {
    if let Some(guard) = guard {
        guard.update(hide).map_err(|error| {
            Error::new(
                windows_api::core::HRESULT(0x80004005_u32 as i32),
                error.to_string(),
            )
        })?;
    }
    Ok(())
}
fn run_native() -> Result<()> {
    unsafe {
        let _environment = Environment::new()?;
        let mut policy = overlay::PhysicalCursorPolicy::Preserve;
        let mut tracking = false;
        for argument in std::env::args().skip(1) {
            match argument.as_str() {
                "--hide-cursor-within-scope" => {
                    policy = overlay::PhysicalCursorPolicy::HideWithinScope
                }
                "--hide-cursor-while-visible" => {
                    policy = overlay::PhysicalCursorPolicy::HideWhileVisible
                }
                "--track-physical-pointer" => tracking = true,
                _ => {
                    return Err(Error::new(
                        windows_api::core::HRESULT(0x80070057_u32 as i32),
                        "Unknown overlay option",
                    ));
                }
            }
        }
        let mut guard = if policy == overlay::PhysicalCursorPolicy::Preserve {
            None
        } else {
            Some(cursor_guard::Guard::start().map_err(|error| {
                Error::new(
                    windows_api::core::HRESULT(0x80004005_u32 as i32),
                    error.to_string(),
                )
            })?)
        };
        let window = create_window()?;
        // Register a transparent window with the shell before querying its
        // virtual desktop. A never-shown HWND may return TYPE_E_ELEMENTNOTFOUND.
        let mut initial = Bitmap::new(1, 1)?;
        initial.upload(&[0, 0, 0, 0], POINT::default(), None);
        initial.present(window.0, POINT::default())?;
        let _ = ShowWindow(window.0, SW_SHOWNOACTIVATE);
        let desktops: Option<IVirtualDesktopManager> =
            CoCreateInstance(&VirtualDesktopManager, None, CLSCTX_INPROC_SERVER).ok();
        let (sender, receiver) = mpsc::sync_channel(128);
        std::thread::spawn(move || {
            for line in io::stdin().lock().lines() {
                let Ok(line) = line else {
                    break;
                };
                match serde_json::from_str::<CursorCommand>(&line) {
                    Ok(command) => {
                        if let Err(error) = command.validate() {
                            eprintln!("Invalid cursor command: {error:?}");
                            continue;
                        }
                        let coordinates = match command {
                            CursorCommand::Move { x, y, .. } | CursorCommand::Click { x, y } => {
                                Some((x, y))
                            }
                            _ => None,
                        };
                        if coordinates.is_some_and(|(x, y)| x.abs() > 1e8 || y.abs() > 1e8) {
                            eprintln!("Cursor coordinate exceeds native desktop range");
                            continue;
                        }
                        if sender.send(command).is_err() {
                            break;
                        }
                    }
                    Err(error) => eprintln!("Invalid cursor command: {error}"),
                }
            }
        });
        let mut state = visual::State::new(std::time::Instant::now());
        let mut previous_scope = state.scope;
        let mut previous_bounds: Option<RECT> = None;
        let mut bitmap: Option<Bitmap> = None;
        let mut unavailable = false;
        loop {
            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                if message.message == WM_QUIT {
                    return Ok(());
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            // Rebase before applying incoming absolute desktop commands.
            if let Some(current) =
                placement(state.scope, window.0, desktops.as_ref()).and_then(|p| p.bounds)
            {
                if let Some(previous) = previous_bounds {
                    state.translate(
                        (current.left as i64 - previous.left as i64) as f64,
                        (current.top as i64 - previous.top as i64) as f64,
                    );
                }
                previous_bounds = Some(current);
            }
            for _ in 0..128 {
                match receiver.try_recv() {
                    Ok(command) => {
                        if !state.command(command, std::time::Instant::now()) {
                            return Ok(());
                        }
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
                }
            }
            if tracking && state.visible {
                let mut pointer = POINT::default();
                GetCursorPos(&mut pointer)?;
                state.track_pointer(
                    (pointer.x as f64, pointer.y as f64),
                    std::time::Instant::now(),
                );
            }
            if previous_scope != state.scope {
                let _ = ShowWindow(window.0, SW_HIDE);
                SetWindowPos(
                    window.0,
                    Some(HWND_NOTOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOOWNERZORDER,
                )?;
                previous_scope = state.scope;
                previous_bounds =
                    placement(state.scope, window.0, desktops.as_ref()).and_then(|p| p.bounds);
            }
            let placement = placement(state.scope, window.0, desktops.as_ref());
            if !state.visible || placement.is_none() {
                guard_update(&mut guard, false)?;
                let _ = ShowWindow(window.0, SW_HIDE);
                if state.visible && !unavailable {
                    eprintln!(
                        "Cursor hidden: target identity, visibility, frame bounds, or current virtual desktop could not be verified"
                    );
                }
                unavailable = state.visible;
                std::thread::sleep(Duration::from_millis(16));
                continue;
            }
            unavailable = false;
            let placement = placement.expect("checked above");
            // Move only our own window to the target's virtual desktop. This
            // never switches the user's desktop or relocates the target.
            if let Some(desktops) = desktops.as_ref() {
                let target = match state.scope {
                    CursorScope::Window { window_id, .. } => HWND(window_id as usize as *mut _),
                    CursorScope::Desktop => GetForegroundWindow(),
                };
                if matches!(state.scope, CursorScope::Window { .. })
                    && !desktops
                        .IsWindowOnCurrentVirtualDesktop(target)
                        .is_ok_and(|current| current.as_bool())
                {
                    guard_update(&mut guard, false)?;
                    let _ = ShowWindow(window.0, SW_HIDE);
                    std::thread::sleep(Duration::from_millis(16));
                    continue;
                }
                let target_desktop = match desktops.GetWindowDesktopId(target) {
                    Ok(id) => Some(id),
                    Err(_) if state.scope == CursorScope::Desktop => None,
                    Err(_) => {
                        guard_update(&mut guard, false)?;
                        let _ = ShowWindow(window.0, SW_HIDE);
                        std::thread::sleep(Duration::from_millis(16));
                        continue;
                    }
                };
                match desktops.GetWindowDesktopId(window.0) {
                    Ok(overlay_desktop)
                        if target_desktop.is_some_and(|id| overlay_desktop != id) =>
                    {
                        let _ = ShowWindow(window.0, SW_HIDE);
                        desktops.MoveWindowToDesktop(
                            window.0,
                            &target_desktop.expect("checked above"),
                        )?;
                    }
                    Ok(_) => {}
                    // Explorer does not register this nonactivating tool window
                    // as an application view. Gate its presentation by the
                    // verified target instead of requiring its own workspace ID.
                    Err(error) if error.code().0 == 0x8002802B_u32 as i32 => {}
                    Err(_) => {
                        guard_update(&mut guard, false)?;
                        let _ = ShowWindow(window.0, SW_HIDE);
                        std::thread::sleep(Duration::from_millis(16));
                        continue;
                    }
                }
            }
            let now = std::time::Instant::now();
            let position = state.position(now);
            let old_half = bitmap.as_ref().map_or(48, |b| b.width as i32 / 2);
            SetWindowPos(
                window.0,
                Some(placement.after),
                position.0.round() as i32 - old_half,
                position.1.round() as i32 - old_half,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOOWNERZORDER,
            )?;
            let dpi = GetDpiForWindow(window.0);
            if dpi == 0 {
                return Err(Error::from_thread());
            }
            let image = state
                .raster(now, (dpi as f64 / 96.).clamp(0.5, 8.))
                .ok_or_else(Error::from_thread)?;
            if bitmap
                .as_ref()
                .is_none_or(|b| b.width != image.width() || b.height != image.height())
            {
                bitmap = Some(Bitmap::new(image.width(), image.height())?);
            }
            let bitmap = bitmap.as_mut().expect("allocated above");
            let origin = POINT {
                x: position.0.round() as i32 - bitmap.width as i32 / 2,
                y: position.1.round() as i32 - bitmap.height as i32 / 2,
            };
            bitmap.upload(image.data(), origin, placement.bounds);
            bitmap.present(window.0, origin)?;
            let _ = ShowWindow(window.0, SW_SHOWNOACTIVATE);
            let mut hide = policy != overlay::PhysicalCursorPolicy::Preserve;
            if let CursorScope::Window { window_id, .. } = state.scope {
                let target = HWND(window_id as usize as *mut _);
                let visible_at = |point: POINT| {
                    let hit = WindowFromPoint(point);
                    hit == window.0 || GetAncestor(hit, GA_ROOT) == target
                };
                // A covered soft hotspot cannot substitute for the real pointer.
                hide &= visible_at(POINT {
                    x: position.0.round() as i32,
                    y: position.1.round() as i32,
                });
                if policy == overlay::PhysicalCursorPolicy::HideWithinScope {
                    let mut pointer = POINT::default();
                    hide &= GetCursorPos(&mut pointer).is_ok() && visible_at(pointer);
                }
            }
            guard_update(&mut guard, hide)?;
            std::thread::sleep(Duration::from_millis(16));
        }
    }
}

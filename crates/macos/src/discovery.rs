//! Stateless process enumeration and an optional legacy front-process probe.
//! Avoid NSWorkspace's notification-driven caches in a synchronous library host.
use crate::error;
use actuate::{Effect, Result};
use objc2::rc::Retained;
use objc2_app_kit::NSRunningApplication;

pub(crate) fn applications() -> Result<Vec<Retained<NSRunningApplication>>> {
    // SAFETY: libproc allows a null buffer for a size query.
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if count <= 0 {
        return Err(error(
            "process_query",
            "Cannot enumerate processes",
            Effect::None,
        ));
    }
    let mut capacity = count as usize + 128;
    for _ in 0..3 {
        let mut pids = vec![0i32; capacity];
        let bytes = i32::try_from(pids.len() * std::mem::size_of::<i32>())
            .map_err(|_| error("process_query", "Process buffer too large", Effect::None))?;
        // SAFETY: valid writable PID buffer with the supplied byte size.
        let read = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
        if read < 0 {
            return Err(error("process_query", "Process query failed", Effect::None));
        }
        if read as usize >= capacity {
            capacity *= 2;
            continue;
        }
        pids.truncate(read as usize);
        pids.sort_unstable();
        pids.dedup();
        return Ok(pids
            .into_iter()
            .filter(|pid| *pid > 0)
            .filter_map(NSRunningApplication::runningApplicationWithProcessIdentifier)
            .collect());
    }
    Err(error(
        "process_query",
        "Process table changed repeatedly during enumeration",
        Effect::None,
    ))
}

/// Deprecated public Process Manager entry points remain an optional provider.
/// Their signatures are checked against the installed HIServices Processes.h.
/// This avoids pumping caller-owned run loops just to refresh AppKit isActive.
pub(crate) fn frontmost_pid() -> Option<i32> {
    #[repr(C)]
    struct ProcessSerialNumber {
        high: u32,
        low: u32,
    }
    type Front = unsafe extern "C" fn(*mut ProcessSerialNumber) -> i16;
    type Pid = unsafe extern "C" fn(*const ProcessSerialNumber, *mut i32) -> i32;
    // SAFETY: optional runtime symbol lookup, exact SDK signatures, valid out pointers.
    unsafe {
        let front = libc::dlsym(libc::RTLD_DEFAULT, c"GetFrontProcess".as_ptr());
        let pid = libc::dlsym(libc::RTLD_DEFAULT, c"GetProcessPID".as_ptr());
        if front.is_null() || pid.is_null() {
            return None;
        }
        let front = std::mem::transmute::<*mut libc::c_void, Front>(front);
        let getpid = std::mem::transmute::<*mut libc::c_void, Pid>(pid);
        let mut serial = ProcessSerialNumber { high: 0, low: 0 };
        let mut pid = 0;
        if front(&mut serial) == 0 && getpid(&serial, &mut pid) == 0 && pid > 0 {
            Some(pid)
        } else {
            None
        }
    }
}

/// Native on-screen membership, independent of activation policy or app naming.
pub(crate) fn visible_windows() -> Result<std::collections::BTreeMap<i32, Vec<u32>>> {
    use objc2_core_foundation::{CFDictionary, CFNumber, CFString, CFType};
    use objc2_core_graphics::{CGWindowListCopyWindowInfo, CGWindowListOption};
    let array =
        CGWindowListCopyWindowInfo(CGWindowListOption::OptionOnScreenOnly, 0).ok_or_else(|| {
            error(
                "window_query",
                "On-screen window metadata unavailable",
                Effect::None,
            )
        })?;
    let mut result = std::collections::BTreeMap::<i32, Vec<u32>>::new();
    for index in 0..array.count() {
        // SAFETY: Window Server returns a live array of CF dictionaries.
        let value = unsafe { &*array.value_at_index(index).cast::<CFType>() };
        let Some(dictionary) = value.downcast_ref::<CFDictionary>() else {
            continue;
        };
        let number = |name: &str| -> Option<i64> {
            let key = CFString::from_str(name);
            // SAFETY: key and dictionary stay alive for the lookup and typed borrow.
            let pointer = unsafe { dictionary.value((&*key as *const CFString).cast()) };
            if pointer.is_null() {
                return None;
            }
            unsafe { &*pointer.cast::<CFType>() }
                .downcast_ref::<CFNumber>()?
                .as_i64()
        };
        if let (Some(pid), Some(id)) = (number("kCGWindowOwnerPID"), number("kCGWindowNumber"))
            && let (Ok(pid), Ok(id)) = (i32::try_from(pid), u32::try_from(id))
        {
            result.entry(pid).or_default().push(id);
        }
    }
    Ok(result)
}

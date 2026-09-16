//! Optional WindowServer ordering groups. No foreign NSWindow objects are made.
//! A missing symbol or stale owner suppresses the visual, never input delivery.
use objc2_core_foundation::{CFBoolean, CFDictionary, CFNumber, CFString, CFType, CGRect};
use objc2_core_graphics::{CGWindowListCopyWindowInfo, CGWindowListOption};
use overlay::CursorScope;

type Connection = unsafe extern "C" fn() -> i32;
type Attach = unsafe extern "C" fn(i32, u32, u32, i32) -> i32;
type Detach = unsafe extern "C" fn(i32, u32) -> i32;
type Order = unsafe extern "C" fn(i32, u32, i32, u32) -> i32;
pub struct Groups {
    handle: *mut libc::c_void,
    connection: Connection,
    attach: Attach,
    detach: Detach,
    order: Order,
}
impl Groups {
    pub fn load() -> Option<Self> {
        // SAFETY: framework stays loaded until all typed function pointers are dropped.
        unsafe {
            let handle = libc::dlopen(
                c"/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight".as_ptr(),
                libc::RTLD_NOW | libc::RTLD_LOCAL,
            );
            if handle.is_null() {
                return None;
            }
            let connection = libc::dlsym(handle, c"SLSMainConnectionID".as_ptr());
            let attach = libc::dlsym(handle, c"SLSAddWindowToWindowOrderingGroup".as_ptr());
            let detach = libc::dlsym(handle, c"SLSRemoveFromOrderingGroup".as_ptr());
            let order = libc::dlsym(handle, c"SLSOrderWindow".as_ptr());
            if [connection, attach, detach, order]
                .iter()
                .any(|p| p.is_null())
            {
                libc::dlclose(handle);
                return None;
            }
            let connection = std::mem::transmute::<*mut libc::c_void, Connection>(connection);
            Some(Self {
                handle,
                connection,
                attach: std::mem::transmute::<*mut libc::c_void, Attach>(attach),
                detach: std::mem::transmute::<*mut libc::c_void, Detach>(detach),
                order: std::mem::transmute::<*mut libc::c_void, Order>(order),
            })
        }
    }
    pub fn attach(&self, parent: u32, child: u32) -> bool {
        // SAFETY: signatures checked against host SkyLight disassembly; IDs are
        // validated before this call. Order only our own child, never the target.
        unsafe {
            (self.attach)((self.connection)(), parent, child, 1) == 0
                && (self.order)((self.connection)(), child, 1, parent) == 0
        }
    }
    pub fn detach(&self, child: u32) {
        // SAFETY: child belongs to this renderer's connection.
        unsafe {
            (self.detach)((self.connection)(), child);
        }
    }
}
impl Drop for Groups {
    fn drop(&mut self) {
        unsafe {
            libc::dlclose(self.handle);
        }
    }
}
pub struct Window {
    pub bounds: CGRect,
    pub level: isize,
    pub on_screen: bool,
}
fn value<'a>(dict: &'a CFDictionary, name: &str) -> Option<&'a CFType> {
    let key = CFString::from_str(name);
    // SAFETY: dictionary owns its CF values for the returned borrow's lifetime.
    let raw = unsafe { dict.value((&*key as *const CFString).cast()) };
    if raw.is_null() {
        None
    } else {
        Some(unsafe { &*raw.cast::<CFType>() })
    }
}
pub fn window(scope: CursorScope) -> Option<Window> {
    let CursorScope::Window { window_id, pid } = scope else {
        return None;
    };
    let window_id = u32::try_from(window_id).ok()?;
    let array = CGWindowListCopyWindowInfo(CGWindowListOption::OptionIncludingWindow, window_id)?;
    if array.count() != 1 {
        return None;
    }
    // SAFETY: WindowServer returns CF dictionary objects in this retained array.
    let dict =
        unsafe { &*array.value_at_index(0).cast::<CFType>() }.downcast_ref::<CFDictionary>()?;
    let number = |key| value(dict, key)?.downcast_ref::<CFNumber>()?.as_i64();
    if number("kCGWindowOwnerPID")? != i64::from(pid)
        || number("kCGWindowNumber")? != i64::from(window_id)
    {
        return None;
    }
    let bounds = value(dict, "kCGWindowBounds")?.downcast_ref::<CFDictionary>()?;
    let mut rect = CGRect::default();
    // SAFETY: retained dictionary and valid out pointer.
    if !unsafe {
        objc2_core_graphics::CGRectMakeWithDictionaryRepresentation(Some(bounds), &mut rect)
    } {
        return None;
    }
    Some(Window {
        bounds: rect,
        level: number("kCGWindowLayer")? as isize,
        on_screen: value(dict, "kCGWindowIsOnscreen")?
            .downcast_ref::<CFBoolean>()?
            .value(),
    })
}

/// Validate compositor order rather than treating a successful private call as
/// a persistent attachment. Cross-process raises can dissolve ordering groups.
pub fn correctly_ordered(parent: u32, child: u32) -> bool {
    let Some(array) = CGWindowListCopyWindowInfo(CGWindowListOption::OptionOnScreenOnly, 0) else {
        return false;
    };
    let mut previous = None;
    for index in 0..array.count() {
        // SAFETY: WindowServer owns CF dictionary values in the live array.
        let object = unsafe { &*array.value_at_index(index).cast::<CFType>() };
        let Some(dict) = object.downcast_ref::<CFDictionary>() else {
            continue;
        };
        let id = value(dict, "kCGWindowNumber")
            .and_then(|v| v.downcast_ref::<CFNumber>())
            .and_then(|v| v.as_i64());
        if id == Some(i64::from(parent)) {
            return previous == Some(i64::from(child));
        }
        previous = id;
    }
    false
}

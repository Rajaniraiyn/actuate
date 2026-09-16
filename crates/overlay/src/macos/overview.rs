//! Dock's AX notifications describe Mission Control, app Exposé and Show Desktop.
//! The observer's lifetime owns its run-loop source; no callback borrows State.
use objc2_app_kit::NSRunningApplication;
use objc2_application_services::{AXError, AXObserver, AXUIElement};
use objc2_core_foundation::{
    CFRetained, CFRunLoop, CFRunLoopSource, CFString, kCFRunLoopCommonModes,
};
use objc2_foundation::NSString;
use std::{
    ptr::NonNull,
    sync::atomic::{AtomicBool, Ordering},
};
static ACTIVE: AtomicBool = AtomicBool::new(false);
const EVENTS: [&str; 4] = [
    "AXExposeShowAllWindows",
    "AXExposeShowFrontWindows",
    "AXExposeShowDesktop",
    "AXExposeExit",
];
unsafe extern "C-unwind" fn event(
    _: NonNull<AXObserver>,
    _: NonNull<AXUIElement>,
    name: NonNull<CFString>,
    _: *mut libc::c_void,
) {
    // SAFETY: AX callback arguments are borrowed for this invocation.
    let name = unsafe { name.as_ref() }.to_string();
    ACTIVE.store(name != "AXExposeExit", Ordering::Relaxed);
}
pub struct Overview {
    _observer: CFRetained<AXObserver>,
    app: objc2::rc::Retained<NSRunningApplication>,
    source: CFRetained<CFRunLoopSource>,
}
impl Overview {
    pub fn observe() -> Option<Self> {
        let apps = NSRunningApplication::runningApplicationsWithBundleIdentifier(
            &NSString::from_str("com.apple.dock"),
        );
        let app = apps.firstObject().or_else(|| {
            eprintln!("overlay: Dock unavailable");
            None
        })?;
        // SAFETY: a live NSRunningApplication supplies a positive process ID.
        let element = unsafe { AXUIElement::new_application(app.processIdentifier()) };
        let mut raw = std::ptr::null_mut();
        // SAFETY: valid out pointer, static callback, no borrowed refcon.
        unsafe {
            if AXObserver::create(
                app.processIdentifier(),
                Some(event),
                NonNull::from(&mut raw),
            ) != AXError::Success
            {
                return None;
            }
            let observer = CFRetained::from_raw(NonNull::new(raw)?);
            for name in EVENTS {
                let status = observer.add_notification(
                    &element,
                    &CFString::from_str(name),
                    std::ptr::null_mut(),
                );
                if status != AXError::Success {
                    eprintln!("overlay: {name}: {status:?}");
                    return None;
                }
            }
            let source = observer.run_loop_source();
            CFRunLoop::main()?.add_source(Some(&source), kCFRunLoopCommonModes);
            ACTIVE.store(false, Ordering::Relaxed);
            Some(Self {
                app,
                _observer: observer,
                source,
            })
        }
    }
    pub fn alive(&self) -> bool {
        !self.app.isTerminated()
    }
    pub fn active(&self) -> bool {
        ACTIVE.load(Ordering::Relaxed)
    }
}
impl Drop for Overview {
    fn drop(&mut self) {
        self.source.invalidate();
    }
}

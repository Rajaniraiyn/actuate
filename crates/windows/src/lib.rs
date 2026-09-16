//! Windows providers. UI Automation sessions belong to their creating MTA thread.
#![cfg(target_os = "windows")]

mod accessibility;
mod capture;
mod input;
mod session;
pub mod shell;
mod windows;

pub use accessibility::Accessibility;
pub use capture::{DesktopCapture, capture_desktop};
pub use input::GlobalInput;
pub use session::{
    AccessibilityCapabilities, HorizontalDirection, InputCapabilities, InputRoute,
    VerticalDirection, WheelCapabilities, WindowsAccessibility, WindowsInput, WindowsSession,
};
pub use windows::{Bounds, Window, WindowDiscovery, discover_windows, displays, virtual_desktop};

use actuate::{Effect, NativeError};
fn error(code: &str, message: impl Into<String>) -> NativeError {
    NativeError {
        code: code.into(),
        message: message.into(),
        effect: Effect::None,
    }
}
fn native(error_value: windows_api::core::Error) -> NativeError {
    error(
        "windows_api",
        format!("{} ({:#x})", error_value, error_value.code().0),
    )
}
fn dispatched(route: &str) -> actuate::Receipt {
    actuate::Receipt {
        effect: Effect::Dispatched,
        route: route.into(),
    }
}

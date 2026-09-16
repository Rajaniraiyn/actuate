//! Linux providers are independent: AT-SPI can run on X11 or Wayland;
//! XTEST and X11 capture require an explicitly available X11 desktop.
#![cfg(target_os = "linux")]

pub mod accessibility;
pub mod provider;
pub mod session;
pub use provider::{AccessibilityProvider, DesktopProvider, Environment, Frame, SessionKind};
pub mod wayland;
pub mod x11;
pub use accessibility::Accessibility;
pub use session::{LinuxRequest, LinuxSession};
pub use x11::X11;

use unimation::{Effect, NativeError};
pub(crate) fn error(code: &str, message: impl ToString) -> NativeError {
    NativeError {
        code: code.into(),
        message: message.to_string(),
        effect: Effect::None,
    }
}
pub(crate) fn unsupported(message: impl ToString) -> NativeError {
    error("unsupported_route", message)
}

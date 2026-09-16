//! Linux providers: AT-SPI2 observation and semantic actions, Wayland and
//! X11 input and capture routes, Hyprland window discovery, and the shared
//! session. Native handles stay inside their provider; see docs/linux.md.
#![cfg(target_os = "linux")]

pub mod atspi;
pub mod hyprland_windows;
pub mod keymap;
pub mod session;
pub mod wayland_capture;
pub mod wayland_input;
pub mod x11;
pub use atspi::AtSpi;
pub use session::LinuxSession;

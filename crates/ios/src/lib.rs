//! Composable iOS and iPadOS providers. Device family is a runtime property.
//! CoreSimulator support is hosted on macOS. Optional physical-device services
//! use usbmuxd on supported hosts; neither route requires our own guest helper.
#[cfg(feature = "physical")]
pub mod physical;

#[cfg(target_os = "macos")]
#[path = "simulator_backend.rs"]
mod simulator_backend;
#[cfg(target_os = "macos")]
pub use simulator_backend::*;

//! iOS and iPadOS Simulator providers. Device family is a runtime property.
//! Native bridge support is hosted on macOS; no installed guest helper is required.
#[cfg(target_os = "macos")]
mod accessibility;
#[cfg(target_os = "macos")]
pub use accessibility::SimulatorAccessibility;

pub mod simulator;

//! iOS and iPadOS Simulator providers. Device family is a runtime property.
//! Native bridge support is hosted on macOS; no installed guest helper is required.
#![cfg(target_os = "macos")]

mod accessibility;
pub use accessibility::{SimulatorAccessibility, SimulatorScope};

pub mod simulator;

pub mod hid;
pub use hid::SimulatorHid;

pub mod providers;

pub mod jsonl;
pub mod session;

#[cfg(feature = "cli")]
pub mod cli;

pub mod pointer;
pub use pointer::SimulatorPointer;

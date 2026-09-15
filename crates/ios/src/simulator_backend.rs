#[path = "accessibility.rs"]
mod accessibility;
pub use accessibility::{SimulatorAccessibility, SimulatorScope};

#[path = "simulator.rs"]
pub mod simulator;

#[path = "hid.rs"]
pub mod hid;
pub use hid::SimulatorHid;

#[path = "providers.rs"]
pub mod providers;

#[path = "jsonl.rs"]
pub mod jsonl;
#[path = "session.rs"]
pub mod session;

#[cfg(feature = "cli")]
#[path = "cli.rs"]
pub mod cli;

#[path = "pointer.rs"]
pub mod pointer;
pub use pointer::SimulatorPointer;

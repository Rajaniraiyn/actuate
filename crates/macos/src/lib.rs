//! Direct macOS providers using objc2 framework bindings. Optional visual overlay helper.
//! Native objects stay on the creating thread; no unsafe Send/Sync implementations.
#![cfg(target_os = "macos")]

mod accessibility;
mod input;
pub use accessibility::Accessibility;
pub use input::QuartzInput;

fn error(
    code: impl ToString,
    message: impl ToString,
    effect: unimation::Effect,
) -> unimation::NativeError {
    unimation::NativeError {
        code: code.to_string(),
        message: message.to_string(),
        effect,
    }
}

pub mod capture;
pub mod skylight;

pub mod target;

pub mod session;

#[cfg(feature = "native-capture")]
pub mod capture_native;

mod discovery;

pub mod overlay;

mod actionability;

impl unimation::ObserveScope for Accessibility {
    type Scope = i32;
    fn observe_scope(
        &mut self,
        pid: i32,
        budget: unimation::ObservationBudget,
    ) -> unimation::Result<unimation::Snapshot> {
        unimation::Observe::observe(
            self,
            unimation::ObserveRequest {
                pid,
                max_nodes: budget.max_nodes,
                max_depth: budget.max_depth,
            },
        )
    }
}

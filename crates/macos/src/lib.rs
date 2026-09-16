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
    unimation::NativeError::new(code.to_string(), message).with_effect(effect)
}

pub mod capture;
pub mod skylight;

pub mod spaces;
pub mod target;

pub mod session;

#[cfg(feature = "native-capture")]
pub mod capture_native;

mod discovery;

mod actionability;

impl unimation::ObserveScope for Accessibility {
    type Scope = i32;
    fn observe_scope(
        &mut self,
        pid: i32,
        budget: unimation::ObservationBudget,
    ) -> unimation::Result<unimation::Snapshot> {
        unimation::Observe::observe(self, unimation::ObserveRequest::new(pid, budget))
    }
}

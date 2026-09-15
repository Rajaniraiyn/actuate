//! Direct macOS providers using objc2 framework bindings. Optional visual overlay helper.
//! Native objects stay on the creating thread; no unsafe Send/Sync implementations.
#[cfg(target_os = "macos")]
mod accessibility;
#[cfg(target_os = "macos")]
mod input;
#[cfg(target_os = "macos")]
pub use accessibility::Accessibility;
#[cfg(target_os = "macos")]
pub use input::QuartzInput;

#[cfg(target_os = "macos")]
fn error(
    code: impl ToString,
    message: impl ToString,
    effect: unimation_core::Effect,
) -> unimation_core::NativeError {
    unimation_core::NativeError {
        code: code.to_string(),
        message: message.to_string(),
        effect,
    }
}

#[cfg(target_os = "macos")]
pub mod capture;
#[cfg(target_os = "macos")]
pub mod skylight;

#[cfg(target_os = "macos")]
pub mod target;

#[cfg(target_os = "macos")]
pub mod session;

#[cfg(all(target_os = "macos", feature = "native-capture"))]
pub mod capture_native;

#[cfg(target_os = "macos")]
mod discovery;

#[cfg(target_os = "macos")]
pub mod overlay;

#[cfg(target_os = "macos")]
mod actionability;

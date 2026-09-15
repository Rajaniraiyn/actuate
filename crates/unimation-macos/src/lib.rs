//! Direct macOS providers using objc2 framework bindings. No helper runtime.
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

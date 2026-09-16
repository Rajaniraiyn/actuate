//! Linux desktop primitives shared by providers and the cursor renderer:
//! Hyprland IPC and Wayland client plumbing. Nothing here dispatches input.
#![cfg(target_os = "linux")]
pub mod hyprland;
pub mod wayland;
pub use hyprland::Hyprland;

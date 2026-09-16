//! Provider contracts keep compositor transport out of the session protocol.
use crate::{X11, unsupported};
use serde::Serialize;
use serde_json::{Value, json};
use std::path::Path;
use unimation::{
    Delivery, Discover, KeyChord, KeyboardInput, Observe, Point, PointerInput, Receipt, Result,
    SemanticActions, geometry::FrameMapping,
};

pub trait AccessibilityProvider: Discover + Observe + SemanticActions {
    fn descriptor(&self) -> Value {
        json!({"implementation":"custom","operations":null})
    }
}
impl AccessibilityProvider for crate::Accessibility {
    fn descriptor(&self) -> Value {
        json!({"implementation":"atspi_dbus","operations":["discover","snapshot","semantic"],"coordinate_space":"atspi_screen"})
    }
}

#[derive(Debug, Serialize)]
pub struct Frame {
    pub path: String,
    pub mapping: FrameMapping,
    pub coordinate_space: &'static str,
    pub cursor_included: bool,
}
/// Implementations own their session and permission lifecycle. Input remains
/// explicit global/process delivery; unsupported operations must never fall back.
pub trait DesktopProvider: PointerInput + KeyboardInput<Key = KeyChord> {
    fn wayland(&mut self, _command: crate::wayland::Command) -> Result<Value> {
        Err(unsupported("This provider has no Wayland portal session"))
    }
    fn route(&self) -> &'static str;
    fn descriptor(&self) -> Value {
        json!({"implementation":self.route(),"operations":null})
    }
    fn windows(&mut self) -> Result<Value>;
    fn displays(&mut self) -> Result<Value>;
    fn capture_root(&mut self, path: &Path) -> Result<Frame>;
    fn text(&mut self, _delivery: Delivery, _text: &str) -> Result<Receipt> {
        Err(unsupported(
            "Desktop text input is not implemented; use AT-SPI SetString attribute=text on an editable reference",
        ))
    }
    fn wheel(
        &mut self,
        _vertical: i32,
        _horizontal: i32,
        _point: Option<Point>,
    ) -> Result<Receipt> {
        Err(unsupported(
            "This desktop provider has no X11 wheel-detent extension",
        ))
    }
}
impl DesktopProvider for X11 {
    fn route(&self) -> &'static str {
        "x11"
    }
    fn descriptor(&self) -> Value {
        json!({"implementation":"x11rb_xtest","scope":"connected_x11_server","operations":["windows","displays","capture","pointer","key","x11_wheel"],"delivery":"global","text":false,"pixel_scroll":false,"coordinate_space":"x11_root_pixels"})
    }
    fn windows(&mut self) -> Result<Value> {
        X11::windows(self)
    }
    fn displays(&mut self) -> Result<Value> {
        X11::displays(self)
    }
    fn capture_root(&mut self, path: &Path) -> Result<Frame> {
        X11::capture_root(self, path)
    }
    fn wheel(&mut self, vertical: i32, horizontal: i32, point: Option<Point>) -> Result<Receipt> {
        X11::wheel(self, vertical, horizontal, point)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    X11,
    Wayland,
    HeadlessOrUnknown,
}
#[derive(Debug, Serialize)]
pub struct Environment {
    pub kind: SessionKind,
    pub x11_display: Option<String>,
    pub wayland_display: Option<String>,
    pub desktop: Option<String>,
}
impl Environment {
    pub fn detect() -> Self {
        let x11_display = std::env::var("DISPLAY").ok().filter(|s| !s.is_empty());
        let wayland_display = std::env::var("WAYLAND_DISPLAY")
            .ok()
            .filter(|s| !s.is_empty());
        let session_type = std::env::var("XDG_SESSION_TYPE").ok();
        let kind = Self::classify(
            session_type.as_deref(),
            x11_display.is_some(),
            wayland_display.is_some(),
        );
        Self {
            kind,
            x11_display,
            wayland_display,
            desktop: std::env::var("XDG_CURRENT_DESKTOP").ok(),
        }
    }
    fn classify(session_type: Option<&str>, x11: bool, wayland: bool) -> SessionKind {
        if wayland || session_type == Some("wayland") {
            SessionKind::Wayland
        } else if x11 {
            SessionKind::X11
        } else {
            SessionKind::HeadlessOrUnknown
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn xwayland_does_not_become_full_desktop() {
        assert_eq!(
            Environment::classify(Some("wayland"), true, false),
            SessionKind::Wayland
        );
        assert_eq!(
            Environment::classify(None, true, true),
            SessionKind::Wayland
        );
        assert_eq!(Environment::classify(None, true, false), SessionKind::X11);
        assert_eq!(
            Environment::classify(None, false, false),
            SessionKind::HeadlessOrUnknown
        );
    }
}

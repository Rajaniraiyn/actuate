//! Capability composition without platform switches or implicit fallback routes.
use crate::{
    Capture, ElementRef, NativeError, Receipt, Result, SemanticAction, SemanticActions, Snapshot,
};
use serde::{Deserialize, Serialize};

/// No capability implementation. Missing providers are rejected by trait bounds.
#[derive(Debug, Default)]
pub struct Unavailable;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationBudget {
    pub max_nodes: usize,
    pub max_depth: usize,
}
impl Default for ObservationBudget {
    fn default() -> Self {
        Self {
            max_nodes: crate::default_max_nodes(),
            max_depth: crate::default_max_depth(),
        }
    }
}
/// The provider owns the scope type. A process ID is not imposed on mobile scenes.
pub trait ObserveScope {
    type Scope;
    fn observe_scope(&mut self, scope: Self::Scope, budget: ObservationBudget) -> Result<Snapshot>;
}
pub trait ObservedInteraction: ObserveScope + SemanticActions {}
impl<T: ObserveScope + SemanticActions + ?Sized> ObservedInteraction for T {}

/// A normalized point on the provider's raw, unrotated primary input framebuffer.
/// Conversion from AX points, rotated display pixels or another display is explicit.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(try_from = "PointWire", into = "PointWire")]
pub struct NormalizedPoint {
    x: f64,
    y: f64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PointWire {
    x: f64,
    y: f64,
}
impl NormalizedPoint {
    pub fn new(x: f64, y: f64) -> Result<Self> {
        if [x, y]
            .into_iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(&v))
        {
            Ok(Self { x, y })
        } else {
            Err(NativeError::new(
                "invalid_coordinates",
                "Normalized coordinates must be finite and between 0 and 1",
            ))
        }
    }
    pub fn x(self) -> f64 {
        self.x
    }
    pub fn y(self) -> f64 {
        self.y
    }
}
impl TryFrom<PointWire> for NormalizedPoint {
    type Error = NativeError;
    fn try_from(v: PointWire) -> Result<Self> {
        Self::new(v.x, v.y)
    }
}
impl From<NormalizedPoint> for PointWire {
    fn from(v: NormalizedPoint) -> Self {
        Self { x: v.x, y: v.y }
    }
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TouchEdge {
    #[default]
    None,
    Left,
    Top,
    Bottom,
    Right,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TouchAction {
    Tap {
        point: NormalizedPoint,
    },
    Swipe {
        from: NormalizedPoint,
        to: NormalizedPoint,
        duration_ms: u64,
        #[serde(default)]
        edge: TouchEdge,
    },
}
pub trait TouchInput {
    fn touch(&mut self, action: TouchAction) -> Result<Receipt>;
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HardwareButton {
    Home,
    Lock,
    VolumeUp,
    VolumeDown,
}
pub trait HardwareButtons {
    fn press_button(&mut self, button: HardwareButton) -> Result<Receipt>;
}
/// USB HID usage page 0x07. Values are not host virtual keycodes or Unicode.
pub trait HidKeyboard {
    fn press_usage(&mut self, usage: u16, modifiers: &[u16]) -> Result<Receipt>;
}
pub trait TouchKeyboard: TouchInput + HidKeyboard {}
impl<T: TouchInput + HidKeyboard + ?Sized> TouchKeyboard for T {}
/// Relative motion of a provider-selected native pointer, never touch emulation.
/// Service activation and its device ownership are provider-specific contracts.
pub trait RelativePointerInput {
    fn move_relative(&mut self, dx: f64, dy: f64) -> Result<Receipt>;
    fn pointer_button(&mut self, button: u8, down: bool) -> Result<Receipt>;
}
/// A visual cursor is independent of pointer or touch injection. Status must say
/// whether rendering is confirmed or only queued.
pub trait CursorVisualization {
    type Command;
    type Status;
    fn visualize(&mut self, command: Self::Command) -> Result<Self::Status>;
}
pub trait AppLifecycle {
    type App;
    fn launch_app(&mut self, app: Self::App) -> Result<Receipt>;
}

/// Observation and semantic actions share a native reference owner. Input,
/// capture, cursor rendering and app lifecycle are independently replaceable.
///
/// ```compile_fail
/// let mut backend = actuate::Backend::new(());
/// // No TouchInput implementation exists for Unavailable.
/// actuate::TouchInput::touch(&mut backend, actuate::TouchAction::Tap {
///     point: actuate::NormalizedPoint::new(0.5, 0.5).unwrap()
/// });
/// ```
pub struct Backend<O, I = Unavailable, C = Unavailable, V = Unavailable, A = Unavailable> {
    pub observation: O,
    pub input: I,
    pub capture: C,
    pub cursor: V,
    pub apps: A,
}
impl<O> Backend<O> {
    pub fn new(observation: O) -> Self {
        Self {
            observation,
            input: Unavailable,
            capture: Unavailable,
            cursor: Unavailable,
            apps: Unavailable,
        }
    }
}
impl<O, I, C, V, A> Backend<O, I, C, V, A> {
    pub fn with_input<N>(self, input: N) -> Backend<O, N, C, V, A> {
        Backend {
            observation: self.observation,
            input,
            capture: self.capture,
            cursor: self.cursor,
            apps: self.apps,
        }
    }
    pub fn with_capture<N>(self, capture: N) -> Backend<O, I, N, V, A> {
        Backend {
            observation: self.observation,
            input: self.input,
            capture,
            cursor: self.cursor,
            apps: self.apps,
        }
    }
    pub fn with_cursor<N>(self, cursor: N) -> Backend<O, I, C, N, A> {
        Backend {
            observation: self.observation,
            input: self.input,
            capture: self.capture,
            cursor,
            apps: self.apps,
        }
    }
    pub fn with_apps<N>(self, apps: N) -> Backend<O, I, C, V, N> {
        Backend {
            observation: self.observation,
            input: self.input,
            capture: self.capture,
            cursor: self.cursor,
            apps,
        }
    }
}
impl<O: ObserveScope, I, C, V, A> ObserveScope for Backend<O, I, C, V, A> {
    type Scope = O::Scope;
    fn observe_scope(&mut self, s: Self::Scope, b: ObservationBudget) -> Result<Snapshot> {
        self.observation.observe_scope(s, b)
    }
}
impl<O: SemanticActions, I, C, V, A> SemanticActions for Backend<O, I, C, V, A> {
    fn semantic(&mut self, t: &ElementRef, a: SemanticAction) -> Result<Receipt> {
        self.observation.semantic(t, a)
    }
}
impl<O, I: TouchInput, C, V, A> TouchInput for Backend<O, I, C, V, A> {
    fn touch(&mut self, a: TouchAction) -> Result<Receipt> {
        self.input.touch(a)
    }
}
impl<O, I: HardwareButtons, C, V, A> HardwareButtons for Backend<O, I, C, V, A> {
    fn press_button(&mut self, b: HardwareButton) -> Result<Receipt> {
        self.input.press_button(b)
    }
}
impl<O, I: HidKeyboard, C, V, A> HidKeyboard for Backend<O, I, C, V, A> {
    fn press_usage(&mut self, u: u16, m: &[u16]) -> Result<Receipt> {
        self.input.press_usage(u, m)
    }
}
impl<O, I, C: Capture, V, A> Capture for Backend<O, I, C, V, A> {
    type Request = C::Request;
    type Frame = C::Frame;
    fn capture(&mut self, r: Self::Request) -> Result<Self::Frame> {
        self.capture.capture(r)
    }
}
impl<O, I, C, V: CursorVisualization, A> CursorVisualization for Backend<O, I, C, V, A> {
    type Command = V::Command;
    type Status = V::Status;
    fn visualize(&mut self, c: Self::Command) -> Result<Self::Status> {
        self.cursor.visualize(c)
    }
}
impl<O, I, C, V, A: AppLifecycle> AppLifecycle for Backend<O, I, C, V, A> {
    type App = A::App;
    fn launch_app(&mut self, a: Self::App) -> Result<Receipt> {
        self.apps.launch_app(a)
    }
}

impl<O, I: crate::PointerInput, C, V, A> crate::PointerInput for Backend<O, I, C, V, A> {
    fn pointer(&mut self, d: crate::Delivery, a: crate::PointerAction) -> Result<Receipt> {
        self.input.pointer(d, a)
    }
}
impl<O, I: crate::TextInput, C, V, A> crate::TextInput for Backend<O, I, C, V, A> {
    fn type_text(&mut self, d: crate::Delivery, t: &str) -> Result<Receipt> {
        self.input.type_text(d, t)
    }
}
impl<O, I: crate::KeyboardInput, C, V, A> crate::KeyboardInput for Backend<O, I, C, V, A> {
    type Key = I::Key;
    fn key_press(&mut self, d: crate::Delivery, k: Self::Key) -> Result<Receipt> {
        self.input.key_press(d, k)
    }
}

impl<O, I: RelativePointerInput, C, V, A> RelativePointerInput for Backend<O, I, C, V, A> {
    fn move_relative(&mut self, dx: f64, dy: f64) -> Result<Receipt> {
        self.input.move_relative(dx, dy)
    }
    fn pointer_button(&mut self, button: u8, down: bool) -> Result<Receipt> {
        self.input.pointer_button(button, down)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Effect;
    #[test]
    fn coordinates_cannot_bypass_validation_via_json() {
        for input in [r#"{"x":2,"y":0}"#, r#"{"x":0,"y":0,"space":"pixels"}"#] {
            assert!(serde_json::from_str::<NormalizedPoint>(input).is_err());
        }
        assert!(NormalizedPoint::new(f64::NAN, 0.).is_err());
        let p = NormalizedPoint::new(0.5, 1.).unwrap();
        assert_eq!(
            serde_json::from_value::<NormalizedPoint>(serde_json::to_value(p).unwrap()).unwrap(),
            p
        );
    }
    struct Input(usize);
    impl TouchInput for Input {
        fn touch(&mut self, _: TouchAction) -> Result<Receipt> {
            self.0 += 1;
            Ok(Receipt {
                effect: Effect::Dispatched,
                route: "test".into(),
            })
        }
    }
    #[test]
    fn replacing_a_provider_preserves_other_state() {
        let mut b = Backend::new("reference owner")
            .with_input(Input(0))
            .with_capture(42);
        b.touch(TouchAction::Tap {
            point: NormalizedPoint::new(0.1, 0.2).unwrap(),
        })
        .unwrap();
        let b = b.with_cursor("visual only");
        assert_eq!(b.input.0, 1);
        assert_eq!(b.observation, "reference owner");
        assert_eq!(b.capture, 42);
    }
}

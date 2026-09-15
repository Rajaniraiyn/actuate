pub mod actions;
pub mod diff;
pub mod discovery;
pub mod geometry;
pub mod presentation;
pub mod query;
pub use actions::*;
// Independent provider capabilities. Native handles never cross this boundary.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Output projection only; stored native observations are never rewritten.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    Json,
    Compact,
    #[default]
    Text,
}
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ElementRef {
    pub session: String,
    pub id: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeError {
    pub code: String,
    pub message: String,
    pub effect: Effect,
}
impl std::fmt::Display for NativeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for NativeError {}
pub type Result<T> = std::result::Result<T, NativeError>;
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    None,
    Dispatched,
    Unknown,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    pub effect: Effect,
    pub route: String,
}
/// Attribute values retain native type tags, unsupported values and read errors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub reference: ElementRef,
    pub attributes: BTreeMap<String, Value>,
    pub actions: Vec<String>,
    pub parameterized_attributes: Vec<String>,
    pub children: Vec<ElementRef>,
    pub issues: Vec<Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub root: ElementRef,
    pub nodes: Vec<Node>,
    pub complete: bool,
    pub traversal_complete: bool,
    pub revision: u64,
    pub issues: Vec<Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObserveRequest {
    pub pid: i32,
    #[serde(default = "default_max_nodes")]
    pub max_nodes: usize,
    #[serde(default = "default_max_depth")]
    pub max_depth: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SemanticAction {
    Perform {
        name: String,
    },
    SetString {
        attribute: String,
        value: String,
    },
    SetBool {
        attribute: String,
        value: bool,
    },
    SetInteger {
        attribute: String,
        value: i64,
    },
    SetFloat {
        attribute: String,
        value: f64,
    },
    SetRange {
        attribute: String,
        location: i64,
        length: i64,
    },
    SetPoint {
        attribute: String,
        value: Point,
    },
    SetSize {
        attribute: String,
        width: f64,
        height: f64,
    },
}
/// Global input may affect foreground focus. Process input is delivery, not proof
/// that a control consumed an event. Neither route silently falls back.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Delivery {
    Global {},
    Process { pid: i32 },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PointerAction {
    Move {
        point: Point,
    },
    Click {
        point: Point,
        #[serde(default)]
        button: MouseButton,
        #[serde(default = "one_click")]
        count: u8,
        #[serde(default)]
        modifiers: Modifiers,
    },
    Scroll {
        vertical: i32,
        horizontal: i32,
        #[serde(default)]
        point: Option<Point>,
    },
    Drag {
        from: Point,
        to: Point,
        #[serde(default)]
        button: MouseButton,
        #[serde(default)]
        modifiers: Modifiers,
        #[serde(default = "default_drag_duration")]
        duration_ms: u64,
    },
}
pub trait Discover {
    fn discover(&mut self) -> Result<Value>;
}
pub trait Observe {
    fn observe(&mut self, request: ObserveRequest) -> Result<Snapshot>;
}
pub trait SemanticActions {
    fn semantic(&mut self, target: &ElementRef, action: SemanticAction) -> Result<Receipt>;
}
/// Read-only, route-specific evidence. Reports must preserve unknown states;
/// an observed pass never guarantees a later action will be consumed.
pub trait Actionability {
    type Report;
    fn actionability(&mut self, target: &ElementRef) -> Result<Self::Report>;
}
pub trait PointerInput {
    fn pointer(&mut self, delivery: Delivery, action: PointerAction) -> Result<Receipt>;
}
pub trait TextInput {
    fn type_text(&mut self, delivery: Delivery, text: &str) -> Result<Receipt>;
}
pub trait Interactive: PointerInput + TextInput {}
impl<T: PointerInput + TextInput> Interactive for T {}
/// Each field may use an independently selected implementation.
pub struct Providers<O, S, P, T> {
    pub observe: O,
    pub semantic: S,
    pub pointer: P,
    pub text: T,
}

/// Extra capabilities define their own request/response types until a portable
/// contract is established. Providers can implement only the families they support.
pub trait Capture {
    type Request;
    type Frame;
    fn capture(&mut self, request: Self::Request) -> Result<Self::Frame>;
}
pub trait KeyboardInput {
    type Key;
    fn key_press(&mut self, delivery: Delivery, key: Self::Key) -> Result<Receipt>;
}
pub trait WindowControl {
    type Window;
    type Action;
    fn window_action(&mut self, window: &Self::Window, action: Self::Action) -> Result<Receipt>;
}
pub trait Subscribe {
    type Scope;
    type Subscription;
    fn subscribe(&mut self, scope: Self::Scope) -> Result<Self::Subscription>;
}

pub const fn default_max_nodes() -> usize {
    1000
}
pub const fn default_max_depth() -> usize {
    30
}

/// Session transport commands. Each operation validates its full payload before dispatch.
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionRequest {
    View {
        #[serde(default)]
        revision: Option<u64>,
        #[serde(default)]
        options: presentation::PresentationOptions,
        #[serde(default)]
        format: OutputFormat,
    },
    DiffView {
        before: u64,
        #[serde(default)]
        after: Option<u64>,
        #[serde(default)]
        options: presentation::PresentationOptions,
        #[serde(default = "default_view_changes")]
        max_changes: usize,
    },
    Discover {
        #[serde(default)]
        scope: discovery::DiscoveryScope,
        #[serde(default = "default_raw_format")]
        format: OutputFormat,
    },
    Observe {
        request: ObserveRequest,
    },
    Semantic {
        target: ElementRef,
        action: SemanticAction,
    },
    Pointer {
        delivery: Delivery,
        action: PointerAction,
    },
    Text {
        delivery: Delivery,
        text: String,
    },
    Attribute {
        target: ElementRef,
        name: String,
    },
    ObserveSubtree {
        target: ElementRef,
        #[serde(default = "default_max_nodes")]
        max_nodes: usize,
        #[serde(default = "default_max_depth")]
        max_depth: usize,
    },
    ParameterizedAttribute {
        target: ElementRef,
        name: String,
        parameter: AttributeParameter,
    },
    WaitAttribute {
        target: ElementRef,
        name: String,
        expected: Value,
        #[serde(default = "default_wait_timeout")]
        timeout_ms: u64,
    },
    Diff {
        before: u64,
        #[serde(default)]
        after: Option<u64>,
    },
    Query {
        #[serde(default)]
        revision: Option<u64>,
        query: query::NodeQuery,
    },
    Snapshots {},
    Key {
        delivery: Delivery,
        chord: KeyChord,
    },
    HitTest {
        point: Point,
    },
    Window {
        target: ElementRef,
    },
    Click {
        target: ElementRef,
        mode: ClickMode,
        #[serde(default)]
        button: MouseButton,
        #[serde(default = "one_click")]
        count: u8,
        #[serde(default)]
        modifiers: Modifiers,
    },
    Inspect {
        target: ElementRef,
    },
}

pub const fn default_raw_format() -> OutputFormat {
    OutputFormat::Json
}

pub const fn default_view_changes() -> usize {
    100
}

pub const fn default_wait_timeout() -> u64 {
    5000
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejects_unrepresentable_action_fields_before_dispatch() {
        for payload in [
            json!({"op":"pointer", "delivery":{"kind":"global", "pid":123}, "action":{"kind":"click", "point":{"x":0,"y":0}}}),
            json!({"op":"pointer", "delivery":{"kind":"global"}, "action":{"kind":"click", "button":"unsupported_button", "point":{"x":0,"y":0}}}),
            json!({"op":"discover", "unexpected":true}),
            json!({"op":"text", "delivery":{"kind":"process","pid":123}, "text":"x", "paste":true}),
        ] {
            assert!(serde_json::from_value::<SessionRequest>(payload).is_err());
        }
    }

    #[test]
    fn observation_defaults_match_cli_contract() {
        let SessionRequest::Observe { request } =
            serde_json::from_value(json!({"op":"observe","request":{"pid":123}})).unwrap()
        else {
            panic!("wrong request")
        };
        assert_eq!(request.max_nodes, default_max_nodes());
        assert_eq!(request.max_depth, default_max_depth());
    }
}

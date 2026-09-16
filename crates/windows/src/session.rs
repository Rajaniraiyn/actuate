use super::{Accessibility, GlobalInput};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use unimation::*;

/// Provider metadata describes the injected implementation, not every Windows app.
#[derive(Debug, Serialize)]
pub struct AccessibilityCapabilities {
    pub provider: String,
    pub semantic_patterns: Vec<String>,
    pub live_validated: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InputRoute {
    Global,
    Process,
}
#[derive(Debug, Serialize)]
pub struct InputCapabilities {
    pub provider: String,
    pub pointer_routes: Vec<InputRoute>,
    pub keyboard_routes: Vec<InputRoute>,
    pub pixel_scroll: bool,
    pub wheel_detents: Option<WheelCapabilities>,
    pub live_validated: bool,
}
#[derive(Debug, Serialize)]
pub struct WheelCapabilities {
    pub route: InputRoute,
    pub max_detents_per_axis: u16,
    pub positive_vertical: VerticalDirection,
    pub positive_horizontal: HorizontalDirection,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerticalDirection {
    Down,
    Up,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HorizontalDirection {
    Right,
    Left,
}
pub trait WindowsAccessibility: Observe + SemanticActions {
    fn inspect(&mut self, target: &ElementRef) -> Result<Node>;
    fn capabilities(&self) -> AccessibilityCapabilities;
}
pub trait WindowsInput: PointerInput + TextInput + KeyboardInput<Key = KeyChord> {
    fn wheel(
        &mut self,
        _delivery: Delivery,
        _vertical: i32,
        _horizontal: i32,
        _point: Option<Point>,
    ) -> Result<Receipt> {
        Err(super::error(
            "unsupported_windows_wheel",
            "This input provider does not implement Windows wheel detents",
        ))
    }
    fn capabilities(&self) -> InputCapabilities;
}
fn uia_capabilities() -> AccessibilityCapabilities {
    AccessibilityCapabilities {
        provider: "windows_uia_raw_view".into(),
        semantic_patterns: [
            "invoke",
            "toggle",
            "select",
            "expand",
            "collapse",
            "scroll_into_view",
            "focus",
            "set_value",
        ]
        .map(str::to_owned)
        .to_vec(),
        live_validated: false,
    }
}
impl WindowsAccessibility for Accessibility {
    fn inspect(&mut self, target: &ElementRef) -> Result<Node> {
        Accessibility::inspect(self, target)
    }
    fn capabilities(&self) -> AccessibilityCapabilities {
        uia_capabilities()
    }
}
impl WindowsInput for GlobalInput {
    fn wheel(
        &mut self,
        delivery: Delivery,
        vertical: i32,
        horizontal: i32,
        point: Option<Point>,
    ) -> Result<Receipt> {
        GlobalInput::wheel(self, delivery, vertical, horizontal, point)
    }
    fn capabilities(&self) -> InputCapabilities {
        InputCapabilities {
            provider: "windows_send_input".into(),
            pointer_routes: vec![InputRoute::Global],
            keyboard_routes: vec![InputRoute::Global],
            pixel_scroll: false,
            wheel_detents: Some(WheelCapabilities {
                route: InputRoute::Global,
                max_detents_per_axis: 120,
                positive_vertical: VerticalDirection::Down,
                positive_horizontal: HorizontalDirection::Right,
            }),
            live_validated: false,
        }
    }
}
#[derive(Default)]
struct LazyAccessibility(Option<Accessibility>);
impl LazyAccessibility {
    fn get(&mut self) -> Result<&mut Accessibility> {
        if self.0.is_none() {
            self.0 = Some(Accessibility::new()?);
        }
        Ok(self.0.as_mut().unwrap())
    }
}
impl Observe for LazyAccessibility {
    fn observe(&mut self, request: ObserveRequest) -> Result<Snapshot> {
        self.get()?.observe(request)
    }
}
impl SemanticActions for LazyAccessibility {
    fn semantic(&mut self, target: &ElementRef, action: SemanticAction) -> Result<Receipt> {
        self.get()?.semantic(target, action)
    }
}
impl WindowsAccessibility for LazyAccessibility {
    fn inspect(&mut self, target: &ElementRef) -> Result<Node> {
        self.get()?.inspect(target)
    }
    fn capabilities(&self) -> AccessibilityCapabilities {
        uia_capabilities()
    }
}

/// Transport adapter owns references; native capabilities remain independently usable.
pub struct WindowsSession {
    accessibility: Box<dyn WindowsAccessibility>,
    input: Box<dyn WindowsInput>,
    snapshots: BTreeMap<u64, Snapshot>,
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Capabilities,
    Inspect {
        target: ElementRef,
    },
    Snapshots,
    Discover,
    Windows,
    Displays,
    Capture {
        path: String,
    },
    #[serde(alias = "observe")]
    Snapshot {
        request: ObserveRequest,
    },
    Semantic {
        target: ElementRef,
        action: SemanticAction,
    },
    WindowsWheel {
        delivery: Delivery,
        vertical: i32,
        horizontal: i32,
        #[serde(default)]
        point: Option<Point>,
    },
    Pointer {
        delivery: Delivery,
        action: PointerAction,
    },
    Text {
        delivery: Delivery,
        text: String,
    },
    Key {
        delivery: Delivery,
        chord: KeyChord,
    },
    Diff {
        before: u64,
        after: Option<u64>,
    },
}
impl WindowsSession {
    pub fn new() -> Result<Self> {
        Ok(Self {
            accessibility: Box::<LazyAccessibility>::default(),
            input: Box::new(GlobalInput),
            snapshots: BTreeMap::new(),
        })
    }
    /// Inject app-specific adapters without changing transport commands or shared traits.
    pub fn with_providers(
        accessibility: impl WindowsAccessibility + 'static,
        input: impl WindowsInput + 'static,
    ) -> Self {
        Self {
            accessibility: Box::new(accessibility),
            input: Box::new(input),
            snapshots: BTreeMap::new(),
        }
    }
    pub fn dispatch(&mut self, request: Value) -> Result<Value> {
        let request: Request = serde_json::from_value(request)
            .map_err(|e| super::error("invalid_request", e.to_string()))?;
        match request {
            Request::Inspect {target} => Ok(json!(self.accessibility.inspect(&target)?)),
            Request::Snapshots => Ok(json!(self.snapshots.values().map(|s|json!({"revision":s.revision,"nodes":s.nodes.len(),"complete":s.complete})).collect::<Vec<_>>())),
            Request::Capabilities => Ok(json!({"provider":"windows","coordinate_space":"physical_desktop_pixels","discovery":true,"accessibility":self.accessibility.capabilities(),"input":self.input.capabilities(),"capture":"gdi_desktop_png","verified_effects":false,"live_validated":false,"threading":"Default UIA provider must stay on its creating non-UI MTA thread"})),
            Request::Discover => super::WindowDiscovery.discover(),
            Request::Windows => Ok(json!(super::discover_windows()?)),
            Request::Capture { path } => super::capture_desktop(&path),
            Request::Displays => super::displays(),
            Request::Snapshot { request } => {
                let snapshot = self.accessibility.observe(request)?;
                let value = json!(snapshot);
                self.snapshots.insert(snapshot.revision, snapshot);
                while self.snapshots.len() > 16 {
                    self.snapshots.pop_first();
                }
                Ok(value)
            }
            Request::Semantic { target, action } => {
                Ok(json!(self.accessibility.semantic(&target, action)?))
            }
            Request::WindowsWheel {delivery,vertical,horizontal,point} => Ok(json!(self.input.wheel(delivery,vertical,horizontal,point)?)),
            Request::Pointer { delivery, action } => {
                Ok(json!(self.input.pointer(delivery, action)?))
            }
            Request::Text { delivery, text } => Ok(json!(self.input.type_text(delivery, &text)?)),
            Request::Key { delivery, chord } => Ok(json!(self.input.key_press(delivery, chord)?)),
            Request::Diff { before, after } => {
                let before = self.snapshots.get(&before).ok_or_else(|| {
                    super::error("unknown_revision", "Before revision not retained")
                })?;
                let after = after
                    .or_else(|| self.snapshots.keys().next_back().copied())
                    .and_then(|id| self.snapshots.get(&id))
                    .ok_or_else(|| {
                        super::error("unknown_revision", "After revision not retained")
                    })?;
                Ok(json!(unimation::diff::diff_snapshots(before, after)?))
            }
        }
    }
}

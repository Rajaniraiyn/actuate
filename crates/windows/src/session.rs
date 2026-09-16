use super::{Accessibility, GlobalInput};
use actuate::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Provider metadata describes the injected implementation, not every Windows app.
#[derive(Debug, Serialize)]
pub struct AccessibilityCapabilities {
    pub provider: String,
    pub semantic_patterns: Vec<String>,
    pub live_validated: bool,
    /// UIA providers can activate their own windows even without SetFocus.
    pub may_activate_target: bool,
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
    fn desktop_roots(&mut self) -> Result<Value> {
        Err(super::error(
            "unsupported_root_discovery",
            "This accessibility provider does not expose desktop roots",
        ))
    }
    fn observe_window(&mut self, _request: ObserveRequest, _window_id: u64) -> Result<Snapshot> {
        Err(super::error(
            "unsupported_window_scope",
            "This accessibility provider does not support window-scoped observation",
        ))
    }
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
        live_validated: true,
        may_activate_target: true,
    }
}
impl WindowsAccessibility for Accessibility {
    fn desktop_roots(&mut self) -> Result<Value> {
        Accessibility::desktop_roots(self)
    }
    fn observe_window(&mut self, request: ObserveRequest, window_id: u64) -> Result<Snapshot> {
        Accessibility::observe_window(self, request, window_id)
    }
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
    fn desktop_roots(&mut self) -> Result<Value> {
        self.get()?.desktop_roots()
    }
    fn observe_window(&mut self, request: ObserveRequest, window_id: u64) -> Result<Snapshot> {
        self.get()?.observe_window(request, window_id)
    }
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
    cursor: Option<overlay::OverlayController>,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum CursorOverlayAction {
    Start {
        executable: String,
        #[serde(default)]
        physical_cursor: overlay::PhysicalCursorPolicy,
        #[serde(default)]
        tracking: overlay::CursorTracking,
    },
    Stop {},
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Capabilities,
    UiaRoots,
    TaskbarState,
    TaskbarAutoHide {
        enabled: bool,
    },
    CursorOverlay {
        action: CursorOverlayAction,
    },
    Cursor {
        command: overlay::CursorCommand,
    },
    CursorState,
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
        #[serde(default)]
        window_id: Option<u64>,
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
            cursor: None,
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
            cursor: None,
        }
    }
    fn cursor_state(&mut self) -> Result<overlay::OverlayState> {
        self.cursor
            .as_mut()
            .map(overlay::OverlayController::state)
            .transpose()
            .map(Option::unwrap_or_default)
    }

    fn capabilities(&self) -> Value {
        json!({
            "provider": "windows",
            "coordinate_space": "physical_desktop_pixels",
            "discovery": true,
            "accessibility": self.accessibility.capabilities(),
            "input": self.input.capabilities(),
            "capture": "gdi_desktop_png",
            "cursor": {
                "lifecycle": "explicit_native_helper",
                "scopes": ["desktop", "window"],
                "physical_cursor_policies": ["preserve", "hide_within_scope", "hide_while_visible"],
                "tracking": ["commands", "physical_pointer"],
                "acknowledgement": "queued",
                "presentation_acknowledged": false
            },
            "taskbar": {"scope": "shared_shell", "state": true, "set_auto_hide": true},
            "verified_effects": false,
            "live_validated": false,
            "threading": "Default UIA provider must stay on its creating non-UI MTA thread"
        })
    }

    fn cursor_overlay(&mut self, action: CursorOverlayAction) -> Result<overlay::OverlayState> {
        match action {
            CursorOverlayAction::Start {
                executable,
                physical_cursor,
                tracking,
            } => {
                if self.cursor.is_some() {
                    return Err(super::error(
                        "overlay_running",
                        "Stop the current overlay before starting another",
                    ));
                }
                self.cursor = Some(overlay::OverlayController::start_with_options(
                    executable,
                    physical_cursor,
                    tracking,
                )?);
            }
            CursorOverlayAction::Stop {} => {
                if let Some(mut cursor) = self.cursor.take() {
                    cursor.stop()?;
                }
            }
        }
        self.cursor_state()
    }

    pub fn dispatch(&mut self, mut request: Value) -> Result<Value> {
        transport::prepare(&mut request, || {
            self.snapshots
                .last_key_value()
                .map(|(_, snapshot)| snapshot.root.session.clone())
                .ok_or_else(|| super::error("no_snapshot", "Observe before using short references"))
        })?;
        let request: Request = serde_json::from_value(request)
            .map_err(|e| super::error("invalid_request", e.to_string()))?;
        match request {
            Request::UiaRoots => self.accessibility.desktop_roots(),
            Request::TaskbarState => Ok(json!(super::shell::taskbar_state()?)),
            Request::TaskbarAutoHide { enabled } => {
                Ok(json!(super::shell::set_taskbar_auto_hide(enabled)?))
            }
            Request::CursorOverlay { action } => Ok(json!(self.cursor_overlay(action)?)),
            Request::Cursor { command } => {
                command.validate()?;
                let cursor = self.cursor.as_mut().ok_or_else(|| {
                    super::error(
                        "overlay_not_started",
                        "Start the overlay before sending cursor commands",
                    )
                })?;
                let acknowledgement = cursor.visualize(command)?;
                Ok(json!({
                    "acknowledgement": acknowledgement,
                    "physical_cursor_policy": cursor.physical_cursor_policy()
                }))
            }
            Request::CursorState => Ok(json!(self.cursor_state()?)),
            Request::Inspect { target } => Ok(json!(self.accessibility.inspect(&target)?)),
            Request::Snapshots => {
                let summaries: Vec<_> = self
                    .snapshots
                    .values()
                    .map(|snapshot| {
                        json!({
                            "revision": snapshot.revision,
                            "nodes": snapshot.nodes.len(),
                            "complete": snapshot.complete
                        })
                    })
                    .collect();
                Ok(json!(summaries))
            }
            Request::Capabilities => Ok(self.capabilities()),
            Request::Discover => super::WindowDiscovery.discover(),
            Request::Windows => Ok(json!(super::discover_windows()?)),
            Request::Capture { path } => super::capture_desktop(&path),
            Request::Displays => super::displays(),
            Request::Snapshot { request, window_id } => {
                let snapshot = match window_id {
                    Some(id) => self.accessibility.observe_window(request, id)?,
                    None => self.accessibility.observe(request)?,
                };
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
            Request::WindowsWheel {
                delivery,
                vertical,
                horizontal,
                point,
            } => Ok(json!(
                self.input.wheel(delivery, vertical, horizontal, point)?
            )),
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
                Ok(json!(actuate::diff::diff_snapshots(before, after)?))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_requires_explicit_start_and_validates_before_delivery() {
        let mut session = WindowsSession::new().unwrap();
        let state = session.dispatch(json!({"op":"cursor_state"})).unwrap();
        assert_eq!(state["running"], false);
        assert_eq!(state["physical_cursor_policy"], "preserve");
        assert_eq!(
            session
                .dispatch(json!({"op":"cursor","command":{"op":"show"}}))
                .unwrap_err()
                .code,
            "overlay_not_started"
        );
        assert_eq!(
            session
                .dispatch(
                    json!({"op":"cursor","command":{"op":"move","x":1,"y":2,"duration_ms":10001}})
                )
                .unwrap_err()
                .code,
            "invalid_cursor_command"
        );
        let stopped = session
            .dispatch(json!({"op":"cursor_overlay","action":{"kind":"stop"}}))
            .unwrap();
        assert_eq!(stopped["running"], false);
    }

    #[test]
    fn correlation_id_is_removed_before_strict_request_parsing() {
        let mut session = WindowsSession::new().unwrap();
        let error = session
            .dispatch(json!({"id":42,"op":"text","delivery":{"kind":"process","pid":1},"text":"x"}))
            .unwrap_err();
        assert_ne!(error.code, "invalid_request");
        assert!(matches!(error.effect, Effect::None));
        assert_eq!(
            session
                .dispatch(json!({"id":null,"op":"snapshots"}))
                .unwrap(),
            json!([])
        );
        assert_eq!(
            session
                .dispatch(json!({"op":"text","extra":true,"delivery":{"kind":"global"},"text":""}))
                .unwrap_err()
                .code,
            "invalid_request"
        );
    }

    #[test]
    fn short_references_require_an_observation() {
        let mut session = WindowsSession::new().unwrap();
        assert_eq!(
            session
                .dispatch(json!({"op":"inspect","target":"@e1"}))
                .unwrap_err()
                .code,
            "no_snapshot"
        );
    }
}

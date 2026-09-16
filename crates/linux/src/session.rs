//! Linux session: AT-SPI observation plus explicitly selected input,
//! capture, compositor and cursor providers. Mirrors the macOS session
//! surface where the underlying capability exists and reports the rest
//! as unsupported instead of substituting a route.
use crate::{
    AtSpi,
    hyprland_windows::{HyprlandOrigins, window_records},
    wayland_capture::{self, CaptureSource as WaylandSource, WaylandCapture},
    wayland_input::WaylandInput,
    x11::{X11, XSpace},
};
use actuate::{
    session::{ActionEpoch, FrameHistory, SnapshotHistory, render, wait_attribute},
    *,
};
use compositor::{
    Hyprland,
    hyprland::{self, Client, Monitor},
};
use overlay::{CursorCommand, CursorScope, OverlayController, linux::LayerCursor};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;

/// Which server receives global pointer, text and key input.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InputRoute {
    /// Compositor virtual devices; moves the shared cursor and follows focus.
    Wayland,
    /// XTest on the X server; local to Xwayland clients under a compositor.
    X11,
}
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CaptureSource {
    /// The focused Hyprland monitor, else the first output. The default, so
    /// `{"op":"capture","path":...}` means the same thing on every desktop host.
    FocusedOutput {},
    Output {
        name: String,
    },
    /// A Hyprland window by address, captured unoccluded through the
    /// compositor's toplevel capture protocol.
    Window {
        address: String,
    },
    Region {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
    /// An X11 drawable through the X server.
    X11Window {
        window_id: u32,
    },
}
impl Default for CaptureSource {
    fn default() -> Self {
        Self::FocusedOutput {}
    }
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AccessibilityAction {
    Status {},
    /// Desktop-wide `org.a11y.Status.IsEnabled`; toolkits observe it.
    SetEnabled {
        enabled: bool,
    },
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CursorOverlayAction {
    /// Without an executable the layer-shell renderer runs in-process.
    Start {
        #[serde(default)]
        executable: Option<PathBuf>,
    },
    Stop {},
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum LinuxRequest {
    Snapshot {
        request: ObserveRequest,
        #[serde(default)]
        scope: SnapshotScope,
        #[serde(default)]
        options: presentation::PresentationOptions,
        #[serde(default)]
        format: OutputFormat,
    },
    Capture {
        #[serde(default)]
        source: CaptureSource,
        path: PathBuf,
    },
    Displays {},
    Windows {},
    Capabilities {},
    Accessibility {
        action: AccessibilityAction,
    },
    Actionability {
        target: ElementRef,
    },
    CursorOverlay {
        action: CursorOverlayAction,
    },
    CursorState {},
    /// Selects the server for global delivery; nothing switches implicitly.
    InputRoute {
        route: InputRoute,
    },
    ClickImage {
        frame: u64,
        point: Point,
        mode: ClickMode,
        #[serde(default)]
        button: MouseButton,
        #[serde(default = "one_click")]
        count: u8,
    },
    ClickWindow {
        target: ElementRef,
        point: Point,
        mode: ClickMode,
        #[serde(default)]
        button: MouseButton,
        #[serde(default = "one_click")]
        count: u8,
    },
    ScrollTarget {
        target: ElementRef,
        mode: ClickMode,
        vertical: i32,
        horizontal: i32,
    },
    /// Window-targeted keyboard shortcut through Hyprland, in its own key
    /// and modifier vocabulary. The window need not be focused.
    HyprlandShortcut {
        #[serde(default)]
        target: Option<ElementRef>,
        #[serde(default)]
        address: Option<String>,
        #[serde(default)]
        mods: String,
        key: String,
        #[serde(default)]
        state: Option<KeyState>,
    },
}
/// Extension operation names, kept next to the enum they select.
const EXTENSION_OPS: [&str; 14] = [
    "snapshot",
    "capture",
    "displays",
    "windows",
    "capabilities",
    "accessibility",
    "actionability",
    "cursor_overlay",
    "cursor_state",
    "input_route",
    "click_image",
    "click_window",
    "scroll_target",
    "hyprland_shortcut",
];
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyState {
    Down,
    Up,
}

/// Global input providers share one contract; the route picks which is used.
pub trait GlobalInput: PointerInput + TextInput + KeyboardInput<Key = KeyChord> {}
impl<T: PointerInput + TextInput + KeyboardInput<Key = KeyChord>> GlobalInput for T {}

enum Cursor {
    Process(OverlayController),
    Layer(LayerCursor),
}
impl Cursor {
    fn send(&mut self, command: CursorCommand) -> Result<()> {
        match self {
            Self::Process(controller) => {
                CursorVisualization::visualize(controller, command).map(|_| ())
            }
            Self::Layer(cursor) => cursor.visualize(command).map(|_| ()),
        }
    }
    fn running(&mut self) -> bool {
        match self {
            Self::Process(controller) => controller.is_running().unwrap_or(false),
            Self::Layer(cursor) => cursor.is_running(),
        }
    }
    /// The helper process, when rendering happens outside this process.
    fn pid(&self) -> Option<u32> {
        match self {
            Self::Process(controller) => controller.pid(),
            Self::Layer(_) => None,
        }
    }
    fn stop(&mut self) -> Result<()> {
        match self {
            Self::Process(controller) => controller.stop(),
            Self::Layer(cursor) => cursor.stop(),
        }
    }
}

/// Where a retained frame's pixels came from, which decides how its
/// geometry is revalidated and mapped before an image click.
enum Provenance {
    Output,
    /// Pixels are window-local; the window's current position maps them.
    Window(Box<Client>),
    X11Window(u32),
}
struct RetainedFrame {
    frame: wayland_capture::Frame,
    provenance: Provenance,
}

pub struct LinuxSession {
    atspi: AtSpi,
    hypr: Option<Hyprland>,
    wayland: Option<WaylandInput>,
    x11: Option<X11>,
    capture: Option<WaylandCapture>,
    route: InputRoute,
    cursor: Option<Cursor>,
    cursor_error: Option<NativeError>,
    last_pointer: Option<Point>,
    snapshots: SnapshotHistory,
    frames: FrameHistory<RetainedFrame>,
    epoch: ActionEpoch,
}
fn fail(code: &str, message: impl ToString) -> NativeError {
    NativeError::new(code, message)
}

impl LinuxSession {
    pub fn connect() -> Result<Self> {
        let hypr = Hyprland::from_env();
        let atspi = match &hypr {
            Some(h) => AtSpi::connect_with(Box::new(HyprlandOrigins::new(h.clone())))?,
            None => AtSpi::connect()?,
        };
        let route = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            InputRoute::Wayland
        } else {
            InputRoute::X11
        };
        // A person watching the agent can ask for the soft cursor from the start.
        let (cursor, cursor_error) = if std::env::var_os("ACTUATE_SOFT_CURSOR").is_some() {
            match LayerCursor::start() {
                Ok(cursor) => (Some(Cursor::Layer(cursor)), None),
                Err(e) => (None, Some(e)),
            }
        } else {
            (None, None)
        };
        Ok(Self {
            atspi,
            hypr,
            wayland: None,
            x11: None,
            capture: None,
            route,
            cursor,
            cursor_error,
            last_pointer: None,
            snapshots: SnapshotHistory::default(),
            frames: FrameHistory::default(),
            epoch: ActionEpoch::default(),
        })
    }
    pub fn hyprland(&self) -> Option<&Hyprland> {
        self.hypr.as_ref()
    }
    fn hypr(&self) -> Result<&Hyprland> {
        self.hypr.as_ref().ok_or_else(|| {
            fail(
                "unsupported",
                "This operation needs Hyprland IPC; HYPRLAND_INSTANCE_SIGNATURE is not set",
            )
        })
    }
    fn monitors(&self) -> Vec<Monitor> {
        self.hypr
            .as_ref()
            .and_then(|h| h.monitors().ok())
            .unwrap_or_default()
    }
    fn wayland(&mut self) -> Result<&mut WaylandInput> {
        if self.wayland.is_none() {
            self.wayland = Some(WaylandInput::connect()?);
        }
        Ok(self.wayland.as_mut().expect("connected"))
    }
    /// The X provider converts logical layout points itself, using the
    /// per-monitor scale Hyprland applies to Xwayland.
    fn x11(&mut self) -> Result<&mut X11> {
        if self.x11.is_none() {
            let space = XSpace::from_monitors(
                self.monitors()
                    .iter()
                    .map(|m| (m.rect(), m.scale))
                    .collect(),
            );
            self.x11 = Some(X11::connect(space)?);
        }
        Ok(self.x11.as_mut().expect("connected"))
    }
    fn capture(&mut self) -> Result<&mut WaylandCapture> {
        if self.capture.is_none() {
            self.capture = Some(WaylandCapture::connect()?);
        }
        Ok(self.capture.as_mut().expect("connected"))
    }
    fn input(&mut self) -> Result<&mut dyn GlobalInput> {
        Ok(match self.route {
            InputRoute::Wayland => self.wayland()?,
            InputRoute::X11 => self.x11()?,
        })
    }
    /// The Hyprland client owning a reference, through its pid and frame title.
    fn client_for(&mut self, target: &ElementRef) -> Result<Client> {
        let (pid, title) = self.atspi.owning_window(target)?;
        self.hypr()?
            .client_for(i64::from(pid), &title)?
            .ok_or_else(|| {
                fail(
                    "no_window",
                    format!("No unique Hyprland window for pid {pid} titled {title:?}"),
                )
            })
    }
    /// Global input reaches whatever is shown. Refuse when the target's
    /// window is not mapped, or, for the compositor seat route, when it is
    /// not on its monitor's active workspace. XTest inside Xwayland delivers
    /// by X window geometry and does not depend on the shown workspace.
    fn require_visible(&self, client: &Client) -> Result<()> {
        if !client.mapped || client.hidden {
            return Err(fail("not_visible", "Target window is unmapped or hidden"));
        }
        if self.route == InputRoute::X11 && client.xwayland {
            return Ok(());
        }
        if !hyprland::is_on_active_workspace(client, &self.monitors()) {
            return Err(fail(
                "not_on_active_workspace",
                format!(
                    "Window {} is on workspace {} which is not active on its monitor; global input would reach another window",
                    client.address, client.workspace.id
                ),
            ));
        }
        Ok(())
    }
    fn visual(&mut self, command: CursorCommand) {
        if let Some(cursor) = self.cursor.as_mut()
            && let Err(e) = cursor.send(command)
        {
            self.cursor_error = Some(e);
        }
    }
    /// Points the soft cursor at what a targeted action is about to touch,
    /// scoped to the target's window, and pulses it once the route reports
    /// dispatch. Semantic and window-targeted routes never move the real
    /// pointer, so this is the only visible trace of those actions.
    fn visual_action(
        &mut self,
        target: &ElementRef,
        act: impl FnOnce(&mut Self) -> Result<Receipt>,
    ) -> Result<Receipt> {
        let point = if self.cursor.is_some() {
            self.atspi.element_center(target).ok()
        } else {
            None
        };
        if let Some(point) = &point {
            let scope = match self.client_for(target) {
                Ok(c) if c.pid > 0 && c.handle().is_some() => CursorScope::Window {
                    window_id: u64::from(c.handle().expect("checked")),
                    pid: c.pid as i32,
                },
                _ => CursorScope::Desktop,
            };
            self.visual(CursorCommand::Scope { scope });
            self.visual(CursorCommand::Move {
                x: point.x,
                y: point.y,
                duration_ms: 180,
            });
            self.visual(CursorCommand::Show);
        }
        let receipt = act(self);
        if let Some(point) = point
            && receipt
                .as_ref()
                .is_ok_and(|r| matches!(r.effect, Effect::Dispatched))
        {
            self.visual(CursorCommand::Click {
                x: point.x,
                y: point.y,
            });
        }
        receipt
    }
    /// Global pointer dispatch on the selected route, with the visual cursor
    /// following each dispatched action. Visual failures never alter receipts.
    fn global_pointer(
        &mut self,
        action: PointerAction,
        window: Option<&Client>,
    ) -> Result<Receipt> {
        action.validate()?;
        let (target, is_click) = match &action {
            PointerAction::Move { point } => (point.clone(), false),
            PointerAction::Click { point, .. } => (point.clone(), true),
            PointerAction::Scroll { point, .. } => (
                point
                    .clone()
                    .or_else(|| self.last_pointer.clone())
                    .unwrap_or(Point { x: 0., y: 0. }),
                false,
            ),
            PointerAction::Drag { to, .. } => (to.clone(), false),
        };
        let scope = match window.and_then(|c| c.handle().map(|h| (h, c.pid))) {
            Some((handle, pid)) if pid > 0 => CursorScope::Window {
                window_id: u64::from(handle),
                pid: pid as i32,
            },
            _ => CursorScope::Desktop,
        };
        self.visual(CursorCommand::Scope { scope });
        if matches!(
            action,
            PointerAction::Move { .. } | PointerAction::Click { .. }
        ) {
            self.visual(CursorCommand::Move {
                x: target.x,
                y: target.y,
                duration_ms: 180,
            });
            self.visual(CursorCommand::Show);
        }
        let receipt = self.input()?.pointer(Delivery::Global {}, action);
        if receipt
            .as_ref()
            .is_ok_and(|r| matches!(r.effect, Effect::Dispatched))
        {
            self.last_pointer = Some(target.clone());
            if is_click {
                self.visual(CursorCommand::Click {
                    x: target.x,
                    y: target.y,
                });
            }
        }
        receipt
    }
    fn click_at(
        &mut self,
        target: Option<&ElementRef>,
        point: Point,
        mode: ClickMode,
        button: MouseButton,
        count: u8,
        modifiers: Modifiers,
    ) -> Result<Receipt> {
        match mode {
            ClickMode::Semantic => {
                let target = ClickMode::require_semantic_target(target, button, count, modifiers)?;
                self.visual_action(target, |s| s.atspi.activate(target))
            }
            ClickMode::Global => {
                let client = match target {
                    Some(target) => {
                        let client = self.client_for(target)?;
                        self.require_visible(&client)?;
                        Some(client)
                    }
                    None => None,
                };
                self.global_pointer(
                    PointerAction::Click {
                        point,
                        button,
                        count,
                        modifiers,
                    },
                    client.as_ref(),
                )
            }
            ClickMode::Process => {
                let target = target.ok_or_else(|| {
                    fail(
                        "invalid_request",
                        "Process mode requires a target reference",
                    )
                })?;
                let client = self.client_for(target)?;
                if !client.xwayland {
                    return Err(fail(
                        "unsupported",
                        "Process-directed pointer delivery exists only for X11 windows (synthetic send_event); Wayland windows accept semantic actions or global input",
                    ));
                }
                self.visual_action(target, |s| {
                    let x11 = s.x11()?;
                    let window = x11.window_for(client.pid as u32, Some(&client.title))?;
                    x11.click_window(window.window_id, &point, button, count)
                })
            }
            ClickMode::Skylight => Err(fail("unsupported", "SkyLight is a macOS route")),
        }
    }
    pub fn dispatch(&mut self, mut value: Value) -> Result<Value> {
        transport::prepare(&mut value, || Ok(self.atspi.session_id().to_owned()))?;
        let extension = value["op"]
            .as_str()
            .is_some_and(|op| EXTENSION_OPS.contains(&op));
        let result = if extension {
            let request: LinuxRequest =
                serde_json::from_value(value).map_err(|e| fail("invalid_request", e))?;
            self.execute_extension(request)
        } else {
            let request: SessionRequest =
                serde_json::from_value(value).map_err(|e| fail("invalid_request", e))?;
            self.execute_core(request)
        };
        self.epoch.record(&result);
        result
    }
    pub fn extension(&mut self, request: LinuxRequest) -> Result<Value> {
        let result = self.execute_extension(request);
        self.epoch.record(&result);
        result
    }
    fn discover_value(&mut self) -> Result<Value> {
        let mut raw = self.atspi.discover()?;
        let hypr = self.hypr.clone();
        let clients = hypr
            .as_ref()
            .and_then(|h| h.clients().ok())
            .unwrap_or_default();
        let active = hypr.as_ref().and_then(|h| h.active_window().ok().flatten());
        if let Some(apps) = raw["applications"].as_array_mut() {
            for app in apps {
                let pid = app["pid"].as_i64().unwrap_or(-1);
                let mine: Vec<&Client> = clients.iter().filter(|c| c.pid == pid).collect();
                app["visible_window_ids"] = json!(
                    mine.iter()
                        .filter(|c| c.mapped)
                        .filter_map(|c| c.handle())
                        .collect::<Vec<_>>()
                );
                app["windows"] = json!(mine);
                app["active"] = json!(active.as_ref().is_some_and(|a| a.pid == pid));
            }
        }
        raw["active_pid"] = json!(active.as_ref().map(|a| a.pid));
        // The bus answered, so observation is permitted; toolkits may still
        // withhold trees until the bus-wide enabled flag is set.
        raw["accessibility_trusted"] = json!(true);
        raw["compositor"] = json!({
            "hyprland": hypr.as_ref().and_then(|h| h.version().ok()),
            "session_type": std::env::var("XDG_SESSION_TYPE").ok(),
            "wayland_display": std::env::var("WAYLAND_DISPLAY").ok(),
            "x11_display": std::env::var("DISPLAY").ok(),
        });
        let known: Vec<i64> = raw["applications"]
            .as_array()
            .map(|apps| apps.iter().filter_map(|a| a["pid"].as_i64()).collect())
            .unwrap_or_default();
        raw["unregistered_windows"] = json!(
            clients
                .iter()
                .filter(|c| c.mapped && !known.contains(&c.pid))
                .collect::<Vec<_>>()
        );
        Ok(raw)
    }
    /// The topmost visible Hyprland window at a layout point.
    fn client_at(&self, point: &Point) -> Result<Client> {
        let hypr = self.hypr()?;
        let monitors = hypr.monitors()?;
        let mut candidates: Vec<Client> = hypr
            .clients()?
            .into_iter()
            .filter(|c| c.mapped && !c.hidden && c.contains(point))
            .filter(|c| hyprland::is_on_active_workspace(c, &monitors))
            .collect();
        candidates.sort_by_key(|c| c.focus_history_id);
        candidates
            .into_iter()
            .next()
            .ok_or_else(|| fail("no_window", "No visible window at that point"))
    }
    fn execute_core(&mut self, request: SessionRequest) -> Result<Value> {
        match request {
            SessionRequest::View {
                revision,
                options,
                format,
            } => Ok(json!(render(
                self.snapshots.get(revision)?.clone(),
                format,
                &options
            )?)),
            SessionRequest::DiffView {
                before,
                after,
                options,
                max_changes,
            } => Ok(json!(presentation::render_view_diff(
                self.snapshots.get(Some(before))?,
                self.snapshots.get(after)?,
                &options,
                max_changes
            )?)),
            SessionRequest::Diff { before, after } => diff::diff_snapshots(
                self.snapshots.get(Some(before))?,
                self.snapshots.get(after)?,
            )
            .map(|d| json!(d)),
            SessionRequest::Query { revision, query } => Ok(json!(query::query_nodes(
                self.snapshots.get(revision)?,
                &query
            ))),
            SessionRequest::Snapshots {} => Ok(json!(self.snapshots.summaries())),
            SessionRequest::Discover { scope, format } => {
                let raw = self.discover_value()?;
                Ok(discovery::present_discovery(&raw, scope, format))
            }
            SessionRequest::Observe { request } => {
                let snapshot = self.atspi.observe_scope(request.pid, request.budget())?;
                Ok(json!(self.snapshots.remember(snapshot).as_ref()))
            }
            SessionRequest::ObserveSubtree {
                target,
                max_nodes,
                max_depth,
            } => {
                let snapshot = self.atspi.observe_subtree(
                    &target,
                    ObservationBudget {
                        max_nodes,
                        max_depth,
                    },
                )?;
                Ok(json!(self.snapshots.remember(snapshot).as_ref()))
            }
            SessionRequest::Inspect { target } => self.atspi.inspect(&target),
            SessionRequest::Attribute { target, name } => self.atspi.read_attribute(&target, &name),
            SessionRequest::ParameterizedAttribute { .. } => Err(fail(
                "unsupported",
                "AT-SPI has no parameterized attributes; use attribute with Interface.Property names",
            )),
            SessionRequest::WaitAttribute {
                target,
                name,
                expected,
                timeout_ms,
            } => wait_attribute(timeout_ms, &expected, || {
                self.atspi.read_attribute(&target, &name)
            }),
            SessionRequest::Semantic { target, action } => self
                .visual_action(&target, |s| s.atspi.semantic(&target, action))
                .map(|r| json!(r)),
            SessionRequest::Pointer { delivery, action } => match delivery {
                Delivery::Global {} => self.global_pointer(action, None).map(|r| json!(r)),
                Delivery::Process { pid } => match action {
                    PointerAction::Click {
                        point,
                        button,
                        count,
                        ..
                    } => {
                        let x11 = self.x11()?;
                        let window = x11.window_for(pid as u32, None)?;
                        x11.click_window(window.window_id, &point, button, count)
                            .map(|r| json!(r))
                    }
                    _ => Err(fail(
                        "unsupported",
                        "Process-directed delivery supports click only, through X11 synthetic events",
                    )),
                },
            },
            SessionRequest::Text { delivery, text } => {
                self.input()?.type_text(delivery, &text).map(|r| json!(r))
            }
            SessionRequest::Key { delivery, chord } => {
                self.input()?.key_press(delivery, chord).map(|r| json!(r))
            }
            SessionRequest::HitTest { point } => {
                if !point.x.is_finite() || !point.y.is_finite() {
                    return Err(fail("invalid_geometry", "Finite layout point required"));
                }
                let client = self.client_at(&point)?;
                let app = self.atspi.application_root(client.pid as i32)?;
                let frame = self.atspi.frame_for_title(&app, &client.title)?;
                let local = (
                    (point.x - client.at[0] as f64) as i32,
                    (point.y - client.at[1] as f64) as i32,
                );
                self.atspi
                    .hit_test_window(&frame, local.0, local.1)
                    .map(|r| json!(r))
            }
            SessionRequest::Window { target } => {
                let client = self.client_for(&target)?;
                let (pid, title) = self.atspi.owning_window(&target)?;
                let app = self.atspi.application_root(pid)?;
                let frame = self.atspi.frame_for_title(&app, &title)?;
                Ok(
                    json!({"reference": frame, "pid": client.pid, "window_id": client.handle(), "address": client.address,
                    "bounds": client.rect(), "workspace": client.workspace, "monitor": client.monitor,
                    "xwayland": client.xwayland, "class": client.class, "title": client.title}),
                )
            }
            SessionRequest::Click {
                target,
                mode,
                button,
                count,
                modifiers,
            } => {
                let point = if matches!(mode, ClickMode::Semantic) {
                    Point { x: 0., y: 0. }
                } else {
                    self.atspi.element_center(&target)?
                };
                self.click_at(Some(&target), point, mode, button, count, modifiers)
                    .map(|r| json!(r))
            }
        }
    }
    fn focused_output(&mut self) -> Result<String> {
        if let Some(name) = self
            .monitors()
            .into_iter()
            .find(|m| m.focused)
            .map(|m| m.name)
        {
            return Ok(name);
        }
        self.capture()?
            .outputs()
            .first()
            .map(|o| o.name.clone())
            .ok_or_else(|| fail("unknown_output", "No outputs are available"))
    }
    fn execute_extension(&mut self, request: LinuxRequest) -> Result<Value> {
        match request {
            LinuxRequest::Snapshot {
                request,
                scope,
                options,
                format,
            } => {
                let root = self.atspi.application_root(request.pid)?;
                let root = match scope {
                    SnapshotScope::Application => root,
                    SnapshotScope::FocusedWindow => {
                        let active = self
                            .hypr()?
                            .active_window()?
                            .ok_or_else(|| fail("no_window", "No active window"))?;
                        if active.pid != i64::from(request.pid) {
                            return Err(fail(
                                "no_window",
                                "The active window belongs to another pid; use application scope",
                            ));
                        }
                        self.atspi.frame_for_title(&root, &active.title)?
                    }
                };
                let snapshot = self.atspi.observe_subtree(&root, request.budget())?;
                let snapshot = self.snapshots.remember(snapshot);
                Ok(json!(render(snapshot, format, &options)?))
            }
            LinuxRequest::Displays {} => {
                let outputs: Vec<Value> =
                    self.capture()?.outputs().iter().map(|o| json!(o)).collect();
                let monitors = self.hypr.as_ref().and_then(|h| h.monitors().ok());
                Ok(json!({"outputs": outputs, "hyprland_monitors": monitors}))
            }
            LinuxRequest::Windows {} => {
                let hypr = self.hypr.as_ref().map(window_records).transpose()?;
                let x11 = if std::env::var_os("DISPLAY").is_some() {
                    Some(self.x11()?.windows()?)
                } else {
                    None
                };
                Ok(json!({"hyprland": hypr, "x11": x11}))
            }
            LinuxRequest::Capabilities {} => Ok(json!({
                "observation": "atspi",
                "accessibility": self.atspi.status().ok(),
                "hyprland": self.hypr.is_some(),
                "input_route": self.route,
                "wayland_input": self.wayland.as_ref().map(WaylandInput::capabilities),
                "x11": self.x11.as_ref().map(X11::capabilities),
                "capture": self.capture.as_ref().map(WaylandCapture::capabilities),
                "snapshots_retained": self.snapshots.capacity(),
                "frames_retained": self.frames.capacity(),
                "process_delivery": {"pointer": "x11_send_event_only", "keys": "hyprland_shortcut", "text": false},
                "consumption_verified": false,
            })),
            LinuxRequest::Accessibility { action } => match action {
                AccessibilityAction::Status {} => self.atspi.status(),
                AccessibilityAction::SetEnabled { enabled } => {
                    self.atspi.set_enabled(enabled).map(|r| json!(r))
                }
            },
            LinuxRequest::Actionability { target } => self.actionability(&target),
            LinuxRequest::InputRoute { route } => {
                self.route = route;
                Ok(json!({"input_route": route}))
            }
            LinuxRequest::CursorOverlay { action } => {
                match action {
                    CursorOverlayAction::Start { executable } => {
                        if self.cursor.is_some() {
                            return Err(fail(
                                "overlay_running",
                                "Stop the current overlay before replacing it",
                            ));
                        }
                        self.cursor = Some(match executable {
                            Some(path) => Cursor::Process(OverlayController::start(path)?),
                            None => Cursor::Layer(LayerCursor::start()?),
                        });
                        self.cursor_error = None;
                    }
                    CursorOverlayAction::Stop {} => {
                        if let Some(mut cursor) = self.cursor.take() {
                            cursor.stop()?;
                        }
                        self.cursor_error = None;
                    }
                }
                Ok(json!({"overlay_running": self.cursor.is_some(), "render_acknowledged": false}))
            }
            LinuxRequest::CursorState {} => {
                let running = self.cursor.as_mut().map(Cursor::running).unwrap_or(false);
                Ok(json!({
                    "last_pointer": self.last_pointer,
                    "meaning": "last_dispatched_global_pointer",
                    "shared_cursor": self.hypr.as_ref().and_then(|h| h.cursor_position().ok()),
                    "consumption_verified": false,
                    "overlay_running": running,
                    "overlay_configured": self.cursor.is_some(),
                    "overlay_in_process": matches!(self.cursor, Some(Cursor::Layer(_))),
                    "overlay_pid": self.cursor.as_ref().and_then(Cursor::pid),
                    "overlay_error": self.cursor_error,
                    "overlay_acknowledgement": "queued_only"
                }))
            }
            LinuxRequest::Capture { source, path } => {
                let (frame, provenance) = match &source {
                    CaptureSource::Output { .. } | CaptureSource::FocusedOutput {} => {
                        let name = match &source {
                            CaptureSource::Output { name } => name.clone(),
                            _ => self.focused_output()?,
                        };
                        let frame = self.capture()?.capture(wayland_capture::CaptureRequest {
                            source: WaylandSource::Output { name },
                            path,
                        })?;
                        (frame, Provenance::Output)
                    }
                    CaptureSource::Region {
                        x,
                        y,
                        width,
                        height,
                    } => {
                        let frame = self.capture()?.capture(wayland_capture::CaptureRequest {
                            source: WaylandSource::Region {
                                x: *x,
                                y: *y,
                                width: *width,
                                height: *height,
                            },
                            path,
                        })?;
                        (frame, Provenance::Output)
                    }
                    CaptureSource::Window { address } => {
                        let client = self.hypr()?.client_by_address(address)?.ok_or_else(|| {
                            fail("no_window", "No Hyprland window with that address")
                        })?;
                        let frame = self.capture()?.capture(wayland_capture::CaptureRequest {
                            source: WaylandSource::Toplevel {
                                identifier: None,
                                title: Some(client.title.clone()),
                                app_id: Some(client.class.clone()),
                            },
                            path,
                        })?;
                        (frame, Provenance::Window(Box::new(client)))
                    }
                    CaptureSource::X11Window { window_id } => {
                        let frame = self.x11()?.capture_window(*window_id, &path)?;
                        (frame, Provenance::X11Window(*window_id))
                    }
                };
                let window = match &provenance {
                    Provenance::Window(client) => Some((**client).clone()),
                    _ => None,
                };
                let mut result = json!({"frame": frame, "source": source, "window": window});
                result["frame_id"] = json!(
                    self.frames
                        .remember(RetainedFrame { frame, provenance }, self.epoch)
                );
                Ok(result)
            }
            LinuxRequest::ClickImage {
                frame,
                point,
                mode,
                button,
                count,
            } => {
                if !matches!(mode, ClickMode::Global | ClickMode::Process) {
                    return Err(fail(
                        "unsupported",
                        "Image clicks use global or process delivery",
                    ));
                }
                let retained = self.frames.get(frame, self.epoch)?;
                let (mapping, source) = (
                    retained.frame.mapping.clone(),
                    retained.frame.source.clone(),
                );
                let (window, x11_window) = match &retained.provenance {
                    Provenance::Output => (None, None),
                    Provenance::Window(client) => (Some((**client).clone()), None),
                    Provenance::X11Window(id) => (None, Some(*id)),
                };
                let revision = match x11_window {
                    Some(window_id) => self.x11()?.window_revision(window_id)?,
                    None => self.capture()?.revision(&source)?,
                };
                let local = mapping.pixel_to_global(point, &revision)?;
                let (global, window) = match (window, x11_window) {
                    // An X window is still a compositor client: it scopes the
                    // soft cursor and the visibility check, and X coordinates
                    // already map onto the desktop.
                    (None, Some(window_id)) => {
                        let client = self.x11()?.window(window_id).ok().and_then(|record| {
                            let hypr = self.hypr.as_ref()?;
                            hypr.client_for(
                                i64::from(record.pid?),
                                record.title.as_deref().unwrap_or(""),
                            )
                            .ok()
                            .flatten()
                        });
                        if let Some(client) = &client {
                            self.require_visible(client)?;
                        }
                        (local, client)
                    }
                    (Some(client), _) => {
                        // Window captures map pixels onto the window's current position.
                        let current = self
                            .hypr()?
                            .client_by_address(&client.address)?
                            .ok_or_else(|| fail("no_window", "Captured window closed"))?;
                        if current.size != client.size {
                            return Err(fail(
                                "stale_geometry",
                                "Window was resized since the capture",
                            ));
                        }
                        self.require_visible(&current)?;
                        (current.rect().local_to_global(local)?, Some(current))
                    }
                    (None, None) => (local, None),
                };
                match mode {
                    ClickMode::Global => self
                        .global_pointer(
                            PointerAction::Click {
                                point: global,
                                button,
                                count,
                                modifiers: Modifiers::default(),
                            },
                            window.as_ref(),
                        )
                        .map(|r| json!(r)),
                    _ => {
                        let window_id = x11_window.ok_or_else(|| {
                            fail(
                                "unsupported",
                                "Process image clicks require an X11 window capture",
                            )
                        })?;
                        self.x11()?
                            .click_window(window_id, &global, button, count)
                            .map(|r| json!(r))
                    }
                }
            }
            LinuxRequest::ClickWindow {
                target,
                point,
                mode,
                button,
                count,
            } => {
                if matches!(mode, ClickMode::Semantic) {
                    return Err(fail(
                        "invalid_request",
                        "Window coordinates require a pointer route",
                    ));
                }
                let client = self.client_for(&target)?;
                let global = client.rect().local_to_global(point)?;
                self.click_at(
                    Some(&target),
                    global,
                    mode,
                    button,
                    count,
                    Modifiers::default(),
                )
                .map(|r| json!(r))
            }
            LinuxRequest::ScrollTarget {
                target,
                mode,
                vertical,
                horizontal,
            } => {
                let point = self.atspi.element_center(&target)?;
                match mode {
                    ClickMode::Global => {
                        let client = self.client_for(&target)?;
                        self.require_visible(&client)?;
                        self.global_pointer(
                            PointerAction::Scroll {
                                vertical,
                                horizontal,
                                point: Some(point),
                            },
                            Some(&client),
                        )
                        .map(|r| json!(r))
                    }
                    ClickMode::Semantic => Err(fail(
                        "unsupported",
                        "Use component.scroll_to or the container's advertised actions for semantic scrolling",
                    )),
                    _ => Err(fail(
                        "unsupported",
                        "Scrolling supports the global route only",
                    )),
                }
            }
            LinuxRequest::HyprlandShortcut {
                target,
                address,
                mods,
                key,
                state,
            } => {
                let address = match (&address, &target) {
                    (Some(address), _) => address.clone(),
                    (None, Some(target)) => self.client_for(target)?.address,
                    (None, None) => {
                        return Err(fail(
                            "invalid_request",
                            "hyprland_shortcut needs a target reference or window address",
                        ));
                    }
                };
                if !address.starts_with("0x")
                    || !address[2..].chars().all(|c| c.is_ascii_hexdigit())
                    || key.is_empty()
                    || key.len() > 64
                    || mods.len() > 64
                {
                    return Err(fail(
                        "invalid_request",
                        "Window address must be hexadecimal and key names bounded",
                    ));
                }
                let quote = hyprland::lua_string;
                let window = quote(&format!("address:{address}"));
                let lua = match state {
                    None => format!(
                        "hl.dsp.send_shortcut({{ window = {window}, mods = {}, key = {} }})",
                        quote(&mods),
                        quote(&key)
                    ),
                    Some(state) => format!(
                        "hl.dsp.send_key_state({{ window = {window}, mods = {}, key = {}, state = {} }})",
                        quote(&mods),
                        quote(&key),
                        quote(match state {
                            KeyState::Down => "down",
                            KeyState::Up => "up",
                        })
                    ),
                };
                let send = |s: &mut Self| {
                    s.hypr()?.dispatch(&lua)?;
                    Ok(Receipt::dispatched("linux.hyprland.send_shortcut"))
                };
                let receipt = match &target {
                    Some(target) => self.visual_action(target, send)?,
                    None => send(self)?,
                };
                Ok(json!(receipt))
            }
        }
    }
    /// Read-only evidence for one element: native states, geometry, the
    /// owning window's visibility on its workspace and advertised actions.
    fn actionability(&mut self, target: &ElementRef) -> Result<Value> {
        let inspected = self.atspi.inspect(target)?;
        let attributes = &inspected["attributes"];
        let flag = |name: &str| match attributes.get(name) {
            Some(v) => json!({"state":"observed","value":v["value"],"attribute":name}),
            None => json!({"state":"unknown","attribute":name}),
        };
        let bounds = match attributes.get("bounds") {
            Some(b) => json!({"state":"observed","value":b}),
            None => json!({"state":"unknown","reason":"no global bounds"}),
        };
        let window = self.client_for(target);
        let visibility = match &window {
            Ok(client) => match self.require_visible(client) {
                Ok(()) => {
                    json!({"state":"passed","window":client.address,"workspace":client.workspace})
                }
                Err(e) => json!({"state":"failed","error":e}),
            },
            Err(e) => json!({"state":"unknown","error":e}),
        };
        Ok(json!({
            "reference": target,
            "effect": "none",
            "native": {"sensitive": flag("state.sensitive"), "showing": flag("state.showing"), "visible": flag("state.visible"), "focused": flag("state.focused"), "enabled": flag("state.enabled")},
            "bounds": bounds,
            "bounds_source": attributes.get("bounds_source"),
            "routes": {
                "global": {"window_visibility": visibility, "scope": "compositor workspace membership and mapped state; not occlusion by other windows"},
                "semantic": {"advertised": inspected["actions"], "scope": "advertised actions do not prove successful activation"},
                "process": {"state": if window.as_ref().is_ok_and(|c| c.xwayland) {"available_x11_send_event"} else {"unavailable"}}
            },
            "note": "No aggregate clickable boolean. Evidence is sampled sequentially and may change after this query."
        }))
    }
}

impl Drop for LinuxSession {
    fn drop(&mut self) {
        if let Some(mut cursor) = self.cursor.take() {
            let _ = cursor.stop();
        }
    }
}

/// Serves the JSONL protocol on stdio until EOF.
pub fn serve(format: OutputFormat) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut session = LinuxSession::connect()?;
    transport::serve(
        std::io::stdin().lock(),
        std::io::stdout().lock(),
        format,
        |request| session.dispatch(request).map(transport::ValueReply::from),
    )?;
    Ok(())
}

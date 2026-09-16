use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureBackend {
    #[default]
    Native,
    Executable,
}
use crate::{
    Accessibility, QuartzInput,
    capture::{self, CaptureRequest, CaptureSource, Frame, ScreenshotCapture},
    skylight::*,
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};
use unimation::{
    session::{ActionEpoch, FrameHistory, SnapshotHistory, wait_attribute},
    *,
};

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum MacRequest {
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
        source: CaptureSource,
        path: PathBuf,
        #[serde(default)]
        backend: CaptureBackend,
        #[serde(default)]
        max_pixel_edge: Option<u32>,
    },
    Displays {},
    Windows {},
    Capabilities {},
    CursorState {},
    Actionability {
        target: ElementRef,
    },
    /// Starts hidden; follows subsequently dispatched SkyLight pointer packets.
    CursorOverlay {
        action: CursorOverlayAction,
    },
    /// Frame IDs refer to captures owned by this process, not client-supplied transforms.
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
    SkylightPointer {
        target: ElementRef,
        action: PointerAction,
    },
    ScrollTarget {
        target: ElementRef,
        mode: ClickMode,
        vertical: i32,
        horizontal: i32,
    },
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CursorOverlayAction {
    Start { executable: PathBuf },
    Stop {},
}
pub struct MacSession {
    ax: Accessibility,
    input: QuartzInput,
    sky: Option<SkyLightInput>,
    overlay: Option<overlay::OverlayController>,
    overlay_error: Option<NativeError>,
    snapshots: SnapshotHistory,
    frames: FrameHistory<Frame>,
    epoch: ActionEpoch,
}
fn fail(code: &str, message: &str) -> NativeError {
    NativeError::new(code, message)
}
impl MacSession {
    pub fn new() -> Self {
        Self {
            ax: Accessibility::new(),
            input: QuartzInput,
            sky: None,
            overlay: None,
            overlay_error: None,
            snapshots: SnapshotHistory::default(),
            frames: FrameHistory::default(),
            epoch: ActionEpoch::default(),
        }
    }
    fn snapshot(&self, revision: Option<u64>) -> Result<&Arc<Snapshot>> {
        self.snapshots.get(revision)
    }
    fn remember(&mut self, snapshot: Snapshot) -> Value {
        json!(self.snapshots.remember(snapshot).as_ref())
    }
    fn sky_target(&mut self, target: &ElementRef, point: Point) -> Result<SkyLightTarget> {
        let window = self.ax.native_window(target)?;
        Ok(SkyLightTarget {
            pid: window.pid,
            window_id: window.window_id,
            window_local: Point {
                x: point.x - window.bounds.x,
                y: point.y - window.bounds.y,
            },
            desktop: point,
        })
    }
    fn sky_dispatch(
        &mut self,
        target: &SkyLightTarget,
        action: SkyLightPointerAction,
    ) -> Result<Receipt> {
        if self.sky.is_none() {
            self.sky = Some(SkyLightInput::new()?);
        }
        let pulse = matches!(action, SkyLightPointerAction::Click { .. });
        let visual_duration = if matches!(action, SkyLightPointerAction::Move { .. }) {
            180
        } else {
            0
        };
        if let Some(overlay) = self.overlay.as_mut() {
            self.overlay_error = overlay
                .scope(overlay::CursorScope::Window {
                    window_id: u64::from(target.window_id),
                    pid: target.pid,
                })
                .err();
        }
        let overlay = &mut self.overlay;
        let visual_error = &mut self.overlay_error;
        let result = self
            .sky
            .as_mut()
            .expect("provider initialized")
            .pointer_at_observed(target, action, |point| {
                if let Some(overlay) = overlay.as_mut()
                    && let Err(error) = overlay
                        .move_to(point.x, point.y, visual_duration)
                        .and_then(|()| overlay.show())
                {
                    *visual_error = Some(error);
                }
            });
        // Visual failures never turn dispatched input into a retryable error.
        if pulse
            && result
                .as_ref()
                .is_ok_and(|r| matches!(r.effect, Effect::Dispatched))
            && let Some(overlay) = self.overlay.as_mut()
            && let Some(point) = self.sky.as_ref().and_then(SkyLightInput::last_pointer)
            && let Err(error) = overlay.click(point.x, point.y)
        {
            self.overlay_error = Some(error);
        }
        result
    }

    fn sky_button(button: MouseButton) -> SkyLightButton {
        match button {
            MouseButton::Left => SkyLightButton::Left,
            MouseButton::Right => SkyLightButton::Right,
            MouseButton::Middle => SkyLightButton::Middle,
        }
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
        if !matches!(mode, ClickMode::Semantic)
            && let Some(target) = target
        {
            self.ax.require_pointer_access(target)?;
            self.ax.require_point_in_viewport(target, &point)?;
        }
        match mode {
            ClickMode::Semantic => {
                let target = ClickMode::require_semantic_target(target, button, count, modifiers)?;
                self.ax.semantic(
                    target,
                    SemanticAction::Perform {
                        name: "AXPress".into(),
                    },
                )
            }
            ClickMode::Global => {
                if let Some(target) = target {
                    self.ax.require_hit(target, &point)?;
                }
                self.input.pointer(
                    Delivery::Global {},
                    PointerAction::Click {
                        point,
                        button,
                        count,
                        modifiers,
                    },
                )
            }
            ClickMode::Process => {
                let pid = self.ax.element_pid(target.ok_or_else(|| {
                    fail("invalid_request", "Process mode requires target reference")
                })?)?;
                self.input.pointer(
                    Delivery::Process { pid },
                    PointerAction::Click {
                        point,
                        button,
                        count,
                        modifiers,
                    },
                )
            }
            ClickMode::Skylight => {
                let target = self.sky_target(
                    target.ok_or_else(|| {
                        fail("invalid_request", "SkyLight requires a window reference")
                    })?,
                    point,
                )?;
                self.sky_dispatch(
                    &target,
                    SkyLightPointerAction::Click {
                        button: Self::sky_button(button),
                        count,
                        modifiers,
                    },
                )
            }
        }
    }
    pub fn dispatch(&mut self, mut value: Value) -> Result<Value> {
        transport::prepare(&mut value, || Ok(self.ax.session_id().to_owned()))?;
        let extension = matches!(
            value["op"].as_str(),
            Some(
                "capture"
                    | "displays"
                    | "windows"
                    | "capabilities"
                    | "click_image"
                    | "click_window"
                    | "scroll_target"
                    | "skylight_pointer"
                    | "cursor_state"
                    | "cursor_overlay"
                    | "actionability"
                    | "snapshot"
            )
        );
        if extension {
            let request: MacRequest = serde_json::from_value(value)
                .map_err(|e| fail("invalid_request", &e.to_string()))?;
            return self.extension(request);
        }
        let request: SessionRequest =
            serde_json::from_value(value).map_err(|e| fail("invalid_request", &e.to_string()))?;
        self.dispatch_core(request)
    }
    pub fn dispatch_core(&mut self, request: SessionRequest) -> Result<Value> {
        let result = self.execute_core(request);
        self.epoch.record(&result);
        result
    }
    fn execute_core(&mut self, request: SessionRequest) -> Result<Value> {
        match request {
            SessionRequest::View {
                revision,
                options,
                format,
            } => Ok(json!(session::render(
                self.snapshot(revision)?.clone(),
                format,
                &options
            )?)),
            SessionRequest::DiffView {
                before,
                after,
                options,
                max_changes,
            } => Ok(json!(presentation::render_view_diff(
                self.snapshot(Some(before))?,
                self.snapshot(after)?,
                &options,
                max_changes
            )?)),
            SessionRequest::ParameterizedAttribute {
                target,
                name,
                parameter,
            } => self.ax.read_parameterized(&target, &name, parameter),
            SessionRequest::WaitAttribute {
                target,
                name,
                expected,
                timeout_ms,
            } => wait_attribute(timeout_ms, &expected, || {
                self.ax.read_attribute(&target, &name)
            }),
            SessionRequest::Discover { scope, format } => self
                .ax
                .discover()
                .map(|raw| discovery::present_discovery(&raw, scope, format)),
            SessionRequest::Observe { request } => {
                let s = self.ax.observe(request)?;
                Ok(self.remember(s))
            }
            SessionRequest::ObserveSubtree {
                target,
                max_nodes,
                max_depth,
            } => {
                let s = self.ax.observe_subtree(&target, max_nodes, max_depth)?;
                Ok(self.remember(s))
            }
            SessionRequest::Inspect { target } => self.ax.inspect(&target),
            SessionRequest::Attribute { target, name } => self.ax.read_attribute(&target, &name),
            SessionRequest::Semantic { target, action } => {
                self.ax.semantic(&target, action).map(|r| json!(r))
            }
            SessionRequest::Pointer { delivery, action } => {
                self.input.pointer(delivery, action).map(|r| json!(r))
            }
            SessionRequest::Text { delivery, text } => {
                self.input.type_text(delivery, &text).map(|r| json!(r))
            }
            SessionRequest::Key { delivery, chord } => {
                self.input.key_press(delivery, chord).map(|r| json!(r))
            }
            SessionRequest::HitTest { point } => self.ax.hit_test(point).map(|r| json!(r)),
            SessionRequest::Window { target } => self.ax.native_window(&target).map(|r| json!(r)),
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
                    self.ax.element_center(&target)?
                };
                self.click_at(Some(&target), point, mode, button, count, modifiers)
                    .map(|r| json!(r))
            }
            SessionRequest::Diff { before, after } => {
                diff::diff_snapshots(self.snapshot(Some(before))?, self.snapshot(after)?)
                    .map(|d| json!(d))
            }
            SessionRequest::Query { revision, query } => {
                Ok(json!(query::query_nodes(self.snapshot(revision)?, &query)))
            }
            SessionRequest::Snapshots {} => Ok(json!(self.snapshots.summaries())),
        }
    }
    pub fn extension(&mut self, request: MacRequest) -> Result<Value> {
        let result = self.execute_extension(request);
        self.epoch.record(&result);
        result
    }
    fn execute_extension(&mut self, request: MacRequest) -> Result<Value> {
        match request {
            MacRequest::Snapshot {
                request,
                scope,
                options,
                format,
            } => {
                let snapshot = match scope {
                    SnapshotScope::Application => self.ax.observe(request)?,
                    SnapshotScope::FocusedWindow => {
                        let app = self.ax.observe(ObserveRequest {
                            pid: request.pid,
                            max_nodes: 1,
                            max_depth: 0,
                        })?;
                        let window = self.ax.read_attribute(&app.root, "AXFocusedWindow")?;
                        let reference: ElementRef = serde_json::from_value(window["value"].clone())
                            .map_err(|_| {
                                fail(
                                    "no_window",
                                    "Application has no focused AX window; use application scope",
                                )
                            })?;
                        self.ax
                            .observe_subtree(&reference, request.max_nodes, request.max_depth)?
                    }
                };
                let snapshot = self.snapshots.remember(snapshot);
                Ok(json!(session::render(snapshot, format, &options)?))
            }
            MacRequest::Actionability { target } => self.ax.actionability(&target),
            MacRequest::CursorState {} => {
                let alive = match self.overlay.as_mut() {
                    Some(o) => o.is_running()?,
                    None => false,
                };
                Ok(json!({
                    "skylight": self.sky.as_ref().and_then(SkyLightInput::last_target),
                    "meaning": "last_dispatched_pointer_packet",
                    "consumption_verified": false,
                    "overlay_running": alive,
                    "overlay_configured": self.overlay.is_some(),
                    "overlay_pid": self.overlay.as_ref().and_then(overlay::OverlayController::pid),
                    "overlay_error": self.overlay_error,
                    "overlay_acknowledgement": "queued_only"
                }))
            }
            MacRequest::CursorOverlay { action } => {
                match action {
                    CursorOverlayAction::Start { executable } => {
                        if self.overlay.is_some() {
                            return Err(fail(
                                "overlay_running",
                                "Stop the current overlay before replacing it",
                            ));
                        }
                        self.overlay = Some(overlay::OverlayController::start(executable)?);
                        self.overlay_error = None;
                    }
                    CursorOverlayAction::Stop {} => {
                        if let Some(mut overlay) = self.overlay.take() {
                            overlay.stop()?;
                        }
                        self.overlay_error = None;
                    }
                }
                Ok(json!({"overlay_running":self.overlay.is_some(),"render_acknowledged":false}))
            }
            MacRequest::Capabilities {} => Ok(
                json!({"skylight":SkyLightInput::capabilities(),"snapshots_retained":self.snapshots.capacity(),"frames_retained":self.frames.capacity(),"quartz":{"global":true,"process":true,"consumption_verified":false}}),
            ),
            MacRequest::Displays {} => capture::displays().map(|v| json!(v)),
            MacRequest::Windows {} => crate::spaces::windows().map(|v| json!(v)),
            MacRequest::Capture {
                source,
                path,
                backend,
                max_pixel_edge,
            } => {
                let request = CaptureRequest { source, path };
                let frame = match backend {
                    CaptureBackend::Native => {
                        #[cfg(feature = "native-capture")]
                        {
                            crate::capture_native::NativeScreenshotCapture { max_pixel_edge }
                                .capture(request)?
                        }
                        #[cfg(not(feature = "native-capture"))]
                        {
                            return Err(fail(
                                "unsupported",
                                "Build without native-capture; select executable provider explicitly",
                            ));
                        }
                    }
                    CaptureBackend::Executable => {
                        if max_pixel_edge.is_some() {
                            return Err(fail(
                                "unsupported",
                                "Resize requires native capture backend",
                            ));
                        }
                        ScreenshotCapture.capture(request)?
                    }
                };
                let mut result = json!({"frame":frame});
                result["frame_id"] = json!(self.frames.remember(frame, self.epoch));
                Ok(result)
            }
            MacRequest::ClickImage {
                frame,
                point,
                mode,
                button,
                count,
            } => {
                let frame = self.frames.get(frame, self.epoch)?;
                let point = frame
                    .mapping
                    .pixel_to_global(point, &capture::current_revision(&frame.source)?)?;
                let receipt = match (&frame.source, mode) {
                    (CaptureSource::Window { window_id }, ClickMode::Skylight) => {
                        let pid = i32::try_from(
                            frame
                                .owner_pid
                                .ok_or_else(|| fail("no_window", "Missing owner"))?,
                        )
                        .map_err(|_| fail("no_window", "Invalid owner PID"))?;
                        let target = SkyLightTarget {
                            pid,
                            window_id: *window_id,
                            window_local: Point {
                                x: point.x - frame.mapping.source_bounds.x,
                                y: point.y - frame.mapping.source_bounds.y,
                            },
                            desktop: point,
                        };
                        self.sky_dispatch(
                            &target,
                            SkyLightPointerAction::Click {
                                button: Self::sky_button(button),
                                count,
                                modifiers: Modifiers::default(),
                            },
                        )?
                    }
                    (CaptureSource::Window { window_id }, ClickMode::Global) => {
                        let hit = self.ax.hit_test(point.clone())?;
                        self.ax.require_pointer_access(&hit)?;
                        let actual = self.ax.native_window(&hit)?;
                        if actual.window_id != *window_id
                            || Some(actual.pid as i64) != frame.owner_pid
                        {
                            return Err(fail(
                                "occluded_or_moved",
                                "Window screenshot is not the window receiving this global click",
                            ));
                        }
                        self.input.pointer(
                            Delivery::Global {},
                            PointerAction::Click {
                                point,
                                button,
                                count,
                                modifiers: Modifiers::default(),
                            },
                        )?
                    }
                    (CaptureSource::Window { .. }, ClickMode::Process) => {
                        let pid = i32::try_from(
                            frame
                                .owner_pid
                                .ok_or_else(|| fail("no_window", "Missing owner"))?,
                        )
                        .map_err(|_| fail("no_window", "Invalid owner PID"))?;
                        self.input.pointer(
                            Delivery::Process { pid },
                            PointerAction::Click {
                                point,
                                button,
                                count,
                                modifiers: Modifiers::default(),
                            },
                        )?
                    }
                    (CaptureSource::Display { .. }, ClickMode::Global) => self.input.pointer(
                        Delivery::Global {},
                        PointerAction::Click {
                            point,
                            button,
                            count,
                            modifiers: Modifiers::default(),
                        },
                    )?,
                    _ => {
                        return Err(fail(
                            "unsupported",
                            "Display pixels require global delivery; targeted image clicks require a window capture",
                        ));
                    }
                };
                Ok(json!(receipt))
            }
            MacRequest::ClickWindow {
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
                self.ax.require_pointer_access(&target)?;
                let w = self.ax.native_window(&target)?;
                let global = w.bounds.local_to_global(point.clone())?;
                if matches!(mode, ClickMode::Skylight) {
                    let t = SkyLightTarget {
                        pid: w.pid,
                        window_id: w.window_id,
                        desktop: global,
                        window_local: point,
                    };
                    self.sky_dispatch(
                        &t,
                        SkyLightPointerAction::Click {
                            button: Self::sky_button(button),
                            count,
                            modifiers: Modifiers::default(),
                        },
                    )
                    .map(|r| json!(r))
                } else {
                    self.click_at(
                        Some(&w.reference),
                        global,
                        mode,
                        button,
                        count,
                        Modifiers::default(),
                    )
                    .map(|r| json!(r))
                }
            }
            MacRequest::SkylightPointer { target, action } => {
                self.ax.require_pointer_access(&target)?;
                let (point, action) = match action {
                    PointerAction::Move { point } => (
                        point,
                        SkyLightPointerAction::Move {
                            modifiers: Modifiers::default(),
                        },
                    ),
                    PointerAction::Click {
                        point,
                        button,
                        count,
                        modifiers,
                    } => (
                        point,
                        SkyLightPointerAction::Click {
                            button: Self::sky_button(button),
                            count,
                            modifiers,
                        },
                    ),
                    PointerAction::Scroll {
                        vertical,
                        horizontal,
                        point,
                    } => (
                        match point {
                            Some(p) => p,
                            None => self.ax.element_center(&target)?,
                        },
                        SkyLightPointerAction::Scroll {
                            vertical,
                            horizontal,
                            modifiers: Modifiers::default(),
                        },
                    ),
                    PointerAction::Drag {
                        from,
                        to,
                        button,
                        modifiers,
                        duration_ms,
                    } => (
                        from,
                        SkyLightPointerAction::Drag {
                            to,
                            button: Self::sky_button(button),
                            modifiers,
                            duration_ms,
                        },
                    ),
                };
                let target = self.sky_target(&target, point)?;
                self.sky_dispatch(&target, action).map(|r| json!(r))
            }
            MacRequest::ScrollTarget {
                target,
                mode,
                vertical,
                horizontal,
            } => {
                self.ax.require_pointer_access(&target)?;
                let point = self.ax.element_center(&target)?;
                let receipt = match mode {
                    ClickMode::Skylight => {
                        let t = self.sky_target(&target, point)?;
                        self.sky_dispatch(
                            &t,
                            SkyLightPointerAction::Scroll {
                                vertical,
                                horizontal,
                                modifiers: Modifiers::default(),
                            },
                        )?
                    }
                    ClickMode::Global => {
                        self.ax.require_hit(&target, &point)?;
                        self.input.pointer(
                            Delivery::Global {},
                            PointerAction::Scroll {
                                vertical,
                                horizontal,
                                point: Some(point),
                            },
                        )?
                    }
                    ClickMode::Process => self.input.pointer(
                        Delivery::Process {
                            pid: self.ax.element_pid(&target)?,
                        },
                        PointerAction::Scroll {
                            vertical,
                            horizontal,
                            point: Some(point),
                        },
                    )?,
                    ClickMode::Semantic => {
                        return Err(fail(
                            "unsupported",
                            "Use an explicit native AX scroll action for semantic scrolling",
                        ));
                    }
                };
                Ok(json!(receipt))
            }
        }
    }
}

impl Default for MacSession {
    fn default() -> Self {
        Self::new()
    }
}

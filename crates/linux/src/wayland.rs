//! Explicit, persistent XDG RemoteDesktop sessions using the D-Bus Notify route.
//! This provider never connects to EIS or guesses desktop-to-stream coordinates.
use crate::{DesktopProvider, Frame, error, unsupported};
use ashpd::desktop::{
    Session,
    remote_desktop::{DeviceType, KeyState, RemoteDesktop, SelectDevicesOptions},
    screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType, Stream},
};
use futures_lite::{
    StreamExt,
    future::{block_on, poll_once},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    path::Path,
    time::{Duration, Instant},
};
use unimation::{
    Delivery, Effect, KeyChord, KeyboardInput, Modifiers, MouseButton, NativeError, Point,
    PointerAction, PointerInput, Receipt, Result,
    motion::{MotionPlan, MotionStyle},
};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartOptions {
    pub keyboard: bool,
    pub pointer: bool,
    /// Request monitor streams and logical coordinate metadata; does not capture frames.
    pub screencast: bool,
}
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Status {},
    Stop {},
    SelectStream {
        stream: u32,
    },
    RelativeMotion {
        dx: f64,
        dy: f64,
    },
    /// Continuous portal axis units, not portable pixel scroll distances.
    Axis {
        dx: f64,
        dy: f64,
    },
}

pub struct Portal {
    remote: RemoteDesktop,
    session: Session<RemoteDesktop>,
    closed_signal: zbus::proxy::SignalStream<'static>,
    closed: bool,
    keyboard: bool,
    pointer: bool,
    streams: Vec<Stream>,
    selected_stream: Option<u32>,
}
impl Portal {
    /// The only constructor that shows consent UI. Discovery never calls it.
    pub fn start(options: StartOptions) -> Result<Self> {
        if !options.keyboard && !options.pointer {
            return Err(error(
                "invalid_request",
                "Select keyboard or pointer access",
            ));
        }
        block_on(async {
            let connection = zbus::connection::Builder::session()
                .map_err(portal_error)?
                .method_timeout(Duration::from_secs(5))
                .build()
                .await
                .map_err(portal_error)?;
            let remote = RemoteDesktop::with_connection(connection)
                .await
                .map_err(portal_error)?;
            let mut devices = DeviceType::Keyboard | DeviceType::Pointer;
            if !options.keyboard {
                devices.remove(DeviceType::Keyboard);
            }
            if !options.pointer {
                devices.remove(DeviceType::Pointer);
            }
            let available = remote
                .available_device_types()
                .await
                .map_err(portal_error)?;
            if !available.contains(devices) {
                return Err(unsupported(
                    "Requested devices are unavailable in this portal backend",
                ));
            }
            let session = remote
                .create_session(Default::default())
                .await
                .map_err(portal_error)?;
            // Subscribe before Start so early revocation is not lost.
            let watch = async {
                let path: String = serde_json::from_value(json!(&session))
                    .map_err(|e| error("portal_session", e))?;
                let proxy = zbus::Proxy::new_owned(
                    remote.connection().clone(),
                    "org.freedesktop.portal.Desktop",
                    path,
                    "org.freedesktop.portal.Session",
                )
                .await
                .map_err(portal_error)?;
                proxy.receive_signal("Closed").await.map_err(portal_error)
            }
            .await;
            let closed_signal = match watch {
                Ok(stream) => stream,
                Err(e) => {
                    let _ = session.close().await;
                    return Err(e);
                }
            };
            let setup = async {
                remote
                    .select_devices(
                        &session,
                        SelectDevicesOptions::default().set_devices(devices),
                    )
                    .await
                    .map_err(portal_error)?
                    .response()
                    .map_err(portal_error)?;
                if options.screencast {
                    let cast = Screencast::with_connection(remote.connection().clone())
                        .await
                        .map_err(portal_error)?;
                    if !cast
                        .available_source_types()
                        .await
                        .map_err(portal_error)?
                        .contains(SourceType::Monitor)
                    {
                        return Err(unsupported("This ScreenCast backend cannot share monitors"));
                    }
                    let mut sources = SelectSourcesOptions::default()
                        .set_sources(Some(SourceType::Monitor.into()))
                        .set_multiple(true);
                    if cast.version() >= 2
                        && cast
                            .available_cursor_modes()
                            .await
                            .map_err(portal_error)?
                            .contains(CursorMode::Embedded)
                    {
                        sources = sources.set_cursor_mode(CursorMode::Embedded);
                    }
                    cast.select_sources(&session, sources)
                        .await
                        .map_err(portal_error)?
                        .response()
                        .map_err(portal_error)?;
                }
                remote
                    .start(&session, None, Default::default())
                    .await
                    .map_err(portal_error)?
                    .response()
                    .map_err(portal_error)
            }
            .await;
            let response = match setup {
                Ok(r) => r,
                Err(e) => {
                    let _ = session.close().await;
                    return Err(e);
                }
            };
            Ok(Self {
                remote,
                session,
                closed_signal,
                closed: false,
                keyboard: response.devices().contains(DeviceType::Keyboard),
                pointer: response.devices().contains(DeviceType::Pointer),
                streams: response.streams().to_vec(),
                selected_stream: None,
            })
        })
    }
    fn refresh(&mut self) {
        if block_on(poll_once(self.closed_signal.next())).is_some() {
            self.closed = true;
        }
    }
    fn live(&mut self, keyboard: bool) -> Result<()> {
        self.refresh();
        if self.closed {
            return Err(error(
                "portal_session_closed",
                "Portal session closed or revoked; explicitly start a new session",
            ));
        }
        if if keyboard {
            !self.keyboard
        } else {
            !self.pointer
        } {
            return Err(error(
                "portal_device_not_granted",
                "The user did not grant this input device",
            ));
        }
        Ok(())
    }
    pub fn status(&mut self) -> Value {
        self.refresh();
        json!({"route":"wayland_portal_notify","closed":self.closed,"granted":{"keyboard":self.keyboard,"pointer":self.pointer},
            "streams": self.streams.iter().map(|s| json!({"id":s.pipe_wire_node_id(),"position":s.position(),"logical_size":s.size(),"native":s})).collect::<Vec<_>>(),
            "selected_stream":self.selected_stream,"coordinate_space":"selected_stream_logical","capture":false,"input_transport":"dbus_notify","runtime_verified":false})
    }
    pub fn command(&mut self, command: Command) -> Result<Value> {
        match command {
            Command::Status {} => Ok(self.status()),
            Command::Stop {} => {
                self.close()?;
                Ok(self.status())
            }
            Command::SelectStream { stream } => {
                self.live(false)?;
                if !self.streams.iter().any(|s| s.pipe_wire_node_id() == stream) {
                    return Err(error(
                        "unknown_stream",
                        "Stream was not granted by this session",
                    ));
                }
                self.selected_stream = Some(stream);
                Ok(self.status())
            }
            Command::Axis { dx, dy } => {
                self.live(false)?;
                finite(dx, dy)?;
                block_on(self.remote.notify_pointer_axis(
                    &self.session,
                    dx,
                    dy,
                    Default::default(),
                ))
                .map_err(input_error)?;
                block_on(
                    self.remote.notify_pointer_axis(
                        &self.session,
                        0.,
                        0.,
                        ashpd::desktop::remote_desktop::NotifyPointerAxisOptions::default()
                            .set_finish(true),
                    ),
                )
                .map_err(input_error)?;
                Ok(json!(receipt()))
            }
            Command::RelativeMotion { dx, dy } => {
                self.live(false)?;
                finite(dx, dy)?;
                block_on(self.remote.notify_pointer_motion(
                    &self.session,
                    dx,
                    dy,
                    Default::default(),
                ))
                .map_err(input_error)?;
                Ok(json!(receipt()))
            }
        }
    }
    /// Hand off the granted streams to an independently implemented capture provider.
    /// This FD alone does not provide decoded frames or a screenshot coordinate mapping.
    pub fn open_pipewire_remote(&mut self) -> Result<std::os::fd::OwnedFd> {
        self.refresh();
        if self.closed {
            return Err(error("portal_session_closed", "Portal session closed"));
        }
        if self.streams.is_empty() {
            return Err(error("no_streams", "No screen streams were granted"));
        }
        block_on(async {
            Screencast::with_connection(self.remote.connection().clone())
                .await
                .map_err(portal_error)?
                .open_pipe_wire_remote(&self.session, Default::default())
                .await
                .map_err(portal_error)
        })
    }
    pub fn close(&mut self) -> Result<()> {
        self.refresh();
        if !self.closed {
            let result = block_on(self.session.close()).map_err(portal_error);
            self.closed = true;
            result?;
        }
        Ok(())
    }
    fn validate_point(&self, point: &Point) -> Result<u32> {
        finite(point.x, point.y)?;
        let id=self.selected_stream.ok_or_else(|| error("stream_required", "Select a granted stream before absolute input; coordinates are stream-local logical units"))?;
        let stream = self
            .streams
            .iter()
            .find(|s| s.pipe_wire_node_id() == id)
            .ok_or_else(|| error("unknown_stream", "Selected stream is unavailable"))?;
        if let Some((w, h)) = stream.size() {
            if point.x < 0. || point.y < 0. || point.x >= f64::from(w) || point.y >= f64::from(h) {
                return Err(error(
                    "point_outside_stream",
                    "Point is outside the selected stream's logical bounds",
                ));
            }
        } else {
            return Err(error(
                "stream_geometry_unknown",
                "Portal did not supply logical bounds; absolute input is unavailable",
            ));
        }
        Ok(id)
    }
    fn move_to(&mut self, point: &Point) -> Result<()> {
        self.live(false)?;
        let stream = self.validate_point(point)?;
        block_on(self.remote.notify_pointer_motion_absolute(
            &self.session,
            stream,
            point.x,
            point.y,
            Default::default(),
        ))
        .map_err(input_error)
    }
    fn button(&mut self, button: MouseButton, state: KeyState) -> Result<()> {
        self.live(false)?;
        let code = match button {
            MouseButton::Left => 272,
            MouseButton::Right => 273,
            MouseButton::Middle => 274,
        };
        block_on(
            self.remote
                .notify_pointer_button(&self.session, code, state, Default::default()),
        )
        .map_err(input_error)
    }
    fn key(&mut self, code: i32, state: KeyState) -> Result<()> {
        self.live(true)?;
        block_on(self.remote.notify_keyboard_keycode(
            &self.session,
            code,
            state,
            Default::default(),
        ))
        .map_err(input_error)
    }
    fn with_modifiers(
        &mut self,
        modifiers: Modifiers,
        action: impl FnOnce(&mut Self) -> Result<()>,
    ) -> Result<()> {
        let keys = modifier_codes(modifiers);
        if !keys.is_empty() {
            self.live(true)?;
        }
        let mut pressed = Vec::new();
        let result = (|| {
            for code in keys {
                pressed.push(code);
                self.key(code, KeyState::Pressed)?;
            }
            action(self)
        })();
        let mut release_error = None;
        for code in pressed.into_iter().rev() {
            if let Err(e) = self.key(code, KeyState::Released) {
                release_error = Some(e);
            }
        }
        finish_input(result, release_error.map_or(Ok(()), Err)).map_err(|mut e| {
            e.effect = Effect::Unknown;
            e
        })
    }
}
impl Drop for Portal {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
impl PointerInput for Portal {
    fn pointer(&mut self, delivery: Delivery, action: PointerAction) -> Result<Receipt> {
        global(delivery)?;
        self.live(false)?;
        match action {
            PointerAction::Move { point } => self.move_to(&point)?,
            PointerAction::Click {
                point,
                button,
                count,
                modifiers,
            } => {
                self.validate_point(&point)?;
                if count == 0 || count > 3 {
                    return Err(error("invalid_click_count", "Click count must be 1..=3"));
                }
                self.with_modifiers(modifiers, |s| {
                    s.move_to(&point)?;
                    for _ in 0..count {
                        let down = s.button(button, KeyState::Pressed);
                        let up = s.button(button, KeyState::Released);
                        finish_input(down, up)?;
                    }
                    Ok(())
                })?;
            }
            PointerAction::Scroll { .. } => {
                return Err(unsupported(
                    "Portal axis units are not portable pixels; use wayland command kind=axis with explicit portal dx/dy",
                ));
            }
            PointerAction::Drag {
                from,
                to,
                button,
                modifiers,
                duration_ms,
            } => {
                self.validate_point(&from)?;
                self.validate_point(&to)?;
                if duration_ms > 30_000 {
                    return Err(error(
                        "invalid_duration",
                        "Drag duration must be at most 30000 ms",
                    ));
                }
                let plan = MotionPlan::new(
                    (from.x, from.y),
                    (to.x, to.y),
                    Duration::from_millis(duration_ms),
                    ((duration_ms / 16).max(1)) as usize,
                    MotionStyle::Straight,
                )?;
                self.with_modifiers(modifiers, |s| {
                    s.move_to(&from)?;
                    let result = (|| {
                        s.button(button, KeyState::Pressed)?;
                        let start = Instant::now();
                        for sample in plan.samples() {
                            std::thread::sleep(sample.at.saturating_sub(start.elapsed()));
                            s.move_to(&Point {
                                x: sample.position.0,
                                y: sample.position.1,
                            })?;
                        }
                        Ok(())
                    })();
                    let up = s.button(button, KeyState::Released);
                    finish_input(result, up)
                })?;
            }
        }
        Ok(receipt())
    }
}
impl KeyboardInput for Portal {
    type Key = KeyChord;
    fn key_press(&mut self, delivery: Delivery, chord: KeyChord) -> Result<Receipt> {
        global(delivery)?;
        self.live(true)?;
        if chord.key_code > 767 {
            return Err(error(
                "invalid_keycode",
                "Portal key codes use Linux evdev, range 0..=767",
            ));
        }
        if modifier_codes(chord.modifiers).contains(&i32::from(chord.key_code)) {
            return Err(error(
                "duplicate_modifier",
                "Main key cannot also be held as a modifier",
            ));
        }
        self.with_modifiers(chord.modifiers, |s| {
            let down = s.key(i32::from(chord.key_code), KeyState::Pressed);
            let up = s.key(i32::from(chord.key_code), KeyState::Released);
            finish_input(down, up)
        })?;
        Ok(receipt())
    }
}
impl DesktopProvider for Portal {
    fn route(&self) -> &'static str {
        "wayland_portal_notify"
    }
    fn descriptor(&self) -> Value {
        json!({"implementation":self.route(),"operations":["move","click","drag","key","wayland"],"pixel_scroll":false,"availability":"checked_per_action_from_live_session","capture":false,"input_transport":"dbus_notify","coordinate_space":"selected_stream_logical","keycode_space":"linux_evdev","runtime_verified":false})
    }
    fn windows(&mut self) -> Result<Value> {
        Err(unsupported(
            "RemoteDesktop does not enumerate foreign windows; use AT-SPI or a compositor provider",
        ))
    }
    fn displays(&mut self) -> Result<Value> {
        Ok(self.status())
    }
    fn capture_root(&mut self, _path: &Path) -> Result<Frame> {
        Err(unsupported(
            "PipeWire frame capture is not implemented; stream metadata is not a screenshot mapping",
        ))
    }
    fn wayland(&mut self, command: Command) -> Result<Value> {
        self.command(command)
    }
}
fn finish_input(result: Result<()>, cleanup: Result<()>) -> Result<()> {
    match (result, cleanup) {
        (Err(mut action), Err(release)) => {
            action.effect = Effect::Unknown;
            action
                .message
                .push_str(&format!("; release cleanup failed: {release}"));
            Err(action)
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
        _ => Ok(()),
    }
}
fn global(delivery: Delivery) -> Result<()> {
    if matches!(delivery, Delivery::Global {}) {
        Ok(())
    } else {
        Err(unsupported(
            "Portal input is shared desktop input, not background process input",
        ))
    }
}
fn finite(x: f64, y: f64) -> Result<()> {
    if x.is_finite() && y.is_finite() && x.abs() <= 1e9 && y.abs() <= 1e9 {
        Ok(())
    } else {
        Err(error(
            "invalid_coordinate",
            "Coordinates must be finite within +/-1e9 logical units",
        ))
    }
}
fn modifier_codes(m: Modifiers) -> Vec<i32> {
    [(m.control, 29), (m.shift, 42), (m.alt, 56), (m.meta, 125)]
        .into_iter()
        .filter_map(|(pressed, key)| pressed.then_some(key))
        .collect()
}
fn receipt() -> Receipt {
    Receipt {
        effect: Effect::Dispatched,
        route: "wayland_portal_notify".into(),
    }
}
fn portal_error(e: impl std::fmt::Display) -> NativeError {
    error("wayland_portal", e)
}
fn input_error(e: impl std::fmt::Display) -> NativeError {
    NativeError {
        effect: Effect::Unknown,
        ..error("wayland_portal_input", e)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn process_input_has_no_global_fallback() {
        assert!(global(Delivery::Process { pid: 12 }).is_err());
    }
    #[test]
    fn invalid_coordinates_rejected() {
        assert!(finite(f64::NAN, 0.).is_err());
        assert!(finite(0., f64::INFINITY).is_err());
    }
    #[test]
    fn modifiers_use_evdev_codes() {
        assert_eq!(
            modifier_codes(Modifiers {
                shift: true,
                control: true,
                alt: true,
                meta: true
            }),
            vec![29, 42, 56, 125]
        );
    }
    #[test]
    fn start_requires_explicit_device_choices() {
        assert!(serde_json::from_value::<StartOptions>(json!({})).is_err());
    }
}

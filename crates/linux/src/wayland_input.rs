//! Global input through the compositor's virtual pointer and virtual
//! keyboard protocols. These move the shared seat pointer and type into
//! whatever surface has keyboard focus; there is no per-window delivery.
use crate::keymap::{self, UnicodeKeymap, XKB_OFFSET};
use actuate::{
    Delivery, KeyChord, KeyboardInput, Modifiers, MouseButton, NativeError, Point, PointerAction,
    PointerInput, Receipt, Result, TextInput, motion::MotionPlan,
};
use compositor::wayland::{Desktop, Outputs, SeatKeymap, fail};
use std::{
    collections::HashMap,
    io::Write,
    os::fd::AsFd,
    time::{Duration, Instant},
};
use wayland_client::protocol::{wl_keyboard, wl_pointer};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1, zwp_virtual_keyboard_v1,
};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1, zwlr_virtual_pointer_v1,
};

pub const BTN_LEFT: u32 = 0x110;
pub const BTN_RIGHT: u32 = 0x111;
pub const BTN_MIDDLE: u32 = 0x112;
/// Time for the compositor to deliver pointer enter/motion before a button,
/// otherwise toolkits can drop a press that precedes their hover state.
const SETTLE: Duration = Duration::from_millis(40);
const ROUTE_POINTER: &str = "linux.wayland.virtual_pointer";
const ROUTE_KEYBOARD: &str = "linux.wayland.virtual_keyboard";
const PROCESS_REASON: &str = "Wayland virtual devices deliver to the seat focus; process-directed delivery needs the Hyprland window route or X11";

#[derive(Default)]
struct State {
    outputs: Outputs,
    seat: SeatKeymap,
}
impl AsMut<Outputs> for State {
    fn as_mut(&mut self) -> &mut Outputs {
        &mut self.outputs
    }
}
impl AsMut<SeatKeymap> for State {
    fn as_mut(&mut self) -> &mut SeatKeymap {
        &mut self.seat
    }
}
compositor::delegate_registry!(State);
compositor::delegate_outputs!(State);
compositor::delegate_seat_keymap!(State);
compositor::delegate_silent!(
    State,
    [
        zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
        zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
        zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
        zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
    ]
);

pub fn button_code(button: MouseButton) -> u32 {
    match button {
        MouseButton::Left => BTN_LEFT,
        MouseButton::Right => BTN_RIGHT,
        MouseButton::Middle => BTN_MIDDLE,
    }
}

/// A virtual pointer bound to one output, with that output's logical origin and size.
type OutputPointer = (
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
    (f64, f64),
    (f64, f64),
);

/// One connection owning virtual devices. Pointers are created per output
/// so absolute motion maps onto that output's logical box exactly.
pub struct WaylandInput {
    desktop: Desktop<State>,
    state: State,
    pointer_manager: zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    keyboard_manager: Option<zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1>,
    pointers: HashMap<String, zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1>,
    keyboard: Option<zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1>,
    uploaded_keymap: Option<String>,
    started: Instant,
    last_point: Option<Point>,
}
impl WaylandInput {
    pub fn connect() -> Result<Self> {
        let mut state = State::default();
        let mut desktop = Desktop::connect_with_outputs(&mut state)?;
        let qh = desktop.qh();
        state.seat.bind(&desktop.globals, &qh)?;
        let pointer_manager = desktop.globals.bind(&qh, 1..=2, ()).map_err(|_| {
            NativeError::unsupported("Compositor lacks zwlr_virtual_pointer_manager_v1")
        })?;
        let keyboard_manager = desktop.globals.bind(&qh, 1..=1, ()).ok();
        // The keyboard is requested while the seat capabilities dispatch, so
        // its keymap needs a second round trip.
        desktop.roundtrip(&mut state)?;
        desktop.roundtrip(&mut state)?;
        Ok(Self {
            desktop,
            state,
            pointer_manager,
            keyboard_manager,
            pointers: HashMap::new(),
            keyboard: None,
            uploaded_keymap: None,
            started: Instant::now(),
            last_point: None,
        })
    }
    pub fn capabilities(&self) -> serde_json::Value {
        serde_json::json!({
            "virtual_pointer": true,
            "virtual_keyboard": self.keyboard_manager.is_some(),
            "seat_keymap": self.state.seat.keymap.is_some(),
            "outputs": self.state.outputs.outputs.iter().map(|o| serde_json::json!({"name":o.name,"bounds":o.rect(),"scale":o.scale})).collect::<Vec<_>>(),
            "delivery": "global_seat",
            "moves_shared_cursor": true,
        })
    }
    fn time(&self) -> u32 {
        self.started.elapsed().as_millis() as u32
    }
    /// Hands queued requests to the compositor without waiting for it.
    fn flush(&mut self) -> Result<()> {
        self.desktop.connection.flush().map_err(fail)
    }
    /// Flushes and waits until the compositor has processed everything sent.
    fn sync(&mut self) -> Result<()> {
        self.flush()?;
        self.desktop.roundtrip(&mut self.state)
    }
    fn pointer_for(&mut self, point: &Point) -> Result<OutputPointer> {
        let output = self.state.outputs.find(point).ok_or_else(|| {
            NativeError::new(
                "outside_displays",
                format!("Point {},{} is outside every output", point.x, point.y),
            )
        })?;
        let (name, origin, size, wl) = (
            output.name.clone(),
            (output.x as f64, output.y as f64),
            (output.width as f64, output.height as f64),
            output.wl.clone(),
        );
        let qh = self.desktop.qh();
        let pointer = self
            .pointers
            .entry(name)
            .or_insert_with(|| {
                self.pointer_manager
                    .create_virtual_pointer_with_output(None, Some(&wl), &qh, ())
            })
            .clone();
        Ok((pointer, origin, size))
    }
    fn motion(&mut self, point: &Point) -> Result<()> {
        let time = self.time();
        let (pointer, origin, size) = self.pointer_for(point)?;
        // Sub-pixel precision through scaled integer extents.
        let x = ((point.x - origin.0) * 1000.).round().max(0.) as u32;
        let y = ((point.y - origin.1) * 1000.).round().max(0.) as u32;
        pointer.motion_absolute(time, x, y, (size.0 * 1000.) as u32, (size.1 * 1000.) as u32);
        pointer.frame();
        self.last_point = Some(point.clone());
        Ok(())
    }
    fn button(&mut self, point: &Point, code: u32, pressed: bool) -> Result<()> {
        let time = self.time();
        let (pointer, _, _) = self.pointer_for(point)?;
        pointer.button(
            time,
            code,
            if pressed {
                wl_pointer::ButtonState::Pressed
            } else {
                wl_pointer::ButtonState::Released
            },
        );
        pointer.frame();
        Ok(())
    }
    fn keyboard(&mut self) -> Result<zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1> {
        if let Some(keyboard) = &self.keyboard {
            return Ok(keyboard.clone());
        }
        let manager = self.keyboard_manager.as_ref().ok_or_else(|| {
            NativeError::unsupported("Compositor lacks zwp_virtual_keyboard_manager_v1")
        })?;
        let seat = self
            .state
            .seat
            .seat
            .as_ref()
            .ok_or_else(|| fail("No wl_seat"))?;
        let keyboard = manager.create_virtual_keyboard(seat, &self.desktop.qh(), ());
        self.keyboard = Some(keyboard.clone());
        Ok(keyboard)
    }
    fn upload_keymap(&mut self, text: &str) -> Result<()> {
        if self.uploaded_keymap.as_deref() == Some(text) {
            return Ok(());
        }
        let keyboard = self.keyboard()?;
        let fd = rustix::fs::memfd_create("actuate-keymap", rustix::fs::MemfdFlags::CLOEXEC)
            .map_err(fail)?;
        let mut file = std::fs::File::from(fd);
        file.write_all(text.as_bytes()).map_err(fail)?;
        file.write_all(&[0]).map_err(fail)?;
        file.flush().map_err(fail)?;
        keyboard.keymap(
            wl_keyboard::KeymapFormat::XkbV1 as u32,
            file.as_fd(),
            text.len() as u32 + 1,
        );
        self.uploaded_keymap = Some(text.to_owned());
        self.sync()
    }
    /// Uploads the seat's real keymap so evdev codes keep the user's layout.
    fn use_seat_keymap(&mut self) -> Result<()> {
        let map = self.state.seat.keymap.clone().ok_or_else(|| {
            NativeError::unsupported(
                "The seat advertised no XKB keymap; physical key chords need one",
            )
        })?;
        self.upload_keymap(&map)
    }
    fn key(&mut self, code: u32, pressed: bool) -> Result<()> {
        let time = self.time();
        let keyboard = self.keyboard()?;
        keyboard.key(time, code, u32::from(pressed));
        Ok(())
    }
    fn modifiers(&mut self, depressed: u32) -> Result<()> {
        let keyboard = self.keyboard()?;
        keyboard.modifiers(depressed, 0, 0, 0);
        Ok(())
    }
    /// Holds portable modifiers on the virtual keyboard around a pointer action.
    fn with_modifiers(
        &mut self,
        modifiers: Modifiers,
        action: impl FnOnce(&mut Self) -> Result<()>,
    ) -> Result<()> {
        if modifiers == Modifiers::default() {
            return action(self);
        }
        self.use_seat_keymap()?;
        self.modifiers(keymap::modifier_mask(modifiers))?;
        let result = action(self);
        self.modifiers(0)?;
        self.sync()?;
        result
    }
}

impl PointerInput for WaylandInput {
    fn pointer(&mut self, delivery: Delivery, action: PointerAction) -> Result<Receipt> {
        delivery.require_global(PROCESS_REASON)?;
        action.validate()?;
        match action {
            PointerAction::Move { point } => {
                self.motion(&point)?;
                self.sync()?;
            }
            PointerAction::Click {
                point,
                button,
                count,
                modifiers,
            } => {
                let code = button_code(button);
                self.with_modifiers(modifiers, |input| {
                    input.motion(&point)?;
                    input.sync()?;
                    std::thread::sleep(SETTLE);
                    for n in 0..count {
                        if n > 0 {
                            std::thread::sleep(Duration::from_millis(60));
                        }
                        input.button(&point, code, true)?;
                        input.flush()?;
                        std::thread::sleep(Duration::from_millis(20));
                        input.button(&point, code, false)?;
                        input.sync()?;
                    }
                    Ok(())
                })?;
            }
            PointerAction::Scroll {
                vertical,
                horizontal,
                point,
            } => {
                let point = point.or_else(|| self.last_point.clone()).ok_or_else(|| {
                    NativeError::invalid_request(
                        "Scroll needs a point; no previous virtual pointer position exists",
                    )
                })?;
                self.motion(&point)?;
                let time = self.time();
                let (pointer, _, _) = self.pointer_for(&point)?;
                pointer.axis_source(wl_pointer::AxisSource::Wheel);
                // Positive vertical scrolls content up, matching the macOS route.
                if vertical != 0 {
                    pointer.axis(time, wl_pointer::Axis::VerticalScroll, -f64::from(vertical));
                }
                if horizontal != 0 {
                    pointer.axis(
                        time,
                        wl_pointer::Axis::HorizontalScroll,
                        -f64::from(horizontal),
                    );
                }
                pointer.frame();
                self.sync()?;
            }
            PointerAction::Drag {
                from,
                to,
                button,
                modifiers,
                duration_ms,
            } => {
                let plan = MotionPlan::for_drag((from.x, from.y), (to.x, to.y), duration_ms)?;
                let code = button_code(button);
                self.with_modifiers(modifiers, |input| {
                    input.motion(&from)?;
                    input.sync()?;
                    std::thread::sleep(SETTLE);
                    input.button(&from, code, true)?;
                    input.flush()?;
                    plan.walk(|sample| {
                        input.motion(&Point {
                            x: sample.position.0,
                            y: sample.position.1,
                        })?;
                        input.flush()
                    })?;
                    input.button(&to, code, false)?;
                    input.sync()
                })?;
            }
        }
        Ok(Receipt::dispatched(ROUTE_POINTER))
    }
}
impl TextInput for WaylandInput {
    fn type_text(&mut self, delivery: Delivery, text: &str) -> Result<Receipt> {
        delivery.require_global(PROCESS_REASON)?;
        if text.is_empty() {
            return Ok(Receipt::none(ROUTE_KEYBOARD));
        }
        for chunk in keymap::chunks(text) {
            let map = UnicodeKeymap::for_text(&chunk).ok_or_else(|| {
                NativeError::invalid_request("Text chunk exceeds keymap capacity")
            })?;
            self.upload_keymap(&map.text)?;
            self.modifiers(0)?;
            for c in chunk.chars() {
                let Some(code) = map.code(c) else { continue };
                self.key(code, true)?;
                self.key(code, false)?;
                self.flush()?;
            }
            self.sync()?;
        }
        // Later chords use the user's layout again.
        if self.state.seat.keymap.is_some() {
            self.use_seat_keymap()?;
        }
        Ok(Receipt::dispatched(ROUTE_KEYBOARD))
    }
}
impl KeyboardInput for WaylandInput {
    type Key = KeyChord;
    /// `key_code` is a Linux evdev code (KEY_A = 30), interpreted through the
    /// seat's current keymap.
    fn key_press(&mut self, delivery: Delivery, key: KeyChord) -> Result<Receipt> {
        delivery.require_global(PROCESS_REASON)?;
        if u32::from(key.key_code) + XKB_OFFSET > 0xFFFF {
            return Err(NativeError::invalid_request("evdev key code out of range"));
        }
        self.use_seat_keymap()?;
        self.modifiers(keymap::modifier_mask(key.modifiers))?;
        self.key(u32::from(key.key_code), true)?;
        self.flush()?;
        std::thread::sleep(Duration::from_millis(10));
        self.key(u32::from(key.key_code), false)?;
        self.modifiers(0)?;
        self.sync()?;
        Ok(Receipt::dispatched(ROUTE_KEYBOARD))
    }
}

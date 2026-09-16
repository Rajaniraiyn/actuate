//! X11 routes through XTest, window enumeration and drawable capture.
//!
//! Under Xwayland started without EI portal support, XTest events stay
//! inside the X server: X11 clients receive them while the compositor's
//! shared cursor never moves. On a native X server the same calls are
//! global input. The receipt route names which server answered.
//!
//! All public coordinates are logical layout points. Hyprland runs Xwayland
//! in a space scaled by each monitor's scale factor; `XSpace` converts at
//! this boundary so callers never see X pixels.
use crate::{keymap::CONTROL_KEYSYMS, wayland_capture::write_png};
use actuate::{
    Delivery, KeyChord, KeyboardInput, Modifiers, MouseButton, NativeError, Point, PointerAction,
    PointerInput, Receipt, Result, TextInput,
    geometry::{FrameMapping, Rect},
    motion::MotionPlan,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use x11rb::{
    connection::Connection,
    protocol::{
        xproto::{self, AtomEnum, ConnectionExt, ImageFormat, Window},
        xtest::ConnectionExt as _,
    },
    rust_connection::RustConnection,
};

const KEY_PRESS: u8 = 2;
const KEY_RELEASE: u8 = 3;
const BUTTON_PRESS: u8 = 4;
const BUTTON_RELEASE: u8 = 5;
const MOTION_NOTIFY: u8 = 6;
const XK_SHIFT_L: u32 = 0xffe1;
const XK_CONTROL_L: u32 = 0xffe3;
const XK_ALT_L: u32 = 0xffe9;
const XK_SUPER_L: u32 = 0xffeb;

/// Per-monitor scale between logical layout points and X pixels.
#[derive(Debug, Clone, Default)]
pub struct XSpace {
    monitors: Vec<(Rect, f64)>,
}
impl XSpace {
    /// Identity space: X pixels are logical points (native X sessions).
    pub fn identity() -> Self {
        Self::default()
    }
    pub fn from_monitors(monitors: Vec<(Rect, f64)>) -> Self {
        Self {
            monitors: monitors.into_iter().filter(|(_, s)| *s > 0.).collect(),
        }
    }
    fn scale_at(&self, point: &Point) -> f64 {
        self.monitors
            .iter()
            .find(|(rect, _)| rect.contains(point))
            .map(|(_, scale)| *scale)
            .unwrap_or(1.)
    }
    pub fn to_x(&self, point: Point) -> Point {
        let scale = self.scale_at(&point);
        Point {
            x: point.x * scale,
            y: point.y * scale,
        }
    }
    /// Logical rectangle for an X rectangle, using the scale at its origin.
    pub fn to_logical(&self, rect: Rect) -> Rect {
        let scale = self
            .monitors
            .iter()
            .find(|(m, s)| {
                m.contains(&Point {
                    x: rect.x / s,
                    y: rect.y / s,
                })
            })
            .map(|(_, s)| *s)
            .unwrap_or(1.);
        Rect {
            x: rect.x / scale,
            y: rect.y / scale,
            width: rect.width / scale,
            height: rect.height / scale,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct X11Window {
    pub window_id: u32,
    pub pid: Option<u32>,
    pub class: Option<String>,
    pub instance: Option<String>,
    pub title: Option<String>,
    /// Logical layout geometry.
    pub bounds: Rect,
    /// Root-relative geometry in X pixels.
    pub x_bounds: Rect,
    pub mapped: bool,
}

fn fail(message: impl ToString) -> NativeError {
    NativeError::new("x11", message)
}

struct KeyboardMapping {
    min: u8,
    per: u8,
    keysyms: Vec<u32>,
}

pub struct X11 {
    conn: RustConnection,
    screen: usize,
    root: Window,
    atoms: Atoms,
    xwayland: bool,
    space: XSpace,
    mapping: KeyboardMapping,
    remapped: Option<(u8, Vec<u32>)>,
}
struct Atoms {
    client_list: u32,
    wm_pid: u32,
    wm_name: u32,
    utf8_string: u32,
}
impl X11 {
    pub fn connect(space: XSpace) -> Result<Self> {
        let (conn, screen) =
            RustConnection::connect(None).map_err(|e| NativeError::new("x11_unavailable", e))?;
        let setup = conn.setup();
        let root = setup.roots[screen].root;
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let vendor = String::from_utf8_lossy(&setup.vendor).to_string();
        let names = [
            "_NET_CLIENT_LIST",
            "_NET_WM_PID",
            "_NET_WM_NAME",
            "UTF8_STRING",
        ];
        let cookies: Vec<_> = names
            .iter()
            .map(|name| conn.intern_atom(false, name.as_bytes()))
            .collect::<std::result::Result<_, _>>()
            .map_err(fail)?;
        let version = conn.xtest_get_version(2, 2).map_err(fail)?;
        let extensions = conn.list_extensions().map_err(fail)?;
        let mapping = conn
            .get_keyboard_mapping(min, max - min + 1)
            .map_err(fail)?;
        let atoms: Vec<u32> = cookies
            .into_iter()
            .map(|c| c.reply().map(|r| r.atom).map_err(fail))
            .collect::<Result<_>>()?;
        let atoms = Atoms {
            client_list: atoms[0],
            wm_pid: atoms[1],
            wm_name: atoms[2],
            utf8_string: atoms[3],
        };
        version
            .reply()
            .map_err(|_| NativeError::unsupported("X server lacks the XTEST extension"))?;
        let xwayland = extensions
            .reply()
            .map_err(fail)?
            .names
            .iter()
            .any(|n| n.name == b"XWAYLAND")
            || vendor.contains("Xwayland");
        let mapping = mapping.reply().map_err(fail)?;
        Ok(Self {
            conn,
            screen,
            root,
            atoms,
            xwayland,
            space,
            mapping: KeyboardMapping {
                min,
                per: mapping.keysyms_per_keycode,
                keysyms: mapping.keysyms,
            },
            remapped: None,
        })
    }
    pub fn capabilities(&self) -> Value {
        let screen = &self.conn.setup().roots[self.screen];
        json!({
            "xwayland": self.xwayland,
            "xtest": true,
            "delivery": if self.xwayland { "x_server_local" } else { "global" },
            "moves_shared_cursor": !self.xwayland,
            "screen": {"width": screen.width_in_pixels, "height": screen.height_in_pixels},
            "scaled_monitors": self.space.monitors.len(),
        })
    }
    fn property_string(&self, window: Window, property: u32, kind: u32) -> Option<String> {
        let reply = self
            .conn
            .get_property(false, window, property, kind, 0, 4096)
            .ok()?
            .reply()
            .ok()?;
        if reply.value.is_empty() {
            return None;
        }
        Some(
            String::from_utf8_lossy(&reply.value)
                .trim_end_matches('\0')
                .to_owned(),
        )
    }
    fn window_record(&self, window: Window) -> Result<X11Window> {
        let geometry = self.conn.get_geometry(window).map_err(fail)?;
        let translated = self
            .conn
            .translate_coordinates(window, self.root, 0, 0)
            .map_err(fail)?;
        let attributes = self.conn.get_window_attributes(window).map_err(fail)?;
        let pid = self
            .conn
            .get_property(false, window, self.atoms.wm_pid, AtomEnum::CARDINAL, 0, 1)
            .map_err(fail)?;
        let geometry = geometry.reply().map_err(fail)?;
        let translated = translated.reply().map_err(fail)?;
        let attributes = attributes.reply().map_err(fail)?;
        let pid = pid
            .reply()
            .ok()
            .and_then(|r| r.value32().and_then(|mut v| v.next()));
        let class =
            self.property_string(window, AtomEnum::WM_CLASS.into(), AtomEnum::STRING.into());
        let (instance, class) = match class {
            Some(raw) => {
                let mut parts = raw.split('\0');
                (
                    parts.next().map(str::to_owned),
                    parts.next().map(str::to_owned),
                )
            }
            None => (None, None),
        };
        let title = self
            .property_string(window, self.atoms.wm_name, self.atoms.utf8_string)
            .or_else(|| {
                self.property_string(window, AtomEnum::WM_NAME.into(), AtomEnum::STRING.into())
            });
        let x_bounds = Rect {
            x: f64::from(translated.dst_x),
            y: f64::from(translated.dst_y),
            width: f64::from(geometry.width),
            height: f64::from(geometry.height),
        };
        Ok(X11Window {
            window_id: window,
            pid,
            class,
            instance,
            title,
            bounds: self.space.to_logical(x_bounds),
            x_bounds,
            mapped: attributes.map_state == xproto::MapState::VIEWABLE,
        })
    }
    /// Managed toplevels from the window manager's client list.
    pub fn windows(&self) -> Result<Vec<X11Window>> {
        let reply = self
            .conn
            .get_property(
                false,
                self.root,
                self.atoms.client_list,
                AtomEnum::WINDOW,
                0,
                4096,
            )
            .map_err(fail)?
            .reply()
            .map_err(fail)?;
        let ids: Vec<Window> = reply.value32().map(|v| v.collect()).unwrap_or_default();
        Ok(ids
            .into_iter()
            .filter_map(|w| self.window_record(w).ok())
            .collect())
    }
    /// One toplevel by X window id.
    pub fn window(&self, window_id: u32) -> Result<X11Window> {
        self.window_record(window_id)
    }
    /// The mapped toplevel of a pid, narrowed by title when given.
    pub fn window_for(&self, pid: u32, title: Option<&str>) -> Result<X11Window> {
        self.windows()?
            .into_iter()
            .find(|w| {
                w.mapped
                    && w.pid == Some(pid)
                    && title.is_none_or(|t| w.title.as_deref() == Some(t))
            })
            .ok_or_else(|| {
                NativeError::new(
                    "no_window",
                    format!(
                        "No mapped X11 window belongs to pid {pid}{}",
                        title.map(|t| format!(" titled {t:?}")).unwrap_or_default()
                    ),
                )
            })
    }
    fn flush(&self) -> Result<()> {
        self.conn.flush().map_err(fail)
    }
    fn fake(&self, kind: u8, detail: u8, x: i16, y: i16) -> Result<()> {
        self.conn
            .xtest_fake_input(kind, detail, x11rb::CURRENT_TIME, self.root, x, y, 0)
            .map_err(fail)?;
        Ok(())
    }
    fn motion(&self, point: &Point) -> Result<()> {
        let point = self.space.to_x(point.clone());
        if point.x.abs() >= 32767. || point.y.abs() >= 32767. {
            return Err(NativeError::invalid_request(
                "X coordinates must fit 16 bits",
            ));
        }
        self.fake(
            MOTION_NOTIFY,
            0,
            point.x.round() as i16,
            point.y.round() as i16,
        )
    }
    fn button(&self, button: u8, pressed: bool) -> Result<()> {
        self.fake(
            if pressed {
                BUTTON_PRESS
            } else {
                BUTTON_RELEASE
            },
            button,
            0,
            0,
        )
    }
    /// Key code for a keysym in the cached mapping, remapping a spare
    /// key code when the symbol is absent. Remaps are restored on drop.
    /// Returns whether a remap was issued, so callers can let clients
    /// observe the new mapping before the key arrives.
    fn keycode_for(&mut self, keysym: u32) -> Result<(u8, bool)> {
        let per = usize::from(self.mapping.per);
        let mut spare = None;
        for (i, chunk) in self.mapping.keysyms.chunks(per).enumerate() {
            let code = self.mapping.min + i as u8;
            if chunk.first().copied() == Some(keysym) {
                return Ok((code, false));
            }
            if spare.is_none() && chunk.iter().all(|k| *k == 0) && code > self.mapping.min + 8 {
                spare = Some((code, chunk.to_vec()));
            }
        }
        let (code, original) =
            spare.ok_or_else(|| NativeError::unsupported("No spare X key code for remapping"))?;
        let mut symbols = vec![0u32; per];
        symbols[0] = keysym;
        self.conn
            .change_keyboard_mapping(1, code, self.mapping.per, &symbols)
            .map_err(fail)?;
        self.flush()?;
        let offset = usize::from(code - self.mapping.min) * per;
        self.mapping.keysyms[offset..offset + per].copy_from_slice(&symbols);
        if self.remapped.is_none() {
            self.remapped = Some((code, original));
        }
        Ok((code, true))
    }
    fn restore_mapping(&mut self) {
        if let Some((code, original)) = self.remapped.take() {
            let _ = self
                .conn
                .change_keyboard_mapping(1, code, original.len() as u8, &original);
            let _ = self.flush();
        }
    }
    fn modifier_codes(&mut self, modifiers: Modifiers) -> Result<Vec<u8>> {
        let mut codes = vec![];
        for (flag, keysym) in [
            (modifiers.shift, XK_SHIFT_L),
            (modifiers.control, XK_CONTROL_L),
            (modifiers.alt, XK_ALT_L),
            (modifiers.meta, XK_SUPER_L),
        ] {
            if flag {
                codes.push(self.keycode_for(keysym)?.0);
            }
        }
        Ok(codes)
    }
    /// Pixels of a viewable X window drawable. Under rootless Xwayland the
    /// root has no content; only windows can be captured.
    pub fn capture_window(
        &self,
        window_id: u32,
        path: &Path,
    ) -> Result<crate::wayland_capture::Frame> {
        let record = self.window_record(window_id)?;
        if !record.mapped {
            return Err(NativeError::new("no_window", "Window is not viewable"));
        }
        let (w, h) = (record.x_bounds.width as u16, record.x_bounds.height as u16);
        let image = self
            .conn
            .get_image(ImageFormat::Z_PIXMAP, window_id, 0, 0, w, h, !0)
            .map_err(fail)?
            .reply()
            .map_err(|e| NativeError::new("capture_failed", e))?;
        if image.depth != 24 && image.depth != 32 {
            return Err(NativeError::new(
                "capture_failed",
                format!("Unsupported depth {}", image.depth),
            ));
        }
        let rgba: Vec<u8> = image
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|px| [px[2], px[1], px[0], 255])
            .collect();
        let path = actuate::image::absolute_output(path)?;
        write_png(&path, u32::from(w), u32::from(h), &rgba)?;
        let revision = serde_json::to_string(&record).map_err(fail)?;
        Ok(crate::wayland_capture::Frame {
            source: crate::wayland_capture::CaptureSource::Region {
                x: record.bounds.x,
                y: record.bounds.y,
                width: record.bounds.width,
                height: record.bounds.height,
            },
            path,
            route: "linux.x11.get_image".into(),
            mapping: FrameMapping {
                source_bounds: record.bounds,
                pixel_width: u32::from(w),
                pixel_height: u32::from(h),
                geometry_revision: revision,
            },
            toplevel: None,
        })
    }
    pub fn window_revision(&self, window_id: u32) -> Result<String> {
        serde_json::to_string(&self.window_record(window_id)?).map_err(fail)
    }
    fn route(&self, device: &str) -> Receipt {
        Receipt::dispatched(format!(
            "linux.x11.xtest.{device}{}",
            if self.xwayland { ".xwayland_local" } else { "" }
        ))
    }
    fn send_synthetic_button(
        &self,
        window: Window,
        point: &Point,
        button: u8,
        pressed: bool,
    ) -> Result<()> {
        let translated = self
            .conn
            .translate_coordinates(
                self.root,
                window,
                point.x.round() as i16,
                point.y.round() as i16,
            )
            .map_err(fail)?
            .reply()
            .map_err(fail)?;
        let event = xproto::ButtonPressEvent {
            response_type: if pressed {
                BUTTON_PRESS
            } else {
                BUTTON_RELEASE
            },
            detail: button,
            sequence: 0,
            time: x11rb::CURRENT_TIME,
            root: self.root,
            event: window,
            child: x11rb::NONE,
            root_x: point.x.round() as i16,
            root_y: point.y.round() as i16,
            event_x: translated.dst_x,
            event_y: translated.dst_y,
            state: 0u16.into(),
            same_screen: true,
        };
        let mask = if pressed {
            xproto::EventMask::BUTTON_PRESS
        } else {
            xproto::EventMask::BUTTON_RELEASE
        };
        self.conn
            .send_event(true, window, mask, event)
            .map_err(fail)?;
        Ok(())
    }
    /// Synthetic `send_event` delivery to one window. Many toolkits ignore
    /// synthetic events; the receipt reports delivery, not consumption.
    pub fn click_window(
        &self,
        window: Window,
        point: &Point,
        button: MouseButton,
        count: u8,
    ) -> Result<Receipt> {
        let action = PointerAction::Click {
            point: point.clone(),
            button,
            count,
            modifiers: Modifiers::default(),
        };
        action.validate()?;
        let point = self.space.to_x(point.clone());
        let detail = button_number(button);
        for _ in 0..count {
            self.send_synthetic_button(window, &point, detail, true)?;
            self.send_synthetic_button(window, &point, detail, false)?;
        }
        self.flush()?;
        Ok(Receipt::dispatched("linux.x11.send_event"))
    }
}
impl Drop for X11 {
    fn drop(&mut self) {
        self.restore_mapping();
    }
}

fn button_number(button: MouseButton) -> u8 {
    match button {
        MouseButton::Left => 1,
        MouseButton::Middle => 2,
        MouseButton::Right => 3,
    }
}
const PROCESS_REASON: &str =
    "XTest is server-wide; process delivery uses the click_window synthetic route";
fn keysym_for(c: char) -> u32 {
    CONTROL_KEYSYMS
        .iter()
        .find(|(ch, _, _)| *ch == c)
        .map(|(_, _, keysym)| *keysym)
        .unwrap_or(if (c as u32) < 0x100 {
            c as u32
        } else {
            0x0100_0000 | c as u32
        })
}

impl PointerInput for X11 {
    fn pointer(&mut self, delivery: Delivery, action: PointerAction) -> Result<Receipt> {
        delivery.require_global(PROCESS_REASON)?;
        action.validate()?;
        match action {
            PointerAction::Move { point } => self.motion(&point)?,
            PointerAction::Click {
                point,
                button,
                count,
                modifiers,
            } => {
                let held = self.modifier_codes(modifiers)?;
                self.motion(&point)?;
                for code in &held {
                    self.fake(KEY_PRESS, *code, 0, 0)?;
                }
                for n in 0..count {
                    if n > 0 {
                        self.flush()?;
                        std::thread::sleep(Duration::from_millis(60));
                    }
                    self.button(button_number(button), true)?;
                    self.button(button_number(button), false)?;
                }
                for code in held.iter().rev() {
                    self.fake(KEY_RELEASE, *code, 0, 0)?;
                }
            }
            PointerAction::Scroll {
                vertical,
                horizontal,
                point,
            } => {
                if let Some(point) = &point {
                    self.motion(point)?;
                }
                // Positive vertical scrolls content up (button 4), matching macOS.
                for (steps, up, down) in [(vertical, 4, 5), (horizontal, 6, 7)] {
                    let button = if steps >= 0 { up } else { down };
                    let clicks = (steps.unsigned_abs() / 15).clamp(u32::from(steps != 0), 100);
                    for _ in 0..clicks {
                        self.button(button, true)?;
                        self.button(button, false)?;
                    }
                }
            }
            PointerAction::Drag {
                from,
                to,
                button,
                modifiers,
                duration_ms,
            } => {
                let plan = MotionPlan::for_drag((from.x, from.y), (to.x, to.y), duration_ms)?;
                let held = self.modifier_codes(modifiers)?;
                for code in &held {
                    self.fake(KEY_PRESS, *code, 0, 0)?;
                }
                self.motion(&from)?;
                self.button(button_number(button), true)?;
                self.flush()?;
                plan.walk(|sample| {
                    self.motion(&Point {
                        x: sample.position.0,
                        y: sample.position.1,
                    })?;
                    self.flush()
                })?;
                self.button(button_number(button), false)?;
                for code in held.iter().rev() {
                    self.fake(KEY_RELEASE, *code, 0, 0)?;
                }
            }
        }
        self.flush()?;
        Ok(self.route("pointer"))
    }
}
impl TextInput for X11 {
    fn type_text(&mut self, delivery: Delivery, text: &str) -> Result<Receipt> {
        delivery.require_global(PROCESS_REASON)?;
        for c in text.chars() {
            if c == '\r' {
                continue;
            }
            let (code, remapped) = self.keycode_for(keysym_for(c))?;
            if remapped {
                // Let clients that read the mapping notice the change first.
                std::thread::sleep(Duration::from_millis(5));
            }
            self.fake(KEY_PRESS, code, 0, 0)?;
            self.fake(KEY_RELEASE, code, 0, 0)?;
            self.flush()?;
        }
        Ok(self.route("keyboard"))
    }
}
impl KeyboardInput for X11 {
    type Key = KeyChord;
    /// `key_code` is a Linux evdev code; X adds its offset of 8.
    fn key_press(&mut self, delivery: Delivery, key: KeyChord) -> Result<Receipt> {
        delivery.require_global(PROCESS_REASON)?;
        let code = u8::try_from(key.key_code + 8)
            .map_err(|_| NativeError::invalid_request("X key codes are limited to 255"))?;
        let held = self.modifier_codes(key.modifiers)?;
        for m in &held {
            self.fake(KEY_PRESS, *m, 0, 0)?;
        }
        self.fake(KEY_PRESS, code, 0, 0)?;
        self.fake(KEY_RELEASE, code, 0, 0)?;
        for m in held.iter().rev() {
            self.fake(KEY_RELEASE, *m, 0, 0)?;
        }
        self.flush()?;
        Ok(self.route("keyboard"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keysyms_follow_x_conventions() {
        assert_eq!(keysym_for('a'), 0x61);
        assert_eq!(keysym_for('\n'), 0xff0d);
        assert_eq!(keysym_for('🦀'), 0x0100_0000 | 0x1F980);
        assert_eq!(button_number(MouseButton::Right), 3);
    }
    #[test]
    fn x_space_scales_per_monitor() {
        let space = XSpace::from_monitors(vec![(
            Rect {
                x: 0.,
                y: 0.,
                width: 1536.,
                height: 960.,
            },
            1.25,
        )]);
        let p = space.to_x(Point { x: 100., y: 40. });
        assert_eq!((p.x, p.y), (125., 50.));
        let r = space.to_logical(Rect {
            x: 1441.,
            y: 38.,
            width: 473.,
            height: 1155.,
        });
        assert!((r.x - 1152.8).abs() < 0.01 && (r.width - 378.4).abs() < 0.01);
        assert_eq!(XSpace::identity().to_x(Point { x: 3., y: 4. }).x, 3.);
    }
}

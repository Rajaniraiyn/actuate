//! X11 root coordinates are server pixels. They are not Wayland logical coordinates.
use crate::{Environment, Frame, SessionKind, error, unsupported};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    fs::File,
    path::Path,
    time::{Duration, Instant},
};
use unimation::{
    Capture, Delivery, Discover, Effect, KeyChord, KeyboardInput, Modifiers, MouseButton, Point,
    PointerAction, PointerInput, Receipt, Result,
    geometry::{FrameMapping, Rect},
    motion::{MotionPlan, MotionStyle},
};
use x11rb::{
    CURRENT_TIME,
    connection::Connection,
    protocol::{randr::ConnectionExt as _, xproto::*, xtest::ConnectionExt as _},
    rust_connection::RustConnection,
};

pub struct X11 {
    connection: RustConnection,
    screen: usize,
}
#[derive(Debug, Serialize)]
pub struct WindowInfo {
    pub id: u32,
    pub pid: Option<u32>,
    pub title: String,
    pub bounds: Rect,
    pub mapped: bool,
    pub desktop: Option<u32>,
}
impl X11 {
    /// Automatic routing never mistakes XWayland for the whole Wayland desktop.
    /// An embedder may explicitly connect to a particular X server with `connect`.
    pub fn auto() -> Result<Self> {
        if Environment::detect().kind == SessionKind::Wayland {
            return Err(unsupported(
                "Wayland capture/input needs a portal or compositor provider; automatic XWayland fallback is disabled",
            ));
        }
        Self::connect(None)
    }
    pub fn connect(display: Option<&str>) -> Result<Self> {
        let (connection, screen) = x11rb::connect(display).map_err(|e| error("x11_connect", e))?;
        Ok(Self { connection, screen })
    }
    fn root(&self) -> u32 {
        self.connection.setup().roots[self.screen].root
    }
    fn atom(&self, name: &str) -> Result<u32> {
        Ok(self
            .connection
            .intern_atom(false, name.as_bytes())
            .map_err(read_error)?
            .reply()
            .map_err(read_error)?
            .atom)
    }
    fn property(&self, window: u32, name: &str) -> Result<GetPropertyReply> {
        self.connection
            .get_property(false, window, self.atom(name)?, AtomEnum::ANY, 0, 1_048_576)
            .map_err(read_error)?
            .reply()
            .map_err(read_error)
    }
    fn cardinal(&self, window: u32, name: &str) -> Option<u32> {
        self.property(window, name).ok()?.value32()?.next()
    }
    fn bounds(&self, window: u32) -> Result<Rect> {
        let g = self
            .connection
            .get_geometry(window)
            .map_err(read_error)?
            .reply()
            .map_err(read_error)?;
        let p = self
            .connection
            .translate_coordinates(window, self.root(), 0, 0)
            .map_err(read_error)?
            .reply()
            .map_err(read_error)?;
        Ok(Rect {
            x: p.dst_x.into(),
            y: p.dst_y.into(),
            width: g.width.into(),
            height: g.height.into(),
        })
    }
    pub fn windows(&self) -> Result<Value> {
        let ids = self.property(self.root(), "_NET_CLIENT_LIST_STACKING")?;
        let (ids, source): (Vec<u32>, _) = if let Some(v) = ids.value32() {
            (v.collect(), "ewmh_stacking_bottom_to_top")
        } else {
            (
                self.connection
                    .query_tree(self.root())
                    .map_err(read_error)?
                    .reply()
                    .map_err(read_error)?
                    .children,
                "root_children",
            )
        };
        let mut windows = Vec::new();
        let mut issues = Vec::new();
        for id in ids {
            let item = (|| -> Result<WindowInfo> {
                let attrs = self
                    .connection
                    .get_window_attributes(id)
                    .map_err(read_error)?
                    .reply()
                    .map_err(read_error)?;
                let title = self
                    .property(id, "_NET_WM_NAME")
                    .ok()
                    .filter(|v| !v.value.is_empty())
                    .or_else(|| self.property(id, "WM_NAME").ok())
                    .map(|v| String::from_utf8_lossy(&v.value).into_owned())
                    .unwrap_or_default();
                Ok(WindowInfo {
                    id,
                    pid: self.cardinal(id, "_NET_WM_PID"),
                    title,
                    bounds: self.bounds(id)?,
                    mapped: attrs.map_state == MapState::VIEWABLE,
                    desktop: self.cardinal(id, "_NET_WM_DESKTOP"),
                })
            })();
            match item {
                Ok(w) => windows.push(w),
                Err(e) => issues.push(json!({"window":id,"error":e})),
            }
        }
        Ok(
            json!({"windows":windows,"issues":issues,"source":source,"reported_active_pid":self.cardinal(self.root(),"_NET_ACTIVE_WINDOW").and_then(|w|self.cardinal(w,"_NET_WM_PID")),"current_desktop":self.cardinal(self.root(), "_NET_CURRENT_DESKTOP"),"coordinate_space":"x11_root_pixels"}),
        )
    }
    pub fn displays(&self) -> Result<Value> {
        let monitors = self
            .connection
            .randr_get_monitors(self.root(), true)
            .map_err(read_error)?
            .reply()
            .map_err(read_error)?;
        Ok(
            json!({"coordinate_space":"x11_root_pixels","root":self.root(),"displays":monitors.monitors.iter().map(|m| json!({"id":m.name,"primary":m.primary,"bounds":{"x":m.x,"y":m.y,"width":m.width,"height":m.height},"physical_mm":{"width":m.width_in_millimeters,"height":m.height_in_millimeters},"outputs":m.outputs})).collect::<Vec<_>>() }),
        )
    }
    fn point(&self, p: &Point) -> Result<(i16, i16)> {
        let bounds = self.bounds(self.root())?;
        validate_point(p, bounds.width, bounds.height)
    }
    fn fake(&self, kind: u8, detail: u8, point: (i16, i16)) -> Result<()> {
        self.connection
            .xtest_fake_input(kind, detail, CURRENT_TIME, self.root(), point.0, point.1, 0)
            .map_err(input_error)?
            .check()
            .map_err(input_error)
    }
    fn modifier_codes(&self, m: Modifiers) -> Result<Vec<u8>> {
        let requested = [
            (m.shift, 0xffe1),
            (m.control, 0xffe3),
            (m.alt, 0xffe9),
            (m.meta, 0xffeb),
        ];
        let setup = self.connection.setup();
        let mapping = self
            .connection
            .get_keyboard_mapping(setup.min_keycode, setup.max_keycode - setup.min_keycode + 1)
            .map_err(read_error)?
            .reply()
            .map_err(read_error)?;
        if mapping.keysyms_per_keycode == 0 {
            return Err(error(
                "invalid_keymap",
                "X server returned an empty keyboard mapping",
            ));
        }
        let mut keys = Vec::new();
        for (_, sym) in requested.into_iter().filter(|(enabled, _)| *enabled) {
            let index = mapping
                .keysyms
                .chunks(mapping.keysyms_per_keycode.into())
                .position(|v| v.contains(&sym))
                .ok_or_else(|| {
                    unsupported(format!("Modifier keysym {sym:#x} has no native keycode"))
                })?;
            keys.push(setup.min_keycode + index as u8);
        }
        self.ensure_idle_pointer()?;
        let held = self
            .connection
            .query_keymap()
            .map_err(read_error)?
            .reply()
            .map_err(read_error)?;
        if keys
            .iter()
            .any(|k| held.keys[*k as usize / 8] & (1 << (*k % 8)) != 0)
        {
            return Err(error(
                "input_busy",
                "A requested modifier is already held; no input was sent",
            ));
        }
        Ok(keys)
    }
    fn with_modifiers(&self, keys: &[u8], action: impl FnOnce() -> Result<()>) -> Result<()> {
        let mut pressed = Vec::new();
        let result = (|| {
            for &key in keys {
                pressed.push(key);
                self.fake(KEY_PRESS_EVENT, key, (0, 0))?;
            }
            action()
        })();
        let mut release_error = None;
        for &key in pressed.iter().rev() {
            if let Err(e) = self.fake(KEY_RELEASE_EVENT, key, (0, 0)) {
                release_error = Some(e);
                let _ = self.fake(KEY_RELEASE_EVENT, key, (0, 0));
            }
        }
        result.and(release_error.map_or(Ok(()), Err))
    }
    fn ensure_idle_pointer(&self) -> Result<()> {
        let pointer = self
            .connection
            .query_pointer(self.root())
            .map_err(read_error)?
            .reply()
            .map_err(read_error)?;
        let keys = self
            .connection
            .query_keymap()
            .map_err(read_error)?
            .reply()
            .map_err(read_error)?;
        if u16::from(pointer.mask) & 0x1f00 != 0 || keys.keys.iter().any(|v| *v != 0) {
            return Err(error(
                "input_busy",
                "A mouse button or keyboard key is already held; no input was sent",
            ));
        }
        Ok(())
    }
    /// Positive vertical detents scroll down; positive horizontal detents scroll right.
    pub fn wheel(&self, vertical: i32, horizontal: i32, point: Option<Point>) -> Result<Receipt> {
        if vertical.unsigned_abs() > 1000 || horizontal.unsigned_abs() > 1000 {
            return Err(error(
                "invalid_scroll",
                "Wheel is limited to 1000 detents per axis",
            ));
        }
        let point = point.as_ref().map(|p| self.point(p)).transpose()?;
        self.ensure_idle_pointer()?;
        if let Some(p) = point {
            self.fake(MOTION_NOTIFY_EVENT, 0, p)?;
        }
        for (amount, positive, negative) in [(vertical, 5, 4), (horizontal, 7, 6)] {
            for _ in 0..amount.unsigned_abs() {
                self.pair(
                    BUTTON_PRESS_EVENT,
                    BUTTON_RELEASE_EVENT,
                    if amount > 0 { positive } else { negative },
                    (0, 0),
                )?;
            }
        }
        Ok(Receipt {
            effect: Effect::Dispatched,
            route: "x11_xtest_wheel_detents".into(),
        })
    }
    fn pair(&self, press: u8, release: u8, detail: u8, point: (i16, i16)) -> Result<()> {
        let pressed = self.fake(press, detail, point);
        // A connection failure may happen after server acceptance. Always attempt
        // release, even when the press acknowledgement was lost.
        let released = self.fake(release, detail, point);
        if released.is_err() {
            let _ = self.fake(release, detail, point);
        }
        pressed.and(released)
    }
    pub fn capture_root(&self, path: &Path) -> Result<Frame> {
        let root = self.root();
        let g = self
            .connection
            .get_geometry(root)
            .map_err(read_error)?
            .reply()
            .map_err(read_error)?;
        capture_budget(g.width as usize, g.height as usize)?;
        let image = self
            .connection
            .get_image(
                ImageFormat::Z_PIXMAP,
                root,
                0,
                0,
                g.width,
                g.height,
                u32::MAX,
            )
            .map_err(read_error)?
            .reply()
            .map_err(read_error)?;
        let screen = &self.connection.setup().roots[self.screen];
        let visual = screen
            .allowed_depths
            .iter()
            .flat_map(|d| &d.visuals)
            .find(|v| v.visual_id == image.visual)
            .ok_or_else(|| unsupported("Capture visual is not described by the X server"))?;
        if visual.class != VisualClass::TRUE_COLOR {
            return Err(unsupported("Capture currently requires a TrueColor visual"));
        }
        let format = self
            .connection
            .setup()
            .pixmap_formats
            .iter()
            .find(|f| f.depth == image.depth)
            .ok_or_else(|| unsupported("No pixmap format for capture depth"))?;
        let rgb = decode_rgb(
            &image.data,
            g.width as usize,
            g.height as usize,
            format.bits_per_pixel,
            format.scanline_pad,
            self.connection.setup().image_byte_order == ImageOrder::LSB_FIRST,
            [visual.red_mask, visual.green_mask, visual.blue_mask],
        )?;
        let file = File::create(path).map_err(|e| error("capture_write", e))?;
        let mut encoder = png::Encoder::new(file, g.width.into(), g.height.into());
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .map_err(|e| error("capture_encode", e))?
            .write_image_data(&rgb)
            .map_err(|e| error("capture_encode", e))?;
        Ok(Frame {
            path: path.to_string_lossy().into_owned(),
            mapping: FrameMapping {
                source_bounds: Rect {
                    x: 0.,
                    y: 0.,
                    width: g.width.into(),
                    height: g.height.into(),
                },
                pixel_width: g.width.into(),
                pixel_height: g.height.into(),
                geometry_revision: format!("x11:{root}:{}:{}", g.width, g.height),
            },
            coordinate_space: "x11_root_pixels",
            cursor_included: false,
        })
    }
}
impl Discover for X11 {
    fn discover(&mut self) -> Result<Value> {
        self.windows()
    }
}
impl Capture for X11 {
    type Request = std::path::PathBuf;
    type Frame = Frame;
    fn capture(&mut self, path: Self::Request) -> Result<Frame> {
        self.capture_root(&path)
    }
}
impl PointerInput for X11 {
    fn pointer(&mut self, delivery: Delivery, action: PointerAction) -> Result<Receipt> {
        if !matches!(delivery, Delivery::Global {}) {
            return Err(unsupported(
                "XTEST is global input; process delivery is not supported",
            ));
        }
        self.connection
            .xtest_get_version(2, 2)
            .map_err(read_error)?
            .reply()
            .map_err(read_error)?;
        self.ensure_idle_pointer()?;
        match action {
            PointerAction::Move { point } => {
                self.fake(MOTION_NOTIFY_EVENT, 0, self.point(&point)?)?
            }
            PointerAction::Click {
                point,
                button,
                count,
                modifiers,
            } => {
                if count == 0 || count > 3 {
                    return Err(error("invalid_click_count", "Click count must be 1..=3"));
                }
                let point = self.point(&point)?;
                let keys = self.modifier_codes(modifiers)?;
                self.with_modifiers(&keys, || {
                    self.fake(MOTION_NOTIFY_EVENT, 0, point)?;
                    for i in 0..count {
                        self.pair(
                            BUTTON_PRESS_EVENT,
                            BUTTON_RELEASE_EVENT,
                            button_id(button),
                            point,
                        )?;
                        if i + 1 < count {
                            std::thread::sleep(Duration::from_millis(80));
                        }
                    }
                    Ok(())
                })?;
            }
            PointerAction::Scroll { .. } => {
                return Err(unsupported(
                    "XTEST provides wheel detents, not pixel scrolling; use the explicit x11_wheel extension",
                ));
            }
            PointerAction::Drag {
                from,
                to,
                button,
                modifiers,
                duration_ms,
            } => {
                if duration_ms > 30_000 {
                    return Err(error(
                        "invalid_duration",
                        "Drag duration must be at most 30000 ms",
                    ));
                }
                let from = self.point(&from)?;
                let to = self.point(&to)?;
                let keys = self.modifier_codes(modifiers)?;
                let plan = MotionPlan::new(
                    (from.0.into(), from.1.into()),
                    (to.0.into(), to.1.into()),
                    Duration::from_millis(duration_ms),
                    ((duration_ms / 16).max(1)) as usize,
                    MotionStyle::Straight,
                )?;
                self.with_modifiers(&keys, || {
                    self.fake(MOTION_NOTIFY_EVENT, 0, from)?;
                    if let Err(e) = self.fake(BUTTON_PRESS_EVENT, button_id(button), from) {
                        let _ = self.fake(BUTTON_RELEASE_EVENT, button_id(button), from);
                        return Err(e);
                    }
                    let started = Instant::now();
                    let mut last = from;
                    let result = (|| {
                        for sample in plan.samples() {
                            std::thread::sleep(sample.at.saturating_sub(started.elapsed()));
                            let p = (
                                sample.position.0.round() as i16,
                                sample.position.1.round() as i16,
                            );
                            self.fake(MOTION_NOTIFY_EVENT, 0, p)?;
                            last = p;
                        }
                        Ok(())
                    })();
                    let release = self.fake(BUTTON_RELEASE_EVENT, button_id(button), last);
                    if release.is_err() {
                        let _ = self.fake(BUTTON_RELEASE_EVENT, button_id(button), last);
                    }
                    result.and(release)
                })?;
            }
        }
        Ok(Receipt {
            effect: Effect::Dispatched,
            route: "x11_xtest_global".into(),
        })
    }
}
impl KeyboardInput for X11 {
    type Key = KeyChord;
    fn key_press(&mut self, delivery: Delivery, chord: KeyChord) -> Result<Receipt> {
        if !matches!(delivery, Delivery::Global {}) {
            return Err(unsupported("XTEST keyboard input is global"));
        }
        let key = u8::try_from(chord.key_code)
            .map_err(|_| error("invalid_key", "X11 keycode must fit u8"))?;
        let setup = self.connection.setup();
        if key < setup.min_keycode || key > setup.max_keycode {
            return Err(error("invalid_key", "Keycode is outside the server keymap"));
        }
        let keys = self.modifier_codes(chord.modifiers)?;
        if keys.contains(&key) {
            return Err(error(
                "invalid_key",
                "Main key cannot also be a requested modifier",
            ));
        }
        let held = self
            .connection
            .query_keymap()
            .map_err(read_error)?
            .reply()
            .map_err(read_error)?;
        if held.keys[key as usize / 8] & (1 << (key % 8)) != 0 {
            return Err(error("input_busy", "Requested key is already held"));
        }
        self.with_modifiers(&keys, || {
            self.pair(KEY_PRESS_EVENT, KEY_RELEASE_EVENT, key, (0, 0))
        })?;
        Ok(Receipt {
            effect: Effect::Dispatched,
            route: "x11_xtest_global".into(),
        })
    }
}
fn button_id(button: MouseButton) -> u8 {
    match button {
        MouseButton::Left => 1,
        MouseButton::Middle => 2,
        MouseButton::Right => 3,
    }
}
fn read_error(e: impl ToString) -> unimation::NativeError {
    error("x11_request", e)
}
fn input_error(e: impl ToString) -> unimation::NativeError {
    unimation::NativeError {
        code: "x11_input".into(),
        message: e.to_string(),
        effect: Effect::Unknown,
    }
}
fn validate_point(p: &Point, width: f64, height: f64) -> Result<(i16, i16)> {
    if !p.x.is_finite()
        || !p.y.is_finite()
        || p.x < 0.
        || p.y < 0.
        || p.x.round() >= width
        || p.y.round() >= height
        || p.x.round() > i16::MAX as f64
        || p.y.round() > i16::MAX as f64
    {
        return Err(error(
            "invalid_coordinate",
            "Point is outside the X11 root or protocol coordinate range",
        ));
    }
    Ok((p.x.round() as i16, p.y.round() as i16))
}
fn decode_rgb(
    data: &[u8],
    width: usize,
    height: usize,
    bits: u8,
    pad: u8,
    little: bool,
    masks: [u32; 3],
) -> Result<Vec<u8>> {
    capture_budget(width, height)?;
    if ![16, 24, 32].contains(&bits) || ![8, 16, 32].contains(&pad) || masks.contains(&0) {
        return Err(unsupported("Unsupported X11 pixel layout"));
    }
    let stride = (width * bits as usize).div_ceil(pad as usize) * (pad as usize / 8);
    let bytes = bits as usize / 8;
    if data.len() < stride * height {
        return Err(error(
            "invalid_capture",
            "X11 image is shorter than its declared layout",
        ));
    }
    let mut rgb = Vec::with_capacity(width * height * 3);
    for row in data.chunks(stride).take(height) {
        for pixel in row[..width * bytes].chunks(bytes) {
            let mut value = 0u32;
            for (i, b) in pixel.iter().enumerate() {
                value |= (*b as u32) << if little { i * 8 } else { (bytes - i - 1) * 8 };
            }
            for mask in masks {
                let shift = mask.trailing_zeros();
                let max = mask >> shift;
                rgb.push(
                    ((((value & mask) >> shift) as u64 * 255 + max as u64 / 2) / max as u64) as u8,
                );
            }
        }
    }
    Ok(rgb)
}
fn capture_budget(width: usize, height: usize) -> Result<()> {
    // Bound both the incoming four-byte layout and outgoing RGB allocation.
    if width == 0
        || height == 0
        || width
            .checked_mul(height)
            .and_then(|v| v.checked_mul(7))
            .is_none_or(|bytes| bytes > 256 * 1024 * 1024)
    {
        return Err(error(
            "capture_limit",
            "Capture needs more than the 256 MiB combined image budget or has empty dimensions",
        ));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_budget_rejects_overflow_and_excess_before_allocation() {
        assert!(capture_budget(usize::MAX, 2).is_err());
        assert!(capture_budget(65535, 65535).is_err());
        assert!(capture_budget(0, 1080).is_err());
        assert!(capture_budget(3840, 2160).is_ok());
    }
    #[test]
    fn capture_decodes_stride_endianness_and_rgb565() {
        assert_eq!(
            decode_rgb(&[0, 0xf8, 0, 0], 1, 1, 16, 32, true, [0xf800, 0x7e0, 0x1f]).unwrap(),
            [255, 0, 0]
        );
        assert_eq!(
            decode_rgb(
                &[0, 0x12, 0x34, 0x56],
                1,
                1,
                32,
                32,
                false,
                [0xff0000, 0xff00, 0xff]
            )
            .unwrap(),
            [0x12, 0x34, 0x56]
        );
    }
    #[test]
    fn points_reject_nonfinite_and_rounded_outside() {
        for p in [
            Point { x: f64::NAN, y: 0. },
            Point { x: -1., y: 0. },
            Point { x: 99.6, y: 0. },
            Point { x: 40000., y: 0. },
        ] {
            assert!(validate_point(&p, 100., 100.).is_err());
        }
        assert_eq!(
            validate_point(&Point { x: 25.4, y: 9.8 }, 100., 100.).unwrap(),
            (25, 10)
        );
    }
}

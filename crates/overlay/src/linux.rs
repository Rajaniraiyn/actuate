//! Wayland layer-shell cursor renderer.
//!
//! One small overlay-layer surface per output shows the shared cursor glyph
//! with an empty input region, so it never receives or blocks pointer
//! events and never takes keyboard focus. Window scope clips the glyph to a
//! Hyprland window rectangle, follows that window, and hides when the window
//! is unmapped or not on its monitor's active workspace. Occluding windows
//! cannot cover an overlay-layer surface; that differs from macOS.
use crate::{
    CursorAcknowledgement, CursorAppearance, CursorCommand, CursorScope,
    shape::{HOTSPOT, OUTLINE, UNIT},
};
use compositor::{
    Hyprland,
    wayland::{Desktop, Outputs, ShmBuffer, fail},
};
use std::{
    io::BufRead,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use unimation::{
    NativeError, Result,
    motion::{self, MotionStyle},
};
use wayland_client::{
    Connection, Dispatch, QueueHandle,
    globals::GlobalListContents,
    protocol::{wl_buffer, wl_compositor, wl_region, wl_registry, wl_shm, wl_shm_pool, wl_surface},
};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

/// Logical size of the square surface that holds the glyph and click ring.
const BOX: i32 = 96;
/// Where the hotspot sits inside the box.
const TIP: (f64, f64) = (32., 32.);
const TICK: Duration = Duration::from_millis(16);
/// Poll interval while nothing moves; commands still arrive through the channel.
const IDLE_TICK: Duration = Duration::from_millis(100);
const WINDOW_POLL: Duration = Duration::from_millis(100);
/// The glyph motion in progress.
#[derive(Clone, Copy)]
struct Motion {
    from: (f64, f64),
    to: (f64, f64),
    since: Instant,
    duration: Duration,
}
/// What the last uploaded frame drew, so unchanged ticks skip rasterization.
#[derive(Clone, PartialEq)]
struct FrameKey {
    fractional: (i32, i32),
    idle: (i32, i32),
    pulse: Option<i32>,
    appearance: CursorAppearance,
    clip: Option<(i64, i64, i64, i64)>,
}

struct Layer {
    output_origin: (i32, i32),
    output_size: (i32, i32),
    scale: i32,
    surface: wl_surface::WlSurface,
    layer: zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
    configured: bool,
    buffer: Option<ShmBuffer>,
    last_margin: Option<(i32, i32)>,
    last_key: Option<FrameKey>,
    mapped_visible: bool,
}
#[derive(Default)]
struct State {
    outputs: Outputs,
    configured: Vec<(zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, u32)>,
}
impl AsMut<Outputs> for State {
    fn as_mut(&mut self) -> &mut Outputs {
        &mut self.outputs
    }
}
impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
compositor::delegate_outputs!(State);
compositor::delegate_silent!(
    State,
    [
        wl_compositor::WlCompositor,
        wl_shm::WlShm,
        wl_shm_pool::WlShmPool,
        wl_buffer::WlBuffer,
        wl_surface::WlSurface,
        wl_region::WlRegion,
        zwlr_layer_shell_v1::ZwlrLayerShellV1,
    ]
);
impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, ()> for State {
    fn event(
        state: &mut Self,
        layer: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure { serial, .. } => {
                state.configured.push((layer.clone(), serial))
            }
            zwlr_layer_surface_v1::Event::Closed => {
                state.configured.retain(|(l, _)| l != layer);
            }
            _ => {}
        }
    }
}

struct Target {
    rect: Option<(f64, f64, f64, f64)>,
    visible: bool,
    checked: Instant,
}

struct Renderer {
    desktop: Desktop<State>,
    state: State,
    compositor: wl_compositor::WlCompositor,
    shm: wl_shm::WlShm,
    layer_shell: zwlr_layer_shell_v1::ZwlrLayerShellV1,
    layers: Vec<Layer>,
    hypr: Option<Hyprland>,
    appearance: CursorAppearance,
    scope: CursorScope,
    target: Option<Target>,
    position: (f64, f64),
    motion: Option<Motion>,
    pulse: Option<Instant>,
    last_activity: Instant,
    visible: bool,
}
impl Renderer {
    fn new() -> Result<Self> {
        let mut desktop = Desktop::<State>::connect()?;
        let qh = desktop.qh();
        let mut state = State::default();
        state.outputs.bind(&desktop.globals, &qh);
        let compositor: wl_compositor::WlCompositor =
            desktop.globals.bind(&qh, 4..=6, ()).map_err(fail)?;
        let shm: wl_shm::WlShm = desktop.globals.bind(&qh, 1..=1, ()).map_err(fail)?;
        let layer_shell: zwlr_layer_shell_v1::ZwlrLayerShellV1 =
            desktop.globals.bind(&qh, 1..=4, ()).map_err(|_| {
                NativeError::new(
                    "cursor_renderer_unavailable",
                    "Compositor lacks zwlr_layer_shell_v1",
                )
            })?;
        desktop.roundtrip(&mut state)?;
        desktop.roundtrip(&mut state)?;
        let mut renderer = Self {
            desktop,
            state,
            compositor,
            shm,
            layer_shell,
            layers: vec![],
            hypr: Hyprland::from_env(),
            appearance: CursorAppearance::default(),
            scope: CursorScope::Desktop,
            target: None,
            position: (0., 0.),
            motion: None,
            pulse: None,
            last_activity: Instant::now(),
            visible: false,
        };
        renderer.create_layers()?;
        Ok(renderer)
    }
    fn create_layers(&mut self) -> Result<()> {
        let qh = self.desktop.qh();
        let outputs: Vec<_> = self
            .state
            .outputs
            .outputs
            .iter()
            .filter(|o| o.done && o.width > 0)
            .map(|o| {
                (
                    (o.x, o.y),
                    (o.width, o.height),
                    o.scale.max(1),
                    o.wl.clone(),
                )
            })
            .collect();
        for (origin, size, scale, wl) in outputs {
            let surface = self.compositor.create_surface(&qh, ());
            let region = self.compositor.create_region(&qh, ());
            surface.set_input_region(Some(&region));
            region.destroy();
            surface.set_buffer_scale(scale);
            let layer = self.layer_shell.get_layer_surface(
                &surface,
                Some(&wl),
                zwlr_layer_shell_v1::Layer::Overlay,
                "unimation-cursor".into(),
                &qh,
                (),
            );
            layer.set_anchor(
                zwlr_layer_surface_v1::Anchor::Top | zwlr_layer_surface_v1::Anchor::Left,
            );
            layer.set_size(BOX as u32, BOX as u32);
            layer.set_exclusive_zone(-1);
            layer.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::None);
            layer.set_margin(-BOX, 0, 0, -BOX);
            surface.commit();
            self.layers.push(Layer {
                output_origin: origin,
                output_size: size,
                scale,
                surface,
                layer,
                configured: false,
                buffer: None,
                last_margin: None,
                last_key: None,
                mapped_visible: false,
            });
        }
        self.desktop.roundtrip(&mut self.state)?;
        self.handle_configures();
        Ok(())
    }
    fn handle_configures(&mut self) {
        let configured = std::mem::take(&mut self.state.configured);
        for (layer, serial) in configured {
            if let Some(entry) = self.layers.iter_mut().find(|l| l.layer == layer) {
                layer.ack_configure(serial);
                entry.configured = true;
            }
        }
    }
    fn refresh_target(&mut self) {
        let CursorScope::Window { window_id, pid } = self.scope else {
            self.target = None;
            return;
        };
        let due = self
            .target
            .as_ref()
            .is_none_or(|t| t.checked.elapsed() >= WINDOW_POLL);
        if !due {
            return;
        }
        let Some(hypr) = &self.hypr else {
            self.target = Some(Target {
                rect: None,
                visible: false,
                checked: Instant::now(),
            });
            return;
        };
        let client = hypr
            .client_by_handle(window_id as u32, i64::from(pid))
            .ok()
            .flatten();
        let monitors = hypr.monitors().unwrap_or_default();
        let previous = self.target.as_ref().and_then(|t| t.rect);
        let target = match &client {
            Some(c) => Target {
                rect: Some((
                    c.at[0] as f64,
                    c.at[1] as f64,
                    c.size[0] as f64,
                    c.size[1] as f64,
                )),
                visible: c.mapped
                    && !c.hidden
                    && compositor::hyprland::is_on_active_workspace(c, &monitors),
                checked: Instant::now(),
            },
            None => Target {
                rect: None,
                visible: false,
                checked: Instant::now(),
            },
        };
        if let (Some(old), Some(new)) = (previous, target.rect) {
            let delta = (new.0 - old.0, new.1 - old.1);
            if delta != (0., 0.) {
                self.position.0 += delta.0;
                self.position.1 += delta.1;
                if let Some(motion) = &mut self.motion {
                    motion.from.0 += delta.0;
                    motion.from.1 += delta.1;
                    motion.to.0 += delta.0;
                    motion.to.1 += delta.1;
                }
            }
        }
        self.target = Some(target);
    }
    fn apply(&mut self, command: CursorCommand) -> bool {
        match command {
            CursorCommand::Move { x, y, duration_ms } => {
                self.last_activity = Instant::now();
                if duration_ms == 0 || self.appearance.motion == MotionStyle::Reduced {
                    self.position = (x, y);
                    self.motion = None;
                } else {
                    self.motion = Some(Motion {
                        from: self.position,
                        to: (x, y),
                        since: Instant::now(),
                        duration: Duration::from_millis(duration_ms),
                    });
                }
            }
            CursorCommand::Click { x, y } => {
                self.last_activity = Instant::now();
                self.position = (x, y);
                self.motion = None;
                self.pulse = Some(Instant::now());
            }
            CursorCommand::Configure { appearance } => self.appearance = appearance,
            CursorCommand::Scope { scope } => {
                self.scope = scope;
                self.target = None;
            }
            CursorCommand::Hide => self.visible = false,
            CursorCommand::Show => {
                self.visible = true;
                self.last_activity = Instant::now();
            }
            CursorCommand::Quit => return false,
        }
        true
    }
    /// Advances animation clocks; returns the glyph offset and ring progress.
    fn animate(&mut self) -> ((f64, f64), Option<f64>) {
        if let Some(m) = self.motion {
            let t = if m.duration.is_zero() {
                1.
            } else {
                m.since.elapsed().as_secs_f64() / m.duration.as_secs_f64()
            };
            self.position = motion::sample(m.from, m.to, t, self.appearance.motion);
            if t >= 1. {
                self.motion = None;
                self.last_activity = Instant::now();
            }
        }
        let pulse = match self.pulse {
            Some(since) => {
                let t = since.elapsed().as_secs_f64() / 0.45;
                if t >= 1. || self.appearance.motion == MotionStyle::Reduced {
                    self.pulse = None;
                    self.last_activity = Instant::now();
                    None
                } else {
                    Some(t)
                }
            }
            None => None,
        };
        let idle = if self.visible && self.motion.is_none() && self.pulse.is_none() {
            self.appearance.idle.offset(
                self.last_activity.elapsed().as_secs_f64(),
                self.appearance.motion,
            )
        } else {
            (0., 0.)
        };
        (idle, pulse)
    }
    fn render(&mut self) -> Result<()> {
        self.handle_configures();
        self.refresh_target();
        let (idle, pulse) = self.animate();
        let scope_visible = match self.scope {
            CursorScope::Desktop => true,
            CursorScope::Window { .. } => self.target.as_ref().is_some_and(|t| t.visible),
        };
        let clip = match self.scope {
            CursorScope::Desktop => None,
            CursorScope::Window { .. } => self.target.as_ref().and_then(|t| t.rect),
        };
        let show = self.visible && scope_visible;
        let position = self.position;
        let appearance = self.appearance.clone();
        let qh = self.desktop.qh();
        let shm = self.shm.clone();
        for layer in &mut self.layers {
            if !layer.configured {
                continue;
            }
            let on_output = show
                && position.0 >= layer.output_origin.0 as f64
                && position.1 >= layer.output_origin.1 as f64
                && position.0 < (layer.output_origin.0 + layer.output_size.0) as f64
                && position.1 < (layer.output_origin.1 + layer.output_size.1) as f64;
            if !on_output {
                if layer.mapped_visible {
                    // Keep the surface mapped but fully transparent and parked off-screen.
                    layer.layer.set_margin(-BOX, 0, 0, -BOX);
                    let pixels = layer.scale * BOX;
                    let buffer =
                        ShmBuffer::new(&shm, &qh, pixels, pixels, wl_shm::Format::Argb8888)?;
                    layer.surface.attach(Some(&buffer.buffer), 0, 0);
                    layer.surface.damage_buffer(0, 0, pixels, pixels);
                    layer.surface.commit();
                    layer.buffer = Some(buffer);
                    layer.last_margin = None;
                    layer.last_key = None;
                    layer.mapped_visible = false;
                }
                continue;
            }
            let local = (
                position.0 - layer.output_origin.0 as f64,
                position.1 - layer.output_origin.1 as f64,
            );
            let origin = (
                (local.0 - TIP.0).floor() as i32,
                (local.1 - TIP.1).floor() as i32,
            );
            let fractional = (
                local.0 - TIP.0 - origin.0 as f64,
                local.1 - TIP.1 - origin.1 as f64,
            );
            let pixels = layer.scale * BOX;
            let clip_local = clip.map(|(x, y, w, h)| {
                (
                    x - layer.output_origin.0 as f64 - origin.0 as f64,
                    y - layer.output_origin.1 as f64 - origin.1 as f64,
                    w,
                    h,
                )
            });
            // Quantize to a quarter point so idle float does not redraw every tick.
            let quarter = |v: f64| (v * 4.).round() as i32;
            let key = FrameKey {
                fractional: (quarter(fractional.0), quarter(fractional.1)),
                idle: (quarter(idle.0), quarter(idle.1)),
                pulse: pulse.map(|t| (t * 60.) as i32),
                appearance: appearance.clone(),
                clip: clip_local.map(|(x, y, w, h)| (x as i64, y as i64, w as i64, h as i64)),
            };
            let margin_changed = layer.last_margin != Some(origin);
            let frame_changed = layer.last_key.as_ref() != Some(&key);
            if !margin_changed && !frame_changed && layer.mapped_visible {
                continue;
            }
            if margin_changed {
                layer.layer.set_margin(origin.1, 0, 0, origin.0);
                layer.last_margin = Some(origin);
            }
            if frame_changed || layer.buffer.is_none() {
                let frame = draw(
                    pixels as u32,
                    layer.scale as f64,
                    &appearance,
                    (TIP.0 + fractional.0 + idle.0, TIP.1 + fractional.1 + idle.1),
                    pulse,
                    clip_local,
                );
                let mut buffer =
                    ShmBuffer::new(&shm, &qh, pixels, pixels, wl_shm::Format::Argb8888)?;
                buffer.map[..frame.len()].copy_from_slice(&frame);
                layer.surface.attach(Some(&buffer.buffer), 0, 0);
                layer.surface.damage_buffer(0, 0, pixels, pixels);
                layer.buffer = Some(buffer);
                layer.last_key = Some(key);
            }
            layer.surface.commit();
            layer.mapped_visible = true;
        }
        self.desktop.connection.flush().map_err(fail)?;
        Ok(())
    }
    /// Whether the next tick can change pixels without a new command.
    fn animating(&self) -> bool {
        self.motion.is_some()
            || self.pulse.is_some()
            || (self.visible
                && self.appearance.motion != MotionStyle::Reduced
                && self.appearance.idle.style != crate::idle::IdleStyle::Off)
            || matches!(self.scope, CursorScope::Window { .. })
    }
    fn pump(&mut self, wait: Duration) -> Result<()> {
        self.desktop
            .queue
            .dispatch_pending(&mut self.state)
            .map_err(fail)?;
        self.desktop.connection.flush().map_err(fail)?;
        if let Some(guard) = self.desktop.queue.prepare_read() {
            let fd = guard.connection_fd();
            let mut fds = [rustix::event::PollFd::new(
                &fd,
                rustix::event::PollFlags::IN,
            )];
            let timeout = rustix::time::Timespec {
                tv_sec: wait.as_secs() as _,
                tv_nsec: wait.subsec_nanos() as _,
            };
            let ready = rustix::event::poll(&mut fds, Some(&timeout)).unwrap_or(0);
            if ready > 0 {
                let _ = guard.read();
            } else {
                drop(guard);
            }
        }
        self.desktop
            .queue
            .dispatch_pending(&mut self.state)
            .map_err(fail)?;
        Ok(())
    }
}
impl Drop for Renderer {
    fn drop(&mut self) {
        for layer in &self.layers {
            layer.layer.destroy();
            layer.surface.destroy();
        }
        let _ = self.desktop.connection.flush();
    }
}

/// Rasterizes the glyph and ring into premultiplied ARGB8888 bytes.
fn draw(
    size: u32,
    scale: f64,
    appearance: &CursorAppearance,
    tip: (f64, f64),
    pulse: Option<f64>,
    clip: Option<(f64, f64, f64, f64)>,
) -> Vec<u8> {
    use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};
    let mut pixmap = Pixmap::new(size, size).expect("nonzero size");
    let glyph = appearance.scale;
    let point = |x: f64, y: f64| -> (f32, f32) {
        (
            ((tip.0 + (x - HOTSPOT.0) * UNIT * glyph) * scale) as f32,
            ((tip.1 + (y - HOTSPOT.1) * UNIT * glyph) * scale) as f32,
        )
    };
    let mut builder = PathBuilder::new();
    let (sx, sy) = point(OUTLINE[0].point.0, OUTLINE[0].point.1);
    builder.move_to(sx, sy);
    for index in 0..OUTLINE.len() {
        let from = OUTLINE[index];
        let to = OUTLINE[(index + 1) % OUTLINE.len()];
        let (c1x, c1y) = point(
            from.point.0 + from.outgoing.0,
            from.point.1 + from.outgoing.1,
        );
        let (c2x, c2y) = point(to.point.0 + to.incoming.0, to.point.1 + to.incoming.1);
        let (x, y) = point(to.point.0, to.point.1);
        builder.cubic_to(c1x, c1y, c2x, c2y, x, y);
    }
    builder.close();
    let [r, g, b] = appearance.color;
    if let Some(path) = builder.finish() {
        // Soft shadow: layered offset fills stand in for a blur.
        for (offset, alpha) in [(3.5, 0.10), (2.5, 0.14), (1.5, 0.18)] {
            let mut paint = Paint::default();
            paint.set_color(Color::from_rgba(0., 0., 0., alpha).unwrap());
            paint.anti_alias = true;
            let stroke = Stroke {
                width: (offset * glyph * scale) as f32,
                ..Default::default()
            };
            let shifted = Transform::from_translate(0., (2. * glyph * scale) as f32);
            pixmap.fill_path(&path, &paint, FillRule::Winding, shifted, None);
            pixmap.stroke_path(&path, &paint, &stroke, shifted, None);
        }
        let mut fill = Paint::default();
        fill.set_color(Color::from_rgba(r as f32, g as f32, b as f32, 1.).unwrap());
        fill.anti_alias = true;
        pixmap.fill_path(&path, &fill, FillRule::Winding, Transform::identity(), None);
        let mut outline = Paint::default();
        outline.set_color(Color::WHITE);
        outline.anti_alias = true;
        let stroke = Stroke {
            width: (1.5 * glyph * scale) as f32,
            ..Default::default()
        };
        pixmap.stroke_path(&path, &outline, &stroke, Transform::identity(), None);
    }
    if let Some(t) = pulse {
        let radius = ((5. + 17. * t) * glyph * scale) as f32;
        if let Some(ring) =
            PathBuilder::from_circle((tip.0 * scale) as f32, (tip.1 * scale) as f32, radius)
        {
            let mut paint = Paint::default();
            paint.set_color(
                Color::from_rgba(r as f32, g as f32, b as f32, ((1. - t) * 0.65) as f32).unwrap(),
            );
            paint.anti_alias = true;
            let stroke = Stroke {
                width: (2. * glyph * scale * (1. - t * 0.5)) as f32,
                ..Default::default()
            };
            pixmap.stroke_path(&ring, &paint, &stroke, Transform::identity(), None);
        }
    }
    let mut bytes = pixmap.take();
    if let Some((cx, cy, cw, ch)) = clip {
        let (x0, y0, x1, y1) = (cx * scale, cy * scale, (cx + cw) * scale, (cy + ch) * scale);
        for y in 0..size {
            for x in 0..size {
                let inside =
                    (x as f64) >= x0 && (x as f64) < x1 && (y as f64) >= y0 && (y as f64) < y1;
                if !inside {
                    let o = ((y * size + x) * 4) as usize;
                    bytes[o..o + 4].fill(0);
                }
            }
        }
    }
    // tiny-skia is premultiplied RGBA; wl_shm ARGB8888 is BGRA in memory.
    for px in bytes.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    bytes
}

fn run_loop(
    receiver: Receiver<CursorCommand>,
    running: Arc<AtomicBool>,
    ready: SyncSender<Result<()>>,
) {
    let mut renderer = match Renderer::new() {
        Ok(r) => {
            let _ = ready.send(Ok(()));
            r
        }
        Err(e) => {
            let _ = ready.send(Err(e));
            running.store(false, Ordering::Release);
            return;
        }
    };
    let mut last_error: Option<NativeError> = None;
    'outer: while running.load(Ordering::Acquire) {
        for _ in 0..256 {
            match receiver.try_recv() {
                Ok(command) => {
                    if !renderer.apply(command) {
                        break 'outer;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break 'outer,
            }
        }
        if let Err(e) = renderer.render() {
            if last_error
                .as_ref()
                .map(|l| l.message != e.message)
                .unwrap_or(true)
            {
                eprintln!("unimation-overlay: {e}");
            }
            last_error = Some(e);
        }
        let wait = if renderer.animating() {
            TICK
        } else {
            IDLE_TICK
        };
        if renderer.pump(wait).is_err() {
            break;
        }
    }
    running.store(false, Ordering::Release);
}

/// In-process renderer on its own thread and Wayland connection.
pub struct LayerCursor {
    sender: Option<SyncSender<CursorCommand>>,
    thread: Option<JoinHandle<()>>,
    running: Arc<AtomicBool>,
}
impl LayerCursor {
    pub fn start() -> Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(256);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let running = Arc::new(AtomicBool::new(true));
        let flag = running.clone();
        let thread = std::thread::Builder::new()
            .name("unimation-cursor".into())
            .spawn(move || run_loop(receiver, flag, ready_tx))
            .map_err(|e| NativeError::new("cursor_renderer_unavailable", e))?;
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Self {
                sender: Some(sender),
                thread: Some(thread),
                running,
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(NativeError::new(
                "cursor_renderer_unavailable",
                "Renderer thread did not start",
            )),
        }
    }
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }
    pub fn visualize(&mut self, command: CursorCommand) -> Result<CursorAcknowledgement> {
        command.validate()?;
        let sender = self
            .sender
            .as_ref()
            .filter(|_| self.running.load(Ordering::Acquire))
            .ok_or_else(|| NativeError::new("overlay_failed", "Renderer stopped"))?;
        sender.try_send(command).map_err(|e| match e {
            TrySendError::Full(_) => NativeError::new("overlay_failed", "Overlay queue is full"),
            TrySendError::Disconnected(_) => {
                NativeError::new("overlay_failed", "Renderer disconnected")
            }
        })?;
        Ok(CursorAcknowledgement::Queued)
    }
    pub fn stop(&mut self) -> Result<()> {
        if let Some(sender) = self.sender.take() {
            let _ = sender.try_send(CursorCommand::Quit);
        }
        self.running.store(false, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        Ok(())
    }
}
impl Drop for LayerCursor {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
impl unimation::CursorVisualization for LayerCursor {
    type Command = CursorCommand;
    type Status = CursorAcknowledgement;
    fn visualize(&mut self, command: CursorCommand) -> Result<CursorAcknowledgement> {
        LayerCursor::visualize(self, command)
    }
}

/// Standalone helper: one JSON command per stdin line until EOF or quit.
pub fn run() {
    let mut cursor = match LayerCursor::start() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("unimation-overlay: {e}");
            std::process::exit(1);
        }
    };
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        match serde_json::from_str::<CursorCommand>(&line) {
            Ok(CursorCommand::Quit) => break,
            Ok(command) => {
                if let Err(e) = cursor.visualize(command) {
                    eprintln!("unimation-overlay: {e}");
                }
            }
            Err(e) => eprintln!("unimation-overlay: invalid command: {e}"),
        }
        if !cursor.is_running() {
            break;
        }
    }
    let _ = cursor.stop();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn glyph_is_drawn_inside_the_box_and_clip_clears_outside() {
        let frame = draw(96, 1., &CursorAppearance::default(), TIP, Some(0.3), None);
        assert_eq!(frame.len(), 96 * 96 * 4);
        assert!(frame.chunks(4).any(|p| p[3] > 0));
        let clipped = draw(
            96,
            1.,
            &CursorAppearance::default(),
            TIP,
            None,
            Some((0., 0., 1., 1.)),
        );
        assert!(clipped.chunks(4).skip(97).all(|p| p[3] == 0));
    }
}

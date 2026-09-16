//! Raster and animation state use the same outline, easing and idle policy as
//! other renderers. Positions are never forwarded to an input provider.
use overlay::{CursorAppearance, CursorCommand, CursorScope, idle, shape};
use std::time::{Duration, Instant};
use tiny_skia::{Color, Paint, PathBuilder, Pixmap, Stroke, Transform};
use unimation::motion;

pub struct State {
    pub scope: CursorScope,
    pub visible: bool,
    pub appearance: CursorAppearance,
    from: (f64, f64),
    to: (f64, f64),
    started: Instant,
    duration: Duration,
    activity: Instant,
    click: Option<Instant>,
}
impl State {
    pub fn new(now: Instant) -> Self {
        Self {
            scope: CursorScope::Desktop,
            visible: false,
            appearance: CursorAppearance::default(),
            from: (0., 0.),
            to: (0., 0.),
            started: now,
            duration: Duration::ZERO,
            activity: now,
            click: None,
        }
    }
    pub fn position(&self, now: Instant) -> (f64, f64) {
        let progress = if self.duration.is_zero() {
            1.
        } else {
            now.duration_since(self.started).as_secs_f64() / self.duration.as_secs_f64()
        };
        motion::sample(self.from, self.to, progress, self.appearance.motion)
    }
    pub fn translate(&mut self, x: f64, y: f64) {
        self.from.0 += x;
        self.from.1 += y;
        self.to.0 += x;
        self.to.1 += y;
    }
    pub fn command(&mut self, command: CursorCommand, now: Instant) -> bool {
        match command {
            CursorCommand::Move { x, y, duration_ms } => {
                self.from = self.position(now);
                self.to = (x, y);
                self.started = now;
                self.duration = Duration::from_millis(duration_ms);
                self.activity = now + self.duration;
                self.visible = true;
            }
            CursorCommand::Click { x, y } => {
                self.from = (x, y);
                self.to = (x, y);
                self.duration = Duration::ZERO;
                self.click = Some(now);
                self.activity = now;
                self.visible = true;
            }
            CursorCommand::Configure { appearance } => self.appearance = appearance,
            CursorCommand::Scope { scope } => self.scope = scope,
            CursorCommand::Hide => self.visible = false,
            CursorCommand::Show => self.visible = true,
            CursorCommand::Quit => return false,
        }
        true
    }
    /// Premultiplied RGBA. Hotspot is the center of the square canvas.
    pub fn raster(&self, now: Instant, dpi_scale: f64) -> Option<Pixmap> {
        let size = (96. * dpi_scale).ceil() as u32;
        let mut pixmap = Pixmap::new(size, size)?;
        let center = size as f32 / 2.;
        let idle: idle::IdleAppearance = self.appearance.idle;
        let offset = idle.offset(
            now.saturating_duration_since(self.activity).as_secs_f64(),
            self.appearance.motion,
        );
        let scale = (shape::UNIT * self.appearance.scale * dpi_scale) as f32;
        let mut builder = PathBuilder::new();
        let vertex = shape::OUTLINE[0];
        builder.move_to(vertex.point.0 as f32, vertex.point.1 as f32);
        for i in 0..shape::OUTLINE.len() {
            let a = shape::OUTLINE[i];
            let b = shape::OUTLINE[(i + 1) % shape::OUTLINE.len()];
            builder.cubic_to(
                (a.point.0 + a.outgoing.0) as f32,
                (a.point.1 + a.outgoing.1) as f32,
                (b.point.0 + b.incoming.0) as f32,
                (b.point.1 + b.incoming.1) as f32,
                b.point.0 as f32,
                b.point.1 as f32,
            );
        }
        builder.close();
        let path = builder.finish()?;
        let transform = Transform::from_row(
            scale,
            0.,
            0.,
            scale,
            center - shape::HOTSPOT.0 as f32 * scale + (offset.0 * dpi_scale) as f32,
            center - shape::HOTSPOT.1 as f32 * scale + (offset.1 * dpi_scale) as f32,
        );
        let mut paint = Paint::default();
        paint.set_color_rgba8(0, 0, 0, 28);
        for spread in [2., 1.] {
            let shadow = transform.post_translate(dpi_scale as f32, 2. * dpi_scale as f32);
            pixmap.stroke_path(
                &path,
                &paint,
                &Stroke {
                    width: spread / shape::UNIT as f32,
                    ..Default::default()
                },
                shadow,
                None,
            );
        }
        paint.set_color_rgba8(250, 250, 252, 240);
        pixmap.stroke_path(
            &path,
            &paint,
            &Stroke {
                width: 1.25 / shape::UNIT as f32,
                ..Default::default()
            },
            transform,
            None,
        );
        paint.set_color(Color::from_rgba(
            self.appearance.color[0] as f32,
            self.appearance.color[1] as f32,
            self.appearance.color[2] as f32,
            1.,
        )?);
        pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, transform, None);
        if let Some(clicked) = self.click {
            let t = now.duration_since(clicked).as_secs_f32() / 0.55;
            if t < 1. {
                let radius = (5. + t * 18.) * dpi_scale as f32;
                let mut ring = PathBuilder::new();
                ring.push_circle(center, center, radius);
                if let Some(path) = ring.finish() {
                    paint.set_color_rgba8(75, 151, 230, ((1. - t) * 150.) as u8);
                    pixmap.stroke_path(
                        &path,
                        &paint,
                        &Stroke {
                            width: 1.5 * dpi_scale as f32,
                            ..Default::default()
                        },
                        Transform::identity(),
                        None,
                    );
                }
            }
        }
        Some(pixmap)
    }
}

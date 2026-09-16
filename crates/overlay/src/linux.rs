//! X11 renderer. Native Wayland overlays need a compositor-specific provider.
use overlay::{CursorAppearance, CursorCommand, CursorScope, motion, shape};
use std::{
    error::Error,
    io::{self, BufRead},
    sync::mpsc,
    time::{Duration, Instant},
};
use x11rb::{
    connection::Connection,
    protocol::{
        shape::{ConnectionExt as _, SK, SO},
        xproto::*,
    },
    rust_connection::RustConnection,
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
const SIZE: u16 = 192;
const TIP: f64 = 64.;

pub fn run() {
    if let Err(error) = run_x11() {
        eprintln!("overlay: {error}");
        std::process::exit(1);
    }
}
fn run_x11() -> Result<()> {
    // An XWayland window cannot represent the compositor's native window stack.
    if std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE").is_ok_and(|s| s == "wayland")
    {
        return Err("wayland_overlay_unavailable: native compositor integration is required; XWayland is not a desktop overlay fallback".into());
    }
    let (connection, screen_number) = x11rb::connect(None)?;
    let screen = &connection.setup().roots[screen_number];
    let root = screen.root;
    let version = connection.shape_query_version()?.reply()?;
    if (version.major_version, version.minor_version) < (1, 1) {
        return Err("X Shape 1.1 is required for click-through overlays".into());
    }
    let window = connection.generate_id()?;
    connection
        .create_window(
            screen.root_depth,
            window,
            root,
            0,
            0,
            SIZE,
            SIZE,
            0,
            WindowClass::INPUT_OUTPUT,
            screen.root_visual,
            &CreateWindowAux::new()
                .override_redirect(1)
                .background_pixel(screen.black_pixel),
        )?
        .check()?;
    connection
        .shape_rectangles(
            SO::SET,
            SK::INPUT,
            ClipOrdering::UNSORTED,
            window,
            0,
            0,
            &[],
        )?
        .check()?;
    let gc = connection.generate_id()?;
    connection
        .create_gc(gc, window, &CreateGCAux::new())?
        .check()?;
    let atom =
        |name: &[u8]| -> Result<Atom> { Ok(connection.intern_atom(false, name)?.reply()?.atom) };
    let atoms = Atoms {
        pid: atom(b"_NET_WM_PID")?,
        desktop: atom(b"_NET_WM_DESKTOP")?,
        current: atom(b"_NET_CURRENT_DESKTOP")?,
    };
    let (send, receive) = mpsc::sync_channel(128);
    std::thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            match line
                .map_err(|e| e.to_string())
                .and_then(|s| serde_json::from_str::<CursorCommand>(&s).map_err(|e| e.to_string()))
            {
                Ok(command) => {
                    if send.send(command).is_err() {
                        break;
                    }
                }
                Err(error) => eprintln!("overlay command: {error}"),
            }
        }
    });
    let mut state = State::default();
    let mut mapped = false;
    let mut hidden_reason: Option<String> = None;
    let mut allocated_color: Option<([f64; 3], u32)> = None;
    loop {
        // Follow the previous target before applying fresh absolute coordinates.
        let old_scope = state.scope;
        let mut resolved = resolve_scope(&connection, root, old_scope, &atoms);
        if let Ok(Some(target)) = resolved.as_ref() {
            state.follow(target);
        }
        for _ in 0..128 {
            match receive.try_recv() {
                Ok(command) => {
                    if let Err(error) = command.validate() {
                        eprintln!("overlay command: {error}");
                        continue;
                    }
                    if !state.command(command) {
                        connection.destroy_window(window)?.check()?;
                        connection.flush()?;
                        return Ok(());
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    connection.destroy_window(window)?.check()?;
                    connection.flush()?;
                    return Ok(());
                }
            }
        }
        for _ in 0..128 {
            if connection.poll_for_event()?.is_none() {
                break;
            }
        }
        if state.scope != old_scope {
            resolved = resolve_scope(&connection, root, state.scope, &atoms);
            // A new scope establishes its origin without changing absolute commands.
            if let Ok(Some(target)) = resolved.as_ref() {
                state.origin = Some((target.x, target.y));
            }
        }
        let attachment = match resolved {
            Ok(target) => {
                hidden_reason = None;
                Some(target)
            }
            Err(error) => {
                let reason = error.to_string();
                if hidden_reason.as_ref() != Some(&reason) {
                    eprintln!("overlay target hidden: {reason}");
                    hidden_reason = Some(reason);
                }
                None
            }
        };
        if state.visible && attachment.is_some() {
            let (x, y) = state.position();
            let idle = state.appearance.idle.offset(
                state.activity.elapsed().as_secs_f64(),
                state.appearance.motion,
            );
            let ox = (x - TIP).round();
            let oy = (y - TIP).round();
            if ox < i16::MIN as f64
                || ox > i16::MAX as f64
                || oy < i16::MIN as f64
                || oy > i16::MAX as f64
            {
                if mapped {
                    connection.unmap_window(window)?.check()?;
                    mapped = false;
                }
            } else {
                let target = attachment.flatten();
                let mut configure = ConfigureWindowAux::new().x(ox as i32).y(oy as i32);
                configure = if let Some(target) = target {
                    configure.sibling(target.frame).stack_mode(StackMode::ABOVE)
                } else {
                    configure.stack_mode(StackMode::ABOVE)
                };
                connection.configure_window(window, &configure)?.check()?;
                let region = glyph_region(&state, idle, ox, oy, target);
                connection
                    .shape_rectangles(
                        SO::SET,
                        SK::BOUNDING,
                        ClipOrdering::UNSORTED,
                        window,
                        0,
                        0,
                        &region,
                    )?
                    .check()?;
                if allocated_color.is_none_or(|(rgb, _)| rgb != state.appearance.color) {
                    let [r, g, b] = state.appearance.color;
                    let color = connection
                        .alloc_color(
                            screen.default_colormap,
                            (r * 65535.) as u16,
                            (g * 65535.) as u16,
                            (b * 65535.) as u16,
                        )?
                        .reply()?;
                    connection.change_gc(gc, &ChangeGCAux::new().foreground(color.pixel))?;
                    if let Some((_, old)) =
                        allocated_color.replace((state.appearance.color, color.pixel))
                    {
                        connection.free_colors(screen.default_colormap, 0, &[old])?;
                    }
                }
                connection.poly_fill_rectangle(
                    window,
                    gc,
                    &[Rectangle {
                        x: 0,
                        y: 0,
                        width: SIZE,
                        height: SIZE,
                    }],
                )?;
                if !mapped {
                    connection.map_window(window)?.check()?;
                    mapped = true;
                }
            }
        } else if mapped {
            connection.unmap_window(window)?.check()?;
            mapped = false;
        }
        connection.flush()?;
        std::thread::sleep(Duration::from_millis(16));
    }
}
struct Atoms {
    pid: Atom,
    desktop: Atom,
    current: Atom,
}
#[derive(Clone, Copy)]
struct Attachment {
    frame: Window,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}
fn property(c: &RustConnection, window: Window, atom: Atom) -> Result<Option<u32>> {
    Ok(
        c.get_property(false, window, atom, AtomEnum::CARDINAL, 0, 1)?
            .reply()?
            .value32()
            .and_then(|mut v| v.next()),
    )
}
fn resolve_scope(
    c: &RustConnection,
    root: Window,
    scope: CursorScope,
    atoms: &Atoms,
) -> Result<Option<Attachment>> {
    match scope {
        CursorScope::Desktop => Ok(None),
        CursorScope::Window { window_id, pid } => u32::try_from(window_id)
            .map_err(|e| Box::new(e) as Box<dyn Error>)
            .and_then(|id| attachment(c, root, id, pid, atoms))
            .map(Some),
    }
}
fn attachment(
    c: &RustConnection,
    root: Window,
    window: Window,
    pid: i32,
    atoms: &Atoms,
) -> Result<Attachment> {
    if property(c, window, atoms.pid)? != Some(pid as u32) {
        return Err("target owner unavailable or changed".into());
    }
    if c.get_window_attributes(window)?.reply()?.map_state != MapState::VIEWABLE {
        return Err("target not viewable".into());
    }
    let desktop = property(c, window, atoms.desktop)?.ok_or("target workspace unavailable")?;
    let current = property(c, root, atoms.current)?.ok_or("current workspace unavailable")?;
    if desktop != u32::MAX && desktop != current {
        return Err("target on inactive workspace".into());
    }
    let geometry = c.get_geometry(window)?.reply()?;
    let translated = c.translate_coordinates(window, root, 0, 0)?.reply()?;
    if !translated.same_screen {
        return Err("target is on another X screen".into());
    }
    let mut frame = window;
    for _ in 0..64 {
        let tree = c.query_tree(frame)?.reply()?;
        if tree.parent == root {
            return Ok(Attachment {
                frame,
                x: translated.dst_x as f64,
                y: translated.dst_y as f64,
                width: geometry.width as f64,
                height: geometry.height as f64,
            });
        }
        if tree.parent == 0 {
            break;
        }
        frame = tree.parent;
    }
    Err("target has no root sibling".into())
}
struct State {
    point: (f64, f64),
    start: (f64, f64),
    end: (f64, f64),
    movement: Instant,
    duration: f64,
    activity: Instant,
    pulse: Option<Instant>,
    visible: bool,
    scope: CursorScope,
    origin: Option<(f64, f64)>,
    appearance: CursorAppearance,
}
impl Default for State {
    fn default() -> Self {
        Self {
            point: (0., 0.),
            start: (0., 0.),
            end: (0., 0.),
            movement: Instant::now(),
            duration: 0.,
            activity: Instant::now(),
            pulse: None,
            visible: false,
            scope: CursorScope::Desktop,
            origin: None,
            appearance: CursorAppearance::default(),
        }
    }
}
impl State {
    fn position(&mut self) -> (f64, f64) {
        self.point = motion::sample(
            self.start,
            self.end,
            if self.duration == 0. {
                1.
            } else {
                self.movement.elapsed().as_secs_f64() / self.duration
            },
            self.appearance.motion,
        );
        self.point
    }
    fn follow(&mut self, target: &Attachment) {
        if let Some(old) = self.origin.replace((target.x, target.y)) {
            self.rebase((target.x - old.0, target.y - old.1));
        }
    }
    fn rebase(&mut self, d: (f64, f64)) {
        self.start.0 += d.0;
        self.start.1 += d.1;
        self.end.0 += d.0;
        self.end.1 += d.1;
    }
    fn command(&mut self, command: CursorCommand) -> bool {
        match command {
            CursorCommand::Move { x, y, duration_ms } => {
                self.start = self.position();
                self.end = (x, y);
                self.duration = duration_ms as f64 / 1000.;
                self.movement = Instant::now();
                self.activity = Instant::now();
                self.visible = true;
            }
            CursorCommand::Click { x, y } => {
                self.start = (x, y);
                self.end = (x, y);
                self.duration = 0.;
                self.pulse = Some(Instant::now());
                self.activity = Instant::now();
                self.visible = true;
            }
            CursorCommand::Configure { appearance } => self.appearance = appearance,
            CursorCommand::Scope { scope } => {
                self.scope = scope;
                self.origin = None;
            }
            CursorCommand::Hide => self.visible = false,
            CursorCommand::Show => self.visible = true,
            CursorCommand::Quit => return false,
        }
        true
    }
}
fn glyph_region(
    state: &State,
    idle: (f64, f64),
    ox: f64,
    oy: f64,
    target: Option<Attachment>,
) -> Vec<Rectangle> {
    let scale = shape::UNIT * state.appearance.scale;
    let mut polygon = Vec::new();
    for i in 0..shape::OUTLINE.len() {
        let a = shape::OUTLINE[i];
        let b = shape::OUTLINE[(i + 1) % shape::OUTLINE.len()];
        for step in 0..12 {
            let t = step as f64 / 12.;
            let u = 1. - t;
            let coord = |p: f64, q: f64, r: f64, s: f64| {
                u * u * u * p + 3. * u * u * t * q + 3. * u * t * t * r + t * t * t * s
            };
            polygon.push((
                TIP + idle.0
                    + (coord(
                        a.point.0,
                        a.point.0 + a.outgoing.0,
                        b.point.0 + b.incoming.0,
                        b.point.0,
                    ) - shape::HOTSPOT.0)
                        * scale,
                TIP + idle.1
                    + (coord(
                        a.point.1,
                        a.point.1 + a.outgoing.1,
                        b.point.1 + b.incoming.1,
                        b.point.1,
                    ) - shape::HOTSPOT.1)
                        * scale,
            ));
        }
    }
    let pulse = state
        .pulse
        .map(|p| p.elapsed().as_secs_f64() / 0.5)
        .filter(|t| *t < 1.);
    let mut region = Vec::new();
    let radius = pulse
        .map(|t| (5. + 17. * t) * state.appearance.scale + 2.)
        .unwrap_or(0.);
    let min_x = polygon
        .iter()
        .map(|p| p.0)
        .fold(TIP - radius, f64::min)
        .floor()
        .max(0.) as u16;
    let max_x = polygon
        .iter()
        .map(|p| p.0)
        .fold(TIP + radius, f64::max)
        .ceil()
        .min(SIZE as f64) as u16;
    let min_y = polygon
        .iter()
        .map(|p| p.1)
        .fold(TIP - radius, f64::min)
        .floor()
        .max(0.) as u16;
    let max_y = polygon
        .iter()
        .map(|p| p.1)
        .fold(TIP + radius, f64::max)
        .ceil()
        .min(SIZE as f64) as u16;
    for y in min_y..max_y {
        for x in min_x..max_x {
            let px = x as f64 + 0.5;
            let py = y as f64 + 0.5;
            if target.is_some_and(|a| {
                ox + px < a.x
                    || oy + py < a.y
                    || ox + px >= a.x + a.width
                    || oy + py >= a.y + a.height
            }) {
                continue;
            }
            let mut inside = false;
            for i in 0..polygon.len() {
                let a = polygon[i];
                let b = polygon[(i + 1) % polygon.len()];
                if (a.1 > py) != (b.1 > py) && px < (b.0 - a.0) * (py - a.1) / (b.1 - a.1) + a.0 {
                    inside = !inside;
                }
            }
            let ring = pulse.is_some_and(|t| {
                ((px - TIP).hypot(py - TIP) - (5. + 17. * t) * state.appearance.scale).abs() < 1.
            });
            if inside || ring {
                region.push(Rectangle {
                    x: x as i16,
                    y: y as i16,
                    width: 1,
                    height: 1,
                });
            }
        }
    }
    region
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scoped_glyph_never_paints_outside_target_client() {
        let state = State::default();
        let target = Attachment {
            frame: 10,
            x: 65.,
            y: 65.,
            width: 5.,
            height: 5.,
        };
        let region = glyph_region(&state, (0., 0.), 0., 0., Some(target));
        assert!(!region.is_empty());
        assert!(region.iter().all(|r| r.x >= 65
            && r.y >= 65
            && r.x + r.width as i16 <= 70
            && r.y + r.height as i16 <= 70));
        assert!(glyph_region(&state, (0., 0.), 1000., 1000., Some(target)).is_empty());
    }
    #[test]
    fn target_motion_precedes_fresh_absolute_commands() {
        let mut state = State {
            origin: Some((50., 50.)),
            ..Default::default()
        };
        let target = Attachment {
            frame: 10,
            x: 60.,
            y: 40.,
            width: 100.,
            height: 100.,
        };
        state.follow(&target);
        state.command(CursorCommand::Move {
            x: 100.,
            y: 200.,
            duration_ms: 0,
        });
        assert_eq!(state.position(), (100., 200.));
        state.follow(&target);
        assert_eq!(state.position(), (100., 200.));
        state.follow(&Attachment { x: 70., ..target });
        assert_eq!(state.position(), (110., 200.));
    }
}

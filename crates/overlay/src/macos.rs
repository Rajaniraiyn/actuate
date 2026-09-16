mod attachment;
mod overview;
use overlay::CursorScope;
use overlay::{
    CursorAppearance, CursorCommand as Command,
    shape::{HOTSPOT, OUTLINE, UNIT},
};
use unimation::motion::{self, MotionStyle};
const SIZE: f64 = 96.;
const TIP: (f64, f64) = (96., 160.);
fn appkit_origin(x: f64, y: f64, main_height: f64) -> (f64, f64) {
    (x - TIP.0, main_height - y - TIP.1)
}
use block2::RcBlock;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained};
use objc2_app_kit::*;
use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect, NSSize, NSTimer};
use std::{
    cell::RefCell,
    io::{self, BufRead},
    ptr::NonNull,
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};
thread_local! {static STATE:RefCell<Option<State>>=const{RefCell::new(None)};}
define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Visual]
    struct CursorView;
    unsafe impl NSObjectProtocol for CursorView {}
    impl CursorView {
        #[unsafe(method(drawRect:))]
        fn draw(&self,_rect:NSRect){
            let visual = self.ivars();
            if let Some(clip) = visual.clip.get() { NSBezierPath::clipRect(clip); }
            let appearance = visual.appearance.borrow();
            let scale = appearance.scale;
            let idle = visual.idle_offset.get();
            let point = |x: f64, y: f64| NSPoint::new(
                TIP.0 + idle.0 + (x - HOTSPOT.0) * UNIT * scale,
                TIP.1 + idle.1 - (y - HOTSPOT.1) * UNIT * scale,
            );
            let [r,g,b] = appearance.color;
            let pulse = visual.pulse.get();
            if let Some(t) = pulse {
                let radius = (5. + 17. * t) * scale;
                let ring = NSBezierPath::bezierPathWithOvalInRect(NSRect::new(NSPoint::new(TIP.0-radius,TIP.1-radius),NSSize::new(radius*2.,radius*2.)));
                NSColor::colorWithSRGBRed_green_blue_alpha(r,g,b,(1.-t)*0.65).setStroke();
                ring.setLineWidth(2. * scale * (1.-t*0.5));
                ring.stroke();
            }
            let shape=NSBezierPath::bezierPath();
            shape.moveToPoint(point(OUTLINE[0].point.0, OUTLINE[0].point.1));
            for index in 0..OUTLINE.len() {
                let from = OUTLINE[index];
                let to = OUTLINE[(index + 1) % OUTLINE.len()];
                shape.curveToPoint_controlPoint1_controlPoint2(
                    point(to.point.0, to.point.1),
                    point(from.point.0 + from.outgoing.0, from.point.1 + from.outgoing.1),
                    point(to.point.0 + to.incoming.0, to.point.1 + to.incoming.1),
                );
            }
            shape.closePath();
            NSGraphicsContext::saveGraphicsState_class();
            let shadow = NSShadow::new();
            shadow.setShadowOffset(NSSize::new(0.,-2.*scale));
            shadow.setShadowBlurRadius(3.5*scale);
            shadow.setShadowColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(0.,0.,0.,0.42)));
            shadow.set();
            NSColor::colorWithSRGBRed_green_blue_alpha(r,g,b,1.).setFill();
            shape.fill();
            NSGraphicsContext::restoreGraphicsState_class();
            NSColor::whiteColor().setStroke();shape.setLineWidth(1.5*scale);shape.stroke();
        }
    }
);
#[derive(Default)]
struct Visual {
    appearance: RefCell<CursorAppearance>,
    clip: std::cell::Cell<Option<NSRect>>,
    pulse: std::cell::Cell<Option<f64>>,
    idle_offset: std::cell::Cell<(f64, f64)>,
}
type Motion = ((f64, f64), (f64, f64), Instant, Duration);
struct State {
    panel: Retained<NSPanel>,
    view: Retained<CursorView>,
    receiver: Receiver<Command>,
    position: (f64, f64),
    motion: Option<Motion>,
    pulse: Option<Instant>,
    last_origin: Option<(f64, f64)>,
    last_activity: Instant,
    visible: bool,
    scope: CursorScope,
    groups: Option<attachment::Groups>,
    overview: Option<overview::Overview>,
    overview_retry: Instant,
    attached: bool,
    target_origin: Option<(f64, f64)>,
    target_bounds: Option<objc2_core_foundation::CGRect>,
}
fn tick() {
    let mtm = MainThreadMarker::new().expect("overlay timer runs on main thread");
    STATE.with(|cell| {
        let mut state = cell.borrow_mut();
        let Some(s) = state.as_mut() else {
            return;
        };
        if let Some(window) = attachment::window(s.scope) {
            let origin = (window.bounds.origin.x, window.bounds.origin.y);
            if let Some(old) = s.target_origin {
                let delta = (origin.0 - old.0, origin.1 - old.1);
                s.position.0 += delta.0;
                s.position.1 += delta.1;
                if let Some((ref mut start, ref mut end, _, _)) = s.motion {
                    start.0 += delta.0;
                    start.1 += delta.1;
                    end.0 += delta.0;
                    end.1 += delta.1;
                }
            }
            s.target_origin = Some(origin);
        }
        for _ in 0..256 {
            let command = match s.receiver.try_recv() {
                Ok(c) => c,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    NSApplication::sharedApplication(mtm).terminate(None);
                    return;
                }
            };
            match command {
                Command::Move { x, y, duration_ms }
                    if x.is_finite() && y.is_finite() && duration_ms <= 10000 =>
                {
                    s.last_activity = Instant::now();
                    if duration_ms == 0
                        || s.view.ivars().appearance.borrow().motion == MotionStyle::Reduced
                    {
                        s.position = (x, y);
                        s.motion = None;
                        continue;
                    }
                    s.motion = Some((
                        s.position,
                        (x, y),
                        Instant::now(),
                        Duration::from_millis(duration_ms),
                    ));
                }
                Command::Click { x, y } if x.is_finite() && y.is_finite() => {
                    s.last_activity = Instant::now();
                    s.position = (x, y);
                    s.motion = None;
                    s.pulse = Some(Instant::now());
                }
                Command::Configure { appearance } if appearance.is_valid() => {
                    s.last_activity = Instant::now();
                    if appearance.motion == MotionStyle::Reduced {
                        if let Some((_, end, _, _)) = s.motion.take() {
                            s.position = end;
                        }
                        s.pulse = None;
                    }
                    *s.view.ivars().appearance.borrow_mut() = appearance;
                    s.view.setNeedsDisplay(true);
                }
                Command::Scope { scope } => {
                    if scope != s.scope {
                        s.panel.orderOut(None);
                        if let Some(groups) = &s.groups {
                            groups.detach(s.panel.windowNumber() as u32);
                        }
                        s.scope = scope;
                        s.attached = false;
                        s.target_origin = attachment::window(scope)
                            .map(|w| (w.bounds.origin.x, w.bounds.origin.y));
                    }
                }
                Command::Show => {
                    s.visible = true;
                    s.last_activity = Instant::now();
                }
                Command::Hide => {
                    s.visible = false;
                    s.panel.orderOut(None);
                }
                Command::Quit => {
                    NSApplication::sharedApplication(mtm).terminate(None);
                    return;
                }
                _ => {
                    eprintln!("Invalid coordinates or duration_ms exceeds 10000");
                }
            }
        }
        if s.overview.as_ref().is_some_and(|o| !o.alive()) {
            s.overview = None;
        }
        if s.overview.is_none() && s.overview_retry.elapsed() >= Duration::from_secs(1) {
            s.overview = overview::Overview::observe();
            s.overview_retry = Instant::now();
        }
        // Missing overview monitoring suppresses visuals until it reconnects.
        let overview = s.overview.as_ref().is_none_or(|o| o.active());
        match s.scope {
            CursorScope::Desktop => {
                s.target_bounds = None;
                if s.panel.level() != 1000 {
                    s.panel.setLevel(1000);
                }
                let behavior = NSWindowCollectionBehavior::CanJoinAllSpaces
                    | NSWindowCollectionBehavior::Transient
                    | NSWindowCollectionBehavior::IgnoresCycle;
                if s.panel.collectionBehavior() != behavior {
                    s.panel.setCollectionBehavior(behavior);
                }
                if s.visible && !overview && !s.panel.isVisible() {
                    s.panel.orderFrontRegardless();
                }
            }
            CursorScope::Window { window_id, .. } => {
                if let (Ok(window_id), Some(window)) =
                    (u32::try_from(window_id), attachment::window(s.scope))
                {
                    s.target_bounds = Some(window.bounds);
                    if s.panel.level() != window.level {
                        s.panel.setLevel(window.level);
                        s.attached = false;
                    }
                    let behavior = NSWindowCollectionBehavior::Transient
                        | NSWindowCollectionBehavior::IgnoresCycle
                        | NSWindowCollectionBehavior::FullScreenAuxiliary
                        | NSWindowCollectionBehavior::MoveToActiveSpace;
                    if s.panel.collectionBehavior() != behavior {
                        s.panel.setCollectionBehavior(behavior);
                        s.attached = false;
                    }
                    if s.visible && window.on_screen && !overview {
                        if !s.panel.isVisible()
                            || !s.attached
                            || !attachment::correctly_ordered(
                                window_id,
                                s.panel.windowNumber() as u32,
                            )
                        {
                            s.panel.orderWindow_relativeTo(
                                NSWindowOrderingMode::Above,
                                window_id as isize,
                            );
                            s.attached = s.groups.as_ref().is_some_and(|g| {
                                g.attach(window_id, s.panel.windowNumber() as u32)
                            });
                        }
                        if !s.attached {
                            s.panel.orderOut(None);
                        }
                    } else {
                        s.panel.orderOut(None);
                    }
                } else {
                    s.panel.orderOut(None);
                    s.attached = false;
                }
            }
        }
        if overview {
            s.panel.orderOut(None);
        }
        if let Some((start, end, time, duration)) = s.motion {
            let t = if duration.is_zero() {
                1.
            } else {
                time.elapsed().as_secs_f64() / duration.as_secs_f64()
            };
            s.position = motion::sample(start, end, t, s.view.ivars().appearance.borrow().motion);
            if t >= 1. {
                s.motion = None;
                s.last_activity = Instant::now();
            }
        }
        let main = objc2_core_graphics::CGDisplayBounds(objc2_core_graphics::CGMainDisplayID());
        let (x, y) = appkit_origin(s.position.0, s.position.1, main.size.height);
        let clip = s.target_bounds.map(|bounds| {
            NSRect::new(
                NSPoint::new(
                    bounds.origin.x - x,
                    main.size.height - bounds.origin.y - bounds.size.height - y,
                ),
                NSSize::new(bounds.size.width, bounds.size.height),
            )
        });
        if s.view.ivars().clip.replace(clip) != clip {
            s.view.setNeedsDisplay(true);
        }
        if s.last_origin != Some((x, y)) {
            s.panel.setFrameOrigin(NSPoint::new(x, y));
            s.last_origin = Some((x, y));
        }
        if let Some(time) = s.pulse {
            let t = time.elapsed().as_secs_f64() / 0.45;
            if t >= 1. || s.view.ivars().appearance.borrow().motion == MotionStyle::Reduced {
                s.pulse = None;
                s.last_activity = Instant::now();
                s.view.ivars().pulse.set(None);
            } else {
                s.view.ivars().pulse.set(Some(t));
            }
            s.view.setNeedsDisplay(true);
        } else if s.view.ivars().pulse.replace(None).is_some() {
            s.view.setNeedsDisplay(true);
        }
        // Decoration is view-local. The panel origin, semantic target and click
        // ring remain anchored to position; no input provider sees this offset.
        let offset = if s.visible && s.motion.is_none() && s.pulse.is_none() {
            let appearance = s.view.ivars().appearance.borrow();
            appearance
                .idle
                .offset(s.last_activity.elapsed().as_secs_f64(), appearance.motion)
        } else {
            (0., 0.)
        };
        if s.view.ivars().idle_offset.replace(offset) != offset {
            s.view.setNeedsDisplay(true);
        }
    });
}
pub fn run() {
    let mtm = MainThreadMarker::new().expect("overlay must start on main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
    let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
        NSPanel::alloc(mtm),
        NSRect::new(NSPoint::new(0., 0.), NSSize::new(SIZE * 3., SIZE * 3.)),
        NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
        NSBackingStoreType::Buffered,
        false,
    );
    panel.setOpaque(false);
    panel.setBackgroundColor(Some(&NSColor::clearColor()));
    panel.setHasShadow(false);
    panel.setIgnoresMouseEvents(true);
    panel.setLevel(1000);
    panel.setHidesOnDeactivate(false);
    panel.setBecomesKeyOnlyIfNeeded(true);
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Stationary,
    );
    // SAFETY: superclass initializer initializes a main-thread allocated NSView subclass.
    let view: Retained<CursorView> = unsafe {
        msg_send![super(CursorView::alloc(mtm).set_ivars(Visual::default())),initWithFrame:NSRect::new(NSPoint::new(0.,0.),NSSize::new(SIZE*3.,SIZE*3.))]
    };
    panel.setContentView(Some(&view));
    let (sender, receiver) = mpsc::sync_channel(256);
    std::thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            match line {
                Ok(line) => match serde_json::from_str::<Command>(&line) {
                    Ok(command) => {
                        if sender.send(command).is_err() {
                            break;
                        }
                    }
                    Err(e) => eprintln!("Invalid overlay command: {e}"),
                },
                Err(_) => break,
            }
        }
    });
    STATE.with(|s| {
        *s.borrow_mut() = Some(State {
            panel,
            view,
            receiver,
            position: (0., 0.),
            motion: None,
            pulse: None,
            last_origin: None,
            last_activity: Instant::now(),
            visible: false,
            scope: CursorScope::Desktop,
            groups: attachment::Groups::load(),
            overview: overview::Overview::observe(),
            overview_retry: Instant::now(),
            attached: false,
            target_origin: None,
            target_bounds: None,
        })
    });
    let block = RcBlock::new(|_timer: NonNull<NSTimer>| tick());
    // SAFETY: closure captures no thread-bound state; timer is scheduled on this main run loop.
    let _timer =
        unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(1. / 60., true, &block) };
    app.run();
}

#[cfg(test)]
mod tests {
    #[test]
    fn desktop_coordinates() {
        assert_eq!(super::appkit_origin(-200., -50., 1080.), (-296., 970.));
    }
}

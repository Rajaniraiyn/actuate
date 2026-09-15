use overlay::{
    CursorAppearance, CursorCommand as Command,
    motion::{self, MotionStyle},
};
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
            let appearance = visual.appearance.borrow();
            let scale = appearance.scale;
            let point = |x: f64, y: f64| NSPoint::new(TIP.0 + x * scale, TIP.1 - y * scale);
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
            shape.moveToPoint(point(0.,0.));
            for (x,y) in [(1.,25.),(7.,19.),(12.,30.),(17.,27.5),(12.,17.),(21.,16.)] {
                shape.lineToPoint(point(x,y));
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
    pulse: std::cell::Cell<Option<f64>>,
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
}
fn tick() {
    let mtm = MainThreadMarker::new().expect("overlay timer runs on main thread");
    STATE.with(|cell| {
        let mut state = cell.borrow_mut();
        let Some(s) = state.as_mut() else {
            return;
        };
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
                    s.position = (x, y);
                    s.motion = None;
                    s.pulse = Some(Instant::now());
                }
                Command::Configure { appearance } if appearance.is_valid() => {
                    if appearance.motion == MotionStyle::Reduced {
                        if let Some((_, end, _, _)) = s.motion.take() {
                            s.position = end;
                        }
                        s.pulse = None;
                    }
                    *s.view.ivars().appearance.borrow_mut() = appearance;
                    s.view.setNeedsDisplay(true);
                }
                Command::Show => {
                    s.panel.orderFrontRegardless();
                }
                Command::Hide => {
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
        if let Some((start, end, time, duration)) = s.motion {
            let t = if duration.is_zero() {
                1.
            } else {
                time.elapsed().as_secs_f64() / duration.as_secs_f64()
            };
            s.position = motion::sample(start, end, t, s.view.ivars().appearance.borrow().motion);
            if t >= 1. {
                s.motion = None;
            }
        }
        let main = objc2_core_graphics::CGDisplayBounds(objc2_core_graphics::CGMainDisplayID());
        let (x, y) = appkit_origin(s.position.0, s.position.1, main.size.height);
        if s.last_origin != Some((x, y)) {
            s.panel.setFrameOrigin(NSPoint::new(x, y));
            s.last_origin = Some((x, y));
        }
        if let Some(time) = s.pulse {
            let t = time.elapsed().as_secs_f64() / 0.45;
            if t >= 1. || s.view.ivars().appearance.borrow().motion == MotionStyle::Reduced {
                s.pulse = None;
                s.view.ivars().pulse.set(None);
            } else {
                s.view.ivars().pulse.set(Some(t));
            }
            s.view.setNeedsDisplay(true);
        } else if s.view.ivars().pulse.replace(None).is_some() {
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

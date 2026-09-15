use super::*;
use block2::RcBlock;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained};
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
    struct CursorView;
    unsafe impl NSObjectProtocol for CursorView {}
    impl CursorView {
        #[unsafe(method(drawRect:))]
        fn draw(&self,_rect:NSRect){
            let shape=NSBezierPath::bezierPath();
            shape.moveToPoint(NSPoint::new(8.,40.));shape.lineToPoint(NSPoint::new(10.,12.));shape.lineToPoint(NSPoint::new(18.,20.));shape.lineToPoint(NSPoint::new(25.,5.));shape.lineToPoint(NSPoint::new(31.,8.));shape.lineToPoint(NSPoint::new(23.,23.));shape.lineToPoint(NSPoint::new(36.,24.));shape.closePath();
            NSColor::systemPurpleColor().setFill();shape.fill();NSColor::whiteColor().setStroke();shape.setLineWidth(2.);shape.stroke();
        }
    }
);
type Motion = ((f64, f64), (f64, f64), Instant, Duration);
struct State {
    panel: Retained<NSPanel>,
    receiver: Receiver<Command>,
    position: (f64, f64),
    motion: Option<Motion>,
    pulse: Option<Instant>,
    visible: bool,
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
                    if duration_ms == 0 {
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
                Command::Show => {
                    s.visible = true;
                    s.panel.orderFrontRegardless();
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
        if let Some((start, end, time, duration)) = s.motion {
            let t = if duration.is_zero() {
                1.
            } else {
                time.elapsed().as_secs_f64() / duration.as_secs_f64()
            };
            s.position = interpolate(start, end, t);
            if t >= 1. {
                s.motion = None;
            }
        }
        let main = objc2_core_graphics::CGDisplayBounds(objc2_core_graphics::CGMainDisplayID());
        let (x, y) = appkit_origin(s.position.0, s.position.1, main.size.height);
        s.panel.setFrameOrigin(NSPoint::new(x, y));
        if let Some(time) = s.pulse {
            let t = time.elapsed().as_secs_f64() / 0.45;
            if t >= 1. {
                s.pulse = None;
                s.panel.setAlphaValue(1.);
            } else {
                s.panel
                    .setAlphaValue(0.35 + 0.65 * (t * std::f64::consts::PI).sin().abs());
            }
        }
    });
}
pub fn run() {
    let mtm = MainThreadMarker::new().expect("overlay must start on main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
    let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
        NSPanel::alloc(mtm),
        NSRect::new(NSPoint::new(0., 0.), NSSize::new(48., 48.)),
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
        msg_send![CursorView::alloc(mtm),initWithFrame:NSRect::new(NSPoint::new(0.,0.),NSSize::new(48.,48.))]
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
            receiver,
            position: (0., 0.),
            motion: None,
            pulse: None,
            visible: false,
        })
    });
    let block = RcBlock::new(|_timer: NonNull<NSTimer>| tick());
    // SAFETY: closure captures no thread-bound state; timer is scheduled on this main run loop.
    let _timer =
        unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(1. / 60., true, &block) };
    app.run();
}

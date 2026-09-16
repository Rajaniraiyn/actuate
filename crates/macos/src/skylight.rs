//! Window-addressed SkyLight input. Private entry points are runtime capabilities.
//! A dispatch receipt does not prove that an application consumed an event.
use crate::{Accessibility, error};
use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFString, CFType, CGPoint};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventType, CGMouseButton, CGScrollEventUnit,
    CGWindowListCopyWindowInfo, CGWindowListOption,
};
use serde::{Deserialize, Serialize};
use std::{
    ffi::{CStr, c_void},
    sync::{
        OnceLock,
        atomic::{AtomicI64, Ordering},
    },
};
use unimation::{Effect, Modifiers, Point, Receipt, Result};

type Post = unsafe extern "C" fn(i32, *const CGEvent);
type SetLocal = unsafe extern "C" fn(*const CGEvent, CGPoint);
type SetField = unsafe extern "C" fn(*const CGEvent, u32, i64);

struct Symbols {
    post: Post,
    local: SetLocal,
    field: SetField,
}
fn symbols() -> Option<&'static Symbols> {
    static SYMBOLS: OnceLock<Option<Symbols>> = OnceLock::new();
    SYMBOLS
        .get_or_init(|| {
            // The framework stays loaded for the lifetime of these function pointers.
            let handle = unsafe {
                libc::dlopen(
                    c"/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight".as_ptr(),
                    libc::RTLD_LAZY | libc::RTLD_LOCAL,
                )
            };
            if handle.is_null() {
                return None;
            }
            let lookup = |name: &CStr| {
                let p = unsafe { libc::dlsym(handle, name.as_ptr()) };
                (!p.is_null()).then_some(p)
            };
            // Each signature follows the native API, including CGPoint by value.
            Some(unsafe {
                Symbols {
                    post: std::mem::transmute::<*mut c_void, Post>(lookup(c"SLEventPostToPid")?),
                    local: std::mem::transmute::<*mut c_void, SetLocal>(lookup(
                        c"CGEventSetWindowLocation",
                    )?),
                    field: std::mem::transmute::<*mut c_void, SetField>(lookup(
                        c"SLEventSetIntegerValueField",
                    )?),
                }
            })
        })
        .as_ref()
}

/// Coordinates must describe the same observation of this native window.
/// Desktop coordinates use Quartz points; local coordinates use top-left window points.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkyLightTarget {
    pub pid: i32,
    pub window_id: u32,
    pub desktop: Point,
    pub window_local: Point,
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkyLightButton {
    #[default]
    Left,
    Right,
    Middle,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SkyLightPointerAction {
    Move {
        #[serde(default)]
        modifiers: Modifiers,
    },
    Click {
        #[serde(default)]
        button: SkyLightButton,
        #[serde(default = "one")]
        count: u8,
        #[serde(default)]
        modifiers: Modifiers,
    },
    Scroll {
        vertical: i32,
        horizontal: i32,
        #[serde(default)]
        modifiers: Modifiers,
    },
    Drag {
        to: Point,
        duration_ms: u64,
        #[serde(default)]
        button: SkyLightButton,
        #[serde(default)]
        modifiers: Modifiers,
    },
}
fn one() -> u8 {
    1
}

/// This provider never activates a process, warps the shared cursor, or retries
/// through another delivery route. Applications may still change focus themselves.
pub struct SkyLightInput {
    symbols: &'static Symbols,
    last_target: Option<SkyLightTarget>,
}
impl SkyLightInput {
    pub fn new() -> Result<Self> {
        Ok(Self {
            last_target: None,
            symbols: symbols().ok_or_else(|| {
                error(
                    "unsupported",
                    "Required SkyLight input symbols unavailable",
                    Effect::None,
                )
            })?,
        })
    }
    /// Last dispatched desktop point for this provider, independent of the OS
    /// cursor. It records delivery, not the application's consumption of input.
    pub fn last_pointer(&self) -> Option<&Point> {
        self.last_target.as_ref().map(|target| &target.desktop)
    }
    /// Preserve window/PID provenance alongside the virtual pointer position.
    pub fn last_target(&self) -> Option<&SkyLightTarget> {
        self.last_target.as_ref()
    }
    pub fn capabilities() -> serde_json::Value {
        serde_json::json!({"available":symbols().is_some(),"route":"macos.skylight.window", "private_api":true,"activates_process":false,"warps_cursor":false,"consumption_verified":false,"authenticated_keyboard":false})
    }
    pub fn pointer_at(
        &mut self,
        target: &SkyLightTarget,
        action: SkyLightPointerAction,
    ) -> Result<Receipt> {
        self.pointer_at_observed(target, action, |_| {})
    }
    /// Reports dispatched coordinates as input is delivered, for optional visuals.
    /// The callback must not block or inject additional input.
    pub fn pointer_at_observed(
        &mut self,
        target: &SkyLightTarget,
        action: SkyLightPointerAction,
        mut dispatched: impl FnMut(&Point),
    ) -> Result<Receipt> {
        validate(target)?;
        if !Accessibility::is_trusted() {
            return Err(error(
                "permission_denied",
                "Accessibility access required",
                Effect::None,
            ));
        }
        let dragging = matches!(action, SkyLightPointerAction::Drag { .. });
        let modifiers = match &action {
            SkyLightPointerAction::Move { modifiers }
            | SkyLightPointerAction::Click { modifiers, .. }
            | SkyLightPointerAction::Scroll { modifiers, .. }
            | SkyLightPointerAction::Drag { modifiers, .. } => *modifiers,
        };
        let flags = modifier_flags(modifiers);
        let mut events = vec![];
        let group = next_group();
        // Every event, including the final release, exists before dispatch begins.
        let make_mouse = |kind,
                          native,
                          location: &SkyLightTarget,
                          count: i64,
                          pressure: f64|
         -> Result<CFRetained<CGEvent>> {
            let event = allocate(CGEvent::new_mouse_event(
                None,
                kind,
                cg(&location.desktop),
                native,
            ))?;
            self.stamp(&event, location, group);
            CGEvent::set_flags(Some(&event), flags);
            CGEvent::set_integer_value_field(
                Some(&event),
                CGEventField::MouseEventClickState,
                count,
            );
            CGEvent::set_double_value_field(
                Some(&event),
                CGEventField::MouseEventPressure,
                pressure,
            );
            Ok(event)
        };
        events.push((
            make_mouse(CGEventType::MouseMoved, CGMouseButton::Left, target, 0, 0.0)?,
            12,
        ));
        match action {
            SkyLightPointerAction::Move { .. } => {}
            SkyLightPointerAction::Click { button, count, .. } => {
                if !(1..=3).contains(&count) {
                    return Err(error(
                        "invalid_request",
                        "Click count must be 1 through 3",
                        Effect::None,
                    ));
                }
                let (native, down, up, _) = button_types(button);
                for click in 1..=count {
                    events.push((make_mouse(down, native, target, click.into(), 1.0)?, 28));
                    events.push((make_mouse(up, native, target, click.into(), 0.0)?, 28));
                }
            }
            SkyLightPointerAction::Scroll {
                vertical,
                horizontal,
                ..
            } => {
                if vertical == 0 && horizontal == 0 {
                    return Ok(Receipt {
                        effect: Effect::None,
                        route: "macos.skylight.window".into(),
                    });
                }
                let event = allocate(CGEvent::new_scroll_wheel_event2(
                    None,
                    CGScrollEventUnit::Pixel,
                    2,
                    vertical,
                    horizontal,
                    0,
                ))?;
                CGEvent::set_location(Some(&event), cg(&target.desktop));
                CGEvent::set_flags(Some(&event), flags);
                self.stamp(&event, target, group);
                events.push((event, 0));
            }
            SkyLightPointerAction::Drag {
                to,
                duration_ms,
                button,
                ..
            } => {
                if !to.x.is_finite() || !to.y.is_finite() || !(16..=10_000).contains(&duration_ms) {
                    return Err(error(
                        "invalid_request",
                        "Drag needs finite destination and duration 16..10000ms",
                        Effect::None,
                    ));
                }
                let (native, down, up, dragged) = button_types(button);
                // A window move during this sequence invalidates its mapping. The
                // caller owns geometry stability and the target input lease.
                let steps = duration_ms.div_ceil(16);
                events.push((
                    make_mouse(down, native, target, 1, 1.0)?,
                    duration_ms / steps,
                ));
                let mut end = target.clone();
                let mut previous = target.desktop.clone();
                for step in 1..=steps {
                    let fraction = step as f64 / steps as f64;
                    end.desktop = Point {
                        x: target.desktop.x + (to.x - target.desktop.x) * fraction,
                        y: target.desktop.y + (to.y - target.desktop.y) * fraction,
                    };
                    end.window_local = Point {
                        x: target.window_local.x + end.desktop.x - target.desktop.x,
                        y: target.window_local.y + end.desktop.y - target.desktop.y,
                    };
                    validate(&end)?;
                    let event = make_mouse(dragged, native, &end, 1, 1.0)?;
                    CGEvent::set_integer_value_field(
                        Some(&event),
                        CGEventField::MouseEventDeltaX,
                        (end.desktop.x.round() - previous.x.round()) as i64,
                    );
                    CGEvent::set_integer_value_field(
                        Some(&event),
                        CGEventField::MouseEventDeltaY,
                        (end.desktop.y.round() - previous.y.round()) as i64,
                    );
                    previous = end.desktop.clone();
                    events.push((
                        event,
                        if step == steps {
                            0
                        } else {
                            duration_ms / steps
                        },
                    ));
                }
                events.push((make_mouse(up, native, &end, 1, 0.0)?, 0));
            }
        }
        // Prepared packets must receive delivery-time timestamps, not their
        // allocation times. Use a native timestamp plus monotonic elapsed time.
        let mut uptime = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // CLOCK_UPTIME_RAW uses mach_absolute_time, excluding sleep, and returns
        // nanoseconds without deprecated Mach bindings or allocation per packet.
        if unsafe { libc::clock_gettime(libc::CLOCK_UPTIME_RAW, &mut uptime) } != 0 {
            return Err(error(
                "event_clock",
                "Native event clock unavailable",
                Effect::None,
            ));
        }
        let base = (uptime.tv_sec as u64)
            .saturating_mul(1_000_000_000)
            .saturating_add(uptime.tv_nsec as u64);
        let started = std::time::Instant::now();
        let timestamp =
            || base.saturating_add(started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64);
        // Recheck ownership after preparation and immediately before any input.
        // Caller lifecycle tokens must still guard reuse of an ID by the same PID.
        validate_owner(target)?;
        let mut deadline = started;
        for (index, (event, delay)) in events.iter().enumerate() {
            if dragging
                && index > 0
                && let Err(mut failure) = validate_owner(target)
            {
                // A gesture may already have pressed a button. Send only its
                // prepared release, then stop; never continue a stale path.
                if let Some((release, _)) = events.last() {
                    // Release where the last packet went, not at the planned
                    // destination of an interrupted gesture.
                    if let Some(last) = &self.last_target {
                        CGEvent::set_location(Some(release), cg(&last.desktop));
                        let local = Point {
                            x: target.window_local.x + last.desktop.x - target.desktop.x,
                            y: target.window_local.y + last.desktop.y - target.desktop.y,
                        };
                        unsafe { (self.symbols.local)(&**release, cg(&local)) };
                    }
                    CGEvent::set_timestamp(Some(release), timestamp());
                    unsafe { (self.symbols.post)(target.pid, &**release) };
                }
                failure.effect = Effect::Unknown;
                return Err(failure);
            }
            CGEvent::set_timestamp(Some(event), timestamp());
            // The API has no acknowledgement; it reports dispatch only.
            unsafe { (self.symbols.post)(target.pid, &**event) };
            let position = CGEvent::location(Some(event));
            self.last_target = Some(pointer_state(
                target,
                Point {
                    x: position.x,
                    y: position.y,
                },
            ));
            dispatched(
                &self
                    .last_target
                    .as_ref()
                    .expect("dispatched target")
                    .desktop,
            );
            if index + 1 < events.len() && *delay != 0 {
                deadline += std::time::Duration::from_millis(*delay);
                std::thread::sleep(deadline.saturating_duration_since(std::time::Instant::now()));
            }
        }
        Ok(Receipt {
            effect: Effect::Dispatched,
            route: "macos.skylight.window".into(),
        })
    }
    fn stamp(&self, event: &CGEvent, target: &SkyLightTarget, group: i64) {
        // Only the target-window field remains private. Field 58 is a native
        // timestamp alias on the tested host, NOT a gesture identifier.
        CGEvent::set_integer_value_field(Some(event), CGEventField::MouseEventNumber, group);
        CGEvent::set_integer_value_field(
            Some(event),
            CGEventField::EventTargetUnixProcessID,
            i64::from(target.pid),
        );
        for field in [
            CGEventField::MouseEventWindowUnderMousePointer,
            CGEventField::MouseEventWindowUnderMousePointerThatCanHandleThisEvent,
        ] {
            CGEvent::set_integer_value_field(Some(event), field, i64::from(target.window_id));
        }
        unsafe {
            (self.symbols.local)(event, cg(&target.window_local));
            (self.symbols.field)(event, 51, i64::from(target.window_id));
        }
    }
}
fn pointer_state(target: &SkyLightTarget, desktop: Point) -> SkyLightTarget {
    SkyLightTarget {
        pid: target.pid,
        window_id: target.window_id,
        window_local: Point {
            x: target.window_local.x + desktop.x - target.desktop.x,
            y: target.window_local.y + desktop.y - target.desktop.y,
        },
        desktop,
    }
}
fn validate_owner(target: &SkyLightTarget) -> Result<()> {
    let invalid = || {
        error(
            "stale_target",
            "Window metadata unavailable or window is owned by a different PID",
            Effect::None,
        )
    };
    let array =
        CGWindowListCopyWindowInfo(CGWindowListOption::OptionIncludingWindow, target.window_id)
            .ok_or_else(invalid)?;
    if array.count() != 1 {
        return Err(invalid());
    }
    // Window-info arrays contain CF objects owned by the retained array.
    let object = unsafe { &*array.value_at_index(0).cast::<CFType>() };
    let dictionary = object.downcast_ref::<CFDictionary>().ok_or_else(invalid)?;
    let number = |name: &str| -> Option<i64> {
        let key = CFString::from_str(name);
        // Key and dictionary remain live through the borrowed-value lookup.
        let raw = unsafe { dictionary.value((&*key as *const CFString).cast()) };
        if raw.is_null() {
            return None;
        }
        unsafe { &*raw.cast::<CFType>() }
            .downcast_ref::<CFNumber>()?
            .as_i64()
    };
    if number("kCGWindowNumber") != Some(i64::from(target.window_id))
        || number("kCGWindowOwnerPID") != Some(i64::from(target.pid))
    {
        return Err(invalid());
    }
    let get = |name: &str| {
        let key = CFString::from_str(name);
        // SAFETY: borrowed CF values remain owned by the retained dictionary.
        let raw = unsafe { dictionary.value((&*key as *const CFString).cast()) };
        if raw.is_null() {
            None
        } else {
            Some(unsafe { &*raw.cast::<CFType>() })
        }
    };
    if !get("kCGWindowIsOnscreen")
        .and_then(|v| v.downcast_ref::<objc2_core_foundation::CFBoolean>())
        .is_some_and(|v| v.value())
    {
        return Err(error(
            "target_not_on_screen",
            "Target is minimized, hidden, or on another Space",
            Effect::None,
        ));
    }
    let bounds = get("kCGWindowBounds")
        .and_then(|v| v.downcast_ref::<CFDictionary>())
        .ok_or_else(invalid)?;
    let mut rect = objc2_core_foundation::CGRect::default();
    // SAFETY: retained dictionary and initialized output storage.
    if !unsafe {
        objc2_core_graphics::CGRectMakeWithDictionaryRepresentation(Some(bounds), &mut rect)
    } || (target.desktop.x - target.window_local.x - rect.origin.x).abs() > 0.5
        || (target.desktop.y - target.window_local.y - rect.origin.y).abs() > 0.5
    {
        return Err(error(
            "stale_geometry",
            "Target moved since coordinates were resolved; capture or resolve it again",
            Effect::None,
        ));
    }
    Ok(())
}

fn modifier_flags(modifiers: Modifiers) -> CGEventFlags {
    let mut flags = CGEventFlags::empty();
    for (enabled, flag) in [
        (modifiers.shift, CGEventFlags::MaskShift),
        (modifiers.control, CGEventFlags::MaskControl),
        (modifiers.alt, CGEventFlags::MaskAlternate),
        (modifiers.meta, CGEventFlags::MaskCommand),
    ] {
        if enabled {
            flags |= flag;
        }
    }
    flags
}
fn button_types(button: SkyLightButton) -> (CGMouseButton, CGEventType, CGEventType, CGEventType) {
    match button {
        SkyLightButton::Left => (
            CGMouseButton::Left,
            CGEventType::LeftMouseDown,
            CGEventType::LeftMouseUp,
            CGEventType::LeftMouseDragged,
        ),
        SkyLightButton::Right => (
            CGMouseButton::Right,
            CGEventType::RightMouseDown,
            CGEventType::RightMouseUp,
            CGEventType::RightMouseDragged,
        ),
        SkyLightButton::Middle => (
            CGMouseButton::Center,
            CGEventType::OtherMouseDown,
            CGEventType::OtherMouseUp,
            CGEventType::OtherMouseDragged,
        ),
    }
}

fn allocate(event: Option<CFRetained<CGEvent>>) -> Result<CFRetained<CGEvent>> {
    event.ok_or_else(|| {
        error(
            "event_creation",
            "Quartz could not allocate event",
            Effect::None,
        )
    })
}
fn cg(point: &Point) -> CGPoint {
    CGPoint::new(point.x, point.y)
}
fn next_group() -> i64 {
    static GROUP: AtomicI64 = AtomicI64::new(1);
    GROUP.fetch_add(1, Ordering::Relaxed)
}
fn validate(target: &SkyLightTarget) -> Result<()> {
    if target.pid <= 0
        || target.window_id == 0
        || [
            target.desktop.x,
            target.desktop.y,
            target.window_local.x,
            target.window_local.y,
        ]
        .iter()
        .any(|v| !v.is_finite())
    {
        return Err(error(
            "invalid_request",
            "Positive PID, nonzero window ID, and finite coordinates required",
            Effect::None,
        ));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn routing_preserves_timestamp_and_uses_public_mouse_event_number() {
        let Some(symbols) = symbols() else {
            return;
        };
        let provider = SkyLightInput {
            symbols,
            last_target: None,
        };
        let event = CGEvent::new_mouse_event(
            None,
            CGEventType::LeftMouseDown,
            CGPoint::new(20., 30.),
            CGMouseButton::Left,
        )
        .unwrap();
        CGEvent::set_timestamp(Some(&event), 987654321000);
        provider.stamp(
            &event,
            &SkyLightTarget {
                pid: 42,
                window_id: 99,
                desktop: Point { x: 20., y: 30. },
                window_local: Point { x: 10., y: 10. },
            },
            7,
        );
        assert_eq!(CGEvent::timestamp(Some(&event)), 987654321000);
        assert_eq!(
            CGEvent::integer_value_field(Some(&event), CGEventField::MouseEventNumber),
            7
        );
        assert_eq!(
            CGEvent::integer_value_field(Some(&event), CGEventField::EventTargetUnixProcessID),
            42
        );
    }
    #[test]
    fn validation_preserves_negative_desktop_positions() {
        let mut target = SkyLightTarget {
            pid: 42,
            window_id: 99,
            desktop: Point { x: -500.0, y: 20.0 },
            window_local: Point { x: 10.0, y: 20.0 },
        };
        assert!(validate(&target).is_ok());
        target.window_local.x = f64::NAN;
        assert!(validate(&target).is_err());
        target.window_local.x = 1.0;
        target.window_id = 0;
        assert!(validate(&target).is_err());
    }
    #[test]
    fn pointer_state_keeps_window_local_mapping_and_provenance() {
        let target = SkyLightTarget {
            pid: 42,
            window_id: 99,
            desktop: Point { x: -500.0, y: 20.0 },
            window_local: Point { x: 10.0, y: 20.0 },
        };
        let moved = pointer_state(&target, Point { x: -470.0, y: 35.0 });
        assert_eq!((moved.pid, moved.window_id), (42, 99));
        assert_eq!((moved.desktop.x, moved.desktop.y), (-470.0, 35.0));
        assert_eq!((moved.window_local.x, moved.window_local.y), (40.0, 35.0));
    }
    #[test]
    fn rejected_target_does_not_change_virtual_pointer() {
        let Some(symbols) = symbols() else {
            return;
        };
        let target = SkyLightTarget {
            pid: 42,
            window_id: 99,
            desktop: Point { x: 1.0, y: 2.0 },
            window_local: Point { x: 1.0, y: 2.0 },
        };
        let mut provider = SkyLightInput {
            symbols,
            last_target: Some(target.clone()),
        };
        let mut invalid = target;
        invalid.pid = 0;
        assert!(
            provider
                .pointer_at(
                    &invalid,
                    SkyLightPointerAction::Move {
                        modifiers: Modifiers::default()
                    }
                )
                .is_err()
        );
        let point = provider.last_pointer().unwrap();
        assert_eq!((point.x, point.y), (1.0, 2.0));
    }
    #[test]
    fn gesture_groups_are_distinct() {
        assert_ne!(next_group(), next_group());
    }
}

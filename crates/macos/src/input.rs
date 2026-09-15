use crate::{Accessibility, error};
use objc2_core_foundation::{CFRetained, CGPoint};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventTapLocation, CGEventType, CGMouseButton,
    CGScrollEventUnit,
};
use std::time::Duration;
use unimation::*;

/// Public Quartz delivery. Does not activate apps or promise background consumption.
#[derive(Default)]
pub struct QuartzInput;
type Packet = (CFRetained<CGEvent>, Duration);
fn event(value: Option<CFRetained<CGEvent>>) -> Result<CFRetained<CGEvent>> {
    value.ok_or_else(|| {
        error(
            "event_creation",
            "Quartz could not allocate an event",
            Effect::None,
        )
    })
}
fn validate(delivery: &Delivery) -> Result<()> {
    if let Delivery::Process { pid } = delivery
        && *pid <= 0
    {
        return Err(error(
            "invalid_request",
            "Positive PID required",
            Effect::None,
        ));
    }
    if !Accessibility::is_trusted() {
        return Err(error(
            "permission_denied",
            "Accessibility access required",
            Effect::None,
        ));
    }
    Ok(())
}
fn flags(m: Modifiers) -> CGEventFlags {
    let mut f = CGEventFlags::empty();
    if m.shift {
        f |= CGEventFlags::MaskShift;
    }
    if m.control {
        f |= CGEventFlags::MaskControl;
    }
    if m.alt {
        f |= CGEventFlags::MaskAlternate;
    }
    if m.meta {
        f |= CGEventFlags::MaskCommand;
    }
    f
}
fn post(events: Vec<Packet>, delivery: Delivery) -> Receipt {
    for (event, delay) in &events {
        if !delay.is_zero() {
            std::thread::sleep(*delay);
        }
        match delivery {
            Delivery::Global {} => CGEvent::post(CGEventTapLocation::HIDEventTap, Some(event)),
            Delivery::Process { pid } => CGEvent::post_to_pid(pid, Some(event)),
        }
    }
    Receipt {
        effect: if events.is_empty() {
            Effect::None
        } else {
            Effect::Dispatched
        },
        route: match delivery {
            Delivery::Global {} => "macos.quartz.global",
            Delivery::Process { .. } => "macos.quartz.process",
        }
        .into(),
    }
}
fn finite(p: &Point) -> Result<()> {
    if p.x.is_finite() && p.y.is_finite() {
        Ok(())
    } else {
        Err(error(
            "invalid_request",
            "Finite Quartz desktop coordinates required",
            Effect::None,
        ))
    }
}
fn mouse_types(button: MouseButton) -> (CGMouseButton, CGEventType, CGEventType, CGEventType) {
    match button {
        MouseButton::Left => (
            CGMouseButton::Left,
            CGEventType::LeftMouseDown,
            CGEventType::LeftMouseUp,
            CGEventType::LeftMouseDragged,
        ),
        MouseButton::Right => (
            CGMouseButton::Right,
            CGEventType::RightMouseDown,
            CGEventType::RightMouseUp,
            CGEventType::RightMouseDragged,
        ),
        MouseButton::Middle => (
            CGMouseButton::Center,
            CGEventType::OtherMouseDown,
            CGEventType::OtherMouseUp,
            CGEventType::OtherMouseDragged,
        ),
    }
}
fn mouse(
    point: &Point,
    kind: CGEventType,
    button: CGMouseButton,
    count: i64,
    modifiers: Modifiers,
) -> Result<CFRetained<CGEvent>> {
    finite(point)?;
    let e = event(CGEvent::new_mouse_event(
        None,
        kind,
        CGPoint::new(point.x, point.y),
        button,
    ))?;
    CGEvent::set_integer_value_field(Some(&e), CGEventField::MouseEventClickState, count);
    CGEvent::set_flags(Some(&e), flags(modifiers));
    Ok(e)
}
impl PointerInput for QuartzInput {
    fn pointer(&mut self, delivery: Delivery, action: PointerAction) -> Result<Receipt> {
        validate(&delivery)?;
        let mut events = vec![];
        match action {
            PointerAction::Scroll {
                vertical,
                horizontal,
                point,
            } => {
                if let Some(p) = &point {
                    finite(p)?;
                }
                let e = event(CGEvent::new_scroll_wheel_event2(
                    None,
                    CGScrollEventUnit::Pixel,
                    2,
                    vertical,
                    horizontal,
                    0,
                ))?;
                if let Some(p) = point {
                    CGEvent::set_location(Some(&e), CGPoint::new(p.x, p.y));
                }
                events.push((e, Duration::ZERO));
            }
            PointerAction::Move { point } => events.push((
                mouse(
                    &point,
                    CGEventType::MouseMoved,
                    CGMouseButton::Left,
                    0,
                    Modifiers::default(),
                )?,
                Duration::ZERO,
            )),
            PointerAction::Click {
                point,
                button,
                count,
                modifiers,
            } => {
                if !(1..=3).contains(&count) {
                    return Err(error(
                        "invalid_request",
                        "Click count must be 1..3",
                        Effect::None,
                    ));
                }
                let (button, down, up, _) = mouse_types(button);
                for n in 1..=count {
                    events.push((
                        mouse(&point, down, button, n.into(), modifiers)?,
                        if n == 1 {
                            Duration::ZERO
                        } else {
                            Duration::from_millis(50)
                        },
                    ));
                    events.push((
                        mouse(&point, up, button, n.into(), modifiers)?,
                        Duration::from_millis(10),
                    ));
                }
            }
            PointerAction::Drag {
                from,
                to,
                button,
                modifiers,
                duration_ms,
            } => {
                finite(&from)?;
                finite(&to)?;
                if !(1..=10_000).contains(&duration_ms) {
                    return Err(error(
                        "invalid_request",
                        "Drag duration must be 1..10000 ms",
                        Effect::None,
                    ));
                }
                let (button, down, up, dragged) = mouse_types(button);
                events.push((mouse(&from, down, button, 1, modifiers)?, Duration::ZERO));
                let steps = (duration_ms / 10).clamp(1, 100);
                for step in 1..=steps {
                    let t = step as f64 / steps as f64;
                    let point = Point {
                        x: from.x + (to.x - from.x) * t,
                        y: from.y + (to.y - from.y) * t,
                    };
                    events.push((
                        mouse(&point, dragged, button, 1, modifiers)?,
                        Duration::from_millis(duration_ms / steps),
                    ));
                }
                events.push((mouse(&to, up, button, 1, modifiers)?, Duration::ZERO));
            }
        }
        // Every packet is validated and allocated before the first button-down.
        Ok(post(events, delivery))
    }
}
impl TextInput for QuartzInput {
    fn type_text(&mut self, delivery: Delivery, text: &str) -> Result<Receipt> {
        validate(&delivery)?;
        let mut events = vec![];
        for scalar in text.chars() {
            let mut buffer = [0; 2];
            let units = scalar.encode_utf16(&mut buffer);
            for down in [true, false] {
                let e = event(CGEvent::new_keyboard_event(None, 0, down))?;
                CGEvent::set_flags(Some(&e), CGEventFlags::empty());
                // SAFETY: valid UTF-16 slice remains alive during the copy; surrogate pairs stay together.
                unsafe {
                    CGEvent::keyboard_set_unicode_string(
                        Some(&e),
                        units.len() as _,
                        units.as_ptr(),
                    );
                }
                events.push((e, Duration::ZERO));
            }
        }
        Ok(post(events, delivery))
    }
}
impl KeyboardInput for QuartzInput {
    type Key = KeyChord;
    fn key_press(&mut self, delivery: Delivery, key: KeyChord) -> Result<Receipt> {
        validate(&delivery)?;
        if key.key_code > 127 {
            return Err(error(
                "invalid_request",
                "macOS virtual key codes must be <=127",
                Effect::None,
            ));
        }
        let mut events = vec![];
        for down in [true, false] {
            let e = event(CGEvent::new_keyboard_event(None, key.key_code, down))?;
            CGEvent::set_flags(Some(&e), flags(key.modifiers));
            events.push((e, Duration::ZERO));
        }
        Ok(post(events, delivery))
    }
}

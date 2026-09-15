use crate::{Accessibility, error};
use objc2_core_foundation::{CFRetained, CGPoint};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventTapLocation, CGEventType, CGMouseButton, CGScrollEventUnit,
};
use unimation_core::*;

/// Public Quartz delivery. Does not activate apps or promise background consumption.
#[derive(Default)]
pub struct QuartzInput;
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
fn post(events: Vec<CFRetained<CGEvent>>, delivery: Delivery) -> Receipt {
    for event in &events {
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
impl PointerInput for QuartzInput {
    fn pointer(&mut self, delivery: Delivery, action: PointerAction) -> Result<Receipt> {
        validate(&delivery)?;
        let events = match action {
            PointerAction::Scroll {
                vertical,
                horizontal,
            } => vec![event(CGEvent::new_scroll_wheel_event2(
                None,
                CGScrollEventUnit::Pixel,
                2,
                vertical,
                horizontal,
                0,
            ))?],
            PointerAction::Move { ref point } | PointerAction::Click { ref point } => {
                if !point.x.is_finite() || !point.y.is_finite() {
                    return Err(error(
                        "invalid_request",
                        "Finite Quartz desktop coordinates required",
                        Effect::None,
                    ));
                }
                let types: &[CGEventType] = if matches!(action, PointerAction::Move { .. }) {
                    &[CGEventType::MouseMoved]
                } else {
                    &[CGEventType::LeftMouseDown, CGEventType::LeftMouseUp]
                };
                let mut events = vec![];
                for kind in types {
                    let e = event(CGEvent::new_mouse_event(
                        None,
                        *kind,
                        CGPoint::new(point.x, point.y),
                        CGMouseButton::Left,
                    ))?;
                    if *kind != CGEventType::MouseMoved {
                        CGEvent::set_integer_value_field(
                            Some(&e),
                            CGEventField::MouseEventClickState,
                            1,
                        );
                    }
                    events.push(e);
                }
                events
            }
        };
        Ok(post(events, delivery))
    }
}
impl TextInput for QuartzInput {
    fn type_text(&mut self, delivery: Delivery, text: &str) -> Result<Receipt> {
        validate(&delivery)?;
        let mut events = vec![];
        // Allocate the whole sequence before dispatch. Never split a surrogate pair.
        // Unicode packets do not claim hardware-key or IME equivalence.
        for scalar in text.chars() {
            let mut buffer = [0; 2];
            let units = scalar.encode_utf16(&mut buffer);
            for down in [true, false] {
                let e = event(CGEvent::new_keyboard_event(None, 0, down))?;
                // SAFETY: the slice has the supplied UTF-16 length and remains alive during the copy.
                unsafe {
                    CGEvent::keyboard_set_unicode_string(
                        Some(&e),
                        units.len() as _,
                        units.as_ptr(),
                    );
                }
                events.push(e);
            }
        }
        Ok(post(events, delivery))
    }
}

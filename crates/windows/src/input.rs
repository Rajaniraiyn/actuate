use std::time::{Duration, Instant};
use unimation::*;
use windows_api::Win32::UI::Input::KeyboardAndMouse::*;

/// Explicit shared-desktop input. It never activates a window or falls back from UIA.
#[derive(Default)]
pub struct GlobalInput;
fn global(delivery: Delivery) -> Result<()> {
    match delivery {
        Delivery::Global {} => Ok(()),
        _ => Err(super::error(
            "unsupported_delivery",
            "SendInput only supports explicit global delivery",
        )),
    }
}
fn send(events: &[INPUT]) -> Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    let count = unsafe { SendInput(events, std::mem::size_of::<INPUT>() as i32) };
    if count as usize != events.len() {
        return Err(NativeError {
            code: "input_incomplete".into(),
            message: format!(
                "SendInput accepted {count}/{} events. UIPI can block input to higher-integrity applications; no fallback was attempted",
                events.len()
            ),
            effect: Effect::Unknown,
        });
    }
    Ok(())
}
fn mouse(flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32, data: u32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: data,
                dwFlags: flags,
                ..Default::default()
            },
        },
    }
}
fn key(code: u16, release: bool) -> INPUT {
    let mut flags = if release {
        KEYEVENTF_KEYUP
    } else {
        KEYBD_EVENT_FLAGS(0)
    };
    // Navigation and right-hand modifier virtual keys require the extended flag.
    if matches!(code, 0x21..=0x28 | 0x2D | 0x2E | 0x5B | 0x5C | 0xA3 | 0xA5) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(code),
                dwFlags: flags,
                ..Default::default()
            },
        },
    }
}
fn modifiers(value: Modifiers) -> Vec<u16> {
    [
        (value.control, VK_CONTROL.0),
        (value.alt, VK_MENU.0),
        (value.shift, VK_SHIFT.0),
        (value.meta, VK_LWIN.0),
    ]
    .into_iter()
    .filter_map(|(enabled, key)| enabled.then_some(key))
    .collect()
}
fn ensure_released(keys: &[u16]) -> Result<()> {
    let shared_keys = [
        VK_CONTROL.0,
        VK_SHIFT.0,
        VK_MENU.0,
        VK_LWIN.0,
        VK_RWIN.0,
        VK_LBUTTON.0,
        VK_RBUTTON.0,
        VK_MBUTTON.0,
        VK_XBUTTON1.0,
        VK_XBUTTON2.0,
    ];
    if keys
        .iter()
        .copied()
        .chain(shared_keys)
        .any(|key| unsafe { GetAsyncKeyState(i32::from(key)) } < 0)
    {
        return Err(super::error(
            "shared_input_busy",
            "A key or mouse button is already held; no input was sent",
        ));
    }
    Ok(())
}
fn normalized(point: &Point, desktop: &super::windows::Bounds) -> Result<(i32, i32)> {
    let x = point.x - f64::from(desktop.x);
    let y = point.y - f64::from(desktop.y);
    if !x.is_finite()
        || !y.is_finite()
        || x < 0.0
        || y < 0.0
        || x >= f64::from(desktop.width)
        || y >= f64::from(desktop.height)
    {
        return Err(super::error(
            "invalid_coordinate",
            "Point must be finite and inside the physical virtual desktop",
        ));
    }
    // Aim at the physical pixel center, including negative monitor origins.
    Ok((
        ((x.floor() + 0.5) * 65536.0 / f64::from(desktop.width))
            .floor()
            .min(65535.0) as i32,
        ((y.floor() + 0.5) * 65536.0 / f64::from(desktop.height))
            .floor()
            .min(65535.0) as i32,
    ))
}
fn move_to(point: &Point, desktop: &super::windows::Bounds) -> Result<INPUT> {
    let (x, y) = normalized(point, desktop)?;
    use windows_api::Win32::{
        Foundation::POINT,
        Graphics::Gdi::{MONITOR_DEFAULTTONULL, MonitorFromPoint},
    };
    if unsafe {
        MonitorFromPoint(
            POINT {
                x: point.x.floor() as i32,
                y: point.y.floor() as i32,
            },
            MONITOR_DEFAULTTONULL,
        )
    }
    .0
    .is_null()
    {
        return Err(super::error(
            "invalid_coordinate",
            "Point is in a gap between monitors",
        ));
    }
    Ok(mouse(
        MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
        x,
        y,
        0,
    ))
}
fn button(value: MouseButton) -> (MOUSE_EVENT_FLAGS, MOUSE_EVENT_FLAGS, u16) {
    match value {
        MouseButton::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, VK_LBUTTON.0),
        MouseButton::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, VK_RBUTTON.0),
        MouseButton::Middle => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, VK_MBUTTON.0),
    }
}
/// Always attempts release after partial dispatch. It does not restore shared cursor/focus.
struct Release(Vec<INPUT>);
impl Drop for Release {
    fn drop(&mut self) {
        let _ = send(&self.0);
    }
}
/// Explicit Win32 wheel units; positive vertical moves down, horizontal moves right.
/// One detent becomes WHEEL_DELTA native units. No window activation occurs.
impl GlobalInput {
    pub fn wheel(
        &mut self,
        delivery: Delivery,
        vertical: i32,
        horizontal: i32,
        point: Option<Point>,
    ) -> Result<Receipt> {
        global(delivery)?;
        let (vertical, horizontal) = wheel_deltas(vertical, horizontal)?;
        let _dpi = super::windows::DpiGuard::new()?;
        let mut events = Vec::new();
        if let Some(point) = point {
            events.push(move_to(&point, &super::virtual_desktop()?)?);
        }
        if vertical != 0 {
            events.push(mouse(MOUSEEVENTF_WHEEL, 0, 0, vertical as u32));
        }
        if horizontal != 0 {
            events.push(mouse(MOUSEEVENTF_HWHEEL, 0, 0, horizontal as u32));
        }
        if events.is_empty() {
            return Ok(Receipt {
                effect: Effect::None,
                route: "windows_send_input_wheel".into(),
            });
        }
        ensure_released(&[])?;
        send(&events)?;
        Ok(super::dispatched("windows_send_input_wheel"))
    }
}
fn wheel_deltas(vertical: i32, horizontal: i32) -> Result<(i32, i32)> {
    if !(-120..=120).contains(&vertical) || !(-120..=120).contains(&horizontal) {
        return Err(super::error(
            "invalid_wheel_detents",
            "Each axis must be between -120 and 120 detents",
        ));
    }
    let vertical = vertical
        .checked_mul(-120)
        .ok_or_else(|| super::error("invalid_wheel_detents", "Vertical wheel delta overflow"))?;
    let horizontal = horizontal
        .checked_mul(120)
        .ok_or_else(|| super::error("invalid_wheel_detents", "Horizontal wheel delta overflow"))?;
    Ok((vertical, horizontal))
}
impl PointerInput for GlobalInput {
    fn pointer(&mut self, delivery: Delivery, action: PointerAction) -> Result<Receipt> {
        global(delivery)?;
        let _dpi = super::windows::DpiGuard::new()?;
        let desktop = super::virtual_desktop()?;
        let mut events = vec![];
        let mut release = vec![];
        let mut scheduled = vec![];
        let mut held = vec![];
        match action {
            PointerAction::Move { point } => events.push(move_to(&point, &desktop)?),
            PointerAction::Scroll { .. } => {
                return Err(super::error(
                    "unsupported_scroll_units",
                    "Portable pixel scrolling is not mapped to wheel ticks; native wheel support is a separate capability",
                ));
            }
            PointerAction::Click {
                point,
                button: which,
                count,
                modifiers: mods,
            } => {
                if !(1..=3).contains(&count) {
                    return Err(super::error(
                        "invalid_click_count",
                        "Click count must be 1..3",
                    ));
                }
                events.push(move_to(&point, &desktop)?);
                let (down, up, vk) = button(which);
                held = modifiers(mods);
                held.push(vk);
                events.extend(modifiers(mods).into_iter().map(|k| key(k, false)));
                release.push(mouse(up, 0, 0, 0));
                release.extend(modifiers(mods).into_iter().rev().map(|k| key(k, true)));
                for _ in 0..count {
                    events.extend([mouse(down, 0, 0, 0), mouse(up, 0, 0, 0)]);
                }
            }
            PointerAction::Drag {
                from,
                to,
                button: which,
                modifiers: mods,
                duration_ms,
            } => {
                if !(16..=30_000).contains(&duration_ms) {
                    return Err(super::error(
                        "invalid_duration",
                        "Drag duration must be 16..30000 milliseconds",
                    ));
                }
                let start = move_to(&from, &desktop)?;
                move_to(&to, &desktop)?;
                let (down, up, vk) = button(which);
                held = modifiers(mods);
                held.push(vk);
                events.push(start);
                events.extend(modifiers(mods).into_iter().map(|k| key(k, false)));
                events.push(mouse(down, 0, 0, 0));
                release.push(mouse(up, 0, 0, 0));
                release.extend(modifiers(mods).into_iter().rev().map(|k| key(k, true)));
                let steps = duration_ms.div_ceil(8);
                for step in 1..=steps {
                    let t = step as f64 / steps as f64;
                    let t = t * t * (3.0 - 2.0 * t);
                    let point = Point {
                        x: from.x + (to.x - from.x) * t,
                        y: from.y + (to.y - from.y) * t,
                    };
                    scheduled.push((
                        Duration::from_millis(duration_ms * step / steps),
                        move_to(&point, &desktop)?,
                    ));
                }
            }
        }
        ensure_released(&held)?;
        let mut guard = Release(release);
        send(&events)?;
        let start = Instant::now();
        for (deadline, event) in scheduled {
            std::thread::sleep(deadline.saturating_sub(start.elapsed()));
            send(&[event])?;
        }
        send(&guard.0)?;
        guard.0.clear();
        Ok(super::dispatched("windows_send_input"))
    }
}
impl TextInput for GlobalInput {
    fn type_text(&mut self, delivery: Delivery, text: &str) -> Result<Receipt> {
        global(delivery)?;
        if text.is_empty() {
            return Ok(Receipt {
                effect: Effect::None,
                route: "windows_unicode_input".into(),
            });
        }
        if text.len() > 65_536 {
            return Err(super::error(
                "text_too_large",
                "Text must be at most 65536 bytes",
            ));
        }
        ensure_released(&[])?;
        let events: Vec<_> = text
            .encode_utf16()
            .flat_map(|unit| {
                [false, true].map(|up| INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wScan: unit,
                            dwFlags: KEYEVENTF_UNICODE
                                | if up {
                                    KEYEVENTF_KEYUP
                                } else {
                                    KEYBD_EVENT_FLAGS(0)
                                },
                            ..Default::default()
                        },
                    },
                })
            })
            .collect();
        if let Err(e) = send(&events) {
            let releases: Vec<_> = events
                .into_iter()
                .enumerate()
                .filter_map(|(i, e)| (i % 2 == 1).then_some(e))
                .collect();
            let _ = send(&releases);
            return Err(e);
        }
        Ok(super::dispatched("windows_unicode_input"))
    }
}
impl KeyboardInput for GlobalInput {
    type Key = KeyChord;
    fn key_press(&mut self, delivery: Delivery, chord: KeyChord) -> Result<Receipt> {
        global(delivery)?;
        if chord.key_code == 0 || chord.key_code > 254 {
            return Err(super::error(
                "invalid_key",
                "Use a Win32 virtual-key code in 1..254",
            ));
        }
        let mut codes = modifiers(chord.modifiers);
        if !codes.contains(&chord.key_code) {
            codes.push(chord.key_code);
        }
        ensure_released(&codes)?;
        let mut guard = Release(codes.iter().rev().map(|k| key(*k, true)).collect());
        let mut events: Vec<_> = codes.iter().map(|k| key(*k, false)).collect();
        events.extend_from_slice(&guard.0);
        send(&events)?;
        guard.0.clear();
        Ok(super::dispatched("windows_send_input"))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wheel_uses_declared_down_and_right_signs() {
        assert_eq!(wheel_deltas(1, 1).unwrap(), (-120, 120));
        assert_eq!(wheel_deltas(-120, 120).unwrap(), (14400, 14400));
        assert!(wheel_deltas(i32::MIN, 0).is_err());
        assert!(wheel_deltas(0, 121).is_err());
    }
    #[test]
    fn negative_monitor_origins_map_to_pixel_centers() {
        let desktop = super::super::windows::Bounds {
            x: -1920,
            y: -200,
            width: 3840,
            height: 1280,
        };
        assert_eq!(
            normalized(
                &Point {
                    x: -1920.0,
                    y: -200.0
                },
                &desktop
            )
            .unwrap(),
            (8, 25)
        );
        assert!(normalized(&Point { x: 1920.0, y: 0.0 }, &desktop).is_err());
        assert!(
            normalized(
                &Point {
                    x: f64::NAN,
                    y: 0.0
                },
                &desktop
            )
            .is_err()
        );
    }
}

use crate::{ElementRef, NativeError, Point, PointerAction, Result};
use serde::{Deserialize, Serialize};

fn finite(point: &Point) -> Result<()> {
    if point.x.is_finite() && point.y.is_finite() {
        Ok(())
    } else {
        Err(NativeError::invalid_request(
            "Finite desktop coordinates required",
        ))
    }
}
impl PointerAction {
    /// Shared bounds every pointer route enforces before allocating events:
    /// finite points, 1..=3 clicks and 1..=10000 ms drags.
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Move { point } => finite(point),
            Self::Click { point, count, .. } => {
                finite(point)?;
                if !(1..=3).contains(count) {
                    return Err(NativeError::invalid_request("Click count must be 1..3"));
                }
                Ok(())
            }
            Self::Scroll { point, .. } => point.as_ref().map_or(Ok(()), finite),
            Self::Drag {
                from,
                to,
                duration_ms,
                ..
            } => {
                finite(from)?;
                finite(to)?;
                if !(1..=10_000).contains(duration_ms) {
                    return Err(NativeError::invalid_request(
                        "Drag duration must be 1..10000 ms",
                    ));
                }
                Ok(())
            }
        }
    }
    /// The same action in another coordinate space.
    pub fn map_points(self, f: impl Fn(Point) -> Point) -> Self {
        match self {
            Self::Move { point } => Self::Move { point: f(point) },
            Self::Click {
                point,
                button,
                count,
                modifiers,
            } => Self::Click {
                point: f(point),
                button,
                count,
                modifiers,
            },
            Self::Scroll {
                vertical,
                horizontal,
                point,
            } => Self::Scroll {
                vertical,
                horizontal,
                point: point.map(f),
            },
            Self::Drag {
                from,
                to,
                button,
                modifiers,
                duration_ms,
            } => Self::Drag {
                from: f(from),
                to: f(to),
                button,
                modifiers,
                duration_ms,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    #[default]
    Left,
    Right,
    Middle,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub meta: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyChord {
    /// Native virtual key code, not a Unicode scalar or assumed keyboard layout.
    pub key_code: u16,
    #[serde(default)]
    pub modifiers: Modifiers,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClickMode {
    Semantic,
    Global,
    Process,
    Skylight,
}
impl ClickMode {
    /// Semantic activation needs a reference and has no button, count or
    /// modifier meaning; pointer routes carry those instead.
    pub fn require_semantic_target(
        target: Option<&ElementRef>,
        button: MouseButton,
        count: u8,
        modifiers: Modifiers,
    ) -> Result<&ElementRef> {
        if button != MouseButton::Left || count != 1 || modifiers != Modifiers::default() {
            return Err(NativeError::unsupported(
                "Semantic activation has no mouse button/count/modifier semantics",
            ));
        }
        target
            .ok_or_else(|| NativeError::invalid_request("Semantic activation requires a reference"))
    }
}
pub const fn one_click() -> u8 {
    1
}
pub const fn default_drag_duration() -> u64 {
    250
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AttributeParameter {
    String { value: String },
    Integer { value: i64 },
    Range { location: i64, length: i64 },
    Point { value: crate::Point },
}

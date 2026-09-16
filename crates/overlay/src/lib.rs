use serde::{Deserialize, Serialize};
use unimation::{Effect, NativeError, Result};

mod controller;
pub mod idle;
pub mod motion;
pub mod shape;
pub use controller::OverlayController;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum CursorCommand {
    Move {
        x: f64,
        y: f64,
        #[serde(default = "duration")]
        duration_ms: u64,
    },
    Click {
        x: f64,
        y: f64,
    },
    Configure {
        appearance: CursorAppearance,
    },
    Scope {
        scope: CursorScope,
    },
    Hide,
    Show,
    Quit,
}
/// Where the compositor may present the cursor. Window identity includes its
/// owner to reject stale IDs belonging to a different process.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CursorScope {
    #[default]
    Desktop,
    Window {
        window_id: u64,
        pid: i32,
    },
}
/// Portable visual preferences, independent of AppKit or input delivery.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CursorAppearance {
    pub scale: f64,
    pub color: [f64; 3],
    pub motion: motion::MotionStyle,
    pub idle: idle::IdleAppearance,
}
impl Default for CursorAppearance {
    fn default() -> Self {
        Self {
            scale: 1.,
            color: [0.16, 0.18, 0.22],
            motion: Default::default(),
            idle: Default::default(),
        }
    }
}
impl CursorAppearance {
    pub fn is_valid(&self) -> bool {
        self.idle.is_valid()
            && self.scale.is_finite()
            && (0.5..=3.).contains(&self.scale)
            && self
                .color
                .iter()
                .all(|c| c.is_finite() && (0. ..=1.).contains(c))
    }
}
fn duration() -> u64 {
    250
}

impl CursorCommand {
    pub fn validate(&self) -> Result<()> {
        let invalid = match self {
            Self::Move { x, y, duration_ms } => {
                !x.is_finite() || !y.is_finite() || *duration_ms > 10000
            }
            Self::Click { x, y } => !x.is_finite() || !y.is_finite(),
            Self::Configure { appearance } => !appearance.is_valid(),
            Self::Scope {
                scope: CursorScope::Window { window_id, pid },
            } => *window_id == 0 || *pid <= 0,
            _ => false,
        };
        if invalid {
            Err(NativeError {
                code: "invalid_cursor_command".into(),
                message: "Coordinates must be finite, duration_ms <= 10000, scale 0.5..=3, and RGB components 0..=1, idle amplitude 0..=2 and period_ms 800..=10000".into(),
                effect: Effect::None,
            })
        } else {
            Ok(())
        }
    }
}

/// Queued is a local transport acknowledgement. It does not prove that the
/// helper has read the command or that a compositor displayed a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorAcknowledgement {
    Queued,
    /// Reserved for renderers that can actually acknowledge presentation.
    Rendered,
}

macro_rules! unavailable_renderer {
    ($module:ident, $platform:literal) => {
        pub mod $module {
            /// Placeholder with no drawing or input side effects.
            pub struct UnavailableRenderer;
            impl unimation::CursorVisualization for UnavailableRenderer {
                type Command = super::CursorCommand;
                type Status = super::CursorAcknowledgement;
                fn visualize(&mut self, command: Self::Command) -> unimation::Result<Self::Status> {
                    command.validate()?;
                    Err(unimation::NativeError {
                        code: "cursor_renderer_unavailable".into(),
                        message: concat!($platform, " cursor renderer is not implemented").into(),
                        effect: unimation::Effect::None,
                    })
                }
            }
        }
    };
}
unavailable_renderer!(ios, "iOS/iPadOS");
unavailable_renderer!(android, "Android");

#[cfg(test)]
mod tests {
    use super::*;
    use unimation::CursorVisualization;
    #[test]
    fn command_roundtrip_and_validation() {
        let command: CursorCommand =
            serde_json::from_str(r#"{"op":"move","x":-10,"y":20}"#).unwrap();
        assert_eq!(
            command,
            CursorCommand::Move {
                x: -10.,
                y: 20.,
                duration_ms: 250
            }
        );
        assert!(
            CursorCommand::Click { x: f64::NAN, y: 0. }
                .validate()
                .is_err()
        );
        assert!(
            CursorCommand::Move {
                x: 0.,
                y: 0.,
                duration_ms: 10001
            }
            .validate()
            .is_err()
        );
        assert_eq!(
            serde_json::to_value(CursorCommand::Hide).unwrap(),
            serde_json::json!({"op":"hide"})
        );
    }
    #[test]
    fn scope_preserves_wide_native_ids_and_rejects_invalid_owners() {
        let scope = CursorScope::Window {
            window_id: u64::from(u32::MAX) + 1,
            pid: 42,
        };
        let command = CursorCommand::Scope { scope };
        assert!(command.validate().is_ok());
        assert_eq!(
            serde_json::from_value::<CursorCommand>(serde_json::to_value(&command).unwrap())
                .unwrap(),
            command
        );
        for scope in [
            CursorScope::Window {
                window_id: 0,
                pid: 42,
            },
            CursorScope::Window {
                window_id: 1,
                pid: 0,
            },
        ] {
            assert!(CursorCommand::Scope { scope }.validate().is_err());
        }
    }
    #[test]
    fn appearance_is_bounded_and_reduced_motion_is_typed() {
        let command: CursorCommand = serde_json::from_str(
            r#"{"op":"configure","appearance":{"motion":"reduced","scale":1.5}}"#,
        )
        .unwrap();
        assert!(command.validate().is_ok());
        for scale in [0., 3.1, f64::NAN] {
            assert!(
                CursorCommand::Configure {
                    appearance: CursorAppearance {
                        scale,
                        ..Default::default()
                    }
                }
                .validate()
                .is_err()
            );
        }
        assert!(
            CursorCommand::Configure {
                appearance: CursorAppearance {
                    color: [1.1, 0., 0.],
                    ..Default::default()
                }
            }
            .validate()
            .is_err()
        );
        assert!(
            serde_json::from_str::<CursorCommand>(
                r#"{"op":"configure","appearance":{"motion":"magic"}}"#
            )
            .is_err()
        );
    }
    #[test]
    fn unavailable_renderer_does_not_acknowledge_a_frame() {
        let error = ios::UnavailableRenderer
            .visualize(CursorCommand::Show)
            .unwrap_err();
        assert_eq!(error.code, "cursor_renderer_unavailable");
        assert!(matches!(error.effect, Effect::None));
    }
}

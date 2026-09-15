use serde::{Deserialize, Serialize};
use unimation::{Effect, NativeError, Result};

mod controller;
pub mod motion;
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
    Hide,
    Show,
    Quit,
}
/// Portable visual preferences, independent of AppKit or input delivery.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CursorAppearance {
    pub scale: f64,
    pub color: [f64; 3],
    pub motion: motion::MotionStyle,
}
impl Default for CursorAppearance {
    fn default() -> Self {
        Self {
            scale: 1.,
            color: [0.46, 0.28, 0.95],
            motion: Default::default(),
        }
    }
}
impl CursorAppearance {
    pub fn is_valid(&self) -> bool {
        self.scale.is_finite()
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
pub fn interpolate(start: (f64, f64), end: (f64, f64), t: f64) -> (f64, f64) {
    let t = t.clamp(0., 1.);
    let u = 1. - t;
    // Repeated endpoint controls produce a cubic Bézier with zero endpoint velocity.
    let k = 3. * u * t * t + t * t * t;
    (
        start.0 + (end.0 - start.0) * k,
        start.1 + (end.1 - start.1) * k,
    )
}

impl CursorCommand {
    pub fn validate(&self) -> Result<()> {
        let invalid = match self {
            Self::Move { x, y, duration_ms } => {
                !x.is_finite() || !y.is_finite() || *duration_ms > 10000
            }
            Self::Click { x, y } => !x.is_finite() || !y.is_finite(),
            Self::Configure { appearance } => !appearance.is_valid(),
            _ => false,
        };
        if invalid {
            Err(NativeError {
                code: "invalid_cursor_command".into(),
                message: "Coordinates must be finite, duration_ms <= 10000, scale 0.5..=3, and RGB components 0..=1".into(),
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
unavailable_renderer!(linux, "Linux");
unavailable_renderer!(windows, "Windows");
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
        let error = linux::UnavailableRenderer
            .visualize(CursorCommand::Show)
            .unwrap_err();
        assert_eq!(error.code, "cursor_renderer_unavailable");
        assert!(matches!(error.effect, Effect::None));
    }
    #[test]
    fn bezier_endpoints_and_clamping() {
        assert_eq!(interpolate((0., 0.), (100., 50.), -1.), (0., 0.));
        assert_eq!(interpolate((0., 0.), (100., 50.), 2.), (100., 50.));
        assert_eq!(interpolate((0., 0.), (100., 50.), 0.5), (50., 25.));
    }
}

//! Decorative offsets only. Never feed these values into input coordinates.
use serde::{Deserialize, Serialize};
use unimation::motion::MotionStyle;

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdleStyle {
    Off,
    #[default]
    Bob,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IdleAppearance {
    pub style: IdleStyle,
    /// Logical points, independent of the glyph scale and screen pixel density.
    pub amplitude: f64,
    pub period_ms: u64,
}
impl Default for IdleAppearance {
    fn default() -> Self {
        Self {
            style: IdleStyle::Bob,
            amplitude: 0.65,
            period_ms: 2400,
        }
    }
}
impl IdleAppearance {
    pub fn is_valid(self) -> bool {
        self.amplitude.is_finite()
            && (0. ..=2.).contains(&self.amplitude)
            && (800..=10000).contains(&self.period_ms)
    }
    /// Wait 600 ms after activity, then fade in over a second. Reduced motion
    /// and disabled idle styling return exact zero regardless of the clock.
    pub fn offset(self, idle_seconds: f64, motion: MotionStyle) -> (f64, f64) {
        if !self.is_valid()
            || !idle_seconds.is_finite()
            || idle_seconds <= 0.6
            || self.style == IdleStyle::Off
            || motion == MotionStyle::Reduced
        {
            return (0., 0.);
        }
        let elapsed = idle_seconds - 0.6;
        let fade = elapsed.min(1.);
        let fade = fade * fade * (3. - 2. * fade);
        let phase = std::f64::consts::TAU * (elapsed % (self.period_ms as f64 / 1000.))
            / (self.period_ms as f64 / 1000.);
        (0., self.amplitude * fade * phase.sin())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn idle_stays_bounded_and_reduced_motion_is_stationary() {
        let idle = IdleAppearance::default();
        for tick in 0..2000 {
            let time = tick as f64 / 60.;
            let offset = idle.offset(time, MotionStyle::Curved);
            assert_eq!(offset.0, 0.);
            assert!(offset.1.abs() <= idle.amplitude);
            assert_eq!(idle.offset(time, MotionStyle::Reduced), (0., 0.));
        }
        assert_eq!(idle.offset(0.6, MotionStyle::Curved), (0., 0.));
        assert_eq!(idle.offset(f64::NAN, MotionStyle::Curved), (0., 0.));
        assert_eq!(
            IdleAppearance {
                style: IdleStyle::Off,
                ..idle
            }
            .offset(3., MotionStyle::Curved),
            (0., 0.)
        );
    }
    #[test]
    fn rejects_large_or_nonfinite_ornament() {
        assert!(
            !IdleAppearance {
                amplitude: 2.1,
                ..Default::default()
            }
            .is_valid()
        );
        assert!(
            !IdleAppearance {
                amplitude: f64::INFINITY,
                ..Default::default()
            }
            .is_valid()
        );
        assert!(
            !IdleAppearance {
                period_ms: 0,
                ..Default::default()
            }
            .is_valid()
        );
    }
}

//! Shared click feedback in logical points, independent of input delivery.
use actuate::motion::MotionStyle;

pub const DURATION_SECONDS: f64 = 0.45;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ripple {
    pub radius: f64,
    pub width: f64,
    pub alpha: f64,
}

/// Normalized lifetime. Reduced motion suppresses decorative click animation.
pub fn sample(progress: f64, motion: MotionStyle) -> Option<Ripple> {
    if !progress.is_finite() || !(0.0..1.0).contains(&progress) || motion == MotionStyle::Reduced {
        return None;
    }
    let eased = 1.0 - (1.0 - progress).powi(3);
    Some(Ripple {
        radius: 5.0 + 17.0 * eased,
        width: 2.0 - eased,
        alpha: 0.65 * (1.0 - progress).powi(2),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_and_fades_with_bounded_geometry() {
        let mut previous = sample(0.0, MotionStyle::Curved).unwrap();
        for tick in 1..100 {
            let next = sample(tick as f64 / 100.0, MotionStyle::Curved).unwrap();
            assert!(next.radius > previous.radius && next.radius <= 22.0);
            assert!(next.alpha < previous.alpha && next.alpha >= 0.0);
            assert!((1.0..=2.0).contains(&next.width));
            previous = next;
        }
        for progress in [-1.0, 1.0, f64::NAN, f64::INFINITY] {
            assert!(sample(progress, MotionStyle::Curved).is_none());
        }
        assert!(sample(0.5, MotionStyle::Reduced).is_none());
    }
}

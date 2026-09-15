//! Deterministic visual paths. Input providers never consume these samples.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionStyle {
    Straight,
    #[default]
    Curved,
    Reduced,
}
/// A cubic spatial curve with a minimum-jerk time profile. Curvature is bounded
/// to 24 logical points and endpoints are exact. No randomness or overshoot.
pub fn sample(start: (f64, f64), end: (f64, f64), progress: f64, style: MotionStyle) -> (f64, f64) {
    if style == MotionStyle::Reduced || progress >= 1. {
        return end;
    }
    if progress <= 0. {
        return start;
    }
    let t = progress.clamp(0., 1.);
    let t = t * t * t * (10. + t * (-15. + 6. * t));
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let distance = dx.hypot(dy);
    let bend = if style == MotionStyle::Curved {
        (distance * 0.08).min(24.)
    } else {
        0.
    };
    let (nx, ny) = if distance > 0. {
        (-dy / distance, dx / distance)
    } else {
        (0., 0.)
    };
    let arc = 3. * (1. - t) * t * bend;
    (start.0 + dx * t + nx * arc, start.1 + dy * t + ny * arc)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn curves_are_bounded_and_finish_exactly() {
        let a = (-100., 50.);
        let b = (900., 50.);
        assert_eq!(sample(a, b, 0., MotionStyle::Curved), a);
        assert_eq!(sample(a, b, 1., MotionStyle::Curved), b);
        for step in 0..=100 {
            let (x, y) = sample(a, b, step as f64 / 100., MotionStyle::Curved);
            assert!((-100. ..=900.).contains(&x));
            assert!((50. ..=68.).contains(&y));
        }
        assert_eq!(sample(a, b, 0., MotionStyle::Reduced), b);
        assert_eq!(sample(a, a, 0.5, MotionStyle::Curved), a);
        assert_eq!(sample(a, b, 0.5, MotionStyle::Straight), (400., 50.));
    }
}

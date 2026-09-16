//! Deterministic motion in caller-selected units. Plans do not dispatch input,
//! sleep, infer screen coordinates, or animate idle cursor decoration.
use crate::{NativeError, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionStyle {
    Straight,
    #[default]
    Curved,
    Reduced,
}

/// Minimum-jerk timing with a cubic spatial arc bounded to 18 caller units.
/// Endpoints are exact. Callers of this primitive must supply finite points and
/// progress; use `MotionPlan` when those values come from untrusted input.
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionSample {
    /// Offset from dispatch start, not a delay from the preceding sample.
    pub at: Duration,
    pub position: (f64, f64),
}

#[derive(Debug, Clone)]
pub struct MotionPlan {
    start: (f64, f64),
    end: (f64, f64),
    samples: Vec<MotionSample>,
}
impl MotionPlan {
    /// Includes the start at zero and the exact endpoint at `duration`.
    /// Reduced motion contains only the endpoint at zero. Coordinates may be
    /// negative; the caller validates its own display or input bounds.
    pub fn new(
        start: (f64, f64),
        end: (f64, f64),
        duration: Duration,
        steps: usize,
        style: MotionStyle,
    ) -> Result<Self> {
        validate(duration, steps)?;
        if [start.0, start.1, end.0, end.1]
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 1e9)
        {
            return Err(invalid(
                "coordinates must be finite and within +/-1e9 caller units",
            ));
        }
        let samples = if style == MotionStyle::Reduced || duration.is_zero() {
            vec![MotionSample {
                at: Duration::ZERO,
                position: end,
            }]
        } else {
            (0..=steps)
                .map(|i| {
                    let progress = i as f64 / steps as f64;
                    MotionSample {
                        at: duration.mul_f64(progress),
                        position: sample(start, end, progress, style),
                    }
                })
                .collect()
        };
        Ok(Self {
            start,
            end,
            samples,
        })
    }
    /// Straight-line drag with one sample per 10 ms, bounded to 100 samples.
    pub fn for_drag(from: (f64, f64), to: (f64, f64), duration_ms: u64) -> Result<Self> {
        Self::new(
            from,
            to,
            Duration::from_millis(duration_ms),
            (duration_ms / 10).clamp(1, 100) as usize,
            MotionStyle::Straight,
        )
    }
    /// Replays the plan in real time, sleeping until each sample is due.
    pub fn walk(&self, mut emit: impl FnMut(&MotionSample) -> Result<()>) -> Result<()> {
        let started = std::time::Instant::now();
        for sample in &self.samples {
            let wait = sample.at.saturating_sub(started.elapsed());
            if !wait.is_zero() {
                std::thread::sleep(wait);
            }
            emit(sample)?;
        }
        Ok(())
    }
    pub fn samples(&self) -> &[MotionSample] {
        &self.samples
    }
    pub fn start(&self) -> (f64, f64) {
        self.start
    }
    pub fn end(&self) -> (f64, f64) {
        self.end
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelativeSample {
    pub at: Duration,
    pub dx: i8,
    pub dy: i8,
}

/// Relative counts, not pixel displacement. Acceleration, edge clipping, and
/// other devices can change where the native pointer actually ends up.
#[derive(Debug, Clone)]
pub struct RelativeMotionPlan {
    samples: Vec<RelativeSample>,
}
impl RelativeMotionPlan {
    pub fn new(
        dx: i32,
        dy: i32,
        duration: Duration,
        steps: usize,
        style: MotionStyle,
    ) -> Result<Self> {
        let plan = MotionPlan::new((0., 0.), (dx as f64, dy as f64), duration, steps, style)?;
        let mut samples = Vec::new();
        let mut previous = (0_i64, 0_i64);
        let mut previous_time = Duration::ZERO;
        for point in plan.samples() {
            let next = (
                point.position.0.round() as i64,
                point.position.1.round() as i64,
            );
            let delta = (next.0 - previous.0, next.1 - previous.1);
            let parts = ((delta.0.abs().max(delta.1.abs()) + 126) / 127) as usize;
            if samples.len().saturating_add(parts) > 65_536 {
                return Err(invalid("relative plan exceeds 65536 reports"));
            }
            let mut emitted = (0_i64, 0_i64);
            for part in 1..=parts {
                // Divide cumulative displacement, not individual rounded deltas:
                // this preserves exact totals without accumulating drift.
                let cumulative = (
                    delta.0 * part as i64 / parts as i64,
                    delta.1 * part as i64 / parts as i64,
                );
                samples.push(RelativeSample {
                    at: previous_time
                        + (point.at - previous_time).mul_f64(part as f64 / parts as f64),
                    dx: (cumulative.0 - emitted.0) as i8,
                    dy: (cumulative.1 - emitted.1) as i8,
                });
                emitted = cumulative;
            }
            previous = next;
            previous_time = point.at;
        }
        // Quantization may reach the final integer count before the curve ends.
        // Preserve the requested completion deadline with a stationary sample.
        let end_time = plan.samples().last().unwrap().at;
        if !end_time.is_zero() && samples.last().is_none_or(|s| s.at < end_time) {
            if samples.len() == 65_536 {
                return Err(invalid("relative plan exceeds 65536 reports"));
            }
            samples.push(RelativeSample {
                at: end_time,
                dx: 0,
                dy: 0,
            });
        }
        Ok(Self { samples })
    }
    pub fn samples(&self) -> &[RelativeSample] {
        &self.samples
    }
}

fn validate(duration: Duration, steps: usize) -> Result<()> {
    if duration > Duration::from_secs(60) || !(1..=4096).contains(&steps) {
        return Err(invalid(
            "motion requires 1..4096 steps and duration at most 60 seconds",
        ));
    }
    Ok(())
}
fn invalid(message: &str) -> NativeError {
    NativeError::new("motion_plan", message)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absolute_endpoints_and_arc_are_bounded() {
        let plan = MotionPlan::new(
            (-100., 50.),
            (900., 50.),
            Duration::from_millis(300),
            100,
            MotionStyle::Curved,
        )
        .unwrap();
        assert_eq!(plan.samples()[0].position, (-100., 50.));
        assert_eq!(plan.samples().last().unwrap().position, (900., 50.));
        for p in plan.samples() {
            assert!((-100. ..=900.).contains(&p.position.0));
            assert!((50. ..=68.).contains(&p.position.1));
        }
    }
    #[test]
    fn relative_quantization_preserves_counts_and_report_bounds() {
        for (dx, dy) in [(1, 0), (-1, 1), (70000, -23333), (-400, 1500), (0, 0)] {
            for style in [
                MotionStyle::Straight,
                MotionStyle::Curved,
                MotionStyle::Reduced,
            ] {
                let p =
                    RelativeMotionPlan::new(dx, dy, Duration::from_millis(350), 43, style).unwrap();
                let sum = p.samples().iter().fold((0_i64, 0_i64), |(x, y), s| {
                    assert!(s.dx >= -127 && s.dy >= -127);
                    (x + i64::from(s.dx), y + i64::from(s.dy))
                });
                assert_eq!(sum, (i64::from(dx), i64::from(dy)));
                assert!(p.samples().windows(2).all(|w| w[0].at <= w[1].at));
                if style == MotionStyle::Reduced {
                    assert!(p.samples().iter().all(|s| s.at.is_zero()));
                } else {
                    assert_eq!(p.samples().last().unwrap().at, Duration::from_millis(350));
                }
            }
        }
    }
    #[test]
    fn invalid_and_excessive_plans_fail_before_dispatch() {
        assert!(
            MotionPlan::new(
                (f64::NAN, 0.),
                (0., 0.),
                Duration::ZERO,
                1,
                MotionStyle::Straight
            )
            .is_err()
        );
        assert!(
            MotionPlan::new((0., 0.), (0., 0.), Duration::ZERO, 0, MotionStyle::Straight).is_err()
        );
        assert!(
            RelativeMotionPlan::new(i32::MAX, 0, Duration::ZERO, 1, MotionStyle::Reduced).is_err()
        );
        assert!(
            RelativeMotionPlan::new(1, 0, Duration::from_secs(61), 1, MotionStyle::Straight)
                .is_err()
        );
    }
    #[test]
    fn immediate_motion_is_one_absolute_endpoint() {
        let p =
            MotionPlan::new((1., 2.), (3., 4.), Duration::ZERO, 10, MotionStyle::Curved).unwrap();
        assert_eq!(
            p.samples(),
            &[MotionSample {
                at: Duration::ZERO,
                position: (3., 4.)
            }]
        );
    }
}

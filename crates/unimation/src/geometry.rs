//! Coordinates are Quartz-style logical desktop points, with a top-left origin.
use crate::{Effect, NativeError, Point, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl Rect {
    pub fn valid(&self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|v| v.is_finite())
            && self.width > 0.
            && self.height > 0.
            && (self.x + self.width).is_finite()
            && (self.y + self.height).is_finite()
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameMapping {
    pub source_bounds: Rect,
    pub pixel_width: u32,
    pub pixel_height: u32,
    /// Geometry fingerprint, not a claim that pixels or UI content are current.
    pub geometry_revision: String,
}
fn invalid(message: &str) -> NativeError {
    NativeError {
        code: "invalid_geometry".into(),
        message: message.into(),
        effect: Effect::None,
    }
}
impl FrameMapping {
    pub fn validate(&self, current_revision: &str) -> Result<()> {
        if self.geometry_revision != current_revision {
            return Err(NativeError {
                code: "stale_geometry".into(),
                message: "Capture geometry changed; capture again before using its coordinates"
                    .into(),
                effect: Effect::None,
            });
        }
        if !self.source_bounds.valid() || self.pixel_width == 0 || self.pixel_height == 0 {
            return Err(invalid("Invalid frame dimensions"));
        }
        Ok(())
    }
    pub fn pixel_to_global(&self, point: Point, current_revision: &str) -> Result<Point> {
        self.validate(current_revision)?;
        if !point.x.is_finite()
            || !point.y.is_finite()
            || point.x < 0.
            || point.y < 0.
            || point.x >= self.pixel_width as f64
            || point.y >= self.pixel_height as f64
        {
            return Err(invalid("Pixel coordinate is outside the frame"));
        }
        Ok(Point {
            x: self.source_bounds.x
                + (point.x / self.pixel_width as f64) * self.source_bounds.width,
            y: self.source_bounds.y
                + (point.y / self.pixel_height as f64) * self.source_bounds.height,
        })
    }
    pub fn local_to_global(&self, point: Point, current_revision: &str) -> Result<Point> {
        self.validate(current_revision)?;
        if !point.x.is_finite()
            || !point.y.is_finite()
            || point.x < 0.
            || point.y < 0.
            || point.x >= self.source_bounds.width
            || point.y >= self.source_bounds.height
        {
            return Err(invalid("Logical coordinate is outside the source"));
        }
        Ok(Point {
            x: self.source_bounds.x + point.x,
            y: self.source_bounds.y + point.y,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn mapping() -> FrameMapping {
        FrameMapping {
            source_bounds: Rect {
                x: -1000.,
                y: -200.,
                width: 1000.,
                height: 500.,
            },
            pixel_width: 2000,
            pixel_height: 1500,
            geometry_revision: "a".into(),
        }
    }
    #[test]
    fn negative_origin_and_independent_scales() {
        let p = mapping()
            .pixel_to_global(Point { x: 1000., y: 750. }, "a")
            .unwrap();
        assert_eq!((p.x, p.y), (-500., 50.));
    }
    #[test]
    fn rejects_stale_outside_and_nan() {
        for p in [
            Point { x: 2000., y: 0. },
            Point { x: f64::NAN, y: 0. },
            Point { x: -1., y: 0. },
        ] {
            assert!(mapping().pixel_to_global(p, "a").is_err());
        }
        assert!(
            mapping()
                .pixel_to_global(Point { x: 0., y: 0. }, "b")
                .is_err()
        );
    }
    #[test]
    fn mapping_avoids_intermediate_overflow() {
        let mut m = mapping();
        m.source_bounds = Rect {
            x: 0.,
            y: 0.,
            width: 1e307,
            height: 1e307,
        };
        let p = m.pixel_to_global(Point { x: 1000., y: 750. }, "a").unwrap();
        assert_eq!(p.x, 5e306);
        assert_eq!(p.y, 5e306);
    }
    #[test]
    fn logical_window_coordinates() {
        let p = mapping()
            .local_to_global(Point { x: 10., y: 20. }, "a")
            .unwrap();
        assert_eq!((p.x, p.y), (-990., -180.));
    }
}

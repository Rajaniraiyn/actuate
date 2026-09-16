//! Rectangles and affine capture mappings use the coordinate basis declared by
//! the provider, with a top-left origin. Origins may be negative. Callers must
//! not mix macOS logical points, Windows physical desktop pixels, Wayland layout
//! coordinates or X11 server pixels.
use crate::{NativeError, Point, Result};
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
    pub fn contains(&self, point: &Point) -> bool {
        point.x >= self.x
            && point.y >= self.y
            && point.x < self.x + self.width
            && point.y < self.y + self.height
    }
    pub fn center(&self) -> Point {
        Point {
            x: self.x + self.width / 2.,
            y: self.y + self.height / 2.,
        }
    }
    /// A point inside this rectangle, expressed relative to its origin,
    /// converted to the rectangle's coordinate space.
    pub fn local_to_global(&self, point: Point) -> Result<Point> {
        if !point.x.is_finite()
            || !point.y.is_finite()
            || point.x < 0.
            || point.y < 0.
            || point.x >= self.width
            || point.y >= self.height
        {
            return Err(invalid("Logical coordinate is outside the source"));
        }
        Ok(Point {
            x: self.x + point.x,
            y: self.y + point.y,
        })
    }
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
    NativeError::new("invalid_geometry", message)
}
impl FrameMapping {
    pub fn validate(&self, current_revision: &str) -> Result<()> {
        if self.geometry_revision != current_revision {
            return Err(NativeError::new(
                "stale_geometry",
                "Capture geometry changed; capture again before using its coordinates",
            ));
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
        self.source_bounds.local_to_global(point)
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
    fn rect_containment_and_center() {
        let r = Rect {
            x: 10.,
            y: 20.,
            width: 30.,
            height: 40.,
        };
        assert!(r.contains(&Point { x: 10., y: 20. }));
        assert!(!r.contains(&Point { x: 40., y: 20. }));
        let c = r.center();
        assert_eq!((c.x, c.y), (25., 40.));
        assert!(r.local_to_global(Point { x: 30., y: 0. }).is_err());
    }
    #[test]
    fn logical_window_coordinates() {
        let p = mapping()
            .local_to_global(Point { x: 10., y: 20. }, "a")
            .unwrap();
        assert_eq!((p.x, p.y), (-990., -180.));
    }
}

//! Portable vector outline adapted from Cua's MIT-licensed cursor theme.
//! Source and license: `../THIRD_PARTY_NOTICES.md`.
//!
//! Coordinates use a downward Y axis. The hotspot is the midpoint of the
//! rounded leading tip, exactly on its cubic curve, rather than a bounding-box
//! corner. Renderers translate this point to the requested desktop coordinate.

pub type Point = (f64, f64);

/// A cubic-path vertex with incoming/outgoing control offsets.
#[derive(Debug, Clone, Copy)]
pub struct Vertex {
    pub point: Point,
    pub incoming: Point,
    pub outgoing: Point,
}

/// Logical points per source coordinate, yielding a roughly 16-point cursor.
pub const UNIT: f64 = 0.22;
pub const HOTSPOT: Point = (46., 31.75);

/// Closed rounded arrowhead with an inward notch and no extended tail.
pub const OUTLINE: [Vertex; 8] = [
    Vertex {
        point: (55., 30.),
        incoming: (0., 0.),
        outgoing: (-7., -2.),
    },
    Vertex {
        point: (43., 41.),
        incoming: (-1., -8.),
        outgoing: (0., 0.),
    },
    Vertex {
        point: (64., 98.),
        incoming: (0., 0.),
        outgoing: (3., 8.),
    },
    Vertex {
        point: (77., 99.),
        incoming: (-4., 7.),
        outgoing: (0., 0.),
    },
    Vertex {
        point: (86., 79.),
        incoming: (0., 0.),
        outgoing: (2., -4.),
    },
    Vertex {
        point: (95., 70.),
        incoming: (-4., 2.),
        outgoing: (0., 0.),
    },
    Vertex {
        point: (108., 63.),
        incoming: (0., 0.),
        outgoing: (7., -4.),
    },
    Vertex {
        point: (107., 50.),
        incoming: (7., 3.),
        outgoing: (0., 0.),
    },
];

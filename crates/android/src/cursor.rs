//! Observe the actual Android pointer hotspot, not the cursor bitmap's top-left.
//!
//! Schema: AOSP Android 16 external/perfetto
//! `protos/perfetto/trace/android/surfaceflinger_{layers,common}.proto`.
//! Cursor metadata: frameworks/base/libs/input/SpriteController.cpp, key 4.
//! This adapter deliberately supports only unrotated, unscaled full displays.
//! Other display mappings must be implemented explicitly rather than guessed.
use crate::{CommandTransport, LimitedOutput, error};
use actuate::{Effect, Result};
use prost::Message;
use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

const LIMIT: usize = 16 * 1024 * 1024;
const MAGIC: u64 = 0x454341525452594c;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// Geometry identifies the coordinate space; changing it invalidates a prior aim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DisplayGeometry {
    pub display_id: u64,
    pub layer_stack: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CursorObservation {
    /// Hotspot in the full, unscaled display's pixel coordinate space.
    pub position: Point,
    pub surface_origin: Point,
    pub hotspot: Point,
    pub style: i32,
    pub native_shadow: bool,
    pub layer_id: i32,
    pub layer_name: String,
    pub geometry: DisplayGeometry,
    /// Device elapsed realtime, not host wall time. Plain dumps may omit it.
    pub elapsed_realtime_nanos: Option<i64>,
    /// Host receipt time; not a synchronization guarantee with a screenshot.
    pub observed_at_unix_ms: u64,
    pub alpha: Option<f32>,
    pub hidden: bool,
    pub layer_transform_type: i32,
    pub display_transform_type: i32,
}

/// Read-only observation on the existing connection. Does not move or unhide a pointer.
pub fn observe<T: CommandTransport>(transport: &mut T) -> Result<CursorObservation> {
    let mut out = LimitedOutput::new(LIMIT);
    let mut err = LimitedOutput::new(64 * 1024);
    let status = transport.execute("dumpsys SurfaceFlinger --proto", &mut out, &mut err)?;
    if status != Some(0) {
        return Err(error(
            "android_cursor_observation",
            format!(
                "SurfaceFlinger exit {status:?}: {}",
                String::from_utf8_lossy(&err.bytes)
            ),
            Effect::None,
        ));
    }
    parse_surface_flinger(&out.bytes)
}

fn invalid(message: impl ToString) -> actuate::NativeError {
    error("android_cursor_geometry", message, Effect::None)
}

/// Decode a single SurfaceFlinger dump. No pixel ratio or icon hotspot is assumed.
pub fn parse_surface_flinger(bytes: &[u8]) -> Result<CursorObservation> {
    if bytes.len() > LIMIT {
        return Err(invalid("SurfaceFlinger dump exceeds observation limit"));
    }
    let file = Trace::decode(bytes)
        .map_err(|e| invalid(format!("invalid SurfaceFlinger protobuf: {e}")))?;
    if file.magic != Some(MAGIC) || file.entries.len() != 1 {
        return Err(invalid("expected one LYRTRACE SurfaceFlinger snapshot"));
    }
    let snapshot = &file.entries[0];
    let elapsed = snapshot.elapsed;
    if elapsed.is_some_and(|value| value < 0) {
        return Err(invalid("negative snapshot timestamp"));
    }
    let layers = &snapshot
        .layers
        .as_ref()
        .ok_or_else(|| invalid("missing layers"))?
        .layers;
    let candidates: Vec<_> = layers
        .iter()
        .filter(|l| l.metadata.iter().any(|m| m.key == Some(4)))
        .collect();
    if candidates.len() != 1 {
        return Err(invalid(format!(
            "expected one cursor metadata layer, found {}",
            candidates.len()
        )));
    }
    let layer = candidates[0];
    let metadata: Vec<_> = layer.metadata.iter().filter(|m| m.key == Some(4)).collect();
    if metadata.len() != 1 {
        return Err(invalid("duplicate cursor metadata"));
    }
    let raw = metadata[0]
        .value
        .as_deref()
        .ok_or_else(|| invalid("empty cursor metadata"))?;
    if raw.len() != 16 {
        return Err(invalid("unsupported cursor metadata Parcel layout"));
    }
    let style = i32::from_le_bytes(raw[0..4].try_into().unwrap());
    let hotspot = Point {
        x: f32::from_le_bytes(raw[4..8].try_into().unwrap()),
        y: f32::from_le_bytes(raw[8..12].try_into().unwrap()),
    };
    let shadow = i32::from_le_bytes(raw[12..16].try_into().unwrap());
    if !hotspot.x.is_finite()
        || !hotspot.y.is_finite()
        || hotspot.x < 0.0
        || hotspot.y < 0.0
        || !matches!(shadow, 0 | 1)
    {
        return Err(invalid("invalid cursor metadata values"));
    }
    let layer_stack = layer
        .stack
        .ok_or_else(|| invalid("cursor lacks layer stack"))?;
    let displays: Vec<_> = snapshot
        .displays
        .iter()
        .filter(|d| d.stack == Some(layer_stack))
        .collect();
    if displays.len() != 1 {
        return Err(invalid(
            "cursor layer stack must identify exactly one display",
        ));
    }
    let display = displays[0];
    if display.is_virtual != Some(false) {
        return Err(invalid("virtual or unknown display unsupported"));
    }
    let display_transform_type = translation_type(display.transform.as_ref())?;
    if display_transform_type != 0 {
        return Err(invalid("translated display mapping unsupported"));
    }
    let size = display
        .size
        .as_ref()
        .ok_or_else(|| invalid("missing display size"))?;
    let (width, height) = (size.w.unwrap_or(0), size.h.unwrap_or(0));
    if width <= 0 || height <= 0 {
        return Err(invalid("invalid display size"));
    }
    let rect = display
        .rect
        .as_ref()
        .ok_or_else(|| invalid("missing display viewport"))?;
    if rect.values() != Some([0, 0, width, height]) {
        return Err(invalid("cropped or offset display viewport unsupported"));
    }
    let layer_transform_type = translation_type(layer.transform.as_ref())?;
    if let Some(t) = &layer.effective_transform {
        translation_type(Some(t))?;
    }
    if layer
        .crop
        .as_ref()
        .is_some_and(|c| c.values() != Some([0, 0, -1, -1]))
    {
        return Err(invalid("cropped cursor surface unsupported"));
    }
    let origin = layer
        .position
        .as_ref()
        .ok_or_else(|| invalid("cursor lacks actual surface position"))?;
    let surface_origin = Point {
        x: origin.x.unwrap_or(0.0),
        y: origin.y.unwrap_or(0.0),
    };
    let position = Point {
        x: surface_origin.x + hotspot.x,
        y: surface_origin.y + hotspot.y,
    };
    if !position.x.is_finite()
        || !position.y.is_finite()
        || position.x < 0.0
        || position.y < 0.0
        || position.x > (width - 1) as f32
        || position.y > (height - 1) as f32
    {
        return Err(invalid("cursor hotspot is outside the validated display"));
    }
    let alpha = layer.color.as_ref().and_then(|c| c.a);
    if alpha.is_some_and(|a| !a.is_finite() || !(0.0..=1.0).contains(&a)) {
        return Err(invalid("invalid cursor opacity"));
    }
    Ok(CursorObservation {
        position,
        surface_origin,
        hotspot,
        style,
        native_shadow: shadow == 1,
        layer_id: layer.id.ok_or_else(|| invalid("missing cursor layer ID"))?,
        layer_name: layer.name.clone().unwrap_or_default(),
        geometry: DisplayGeometry {
            display_id: display.id.ok_or_else(|| invalid("missing display ID"))?,
            layer_stack,
            width: width as u32,
            height: height as u32,
        },
        elapsed_realtime_nanos: elapsed,
        observed_at_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX),
        alpha,
        hidden: layer.flags.unwrap_or(0) & 1 != 0,
        layer_transform_type,
        display_transform_type,
    })
}

fn translation_type(t: Option<&Transform>) -> Result<i32> {
    let t = t.ok_or_else(|| invalid("missing transform evidence"))?;
    let kind = t.kind.ok_or_else(|| invalid("missing transform type"))?;
    // ui::Transform types: IDENTITY=0, TRANSLATE=1; matrices may be omitted for these.
    if !matches!(kind, 0 | 1)
        || t.dsdx.is_some_and(|v| v != 1.0)
        || t.dtdy.is_some_and(|v| v != 1.0)
        || t.dtdx.is_some_and(|v| v != 0.0)
        || t.dsdy.is_some_and(|v| v != 0.0)
    {
        return Err(invalid(
            "rotated, scaled, or unknown cursor/display transform unsupported",
        ));
    }
    Ok(kind)
}

// Projected protobuf schema: unknown fields are skipped by prost. Repeated map entries
// deliberately remain entries, so duplicate cursor keys are rejected instead of overwritten.
// Perfetto currently calls metadata values strings; OEM/older dumps contain binary Parcels.
// `bytes` has the same length-delimited wire representation and preserves those values.
#[derive(Clone, PartialEq, Message)]
struct Trace {
    #[prost(fixed64, optional, tag = "1")]
    magic: Option<u64>,
    #[prost(message, repeated, tag = "2")]
    entries: Vec<Snapshot>,
}
#[derive(Clone, PartialEq, Message)]
struct Snapshot {
    #[prost(sfixed64, optional, tag = "1")]
    elapsed: Option<i64>,
    #[prost(message, optional, tag = "3")]
    layers: Option<Layers>,
    #[prost(message, repeated, tag = "7")]
    displays: Vec<Display>,
}
#[derive(Clone, PartialEq, Message)]
struct Layers {
    #[prost(message, repeated, tag = "1")]
    layers: Vec<Layer>,
}
#[derive(Clone, PartialEq, Message)]
struct Layer {
    #[prost(int32, optional, tag = "1")]
    id: Option<i32>,
    #[prost(string, optional, tag = "2")]
    name: Option<String>,
    #[prost(uint32, optional, tag = "9")]
    stack: Option<u32>,
    #[prost(message, optional, tag = "11")]
    position: Option<Position>,
    #[prost(message, optional, tag = "14")]
    crop: Option<Rect>,
    #[prost(message, optional, tag = "20")]
    color: Option<Color>,
    #[prost(uint32, optional, tag = "22")]
    flags: Option<u32>,
    #[prost(message, optional, tag = "23")]
    transform: Option<Transform>,
    #[prost(message, repeated, tag = "42")]
    metadata: Vec<Metadata>,
    #[prost(message, optional, tag = "43")]
    effective_transform: Option<Transform>,
}
#[derive(Clone, PartialEq, Message)]
struct Metadata {
    #[prost(int32, optional, tag = "1")]
    key: Option<i32>,
    #[prost(bytes = "vec", optional, tag = "2")]
    value: Option<Vec<u8>>,
}
#[derive(Clone, PartialEq, Message)]
struct Display {
    #[prost(uint64, optional, tag = "1")]
    id: Option<u64>,
    #[prost(uint32, optional, tag = "3")]
    stack: Option<u32>,
    #[prost(message, optional, tag = "4")]
    size: Option<Size>,
    #[prost(message, optional, tag = "5")]
    rect: Option<Rect>,
    #[prost(message, optional, tag = "6")]
    transform: Option<Transform>,
    #[prost(bool, optional, tag = "7")]
    is_virtual: Option<bool>,
}
#[derive(Clone, PartialEq, Message)]
struct Position {
    #[prost(float, optional, tag = "1")]
    x: Option<f32>,
    #[prost(float, optional, tag = "2")]
    y: Option<f32>,
}
#[derive(Clone, PartialEq, Message)]
struct Size {
    #[prost(int32, optional, tag = "1")]
    w: Option<i32>,
    #[prost(int32, optional, tag = "2")]
    h: Option<i32>,
}
#[derive(Clone, PartialEq, Message)]
struct Rect {
    #[prost(int32, optional, tag = "1")]
    left: Option<i32>,
    #[prost(int32, optional, tag = "2")]
    top: Option<i32>,
    #[prost(int32, optional, tag = "3")]
    right: Option<i32>,
    #[prost(int32, optional, tag = "4")]
    bottom: Option<i32>,
}
impl Rect {
    fn values(&self) -> Option<[i32; 4]> {
        Some([self.left?, self.top?, self.right?, self.bottom?])
    }
}
#[derive(Clone, PartialEq, Message)]
struct Transform {
    #[prost(float, optional, tag = "1")]
    dsdx: Option<f32>,
    #[prost(float, optional, tag = "2")]
    dtdx: Option<f32>,
    #[prost(float, optional, tag = "3")]
    dsdy: Option<f32>,
    #[prost(float, optional, tag = "4")]
    dtdy: Option<f32>,
    #[prost(int32, optional, tag = "5")]
    kind: Option<i32>,
}
#[derive(Clone, PartialEq, Message)]
struct Color {
    #[prost(float, optional, tag = "4")]
    a: Option<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Trace {
        let metadata = [
            1000i32.to_le_bytes(),
            2f32.to_le_bytes(),
            3f32.to_le_bytes(),
            0i32.to_le_bytes(),
        ]
        .concat();
        Trace {
            magic: Some(MAGIC),
            entries: vec![Snapshot {
                elapsed: Some(123),
                layers: Some(Layers {
                    layers: vec![Layer {
                        id: Some(7),
                        stack: Some(0),
                        position: Some(Position {
                            x: Some(98.0),
                            y: Some(197.0),
                        }),
                        transform: Some(Transform {
                            kind: Some(1),
                            ..Default::default()
                        }),
                        metadata: vec![Metadata {
                            key: Some(4),
                            value: Some(metadata),
                        }],
                        ..Default::default()
                    }],
                }),
                displays: vec![Display {
                    id: Some(42),
                    stack: Some(0),
                    size: Some(Size {
                        w: Some(1080),
                        h: Some(2340),
                    }),
                    rect: Some(Rect {
                        left: Some(0),
                        top: Some(0),
                        right: Some(1080),
                        bottom: Some(2340),
                    }),
                    transform: Some(Transform {
                        kind: Some(0),
                        ..Default::default()
                    }),
                    is_virtual: Some(false),
                }],
            }],
        }
    }
    #[test]
    fn uses_metadata_hotspot_in_both_axes() {
        let observed = parse_surface_flinger(&fixture().encode_to_vec()).unwrap();
        assert_eq!(observed.position, Point { x: 100.0, y: 200.0 });
        assert_eq!(observed.surface_origin, Point { x: 98.0, y: 197.0 });
        assert_eq!(observed.geometry.display_id, 42);
    }
    #[test]
    fn plain_dump_may_omit_device_clock() {
        let mut f = fixture();
        f.entries[0].elapsed = None;
        let observed = parse_surface_flinger(&f.encode_to_vec()).unwrap();
        assert_eq!(observed.elapsed_realtime_nanos, None);
        assert!(observed.observed_at_unix_ms > 0);
    }
    #[test]
    fn rejects_ambiguous_or_mismatched_displays() {
        let mut f = fixture();
        f.entries[0].displays[0].stack = Some(1);
        assert!(parse_surface_flinger(&f.encode_to_vec()).is_err());
        let mut f = fixture();
        let display = f.entries[0].displays[0].clone();
        f.entries[0].displays.push(display);
        assert!(parse_surface_flinger(&f.encode_to_vec()).is_err());
    }
    #[test]
    fn rejects_multiple_cursors_and_unknown_mapping() {
        let mut f = fixture();
        let ls = &mut f.entries[0].layers.as_mut().unwrap().layers;
        ls.push(ls[0].clone());
        assert!(parse_surface_flinger(&f.encode_to_vec()).is_err());
        let mut f = fixture();
        f.entries[0].displays[0].transform.as_mut().unwrap().kind = Some(4);
        assert!(parse_surface_flinger(&f.encode_to_vec()).is_err());
        let mut f = fixture();
        f.entries[0].displays[0].rect.as_mut().unwrap().top = Some(30);
        assert!(parse_surface_flinger(&f.encode_to_vec()).is_err());
    }
    #[test]
    fn rejects_malformed_or_nonfinite_evidence() {
        assert!(parse_surface_flinger(b"not protobuf").is_err());
        let mut f = fixture();
        let l = &mut f.entries[0].layers.as_mut().unwrap().layers[0];
        l.position.as_mut().unwrap().x = Some(f32::NAN);
        assert!(parse_surface_flinger(&f.encode_to_vec()).is_err());
        let mut f = fixture();
        f.entries[0].layers.as_mut().unwrap().layers[0].metadata[0]
            .value
            .as_mut()
            .unwrap()
            .pop();
        assert!(parse_surface_flinger(&f.encode_to_vec()).is_err());
    }
}

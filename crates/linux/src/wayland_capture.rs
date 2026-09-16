//! Still capture through wlr-screencopy (outputs) and ext-image-copy-capture
//! (toplevels). Frames are written as PNG with a geometry mapping in logical
//! layout coordinates. Capturing never raises or focuses anything.
use actuate::{
    Capture, NativeError, Result,
    geometry::{FrameMapping, Rect},
    image::absolute_output,
};
use compositor::wayland::{Desktop, Outputs, ShmBuffer, fail};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use wayland_client::{
    Connection, Dispatch, QueueHandle,
    protocol::{wl_buffer, wl_shm, wl_shm_pool},
};
use wayland_protocols::ext::{
    foreign_toplevel_list::v1::client::{
        ext_foreign_toplevel_handle_v1, ext_foreign_toplevel_list_v1,
    },
    image_capture_source::v1::client::{
        ext_foreign_toplevel_image_capture_source_manager_v1, ext_image_capture_source_v1,
    },
    image_copy_capture::v1::client::{
        ext_image_copy_capture_frame_v1, ext_image_copy_capture_manager_v1,
        ext_image_copy_capture_session_v1,
    },
};
use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1, zwlr_screencopy_manager_v1,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CaptureSource {
    /// One output by connector name, such as `eDP-1`.
    Output { name: String },
    /// A toplevel by its foreign-toplevel identifier, title and app id, or
    /// by Hyprland address; the session resolves addresses to titles.
    Toplevel {
        #[serde(default)]
        identifier: Option<String>,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        app_id: Option<String>,
    },
    /// A logical-coordinate region cropped from the output that contains it.
    Region {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureRequest {
    pub source: CaptureSource,
    pub path: PathBuf,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frame {
    pub source: CaptureSource,
    pub path: PathBuf,
    pub route: String,
    pub mapping: FrameMapping,
    /// Title and app id of the captured toplevel, when one was selected.
    pub toplevel: Option<ToplevelInfo>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToplevelInfo {
    pub identifier: String,
    pub title: String,
    pub app_id: String,
}

#[derive(Default)]
struct Pending {
    format: Option<wl_shm::Format>,
    width: u32,
    height: u32,
    stride: u32,
    buffer_done: bool,
    ready: bool,
    failed: Option<String>,
    y_invert: bool,
}
#[derive(Default)]
struct State {
    outputs: Outputs,
    toplevels: Vec<(
        ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
        ToplevelInfo,
        bool,
    )>,
    pending: Pending,
}
impl AsMut<Outputs> for State {
    fn as_mut(&mut self) -> &mut Outputs {
        &mut self.outputs
    }
}
compositor::delegate_registry!(State);
compositor::delegate_outputs!(State);
compositor::delegate_silent!(
    State,
    [
        wl_shm::WlShm,
        wl_shm_pool::WlShmPool,
        wl_buffer::WlBuffer,
        zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
        ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1,
        ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1,
        ext_image_capture_source_v1::ExtImageCaptureSourceV1,
    ]
);
impl Dispatch<zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1,
        event: zwlr_screencopy_frame_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwlr_screencopy_frame_v1::Event;
        let p = &mut state.pending;
        match event {
            Event::Buffer {
                format,
                width,
                height,
                stride,
            } => {
                if p.format.is_none()
                    && let Ok(format) = format.into_result()
                {
                    p.format = Some(format);
                    p.width = width;
                    p.height = height;
                    p.stride = stride;
                }
            }
            Event::BufferDone => p.buffer_done = true,
            Event::Flags { flags } => {
                p.y_invert = flags
                    .into_result()
                    .is_ok_and(|f| f.contains(zwlr_screencopy_frame_v1::Flags::YInvert));
            }
            Event::Ready { .. } => p.ready = true,
            Event::Failed => p.failed = Some("screencopy failed".into()),
            _ => {}
        }
    }
}
impl Dispatch<ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1, ()> for State {
    wayland_client::event_created_child!(State, ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1, [
        ext_foreign_toplevel_list_v1::EVT_TOPLEVEL_OPCODE => (ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1, ()),
    ]);
    fn event(
        state: &mut Self,
        _: &ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1,
        event: ext_foreign_toplevel_list_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_foreign_toplevel_list_v1::Event::Toplevel { toplevel } = event {
            state.toplevels.push((
                toplevel,
                ToplevelInfo {
                    identifier: String::new(),
                    title: String::new(),
                    app_id: String::new(),
                },
                false,
            ));
        }
    }
}
impl Dispatch<ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1, ()> for State {
    fn event(
        state: &mut Self,
        handle: &ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
        event: ext_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_foreign_toplevel_handle_v1::Event;
        let Some(entry) = state.toplevels.iter_mut().find(|(h, _, _)| h == handle) else {
            return;
        };
        match event {
            Event::Title { title } => entry.1.title = title,
            Event::AppId { app_id } => entry.1.app_id = app_id,
            Event::Identifier { identifier } => entry.1.identifier = identifier,
            Event::Done => entry.2 = true,
            Event::Closed => entry.2 = false,
            _ => {}
        }
    }
}
impl Dispatch<ext_image_copy_capture_session_v1::ExtImageCopyCaptureSessionV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ext_image_copy_capture_session_v1::ExtImageCopyCaptureSessionV1,
        event: ext_image_copy_capture_session_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_image_copy_capture_session_v1::Event;
        let p = &mut state.pending;
        match event {
            Event::BufferSize { width, height } => {
                p.width = width;
                p.height = height;
                p.stride = width * 4;
            }
            Event::ShmFormat { format } => {
                if let Ok(format) = format.into_result()
                    && (p.format.is_none()
                        || matches!(format, wl_shm::Format::Argb8888 | wl_shm::Format::Xrgb8888))
                {
                    p.format = Some(format);
                }
            }
            Event::Done => p.buffer_done = true,
            Event::Stopped => p.failed = Some("capture session stopped".into()),
            _ => {}
        }
    }
}
impl Dispatch<ext_image_copy_capture_frame_v1::ExtImageCopyCaptureFrameV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ext_image_copy_capture_frame_v1::ExtImageCopyCaptureFrameV1,
        event: ext_image_copy_capture_frame_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_image_copy_capture_frame_v1::Event;
        match event {
            Event::Ready => state.pending.ready = true,
            Event::Failed { reason } => {
                state.pending.failed = Some(format!("capture failed: {reason:?}"));
            }
            _ => {}
        }
    }
}

pub struct WaylandCapture {
    desktop: Desktop<State>,
    state: State,
    shm: wl_shm::WlShm,
    screencopy: Option<zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1>,
    copy_capture: Option<ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1>,
    toplevel_sources: Option<ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1>,
}
impl WaylandCapture {
    pub fn connect() -> Result<Self> {
        let mut state = State::default();
        let mut desktop = Desktop::connect_with_outputs(&mut state)?;
        let qh = desktop.qh();
        let shm: wl_shm::WlShm = desktop.globals.bind(&qh, 1..=1, ()).map_err(fail)?;
        let screencopy = desktop.globals.bind(&qh, 1..=3, ()).ok();
        let copy_capture = desktop.globals.bind(&qh, 1..=1, ()).ok();
        let toplevel_sources = desktop.globals.bind(&qh, 1..=1, ()).ok();
        let _list: Option<ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1> =
            desktop.globals.bind(&qh, 1..=1, ()).ok();
        desktop.roundtrip(&mut state)?;
        Ok(Self {
            desktop,
            state,
            shm,
            screencopy,
            copy_capture,
            toplevel_sources,
        })
    }
    pub fn capabilities(&self) -> serde_json::Value {
        serde_json::json!({
            "output_capture": self.screencopy.is_some(),
            "toplevel_capture": self.copy_capture.is_some() && self.toplevel_sources.is_some(),
            "toplevels": self.state.toplevels.iter().filter(|t| t.2).map(|t| &t.1).collect::<Vec<_>>(),
        })
    }
    pub fn outputs(&self) -> &[compositor::wayland::Output] {
        &self.state.outputs.outputs
    }
    pub fn toplevels(&mut self) -> Result<Vec<ToplevelInfo>> {
        self.desktop.roundtrip(&mut self.state)?;
        Ok(self
            .state
            .toplevels
            .iter()
            .filter(|t| t.2)
            .map(|t| t.1.clone())
            .collect())
    }
    /// Geometry fingerprint of the output layout for stale-frame detection.
    pub fn revision(&mut self, source: &CaptureSource) -> Result<String> {
        self.desktop.roundtrip(&mut self.state)?;
        let outputs: Vec<_> = self
            .state
            .outputs
            .outputs
            .iter()
            .map(|o| {
                (
                    o.name.clone(),
                    o.x,
                    o.y,
                    o.width,
                    o.height,
                    o.scale,
                    o.transform,
                )
            })
            .collect();
        serde_json::to_string(&(source, outputs)).map_err(fail)
    }
    /// Dispatches until `done` holds, the frame fails, or the deadline passes.
    fn wait_until(&mut self, done: fn(&Pending) -> bool, what: &str) -> Result<()> {
        let started = Instant::now();
        while !done(&self.state.pending) && self.state.pending.failed.is_none() {
            if started.elapsed() > Duration::from_secs(5) {
                return Err(NativeError::new(
                    "capture_failed",
                    format!("Compositor did not deliver {what} in time"),
                ));
            }
            self.desktop
                .queue
                .blocking_dispatch(&mut self.state)
                .map_err(fail)?;
        }
        if let Some(reason) = self.state.pending.failed.take() {
            return Err(NativeError::new("capture_failed", reason));
        }
        Ok(())
    }
    /// Upright RGBA pixels of one output, or of a rectangle on it in
    /// output-local logical coordinates when `region` is given.
    fn output_pixels(
        &mut self,
        name: &str,
        region: Option<(i32, i32, i32, i32)>,
    ) -> Result<(Vec<u8>, u32, u32)> {
        let manager = self.screencopy.clone().ok_or_else(|| {
            NativeError::unsupported("Compositor lacks zwlr_screencopy_manager_v1")
        })?;
        let output = self
            .state
            .outputs
            .by_name(name)
            .ok_or_else(|| NativeError::new("unknown_output", format!("No output named {name:?}")))?
            .wl
            .clone();
        self.state.pending = Pending::default();
        let qh = self.desktop.qh();
        let frame = match region {
            Some((x, y, w, h)) => manager.capture_output_region(0, &output, x, y, w, h, &qh, ()),
            None => manager.capture_output(0, &output, &qh, ()),
        };
        self.wait_until(|p| p.buffer_done, "the frame buffer description")?;
        let p = &self.state.pending;
        let format = p
            .format
            .ok_or_else(|| NativeError::new("capture_failed", "No shm format offered"))?;
        let buffer = ShmBuffer::with_stride(
            &self.shm,
            &qh,
            p.width as i32,
            p.height as i32,
            p.stride as i32,
            format,
        )?;
        frame.copy(&buffer.buffer);
        self.desktop.connection.flush().map_err(fail)?;
        let result = self.wait_until(|p| p.ready, "the frame");
        frame.destroy();
        result?;
        let mut rgba = buffer.to_rgba()?;
        if self.state.pending.y_invert {
            flip(&mut rgba, buffer.width as u32, buffer.height as u32);
        }
        Ok((rgba, buffer.width as u32, buffer.height as u32))
    }
    fn toplevel_pixels(&mut self, info: &ToplevelInfo) -> Result<ShmBuffer> {
        let (Some(copy), Some(sources)) =
            (self.copy_capture.clone(), self.toplevel_sources.clone())
        else {
            return Err(NativeError::new(
                "unsupported",
                "Compositor lacks ext-image-copy-capture with toplevel sources",
            ));
        };
        let handle = self
            .state
            .toplevels
            .iter()
            .find(|(_, i, alive)| *alive && i == info)
            .map(|(h, _, _)| h.clone())
            .ok_or_else(|| NativeError::new("no_window", "Toplevel is no longer listed"))?;
        let qh = self.desktop.qh();
        let source = sources.create_source(&handle, &qh, ());
        self.state.pending = Pending::default();
        let session = copy.create_session(
            &source,
            ext_image_copy_capture_manager_v1::Options::empty(),
            &qh,
            (),
        );
        self.wait_until(|p| p.buffer_done, "the frame buffer description")?;
        let p = &self.state.pending;
        let format = p
            .format
            .ok_or_else(|| NativeError::new("capture_failed", "No shm format offered"))?;
        let buffer = ShmBuffer::new(&self.shm, &qh, p.width as i32, p.height as i32, format)?;
        let frame = session.create_frame(&qh, ());
        frame.attach_buffer(&buffer.buffer);
        frame.damage_buffer(0, 0, i32::MAX, i32::MAX);
        frame.capture();
        self.desktop.connection.flush().map_err(fail)?;
        let result = self.wait_until(|p| p.ready, "the frame");
        frame.destroy();
        session.destroy();
        source.destroy();
        result?;
        Ok(buffer)
    }
    fn resolve_toplevel(
        &mut self,
        identifier: &Option<String>,
        title: &Option<String>,
        app_id: &Option<String>,
    ) -> Result<ToplevelInfo> {
        let list = self.toplevels()?;
        let matches: Vec<&ToplevelInfo> = list
            .iter()
            .filter(|t| identifier.as_ref().is_none_or(|i| &t.identifier == i))
            .filter(|t| title.as_ref().is_none_or(|v| &t.title == v))
            .filter(|t| app_id.as_ref().is_none_or(|v| &t.app_id == v))
            .collect();
        match matches.as_slice() {
            [one] => Ok((*one).clone()),
            [] => Err(NativeError::new(
                "no_window",
                "No listed toplevel matches the selector",
            )),
            many => Err(NativeError::new(
                "ambiguous_window",
                format!(
                    "{} toplevels match; add title, app_id or identifier",
                    many.len()
                ),
            )),
        }
    }
}

pub(crate) fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<()> {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| NativeError::new("capture_output", e))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let result = encoder
        .write_header()
        .and_then(|mut w| w.write_image_data(rgba).map(|()| w))
        .and_then(|w| w.finish());
    if let Err(e) = result {
        let _ = std::fs::remove_file(path);
        return Err(NativeError::new("capture_output", e));
    }
    Ok(())
}
fn flip(rgba: &mut [u8], width: u32, height: u32) {
    let stride = (width * 4) as usize;
    for y in 0..(height as usize / 2) {
        let (top, bottom) = rgba.split_at_mut((height as usize - y - 1) * stride);
        top[y * stride..y * stride + stride].swap_with_slice(&mut bottom[..stride]);
    }
}
impl Capture for WaylandCapture {
    type Request = CaptureRequest;
    type Frame = Frame;
    fn capture(&mut self, request: CaptureRequest) -> Result<Frame> {
        let path = absolute_output(&request.path)?;
        let revision = self.revision(&request.source)?;
        let (bounds, width, height, rgba, route, toplevel) = match &request.source {
            CaptureSource::Output { name } => {
                let output = self
                    .state
                    .outputs
                    .by_name(name)
                    .map(|o| o.rect())
                    .ok_or_else(|| {
                        NativeError::new("unknown_output", format!("No output named {name:?}"))
                    })?;
                let (rgba, width, height) = self.output_pixels(name, None)?;
                (
                    output,
                    width,
                    height,
                    rgba,
                    "linux.wayland.screencopy",
                    None,
                )
            }
            CaptureSource::Region {
                x,
                y,
                width,
                height,
            } => {
                let region = Rect {
                    x: *x,
                    y: *y,
                    width: *width,
                    height: *height,
                };
                if !region.valid() {
                    return Err(NativeError::new(
                        "invalid_geometry",
                        "Region must be finite and nonempty",
                    ));
                }
                let (name, rect) = self
                    .state
                    .outputs
                    .find(&actuate::Point { x: *x, y: *y })
                    .map(|o| (o.name.clone(), o.rect()))
                    .ok_or_else(|| {
                        NativeError::new(
                            "outside_displays",
                            "Region origin is outside every output",
                        )
                    })?;
                if x + width > rect.x + rect.width || y + height > rect.y + rect.height {
                    return Err(NativeError::new(
                        "invalid_geometry",
                        "Region must lie within one output",
                    ));
                }
                // The compositor crops in output-local logical coordinates.
                let local = (
                    (x - rect.x).round() as i32,
                    (y - rect.y).round() as i32,
                    width.round().max(1.) as i32,
                    height.round().max(1.) as i32,
                );
                let (rgba, w, h) = self.output_pixels(&name, Some(local))?;
                (region, w, h, rgba, "linux.wayland.screencopy_region", None)
            }
            CaptureSource::Toplevel {
                identifier,
                title,
                app_id,
            } => {
                let info = self.resolve_toplevel(identifier, title, app_id)?;
                let buffer = self.toplevel_pixels(&info)?;
                let rgba = buffer.to_rgba()?;
                // Toplevel capture has no layout position of its own; the
                // session attaches compositor geometry when it knows it.
                let bounds = Rect {
                    x: 0.,
                    y: 0.,
                    width: buffer.width as f64,
                    height: buffer.height as f64,
                };
                (
                    bounds,
                    buffer.width as u32,
                    buffer.height as u32,
                    rgba,
                    "linux.wayland.image_copy_capture",
                    Some(info),
                )
            }
        };
        write_png(&path, width, height, &rgba)?;
        let mapping = FrameMapping {
            source_bounds: bounds,
            pixel_width: width,
            pixel_height: height,
            geometry_revision: revision,
        };
        Ok(Frame {
            source: request.source,
            path,
            route: route.into(),
            mapping,
            toplevel,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn flip_operates_on_rgba_rows() {
        let mut image: Vec<u8> = (0..16).flat_map(|i| [i, i, i, 255]).collect();
        flip(&mut image, 4, 4);
        assert_eq!(image[0], 12);
        assert_eq!(image[60], 3);
    }
    #[test]
    fn source_selectors_are_strict() {
        assert!(
            serde_json::from_str::<CaptureSource>(r#"{"kind":"output","name":"eDP-1"}"#).is_ok()
        );
        assert!(
            serde_json::from_str::<CaptureSource>(r#"{"kind":"output","name":"eDP-1","extra":1}"#)
                .is_err()
        );
        assert!(
            serde_json::from_str::<CaptureSource>(r#"{"kind":"toplevel","title":"x"}"#).is_ok()
        );
    }
}

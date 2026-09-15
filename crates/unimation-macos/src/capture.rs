//! Explicit macOS screencapture provider. Native geometry uses current objc2 bindings.
//! Capture does not raise windows. An image is not proof that every pixel is readable.
use crate::error;
use objc2_core_foundation::{CFDictionary, CFNumber, CFString, CFType, CGRect};
use objc2_core_graphics::*;
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use unimation_core::{
    Capture, Effect, Result,
    geometry::{FrameMapping, Rect},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CaptureSource {
    Display { display_id: u32 },
    Window { window_id: u32 },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureRequest {
    pub source: CaptureSource,
    pub path: PathBuf,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayGeometry {
    pub display_id: u32,
    pub bounds: Rect,
    /// CGDisplayPixelsWide value. Screenshot dimensions are authoritative for frame scaling.
    pub reported_pixel_width: usize,
    pub reported_pixel_height: usize,
    pub is_main: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frame {
    pub owner_pid: Option<i64>,
    pub source: CaptureSource,
    pub path: PathBuf,
    pub route: String,
    pub mapping: FrameMapping,
    pub displays: Vec<DisplayGeometry>,
}
pub struct ScreenshotCapture;
fn fail(message: impl ToString) -> unimation_core::NativeError {
    error("capture_failed", message, Effect::None)
}
fn rect(r: CGRect) -> Rect {
    Rect {
        x: r.origin.x,
        y: r.origin.y,
        width: r.size.width,
        height: r.size.height,
    }
}
pub fn displays() -> Result<Vec<DisplayGeometry>> {
    let mut count = 0;
    // SAFETY: output count is valid and null buffer is allowed for count query.
    if unsafe { CGGetActiveDisplayList(0, std::ptr::null_mut(), &mut count) }.0 != 0 {
        return Err(fail("Cannot enumerate displays"));
    }
    let mut ids = vec![0; count as usize];
    // SAFETY: buffer has count slots and count output remains valid.
    if unsafe { CGGetActiveDisplayList(count, ids.as_mut_ptr(), &mut count) }.0 != 0 {
        return Err(fail("Cannot read displays"));
    }
    ids.truncate(count as usize);
    Ok(ids
        .into_iter()
        .map(|id| DisplayGeometry {
            display_id: id,
            bounds: rect(CGDisplayBounds(id)),
            reported_pixel_width: CGDisplayPixelsWide(id),
            reported_pixel_height: CGDisplayPixelsHigh(id),
            is_main: id == CGMainDisplayID(),
        })
        .collect())
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowGeometry {
    pub window_id: u32,
    pub pid: i64,
    pub bounds: Rect,
}
/// Window Server IDs are discovery handles, not persistent identities across process lifetimes.
pub fn windows() -> Result<Vec<WindowGeometry>> {
    let array = CGWindowListCopyWindowInfo(CGWindowListOption::OptionAll, 0)
        .ok_or_else(|| fail("Window metadata unavailable"))?;
    let mut result = Vec::new();
    for i in 0..array.count() {
        // SAFETY: window info arrays contain retained CF dictionaries.
        let object = unsafe { &*array.value_at_index(i).cast::<CFType>() };
        let Some(dictionary) = object.downcast_ref::<CFDictionary>() else {
            continue;
        };
        let number = |name: &str| -> Option<i64> {
            let key = CFString::from_str(name);
            // SAFETY: CF dictionary key pointer remains live throughout lookup.
            let raw = unsafe { dictionary.value((&*key as *const CFString).cast()) };
            if raw.is_null() {
                return None;
            }
            // SAFETY: returned value remains owned by the live dictionary; type checked below.
            unsafe { &*raw.cast::<CFType>() }
                .downcast_ref::<CFNumber>()?
                .as_i64()
        };
        if let (Some(id), Some(pid)) = (number("kCGWindowNumber"), number("kCGWindowOwnerPID")) {
            let window_id = id as u32;
            if let Ok(bounds) = dictionary_bounds(dictionary) {
                result.push(WindowGeometry {
                    window_id,
                    pid,
                    bounds,
                });
            }
        }
    }
    Ok(result)
}
pub fn main_display_id() -> u32 {
    CGMainDisplayID()
}
pub fn window_bounds(window_id: u32) -> Result<Rect> {
    let array = CGWindowListCopyWindowInfo(CGWindowListOption::OptionIncludingWindow, window_id)
        .ok_or_else(|| fail("Window metadata unavailable"))?;
    if array.count() != 1 {
        return Err(fail("Window no longer exists or is unavailable"));
    }
    // SAFETY: CGWindowListCopyWindowInfo returns CF dictionaries. Runtime downcasts check each value.
    let object = unsafe { &*array.value_at_index(0).cast::<CFType>() };
    let dictionary = object
        .downcast_ref::<CFDictionary>()
        .ok_or_else(|| fail("Invalid window metadata"))?;
    dictionary_bounds(dictionary)
}
fn dictionary_bounds(dictionary: &CFDictionary) -> Result<Rect> {
    let key = CFString::from_str("kCGWindowBounds");
    let raw = unsafe { dictionary.value((&*key as *const CFString).cast()) };
    if raw.is_null() {
        return Err(fail("Missing window bounds"));
    }
    let bounds = unsafe { &*raw.cast::<CFType>() }
        .downcast_ref::<CFDictionary>()
        .ok_or_else(|| fail("Invalid bounds dictionary"))?;
    let mut value = CGRect::default();
    // SAFETY: checked dictionary from system metadata and valid out pointer.
    if !unsafe { CGRectMakeWithDictionaryRepresentation(Some(bounds), &mut value) } {
        return Err(fail("Invalid window rectangle"));
    }
    Ok(rect(value))
}
fn source_state(source: &CaptureSource, all: &[DisplayGeometry]) -> Result<(Rect, Option<i64>)> {
    match source {
        CaptureSource::Window { window_id } => windows()?
            .into_iter()
            .find(|w| w.window_id == *window_id)
            .map(|w| (w.bounds, Some(w.pid)))
            .ok_or_else(|| fail("Window is unavailable")),
        CaptureSource::Display { display_id } => all
            .iter()
            .find(|d| d.display_id == *display_id)
            .map(|d| (d.bounds, None))
            .ok_or_else(|| fail("Display is not active")),
    }
}
pub fn current_revision(source: &CaptureSource) -> Result<String> {
    let all = displays()?;
    let (bounds, owner_pid) = source_state(source, &all)?;
    serde_json::to_string(&(source, bounds, owner_pid, all)).map_err(fail)
}
fn absolute_output(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(fail("Capture output path is empty"));
    }
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir().map_err(fail)?.join(path))
    }
}
/// The helper can only write into a newly created private directory. The user's
/// destination is opened with create_new after capture, then written through its handle.
struct Staging(PathBuf);
impl Staging {
    fn new() -> Result<Self> {
        let name = objc2_foundation::NSUUID::new().UUIDString().to_string();
        let path = std::env::temp_dir().join(format!("unimation-capture-{name}"));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .map_err(fail)?;
        Ok(Self(path))
    }
}
impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct CaptureChild(std::process::Child);
impl Drop for CaptureChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
impl Capture for ScreenshotCapture {
    type Request = CaptureRequest;
    type Frame = Frame;
    fn capture(&mut self, request: CaptureRequest) -> Result<Frame> {
        let output_path = absolute_output(&request.path)?;
        if !CGPreflightScreenCaptureAccess() {
            return Err(error(
                "screen_recording_denied",
                "Grant Screen Recording permission to the launching application",
                Effect::None,
            ));
        }
        let all = displays()?;
        let (bounds, owner_pid) = source_state(&request.source, &all)?;
        if !bounds.valid() {
            return Err(fail("Source has invalid or empty bounds"));
        }
        let revision =
            serde_json::to_string(&(&request.source, bounds, owner_pid, &all)).map_err(fail)?;
        let staging = Staging::new()?;
        let staging_path = staging.0.join("capture.png");
        let mut command = Command::new("/usr/sbin/screencapture");
        command.args(["-x", "-t", "png"]);
        match request.source {
            CaptureSource::Window { window_id } => {
                command.arg("-o").arg(format!("-l{window_id}"));
            }
            CaptureSource::Display { .. } => {
                command.arg(format!(
                    "-R{},{},{},{}",
                    bounds.x, bounds.y, bounds.width, bounds.height
                ));
            }
        }
        command
            .arg(&staging_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = CaptureChild(command.spawn().map_err(fail)?);
        let start = Instant::now();
        loop {
            match child.0.try_wait().map_err(fail)? {
                Some(status) => {
                    if !status.success() {
                        return Err(fail(format!("screencapture exited {status}")));
                    }
                    break;
                }
                None => {
                    if start.elapsed() > Duration::from_secs(15) {
                        let _ = child.0.kill();
                        let _ = child.0.wait();
                        return Err(fail("screencapture timed out"));
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        }
        let bytes = std::fs::read(&staging_path).map_err(fail)?;
        if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" || &bytes[12..16] != b"IHDR" {
            return Err(fail("Capture did not produce a PNG"));
        }
        let pixel_width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
        let pixel_height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
        let mapping = FrameMapping {
            source_bounds: bounds,
            pixel_width,
            pixel_height,
            geometry_revision: revision,
        };
        mapping.validate(&current_revision(&request.source)?)?;
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output_path)
            .map_err(fail)?;
        output.write_all(&bytes).map_err(fail)?;
        Ok(Frame {
            owner_pid,
            source: request.source,
            path: output_path,
            route: "macos_screencapture_executable".into(),
            mapping,
            displays: all,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn flaglike_output_is_absolute() {
        for name in ["-c", "-R-1,-2,100,100", "-42.png", "relative.png"] {
            let path = absolute_output(Path::new(name)).unwrap();
            assert!(path.is_absolute());
            assert_eq!(path.file_name().unwrap(), name);
        }
        assert_eq!(
            absolute_output(Path::new("/tmp/-x.png")).unwrap(),
            PathBuf::from("/tmp/-x.png")
        );
        assert!(absolute_output(Path::new("")).is_err());
    }
    #[test]
    #[ignore = "requires a logged-in macOS desktop and Screen Recording permission"]
    fn capture_live_display() {
        let path =
            PathBuf::from("/tmp").join(format!("unimation-capture-{}.png", std::process::id()));
        let frame = ScreenshotCapture
            .capture(CaptureRequest {
                source: CaptureSource::Display {
                    display_id: main_display_id(),
                },
                path,
            })
            .unwrap();
        println!("{}", serde_json::to_string(&frame).unwrap());
        assert!(frame.mapping.pixel_width > 0);
    }
    #[test]
    #[ignore = "requires a live visible window and Screen Recording permission"]
    fn capture_live_window() {
        let id = std::env::var("UNIMATION_TEST_WINDOW_ID")
            .expect("set native window id")
            .parse()
            .unwrap();
        let path = PathBuf::from("/tmp").join(format!(
            "unimation-window-capture-{}.png",
            std::process::id()
        ));
        let frame = ScreenshotCapture
            .capture(CaptureRequest {
                source: CaptureSource::Window { window_id: id },
                path,
            })
            .unwrap();
        println!("{}", serde_json::to_string(&frame).unwrap());
        assert!(frame.mapping.pixel_width > 0);
    }
}

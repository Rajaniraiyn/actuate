//! ScreenCaptureKit still screenshots, macOS 14+. Callbacks encode on their
//! originating queue; only owned PNG bytes cross back to the caller.
use crate::{
    capture::{CaptureRequest, CaptureSource, Frame, current_revision, displays, windows},
    error,
};
use actuate::{Capture, Effect, Result, geometry::FrameMapping};
use block2::RcBlock;
use objc2::{AllocAnyThread, available};
use objc2_core_foundation::{CFMutableData, CFString};
use objc2_core_graphics::{CGImage, CGPreflightScreenCaptureAccess};
use objc2_foundation::{NSArray, NSError};
use objc2_image_io::CGImageDestination;
use objc2_screen_capture_kit::{
    SCContentFilter, SCScreenshotManager, SCShareableContent, SCStreamConfiguration,
};
use std::{io::Write, sync::mpsc, time::Duration};

/// `max_pixel_edge` requests proportional downscaling by ScreenCaptureKit.
/// None uses the filter's reported point-to-pixel scale. No route falls back implicitly.
#[derive(Default)]
pub struct NativeScreenshotCapture {
    pub max_pixel_edge: Option<u32>,
}
fn fail(message: impl ToString) -> actuate::NativeError {
    error("native_capture_failed", message, Effect::None)
}
fn dimensions(width: f64, height: f64, scale: f64, limit: Option<u32>) -> Result<(usize, usize)> {
    if !width.is_finite()
        || !height.is_finite()
        || !scale.is_finite()
        || width <= 0.
        || height <= 0.
        || scale <= 0.
        || limit == Some(0)
    {
        return Err(fail("Invalid capture dimensions"));
    }
    if !(width * scale).is_finite() || !(height * scale).is_finite() {
        return Err(fail("Capture scale overflow"));
    }
    let factor = limit
        .map(|n| (n as f64 / (width * scale).max(height * scale)).min(1.))
        .unwrap_or(1.);
    let w = (width * scale * factor).round().max(1.);
    let h = (height * scale * factor).round().max(1.);
    if w > 32768. || h > 32768. {
        return Err(fail("Capture dimensions exceed provider allocation limit"));
    }
    Ok((w as usize, h as usize))
}
// SAFETY: caller uses callback-owned image only during its documented callback lifetime.
unsafe fn png(image: *mut CGImage) -> Result<(Vec<u8>, u32, u32)> {
    let image =
        unsafe { image.as_ref() }.ok_or_else(|| fail("ScreenCaptureKit returned no image"))?;
    let data = CFMutableData::new(None, 0).ok_or_else(|| fail("PNG buffer allocation failed"))?;
    let destination =
        unsafe { CGImageDestination::with_data(&data, &CFString::from_str("public.png"), 1, None) }
            .ok_or_else(|| fail("PNG encoder unavailable"))?;
    unsafe { destination.add_image(image, None) };
    if !unsafe { destination.finalize() } {
        return Err(fail("PNG encoding failed"));
    }
    Ok((
        data.to_vec(),
        CGImage::width(Some(image)) as u32,
        CGImage::height(Some(image)) as u32,
    ))
}
impl Capture for NativeScreenshotCapture {
    type Request = CaptureRequest;
    type Frame = Frame;
    fn capture(&mut self, request: CaptureRequest) -> Result<Frame> {
        if !available!(macos = 14.0) {
            return Err(fail("ScreenCaptureKit screenshots require macOS 14"));
        }
        if !CGPreflightScreenCaptureAccess() {
            return Err(error(
                "screen_recording_denied",
                "Screen Recording permission is required",
                Effect::None,
            ));
        }
        if self.max_pixel_edge == Some(0) {
            return Err(fail("max_pixel_edge must be positive"));
        }
        let all = displays()?;
        let (bounds, owner_pid) = match request.source {
            CaptureSource::Display { display_id } => all
                .iter()
                .find(|d| d.display_id == display_id)
                .map(|d| (d.bounds, None))
                .ok_or_else(|| fail("Display unavailable"))?,
            CaptureSource::Window { window_id } => windows()?
                .into_iter()
                .find(|w| w.window_id == window_id)
                .map(|w| (w.bounds, Some(w.pid)))
                .ok_or_else(|| fail("Window unavailable"))?,
        };
        let revision =
            serde_json::to_string(&(&request.source, bounds, owner_pid, &all)).map_err(fail)?;
        let source = request.source.clone();
        let limit = self.max_pixel_edge;
        let (sender, receiver) = mpsc::channel();
        let callback = RcBlock::new(move |content: *mut SCShareableContent, err: *mut NSError| {
            // SAFETY: ScreenCaptureKit supplies these objects for this callback; all native
            // objects remain on this queue and are consumed before the callback returns.
            unsafe {
                if let Some(err) = err.as_ref() {
                    let _ = sender.send(Err(fail(err.localizedDescription())));
                    return;
                }
                let Some(content) = content.as_ref() else {
                    let _ = sender.send(Err(fail("No shareable content")));
                    return;
                };
                let filter = match source {
                    CaptureSource::Display { display_id } => content
                        .displays()
                        .iter()
                        .find(|d| d.displayID() == display_id)
                        .map(|d| {
                            SCContentFilter::initWithDisplay_excludingWindows(
                                SCContentFilter::alloc(),
                                &d,
                                &NSArray::new(),
                            )
                        }),
                    CaptureSource::Window { window_id } => content
                        .windows()
                        .iter()
                        .find(|w| w.windowID() == window_id)
                        .map(|w| {
                            SCContentFilter::initWithDesktopIndependentWindow(
                                SCContentFilter::alloc(),
                                &w,
                            )
                        }),
                };
                let Some(filter) = filter else {
                    let _ = sender.send(Err(fail("Source is absent from shareable content")));
                    return;
                };
                let (width, height) = match dimensions(
                    bounds.width,
                    bounds.height,
                    filter.pointPixelScale() as f64,
                    limit,
                ) {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = sender.send(Err(e));
                        return;
                    }
                };
                let config = SCStreamConfiguration::new();
                config.setWidth(width);
                config.setHeight(height);
                config.setShowsCursor(false);
                config.setIgnoreShadowsSingleWindow(true);
                let complete_sender = sender.clone();
                let complete = RcBlock::new(move |image: *mut CGImage, err: *mut NSError| {
                    let result = if let Some(err) = err.as_ref() {
                        Err(fail(err.localizedDescription()))
                    } else {
                        png(image)
                    };
                    let _ = complete_sender.send(result);
                });
                SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(
                    &filter,
                    &config,
                    Some(&complete),
                );
            }
        });
        // SAFETY: ScreenCaptureKit copies completion blocks for async completion.
        unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&callback) };
        let (bytes, pixel_width, pixel_height) = receiver
            .recv_timeout(Duration::from_secs(15))
            .map_err(|_| {
            fail("ScreenCaptureKit timed out; late callback output is discarded")
        })??;
        let mapping = FrameMapping {
            source_bounds: bounds,
            pixel_width,
            pixel_height,
            geometry_revision: revision,
        };
        mapping.validate(&current_revision(&request.source)?)?;
        let path = if request.path.is_absolute() {
            request.path
        } else {
            std::env::current_dir().map_err(fail)?.join(request.path)
        };
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(fail)?;
        output.write_all(&bytes).map_err(fail)?;
        Ok(Frame {
            owner_pid,
            source: request.source,
            path,
            route: "macos_screen_capture_kit".into(),
            mapping,
            displays: all,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scales_without_upscaling() {
        assert_eq!(
            dimensions(2560., 1080., 2., Some(1280)).unwrap(),
            (1280, 540)
        );
        assert_eq!(dimensions(100., 50., 2., Some(1000)).unwrap(), (200, 100));
        assert!(dimensions(100., 50., 2., Some(0)).is_err());
    }
}

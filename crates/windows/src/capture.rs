use actuate::Result;
use serde_json::{Value, json};
use windows_api::Win32::Graphics::Gdi::*;

struct Context {
    desktop: HDC,
    memory: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
}
impl Drop for Context {
    fn drop(&mut self) {
        unsafe {
            if !self.previous.0.is_null() {
                SelectObject(self.memory, self.previous);
            }
            if !self.bitmap.0.is_null() {
                let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            }
            if !self.memory.0.is_null() {
                let _ = DeleteDC(self.memory);
            }
            if !self.desktop.0.is_null() {
                ReleaseDC(None, self.desktop);
            }
        }
    }
}
/// Capture the composed shared desktop without resizing. The cursor is not included.
/// Protected content, secure desktops and hardware overlays may be absent.
pub fn capture_desktop(path: &str) -> Result<Value> {
    let _dpi = super::windows::DpiGuard::new()?;
    let bounds = super::virtual_desktop()?;
    let bytes = (bounds.width as usize)
        .checked_mul(bounds.height as usize)
        .and_then(|n| n.checked_mul(4))
        .filter(|n| *n <= 512 * 1024 * 1024)
        .ok_or_else(|| {
            super::error("capture_too_large", "Desktop exceeds 512 MiB capture limit")
        })?;
    let mut context = Context {
        desktop: unsafe { GetDC(None) },
        memory: HDC::default(),
        bitmap: HBITMAP::default(),
        previous: HGDIOBJ::default(),
    };
    if context.desktop.0.is_null() {
        return Err(super::error(
            "capture_unavailable",
            "Cannot acquire interactive desktop DC",
        ));
    }
    context.memory = unsafe { CreateCompatibleDC(Some(context.desktop)) };
    if context.memory.0.is_null() {
        return Err(super::error(
            "capture_unavailable",
            "Cannot allocate capture DC",
        ));
    }
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: bounds.width,
            biHeight: -bounds.height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut();
    context.bitmap = unsafe {
        CreateDIBSection(
            Some(context.desktop),
            &info,
            DIB_RGB_COLORS,
            &mut bits,
            None,
            0,
        )
    }
    .map_err(super::native)?;
    if bits.is_null() {
        return Err(super::error(
            "capture_unavailable",
            "Bitmap has no pixel storage",
        ));
    }
    context.previous = unsafe { SelectObject(context.memory, HGDIOBJ(context.bitmap.0)) };
    if context.previous.0.is_null() || context.previous.0 as isize == -1 {
        context.previous = HGDIOBJ::default();
        return Err(super::error(
            "capture_unavailable",
            "Cannot select capture bitmap",
        ));
    }
    unsafe {
        BitBlt(
            context.memory,
            0,
            0,
            bounds.width,
            bounds.height,
            Some(context.desktop),
            bounds.x,
            bounds.y,
            SRCCOPY | CAPTUREBLT,
        )
    }
    .map_err(super::native)?;
    if !unsafe { GdiFlush() }.as_bool() {
        return Err(super::error(
            "capture_unavailable",
            "GDI capture flush failed",
        ));
    }
    // The top-down 32-bit BI_RGB DIB has a tightly packed BGRX row of width*4 bytes.
    let mut pixels = unsafe { std::slice::from_raw_parts(bits.cast::<u8>(), bytes) }.to_vec();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
        pixel[3] = 255;
    }
    let file =
        std::fs::File::create(path).map_err(|e| super::error("capture_file", e.to_string()))?;
    let mut encoder = png::Encoder::new(
        std::io::BufWriter::new(file),
        bounds.width as u32,
        bounds.height as u32,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| super::error("capture_encode", e.to_string()))?;
    writer
        .write_image_data(&pixels)
        .map_err(|e| super::error("capture_encode", e.to_string()))?;
    writer
        .finish()
        .map_err(|e| super::error("capture_encode", e.to_string()))?;
    Ok(
        json!({"path":path,"pixel_width":bounds.width,"pixel_height":bounds.height,"bounds":bounds,"coordinate_space":"physical_desktop_pixels","pixel_to_desktop":{"scale_x":1,"scale_y":1,"offset_x":bounds.x,"offset_y":bounds.y},"cursor_included":false,"route":"windows_gdi_desktop","limitations":["protected content and hardware overlays may be absent","secure desktop unsupported"]}),
    )
}

#[derive(Default)]
pub struct DesktopCapture;
impl actuate::Capture for DesktopCapture {
    type Request = String;
    type Frame = Value;
    fn capture(&mut self, path: String) -> Result<Value> {
        capture_desktop(&path)
    }
}

//! Resolve AX references to their actual native process, window and current geometry.
use crate::{Accessibility, accessibility::attribute, error};
use objc2_application_services::{AXError, AXUIElement, AXValue, AXValueType};
use objc2_core_foundation::{CFArray, CFBoolean, CFRetained, CFString, CGPoint, CGSize};
use serde::{Deserialize, Serialize};
use std::ptr::{self, NonNull};
use unimation_core::{Effect, ElementRef, Point, Result, geometry::Rect};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AxWindow {
    pub reference: ElementRef,
    pub pid: i32,
    pub ax_pid: i32,
    pub window_id: u32,
    pub bounds: Rect,
}
fn bounds(element: &AXUIElement) -> Result<Rect> {
    let position = attribute(element, "AXPosition")?;
    let size = attribute(element, "AXSize")?;
    let p = position
        .downcast_ref::<AXValue>()
        .ok_or_else(|| error("native_type", "AXPosition is not AXValue", Effect::None))?;
    let s = size
        .downcast_ref::<AXValue>()
        .ok_or_else(|| error("native_type", "AXSize is not AXValue", Effect::None))?;
    let mut point = CGPoint::default();
    let mut size = CGSize::default();
    // SAFETY: each AXValue decoder checks the supplied type and writes matching storage.
    if !unsafe { p.value(AXValueType::CGPoint, NonNull::from(&mut point).cast()) }
        || !unsafe { s.value(AXValueType::CGSize, NonNull::from(&mut size).cast()) }
    {
        return Err(error("native_type", "Invalid AX geometry", Effect::None));
    }
    let rect = Rect {
        x: point.x,
        y: point.y,
        width: size.width,
        height: size.height,
    };
    if !rect.valid() {
        return Err(error(
            "invalid_geometry",
            "Element has no finite nonempty bounds",
            Effect::None,
        ));
    }
    Ok(rect)
}
impl Accessibility {
    pub fn element_bounds(&self, target: &ElementRef) -> Result<Rect> {
        {
            let element = self.resolve(target)?;
            bounds(&element)
        }
    }
    pub fn element_center(&self, target: &ElementRef) -> Result<Point> {
        let element = self.resolve(target)?;
        if let Ok(enabled) = attribute(&element, "AXEnabled")
            && let Some(v) = enabled.downcast_ref::<CFBoolean>()
            && !v.as_bool()
        {
            return Err(error("disabled", "Element is disabled", Effect::None));
        }
        let r = bounds(&element)?;
        Ok(Point {
            x: r.x + r.width / 2.,
            y: r.y + r.height / 2.,
        })
    }
    pub fn element_pid(&self, target: &ElementRef) -> Result<i32> {
        let element = self.resolve(target)?;
        let mut pid = 0;
        // SAFETY: owned native element and valid PID output pointer.
        let status = unsafe { element.pid(NonNull::from(&mut pid)) };
        if status != AXError::Success {
            return Err(error(
                format!("ax_{}", status.0),
                "Get native PID",
                Effect::None,
            ));
        }
        Ok(pid)
    }
    pub fn native_window(&mut self, target: &ElementRef) -> Result<AxWindow> {
        let element = self.resolve(target)?;
        let role = attribute(&element, "AXRole")?;
        let window = if role
            .downcast_ref::<CFString>()
            .is_some_and(|s| s.to_string() == "AXWindow")
        {
            element
        } else {
            attribute(&element, "AXWindow")?
                .downcast::<AXUIElement>()
                .map_err(|_| error("no_window", "Target has no native AXWindow", Effect::None))?
        };
        type GetWindow = unsafe extern "C" fn(*const AXUIElement, *mut u32) -> AXError;
        // Private symbol is optional; retain exact native ownership rather than guessing by title.
        let symbol = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"_AXUIElementGetWindow".as_ptr()) };
        if symbol.is_null() {
            return Err(error(
                "unsupported",
                "_AXUIElementGetWindow unavailable",
                Effect::None,
            ));
        }
        // SAFETY: signature follows the native HIServices entry point, out pointer is valid.
        let get_window = unsafe { std::mem::transmute::<*mut libc::c_void, GetWindow>(symbol) };
        let mut id = 0;
        let status = unsafe { get_window(&*window, &mut id) };
        if status != AXError::Success || id == 0 {
            return Err(error(
                "no_window",
                format!("Native window ID unavailable: {}", status.0),
                Effect::None,
            ));
        }
        let reference = self.intern(&window);
        let ax_pid = self.element_pid(&reference)?;
        // Native input routes to the Window Server owner; AX can belong to a view service.
        let native = crate::capture::windows()?
            .into_iter()
            .find(|w| w.window_id == id)
            .ok_or_else(|| {
                error(
                    "no_window",
                    "Window Server metadata unavailable",
                    Effect::None,
                )
            })?;
        let pid = i32::try_from(native.pid)
            .map_err(|_| error("no_window", "Invalid owner PID", Effect::None))?;
        Ok(AxWindow {
            reference,
            pid,
            ax_pid,
            window_id: id,
            bounds: native.bounds,
        })
    }
    pub fn hit_test(&mut self, point: Point) -> Result<ElementRef> {
        if !point.x.is_finite() || !point.y.is_finite() {
            return Err(error(
                "invalid_geometry",
                "Finite desktop point required",
                Effect::None,
            ));
        }
        let mut raw = ptr::null();
        // SAFETY: native system proxy and valid output pointer. Copy returns retained ownership.
        unsafe {
            let system = AXUIElement::new_system_wide();
            let status = system.copy_element_at_position(
                point.x as f32,
                point.y as f32,
                NonNull::from(&mut raw),
            );
            if status != AXError::Success {
                return Err(error(
                    format!("ax_{}", status.0),
                    "AX hit test failed",
                    Effect::None,
                ));
            }
            let element =
                CFRetained::from_raw(NonNull::new(raw.cast_mut()).ok_or_else(|| {
                    error("native_null", "AX hit test returned null", Effect::None)
                })?);
            Ok(self.intern(&element))
        }
    }
}

impl Accessibility {
    /// Known native scroll-area/window bounds constrain reference-center clicks.
    /// This does not establish occlusion or clipping by arbitrary custom views.
    pub fn require_point_in_viewport(&self, target: &ElementRef, point: &Point) -> Result<()> {
        let mut element = self.resolve(target)?;
        let mut seen = Vec::new();
        for _ in 0..64 {
            let role = attribute(&element, "AXRole")
                .ok()
                .and_then(|v| v.downcast::<CFString>().ok())
                .map(|v| v.to_string());
            if matches!(role.as_deref(), Some("AXScrollArea" | "AXWindow"))
                && let Ok(rect) = bounds(&element)
                && (point.x < rect.x
                    || point.y < rect.y
                    || point.x >= rect.x + rect.width
                    || point.y >= rect.y + rect.height)
            {
                return Err(error(
                    "outside_viewport",
                    "Reference center is outside an observed native scroll area or window; scroll and observe again",
                    Effect::None,
                ));
            }
            seen.push(element.clone());
            element = match attribute(&element, "AXParent").and_then(|v| {
                v.downcast::<AXUIElement>()
                    .map_err(|_| error("native_type", "Invalid AXParent", Effect::None))
            }) {
                Ok(parent) if !seen.iter().any(|v| **v == *parent) => parent,
                _ => break,
            };
        }
        Ok(())
    }
    /// Reject only explicit native hidden/minimized/disabled evidence. Missing
    /// visibility attributes are unknown, never assumed hidden or visible.
    pub fn require_pointer_access(&self, target: &ElementRef) -> Result<()> {
        let mut element = self.resolve(target)?;
        let mut seen = Vec::new();
        for depth in 0..64 {
            for (name, blocked) in [
                ("AXHidden", true),
                ("AXVisible", false),
                ("AXMinimized", true),
            ] {
                if let Ok(value) = attribute(&element, name)
                    && value
                        .downcast_ref::<CFBoolean>()
                        .is_some_and(|v| v.as_bool() == blocked)
                {
                    return Err(error(
                        "not_visible",
                        format!(
                            "Native {name} blocks reference pointer input at ancestor depth {depth}"
                        ),
                        Effect::None,
                    ));
                }
            }
            if depth == 0
                && let Ok(value) = attribute(&element, "AXEnabled")
                && value
                    .downcast_ref::<CFBoolean>()
                    .is_some_and(|v| !v.as_bool())
            {
                return Err(error("disabled", "Element is disabled", Effect::None));
            }
            seen.push(element.clone());
            element = match attribute(&element, "AXParent").and_then(|v| {
                v.downcast::<AXUIElement>()
                    .map_err(|_| error("native_type", "Invalid AXParent", Effect::None))
            }) {
                Ok(parent) if !seen.iter().any(|v| **v == *parent) => parent,
                _ => break,
            };
        }
        self.require_sheet_access(target)
    }
    /// Native hit testing can see through an attached modal sheet. Check the
    /// owning window's advertised sheets before dispatching reference input.
    pub fn require_sheet_access(&self, target: &ElementRef) -> Result<()> {
        let intended = self.resolve(target)?;
        let window = if attribute(&intended, "AXRole")?
            .downcast_ref::<CFString>()
            .is_some_and(|s| s.to_string() == "AXWindow")
        {
            intended.clone()
        } else {
            match attribute(&intended, "AXWindow").and_then(|v| {
                v.downcast::<AXUIElement>()
                    .map_err(|_| error("no_window", "No owning AX window", Effect::None))
            }) {
                Ok(w) => w,
                // Menu-bar and other system elements need not belong to a window.
                Err(_) => return Ok(()),
            }
        };
        let mut sheets = Vec::new();
        for name in ["AXSheets", "AXChildren"] {
            if let Ok(value) = attribute(&window, name)
                && let Some(array) = value.downcast_ref::<CFArray>()
            {
                for child in crate::accessibility::cf_items(array) {
                    if let Ok(child) = child.downcast::<AXUIElement>()
                        && attribute(&child, "AXRole")
                            .ok()
                            .and_then(|v| v.downcast::<CFString>().ok())
                            .is_some_and(|s| s.to_string() == "AXSheet")
                    {
                        sheets.push(child);
                    }
                }
            }
        }
        if sheets.is_empty() {
            return Ok(());
        }
        let mut ancestor = intended;
        for _ in 0..64 {
            if sheets.iter().any(|sheet| **sheet == *ancestor) {
                return Ok(());
            }
            ancestor = match attribute(&ancestor, "AXParent").and_then(|v| {
                v.downcast::<AXUIElement>()
                    .map_err(|_| error("native_type", "Invalid AXParent", Effect::None))
            }) {
                Ok(parent) => parent,
                Err(_) => break,
            };
        }
        Err(error(
            "blocked_by_modal",
            "Owning window has an attached sheet; target the sheet or dismiss it first",
            Effect::None,
        ))
    }
    /// Ensure a global coordinate still resolves to the requested element or one
    /// of its descendants. No focus or z-order mutations are performed.
    pub fn require_hit(&mut self, target: &ElementRef, point: &Point) -> Result<()> {
        self.require_pointer_access(target)?;
        self.require_point_in_viewport(target, point)?;
        let intended = self.resolve(target)?;
        let hit = self.hit_test(point.clone())?;
        let mut element = self.resolve(&hit)?;
        for _ in 0..64 {
            if *element == *intended {
                return Ok(());
            }
            element = match attribute(&element, "AXParent").and_then(|p| {
                p.downcast::<AXUIElement>()
                    .map_err(|_| error("native_type", "AXParent is not an element", Effect::None))
            }) {
                Ok(p) => p,
                Err(_) => break,
            };
        }
        Err(error(
            "occluded_or_moved",
            "Global hit test does not reach the requested element",
            Effect::None,
        ))
    }
}

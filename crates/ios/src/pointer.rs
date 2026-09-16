//! Experimental native guest mouse, distinct from the touch provider.
//! Native SimulatorKit constructors encode service and IOHID pointer messages.
//! This provider moves the guest's shared pointer. It never posts a host event.
//! Service target 0x36 is guest-wide; use a dedicated simulator for exclusive
//! ownership. Call `enable` explicitly before dispatch and `close` afterward.
use crate::SimulatorHid;
use std::{ffi::c_void, path::Path, time::Duration};
use unimation::{Effect, NativeError, Receipt, Result};
type Service = unsafe extern "C" fn() -> *mut c_void;
type Convert = unsafe extern "C" fn(*const c_void, u32) -> *mut c_void;
type Relative =
    unsafe extern "C" fn(*const c_void, u64, f64, f64, f64, u32, u32, u32) -> *mut c_void;
type Append = unsafe extern "C" fn(*mut c_void, *const c_void, u32);
type Children = unsafe extern "C" fn(*const c_void) -> *const c_void;
unsafe extern "C" {
    fn mach_absolute_time() -> u64;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(value: *const c_void);
}
fn error(code: &str, message: impl Into<String>, effect: Effect) -> NativeError {
    NativeError {
        code: code.into(),
        message: message.into(),
        effect,
    }
}
struct Event(*mut c_void);
impl Drop for Event {
    fn drop(&mut self) {
        unsafe {
            CFRelease(self.0);
        }
    }
}
struct Constructors {
    create: Service,
    remove: Service,
    convert: Convert,
    relative: Relative,
    append: Append,
    children: Children,
}
impl Constructors {
    fn load() -> Result<Self> {
        unsafe {
            let sim = crate::hid::simulator_kit()?;
            let io = libc::dlopen(
                c"/System/Library/Frameworks/IOKit.framework/IOKit".as_ptr(),
                libc::RTLD_NOW | libc::RTLD_GLOBAL,
            );
            if io.is_null() {
                return Err(error("framework_unavailable", "IOKit", Effect::None));
            }
            use crate::hid::symbol;
            Ok(Self {
                create: std::mem::transmute::<*mut c_void, Service>(symbol(
                    sim,
                    c"IndigoHIDMessageToCreateMouseService",
                )?),
                remove: std::mem::transmute::<*mut c_void, Service>(symbol(
                    sim,
                    c"IndigoHIDMessageToRemoveMouseService",
                )?),
                convert: std::mem::transmute::<*mut c_void, Convert>(symbol(
                    sim,
                    c"IndigoHIDMessageForPointerEventFromHIDEventRef",
                )?),
                relative: std::mem::transmute::<*mut c_void, Relative>(symbol(
                    io,
                    c"IOHIDEventCreateRelativePointerEvent",
                )?),
                append: std::mem::transmute::<*mut c_void, Append>(symbol(
                    io,
                    c"IOHIDEventAppendEvent",
                )?),
                children: std::mem::transmute::<*mut c_void, Children>(symbol(
                    io,
                    c"IOHIDEventGetChildren",
                )?),
            })
        }
    }
    fn packet(&self, dx: f64, dy: f64, buttons: u32, previous: u32) -> Result<*mut c_void> {
        unsafe {
            let event = (self.relative)(
                std::ptr::null(),
                mach_absolute_time(),
                dx,
                dy,
                0.0,
                buttons,
                previous,
                0,
            );
            if event.is_null() {
                return Err(error(
                    "hid_message_failed",
                    "IOHID pointer allocation failed",
                    Effect::None,
                ));
            }
            let event = Event(event);
            // The converter requires a composite event. Button transitions already have
            // children. Pure motion gets a zero-displacement child with the same mask.
            // Main motion remains in the parent; no movement is duplicated.
            if (self.children)(event.0).is_null() {
                let child = (self.relative)(
                    std::ptr::null(),
                    mach_absolute_time(),
                    0.0,
                    0.0,
                    0.0,
                    buttons,
                    buttons,
                    0,
                );
                if child.is_null() {
                    return Err(error(
                        "hid_message_failed",
                        "IOHID child allocation failed",
                        Effect::None,
                    ));
                }
                let child = Event(child);
                (self.append)(event.0, child.0, 0);
            }
            let packet = (self.convert)(event.0, 0x36);
            if packet.is_null() {
                return Err(error(
                    "hid_message_failed",
                    "Native pointer conversion failed",
                    Effect::None,
                ));
            }
            Ok(packet)
        }
    }
}
/// Experimental relative guest mouse. `connect` performs no UI input. Native
/// service ownership begins only with `enable`; absolute coordinates and cursor
/// visibility are not inferred from accessibility geometry.
pub struct SimulatorPointer {
    transport: SimulatorHid,
    constructors: Constructors,
    enabled: bool,
    buttons: u32,
}
impl SimulatorPointer {
    pub fn connect(udid: &str, path: &Path) -> Result<Self> {
        Ok(Self {
            transport: SimulatorHid::connect(udid, path)?,
            constructors: Constructors::load()?,
            enabled: false,
            buttons: 0,
        })
    }
    pub fn enable(&mut self) -> Result<Receipt> {
        if self.enabled {
            return Err(error(
                "already_enabled",
                "This provider already enabled its guest mouse service",
                Effect::None,
            ));
        }
        self.enabled = true;
        self.transport
            .send(unsafe { (self.constructors.create)() })?;
        Ok(receipt())
    }
    pub fn move_relative(&mut self, dx: f64, dy: f64) -> Result<Receipt> {
        self.dispatch(dx, dy, self.buttons)
    }
    /// Follow a shared relative-count plan using the explicitly enabled guest mouse.
    /// Guest acceleration still determines the screen endpoint. Held buttons remain
    /// held, and a failed sample is never retried.
    pub fn move_smooth(&mut self, plan: &unimation::motion::RelativeMotionPlan) -> Result<Receipt> {
        if !self.enabled {
            return Err(error(
                "not_enabled",
                "Enable the guest mouse service first",
                Effect::None,
            ));
        }
        let start = std::time::Instant::now();
        let mut dispatched = false;
        for sample in plan.samples() {
            if let Some(wait) = sample.at.checked_sub(start.elapsed()) {
                std::thread::sleep(wait);
            }
            self.move_relative(f64::from(sample.dx), f64::from(sample.dy))
                .map_err(|mut failure| {
                    if dispatched {
                        failure.effect = Effect::Unknown;
                    }
                    failure
                })?;
            dispatched = true;
        }
        Ok(Receipt {
            effect: if dispatched {
                Effect::Dispatched
            } else {
                Effect::None
            },
            route: "ios.simulator.indigo.mouse".into(),
        })
    }
    pub fn button(&mut self, button: u8, down: bool) -> Result<Receipt> {
        if !(1..=8).contains(&button) {
            return Err(error(
                "invalid_button",
                "Guest mouse buttons are numbered 1 through 8",
                Effect::None,
            ));
        }
        let bit = 1u32 << (button - 1);
        let mask = if down {
            self.buttons | bit
        } else {
            self.buttons & !bit
        };
        self.dispatch(0.0, 0.0, mask)
    }
    pub fn click(&mut self, button: u8) -> Result<Receipt> {
        if let Err(e) = self.button(button, true) {
            let _ = self.button(button, false);
            return Err(e);
        }
        std::thread::sleep(Duration::from_millis(40));
        self.button(button, false)
    }
    pub fn close(&mut self) -> Result<Receipt> {
        if !self.enabled {
            return Err(error(
                "not_enabled",
                "Guest mouse service is not enabled",
                Effect::None,
            ));
        }
        if self.buttons != 0 {
            let _ = self.dispatch(0.0, 0.0, 0);
        }
        let result = self.transport.send(unsafe { (self.constructors.remove)() });
        if result.is_ok() {
            self.enabled = false;
        }
        result.map(|_| receipt())
    }
    fn dispatch(&mut self, dx: f64, dy: f64, buttons: u32) -> Result<Receipt> {
        if !self.enabled {
            return Err(error(
                "not_enabled",
                "Enable the guest mouse service first",
                Effect::None,
            ));
        }
        if !dx.is_finite() || !dy.is_finite() || dx.abs() > 100_000.0 || dy.abs() > 100_000.0 {
            return Err(error(
                "invalid_delta",
                "Expected finite relative mouse deltas within +/-100000",
                Effect::None,
            ));
        }
        let packet = self.constructors.packet(dx, dy, buttons, self.buttons)?;
        self.buttons = buttons;
        self.transport.send(packet)?;
        Ok(receipt())
    }
}
impl Drop for SimulatorPointer {
    fn drop(&mut self) {
        if self.enabled {
            let _ = self.close();
        }
    }
}
fn receipt() -> Receipt {
    Receipt {
        effect: Effect::Dispatched,
        route: "ios.simulator.indigo.mouse".into(),
    }
}
impl unimation::RelativePointerInput for SimulatorPointer {
    fn move_relative(&mut self, dx: f64, dy: f64) -> Result<Receipt> {
        SimulatorPointer::move_relative(self, dx, dy)
    }
    fn pointer_button(&mut self, button: u8, down: bool) -> Result<Receipt> {
        self.button(button, down)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires installed private SimulatorKit; creates native objects only, no dispatch"]
    fn native_packet_preserves_relative_delta() {
        let constructors = Constructors::load().unwrap();
        let packet = constructors.packet(12.5, -4.5, 0, 0).unwrap();
        unsafe {
            let bytes = std::slice::from_raw_parts(packet as *const u8, 0xc0);
            assert_eq!(
                f64::from_ne_bytes(bytes[0x30..0x38].try_into().unwrap()),
                12.5
            );
            assert_eq!(
                f64::from_ne_bytes(bytes[0x38..0x40].try_into().unwrap()),
                -4.5
            );
            assert_eq!(bytes[0x1c], 2);
            libc::free(packet);
        }
    }
}

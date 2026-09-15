//! Direct CoreSimulator HID injection through SimulatorKit's Indigo transport.
//! The MouseNSEvent symbol creates guest touch contacts, not a guest mouse cursor.
//! Private message layout follows the observed idb/accessibility-cli transport.
#![allow(unsafe_op_in_unsafe_fn)]
use block2::{Block, RcBlock};
use objc2::{
    msg_send,
    rc::Retained,
    runtime::{AnyClass, AnyObject, Bool, Sel},
    sel,
};
use objc2_core_foundation::{CGPoint, CGSize};
use objc2_foundation::NSString;
use serde::{Deserialize, Serialize};
use std::{
    ffi::{CStr, CString, c_void},
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use unimation::{Effect, NativeError, Receipt, Result};
fn error(code: &str, message: impl Into<String>, effect: Effect) -> NativeError {
    NativeError {
        code: code.into(),
        message: message.into(),
        effect,
    }
}
unsafe extern "C" {
    fn dispatch_queue_create(label: *const libc::c_char, attr: *const c_void) -> *mut c_void;
    fn dispatch_release(queue: *mut c_void);
    fn mach_absolute_time() -> u64;
    fn malloc_size(ptr: *const c_void) -> usize;
}
type TouchMessage =
    unsafe extern "C" fn(*const CGPoint, *const CGPoint, u32, u64, CGSize, u32) -> *mut c_void;
type ButtonMessage = unsafe extern "C" fn(u32, u32, u32) -> *mut c_void;
type ArbitraryMessage = unsafe extern "C" fn(u32, u32, u32, u32) -> *mut c_void;
type KeyMessage = unsafe extern "C" fn(u32, u32) -> *mut c_void;
#[derive(Debug, Clone, Copy, Serialize)]
pub struct HidGeometry {
    pub pixel_width: f64,
    pub pixel_height: f64,
    pub scale: f64,
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TouchEdge {
    #[default]
    None,
    Left,
    Top,
    Bottom,
    Right,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HardwareButton {
    Home,
    Lock,
    Side,
    Siri,
}
struct Owner {
    client: usize,
    device: usize,
    queue: usize,
}
impl Drop for Owner {
    fn drop(&mut self) {
        unsafe {
            drop(Retained::from_raw(self.client as *mut AnyObject));
            drop(Retained::from_raw(self.device as *mut AnyObject));
            dispatch_release(self.queue as *mut c_void);
        }
    }
}
/// Input provider independent of accessibility observation. All coordinates use
/// unrotated framebuffer ratios. No host keyboard or mouse input is dispatched.
/// A send has a five-second completion deadline. An error after any dispatch is
/// reported with unknown effect; no transport fallback is attempted.
pub struct SimulatorHid {
    owner: Arc<Owner>,
    geometry: HidGeometry,
    touch_message: TouchMessage,
    button_message: ButtonMessage,
    key_message: KeyMessage,
    arbitrary_message: Option<ArbitraryMessage>,
    _thread: std::marker::PhantomData<*mut ()>,
}
unsafe fn require(receiver: *mut AnyObject, selector: Sel) -> Result<()> {
    let yes: Bool = msg_send![receiver,respondsToSelector:selector];
    if yes.as_bool() {
        Ok(())
    } else {
        Err(error(
            "private_api_unavailable",
            selector.name().to_string_lossy(),
            Effect::None,
        ))
    }
}
unsafe fn string(object: *mut AnyObject) -> Option<String> {
    if object.is_null() {
        return None;
    }
    let p: *const libc::c_char = msg_send![object, UTF8String];
    (!p.is_null()).then(|| CStr::from_ptr(p).to_string_lossy().into_owned())
}
fn load(path: &str) -> Result<*mut c_void> {
    let path = CString::new(path)
        .map_err(|_| error("invalid_path", "NUL in framework path", Effect::None))?;
    let handle = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL) };
    if handle.is_null() {
        Err(error(
            "framework_unavailable",
            path.to_string_lossy(),
            Effect::None,
        ))
    } else {
        Ok(handle)
    }
}
pub(crate) unsafe fn symbol(handle: *mut c_void, name: &CStr) -> Result<*mut c_void> {
    let symbol = libc::dlsym(handle, name.as_ptr());
    if symbol.is_null() {
        Err(error(
            "private_api_unavailable",
            name.to_string_lossy(),
            Effect::None,
        ))
    } else {
        Ok(symbol)
    }
}
pub(crate) fn simulator_kit() -> Result<*mut c_void> {
    let mut paths = vec![];
    if let Ok(dir) = std::env::var("DEVELOPER_DIR") {
        paths.push(format!(
            "{dir}/../SharedFrameworks/SimulatorKit.framework/SimulatorKit"
        ));
        paths.push(format!(
            "{dir}/Library/PrivateFrameworks/SimulatorKit.framework/SimulatorKit"
        ));
    }
    paths.extend(["/Applications/Xcode.app/Contents/SharedFrameworks/SimulatorKit.framework/SimulatorKit".into(),"/Applications/Xcode.app/Contents/Developer/Library/PrivateFrameworks/SimulatorKit.framework/SimulatorKit".into()]);
    for path in paths {
        if let Ok(handle) = load(&path) {
            return Ok(handle);
        }
    }
    Err(error(
        "framework_unavailable",
        "Installed SimulatorKit was not found",
        Effect::None,
    ))
}
unsafe fn device(udid: &str, path: &Path) -> Result<Retained<AnyObject>> {
    if !path.is_absolute() || !path.is_dir() {
        return Err(error(
            "invalid_device_set",
            "Expected existing absolute device-set directory",
            Effect::None,
        ));
    }
    load("/Library/Developer/PrivateFrameworks/CoreSimulator.framework/CoreSimulator")?;
    let cls = AnyClass::get(c"SimServiceContext")
        .ok_or_else(|| error("private_api_unavailable", "SimServiceContext", Effect::None))?;
    require(
        cls as *const _ as *mut _,
        sel!(sharedServiceContextForDeveloperDir:error:),
    )?;
    let mut err: *mut AnyObject = std::ptr::null_mut();
    let context: *mut AnyObject = msg_send![cls,sharedServiceContextForDeveloperDir:std::ptr::null::<AnyObject>(),error:&mut err];
    if context.is_null() {
        return Err(error(
            "simulator_context",
            "Cannot connect to CoreSimulator service",
            Effect::None,
        ));
    }
    require(context, sel!(deviceSetWithPath:error:))?;
    let path = NSString::from_str(
        path.to_str()
            .ok_or_else(|| error("invalid_device_set", "Path must be UTF-8", Effect::None))?,
    );
    let set: *mut AnyObject = msg_send![context,deviceSetWithPath:&*path,error:&mut err];
    if set.is_null() {
        return Err(error(
            "device_set_unavailable",
            "CoreSimulator rejected device set",
            Effect::None,
        ));
    }
    let devices: *mut AnyObject = msg_send![set, availableDevices];
    let count: usize = msg_send![devices, count];
    for i in 0..count {
        let d: *mut AnyObject = msg_send![devices,objectAtIndex:i];
        let uuid: *mut AnyObject = msg_send![d, UDID];
        let uuid: *mut AnyObject = msg_send![uuid, UUIDString];
        if string(uuid).is_some_and(|v| v.eq_ignore_ascii_case(udid)) {
            let state: usize = msg_send![d, state];
            if state != 3 {
                return Err(error("simulator_not_booted", udid, Effect::None));
            }
            return Retained::retain(d)
                .ok_or_else(|| error("simulator_not_found", udid, Effect::None));
        }
    }
    Err(error("simulator_not_found", udid, Effect::None))
}
fn validate_ratio(point: (f64, f64)) -> Result<()> {
    if !point.0.is_finite()
        || !point.1.is_finite()
        || !(0.0..=1.0).contains(&point.0)
        || !(0.0..=1.0).contains(&point.1)
    {
        return Err(error(
            "invalid_coordinate",
            "Expected finite framebuffer ratios in [0, 1]",
            Effect::None,
        ));
    }
    Ok(())
}
fn validate_duration(ms: u64) -> Result<()> {
    if ms > 10_000 {
        return Err(error(
            "invalid_duration",
            "Input duration must not exceed 10000 ms",
            Effect::None,
        ));
    }
    Ok(())
}
impl SimulatorHid {
    pub fn connect(udid: &str, path: &Path) -> Result<Self> {
        objc2::rc::autoreleasepool(|_| unsafe {
            let device = device(udid, path)?;
            let handle = simulator_kit()?;
            let arbitrary = libc::dlsym(handle, c"IndigoHIDMessageForHIDArbitrary".as_ptr());
            let arbitrary_message = if arbitrary.is_null() {
                None
            } else {
                Some(std::mem::transmute::<*mut c_void, ArbitraryMessage>(
                    arbitrary,
                ))
            };
            let touch_message = std::mem::transmute::<*mut c_void, TouchMessage>(symbol(
                handle,
                c"IndigoHIDMessageForMouseNSEvent",
            )?);
            let button_message = std::mem::transmute::<*mut c_void, ButtonMessage>(symbol(
                handle,
                c"IndigoHIDMessageForButton",
            )?);
            let key_message = std::mem::transmute::<*mut c_void, KeyMessage>(symbol(
                handle,
                c"IndigoHIDMessageForKeyboardArbitrary",
            )?);
            let cls = AnyClass::get(c"SimulatorKit.SimDeviceLegacyHIDClient")
                .or_else(|| AnyClass::get(c"_TtC12SimulatorKit24SimDeviceLegacyHIDClient"))
                .ok_or_else(|| {
                    error(
                        "private_api_unavailable",
                        "SimDeviceLegacyHIDClient",
                        Effect::None,
                    )
                })?;
            let allocated: *mut AnyObject = msg_send![cls, alloc];
            require(
                allocated,
                sel!(initWithDevice:sessionResetQueue:error:sessionResetHandler:),
            )?;
            let mut err: *mut AnyObject = std::ptr::null_mut();
            let client: *mut AnyObject = msg_send![allocated,initWithDevice:&*device,sessionResetQueue:std::ptr::null::<AnyObject>(),error:&mut err,sessionResetHandler:std::ptr::null::<AnyObject>()];
            let client = Retained::from_raw(client).ok_or_else(|| {
                error(
                    "hid_connection_failed",
                    "SimulatorKit rejected HID client",
                    Effect::None,
                )
            })?;
            require(
                Retained::as_ptr(&client) as *mut _,
                sel!(sendWithMessage:freeWhenDone:completionQueue:completion:),
            )?;
            let kind: *mut AnyObject = msg_send![&*device, deviceType];
            require(kind, sel!(mainScreenSize))?;
            require(kind, sel!(mainScreenScale))?;
            let size: CGSize = msg_send![kind, mainScreenSize];
            let scale: f32 = msg_send![kind, mainScreenScale];
            if !size.width.is_finite()
                || !size.height.is_finite()
                || size.width <= 0.0
                || size.height <= 0.0
                || !scale.is_finite()
                || scale <= 0.0
            {
                return Err(error(
                    "invalid_geometry",
                    "Simulator reported invalid native display geometry",
                    Effect::None,
                ));
            }
            let queue =
                dispatch_queue_create(c"unimation.simulator.hid".as_ptr(), std::ptr::null());
            if queue.is_null() {
                return Err(error(
                    "allocation_failed",
                    "Cannot create HID completion queue",
                    Effect::None,
                ));
            }
            Ok(Self {
                owner: Arc::new(Owner {
                    client: Retained::into_raw(client) as usize,
                    device: Retained::into_raw(device) as usize,
                    queue: queue as usize,
                }),
                geometry: HidGeometry {
                    pixel_width: size.width,
                    pixel_height: size.height,
                    scale: scale as f64,
                },
                touch_message,
                button_message,
                key_message,
                arbitrary_message,
                _thread: std::marker::PhantomData,
            })
        })
    }
    pub fn geometry(&self) -> HidGeometry {
        self.geometry
    }
    pub(crate) fn send(&self, message: *mut c_void) -> Result<()> {
        if message.is_null() {
            return Err(error(
                "hid_message_failed",
                "Native message allocation returned null",
                Effect::None,
            ));
        }
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let owner = self.owner.clone();
        let completion = RcBlock::new(move |err: *mut AnyObject| {
            let _keep_alive = &owner;
            let result = unsafe {
                if err.is_null() {
                    None
                } else {
                    let desc: *mut AnyObject = msg_send![err, localizedDescription];
                    Some(string(desc).unwrap_or_else(|| "Native HID transport failed".into()))
                }
            };
            let _ = tx.send(result);
        });
        unsafe {
            type Send = unsafe extern "C" fn(
                *mut AnyObject,
                Sel,
                *mut c_void,
                Bool,
                *mut AnyObject,
                *const Block<dyn Fn(*mut AnyObject)>,
            );
            let send: Send = std::mem::transmute(objc2::ffi::objc_msgSend as *const c_void);
            send(
                self.owner.client as *mut _,
                sel!(sendWithMessage:freeWhenDone:completionQueue:completion:),
                message,
                Bool::YES,
                self.owner.queue as *mut _,
                &*completion,
            );
        }
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(None) => Ok(()),
            Ok(Some(message)) => Err(error("hid_send_failed", message, Effect::Unknown)),
            Err(_) => Err(error(
                "hid_timeout",
                "HID completion exceeded five seconds",
                Effect::Unknown,
            )),
        }
    }
    fn contact(&self, point: (f64, f64), down: bool, edge: TouchEdge) -> Result<()> {
        unsafe {
            let p = CGPoint {
                x: point.0,
                y: point.1,
            };
            let edge = match edge {
                TouchEdge::None => 0,
                TouchEdge::Left => 1,
                TouchEdge::Top => 2,
                TouchEdge::Bottom => 3,
                TouchEdge::Right => 4,
            };
            let template = (self.touch_message)(
                &p,
                std::ptr::null(),
                0x32,
                if down { 1 } else { 2 },
                CGSize {
                    width: 1.0,
                    height: 1.0,
                },
                edge,
            );
            if template.is_null() {
                return Err(error(
                    "hid_message_failed",
                    "Touch template allocation failed",
                    Effect::None,
                ));
            }
            if malloc_size(template) < 0xa0 {
                libc::free(template);
                return Err(error(
                    "private_abi_changed",
                    "Touch template is shorter than the supported layout",
                    Effect::None,
                ));
            }
            let bytes = touch_packet(
                std::slice::from_raw_parts(template as *const u8, 0xa0),
                point,
                down,
                mach_absolute_time(),
            );
            libc::free(template);
            let message = libc::malloc(bytes.len());
            if message.is_null() {
                return Err(error(
                    "allocation_failed",
                    "Touch message allocation failed",
                    Effect::None,
                ));
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), message as *mut u8, bytes.len());
            self.send(message)
        }
    }
    pub fn tap_ratio(&mut self, x: f64, y: f64) -> Result<Receipt> {
        validate_ratio((x, y))?;
        if let Err(e) = self.contact((x, y), true, TouchEdge::None) {
            let _ = self.contact((x, y), false, TouchEdge::None);
            return Err(after_dispatch(e));
        }
        std::thread::sleep(Duration::from_millis(50));
        self.contact((x, y), false, TouchEdge::None)
            .map_err(after_dispatch)?;
        Ok(receipt("touch"))
    }
    pub fn swipe_ratio(
        &mut self,
        from: (f64, f64),
        to: (f64, f64),
        duration_ms: u64,
        edge: TouchEdge,
    ) -> Result<Receipt> {
        validate_ratio(from)?;
        validate_ratio(to)?;
        validate_duration(duration_ms)?;
        if duration_ms == 0 {
            return Err(error(
                "invalid_duration",
                "Swipe duration must be positive",
                Effect::None,
            ));
        }
        if let Err(e) = self.contact(from, true, edge) {
            let _ = self.contact(from, false, edge);
            return Err(after_dispatch(e));
        }
        let steps = (duration_ms / 16).clamp(1, 600);
        let start = Instant::now();
        for i in 1..=steps {
            let fraction = i as f64 / steps as f64;
            let expected = Duration::from_millis(duration_ms * i / steps);
            if let Some(wait) = expected.checked_sub(start.elapsed()) {
                std::thread::sleep(wait);
            }
            let p = (
                from.0 + (to.0 - from.0) * fraction,
                from.1 + (to.1 - from.1) * fraction,
            );
            if let Err(mut failure) = self.contact(p, true, edge) {
                failure.effect = Effect::Unknown;
                let _ = self.contact(p, false, edge);
                return Err(failure);
            }
        }
        self.contact(to, false, edge).map_err(after_dispatch)?;
        Ok(receipt("touch"))
    }
    pub fn press_hardware(&mut self, button: HardwareButton, hold_ms: u64) -> Result<Receipt> {
        validate_duration(hold_ms)?;
        let code = match button {
            HardwareButton::Home => 0,
            HardwareButton::Lock => 1,
            HardwareButton::Side => 0xbb8,
            HardwareButton::Siri => 0x400002,
        };
        if let Err(e) = self.send(unsafe { (self.button_message)(code, 1, 0x33) }) {
            let _ = self.send(unsafe { (self.button_message)(code, 2, 0x33) });
            return Err(after_dispatch(e));
        }
        std::thread::sleep(Duration::from_millis(hold_ms.max(50)));
        self.send(unsafe { (self.button_message)(code, 2, 0x33) })
            .map_err(after_dispatch)?;
        Ok(receipt("hardware"))
    }
    /// USB HID keyboard usage page 0x07. Modifiers are usages 0xe0..=0xe7.
    pub fn key(&mut self, usage: u16, modifiers: &[u16]) -> Result<Receipt> {
        if usage == 0
            || usage > 0xff
            || modifiers.len() > 8
            || modifiers.iter().any(|m| !(0xe0..=0xe7).contains(m))
        {
            return Err(error(
                "invalid_key",
                "Expected USB HID keyboard usage and modifier usages 224..231",
                Effect::None,
            ));
        }
        let mut held = vec![];
        for &modifier in modifiers {
            if held.contains(&modifier) {
                continue;
            }
            held.push(modifier);
            if let Err(e) = self.send(unsafe { (self.key_message)(modifier as u32, 1) }) {
                self.release_keys(&held);
                return Err(after_dispatch(e));
            }
        }
        held.push(usage);
        let down = self.send(unsafe { (self.key_message)(usage as u32, 1) });
        if let Err(e) = down {
            self.release_keys(&held);
            return Err(after_dispatch(e));
        }
        std::thread::sleep(Duration::from_millis(12));
        let mut failure = None;
        for key in held.into_iter().rev() {
            if let Err(e) = self.send(unsafe { (self.key_message)(key as u32, 2) }) {
                failure = Some(after_dispatch(e));
            }
        }
        if let Some(e) = failure {
            return Err(e);
        }
        Ok(receipt("keyboard"))
    }
    fn release_keys(&self, held: &[u16]) {
        for &key in held.iter().rev() {
            let _ = self.send(unsafe { (self.key_message)(key as u32, 2) });
        }
    }
}
fn after_dispatch(mut e: NativeError) -> NativeError {
    e.effect = Effect::Unknown;
    e
}
fn receipt(kind: &str) -> Receipt {
    Receipt {
        effect: Effect::Dispatched,
        route: format!("ios.simulator.indigo.{kind}"),
    }
}
fn touch_packet(template: &[u8], point: (f64, f64), down: bool, time: u64) -> [u8; 0x140] {
    let mut bytes = [0u8; 0x140];
    bytes[..0x20].copy_from_slice(&template[..0x20]);
    bytes[0x18..0x1c].copy_from_slice(&0x80u32.to_ne_bytes());
    bytes[0x1c] = 2;
    bytes[0x20..0x24].copy_from_slice(&11u32.to_ne_bytes());
    bytes[0x24..0x2c].copy_from_slice(&time.to_ne_bytes());
    bytes[0x30..0xa0].copy_from_slice(&template[0x30..0xa0]);
    bytes[0x3c..0x44].copy_from_slice(&point.0.to_ne_bytes());
    bytes[0x44..0x4c].copy_from_slice(&point.1.to_ne_bytes());
    let flag = u32::from(down).to_ne_bytes();
    bytes[0x30..0x34].copy_from_slice(&flag);
    bytes[0x34..0x38].copy_from_slice(&flag);
    bytes.copy_within(0x20..0xa0, 0xa0);
    bytes[0xb0..0xb4].copy_from_slice(&1u32.to_ne_bytes());
    bytes[0xb4..0xb8].copy_from_slice(&2u32.to_ne_bytes());
    bytes
}
impl unimation::TouchInput for SimulatorHid {
    fn touch(&mut self, action: unimation::TouchAction) -> Result<Receipt> {
        match action {
            unimation::TouchAction::Tap { point } => self.tap_ratio(point.x(), point.y()),
            unimation::TouchAction::Swipe {
                from,
                to,
                duration_ms,
                edge,
            } => self.swipe_ratio(
                (from.x(), from.y()),
                (to.x(), to.y()),
                duration_ms,
                match edge {
                    unimation::TouchEdge::None => TouchEdge::None,
                    unimation::TouchEdge::Left => TouchEdge::Left,
                    unimation::TouchEdge::Top => TouchEdge::Top,
                    unimation::TouchEdge::Bottom => TouchEdge::Bottom,
                    unimation::TouchEdge::Right => TouchEdge::Right,
                },
            ),
        }
    }
}
impl unimation::HardwareButtons for SimulatorHid {
    fn press_button(&mut self, button: unimation::HardwareButton) -> Result<Receipt> {
        match button {
            unimation::HardwareButton::Home => self.press_hardware(HardwareButton::Home, 50),
            unimation::HardwareButton::Lock => self.press_hardware(HardwareButton::Lock, 50),
            unimation::HardwareButton::VolumeUp | unimation::HardwareButton::VolumeDown => {
                let f = self.arbitrary_message.ok_or_else(|| {
                    error(
                        "private_api_unavailable",
                        "IndigoHIDMessageForHIDArbitrary",
                        Effect::None,
                    )
                })?;
                let usage = if matches!(button, unimation::HardwareButton::VolumeUp) {
                    0xe9
                } else {
                    0xea
                };
                if let Err(e) = self.send(unsafe { f(0x32, 0x0c, usage, 1) }) {
                    let _ = self.send(unsafe { f(0x32, 0x0c, usage, 2) });
                    return Err(after_dispatch(e));
                }
                std::thread::sleep(Duration::from_millis(50));
                self.send(unsafe { f(0x32, 0x0c, usage, 2) })
                    .map_err(after_dispatch)?;
                Ok(receipt("hardware"))
            }
        }
    }
}
impl unimation::HidKeyboard for SimulatorHid {
    fn press_usage(&mut self, usage: u16, modifiers: &[u16]) -> Result<Receipt> {
        self.key(usage, modifiers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_coordinates_rejected() {
        for p in [
            (f64::NAN, 0.0),
            (0.0, f64::INFINITY),
            (-0.1, 0.0),
            (0.0, 1.01),
        ] {
            assert!(validate_ratio(p).is_err());
        }
        assert!(validate_ratio((0.0, 1.0)).is_ok());
    }
    #[test]
    fn packet_has_both_contacts_and_exact_coordinates() {
        let template = [0u8; 0xa0];
        let p = touch_packet(&template, (0.25, 0.75), true, 123);
        assert_eq!(f64::from_ne_bytes(p[0x3c..0x44].try_into().unwrap()), 0.25);
        assert_eq!(f64::from_ne_bytes(p[0xc4..0xcc].try_into().unwrap()), 0.75);
        assert_eq!(u32::from_ne_bytes(p[0xb4..0xb8].try_into().unwrap()), 2);
        let up = touch_packet(&template, (0.0, 0.0), false, 124);
        assert_eq!(&up[0x30..0x38], &[0; 8]);
    }
}

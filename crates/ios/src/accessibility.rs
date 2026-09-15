//! CoreSimulator/AXPTranslator bridge. Private selectors are isolated here.
//! Based on the API behavior documented by DioxusLabs/accessibility-cli's
//! accessibility-ios-sys reader and dispatcher. No host keyboard input is sent.
#![allow(unsafe_op_in_unsafe_fn)]
use block2::RcBlock;
use objc2::{
    ClassType, msg_send,
    rc::Retained,
    runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel},
    sel,
};
use objc2_core_foundation::{CGPoint, CGRect};
use objc2_foundation::{NSObject, NSString, NSUUID};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::{CStr, CString, c_void},
    path::Path,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use unimation::{
    Effect, ElementRef, NativeError, Node, Receipt, Result, SemanticAction, SemanticActions,
    Snapshot,
};
fn error(code: &str, message: impl Into<String>, effect: Effect) -> NativeError {
    NativeError {
        code: code.into(),
        message: message.into(),
        effect,
    }
}
#[derive(Clone)]
struct Route {
    owner: Arc<RouteOwner>,
    timed_out: Arc<AtomicBool>,
}
// CoreSimulator permits sends from worker threads; the callback queue is serial.
struct RouteOwner {
    device: usize,
    queue: usize,
}
impl Drop for RouteOwner {
    fn drop(&mut self) {
        unsafe {
            drop(Retained::from_raw(self.device as *mut AnyObject));
            dispatch_release(self.queue as *mut c_void);
        }
    }
}
unsafe fn require(receiver: *mut AnyObject, selector: Sel) -> Result<()> {
    let responds: Bool = msg_send![receiver,respondsToSelector:selector];
    if responds.as_bool() {
        Ok(())
    } else {
        Err(error(
            "private_api_unavailable",
            selector.name().to_string_lossy(),
            Effect::None,
        ))
    }
}
static ROUTES: OnceLock<Mutex<HashMap<String, Route>>> = OnceLock::new();
fn routes() -> &'static Mutex<HashMap<String, Route>> {
    ROUTES.get_or_init(Default::default)
}
unsafe extern "C" {
    fn dispatch_queue_create(label: *const libc::c_char, attr: *const c_void) -> *mut c_void;
    fn dispatch_release(object: *mut c_void);
}
unsafe fn string(object: *mut AnyObject) -> Option<String> {
    if object.is_null() {
        None
    } else {
        let s: *const libc::c_char = msg_send![object, UTF8String];
        if s.is_null() {
            None
        } else {
            Some(CStr::from_ptr(s).to_string_lossy().into_owned())
        }
    }
}
unsafe fn empty() -> *mut AnyObject {
    AnyClass::get(c"AXPTranslatorResponse")
        .map(|c| msg_send![c, emptyResponse])
        .unwrap_or(std::ptr::null_mut())
}
unsafe extern "C-unwind" fn callback(
    _: &AnyObject,
    _: Sel,
    token: *mut AnyObject,
) -> *mut AnyObject {
    let route = string(token).and_then(|t| routes().lock().unwrap().get(&t).cloned());
    let block = RcBlock::new(move |request: *mut AnyObject| -> *mut AnyObject {
        let Some(route) = &route else {
            return unsafe { empty() };
        };
        let (tx, rx) = std::sync::mpsc::sync_channel::<Response>(1);
        let owner = route.owner.clone();
        let completion = RcBlock::new(move |value: *mut AnyObject| {
            let _keep_alive = &owner;
            let retained = unsafe { Retained::retain(value) };
            let _ = tx.send(Response(
                retained
                    .map(Retained::into_raw)
                    .unwrap_or(std::ptr::null_mut()) as usize,
            ));
        });
        unsafe {
            let _: () = msg_send![route.owner.device as *mut AnyObject,sendAccessibilityRequestAsync:request,completionQueue:route.owner.queue as *mut AnyObject,completionHandler:&*completion];
        }
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(mut response) => {
                let p = response.0 as *mut AnyObject;
                response.0 = 0;
                if p.is_null() {
                    unsafe { empty() }
                } else {
                    unsafe { msg_send![p, autorelease] }
                }
            }
            Err(_) => {
                route.timed_out.store(true, Ordering::Release);
                unsafe { empty() }
            }
        }
    });
    let p = RcBlock::into_raw(block) as *mut AnyObject;
    unsafe { msg_send![p, autorelease] }
}
struct Response(usize);
impl Drop for Response {
    fn drop(&mut self) {
        if self.0 != 0 {
            unsafe {
                drop(Retained::from_raw(self.0 as *mut AnyObject));
            }
        }
    }
}
unsafe extern "C-unwind" fn frame(
    _: &AnyObject,
    _: Sel,
    rect: CGRect,
    _: *mut AnyObject,
) -> CGRect {
    rect
}
unsafe extern "C-unwind" fn parent(_: &AnyObject, _: Sel, _: *mut AnyObject) -> *mut AnyObject {
    std::ptr::null_mut()
}
fn dispatcher() -> usize {
    static INSTANCE: OnceLock<usize> = OnceLock::new();
    *INSTANCE.get_or_init(|| unsafe {
        let mut b = ClassBuilder::new(c"UnimationSimulatorAXDelegate", NSObject::class())
            .expect("unique native class");
        b.add_method(
            sel!(accessibilityTranslationDelegateBridgeCallbackWithToken:),
            callback as unsafe extern "C-unwind" fn(_, _, _) -> _,
        );
        b.add_method(
            sel!(accessibilityTranslationConvertPlatformFrameToSystem:withToken:),
            frame as unsafe extern "C-unwind" fn(_, _, _, _) -> _,
        );
        b.add_method(
            sel!(accessibilityTranslationRootParentWithToken:),
            parent as unsafe extern "C-unwind" fn(_, _, _) -> _,
        );
        let c = b.register();
        let p: *mut AnyObject = msg_send![c, new];
        p as usize
    })
}
fn load(path: &str) -> Result<()> {
    let p = CString::new(path).unwrap();
    if unsafe { libc::dlopen(p.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL) }.is_null() {
        return Err(error("framework_unavailable", path, Effect::None));
    }
    Ok(())
}

/// A retained, explicitly selected simulator. This provider must stay on its
/// owning thread. Each AX bridge callback has a five-second response deadline;
/// a multi-property observation can take longer and has no total deadline yet.
/// References reuse the retained platform element's native `isEqual:` identity.
/// They are session-local, never reconstructed from labels, paths or coordinates.
/// Retained native references do not prove that an element remains actionable.
/// Values and element objects are retained until this provider is dropped.
pub struct SimulatorAccessibility {
    device: Retained<AnyObject>,
    translator: Retained<AnyObject>,
    token: String,
    timed_out: Arc<AtomicBool>,
    elements: BTreeMap<u64, Retained<AnyObject>>,
    next_id: u64,
    revision: u64,
    values: BTreeMap<u64, Retained<AnyObject>>,
    next_value: u64,
}
impl SimulatorAccessibility {
    pub fn connect(udid: &str, device_set_path: &Path) -> Result<Self> {
        unsafe {
            if !device_set_path.is_absolute() || !device_set_path.is_dir() {
                return Err(error(
                    "invalid_device_set",
                    "Device set must be an existing absolute directory",
                    Effect::None,
                ));
            }
            load("/Library/Developer/PrivateFrameworks/CoreSimulator.framework/CoreSimulator")?;
            load(
                "/System/Library/PrivateFrameworks/AccessibilityPlatformTranslation.framework/AccessibilityPlatformTranslation",
            )?;
            let cls = AnyClass::get(c"SimServiceContext")
                .ok_or_else(|| error("framework_unavailable", "SimServiceContext", Effect::None))?;
            require(
                cls as *const AnyClass as *mut AnyObject,
                sel!(sharedServiceContextForDeveloperDir:error:),
            )?;
            let mut e: *mut AnyObject = std::ptr::null_mut();
            let ctx: *mut AnyObject = msg_send![cls,sharedServiceContextForDeveloperDir:std::ptr::null::<AnyObject>(),error:&mut e];
            if ctx.is_null() {
                return Err(error(
                    "simulator_context",
                    "Cannot connect to CoreSimulator service",
                    Effect::None,
                ));
            }
            let path =
                NSString::from_str(device_set_path.to_str().ok_or_else(|| {
                    error("invalid_device_set", "Path must be UTF-8", Effect::None)
                })?);
            require(ctx, sel!(deviceSetWithPath:error:))?;
            let set: *mut AnyObject = msg_send![ctx,deviceSetWithPath:&*path,error:&mut e];
            if set.is_null() {
                return Err(error(
                    "device_set_unavailable",
                    device_set_path.display().to_string(),
                    Effect::None,
                ));
            }
            let devices: *mut AnyObject = msg_send![set, availableDevices];
            let count: usize = msg_send![devices, count];
            let mut selected = None;
            for i in 0..count {
                let d: *mut AnyObject = msg_send![devices,objectAtIndex:i];
                let uuid: *mut AnyObject = msg_send![d, UDID];
                let uuid: *mut AnyObject = msg_send![uuid, UUIDString];
                if string(uuid).is_some_and(|s| s.eq_ignore_ascii_case(udid)) {
                    let state: usize = msg_send![d, state];
                    if state != 3 {
                        return Err(error("simulator_not_booted", udid, Effect::None));
                    }
                    selected = Retained::retain(d);
                    break;
                }
            }
            let device =
                selected.ok_or_else(|| error("simulator_not_found", udid, Effect::None))?;
            let cls = AnyClass::get(c"AXPTranslator")
                .ok_or_else(|| error("framework_unavailable", "AXPTranslator", Effect::None))?;
            let p: *mut AnyObject = msg_send![cls, sharedInstance];
            let translator = Retained::retain(p)
                .ok_or_else(|| error("bridge_unavailable", "AXPTranslator", Effect::None))?;
            require(
                Retained::as_ptr(&device) as *mut AnyObject,
                sel!(sendAccessibilityRequestAsync:completionQueue:completionHandler:),
            )?;
            require(
                p,
                sel!(frontmostApplicationWithDisplayId:bridgeDelegateToken:),
            )?;
            require(p, sel!(macPlatformElementFromTranslation:))?;
            require(p, sel!(setSupportsDelegateTokens:))?;
            require(p, sel!(setBridgeTokenDelegate:))?;
            let token = NSUUID::new().UUIDString().to_string();
            let queue = dispatch_queue_create(c"unimation.simulator.ax".as_ptr(), std::ptr::null());
            let timed_out = Arc::new(AtomicBool::new(false));
            routes().lock().unwrap().insert(
                token.clone(),
                Route {
                    owner: Arc::new(RouteOwner {
                        device: Retained::into_raw(device.clone()) as usize,
                        queue: queue as usize,
                    }),
                    timed_out: timed_out.clone(),
                },
            );
            let _: () = msg_send![&*translator,setSupportsDelegateTokens:Bool::YES];
            let _: () =
                msg_send![&*translator,setBridgeTokenDelegate:dispatcher() as *mut AnyObject];
            Ok(Self {
                device,
                translator,
                token,
                timed_out,
                elements: BTreeMap::new(),
                next_id: 1,
                revision: 0,
                values: BTreeMap::new(),
                next_value: 1,
            })
        }
    }
    unsafe fn bind(&self, e: *mut AnyObject) {
        let translation: *mut AnyObject = msg_send![e, translation];
        if !translation.is_null() {
            let token = NSString::from_str(&self.token);
            let _: () = msg_send![translation,setBridgeDelegateToken:&*token];
        }
    }
    fn reference(&mut self, e: *mut AnyObject) -> ElementRef {
        for (id, old) in &self.elements {
            let equal: Bool = unsafe { msg_send![&**old,isEqual:e] };
            if equal.as_bool() {
                return ElementRef {
                    session: self.token.clone(),
                    id: *id,
                };
            }
        }
        let id = self.next_id;
        self.next_id += 1;
        self.elements
            .insert(id, unsafe { Retained::retain(e).unwrap() });
        ElementRef {
            session: self.token.clone(),
            id,
        }
    }
    pub fn observe_frontmost(&mut self, max_nodes: usize, max_depth: usize) -> Result<Snapshot> {
        self.observe_selected(SimulatorScope::Frontmost, max_nodes, max_depth)
    }
    /// Query one guest application explicitly. The PID is the guest PID, not a
    /// host app PID. This does not activate the application.
    pub fn observe_application(
        &mut self,
        pid: i32,
        max_nodes: usize,
        max_depth: usize,
    ) -> Result<Snapshot> {
        self.observe_selected(SimulatorScope::Application { pid }, max_nodes, max_depth)
    }
    /// Hit-test and observe the returned subtree in AX translator coordinates.
    /// Coordinates are not screenshot pixels or normalized HID ratios.
    pub fn observe_at_point(
        &mut self,
        x: f64,
        y: f64,
        max_nodes: usize,
        max_depth: usize,
    ) -> Result<Snapshot> {
        self.observe_selected(SimulatorScope::Point { x, y }, max_nodes, max_depth)
    }
    fn observe_selected(
        &mut self,
        scope: SimulatorScope,
        max_nodes: usize,
        max_depth: usize,
    ) -> Result<Snapshot> {
        objc2::rc::autoreleasepool(|_| unsafe {
            if max_nodes == 0 {
                return Err(error(
                    "invalid_limit",
                    "max_nodes must be positive",
                    Effect::None,
                ));
            }
            self.timed_out.store(false, Ordering::Release);
            let token = NSString::from_str(&self.token);
            let translation: *mut AnyObject = match scope {
                SimulatorScope::Frontmost => {
                    msg_send![&*self.translator,frontmostApplicationWithDisplayId:0u32,bridgeDelegateToken:&*token]
                }
                SimulatorScope::Application { pid } => {
                    if pid <= 0 {
                        return Err(error(
                            "invalid_pid",
                            "Guest PID must be positive",
                            Effect::None,
                        ));
                    }
                    require(
                        Retained::as_ptr(&self.translator) as *mut _,
                        sel!(translationApplicationObjectForPid:),
                    )?;
                    msg_send![&*self.translator,translationApplicationObjectForPid:pid]
                }
                SimulatorScope::Point { x, y } => {
                    if !x.is_finite() || !y.is_finite() {
                        return Err(error(
                            "invalid_coordinate",
                            "AX coordinates must be finite",
                            Effect::None,
                        ));
                    }
                    require(
                        Retained::as_ptr(&self.translator) as *mut _,
                        sel!(objectAtPoint:displayId:bridgeDelegateToken:),
                    )?;
                    msg_send![&*self.translator,objectAtPoint:CGPoint{x,y},displayId:0u32,bridgeDelegateToken:&*token]
                }
            };
            if translation.is_null() {
                return Err(error(
                    "bridge_unavailable",
                    "No native element for the requested simulator scope",
                    Effect::None,
                ));
            }
            let _: () = msg_send![translation,setBridgeDelegateToken:&*token];
            let root: *mut AnyObject =
                msg_send![&*self.translator,macPlatformElementFromTranslation:translation];
            if root.is_null() {
                return Err(error(
                    "bridge_unavailable",
                    "No translated application element",
                    Effect::None,
                ));
            }
            let root_ref = self.reference(root);
            let mut nodes = Vec::new();
            let mut seen = HashSet::new();
            let mut todo = vec![(root_ref.clone(), 0usize)];
            let mut complete = true;
            while let Some((r, depth)) = todo.pop() {
                if !seen.insert(r.id) {
                    continue;
                }
                if nodes.len() >= max_nodes {
                    complete = false;
                    break;
                }
                let p = Retained::as_ptr(&self.elements[&r.id]) as *mut AnyObject;
                for selector in [
                    sel!(translation),
                    sel!(accessibilityFrame),
                    sel!(accessibilityChildren),
                    sel!(accessibilityActionNames),
                ] {
                    require(p, selector)?;
                }
                self.bind(p);
                let mut attrs = BTreeMap::new();
                for (name, selector) in [
                    ("AXRole", sel!(accessibilityRole)),
                    ("AXTitle", sel!(accessibilityTitle)),
                    ("AXLabel", sel!(accessibilityLabel)),
                    ("AXIdentifier", sel!(accessibilityIdentifier)),
                    ("AXValue", sel!(accessibilityValue)),
                ] {
                    let responds: Bool = msg_send![p,respondsToSelector:selector];
                    if responds.as_bool() {
                        let f: unsafe extern "C" fn(*mut AnyObject, Sel) -> *mut AnyObject =
                            std::mem::transmute(objc2::ffi::objc_msgSend as *const c_void);
                        let value = f(p, selector);
                        attrs.insert(name.into(), self.encode(value));
                    }
                }
                for (name, selector) in [
                    ("AXEnabled", sel!(isAccessibilityEnabled)),
                    ("AXFocused", sel!(isAccessibilityFocused)),
                ] {
                    let supported: Bool = msg_send![p,respondsToSelector:selector];
                    if supported.as_bool() {
                        let f: unsafe extern "C" fn(*mut AnyObject, Sel) -> Bool =
                            std::mem::transmute(objc2::ffi::objc_msgSend as *const c_void);
                        attrs.insert(
                            name.into(),
                            json!({"type":"bool","value":f(p,selector).as_bool()}),
                        );
                    }
                }
                let bounds: CGRect = msg_send![p, accessibilityFrame];
                attrs.insert(
                    "AXPosition".into(),
                    json!({"type":"point","x":bounds.origin.x,"y":bounds.origin.y}),
                );
                attrs.insert(
                    "AXSize".into(),
                    json!({"type":"size","width":bounds.size.width,"height":bounds.size.height}),
                );
                attrs.insert(
                    "ios_coordinate_space".into(),
                    json!({"type":"string","value":"ax_translator_system"}),
                );
                let actions = actions(p);
                let children: *mut AnyObject = msg_send![p, accessibilityChildren];
                let count: usize = if children.is_null() {
                    0
                } else {
                    msg_send![children, count]
                };
                let mut refs = Vec::new();
                if depth >= max_depth && count > 0 {
                    complete = false;
                } else {
                    for i in 0..count {
                        if nodes.len() + todo.len() + refs.len() + 1 >= max_nodes {
                            complete = false;
                            break;
                        }
                        let child: *mut AnyObject = msg_send![children,objectAtIndex:i];
                        if !child.is_null() {
                            self.bind(child);
                            refs.push(self.reference(child));
                        }
                    }
                }
                for child in refs.iter().rev() {
                    todo.push((child.clone(), depth + 1));
                }
                nodes.push(Node {
                    reference: r,
                    attributes: attrs,
                    actions,
                    parameterized_attributes: vec![],
                    children: refs,
                    issues: vec![],
                });
                if self.timed_out.load(Ordering::Acquire) {
                    complete = false;
                    break;
                }
            }
            self.revision += 1;
            let timed_out = self.timed_out.load(Ordering::Acquire);
            Ok(Snapshot {
                root: root_ref,
                nodes,
                complete: false,
                traversal_complete: complete,
                revision: self.revision,
                issues: if timed_out {
                    vec![
                        json!({"code":"bridge_timeout","message":"Native AX response exceeded five seconds"}),
                    ]
                } else {
                    vec![
                        json!({"code":"selected_native_attributes","message":"Provider currently reads known translated attributes; other native properties are not enumerated"}),
                    ]
                },
            })
        })
    }
}
impl SimulatorAccessibility {
    /// Read a retained native value by its opaque handle on the owning thread.
    pub fn native_value(&self, handle: u64) -> Option<&AnyObject> {
        self.values.get(&handle).map(|v| &**v)
    }
    fn retain_value(&mut self, p: *mut AnyObject) -> u64 {
        for (id, existing) in &self.values {
            let equal: Bool = unsafe { msg_send![&**existing,isEqual:p] };
            if equal.as_bool() {
                return *id;
            }
        }
        let handle = self.next_value;
        self.next_value += 1;
        self.values
            .insert(handle, unsafe { Retained::retain(p).unwrap() });
        handle
    }
    unsafe fn encode(&mut self, p: *mut AnyObject) -> Value {
        if p.is_null() {
            return json!({"type":"null"});
        }
        let is_string: Bool = msg_send![p,isKindOfClass:NSString::class()];
        if is_string.as_bool() {
            let length: usize = msg_send![p, length];
            let units: Vec<u16> = (0..length)
                .map(|i| msg_send![p,characterAtIndex:i])
                .collect();
            return match String::from_utf16(&units) {
                Ok(value) => json!({"type":"string","value":value}),
                Err(_) => {
                    let handle = self.retain_value(p);
                    json!({"type":"string","utf16":units,"native":{"type":"opaque","handle":handle,"session":self.token}})
                }
            };
        }
        let class = (&*p).class().name().to_string_lossy();
        if let Some(number) = AnyClass::get(c"NSNumber") {
            let is_number: Bool = msg_send![p,isKindOfClass:number];
            if is_number.as_bool() {
                let code: *const libc::c_char = msg_send![p, objCType];
                let code = CStr::from_ptr(code).to_string_lossy();
                let description: *mut AnyObject = msg_send![p, stringValue];
                let decimal = string(description);
                if class == "__NSCFBoolean" {
                    let value: Bool = msg_send![p, boolValue];
                    return json!({"type":"bool","value":value.as_bool(),"encoding":code});
                }
                match code.as_ref() {
                    "c" | "s" | "i" | "l" | "q" => {
                        let value: i64 = msg_send![p, longLongValue];
                        return json!({"type":"integer","value":value,"encoding":code,"decimal":decimal});
                    }
                    "C" | "S" | "I" | "L" | "Q" => {
                        let value: u64 = msg_send![p, unsignedLongLongValue];
                        return json!({"type":"integer","value":value,"encoding":code,"decimal":decimal});
                    }
                    "f" | "d" => {
                        let value: f64 = msg_send![p, doubleValue];
                        if value.is_finite() {
                            return json!({"type":"float","value":value,"encoding":code,"decimal":decimal});
                        }
                    }
                    _ => {}
                }
            }
        }
        let handle = self.retain_value(p);
        json!({"type":"opaque","native_class":class,"handle":handle,"session":self.token})
    }
}
unsafe fn actions(p: *mut AnyObject) -> Vec<String> {
    let a: *mut AnyObject = msg_send![p, accessibilityActionNames];
    if a.is_null() {
        return vec![];
    }
    let n: usize = msg_send![a, count];
    (0..n)
        .filter_map(|i| {
            let p: *mut AnyObject = msg_send![a,objectAtIndex:i];
            string(p)
        })
        .collect()
}
impl SemanticActions for SimulatorAccessibility {
    fn semantic(&mut self, target: &ElementRef, action: SemanticAction) -> Result<Receipt> {
        objc2::rc::autoreleasepool(|_| unsafe {
            if target.session != self.token {
                return Err(error(
                    "foreign_reference",
                    "Element belongs to another session",
                    Effect::None,
                ));
            }
            let p = self.elements.get(&target.id).ok_or_else(|| {
                error(
                    "unknown_reference",
                    "No retained native element",
                    Effect::None,
                )
            })?;
            let p = Retained::as_ptr(p) as *mut AnyObject;
            self.bind(p);
            let SemanticAction::Perform { name } = action else {
                return Err(error(
                    "unsupported",
                    "Only advertised native actions are implemented",
                    Effect::None,
                ));
            };
            if !actions(p).contains(&name) {
                return Err(error(
                    "unsupported",
                    format!("Action {name} is not advertised"),
                    Effect::None,
                ));
            }
            self.timed_out.store(false, Ordering::Release);
            if name == "AXPress" {
                require(p, sel!(accessibilityPerformPress))?;
                let ok: Bool = msg_send![p, accessibilityPerformPress];
                if !ok.as_bool() {
                    return Err(error(
                        "native_action_failed",
                        "accessibilityPerformPress returned false",
                        Effect::Unknown,
                    ));
                }
            } else {
                require(p, sel!(accessibilityPerformAction:))?;
                let n = NSString::from_str(&name);
                let _: () = msg_send![p,accessibilityPerformAction:&*n];
            }
            if self.timed_out.load(Ordering::Acquire) {
                return Err(error(
                    "bridge_timeout",
                    "Native action response timed out",
                    Effect::Unknown,
                ));
            }
            Ok(Receipt {
                effect: Effect::Dispatched,
                route: "ios.simulator.axp".into(),
            })
        })
    }
}
impl Drop for SimulatorAccessibility {
    fn drop(&mut self) {
        routes().lock().unwrap().remove(&self.token);
        self.elements.clear();
        let _ = &self.device;
    }
}

/// Explicit native scope. Frontmost does not imply the whole display. Guest
/// application selection and point hit-testing do not activate or focus UI.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SimulatorScope {
    Frontmost,
    Application { pid: i32 },
    Point { x: f64, y: f64 },
}
impl unimation::ObserveScope for SimulatorAccessibility {
    type Scope = SimulatorScope;
    fn observe_scope(
        &mut self,
        scope: Self::Scope,
        budget: unimation::ObservationBudget,
    ) -> Result<Snapshot> {
        self.observe_selected(scope, budget.max_nodes, budget.max_depth)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn value_store() -> SimulatorAccessibility {
        SimulatorAccessibility {
            device: unsafe { msg_send![NSObject::class(), new] },
            translator: unsafe { msg_send![NSObject::class(), new] },
            token: "encoding-test".into(),
            timed_out: Arc::new(AtomicBool::new(false)),
            elements: BTreeMap::new(),
            next_id: 1,
            revision: 0,
            values: BTreeMap::new(),
            next_value: 1,
        }
    }
    #[test]
    fn equal_text_has_identical_encoding_without_retained_value_growth() {
        let mut store = value_store();
        let text = NSString::from_str("Settings 📱");
        let first = unsafe { store.encode(Retained::as_ptr(&text) as *mut _) };
        let second = unsafe { store.encode(Retained::as_ptr(&text) as *mut _) };
        assert_eq!(first["value"], "Settings 📱");
        assert_eq!(first, second);
        assert!(store.values.is_empty());
    }
    #[test]
    fn unavailable_selector_returns_capability_error() {
        let object: Retained<AnyObject> = unsafe { msg_send![NSObject::class(), new] };
        let failure = unsafe {
            require(
                Retained::as_ptr(&object) as *mut _,
                sel!(unimationAbsentCapability),
            )
        }
        .unwrap_err();
        assert_eq!(failure.code, "private_api_unavailable");
    }
    #[test]
    fn native_reference_is_reused_without_position_matching() {
        let mut store = value_store();
        let object: Retained<AnyObject> = unsafe { msg_send![NSObject::class(), new] };
        let pointer = Retained::as_ptr(&object) as *mut _;
        assert_eq!(store.reference(pointer), store.reference(pointer));
        let other: Retained<AnyObject> = unsafe { msg_send![NSObject::class(), new] };
        assert_ne!(
            store.reference(pointer),
            store.reference(Retained::as_ptr(&other) as *mut _)
        );
    }
}

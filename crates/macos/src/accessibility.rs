use crate::error;
use objc2_application_services::{AXError, AXIsProcessTrusted, AXUIElement, AXValue, AXValueType};
use objc2_core_foundation::{
    CFArray, CFAttributedString, CFBoolean, CFNumber, CFRange, CFRetained, CFString, CFType,
    CGPoint, CGRect, CGSize, Type,
};
use objc2_foundation::NSUUID;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    marker::PhantomData,
    ptr::{self, NonNull},
    rc::Rc,
};
use unimation::*;

/// One reference namespace. References are never reused or structurally rebound.
/// Drop the provider to release its retained objects. Event-based retirement is pending.
pub struct Accessibility {
    session: String,
    elements: Vec<CFRetained<AXUIElement>>,
    element_index: HashMap<CFRetained<AXUIElement>, usize>,
    opaque: Vec<CFRetained<CFType>>,
    revision: u64,
    _thread_bound: PhantomData<Rc<()>>,
}
fn ax_name(status: AXError) -> &'static str {
    match status {
        AXError::NoValue => "no_value",
        AXError::AttributeUnsupported => "attribute_unsupported",
        AXError::InvalidUIElement => "invalid_ui_element",
        AXError::CannotComplete => "cannot_complete",
        AXError::ActionUnsupported => "action_unsupported",
        AXError::APIDisabled => "api_disabled",
        AXError::NotImplemented => "not_implemented",
        AXError::IllegalArgument => "illegal_argument",
        _ => "native_error",
    }
}
fn check(status: AXError, context: &str, mutation: bool) -> Result<()> {
    if status == AXError::Success {
        Ok(())
    } else {
        Err(error(
            format!("ax_{}", status.0),
            format!("{context}: {}", ax_name(status)),
            if mutation && matches!(status, AXError::CannotComplete | AXError::Failure) {
                Effect::Unknown
            } else {
                Effect::None
            },
        ))
    }
}
pub(crate) fn attribute(element: &AXUIElement, name: &str) -> Result<CFRetained<CFType>> {
    let mut raw = ptr::null();
    // SAFETY: valid retained element/string and initialized out pointer. Copy returns +1 ownership.
    unsafe {
        check(element.set_messaging_timeout(2.0), "Set AX timeout", false)?;
        check(
            element.copy_attribute_value(&CFString::from_str(name), NonNull::from(&mut raw)),
            name,
            false,
        )?;
        NonNull::new(raw.cast_mut())
            .map(|p| CFRetained::from_raw(p))
            .ok_or_else(|| error("native_null", name, Effect::None))
    }
}
// AX arrays contain retained CF objects according to the native AX contract.
pub(crate) fn cf_items(array: &CFArray) -> Vec<CFRetained<CFType>> {
    // SAFETY: only called for arrays returned as AX attribute values or name lists.
    unsafe { array.cast_unchecked::<CFType>().to_vec() }
}
fn names(element: &AXUIElement, actions: bool, parameterized: bool) -> Result<Vec<String>> {
    let mut raw = ptr::null();
    // SAFETY: each Copy API writes a retained CFArray to a valid out pointer.
    unsafe {
        let status = if actions {
            element.copy_action_names(NonNull::from(&mut raw))
        } else if parameterized {
            element.copy_parameterized_attribute_names(NonNull::from(&mut raw))
        } else {
            element.copy_attribute_names(NonNull::from(&mut raw))
        };
        check(status, "enumerate names", false)?;
        let array = CFRetained::from_raw(
            NonNull::new(raw.cast_mut())
                .ok_or_else(|| error("native_null", "names", Effect::None))?,
        );
        cf_items(&array)
            .into_iter()
            .map(|v| {
                v.downcast_ref::<CFString>()
                    .map(ToString::to_string)
                    .ok_or_else(|| error("native_type", "Expected a string name", Effect::None))
            })
            .collect()
    }
}
impl Default for Accessibility {
    fn default() -> Self {
        Self::new()
    }
}
impl Accessibility {
    pub fn new() -> Self {
        Self {
            session: NSUUID::UUID().UUIDString().to_string(),
            elements: vec![],
            element_index: HashMap::new(),
            opaque: vec![],
            revision: 0,
            _thread_bound: PhantomData,
        }
    }
    /// Short display references are valid only inside this retained provider session.
    pub fn expand_reference(&self, short: &str) -> Result<ElementRef> {
        let id = short
            .strip_prefix("@e")
            .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(|| {
                error(
                    "invalid_reference",
                    "Expected a session-local @e<number> reference",
                    Effect::None,
                )
            })?;
        let reference = ElementRef {
            session: self.session.clone(),
            id,
        };
        self.resolve(&reference)?;
        Ok(reference)
    }
    pub fn is_trusted() -> bool {
        // SAFETY: permission query has no arguments and does not prompt.
        unsafe { AXIsProcessTrusted() }
    }
    pub(crate) fn intern(&mut self, element: &AXUIElement) -> ElementRef {
        let index = match self.element_index.get(element) {
            Some(index) => *index,
            None => {
                let index = self.elements.len();
                self.elements.push(element.retain());
                self.element_index.insert(element.retain(), index);
                index
            }
        };
        ElementRef {
            session: self.session.clone(),
            id: index as u64 + 1,
        }
    }
    pub(crate) fn resolve(&self, target: &ElementRef) -> Result<CFRetained<AXUIElement>> {
        if target.session != self.session {
            return Err(error(
                "stale_reference",
                "Foreign or retired provider session",
                Effect::None,
            ));
        }
        let element = target
            .id
            .checked_sub(1)
            .and_then(|i| usize::try_from(i).ok())
            .and_then(|i| self.elements.get(i))
            .cloned()
            .ok_or_else(|| error("stale_reference", "Unknown element reference", Effect::None))?;
        // SAFETY: configure this retained object before any potentially blocking AX operation.
        unsafe {
            check(element.set_messaging_timeout(2.0), "Set AX timeout", false)?;
        }
        Ok(element)
    }
    /// Borrow the exact retained CF object behind an opaque value. IDs are session-local.
    pub fn native_value(&self, session: &str, id: u64) -> Option<&CFType> {
        if session != self.session {
            return None;
        }
        self.opaque
            .get(usize::try_from(id.checked_sub(1)?).ok()?)
            .map(|v| &**v)
    }
    fn opaque(&mut self, value: &CFType, reason: &str) -> Value {
        self.opaque.push(value.retain());
        json!({"type":"opaque", "session":self.session, "id":self.opaque.len(), "reason":reason, "description":format!("{value:?}")})
    }
    fn encode(&mut self, value: &CFType, depth: usize) -> Value {
        if depth > 64 {
            return self.opaque(value, "encoding_depth_limit");
        }
        if let Some(v) = value.downcast_ref::<AXUIElement>() {
            return json!({"type":"element", "value": self.intern(v)});
        }
        if let Some(v) = value.downcast_ref::<CFAttributedString>()
            && let Some(text) = v.string()
        {
            let text = self.encode(&text, depth + 1);
            // Keep the exact styled object available to embeddings; plain text
            // is extracted by the native API, never parsed from diagnostics.
            let native = self.opaque(value, "attributed_string_native");
            return json!({"type":"attributed_string", "text":text, "native":native});
        }
        if let Some(v) = value.downcast_ref::<CFString>() {
            let mut units = vec![0; v.length() as usize];
            // SAFETY: buffer covers exactly the requested UTF-16 range.
            unsafe {
                v.characters(CFRange::new(0, v.length()), units.as_mut_ptr());
            }
            return match String::from_utf16(&units) {
                Ok(s) => json!({"type":"string", "value":s}),
                Err(_) => json!({"type":"utf16", "units":units}),
            };
        }
        if let Some(v) = value.downcast_ref::<CFBoolean>() {
            return json!({"type":"bool", "value":v.as_bool()});
        }
        if let Some(v) = value.downcast_ref::<CFNumber>() {
            if !v.is_float_type() {
                if let Some(n) = v.as_i64() {
                    return json!({"type":"integer", "value":n});
                }
            } else if let Some(n) = v.as_f64() {
                return json!({"type":"float", "bits":n.to_bits(), "value":n.to_string()});
            }
        }
        if let Some(v) = value.downcast_ref::<CFArray>() {
            return json!({"type":"array", "value":cf_items(v).into_iter().map(|v| self.encode(&v, depth + 1)).collect::<Vec<_>>()});
        }
        if let Some(v) = value.downcast_ref::<AXValue>() {
            // SAFETY: select the native layout from AXValueGetType and provide matching writable storage.
            unsafe {
                macro_rules! unpack {
                    ($ty:ty, $kind:expr) => {{
                        let mut out: $ty = std::mem::zeroed();
                        if !v.value($kind, NonNull::from(&mut out).cast()) {
                            return self.opaque(value, "ax_value_decode_failed");
                        }
                        out
                    }};
                }
                match v.r#type() {
                    AXValueType::CGPoint => {
                        let p = unpack!(CGPoint, AXValueType::CGPoint);
                        return json!({"type":"point", "x":p.x, "y":p.y});
                    }
                    AXValueType::CGSize => {
                        let s = unpack!(CGSize, AXValueType::CGSize);
                        return json!({"type":"size", "width":s.width, "height":s.height});
                    }
                    AXValueType::CGRect => {
                        let r = unpack!(CGRect, AXValueType::CGRect);
                        return json!({"type":"rect", "x":r.origin.x, "y":r.origin.y, "width":r.size.width, "height":r.size.height});
                    }
                    AXValueType::CFRange => {
                        let r = unpack!(CFRange, AXValueType::CFRange);
                        return json!({"type":"range", "location":r.location, "length":r.length});
                    }
                    AXValueType::AXError => {
                        let e = unpack!(AXError, AXValueType::AXError);
                        return json!({"type":"ax_error", "code":e.0});
                    }
                    _ => {}
                }
            }
        }
        self.opaque(value, "unmapped_native_type")
    }
    /// Read a referenced element without traversing or requiring it in AXChildren.
    pub fn inspect(&mut self, target: &ElementRef) -> Result<Value> {
        let element = self.resolve(target)?;
        let mut attributes = BTreeMap::new();
        for name in names(&element, false, false)? {
            let value = match attribute(&element, &name) {
                Ok(v) => self.encode(&v, 0),
                Err(e) => json!({"type":"read_error", "error":e}),
            };
            attributes.insert(name, value);
        }
        Ok(json!({"reference":target, "attributes":attributes,
            "actions":names(&element, true, false)?,
            "parameterized_attributes":names(&element, false, true)?}))
    }
    pub fn read_parameterized(
        &mut self,
        target: &ElementRef,
        name: &str,
        parameter: AttributeParameter,
    ) -> Result<Value> {
        let element = self.resolve(target)?;
        macro_rules! erase {
            ($value:expr) => {{
                let owned = $value;
                let value: &CFType = &owned;
                value.retain()
            }};
        }
        let parameter = match parameter {
            AttributeParameter::String { value } => erase!(CFString::from_str(&value)),
            AttributeParameter::Integer { value } => erase!(CFNumber::new_i64(value)),
            AttributeParameter::Range { location, length } => {
                if location < 0 || length < 0 || location.checked_add(length).is_none() {
                    return Err(error(
                        "invalid_request",
                        "Invalid UTF-16 range",
                        Effect::None,
                    ));
                }
                let range = CFRange::new(location as isize, length as isize);
                // SAFETY: type and native storage match and remain alive during copy.
                erase!(
                    unsafe { AXValue::new(AXValueType::CFRange, NonNull::from(&range).cast()) }
                        .ok_or_else(|| error(
                            "native_value",
                            "Cannot create range",
                            Effect::None
                        ))?
                )
            }
            AttributeParameter::Point { value } => {
                if !value.x.is_finite() || !value.y.is_finite() {
                    return Err(error(
                        "invalid_request",
                        "Finite point required",
                        Effect::None,
                    ));
                }
                let point = CGPoint::new(value.x, value.y);
                // SAFETY: type and native storage match and remain alive during copy.
                erase!(
                    unsafe { AXValue::new(AXValueType::CGPoint, NonNull::from(&point).cast()) }
                        .ok_or_else(|| error(
                            "native_value",
                            "Cannot create point",
                            Effect::None
                        ))?
                )
            }
        };
        let mut raw = ptr::null();
        // SAFETY: owned arguments and valid out pointer; Copy returns +1 ownership.
        let value = unsafe {
            check(
                element.copy_parameterized_attribute_value(
                    &CFString::from_str(name),
                    &parameter,
                    NonNull::from(&mut raw),
                ),
                name,
                false,
            )?;
            CFRetained::from_raw(
                NonNull::new(raw.cast_mut())
                    .ok_or_else(|| error("native_null", name, Effect::None))?,
            )
        };
        Ok(self.encode(&value, 0))
    }
    /// Query attributes omitted by traversal, retaining backend-specific names and values.
    pub fn read_attribute(&mut self, target: &ElementRef, name: &str) -> Result<Value> {
        let element = self.resolve(target)?;
        let value = attribute(&element, name)?;
        Ok(self.encode(&value, 0))
    }
}
impl Discover for Accessibility {
    fn discover(&mut self) -> Result<Value> {
        let apps = crate::discovery::applications()?;
        let active_pid = crate::discovery::frontmost_pid();
        let visible_windows = crate::discovery::visible_windows();
        // Query native AXFrontmost per app. AppKit caches isActive until its run
        // loop runs, which is not guaranteed in a synchronous embedding host.
        let mut applications = vec![];
        let mut active_pids = vec![];
        for app in apps.iter() {
            let pid = app.processIdentifier();
            let frontmost = (|| -> Result<bool> {
                // SAFETY: valid positive native PID; remote status remains fallible.
                let element = unsafe { AXUIElement::new_application(pid) };
                unsafe {
                    check(
                        element.set_messaging_timeout(0.2),
                        "Set discovery timeout",
                        false,
                    )?;
                }
                let mut raw = ptr::null();
                let value = unsafe {
                    check(
                        element.copy_attribute_value(
                            &CFString::from_str("AXFrontmost"),
                            NonNull::from(&mut raw),
                        ),
                        "AXFrontmost",
                        false,
                    )?;
                    CFRetained::from_raw(
                        NonNull::new(raw.cast_mut())
                            .ok_or_else(|| error("native_null", "AXFrontmost", Effect::None))?,
                    )
                };
                value
                    .downcast_ref::<CFBoolean>()
                    .map(CFBoolean::as_bool)
                    .ok_or_else(|| error("native_type", "AXFrontmost is not boolean", Effect::None))
            })();
            if matches!(frontmost, Ok(true)) {
                active_pids.push(pid);
            }
            let policy = match app.activationPolicy().0 {
                0 => "regular",
                1 => "accessory",
                2 => "prohibited",
                _ => "unknown",
            };
            let bundle = app.bundleIdentifier().map(|s| s.to_string());
            // These OS controls can exist without an open window. This explicit native
            // adapter list is presentation metadata, never an access restriction.
            let system_ui = matches!(
                bundle.as_deref(),
                Some(
                    "com.apple.Spotlight"
                        | "com.apple.controlcenter"
                        | "com.apple.dock"
                        | "com.apple.systemuiserver"
                )
            );
            applications.push(json!({"pid":pid,"name":app.localizedName().map(|s|s.to_string()),
                "bundle_id":bundle,"activation_policy":policy,"hidden":app.isHidden(),"terminated":app.isTerminated(),"system_ui":system_ui,"visible_window_ids":visible_windows.as_ref().ok().map(|windows|windows.get(&pid).cloned().unwrap_or_default()),"active":active_pid.map(|front|front==pid), "ax_frontmost":frontmost.as_ref().ok(),
                "active_error":frontmost.err()}));
        }
        Ok(
            json!({"session":self.session,"accessibility_trusted":Self::is_trusted(),
            "active_pid":active_pid,"ax_frontmost_pids":active_pids,
            "window_error":visible_windows.err(),"active_state_source":"optional_process_manager_probe","application_list_source":"libproc_per_pid_appkit","applications":applications}),
        )
    }
}
impl Observe for Accessibility {
    fn observe(&mut self, request: ObserveRequest) -> Result<Snapshot> {
        if !Self::is_trusted() {
            return Err(error(
                "permission_denied",
                "Accessibility access required",
                Effect::None,
            ));
        }
        if request.pid <= 0 || request.max_nodes == 0 {
            return Err(error(
                "invalid_request",
                "Positive PID and node budget required",
                Effect::None,
            ));
        }
        // SAFETY: positive PID; AX creates a proxy whose validity is checked by subsequent reads.
        let root = unsafe { AXUIElement::new_application(request.pid) };
        self.walk(root, request)
    }
}
impl Accessibility {
    pub fn observe_subtree(
        &mut self,
        target: &ElementRef,
        max_nodes: usize,
        max_depth: usize,
    ) -> Result<Snapshot> {
        let root = self.resolve(target)?;
        self.walk(
            root,
            ObserveRequest {
                pid: 0,
                max_nodes,
                max_depth,
            },
        )
    }
    fn walk(&mut self, root: CFRetained<AXUIElement>, request: ObserveRequest) -> Result<Snapshot> {
        if request.max_nodes == 0 {
            return Err(error(
                "invalid_request",
                "Positive node budget required",
                Effect::None,
            ));
        }
        let root_ref = self.intern(&root);
        let mut queue = VecDeque::from([(root, 0)]);
        let mut seen = HashSet::new();
        let mut nodes = vec![];
        let mut complete = true;
        let mut traversal_complete = true;
        while !queue.is_empty() && nodes.len() < request.max_nodes {
            let (element, depth) = queue.pop_front().unwrap();
            let reference = self.intern(&element);
            if !seen.insert(reference.id) {
                continue;
            }
            // SAFETY: set timeout on this exact retained object, without changing host process globals.
            unsafe {
                check(element.set_messaging_timeout(2.0), "Set AX timeout", false)?;
            }
            let mut attributes = BTreeMap::new();
            let mut issues = vec![];
            let attr_names = match names(&element, false, false) {
                Ok(v) => v,
                Err(e) => {
                    issues.push(json!(e));
                    complete = false;
                    traversal_complete = false;
                    vec![]
                }
            };
            let mut children = vec![];
            for name in attr_names {
                match attribute(&element, &name) {
                    Ok(value) => {
                        if name == "AXChildren"
                            && let Some(array) = value.downcast_ref::<CFArray>()
                        {
                            for child in cf_items(array).into_iter() {
                                if let Ok(child) = child.downcast::<AXUIElement>() {
                                    children.push(child);
                                } else {
                                    complete = false;
                                    traversal_complete = false;
                                    issues.push(
                                        json!({"code":"native_type", "attribute":"AXChildren"}),
                                    );
                                }
                            }
                        }
                        if name == "AXChildren" && value.downcast_ref::<CFArray>().is_none() {
                            complete = false;
                            traversal_complete = false;
                            issues.push(json!({"code":"native_type", "attribute":"AXChildren", "expected":"array"}));
                        }
                        attributes.insert(name, self.encode(&value, 0));
                    }
                    Err(e) => {
                        if e.code != "ax_-25212" && e.code != "ax_-25205" {
                            complete = false;
                        }
                        if name == "AXChildren" && e.code != "ax_-25212" && e.code != "ax_-25205" {
                            traversal_complete = false;
                        }
                        attributes.insert(name, json!({"type":"read_error", "error":e}));
                    }
                }
            }
            let actions = match names(&element, true, false) {
                Ok(v) => v,
                Err(e) => {
                    complete = false;
                    issues.push(json!(e));
                    vec![]
                }
            };
            let parameterized = match names(&element, false, true) {
                Ok(v) => v,
                Err(e) => {
                    complete = false;
                    issues.push(json!(e));
                    vec![]
                }
            };
            let child_refs = children.iter().map(|v| self.intern(v)).collect();
            if depth < request.max_depth {
                queue.extend(children.into_iter().map(|c| (c, depth + 1)));
            } else if !children.is_empty() {
                complete = false;
                traversal_complete = false;
                issues.push(json!({"code":"depth_limit"}));
            }
            nodes.push(Node {
                reference,
                attributes,
                actions,
                parameterized_attributes: parameterized,
                children: child_refs,
                issues,
            });
        }
        queue.retain(|(element, _)| !seen.contains(&self.intern(element).id));
        if !queue.is_empty() {
            traversal_complete = false;
            complete = false;
        }
        self.revision += 1;
        Ok(Snapshot {
            root: root_ref,
            nodes,
            complete,
            traversal_complete,
            revision: self.revision,
            issues: if queue.is_empty() {
                vec![]
            } else {
                vec![json!({"code":"node_limit", "pending_nodes":queue.len()})]
            },
        })
    }
}
fn set_value(element: &AXUIElement, attribute: &str, value: &CFType) -> Result<()> {
    let name = CFString::from_str(attribute);
    let mut settable = 0;
    // SAFETY: retained element/value/name and initialized out pointer.
    unsafe {
        check(
            element.is_attribute_settable(&name, NonNull::from(&mut settable)),
            attribute,
            false,
        )?;
        if settable == 0 {
            return Err(error(
                "unsupported",
                "Attribute is not settable",
                Effect::None,
            ));
        }
        check(element.set_attribute_value(&name, value), attribute, true)
    }
}
impl SemanticActions for Accessibility {
    fn semantic(&mut self, target: &ElementRef, action: SemanticAction) -> Result<Receipt> {
        let element = self.resolve(target)?;
        match action {
            // SAFETY: valid retained element and action name; native result determines uncertainty.
            SemanticAction::Perform { name } => unsafe {
                check(
                    element.perform_action(&CFString::from_str(&name)),
                    &name,
                    true,
                )?;
            },
            SemanticAction::SetString { attribute, value } => {
                set_value(&element, &attribute, &CFString::from_str(&value))?
            }
            SemanticAction::SetBool { attribute, value } => {
                set_value(&element, &attribute, CFBoolean::new(value))?
            }
            SemanticAction::SetInteger { attribute, value } => {
                set_value(&element, &attribute, &CFNumber::new_i64(value))?
            }
            SemanticAction::SetFloat { attribute, value } => {
                if !value.is_finite() {
                    return Err(error(
                        "invalid_request",
                        "Finite set value required",
                        Effect::None,
                    ));
                }
                set_value(&element, &attribute, &CFNumber::new_f64(value))?;
            }
            SemanticAction::SetRange {
                attribute,
                location,
                length,
            } => {
                if location < 0 || length < 0 || location.checked_add(length).is_none() {
                    return Err(error(
                        "invalid_request",
                        "Invalid UTF-16 range",
                        Effect::None,
                    ));
                }
                let range = CFRange::new(location as isize, length as isize);
                // SAFETY: matching CFRange layout remains alive during AXValueCreate's copy.
                let value =
                    unsafe { AXValue::new(AXValueType::CFRange, NonNull::from(&range).cast()) }
                        .ok_or_else(|| {
                            error("native_value", "Cannot create range", Effect::None)
                        })?;
                set_value(&element, &attribute, &value)?;
            }
            SemanticAction::SetPoint { attribute, value } => {
                if !value.x.is_finite() || !value.y.is_finite() {
                    return Err(error(
                        "invalid_request",
                        "Finite point required",
                        Effect::None,
                    ));
                }
                let point = CGPoint::new(value.x, value.y);
                // SAFETY: matching CGPoint layout remains alive during copy.
                let value =
                    unsafe { AXValue::new(AXValueType::CGPoint, NonNull::from(&point).cast()) }
                        .ok_or_else(|| {
                            error("native_value", "Cannot create point", Effect::None)
                        })?;
                set_value(&element, &attribute, &value)?;
            }
            SemanticAction::SetSize {
                attribute,
                width,
                height,
            } => {
                if !width.is_finite() || !height.is_finite() || width <= 0. || height <= 0. {
                    return Err(error(
                        "invalid_request",
                        "Positive finite size required",
                        Effect::None,
                    ));
                }
                let size = CGSize::new(width, height);
                // SAFETY: matching CGSize layout remains alive during copy.
                let value =
                    unsafe { AXValue::new(AXValueType::CGSize, NonNull::from(&size).cast()) }
                        .ok_or_else(|| error("native_value", "Cannot create size", Effect::None))?;
                set_value(&element, &attribute, &value)?;
            }
        }
        Ok(Receipt {
            effect: Effect::Dispatched,
            route: "macos.ax".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn attributed_text_keeps_text_and_exact_native_object() {
        let mut provider = Accessibility::new();
        let text = CFString::from_str("Sound 🦀");
        // SAFETY: default allocator and an empty attribute dictionary are allowed.
        let attributed = unsafe { CFAttributedString::new(None, Some(&text), None) }.unwrap();
        let encoded = provider.encode(&attributed, 0);
        assert_eq!(encoded["type"], "attributed_string");
        assert_eq!(encoded["text"]["value"], "Sound 🦀");
        let handle = &encoded["native"];
        let native = provider
            .native_value(
                handle["session"].as_str().unwrap(),
                handle["id"].as_u64().unwrap(),
            )
            .unwrap();
        assert!(native.downcast_ref::<CFAttributedString>().is_some());
    }
    #[test]
    fn retains_unknown_native_values_and_rejects_foreign_sessions() {
        let mut provider = Accessibility::new();
        let value = objc2_core_foundation::CFData::from_bytes(&[0, 1, 255]);
        let encoded = provider.encode(&value, 0);
        assert_eq!(encoded["type"], "opaque");
        assert!(
            provider
                .native_value(
                    encoded["session"].as_str().unwrap(),
                    encoded["id"].as_u64().unwrap()
                )
                .unwrap()
                .downcast_ref::<objc2_core_foundation::CFData>()
                .is_some()
        );
        assert!(provider.native_value("foreign", 1).is_none());
    }
    #[test]
    fn preserves_nonfinite_number_bits() {
        let mut provider = Accessibility::new();
        let value = CFNumber::new_f64(f64::INFINITY);
        let encoded = provider.encode(&value, 0);
        assert_eq!(encoded["bits"], f64::INFINITY.to_bits());
        assert_eq!(encoded["value"], "inf");
    }
}

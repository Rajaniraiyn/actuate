use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    marker::PhantomData,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};
use unimation::*;
use windows_api::{
    Win32::{Foundation::HWND, System::Com::*, UI::Accessibility::*},
    core::{BSTR, Interface},
};

static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);
/// Keep this provider on one dedicated, non-UI thread. It initializes COM as MTA.
/// COM objects and their identity are retained until this session is dropped.
pub struct Accessibility {
    automation: Option<IUIAutomation>,
    elements: Vec<(ElementRef, IUIAutomationElement, i32, u64)>,
    roots: HashMap<(i32, u64, Option<u64>), ElementRef>,
    identities: HashMap<(i32, u64, Vec<i32>), usize>,
    session: String,
    next_id: u64,
    revision: u64,
    _thread_affine: PhantomData<Rc<()>>,
}
impl Accessibility {
    pub fn new() -> Result<Self> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .map_err(super::native)?;
        let automation = match unsafe {
            CoCreateInstance::<_, IUIAutomation>(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
        } {
            Ok(value) => value,
            Err(e) => {
                unsafe {
                    CoUninitialize();
                }
                return Err(super::native(e));
            }
        };
        let configuration = (|| -> windows_api::core::Result<()> {
            let options = automation.cast::<IUIAutomation2>()?;
            unsafe {
                options.SetConnectionTimeout(3000)?;
                options.SetTransactionTimeout(3000)?;
            }
            Ok(())
        })();
        if let Err(e) = configuration {
            drop(automation);
            unsafe {
                CoUninitialize();
            }
            return Err(super::native(e));
        }
        Ok(Self {
            automation: Some(automation),
            elements: vec![],
            roots: HashMap::new(),
            identities: HashMap::new(),
            session: format!(
                "windows-{}-{}-{}",
                std::process::id(),
                NEXT_SESSION.fetch_add(1, Ordering::Relaxed),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ),
            next_id: 1,
            revision: 0,
            _thread_affine: PhantomData,
        })
    }
    fn automation(&self) -> &IUIAutomation {
        self.automation.as_ref().expect("live COM apartment")
    }
    fn allocate(&mut self) -> ElementRef {
        let reference = ElementRef {
            session: self.session.clone(),
            id: self.next_id,
        };
        self.next_id += 1;
        reference
    }
    fn intern(&mut self, element: &IUIAutomationElement) -> Result<ElementRef> {
        let pid = unsafe { element.CurrentProcessId() }.map_err(super::native)?;
        let generation = super::windows::process_generation(pid as u32)?;
        let identity = (pid, generation, runtime_id(element)?);
        if let Some(index) = self.identities.get(&identity) {
            let (reference, previous, ..) = &self.elements[*index];
            // Runtime IDs can be reused after destruction. Never revive a dead handle.
            if unsafe { previous.CurrentProcessId() }.is_ok() {
                match unsafe { self.automation().CompareElements(previous, element) } {
                    Ok(same) if same.as_bool() => return Ok(reference.clone()),
                    Ok(_) => {}
                    Err(e) => return Err(super::error("identity_comparison", e.to_string())),
                }
            }
        }
        if self.elements.len() >= 20_000 {
            return Err(super::error(
                "reference_capacity",
                "Session retained 20000 native elements; start a new session",
            ));
        }
        let reference = self.allocate();
        self.identities.insert(identity, self.elements.len());
        self.elements
            .push((reference.clone(), element.clone(), pid, generation));
        Ok(reference)
    }
    fn resolve(&self, reference: &ElementRef) -> Result<&IUIAutomationElement> {
        if reference.session != self.session {
            return Err(super::error(
                "wrong_session",
                "Element belongs to another UI Automation session",
            ));
        }
        let (_, element, pid, generation) = self
            .elements
            .iter()
            .find(|(candidate, ..)| candidate == reference)
            .ok_or_else(|| {
                super::error(
                    "unknown_reference",
                    "Unknown or synthetic application reference",
                )
            })?;
        if super::windows::process_generation(*pid as u32)? != *generation {
            return Err(super::error(
                "stale_reference",
                "Target process was replaced",
            ));
        }
        unsafe { element.CurrentProcessId() }
            .map_err(|e| super::error("stale_reference", e.to_string()))?;
        Ok(element)
    }
    pub fn inspect(&mut self, reference: &ElementRef) -> Result<Node> {
        let _dpi = super::windows::DpiGuard::new()?;
        let element = self.resolve(reference)?.clone();
        let mut node = self.node(&element)?;
        let mut point = windows_api::Win32::Foundation::POINT::default();
        match unsafe { element.GetClickablePoint(&mut point) } {
            Ok(available) if available.as_bool() => {
                node.attributes.insert(
                    "clickable_point".into(),
                    json!({"x":point.x,"y":point.y,"coordinate_space":"physical_desktop_pixels"}),
                );
            }
            Ok(_) => {
                node.attributes
                    .insert("clickable_point".into(), Value::Null);
            }
            Err(error) => node
                .issues
                .push(json!({"attribute":"clickable_point","error":error.to_string()})),
        }
        Ok(node)
    }
    /// UIA desktop children include shell/XAML roots omitted by EnumWindows.
    /// This is bounded root discovery, not a complete desktop-tree snapshot.
    pub fn desktop_roots(&mut self) -> Result<Value> {
        let _dpi = super::windows::DpiGuard::new()?;
        let root = unsafe { self.automation().GetRootElement() }.map_err(super::native)?;
        let walker = unsafe { self.automation().RawViewWalker() }.map_err(super::native)?;
        let mut next = unsafe { walker.GetFirstChildElement(&root) };
        let mut nodes = vec![];
        let mut issues = vec![];
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut attempted = 0;
        loop {
            match next {
                Ok(element) => {
                    if attempted >= 512 || std::time::Instant::now() >= deadline {
                        issues.push(json!({"reason":"root_discovery_limit"}));
                        break;
                    }
                    attempted += 1;
                    match self.node(&element) {
                        Ok(node) => nodes.push(node),
                        Err(error) => issues.push(json!({"reason":"node_query","error":error})),
                    }
                    next = unsafe { walker.GetNextSiblingElement(&element) };
                }
                Err(error) if error.code().0 == 0 || error.code().0 == 0x80004003_u32 as i32 => {
                    break;
                }
                Err(error) => {
                    issues.push(json!({"reason":"root_query","error":error.to_string()}));
                    break;
                }
            }
        }
        let complete = issues.is_empty() && nodes.iter().all(|node| node.issues.is_empty());
        Ok(json!({"roots":nodes,"complete":complete,"issues":issues}))
    }
    fn node(&mut self, element: &IUIAutomationElement) -> Result<Node> {
        let reference = self.intern(element)?;
        let mut attributes = BTreeMap::new();
        let mut issues = vec![];
        macro_rules! property {
            ($key:expr, $value:expr) => { match $value { Ok(value) => { attributes.insert($key.into(), json!(value)); }, Err(e) => issues.push(json!({"attribute":$key,"error":e.to_string()})) } };
        }
        unsafe {
            property!("name", element.CurrentName().map(|v| v.to_string()));
            property!(
                "automation_id",
                element.CurrentAutomationId().map(|v| v.to_string())
            );
            property!(
                "framework_id",
                element.CurrentFrameworkId().map(|v| v.to_string())
            );
            property!("control_type", element.CurrentControlType().map(|v| v.0));
            property!(
                "role",
                element.CurrentLocalizedControlType().map(|v| v.to_string())
            );
            property!("enabled", element.CurrentIsEnabled().map(|v| v.as_bool()));
            property!(
                "offscreen",
                element.CurrentIsOffscreen().map(|v| v.as_bool())
            );
            property!(
                "is_password",
                element.CurrentIsPassword().map(|v| v.as_bool())
            );
            property!("pid", element.CurrentProcessId());
            property!(
                "native_window_handle",
                element
                    .CurrentNativeWindowHandle()
                    .map(|h| h.0 as usize as u64)
            );
            property!("bounds", element.CurrentBoundingRectangle().map(|r| json!({"x":r.left,"y":r.top,"width":r.right-r.left,"height":r.bottom-r.top,"coordinate_space":"physical_desktop_pixels"})));
        }
        let mut actions = vec![];
        if unsafe { element.CurrentIsKeyboardFocusable() }.is_ok_and(|v| v.as_bool()) {
            actions.push("focus".into());
        }
        unsafe {
            if element
                .GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)
                .is_ok()
            {
                actions.push("invoke".into());
            }
            if let Ok(pattern) =
                element.GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId)
            {
                actions.push("toggle".into());
                property!("toggle_state", pattern.CurrentToggleState().map(|v| v.0));
            }
            if let Ok(pattern) = element.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(
                UIA_SelectionItemPatternId,
            ) {
                actions.push("select".into());
                property!("selected", pattern.CurrentIsSelected().map(|v| v.as_bool()));
            }
            if let Ok(pattern) = element.GetCurrentPatternAs::<IUIAutomationExpandCollapsePattern>(
                UIA_ExpandCollapsePatternId,
            ) {
                actions.extend(["expand".into(), "collapse".into()]);
                property!(
                    "expand_collapse_state",
                    pattern.CurrentExpandCollapseState().map(|v| v.0)
                );
            }
            if element
                .GetCurrentPatternAs::<IUIAutomationScrollItemPattern>(UIA_ScrollItemPatternId)
                .is_ok()
            {
                actions.push("scroll_into_view".into());
            }
            if let Ok(pattern) =
                element.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
            {
                property!("value", pattern.CurrentValue().map(|v| v.to_string()));
                property!(
                    "value_read_only",
                    pattern.CurrentIsReadOnly().map(|v| v.as_bool())
                );
                if pattern.CurrentIsReadOnly().is_ok_and(|v| !v.as_bool()) {
                    actions.push("set_value".into());
                }
            }
        }
        Ok(Node {
            reference,
            attributes,
            actions,
            parameterized_attributes: vec![],
            children: vec![],
            issues,
        })
    }
}
fn runtime_id(element: &IUIAutomationElement) -> Result<Vec<i32>> {
    use windows_api::Win32::System::Ole::*;
    struct Array(*mut SAFEARRAY);
    impl Drop for Array {
        fn drop(&mut self) {
            unsafe {
                let _ = SafeArrayDestroy(self.0);
            }
        }
    }
    let array = Array(unsafe { element.GetRuntimeId() }.map_err(super::native)?);
    if array.0.is_null()
        || unsafe { SafeArrayGetDim(array.0) } != 1
        || unsafe { SafeArrayGetElemsize(array.0) } != 4
    {
        return Err(super::error(
            "invalid_runtime_id",
            "Expected a one-dimensional UIA integer runtime ID",
        ));
    }
    let low = unsafe { SafeArrayGetLBound(array.0, 1) }.map_err(super::native)?;
    let high = unsafe { SafeArrayGetUBound(array.0, 1) }.map_err(super::native)?;
    if !(1..=1024).contains(&(i64::from(high) - i64::from(low) + 1)) {
        return Err(super::error(
            "invalid_runtime_id",
            "Unexpected UIA runtime ID length",
        ));
    }
    (low..=high)
        .map(|index| {
            let mut value = 0i32;
            unsafe { SafeArrayGetElement(array.0, &index, (&mut value as *mut i32).cast()) }
                .map_err(super::native)?;
            Ok(value)
        })
        .collect()
}
impl Drop for Accessibility {
    fn drop(&mut self) {
        self.elements.clear();
        self.automation.take();
        unsafe {
            CoUninitialize();
        }
    }
}
impl Discover for Accessibility {
    fn discover(&mut self) -> Result<Value> {
        super::WindowDiscovery.discover()
    }
}
impl Observe for Accessibility {
    fn observe(&mut self, request: ObserveRequest) -> Result<Snapshot> {
        self.observe_windows(request, None)
    }
}
impl Accessibility {
    /// Restrict observation to one verified HWND owned by request.pid.
    pub fn observe_window(&mut self, request: ObserveRequest, window_id: u64) -> Result<Snapshot> {
        self.observe_windows(request, Some(window_id))
    }

    fn observe_windows(
        &mut self,
        request: ObserveRequest,
        window_id: Option<u64>,
    ) -> Result<Snapshot> {
        if request.pid <= 0
            || request.max_nodes == 0
            || request.max_nodes > 20_000
            || request.max_depth > 256
        {
            return Err(super::error(
                "invalid_observation",
                "Positive PID, 1..20000 nodes and depth <=256 required",
            ));
        }
        let _dpi = super::windows::DpiGuard::new()?;
        let windows: Vec<u64> = if let Some(id) = window_id {
            use windows_api::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, IsWindow};
            let raw = usize::try_from(id).map_err(|_| {
                super::error(
                    "target_not_found",
                    "Window handle exceeds native pointer width",
                )
            })?;
            let handle = HWND(raw as *mut _);
            let mut owner = 0;
            if id == 0
                || !unsafe { IsWindow(Some(handle)) }.as_bool()
                || unsafe { GetWindowThreadProcessId(handle, Some(&mut owner)) } == 0
                || owner != request.pid as u32
            {
                return Err(super::error(
                    "target_not_found",
                    "Window does not belong to the requested process",
                ));
            }
            vec![id]
        } else {
            super::discover_windows()?
                .into_iter()
                .filter(|w| w.pid == request.pid as u32)
                .map(|w| w.window_id)
                .collect()
        };
        if windows.is_empty() {
            return Err(super::error(
                "target_not_found",
                "No top-level windows for this process",
            ));
        }
        let process_key = (
            request.pid,
            super::windows::process_generation(request.pid as u32)?,
            window_id,
        );
        let root = if let Some(root) = self.roots.get(&process_key) {
            root.clone()
        } else {
            let root = self.allocate();
            self.roots.insert(process_key, root.clone());
            root
        };
        let mut nodes = vec![Node {
            reference: root.clone(),
            attributes: BTreeMap::from([
                ("role".into(), json!("application")),
                ("pid".into(), json!(request.pid)),
            ]),
            actions: vec![],
            parameterized_attributes: vec![],
            children: vec![],
            issues: vec![],
        }];
        let walker = unsafe { self.automation().RawViewWalker() }.map_err(super::native)?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut pending = Vec::new();
        let mut issues = vec![];
        let mut complete = true;
        for window in windows.into_iter().rev() {
            match unsafe {
                self.automation()
                    .ElementFromHandle(HWND(window as usize as *mut _))
            } {
                Ok(element) => pending.push((element, 0usize, 1usize)),
                Err(e) => {
                    complete = false;
                    issues.push(json!({"window_id":window,"error":e.to_string()}));
                }
            }
        }
        let mut visited = std::collections::HashSet::new();
        while let Some((element, parent, depth)) = pending.pop() {
            if std::time::Instant::now() >= deadline {
                complete = false;
                issues.push(json!({"reason":"observation_deadline","timeout_ms":10000}));
                break;
            }
            if nodes.len() >= request.max_nodes {
                complete = false;
                issues.push(json!({"reason":"node_limit"}));
                break;
            }
            if depth > request.max_depth {
                complete = false;
                issues.push(json!({"reason":"depth_limit"}));
                continue;
            }
            let node = match self.node(&element) {
                Ok(node) => node,
                Err(error) => {
                    complete = false;
                    nodes[parent]
                        .issues
                        .push(json!({"reason":"node_query","error":error}));
                    continue;
                }
            };
            if !visited.insert(node.reference.clone()) {
                complete = false;
                issues.push(json!({"reason":"repeated_native_element","reference":node.reference}));
                continue;
            }
            nodes[parent].children.push(node.reference.clone());
            let index = nodes.len();
            nodes.push(node);
            if depth >= request.max_depth {
                complete = false;
                nodes[index].issues.push(json!({"reason":"depth_limit"}));
                continue;
            }
            // A successful COM call can return a null interface at list end.
            // windows-rs represents that as an Error with S_OK; some providers
            // use E_POINTER instead. Neither denotes a failed traversal here.
            let mut child = unsafe { walker.GetFirstChildElement(&element) };
            let mut children = vec![];
            while let Ok(current) = child.clone() {
                if std::time::Instant::now() >= deadline {
                    complete = false;
                    issues.push(json!({"reason":"observation_deadline","timeout_ms":10000}));
                    break;
                }
                if children.len() + nodes.len() >= request.max_nodes {
                    complete = false;
                    issues.push(json!({"reason":"node_limit"}));
                    break;
                }
                child = unsafe { walker.GetNextSiblingElement(&current) };
                children.push(current);
            }
            if let Err(e) = child
                && e.code().0 != 0
                && e.code().0 != 0x80004003_u32 as i32
            {
                complete = false;
                nodes[index]
                    .issues
                    .push(json!({"reason":"child_query","error":e.to_string()}));
            }
            pending.extend(children.into_iter().rev().map(|e| (e, index, depth + 1)));
        }
        self.revision += 1;
        let traversal_complete = complete;
        complete &= nodes.iter().all(|node| node.issues.is_empty());
        Ok(Snapshot {
            root,
            nodes,
            complete,
            traversal_complete,
            revision: self.revision,
            issues,
        })
    }
}
impl SemanticActions for Accessibility {
    fn semantic(&mut self, target: &ElementRef, action: SemanticAction) -> Result<Receipt> {
        let element = self.resolve(target)?;
        let result = unsafe {
            match action {
                SemanticAction::Perform { name } => match name.as_str() {
                    "focus" => element.SetFocus(),
                    "invoke" => element
                        .GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)
                        .and_then(|p| p.Invoke()),
                    "toggle" => element
                        .GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId)
                        .and_then(|p| p.Toggle()),
                    "select" => element
                        .GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(
                            UIA_SelectionItemPatternId,
                        )
                        .and_then(|p| p.Select()),
                    "expand" => element
                        .GetCurrentPatternAs::<IUIAutomationExpandCollapsePattern>(
                            UIA_ExpandCollapsePatternId,
                        )
                        .and_then(|p| p.Expand()),
                    "collapse" => element
                        .GetCurrentPatternAs::<IUIAutomationExpandCollapsePattern>(
                            UIA_ExpandCollapsePatternId,
                        )
                        .and_then(|p| p.Collapse()),
                    "scroll_into_view" => element
                        .GetCurrentPatternAs::<IUIAutomationScrollItemPattern>(
                            UIA_ScrollItemPatternId,
                        )
                        .and_then(|p| p.ScrollIntoView()),
                    _ => return Err(super::error("unsupported_action", name)),
                },
                SemanticAction::SetString { attribute, value } if attribute == "value" => element
                    .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                    .and_then(|p| p.SetValue(&BSTR::from(value))),
                _ => {
                    return Err(super::error(
                        "unsupported_action",
                        "Only named patterns and string value are implemented",
                    ));
                }
            }
        };
        result.map_err(|e| NativeError {
            effect: Effect::Unknown,
            ..super::native(e)
        })?;
        Ok(super::dispatched("windows_uia"))
    }
}

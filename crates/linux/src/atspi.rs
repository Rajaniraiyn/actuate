//! AT-SPI2 observation and semantic actions over D-Bus.
//!
//! Every accessible is a (bus name, object path) pair owned by the target
//! application. This provider interns those pairs into session-local
//! references and reads native properties without renaming them. Toolkits
//! that gate accessibility on the bus `IsEnabled` flag (Chromium, Electron,
//! Qt) stay invisible until that flag is set explicitly.
use actuate::{
    Discover, Effect, ElementRef, NativeError, Node, ObservationBudget, ObserveScope, Point,
    Receipt, Result, SemanticAction, SemanticActions, Snapshot, geometry::Rect, schema::ATSPI,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    future::Future,
    time::Duration,
};
use zbus::{Connection, zvariant::OwnedObjectPath};
use zvariant::{OwnedValue, Value as ZValue};

const ACCESSIBLE: &str = "org.a11y.atspi.Accessible";
const COMPONENT: &str = "org.a11y.atspi.Component";
const ACTION: &str = "org.a11y.atspi.Action";
const TEXT: &str = "org.a11y.atspi.Text";
const EDITABLE_TEXT: &str = "org.a11y.atspi.EditableText";
const VALUE: &str = "org.a11y.atspi.Value";
const APPLICATION: &str = "org.a11y.atspi.Application";
const SELECTION: &str = "org.a11y.atspi.Selection";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const REGISTRY: &str = "org.a11y.atspi.Registry";
const ROOT_PATH: &str = "/org/a11y/atspi/accessible/root";
const NULL_PATH: &str = "/org/a11y/atspi/null";
const COORD_SCREEN: u32 = 0;
const COORD_WINDOW: u32 = 1;
/// Text longer than this is summarized by length; read it with `attribute`.
const INLINE_TEXT_LIMIT: i32 = 4096;

/// Canonical AT-SPI role names in `AtspiRole` enum order. `GetRole` is the
/// stable cross-toolkit value; `GetRoleName` is whatever the toolkit says
/// (GTK4 reports "button" where the enum says "push button").
pub const ROLES: [&str; 131] = [
    "invalid",
    "accelerator label",
    "alert",
    "animation",
    "arrow",
    "calendar",
    "canvas",
    "check box",
    "check menu item",
    "color chooser",
    "column header",
    "combo box",
    "date editor",
    "desktop icon",
    "desktop frame",
    "dial",
    "dialog",
    "directory pane",
    "drawing area",
    "file chooser",
    "filler",
    "focus traversable",
    "font chooser",
    "frame",
    "glass pane",
    "html container",
    "icon",
    "image",
    "internal frame",
    "label",
    "layered pane",
    "list",
    "list item",
    "menu",
    "menu bar",
    "menu item",
    "option pane",
    "page tab",
    "page tab list",
    "panel",
    "password text",
    "popup menu",
    "progress bar",
    "push button",
    "radio button",
    "radio menu item",
    "root pane",
    "row header",
    "scroll bar",
    "scroll pane",
    "separator",
    "slider",
    "spin button",
    "split pane",
    "status bar",
    "table",
    "table cell",
    "table column header",
    "table row header",
    "tearoff menu item",
    "terminal",
    "text",
    "toggle button",
    "tool bar",
    "tool tip",
    "tree",
    "tree table",
    "unknown",
    "viewport",
    "window",
    "extended",
    "header",
    "footer",
    "paragraph",
    "ruler",
    "application",
    "autocomplete",
    "editbar",
    "embedded",
    "entry",
    "chart",
    "caption",
    "document frame",
    "heading",
    "page",
    "section",
    "redundant object",
    "form",
    "link",
    "input method window",
    "table row",
    "tree item",
    "document spreadsheet",
    "document presentation",
    "document text",
    "document web",
    "document email",
    "comment",
    "list box",
    "grouping",
    "image map",
    "notification",
    "info bar",
    "level bar",
    "title bar",
    "block quote",
    "audio",
    "video",
    "definition",
    "article",
    "landmark",
    "log",
    "marquee",
    "math",
    "rating",
    "timer",
    "static",
    "math fraction",
    "math root",
    "subscript",
    "superscript",
    "description list",
    "description term",
    "description value",
    "footnote",
    "content deletion",
    "content insertion",
    "mark",
    "suggestion",
    "push button menu",
    "switch",
];

/// The canonical name for a numeric role, or `None` past the known table.
pub fn role_name(code: u32) -> Option<&'static str> {
    ROLES.get(code as usize).copied()
}

/// AT-SPI state bits in enum order. Names follow libatspi without the prefix.
pub const STATES: [&str; 44] = [
    "invalid",
    "active",
    "armed",
    "busy",
    "checked",
    "collapsed",
    "defunct",
    "editable",
    "enabled",
    "expandable",
    "expanded",
    "focusable",
    "focused",
    "has_tooltip",
    "horizontal",
    "iconified",
    "modal",
    "multi_line",
    "multiselectable",
    "opaque",
    "pressed",
    "resizable",
    "selectable",
    "selected",
    "sensitive",
    "showing",
    "single_line",
    "stale",
    "transient",
    "vertical",
    "visible",
    "manages_descendants",
    "indeterminate",
    "required",
    "truncated",
    "animated",
    "invalid_entry",
    "supports_autocompletion",
    "selectable_text",
    "is_default",
    "visited",
    "checkable",
    "has_popup",
    "read_only",
];

/// Decodes the two-word AT-SPI state set into named flags.
pub fn state_names(words: &[u32]) -> Vec<&'static str> {
    STATES
        .iter()
        .enumerate()
        .filter(|(bit, _)| {
            words
                .get(bit / 32)
                .is_some_and(|word| word & (1 << (bit % 32)) != 0)
        })
        .map(|(_, name)| *name)
        .collect()
}

/// Boolean state evidence for a node. Set states are always reported.
/// A false value is reported only where its absence is meaningful: visual
/// states need a Component, and toggles need their capability state.
pub fn state_flags(names: &[&'static str], has_component: bool) -> Vec<(&'static str, bool)> {
    let set = |s: &str| names.contains(&s);
    let mut flags: Vec<(&'static str, bool)> = names.iter().map(|n| (*n, true)).collect();
    let mut negative = |state: &'static str, meaningful: bool| {
        if meaningful && !set(state) {
            flags.push((state, false));
        }
    };
    negative("visible", has_component);
    negative("showing", has_component);
    negative("sensitive", has_component);
    negative("focused", set("focusable"));
    negative("selected", set("selectable"));
    negative("checked", set("checkable"));
    negative("expanded", set("expandable"));
    negative("pressed", set("checkable") && names.contains(&"armed"));
    flags
}

/// A native accessible identity. Bus names are unique per connection and
/// paths are unique within it; both change when the application restarts.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObjectRef {
    pub bus: String,
    pub path: String,
}
impl ObjectRef {
    fn is_null(&self) -> bool {
        self.path == NULL_PATH || self.path.is_empty()
    }
}
type Pair = (String, OwnedObjectPath);
fn pair(value: Pair) -> ObjectRef {
    ObjectRef {
        bus: value.0,
        path: value.1.to_string(),
    }
}

/// Where a toplevel sits in global logical coordinates and how its
/// toolkit's extent units relate to those coordinates. Xwayland toolkits
/// measure in X pixels, which Hyprland scales by the monitor scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowFrame {
    pub x: f64,
    pub y: f64,
    /// Toolkit units per logical unit; 1 for native Wayland clients.
    pub scale: f64,
}
impl WindowFrame {
    pub fn to_global(&self, rect: &Value) -> Option<Value> {
        let scale = if self.scale > 0. { self.scale } else { 1. };
        let field = |key: &str| rect.get(key)?.as_f64();
        Some(json!({
            "type": "rect",
            "x": self.x + field("x")? / scale,
            "y": self.y + field("y")? / scale,
            "width": field("width")? / scale,
            "height": field("height")? / scale,
        }))
    }
}

/// Window frames supplied by the compositor adapter. AT-SPI on Wayland
/// cannot know where a toplevel is.
pub trait WindowOrigins {
    /// The frame of the toplevel with this pid and title, when unambiguous.
    fn origin(&self, pid: i32, title: &str) -> Option<WindowFrame>;
    fn source(&self) -> &'static str;
}
/// Global bounds derived from window-relative extents and a compositor frame.
fn apply_origin(attributes: &mut BTreeMap<String, Value>, frame: WindowFrame, source: &str) {
    if attributes.contains_key("bounds") {
        return;
    }
    if let Some(global) = attributes
        .get("bounds_window")
        .and_then(|r| frame.to_global(r))
    {
        attributes.insert("bounds".into(), global);
        attributes.insert(
            "bounds_source".into(),
            json!({"type":"string","value":format!("window_extents_plus_{source}")}),
        );
    }
}

/// A discovered application registered on the accessibility bus.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Application {
    pub bus: String,
    pub pid: Option<i32>,
    pub name: String,
    pub toolkit: Option<String>,
    pub version: Option<String>,
    pub child_count: Option<i32>,
    pub reference: ElementRef,
}

/// A failed native mutation has an unknown effect unless the method itself
/// is unsupported, in which case nothing was attempted.
fn mutation(e: NativeError) -> NativeError {
    if e.code == "unsupported" {
        e
    } else {
        e.with_effect(Effect::Unknown)
    }
}

fn timeout_error() -> NativeError {
    NativeError::new("atspi_timeout", "Accessibility call timed out").with_effect(Effect::Unknown)
}

pub struct AtSpi {
    session: String,
    bus_address: String,
    connection: Connection,
    session_bus: Connection,
    elements: Vec<ObjectRef>,
    index: HashMap<ObjectRef, usize>,
    pids: HashMap<String, Option<i32>>,
    revision: u64,
    timeout: Duration,
    /// Compositor adapter for toplevel frames; without one, screen extents
    /// are the only global evidence and are reported as unverified.
    origins: Option<Box<dyn WindowOrigins>>,
}

impl AtSpi {
    /// Connects to the accessibility bus advertised on the session bus.
    pub fn connect() -> Result<Self> {
        Self::connect_origins(None)
    }
    pub fn connect_with(origins: Box<dyn WindowOrigins>) -> Result<Self> {
        Self::connect_origins(Some(origins))
    }
    fn connect_origins(origins: Option<Box<dyn WindowOrigins>>) -> Result<Self> {
        let timeout = Duration::from_secs(5);
        let (session_bus, connection, bus_address) = block(timeout, async {
            let session_bus = Connection::session().await?;
            let reply = session_bus
                .call_method(
                    Some("org.a11y.Bus"),
                    "/org/a11y/bus",
                    Some("org.a11y.Bus"),
                    "GetAddress",
                    &(),
                )
                .await?;
            let address: String = reply.body().deserialize()?;
            let connection = zbus::connection::Builder::address(address.as_str())?
                .build()
                .await?;
            Ok((session_bus, connection, address))
        })
        .map_err(|e| NativeError::new("atspi_unavailable", e.message))?;
        let session = format!(
            "atspi-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        );
        Ok(Self {
            session,
            bus_address,
            connection,
            session_bus,
            elements: Vec::new(),
            index: HashMap::new(),
            pids: HashMap::new(),
            revision: 0,
            timeout: Duration::from_secs(2),
            origins,
        })
    }
    pub fn session_id(&self) -> &str {
        &self.session
    }
    fn run<T>(&self, future: impl Future<Output = zbus::Result<T>>) -> Result<T> {
        block(self.timeout, future)
    }
    fn call<T: for<'de> serde::Deserialize<'de> + zvariant::Type>(
        &self,
        target: &ObjectRef,
        interface: &str,
        method: &str,
        body: &(impl serde::Serialize + zvariant::DynamicType),
    ) -> Result<T> {
        self.run(async {
            let reply = self
                .connection
                .call_method(
                    Some(target.bus.as_str()),
                    target.path.as_str(),
                    Some(interface),
                    method,
                    body,
                )
                .await?;
            reply.body().deserialize::<T>()
        })
        .map_err(|e| NativeError {
            message: format!("{interface}.{method} at {}: {}", target.path, e.message),
            ..e
        })
    }
    fn property(&self, target: &ObjectRef, interface: &str, name: &str) -> Result<OwnedValue> {
        self.call::<OwnedValue>(target, PROPERTIES, "Get", &(interface, name))
    }
    fn set_property(
        &self,
        target: &ObjectRef,
        interface: &str,
        name: &str,
        value: ZValue<'_>,
    ) -> Result<()> {
        self.call::<()>(target, PROPERTIES, "Set", &(interface, name, value))
            .map_err(mutation)
    }
    fn properties(
        &self,
        target: &ObjectRef,
        interface: &str,
    ) -> Result<HashMap<String, OwnedValue>> {
        self.call(target, PROPERTIES, "GetAll", &(interface,))
    }
    fn intern(&mut self, object: &ObjectRef) -> ElementRef {
        let index = match self.index.get(object) {
            Some(index) => *index,
            None => {
                let index = self.elements.len();
                self.elements.push(object.clone());
                self.index.insert(object.clone(), index);
                index
            }
        };
        ElementRef {
            session: self.session.clone(),
            id: index as u64 + 1,
        }
    }
    pub(crate) fn resolve(&self, target: &ElementRef) -> Result<ObjectRef> {
        if target.session != self.session {
            return Err(NativeError::new(
                "stale_reference",
                "Foreign or retired provider session",
            ));
        }
        target
            .id
            .checked_sub(1)
            .and_then(|i| usize::try_from(i).ok())
            .and_then(|i| self.elements.get(i))
            .cloned()
            .ok_or_else(|| NativeError::new("stale_reference", "Unknown element reference"))
    }
    fn pid_of(&mut self, bus: &str) -> Option<i32> {
        if let Some(pid) = self.pids.get(bus) {
            return *pid;
        }
        let pid = self
            .run(async {
                let reply = self
                    .connection
                    .call_method(
                        Some("org.freedesktop.DBus"),
                        "/org/freedesktop/DBus",
                        Some("org.freedesktop.DBus"),
                        "GetConnectionUnixProcessID",
                        &(bus,),
                    )
                    .await?;
                reply.body().deserialize::<u32>()
            })
            .ok()
            .and_then(|pid| i32::try_from(pid).ok());
        self.pids.insert(bus.to_owned(), pid);
        pid
    }
    /// Whether toolkits are told to expose accessibility. Screen readers
    /// set this; some toolkits only register on the bus after it is set.
    pub fn status(&self) -> Result<Value> {
        let read = |name: &str| -> Result<bool> {
            self.run(async {
                let reply = self
                    .session_bus
                    .call_method(
                        Some("org.a11y.Bus"),
                        "/org/a11y/bus",
                        Some(PROPERTIES),
                        "Get",
                        &("org.a11y.Status", name),
                    )
                    .await?;
                let value: OwnedValue = reply.body().deserialize()?;
                Ok(bool::try_from(value).unwrap_or(false))
            })
        };
        Ok(json!({
            "bus_address": self.bus_address,
            "is_enabled": read("IsEnabled")?,
            "screen_reader_enabled": read("ScreenReaderEnabled")?,
        }))
    }
    /// Sets the bus-wide accessibility flag. This is a desktop-wide setting
    /// that other applications observe; it is never changed implicitly.
    pub fn set_enabled(&self, enabled: bool) -> Result<Receipt> {
        self.run(async {
            self.session_bus
                .call_method(
                    Some("org.a11y.Bus"),
                    "/org/a11y/bus",
                    Some(PROPERTIES),
                    "Set",
                    &("org.a11y.Status", "IsEnabled", ZValue::from(enabled)),
                )
                .await?;
            Ok(())
        })
        .map_err(mutation)?;
        Ok(Receipt::dispatched("linux.atspi.bus_status"))
    }
    pub fn applications(&mut self) -> Result<Vec<Application>> {
        let root = ObjectRef {
            bus: REGISTRY.into(),
            path: ROOT_PATH.into(),
        };
        let children: Vec<Pair> = self.call(&root, ACCESSIBLE, "GetChildren", &())?;
        let mut apps = Vec::new();
        for child in children.into_iter().map(pair) {
            let name = self
                .property(&child, ACCESSIBLE, "Name")
                .ok()
                .and_then(|v| String::try_from(v).ok())
                .unwrap_or_default();
            let child_count = self
                .property(&child, ACCESSIBLE, "ChildCount")
                .ok()
                .and_then(|v| i32::try_from(v).ok());
            let toolkit = self
                .property(&child, APPLICATION, "ToolkitName")
                .ok()
                .and_then(|v| String::try_from(v).ok());
            let version = self
                .property(&child, APPLICATION, "Version")
                .ok()
                .and_then(|v| String::try_from(v).ok());
            let pid = self.pid_of(&child.bus);
            let reference = self.intern(&child);
            apps.push(Application {
                bus: child.bus.clone(),
                pid,
                name,
                toolkit,
                version,
                child_count,
                reference,
            });
        }
        Ok(apps)
    }
    /// The first registered application owned by a pid, without reading
    /// the descriptive properties that discovery reports.
    pub fn application_root(&mut self, pid: i32) -> Result<ElementRef> {
        let root = ObjectRef {
            bus: REGISTRY.into(),
            path: ROOT_PATH.into(),
        };
        let children: Vec<Pair> = self.call(&root, ACCESSIBLE, "GetChildren", &())?;
        for child in children.into_iter().map(pair) {
            if self.pid_of(&child.bus) == Some(pid) {
                return Ok(self.intern(&child));
            }
        }
        Err(NativeError::new(
            "no_accessible_application",
            format!(
                "No application with pid {pid} is registered on the accessibility bus; toolkits may need the bus IsEnabled flag or an accessibility environment variable"
            ),
        ))
    }
    fn encode_error(e: NativeError) -> Value {
        json!({"type":"read_error","error":e})
    }
    fn read_node(
        &mut self,
        object: &ObjectRef,
        origin: Option<WindowFrame>,
    ) -> (Node, Vec<ObjectRef>, bool) {
        let reference = self.intern(object);
        let mut attributes = BTreeMap::new();
        let mut issues = vec![];
        let mut complete = true;
        let mut children = vec![];
        match self.properties(object, ACCESSIBLE) {
            Ok(props) => {
                for (key, value) in props {
                    let name = match key.as_str() {
                        "Name" => "name",
                        "Description" => "description",
                        "ChildCount" => "child_count",
                        "Locale" => "locale",
                        "AccessibleId" => "accessible_id",
                        "HelpText" => "help_text",
                        "Parent" => {
                            if let Ok(parent) = <Pair>::try_from(value) {
                                let parent = pair(parent);
                                if !parent.is_null() {
                                    let parent = self.intern(&parent);
                                    attributes.insert(
                                        "parent".into(),
                                        json!({"type":"element","value":parent}),
                                    );
                                }
                            }
                            continue;
                        }
                        other => other,
                    };
                    attributes.insert(name.into(), encode_value(&value));
                }
            }
            Err(e) => {
                complete = false;
                issues.push(json!(e));
            }
        }
        match self.call::<u32>(object, ACCESSIBLE, "GetRole", &()) {
            Ok(code) => {
                attributes.insert("role_code".into(), json!({"type":"integer","value":code}));
                match role_name(code) {
                    Some(role) => {
                        attributes.insert("role".into(), json!({"type":"string","value":role}));
                    }
                    None => {
                        attributes.insert(
                            "role".into(),
                            json!({"type":"string","value":format!("role {code}")}),
                        );
                    }
                }
            }
            Err(e) => {
                complete = false;
                attributes.insert("role".into(), Self::encode_error(e));
            }
        }
        match self.call::<String>(object, ACCESSIBLE, "GetRoleName", &()) {
            Ok(role) => {
                attributes.insert("role_name".into(), json!({"type":"string","value":role}));
            }
            Err(e) => {
                attributes.insert("role_name".into(), Self::encode_error(e));
            }
        }
        let interfaces: Vec<String> = match self.call(object, ACCESSIBLE, "GetInterfaces", &()) {
            Ok(v) => v,
            Err(e) => {
                complete = false;
                attributes.insert("interfaces".into(), Self::encode_error(e));
                vec![]
            }
        };
        let has = |name: &str| interfaces.iter().any(|i| i == name);
        match self.call::<Vec<u32>>(object, ACCESSIBLE, "GetState", &()) {
            Ok(words) => {
                let names = state_names(&words);
                for (state, value) in state_flags(&names, has(COMPONENT)) {
                    attributes.insert(
                        format!("state.{state}"),
                        json!({"type":"bool","value":value}),
                    );
                }
                attributes.insert("states".into(), json!({"type":"array","value":names}));
            }
            Err(e) => {
                complete = false;
                attributes.insert("states".into(), Self::encode_error(e));
            }
        }
        attributes.insert(
            "interfaces".into(),
            json!({"type":"array","value":interfaces}),
        );
        match self.call::<HashMap<String, String>>(object, ACCESSIBLE, "GetAttributes", &()) {
            Ok(map) if !map.is_empty() => {
                attributes.insert("attributes".into(), json!({"type":"map","value":map}));
            }
            Ok(_) => {}
            Err(e) => {
                attributes.insert("attributes".into(), Self::encode_error(e));
            }
        }
        attributes.insert(
            "application".into(),
            json!({"type":"string","value":object.bus}),
        );
        if has(COMPONENT) {
            // Screen extents are only evidence without a compositor adapter;
            // toolkits on Wayland report window-relative values for both.
            let coords: &[(u32, &str)] = if self.origins.is_some() {
                &[(COORD_WINDOW, "bounds_window")]
            } else {
                &[
                    (COORD_SCREEN, "bounds_screen"),
                    (COORD_WINDOW, "bounds_window"),
                ]
            };
            for (coord, key) in coords {
                match self.call::<(i32, i32, i32, i32)>(object, COMPONENT, "GetExtents", &(*coord,))
                {
                    Ok((x, y, w, h)) => {
                        attributes.insert(
                            (*key).into(),
                            json!({"type":"rect","x":x,"y":y,"width":w,"height":h}),
                        );
                    }
                    Err(e) => {
                        attributes.insert((*key).into(), Self::encode_error(e));
                    }
                }
            }
            match (origin, &self.origins) {
                (Some(frame), Some(origins)) => {
                    apply_origin(&mut attributes, frame, origins.source());
                }
                (_, None) => {
                    if let Some(screen) = attributes.get("bounds_screen").cloned()
                        && screen.get("type").and_then(Value::as_str) == Some("rect")
                    {
                        attributes.insert("bounds".into(), screen);
                        attributes.insert(
                            "bounds_source".into(),
                            json!({"type":"string","value":"screen_extents_unverified"}),
                        );
                    }
                }
                _ => {}
            }
        }
        if has(ACTION) {
            match self.call::<Vec<(String, String, String)>>(object, ACTION, "GetActions", &()) {
                Ok(list) => {
                    let detail: Vec<Value> = list
                        .iter()
                        .map(|(name, localized, key)| json!({"name":name,"localized_name":localized,"key_binding":key}))
                        .collect();
                    attributes.insert(
                        "actions_detail".into(),
                        json!({"type":"array","value":detail}),
                    );
                    let names: Vec<String> = list.into_iter().map(|(name, _, _)| name).collect();
                    attributes.insert("action_names".into(), json!({"type":"array","value":names}));
                }
                Err(e) => {
                    complete = false;
                    issues.push(json!({"code":"actions_failed","error":e}));
                }
            }
        }
        if has(TEXT) {
            match self.properties(object, TEXT) {
                Ok(props) => {
                    let count = props
                        .get("CharacterCount")
                        .and_then(|v| i32::try_from(v.clone()).ok());
                    if let Some(count) = count {
                        attributes.insert(
                            "text_length".into(),
                            json!({"type":"integer","value":count}),
                        );
                        if count <= INLINE_TEXT_LIMIT {
                            match self.call::<String>(object, TEXT, "GetText", &(0i32, count)) {
                                Ok(text) => {
                                    attributes.insert(
                                        "text".into(),
                                        json!({"type":"string","value":text}),
                                    );
                                }
                                Err(e) => {
                                    attributes.insert("text".into(), Self::encode_error(e));
                                }
                            }
                        }
                    }
                    if let Some(caret) = props
                        .get("CaretOffset")
                        .and_then(|v| i32::try_from(v.clone()).ok())
                    {
                        attributes.insert(
                            "caret_offset".into(),
                            json!({"type":"integer","value":caret}),
                        );
                    }
                }
                Err(e) => {
                    attributes.insert("text".into(), Self::encode_error(e));
                }
            }
        }
        if has(VALUE) {
            match self.properties(object, VALUE) {
                Ok(props) => {
                    for (key, name) in [
                        ("CurrentValue", "value"),
                        ("MinimumValue", "value_min"),
                        ("MaximumValue", "value_max"),
                        ("MinimumIncrement", "value_step"),
                    ] {
                        if let Some(v) = props.get(key).and_then(|v| f64::try_from(v.clone()).ok())
                        {
                            attributes.insert(name.into(), json!({"type":"float","value":v}));
                        }
                    }
                    if let Some(text) = props
                        .get("Text")
                        .and_then(|v| String::try_from(v.clone()).ok())
                    {
                        attributes
                            .insert("value_text".into(), json!({"type":"string","value":text}));
                    }
                }
                Err(e) => {
                    attributes.insert("value".into(), Self::encode_error(e));
                }
            }
        }
        if has(APPLICATION)
            && let Ok(props) = self.properties(object, APPLICATION)
        {
            for (key, name) in [
                ("ToolkitName", "toolkit"),
                ("Version", "toolkit_version"),
                ("AtspiVersion", "atspi_version"),
            ] {
                if let Some(v) = props
                    .get(key)
                    .and_then(|v| String::try_from(v.clone()).ok())
                {
                    attributes.insert(name.into(), json!({"type":"string","value":v}));
                }
            }
        }
        let child_list: Vec<ObjectRef> =
            match self.call::<Vec<Pair>>(object, ACCESSIBLE, "GetChildren", &()) {
                Ok(list) => list
                    .into_iter()
                    .map(pair)
                    .filter(|c| !c.is_null())
                    .collect(),
                Err(e) => {
                    complete = false;
                    issues.push(json!({"code":"children_failed","error":e}));
                    vec![]
                }
            };
        let actions = attributes
            .get("action_names")
            .and_then(|v| v.get("value"))
            .and_then(Value::as_array)
            .map(|names| {
                names
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let child_refs: Vec<ElementRef> = child_list.iter().map(|c| self.intern(c)).collect();
        children.extend(child_list);
        (
            Node {
                reference,
                attributes,
                actions,
                parameterized_attributes: vec![],
                children: child_refs,
                issues,
            },
            children,
            complete,
        )
    }
    fn walk(&mut self, root: ObjectRef, budget: ObservationBudget) -> Result<Snapshot> {
        if budget.max_nodes == 0 {
            return Err(NativeError::new(
                "invalid_request",
                "Positive node budget required",
            ));
        }
        let root_ref = self.intern(&root);
        let root_pid = self.pid_of(&root.bus);
        let mut queue = VecDeque::from([(root, 0usize, None::<WindowFrame>)]);
        let mut seen = HashSet::new();
        let mut nodes = vec![];
        let mut complete = true;
        let mut traversal_complete = true;
        while let Some((object, depth, mut origin)) = queue.pop_front() {
            if nodes.len() >= budget.max_nodes {
                queue.push_front((object, depth, origin));
                break;
            }
            if !seen.insert(object.clone()) {
                continue;
            }
            let (mut node, children, node_complete) = self.read_node(&object, origin);
            complete &= node_complete;
            let role = node
                .attributes
                .get("role")
                .and_then(actuate::values::string)
                .unwrap_or("")
                .to_owned();
            if origin.is_none()
                && ATSPI.is_toplevel_role(&role)
                && let Some(origins) = &self.origins
            {
                let title = node
                    .attributes
                    .get("name")
                    .and_then(actuate::values::string)
                    .unwrap_or("");
                origin = root_pid.and_then(|pid| origins.origin(pid, title));
                // The toplevel's own global bounds come from the compositor frame.
                if let Some(frame) = origin {
                    apply_origin(&mut node.attributes, frame, origins.source());
                }
            }
            if depth < budget.max_depth {
                queue.extend(children.into_iter().map(|c| (c, depth + 1, origin)));
            } else if !children.is_empty() {
                complete = false;
                traversal_complete = false;
                node.issues.push(json!({"code":"depth_limit"}));
            }
            nodes.push(node);
        }
        queue.retain(|(object, _, _)| !seen.contains(object));
        if !queue.is_empty() {
            complete = false;
            traversal_complete = false;
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
                vec![json!({"code":"node_limit","pending_nodes":queue.len()})]
            },
        })
    }
    pub fn observe_subtree(
        &mut self,
        target: &ElementRef,
        budget: ObservationBudget,
    ) -> Result<Snapshot> {
        let root = self.resolve(target)?;
        self.walk(root, budget)
    }
    /// Reads one element without traversal or requiring it in a child list.
    pub fn inspect(&mut self, target: &ElementRef) -> Result<Value> {
        let object = self.resolve(target)?;
        let origin = self.origin_for(&object);
        let (node, _, _) = self.read_node(&object, origin);
        Ok(
            json!({"reference":target,"attributes":node.attributes,"actions":node.actions,"issues":node.issues,"native":{"bus":object.bus,"path":object.path}}),
        )
    }
    /// One native property by interface and name, such as `Text.CaretOffset`.
    pub fn read_attribute(&mut self, target: &ElementRef, name: &str) -> Result<Value> {
        let object = self.resolve(target)?;
        let (interface, property) = match name.split_once('.') {
            Some((iface, prop)) => (format!("org.a11y.atspi.{iface}"), prop.to_owned()),
            None => (ACCESSIBLE.to_owned(), name.to_owned()),
        };
        if property == "Text" && interface == TEXT {
            let count: i32 = i32::try_from(self.property(&object, TEXT, "CharacterCount")?)
                .map_err(|e| NativeError::new("native_type", e))?;
            let text: String = self.call(&object, TEXT, "GetText", &(0i32, count))?;
            return Ok(json!({"type":"string","value":text}));
        }
        let value = self.property(&object, &interface, &property)?;
        Ok(encode_value(&value))
    }
    /// Global logical bounds, or an error when no coordinate space can
    /// place the element on the layout.
    pub fn element_bounds(&mut self, target: &ElementRef) -> Result<Rect> {
        let object = self.resolve(target)?;
        let origin = self.origin_for(&object);
        let (node, _, _) = self.read_node(&object, origin);
        let rect = node.attributes.get("bounds").ok_or_else(|| {
            NativeError::new(
                "invalid_geometry",
                "Element has no global bounds; it lacks Component extents or its window is unknown to the compositor",
            )
        })?;
        let rect = Rect {
            x: rect["x"].as_f64().unwrap_or(f64::NAN),
            y: rect["y"].as_f64().unwrap_or(f64::NAN),
            width: rect["width"].as_f64().unwrap_or(f64::NAN),
            height: rect["height"].as_f64().unwrap_or(f64::NAN),
        };
        if !rect.valid() {
            return Err(NativeError::new(
                "invalid_geometry",
                "Element has no finite nonempty bounds",
            ));
        }
        Ok(rect)
    }
    pub fn element_center(&mut self, target: &ElementRef) -> Result<Point> {
        Ok(self.element_bounds(target)?.center())
    }
    /// Role, name and parent with two calls, for ancestor walks.
    fn light(&mut self, object: &ObjectRef) -> Result<(String, String, Option<ObjectRef>)> {
        let props = self.properties(object, ACCESSIBLE)?;
        let name = props
            .get("Name")
            .and_then(|v| String::try_from(v.clone()).ok())
            .unwrap_or_default();
        let parent = props
            .get("Parent")
            .and_then(|v| <Pair>::try_from(v.clone()).ok())
            .map(pair)
            .filter(|p| !p.is_null());
        let role: u32 = self.call(object, ACCESSIBLE, "GetRole", &())?;
        Ok((
            role_name(role).unwrap_or("unknown").to_owned(),
            name,
            parent,
        ))
    }
    /// The pid and title of the toplevel containing an element.
    fn toplevel_of(&mut self, object: &ObjectRef) -> Result<(i32, String)> {
        let pid = self
            .pid_of(&object.bus)
            .ok_or_else(|| NativeError::new("no_pid", "Bus name has no process id"))?;
        let mut current = object.clone();
        let mut title = None;
        for _ in 0..64 {
            let (role, name, parent) = self.light(&current)?;
            if role == "application" {
                return title.map(|t| (pid, t)).ok_or_else(|| {
                    NativeError::new("no_window", "Element has no toplevel ancestor")
                });
            }
            if ATSPI.is_toplevel_role(&role) {
                title = Some(name);
            }
            match parent {
                Some(parent) if parent != current => current = parent,
                _ => break,
            }
        }
        Err(NativeError::new(
            "no_window",
            "Element has no application ancestor",
        ))
    }
    fn origin_for(&mut self, object: &ObjectRef) -> Option<WindowFrame> {
        self.origins.as_ref()?;
        let (pid, title) = self.toplevel_of(object).ok()?;
        self.origins.as_ref()?.origin(pid, &title)
    }
    /// The pid and toplevel title that own an element.
    pub fn owning_window(&mut self, target: &ElementRef) -> Result<(i32, String)> {
        let object = self.resolve(target)?;
        self.toplevel_of(&object)
    }
    /// Descends from a frame through `GetAccessibleAtPoint` in window coordinates.
    pub fn hit_test_window(
        &mut self,
        frame: &ElementRef,
        local_x: i32,
        local_y: i32,
    ) -> Result<ElementRef> {
        let mut object = self.resolve(frame)?;
        for _ in 0..64 {
            let child: Pair = self.call(
                &object,
                COMPONENT,
                "GetAccessibleAtPoint",
                &(local_x, local_y, COORD_WINDOW),
            )?;
            let child = pair(child);
            if child.is_null() || child == object {
                break;
            }
            object = child;
        }
        Ok(self.intern(&object))
    }
    /// The toplevel child of an application root whose name is the window
    /// title, or the only child when titles differ.
    pub fn frame_for_title(&mut self, app: &ElementRef, title: &str) -> Result<ElementRef> {
        let root = self.resolve(app)?;
        let children: Vec<ObjectRef> = self
            .call::<Vec<Pair>>(&root, ACCESSIBLE, "GetChildren", &())?
            .into_iter()
            .map(pair)
            .collect();
        let mut named = Vec::new();
        for child in &children {
            let name = self
                .property(child, ACCESSIBLE, "Name")
                .ok()
                .and_then(|v| String::try_from(v).ok())
                .unwrap_or_default();
            if name == title {
                named.push(child.clone());
            }
        }
        let frame = match (named.as_slice(), children.as_slice()) {
            ([one], _) => one.clone(),
            ([], [only]) => only.clone(),
            _ => {
                return Err(NativeError::new(
                    "no_window",
                    "Window has no unique accessible frame with its title",
                ));
            }
        };
        Ok(self.intern(&frame))
    }
    /// Performs the schema's activation action when the element advertises one.
    pub fn activate(&mut self, target: &ElementRef) -> Result<Receipt> {
        let object = self.resolve(target)?;
        let actions: Vec<(String, String, String)> =
            self.call(&object, ACTION, "GetActions", &())?;
        let names: Vec<String> = actions.into_iter().map(|(n, _, _)| n).collect();
        let name = ATSPI.activate_action(&names).ok_or_else(|| {
            NativeError::unsupported(format!(
                "No activation action advertised; available actions: {names:?}"
            ))
        })?;
        self.semantic(
            target,
            SemanticAction::Perform {
                name: name.to_owned(),
            },
        )
    }
}

/// Encodes a D-Bus value with the shared type-tagged JSON convention.
pub fn encode_value(value: &OwnedValue) -> Value {
    fn inner(value: &ZValue<'_>, depth: usize) -> Value {
        if depth > 32 {
            return json!({"type":"opaque","reason":"encoding_depth_limit"});
        }
        match value {
            ZValue::U8(v) => json!({"type":"integer","value":v}),
            ZValue::Bool(v) => json!({"type":"bool","value":v}),
            ZValue::I16(v) => json!({"type":"integer","value":v}),
            ZValue::U16(v) => json!({"type":"integer","value":v}),
            ZValue::I32(v) => json!({"type":"integer","value":v}),
            ZValue::U32(v) => json!({"type":"integer","value":v}),
            ZValue::I64(v) => json!({"type":"integer","value":v}),
            ZValue::U64(v) => json!({"type":"integer","value":v}),
            ZValue::F64(v) => json!({"type":"float","value":v}),
            ZValue::Str(v) => json!({"type":"string","value":v.as_str()}),
            ZValue::Signature(v) => json!({"type":"string","value":v.to_string()}),
            ZValue::ObjectPath(v) => json!({"type":"string","value":v.as_str()}),
            ZValue::Value(v) => inner(v, depth + 1),
            ZValue::Array(items) => {
                json!({"type":"array","value":items.iter().map(|v| inner(v, depth + 1)).collect::<Vec<_>>()})
            }
            ZValue::Dict(dict) => {
                let mut map = serde_json::Map::new();
                for (k, v) in dict.iter() {
                    let key = match k {
                        ZValue::Str(s) => s.to_string(),
                        other => format!("{other:?}"),
                    };
                    map.insert(key, inner(v, depth + 1));
                }
                json!({"type":"map","value":map})
            }
            ZValue::Structure(fields) => {
                json!({"type":"array","value":fields.fields().iter().map(|v| inner(v, depth + 1)).collect::<Vec<_>>()})
            }
            ZValue::Fd(_) => json!({"type":"opaque","reason":"file_descriptor"}),
        }
    }
    inner(value, 0)
}

/// Maps a D-Bus failure to a native error. Unimplemented methods and
/// interfaces are a capability gap (`unsupported`, no effect); anything
/// else is an ordinary call failure.
fn dbus_error(e: zbus::Error) -> NativeError {
    let unsupported = matches!(&e, zbus::Error::MethodError(name, _, _)
        if name.ends_with(".NotSupported") || name.ends_with(".UnknownMethod") || name.ends_with(".UnknownInterface"));
    if unsupported {
        NativeError::unsupported(e.to_string())
    } else {
        NativeError::new("atspi_call", e.to_string())
    }
}
fn block<T>(timeout: Duration, future: impl Future<Output = zbus::Result<T>>) -> Result<T> {
    async_io::block_on(async {
        let call = async { future.await.map_err(dbus_error) };
        let deadline = async {
            async_io::Timer::after(timeout).await;
            Err(timeout_error())
        };
        futures_lite::future::or(call, deadline).await
    })
}

impl Discover for AtSpi {
    fn discover(&mut self) -> Result<Value> {
        let status = self.status().ok();
        let applications = self.applications()?;
        Ok(json!({
            "session": self.session,
            "accessibility": status,
            "application_list_source": "atspi_registry",
            "applications": applications,
        }))
    }
}
impl ObserveScope for AtSpi {
    type Scope = i32;
    fn observe_scope(&mut self, pid: i32, budget: ObservationBudget) -> Result<Snapshot> {
        let root = self.application_root(pid)?;
        self.observe_subtree(&root, budget)
    }
}
impl actuate::Observe for AtSpi {
    fn observe(&mut self, request: actuate::ObserveRequest) -> Result<Snapshot> {
        self.observe_scope(request.pid, request.budget())
    }
}
impl SemanticActions for AtSpi {
    fn semantic(&mut self, target: &ElementRef, action: SemanticAction) -> Result<Receipt> {
        let object = self.resolve(target)?;
        let route = |what: &str| Receipt::dispatched(format!("linux.atspi.{what}"));
        match action {
            SemanticAction::Perform { name } => match name.as_str() {
                "component.grab_focus" => {
                    let ok: bool = self
                        .call(&object, COMPONENT, "GrabFocus", &())
                        .map_err(mutation)?;
                    if !ok {
                        return Err(
                            NativeError::new("action_failed", "GrabFocus returned false")
                                .with_effect(Effect::Unknown),
                        );
                    }
                    Ok(route("component.grab_focus"))
                }
                "component.scroll_to" => {
                    let ok: bool = self
                        .call(&object, COMPONENT, "ScrollTo", &(0u32,))
                        .map_err(mutation)?;
                    if !ok {
                        return Err(NativeError::new("action_failed", "ScrollTo returned false")
                            .with_effect(Effect::Unknown));
                    }
                    Ok(route("component.scroll_to"))
                }
                "selection.clear" => {
                    let ok: bool = self
                        .call(&object, SELECTION, "ClearSelection", &())
                        .map_err(mutation)?;
                    if !ok {
                        return Err(NativeError::new(
                            "action_failed",
                            "ClearSelection returned false",
                        )
                        .with_effect(Effect::Unknown));
                    }
                    Ok(route("selection.clear"))
                }
                _ => {
                    let actions: Vec<(String, String, String)> =
                        self.call(&object, ACTION, "GetActions", &())?;
                    let index =
                        actions
                            .iter()
                            .position(|(n, _, _)| *n == name)
                            .ok_or_else(|| {
                                NativeError::new(
                                    "unsupported",
                                    format!("Action {name:?} is not advertised by this element"),
                                )
                            })?;
                    let ok: bool = self
                        .call(&object, ACTION, "DoAction", &(index as i32,))
                        .map_err(mutation)?;
                    if !ok {
                        return Err(NativeError::new("action_failed", "DoAction returned false")
                            .with_effect(Effect::Unknown));
                    }
                    Ok(route("action"))
                }
            },
            SemanticAction::SetString { attribute, value } => match attribute.as_str() {
                "text" => {
                    let ok: bool = self
                        .call(&object, EDITABLE_TEXT, "SetTextContents", &(value,))
                        .map_err(mutation)?;
                    if !ok {
                        return Err(NativeError::new(
                            "action_failed",
                            "SetTextContents returned false",
                        )
                        .with_effect(Effect::Unknown));
                    }
                    Ok(route("editable_text.set_text_contents"))
                }
                _ => Err(NativeError::new(
                    "unsupported",
                    "Only the `text` attribute accepts strings",
                )),
            },
            SemanticAction::SetFloat { attribute, value } if attribute == "value" => {
                if !value.is_finite() {
                    return Err(NativeError::new("invalid_request", "Finite value required"));
                }
                self.set_property(&object, VALUE, "CurrentValue", ZValue::from(value))?;
                Ok(route("value.current_value"))
            }
            SemanticAction::SetInteger { attribute, value } => match attribute.as_str() {
                "value" => {
                    self.set_property(&object, VALUE, "CurrentValue", ZValue::from(value as f64))?;
                    Ok(route("value.current_value"))
                }
                "caret_offset" => {
                    let offset = i32::try_from(value).map_err(|_| {
                        NativeError::new("invalid_request", "Caret offset out of range")
                    })?;
                    let ok: bool = self
                        .call(&object, TEXT, "SetCaretOffset", &(offset,))
                        .map_err(mutation)?;
                    if !ok {
                        return Err(NativeError::new(
                            "action_failed",
                            "SetCaretOffset returned false",
                        )
                        .with_effect(Effect::Unknown));
                    }
                    Ok(route("text.set_caret_offset"))
                }
                _ => Err(NativeError::new(
                    "unsupported",
                    "Only `value` and `caret_offset` accept integers",
                )),
            },
            SemanticAction::SetRange {
                attribute,
                location,
                length,
            } if attribute == "selection" => {
                let (start, end) = (
                    i32::try_from(location).map_err(|_| {
                        NativeError::new("invalid_request", "Selection out of range")
                    })?,
                    i32::try_from(location + length).map_err(|_| {
                        NativeError::new("invalid_request", "Selection out of range")
                    })?,
                );
                let selections: i32 = i32::try_from(
                    self.property(&object, TEXT, "NSelections")
                        .unwrap_or(OwnedValue::from(0i32)),
                )
                .unwrap_or(0);
                let ok: bool = if selections > 0 {
                    self.call(&object, TEXT, "SetSelection", &(0i32, start, end))
                } else {
                    self.call(&object, TEXT, "AddSelection", &(start, end))
                }
                .map_err(mutation)?;
                if !ok {
                    return Err(
                        NativeError::new("action_failed", "Text selection returned false")
                            .with_effect(Effect::Unknown),
                    );
                }
                Ok(route("text.set_selection"))
            }
            SemanticAction::SetBool { .. }
            | SemanticAction::SetFloat { .. }
            | SemanticAction::SetRange { .. }
            | SemanticAction::SetPoint { .. }
            | SemanticAction::SetSize { .. } => Err(NativeError::new(
                "unsupported",
                "AT-SPI exposes text, value, caret_offset and selection setters; other attributes are read-only",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_state_bits_across_words() {
        let names = state_names(&[(1 << 8) | (1 << 25), 1]);
        assert!(names.contains(&"enabled"));
        assert!(names.contains(&"showing"));
        assert!(names.contains(&"indeterminate"));
        assert_eq!(state_names(&[0, 0]), Vec::<&str>::new());
        let visible = state_names(&[1 << 30, 0]);
        assert_eq!(visible, vec!["visible"]);
    }
    #[test]
    fn canonical_roles_follow_the_enum() {
        assert_eq!(role_name(43), Some("push button"));
        assert_eq!(role_name(23), Some("frame"));
        assert_eq!(role_name(61), Some("text"));
        assert_eq!(role_name(95), Some("document web"));
        assert_eq!(role_name(130), Some("switch"));
        assert_eq!(role_name(131), None);
    }
    #[test]
    fn negative_flags_need_meaningful_context() {
        let flags = state_flags(&["sensitive", "showing", "visible", "focusable"], true);
        assert!(flags.contains(&("focused", false)));
        assert!(!flags.iter().any(|(s, _)| *s == "selected"));
        let app = state_flags(&[], false);
        assert!(app.is_empty());
        let hidden = state_flags(&["sensitive"], true);
        assert!(hidden.contains(&("visible", false)));
    }
    #[test]
    fn encodes_dbus_values_with_type_tags() {
        let value = OwnedValue::from(42i32);
        assert_eq!(encode_value(&value), json!({"type":"integer","value":42}));
        let value = OwnedValue::try_from(ZValue::from("hi")).unwrap();
        assert_eq!(encode_value(&value), json!({"type":"string","value":"hi"}));
    }
}

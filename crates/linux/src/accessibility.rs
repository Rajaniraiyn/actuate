//! AT-SPI over its dedicated D-Bus connection. This provider is independent of
//! the window system. References retain the unique bus owner and object path.
use crate::{error, unsupported};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use unimation::{
    Discover, Effect, ElementRef, NativeError, Node, Observe, ObserveRequest, Receipt, Result,
    SemanticAction, SemanticActions, Snapshot,
};
use zbus::{
    blocking::{Connection, Proxy},
    zvariant::OwnedObjectPath,
};
const ACCESSIBLE: &str = "org.a11y.atspi.Accessible";
const ROOT: &str = "/org/a11y/atspi/accessible/root";
type Object = (String, OwnedObjectPath);
static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);
pub struct Accessibility {
    connection: Connection,
    session: String,
    ids: HashMap<Object, u64>,
    objects: HashMap<u64, Object>,
    next_id: u64,
    revision: u64,
}
impl Accessibility {
    pub fn connect() -> Result<Self> {
        let session = zbus::blocking::connection::Builder::session()
            .map_err(bus_error)?
            .method_timeout(Duration::from_secs(3))
            .build()
            .map_err(bus_error)?;
        let bus = Proxy::new(&session, "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Bus")
            .map_err(bus_error)?;
        let address: String = bus.call("GetAddress", &()).map_err(bus_error)?;
        let connection = zbus::blocking::connection::Builder::address(address.as_str())
            .map_err(bus_error)?
            .method_timeout(Duration::from_secs(3))
            .build()
            .map_err(bus_error)?;
        Ok(Self {
            connection,
            session: format!(
                "atspi-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos(),
                NEXT_SESSION.fetch_add(1, Ordering::Relaxed)
            ),
            ids: HashMap::new(),
            objects: HashMap::new(),
            next_id: 1,
            revision: 0,
        })
    }
    fn proxy<'a>(&'a self, object: &'a Object, interface: &'a str) -> Result<Proxy<'a>> {
        Proxy::new(
            &self.connection,
            object.0.as_str(),
            object.1.as_str(),
            interface,
        )
        .map_err(bus_error)
    }
    fn applications(&self) -> Result<Vec<Object>> {
        let registry = (
            "org.a11y.atspi.Registry".to_string(),
            OwnedObjectPath::try_from(ROOT).unwrap(),
        );
        self.proxy(&registry, ACCESSIBLE)?
            .call("GetChildren", &())
            .map_err(bus_error)
    }
    fn process_id(&self, object: &Object) -> Result<u32> {
        let dbus = Proxy::new(
            &self.connection,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
        )
        .map_err(bus_error)?;
        dbus.call("GetConnectionUnixProcessID", &(object.0.as_str(),))
            .map_err(bus_error)
    }
    fn reference(&mut self, object: &Object) -> Result<ElementRef> {
        // Unique names cannot transfer to a replacement app. Never keep a well-known
        // name as an actionable identity, even if a broken toolkit advertises one.
        if !object.0.starts_with(':') {
            return Err(error(
                "unstable_accessible_identity",
                "AT-SPI object did not advertise a unique bus owner",
            ));
        }
        if self.ids.len() >= 100_000 && !self.ids.contains_key(object) {
            return Err(error(
                "reference_limit",
                "Session retained 100000 references; start a new session",
            ));
        }
        let id = *self.ids.entry(object.clone()).or_insert_with(|| {
            let id = self.next_id;
            self.next_id += 1;
            self.objects.insert(id, object.clone());
            id
        });
        Ok(ElementRef {
            session: self.session.clone(),
            id,
        })
    }
    fn target(&self, reference: &ElementRef) -> Result<Object> {
        if reference.session != self.session {
            return Err(error(
                "stale_reference",
                "Reference belongs to another accessibility session",
            ));
        }
        self.objects
            .get(&reference.id)
            .cloned()
            .ok_or_else(|| error("stale_reference", "Unknown accessibility reference"))
    }
    fn read_node(&mut self, object: &Object) -> Result<(Node, Vec<Object>)> {
        let reference = self.reference(object)?;
        let proxy = self.proxy(object, ACCESSIBLE)?;
        let mut attributes = BTreeMap::new();
        let mut issues = Vec::new();
        attributes.insert("atspi_bus".into(), json!(object.0));
        attributes.insert("atspi_path".into(), json!(object.1.as_str()));
        for (property, key) in [
            ("Name", "name"),
            ("Description", "description"),
            ("AccessibleId", "accessible_id"),
        ] {
            collect(
                &mut attributes,
                &mut issues,
                key,
                proxy.get_property::<String>(property),
            );
        }
        collect(
            &mut attributes,
            &mut issues,
            "role",
            proxy.call::<_, _, String>("GetRoleName", &()),
        );
        collect(
            &mut attributes,
            &mut issues,
            "role_id",
            proxy.call::<_, _, u32>("GetRole", &()),
        );
        collect(
            &mut attributes,
            &mut issues,
            "native_attributes",
            proxy.call::<_, _, HashMap<String, String>>("GetAttributes", &()),
        );
        collect(
            &mut attributes,
            &mut issues,
            "states",
            proxy.call::<_, _, Vec<u32>>("GetState", &()),
        );
        project_states(&mut attributes);
        collect(
            &mut attributes,
            &mut issues,
            "relations",
            proxy.call::<_, _, Vec<(u32, Vec<Object>)>>("GetRelationSet", &()),
        );
        let interfaces: Vec<String> = match proxy.call("GetInterfaces", &()) {
            Ok(v) => v,
            Err(e) => {
                issues.push(json!({"field":"interfaces","error":e.to_string()}));
                Vec::new()
            }
        };
        let children: Vec<Object> = match proxy.call("GetChildren", &()) {
            Ok(v) => v,
            Err(e) => {
                issues.push(json!({"field":"children","error":e.to_string()}));
                Vec::new()
            }
        };
        let mut actions = Vec::new();
        if interfaces.iter().any(|i| i == "org.a11y.atspi.Action") {
            let action = self.proxy(object, "org.a11y.atspi.Action")?;
            match action.call::<_, _, Vec<(String, String, String)>>("GetActions", &()) {
                Ok(v) => {
                    actions = v.iter().map(|a| a.0.clone()).collect();
                    attributes.insert("native_actions".into(), json!(v));
                }
                Err(e) => issues.push(json!({"field":"actions","error":e.to_string()})),
            }
        }
        if interfaces.iter().any(|i| i == "org.a11y.atspi.Component") {
            let component = self.proxy(object, "org.a11y.atspi.Component")?;
            match component.call::<_, _, (i32, i32, i32, i32)>("GetExtents", &(0u32,)) {
                Ok((x, y, width, height)) => {
                    attributes.insert("bounds".into(),json!({"x":x,"y":y,"width":width,"height":height,"coordinate_space":"atspi_screen"}));
                }
                Err(e) => issues.push(json!({"field":"bounds","error":e.to_string()})),
            }
        }
        attributes.insert("interfaces".into(), json!(interfaces));
        // State numbers are preserved; no inference that Visible/Showing proves
        // unobscured hit testing or agrees with X11 pixels on mixed-scale desktops.
        Ok((
            Node {
                reference,
                attributes,
                actions,
                parameterized_attributes: Vec::new(),
                children: Vec::new(),
                issues,
            },
            children,
        ))
    }
}
// GNOME atspi-constants.h AtspiStateType values. Raw words stay in each node.
// These aliases are projection evidence, not an input-coordinate conversion.
const STATE_DEFUNCT: u32 = 6;
const STATE_STALE: u32 = 27;
fn project_states(attributes: &mut BTreeMap<String, Value>) {
    if let Some(states) = attributes.get("states").and_then(Value::as_array).cloned() {
        for (name, bit) in [
            ("enabled", 8usize),
            ("focused", 12),
            ("selected", 23),
            ("expanded", 10),
            ("visible", 30),
            ("showing", 25),
            ("defunct", 6),
        ] {
            if let Some(word) = states.get(bit / 32).and_then(Value::as_u64) {
                attributes.insert(name.into(), json!(word & (1u64 << (bit % 32)) != 0));
            }
        }
    }
}
fn collect<T: serde::Serialize>(
    attrs: &mut BTreeMap<String, Value>,
    issues: &mut Vec<Value>,
    key: &str,
    result: zbus::Result<T>,
) {
    match result {
        Ok(v) => {
            attrs.insert(key.into(), json!(v));
        }
        Err(e) => issues.push(json!({"field":key,"error":e.to_string()})),
    }
}
impl Discover for Accessibility {
    fn discover(&mut self) -> Result<Value> {
        let mut apps = Vec::new();
        let mut issues = Vec::new();
        for object in self.applications()? {
            let app = (|| -> Result<Value> {
                let pid = self.process_id(&object)?;
                let proxy = self.proxy(&object, ACCESSIBLE)?;
                let name: String = proxy.get_property("Name").map_err(bus_error)?;
                Ok(
                    json!({"pid":pid,"name":name,"atspi_bus":object.0,"atspi_path":object.1.as_str(),"active":null,"application":true}),
                )
            })();
            match app {
                Ok(app) => apps.push(app),
                Err(e) => issues.push(json!({"bus":object.0,"error":e})),
            }
        }
        Ok(json!({"applications":apps,"issues":issues,"active_pid":null,"provider":"atspi"}))
    }
}
impl Observe for Accessibility {
    fn observe(&mut self, request: ObserveRequest) -> Result<Snapshot> {
        if request.pid <= 0
            || request.max_nodes == 0
            || request.max_nodes > 100_000
            || request.max_depth > 256
        {
            return Err(error(
                "invalid_observation",
                "Positive PID, 1..=100000 nodes and depth <=256 are required",
            ));
        }
        let root = self
            .applications()?
            .into_iter()
            .find(|o| self.process_id(o).ok() == Some(request.pid as u32))
            .ok_or_else(|| {
                error(
                    "application_not_found",
                    "PID has no registered AT-SPI application",
                )
            })?;
        let root_ref = self.reference(&root)?;
        let mut queue = VecDeque::from([(root, 0usize)]);
        let mut seen = HashSet::new();
        let mut nodes = Vec::new();
        let mut traversal_complete = true;
        let mut issues = vec![
            json!({"code":"selected_attribute_coverage", "message":"Baseline reads selected AT-SPI interfaces and properties"}),
        ];
        let deadline = Instant::now() + Duration::from_secs(30);
        while let Some((object, depth)) = queue.pop_front() {
            if Instant::now() >= deadline {
                traversal_complete = false;
                issues.push(json!({"code":"observation_deadline"}));
                break;
            }
            if !seen.insert(object.clone()) {
                traversal_complete = false;
                issues.push(
                    json!({"code":"repeated_object","bus":object.0,"path":object.1.as_str()}),
                );
                continue;
            }
            if nodes.len() >= request.max_nodes {
                traversal_complete = false;
                issues.push(json!({"code":"node_limit"}));
                break;
            }
            let (mut node, children) = match self.read_node(&object) {
                Ok(v) => v,
                Err(e) => {
                    traversal_complete = false;
                    issues.push(json!({"bus":object.0,"path":object.1.as_str(),"error":e}));
                    continue;
                }
            };
            if node.issues.iter().any(|i| i["field"] == "children") {
                traversal_complete = false;
            }
            for child in children {
                match self.reference(&child) {
                    Ok(r) => node.children.push(r),
                    Err(e) => {
                        traversal_complete = false;
                        node.issues.push(json!({"error":e}));
                        continue;
                    }
                }
                if depth < request.max_depth && queue.len() < request.max_nodes {
                    queue.push_back((child, depth + 1));
                } else {
                    traversal_complete = false;
                }
            }
            nodes.push(node);
        }
        self.revision += 1;
        Ok(Snapshot {
            root: root_ref,
            nodes,
            complete: false,
            traversal_complete,
            revision: self.revision,
            issues,
        })
    }
}
impl SemanticActions for Accessibility {
    fn semantic(&mut self, target: &ElementRef, action: SemanticAction) -> Result<Receipt> {
        let object = self.target(target)?;
        let states: Vec<u32> = self
            .proxy(&object, ACCESSIBLE)?
            .call("GetState", &())
            .map_err(bus_error)?;
        if states
            .first()
            .is_none_or(|word| word & (1 << STATE_DEFUNCT) != 0 || word & (1 << STATE_STALE) != 0)
        {
            return Err(error(
                "stale_reference",
                "Accessible is defunct, stale, or has no state evidence; observe again",
            ));
        }
        let accepted = match action {
            SemanticAction::Perform { name } => {
                let proxy = self.proxy(&object, "org.a11y.atspi.Action")?;
                let actions: Vec<(String, String, String)> =
                    proxy.call("GetActions", &()).map_err(bus_error)?;
                let index = actions.iter().position(|a| a.0 == name).ok_or_else(|| {
                    unsupported(format!("Accessible does not advertise action {name}"))
                })?;
                proxy
                    .call::<_, _, bool>("DoAction", &(index as i32,))
                    .map_err(action_error)?
            }
            SemanticAction::SetString { attribute, value } if attribute == "text" => self
                .proxy(&object, "org.a11y.atspi.EditableText")?
                .call::<_, _, bool>("SetTextContents", &(value,))
                .map_err(action_error)?,
            _ => {
                return Err(unsupported(
                    "AT-SPI baseline supports advertised actions and SetString attribute=text",
                ));
            }
        };
        if !accepted {
            return Err(NativeError {
                code: "action_rejected".into(),
                message: "AT-SPI provider returned false; observe before retrying".into(),
                effect: Effect::Unknown,
            });
        }
        Ok(Receipt {
            effect: Effect::Dispatched,
            route: "atspi_semantic".into(),
        })
    }
}
fn bus_error(e: impl ToString) -> NativeError {
    error("atspi_bus", e)
}
fn action_error(e: impl ToString) -> NativeError {
    NativeError {
        code: "atspi_action".into(),
        message: e.to_string(),
        effect: Effect::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn state_aliases_retain_native_words_and_unknowns() {
        let words = json!([(1u32 << 8) | (1u32 << 25) | (1u32 << 30), 0]);
        let mut attributes = BTreeMap::from([("states".into(), words.clone())]);
        project_states(&mut attributes);
        assert_eq!(attributes["states"], words);
        assert_eq!(attributes["enabled"], true);
        assert_eq!(attributes["showing"], true);
        assert_eq!(attributes["visible"], true);
        assert_eq!(attributes["focused"], false);
        let mut unknown = BTreeMap::new();
        project_states(&mut unknown);
        assert!(!unknown.contains_key("visible"));
    }
}

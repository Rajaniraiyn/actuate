//! Native attribute vocabularies for presentation and queries.
//!
//! Observations keep native attribute names. A schema tells the portable
//! renderer and query engine which native names carry role, name, value,
//! state and geometry for one provider family, without inventing canonical
//! aliases or rewriting stored nodes. Providers can supply their own schema.
use crate::{Node, Snapshot, values};

/// How a node's bounds are encoded in its attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundsEncoding {
    /// A `{"type":"point"}` attribute and a `{"type":"size"}` attribute.
    PointAndSize {
        position: &'static str,
        size: &'static str,
    },
    /// One `{"type":"rect"}` attribute.
    Rect(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeSchema {
    /// Stable identifier reported in projections, such as `macos_ax` or `atspi`.
    pub id: &'static str,
    pub role: &'static str,
    /// Checked in order; the first non-empty readable value names the node.
    pub names: &'static [&'static str],
    pub value: &'static str,
    pub enabled: &'static str,
    pub focused: &'static str,
    pub selected: &'static str,
    pub expanded: &'static str,
    /// Attribute and the boolean that establishes hidden state.
    pub hidden_when: &'static [(&'static str, bool)],
    /// Attribute and the boolean that establishes visible state.
    pub visible_when: (&'static str, bool),
    pub bounds: BoundsEncoding,
    /// Roles that are interaction candidates even without a listed action.
    pub interactive_roles: &'static [&'static str],
    /// Roles retained in text output even when unnamed, for structure.
    pub context_roles: &'static [&'static str],
    pub viewport_roles: &'static [&'static str],
    pub scroll_roles: &'static [&'static str],
    /// Generic actions advertised by wrappers that do not make them interactive.
    pub generic_actions: &'static [&'static str],
    /// Advertised action names that activate a control, in preference order.
    /// Matched case-insensitively; the advertised spelling is performed.
    pub activate_actions: &'static [&'static str],
    /// Roles of toplevel containers whose geometry a compositor can locate.
    pub toplevel_roles: &'static [&'static str],
    /// Error codes that mean an optional attribute is simply absent.
    pub absent_error_codes: &'static [&'static str],
    /// Attribute that carries the native child list, when children are also
    /// exposed as an attribute value whose read errors make reparenting uncertain.
    pub children: Option<&'static str>,
    /// A key whose presence on the root node identifies this schema.
    pub marker: &'static str,
}

pub static MACOS_AX: NativeSchema = NativeSchema {
    id: "macos_ax",
    role: "AXRole",
    names: &[
        "AXTitle",
        "AXDescription",
        "AXLabel",
        "AXAttributedTitle",
        "AXAttributedDescription",
        "AXAttributedLabel",
    ],
    value: "AXValue",
    enabled: "AXEnabled",
    focused: "AXFocused",
    selected: "AXSelected",
    expanded: "AXExpanded",
    hidden_when: &[
        ("AXHidden", true),
        ("AXVisible", false),
        ("AXMinimized", true),
    ],
    visible_when: ("AXVisible", true),
    bounds: BoundsEncoding::PointAndSize {
        position: "AXPosition",
        size: "AXSize",
    },
    interactive_roles: &[
        "AXTextField",
        "AXTextArea",
        "AXComboBox",
        "AXSearchField",
        "AXCheckBox",
        "AXRadioButton",
        "AXSlider",
        "AXPopUpButton",
        "AXMenuItem",
        "AXButton",
        "AXLink",
        "AXTab",
    ],
    context_roles: &["AXWindow", "AXScrollArea", "AXWebArea"],
    viewport_roles: &["AXScrollArea", "AXWindow"],
    scroll_roles: &["AXScrollArea"],
    generic_actions: &["AXShowMenu", "AXScrollToVisible"],
    activate_actions: &["AXPress"],
    toplevel_roles: &["AXWindow", "AXSheet"],
    absent_error_codes: &["ax_-25212", "ax_-25205"],
    children: Some("AXChildren"),
    marker: "AXRole",
};

/// Linux AT-SPI2 nodes as encoded by the `linux` provider.
pub static ATSPI: NativeSchema = NativeSchema {
    id: "atspi",
    role: "role",
    names: &["name", "description"],
    value: "value",
    enabled: "state.sensitive",
    focused: "state.focused",
    selected: "state.selected",
    expanded: "state.expanded",
    hidden_when: &[("state.visible", false), ("state.showing", false)],
    visible_when: ("state.showing", true),
    bounds: BoundsEncoding::Rect("bounds"),
    interactive_roles: &[
        "push button",
        "toggle button",
        "check box",
        "radio button",
        "text",
        "entry",
        "password text",
        "combo box",
        "slider",
        "spin button",
        "link",
        "menu item",
        "check menu item",
        "radio menu item",
        "page tab",
        "list item",
        "tree item",
        "table cell",
    ],
    context_roles: &["frame", "window", "dialog", "scroll pane", "document web"],
    viewport_roles: &["scroll pane", "frame", "window", "dialog"],
    scroll_roles: &["scroll pane"],
    generic_actions: &[],
    activate_actions: &[
        "click",
        "press",
        "activate",
        "toggle",
        "jump",
        "default.activate",
    ],
    toplevel_roles: &["frame", "window", "dialog", "alert", "file chooser"],
    absent_error_codes: &[],
    children: None,
    marker: "interfaces",
};

/// Provider-neutral records, such as the Windows UI Automation provider:
/// `role`, `name`, `description`, `value`, plain state booleans, `offscreen`
/// and a `bounds` rectangle. Only advertised actions make a node interactive.
pub static PORTABLE: NativeSchema = NativeSchema {
    id: "portable",
    role: "role",
    names: &["name", "description"],
    value: "value",
    enabled: "enabled",
    focused: "focused",
    selected: "selected",
    expanded: "expanded",
    hidden_when: &[
        ("hidden", true),
        ("offscreen", true),
        ("visible", false),
        ("showing", false),
    ],
    visible_when: ("visible", true),
    bounds: BoundsEncoding::Rect("bounds"),
    interactive_roles: &[],
    context_roles: &["application", "window", "frame", "dialog", "scroll pane"],
    viewport_roles: &[],
    scroll_roles: &[],
    generic_actions: &[],
    activate_actions: &["invoke", "click", "press", "toggle", "select"],
    toplevel_roles: &["window", "frame", "dialog"],
    absent_error_codes: &[],
    children: None,
    marker: "role",
};

/// Checked in order; the portable marker is the most general and comes last.
pub static SCHEMAS: [&NativeSchema; 3] = [&MACOS_AX, &ATSPI, &PORTABLE];

impl NativeSchema {
    /// Selects the schema whose marker attribute the root node carries. Nodes
    /// without any known marker default to the macOS AX vocabulary, which keeps
    /// previously saved observations renderable.
    pub fn detect(snapshot: &Snapshot) -> &'static NativeSchema {
        let root = snapshot
            .nodes
            .iter()
            .find(|node| node.reference == snapshot.root)
            .or(snapshot.nodes.first());
        root.and_then(Self::detect_node).unwrap_or(&MACOS_AX)
    }
    pub fn detect_node(node: &Node) -> Option<&'static NativeSchema> {
        SCHEMAS
            .iter()
            .copied()
            .find(|schema| node.attributes.contains_key(schema.marker))
    }
    /// The advertised action that activates a node, if any.
    pub fn activate_action<'a>(&self, advertised: &'a [String]) -> Option<&'a str> {
        self.activate_actions.iter().find_map(|candidate| {
            advertised
                .iter()
                .find(|a| a.eq_ignore_ascii_case(candidate))
                .map(String::as_str)
        })
    }
    pub fn is_toplevel_role(&self, role: &str) -> bool {
        self.toplevel_roles.contains(&role)
    }
    pub fn role<'a>(&self, node: &'a Node) -> Option<&'a str> {
        node.attributes.get(self.role).and_then(values::string)
    }
    /// The first readable, non-empty native name and the attribute it came from.
    pub fn name<'a>(&self, node: &'a Node) -> Option<(&'static str, &'a str)> {
        self.names.iter().find_map(|key| {
            node.attributes
                .get(*key)
                .and_then(values::string)
                .filter(|s| !s.is_empty())
                .map(|s| (*key, s))
        })
    }
    pub fn flag(&self, node: &Node, attribute: &str) -> Option<bool> {
        node.attributes.get(attribute).and_then(values::boolean)
    }
    pub fn is_absent_error(&self, code: &str) -> bool {
        self.absent_error_codes.contains(&code)
    }
    pub fn is_interactive_role(&self, role: &str) -> bool {
        self.interactive_roles.contains(&role)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ElementRef;
    use serde_json::json;
    fn node(attributes: &[(&str, serde_json::Value)]) -> Node {
        Node {
            reference: ElementRef {
                session: "s".into(),
                id: 1,
            },
            attributes: attributes
                .iter()
                .map(|(k, v)| ((*k).to_owned(), v.clone()))
                .collect(),
            actions: vec![],
            parameterized_attributes: vec![],
            children: vec![],
            issues: vec![],
        }
    }
    #[test]
    fn detects_schema_from_root_marker_and_defaults_to_ax() {
        let mut snapshot = Snapshot {
            root: ElementRef {
                session: "s".into(),
                id: 1,
            },
            nodes: vec![node(&[("interfaces", json!([])), ("role", json!("frame"))])],
            complete: true,
            traversal_complete: true,
            revision: 1,
            issues: vec![],
        };
        assert_eq!(NativeSchema::detect(&snapshot).id, "atspi");
        snapshot.nodes[0].attributes.remove("interfaces");
        assert_eq!(NativeSchema::detect(&snapshot).id, "portable");
        snapshot.nodes[0].attributes.clear();
        assert_eq!(NativeSchema::detect(&snapshot).id, "macos_ax");
        assert_eq!(
            ATSPI.activate_action(&["Click".to_owned(), "press".to_owned()]),
            Some("Click")
        );
        assert!(ATSPI.is_toplevel_role("frame"));
    }
    #[test]
    fn name_lookup_skips_unreadable_and_empty_values() {
        let node = node(&[
            ("name", json!({"type":"read_error","error":{}})),
            ("description", json!({"type":"string","value":"Close"})),
        ]);
        assert_eq!(ATSPI.name(&node), Some(("description", "Close")));
        assert_eq!(MACOS_AX.name(&node), None);
    }
}

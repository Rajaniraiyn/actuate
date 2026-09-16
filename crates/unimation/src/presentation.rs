//! Lossy agent views of retained observations. Filtering never changes native references.
use crate::{
    ElementRef, NativeError, Node, Result, Snapshot,
    diff::{ChangeEvidence, SnapshotDiff},
    schema::{self, BoundsEncoding, NativeSchema},
    values,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fmt::Write,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PresentationOptions {
    pub root: Option<ElementRef>,
    pub actionable_only: bool,
    pub hide_known_hidden: bool,
    pub max_nodes: usize,
    pub max_text_chars: usize,
}
impl Default for PresentationOptions {
    fn default() -> Self {
        Self {
            root: None,
            actionable_only: false,
            hide_known_hidden: false,
            max_nodes: 200,
            max_text_chars: 160,
        }
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct Preview {
    pub text: String,
    pub omitted_chars: usize,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Visible,
    Hidden,
    Unknown,
}
#[derive(Debug, Clone, Serialize)]
pub struct VisibilityEvidence {
    pub attribute: String,
    pub value: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewportRelation {
    Unknown,
    Above,
    Below,
    Left,
    Right,
    Inside,
    PartiallyInside,
}
#[derive(Debug, Clone, Copy)]
pub struct ViewportBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl ViewportBounds {
    fn valid(self) -> bool {
        [
            self.x,
            self.y,
            self.width,
            self.height,
            self.x + self.width,
            self.y + self.height,
        ]
        .iter()
        .all(|n| n.is_finite())
            && self.width > 0.0
            && self.height > 0.0
    }
    fn relative_to(self, viewport: Self) -> ViewportRelation {
        if !self.valid() || !viewport.valid() {
            return ViewportRelation::Unknown;
        }
        if self.y + self.height <= viewport.y {
            ViewportRelation::Above
        } else if self.y >= viewport.y + viewport.height {
            ViewportRelation::Below
        } else if self.x + self.width <= viewport.x {
            ViewportRelation::Left
        } else if self.x >= viewport.x + viewport.width {
            ViewportRelation::Right
        } else if self.x >= viewport.x
            && self.y >= viewport.y
            && self.x + self.width <= viewport.x + viewport.width
            && self.y + self.height <= viewport.y + viewport.height
        {
            ViewportRelation::Inside
        } else {
            ViewportRelation::PartiallyInside
        }
    }
}
/// Providers interpret their native attributes. Rendering only traverses references.
/// Bounds must use one common, top-left coordinate space within the snapshot.
pub trait PresentationAdapter {
    fn project(&self, node: &Node, depth: usize, max_text_chars: usize) -> CompactNode;
    /// Projections should expose comparable semantic values. Override for opaque values.
    fn value_is_comparable(&self, _node: &Node) -> bool {
        true
    }
    /// Native uncertainty beyond the projected error and truncation counts.
    fn has_uncertain_values(&self, _node: &Node) -> bool {
        false
    }
    fn bounds(&self, _node: &Node) -> Option<ViewportBounds> {
        None
    }
    fn is_viewport(&self, _node: &Node) -> bool {
        false
    }
    fn is_scroll_container(&self, _node: &Node) -> bool {
        false
    }
}
/// Data-driven adapter for any provider whose vocabulary is a `NativeSchema`.
#[derive(Debug, Clone, Copy)]
pub struct SchemaAdapter(pub &'static NativeSchema);
impl Default for SchemaAdapter {
    fn default() -> Self {
        Self(&schema::MACOS_AX)
    }
}
impl PresentationAdapter for SchemaAdapter {
    fn project(&self, node: &Node, depth: usize, max_text_chars: usize) -> CompactNode {
        compact(self.0, node, depth, max_text_chars)
    }
    fn value_is_comparable(&self, node: &Node) -> bool {
        node.attributes
            .get(self.0.value)
            .and_then(values::scalar)
            .is_some()
    }
    fn has_uncertain_values(&self, node: &Node) -> bool {
        node.attributes.contains_key(self.0.value) && !self.value_is_comparable(node)
    }
    fn bounds(&self, node: &Node) -> Option<ViewportBounds> {
        let b = match self.0.bounds {
            BoundsEncoding::PointAndSize { position, size } => {
                let p = node.attributes.get(position)?;
                let s = node.attributes.get(size)?;
                if p.get("type")?.as_str()? != "point" || s.get("type")?.as_str()? != "size" {
                    return None;
                }
                ViewportBounds {
                    x: p.get("x")?.as_f64()?,
                    y: p.get("y")?.as_f64()?,
                    width: s.get("width")?.as_f64()?,
                    height: s.get("height")?.as_f64()?,
                }
            }
            BoundsEncoding::Rect(name) => {
                let r = node.attributes.get(name)?;
                if r.get("type")?.as_str()? != "rect" {
                    return None;
                }
                ViewportBounds {
                    x: r.get("x")?.as_f64()?,
                    y: r.get("y")?.as_f64()?,
                    width: r.get("width")?.as_f64()?,
                    height: r.get("height")?.as_f64()?,
                }
            }
        };
        b.valid().then_some(b)
    }
    fn is_viewport(&self, node: &Node) -> bool {
        self.0
            .role(node)
            .is_some_and(|role| self.0.viewport_roles.contains(&role))
    }
    fn is_scroll_container(&self, node: &Node) -> bool {
        self.0
            .role(node)
            .is_some_and(|role| self.0.scroll_roles.contains(&role))
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct InheritedHiddenEvidence {
    pub ancestor: ElementRef,
    pub evidence: Vec<VisibilityEvidence>,
}
#[derive(Debug, Clone, Serialize)]
pub struct CompactNode {
    pub reference: ElementRef,
    pub depth: usize,
    /// Nearest ancestor first, retained only for trustworthy text indentation.
    #[serde(skip_serializing)]
    pub ancestors: Vec<ElementRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<Preview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<Preview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_attribute: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Preview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focused: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expanded: Option<bool>,
    /// Geometry relative to an observed ancestor, not visibility or hit-test evidence.
    pub viewport_relation: ViewportRelation,
    pub viewport: Option<ElementRef>,
    pub scroll_container: Option<ElementRef>,
    pub geometry_reason: String,
    pub visibility: Visibility,
    pub visibility_evidence: Vec<VisibilityEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inherited_hidden: Option<InheritedHiddenEvidence>,
    /// A listed action or editable native role, not a promise of clickability.
    pub interactive_candidate: bool,
    /// Adapter requests unnamed structural context in text output.
    pub retain_context: bool,
    pub actions: Vec<String>,
    pub error_count: usize,
    pub unavailable_count: usize,
}
#[derive(Debug, Clone, Serialize)]
pub struct CompactSnapshot {
    pub root: ElementRef,
    pub revision: u64,
    pub native_complete: bool,
    pub native_traversal_complete: bool,
    pub native_issue_count: usize,
    pub scope_traversal_complete: bool,
    pub observed_in_scope: usize,
    pub filtered_nodes: usize,
    pub truncated_nodes: usize,
    pub unresolved_references: usize,
    pub repeated_references: usize,
    pub rows: Vec<CompactNode>,
}
fn invalid(message: &str) -> NativeError {
    NativeError::new("invalid_presentation", message)
}
fn preview(text: &str, limit: usize) -> Preview {
    let count = text.chars().count();
    Preview {
        text: text.chars().take(limit).collect(),
        omitted_chars: count.saturating_sub(limit),
    }
}
/// Counts error records that only mean an optional attribute is absent.
fn unavailable(schema: &NativeSchema, value: &Value) -> usize {
    match value {
        Value::Object(_) if values::is_error(value) => usize::from(
            value
                .pointer("/error/code")
                .and_then(Value::as_str)
                .is_some_and(|code| schema.is_absent_error(code)),
        ),
        Value::Object(o) => o.values().map(|v| unavailable(schema, v)).sum(),
        Value::Array(a) => a.iter().map(|v| unavailable(schema, v)).sum(),
        _ => 0,
    }
}
/// Counts error records other than expected absences.
fn errors(schema: &NativeSchema, value: &Value) -> usize {
    match value {
        Value::Object(_) if values::is_error(value) => {
            1 - usize::from(unavailable(schema, value) > 0)
        }
        Value::Object(object) => object.values().map(|v| errors(schema, v)).sum(),
        Value::Array(items) => items.iter().map(|v| errors(schema, v)).sum(),
        _ => 0,
    }
}
fn compact(schema: &NativeSchema, node: &Node, depth: usize, limit: usize) -> CompactNode {
    let role = schema.role(node);
    let name = schema.name(node);
    let hidden_keys = schema.hidden_when.iter().map(|(key, _)| *key);
    let visible_key = std::iter::once(schema.visible_when.0)
        .filter(|key| !schema.hidden_when.iter().any(|(k, _)| k == key));
    let visibility_evidence: Vec<_> = hidden_keys
        .chain(visible_key)
        .filter_map(|key| {
            schema.flag(node, key).map(|value| VisibilityEvidence {
                attribute: key.into(),
                value,
            })
        })
        .collect();
    let hidden = visibility_evidence.iter().any(|e| {
        schema
            .hidden_when
            .iter()
            .any(|(key, blocked)| *key == e.attribute && *blocked == e.value)
    });
    // A false hidden flag does not establish on-screen visibility.
    let visibility = if hidden {
        Visibility::Hidden
    } else if schema.flag(node, schema.visible_when.0) == Some(schema.visible_when.1) {
        Visibility::Visible
    } else {
        Visibility::Unknown
    };
    CompactNode {
        reference: node.reference.clone(),
        depth,
        ancestors: vec![],
        role: role.map(|s| preview(s, limit)),
        name: name.map(|(_, s)| preview(s, limit)),
        name_attribute: name.map(|(key, _)| key.into()),
        value: node.attributes.get(schema.value).and_then(|v| {
            // Optional values are commonly absent on controls. Retain their
            // unavailable count without presenting a native error as a value.
            if values::is_error(v) && unavailable(schema, v) > 0 {
                return None;
            }
            Some(preview(
                &values::scalar(v).unwrap_or_else(|| "<unknown; inspect>".into()),
                limit,
            ))
        }),
        enabled: schema.flag(node, schema.enabled),
        focused: schema.flag(node, schema.focused),
        selected: schema.flag(node, schema.selected),
        expanded: schema.flag(node, schema.expanded),
        viewport_relation: ViewportRelation::Unknown,
        viewport: None,
        scroll_container: None,
        geometry_reason: "no_observed_viewport".into(),
        visibility,
        visibility_evidence,
        inherited_hidden: None,
        interactive_candidate: node
            .actions
            .iter()
            .any(|a| !schema.generic_actions.contains(&a.as_str()))
            || role.is_some_and(|role| schema.is_interactive_role(role)),
        retain_context: role.is_some_and(|role| schema.context_roles.contains(&role)),
        actions: node.actions.clone(),
        error_count: node.issues.len()
            + node
                .attributes
                .values()
                .map(|v| errors(schema, v))
                .sum::<usize>(),
        unavailable_count: node
            .attributes
            .values()
            .map(|v| unavailable(schema, v))
            .sum(),
    }
}
/// Traverses native children in observed order. Cycles and unresolved children
/// are reported. Observed hidden ancestors propagate through unambiguous
/// ancestry with explicit evidence. The schema is detected from the snapshot.
pub fn render_snapshot(
    snapshot: &Snapshot,
    options: &PresentationOptions,
) -> Result<CompactSnapshot> {
    render_snapshot_with_adapter(
        snapshot,
        options,
        &SchemaAdapter(NativeSchema::detect(snapshot)),
    )
}
pub fn render_snapshot_with_adapter(
    snapshot: &Snapshot,
    options: &PresentationOptions,
    adapter: &(impl PresentationAdapter + ?Sized),
) -> Result<CompactSnapshot> {
    if options.max_nodes > 100_000 || options.max_text_chars > 1_000_000 {
        return Err(invalid(
            "Presentation limits exceed max_nodes=100000 or max_text_chars=1000000",
        ));
    }
    let root = options.root.as_ref().unwrap_or(&snapshot.root);
    if root.session != snapshot.root.session {
        return Err(invalid("Scope reference belongs to another session"));
    }
    let mut index = HashMap::new();
    for node in &snapshot.nodes {
        if node.reference.session != snapshot.root.session
            || node
                .children
                .iter()
                .any(|r| r.session != snapshot.root.session)
        {
            return Err(invalid("Snapshot contains foreign-session references"));
        }
        if index.insert(&node.reference, node).is_some() {
            return Err(invalid("Snapshot contains duplicate references"));
        }
    }
    if !index.contains_key(root) {
        return Err(invalid("Scope reference was not observed in this snapshot"));
    }
    let mut result = CompactSnapshot {
        root: root.clone(),
        revision: snapshot.revision,
        native_complete: snapshot.complete,
        native_traversal_complete: snapshot.traversal_complete,
        native_issue_count: snapshot.issues.len(),
        scope_traversal_complete: snapshot.traversal_complete,
        observed_in_scope: 0,
        filtered_nodes: 0,
        truncated_nodes: 0,
        unresolved_references: 0,
        repeated_references: 0,
        rows: vec![],
    };
    // Multiple observed parents make ancestor geometry ambiguous. Never choose one silently.
    let mut parents: HashMap<&ElementRef, Vec<&ElementRef>> = HashMap::new();
    for node in &snapshot.nodes {
        for child in &node.children {
            parents.entry(child).or_default().push(&node.reference);
        }
    }
    let mut projections: HashMap<&ElementRef, CompactNode> = HashMap::new();
    let mut seen = HashSet::new();
    let mut pending = vec![(root, 0)];
    while let Some((reference, depth)) = pending.pop() {
        if !seen.insert(reference) {
            result.repeated_references += 1;
            continue;
        }
        let Some(node) = index.get(reference) else {
            result.unresolved_references += 1;
            continue;
        };
        result.observed_in_scope += 1;
        pending.extend(node.children.iter().rev().map(|child| (child, depth + 1)));
        let mut row = projections
            .entry(reference)
            .or_insert_with(|| adapter.project(node, depth, options.max_text_chars))
            .clone();
        row.inherited_hidden = None;
        // The renderer owns identity even when a provider adapter supplies presentation fields.
        row.reference = node.reference.clone();
        row.depth = depth;
        row.ancestors.clear();
        let mut ancestor = &node.reference;
        let mut ancestors = HashSet::from([ancestor]);
        let mut viewport_bounds = None;
        let mut ambiguous = false;
        while let Some(candidates) = parents.get(ancestor) {
            if candidates.len() != 1 || !ancestors.insert(candidates[0]) {
                ambiguous = true;
                break;
            }
            ancestor = candidates[0];
            row.ancestors.push((*ancestor).clone());
            let Some(parent) = index.get(ancestor) else {
                ambiguous = true;
                break;
            };
            let parent_projection = projections
                .entry(ancestor)
                .or_insert_with(|| adapter.project(parent, 0, options.max_text_chars));
            if row.inherited_hidden.is_none() && parent_projection.visibility == Visibility::Hidden
            {
                row.inherited_hidden = Some(InheritedHiddenEvidence {
                    ancestor: (*ancestor).clone(),
                    evidence: parent_projection.visibility_evidence.clone(),
                });
            }
            if row.scroll_container.is_none() && adapter.is_scroll_container(parent) {
                row.scroll_container = Some((*ancestor).clone());
            }
            if row.viewport.is_none() && adapter.is_viewport(parent) {
                row.viewport = Some((*ancestor).clone());
                viewport_bounds = adapter.bounds(parent);
            }
        }
        if ambiguous {
            row.inherited_hidden = None;
            row.viewport_relation = ViewportRelation::Unknown;
            row.geometry_reason = "ambiguous_or_cyclic_ancestry".into();
            row.ancestors.clear();
            row.viewport = None;
            row.scroll_container = None;
        } else if let (Some(bounds), Some(viewport)) = (adapter.bounds(node), viewport_bounds) {
            row.viewport_relation = bounds.relative_to(viewport);
            row.geometry_reason = "observed_ancestor_bounds_not_visibility".into();
        } else if row.viewport.is_some() {
            row.geometry_reason = "missing_or_invalid_bounds".into();
        }

        if row.inherited_hidden.is_some() {
            row.visibility = Visibility::Hidden;
        }
        if (options.actionable_only && !row.interactive_candidate)
            || (options.hide_known_hidden && row.visibility == Visibility::Hidden)
        {
            result.filtered_nodes += 1;
        } else if result.rows.len() == options.max_nodes {
            result.truncated_nodes += 1;
        } else {
            result.rows.push(row);
        }
    }
    result.scope_traversal_complete &= result.unresolved_references == 0;
    Ok(result)
}
fn quoted(preview: &Preview) -> String {
    let mut result = serde_json::to_string(&preview.text).expect("serializing a string");
    if preview.omitted_chars > 0 {
        write!(result, " [truncated {} chars]", preview.omitted_chars).unwrap();
    }
    result
}
pub fn render_snapshot_text(snapshot: &CompactSnapshot) -> String {
    let mut out = format!(
        "snapshot session={} revision={} root=@e{} native_complete={} native_traversal_complete={} issues={}\n",
        serde_json::to_string(&snapshot.root.session).unwrap(),
        snapshot.revision,
        snapshot.root.id,
        snapshot.native_complete,
        snapshot.native_traversal_complete,
        snapshot.native_issue_count
    );
    out.push_str("visibility=? unless stated; bounds are not hit-test evidence; omitted flags are unspecified\n");
    let mut displayed_depths: HashMap<&ElementRef, usize> = HashMap::new();
    let mut structural_omissions = 0;
    for row in &snapshot.rows {
        if !row.interactive_candidate
            && row.name.is_none()
            && row.value.as_ref().is_none_or(|v| v.text.is_empty())
            && !row.retain_context
        {
            structural_omissions += 1;
            continue;
        }
        let depth = row
            .ancestors
            .iter()
            .find_map(|ancestor| displayed_depths.get(ancestor).map(|d| d + 1))
            .unwrap_or(0);
        write!(
            out,
            "{}- @e{}",
            "  ".repeat(depth.min(16)),
            row.reference.id
        )
        .unwrap();
        displayed_depths.insert(&row.reference, depth);
        if let Some(role) = &row.role {
            write!(out, " {}", quoted(role)).unwrap();
        }
        if let Some(name) = &row.name {
            write!(out, " {}", quoted(name)).unwrap();
        }
        if let Some(value) = &row.value
            && !value.text.is_empty()
        {
            write!(out, " value={}", quoted(value)).unwrap();
        }
        if row.enabled == Some(false) {
            out.push_str(" [disabled]");
        }
        for (name, flag) in [("focused", row.focused), ("selected", row.selected)] {
            if flag == Some(true) {
                write!(out, " [{name}]").unwrap();
            }
        }
        if let Some(expanded) = row.expanded {
            write!(out, " [expanded={expanded}]").unwrap();
        }
        if row.visibility != Visibility::Unknown {
            write!(
                out,
                " [visibility={}]",
                serde_json::to_string(&row.visibility).unwrap()
            )
            .unwrap();
        }
        if row.viewport_relation != ViewportRelation::Unknown {
            write!(
                out,
                " [bounds={}]",
                serde_json::to_string(&row.viewport_relation).unwrap()
            )
            .unwrap();
        }
        if let Some(scroll) = &row.scroll_container {
            write!(out, " [scroll=@e{}]", scroll.id).unwrap();
        }
        if row.error_count > 0 {
            write!(out, " [errors={}]", row.error_count).unwrap();
        }
        out.push('\n');
    }
    if structural_omissions > 0 {
        writeln!(out, "text_omitted_unnamed_wrappers={structural_omissions}").unwrap();
    }
    writeln!(
        out,
        "view shown={} observed_in_scope={} filtered={} truncated={} unresolved={} repeated={} scope_traversal_complete={}",
        snapshot.rows.len(),
        snapshot.observed_in_scope,
        snapshot.filtered_nodes,
        snapshot.truncated_nodes,
        snapshot.unresolved_references,
        snapshot.repeated_references,
        snapshot.scope_traversal_complete
    )
    .unwrap();
    out
}
/// Every changed field remains listed. Values are previews; missing is distinct from null.
pub fn render_diff_text(diff: &SnapshotDiff, max_text_chars: usize) -> String {
    let schema = diff
        .newly_observed
        .iter()
        .chain(&diff.removed_from_scope)
        .chain(&diff.no_longer_observed)
        .find_map(NativeSchema::detect_node)
        .unwrap_or(&schema::MACOS_AX);
    let mut out = format!(
        "diff session={} root=@e{} revisions={}..{} complete={}..{} traversal_complete={}..{} issues={}..{}\n",
        serde_json::to_string(&diff.root.session).unwrap(),
        diff.root.id,
        diff.before_revision,
        diff.after_revision,
        diff.before_coverage.complete,
        diff.after_coverage.complete,
        diff.before_coverage.traversal_complete,
        diff.after_coverage.traversal_complete,
        diff.before_coverage.issues.len(),
        diff.after_coverage.issues.len()
    );
    for (label, nodes) in [
        ("newly_observed", &diff.newly_observed),
        ("removed_from_scope", &diff.removed_from_scope),
        ("no_longer_observed", &diff.no_longer_observed),
    ] {
        for node in nodes {
            let row = compact(schema, node, 0, max_text_chars);
            write!(out, "{label} @e{}", node.reference.id).unwrap();
            if let Some(role) = row.role {
                write!(out, " role={}", quoted(&role)).unwrap();
            }
            if let Some(name) = row.name {
                write!(out, " name={}", quoted(&name)).unwrap();
            }
            out.push('\n');
        }
    }
    for node in &diff.modified {
        for field in &node.fields {
            let value = |present: bool, v: &Option<Value>| {
                if !present {
                    "<missing>".into()
                } else {
                    quoted(&preview(
                        &v.as_ref().unwrap_or(&Value::Null).to_string(),
                        max_text_chars,
                    ))
                }
            };
            writeln!(
                out,
                "modified @e{} path={} evidence={} before={} after={}",
                node.reference.id,
                serde_json::to_string(&field.path).unwrap(),
                match field.evidence {
                    ChangeEvidence::Observed => "observed",
                    ChangeEvidence::Uncertain => "uncertain",
                },
                value(field.before_present, &field.before),
                value(field.after_present, &field.after)
            )
            .unwrap();
        }
    }
    out
}

/// Compares the displayed projection. Presence changes describe observations, never destruction.
/// Opaque native values are excluded, and unchanged truncated previews are not proof of equality.
pub fn render_view_diff(
    before: &Snapshot,
    after: &Snapshot,
    options: &PresentationOptions,
    max_changes: usize,
) -> Result<String> {
    render_view_diff_with_adapter(
        before,
        after,
        options,
        max_changes,
        &SchemaAdapter(NativeSchema::detect(before)),
    )
}
pub fn render_view_diff_with_adapter(
    before: &Snapshot,
    after: &Snapshot,
    options: &PresentationOptions,
    max_changes: usize,
    adapter: &(impl PresentationAdapter + ?Sized),
) -> Result<String> {
    crate::diff::diff_snapshots(before, after)?;
    if max_changes > 100_000 {
        return Err(invalid("max_changes exceeds 100000"));
    }
    let before_view = render_snapshot_with_adapter(before, options, adapter)?;
    let after_view = render_snapshot_with_adapter(after, options, adapter)?;
    let old: HashMap<_, _> = before_view.rows.iter().map(|r| (&r.reference, r)).collect();
    let new: HashMap<_, _> = after_view.rows.iter().map(|r| (&r.reference, r)).collect();
    let mut lines = Vec::new();
    let mut unknown = 0;
    let native_after: HashMap<_, _> = after.nodes.iter().map(|n| (&n.reference, n)).collect();
    let native_before: HashMap<_, _> = before.nodes.iter().map(|n| (&n.reference, n)).collect();
    let comparable = |row: &CompactNode, node: &Node| {
        let mut value = serde_json::to_value(row).unwrap();
        let object = value.as_object_mut().unwrap();
        for key in [
            "reference",
            "depth",
            "error_count",
            "unavailable_count",
            "geometry_reason",
        ] {
            object.remove(key);
        }
        if !adapter.value_is_comparable(node) {
            object.remove("value");
        }
        value
    };
    let summary = |row: &CompactNode| {
        let mut text = format!("@e{}", row.reference.id);
        if let Some(role) = &row.role {
            write!(text, " {}", quoted(role)).unwrap();
        }
        if let Some(name) = &row.name {
            write!(text, " {}", quoted(name)).unwrap();
        }
        if let Some(value) = &row.value
            && !value.text.is_empty()
        {
            write!(text, " value={}", quoted(value)).unwrap();
        }
        if row.enabled == Some(false) {
            text.push_str(" [disabled]");
        }
        if row.focused == Some(true) {
            text.push_str(" [focused]");
        }
        if row.selected == Some(true) {
            text.push_str(" [selected]");
        }
        text
    };
    for row in &after_view.rows {
        let Some(previous) = old.get(&row.reference) else {
            lines.push(format!("entered_view {}", summary(row)));
            continue;
        };
        let a = comparable(previous, native_before[&row.reference]);
        let b = comparable(row, native_after[&row.reference]);
        let a = a.as_object().unwrap();
        let b = b.as_object().unwrap();
        let keys: std::collections::BTreeSet<_> = a.keys().chain(b.keys()).collect();
        for key in keys {
            if a.get(key) != b.get(key) {
                let show = |v: Option<&Value>| {
                    v.map(|v| {
                        if let Some(text) = v.get("text").and_then(Value::as_str) {
                            quoted(&Preview {
                                text: text.into(),
                                omitted_chars: v
                                    .get("omitted_chars")
                                    .and_then(Value::as_u64)
                                    .unwrap_or(0)
                                    as usize,
                            })
                        } else if let Some(text) = v.as_str() {
                            quoted(&preview(text, options.max_text_chars))
                        } else {
                            let text = v.to_string();
                            let p = preview(&text, options.max_text_chars);
                            if p.omitted_chars == 0 {
                                text
                            } else {
                                quoted(&p)
                            }
                        }
                    })
                    .unwrap_or_else(|| "<unspecified>".into())
                };
                lines.push(format!(
                    "modified @e{} {} before={} after={}",
                    row.reference.id,
                    key,
                    show(a.get(key)),
                    show(b.get(key))
                ));
            }
        }
        if adapter.has_uncertain_values(native_before[&row.reference])
            || adapter.has_uncertain_values(native_after[&row.reference])
            || previous.error_count > 0
            || row.error_count > 0
            || [&previous.name, &previous.value, &row.name, &row.value]
                .into_iter()
                .flatten()
                .any(|p| p.omitted_chars > 0)
        {
            unknown += 1;
        }
    }
    for row in &before_view.rows {
        if !new.contains_key(&row.reference) {
            lines.push(format!(
                "left_view {} evidence={}",
                summary(row),
                if after_view.scope_traversal_complete && after_view.truncated_nodes == 0 {
                    "observed_scope_or_filter_change"
                } else {
                    "uncertain_coverage"
                }
            ));
        }
    }
    let total = lines.len();
    let shown = total.min(max_changes);
    let mut text = format!(
        "view_diff session={} root=@e{} revisions={}..{} before_coverage={} after_coverage={}\n",
        serde_json::to_string(&before_view.root.session).unwrap(),
        before_view.root.id,
        before.revision,
        after.revision,
        before_view.scope_traversal_complete,
        after_view.scope_traversal_complete
    );
    for line in lines.into_iter().take(max_changes) {
        writeln!(text, "{line}").unwrap();
    }
    writeln!(text,"changes={total} shown={shown} omitted={} uncertain_rows={unknown} before_filtered={} after_filtered={} before_truncated={} after_truncated={}",total-shown,before_view.filtered_nodes,after_view.filtered_nodes,before_view.truncated_nodes,after_view.truncated_nodes).unwrap();
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn reference(id: u64) -> ElementRef {
        ElementRef {
            session: "test".into(),
            id,
        }
    }
    fn node(id: u64, children: &[u64]) -> Node {
        Node {
            reference: reference(id),
            attributes: Default::default(),
            actions: vec![],
            parameterized_attributes: vec![],
            children: children.iter().map(|&id| reference(id)).collect(),
            issues: vec![],
        }
    }
    fn snapshot(nodes: Vec<Node>) -> Snapshot {
        Snapshot {
            root: reference(9),
            nodes,
            complete: true,
            traversal_complete: true,
            revision: 3,
            issues: vec![],
        }
    }
    #[test]
    fn web_area_content_bounds_do_not_establish_a_viewport() {
        let node = Node {
            reference: ElementRef {
                session: "s".into(),
                id: 1,
            },
            attributes: std::collections::BTreeMap::from([(
                "AXRole".into(),
                serde_json::json!({"type":"string","value":"AXWebArea"}),
            )]),
            children: vec![],
            actions: vec![],
            parameterized_attributes: vec![],
            issues: vec![],
        };
        assert!(!SchemaAdapter::default().is_viewport(&node));
    }
    #[test]
    fn portable_fields_render_without_claiming_visibility_or_changing_records() {
        let mut n = node(9, &[]);
        n.attributes = std::collections::BTreeMap::from([
            ("role".into(), json!("button")),
            ("name".into(), json!("Save")),
            ("enabled".into(), json!(true)),
            ("offscreen".into(), json!(false)),
            (
                "bounds".into(),
                json!({"type":"rect","x":-100,"y":20,"width":80,"height":30}),
            ),
        ]);
        n.actions = vec!["invoke".into()];
        let original = n.attributes.clone();
        let view =
            render_snapshot(&snapshot(vec![n.clone()]), &PresentationOptions::default()).unwrap();
        let row = &view.rows[0];
        assert_eq!(row.name.as_ref().unwrap().text, "Save");
        assert_eq!(row.role.as_ref().unwrap().text, "button");
        assert_eq!(row.enabled, Some(true));
        assert_eq!(row.visibility, Visibility::Unknown);
        assert!(row.interactive_candidate);
        assert_eq!(
            SchemaAdapter(&schema::PORTABLE).bounds(&n).unwrap().x,
            -100.
        );
        assert_eq!(n.attributes, original);
        n.attributes.insert("offscreen".into(), json!(true));
        let hidden = render_snapshot(&snapshot(vec![n]), &PresentationOptions::default()).unwrap();
        assert_eq!(hidden.rows[0].visibility, Visibility::Hidden);
    }
    #[test]
    fn portable_value_diff_uses_retained_scalar_values() {
        let mut n = node(9, &[]);
        n.attributes = std::collections::BTreeMap::from([
            ("role".into(), json!("slider")),
            ("value".into(), json!(5)),
        ]);
        let before = snapshot(vec![n.clone()]);
        n.attributes.insert("value".into(), json!(6));
        let mut after = snapshot(vec![n]);
        after.revision = before.revision + 1;
        let diff = render_view_diff(&before, &after, &PresentationOptions::default(), 20).unwrap();
        assert!(diff.contains("value before=\"5\" after=\"6\""), "{diff}");
        let rendered = render_snapshot(&after, &PresentationOptions::default()).unwrap();
        assert_eq!(rendered.rows[0].role.as_ref().unwrap().text, "slider");
    }
    #[test]
    fn filters_keep_refs_and_report_cycles_and_missing() {
        let root = node(9, &[42, 77, 88]);
        let mut button = node(42, &[9]);
        button.actions.push("AXPress".into());
        let mut hidden = node(77, &[]);
        hidden.actions.push("AXPress".into());
        hidden
            .attributes
            .insert("AXHidden".into(), json!({"type":"bool","value":true}));
        let view = render_snapshot(
            &snapshot(vec![root, button, hidden]),
            &PresentationOptions {
                actionable_only: true,
                hide_known_hidden: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(view.rows[0].reference.id, 42);
        assert_eq!(view.rows.len(), 1);
        assert_eq!(view.filtered_nodes, 2);
        assert_eq!(view.repeated_references, 1);
        assert_eq!(view.unresolved_references, 1);
        assert!(view.native_traversal_complete);
    }
    #[test]
    fn unknown_visibility_is_not_hidden_or_clickable() {
        let mut root = node(9, &[]);
        root.attributes.insert("AXHidden".into(), json!(false));
        root.attributes.insert("AXMinimized".into(), json!(false));
        let view = render_snapshot(
            &snapshot(vec![root]),
            &PresentationOptions {
                hide_known_hidden: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(view.rows[0].visibility, Visibility::Unknown);
        assert!(!view.rows[0].interactive_candidate);
    }
    #[test]
    fn truncation_preserves_unicode_and_escapes_tree_injection() {
        let mut root = node(9, &[42]);
        root.attributes
            .insert("AXTitle".into(), json!("🦀\n\"command\""));
        let view = render_snapshot(
            &snapshot(vec![root, node(42, &[])]),
            &PresentationOptions {
                max_nodes: 1,
                max_text_chars: 3,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(view.truncated_nodes, 1);
        assert_eq!(view.rows[0].name.as_ref().unwrap().text, "🦀\n\"");
        let text = render_snapshot_text(&view);
        assert!(text.contains("\\n\\\""));
        assert!(text.contains("truncated 8 chars"));
        assert_eq!(text.lines().count(), 4);
    }
    #[test]
    fn scoped_views_validate_session_and_preserve_depth() {
        let snap = snapshot(vec![node(9, &[42]), node(42, &[])]);
        let view = render_snapshot(
            &snap,
            &PresentationOptions {
                root: Some(reference(42)),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(view.rows[0].depth, 0);
        assert_eq!(view.root.id, 42);
        assert!(
            render_snapshot(
                &snap,
                &PresentationOptions {
                    root: Some(ElementRef {
                        session: "other".into(),
                        id: 42
                    }),
                    ..Default::default()
                }
            )
            .is_err()
        );
    }
    #[test]
    fn diff_keeps_uncertainty_and_missing_distinct_from_null() {
        let before = snapshot(vec![node(9, &[])]);
        let mut after = before.clone();
        after.revision += 1;
        after.nodes[0]
            .attributes
            .insert("untrusted\nfield".into(), Value::Null);
        let diff = crate::diff::diff_snapshots(&before, &after).unwrap();
        let text = render_diff_text(&diff, 20);
        assert!(text.contains("@e9"));
        assert!(text.contains("evidence=uncertain"));
        assert!(text.contains("before=<missing> after=\"null\""));
        assert!(text.contains("untrusted\\nfield"));
        assert_eq!(text.lines().count(), 2);
    }
    #[test]
    fn conflicting_visibility_evidence_does_not_advertise_visibility() {
        let mut root = node(9, &[]);
        root.attributes.insert("AXVisible".into(), json!(true));
        root.attributes.insert("AXMinimized".into(), json!(true));
        let view = render_snapshot(&snapshot(vec![root]), &PresentationOptions::default()).unwrap();
        assert_eq!(view.rows[0].visibility, Visibility::Hidden);
        assert_eq!(view.rows[0].visibility_evidence.len(), 2);
    }
    fn rect(node: &mut Node, x: f64, y: f64, width: f64, height: f64) {
        node.attributes
            .insert("AXPosition".into(), json!({"type":"point","x":x,"y":y}));
        node.attributes.insert(
            "AXSize".into(),
            json!({"type":"size","width":width,"height":height}),
        );
    }
    #[test]
    fn viewport_scroll_diff_tracks_bounds_without_opaque_noise() {
        let mut scroll = node(9, &[42]);
        scroll
            .attributes
            .insert("AXRole".into(), json!("AXScrollArea"));
        rect(&mut scroll, 0.0, 0.0, 100.0, 100.0);
        let mut item = node(42, &[]);
        item.actions = vec!["AXPress".into()];
        rect(&mut item, 1.0, 120.0, 20.0, 20.0);
        item.attributes
            .insert("AXValue".into(), json!({"type":"opaque","id":100}));
        let before = snapshot(vec![scroll, item]);
        let mut after = before.clone();
        after.revision += 1;
        rect(&mut after.nodes[1], 1.0, 20.0, 20.0, 20.0);
        after.nodes[1]
            .attributes
            .insert("AXValue".into(), json!({"type":"opaque","id":101}));
        let view = render_snapshot(&before, &PresentationOptions::default()).unwrap();
        assert_eq!(view.rows[1].viewport_relation, ViewportRelation::Below);
        assert_eq!(view.rows[1].scroll_container.as_ref().unwrap().id, 9);
        let diff = render_view_diff(&before, &after, &PresentationOptions::default(), 10).unwrap();
        assert!(diff.contains("viewport_relation"));
        assert!(diff.contains("below"));
        assert!(diff.contains("inside"));
        assert!(!diff.contains("opaque"));
        let omitted =
            render_view_diff(&before, &after, &PresentationOptions::default(), 0).unwrap();
        assert!(omitted.contains("omitted=1"));
    }
    #[test]
    fn electron_generic_actions_do_not_make_wrappers_interactive() {
        let mut root = node(9, &[]);
        root.actions = vec!["AXShowMenu".into(), "AXScrollToVisible".into()];
        let view = render_snapshot(
            &snapshot(vec![root]),
            &PresentationOptions {
                actionable_only: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(view.rows.is_empty());
        assert_eq!(view.filtered_nodes, 1);
    }
    #[test]
    fn viewport_ambiguous_parentage_stays_unknown() {
        let mut root = node(9, &[42, 77]);
        root.attributes.insert("AXRole".into(), json!("AXWindow"));
        rect(&mut root, 0.0, 0.0, 100.0, 100.0);
        let mut child = node(42, &[]);
        rect(&mut child, 1.0, 1.0, 5.0, 5.0);
        let view = render_snapshot(
            &snapshot(vec![root, child, node(77, &[42])]),
            &PresentationOptions::default(),
        )
        .unwrap();
        assert_eq!(
            view.rows
                .iter()
                .find(|r| r.reference.id == 42)
                .unwrap()
                .viewport_relation,
            ViewportRelation::Unknown
        );
    }
    #[test]
    fn filtered_wrappers_do_not_turn_siblings_into_children() {
        let root = node(9, &[42, 77]);
        let mut first = node(42, &[]);
        first.actions = vec!["AXPress".into()];
        let wrapper = node(77, &[88]);
        let mut second = node(88, &[]);
        second.actions = vec!["AXPress".into()];
        let view = render_snapshot(
            &snapshot(vec![root, first, wrapper, second]),
            &PresentationOptions {
                actionable_only: true,
                ..Default::default()
            },
        )
        .unwrap();
        let text = render_snapshot_text(&view);
        assert!(text.lines().any(|line| line.starts_with("- @e42")));
        assert!(text.lines().any(|line| line.starts_with("- @e88")));
    }
    #[test]
    fn scalar_preview_and_optional_attribute_absence_are_compact() {
        let mut root = node(9, &[]);
        root.attributes
            .insert("AXValue".into(), json!({"type":"integer","value":1}));
        root.attributes.insert(
            "AXHelp".into(),
            json!({"type":"read_error","error":{"code":"ax_-25212"}}),
        );
        root.attributes.insert(
            "AXTitle".into(),
            json!({"type":"read_error","error":{"code":"ax_-25200"}}),
        );
        let view = render_snapshot(&snapshot(vec![root]), &PresentationOptions::default()).unwrap();
        assert_eq!(view.rows[0].value.as_ref().unwrap().text, "1");
        assert_eq!(view.rows[0].error_count, 1);
        assert_eq!(view.rows[0].unavailable_count, 1);
    }
    #[test]
    fn attributed_names_follow_plain_names_and_skip_read_errors() {
        let mut root = node(9, &[]);
        root.attributes.insert(
            "AXTitle".into(),
            json!({"type":"read_error","error":{"code":"ax_-25212"}}),
        );
        root.attributes.insert("AXAttributedDescription".into(), json!({"type":"attributed_string","text":{"type":"string","value":"Display"},"native":{"type":"opaque","description":"do not parse"}}));
        let view = render_snapshot(
            &snapshot(vec![root.clone()]),
            &PresentationOptions::default(),
        )
        .unwrap();
        assert_eq!(view.rows[0].name.as_ref().unwrap().text, "Display");
        assert_eq!(
            view.rows[0].name_attribute.as_deref(),
            Some("AXAttributedDescription")
        );
        root.attributes
            .insert("AXLabel".into(), json!("Plain label"));
        let view = render_snapshot(&snapshot(vec![root]), &PresentationOptions::default()).unwrap();
        assert_eq!(view.rows[0].name.as_ref().unwrap().text, "Plain label");
    }
    #[test]
    fn absent_ax_values_are_omitted_and_unknown_values_are_bounded() {
        for code in ["ax_-25212", "ax_-25205"] {
            let mut root = node(9, &[]);
            root.attributes.insert(
                "AXValue".into(),
                json!({"type":"read_error","error":{"code":code,"message":"native diagnostic"}}),
            );
            let view =
                render_snapshot(&snapshot(vec![root]), &PresentationOptions::default()).unwrap();
            assert!(view.rows[0].value.is_none());
            assert_eq!(view.rows[0].unavailable_count, 1);
            assert_eq!(view.rows[0].error_count, 0);
            assert!(!render_snapshot_text(&view).contains("native diagnostic"));
        }
        for value in [
            json!({"type":"read_error","error":{"code":"ax_-25200","message":"native diagnostic"}}),
            json!({"type":"opaque","id":1,"description":"native diagnostic".repeat(100)}),
            json!({"type":"array","value":[1,2,3]}),
        ] {
            let mut root = node(9, &[]);
            root.attributes.insert("AXValue".into(), value);
            let before = snapshot(vec![root]);
            let view = render_snapshot(&before, &PresentationOptions::default()).unwrap();
            assert_eq!(
                view.rows[0].value.as_ref().unwrap().text,
                "<unknown; inspect>"
            );
            assert!(!render_snapshot_text(&view).contains("native diagnostic"));
            let mut after = before.clone();
            after.revision += 1;
            let diff =
                render_view_diff(&before, &after, &PresentationOptions::default(), 10).unwrap();
            assert!(diff.contains("uncertain_rows=1"), "{diff}");
            assert!(!diff.contains("modified"), "{diff}");
        }
    }
    #[test]
    fn view_diff_uses_single_quoted_scalar_and_identifies_entered_rows() {
        let mut root = node(9, &[]);
        root.attributes
            .insert("AXValue".into(), json!({"type":"integer","value":1}));
        let before = snapshot(vec![root]);
        let mut after = before.clone();
        after.revision += 1;
        after.nodes[0]
            .attributes
            .insert("AXValue".into(), json!({"type":"integer","value":2}));
        after.nodes[0].children.push(reference(42));
        let mut button = node(42, &[]);
        button.attributes.insert("AXRole".into(), json!("AXButton"));
        button
            .attributes
            .insert("AXTitle".into(), json!("Search\nfiles"));
        after.nodes.push(button);
        let text = render_view_diff(&before, &after, &PresentationOptions::default(), 10).unwrap();
        assert!(text.contains("value before=\"1\" after=\"2\""));
        assert!(!text.contains("omitted_chars"));
        assert!(text.contains("entered_view @e42 \"AXButton\" \"Search\\nfiles\""));
        assert_eq!(text.lines().count(), 4);
    }
    #[test]
    fn observed_hidden_ancestor_propagates_but_absent_ancestry_stays_unknown() {
        let mut root = node(9, &[42]);
        root.attributes.insert("AXHidden".into(), json!(true));
        let snap = snapshot(vec![root, node(42, &[])]);
        let view = render_snapshot(&snap, &PresentationOptions::default()).unwrap();
        assert_eq!(view.rows[1].visibility, Visibility::Hidden);
        assert_eq!(
            view.rows[1].inherited_hidden.as_ref().unwrap().ancestor.id,
            9
        );
        let filtered = render_snapshot(
            &snap,
            &PresentationOptions {
                hide_known_hidden: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(filtered.filtered_nodes, 2);
        let absent = render_snapshot(
            &snapshot(vec![node(9, &[])]),
            &PresentationOptions::default(),
        )
        .unwrap();
        assert_eq!(absent.rows[0].visibility, Visibility::Unknown);
    }
    #[test]
    fn ambiguous_ancestry_discards_inherited_hidden_evidence() {
        let mut root = node(9, &[42, 77]);
        root.attributes.insert("AXHidden".into(), json!(true));
        let view = render_snapshot(
            &snapshot(vec![root, node(42, &[]), node(77, &[42])]),
            &PresentationOptions::default(),
        )
        .unwrap();
        let child = view.rows.iter().find(|r| r.reference.id == 42).unwrap();
        assert_eq!(child.visibility, Visibility::Unknown);
        assert!(child.inherited_hidden.is_none());
    }
    #[test]
    fn dynamic_non_ax_adapter_drives_snapshot_and_projected_diff() {
        struct Custom;
        impl PresentationAdapter for Custom {
            fn project(&self, node: &Node, depth: usize, limit: usize) -> CompactNode {
                let mut row = compact(&schema::MACOS_AX, node, depth, limit);
                row.role = Some(preview("custom_entry", limit));
                row.name = node
                    .attributes
                    .get("label")
                    .and_then(Value::as_str)
                    .map(|s| preview(s, limit));
                row.value = node
                    .attributes
                    .get("content")
                    .and_then(Value::as_str)
                    .map(|s| preview(s, limit));
                row
            }
            fn has_uncertain_values(&self, node: &Node) -> bool {
                node.attributes.get("uncertain") == Some(&Value::Bool(true))
            }
        }
        let adapter: &dyn PresentationAdapter = &Custom;
        let mut root = node(9, &[]);
        root.attributes
            .insert("label".into(), json!("Native field"));
        root.attributes.insert("content".into(), json!("first"));
        let before = snapshot(vec![root]);
        let mut after = before.clone();
        after.revision += 1;
        after.nodes[0]
            .attributes
            .insert("content".into(), json!("second"));
        after.nodes[0]
            .attributes
            .insert("uncertain".into(), json!(true));
        let view = render_snapshot_with_adapter(&before, &PresentationOptions::default(), adapter)
            .unwrap();
        assert_eq!(view.rows[0].name.as_ref().unwrap().text, "Native field");
        let diff = render_view_diff_with_adapter(
            &before,
            &after,
            &PresentationOptions::default(),
            10,
            adapter,
        )
        .unwrap();
        assert!(diff.contains("value before=\"first\" after=\"second\""));
        assert!(diff.contains("uncertain_rows=1"));
        assert!(!diff.contains("modified @e9 name"));
    }
}

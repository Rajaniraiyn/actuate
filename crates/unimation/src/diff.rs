//! Diffs describe observations, never native element destruction.
use crate::{ElementRef, NativeError, Node, Result, Snapshot, schema::NativeSchema, values};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationCoverage {
    pub complete: bool,
    pub traversal_complete: bool,
    pub issues: Vec<Value>,
}
impl From<&Snapshot> for ObservationCoverage {
    fn from(s: &Snapshot) -> Self {
        Self {
            complete: s.complete,
            traversal_complete: s.traversal_complete,
            issues: s.issues.clone(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeEvidence {
    /// Two directly comparable values were observed. This does not establish causality.
    Observed,
    /// Missing attributes, read errors, opaque handles or incomplete enumeration.
    Uncertain,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldChange {
    /// JSON pointer relative to the node, with escaped native attribute names.
    pub path: String,
    /// None means not present in this observation. Some(null) remains distinct.
    pub before_present: bool,
    pub before: Option<Value>,
    pub after_present: bool,
    pub after: Option<Value>,
    pub evidence: ChangeEvidence,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModifiedNode {
    pub reference: ElementRef,
    pub fields: Vec<FieldChange>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotDiff {
    pub root: ElementRef,
    pub before_revision: u64,
    pub after_revision: u64,
    pub before_coverage: ObservationCoverage,
    pub after_coverage: ObservationCoverage,
    /// Previously absent observations, not necessarily newly created elements.
    pub newly_observed: Vec<Node>,
    /// Absent from a completely traversed scope, not proof of native destruction.
    pub removed_from_scope: Vec<Node>,
    /// Absent while scope coverage is incomplete.
    pub no_longer_observed: Vec<Node>,
    pub modified: Vec<ModifiedNode>,
}
fn invalid(message: &str) -> NativeError {
    NativeError::new("incompatible_snapshots", message)
}
fn index(snapshot: &Snapshot) -> Result<BTreeMap<u64, &Node>> {
    let mut result = BTreeMap::new();
    for node in &snapshot.nodes {
        if node.reference.session != snapshot.root.session
            || node
                .children
                .iter()
                .any(|r| r.session != snapshot.root.session)
        {
            return Err(invalid("Snapshot contains foreign-session references"));
        }
        if result.insert(node.reference.id, node).is_some() {
            return Err(invalid("Snapshot contains duplicate node references"));
        }
    }
    if snapshot.traversal_complete
        && (!result.contains_key(&snapshot.root.id)
            || snapshot
                .nodes
                .iter()
                .flat_map(|n| &n.children)
                .any(|r| !result.contains_key(&r.id)))
    {
        return Err(invalid("Complete traversal contains unresolved nodes"));
    }
    Ok(result)
}
fn field(
    fields: &mut Vec<FieldChange>,
    path: String,
    before: Option<Value>,
    after: Option<Value>,
    enumeration_uncertain: bool,
) {
    if before == after {
        return;
    }
    let evidence = if enumeration_uncertain
        || before.as_ref().is_none_or(values::is_uncertain)
        || after.as_ref().is_none_or(values::is_uncertain)
    {
        ChangeEvidence::Uncertain
    } else {
        ChangeEvidence::Observed
    };
    fields.push(FieldChange {
        path,
        before_present: before.is_some(),
        after_present: after.is_some(),
        before,
        after,
        evidence,
    });
}
/// Compare snapshots of the same exact root in increasing revision order.
/// Reparenting keeps element identity and changes the parents' children fields.
/// Collection order is retained, including action and parameter enumeration order.
pub fn diff_snapshots(before: &Snapshot, after: &Snapshot) -> Result<SnapshotDiff> {
    if before.root != after.root {
        return Err(invalid("Snapshots must have the same root and session"));
    }
    if after.revision <= before.revision {
        return Err(invalid("After revision must be newer than before revision"));
    }
    let old = index(before)?;
    let new = index(after)?;
    let children_attribute = NativeSchema::detect(before).children;
    let mut result = SnapshotDiff {
        root: before.root.clone(),
        before_revision: before.revision,
        after_revision: after.revision,
        before_coverage: before.into(),
        after_coverage: after.into(),
        newly_observed: vec![],
        removed_from_scope: vec![],
        no_longer_observed: vec![],
        modified: vec![],
    };
    for (&id, node) in &new {
        let Some(previous) = old.get(&id) else {
            result.newly_observed.push((*node).clone());
            continue;
        };
        let mut fields = vec![];
        let keys: BTreeSet<_> = previous
            .attributes
            .keys()
            .chain(node.attributes.keys())
            .collect();
        for key in keys {
            field(
                &mut fields,
                format!("/attributes/{}", key.replace('~', "~0").replace('/', "~1")),
                previous.attributes.get(key).cloned(),
                node.attributes.get(key).cloned(),
                false,
            );
        }
        // Native enumeration failures currently appear in node issues without a field tag.
        // Conservatively classify all changed collections when either node has issues.
        let enumeration_uncertain = !previous.issues.is_empty() || !node.issues.is_empty();
        for (name, a, b) in [
            (
                "actions",
                serde_json::json!(previous.actions),
                serde_json::json!(node.actions),
            ),
            (
                "parameterized_attributes",
                serde_json::json!(previous.parameterized_attributes),
                serde_json::json!(node.parameterized_attributes),
            ),
            (
                "children",
                serde_json::json!(previous.children),
                serde_json::json!(node.children),
            ),
            (
                "issues",
                serde_json::json!(previous.issues),
                serde_json::json!(node.issues),
            ),
        ] {
            field(
                &mut fields,
                format!("/{name}"),
                Some(a),
                Some(b),
                enumeration_uncertain
                    || (name == "children"
                        && children_attribute.is_some_and(|attribute| {
                            [previous, node].iter().any(|n| {
                                n.attributes
                                    .get(attribute)
                                    .is_some_and(values::is_uncertain)
                            })
                        })),
            );
        }
        if !fields.is_empty() {
            result.modified.push(ModifiedNode {
                reference: node.reference.clone(),
                fields,
            });
        }
    }
    for (&id, node) in &old {
        if !new.contains_key(&id) {
            if after.traversal_complete {
                result.removed_from_scope.push((*node).clone());
            } else {
                result.no_longer_observed.push((*node).clone());
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn r(id: u64) -> ElementRef {
        ElementRef {
            session: "test".into(),
            id,
        }
    }
    fn node(id: u64) -> Node {
        Node {
            reference: r(id),
            attributes: BTreeMap::new(),
            actions: vec![],
            parameterized_attributes: vec![],
            children: vec![],
            issues: vec![],
        }
    }
    fn snapshot() -> Snapshot {
        Snapshot {
            root: r(1),
            nodes: vec![node(1), node(2)],
            complete: true,
            traversal_complete: true,
            revision: 1,
            issues: vec![],
        }
    }
    #[test]
    fn incomplete_absence_is_not_removal() {
        let a = snapshot();
        let mut b = a.clone();
        b.revision = 2;
        b.nodes.pop();
        b.complete = false;
        b.traversal_complete = false;
        let d = diff_snapshots(&a, &b).unwrap();
        assert_eq!(d.no_longer_observed.len(), 1);
        assert!(d.removed_from_scope.is_empty());
        b.traversal_complete = true;
        assert_eq!(diff_snapshots(&a, &b).unwrap().removed_from_scope.len(), 1);
    }
    #[test]
    fn rejects_foreign_and_reversed_observations() {
        let a = snapshot();
        let mut b = a.clone();
        assert!(diff_snapshots(&a, &b).is_err());
        b.revision = 2;
        b.root.session = "other".into();
        assert!(diff_snapshots(&a, &b).is_err());
        b.root = a.root.clone();
        b.nodes[1].reference.session = "other".into();
        assert!(diff_snapshots(&a, &b).is_err());
    }
    #[test]
    fn reparent_and_parameter_change_keep_identity() {
        let mut a = snapshot();
        a.nodes.push(node(3));
        a.nodes[0].children = vec![r(2), r(3)];
        let mut b = a.clone();
        b.revision = 2;
        b.nodes[0].children = vec![r(3)];
        b.nodes[2].children = vec![r(2)];
        b.nodes[1]
            .parameterized_attributes
            .push("AXStringForRange".into());
        let d = diff_snapshots(&a, &b).unwrap();
        assert!(d.newly_observed.is_empty());
        assert!(d.removed_from_scope.is_empty());
        assert_eq!(d.modified.len(), 3);
        assert_eq!(d.modified[1].fields[0].path, "/parameterized_attributes");
    }
    #[test]
    fn retains_errors_opaque_and_missing_distinct_from_null() {
        let mut a = snapshot();
        a.nodes[0].attributes.insert("a/b~c".into(), json!(null));
        a.nodes[0]
            .attributes
            .insert("value".into(), json!({"type":"string","value":"hello"}));
        a.nodes[0]
            .attributes
            .insert("object".into(), json!({"type":"opaque","id":1}));
        let mut b = a.clone();
        b.revision = 2;
        b.nodes[0].attributes.remove("a/b~c");
        b.nodes[0].attributes.insert(
            "value".into(),
            json!({"type":"read_error","error":"timeout"}),
        );
        b.nodes[0]
            .attributes
            .insert("object".into(), json!({"type":"opaque","id":2}));
        let d = diff_snapshots(&a, &b).unwrap();
        let f = &d.modified[0].fields;
        assert_eq!(f[0].path, "/attributes/a~1b~0c");
        assert_eq!(f[0].before, Some(Value::Null));
        assert_eq!(f[0].after, None);
        let encoded = serde_json::to_value(&f[0]).unwrap();
        assert_eq!(encoded["before_present"], true);
        assert_eq!(encoded["after_present"], false);
        assert!(
            f.iter()
                .all(|f| matches!(f.evidence, ChangeEvidence::Uncertain))
        );
    }
    #[test]
    fn rejects_duplicate_and_dangling_complete_nodes() {
        let a = snapshot();
        let mut b = a.clone();
        b.revision = 2;
        b.nodes.push(node(1));
        assert!(diff_snapshots(&a, &b).is_err());
        b.nodes.pop();
        b.nodes[0].children.push(r(99));
        assert!(diff_snapshots(&a, &b).is_err());
    }
    #[test]
    fn failed_children_read_does_not_prove_reparenting() {
        let mut before = snapshot();
        before.nodes[0].children = vec![r(2)];
        let mut after = before.clone();
        after.revision = 2;
        after.complete = false;
        after.traversal_complete = false;
        after.nodes[0].children.clear();
        after.nodes[0].attributes.insert(
            "AXChildren".into(),
            json!({"type":"read_error","error":{"code":"timeout"}}),
        );
        let delta = diff_snapshots(&before, &after).unwrap();
        let children = delta.modified[0]
            .fields
            .iter()
            .find(|field| field.path == "/children")
            .unwrap();
        assert!(matches!(children.evidence, ChangeEvidence::Uncertain));
        assert!(delta.removed_from_scope.is_empty());
    }

    #[test]
    fn nested_opaque_handle_churn_is_uncertain() {
        let mut before = snapshot();
        before.nodes[0].attributes.insert(
            "native".into(),
            json!({"type":"array","value":[{"type":"opaque","session":"test","id":1}]}),
        );
        let mut after = before.clone();
        after.revision = 2;
        after.nodes[0].attributes.get_mut("native").unwrap()["value"][0]["id"] = json!(2);
        let delta = diff_snapshots(&before, &after).unwrap();
        assert_eq!(delta.modified.len(), 1);
        assert!(matches!(
            delta.modified[0].fields[0].evidence,
            ChangeEvidence::Uncertain
        ));
    }
}

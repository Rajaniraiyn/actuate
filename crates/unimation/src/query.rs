//! Read-only filters over an existing observation. Returned nodes retain all data.
use crate::{ElementRef, Node, Snapshot, schema::NativeSchema, values};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TextMatch {
    Exact { value: String },
    Contains { value: String },
}
impl TextMatch {
    fn matches(&self, actual: &str) -> bool {
        match self {
            Self::Exact { value } => actual == value,
            Self::Contains { value } => actual.contains(value),
        }
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeQuery {
    /// Native role, such as AXButton. No role aliases are inferred.
    pub role: Option<TextMatch>,
    /// Matches plain or attributed AX title, description and label text without choosing a canonical name.
    pub name: Option<TextMatch>,
    /// Exact raw JSON equality, including native type tags. All predicates must match.
    #[serde(default)]
    pub attributes: BTreeMap<String, Value>,
    /// Require a listed native action, such as AXPress.
    pub action: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub root: ElementRef,
    pub revision: u64,
    pub complete: bool,
    pub traversal_complete: bool,
    pub issues: Vec<Value>,
    pub matches: Vec<Node>,
    /// Nodes for which at least one predicate could not be evaluated reliably.
    pub indeterminate: Vec<Node>,
}
#[derive(Clone, Copy, PartialEq)]
enum Match {
    Yes,
    No,
    Unknown,
}
fn text(node: &Node, names: &[&str], predicate: &TextMatch) -> Match {
    let mut unknown = false;
    for name in names {
        match node.attributes.get(*name) {
            Some(value) if values::string(value).is_some_and(|s| predicate.matches(s)) => {
                return Match::Yes;
            }
            Some(value) if values::string(value).is_some() => (),
            Some(value) if values::is_unreadable(value) => unknown = true,
            None if !node.issues.is_empty() => unknown = true,
            _ => (),
        }
    }
    if unknown { Match::Unknown } else { Match::No }
}
/// AND across filters; OR across the schema's native name attributes.
/// Missing nodes in incomplete coverage are never implied to be non-matches.
pub fn query_nodes(snapshot: &Snapshot, query: &NodeQuery) -> QueryResult {
    query_nodes_with_schema(snapshot, query, NativeSchema::detect(snapshot))
}
pub fn query_nodes_with_schema(
    snapshot: &Snapshot,
    query: &NodeQuery,
    schema: &NativeSchema,
) -> QueryResult {
    let mut result = QueryResult {
        root: snapshot.root.clone(),
        revision: snapshot.revision,
        complete: snapshot.complete,
        traversal_complete: snapshot.traversal_complete,
        issues: snapshot.issues.clone(),
        matches: vec![],
        indeterminate: vec![],
    };
    for node in &snapshot.nodes {
        let mut predicates = vec![];
        if let Some(role) = &query.role {
            predicates.push(text(node, &[schema.role], role));
        }
        if let Some(name) = &query.name {
            predicates.push(text(node, schema.names, name));
        }
        if let Some(action) = &query.action {
            predicates.push(if node.actions.contains(action) {
                Match::Yes
            } else if !node.issues.is_empty() {
                Match::Unknown
            } else {
                Match::No
            });
        }
        for (key, expected) in &query.attributes {
            // Raw error/opaque equality can itself be queried intentionally. A mismatched
            // unreadable value cannot establish a native-value mismatch.
            predicates.push(match node.attributes.get(key) {
                Some(actual) if actual == expected => Match::Yes,
                Some(actual) if values::is_unreadable(actual) => Match::Unknown,
                None if !node.issues.is_empty() => Match::Unknown,
                _ => Match::No,
            });
        }
        if predicates.contains(&Match::No) {
            continue;
        }
        if predicates.contains(&Match::Unknown) {
            result.indeterminate.push(node.clone());
        } else {
            result.matches.push(node.clone());
        }
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn snapshot() -> Snapshot {
        let r = ElementRef {
            session: "s".into(),
            id: 1,
        };
        Snapshot {
            root: r.clone(),
            revision: 1,
            complete: false,
            traversal_complete: false,
            issues: vec![json!({"code":"node_limit"})],
            nodes: vec![Node {
                reference: r,
                attributes: BTreeMap::from([
                    ("AXRole".into(), json!({"type":"string","value":"AXButton"})),
                    (
                        "AXDescription".into(),
                        json!({"type":"string","value":"Equal to"}),
                    ),
                    (
                        "AXValue".into(),
                        json!({"type":"read_error","error":"timeout"}),
                    ),
                ]),
                actions: vec!["AXPress".into()],
                parameterized_attributes: vec![],
                children: vec![],
                issues: vec![],
            }],
        }
    }
    #[test]
    fn attributed_names_are_searchable_without_parsing_native_diagnostics() {
        let mut source = snapshot();
        source.nodes[0].attributes.insert(
            "AXDescription".into(),
            serde_json::json!({"type":"read_error"}),
        );
        source.nodes[0].attributes.insert("AXAttributedDescription".into(), serde_json::json!({"type":"attributed_string","text":{"type":"string","value":"Display"},"native":{"type":"opaque","description":"not the label"}}));
        let query = NodeQuery {
            name: Some(TextMatch::Exact {
                value: "Display".into(),
            }),
            ..Default::default()
        };
        assert_eq!(query_nodes(&source, &query).matches.len(), 1);
    }
    #[test]
    fn combines_filters_without_projection_or_false_coverage() {
        let s = snapshot();
        let q = NodeQuery {
            role: Some(TextMatch::Exact {
                value: "AXButton".into(),
            }),
            name: Some(TextMatch::Contains {
                value: "Equal".into(),
            }),
            action: Some("AXPress".into()),
            ..Default::default()
        };
        let result = query_nodes(&s, &q);
        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].attributes, s.nodes[0].attributes);
        assert!(!result.traversal_complete);
        assert_eq!(result.issues, s.issues);
    }
    #[test]
    fn unreadable_is_indeterminate_and_exact_error_predicates_work() {
        let s = snapshot();
        let mut q = NodeQuery::default();
        q.attributes.insert("AXValue".into(), json!(1));
        assert_eq!(query_nodes(&s, &q).indeterminate.len(), 1);
        q.attributes
            .insert("AXValue".into(), s.nodes[0].attributes["AXValue"].clone());
        assert_eq!(query_nodes(&s, &q).matches.len(), 1);
    }
    #[test]
    fn query_does_not_accept_unknown_fields() {
        assert!(serde_json::from_value::<NodeQuery>(json!({"typo":"AXButton"})).is_err());
    }
    #[test]
    fn explicit_null_is_not_missing_attribute() {
        let mut source = snapshot();
        let mut query = NodeQuery::default();
        query.attributes.insert("nullable".into(), Value::Null);
        assert!(query_nodes(&source, &query).matches.is_empty());
        source.nodes[0]
            .attributes
            .insert("nullable".into(), Value::Null);
        assert_eq!(query_nodes(&source, &query).matches.len(), 1);
        source.nodes[0].attributes.remove("nullable");
        source.nodes[0]
            .issues
            .push(json!({"code":"attribute_names_failed"}));
        let result = query_nodes(&source, &query);
        assert!(result.matches.is_empty());
        assert_eq!(result.indeterminate.len(), 1);
    }

    #[test]
    fn failed_name_read_does_not_hide_alternative_match() {
        let mut source = snapshot();
        source.nodes[0].attributes.insert(
            "AXTitle".into(),
            json!({"type":"read_error","error":"timeout"}),
        );
        let mut query = NodeQuery {
            name: Some(TextMatch::Exact {
                value: "Equal to".into(),
            }),
            ..Default::default()
        };
        assert_eq!(query_nodes(&source, &query).matches.len(), 1);
        query.name = Some(TextMatch::Exact {
            value: "Other".into(),
        });
        let result = query_nodes(&source, &query);
        assert!(result.matches.is_empty());
        assert_eq!(result.indeterminate.len(), 1);
    }
}

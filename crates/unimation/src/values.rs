//! Shared readers for the tagged native value encoding used by every provider.
//!
//! Providers encode native values as `{"type": ..., ...}` objects so that
//! strings, numbers, errors and opaque handles stay distinguishable. These
//! helpers read that encoding without rewriting or normalizing it.
use serde_json::Value;

fn tag(value: &Value) -> Option<&str> {
    value.get("type").and_then(Value::as_str)
}

/// Plain text of a native string, attributed string, or bare JSON string.
pub fn string(value: &Value) -> Option<&str> {
    match tag(value) {
        Some("string") => value.get("value")?.as_str(),
        Some("attributed_string") => string(value.get("text")?),
        _ => value.as_str(),
    }
}

/// A native or bare JSON boolean.
pub fn boolean(value: &Value) -> Option<bool> {
    match tag(value) {
        Some("bool") => value.get("value")?.as_bool(),
        _ => value.as_bool(),
    }
}

/// Scalar text for compact previews. Containers, handles and errors are `None`.
pub fn scalar(value: &Value) -> Option<String> {
    let scalar = if value.is_object() {
        match tag(value)? {
            "string" | "integer" | "float" | "bool" => value.get("value")?,
            _ => return None,
        }
    } else {
        value
    };
    if let Some(text) = scalar.as_str() {
        Some(text.into())
    } else if scalar.is_number() || scalar.is_boolean() {
        Some(scalar.to_string())
    } else {
        None
    }
}

fn contains_tag(value: &Value, tags: &[&str]) -> bool {
    match value {
        Value::Object(object) => {
            tag(value).is_some_and(|t| tags.contains(&t))
                || object.values().any(|v| contains_tag(v, tags))
        }
        Value::Array(items) => items.iter().any(|v| contains_tag(v, tags)),
        _ => false,
    }
}

/// A value whose semantic content could not be read: an error, an opaque
/// handle, or text that is not valid Unicode. Used by query predicates.
pub fn is_unreadable(value: &Value) -> bool {
    contains_tag(value, &["opaque", "read_error", "ax_error", "utf16"])
}

/// A value whose change cannot be established as a native mutation. Invalid
/// UTF-16 remains comparable byte for byte, so it is not uncertain here.
pub fn is_uncertain(value: &Value) -> bool {
    contains_tag(value, &["opaque", "read_error", "ax_error"])
}

/// A read error or native error record, as opposed to a readable value.
pub fn is_error(value: &Value) -> bool {
    matches!(tag(value), Some("read_error" | "ax_error"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn reads_tagged_and_bare_values() {
        assert_eq!(string(&json!({"type":"string","value":"a"})), Some("a"));
        assert_eq!(
            string(&json!({"type":"attributed_string","text":{"type":"string","value":"b"}})),
            Some("b")
        );
        assert_eq!(string(&json!("c")), Some("c"));
        assert_eq!(boolean(&json!({"type":"bool","value":true})), Some(true));
        assert_eq!(boolean(&json!(false)), Some(false));
        assert_eq!(
            scalar(&json!({"type":"integer","value":3})).as_deref(),
            Some("3")
        );
        assert_eq!(scalar(&json!({"type":"opaque","id":1})), None);
    }
    #[test]
    fn utf16_is_unreadable_text_but_comparable_evidence() {
        let value = json!({"type":"utf16","units":[55296]});
        assert!(is_unreadable(&value));
        assert!(!is_uncertain(&value));
        let nested = json!({"type":"array","value":[{"type":"opaque","id":1}]});
        assert!(is_uncertain(&nested));
        assert!(is_error(&json!({"type":"read_error","error":{}})));
        assert!(!is_error(&json!({"type":"string","value":"x"})));
    }
}

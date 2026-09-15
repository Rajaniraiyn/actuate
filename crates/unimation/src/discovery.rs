//! Lossless discovery records with optional agent-facing projections.
use crate::OutputFormat;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryScope {
    #[default]
    All,
    Apps,
}

/// Classification relies on native adapter evidence. Unknown classifications
/// remain discoverable with All; filtering never affects handle validity.
pub fn is_app(record: &Value) -> bool {
    record["activation_policy"] == "regular"
        || record["system_ui"] == true
        || record["visible_window_ids"]
            .as_array()
            .is_some_and(|ids| !ids.is_empty())
}

pub fn present_discovery(raw: &Value, scope: DiscoveryScope, format: OutputFormat) -> Value {
    if scope == DiscoveryScope::All && format == OutputFormat::Json {
        return raw.clone();
    }
    let all = raw["applications"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let selected: Vec<_> = all
        .iter()
        .filter(|record| scope == DiscoveryScope::All || is_app(record))
        .cloned()
        .collect();
    let omitted = all.len() - selected.len();
    let mut errors = BTreeMap::<String, usize>::new();
    for record in all {
        if let Some(code) = record["active_error"]["code"].as_str() {
            *errors.entry(code.to_owned()).or_default() += 1;
        }
    }
    if format == OutputFormat::Text {
        let mut text = format!(
            "Applications: {} shown, {omitted} omitted; accessibility={}\n",
            selected.len(),
            raw["accessibility_trusted"]
        );
        for record in &selected {
            let clean = |key: &str| {
                serde_json::to_string(record[key].as_str().unwrap_or("?"))
                    .expect("string serialization")
            };
            let marker = if record["active"] == true { "*" } else { " " };
            let windows = record["visible_window_ids"]
                .as_array()
                .map(|ids| ids.len().to_string())
                .unwrap_or_else(|| "?".into());
            text.push_str(&format!(
                "{marker} {} {} [{}] {} windows={windows}{}{}\n",
                record["pid"],
                clean("name"),
                clean("bundle_id"),
                clean("activation_policy"),
                if record["system_ui"] == true {
                    " system-ui"
                } else {
                    ""
                },
                if record["hidden"] == true {
                    " hidden"
                } else {
                    ""
                }
            ));
        }
        if !errors.is_empty() {
            text.push_str(&format!(
                "AX state errors across all records: {}\n",
                errors
                    .iter()
                    .map(|(code, count)| format!("{code}={count}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !raw["window_error"].is_null() {
            text.push_str(&format!(
                "Window discovery error: {}\n",
                raw["window_error"]
            ));
        }
        return Value::String(text);
    }
    let mut result = raw.clone();
    result["applications"] = Value::Array(if format == OutputFormat::Compact {
        selected
            .iter()
            .map(|record| {
                let mut projected = serde_json::Map::new();
                for key in [
                    "pid",
                    "name",
                    "bundle_id",
                    "active",
                    "activation_policy",
                    "visible_window_ids",
                    "hidden",
                    "system_ui",
                ] {
                    if let Some(value) = record.get(key) {
                        projected.insert(key.into(), value.clone());
                    }
                }
                Value::Object(projected)
            })
            .collect()
    } else {
        selected
    });
    result["presentation"] =
        json!({"scope":scope,"omitted":omitted,"total":all.len(),"active_error_counts":errors});
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projection_preserves_default_and_retains_native_evidence() {
        let raw = json!({"accessibility_trusted":true,"applications":[
            {"pid":1,"activation_policy":"regular","hidden":true},
            {"pid":2,"activation_policy":"accessory","visible_window_ids":[42],"extra":{"native":true}},
            {"pid":3,"activation_policy":"prohibited","system_ui":true},
            {"pid":4,"activation_policy":"accessory","visible_window_ids":[],"active_error":{"code":"ax_error"}}
        ]});
        assert_eq!(
            present_discovery(&raw, DiscoveryScope::All, OutputFormat::Json),
            raw
        );
        let projected = present_discovery(&raw, DiscoveryScope::Apps, OutputFormat::Json);
        assert_eq!(projected["applications"].as_array().unwrap().len(), 3);
        assert_eq!(
            projected["applications"][1]["extra"],
            json!({"native":true})
        );
        assert_eq!(projected["presentation"]["omitted"], 1);
        assert_eq!(
            projected["presentation"]["active_error_counts"]["ax_error"],
            1
        );
        assert!(!is_app(&json!({"pid":5})));
        let compact = present_discovery(&raw, DiscoveryScope::Apps, OutputFormat::Compact);
        assert!(compact["applications"][1].get("extra").is_none());
        assert_eq!(
            compact["applications"][1]["visible_window_ids"],
            json!([42])
        );
    }
    #[test]
    fn text_keeps_each_application_on_one_line() {
        let raw = json!({"applications":[{"pid":1,"name":"App\nforged\u{001b}","bundle_id":"a\tb","activation_policy":"regular"}]});
        let text = present_discovery(&raw, DiscoveryScope::Apps, OutputFormat::Text);
        let text = text.as_str().unwrap();
        assert!(text.contains(r#""App\nforged\u001b" ["a\tb"]"#));
        assert_eq!(text.lines().count(), 2);
        assert!(!text.contains('\u{001b}'));
    }
}

//! Plain, line-oriented fallback for records without a specialized presentation.
//! Paths keep nested fields unambiguous; control characters cannot inject rows.
use crate::OutputFormat;
use serde_json::Value;
use std::io::{self, Write};

pub fn write_value(mut writer: impl Write, value: &Value, format: OutputFormat) -> io::Result<()> {
    match format {
        OutputFormat::Text => write_text(&mut writer, value, ""),
        OutputFormat::Json | OutputFormat::Compact => {
            serde_json::to_writer(&mut writer, value).map_err(io::Error::other)?;
            writeln!(writer)
        }
    }
}
fn escaped(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_control() || c == '\\' {
                c.escape_default().to_string()
            } else {
                c.to_string()
            }
        })
        .collect()
}
fn write_text(writer: &mut impl Write, value: &Value, path: &str) -> io::Result<()> {
    match value {
        Value::Object(fields) if !fields.is_empty() => {
            for (key, value) in fields {
                let key = escaped(key);
                let next = if path.is_empty() {
                    key
                } else {
                    format!("{path}.{key}")
                };
                write_text(writer, value, &next)?;
            }
        }
        Value::Array(items) if !items.is_empty() => {
            for (index, value) in items.iter().enumerate() {
                write_text(writer, value, &format!("{path}[{index}]"))?;
            }
        }
        _ => {
            let value = match value {
                Value::String(text) => escaped(text),
                _ => value.to_string(),
            };
            if path.is_empty() {
                writeln!(writer, "{value}")?;
            } else {
                writeln!(writer, "{path}\t{value}")?;
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plain_records_escape_controls_and_preserve_paths() {
        let mut output = Vec::new();
        write_value(
            &mut output,
            &serde_json::json!({"devices":[{"name":"a\nb\t"}]}),
            OutputFormat::Text,
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "devices[0].name\ta\\nb\\t\n"
        );
    }
}

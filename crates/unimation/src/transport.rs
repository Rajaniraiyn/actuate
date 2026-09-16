//! Line-oriented JSON session framing shared by every platform session.
//!
//! One request per line, one reply per line, in order. A request's optional
//! `id` is echoed on every valid-JSON request, including operation errors.
//! Parse errors never terminate the session. Platform crates own request
//! vocabularies; this module only owns framing and format-specific projection.
use crate::{
    NativeError, OutputFormat, Result, Snapshot, output,
    presentation::{self, CompactSnapshot},
};
use serde::{Serialize, Serializer};
use serde_json::{Value, json};
use std::{
    cell::OnceCell,
    io::{BufRead, Write},
};

/// A typed session result that the transport can project into text or
/// compact output without re-parsing it.
pub trait Reply: Serialize {
    /// A raw observation, so text and compact sessions can project it.
    fn snapshot(&self) -> Option<&Snapshot> {
        None
    }
    /// Already-rendered text, emitted verbatim in text sessions.
    fn text(&self) -> Option<&str> {
        None
    }
    /// The reply as JSON when it already is one, so text output need not re-encode it.
    fn value(&self) -> Option<&Value> {
        None
    }
}

/// Untyped session results. A snapshot is recognized structurally, and only
/// parsed when an output format actually needs the projection.
pub struct ValueReply {
    value: Value,
    snapshot: OnceCell<Option<Snapshot>>,
}
impl From<Value> for ValueReply {
    fn from(value: Value) -> Self {
        Self {
            value,
            snapshot: OnceCell::new(),
        }
    }
}
impl Serialize for ValueReply {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        self.value.serialize(serializer)
    }
}
impl Reply for ValueReply {
    fn snapshot(&self) -> Option<&Snapshot> {
        self.snapshot
            .get_or_init(|| {
                self.value
                    .get("nodes")
                    .and_then(|_| serde_json::from_value(self.value.clone()).ok())
            })
            .as_ref()
    }
    fn text(&self) -> Option<&str> {
        self.value.as_str()
    }
    fn value(&self) -> Option<&Value> {
        Some(&self.value)
    }
}

/// Removes the correlation id and expands a `"@e<number>"` `target` or
/// `options.root` into a full reference in the given session namespace.
pub fn prepare(value: &mut Value, session: impl Fn() -> Result<String>) -> Result<()> {
    if let Some(object) = value.as_object_mut() {
        object.remove("id");
    }
    for pointer in ["/target", "/options/root"] {
        if let Some(short) = value.pointer(pointer).and_then(Value::as_str) {
            let reference = crate::ElementRef::parse_short(&session()?, short)?;
            *value.pointer_mut(pointer).expect("existing field") = json!(reference);
        }
    }
    Ok(())
}

/// How one reply is presented for an output format.
enum Projection<'a, R> {
    /// Encode the typed reply as is.
    Verbatim(&'a R),
    /// A rendered tree string, or text the reply already carried.
    Text(String),
    Compact(CompactSnapshot),
}
fn project<'a, R: Reply>(reply: &'a R, format: OutputFormat) -> std::io::Result<Projection<'a, R>> {
    if format == OutputFormat::Json {
        return Ok(Projection::Verbatim(reply));
    }
    if let Some(snapshot) = reply.snapshot() {
        let view = presentation::render_snapshot(snapshot, &Default::default())
            .map_err(std::io::Error::other)?;
        return Ok(if format == OutputFormat::Text {
            Projection::Text(presentation::render_snapshot_text(&view))
        } else {
            Projection::Compact(view)
        });
    }
    if format == OutputFormat::Text
        && let Some(text) = reply.text()
    {
        return Ok(Projection::Text(format!("{text}\n")));
    }
    Ok(Projection::Verbatim(reply))
}

#[derive(Serialize)]
struct Envelope<'a, R: Serialize> {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<&'a Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<R>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'a NativeError>,
}

fn write_json(mut writer: impl Write, value: &impl Serialize) -> std::io::Result<()> {
    serde_json::to_writer(&mut writer, value).map_err(std::io::Error::other)?;
    writeln!(writer)
}

/// Writes one reply without an envelope: text sessions print rendered text
/// and tree projections directly, other formats encode the typed result.
pub fn write_reply<R: Reply>(
    reply: &R,
    format: OutputFormat,
    mut writer: impl Write,
) -> std::io::Result<()> {
    match project(reply, format)? {
        Projection::Text(text) => write!(writer, "{text}"),
        Projection::Compact(view) => write_json(writer, &view),
        Projection::Verbatim(reply) if format == OutputFormat::Text => match reply.value() {
            Some(value) => output::write_value(writer, value, format),
            None => {
                let value = serde_json::to_value(reply).map_err(std::io::Error::other)?;
                output::write_value(writer, &value, format)
            }
        },
        Projection::Verbatim(reply) => write_json(writer, reply),
    }
}

/// Serves requests until EOF. `dispatch` receives each parsed request, with
/// its `id` still present so platform code may ignore it.
pub fn serve<R: Reply>(
    reader: impl BufRead,
    mut writer: impl Write,
    format: OutputFormat,
    mut dispatch: impl FnMut(Value) -> Result<R>,
) -> std::io::Result<()> {
    for line in reader.lines() {
        let line = line?;
        let parsed = serde_json::from_str::<Value>(&line);
        let id = parsed.as_ref().ok().and_then(|v| v.get("id")).cloned();
        let result = parsed
            .map_err(NativeError::invalid_request)
            .and_then(&mut dispatch);
        if format == OutputFormat::Text {
            writeln!(
                writer,
                "--- response id={} ---",
                id.as_ref().unwrap_or(&Value::Null)
            )?;
            match &result {
                Ok(reply) => write_reply(reply, format, &mut writer)?,
                Err(error) => output::write_value(&mut writer, &json!({"error":error}), format)?,
            }
            writeln!(writer, "--- end ---")?;
        } else {
            let id = id.as_ref();
            match &result {
                Ok(reply) => match project(reply, format)? {
                    Projection::Compact(view) => write_json(
                        &mut writer,
                        &Envelope {
                            id,
                            result: Some(view),
                            error: None,
                        },
                    )?,
                    _ => write_json(
                        &mut writer,
                        &Envelope {
                            id,
                            result: Some(reply),
                            error: None,
                        },
                    )?,
                },
                Err(error) => write_json(
                    &mut writer,
                    &Envelope::<()> {
                        id,
                        result: None,
                        error: Some(error),
                    },
                )?,
            }
        }
        writer.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ElementRef;
    fn snapshot() -> Value {
        json!({"root":{"session":"s","id":1},"nodes":[{"reference":{"session":"s","id":1},
            "attributes":{"AXRole":"AXWindow","AXTitle":"Hello"},"actions":[],"parameterized_attributes":[],"children":[],"issues":[]}],
            "complete":true,"traversal_complete":true,"revision":1,"issues":[]})
    }
    fn run(format: OutputFormat, input: &str) -> String {
        let mut out = Vec::new();
        serve(input.as_bytes(), &mut out, format, |mut request| {
            prepare(&mut request, || Ok("s".into()))?;
            match request["op"].as_str() {
                Some("observe") => Ok(ValueReply::from(snapshot())),
                Some("text") => Ok(ValueReply::from(json!("rendered"))),
                Some("echo") => Ok(ValueReply::from(request)),
                _ => Err(NativeError::invalid_request("unknown op")),
            }
        })
        .unwrap();
        String::from_utf8(out).unwrap()
    }
    #[test]
    fn json_sessions_echo_ids_and_recover_from_parse_errors() {
        let out = run(
            OutputFormat::Json,
            "nope\n{\"id\":7,\"op\":\"missing\"}\n{\"id\":null,\"op\":\"echo\",\"target\":\"@e3\"}\n",
        );
        let lines: Vec<Value> = out
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines[0]["error"]["code"], "invalid_request");
        assert!(lines[0].get("id").is_none());
        assert_eq!(lines[1]["id"], 7);
        assert_eq!(lines[1]["error"]["code"], "invalid_request");
        assert_eq!(lines[2]["id"], Value::Null);
        assert_eq!(
            lines[2]["result"]["target"],
            json!(ElementRef {
                session: "s".into(),
                id: 3
            })
        );
        assert!(lines[2]["result"].get("id").is_none());
    }
    #[test]
    fn text_and_compact_sessions_project_snapshots() {
        let text = run(
            OutputFormat::Text,
            "{\"op\":\"observe\",\"id\":1}\n{\"op\":\"text\"}\n{\"op\":\"echo\",\"k\":1}\n",
        );
        assert!(text.starts_with("--- response id=1 ---\nsnapshot session=\"s\""));
        assert!(text.contains("- @e1 \"AXWindow\" \"Hello\""));
        assert!(text.contains("--- response id=null ---\nrendered\n--- end ---"));
        assert!(text.contains("k\t1\n"));
        let compact = run(OutputFormat::Compact, "{\"op\":\"observe\"}\n");
        let reply: Value = serde_json::from_str(compact.trim()).unwrap();
        assert!(reply["result"]["rows"].is_array());
        assert!(reply["result"].get("nodes").is_none());
        let json = run(OutputFormat::Json, "{\"op\":\"observe\"}\n");
        let reply: Value = serde_json::from_str(json.trim()).unwrap();
        assert!(reply["result"]["nodes"].is_array());
    }
}

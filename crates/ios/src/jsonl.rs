//! External JSONL adapter. Typed providers never serialize between components.
use crate::session::{self, Request, Response, Session, SessionBackend, fail};
use serde::{Serialize, Serializer};
use serde_json::{Value, json};
use std::{
    io::{BufRead, Write},
    path::Path,
};
use unimation::{ElementRef, NativeError, Result};

// Serialize borrowed typed results directly to the output stream. In particular,
// do not clone snapshots into a second serde_json::Value tree before encoding.
struct Encoded<'a>(&'a Response);
impl Serialize for Encoded<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        match self.0 {
            Response::Snapshot(v)=>v.as_ref().serialize(serializer),
            Response::Text(v)=>v.serialize(serializer),
            Response::Compact(v)=>v.serialize(serializer),
            Response::Query(v)=>v.serialize(serializer),
            Response::Diff(v)=>v.serialize(serializer),
            Response::Receipt(v)=>v.serialize(serializer),
            Response::Frame(v)=>v.serialize(serializer),
            Response::Capabilities=>json!({"observation":"native_accessibility","semantic":"advertised_actions","touch":"normalized_raw_framebuffer","keyboard":"usb_hid_page_07","hardware_buttons":true,"native_mouse_cursor":false,"capture":"device_pixels","click_mapping":null}).serialize(serializer),
        }
    }
}
#[derive(Serialize)]
struct Reply<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<&'a Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Encoded<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'a NativeError>,
}
pub fn dispatch<B: SessionBackend>(session: &mut Session<B>, mut value: Value) -> Result<Response> {
    if let Some(object) = value.as_object_mut() {
        object.remove("id");
    }
    if let Some(short) = value.get("target").and_then(Value::as_str) {
        let id = short
            .strip_prefix("@e")
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(|| fail("invalid_request", "Expected @e followed by an integer"))?;
        value["target"] = json!(ElementRef {
            session: session.snapshot(None)?.root.session.clone(),
            id
        });
    }
    let request: Request =
        serde_json::from_value(value).map_err(|e| fail("invalid_request", e.to_string()))?;
    session.execute(request)
}
/// Platform-owned agent protocol; the umbrella CLI only selects this entry point.
pub fn run(udid: &str, device_set: &Path) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut session = session::connect(udid, device_set)?;
    serve(
        &mut session,
        std::io::stdin().lock(),
        std::io::stdout().lock(),
    )?;
    Ok(())
}
pub fn serve<B: SessionBackend>(
    session: &mut Session<B>,
    reader: impl BufRead,
    mut writer: impl Write,
) -> std::io::Result<()> {
    for line in reader.lines() {
        let line = line?;
        let value = serde_json::from_str::<Value>(&line);
        let id = value.as_ref().ok().and_then(|v| v.get("id")).cloned();
        let result = value
            .map_err(|e| fail("invalid_request", e.to_string()))
            .and_then(|v| dispatch(session, v));
        let reply = match &result {
            Ok(v) => Reply {
                id: id.as_ref(),
                result: Some(Encoded(v)),
                error: None,
            },
            Err(e) => Reply {
                id: id.as_ref(),
                result: None,
                error: Some(e),
            },
        };
        serde_json::to_writer(&mut writer, &reply).map_err(std::io::Error::other)?;
        writer.write_all(b"\n")?;
        writer.flush()?;
    }
    Ok(())
}

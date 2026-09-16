//! External JSONL adapter. Typed providers never serialize between components.
use crate::session::{self, Request, Response, Session, SessionBackend, fail};
use serde::{Serialize, Serializer};
use serde_json::{Value, json};
use std::{
    io::{BufRead, Write},
    path::Path,
};
use unimation::{OutputFormat, Result, Snapshot, transport};

// Serialize borrowed typed results directly to the output stream. In particular,
// do not clone snapshots into a second serde_json::Value tree before encoding.
impl Serialize for Response {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
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
impl transport::Reply for Response {
    fn snapshot(&self) -> Option<&Snapshot> {
        match self {
            Response::Snapshot(snapshot) => Some(snapshot),
            _ => None,
        }
    }
    fn text(&self) -> Option<&str> {
        match self {
            Response::Text(text) => Some(text),
            _ => None,
        }
    }
}
pub fn dispatch<B: SessionBackend>(session: &mut Session<B>, mut value: Value) -> Result<Response> {
    transport::prepare(&mut value, || {
        Ok(session.snapshot(None)?.root.session.clone())
    })?;
    let request: Request =
        serde_json::from_value(value).map_err(|e| fail("invalid_request", e.to_string()))?;
    session.execute(request)
}
/// Platform-owned agent protocol; the umbrella CLI only selects this entry point.
pub fn run(udid: &str, device_set: &Path) -> std::result::Result<(), Box<dyn std::error::Error>> {
    run_formatted(udid, device_set, OutputFormat::Json)
}
pub fn run_formatted(
    udid: &str,
    device_set: &Path,
    format: OutputFormat,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut session = session::connect(udid, device_set)?;
    serve_formatted(
        &mut session,
        std::io::stdin().lock(),
        std::io::stdout().lock(),
        format,
    )?;
    Ok(())
}
/// Shared one-shot command output, without a JSONL response envelope.
pub fn write_result_formatted(
    response: &Response,
    format: OutputFormat,
    mut writer: impl Write,
) -> std::io::Result<()> {
    transport::write_reply(response, format, &mut writer)?;
    writer.flush()
}
pub fn serve<B: SessionBackend>(
    session: &mut Session<B>,
    reader: impl BufRead,
    writer: impl Write,
) -> std::io::Result<()> {
    serve_formatted(session, reader, writer, OutputFormat::Json)
}
pub fn serve_formatted<B: SessionBackend>(
    session: &mut Session<B>,
    reader: impl BufRead,
    writer: impl Write,
    format: OutputFormat,
) -> std::io::Result<()> {
    transport::serve(reader, writer, format, |request| dispatch(session, request))
}

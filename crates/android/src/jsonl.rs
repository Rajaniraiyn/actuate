//! External session framing. Typed Android providers never serialize between layers.
use crate::{Android, CommandTransport, error, session};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    io::{BufRead, Write},
    path::{Path, PathBuf},
};
use unimation::{Effect, OutputFormat, Result, transport::ValueReply};

pub fn capture_file<D: CommandTransport>(device: &mut Android<D>, path: &Path) -> Result<Value> {
    let bytes = device.capture_png()?;
    let (width, height) = unimation::image::png_dimensions(&bytes)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| error("capture_output", e, Effect::None))?;
    if let Err(e) = file.write_all(&bytes) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(error("capture_output", e, Effect::None));
    }
    Ok(
        json!({"path":path,"format":"png","pixel_width":width,"pixel_height":height,"encoded_bytes":bytes.len(),"click_mapping":null}),
    )
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum CaptureOp {
    Capture,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CaptureRequest {
    op: CaptureOp,
    path: PathBuf,
}
fn dispatch<D: CommandTransport>(device: &mut Android<D>, mut request: Value) -> Result<Value> {
    if let Some(map) = request.as_object_mut() {
        map.remove("id");
    }
    if request.get("op").and_then(Value::as_str) == Some("capture") {
        let request: CaptureRequest = serde_json::from_value(request)
            .map_err(|e| error("invalid_request", e, Effect::None))?;
        let CaptureOp::Capture = request.op;
        return capture_file(device, &request.path);
    }
    let request: session::Request =
        serde_json::from_value(request).map_err(|e| error("invalid_request", e, Effect::None))?;
    serde_json::to_value(session::execute(device, request)?)
        .map_err(|e| error("encode_response", e, Effect::None))
}
pub fn serve<D: CommandTransport>(
    device: &mut Android<D>,
    reader: impl BufRead,
    writer: impl Write,
    format: OutputFormat,
) -> std::io::Result<()> {
    unimation::transport::serve(reader, writer, format, |request| {
        dispatch(device, request).map(ValueReply::from)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    struct NoCalls;
    impl CommandTransport for NoCalls {
        fn execute(&mut self, _: &str, _: &mut dyn Write, _: &mut dyn Write) -> Result<Option<u8>> {
            panic!("invalid requests must not reach a device");
        }
    }
    #[test]
    fn invalid_requests_keep_session_alive_and_do_not_dispatch() {
        let mut device = Android::new(NoCalls);
        let input=b"not-json\n{\"id\":2,\"op\":\"capture\"}\n{\"id\":3,\"op\":\"tap\",\"point\":{\"x\":1,\"y\":2},\"extra\":true}\n";
        let mut output = Vec::new();
        serve(&mut device, &input[..], &mut output, OutputFormat::Json).unwrap();
        let replies: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(replies.len(), 3);
        assert!(
            replies
                .iter()
                .all(|reply| reply["error"]["code"] == "invalid_request")
        );
        assert_eq!(replies[1]["id"], 2);
        assert_eq!(replies[2]["id"], 3);
    }
}

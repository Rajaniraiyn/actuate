//! Direct Android ADB connections and independently replaceable command transport.
//! No host adb executable is started. See the README for pairing and timeout limits.
use serde::Serialize;
pub mod apps;
pub mod cursor;
pub mod hid;
pub mod jsonl;
pub mod qr;
pub mod session;
pub mod wireless;
use std::{io::Write, time::Duration};
use unimation::{Effect, NativeError, Receipt, Result};
fn error(code: &str, message: impl ToString, effect: Effect) -> NativeError {
    NativeError {
        code: code.into(),
        message: message.to_string(),
        effect,
    }
}
/// A selected device's command channel. Implementations retain their own connections.
pub trait CommandTransport {
    fn execute(
        &mut self,
        command: &str,
        stdout: &mut dyn Write,
        stderr: &mut dyn Write,
    ) -> Result<Option<u8>>;
}
/// Borrow an existing session when composing observation and input adapters.
impl<T: CommandTransport + ?Sized> CommandTransport for &mut T {
    fn execute(
        &mut self,
        command: &str,
        stdout: &mut dyn Write,
        stderr: &mut dyn Write,
    ) -> Result<Option<u8>> {
        (**self).execute(command, stdout, stderr)
    }
}
mod connection;
mod direct;
#[cfg(feature = "usb")]
pub use direct::discover_usb;
pub use direct::{DirectDevice, UsbDevice, init_key};
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PixelPoint {
    pub x: u32,
    pub y: u32,
}
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AndroidKey {
    Home,
    Back,
    Recents,
    Enter,
    Delete,
    VolumeUp,
    VolumeDown,
    Power,
}
impl AndroidKey {
    fn code(self) -> u16 {
        match self {
            Self::Home => 3,
            Self::Back => 4,
            Self::Recents => 187,
            Self::Enter => 66,
            Self::Delete => 67,
            Self::VolumeUp => 24,
            Self::VolumeDown => 25,
            Self::Power => 26,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct DeviceInfo {
    pub manufacturer: String,
    pub model: String,
    pub android_release: String,
    pub sdk: String,
}
pub struct Android<D> {
    transport: D,
}
impl<D> Android<D> {
    pub fn new(transport: D) -> Self {
        Self { transport }
    }
    pub fn into_transport(self) -> D {
        self.transport
    }
}
impl<D: CommandTransport> Android<D> {
    fn run(&mut self, command: &str, limit: usize) -> Result<Vec<u8>> {
        let mut stdout = LimitedOutput::new(limit);
        let mut stderr = LimitedOutput::new(64 * 1024);
        let status = self.transport.execute(command, &mut stdout, &mut stderr)?;
        if status != Some(0) {
            return Err(error(
                "android_command",
                format!(
                    "exit status {status:?}: {}",
                    String::from_utf8_lossy(&stderr.bytes)
                ),
                Effect::Unknown,
            ));
        }
        Ok(stdout.bytes)
    }
    pub fn info(&mut self) -> Result<DeviceInfo> {
        fn property<D: CommandTransport>(device: &mut Android<D>, command: &str) -> Result<String> {
            String::from_utf8(device.run(command, 4096)?)
                .map(|s| s.trim().to_owned())
                .map_err(|e| error("android_encoding", e, Effect::None))
        }
        Ok(DeviceInfo {
            manufacturer: property(self, "getprop ro.product.manufacturer")?,
            model: property(self, "getprop ro.product.model")?,
            android_release: property(self, "getprop ro.build.version.release")?,
            sdk: property(self, "getprop ro.build.version.sdk")?,
        })
    }
    pub fn capture_png(&mut self) -> Result<Vec<u8>> {
        let bytes = self.run("screencap -p", 64 * 1024 * 1024)?;
        if bytes.len() < 33
            || !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
            || &bytes[12..16] != b"IHDR"
            || bytes[8..12] != 13u32.to_be_bytes()
            || bytes[16..20] == [0; 4]
            || bytes[20..24] == [0; 4]
        {
            return Err(error(
                "android_capture",
                "device did not return a PNG",
                Effect::None,
            ));
        }
        Ok(bytes)
    }
    fn input(&mut self, command: &str) -> Result<Receipt> {
        self.run(command, 64 * 1024)?;
        Ok(Receipt {
            effect: Effect::Dispatched,
            route: "android.adb.shell.input".into(),
        })
    }
    pub fn tap(&mut self, point: PixelPoint) -> Result<Receipt> {
        self.input(&format!("input tap {} {}", point.x, point.y))
    }
    pub fn swipe(
        &mut self,
        from: PixelPoint,
        to: PixelPoint,
        duration: Duration,
    ) -> Result<Receipt> {
        let millis = duration.as_millis();
        if !(1..=60_000).contains(&millis) {
            return Err(error(
                "android_duration",
                "swipe duration must be 1..60000 ms",
                Effect::None,
            ));
        }
        self.input(&format!(
            "input swipe {} {} {} {} {millis}",
            from.x, from.y, to.x, to.y
        ))
    }
    pub fn key(&mut self, key: AndroidKey) -> Result<Receipt> {
        self.input(&format!("input keyevent {}", key.code()))
    }
    /// Android's input text command lacks general Unicode and interprets %s specially.
    pub fn type_ascii(&mut self, text: &str) -> Result<Receipt> {
        if text.is_empty()
            || text.len() > 4096
            || !text.bytes().all(|c| (32..=126).contains(&c))
            || text.contains('%')
        {
            return Err(error(
                "android_text",
                "input text accepts 1..4096 printable ASCII bytes without percent; Unicode requires another input provider",
                Effect::None,
            ));
        }
        let quoted = text.replace(' ', "%s").replace('\'', "'\\''");
        self.input(&format!("input text '{quoted}'"))
    }
}
struct LimitedOutput {
    bytes: Vec<u8>,
    limit: usize,
}
impl LimitedOutput {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }
}
impl Write for LimitedOutput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(std::io::Error::other("Android output limit exceeded"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl<D: CommandTransport> unimation::Capture for Android<D> {
    type Request = ();
    type Frame = Vec<u8>;
    fn capture(&mut self, _: ()) -> Result<Vec<u8>> {
        self.capture_png()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Mock {
        commands: Vec<String>,
        status: Option<u8>,
    }
    impl CommandTransport for Mock {
        fn execute(
            &mut self,
            c: &str,
            _out: &mut dyn Write,
            _err: &mut dyn Write,
        ) -> Result<Option<u8>> {
            self.commands.push(c.into());
            Ok(self.status)
        }
    }
    #[test]
    fn text_is_quoted_and_unsupported_input_never_dispatches() {
        let mut a = Android::new(Mock {
            status: Some(0),
            ..Default::default()
        });
        a.type_ascii("a'$(touch x) b").unwrap();
        assert_eq!(a.transport.commands[0], "input text 'a'\\''$(touch%sx)%sb'");
        assert!(a.type_ascii("%s").is_err());
        assert!(a.type_ascii("🙂").is_err());
        assert_eq!(a.transport.commands.len(), 1);
    }
    #[test]
    fn unknown_exit_status_does_not_claim_success() {
        assert!(Android::new(Mock::default()).key(AndroidKey::Home).is_err());
    }
    #[test]
    fn output_is_bounded() {
        let mut w = LimitedOutput::new(3);
        assert!(w.write_all(b"four").is_err());
        assert!(w.bytes.is_empty());
    }
    #[test]
    fn invalid_duration_does_not_dispatch() {
        let mut a = Android::new(Mock::default());
        assert!(
            a.swipe(
                PixelPoint { x: 0, y: 0 },
                PixelPoint { x: 1, y: 1 },
                Duration::ZERO
            )
            .is_err()
        );
        assert!(a.transport.commands.is_empty());
    }
}

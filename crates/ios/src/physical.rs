//! Physical-device services through usbmuxd. CoreSimulator uses a separate transport.
//! Discovery never pairs devices. Capture starts an existing screenshotr service but
//! never mounts developer images, installs software, or starts XCTest.
use actuate::{Capture, Effect, NativeError, Result};
use idevice::{
    IdeviceError, IdeviceService,
    provider::{IdeviceProvider, UsbmuxdProvider},
    services::{lockdown::LockdownClient, screenshotr::ScreenshotService},
    usbmuxd::{Connection, UsbmuxdAddr, UsbmuxdDevice},
};
use serde::Serialize;
use std::{
    future::Future,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "address", rename_all = "snake_case")]
pub enum ConnectionKind {
    Usb,
    Network(std::net::IpAddr),
    Unknown(String),
}
#[derive(Debug, Clone, Serialize)]
pub struct Device {
    pub udid: String,
    /// Ephemeral transport identifier, never an element or session identity.
    pub transport_id: u32,
    pub connection: ConnectionKind,
}
impl From<UsbmuxdDevice> for Device {
    fn from(d: UsbmuxdDevice) -> Self {
        Self {
            udid: d.udid,
            transport_id: d.device_id,
            connection: match d.connection_type {
                Connection::Usb => ConnectionKind::Usb,
                Connection::Network(ip) => ConnectionKind::Network(ip),
                Connection::Unknown(s) => ConnectionKind::Unknown(s),
            },
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Capabilities {
    pub discovery: bool,
    pub device_info: bool,
    pub capture_route: &'static str,
    pub capture_availability: &'static str,
    pub capture_requirement: &'static str,
    pub observation: bool,
    pub input: bool,
    pub streaming: bool,
}
pub fn capabilities() -> Capabilities {
    Capabilities {
        discovery: true,
        device_info: true,
        capture_route: "screenshotr",
        capture_availability: "unprobed",
        capture_requirement: "Existing pairing and screenshotr service; upstream documents this lockdownd route for iOS below 17. No developer image is mounted automatically.",
        observation: false,
        input: false,
        streaming: false,
    }
}
/// Read-only lockdownd evidence. Values do not establish pairing or UI support.
#[derive(Debug, Serialize)]
pub struct DeviceInfo {
    pub device: Device,
    pub name: Option<String>,
    pub product_type: Option<String>,
    pub product_version: Option<String>,
    /// Preserve native values (including data, dates and integer widths) without
    /// forcing them through JSON. The convenience fields above can be absent.
    pub properties_plist_xml: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FrameMetadata {
    pub format: String,
    pub pixel_width: Option<usize>,
    pub pixel_height: Option<usize>,
    /// Encoded pixels are unchanged; orientation has not been verified against input.
    pub orientation: Option<String>,
    pub click_mapping: Option<()>,
    pub encoded_bytes: usize,
}
#[derive(Debug)]
pub struct EncodedFrame {
    pub bytes: Vec<u8>,
    pub metadata: FrameMetadata,
}
#[derive(Debug, Serialize)]
pub struct SavedFrame {
    pub path: PathBuf,
    #[serde(flatten)]
    pub metadata: FrameMetadata,
}
impl EncodedFrame {
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        let dimensions = imagesize::blob_size(&bytes).ok();
        let format = imagesize::image_type(&bytes)
            .map(|f| format!("{f:?}").to_lowercase())
            .unwrap_or_else(|_| "unknown".into());
        let metadata = FrameMetadata {
            format,
            pixel_width: dimensions.map(|d| d.width),
            pixel_height: dimensions.map(|d| d.height),
            orientation: None,
            click_mapping: None,
            encoded_bytes: bytes.len(),
        };
        Self { bytes, metadata }
    }
    /// Preserve the service's original encoding. Refuse to overwrite an existing file.
    pub fn save(self, path: impl AsRef<Path>) -> Result<SavedFrame> {
        use std::io::Write;
        let path = path.as_ref();
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| fail("capture_output", e.to_string()))?;
        file.write_all(&self.bytes)
            .map_err(|e| fail("capture_output", e.to_string()))?;
        Ok(SavedFrame {
            path: path.to_owned(),
            metadata: self.metadata,
        })
    }
}
#[derive(Debug, Serialize)]
pub struct DiscoveryReport {
    pub devices: Vec<Device>,
    pub complete: bool,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PhysicalDevices {
    address: UsbmuxdAddr,
    timeout: Duration,
}
impl PhysicalDevices {
    pub fn new(address: UsbmuxdAddr, timeout: Duration) -> Result<Self> {
        if timeout.is_zero() {
            return Err(fail("invalid_timeout", "Timeout must be positive"));
        }
        Ok(Self { address, timeout })
    }
    /// Honors upstream USBMUXD_SOCKET_ADDRESS; does not start a daemon.
    pub fn from_env(timeout: Duration) -> Result<Self> {
        Self::new(
            UsbmuxdAddr::from_env_var()
                .map_err(|e| fail("invalid_usbmuxd_address", e.to_string()))?,
            timeout,
        )
    }
    pub async fn discover(&self) -> Result<DiscoveryReport> {
        deadline(self.timeout, "discovery", async {
            let mut connection = self.address.connect(0).await.map_err(|e| service_error("usbmuxd_unavailable", e))?;
            Ok(DiscoveryReport { devices: connection.get_devices().await.map_err(|e| service_error("device_discovery", e))?.into_iter().map(Device::from).collect(), complete: false, issues: vec!["Upstream usbmuxd parsing may omit malformed device records; discovery does not probe pairing or service availability".into()] })
        }).await
    }
    /// Query existing lockdownd values without pairing, starting a session,
    /// mounting a developer image, or installing a runner.
    pub async fn info(&self, udid: &str) -> Result<DeviceInfo> {
        require_udid(udid)?;
        deadline(self.timeout, "device_info", async {
            let devices = self.discover().await?;
            let device = select_device(&devices.devices, udid)?.clone();
            let provider = self.provider(&device);
            read_info(&provider, device).await
        })
        .await
    }
    fn provider(&self, device: &Device) -> UsbmuxdProvider {
        UsbmuxdProvider {
            addr: self.address.clone(),
            tag: 0,
            udid: device.udid.clone(),
            device_id: device.transport_id,
            label: "actuate".into(),
        }
    }
    pub async fn screenshot(&self, udid: &str) -> Result<EncodedFrame> {
        require_udid(udid)?;
        deadline(self.timeout, "capture", async {
            // Resolve transport IDs again on each operation; reconnect can change them.
            let devices = self.discover().await?;
            let device = select_device(&devices.devices, udid)?;
            let provider = self.provider(device);
            let mut screenshot = ScreenshotService::connect(&provider)
                .await
                .map_err(|e| service_error("screenshot_service_unavailable", e))?;
            let bytes = screenshot
                .take_screenshot()
                .await
                .map_err(|e| service_error("screenshot_failed", e))?;
            Ok(EncodedFrame::from_bytes(bytes))
        })
        .await
    }
}
fn require_udid(udid: &str) -> Result<()> {
    if udid.trim().is_empty() {
        Err(fail(
            "device_required",
            "An explicit nonempty UDID is required",
        ))
    } else {
        Ok(())
    }
}
async fn read_info(provider: &dyn IdeviceProvider, device: Device) -> Result<DeviceInfo> {
    let mut client = LockdownClient::connect(provider)
        .await
        .map_err(|e| service_error("lockdown_unavailable", e))?;
    let values = client
        .get_value(None, None)
        .await
        .map_err(|e| service_error("device_info_unavailable", e))?;
    let dictionary = values.as_dictionary().ok_or_else(|| {
        fail(
            "invalid_device_info",
            "Lockdownd returned non-dictionary device values",
        )
    })?;
    let string = |key: &str| {
        dictionary
            .get(key)
            .and_then(|v| v.as_string())
            .map(str::to_owned)
    };
    let name = string("DeviceName");
    let product_type = string("ProductType");
    let product_version = string("ProductVersion");
    let mut encoded = Vec::new();
    values
        .to_writer_xml(&mut encoded)
        .map_err(|e| fail("device_info_encoding", e.to_string()))?;
    let properties_plist_xml =
        String::from_utf8(encoded).map_err(|e| fail("device_info_encoding", e.to_string()))?;
    Ok(DeviceInfo {
        device,
        name,
        product_type,
        product_version,
        properties_plist_xml,
    })
}
fn select_device<'a>(devices: &'a [Device], udid: &str) -> Result<&'a Device> {
    let mut matches = devices.iter().filter(|d| d.udid == udid);
    let first = matches.next().ok_or_else(|| {
        fail(
            "device_not_found",
            format!("No connected physical device matches {udid}"),
        )
    })?;
    if matches.next().is_some() {
        return Err(fail(
            "ambiguous_device_transport",
            "UDID has multiple active transports; select one using a dedicated usbmuxd endpoint",
        ));
    }
    Ok(first)
}
async fn deadline<T>(
    timeout: Duration,
    operation: &str,
    future: impl Future<Output = Result<T>>,
) -> Result<T> {
    tokio::time::timeout(timeout, future).await.map_err(|_| {
        fail(
            "device_timeout",
            format!("{operation} exceeded {} ms", timeout.as_millis()),
        )
    })?
}
fn fail(code: &str, message: impl Into<String>) -> NativeError {
    NativeError {
        code: code.into(),
        message: message.into(),
        effect: Effect::None,
    }
}
fn service_error(fallback: &str, error: IdeviceError) -> NativeError {
    let code = match &error {
        IdeviceError::InvalidHostID => "device_unpaired",
        IdeviceError::DeviceLocked => "device_locked",
        IdeviceError::DeveloperModeNotEnabled => "developer_mode_disabled",
        IdeviceError::ImageNotMounted => "developer_image_not_mounted",
        IdeviceError::DeviceNotFound | IdeviceError::NoEstablishedConnection => {
            "device_disconnected"
        }
        IdeviceError::ServiceNotFound => fallback,
        IdeviceError::Timeout => "device_timeout",
        _ => fallback,
    };
    fail(code, error.to_string())
}
/// Synchronous adapter with one runtime per adapter, rather than per capture.
/// Async applications should use `PhysicalDevices` directly.
pub struct PhysicalCapture {
    runtime: Option<tokio::runtime::Runtime>,
    devices: PhysicalDevices,
}
impl PhysicalCapture {
    pub fn from_env(timeout: Duration) -> Result<Self> {
        if tokio::runtime::Handle::try_current().is_ok() {
            return Err(fail(
                "async_runtime_active",
                "Use PhysicalDevices inside async applications",
            ));
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| fail("runtime_unavailable", e.to_string()))?;
        Ok(Self {
            runtime: Some(runtime),
            devices: PhysicalDevices::from_env(timeout)?,
        })
    }
    fn check_runtime(&self) -> Result<()> {
        if tokio::runtime::Handle::try_current().is_ok() {
            Err(fail(
                "async_runtime_active",
                "Use PhysicalDevices inside async applications",
            ))
        } else {
            Ok(())
        }
    }
    pub fn info(&self, udid: &str) -> Result<DeviceInfo> {
        self.check_runtime()?;
        self.runtime
            .as_ref()
            .expect("runtime owned until drop")
            .block_on(self.devices.info(udid))
    }
    pub fn discover(&self) -> Result<DiscoveryReport> {
        self.check_runtime()?;
        self.runtime
            .as_ref()
            .expect("runtime owned until drop")
            .block_on(self.devices.discover())
    }
}
impl Drop for PhysicalCapture {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}
impl Capture for PhysicalCapture {
    type Request = String;
    type Frame = EncodedFrame;
    fn capture(&mut self, udid: String) -> Result<EncodedFrame> {
        self.check_runtime()?;
        self.runtime
            .as_ref()
            .expect("runtime owned until drop")
            .block_on(self.devices.screenshot(&udid))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }
    #[derive(Debug)]
    struct InfoProvider(std::net::SocketAddr);
    impl IdeviceProvider for InfoProvider {
        fn connect(
            &self,
            port: u16,
        ) -> std::pin::Pin<
            Box<dyn Future<Output = std::result::Result<idevice::Idevice, IdeviceError>> + Send>,
        > {
            assert_eq!(port, LockdownClient::LOCKDOWND_PORT);
            let address = self.0;
            Box::pin(async move {
                let stream = tokio::net::TcpStream::connect(address).await?;
                Ok(idevice::Idevice::new(Box::new(stream), "actuate-test"))
            })
        }
        fn label(&self) -> &str {
            "actuate-test"
        }
        fn get_pairing_file(
            &self,
        ) -> std::pin::Pin<
            Box<
                dyn Future<
                        Output = std::result::Result<
                            idevice::pairing_file::PairingFile,
                            IdeviceError,
                        >,
                    > + Send,
            >,
        > {
            panic!("read-only info must not request pairing")
        }
    }
    #[test]
    fn device_info_preserves_native_values_without_pairing() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut header = [0; 4];
            stream.read_exact(&mut header).unwrap();
            let length = u32::from_be_bytes(header) as usize;
            assert!(length < 8192);
            let mut request = vec![0; length];
            stream.read_exact(&mut request).unwrap();
            let text = String::from_utf8(request).unwrap();
            assert!(text.contains("GetValue"));
            assert!(!text.contains("StartSession"));
            let response = br#"<?xml version="1.0"?><plist version="1.0"><dict><key>Value</key><dict><key>DeviceName</key><string>Test phone</string><key>ProductType</key><string>iPhone17,1</string><key>ProductVersion</key><string>18.0</string><key>ExtraData</key><data>AQID</data><key>ExtraDate</key><date>2026-01-01T00:00:00Z</date></dict></dict></plist>"#;
            stream
                .write_all(&(response.len() as u32).to_be_bytes())
                .unwrap();
            stream.write_all(response).unwrap();
            let mut next = [0];
            assert_eq!(stream.read(&mut next).unwrap(), 0);
        });
        let device = Device {
            udid: "test".into(),
            transport_id: 3,
            connection: ConnectionKind::Usb,
        };
        let info = runtime()
            .block_on(read_info(&InfoProvider(address), device))
            .unwrap();
        assert_eq!(info.name.as_deref(), Some("Test phone"));
        assert_eq!(info.product_version.as_deref(), Some("18.0"));
        assert!(info.properties_plist_xml.contains("<data>"));
        assert!(info.properties_plist_xml.contains("AQID"));
        assert!(
            info.properties_plist_xml
                .contains("<date>2026-01-01T00:00:00Z</date>")
        );
        server.join().unwrap();
    }
    #[test]
    fn empty_identity_is_rejected_before_transport() {
        let devices = PhysicalDevices::new(
            UsbmuxdAddr::TcpSocket("127.0.0.1:1".parse().unwrap()),
            Duration::from_secs(1),
        )
        .unwrap();
        assert_eq!(
            runtime().block_on(devices.info(" ")).unwrap_err().code,
            "device_required"
        );
    }
    #[test]
    fn exact_identity_and_ambiguity() {
        let device = Device {
            udid: "one".into(),
            transport_id: 1,
            connection: ConnectionKind::Usb,
        };
        assert_eq!(
            select_device(std::slice::from_ref(&device), "two")
                .unwrap_err()
                .code,
            "device_not_found"
        );
        assert_eq!(
            select_device(&[device.clone(), device], "one")
                .unwrap_err()
                .code,
            "ambiguous_device_transport"
        );
    }
    #[test]
    fn deadline_cancels_pending_transport() {
        let result: Result<()> = runtime().block_on(deadline(
            Duration::from_millis(5),
            "test",
            std::future::pending(),
        ));
        assert_eq!(result.unwrap_err().code, "device_timeout");
    }
    #[test]
    fn unknown_encoding_is_preserved_without_mapping() {
        let bytes = vec![1, 2, 3];
        let frame = EncodedFrame::from_bytes(bytes.clone());
        assert_eq!(frame.bytes, bytes);
        assert_eq!(frame.metadata.format, "unknown");
        assert!(frame.metadata.pixel_width.is_none());
        assert!(frame.metadata.click_mapping.is_none());
    }
    #[test]
    fn png_dimensions_are_not_an_input_transform() {
        let bytes = [
            b"\x89PNG\r\n\x1a\n".as_slice(),
            &[0, 0, 0, 13],
            b"IHDR",
            &640_u32.to_be_bytes(),
            &480_u32.to_be_bytes(),
            &[8, 2, 0, 0, 0],
        ]
        .concat();
        let frame = EncodedFrame::from_bytes(bytes);
        assert_eq!(frame.metadata.pixel_width, Some(640));
        assert_eq!(frame.metadata.pixel_height, Some(480));
        assert!(frame.metadata.orientation.is_none());
    }
    #[test]
    fn sync_adapter_rejects_nested_runtime() {
        runtime().block_on(async { assert!(matches!(PhysicalCapture::from_env(Duration::from_secs(1)), Err(e) if e.code == "async_runtime_active")); });
    }
    #[test]
    fn sync_adapter_can_be_dropped_inside_runtime() {
        let adapter = PhysicalCapture::from_env(Duration::from_secs(1)).unwrap();
        runtime().block_on(async move {
            drop(adapter);
        });
    }
    #[test]
    fn discovery_only_sends_list_devices() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut header = [0u8; 16];
            stream.read_exact(&mut header).unwrap();
            let length = u32::from_le_bytes(header[..4].try_into().unwrap()) as usize;
            let mut request = vec![0; length - 16];
            stream.read_exact(&mut request).unwrap();
            assert!(String::from_utf8_lossy(&request).contains("ListDevices"));
            let body = br#"<?xml version="1.0"?><plist version="1.0"><dict><key>DeviceList</key><array><dict><key>DeviceID</key><integer>9</integer><key>Properties</key><dict><key>ConnectionType</key><string>USB</string><key>SerialNumber</key><string>test-udid</string></dict></dict></array></dict></plist>"#;
            header[..4].copy_from_slice(&((body.len() + 16) as u32).to_le_bytes());
            stream.write_all(&header).unwrap();
            stream.write_all(body).unwrap();
            let mut next = [0; 1];
            assert_eq!(stream.read(&mut next).unwrap(), 0);
        });
        let device =
            PhysicalDevices::new(UsbmuxdAddr::TcpSocket(address), Duration::from_secs(2)).unwrap();
        let devices = runtime().block_on(device.discover()).unwrap();
        assert_eq!(devices.devices.len(), 1);
        assert_eq!(devices.devices[0].udid, "test-udid");
        assert!(!devices.complete);
        server.join().unwrap();
    }
}

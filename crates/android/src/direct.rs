//! Direct transports share the same protocol, authentication and shell engine as TLS.
use crate::{
    CommandTransport,
    connection::{Connection, Runtime},
    error,
};
use actuate::{Effect, Result};
use droidmux::{auth::RsaAdbCredential, client::AdbClient};
use serde::Serialize;
use std::{io::Write, net::SocketAddr, path::Path, sync::Arc, time::Duration};
const TIMEOUT: Duration = Duration::from_secs(30);

pub struct DirectDevice(Connection);
impl DirectDevice {
    /// Legacy RSA-authenticated adbd TCP. Android wireless debugging uses WirelessHost.
    pub fn tcp(address: SocketAddr, key: &Path) -> Result<Self> {
        let credential = load_key(key)?;
        let runtime = Runtime::new()?;
        let client = runtime.run(TIMEOUT, Effect::None, async {
            let transport = droidmux::tcp::TcpTransport::connect(address, Default::default())
                .await
                .map_err(|e| error("android_connect", e, Effect::None))?;
            AdbClient::connect(Box::new(transport), Arc::new(credential))
                .await
                .map_err(|e| error("android_connect", e, Effect::None))
        })?;
        Ok(Self(Connection {
            client,
            runtime,
            timeout: TIMEOUT,
        }))
    }
    #[cfg(feature = "usb")]
    pub fn usb(vendor: u16, product: u16, key: &Path) -> Result<Self> {
        let credential = load_key(key)?;
        let mut matches = droidmux::usb::UsbTransport::discover()
            .map_err(|e| error("android_usb_discovery", e, Effect::None))?
            .into_iter()
            .filter(|d| d.vendor_id == vendor && d.product_id == product);
        let selected = matches.next().ok_or_else(|| {
            error(
                "android_device_selection",
                "No matching USB device",
                Effect::None,
            )
        })?;
        if matches.next().is_some() {
            return Err(error(
                "android_device_selection",
                "Multiple USB devices match this vendor/product; select a unique attachment",
                Effect::None,
            ));
        }
        let runtime = Runtime::new()?;
        let client = runtime.run(TIMEOUT, Effect::None, async {
            let transport =
                droidmux::usb::UsbTransport::connect_device(&selected, Default::default())
                    .await
                    .map_err(|e| error("android_connect", e, Effect::None))?;
            AdbClient::connect(Box::new(transport), Arc::new(credential))
                .await
                .map_err(|e| error("android_connect", e, Effect::None))
        })?;
        Ok(Self(Connection {
            client,
            runtime,
            timeout: TIMEOUT,
        }))
    }
}
impl CommandTransport for DirectDevice {
    fn execute(
        &mut self,
        command: &str,
        stdout: &mut dyn Write,
        stderr: &mut dyn Write,
    ) -> Result<Option<u8>> {
        self.0.execute(command, stdout, stderr)
    }
}
fn load_key(path: &Path) -> Result<RsaAdbCredential> {
    let pem = std::fs::read_to_string(path).map_err(|e| error("android_key", e, Effect::None))?;
    RsaAdbCredential::from_pkcs8_pem(&pem, "actuate")
        .map_err(|e| error("android_key", e, Effect::None))
}
/// Create a persistent host identity without replacing an existing file.
pub fn init_key(path: &Path) -> Result<()> {
    let key =
        RsaAdbCredential::generate("actuate").map_err(|e| error("android_key", e, Effect::None))?;
    let pem = key
        .to_pkcs8_pem()
        .map_err(|e| error("android_key", e, Effect::None))?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|e| error("android_key", e, Effect::None))?;
    file.write_all(pem.as_bytes())
        .map_err(|e| error("android_key", e, Effect::Unknown))
}
#[derive(Debug, Serialize)]
pub struct UsbDevice {
    pub vendor_id: u16,
    pub product_id: u16,
    pub bus_number: u8,
    pub address: u8,
    pub description: String,
}
#[cfg(feature = "usb")]
pub fn discover_usb() -> Result<Vec<UsbDevice>> {
    droidmux::usb::UsbTransport::discover()
        .map(|ds| {
            ds.into_iter()
                .map(|d| UsbDevice {
                    vendor_id: d.vendor_id,
                    product_id: d.product_id,
                    bus_number: d.bus_number,
                    address: d.address,
                    description: d.description,
                })
                .collect()
        })
        .map_err(|e| error("android_usb_discovery", e, Effect::None))
}

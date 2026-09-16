//! Android Wireless debugging QR credentials and terminal rendering.
//!
//! The phone starts a pairing server with the requested service instance name.
//! Browse `_adb-tls-pairing._tcp.local.` and match that name before pairing.
//! The command connection uses the separate `_adb-tls-connect._tcp.local.`
//! service; its port must not be inferred from the pairing port.
//!
//! Protocol: <https://android.googlesource.com/platform/packages/modules/adb/+/HEAD/docs/dev/adb_wifi.md>
//! Scanner: <https://android.googlesource.com/platform/packages/apps/Settings/+/fcdcf5a0a11196df094f2297e4be5ad9c0fc53fb/src/com/android/settings/development/AdbQrcodeScannerFragment.java>
use actuate::{Effect, Result};
use mdns_sd::{Receiver, RecvTimeoutError, ScopedIp, ServiceDaemon, ServiceEvent};
use qrcode::{QrCode, render::unicode::Dense1x2};
use rand::RngCore;
use std::{
    net::{SocketAddr, SocketAddrV6},
    time::{Duration, Instant},
};

/// Fresh credentials for one explicit pairing attempt. No Debug or Serialize
/// implementation, so normal diagnostics cannot accidentally print the secret.
/// The caller owns display duration and pairing deadlines.
pub struct QrPairing {
    service_name: String,
    password: String,
}
impl QrPairing {
    pub fn generate() -> Result<Self> {
        let mut random = [0u8; 21];
        rand::rngs::OsRng.try_fill_bytes(&mut random).map_err(|_| {
            crate::error(
                "android_qr_random",
                "OS randomness unavailable",
                Effect::None,
            )
        })?;
        Ok(Self::from_random(random))
    }
    fn from_random(random: [u8; 21]) -> Self {
        fn hex(bytes: &[u8]) -> String {
            const HEX: &[u8; 16] = b"0123456789abcdef";
            bytes
                .iter()
                .flat_map(|b| {
                    [
                        HEX[(b >> 4) as usize] as char,
                        HEX[(b & 15) as usize] as char,
                    ]
                })
                .collect()
        }
        Self {
            service_name: format!("studio-{}", hex(&random[..5])),
            password: hex(&random[5..]),
        }
    }
    /// Exact mDNS instance name to match. Never pair an arbitrary first result.
    pub fn service_name(&self) -> &str {
        &self.service_name
    }
    /// Secret passed only to the pairing handshake. Do not log this value.
    pub fn password(&self) -> &str {
        &self.password
    }
    /// Contains the secret. Intended for a chosen QR encoder, not diagnostics.
    pub fn payload(&self) -> String {
        // Generated hexadecimal fields cannot contain WIFI URI delimiters.
        format!("WIFI:T:ADB;S:{};P:{};;", self.service_name, self.password)
    }
    /// SVG contains the same secret as the terminal QR. Save only when explicitly
    /// requested and remove the artifact after the pairing attempt.
    pub fn render_svg(&self) -> Result<String> {
        let qr = QrCode::new(self.payload().as_bytes()).map_err(|_| {
            crate::error(
                "android_qr_encode",
                "Could not encode pairing QR",
                Effect::None,
            )
        })?;
        Ok(qr
            .render::<qrcode::render::svg::Color>()
            .quiet_zone(true)
            .min_dimensions(392, 392)
            .build())
    }
    /// Unicode blocks with a four-module quiet zone. Uses explicit black/white
    /// ANSI colors so terminal themes cannot invert the code. Display on a UTF-8
    /// terminal with at least 49 columns; do not wrap or trim the returned rows.
    /// The returned image encodes the secret and should not enter ordinary logs.
    pub fn render_terminal(&self) -> Result<String> {
        let qr = QrCode::new(self.payload().as_bytes()).map_err(|_| {
            crate::error(
                "android_qr_encode",
                "Could not encode pairing QR",
                Effect::None,
            )
        })?;
        let rows = qr.render::<Dense1x2>().quiet_zone(true).build();
        Ok(rows
            .lines()
            .map(|row| format!("\x1b[30;47m{row}\x1b[0m\n"))
            .collect())
    }
}

const PAIRING_SERVICE: &str = "_adb-tls-pairing._tcp.local.";
const CONNECT_SERVICE: &str = "_adb-tls-connect._tcp.local.";

struct Browser(ServiceDaemon);
impl Browser {
    fn new() -> Result<Self> {
        ServiceDaemon::new()
            .map(Self)
            .map_err(|e| crate::error("android_mdns", e, Effect::None))
    }
    fn browse(&self, service: &str) -> Result<Receiver<ServiceEvent>> {
        self.0
            .browse(service)
            .map_err(|e| crate::error("android_mdns", e, Effect::None))
    }
}
impl Drop for Browser {
    fn drop(&mut self) {
        let _ = self.0.shutdown();
    }
}
/// Starts listening before the QR is displayed. Dropping the session requests
/// shutdown of its private mDNS browser. No host service is published.
pub struct QrPairingSession {
    credentials: QrPairing,
    _browser: Browser,
    pairing: Receiver<ServiceEvent>,
}
impl QrPairingSession {
    pub fn new() -> Result<Self> {
        let credentials = QrPairing::generate()?;
        let browser = Browser::new()?;
        let pairing = browser.browse(PAIRING_SERVICE)?;
        Ok(Self {
            credentials,
            _browser: browser,
            pairing,
        })
    }
    pub fn service_name(&self) -> &str {
        self.credentials.service_name()
    }
    pub fn password(&self) -> &str {
        self.credentials.password()
    }
    pub fn render_terminal(&self) -> Result<String> {
        self.credentials.render_terminal()
    }
    pub fn render_svg(&self) -> Result<String> {
        self.credentials.render_svg()
    }
    /// Resolves only the exact instance requested by this QR. mDNS discovery is
    /// unauthenticated; the pairing handshake must still validate the secret.
    pub fn wait_endpoint(&self, timeout: Duration) -> Result<SocketAddr> {
        let name = format!("{}.{}", self.service_name(), PAIRING_SERVICE);
        wait_service(&self.pairing, timeout, |actual| {
            actual.eq_ignore_ascii_case(&name)
        })
    }
}

/// Discover the distinct command endpoint for an authenticated pairing GUID.
/// This starts a new browse and resolves existing announcements as well as new
/// ones. The resulting address is only a hint; authenticate TLS before use.
pub fn discover_connection(guid: &str, timeout: Duration) -> Result<SocketAddr> {
    if guid.is_empty()
        || guid.len() > 128
        || !guid
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(crate::error(
            "android_mdns_guid",
            "Invalid device GUID",
            Effect::None,
        ));
    }
    let browser = Browser::new()?;
    let receiver = browser.browse(CONNECT_SERVICE)?;
    wait_service(&receiver, timeout, |actual| {
        connection_matches(actual, guid)
    })
}
fn connection_matches(actual: &str, guid: &str) -> bool {
    let actual = actual.to_ascii_lowercase();
    let guid = guid.to_ascii_lowercase();
    let Some(instance) = actual.strip_suffix(&format!(".{CONNECT_SERVICE}")) else {
        return false;
    };
    // AOSP advertises ReadDeviceGuid() verbatim. The GUID already contains
    // the adb prefix and random suffix. Never append or strip either part.
    // https://android.googlesource.com/platform/packages/modules/adb/+/refs/heads/main/daemon/mdns.cpp
    instance == guid
}
fn wait_service(
    receiver: &Receiver<ServiceEvent>,
    timeout: Duration,
    matches: impl Fn(&str) -> bool,
) -> Result<SocketAddr> {
    let start = Instant::now();
    loop {
        let remaining = timeout.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            return Err(crate::error(
                "android_mdns_timeout",
                "No matching Android service before deadline",
                Effect::None,
            ));
        }
        match receiver.recv_timeout(remaining) {
            Ok(ServiceEvent::ServiceResolved(info)) if matches(info.get_fullname()) => {
                let mut addresses: Vec<_> = info
                    .get_addresses()
                    .iter()
                    .filter_map(|ip| socket_address(ip, info.get_port()))
                    .collect();
                addresses.sort();
                if let Some(address) = addresses.first() {
                    return Ok(*address);
                }
            }
            Ok(_) => (),
            Err(RecvTimeoutError::Timeout) => {
                return Err(crate::error(
                    "android_mdns_timeout",
                    "No matching Android service before deadline",
                    Effect::None,
                ));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(crate::error(
                    "android_mdns_closed",
                    "Android service discovery stopped",
                    Effect::None,
                ));
            }
        }
    }
}
fn socket_address(ip: &ScopedIp, port: u16) -> Option<SocketAddr> {
    if port == 0 || ip.to_ip_addr().is_unspecified() || ip.to_ip_addr().is_multicast() {
        return None;
    }
    match ip {
        ScopedIp::V4(v4) => Some(SocketAddr::new((*v4.addr()).into(), port)),
        ScopedIp::V6(v6) => {
            Some(SocketAddrV6::new(*v6.addr(), port, 0, v6.scope_id().index).into())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn connection_matching_requires_guid_boundary_and_correct_service() {
        assert!(connection_matches(
            "adb-abc123-random._adb-tls-connect._tcp.local.",
            "adb-abc123-random"
        ));
        assert!(!connection_matches(
            "adb-abc1234-random._adb-tls-connect._tcp.local.",
            "adb-abc123-random"
        ));
        assert!(!connection_matches(
            "adb-abc123-random._adb-tls-pairing._tcp.local.",
            "adb-abc123-random"
        ));
    }
    #[test]
    fn payload_has_exact_requested_service_and_secret_without_delimiters() {
        let qr = QrPairing::from_random([0xab; 21]);
        assert_eq!(qr.service_name(), "studio-ababababab");
        assert_eq!(qr.password(), "abababababababababababababababab");
        assert_eq!(
            qr.payload(),
            "WIFI:T:ADB;S:studio-ababababab;P:abababababababababababababababab;;"
        );
        assert!(qr.password().bytes().all(|b| b.is_ascii_hexdigit()));
    }
    #[test]
    fn terminal_render_has_contrast_reset_and_uniform_width_without_cleartext() {
        let qr = QrPairing::from_random([0xab; 21]);
        let rendered = qr.render_terminal().unwrap();
        assert!(!rendered.contains(qr.password()));
        let rows: Vec<_> = rendered.lines().collect();
        assert!(rows.len() > 10);
        let width = rows[0].chars().count();
        assert!(rows.iter().all(|row| row.starts_with("\x1b[30;47m")
            && row.ends_with("\x1b[0m")
            && row.chars().count() == width));
    }
}

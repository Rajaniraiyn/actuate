use super::{ConnectOptions, Dispatcher, Provider};
use actuate::{NativeError, Result};
use serde_json::Value;

pub(crate) fn connect(options: ConnectOptions) -> Result<Box<dyn Dispatcher>> {
    let host = matches!(
        options.provider,
        Provider::Native | Provider::Windows | Provider::Macos | Provider::Linux
    );
    if host
        && (options.device.is_some()
            || options.device_set.is_some()
            || options.credentials.is_some()
            || options.trust_first_connection)
    {
        return Err(NativeError::invalid_request(
            "Desktop providers do not accept device options",
        ));
    }
    match options.provider {
        #[cfg(target_os = "windows")]
        Provider::Native | Provider::Windows => {
            let mut session = windows::WindowsSession::new()?;
            Ok(Box::new(move |request| session.dispatch(request)))
        }
        #[cfg(target_os = "macos")]
        Provider::Native | Provider::Macos => {
            let mut session = macos::session::MacSession::new();
            Ok(Box::new(move |request| session.dispatch(request)))
        }
        #[cfg(target_os = "linux")]
        Provider::Native | Provider::Linux => {
            let mut session = linux::LinuxSession::connect()?;
            Ok(Box::new(move |request| session.dispatch(request)))
        }
        #[cfg(target_os = "macos")]
        Provider::AppleSimulator => {
            let device = required(&options.device, "device")?;
            let set = options
                .device_set
                .as_deref()
                .ok_or_else(|| NativeError::invalid_request("device_set is required"))?;
            let mut session = ios::session::connect(device, set)?;
            Ok(Box::new(move |request| {
                encode(ios::jsonl::dispatch(&mut session, request)?)
            }))
        }
        #[cfg(feature = "apple-device")]
        Provider::AppleDevice => physical(options),
        #[cfg(feature = "android")]
        Provider::Android => android(options),
        _ => Err(NativeError::unsupported(
            "Provider is not available in this build or on this host",
        )),
    }
}
#[cfg(any(target_os = "macos", feature = "android", feature = "apple-device"))]
fn required<'a>(value: &'a Option<String>, name: &str) -> Result<&'a str> {
    value
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| NativeError::invalid_request(format!("{name} is required")))
}
#[cfg(any(target_os = "macos", feature = "android", feature = "apple-device"))]
fn encode(value: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(value).map_err(|e| NativeError::new("encode_response", e))
}
#[cfg(feature = "apple-device")]
fn physical(options: ConnectOptions) -> Result<Box<dyn Dispatcher>> {
    use actuate::Capture;
    use serde::Deserialize;
    #[derive(Deserialize)]
    #[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
    enum Request {
        Capabilities,
        Discover,
        Info,
        Capture { path: std::path::PathBuf },
    }
    let mut provider =
        ios::physical::PhysicalCapture::from_env(std::time::Duration::from_secs(30))?;
    Ok(Box::new(move |value| {
        let request: Request = serde_json::from_value(value)
            .map_err(|e| NativeError::invalid_request(e.to_string()))?;
        match request {
            Request::Capabilities => encode(ios::physical::capabilities()),
            Request::Discover => encode(provider.discover()?),
            Request::Info => encode(provider.info(required(&options.device, "device")?)?),
            Request::Capture { path } => encode(
                provider
                    .capture(required(&options.device, "device")?.into())?
                    .save(path)?,
            ),
        }
    }))
}
#[cfg(feature = "android")]
fn android(options: ConnectOptions) -> Result<Box<dyn Dispatcher>> {
    use android::{
        Android, DirectDevice,
        wireless::{FirstConnectionPolicy, WirelessHost},
    };
    let device = required(&options.device, "device")?;
    let credentials = options
        .credentials
        .as_deref()
        .ok_or_else(|| NativeError::invalid_request("credentials is required"))?;
    if let Some(ids) = device.strip_prefix("usb:") {
        let (vendor, product) = ids
            .split_once(':')
            .ok_or_else(|| NativeError::invalid_request("Expected usb:VID:PID"))?;
        let id =
            |s| u16::from_str_radix(s, 16).map_err(|e| NativeError::invalid_request(e.to_string()));
        let device = Android::new(DirectDevice::usb(id(vendor)?, id(product)?, credentials)?);
        return Ok(android_dispatch(device));
    }
    if let Some(endpoint) = device.strip_prefix("tcp:") {
        let endpoint = endpoint
            .parse()
            .map_err(|e: std::net::AddrParseError| NativeError::invalid_request(e.to_string()))?;
        return Ok(android_dispatch(Android::new(DirectDevice::tcp(
            endpoint,
            credentials,
        )?)));
    }
    let endpoint = device
        .parse()
        .map_err(|e: std::net::AddrParseError| NativeError::invalid_request(e.to_string()))?;
    let policy = if options.trust_first_connection {
        FirstConnectionPolicy::TrustOnFirstUse
    } else {
        FirstConnectionPolicy::RequireKnownCertificate
    };
    let transport = WirelessHost::new(credentials, std::time::Duration::from_secs(30))?
        .connect(endpoint, policy)?;
    Ok(android_dispatch(Android::new(transport)))
}
#[cfg(feature = "android")]
fn android_dispatch<D: android::CommandTransport + 'static>(
    mut device: android::Android<D>,
) -> Box<dyn Dispatcher> {
    Box::new(move |request: Value| android::jsonl::dispatch(&mut device, request))
}

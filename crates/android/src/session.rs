//! Typed session operations independent of CLI parsing and connection discovery.
use crate::{Android, AndroidKey, CommandTransport, DeviceInfo, PixelPoint};
use actuate::{Receipt, Result};
use serde::{Deserialize, Serialize};
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Info,
    Apps,
    Launch {
        component: crate::apps::Component,
    },
    Capture,
    Tap {
        point: PixelPoint,
    },
    Swipe {
        from: PixelPoint,
        to: PixelPoint,
        duration_ms: u64,
    },
    Key {
        key: AndroidKey,
    },
    TypeAscii {
        text: String,
    },
}
#[derive(Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Response {
    Info(DeviceInfo),
    Apps(crate::apps::LauncherActivities),
    Launch(crate::apps::LaunchReport),
    Frame(Vec<u8>),
    Receipt(Receipt),
}
/// Binary frames remain bytes here. External transports choose their own framing.
pub fn execute<D: CommandTransport>(device: &mut Android<D>, request: Request) -> Result<Response> {
    Ok(match request {
        Request::Info => Response::Info(device.info()?),
        Request::Apps => Response::Apps(device.launcher_activities()?),
        Request::Launch { component } => Response::Launch(device.launch(&component)?),
        Request::Capture => Response::Frame(device.capture_png()?),
        Request::Tap { point } => Response::Receipt(device.tap(point)?),
        Request::Swipe {
            from,
            to,
            duration_ms,
        } => Response::Receipt(device.swipe(
            from,
            to,
            std::time::Duration::from_millis(duration_ms),
        )?),
        Request::Key { key } => Response::Receipt(device.key(key)?),
        Request::TypeAscii { text } => Response::Receipt(device.type_ascii(&text)?),
    })
}

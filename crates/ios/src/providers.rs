//! Independently selectable simulator services and lazy input connection.
use crate::{SimulatorHid, simulator::Simctl};
use serde::Serialize;
use std::path::{Path, PathBuf};
use unimation::{
    AppLifecycle, Capture, Effect, HardwareButton, HardwareButtons, HidKeyboard, NativeError,
    Receipt, Result, TouchAction, TouchInput,
};

/// Loading HID support is deferred until an input request. Observation remains
/// usable when a private HID framework is absent; no other input route is tried.
pub struct LazySimulatorInput {
    udid: String,
    device_set: PathBuf,
    provider: Option<SimulatorHid>,
}
impl LazySimulatorInput {
    pub fn new(udid: impl Into<String>, device_set: impl AsRef<Path>) -> Self {
        Self {
            udid: udid.into(),
            device_set: device_set.as_ref().to_owned(),
            provider: None,
        }
    }
    fn provider(&mut self) -> Result<&mut SimulatorHid> {
        if self.provider.is_none() {
            self.provider = Some(SimulatorHid::connect(&self.udid, &self.device_set)?);
        }
        Ok(self.provider.as_mut().expect("provider connected"))
    }
}
impl TouchInput for LazySimulatorInput {
    fn touch(&mut self, action: TouchAction) -> Result<Receipt> {
        self.provider()?.touch(action)
    }
}
impl HardwareButtons for LazySimulatorInput {
    fn press_button(&mut self, button: HardwareButton) -> Result<Receipt> {
        self.provider()?.press_button(button)
    }
}
impl HidKeyboard for LazySimulatorInput {
    fn press_usage(&mut self, usage: u16, modifiers: &[u16]) -> Result<Receipt> {
        self.provider()?.press_usage(usage, modifiers)
    }
}

#[derive(Clone)]
pub struct SimulatorServices {
    simctl: Simctl,
    udid: String,
}
impl SimulatorServices {
    pub fn new(simctl: Simctl, udid: impl Into<String>) -> Self {
        Self {
            simctl,
            udid: udid.into(),
        }
    }
}
#[derive(Debug, Serialize)]
pub struct SimulatorFrame {
    pub path: PathBuf,
    pub pixel_width: u32,
    pub pixel_height: u32,
    /// No implicit conversion to AX or raw HID coordinates is established.
    pub click_mapping: Option<()>,
}
fn failed(message: impl Into<String>) -> NativeError {
    NativeError {
        code: "capture_failed".into(),
        message: message.into(),
        effect: Effect::None,
    }
}
impl Capture for SimulatorServices {
    type Request = PathBuf;
    type Frame = SimulatorFrame;
    fn capture(&mut self, path: PathBuf) -> Result<SimulatorFrame> {
        use std::io::Read;
        if path.exists() {
            return Err(failed("Output path already exists"));
        }
        self.simctl.screenshot(&self.udid, &path)?;
        let mut header = [0u8; 24];
        std::fs::File::open(&path)
            .and_then(|mut file| file.read_exact(&mut header))
            .map_err(|e| failed(e.to_string()))?;
        if &header[..8] != b"\x89PNG\r\n\x1a\n" || &header[12..16] != b"IHDR" {
            return Err(failed("Native screenshot did not return a PNG header"));
        }
        Ok(SimulatorFrame {
            path,
            pixel_width: u32::from_be_bytes(header[16..20].try_into().unwrap()),
            pixel_height: u32::from_be_bytes(header[20..24].try_into().unwrap()),
            click_mapping: None,
        })
    }
}
impl AppLifecycle for SimulatorServices {
    type App = String;
    fn launch_app(&mut self, bundle: String) -> Result<Receipt> {
        self.simctl.launch(&self.udid, &bundle)?;
        Ok(Receipt {
            effect: Effect::Dispatched,
            route: "ios.simulator.launch".into(),
        })
    }
}

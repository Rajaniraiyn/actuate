//! Hyprland as the window-geometry authority for AT-SPI on Wayland.
//! Toplevel frames come from `hyprctl clients`; matching uses the owning
//! pid and the window title, which GTK and Chromium expose as the frame name.
use crate::atspi::{WindowFrame, WindowOrigins};
use compositor::{
    Hyprland,
    hyprland::{Client, Monitor},
};
use serde_json::{Value, json};
use unimation::Result;

/// Xwayland clients measure in X pixels, which Hyprland scales per monitor.
pub fn xwayland_scale(client: &Client, monitors: &[Monitor]) -> f64 {
    if !client.xwayland {
        return 1.;
    }
    monitors
        .iter()
        .find(|m| m.id == client.monitor)
        .map(|m| m.scale)
        .filter(|s| *s > 0.)
        .unwrap_or(1.)
}

pub struct HyprlandOrigins {
    hypr: Hyprland,
}
impl HyprlandOrigins {
    pub fn new(hypr: Hyprland) -> Self {
        Self { hypr }
    }
}
impl WindowOrigins for HyprlandOrigins {
    fn origin(&self, pid: i32, title: &str) -> Option<WindowFrame> {
        let client = self.hypr.client_for(i64::from(pid), title).ok()??;
        let scale = if client.xwayland {
            xwayland_scale(&client, &self.hypr.monitors().ok()?)
        } else {
            1.
        };
        Some(WindowFrame {
            x: client.at[0] as f64,
            y: client.at[1] as f64,
            scale,
        })
    }
    fn source(&self) -> &'static str {
        "hyprland_client_origin"
    }
}

/// Window records for discovery: the native client plus the portable
/// `window_id` handle and a `bounds` rectangle.
pub fn window_records(hypr: &Hyprland) -> Result<Vec<Value>> {
    Ok(hypr
        .clients()?
        .into_iter()
        .map(|c| {
            let mut record = json!(c);
            record["window_id"] = json!(c.handle());
            record["bounds"] = json!(c.rect());
            record
        })
        .collect())
}

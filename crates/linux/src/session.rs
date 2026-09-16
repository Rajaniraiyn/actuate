//! Typed session protocol. Connections open on first use and live until drop.
use crate::{
    Accessibility, AccessibilityProvider, DesktopProvider, Environment, X11, error, unsupported,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::VecDeque, path::PathBuf};
use unimation::{
    Delivery, ElementRef, KeyChord, ObserveRequest, PointerAction, Result, SemanticAction, Snapshot,
};

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum LinuxRequest {
    Capabilities {},
    WaylandStart {
        keyboard: bool,
        pointer: bool,
        screencast: bool,
    },
    Wayland {
        command: crate::wayland::Command,
    },
    Discover {},
    Windows {},
    Displays {},
    #[serde(alias = "observe")]
    Snapshot {
        request: ObserveRequest,
    },
    Semantic {
        target: ElementRef,
        action: SemanticAction,
    },
    Pointer {
        delivery: Delivery,
        action: PointerAction,
    },
    Text {
        delivery: Delivery,
        text: String,
    },
    Key {
        delivery: Delivery,
        chord: KeyChord,
    },
    /// Native wheel detents, explicitly distinct from portable pixel scrolling.
    X11Wheel {
        vertical: i32,
        horizontal: i32,
        #[serde(default)]
        point: Option<unimation::Point>,
    },
    Capture {
        path: PathBuf,
    },
    Diff {
        before: u64,
        #[serde(default)]
        after: Option<u64>,
    },
    Inspect {
        target: ElementRef,
    },
    Snapshots {},
}
#[derive(Default)]
pub struct LinuxSession {
    accessibility: Option<Box<dyn AccessibilityProvider>>,
    desktop: Option<Box<dyn DesktopProvider>>,
    snapshots: VecDeque<Snapshot>,
}
impl LinuxSession {
    /// No desktop connection or permission prompt occurs until a route is used.
    pub fn new() -> Result<Self> {
        Ok(Self::default())
    }
    pub fn with_providers(
        accessibility: Option<Box<dyn AccessibilityProvider>>,
        desktop: Option<Box<dyn DesktopProvider>>,
    ) -> Self {
        Self {
            accessibility,
            desktop,
            snapshots: VecDeque::new(),
        }
    }
    fn accessibility(&mut self) -> Result<&mut (dyn AccessibilityProvider + '_)> {
        if self.accessibility.is_none() {
            self.accessibility = Some(Box::new(Accessibility::connect()?));
        }
        Ok(self.accessibility.as_mut().unwrap().as_mut())
    }
    fn desktop(&mut self) -> Result<&mut (dyn DesktopProvider + '_)> {
        if self.desktop.is_none() {
            self.desktop = Some(Box::new(X11::auto()?));
        }
        Ok(self.desktop.as_mut().unwrap().as_mut())
    }
    pub fn dispatch(&mut self, request: Value) -> Result<Value> {
        let request = serde_json::from_value(request).map_err(|e| error("invalid_request", e))?;
        self.execute(request)
    }
    pub fn execute(&mut self, request: LinuxRequest) -> Result<Value> {
        match request {
            LinuxRequest::WaylandStart { keyboard, pointer, screencast } => {
                if self.desktop.is_some() { return Err(error("desktop_already_connected", "Stop the current session before selecting a new desktop provider")); }
                let mut portal=crate::wayland::Portal::start(crate::wayland::StartOptions {keyboard,pointer,screencast})?;
                let status=portal.status(); self.desktop=Some(Box::new(portal)); Ok(status)
            }
            LinuxRequest::Wayland {command} => {
                let stop=matches!(command,crate::wayland::Command::Stop{});
                let result=self.desktop.as_mut().ok_or_else(|| error("portal_not_started","Explicitly start a Wayland portal session first"))?.wayland(command);
                if stop && result.is_ok() {self.desktop=None;} result
            }
            LinuxRequest::Capabilities {} => Ok(json!({
                "provider": "linux",
                "environment": Environment::detect(),
                "accessibility": self.accessibility.as_ref().map(|p| p.descriptor())
                    .unwrap_or_else(|| json!({"default": "atspi_dbus", "connected": false})),
                "desktop": self.desktop.as_ref().map(|p| p.descriptor())
                    .unwrap_or_else(|| json!({
                        "default": "auto_x11_only", "connected": false,
                        "wayland": "Explicit wayland_start opens portal consent; automatic XWayland fallback is disabled"
                    })),
                "runtime_verified": false
            })),
            LinuxRequest::Discover {} => self.discover(),
            LinuxRequest::Windows {} => self.desktop()?.windows(),
            LinuxRequest::Displays {} => self.desktop()?.displays(),
            LinuxRequest::Snapshot { request } => {
                let snapshot = self.accessibility()?.observe(request)?;
                let value = json!(snapshot);
                self.snapshots.push_back(snapshot);
                while self.snapshots.len() > 16 {
                    self.snapshots.pop_front();
                }
                Ok(value)
            }
            LinuxRequest::Semantic { target, action } => {
                Ok(json!(self.accessibility()?.semantic(&target, action)?))
            }
            LinuxRequest::Pointer { delivery, action } => {
                if self.desktop.is_none() && !matches!(delivery, Delivery::Global {}) {
                    return Err(unsupported("Linux process pointer input is not implemented; no global fallback"));
                }
                Ok(json!(self.desktop()?.pointer(delivery, action)?))
            }
            LinuxRequest::Text { delivery, text } => {
                Ok(json!(self.desktop()?.text(delivery, &text)?))
            }
            LinuxRequest::Key { delivery, chord } => {
                if self.desktop.is_none() && !matches!(delivery, Delivery::Global {}) {
                    return Err(unsupported("Linux process keyboard input is not implemented"));
                }
                Ok(json!(self.desktop()?.key_press(delivery, chord)?))
            }
            LinuxRequest::X11Wheel { vertical, horizontal, point } => {
                Ok(json!(self.desktop()?.wheel(vertical, horizontal, point)?))
            }
            LinuxRequest::Capture { path } => Ok(json!(self.desktop()?.capture_root(&path)?)),
            LinuxRequest::Diff { before, after } => {
                let first = self.snapshot(Some(before))?;
                let last = self.snapshot(after)?;
                Ok(json!(unimation::diff::diff_snapshots(first, last)?))
            }
            LinuxRequest::Inspect { target } => self.snapshots.iter().rev()
                .find_map(|s| s.nodes.iter().find(|n| n.reference == target))
                .map(|n| json!({"node": n, "source": "cached_observation"}))
                .ok_or_else(|| error("unknown_reference", "No retained observation for this reference")),
            LinuxRequest::Snapshots {} => Ok(json!(self.snapshots.iter().map(|s| json!({
                "revision": s.revision, "root": s.root, "nodes": s.nodes.len(), "complete": s.complete
            })).collect::<Vec<_>>())),
        }
    }
    fn snapshot(&self, revision: Option<u64>) -> Result<&Snapshot> {
        self.snapshots
            .iter()
            .rev()
            .find(|s| revision.is_none_or(|v| v == s.revision))
            .ok_or_else(|| {
                error(
                    "unknown_snapshot",
                    "Snapshot was not observed or is no longer retained",
                )
            })
    }
    fn discover(&mut self) -> Result<Value> {
        let ax = self.accessibility().and_then(|a| a.discover());
        let windows = self.desktop().and_then(|x| x.windows());
        if let (Err(ax), Err(windows)) = (&ax, &windows) {
            return Err(error(
                "discovery_unavailable",
                format!("AT-SPI: {ax}; X11: {windows}"),
            ));
        }
        let mut result = match ax {
            Ok(v) => v,
            Err(e) => json!({"applications":[],"accessibility_error":e}),
        };
        match windows {
            Ok(windows) => {
                // EWMH PIDs are client-reported and can refer to remote machines.
                // They cannot identify a local AT-SPI application by themselves.
                result["window_context"] = windows;
                result["active_pid"] = Value::Null;
                result["window_association"] = json!("unverified_client_reported_pids");
            }
            Err(e) => result["window_error"] = json!(e),
        }
        // AT-SPI registrations are application records, including on Wayland.
        for app in result["applications"].as_array_mut().unwrap() {
            app["application"] = json!(true);
        }
        Ok(result)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    struct TestDesktop;
    impl unimation::PointerInput for TestDesktop {
        fn pointer(
            &mut self,
            _delivery: Delivery,
            _action: PointerAction,
        ) -> Result<unimation::Receipt> {
            Err(unsupported("test has no pointer"))
        }
    }
    impl unimation::KeyboardInput for TestDesktop {
        type Key = KeyChord;
        fn key_press(
            &mut self,
            _delivery: Delivery,
            _chord: KeyChord,
        ) -> Result<unimation::Receipt> {
            Err(unsupported("test has no keyboard"))
        }
    }
    impl DesktopProvider for TestDesktop {
        fn route(&self) -> &'static str {
            "test_compositor"
        }
        fn windows(&mut self) -> Result<Value> {
            Ok(json!({"windows":[]}))
        }
        fn displays(&mut self) -> Result<Value> {
            Ok(json!([]))
        }
        fn capture_root(&mut self, _path: &std::path::Path) -> Result<crate::Frame> {
            Err(unsupported("test has no capture"))
        }
    }
    #[test]
    fn injected_provider_reports_its_own_route_and_keeps_native_extension_scoped() {
        let mut s = LinuxSession::with_providers(None, Some(Box::new(TestDesktop)));
        let capabilities = s.dispatch(json!({"op":"capabilities"})).unwrap();
        assert_eq!(capabilities["desktop"]["implementation"], "test_compositor");
        assert!(capabilities["desktop"]["operations"].is_null());
        assert_eq!(
            s.dispatch(json!({"op":"x11_wheel","vertical":1,"horizontal":0}))
                .unwrap_err()
                .code,
            "unsupported_route"
        );
        assert!(s.accessibility.is_none());
    }
    #[test]
    fn process_route_never_connects_or_falls_back() {
        let mut s = LinuxSession::new().unwrap();
        let e=s.dispatch(json!({"op":"pointer","delivery":{"kind":"process","pid":1},"action":{"kind":"move","point":{"x":0,"y":0}}})).unwrap_err();
        assert_eq!(e.code, "unsupported_route");
        assert!(s.desktop.is_none());
    }
    #[test]
    fn portal_start_requires_explicit_choices_and_never_connects_for_empty_grants() {
        assert!(
            serde_json::from_value::<LinuxRequest>(
                json!({"op":"wayland_start","keyboard":true,"pointer":true})
            )
            .is_err()
        );
        assert!(serde_json::from_value::<LinuxRequest>(json!({"op":"wayland_start","keyboard":true,"pointer":true,"screencast":true,"extra":true})).is_err());
        let mut s = LinuxSession::new().unwrap();
        assert_eq!(
            s.dispatch(
                json!({"op":"wayland_start","keyboard":false,"pointer":false,"screencast":false})
            )
            .unwrap_err()
            .code,
            "invalid_request"
        );
        assert!(s.desktop.is_none());
        assert_eq!(
            s.dispatch(json!({"op":"wayland","command":{"kind":"status"}}))
                .unwrap_err()
                .code,
            "portal_not_started"
        );
    }
    #[test]
    fn portal_start_does_not_replace_an_injected_provider() {
        let mut s = LinuxSession::with_providers(None, Some(Box::new(TestDesktop)));
        assert_eq!(
            s.dispatch(
                json!({"op":"wayland_start","keyboard":true,"pointer":true,"screencast":true})
            )
            .unwrap_err()
            .code,
            "desktop_already_connected"
        );
        assert_eq!(s.desktop.as_ref().unwrap().route(), "test_compositor");
    }
    #[test]
    fn requests_reject_unknown_fields() {
        assert!(
            serde_json::from_value::<LinuxRequest>(
                json!({"op":"capture","path":"x.png","window":1})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<LinuxRequest>(
                json!({"op":"pointer","action":{"kind":"move","point":{"x":0,"y":0}}})
            )
            .is_err()
        );
    }
}

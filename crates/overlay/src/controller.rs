//! Optional visual helper lifetime. Enqueuing a command does not acknowledge a
//! rendered frame and never changes the result of an input-provider operation.
use crate::{CursorAcknowledgement, CursorCommand};
use serde_json::{Value, json};
use std::{
    io::Write,
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, SyncSender, TrySendError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use unimation::Result;

pub struct OverlayController {
    physical_cursor: crate::PhysicalCursorPolicy,
    tracking: crate::CursorTracking,
    child: Option<Child>,
    sender: Option<SyncSender<Vec<u8>>>,
    writer: Option<JoinHandle<()>>,
}
/// Process and configuration state, not proof of a presented frame.
#[derive(Debug, Default, serde::Serialize)]
pub struct OverlayState {
    pub running: bool,
    pub pid: Option<u32>,
    pub physical_cursor_policy: crate::PhysicalCursorPolicy,
    pub tracking: crate::CursorTracking,
    pub presentation_acknowledged: bool,
}
fn failure(message: impl ToString) -> unimation::NativeError {
    unimation::NativeError::new("overlay_failed", message)
}
fn serialize(command: Value) -> Result<Vec<u8>> {
    let typed: CursorCommand = serde_json::from_value(command.clone()).map_err(failure)?;
    typed.validate()?;
    // Serde's internally tagged unit variants accept extra map keys even with
    // deny_unknown_fields. Preserve the wire contract for show/hide/quit.
    if matches!(
        typed,
        CursorCommand::Hide | CursorCommand::Show | CursorCommand::Quit
    ) && command.as_object().is_some_and(|object| object.len() != 1)
    {
        return Err(failure("Unknown overlay command field"));
    }

    let mut bytes = serde_json::to_vec(&command).map_err(failure)?;
    bytes.push(b'\n');
    Ok(bytes)
}
impl OverlayController {
    /// `path` must explicitly name an executable. No PATH search or shell is used.
    /// Returns after spawning, before any acknowledgement from the visual helper.
    pub fn start(path: impl AsRef<Path>) -> Result<Self> {
        Self::start_with_policy(path, crate::PhysicalCursorPolicy::Preserve)
    }
    pub fn physical_cursor_policy(&self) -> crate::PhysicalCursorPolicy {
        self.physical_cursor
    }
    pub fn start_with_policy(
        path: impl AsRef<Path>,
        policy: crate::PhysicalCursorPolicy,
    ) -> Result<Self> {
        Self::start_with_options(path, policy, crate::CursorTracking::Commands)
    }
    pub fn start_with_options(
        path: impl AsRef<Path>,
        policy: crate::PhysicalCursorPolicy,
        tracking: crate::CursorTracking,
    ) -> Result<Self> {
        #[cfg(not(windows))]
        if policy != crate::PhysicalCursorPolicy::Preserve
            || tracking != crate::CursorTracking::Commands
        {
            return Err(failure(
                "Physical cursor hiding and tracking are not implemented by this platform renderer",
            ));
        }
        let path = std::fs::canonicalize(path).map_err(failure)?;
        let mut command = Command::new(path);
        if tracking == crate::CursorTracking::PhysicalPointer {
            command.arg("--track-physical-pointer");
        }
        match policy {
            crate::PhysicalCursorPolicy::Preserve => {}
            crate::PhysicalCursorPolicy::HideWithinScope => {
                command.arg("--hide-cursor-within-scope");
            }
            crate::PhysicalCursorPolicy::HideWhileVisible => {
                command.arg("--hide-cursor-while-visible");
            }
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW for the visual helper.
        }
        let mut child = command.spawn().map_err(failure)?;
        let Some(mut stdin) = child.stdin.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(failure("Overlay stdin unavailable"));
        };
        let (sender, receiver) = mpsc::sync_channel::<Vec<u8>>(64);
        let writer = match thread::Builder::new()
            .name("overlay-writer".into())
            .spawn(move || {
                while let Ok(bytes) = receiver.recv() {
                    if stdin.write_all(&bytes).is_err() {
                        break;
                    }
                }
            }) {
            Ok(writer) => writer,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(failure(e));
            }
        };
        Ok(Self {
            physical_cursor: policy,
            tracking,
            child: Some(child),
            sender: Some(sender),
            writer: Some(writer),
        })
    }
    pub fn scope(&mut self, scope: crate::CursorScope) -> Result<()> {
        self.send(json!({"op":"scope","scope":scope}))
    }
    pub fn state(&mut self) -> Result<OverlayState> {
        let running = self.is_running()?;
        Ok(OverlayState {
            running,
            pid: if running { self.pid() } else { None },
            physical_cursor_policy: self.physical_cursor,
            tracking: self.tracking,
            presentation_acknowledged: false,
        })
    }
    pub fn is_running(&mut self) -> Result<bool> {
        match self.child.as_mut() {
            Some(child) => child
                .try_wait()
                .map(|status| status.is_none())
                .map_err(failure),
            None => Ok(false),
        }
    }
    /// Identifies the owned visual process for capture/window filtering.
    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref().map(Child::id)
    }
    /// Success means queued locally. The helper may not have read or rendered it.
    /// A full queue reports an error instead of blocking input delivery.
    pub fn send(&mut self, command: Value) -> Result<()> {
        let bytes = serialize(command)?;
        if let Some(status) = self
            .child
            .as_mut()
            .ok_or_else(|| failure("Overlay stopped"))?
            .try_wait()
            .map_err(failure)?
        {
            return Err(failure(format!("Overlay exited {status}")));
        }
        self.sender
            .as_ref()
            .ok_or_else(|| failure("Overlay stopped"))?
            .try_send(bytes)
            .map_err(|e| match e {
                TrySendError::Full(_) => failure("Overlay queue is full"),
                TrySendError::Disconnected(_) => failure("Overlay writer disconnected"),
            })
    }
    pub fn move_to(&mut self, x: f64, y: f64, duration_ms: u64) -> Result<()> {
        self.send(json!({"op":"move","x":x,"y":y,"duration_ms":duration_ms}))
    }
    pub fn click(&mut self, x: f64, y: f64) -> Result<()> {
        self.send(json!({"op":"click","x":x,"y":y}))
    }
    pub fn show(&mut self) -> Result<()> {
        self.send(json!({"op":"show"}))
    }
    pub fn hide(&mut self) -> Result<()> {
        self.send(json!({"op":"hide"}))
    }
    /// Close the queue, briefly allow EOF/quit, then terminate the owned process.
    /// Never wait for a writer that could be stuck in a pipe write.
    pub fn stop(&mut self) -> Result<()> {
        if let Some(sender) = self.sender.take() {
            let _ = sender.try_send(b"{\"op\":\"quit\"}\n".to_vec());
            drop(sender);
        }
        let Some(mut child) = self.child.take() else {
            return Ok(());
        };
        let deadline = Instant::now() + Duration::from_millis(250);
        let mut stopped = false;
        while Instant::now() < deadline {
            match child.try_wait() {
                Ok(Some(_)) => {
                    stopped = true;
                    break;
                }
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Err(_) => break,
            }
        }
        if !stopped {
            let _ = child.kill();
            // Reaping can block in unusual kernel states, so the caller never joins
            // this thread. It owns the child until the OS reports termination.
            thread::Builder::new()
                .name("overlay-reaper".into())
                .spawn(move || {
                    let _ = child.wait();
                })
                .map_err(failure)?;
        }
        if self.writer.as_ref().is_some_and(JoinHandle::is_finished)
            && let Some(writer) = self.writer.take()
        {
            let _ = writer.join();
        }
        Ok(())
    }
}
impl Drop for OverlayController {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
impl unimation::CursorVisualization for OverlayController {
    type Command = CursorCommand;
    type Status = CursorAcknowledgement;
    fn visualize(&mut self, command: Self::Command) -> Result<Self::Status> {
        command.validate()?;
        self.send(serde_json::to_value(command).map_err(failure)?)?;
        Ok(CursorAcknowledgement::Queued)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(unix)]
    fn closes_pipe_and_stops_without_ui() {
        let mut controller = OverlayController::start("/bin/cat").unwrap();
        controller.show().unwrap();
        assert_eq!(
            unimation::CursorVisualization::visualize(&mut controller, CursorCommand::Show)
                .unwrap(),
            CursorAcknowledgement::Queued
        );
        controller.stop().unwrap();
        assert!(controller.child.is_none());
        assert!(controller.hide().is_err());
        controller.stop().unwrap();
    }
    #[test]
    fn validates_and_serializes_json_lines() {
        assert_eq!(
            serialize(json!({"op":"show"})).unwrap(),
            b"{\"op\":\"show\"}\n"
        );
        for bad in [
            json!({"op":"move","x":1,"y":2,"duration_ms":10001}),
            json!({"op":"click","x":"1","y":2}),
            json!({"op":"show","extra":true}),
        ] {
            assert!(serialize(bad).is_err());
        }
    }
}

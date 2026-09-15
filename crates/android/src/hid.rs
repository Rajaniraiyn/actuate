//! Optional native pointer through Android's installed `hid -` utility.
//!
//! AOSP protocol and readiness requirement:
//! https://android.googlesource.com/platform/frameworks/base/+/master/cmds/hid/README.md
//! Descriptor follows USB HID 1.11 Appendix E.10, with wheel and Consumer AC Pan.
//! https://www.usb.org/document-library/device-class-definition-hid-111
//! Relative counts are accelerated by Android; they are not screen pixels.
use crate::{connection::Connection, error};
use droidmux::shell::{ShellOptions, ShellSession};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use unimation::{Effect, Receipt, Result};

const DESCRIPTOR: &[u8] = &[
    0x05, 0x01, 0x09, 0x02, 0xa1, 0x01, 0x09, 0x01, 0xa1, 0x00, 0x05, 0x09, 0x19, 0x01, 0x29, 0x05,
    0x15, 0x00, 0x25, 0x01, 0x95, 0x05, 0x75, 0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x03, 0x81, 0x01,
    0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x09, 0x38, 0x15, 0x81, 0x25, 0x7f, 0x75, 0x08, 0x95, 0x03,
    0x81, 0x06, 0x05, 0x0c, 0x0a, 0x38, 0x02, 0x95, 0x01, 0x81, 0x06, 0xc0, 0xc0,
];

#[derive(Clone, Copy, Debug)]
pub enum Button {
    Primary,
    Secondary,
    Middle,
    Back,
    Forward,
}
impl Button {
    fn mask(self) -> u8 {
        1 << (self as u8)
    }
}
/// Relative HID counts, excluding -128 which the descriptor does not permit.
#[derive(Clone, Copy, Debug, Default)]
pub struct Delta(i8);
impl Delta {
    pub fn new(value: i8) -> Result<Self> {
        if value == i8::MIN {
            return Err(error(
                "hid_delta",
                "HID delta must be -127..127",
                Effect::None,
            ));
        }
        Ok(Self(value))
    }
}

/// One persistent HID device. Mutably borrowing the connection serializes reports
/// and prevents its runtime disappearing before the HID subprocess is closed.
/// Call `close` for checked cleanup. Drop performs bounded best-effort cleanup.
pub struct Pointer<'a> {
    connection: &'a mut Connection,
    shell: Arc<ShellSession>,
    drains: Vec<tokio::task::JoinHandle<()>>,
    diagnostics: Arc<Mutex<Vec<u8>>>,
    buttons: u8,
    failed: bool,
    closed: bool,
}
impl<'a> Pointer<'a> {
    /// Requires an installed `hid` utility, access to /dev/uhid, and readable
    /// `dumpsys input`. No helper is uploaded and no privilege escalation occurs.
    pub fn open(connection: &'a mut Connection) -> Result<Self> {
        let diagnostics = Arc::new(Mutex::new(Vec::new()));
        let (shell, drains) =
            connection
                .runtime
                .run(connection.timeout, Effect::Unknown, async {
                    let shell = Arc::new(
                        droidmux::shell::open_shell(
                            &connection.client,
                            "/system/bin/hid -",
                            ShellOptions::default(),
                        )
                        .await
                        .map_err(|e| error("hid_open", e, Effect::Unknown))?,
                    );
                    let mut drains = Vec::new();
                    for stderr in [false, true] {
                        let s = Arc::clone(&shell);
                        let output = Arc::clone(&diagnostics);
                        drains.push(tokio::spawn(async move {
                            loop {
                                let chunk = if stderr {
                                    s.read_stderr().await
                                } else {
                                    s.read_stdout().await
                                };
                                let Some(chunk) = chunk else { break };
                                let mut bytes = output.lock().unwrap_or_else(|e| e.into_inner());
                                let take = chunk.len().min(8192usize.saturating_sub(bytes.len()));
                                bytes.extend_from_slice(&chunk[..take]);
                            }
                        }));
                    }
                    Ok((shell, drains))
                })?;
        let mut pointer = Self {
            connection,
            shell,
            drains,
            diagnostics,
            buttons: 0,
            failed: false,
            closed: false,
        };
        let name = format!("Unimation pointer {:016x}", rand::random::<u64>());
        pointer.send(serde_json::json!({"id":1,"command":"register","name":name,"vid":0,"pid":0,"bus":"usb","descriptor":DESCRIPTOR}))
            .map_err(|e| pointer.stage_error("registration_write", e, "registration command was not acknowledged"))?;
        // Kernel UHID_OPEN does not guarantee InputReader registration. Observe
        // the unique device name in InputReader's device inventory before input.
        let mut inventory = "no input inventory response yet".to_owned();
        let readiness =
            pointer
                .connection
                .runtime
                .run(pointer.connection.timeout, Effect::Unknown, async {
                    loop {
                        let output = droidmux::shell::execute_with_options(
                            &pointer.connection.client,
                            "dumpsys input",
                            ShellOptions {
                                max_output_bytes: Some(2 * 1024 * 1024),
                                ..Default::default()
                            },
                        )
                        .await
                        .map_err(|e| error("hid_readiness", e, Effect::Unknown))?;
                        if output.exit_code != Some(0) {
                            return Err(error(
                                "hid_readiness",
                                String::from_utf8_lossy(&output.stderr),
                                Effect::Unknown,
                            ));
                        }
                        let dump = String::from_utf8_lossy(&output.stdout);
                        inventory = inventory_diagnostic(&dump, &name);
                        if registered(&dump, &name) {
                            return Ok(());
                        }
                        if let Ok(result) =
                            tokio::time::timeout(Duration::from_millis(100), pointer.shell.wait())
                                .await
                        {
                            return Err(error(
                                "hid_exited",
                                format!(
                                    "hid exited before registration: {result:?}; {}",
                                    pointer.diagnostics()
                                ),
                                Effect::Unknown,
                            ));
                        }
                    }
                });
        readiness.map_err(|e| pointer.stage_error("registration_readiness", e, &inventory))?;
        Ok(pointer)
    }
    fn stage_error(
        &self,
        stage: &str,
        cause: unimation::NativeError,
        detail: &str,
    ) -> unimation::NativeError {
        let output = self.diagnostics();
        error(
            "hid_registration",
            format!(
                "stage={stage}; {}: {}; {detail}; hid output: {}",
                cause.code,
                cause.message,
                if output.is_empty() { "<none>" } else { &output }
            ),
            cause.effect,
        )
    }
    fn diagnostics(&self) -> String {
        String::from_utf8_lossy(&self.diagnostics.lock().unwrap_or_else(|e| e.into_inner()))
            .into_owned()
    }
    fn send(&mut self, value: serde_json::Value) -> Result<()> {
        if self.failed || self.closed {
            return Err(error(
                "hid_unavailable",
                "HID session is closed or failed; do not replay input",
                Effect::None,
            ));
        }
        let mut bytes = value.to_string().into_bytes();
        bytes.push(b'\n');
        let result = self
            .connection
            .runtime
            .run(self.connection.timeout, Effect::Unknown, async {
                self.shell
                    .write_stdin(bytes)
                    .await
                    .map_err(|e| error("hid_report", e, Effect::Unknown))
            });
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    fn report(&mut self, dx: Delta, dy: Delta, wheel: Delta, pan: Delta) -> Result<Receipt> {
        self.send(serde_json::json!({"id":1,"command":"report","report":[self.buttons,dx.0 as u8,dy.0 as u8,wheel.0 as u8,pan.0 as u8]}))?;
        Ok(Receipt {
            effect: Effect::Dispatched,
            route: "android.uhid.relative_pointer".into(),
        })
    }
    /// Move with current buttons held. With no buttons this is native hover.
    pub fn move_relative(&mut self, dx: Delta, dy: Delta) -> Result<Receipt> {
        self.report(dx, dy, Delta::default(), Delta::default())
    }
    pub fn scroll(&mut self, vertical: Delta, horizontal: Delta) -> Result<Receipt> {
        self.report(Delta::default(), Delta::default(), vertical, horizontal)
    }
    pub fn button(&mut self, button: Button, pressed: bool) -> Result<Receipt> {
        if pressed {
            self.buttons |= button.mask();
        } else {
            self.buttons &= !button.mask();
        }
        self.report(
            Delta::default(),
            Delta::default(),
            Delta::default(),
            Delta::default(),
        )
    }
    /// EOF unregisters this device, including any held buttons.
    pub fn close(&mut self) -> Result<()> {
        if self.closed {
            return Ok(());
        }
        let result = self
            .connection
            .runtime
            .run(Duration::from_secs(3), Effect::Unknown, async {
                self.shell
                    .close_stdin()
                    .await
                    .map_err(|e| error("hid_close", e, Effect::Unknown))?;
                let exit = self
                    .shell
                    .wait()
                    .await
                    .map_err(|e| error("hid_close", e, Effect::Unknown))?;
                if exit != Some(0) {
                    return Err(error(
                        "hid_close",
                        format!("exit {exit:?}: {}", self.diagnostics()),
                        Effect::Unknown,
                    ));
                }
                Ok(())
            });
        self.closed = true;
        for task in &self.drains {
            task.abort();
        }
        result
    }
}
impl Drop for Pointer<'_> {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn inventory_diagnostic(dump: &str, name: &str) -> String {
    let headings: Vec<_> = dump
        .lines()
        .filter(|line| {
            let line = line.trim();
            line.contains("Reader State")
                || line.contains("InputReader")
                || line == "Event Hub State:"
        })
        .take(8)
        .map(str::trim)
        .collect();
    let matches: Vec<_> = dump
        .lines()
        .filter(|line| line.contains(name))
        .take(8)
        .map(str::trim)
        .collect();
    format!(
        "inventory bytes={}; headings={headings:?}; own-device matches={matches:?}",
        dump.len()
    )
}

fn registered(dump: &str, name: &str) -> bool {
    let mut in_reader = false;
    for line in dump.lines() {
        let trimmed = line.trim();
        if !in_reader {
            let Some(suffix) = trimmed.strip_prefix("Input Reader State") else {
                continue;
            };
            // AOSP has a bare colon; Samsung adds a parenthesized device count.
            in_reader = suffix == ":" || (suffix.starts_with(" (") && suffix.ends_with("):"));
            continue;
        }
        // dumpsys sections are unindented. Do not accept another subsystem's
        // similarly named device after leaving InputReader's inventory.
        if !line.is_empty() && !line.starts_with(char::is_whitespace) {
            break;
        }
        if trimmed.strip_prefix("Device ").is_some_and(|device| {
            device
                .split_once(": ")
                .is_some_and(|(id, found)| id.parse::<u32>().is_ok() && found == name)
        }) {
            return true;
        }
    }
    false
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_out_of_descriptor_values() {
        assert!(Delta::new(-128).is_err());
        assert!(Delta::new(-127).is_ok());
    }
    #[test]
    fn accepts_samsung_reader_heading_without_crossing_sections() {
        assert!(registered(
            "Event Hub State:\n  14: pointer\nInput Reader State (Nums of device: 9):\n  Device 11: pointer\n",
            "pointer"
        ));
        assert!(!registered(
            "Input Reader State (Nums of device: 9):\n  Device 11: other\nInput Dispatcher State:\n  Device 12: pointer\n",
            "pointer"
        ));
        assert!(!registered(
            "Input Reader StateExtra:\n  Device 11: pointer\n",
            "pointer"
        ));
        assert!(!registered(
            "Input Reader State:\n  Device invalid: pointer\n",
            "pointer"
        ));
    }
    #[test]
    fn requires_framework_device_not_kernel_inventory() {
        assert!(!registered(
            "Event Hub State:\n Device 1: example",
            "example"
        ));
        assert!(registered(
            "Input Reader State:\n  Device 12: example\n",
            "example"
        ));
        assert!(!registered(
            "Input Reader State:\n  Device 12: example-other\n",
            "example"
        ));
    }
}

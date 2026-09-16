//! Installed CoreSimulator lifecycle operations. No runtime downloads or implicit device selection.
use actuate::{Effect, NativeError, Result};
use serde_json::Value;
use std::{
    ffi::OsStr,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

const INSTALLED_SIMCTL: &str =
    "/Library/Developer/PrivateFrameworks/CoreSimulator.framework/Versions/A/Resources/bin/simctl";
const OUTPUT_LIMIT: usize = 16 * 1024 * 1024;

/// A caller-selected device set is never recursively deleted by this type.
/// Create a temporary set, explicitly shut down and delete its devices, then let
/// the owner remove its own temporary directory. Dropping this value does no I/O.
#[derive(Clone, Debug)]
pub struct Simctl {
    executable: PathBuf,
    device_set: Option<PathBuf>,
    timeout: Duration,
}

impl Simctl {
    /// Uses the already installed binary directly, avoiding xcrun setup prompts.
    pub fn installed() -> Result<Self> {
        let executable = PathBuf::from(INSTALLED_SIMCTL);
        if !executable.is_file() {
            return Err(error(
                "simctl_unavailable",
                "Installed CoreSimulator simctl was not found",
                Effect::None,
            ));
        }
        Ok(Self {
            executable,
            device_set: None,
            timeout: Duration::from_secs(60),
        })
    }

    pub fn with_set(mut self, path: impl AsRef<Path>) -> Self {
        self.device_set = Some(path.as_ref().to_owned());
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Returns native JSON including devices, device types and runtimes unchanged.
    pub fn list(&self) -> Result<Value> {
        let output = self.run([OsStr::new("list"), OsStr::new("--json")], false)?;
        serde_json::from_str(&output)
            .map_err(|e| error("simctl_invalid_json", e.to_string(), Effect::None))
    }

    pub fn create(&self, name: &str, device_type: &str, runtime: &str) -> Result<String> {
        for argument in [name, device_type, runtime] {
            validate_argument(argument)?;
        }
        let output = self.run(["create", name, device_type, runtime].map(OsStr::new), true)?;
        let udid = output.trim();
        validate_udid(udid).map_err(|_| error("simctl_invalid_udid", "Create completed without a valid device UUID; inspect the device set before retrying", Effect::Unknown))?;
        Ok(udid.to_owned())
    }

    pub fn boot(&self, udid: &str) -> Result<String> {
        self.device_command("boot", udid)
    }
    pub fn shutdown(&self, udid: &str) -> Result<String> {
        self.device_command("shutdown", udid)
    }
    pub fn delete(&self, udid: &str) -> Result<String> {
        self.device_command("delete", udid)
    }

    /// Wait for the selected device to finish booting, bounded by this instance's timeout.
    pub fn boot_status(&self, udid: &str) -> Result<String> {
        validate_udid(udid)?;
        self.run(
            ["bootstatus", udid]
                .map(OsStr::new)
                .into_iter()
                .chain([OsStr::new("-b")]),
            true,
        )
    }

    pub fn launch(&self, udid: &str, bundle: &str) -> Result<String> {
        validate_udid(udid)?;
        validate_argument(bundle)?;
        self.run(["launch", udid, bundle].map(OsStr::new), true)
    }

    /// Native PNG screenshot; simctl determines display pixels and orientation.
    pub fn screenshot(&self, udid: &str, path: impl AsRef<Path>) -> Result<String> {
        validate_udid(udid)?;
        let path = path.as_ref();
        if !path.is_absolute() {
            return Err(error(
                "invalid_path",
                "Screenshot path must be absolute",
                Effect::None,
            ));
        }
        self.run(
            [
                OsStr::new("io"),
                OsStr::new(udid),
                OsStr::new("screenshot"),
                OsStr::new("--type=png"),
                path.as_os_str(),
            ],
            true,
        )
    }

    fn device_command(&self, command: &str, udid: &str) -> Result<String> {
        validate_udid(udid)?;
        self.run([command, udid].map(OsStr::new), true)
    }

    fn run<'a>(
        &self,
        arguments: impl IntoIterator<Item = &'a OsStr>,
        mutation: bool,
    ) -> Result<String> {
        let effect = if mutation {
            Effect::Unknown
        } else {
            Effect::None
        };
        let mut command = Command::new(&self.executable);
        if let Some(set) = &self.device_set {
            command.arg("--set").arg(set);
        }
        command
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let started = Instant::now();
        let mut child = command
            .spawn()
            .map_err(|e| error("simctl_spawn", e.to_string(), Effect::None))?;
        let stdout = drain(child.stdout.take().expect("piped stdout"));
        let stderr = drain(child.stderr.take().expect("piped stderr"));
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if started.elapsed() < self.timeout => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Ok(None) => {
                    let _ = child.kill();
                    // Reap without extending the caller's deadline.
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                    return Err(error(
                        "simctl_timeout",
                        "simctl exceeded its timeout; inspect device state before retrying",
                        effect,
                    ));
                }
                Err(e) => {
                    let _ = child.kill();
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                    return Err(error("simctl_wait", e.to_string(), effect));
                }
            }
        };
        let mut outputs = Vec::new();
        for receiver in [stdout, stderr] {
            let remaining = self.timeout.saturating_sub(started.elapsed());
            let bytes = receiver
                .recv_timeout(remaining)
                .map_err(|_| {
                    error(
                        "simctl_output_timeout",
                        "simctl output stream did not close within the timeout",
                        effect,
                    )
                })?
                .map_err(|e| error("simctl_output", e, effect))?;
            outputs.push(bytes);
        }
        if !status.success() {
            return Err(error(
                "simctl_failed",
                format!("{status}: {}", String::from_utf8_lossy(&outputs[1])),
                effect,
            ));
        }
        String::from_utf8(outputs.remove(0))
            .map_err(|e| error("simctl_encoding", e.to_string(), effect))
    }
}

fn validate_argument(argument: &str) -> Result<()> {
    if argument.is_empty() || argument.starts_with('-') || argument.contains('\0') {
        Err(error(
            "invalid_argument",
            "Argument must be nonempty and must not begin with '-' or contain NUL",
            Effect::None,
        ))
    } else {
        Ok(())
    }
}

fn validate_udid(udid: &str) -> Result<()> {
    let valid = udid.len() == 36
        && udid.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        });
    if valid {
        Ok(())
    } else {
        Err(error(
            "invalid_udid",
            "An explicit simulator UUID is required; aliases such as booted/all are rejected",
            Effect::None,
        ))
    }
}

fn error(code: &str, message: impl Into<String>, effect: Effect) -> NativeError {
    NativeError {
        code: code.to_owned(),
        message: message.into(),
        effect,
    }
}

fn drain(
    mut reader: impl Read + Send + 'static,
) -> mpsc::Receiver<std::result::Result<Vec<u8>, String>> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut output = Vec::new();
        let mut buffer = [0u8; 8192];
        let mut exceeded = false;
        let result = loop {
            match reader.read(&mut buffer) {
                Ok(0) => {
                    break if exceeded {
                        Err("Output exceeded 16 MiB; refusing truncated results".to_owned())
                    } else {
                        Ok(output)
                    };
                }
                Ok(count) => {
                    let remaining = OUTPUT_LIMIT.saturating_sub(output.len());
                    output.extend_from_slice(&buffer[..count.min(remaining)]);
                    exceeded |= count > remaining;
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => break Err(e.to_string()),
            }
        };
        let _ = sender.send(result);
    });
    receiver
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn device_aliases_and_options_are_rejected() {
        for invalid in [
            "booted",
            "all",
            "--help",
            "",
            "12345678-1234-1234-1234-12345678901z",
        ] {
            assert!(validate_udid(invalid).is_err());
        }
        assert!(validate_udid("12345678-1234-1234-ABCD-123456789012").is_ok());
    }
    #[test]
    fn drain_preserves_output() {
        assert_eq!(
            drain(std::io::Cursor::new(b"native output".to_vec()))
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .unwrap(),
            b"native output"
        );
    }
    #[test]
    fn oversized_output_is_reported_not_truncated() {
        let output = drain(std::io::Cursor::new(vec![b'x'; OUTPUT_LIMIT + 1]))
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        assert!(output.unwrap_err().contains("exceeded"));
    }

    #[test]
    #[cfg(unix)]
    fn mutations_timeout_with_unknown_effect() {
        let simctl = Simctl {
            executable: PathBuf::from("/bin/sleep"),
            device_set: None,
            timeout: Duration::from_millis(30),
        };
        let start = Instant::now();
        let error = simctl.run([OsStr::new("5")], true).unwrap_err();
        assert_eq!(error.code, "simctl_timeout");
        assert!(matches!(error.effect, Effect::Unknown));
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}

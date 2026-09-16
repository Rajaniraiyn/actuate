//! Shared runtime and bounded shell transport for USB, TCP and paired TLS.
use crate::{CommandTransport, error as fail};
use actuate::{Effect, Result};
use droidmux::client::AdbClient;
use std::{io::Write, time::Duration};
pub(crate) struct Runtime(Option<tokio::runtime::Runtime>);
impl Runtime {
    pub(crate) fn new() -> Result<Self> {
        if tokio::runtime::Handle::try_current().is_ok() {
            return Err(fail(
                "async_runtime_active",
                "Use the asynchronous DroidMux APIs inside async applications",
                Effect::None,
            ));
        }
        Ok(Self(Some(
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| fail("android_runtime", e, Effect::None))?,
        )))
    }
    pub(crate) fn run<T>(
        &self,
        timeout: Duration,
        effect: Effect,
        f: impl std::future::Future<Output = Result<T>>,
    ) -> Result<T> {
        if tokio::runtime::Handle::try_current().is_ok() {
            return Err(fail(
                "async_runtime_active",
                "Synchronous Android adapter cannot run inside an async runtime",
                Effect::None,
            ));
        }
        self.0.as_ref().expect("owned runtime").block_on(async {
            tokio::time::timeout(timeout, f)
                .await
                .map_err(|_| fail("android_timeout", "Android operation timed out", effect))?
        })
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        if let Some(r) = self.0.take() {
            r.shutdown_background();
        }
    }
}
pub struct Connection {
    pub(crate) client: AdbClient,
    pub(crate) runtime: Runtime,
    pub(crate) timeout: Duration,
}
impl CommandTransport for Connection {
    fn execute(
        &mut self,
        command: &str,
        stdout: &mut dyn Write,
        stderr: &mut dyn Write,
    ) -> Result<Option<u8>> {
        let output = self.runtime.run(self.timeout, Effect::Unknown, async {
            droidmux::shell::execute_with_options(
                &self.client,
                command,
                droidmux::shell::ShellOptions {
                    max_output_bytes: Some(32 * 1024 * 1024),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| fail("android_shell", e, Effect::Unknown))
        })?;
        stdout
            .write_all(&output.stdout)
            .map_err(|e| fail("android_output", e, Effect::Unknown))?;
        stderr
            .write_all(&output.stderr)
            .map_err(|e| fail("android_output", e, Effect::Unknown))?;
        Ok(output.exit_code)
    }
}

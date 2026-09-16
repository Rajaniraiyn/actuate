//! Shared command adapter. Native provider sessions own handles and operations.
use super::*;
use serde_json::json;

#[cfg(target_os = "linux")]
use linux::LinuxSession as NativeSession;
#[cfg(target_os = "windows")]
use windows::WindowsSession as NativeSession;

pub(super) fn run_host(
    command: Command,
    connection: Connection,
    format: unimation::OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let host_provider = match connection.provider {
        Provider::Native => true,
        #[cfg(target_os = "windows")]
        Provider::Windows => true,
        #[cfg(target_os = "linux")]
        Provider::Linux => true,
        #[cfg(any(feature = "android", feature = "idevice"))]
        _ => false,
    };
    if !host_provider {
        return Err("Selected provider is not the native desktop provider".into());
    }
    if connection.device.is_some()
        || connection.device_set.is_some()
        || connection.credentials.is_some()
        || connection.trust_first_connection
    {
        return Err(
            "Device and pairing options do not apply to the native desktop provider".into(),
        );
    }
    let mut session = NativeSession::new()?;
    match command {
        Command::Session {} => serve_session(format, |request| session.dispatch(request)),
        Command::Discover { scope } => {
            let raw = session.dispatch(json!({"op":"discover"}))?;
            let scope = match scope {
                DiscoveryScope::All => unimation::discovery::DiscoveryScope::All,
                DiscoveryScope::Apps => unimation::discovery::DiscoveryScope::Apps,
            };
            emit_value(
                &unimation::discovery::present_discovery(&raw, scope, format),
                format,
            )?;
            Ok(())
        }
        Command::Observe {
            pid,
            max_nodes,
            max_depth,
            interactive,
            hide_hidden,
            limit,
            scope,
        } => {
            if scope != SnapshotScope::Application {
                return Err(
                    "Native baseline currently supports application snapshot scope only".into(),
                );
            }
            let pid = match pid {
                Some(pid) => pid,
                None => session.dispatch(json!({"op":"discover"}))?["active_pid"]
                    .as_i64()
                    .and_then(|pid| i32::try_from(pid).ok())
                    .ok_or("No foreground PID is known; specify a PID")?,
            };
            let value = session.dispatch(json!({"op":"snapshot", "request": {"pid":pid,"max_nodes":max_nodes,"max_depth":max_depth}}))?;
            let snapshot = serde_json::from_value(value)?;
            emit_snapshot(
                &snapshot,
                format,
                &unimation::presentation::PresentationOptions {
                    actionable_only: interactive,
                    hide_known_hidden: hide_hidden,
                    max_nodes: limit,
                    ..Default::default()
                },
            )
        }
        Command::Capture {
            path,
            display,
            window,
            backend,
            max_pixel_edge,
        } => {
            if display.is_some()
                || window.is_some()
                || backend != CaptureRoute::Native
                || max_pixel_edge.is_some()
            {
                return Err("Native baseline capture supports the desktop at native resolution; alternate selections and resizing are unavailable".into());
            }
            emit_value(
                &session.dispatch(json!({"op":"capture","path":path}))?,
                format,
            )?;
            Ok(())
        }
        Command::Displays | Command::Windows | Command::Capabilities => {
            let op = match command {
                Command::Displays => "displays",
                Command::Windows => "windows",
                _ => "capabilities",
            };
            emit_value(&session.dispatch(json!({"op":op}))?, format)?;
            Ok(())
        }
        _ => Err("This command is unavailable for the selected native desktop provider".into()),
    }
}

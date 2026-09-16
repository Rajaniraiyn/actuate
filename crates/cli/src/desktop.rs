//! Windows host adapter. The native session owns handles and operations;
//! this module only selects commands and presents replies.
use super::*;
use serde_json::json;
use windows::WindowsSession;

pub(super) fn run_host(
    command: Command,
    connection: Connection,
    format: unimation::OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    if !matches!(connection.provider, Provider::Native | Provider::Windows) {
        return Err("Selected provider is not the Windows host".into());
    }
    if connection.device.is_some() || connection.device_set.is_some() {
        return Err(
            "--device and --device-set select mobile providers; the Windows host takes neither"
                .into(),
        );
    }
    let mut session = WindowsSession::new()?;
    match command {
        Command::Session {} => {
            unimation::transport::serve(
                std::io::stdin().lock(),
                std::io::stdout().lock(),
                format,
                |request| {
                    session
                        .dispatch(request)
                        .map(unimation::transport::ValueReply::from)
                },
            )?;
            Ok(())
        }
        Command::Discover { scope } => {
            let raw = session.dispatch(json!({"op":"discover"}))?;
            let scope = match scope {
                DiscoveryScope::All => unimation::discovery::DiscoveryScope::All,
                DiscoveryScope::Apps => unimation::discovery::DiscoveryScope::Apps,
            };
            emit_pretty(
                &unimation::discovery::present_discovery(&raw, scope, format),
                format,
            )
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
                return Err("The Windows provider supports application snapshot scope only".into());
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
            require_plain_capture(display, window, backend, max_pixel_edge, "Windows")?;
            emit_pretty(
                &session.dispatch(json!({"op":"capture","path":path}))?,
                format,
            )
        }
        Command::Displays | Command::Windows | Command::Capabilities => {
            let op = match command {
                Command::Displays => "displays",
                Command::Windows => "windows",
                _ => "capabilities",
            };
            emit_pretty(&session.dispatch(json!({"op":op}))?, format)
        }
        _ => Err("This command is unavailable for the Windows provider".into()),
    }
}

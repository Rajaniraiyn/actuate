use usage::{Cli, Subcommands};

#[derive(Cli)]
#[usage(bin = "unimation", version)]
struct App {
    #[usage(subcommand)]
    command: Command,
}
#[derive(Subcommands)]
enum Command {
    /// List running applications and accessibility permission state.
    Discover {
        #[usage(long, default = "json")]
        format: String,
        #[usage(long, default = "all")]
        scope: String,
    },
    /// Read the native accessibility tree for an application.
    #[usage(visible_alias = "snapshot")]
    Observe {
        pid: i32,
        #[usage(long, default = "1000")]
        max_nodes: usize,
        #[usage(long, default = "30")]
        max_depth: usize,
        #[usage(long, default = "json")]
        format: String,
        #[usage(long)]
        interactive: bool,
        #[usage(long)]
        hide_hidden: bool,
        #[usage(long, default = "200")]
        limit: usize,
        #[usage(long, default = "application")]
        scope: String,
    },
    /// Render a saved native snapshot without changing its references.
    View {
        snapshot: std::path::PathBuf,
        #[usage(long, default = "text")]
        format: String,
        #[usage(long)]
        root: Option<String>,
        #[usage(long)]
        interactive: bool,
        #[usage(long)]
        hide_hidden: bool,
        #[usage(long, default = "200")]
        limit: usize,
    },
    /// Capture the main display or an explicitly selected display/window.
    Capture {
        path: std::path::PathBuf,
        #[usage(long)]
        display: Option<u32>,
        #[usage(long)]
        window: Option<u32>,
        #[usage(long, default = "native")]
        backend: String,
        #[usage(long)]
        max_pixel_edge: Option<u32>,
    },
    /// List native display geometry.
    Displays,
    /// List native window IDs, owners and geometry.
    Windows,
    /// Report delivery routes and availability.
    Capabilities,
    /// Compare two snapshots saved from the same live session.
    Diff {
        before: std::path::PathBuf,
        after: std::path::PathBuf,
        #[usage(long)]
        modified_only: bool,
        #[usage(long, default = "json")]
        format: String,
    },
    /// Query a saved snapshot without discarding fields from matching nodes.
    Query {
        snapshot: std::path::PathBuf,
        #[usage(long)]
        role: Option<String>,
        #[usage(long)]
        name: Option<String>,
        #[usage(long)]
        action: Option<String>,
    },
    /// Export the portable Usage command specification.
    Spec,
    /// Show JSON session commands and delivery semantics.
    Protocol,
    /// Read JSON requests from stdin, retaining references until EOF.
    Session {
        /// JSONL is the machine protocol; text is a framed interactive transcript.
        #[usage(long, default = "json")]
        format: String,
    },
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = App::parse();
    if matches!(app.command, Command::Protocol) {
        println!("{}", include_str!("../../../docs/session.md"));
        return Ok(());
    }
    if matches!(app.command, Command::Spec) {
        print!("{}", App::to_kdl());
        return Ok(());
    }
    match app.command {
        Command::Diff {
            before,
            after,
            modified_only,
            format,
        } => {
            let before: unimation_core::Snapshot =
                serde_json::from_reader(std::fs::File::open(before)?)?;
            let after: unimation_core::Snapshot =
                serde_json::from_reader(std::fs::File::open(after)?)?;
            let diff = unimation_core::diff::diff_snapshots(&before, &after)?;
            let format = parse_format(&format)?;
            if format == unimation_core::OutputFormat::Compact {
                if modified_only {
                    return Err("--modified-only selects native fields; use --format json or text for that mode".into());
                }
                println!(
                    "{}",
                    serde_json::json!({
                        "root": before.root, "before_revision": before.revision, "after_revision": after.revision,
                        "text": unimation_core::presentation::render_view_diff(&before, &after, &Default::default(), 100)?
                    })
                );
                return Ok(());
            }
            if format == unimation_core::OutputFormat::Text && !modified_only {
                print!(
                    "{}",
                    unimation_core::presentation::render_view_diff(
                        &before,
                        &after,
                        &Default::default(),
                        100
                    )?
                );
                return Ok(());
            }
            if format == unimation_core::OutputFormat::Text {
                let mut diff = diff;
                if modified_only {
                    diff.newly_observed.clear();
                    diff.removed_from_scope.clear();
                    diff.no_longer_observed.clear();
                }
                print!(
                    "{}",
                    unimation_core::presentation::render_diff_text(&diff, 160)
                );
                return Ok(());
            }
            let value = if modified_only {
                serde_json::json!({"before_revision":diff.before_revision,"after_revision":diff.after_revision,"modified":diff.modified,"before_coverage":diff.before_coverage,"after_coverage":diff.after_coverage})
            } else {
                serde_json::to_value(diff)?
            };
            println!("{}", serde_json::to_string_pretty(&value)?);
            Ok(())
        }
        Command::View {
            snapshot,
            format,
            root,
            interactive,
            hide_hidden,
            limit,
        } => {
            let snapshot: unimation_core::Snapshot =
                serde_json::from_reader(std::fs::File::open(snapshot)?)?;
            let root = root
                .map(|r| parse_short_ref(&snapshot.root.session, &r))
                .transpose()?;
            emit_snapshot(
                &snapshot,
                parse_format(&format)?,
                &unimation_core::presentation::PresentationOptions {
                    root,
                    actionable_only: interactive,
                    hide_known_hidden: hide_hidden,
                    max_nodes: limit,
                    ..Default::default()
                },
            )
        }
        Command::Query {
            snapshot,
            role,
            name,
            action,
        } => {
            let snapshot: unimation_core::Snapshot =
                serde_json::from_reader(std::fs::File::open(snapshot)?)?;
            let query = unimation_core::query::NodeQuery {
                role: role.map(|value| unimation_core::query::TextMatch::Exact { value }),
                name: name.map(|value| unimation_core::query::TextMatch::Contains { value }),
                action,
                ..Default::default()
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&unimation_core::query::query_nodes(
                    &snapshot, &query
                ))?
            );
            Ok(())
        }
        command => run(command),
    }
}
#[cfg(not(target_os = "macos"))]
fn run(_: Command) -> Result<(), Box<dyn std::error::Error>> {
    Err("No provider implemented for this OS yet".into())
}
#[cfg(target_os = "macos")]
fn run(command: Command) -> Result<(), Box<dyn std::error::Error>> {
    use unimation_core::{Discover, ObserveRequest};
    let mut ax = unimation_macos::Accessibility::new();
    let result = match command {
        Command::Discover { format, scope } => {
            let format = parse_format(&format)?;
            let scope = match scope.as_str() {
                "all" => unimation_core::discovery::DiscoveryScope::All,
                "apps" => unimation_core::discovery::DiscoveryScope::Apps,
                _ => return Err("--scope must be apps or all".into()),
            };
            let value =
                unimation_core::discovery::present_discovery(&ax.discover()?, scope, format);
            if let Some(text) = value.as_str() {
                print!("{text}");
            } else if format == unimation_core::OutputFormat::Compact {
                println!("{}", serde_json::to_string(&value)?);
            } else {
                println!("{}", serde_json::to_string_pretty(&value)?);
            }
            return Ok(());
        }
        Command::Observe {
            pid,
            max_nodes,
            max_depth,
            format,
            interactive,
            hide_hidden,
            limit,
            scope,
        } => {
            let format = parse_format(&format)?;
            let scope = match scope.as_str() {
                "application" => unimation_macos::session::SnapshotScope::Application,
                "window" => unimation_macos::session::SnapshotScope::FocusedWindow,
                _ => return Err("--scope must be application or window".into()),
            };
            let value = unimation_macos::session::MacSession::new().extension(
                unimation_macos::session::MacRequest::Snapshot {
                    request: ObserveRequest {
                        pid,
                        max_nodes,
                        max_depth,
                    },
                    scope,
                    format,
                    options: unimation_core::presentation::PresentationOptions {
                        actionable_only: interactive,
                        hide_known_hidden: hide_hidden,
                        max_nodes: limit,
                        ..Default::default()
                    },
                },
            )?;
            if let Some(text) = value.as_str() {
                print!("{text}");
            } else if format == unimation_core::OutputFormat::Compact {
                println!("{}", serde_json::to_string(&value)?);
            } else {
                println!("{}", serde_json::to_string_pretty(&value)?);
            }
            return Ok(());
        }
        Command::Capture {
            path,
            display,
            window,
            backend,
            max_pixel_edge,
        } => {
            if display.is_some() && window.is_some() {
                return Err("Choose either --display or --window".into());
            }
            let source = match window {
                Some(window_id) => unimation_macos::capture::CaptureSource::Window { window_id },
                None => unimation_macos::capture::CaptureSource::Display {
                    display_id: display.unwrap_or_else(unimation_macos::capture::main_display_id),
                },
            };
            unimation_macos::session::MacSession::new().extension(
                unimation_macos::session::MacRequest::Capture {
                    source,
                    path,
                    backend: match backend.as_str() {
                        "native" => unimation_macos::session::CaptureBackend::Native,
                        "executable" => unimation_macos::session::CaptureBackend::Executable,
                        _ => return Err("--backend must be native or executable".into()),
                    },
                    max_pixel_edge,
                },
            )?
        }
        Command::Displays => unimation_macos::session::MacSession::new()
            .extension(unimation_macos::session::MacRequest::Displays {})?,
        Command::Windows => unimation_macos::session::MacSession::new()
            .extension(unimation_macos::session::MacRequest::Windows {})?,
        Command::Capabilities => unimation_macos::session::MacSession::new()
            .extension(unimation_macos::session::MacRequest::Capabilities {})?,
        Command::Session { format } => {
            return session(parse_format(&format)?);
        }
        Command::Spec
        | Command::Protocol
        | Command::Diff { .. }
        | Command::Query { .. }
        | Command::View { .. } => {
            unreachable!()
        }
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
#[cfg(target_os = "macos")]
fn session(format: unimation_core::OutputFormat) -> Result<(), Box<dyn std::error::Error>> {
    use serde_json::{Value, json};
    use std::io::{BufRead, Write};
    let mut runtime = unimation_macos::session::MacSession::new();
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        let request = serde_json::from_str::<Value>(&line);
        let id = request.as_ref().ok().and_then(|v| v.get("id")).cloned();
        let result = match request {
            Ok(r) => runtime.dispatch(r),
            Err(e) => Err(unimation_core::NativeError {
                code: "invalid_request".into(),
                message: e.to_string(),
                effect: unimation_core::Effect::None,
            }),
        };
        let mut reply = match result {
            Ok(v) => json!({"result":v}),
            Err(e) => json!({"error":e}),
        };
        if let Some(id) = id {
            reply["id"] = id;
        }
        if format == unimation_core::OutputFormat::Text {
            println!(
                "--- response id={} ---",
                reply.get("id").unwrap_or(&Value::Null)
            );
            if let Some(result) = reply.get("result") {
                if let Some(text) = result.as_str() {
                    println!("{text}");
                } else if let Ok(snapshot) =
                    serde_json::from_value::<unimation_core::Snapshot>(result.clone())
                {
                    emit_snapshot(&snapshot, format, &Default::default())?;
                } else {
                    println!("{}", serde_json::to_string(result)?);
                }
            } else {
                println!("{}", serde_json::to_string(&reply)?);
            }
            println!("--- end ---");
        } else {
            if format == unimation_core::OutputFormat::Compact
                && let Some(result) = reply.get_mut("result")
                && let Ok(snapshot) =
                    serde_json::from_value::<unimation_core::Snapshot>(result.clone())
            {
                *result = serde_json::to_value(unimation_core::presentation::render_snapshot(
                    &snapshot,
                    &Default::default(),
                )?)?;
            }
            println!("{}", serde_json::to_string(&reply)?);
        }
        std::io::stdout().flush()?;
    }
    Ok(())
}

fn parse_format(value: &str) -> Result<unimation_core::OutputFormat, Box<dyn std::error::Error>> {
    match value {
        "json" => Ok(unimation_core::OutputFormat::Json),
        "compact" => Ok(unimation_core::OutputFormat::Compact),
        "text" => Ok(unimation_core::OutputFormat::Text),
        _ => Err("--format must be text, compact, or json".into()),
    }
}
fn parse_short_ref(
    session: &str,
    value: &str,
) -> Result<unimation_core::ElementRef, Box<dyn std::error::Error>> {
    let id = value
        .strip_prefix("@e")
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .ok_or("--root must be @e<number> from the saved snapshot")?
        .parse::<u64>()?;
    Ok(unimation_core::ElementRef {
        session: session.into(),
        id,
    })
}
fn emit_snapshot(
    snapshot: &unimation_core::Snapshot,
    format: unimation_core::OutputFormat,
    options: &unimation_core::presentation::PresentationOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    match format {
        unimation_core::OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(snapshot)?)
        }
        unimation_core::OutputFormat::Compact => println!(
            "{}",
            serde_json::to_string(&unimation_core::presentation::render_snapshot(
                snapshot, options
            )?)?
        ),
        unimation_core::OutputFormat::Text => print!(
            "{}",
            unimation_core::presentation::render_snapshot_text(
                &unimation_core::presentation::render_snapshot(snapshot, options)?
            )
        ),
    }
    Ok(())
}

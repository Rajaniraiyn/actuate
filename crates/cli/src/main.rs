use usage::{Cli, Subcommands, ValueEnum};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Provider {
    Native,
    #[cfg(target_os = "macos")]
    Macos,
    #[cfg(target_os = "macos")]
    Ios,
}
#[derive(Clone, Copy, Debug, ValueEnum)]
enum Format {
    Json,
    Compact,
    Text,
}
impl From<Format> for unimation::OutputFormat {
    fn from(value: Format) -> Self {
        match value {
            Format::Json => Self::Json,
            Format::Compact => Self::Compact,
            Format::Text => Self::Text,
        }
    }
}
#[derive(Clone, Copy, Debug, ValueEnum)]
enum DiscoveryScope {
    All,
    Apps,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum SnapshotScope {
    Application,
    Window,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum CaptureRoute {
    Native,
    Executable,
}

#[derive(Cli)]
#[usage(bin = "unimation", version, completion)]
struct App {
    /// Automation provider: native host, macos, or ios.
    #[usage(
        short = 'p',
        long,
        global,
        default = "native",
        env = "UNIMATION_PROVIDER",
        value_enum
    )]
    provider: Provider,
    /// Explicit provider device UUID; no implicit first-device selection.
    #[usage(long, global, env = "UNIMATION_DEVICE")]
    device: Option<String>,
    /// Existing simulator device set, required for the iOS provider.
    #[usage(long, global, env = "UNIMATION_DEVICE_SET", value_hint = usage::ValueHint::DirPath)]
    device_set: Option<std::path::PathBuf>,
    /// Output format. Text is plain and identical in terminals and pipes.
    #[usage(long, global, default = "text", value_enum)]
    format: Format,
    /// Emit full JSON; sessions emit one JSON response per line.
    #[usage(long, global, conflicts = "format")]
    json: bool,
    #[usage(subcommand)]
    command: Command,
}
#[derive(Clone, Copy, Debug, ValueEnum)]
enum Shell {
    Bash,
    Zsh,
    Fish,
}
#[derive(Subcommands)]
enum Command {
    /// Generate a shell completion script using the command specification.
    Completions {
        #[usage(value_enum)]
        shell: Shell,
    },
    /// iOS-specific resource discovery; interaction uses the shared commands.
    #[cfg(target_os = "macos")]
    Ios {
        #[usage(subcommand)]
        command: IosCommand,
    },
    /// List running applications and accessibility permission state.
    Discover {
        #[usage(long, default = "all", value_enum)]
        scope: DiscoveryScope,
    },
    /// Read the native accessibility tree for an application.
    #[usage(visible_alias = "snapshot")]
    Observe {
        pid: Option<i32>,
        #[usage(long, default = "1000")]
        max_nodes: usize,
        #[usage(long, default = "30")]
        max_depth: usize,
        #[usage(long)]
        interactive: bool,
        #[usage(long)]
        hide_hidden: bool,
        #[usage(long, default = "200")]
        limit: usize,
        #[usage(long, default = "application", value_enum)]
        scope: SnapshotScope,
    },
    /// Render a saved native snapshot without changing its references.
    View {
        snapshot: std::path::PathBuf,
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
        #[usage(long, default = "native", value_enum)]
        backend: CaptureRoute,
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
    Session {},
}
#[cfg(target_os = "macos")]
#[derive(Subcommands)]
enum IosCommand {
    /// Inspect installed simulators without booting or installing anything.
    Simulators {
        #[usage(subcommand)]
        command: SimulatorCommand,
    },
}
#[cfg(target_os = "macos")]
#[derive(Subcommands)]
enum SimulatorCommand {
    /// List native runtime, device-type and device records.
    List,
}
struct Connection {
    provider: Provider,
    device: Option<String>,
    device_set: Option<std::path::PathBuf>,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = App::parse();
    let format = if app.json {
        unimation::OutputFormat::Json
    } else {
        app.format.into()
    };
    if let Command::Completions { shell } = app.command {
        let shell = match shell {
            Shell::Bash => usage::complete::Shell::Bash,
            Shell::Zsh => usage::complete::Shell::Zsh,
            Shell::Fish => usage::complete::Shell::Fish,
        };
        print!("{}", App::completion_script(shell));
        return Ok(());
    }
    if matches!(app.command, Command::Protocol) {
        println!("{}", include_str!("../../../docs/session.md"));
        return Ok(());
    }
    if matches!(app.command, Command::Spec) {
        print!("{}", App::to_kdl());
        return Ok(());
    }
    let connection = Connection {
        provider: app.provider,
        device: app.device,
        device_set: app.device_set,
    };
    match app.command {
        Command::Diff {
            before,
            after,
            modified_only,
        } => {
            let before: unimation::Snapshot =
                serde_json::from_reader(std::fs::File::open(before)?)?;
            let after: unimation::Snapshot = serde_json::from_reader(std::fs::File::open(after)?)?;
            let diff = unimation::diff::diff_snapshots(&before, &after)?;
            if format == unimation::OutputFormat::Compact {
                if modified_only {
                    return Err("--modified-only selects native fields; use --format json or text for that mode".into());
                }
                println!(
                    "{}",
                    serde_json::json!({
                        "root": before.root, "before_revision": before.revision, "after_revision": after.revision,
                        "text": unimation::presentation::render_view_diff(&before, &after, &Default::default(), 100)?
                    })
                );
                return Ok(());
            }
            if format == unimation::OutputFormat::Text && !modified_only {
                print!(
                    "{}",
                    unimation::presentation::render_view_diff(
                        &before,
                        &after,
                        &Default::default(),
                        100
                    )?
                );
                return Ok(());
            }
            if format == unimation::OutputFormat::Text {
                let mut diff = diff;
                if modified_only {
                    diff.newly_observed.clear();
                    diff.removed_from_scope.clear();
                    diff.no_longer_observed.clear();
                }
                print!("{}", unimation::presentation::render_diff_text(&diff, 160));
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
            root,
            interactive,
            hide_hidden,
            limit,
        } => {
            let snapshot: unimation::Snapshot =
                serde_json::from_reader(std::fs::File::open(snapshot)?)?;
            let root = root
                .map(|r| parse_short_ref(&snapshot.root.session, &r))
                .transpose()?;
            emit_snapshot(
                &snapshot,
                format,
                &unimation::presentation::PresentationOptions {
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
            let snapshot: unimation::Snapshot =
                serde_json::from_reader(std::fs::File::open(snapshot)?)?;
            let query = unimation::query::NodeQuery {
                role: role.map(|value| unimation::query::TextMatch::Exact { value }),
                name: name.map(|value| unimation::query::TextMatch::Contains { value }),
                action,
                ..Default::default()
            };
            emit_value(
                &serde_json::to_value(unimation::query::query_nodes(&snapshot, &query))?,
                format,
            )?;
            Ok(())
        }
        command => run(command, connection, format),
    }
}
#[cfg(not(target_os = "macos"))]
fn run(
    _: Command,
    _: Connection,
    _: unimation::OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    Err("No provider implemented for this OS yet".into())
}
#[cfg(target_os = "macos")]
fn run(
    command: Command,
    connection: Connection,
    format: unimation::OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    use unimation::{Discover, ObserveRequest};
    if let Command::Ios {
        command: IosCommand::Simulators {
            command: SimulatorCommand::List,
        },
    } = command
    {
        if connection.device.is_some() {
            return Err("Simulator listing does not select a device; omit --device".into());
        }
        let mut sim = ios::simulator::Simctl::installed()?;
        if let Some(path) = connection.device_set {
            sim = sim.with_set(path);
        }
        let inventory = sim.list()?;
        if format == unimation::OutputFormat::Text {
            let mut rows = Vec::new();
            for runtime in inventory["runtimes"].as_array().into_iter().flatten() {
                rows.push(serde_json::json!({"runtime":runtime["identifier"],"name":runtime["name"],"available":runtime["isAvailable"]}));
            }
            if let Some(groups) = inventory["devices"].as_object() {
                for (runtime, devices) in groups {
                    for device in devices.as_array().into_iter().flatten() {
                        rows.push(serde_json::json!({"runtime":runtime,"device":device["udid"],"name":device["name"],"state":device["state"],"available":device["isAvailable"]}));
                    }
                }
            }
            if rows.is_empty() {
                println!("No simulator runtimes or devices installed.");
            } else {
                emit_value(&serde_json::Value::Array(rows), format)?;
            }
        } else {
            emit_value(&inventory, format)?;
        }
        return Ok(());
    }
    if connection.provider == Provider::Ios {
        let udid = connection
            .device
            .as_deref()
            .ok_or("--provider ios requires --device UUID")?;
        let set = connection
            .device_set
            .as_deref()
            .ok_or("--provider ios requires --device-set PATH")?;
        let request = match command {
            Command::Session {} => {
                return ios::jsonl::run_formatted(udid, set, format);
            }
            Command::Observe { pid, max_nodes, max_depth, interactive, hide_hidden, limit, scope } => {
                if scope != SnapshotScope::Application { return Err("The iOS provider does not support --scope window; use its explicit observation scopes in a session".into()); }
                ios::session::Request::Snapshot {
                    scope: pid.map(|pid| ios::SimulatorScope::Application {pid}).unwrap_or(ios::SimulatorScope::Frontmost),
                    max_nodes, max_depth, format,
                    options:unimation::presentation::PresentationOptions {actionable_only:interactive,hide_known_hidden:hide_hidden,max_nodes:limit,..Default::default()},
                }
            }
            Command::Capture {path,display,window,backend,max_pixel_edge} => {
                if display.is_some() || window.is_some() || backend != CaptureRoute::Native || max_pixel_edge.is_some() {
                    return Err("The iOS capture provider supports its primary display at native resolution; window/display selection, resizing and alternate capture routes are unavailable".into());
                }
                ios::session::Request::Capture {path}
            }
            Command::Capabilities => ios::session::Request::Capabilities {},
            _ => return Err("This command is not supported by the selected iOS provider; use capabilities for its supported operations".into()),
        };
        let mut session = ios::session::connect(udid, set)?;
        return Ok(ios::jsonl::write_result_formatted(
            &session.execute(request)?,
            format,
            std::io::stdout().lock(),
        )?);
    }
    if connection.device.is_some() || connection.device_set.is_some() {
        return Err(
            "--device and --device-set require --provider ios or ios simulators list".into(),
        );
    }
    let mut ax = macos::Accessibility::new();
    let result = match command {
        Command::Ios { .. } => unreachable!(),
        Command::Discover { scope } => {
            let scope = match scope {
                DiscoveryScope::All => unimation::discovery::DiscoveryScope::All,
                DiscoveryScope::Apps => unimation::discovery::DiscoveryScope::Apps,
            };
            let value = unimation::discovery::present_discovery(&ax.discover()?, scope, format);
            if let Some(text) = value.as_str() {
                print!("{text}");
            } else if format == unimation::OutputFormat::Compact {
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
            interactive,
            hide_hidden,
            limit,
            scope,
        } => {
            let pid = match pid {
                Some(pid) => pid,
                None => ax.discover()?["active_pid"]
                    .as_i64()
                    .and_then(|pid| i32::try_from(pid).ok())
                    .ok_or("No foreground application is available; specify a PID")?,
            };
            let scope = match scope {
                SnapshotScope::Application => macos::session::SnapshotScope::Application,
                SnapshotScope::Window => macos::session::SnapshotScope::FocusedWindow,
            };
            let value = macos::session::MacSession::new().extension(
                macos::session::MacRequest::Snapshot {
                    request: ObserveRequest {
                        pid,
                        max_nodes,
                        max_depth,
                    },
                    scope,
                    format,
                    options: unimation::presentation::PresentationOptions {
                        actionable_only: interactive,
                        hide_known_hidden: hide_hidden,
                        max_nodes: limit,
                        ..Default::default()
                    },
                },
            )?;
            if let Some(text) = value.as_str() {
                print!("{text}");
            } else if format == unimation::OutputFormat::Compact {
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
                Some(window_id) => macos::capture::CaptureSource::Window { window_id },
                None => macos::capture::CaptureSource::Display {
                    display_id: display.unwrap_or_else(macos::capture::main_display_id),
                },
            };
            macos::session::MacSession::new().extension(macos::session::MacRequest::Capture {
                source,
                path,
                backend: match backend {
                    CaptureRoute::Native => macos::session::CaptureBackend::Native,
                    CaptureRoute::Executable => macos::session::CaptureBackend::Executable,
                },
                max_pixel_edge,
            })?
        }
        Command::Displays => {
            macos::session::MacSession::new().extension(macos::session::MacRequest::Displays {})?
        }
        Command::Windows => {
            macos::session::MacSession::new().extension(macos::session::MacRequest::Windows {})?
        }
        Command::Capabilities => macos::session::MacSession::new()
            .extension(macos::session::MacRequest::Capabilities {})?,
        Command::Session {} => {
            return session(format);
        }
        Command::Completions { .. }
        | Command::Spec
        | Command::Protocol
        | Command::Diff { .. }
        | Command::Query { .. }
        | Command::View { .. } => {
            unreachable!()
        }
    };
    emit_value(&result, format)?;
    Ok(())
}
#[cfg(target_os = "macos")]
fn session(format: unimation::OutputFormat) -> Result<(), Box<dyn std::error::Error>> {
    use serde_json::{Value, json};
    use std::io::{BufRead, Write};
    let mut runtime = macos::session::MacSession::new();
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        let request = serde_json::from_str::<Value>(&line);
        let id = request.as_ref().ok().and_then(|v| v.get("id")).cloned();
        let result = match request {
            Ok(r) => runtime.dispatch(r),
            Err(e) => Err(unimation::NativeError {
                code: "invalid_request".into(),
                message: e.to_string(),
                effect: unimation::Effect::None,
            }),
        };
        let mut reply = match result {
            Ok(v) => json!({"result":v}),
            Err(e) => json!({"error":e}),
        };
        if let Some(id) = id {
            reply["id"] = id;
        }
        if format == unimation::OutputFormat::Text {
            println!(
                "--- response id={} ---",
                reply.get("id").unwrap_or(&Value::Null)
            );
            if let Some(result) = reply.get("result") {
                if let Some(text) = result.as_str() {
                    println!("{text}");
                } else if let Ok(snapshot) =
                    serde_json::from_value::<unimation::Snapshot>(result.clone())
                {
                    emit_snapshot(&snapshot, format, &Default::default())?;
                } else {
                    emit_value(result, format)?;
                }
            } else {
                emit_value(&reply, format)?;
            }
            println!("--- end ---");
        } else {
            if format == unimation::OutputFormat::Compact
                && let Some(result) = reply.get_mut("result")
                && let Ok(snapshot) = serde_json::from_value::<unimation::Snapshot>(result.clone())
            {
                *result = serde_json::to_value(unimation::presentation::render_snapshot(
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

fn parse_short_ref(
    session: &str,
    value: &str,
) -> Result<unimation::ElementRef, Box<dyn std::error::Error>> {
    let id = value
        .strip_prefix("@e")
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .ok_or("--root must be @e<number> from the saved snapshot")?
        .parse::<u64>()?;
    Ok(unimation::ElementRef {
        session: session.into(),
        id,
    })
}
fn emit_snapshot(
    snapshot: &unimation::Snapshot,
    format: unimation::OutputFormat,
    options: &unimation::presentation::PresentationOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    match format {
        unimation::OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(snapshot)?)
        }
        unimation::OutputFormat::Compact => println!(
            "{}",
            serde_json::to_string(&unimation::presentation::render_snapshot(
                snapshot, options
            )?)?
        ),
        unimation::OutputFormat::Text => print!(
            "{}",
            unimation::presentation::render_snapshot_text(
                &unimation::presentation::render_snapshot(snapshot, options)?
            )
        ),
    }
    Ok(())
}

fn emit_value(value: &serde_json::Value, format: unimation::OutputFormat) -> std::io::Result<()> {
    unimation::output::write_value(std::io::stdout().lock(), value, format)
}

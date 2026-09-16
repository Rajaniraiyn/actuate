#[cfg(any(target_os = "windows", target_os = "linux"))]
mod desktop;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use desktop::run_host;
use usage::{Cli, Subcommands, ValueEnum};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Provider {
    Native,
    #[cfg(target_os = "windows")]
    Windows,
    #[cfg(target_os = "linux")]
    Linux,
    #[cfg(target_os = "macos")]
    Macos,
    #[cfg(target_os = "macos")]
    #[usage(name = "apple-simulator")]
    Ios,
    #[cfg(feature = "idevice")]
    #[usage(name = "apple-device")]
    Idevice,
    #[cfg(feature = "android")]
    Android,
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
    /// Automation provider; native selects the host, apple-simulator selects CoreSimulator.
    #[usage(
        short = 'p',
        long,
        global,
        default = "native",
        env = "UNIMATION_PROVIDER",
        value_enum
    )]
    provider: Provider,
    /// Provider device identifier: Apple UDID, Android IP:port, or usb:VID:PID.
    #[usage(long, global, env = "UNIMATION_DEVICE")]
    device: Option<String>,
    /// Existing simulator device set, required for apple-simulator.
    #[usage(long, global, env = "UNIMATION_DEVICE_SET", value_hint = usage::ValueHint::DirPath)]
    device_set: Option<std::path::PathBuf>,
    /// Explicit Android credential directory, or PEM key for USB.
    #[usage(long, global, env = "UNIMATION_CREDENTIALS", value_hint = usage::ValueHint::AnyPath)]
    credentials: Option<std::path::PathBuf>,
    /// Record the first wireless public key; subsequent key changes fail.
    #[usage(long, global)]
    trust_first_connection: bool,
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
    /// Android connection setup; device automation uses shared commands.
    #[cfg(feature = "android")]
    Android {
        #[usage(subcommand)]
        command: AndroidCommand,
    },
    /// Generate a shell completion script using the command specification.
    Completions {
        #[usage(value_enum)]
        shell: Shell,
    },
    /// Apple resource discovery; interaction uses the shared commands.
    #[cfg(any(target_os = "macos", feature = "idevice"))]
    Apple {
        #[usage(subcommand)]
        command: AppleCommand,
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
#[cfg(any(target_os = "macos", feature = "idevice"))]
#[derive(Subcommands)]
enum AppleCommand {
    /// Discover physical Apple devices through the available transport.
    #[cfg(feature = "idevice")]
    Devices {
        #[usage(subcommand)]
        command: DeviceCommand,
    },
    /// Inspect installed simulators without booting or installing anything.
    #[cfg(target_os = "macos")]
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
#[cfg(feature = "android")]
#[derive(Subcommands)]
enum AndroidCommand {
    /// Create a persistent private host key for direct USB/classic TCP.
    InitKey { path: std::path::PathBuf },
    /// Pair an Android wireless-debugging endpoint; read the code from stdin.
    Pair { endpoint: std::net::SocketAddr },
    /// Show a QR code, discover the scanner, and pair without an ADB server.
    PairQr {
        #[usage(long)]
        qr_svg: Option<std::path::PathBuf>,
        #[usage(long, default = "120")]
        timeout_secs: u64,
    },
    /// List USB devices without authorizing or selecting one.
    Devices,
}
#[cfg(feature = "idevice")]
#[derive(Subcommands)]
enum DeviceCommand {
    List,
}

struct Connection {
    credentials: Option<std::path::PathBuf>,
    trust_first_connection: bool,
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
        credentials: app.credentials,
        trust_first_connection: app.trust_first_connection,
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
fn run(
    command: Command,
    connection: Connection,
    format: unimation::OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "android")]
    if matches!(command, Command::Android { .. }) || connection.provider == Provider::Android {
        return run_android(command, connection, format);
    }
    if connection.credentials.is_some() || connection.trust_first_connection {
        return Err(
            "--credentials and --trust-first-connection apply to Android connections".into(),
        );
    }
    #[cfg(feature = "idevice")]
    let command = if matches!(
        command,
        Command::Apple {
            command: AppleCommand::Devices {
                command: DeviceCommand::List
            }
        }
    ) {
        if connection.device_set.is_some() || connection.device.is_some() {
            return Err("apple devices list does not select a device or simulator set".into());
        }
        let provider =
            ios::physical::PhysicalCapture::from_env(std::time::Duration::from_secs(10))?;
        return Ok(emit_value(
            &serde_json::to_value(provider.discover()?)?,
            format,
        )?);
    } else {
        command
    };
    #[cfg(feature = "idevice")]
    if connection.provider == Provider::Idevice {
        use unimation::Capture;
        if connection.device_set.is_some() {
            return Err(
                "--device-set selects CoreSimulator storage; omit it for --provider apple-device"
                    .into(),
            );
        }
        match command {
            Command::Capabilities => {
                return Ok(emit_value(&serde_json::to_value(ios::physical::capabilities())?, format)?);
            }
            Command::Discover { scope: DiscoveryScope::All } => {
                let provider = ios::physical::PhysicalCapture::from_env(std::time::Duration::from_secs(10))?;
                if let Some(udid) = connection.device.as_deref() {
                    return Ok(emit_value(&serde_json::to_value(provider.info(udid)?)?, format)?);
                }
                let devices = provider.discover()?;
                if devices.devices.is_empty() && format == unimation::OutputFormat::Text {
                    println!("No physical iOS devices returned by usbmuxd; inventory completeness is unknown.");
                    return Ok(());
                }
                return Ok(emit_value(&serde_json::to_value(devices)?, format)?);
            }
            Command::Capture {path, display, window, backend, max_pixel_edge} => {
                if display.is_some() || window.is_some() || backend != CaptureRoute::Native || max_pixel_edge.is_some() {
                    return Err("apple-device capture returns the device's encoded screenshot; display/window selection, resizing and alternate routes are unavailable".into());
                }
                let udid = connection.device.ok_or("--provider apple-device capture requires --device UDID")?;
                let mut provider = ios::physical::PhysicalCapture::from_env(std::time::Duration::from_secs(10))?;
                let frame = provider.capture(udid)?.save(path)?;
                return Ok(emit_value(&serde_json::to_value(frame)?, format)?);
            }
            _ => return Err("The apple-device provider currently supports discover, capabilities and capture. Accessibility, input, app discovery and interactive sessions are unavailable; CoreSimulator uses --provider apple-simulator.".into()),
        }
    }
    run_host(command, connection, format)
}
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn run_host(
    _: Command,
    _: Connection,
    _: unimation::OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    Err("No provider implemented for this OS yet".into())
}
#[cfg(target_os = "macos")]
fn run_host(
    command: Command,
    connection: Connection,
    format: unimation::OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    use unimation::{Discover, ObserveRequest};
    if let Command::Apple {
        command: AppleCommand::Simulators {
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
            .ok_or("--provider apple-simulator requires --device UUID")?;
        let set = connection
            .device_set
            .as_deref()
            .ok_or("--provider apple-simulator requires --device-set PATH")?;
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
            "--device and --device-set require --provider apple-simulator or apple simulators list"
                .into(),
        );
    }
    let mut ax = macos::Accessibility::new();
    let result = match command {
        Command::Apple { .. } => unreachable!(),
        #[cfg(feature = "android")]
        Command::Android { .. } => unreachable!(),
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
    let mut runtime = macos::session::MacSession::new();
    serve_session(format, |request| runtime.dispatch(request))
}
fn serve_session(
    format: unimation::OutputFormat,
    mut dispatch: impl FnMut(serde_json::Value) -> unimation::Result<serde_json::Value>,
) -> Result<(), Box<dyn std::error::Error>> {
    use serde_json::{Value, json};
    use std::io::{BufRead, Write};
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        let request = serde_json::from_str::<Value>(&line);
        let id = request.as_ref().ok().and_then(|v| v.get("id")).cloned();
        let result = match request {
            Ok(mut r) => {
                if let Some(object) = r.as_object_mut() {
                    object.remove("id");
                }
                dispatch(r)
            }
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

#[cfg(feature = "android")]
fn run_android(
    command: Command,
    connection: Connection,
    format: unimation::OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    use android::wireless::{FirstConnectionPolicy, WirelessHost};
    use std::io::Write;
    use std::time::Duration;
    if connection.device_set.is_some() {
        return Err("--device-set is specific to CoreSimulator".into());
    }
    if matches!(
        command,
        Command::Android {
            command: AndroidCommand::Devices
        }
    ) || matches!(
        command,
        Command::Discover {
            scope: DiscoveryScope::All
        }
    ) && connection.device.is_none()
        && connection.credentials.is_none()
    {
        return Ok(emit_value(
            &serde_json::to_value(android::discover_usb()?)?,
            format,
        )?);
    }
    if let Command::Android {
        command: AndroidCommand::InitKey { path },
    } = command
    {
        android::init_key(&path)?;
        return Ok(emit_value(
            &serde_json::json!({"key":path,"created":true}),
            format,
        )?);
    }
    if matches!(command, Command::Capabilities) {
        return Ok(emit_value(
            &serde_json::json!({"capture":"png_screencap","input":"android_input_command","observation":false,"hid":false,"device_overlay":false,"streaming":false,"transports":["usb","paired_wireless"],"adb_executable_required":false,"adb_server_required":false}),
            format,
        )?);
    }
    if let Command::Capture {
        display,
        window,
        backend,
        max_pixel_edge,
        ..
    } = &command
        && (display.is_some()
            || window.is_some()
            || *backend != CaptureRoute::Native
            || max_pixel_edge.is_some())
    {
        return Err("Android capture uses the current display at native resolution; alternate capture options are unavailable".into());
    }
    let credentials=connection.credentials.as_deref().ok_or("Android requires --credentials PATH: a pairing directory for wireless or a PEM key for usb:VID:PID")?;
    if let Command::Android { command } = command {
        let host = WirelessHost::new(credentials, Duration::from_secs(30))?;
        let paired = match command {
            AndroidCommand::Pair { endpoint } => {
                eprintln!("Enter the Wireless debugging pairing code, then press Enter:");
                let mut code = String::new();
                std::io::stdin().read_line(&mut code)?;
                host.pair(endpoint, code.trim())?
            }
            AndroidCommand::PairQr {
                qr_svg,
                timeout_secs,
            } => {
                if timeout_secs == 0 || timeout_secs > 600 {
                    return Err("--timeout-secs must be 1..=600".into());
                }
                let qr = android::qr::QrPairingSession::new()?;
                if let Some(path) = qr_svg {
                    let mut file = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(path)?;
                    file.write_all(qr.render_svg()?.as_bytes())?;
                }
                eprintln!("Scan with Wireless debugging > Pair device with QR code.");
                eprint!("{}", qr.render_terminal()?);
                std::io::stderr().flush()?;
                let endpoint = qr.wait_endpoint(Duration::from_secs(timeout_secs))?;
                host.pair(endpoint, qr.password())?
            }
            AndroidCommand::InitKey { .. } | AndroidCommand::Devices => unreachable!(),
        };
        return Ok(emit_value(&serde_json::to_value(paired)?, format)?);
    }
    match command {
        Command::Session{}|Command::Capture{..}|Command::Discover{..}=>{},
        _=>return Err("Android currently supports discover, capabilities, capture and session; native accessibility snapshots are unavailable".into()),
    }
    let discovered;
    let device = match connection.device.as_deref() {
        Some(device) => device,
        None => {
            let host = WirelessHost::new(credentials, Duration::from_secs(30))?;
            discovered = android::qr::discover_connection(
                &host.paired_device_id()?,
                Duration::from_secs(10),
            )?
            .to_string();
            &discovered
        }
    };
    if let Some(ids) = device.strip_prefix("usb:") {
        let (vendor, product) = ids
            .split_once(':')
            .ok_or("USB device must be usb:VID:PID with hexadecimal IDs")?;
        let transport = android::DirectDevice::usb(
            u16::from_str_radix(vendor, 16)?,
            u16::from_str_radix(product, 16)?,
            credentials,
        )?;
        return execute_android(command, android::Android::new(transport), format);
    }
    let endpoint = device.parse::<std::net::SocketAddr>()?;
    let policy = if connection.trust_first_connection {
        FirstConnectionPolicy::TrustOnFirstUse
    } else {
        FirstConnectionPolicy::RequireKnownCertificate
    };
    let transport =
        WirelessHost::new(credentials, Duration::from_secs(30))?.connect(endpoint, policy)?;
    execute_android(command, android::Android::new(transport), format)
}
#[cfg(feature = "android")]
fn execute_android<D: android::CommandTransport>(
    command: Command,
    mut device: android::Android<D>,
    format: unimation::OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        Command::Discover {
            scope: DiscoveryScope::Apps,
        } => Ok(android::apps::write_inventory(
            std::io::stdout().lock(),
            &device.launcher_activities()?,
            format,
        )?),
        Command::Discover {
            scope: DiscoveryScope::All,
        } => Ok(emit_value(&serde_json::to_value(device.info()?)?, format)?),
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
                return Err("Android capture uses the current display at native resolution; alternate capture options are unavailable".into());
            }
            Ok(emit_value(
                &android::jsonl::capture_file(&mut device, &path)?,
                format,
            )?)
        }
        Command::Session {} => Ok(android::jsonl::serve(
            &mut device,
            std::io::stdin().lock(),
            std::io::stdout().lock(),
            format,
        )?),
        _ => Err("The selected Android operation is unavailable".into()),
    }
}

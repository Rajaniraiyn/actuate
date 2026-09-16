//! Optional platform-owned binary command surface.
use usage::{Cli, Subcommands};
#[derive(Cli)]
#[usage(bin = "actuate-ios", version)]
struct App {
    /// Opt in to JSON records or JSONL session responses.
    #[usage(long, global)]
    json: bool,
    #[usage(subcommand)]
    command: Command,
}
#[derive(Subcommands)]
enum Command {
    /// Read installed device and runtime metadata.
    List {
        #[usage(long)]
        device_set: Option<std::path::PathBuf>,
    },
    /// Attach a JSONL agent session to an explicitly selected booted simulator.
    Session {
        #[usage(long)]
        udid: String,
        #[usage(long)]
        device_set: std::path::PathBuf,
    },
}
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let app = App::parse();
    let format = if app.json {
        actuate::OutputFormat::Json
    } else {
        actuate::OutputFormat::Text
    };
    match app.command {
        Command::List { device_set } => {
            let mut sim = crate::simulator::Simctl::installed()?;
            if let Some(path) = device_set {
                sim = sim.with_set(path);
            }
            actuate::output::write_value(std::io::stdout().lock(), &sim.list()?, format)?;
            Ok(())
        }
        Command::Session { udid, device_set } => {
            crate::jsonl::run_formatted(&udid, &device_set, format)
        }
    }
}

//! Optional platform-owned binary command surface.
use usage::{Cli, Subcommands};
#[derive(Cli)]
#[usage(bin = "unimation-ios", version)]
struct App {
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
    match App::parse().command {
        Command::List { device_set } => {
            let mut sim = crate::simulator::Simctl::installed()?;
            if let Some(path) = device_set {
                sim = sim.with_set(path);
            }
            println!("{}", serde_json::to_string_pretty(&sim.list()?)?);
            Ok(())
        }
        Command::Session { udid, device_set } => crate::jsonl::run(&udid, &device_set),
    }
}

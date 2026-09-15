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
    Discover,
    /// Read the native accessibility tree for an application.
    Observe {
        pid: i32,
        #[usage(long, default = "1000")]
        max_nodes: usize,
        #[usage(long, default = "30")]
        max_depth: usize,
    },
    /// Export the portable Usage command specification.
    Spec,
    /// Show JSON session commands and delivery semantics.
    Protocol,
    /// Read JSON requests from stdin, retaining references until EOF.
    Session,
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
    run(app.command)
}
#[cfg(not(target_os = "macos"))]
fn run(_: Command) -> Result<(), Box<dyn std::error::Error>> {
    Err("No provider implemented for this OS yet".into())
}
#[cfg(target_os = "macos")]
fn run(command: Command) -> Result<(), Box<dyn std::error::Error>> {
    use unimation_core::{Discover, Observe, ObserveRequest};
    let mut ax = unimation_macos::Accessibility::new();
    let result = match command {
        Command::Discover => ax.discover()?,
        Command::Observe {
            pid,
            max_nodes,
            max_depth,
        } => serde_json::to_value(ax.observe(ObserveRequest {
            pid,
            max_nodes,
            max_depth,
        })?)?,
        Command::Session => {
            return session(&mut ax);
        }
        Command::Spec | Command::Protocol => unreachable!(),
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
#[cfg(target_os = "macos")]
fn session(ax: &mut unimation_macos::Accessibility) -> Result<(), Box<dyn std::error::Error>> {
    use serde_json::{Value, json};
    use std::io::{BufRead, Write};
    use unimation_core::*;
    let mut input = unimation_macos::QuartzInput;
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        let reply = (|| -> std::result::Result<Value, Box<dyn std::error::Error>> {
            let mut request: Value = serde_json::from_str(&line)?;
            if let Some(object) = request.as_object_mut() {
                object.remove("id");
            }
            let request: SessionRequest = serde_json::from_value(request)?;
            let result: Result<Value> = match request {
                SessionRequest::Discover {} => ax.discover(),
                SessionRequest::Observe { request } => ax.observe(request).map(|r| json!(r)),
                SessionRequest::Semantic { target, action } => {
                    ax.semantic(&target, action).map(|r| json!(r))
                }
                SessionRequest::Pointer { delivery, action } => {
                    input.pointer(delivery, action).map(|r| json!(r))
                }
                SessionRequest::Text { delivery, text } => {
                    input.type_text(delivery, &text).map(|r| json!(r))
                }
                SessionRequest::Attribute { target, name } => ax.read_attribute(&target, &name),
                SessionRequest::ObserveSubtree {
                    target,
                    max_nodes,
                    max_depth,
                } => ax
                    .observe_subtree(&target, max_nodes, max_depth)
                    .map(|r| json!(r)),
                SessionRequest::Inspect { target } => ax.inspect(&target),
            };
            Ok(match result {
                Ok(r) => json!({"result":r}),
                Err(e) => json!({"error":e}),
            })
        })();
        let mut reply = reply.unwrap_or_else(|e| json!({"error":{"code":"invalid_request", "message":e.to_string(), "effect":"none"}}));
        if let Ok(Value::Object(request)) = serde_json::from_str::<Value>(&line)
            && let Some(id) = request.get("id")
        {
            reply["id"] = id.clone();
        }
        println!("{}", serde_json::to_string(&reply)?);
        std::io::stdout().flush()?;
    }
    Ok(())
}

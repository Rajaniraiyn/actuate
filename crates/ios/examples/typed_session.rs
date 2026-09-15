//! Read-only proof that a Rust embedding needs no CLI process or serialization.
#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use ios::{
        SimulatorScope,
        session::{Request, Response},
    };
    let arguments: Vec<String> = std::env::args().collect();
    if arguments.len() != 3 {
        return Err("usage: typed_session UDID DEVICE_SET".into());
    }
    let mut session = ios::session::connect(&arguments[1], std::path::Path::new(&arguments[2]))?;
    let Response::Snapshot(snapshot) = session.execute(Request::Observe {
        scope: SimulatorScope::Frontmost,
        max_nodes: 1000,
        max_depth: 30,
    })?
    else {
        return Err("Expected snapshot".into());
    };
    assert!(std::sync::Arc::ptr_eq(
        &snapshot,
        session.snapshot(Some(snapshot.revision))?
    ));
    println!(
        "Read {} nodes through typed calls; snapshot shares cache allocation",
        snapshot.nodes.len()
    );
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("This example requires a macOS Simulator host");
}

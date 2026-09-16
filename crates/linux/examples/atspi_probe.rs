//! Read-only AT-SPI probe: lists applications, then renders one tree.
//! Usage: atspi_probe [pid] [max_nodes]
use unimation::{Discover, ObservationBudget, ObserveScope, presentation};
fn main() {
    let mut atspi = linux::AtSpi::connect().expect("accessibility bus");
    let discovered = atspi.discover().expect("discover");
    println!("{}", serde_json::to_string_pretty(&discovered).unwrap());
    let Some(pid) = std::env::args().nth(1).and_then(|s| s.parse::<i32>().ok()) else {
        return;
    };
    let max_nodes = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);
    let started = std::time::Instant::now();
    let snapshot = atspi
        .observe_scope(
            pid,
            ObservationBudget {
                max_nodes,
                max_depth: 30,
            },
        )
        .expect("observe");
    eprintln!(
        "observed {} nodes in {:?} complete={} traversal={}",
        snapshot.nodes.len(),
        started.elapsed(),
        snapshot.complete,
        snapshot.traversal_complete
    );
    let view = presentation::render_snapshot(&snapshot, &Default::default()).unwrap();
    print!("{}", presentation::render_snapshot_text(&view));
    if std::env::var("DUMP").is_ok() {
        println!(
            "{}",
            serde_json::to_string_pretty(&snapshot.nodes[..snapshot.nodes.len().min(3)]).unwrap()
        );
    }
}

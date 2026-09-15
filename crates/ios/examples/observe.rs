#[cfg(target_os = "macos")]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut a =
        ios::SimulatorAccessibility::connect(&args[1], std::path::Path::new(&args[2])).unwrap();
    println!(
        "{}",
        serde_json::to_string_pretty(&a.observe_frontmost(100, 12).unwrap()).unwrap()
    );
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("iOS Simulator accessibility requires a macOS host.");
}

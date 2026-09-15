#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    ios::cli::run()
}
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("The iOS Simulator host requires macOS");
    std::process::exit(1);
}

#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use actuate::HardwareButtons;
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 || args[3] != "--enable" {
        return Err(
            "usage: pointer_probe UDID DEVICE_SET --enable (dedicated disposable simulator only)"
                .into(),
        );
    }
    let udid = &args[1];
    let path = std::path::Path::new(&args[2]);
    let capture = ios::simulator::Simctl::installed()?.with_set(path);
    let mut input = ios::SimulatorHid::connect(udid, path)?;
    input.press_button(actuate::HardwareButton::Home)?;
    std::thread::sleep(std::time::Duration::from_millis(400));
    capture.screenshot(udid, "/tmp/actuate-pointer-before.png")?;
    let mut pointer = ios::SimulatorPointer::connect(udid, path)?;
    println!("enable: {:?}", pointer.enable()?);
    std::thread::sleep(std::time::Duration::from_millis(250));
    println!("move: {:?}", pointer.move_relative(120.0, 90.0)?);
    capture.screenshot(udid, "/tmp/actuate-pointer-after.png")?;
    println!("move again: {:?}", pointer.move_relative(-65.0, 20.0)?);
    capture.screenshot(udid, "/tmp/actuate-pointer-moved.png")?;
    println!("close: {:?}", pointer.close()?);
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("This probe requires macOS and an explicitly selected disposable iOS simulator.");
}

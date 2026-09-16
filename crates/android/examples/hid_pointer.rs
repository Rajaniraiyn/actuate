//! Live native-pointer smoke test. Run only with a paired test device ready.
//! ACTUATE_CREDENTIALS=/private/device cargo run -p android --example hid_pointer
use android::{
    Android,
    hid::{Button, Delta, Pointer},
    wireless::{FirstConnectionPolicy, WirelessHost},
};
use std::{path::PathBuf, time::Duration};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let credentials = PathBuf::from(std::env::var("ACTUATE_CREDENTIALS")?);
    let host = WirelessHost::new(&credentials, Duration::from_secs(10))?;
    let endpoint =
        android::qr::discover_connection(&host.paired_device_id()?, Duration::from_secs(10))?;
    let mut input = host.connect(endpoint, FirstConnectionPolicy::RequireKnownCertificate)?;
    // Use Calculator for harmless pointer/button testing.
    Android::new(&mut input).launch(
        &"com.sec.android.app.popupcalculator/.Calculator"
            .to_owned()
            .try_into()?,
    )?;
    println!("Calculator launch dispatched");
    let mut pointer = Pointer::open(&mut input)?;
    println!("Native pointer registered");
    let path = actuate::motion::RelativeMotionPlan::new(
        60,
        20,
        Duration::from_millis(650),
        40,
        actuate::motion::MotionStyle::Curved,
    )?;
    pointer.move_smooth(&path)?;
    println!("Hover report dispatched");
    for button in [Button::Primary, Button::Secondary, Button::Middle] {
        pointer.button(button, true)?;
        pointer.button(button, false)?;
    }
    println!("Primary, secondary and middle button transitions dispatched");
    pointer.scroll(Delta::new(-3)?, Delta::new(0)?)?;
    println!("Wheel report dispatched");
    let path = std::env::temp_dir().join(format!("actuate-hid-{}.png", std::process::id()));
    let frame = android::jsonl::capture_file(&mut Android::new(&mut pointer), &path)?;
    println!("{frame}");
    pointer.close()?;
    println!("Native pointer closed");
    Ok(())
}

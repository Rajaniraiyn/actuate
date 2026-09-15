//! Live native-pointer smoke test. Run only with a paired test device ready.
//! UNIMATION_CREDENTIALS=/private/device cargo run -p android --example hid_pointer
use android::{
    Android,
    hid::{Button, Delta, Pointer},
    wireless::{FirstConnectionPolicy, WirelessHost},
};
use std::{path::PathBuf, time::Duration};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let credentials = PathBuf::from(std::env::var("UNIMATION_CREDENTIALS")?);
    let host = WirelessHost::new(&credentials, Duration::from_secs(10))?;
    let endpoint =
        android::qr::discover_connection(&host.paired_device_id()?, Duration::from_secs(10))?;
    let mut input = host.connect(endpoint, FirstConnectionPolicy::RequireKnownCertificate)?;
    let mut observer =
        Android::new(host.connect(endpoint, FirstConnectionPolicy::RequireKnownCertificate)?);
    // Use Calculator for harmless pointer/button testing.
    observer.launch(
        &"com.sec.android.app.popupcalculator/.Calculator"
            .to_owned()
            .try_into()?,
    )?;
    println!("Calculator launch dispatched");
    let mut pointer = Pointer::open(&mut input)?;
    println!("Native pointer registered");
    pointer.move_relative(Delta::new(20)?, Delta::new(0)?)?;
    println!("Hover report dispatched");
    for button in [Button::Primary, Button::Secondary, Button::Middle] {
        pointer.button(button, true)?;
        pointer.button(button, false)?;
    }
    println!("Primary, secondary and middle button transitions dispatched");
    pointer.scroll(Delta::new(-3)?, Delta::new(0)?)?;
    println!("Wheel report dispatched");
    let path = std::env::temp_dir().join(format!("unimation-hid-{}.png", std::process::id()));
    let frame = android::jsonl::capture_file(&mut observer, &path)?;
    println!("{frame}");
    pointer.close()?;
    println!("Native pointer closed");
    Ok(())
}

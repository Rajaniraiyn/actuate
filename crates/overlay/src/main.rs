#[cfg(target_os = "macos")]
mod macos;
fn main() {
    #[cfg(target_os = "macos")]
    macos::run();
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("Native cursor renderer is unavailable on this platform");
        std::process::exit(1);
    }
}
#[cfg(test)]
mod tests {
    use overlay::{CursorCommand as Command, interpolate};
    #[test]
    fn motion_and_coordinates() {
        assert_eq!(interpolate((0., 0.), (100., 50.), 0.), (0., 0.));
        assert_eq!(interpolate((0., 0.), (100., 50.), 1.), (100., 50.));
        assert_eq!(interpolate((0., 0.), (100., 50.), 0.5), (50., 25.));
    }
    #[test]
    fn rejects_unknown_actions() {
        assert!(
            serde_json::from_str::<Command>(r#"{"op":"click","x":1,"y":2,"button":"right"}"#)
                .is_err()
        );
    }
}

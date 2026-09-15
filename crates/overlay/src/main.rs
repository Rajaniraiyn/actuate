use serde::Deserialize;
#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Move {
        x: f64,
        y: f64,
        #[serde(default = "duration")]
        duration_ms: u64,
    },
    Click {
        x: f64,
        y: f64,
    },
    Hide,
    Show,
    Quit,
}
fn duration() -> u64 {
    250
}
fn interpolate(start: (f64, f64), end: (f64, f64), t: f64) -> (f64, f64) {
    let t = t.clamp(0., 1.);
    let u = 1. - t;
    // Repeated endpoint controls produce a cubic Bézier with zero endpoint velocity.
    let k = 3. * u * t * t + t * t * t;
    (
        start.0 + (end.0 - start.0) * k,
        start.1 + (end.1 - start.1) * k,
    )
}
fn appkit_origin(x: f64, y: f64, main_height: f64) -> (f64, f64) {
    (x - 8., main_height - y - 40.)
}
#[cfg(target_os = "macos")]
mod macos;
fn main() {
    #[cfg(target_os = "macos")]
    macos::run();
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("overlay requires macOS");
        std::process::exit(1);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn motion_and_coordinates() {
        assert_eq!(interpolate((0., 0.), (100., 50.), 0.), (0., 0.));
        assert_eq!(interpolate((0., 0.), (100., 50.), 1.), (100., 50.));
        assert_eq!(interpolate((0., 0.), (100., 50.), 0.5), (50., 25.));
        assert_eq!(appkit_origin(-200., -50., 1080.), (-208., 1090.));
    }
    #[test]
    fn rejects_unknown_actions() {
        assert!(
            serde_json::from_str::<Command>(r#"{"op":"click","x":1,"y":2,"button":"right"}"#)
                .is_err()
        );
    }
}

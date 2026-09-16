#[cfg(target_os = "macos")]
mod macos;
fn main() {
    #[cfg(target_os = "macos")]
    macos::run();
    #[cfg(target_os = "linux")]
    overlay::linux::run();
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        eprintln!("Native cursor renderer is unavailable on this platform");
        std::process::exit(1);
    }
}
#[cfg(test)]
mod tests {
    use overlay::CursorCommand as Command;
    #[test]
    fn rejects_unknown_actions() {
        assert!(
            serde_json::from_str::<Command>(r#"{"op":"click","x":1,"y":2,"button":"right"}"#)
                .is_err()
        );
    }
}

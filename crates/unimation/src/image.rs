//! Minimal encoded-image inspection and output-path rules shared by capture providers.
use crate::{NativeError, Result};
use std::path::{Path, PathBuf};

/// Capture outputs are absolute so helper processes and later reads agree.
pub fn absolute_output(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(NativeError::new(
            "capture_output",
            "Capture output path is empty",
        ));
    }
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .map_err(|e| NativeError::new("capture_output", e))?
            .join(path))
    }
}

/// Width and height from a PNG header. Rejects anything that is not a
/// well-formed PNG signature followed by an IHDR chunk with nonzero size.
pub fn png_dimensions(bytes: &[u8]) -> Result<(u32, u32)> {
    if bytes.len() < 33
        || !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes[8..12] != 13u32.to_be_bytes()
        || &bytes[12..16] != b"IHDR"
    {
        return Err(NativeError::new(
            "capture_failed",
            "Data is not a PNG image",
        ));
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().expect("four bytes"));
    let height = u32::from_be_bytes(bytes[20..24].try_into().expect("four bytes"));
    if width == 0 || height == 0 {
        return Err(NativeError::new(
            "capture_failed",
            "PNG header reports an empty image",
        ));
    }
    Ok((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn output_paths_become_absolute() {
        assert!(absolute_output(Path::new("")).is_err());
        assert!(
            absolute_output(Path::new("frame.png"))
                .unwrap()
                .is_absolute()
        );
        let absolute = std::env::temp_dir().join("-x.png");
        assert!(absolute.is_absolute());
        assert_eq!(absolute_output(&absolute).unwrap(), absolute);
    }
    #[test]
    fn parses_header_and_rejects_malformed_input() {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend(13u32.to_be_bytes());
        png.extend(b"IHDR");
        png.extend(640u32.to_be_bytes());
        png.extend(480u32.to_be_bytes());
        png.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(png_dimensions(&png).unwrap(), (640, 480));
        assert!(png_dimensions(&png[..20]).is_err());
        png[16..20].copy_from_slice(&0u32.to_be_bytes());
        assert!(png_dimensions(&png).is_err());
        assert!(png_dimensions(b"not a png at all, definitely not one here").is_err());
    }
}

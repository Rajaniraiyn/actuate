//! Opt-in native common-dialog fixture. Reads/writes stay inside the supplied
//! disposable directory; automation comes from a separate Actuate session.
#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{io, path::PathBuf};
    use windows_api::{
        Win32::{System::Com::*, UI::Shell::*},
        core::{HSTRING, Interface, w},
    };

    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }

    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.len() != 3 || arguments[0] != "--interactive" {
        return Err(io::Error::other(
            "Usage: windows_dialog_fixture --interactive save|open DISPOSABLE_DIRECTORY",
        )
        .into());
    }
    let save = match arguments[1].to_str() {
        Some("save") => true,
        Some("open") => false,
        _ => return Err(io::Error::other("Expected save or open").into()),
    };
    let shell_path = std::path::absolute(PathBuf::from(&arguments[2]))?;
    let root = shell_path.canonicalize()?;
    if !root.is_dir() {
        return Err(io::Error::other("Fixture root must be an existing directory").into());
    }

    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? };
    let _apartment = Apartment;
    let dialog: IFileDialog = unsafe {
        if save {
            CoCreateInstance::<_, IFileSaveDialog>(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)?
                .cast()?
        } else {
            CoCreateInstance::<_, IFileOpenDialog>(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?
                .cast()?
        }
    };
    let folder: IShellItem =
        unsafe { SHCreateItemFromParsingName(&HSTRING::from(shell_path.as_os_str()), None)? };
    unsafe {
        let requirements = if save {
            FOS_OVERWRITEPROMPT
        } else {
            FOS_FILEMUSTEXIST
        };
        dialog.SetOptions(
            FOS_FORCEFILESYSTEM
                | FOS_PATHMUSTEXIST
                | FOS_NOCHANGEDIR
                | FOS_DONTADDTORECENT
                | requirements,
        )?;
        dialog.SetFolder(&folder)?;
        dialog.SetTitle(if save {
            w!("Actuate native Save")
        } else {
            w!("Actuate native Open")
        })?;
        dialog.SetDefaultExtension(w!("txt"))?;
        dialog.SetFileName(w!("report.txt"))?;
        if let Err(error) = dialog.Show(None) {
            if error.code().0 == 0x800704C7_u32 as i32 {
                println!("{}", serde_json::json!({"status":"cancelled"}));
                return Ok(());
            }
            return Err(error.into());
        }
    }
    let item = unsafe { dialog.GetResult()? };
    let native_path = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH)? };
    let path = unsafe { native_path.to_string() };
    unsafe { CoTaskMemFree(Some(native_path.0.cast())) };
    let path = PathBuf::from(path?);
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("Selection has no parent"))?
        .canonicalize()?;
    if !parent.starts_with(&root) || (path.exists() && !path.canonicalize()?.starts_with(&root)) {
        return Err(
            io::Error::other("Selection is outside the disposable fixture directory").into(),
        );
    }
    let bytes = if save {
        let bytes = b"native Rust save fixture\r\n".to_vec();
        std::fs::write(&path, &bytes)?;
        bytes
    } else {
        std::fs::read(&path)?
    };
    println!(
        "{}",
        serde_json::json!({
            "status": if save { "saved" } else { "opened" },
            "path": path,
            "bytes": bytes,
        })
    );
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("This fixture requires Windows");
    std::process::exit(1);
}

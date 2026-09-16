//! A separate native process owns temporary system-cursor suppression. Pipe EOF
//! and a missed heartbeat restore visibility even if the renderer dies or hangs.
use std::{
    io::{self, BufRead, Write},
    os::windows::process::CommandExt,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
use windows_api::Win32::UI::{Magnification::*, WindowsAndMessaging::*};

struct Magnification;
impl Drop for Magnification {
    fn drop(&mut self) {
        unsafe {
            let _ = MagShowSystemCursor(true);
            let _ = MagUninitialize();
        }
    }
}

pub fn run() -> io::Result<()> {
    if !unsafe { MagInitialize() }.as_bool() {
        return Err(io::Error::last_os_error());
    }
    let _restore = Magnification;
    println!("ready");
    io::stdout().flush()?;
    let (tx, rx) = mpsc::sync_channel(16);
    std::thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut hidden = false;
    let mut last = Instant::now();
    loop {
        let desired = match rx.recv_timeout(Duration::from_millis(25)) {
            Ok(line) if line == "hide" || line == "show" => {
                last = Instant::now();
                line == "hide"
            }
            Ok(_) => return Err(io::Error::other("Invalid cursor guard command")),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                hidden && last.elapsed() < Duration::from_millis(500)
            }
        };
        if desired != hidden {
            if !unsafe { MagShowSystemCursor(!desired) }.as_bool() {
                return Err(io::Error::last_os_error());
            }
            hidden = desired;
        }
        unsafe {
            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
}

pub struct Guard {
    child: Child,
    pipe: Option<ChildStdin>,
    hidden: bool,
    heartbeat: Instant,
}
impl Guard {
    pub fn start() -> io::Result<Self> {
        let mut child = Command::new(std::env::current_exe()?)
            .arg("--cursor-visibility-guard")
            .creation_flags(0x08000000)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("Guard stdout unavailable"))?;
        let (tx, rx) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut line = String::new();
            let result = io::BufReader::new(stdout)
                .read_line(&mut line)
                .map(|_| line);
            let _ = tx.send(result);
        });
        match rx.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(line)) if line.trim() == "ready" => {}
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::other(
                    "Cursor visibility guard failed to initialize",
                ));
            }
        }
        Ok(Self {
            pipe: child.stdin.take(),
            child,
            hidden: false,
            heartbeat: Instant::now(),
        })
    }
    pub fn update(&mut self, hide: bool) -> io::Result<()> {
        if self.child.try_wait()?.is_some() {
            return Err(io::Error::other("Cursor visibility guard exited"));
        }
        if hide != self.hidden || self.heartbeat.elapsed() >= Duration::from_millis(100) {
            self.pipe
                .as_mut()
                .ok_or_else(|| io::Error::other("Guard pipe closed"))?
                .write_all(if hide { b"hide\n" } else { b"show\n" })?;
            self.hidden = hide;
            self.heartbeat = Instant::now();
        }
        Ok(())
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        // Close the pipe rather than killing the process that restores visibility.
        self.pipe.take();
        let _ = self.child.wait();
    }
}

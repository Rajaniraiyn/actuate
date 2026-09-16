//! Shared language-binding runtime. Native providers are constructed, called,
//! and destroyed on one worker; no native handle crosses the thread boundary.
mod providers;
use actuate::{Effect, NativeError, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConnectOptions {
    pub provider: Provider,
    pub device: Option<String>,
    pub device_set: Option<std::path::PathBuf>,
    pub credentials: Option<std::path::PathBuf>,
    pub trust_first_connection: bool,
}
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    #[default]
    Native,
    Windows,
    Macos,
    Linux,
    AppleSimulator,
    AppleDevice,
    Android,
}

/// A provider can be non-Send. Its factory alone crosses the thread boundary.
pub trait Dispatcher {
    fn dispatch(&mut self, request: Value) -> Result<Value>;
}
impl<F: FnMut(Value) -> Result<Value>> Dispatcher for F {
    fn dispatch(&mut self, request: Value) -> Result<Value> {
        self(request)
    }
}
enum Message {
    Request(Value, mpsc::SyncSender<Result<Value>>),
    Close,
}
struct State {
    sender: Option<mpsc::SyncSender<Message>>,
    worker: Option<JoinHandle<()>>,
}
/// Cloneable ownership of one serialized native session. Close affects all clones.
#[derive(Clone)]
pub struct Session(Arc<Mutex<State>>);
fn closed() -> NativeError {
    NativeError::new("session_closed", "The session is closed")
}
fn failed() -> NativeError {
    NativeError::new(
        "worker_stopped",
        "The native worker stopped before returning a result",
    )
    .with_effect(Effect::Unknown)
}
impl Session {
    pub fn connect(options: ConnectOptions) -> Result<Self> {
        Self::with_factory(move || providers::connect(options))
    }
    pub fn with_factory(
        factory: impl FnOnce() -> Result<Box<dyn Dispatcher>> + Send + 'static,
    ) -> Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let (ready, started) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("actuate-session".into())
            .spawn(move || {
                let mut provider = match factory() {
                    Ok(provider) => provider,
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return;
                    }
                };
                if ready.send(Ok(())).is_err() {
                    return;
                }
                while let Ok(message) = receiver.recv() {
                    match message {
                        Message::Request(request, reply) => {
                            let _ = reply.send(provider.dispatch(request));
                        }
                        Message::Close => break,
                    }
                }
                // Provider drop runs here, on the same thread as its constructor.
            })
            .map_err(|e| NativeError::new("worker_start", e))?;
        match started.recv() {
            Ok(Ok(())) => Ok(Self(Arc::new(Mutex::new(State {
                sender: Some(sender),
                worker: Some(worker),
            })))),
            result => {
                let _ = worker.join();
                Err(match result {
                    Ok(Err(error)) => error,
                    _ => failed(),
                })
            }
        }
    }
    pub fn request(&self, request: Value) -> Result<Value> {
        if !request.is_object() || !request.get("op").is_some_and(Value::is_string) {
            return Err(NativeError::invalid_request(
                "Request must be an object with a string op",
            ));
        }
        // Hold the gate until the reply. Close cannot drop a provider mid-call.
        let state = self.0.lock().map_err(|_| failed())?;
        let sender = state.sender.as_ref().ok_or_else(closed)?;
        let (reply, result) = mpsc::sync_channel(1);
        sender
            .send(Message::Request(request, reply))
            .map_err(|_| failed())?;
        result.recv().map_err(|_| failed())?
    }
    pub fn request_json(&self, request: &str) -> String {
        envelope(
            serde_json::from_str(request)
                .map_err(|e| NativeError::invalid_request(e.to_string()))
                .and_then(|request| self.request(request)),
        )
    }
    pub fn close(&self) -> Result<()> {
        let mut state = self.0.lock().map_err(|_| failed())?;
        if let Some(sender) = state.sender.take() {
            let _ = sender.send(Message::Close);
        }
        if let Some(worker) = state.worker.take() {
            worker.join().map_err(|_| failed())?;
        }
        Ok(())
    }
}
impl Drop for State {
    fn drop(&mut self) {
        // Dropping an abandoned language object must not block its finalizer.
        // Disconnecting drains the worker and releases handles on that worker.
        self.sender.take();
    }
}
pub fn envelope(result: Result<Value>) -> String {
    match result {
        Ok(result) => json!({"result":result}),
        Err(error) => json!({"error":error}),
    }
    .to_string()
}
pub fn parse_options(options: &str) -> Result<ConnectOptions> {
    serde_json::from_str(options).map_err(|e| NativeError::invalid_request(e.to_string()))
}

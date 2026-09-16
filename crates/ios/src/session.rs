//! Typed sessions. Provider calls never serialize requests or responses.
use crate::{
    SimulatorAccessibility, SimulatorScope,
    providers::{LazySimulatorInput, SimulatorFrame, SimulatorServices},
};
use actuate::{
    presentation::PresentationOptions,
    session::{Rendered, SnapshotHistory},
    *,
};
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Capabilities {},
    Observe {
        #[serde(default = "frontmost")]
        scope: SimulatorScope,
        #[serde(default = "default_max_nodes")]
        max_nodes: usize,
        #[serde(default = "default_max_depth")]
        max_depth: usize,
    },
    Snapshot {
        #[serde(default = "frontmost")]
        scope: SimulatorScope,
        #[serde(default = "default_max_nodes")]
        max_nodes: usize,
        #[serde(default = "default_max_depth")]
        max_depth: usize,
        #[serde(default)]
        format: OutputFormat,
        #[serde(default)]
        options: PresentationOptions,
    },
    View {
        revision: Option<u64>,
        #[serde(default)]
        format: OutputFormat,
        #[serde(default)]
        options: PresentationOptions,
    },
    Query {
        revision: Option<u64>,
        query: query::NodeQuery,
    },
    Diff {
        before: u64,
        after: Option<u64>,
    },
    DiffView {
        before: u64,
        after: Option<u64>,
        #[serde(default)]
        options: PresentationOptions,
    },
    Semantic {
        target: ElementRef,
        action: SemanticAction,
    },
    Touch {
        action: TouchAction,
    },
    Button {
        button: HardwareButton,
    },
    HidKey {
        usage: u16,
        #[serde(default)]
        modifiers: Vec<u16>,
    },
    Launch {
        bundle_id: String,
    },
    Capture {
        path: PathBuf,
    },
}
/// Owned typed results. Raw snapshots share ownership with the revision cache.
pub enum Response {
    Snapshot(Arc<Snapshot>),
    Text(String),
    Compact(presentation::CompactSnapshot),
    Query(query::QueryResult),
    Diff(diff::SnapshotDiff),
    Receipt(Receipt),
    Frame(SimulatorFrame),
    Capabilities,
}
fn frontmost() -> SimulatorScope {
    SimulatorScope::Frontmost
}
pub(crate) fn fail(code: &str, message: impl Into<String>) -> NativeError {
    NativeError::new(code, message.into())
}
fn view(
    snapshot: Arc<Snapshot>,
    format: OutputFormat,
    options: &PresentationOptions,
) -> Result<Response> {
    Ok(match session::render(snapshot, format, options)? {
        Rendered::Raw(snapshot) => Response::Snapshot(snapshot),
        Rendered::Compact(view) => Response::Compact(view),
        Rendered::Text(text) => Response::Text(text),
    })
}
/// A selected combination of capabilities, rather than an enum of OS backends.
pub trait SessionBackend:
    ObserveScope<Scope = SimulatorScope>
    + SemanticActions
    + TouchInput
    + HardwareButtons
    + HidKeyboard
    + Capture<Request = PathBuf, Frame = SimulatorFrame>
    + AppLifecycle<App = String>
{
}
impl<T> SessionBackend for T where
    T: ObserveScope<Scope = SimulatorScope>
        + SemanticActions
        + TouchInput
        + HardwareButtons
        + HidKeyboard
        + Capture<Request = PathBuf, Frame = SimulatorFrame>
        + AppLifecycle<App = String>
{
}
pub struct Session<B> {
    pub backend: B,
    snapshots: SnapshotHistory,
}
impl<B: SessionBackend> Session<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            snapshots: SnapshotHistory::default(),
        }
    }
    pub fn snapshot(&self, revision: Option<u64>) -> Result<&Arc<Snapshot>> {
        self.snapshots.get(revision)
    }
    pub fn execute(&mut self, request: Request) -> Result<Response> {
        match request {
            Request::Capabilities {} => Ok(Response::Capabilities),
            Request::Observe {
                scope,
                max_nodes,
                max_depth,
            }
            | Request::Snapshot {
                scope,
                max_nodes,
                max_depth,
                ..
            } => {
                session::validate_budget(max_nodes, max_depth)?;
                let snapshot = self.snapshots.remember(self.backend.observe_scope(
                    scope,
                    ObservationBudget {
                        max_nodes,
                        max_depth,
                    },
                )?);
                Ok(match request {
                    Request::Snapshot {
                        format, options, ..
                    } => view(snapshot, format, &options)?,
                    _ => Response::Snapshot(snapshot),
                })
            }
            Request::View {
                revision,
                format,
                options,
            } => view(self.snapshot(revision)?.clone(), format, &options),
            Request::Query { revision, query } => Ok(Response::Query(query::query_nodes(
                self.snapshot(revision)?,
                &query,
            ))),
            Request::Diff { before, after } => Ok(Response::Diff(diff::diff_snapshots(
                self.snapshot(Some(before))?,
                self.snapshot(after)?,
            )?)),
            Request::DiffView {
                before,
                after,
                options,
            } => Ok(Response::Text(actuate::presentation::render_view_diff(
                self.snapshot(Some(before))?,
                self.snapshot(after)?,
                &options,
                100,
            )?)),
            Request::Semantic { target, action } => {
                Ok(Response::Receipt(self.backend.semantic(&target, action)?))
            }
            Request::Touch { action } => Ok(Response::Receipt(self.backend.touch(action)?)),
            Request::Button { button } => Ok(Response::Receipt(self.backend.press_button(button)?)),
            Request::HidKey { usage, modifiers } => Ok(Response::Receipt(
                self.backend.press_usage(usage, &modifiers)?,
            )),
            Request::Launch { bundle_id } => {
                Ok(Response::Receipt(self.backend.launch_app(bundle_id)?))
            }
            Request::Capture { path } => Ok(Response::Frame(self.backend.capture(path)?)),
        }
    }
}
pub type NativeBackend = Backend<
    SimulatorAccessibility,
    LazySimulatorInput,
    SimulatorServices,
    Unavailable,
    SimulatorServices,
>;
pub type NativeSession = Session<NativeBackend>;
pub fn connect(udid: &str, device_set: &Path) -> Result<NativeSession> {
    let services = SimulatorServices::new(
        crate::simulator::Simctl::installed()?.with_set(device_set),
        udid,
    );
    Ok(Session::new(
        Backend::new(SimulatorAccessibility::connect(udid, device_set)?)
            .with_input(LazySimulatorInput::new(udid, device_set))
            .with_capture(services.clone())
            .with_apps(services),
    ))
}

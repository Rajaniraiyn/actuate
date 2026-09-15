//! iOS protocol adapter. Native transport remains in the ios library.
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    io::{BufRead, Write},
    path::{Path, PathBuf},
};
use unimation::{
    Effect, ElementRef, NativeError, OutputFormat, SemanticAction, SemanticActions, Snapshot,
    presentation::{self, PresentationOptions},
};

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Capabilities {},
    Observe {
        #[serde(default = "node_budget")]
        max_nodes: usize,
        #[serde(default = "depth_budget")]
        max_depth: usize,
    },
    Snapshot {
        #[serde(default = "node_budget")]
        max_nodes: usize,
        #[serde(default = "depth_budget")]
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
        query: unimation::query::NodeQuery,
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
    Launch {
        bundle_id: String,
    },
    Capture {
        path: PathBuf,
    },
}
fn node_budget() -> usize {
    1000
}
fn depth_budget() -> usize {
    30
}
fn fail(code: &str, message: impl Into<String>) -> NativeError {
    NativeError {
        code: code.into(),
        message: message.into(),
        effect: Effect::None,
    }
}
fn view(
    snapshot: &Snapshot,
    format: OutputFormat,
    options: &PresentationOptions,
) -> unimation::Result<Value> {
    if format == OutputFormat::Json {
        return Ok(json!(snapshot));
    }
    let projection = presentation::render_snapshot(snapshot, options)?;
    Ok(if format == OutputFormat::Text {
        json!(presentation::render_snapshot_text(&projection))
    } else {
        json!(projection)
    })
}
struct Session {
    ax: ios::SimulatorAccessibility,
    sim: ios::simulator::Simctl,
    udid: String,
    snapshots: VecDeque<Snapshot>,
}
impl Session {
    fn snapshot(&self, revision: Option<u64>) -> unimation::Result<&Snapshot> {
        match revision {
            Some(r) => self.snapshots.iter().find(|s| s.revision == r),
            None => self.snapshots.back(),
        }
        .ok_or_else(|| {
            fail(
                "unknown_snapshot",
                "Session retains the latest 32 observations",
            )
        })
    }
    fn dispatch(&mut self, mut value: Value) -> unimation::Result<Value> {
        if let Some(object) = value.as_object_mut() {
            object.remove("id");
        }
        // Short references belong to this session, never to a process-wide registry.
        if let Some(short) = value.get("target").and_then(Value::as_str) {
            let id = short
                .strip_prefix("@e")
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or_else(|| fail("invalid_request", "Expected @e followed by an integer"))?;
            value["target"] = json!(ElementRef {
                session: self.snapshot(None)?.root.session.clone(),
                id
            });
        }
        let request: Request =
            serde_json::from_value(value).map_err(|e| fail("invalid_request", e.to_string()))?;
        match request {
            Request::Capabilities {} => Ok(
                json!({"backend":"ios_simulator","udid":self.udid,"observation":"native_accessibility","semantic":"advertised_actions_only","capture":"simctl_png","physical_devices":false,"input_delivery":"no_global_mac_keyboard_or_mouse"}),
            ),
            Request::Observe {
                max_nodes,
                max_depth,
            }
            | Request::Snapshot {
                max_nodes,
                max_depth,
                ..
            } => {
                if max_nodes == 0 || max_nodes > 10000 || max_depth == 0 || max_depth > 100 {
                    return Err(fail(
                        "invalid_request",
                        "max_nodes must be 1..10000 and max_depth 1..100",
                    ));
                }
                let snapshot = self.ax.observe_frontmost(max_nodes, max_depth)?;
                let output = match request {
                    Request::Snapshot {
                        format, options, ..
                    } => view(&snapshot, format, &options)?,
                    _ => json!(snapshot),
                };
                self.snapshots.push_back(snapshot);
                if self.snapshots.len() > 32 {
                    self.snapshots.pop_front();
                }
                Ok(output)
            }
            Request::View {
                revision,
                format,
                options,
            } => view(self.snapshot(revision)?, format, &options),
            Request::Query { revision, query } => Ok(json!(unimation::query::query_nodes(
                self.snapshot(revision)?,
                &query
            ))),
            Request::Diff { before, after } => Ok(json!(unimation::diff::diff_snapshots(
                self.snapshot(Some(before))?,
                self.snapshot(after)?
            )?)),
            Request::DiffView {
                before,
                after,
                options,
            } => Ok(json!(presentation::render_view_diff(
                self.snapshot(Some(before))?,
                self.snapshot(after)?,
                &options,
                100
            )?)),
            Request::Semantic { target, action } => Ok(json!(self.ax.semantic(&target, action)?)),
            Request::Launch { bundle_id } => {
                Ok(json!({"output":self.sim.launch(&self.udid,&bundle_id)?,"effect":"dispatched"}))
            }
            Request::Capture { path } => {
                if path.exists() {
                    return Err(fail(
                        "output_exists",
                        "Screenshot destination already exists",
                    ));
                }
                let output = self.sim.screenshot(&self.udid, &path)?;
                Ok(
                    json!({"path":path,"output":output,"coordinates":"device_pixels","click_mapping":null}),
                )
            }
        }
    }
}
pub fn run(udid: &str, device_set: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut session = Session {
        ax: ios::SimulatorAccessibility::connect(udid, device_set)?,
        sim: ios::simulator::Simctl::installed()?.with_set(device_set),
        udid: udid.into(),
        snapshots: VecDeque::new(),
    };
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        let value = serde_json::from_str::<Value>(&line);
        let id = value.as_ref().ok().and_then(|v| v.get("id")).cloned();
        let result = value
            .map_err(|e| fail("invalid_request", e.to_string()))
            .and_then(|v| session.dispatch(v));
        let mut reply = match result {
            Ok(v) => json!({"result":v}),
            Err(e) => json!({"error":e}),
        };
        if let Some(id) = id {
            reply["id"] = id;
        }
        println!("{reply}");
        std::io::stdout().flush()?;
    }
    Ok(())
}

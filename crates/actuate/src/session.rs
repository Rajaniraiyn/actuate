//! Platform-independent session state: retained observations and the
//! format-specific rendering every session exposes. Native providers and
//! their request vocabularies stay in platform crates.
use crate::{
    Effect, NativeError, OutputFormat, Result, Snapshot,
    presentation::{self, CompactSnapshot, PresentationOptions},
};
use serde::{Serialize, Serializer};
use serde_json::{Value, json};
use std::{collections::VecDeque, sync::Arc};

pub const DEFAULT_RETAINED_SNAPSHOTS: usize = 32;

impl Default for SnapshotHistory {
    fn default() -> Self {
        Self::new(DEFAULT_RETAINED_SNAPSHOTS)
    }
}

/// Bounded revision history shared through `Arc` so callers can retain an
/// observation without copying or re-encoding it.
#[derive(Debug)]
pub struct SnapshotHistory {
    snapshots: VecDeque<Arc<Snapshot>>,
    capacity: usize,
}
impl SnapshotHistory {
    pub fn new(capacity: usize) -> Self {
        Self {
            snapshots: VecDeque::new(),
            capacity: capacity.max(1),
        }
    }
    pub fn remember(&mut self, snapshot: Snapshot) -> Arc<Snapshot> {
        let snapshot = Arc::new(snapshot);
        self.snapshots.push_back(snapshot.clone());
        while self.snapshots.len() > self.capacity {
            self.snapshots.pop_front();
        }
        snapshot
    }
    /// `None` selects the latest retained observation.
    pub fn get(&self, revision: Option<u64>) -> Result<&Arc<Snapshot>> {
        match revision {
            Some(r) => self.snapshots.iter().find(|s| s.revision == r),
            None => self.snapshots.back(),
        }
        .ok_or_else(|| {
            NativeError::new(
                "unknown_snapshot",
                format!(
                    "Snapshot not retained; session retains the latest {} observations",
                    self.capacity
                ),
            )
        })
    }
    pub fn capacity(&self) -> usize {
        self.capacity
    }
    /// Revision, root and coverage of each retained observation.
    pub fn summaries(&self) -> Vec<Value> {
        self.snapshots
            .iter()
            .map(|s| {
                json!({"revision":s.revision,"root":s.root,"complete":s.complete,"traversal_complete":s.traversal_complete})
            })
            .collect()
    }
}

/// One observation rendered for an output format. Raw shares the retained
/// allocation; the other variants are derived presentations.
pub enum Rendered {
    Raw(Arc<Snapshot>),
    Compact(CompactSnapshot),
    Text(String),
}
impl Serialize for Rendered {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            Self::Raw(snapshot) => snapshot.as_ref().serialize(serializer),
            Self::Compact(view) => view.serialize(serializer),
            Self::Text(text) => text.serialize(serializer),
        }
    }
}

/// JSON returns the raw observation; compact returns presentation rows;
/// text returns the tree string. Nothing here changes retained state.
pub fn render(
    snapshot: Arc<Snapshot>,
    format: OutputFormat,
    options: &PresentationOptions,
) -> Result<Rendered> {
    if format == OutputFormat::Json {
        return Ok(Rendered::Raw(snapshot));
    }
    let view = presentation::render_snapshot(&snapshot, options)?;
    Ok(if format == OutputFormat::Text {
        Rendered::Text(presentation::render_snapshot_text(&view))
    } else {
        Rendered::Compact(view)
    })
}

/// Counts native mutations so retained frames can be invalidated. Any
/// dispatched or unknown effect, in a result or an error, advances it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ActionEpoch(u64);
impl ActionEpoch {
    pub fn record(&mut self, result: &Result<Value>) {
        let changed = match result {
            Ok(v) => matches!(
                v.get("effect").and_then(Value::as_str),
                Some("dispatched" | "unknown")
            ),
            Err(e) => matches!(e.effect, Effect::Dispatched | Effect::Unknown),
        };
        if changed {
            self.0 = self.0.wrapping_add(1);
        }
    }
    pub fn current(&self) -> u64 {
        self.0
    }
}

pub const DEFAULT_RETAINED_FRAMES: usize = 32;

/// Bounded capture history. A frame is usable for input only while no
/// mutation has been dispatched since it was taken.
#[derive(Debug)]
pub struct FrameHistory<F> {
    frames: VecDeque<(u64, u64, F)>,
    next: u64,
    capacity: usize,
}
impl<F> Default for FrameHistory<F> {
    fn default() -> Self {
        Self {
            frames: VecDeque::new(),
            next: 1,
            capacity: DEFAULT_RETAINED_FRAMES,
        }
    }
}
impl<F> FrameHistory<F> {
    pub fn remember(&mut self, frame: F, epoch: ActionEpoch) -> u64 {
        let id = self.next;
        self.next += 1;
        self.frames.push_back((id, epoch.current(), frame));
        while self.frames.len() > self.capacity {
            self.frames.pop_front();
        }
        id
    }
    /// The frame, if retained and not invalidated by a later mutation.
    pub fn get(&self, id: u64, epoch: ActionEpoch) -> Result<&F> {
        let (_, taken, frame) = self
            .frames
            .iter()
            .find(|(i, _, _)| *i == id)
            .ok_or_else(|| {
                NativeError::new("unknown_frame", "Frame not retained in this session")
            })?;
        if *taken != epoch.current() {
            return Err(NativeError::new(
                "stale_frame",
                "Session dispatched input since this capture; capture again before an image click",
            ));
        }
        Ok(frame)
    }
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

/// Polls one native read until it equals `expected` or the deadline passes.
/// The report keeps the last value or error; it never retries a mutation.
pub fn wait_attribute(
    timeout_ms: u64,
    expected: &Value,
    mut read: impl FnMut() -> Result<Value>,
) -> Result<Value> {
    if timeout_ms > 60_000 {
        return Err(NativeError::invalid_request(
            "Wait timeout must be <=60000ms",
        ));
    }
    let start = std::time::Instant::now();
    loop {
        let value = read();
        let matched = value.as_ref().is_ok_and(|v| v == expected);
        if matched || start.elapsed().as_millis() >= u128::from(timeout_ms) {
            return Ok(
                json!({"matched":matched,"elapsed_ms":start.elapsed().as_millis(),"value":value.as_ref().ok(),"error":value.err()}),
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

/// Reject observation budgets that a provider should never attempt.
pub fn validate_budget(max_nodes: usize, max_depth: usize) -> Result<()> {
    if max_nodes == 0 || max_nodes > 10_000 || max_depth == 0 || max_depth > 100 {
        return Err(NativeError::invalid_request(
            "max_nodes must be 1..10000 and max_depth 1..100",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ElementRef;
    fn snapshot(revision: u64) -> Snapshot {
        let root = ElementRef {
            session: "s".into(),
            id: 1,
        };
        Snapshot {
            root: root.clone(),
            nodes: vec![crate::Node {
                reference: root,
                attributes: Default::default(),
                actions: vec![],
                parameterized_attributes: vec![],
                children: vec![],
                issues: vec![],
            }],
            complete: true,
            traversal_complete: true,
            revision,
            issues: vec![],
        }
    }
    #[test]
    fn history_is_bounded_and_shares_allocations() {
        let mut history = SnapshotHistory::new(2);
        let first = history.remember(snapshot(1));
        history.remember(snapshot(2));
        history.remember(snapshot(3));
        assert!(history.get(Some(1)).is_err());
        assert_eq!(history.get(None).unwrap().revision, 3);
        assert_eq!(history.get(Some(2)).unwrap().revision, 2);
        assert_eq!(Arc::strong_count(&first), 1);
        assert_eq!(history.summaries().len(), 2);
        let error = SnapshotHistory::new(0).get(None).unwrap_err();
        assert_eq!(error.code, "unknown_snapshot");
    }
    #[test]
    fn frames_expire_with_the_action_epoch() {
        let mut epoch = ActionEpoch::default();
        let mut frames = FrameHistory::<&str>::default();
        let id = frames.remember("frame", epoch);
        assert_eq!(frames.get(id, epoch).unwrap(), &"frame");
        epoch.record(&Ok(json!({"effect":"none"})));
        assert_eq!(frames.get(id, epoch).unwrap(), &"frame");
        epoch.record(&Ok(json!({"effect":"dispatched"})));
        assert_eq!(frames.get(id, epoch).unwrap_err().code, "stale_frame");
        assert_eq!(frames.get(99, epoch).unwrap_err().code, "unknown_frame");
        let mut reads = vec![Ok(json!(1)), Ok(json!(2))].into_iter();
        let report = wait_attribute(1000, &json!(2), || reads.next().unwrap()).unwrap();
        assert_eq!(report["matched"], true);
        assert!(wait_attribute(60_001, &json!(1), || Ok(json!(1))).is_err());
    }
    #[test]
    fn rendering_follows_format_without_mutating_state() {
        let snapshot = Arc::new(snapshot(7));
        let raw = render(snapshot.clone(), OutputFormat::Json, &Default::default()).unwrap();
        assert!(matches!(raw, Rendered::Raw(_)));
        let text = render(snapshot.clone(), OutputFormat::Text, &Default::default()).unwrap();
        assert!(
            serde_json::to_value(&text)
                .unwrap()
                .as_str()
                .unwrap()
                .contains("revision=7")
        );
        assert!(validate_budget(0, 1).is_err());
        assert!(validate_budget(1, 101).is_err());
        assert!(validate_budget(10, 10).is_ok());
    }
}

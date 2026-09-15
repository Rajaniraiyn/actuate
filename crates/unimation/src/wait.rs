//! Bounded, read-only polling of an explicit provider scope.
//!
//! A successful wait is evidence about its last observation, not app completion
//! or a promise that the next input will succeed. Native observation calls are
//! synchronous: the deadline prevents further polling but cannot cancel a call.
use crate::{
    ObservationBudget, ObserveScope, Result, Snapshot,
    query::{NodeQuery, query_nodes},
};
use std::{
    num::NonZeroUsize,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryCondition {
    Present,
    /// Requires complete attribute and traversal coverage, no observation/node
    /// issues, and no indeterminate query matches.
    Absent,
}

#[derive(Debug, Clone, Copy)]
pub struct WaitOptions {
    pub timeout: Duration,
    pub poll_interval: Duration,
    pub max_observations: NonZeroUsize,
    pub observation: ObservationBudget,
}
impl Default for WaitOptions {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(5),
            poll_interval: Duration::from_millis(100),
            max_observations: NonZeroUsize::new(100).unwrap(),
            observation: ObservationBudget::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionEvidence {
    Satisfied,
    Unsatisfied,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitOutcome {
    Satisfied,
    DeadlineReached,
    ObservationLimitReached,
}
#[derive(Debug)]
pub struct WaitReport {
    pub outcome: WaitOutcome,
    pub evidence: ConditionEvidence,
    pub elapsed: Duration,
    pub observations: usize,
    /// Full last observation including coverage flags and native issues.
    /// None means the deadline expired before the first observation.
    pub snapshot: Option<Snapshot>,
}

/// Evaluate only what this snapshot can establish. Incomplete observations may
/// prove presence, but never absence. This does not infer visibility/actionability.
pub fn query_evidence(
    snapshot: &Snapshot,
    query: &NodeQuery,
    condition: QueryCondition,
) -> ConditionEvidence {
    let result = query_nodes(snapshot, query);
    if !result.matches.is_empty() {
        return match condition {
            QueryCondition::Present => ConditionEvidence::Satisfied,
            QueryCondition::Absent => ConditionEvidence::Unsatisfied,
        };
    }
    if !snapshot.complete
        || !snapshot.traversal_complete
        || !snapshot.issues.is_empty()
        || snapshot.nodes.iter().any(|node| !node.issues.is_empty())
        || !result.indeterminate.is_empty()
    {
        return ConditionEvidence::Unknown;
    }
    match condition {
        QueryCondition::Present => ConditionEvidence::Unsatisfied,
        QueryCondition::Absent => ConditionEvidence::Satisfied,
    }
}

/// Poll a caller-selected scope. Native errors propagate unchanged immediately;
/// this function never retries input, changes scope, or substitutes a provider.
/// A result received after the deadline is retained as evidence but the outcome
/// remains DeadlineReached. A zero timeout performs no observation.
pub fn wait_for_query<P: ObserveScope + ?Sized>(
    provider: &mut P,
    scope: P::Scope,
    query: &NodeQuery,
    condition: QueryCondition,
    options: WaitOptions,
) -> Result<WaitReport>
where
    P::Scope: Clone,
{
    let start = Instant::now();
    let mut report = WaitReport {
        outcome: WaitOutcome::DeadlineReached,
        evidence: ConditionEvidence::Unknown,
        elapsed: Duration::ZERO,
        observations: 0,
        snapshot: None,
    };
    loop {
        report.elapsed = start.elapsed();
        if report.elapsed >= options.timeout {
            return Ok(report);
        }
        let snapshot = provider.observe_scope(scope.clone(), options.observation)?;
        report.observations += 1;
        report.evidence = query_evidence(&snapshot, query, condition);
        report.snapshot = Some(snapshot);
        report.elapsed = start.elapsed();
        if report.elapsed >= options.timeout {
            return Ok(report);
        }
        if report.evidence == ConditionEvidence::Satisfied {
            report.outcome = WaitOutcome::Satisfied;
            return Ok(report);
        }
        if report.observations >= options.max_observations.get() {
            report.outcome = WaitOutcome::ObservationLimitReached;
            return Ok(report);
        }
        std::thread::sleep(
            options
                .poll_interval
                .min(options.timeout.saturating_sub(report.elapsed)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Effect, ElementRef, NativeError, Node, query::TextMatch};
    use std::collections::{BTreeMap, VecDeque};
    fn snapshot(complete: bool, role: &str) -> Snapshot {
        let root = ElementRef {
            session: "test".into(),
            id: 1,
        };
        Snapshot {
            root: root.clone(),
            revision: 1,
            complete,
            traversal_complete: true,
            issues: vec![],
            nodes: vec![Node {
                reference: root,
                attributes: BTreeMap::from([("AXRole".into(), serde_json::json!(role))]),
                actions: vec![],
                parameterized_attributes: vec![],
                children: vec![],
                issues: vec![],
            }],
        }
    }
    fn query() -> NodeQuery {
        NodeQuery {
            role: Some(TextMatch::Exact {
                value: "AXButton".into(),
            }),
            ..Default::default()
        }
    }
    struct Provider(VecDeque<Result<Snapshot>>);
    impl ObserveScope for Provider {
        type Scope = u32;
        fn observe_scope(&mut self, scope: u32, _: ObservationBudget) -> Result<Snapshot> {
            assert_eq!(scope, 7);
            self.0.pop_front().expect("unexpected observation")
        }
    }
    #[test]
    fn incomplete_coverage_proves_presence_but_not_absence() {
        assert_eq!(
            query_evidence(
                &snapshot(false, "AXButton"),
                &query(),
                QueryCondition::Present
            ),
            ConditionEvidence::Satisfied
        );
        assert_eq!(
            query_evidence(
                &snapshot(false, "AXWindow"),
                &query(),
                QueryCondition::Absent
            ),
            ConditionEvidence::Unknown
        );
        let mut s = snapshot(true, "AXWindow");
        assert_eq!(
            query_evidence(&s, &query(), QueryCondition::Absent),
            ConditionEvidence::Satisfied
        );
        s.nodes[0]
            .issues
            .push(serde_json::json!({"code":"read_failed"}));
        assert_eq!(
            query_evidence(&s, &query(), QueryCondition::Absent),
            ConditionEvidence::Unknown
        );
    }
    #[test]
    fn polling_retains_last_full_observation_and_stops_at_limit() {
        let mut p = Provider(VecDeque::from([
            Ok(snapshot(false, "AXWindow")),
            Ok(snapshot(false, "AXWindow")),
        ]));
        let options = WaitOptions {
            timeout: Duration::from_secs(60),
            poll_interval: Duration::ZERO,
            max_observations: NonZeroUsize::new(2).unwrap(),
            ..Default::default()
        };
        let result = wait_for_query(&mut p, 7, &query(), QueryCondition::Absent, options).unwrap();
        assert_eq!(result.outcome, WaitOutcome::ObservationLimitReached);
        assert_eq!(result.evidence, ConditionEvidence::Unknown);
        assert_eq!(result.observations, 2);
        assert!(!result.snapshot.unwrap().complete);
    }
    #[test]
    fn observed_presence_stops_polling_without_requiring_complete_coverage() {
        let mut p = Provider(VecDeque::from([
            Ok(snapshot(false, "AXWindow")),
            Ok(snapshot(false, "AXButton")),
        ]));
        let options = WaitOptions {
            timeout: Duration::from_secs(60),
            poll_interval: Duration::ZERO,
            ..Default::default()
        };
        let result = wait_for_query(&mut p, 7, &query(), QueryCondition::Present, options).unwrap();
        assert_eq!(result.outcome, WaitOutcome::Satisfied);
        assert_eq!(result.observations, 2);
        assert_eq!(
            result.snapshot.unwrap().nodes[0].attributes["AXRole"],
            "AXButton"
        );
    }
    #[test]
    fn zero_timeout_does_not_call_provider_and_native_errors_are_preserved() {
        let mut p = Provider(VecDeque::new());
        let result = wait_for_query(
            &mut p,
            7,
            &query(),
            QueryCondition::Present,
            WaitOptions {
                timeout: Duration::ZERO,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(result.outcome, WaitOutcome::DeadlineReached);
        assert_eq!(result.observations, 0);
        p.0.push_back(Err(NativeError {
            code: "original".into(),
            message: "native detail".into(),
            effect: Effect::Unknown,
        }));
        let error = wait_for_query(
            &mut p,
            7,
            &query(),
            QueryCondition::Present,
            WaitOptions::default(),
        )
        .unwrap_err();
        assert_eq!(error.code, "original");
        assert!(matches!(error.effect, Effect::Unknown));
    }
}

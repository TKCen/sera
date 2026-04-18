//! Circle coordination logic — SPEC-circles.
//!
//! This module implements the runtime-independent core of Circle coordination:
//!
//! - [`ConcurrencyPolicy`] enforcement via [`ConcurrencyScheduler`] — Sequential,
//!   Parallel, Bounded(n).
//! - [`ResultAggregator`] trait with built-in impls ([`FirstSuccess`],
//!   [`Majority`], [`AllComplete`], [`Custom`]).
//! - [`ConvergenceConfig`] loop terminators: MaxIterations, FixedPoint,
//!   PredicateSatisfied.
//! - [`WorkflowMemoryManager`] — per-circle scoped, namespaced K/V handle.
//! - [`CoordinationPolicy`] — the 7 SPEC-circles policies as enum variants.
//! - [`Coordinator::run`] — dispatcher that wires policy -> concurrency ->
//!   aggregator -> convergence.
//!
//! This crate is intentionally transport-agnostic: participants are identified
//! by opaque [`ParticipantId`] strings and tasks are represented by a single
//! serde_json::Value payload.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Identifier for a coordination participant (agent, sub-circle, or human).
pub type ParticipantId = String;

/// A single task payload dispatched to a participant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordTask {
    pub task_id: String,
    pub payload: Value,
}

/// Result of a single participant's execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordResult {
    pub participant: ParticipantId,
    pub task_id: String,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Outcome {
    Success(Value),
    Failure(String),
}

impl Outcome {
    pub fn is_success(&self) -> bool {
        matches!(self, Outcome::Success(_))
    }
}

// =========================================================================
// ConcurrencyPolicy
// =========================================================================

/// How a scheduler dispatches N participants to execute N tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConcurrencyPolicy {
    Sequential,
    Parallel,
    Bounded(u32),
}

/// Execution callback: given a participant + task, produce a result.
pub type ExecFn = dyn Fn(&ParticipantId, &CoordTask) -> CoordResult + Send + Sync;

/// Applies a [`ConcurrencyPolicy`] to dispatch tasks across participants.
pub struct ConcurrencyScheduler {
    policy: ConcurrencyPolicy,
}

impl ConcurrencyScheduler {
    pub fn new(policy: ConcurrencyPolicy) -> Self {
        Self { policy }
    }

    pub fn run(
        &self,
        pairs: Vec<(ParticipantId, CoordTask)>,
        exec: &ExecFn,
    ) -> Vec<CoordResult> {
        match self.policy {
            ConcurrencyPolicy::Sequential => pairs
                .into_iter()
                .map(|(p, t)| exec(&p, &t))
                .collect(),
            ConcurrencyPolicy::Parallel => {
                std::thread::scope(|s| {
                    let handles: Vec<_> = pairs
                        .iter()
                        .map(|(p, t)| s.spawn(|| exec(p, t)))
                        .collect();
                    handles
                        .into_iter()
                        .map(|h| h.join().expect("participant thread panicked"))
                        .collect()
                })
            }
            ConcurrencyPolicy::Bounded(n) => {
                let limit = (n.max(1)) as usize;
                let mut out = Vec::with_capacity(pairs.len());
                for chunk in pairs.chunks(limit) {
                    std::thread::scope(|s| {
                        let handles: Vec<_> = chunk
                            .iter()
                            .map(|(p, t)| s.spawn(|| exec(p, t)))
                            .collect();
                        for h in handles {
                            out.push(h.join().expect("participant thread panicked"));
                        }
                    });
                }
                out
            }
        }
    }
}

// =========================================================================
// ResultAggregator
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AggregatedResult {
    Final(Outcome),
    NeedsMore { received: u32, required: u32 },
    Failed(String),
}

#[derive(Debug, thiserror::Error)]
pub enum AggregationError {
    #[error("no results to aggregate")]
    Empty,
    #[error("aggregation failed: {0}")]
    Other(String),
}

pub trait ResultAggregator: Send + Sync {
    fn name(&self) -> &str;
    fn aggregate(&self, results: &[CoordResult]) -> Result<AggregatedResult, AggregationError>;
}

/// First successful result wins.
pub struct FirstSuccess;

impl ResultAggregator for FirstSuccess {
    fn name(&self) -> &str {
        "first_success"
    }

    fn aggregate(&self, results: &[CoordResult]) -> Result<AggregatedResult, AggregationError> {
        if results.is_empty() {
            return Err(AggregationError::Empty);
        }
        for r in results {
            if r.outcome.is_success() {
                return Ok(AggregatedResult::Final(r.outcome.clone()));
            }
        }
        let last = results.last().unwrap();
        Ok(AggregatedResult::Failed(format!(
            "all {} participants failed (last: {:?})",
            results.len(),
            last.outcome
        )))
    }
}

/// Majority vote over equal-valued successful outcomes.
pub struct Majority {
    pub quorum: f32,
}

impl Majority {
    pub fn simple() -> Self {
        Self { quorum: 0.5 }
    }
}

impl ResultAggregator for Majority {
    fn name(&self) -> &str {
        "majority"
    }

    fn aggregate(&self, results: &[CoordResult]) -> Result<AggregatedResult, AggregationError> {
        if results.is_empty() {
            return Err(AggregationError::Empty);
        }
        let total = results.len() as f32;
        let required = ((total * self.quorum).floor() as u32) + 1;

        let mut counts: HashMap<String, (u32, Value)> = HashMap::new();
        for r in results {
            if let Outcome::Success(v) = &r.outcome {
                let key = serde_json::to_string(v).unwrap_or_default();
                counts
                    .entry(key)
                    .and_modify(|e| e.0 += 1)
                    .or_insert((1, v.clone()));
            }
        }

        let winner = counts
            .iter()
            .max_by(|a, b| a.1 .0.cmp(&b.1 .0).then_with(|| b.0.cmp(a.0)))
            .map(|(_, (c, v))| (*c, v.clone()));

        match winner {
            Some((count, value)) if count >= required => {
                Ok(AggregatedResult::Final(Outcome::Success(value)))
            }
            Some((count, _)) => Ok(AggregatedResult::NeedsMore {
                received: count,
                required,
            }),
            None => Ok(AggregatedResult::Failed(
                "no successful results for majority vote".into(),
            )),
        }
    }
}

/// All participants must succeed. Final value is an array of payloads.
pub struct AllComplete;

impl ResultAggregator for AllComplete {
    fn name(&self) -> &str {
        "all_complete"
    }

    fn aggregate(&self, results: &[CoordResult]) -> Result<AggregatedResult, AggregationError> {
        if results.is_empty() {
            return Err(AggregationError::Empty);
        }
        let mut payloads = Vec::with_capacity(results.len());
        for r in results {
            match &r.outcome {
                Outcome::Success(v) => payloads.push(v.clone()),
                Outcome::Failure(msg) => {
                    return Ok(AggregatedResult::Failed(format!(
                        "participant {} failed: {}",
                        r.participant, msg
                    )));
                }
            }
        }
        Ok(AggregatedResult::Final(Outcome::Success(Value::Array(
            payloads,
        ))))
    }
}

/// Hook-supplied aggregator.
pub struct Custom {
    name: String,
    #[allow(clippy::type_complexity)]
    func: Box<dyn Fn(&[CoordResult]) -> Result<AggregatedResult, AggregationError> + Send + Sync>,
}

impl Custom {
    pub fn new<F>(name: impl Into<String>, func: F) -> Self
    where
        F: Fn(&[CoordResult]) -> Result<AggregatedResult, AggregationError> + Send + Sync + 'static,
    {
        Self {
            name: name.into(),
            func: Box::new(func),
        }
    }
}

impl ResultAggregator for Custom {
    fn name(&self) -> &str {
        &self.name
    }

    fn aggregate(&self, results: &[CoordResult]) -> Result<AggregatedResult, AggregationError> {
        (self.func)(results)
    }
}

// =========================================================================
// ConvergenceConfig
// =========================================================================

#[derive(Clone)]
pub enum ConvergenceConfig {
    MaxIterations(u32),
    FixedPoint,
    #[allow(clippy::type_complexity)]
    PredicateSatisfied(Arc<dyn Fn(&Outcome) -> bool + Send + Sync>),
}

impl std::fmt::Debug for ConvergenceConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConvergenceConfig::MaxIterations(n) => write!(f, "MaxIterations({n})"),
            ConvergenceConfig::FixedPoint => f.write_str("FixedPoint"),
            ConvergenceConfig::PredicateSatisfied(_) => f.write_str("PredicateSatisfied(<fn>)"),
        }
    }
}

pub struct ConvergenceState {
    config: ConvergenceConfig,
    iterations: u32,
    last_payload: Option<String>,
}

impl ConvergenceState {
    pub fn new(config: ConvergenceConfig) -> Self {
        Self {
            config,
            iterations: 0,
            last_payload: None,
        }
    }

    pub fn iterations(&self) -> u32 {
        self.iterations
    }

    pub fn step(&mut self, outcome: &Outcome) -> bool {
        self.iterations += 1;
        match &self.config {
            ConvergenceConfig::MaxIterations(n) => self.iterations >= *n,
            ConvergenceConfig::FixedPoint => {
                let current = match outcome {
                    Outcome::Success(v) => serde_json::to_string(v).unwrap_or_default(),
                    Outcome::Failure(m) => format!("__FAIL__{m}"),
                };
                let stop = self
                    .last_payload
                    .as_ref()
                    .map(|prev| prev == &current)
                    .unwrap_or(false);
                self.last_payload = Some(current);
                stop
            }
            ConvergenceConfig::PredicateSatisfied(f) => f(outcome),
        }
    }
}

// =========================================================================
// WorkflowMemoryManager
// =========================================================================

#[derive(Clone, Default)]
pub struct WorkflowMemoryManager {
    inner: Arc<Mutex<HashMap<String, Value>>>,
}

impl WorkflowMemoryManager {
    pub fn new() -> Self {
        Self::default()
    }

    fn scoped(circle_id: &str, key: &str) -> String {
        format!("{circle_id}::{key}")
    }

    pub fn for_circle(&self, circle_id: impl Into<String>) -> CircleMemory {
        CircleMemory {
            circle_id: circle_id.into(),
            store: self.inner.clone(),
        }
    }

    pub fn dump(&self, circle_id: &str) -> Vec<(String, Value)> {
        let prefix = format!("{circle_id}::");
        let guard = self.inner.lock().expect("memory lock");
        guard
            .iter()
            .filter_map(|(k, v)| k.strip_prefix(&prefix).map(|k| (k.to_string(), v.clone())))
            .collect()
    }
}

pub struct CircleMemory {
    circle_id: String,
    store: Arc<Mutex<HashMap<String, Value>>>,
}

impl CircleMemory {
    pub fn circle_id(&self) -> &str {
        &self.circle_id
    }

    pub fn get(&self, key: &str) -> Option<Value> {
        let scoped = WorkflowMemoryManager::scoped(&self.circle_id, key);
        self.store.lock().expect("memory lock").get(&scoped).cloned()
    }

    pub fn set(&self, key: impl Into<String>, value: Value) {
        let scoped = WorkflowMemoryManager::scoped(&self.circle_id, &key.into());
        self.store.lock().expect("memory lock").insert(scoped, value);
    }

    pub fn remove(&self, key: &str) -> Option<Value> {
        let scoped = WorkflowMemoryManager::scoped(&self.circle_id, key);
        self.store.lock().expect("memory lock").remove(&scoped)
    }

    pub fn keys(&self) -> Vec<String> {
        let prefix = format!("{}::", self.circle_id);
        self.store
            .lock()
            .expect("memory lock")
            .keys()
            .filter_map(|k| k.strip_prefix(&prefix).map(|k| k.to_string()))
            .collect()
    }
}

// =========================================================================
// CoordinationPolicy
// =========================================================================

#[derive(Clone, Debug)]
pub enum CoordinationPolicy {
    Sequential,
    Parallel,
    Pipeline {
        steps: Vec<ParticipantId>,
    },
    Debate {
        proposer: ParticipantId,
        opponent: ParticipantId,
        convergence: ConvergenceConfig,
    },
    Council {
        voters: Vec<ParticipantId>,
    },
    Hierarchical {
        manager: ParticipantId,
        workers: Vec<ParticipantId>,
    },
    Consensus {
        voters: Vec<ParticipantId>,
        quorum: f32,
    },
}

impl CoordinationPolicy {
    pub fn kind(&self) -> &'static str {
        match self {
            CoordinationPolicy::Sequential => "sequential",
            CoordinationPolicy::Parallel => "parallel",
            CoordinationPolicy::Pipeline { .. } => "pipeline",
            CoordinationPolicy::Debate { .. } => "debate",
            CoordinationPolicy::Council { .. } => "council",
            CoordinationPolicy::Hierarchical { .. } => "hierarchical",
            CoordinationPolicy::Consensus { .. } => "consensus",
        }
    }

    fn participants(&self, fallback: &[ParticipantId]) -> Vec<ParticipantId> {
        match self {
            CoordinationPolicy::Sequential | CoordinationPolicy::Parallel => fallback.to_vec(),
            CoordinationPolicy::Pipeline { steps } => steps.clone(),
            CoordinationPolicy::Debate {
                proposer, opponent, ..
            } => vec![proposer.clone(), opponent.clone()],
            CoordinationPolicy::Council { voters } => voters.clone(),
            CoordinationPolicy::Hierarchical { manager, workers } => {
                let mut v = Vec::with_capacity(workers.len() + 1);
                v.push(manager.clone());
                v.extend(workers.clone());
                v
            }
            CoordinationPolicy::Consensus { voters, .. } => voters.clone(),
        }
    }
}

// =========================================================================
// Coordinator dispatcher
// =========================================================================

#[derive(Debug, thiserror::Error)]
pub enum CoordinationError {
    #[error("no participants to dispatch")]
    NoParticipants,
    #[error("aggregation failed: {0}")]
    Aggregation(#[from] AggregationError),
    #[error("convergence exhausted without a successful outcome")]
    ConvergenceExhausted,
}

pub struct Coordinator {
    pub policy: CoordinationPolicy,
    pub concurrency: ConcurrencyPolicy,
    pub aggregator: Box<dyn ResultAggregator>,
    /// Optional observer for sub-agent `delegate-task` dispatches — bead
    /// `sera-8d1.1` (GH#144). See [`SubagentDelegationObserver`].
    pub(crate) subagent_observer: Option<Arc<dyn SubagentDelegationObserver>>,
}

impl Coordinator {
    pub fn new(
        policy: CoordinationPolicy,
        concurrency: ConcurrencyPolicy,
        aggregator: Box<dyn ResultAggregator>,
    ) -> Self {
        Self {
            policy,
            concurrency,
            aggregator,
            subagent_observer: None,
        }
    }

    pub fn run(
        &self,
        task: CoordTask,
        fallback_participants: &[ParticipantId],
        exec: &ExecFn,
    ) -> Result<AggregatedResult, CoordinationError> {
        let participants = self.policy.participants(fallback_participants);
        if participants.is_empty() {
            return Err(CoordinationError::NoParticipants);
        }

        let scheduler = ConcurrencyScheduler::new(self.concurrency);

        match &self.policy {
            CoordinationPolicy::Sequential
            | CoordinationPolicy::Parallel
            | CoordinationPolicy::Pipeline { .. }
            | CoordinationPolicy::Council { .. }
            | CoordinationPolicy::Hierarchical { .. }
            | CoordinationPolicy::Consensus { .. } => {
                let pairs: Vec<(ParticipantId, CoordTask)> = participants
                    .iter()
                    .map(|p| (p.clone(), task.clone()))
                    .collect();
                let results = scheduler.run(pairs, exec);
                Ok(self.aggregator.aggregate(&results)?)
            }
            CoordinationPolicy::Debate {
                proposer,
                opponent,
                convergence,
            } => {
                let mut state = ConvergenceState::new(convergence.clone());
                let mut last: Option<AggregatedResult> = None;
                let mut current_task = task.clone();
                const SAFETY_CAP: u32 = 64;
                while state.iterations() < SAFETY_CAP {
                    let pairs = vec![
                        (proposer.clone(), current_task.clone()),
                        (opponent.clone(), current_task.clone()),
                    ];
                    let results = scheduler.run(pairs, exec);
                    let agg = self.aggregator.aggregate(&results)?;
                    let repr = match &agg {
                        AggregatedResult::Final(o) => o.clone(),
                        AggregatedResult::NeedsMore { .. } => {
                            Outcome::Failure("needs more".into())
                        }
                        AggregatedResult::Failed(msg) => Outcome::Failure(msg.clone()),
                    };
                    let stop = state.step(&repr);
                    if let AggregatedResult::Final(Outcome::Success(v)) = &agg {
                        current_task.payload = v.clone();
                    }
                    last = Some(agg);
                    if stop {
                        break;
                    }
                }
                last.ok_or(CoordinationError::ConvergenceExhausted)
            }
        }
    }
}

// =========================================================================
// Sub-agent delegation observer (bead sera-8d1.1 / GH#144)
// =========================================================================

/// Notice delivered to a [`SubagentDelegationObserver`] when an agent invokes
/// the `delegate-task` agent-tool from `sera_runtime::agent_tool_registry`.
///
/// Mirrors `sera_runtime::agent_tool_registry::DelegationNotice` field-for-field
/// so adapters can be a thin re-pack.  We avoid taking a dep on sera-runtime
/// to keep the dependency graph acyclic — embedders bridge the two via a
/// `CoordinatorHook` impl that pushes into a `SubagentDelegationObserver`.
#[derive(Debug, Clone)]
pub struct SubagentDelegationNotice {
    /// The agent that issued the delegate-task call.
    pub caller: ParticipantId,
    /// The agent that received the delegated task.
    pub target: ParticipantId,
    /// Tokens credited back to the caller's budget for this dispatch.
    pub tokens_used: u64,
}

/// Trait the workflow coordinator implements to observe sub-agent delegate
/// calls.  See bead `sera-8d1.1` (GH#144).
///
/// TODO(sera-8d1.1): `Coordinator::run` currently has no central event bus
/// to publish into.  When the workflow event bus lands, replace this trait
/// with a publish into that bus.  Until then, embedders register an
/// observer via `Coordinator::with_subagent_observer` and bridge it to the
/// runtime registry's `CoordinatorHook` themselves.
pub trait SubagentDelegationObserver: Send + Sync + 'static {
    /// Called once per successful synchronous delegate-task dispatch.
    fn on_delegation(&self, notice: SubagentDelegationNotice);
}

impl Coordinator {
    /// Attach a sub-agent delegation observer.  See
    /// [`SubagentDelegationObserver`] for the consumer contract.
    pub fn with_subagent_observer(
        mut self,
        observer: Arc<dyn SubagentDelegationObserver>,
    ) -> Self {
        self.subagent_observer = Some(observer);
        self
    }

    /// Manually publish a delegation notice through the attached observer
    /// (if any). Wire-up: an `agent_tool_registry::CoordinatorHook` impl
    /// can call this to forward notices into the workflow coordinator.
    pub fn publish_subagent_notice(&self, notice: SubagentDelegationNotice) {
        if let Some(obs) = &self.subagent_observer {
            obs.on_delegation(notice);
        }
    }
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ok(participant: &str, task_id: &str, v: Value) -> CoordResult {
        CoordResult {
            participant: participant.into(),
            task_id: task_id.into(),
            outcome: Outcome::Success(v),
        }
    }
    fn fail(participant: &str, task_id: &str, msg: &str) -> CoordResult {
        CoordResult {
            participant: participant.into(),
            task_id: task_id.into(),
            outcome: Outcome::Failure(msg.into()),
        }
    }

    #[test]
    fn sequential_preserves_order() {
        let sched = ConcurrencyScheduler::new(ConcurrencyPolicy::Sequential);
        let task = CoordTask {
            task_id: "t".into(),
            payload: json!(1),
        };
        let pairs = vec![
            ("a".to_string(), task.clone()),
            ("b".into(), task.clone()),
            ("c".into(), task.clone()),
        ];
        let exec: Box<ExecFn> = Box::new(|p, t| ok(p, &t.task_id, json!(p.to_string())));
        let results = sched.run(pairs, exec.as_ref());
        let names: Vec<String> = results.into_iter().map(|r| r.participant).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[test]
    fn bounded_respects_limit() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let in_flight = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));
        let in_flight2 = in_flight.clone();
        let max_seen2 = max_seen.clone();
        let sched = ConcurrencyScheduler::new(ConcurrencyPolicy::Bounded(2));
        let task = CoordTask {
            task_id: "t".into(),
            payload: json!(1),
        };
        let pairs: Vec<(String, CoordTask)> =
            (0..5).map(|i| (format!("p{i}"), task.clone())).collect();
        let exec: Box<ExecFn> = Box::new(move |p, t| {
            let cur = in_flight2.fetch_add(1, Ordering::SeqCst) + 1;
            max_seen2.fetch_max(cur, Ordering::SeqCst);
            in_flight2.fetch_sub(1, Ordering::SeqCst);
            ok(p, &t.task_id, json!(p.to_string()))
        });
        let results = sched.run(pairs, exec.as_ref());
        assert_eq!(results.len(), 5);
        assert!(max_seen.load(Ordering::SeqCst) <= 2);
    }

    #[test]
    fn parallel_runs_concurrently() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::Duration;
        let in_flight = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));
        let in_flight2 = in_flight.clone();
        let max_seen2 = max_seen.clone();
        let sched = ConcurrencyScheduler::new(ConcurrencyPolicy::Parallel);
        let task = CoordTask {
            task_id: "t".into(),
            payload: json!(1),
        };
        let pairs: Vec<(String, CoordTask)> =
            (0..4).map(|i| (format!("p{i}"), task.clone())).collect();
        let exec: Box<ExecFn> = Box::new(move |p, t| {
            let cur = in_flight2.fetch_add(1, Ordering::SeqCst) + 1;
            max_seen2.fetch_max(cur, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(20));
            in_flight2.fetch_sub(1, Ordering::SeqCst);
            ok(p, &t.task_id, json!(p.to_string()))
        });
        let results = sched.run(pairs, exec.as_ref());
        assert_eq!(results.len(), 4);
        assert!(
            max_seen.load(Ordering::SeqCst) > 1,
            "expected >1 concurrent tasks, got {}" ,
            max_seen.load(Ordering::SeqCst)
        );
    }

    #[test]
    fn first_success_returns_first_ok() {
        let agg = FirstSuccess;
        let res = vec![
            fail("a", "t", "nope"),
            ok("b", "t", json!("B")),
            ok("c", "t", json!("C")),
        ];
        match agg.aggregate(&res).unwrap() {
            AggregatedResult::Final(Outcome::Success(v)) => assert_eq!(v, json!("B")),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn first_success_all_fail() {
        let agg = FirstSuccess;
        let res = vec![fail("a", "t", "x"), fail("b", "t", "y")];
        match agg.aggregate(&res).unwrap() {
            AggregatedResult::Failed(_) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn first_success_empty_errs() {
        let agg = FirstSuccess;
        assert!(matches!(agg.aggregate(&[]), Err(AggregationError::Empty)));
    }

    #[test]
    fn majority_reaches_quorum() {
        let agg = Majority::simple();
        let res = vec![
            ok("a", "t", json!("Y")),
            ok("b", "t", json!("Y")),
            ok("c", "t", json!("N")),
        ];
        match agg.aggregate(&res).unwrap() {
            AggregatedResult::Final(Outcome::Success(v)) => assert_eq!(v, json!("Y")),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn majority_no_quorum_needs_more() {
        let agg = Majority::simple();
        let res = vec![
            ok("a", "t", json!("Y")),
            ok("b", "t", json!("N")),
            ok("c", "t", json!("M")),
        ];
        match agg.aggregate(&res).unwrap() {
            AggregatedResult::NeedsMore { received, required } => {
                assert_eq!(received, 1);
                assert_eq!(required, 2);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn all_complete_collects_payloads() {
        let agg = AllComplete;
        let res = vec![
            ok("a", "t", json!(1)),
            ok("b", "t", json!(2)),
            ok("c", "t", json!(3)),
        ];
        match agg.aggregate(&res).unwrap() {
            AggregatedResult::Final(Outcome::Success(Value::Array(v))) => {
                assert_eq!(v, vec![json!(1), json!(2), json!(3)]);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn all_complete_fails_if_any_fails() {
        let agg = AllComplete;
        let res = vec![ok("a", "t", json!(1)), fail("b", "t", "bad")];
        match agg.aggregate(&res).unwrap() {
            AggregatedResult::Failed(msg) => assert!(msg.contains("b")),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn custom_aggregator_runs() {
        let agg = Custom::new("sum", |results| {
            let sum: i64 = results
                .iter()
                .filter_map(|r| match &r.outcome {
                    Outcome::Success(Value::Number(n)) => n.as_i64(),
                    _ => None,
                })
                .sum();
            Ok(AggregatedResult::Final(Outcome::Success(json!(sum))))
        });
        let res = vec![
            ok("a", "t", json!(1)),
            ok("b", "t", json!(2)),
            ok("c", "t", json!(3)),
        ];
        match agg.aggregate(&res).unwrap() {
            AggregatedResult::Final(Outcome::Success(v)) => assert_eq!(v, json!(6)),
            other => panic!("unexpected: {other:?}"),
        }
        assert_eq!(agg.name(), "sum");
    }

    #[test]
    fn max_iterations_terminator() {
        let mut state = ConvergenceState::new(ConvergenceConfig::MaxIterations(3));
        assert!(!state.step(&Outcome::Success(json!(1))));
        assert!(!state.step(&Outcome::Success(json!(2))));
        assert!(state.step(&Outcome::Success(json!(3))));
    }

    #[test]
    fn fixed_point_terminator() {
        let mut state = ConvergenceState::new(ConvergenceConfig::FixedPoint);
        assert!(!state.step(&Outcome::Success(json!("a"))));
        assert!(!state.step(&Outcome::Success(json!("b"))));
        assert!(state.step(&Outcome::Success(json!("b"))));
    }

    #[test]
    fn predicate_terminator() {
        let pred: Arc<dyn Fn(&Outcome) -> bool + Send + Sync> =
            Arc::new(|o: &Outcome| matches!(o, Outcome::Success(Value::Bool(true))));
        let mut state = ConvergenceState::new(ConvergenceConfig::PredicateSatisfied(pred));
        assert!(!state.step(&Outcome::Success(json!(false))));
        assert!(state.step(&Outcome::Success(json!(true))));
    }

    #[test]
    fn memory_is_namespaced_by_circle() {
        let mgr = WorkflowMemoryManager::new();
        let a = mgr.for_circle("circle-a");
        let b = mgr.for_circle("circle-b");
        a.set("k", json!("in-a"));
        b.set("k", json!("in-b"));
        assert_eq!(a.get("k"), Some(json!("in-a")));
        assert_eq!(b.get("k"), Some(json!("in-b")));
        assert_eq!(a.keys(), vec!["k".to_string()]);
        assert_eq!(a.remove("k"), Some(json!("in-a")));
        assert_eq!(a.get("k"), None);
        assert_eq!(b.get("k"), Some(json!("in-b")));
    }

    #[test]
    fn memory_dump_returns_only_scoped_entries() {
        let mgr = WorkflowMemoryManager::new();
        let a = mgr.for_circle("a");
        a.set("x", json!(1));
        a.set("y", json!(2));
        mgr.for_circle("b").set("x", json!(99));
        let mut dump = mgr.dump("a");
        dump.sort_by(|x, y| x.0.cmp(&y.0));
        assert_eq!(
            dump,
            vec![("x".to_string(), json!(1)), ("y".to_string(), json!(2))]
        );
    }

    #[test]
    fn coordinator_parallel_all_complete() {
        let coord = Coordinator::new(
            CoordinationPolicy::Parallel,
            ConcurrencyPolicy::Parallel,
            Box::new(AllComplete),
        );
        let task = CoordTask {
            task_id: "job".into(),
            payload: json!(0),
        };
        let exec: Box<ExecFn> = Box::new(|p, t| ok(p, &t.task_id, json!(p.clone())));
        let out = coord
            .run(task, &["x".into(), "y".into(), "z".into()], exec.as_ref())
            .unwrap();
        match out {
            AggregatedResult::Final(Outcome::Success(Value::Array(v))) => {
                assert_eq!(v, vec![json!("x"), json!("y"), json!("z")]);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn coordinator_council_majority() {
        let coord = Coordinator::new(
            CoordinationPolicy::Council {
                voters: vec!["a".into(), "b".into(), "c".into()],
            },
            ConcurrencyPolicy::Bounded(2),
            Box::new(Majority::simple()),
        );
        let task = CoordTask {
            task_id: "vote".into(),
            payload: json!(null),
        };
        let exec: Box<ExecFn> = Box::new(|p, t| match p.as_str() {
            "c" => ok(p, &t.task_id, json!("N")),
            _ => ok(p, &t.task_id, json!("Y")),
        });
        let out = coord.run(task, &[], exec.as_ref()).unwrap();
        match out {
            AggregatedResult::Final(Outcome::Success(v)) => assert_eq!(v, json!("Y")),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn coordinator_no_participants_errs() {
        let coord = Coordinator::new(
            CoordinationPolicy::Parallel,
            ConcurrencyPolicy::Sequential,
            Box::new(AllComplete),
        );
        let task = CoordTask {
            task_id: "x".into(),
            payload: json!(null),
        };
        let exec: Box<ExecFn> = Box::new(|p, t| ok(p, &t.task_id, json!(null)));
        let err = coord.run(task, &[], exec.as_ref()).unwrap_err();
        assert!(matches!(err, CoordinationError::NoParticipants));
    }

    #[test]
    fn coordinator_debate_terminates() {
        let coord = Coordinator::new(
            CoordinationPolicy::Debate {
                proposer: "p".into(),
                opponent: "o".into(),
                convergence: ConvergenceConfig::MaxIterations(2),
            },
            ConcurrencyPolicy::Sequential,
            Box::new(FirstSuccess),
        );
        let task = CoordTask {
            task_id: "d".into(),
            payload: json!("start"),
        };
        let exec: Box<ExecFn> =
            Box::new(|p, t| ok(p, &t.task_id, json!(format!("{}:reply", p))));
        let out = coord.run(task, &[], exec.as_ref()).unwrap();
        match out {
            AggregatedResult::Final(Outcome::Success(_)) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn subagent_delegation_observer_receives_notice() {
        struct Recorder(Arc<Mutex<Vec<SubagentDelegationNotice>>>);
        impl SubagentDelegationObserver for Recorder {
            fn on_delegation(&self, notice: SubagentDelegationNotice) {
                self.0.lock().unwrap().push(notice);
            }
        }
        let log = Arc::new(Mutex::new(Vec::new()));
        let coord = Coordinator::new(
            CoordinationPolicy::Sequential,
            ConcurrencyPolicy::Sequential,
            Box::new(FirstSuccess),
        )
        .with_subagent_observer(Arc::new(Recorder(log.clone())));

        coord.publish_subagent_notice(SubagentDelegationNotice {
            caller: "parent".into(),
            target: "worker".into(),
            tokens_used: 17,
        });

        let entries = log.lock().unwrap().clone();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].caller, "parent");
        assert_eq!(entries[0].target, "worker");
        assert_eq!(entries[0].tokens_used, 17);
    }

    #[test]
    fn subagent_observer_absent_is_noop() {
        // No observer attached — must not panic.
        let coord = Coordinator::new(
            CoordinationPolicy::Sequential,
            ConcurrencyPolicy::Sequential,
            Box::new(FirstSuccess),
        );
        coord.publish_subagent_notice(SubagentDelegationNotice {
            caller: "p".into(),
            target: "t".into(),
            tokens_used: 0,
        });
    }

    #[test]
    fn policy_kind_strings() {
        assert_eq!(CoordinationPolicy::Sequential.kind(), "sequential");
        assert_eq!(CoordinationPolicy::Parallel.kind(), "parallel");
        assert_eq!(
            CoordinationPolicy::Pipeline { steps: vec![] }.kind(),
            "pipeline"
        );
        assert_eq!(
            CoordinationPolicy::Council { voters: vec![] }.kind(),
            "council"
        );
        assert_eq!(
            CoordinationPolicy::Hierarchical {
                manager: "m".into(),
                workers: vec![]
            }
            .kind(),
            "hierarchical"
        );
        assert_eq!(
            CoordinationPolicy::Consensus {
                voters: vec![],
                quorum: 0.5
            }
            .kind(),
            "consensus"
        );
    }
}

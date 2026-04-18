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

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use sera_types::circle::{
    BlackboardEntry, BlackboardRetention, PartyConfig, PartyOrdering, PartyOutcome, PartyResponse,
    PartyRound, TerminationCondition, AGENT_DECISION_ARTIFACT,
};

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
// CircleBlackboard (SPEC-circles §3f / bead sera-8d1.3)
// =========================================================================

/// Monotonic cursor into a [`CircleBlackboard`].
///
/// Acts as an index into the append-only log. After compaction, cursors
/// remain valid: callers can always ask for "everything after this cursor".
pub type BlackboardCursor = u64;

/// Starting cursor value — readers that pass this get the full current
/// snapshot (up to retention).
pub const BLACKBOARD_START: BlackboardCursor = 0;

/// Append-only shared artifact bus for a Circle session.
///
/// Entries are stored in insertion order with a monotonically-increasing
/// cursor (`seq`). [`BlackboardRetention`] is applied on every append.
///
/// Thread-safety is the *caller's* responsibility — wrap in `Arc<Mutex<_>>`
/// (or a lock-free cell) for concurrent producers.
pub struct CircleBlackboard {
    entries: VecDeque<(BlackboardCursor, BlackboardEntry)>,
    retention: BlackboardRetention,
    next_seq: BlackboardCursor,
    /// Wall-clock start for `Timeout` predicates. Monotonic; tests may
    /// override via [`CircleBlackboard::set_started_at`].
    started_at: Instant,
    /// External-signal latch — consulted by `ExternalSignal`.
    external_signal: bool,
}

impl std::fmt::Debug for CircleBlackboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CircleBlackboard")
            .field("entries", &self.entries.len())
            .field("retention", &self.retention)
            .field("next_seq", &self.next_seq)
            .field("external_signal", &self.external_signal)
            .finish()
    }
}

impl CircleBlackboard {
    /// Create a new blackboard with unbounded retention.
    pub fn new() -> Self {
        Self::with_retention(BlackboardRetention::default())
    }

    /// Create a new blackboard with the given retention policy.
    pub fn with_retention(retention: BlackboardRetention) -> Self {
        Self {
            entries: VecDeque::new(),
            retention,
            next_seq: 1,
            started_at: Instant::now(),
            external_signal: false,
        }
    }

    /// Replace the session-start instant. Useful for deterministic timeout
    /// tests; production code should leave the default.
    pub fn set_started_at(&mut self, when: Instant) {
        self.started_at = when;
    }

    /// Append a new entry. Returns its assigned cursor.
    /// Compaction via [`BlackboardRetention`] runs on every append.
    pub fn append(&mut self, entry: BlackboardEntry) -> BlackboardCursor {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        self.entries.push_back((seq, entry));
        self.compact();
        seq
    }

    /// Convenience: build and append an entry with the current wall clock.
    pub fn record(
        &mut self,
        participant_id: impl Into<String>,
        artifact_type: impl Into<String>,
        payload: Value,
    ) -> BlackboardCursor {
        let entry = BlackboardEntry {
            participant_id: participant_id.into(),
            timestamp: Utc::now(),
            artifact_type: artifact_type.into(),
            payload,
        };
        self.append(entry)
    }

    /// Entries with a cursor strictly greater than `cursor`.
    /// Pass [`BLACKBOARD_START`] to receive the full current snapshot.
    pub fn entries_since(&self, cursor: BlackboardCursor) -> Vec<&BlackboardEntry> {
        self.entries
            .iter()
            .filter(|(seq, _)| *seq > cursor)
            .map(|(_, e)| e)
            .collect()
    }

    /// All entries authored by `participant_id`, in insertion order.
    pub fn entries_by_participant(&self, participant_id: &str) -> Vec<&BlackboardEntry> {
        self.entries
            .iter()
            .filter(|(_, e)| e.participant_id == participant_id)
            .map(|(_, e)| e)
            .collect()
    }

    /// Cloned snapshot of all live entries in insertion order.
    pub fn snapshot(&self) -> Vec<BlackboardEntry> {
        self.entries.iter().map(|(_, e)| e.clone()).collect()
    }

    /// Current live entry count.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Total entries ever appended (includes dropped-by-retention).
    pub fn total_appended(&self) -> u64 {
        self.next_seq.saturating_sub(1)
    }

    /// True when the blackboard has zero live entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Current retention policy.
    pub fn retention(&self) -> &BlackboardRetention {
        &self.retention
    }

    /// Replace the retention policy, applying it immediately.
    pub fn set_retention(&mut self, retention: BlackboardRetention) {
        self.retention = retention;
        self.compact();
    }

    /// Trigger the `ExternalSignal` termination branch.
    pub fn signal_external_stop(&mut self) {
        self.external_signal = true;
    }

    /// Whether the external stop signal has been raised.
    pub fn external_signal_raised(&self) -> bool {
        self.external_signal
    }

    /// Evaluate a [`TerminationCondition`] against this blackboard.
    pub fn evaluate(&self, cond: &TerminationCondition) -> bool {
        match cond {
            TerminationCondition::MaxMessages(n) => self.total_appended() >= u64::from(*n),
            TerminationCondition::TextMention(needle) => self.entries.iter().any(|(_, e)| {
                payload_contains_text(&e.payload, needle)
                    || e.artifact_type.contains(needle.as_str())
            }),
            TerminationCondition::Timeout(d) => self.started_at.elapsed() >= *d,
            TerminationCondition::AgentDecision => self
                .entries
                .iter()
                .any(|(_, e)| e.artifact_type == AGENT_DECISION_ARTIFACT),
            TerminationCondition::ExternalSignal => self.external_signal,
            TerminationCondition::And(a, b) => self.evaluate(a) && self.evaluate(b),
            TerminationCondition::Or(a, b) => self.evaluate(a) || self.evaluate(b),
        }
    }

    /// Apply retention to trim oldest entries.
    fn compact(&mut self) {
        if let Some(max_age) = self.retention.max_age {
            let cutoff = Utc::now() - chrono::Duration::from_std(max_age).unwrap_or_default();
            while let Some((_, front)) = self.entries.front() {
                if front.timestamp < cutoff {
                    self.entries.pop_front();
                } else {
                    break;
                }
            }
        }
        if let Some(max_entries) = self.retention.max_entries {
            let cap = max_entries.get();
            while self.entries.len() > cap {
                self.entries.pop_front();
            }
        }
    }
}

impl Default for CircleBlackboard {
    fn default() -> Self {
        Self::new()
    }
}

/// Recursive walk — any string leaf in `payload` containing `needle` wins.
fn payload_contains_text(payload: &Value, needle: &str) -> bool {
    match payload {
        Value::String(s) => s.contains(needle),
        Value::Array(xs) => xs.iter().any(|v| payload_contains_text(v, needle)),
        Value::Object(map) => map.values().any(|v| payload_contains_text(v, needle)),
        _ => false,
    }
}

/// Why a [`Coordinator::run`] loop terminated.
#[derive(Debug, Clone, PartialEq)]
pub enum CircleStopReason {
    /// A [`TerminationCondition`] evaluated true against the blackboard.
    Condition,
    /// The aggregator produced a final result and no condition was set.
    Completed,
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
    /// BMAD-style party mode — all members discuss in rounds via the
    /// blackboard, a designated synthesizer produces the final output.
    ///
    /// Drive this variant via [`Coordinator::run_party`] rather than
    /// [`Coordinator::run`]; `run` returns `NoParticipants` for Party because
    /// the party loop is self-directed via [`PartyMember`] trait objects.
    ///
    /// Bead `sera-8d1.2` (GH#145).
    Party { config: PartyConfig },
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
            CoordinationPolicy::Party { .. } => "party",
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
            // Party is driven by `run_party`, which supplies its own member
            // list; `run` rejects Party via NoParticipants by returning an
            // empty fallback here.
            CoordinationPolicy::Party { .. } => Vec::new(),
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
    #[error("terminated by TerminationCondition")]
    TerminatedByCondition,
    /// Returned when [`Coordinator::run_party`] is invoked with a policy that
    /// is not [`CoordinationPolicy::Party`], or when a party run is missing
    /// its synthesizer from the supplied member list.
    #[error("party configuration invalid: {0}")]
    PartyConfig(String),
}

pub struct Coordinator {
    pub policy: CoordinationPolicy,
    pub concurrency: ConcurrencyPolicy,
    pub aggregator: Box<dyn ResultAggregator>,
    /// Optional observer for sub-agent `delegate-task` dispatches — bead
    /// `sera-8d1.1` (GH#144). See [`SubagentDelegationObserver`].
    pub(crate) subagent_observer: Option<Arc<dyn SubagentDelegationObserver>>,
    /// Shared blackboard — populated by policy branches (Party mode in
    /// bead `sera-8d1.2`) and read by termination predicates.
    ///
    /// Bead `sera-8d1.3` (GH#146).
    pub(crate) blackboard: Arc<Mutex<CircleBlackboard>>,
    /// Active termination condition evaluated after each round of
    /// [`Coordinator::run`]. `None` disables blackboard-driven termination
    /// and relies solely on aggregator / convergence semantics.
    pub(crate) termination: Option<TerminationCondition>,
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
            blackboard: Arc::new(Mutex::new(CircleBlackboard::new())),
            termination: None,
        }
    }

    /// Attach a pre-built blackboard (e.g. with a custom retention policy).
    pub fn with_blackboard(mut self, blackboard: CircleBlackboard) -> Self {
        self.blackboard = Arc::new(Mutex::new(blackboard));
        self
    }

    /// Set the termination condition evaluated after each round.
    pub fn with_termination(mut self, cond: TerminationCondition) -> Self {
        self.termination = Some(cond);
        self
    }

    /// Shared blackboard handle — clone the `Arc` for concurrent producers.
    pub fn blackboard_handle(&self) -> Arc<Mutex<CircleBlackboard>> {
        self.blackboard.clone()
    }

    /// Borrow the blackboard via the stored lock. Panics on poisoned lock.
    pub fn blackboard(&self) -> std::sync::MutexGuard<'_, CircleBlackboard> {
        self.blackboard.lock().expect("blackboard lock poisoned")
    }

    /// True when the configured [`TerminationCondition`] evaluates true
    /// against the current blackboard. Always `false` when no condition
    /// is set.
    pub fn should_terminate(&self) -> bool {
        match &self.termination {
            Some(cond) => self.blackboard().evaluate(cond),
            None => false,
        }
    }

    /// Raise the external stop signal. Used by embedders for the
    /// [`TerminationCondition::ExternalSignal`] branch.
    pub fn signal_external_stop(&self) {
        self.blackboard().signal_external_stop();
    }

    pub fn run(
        &self,
        task: CoordTask,
        fallback_participants: &[ParticipantId],
        exec: &ExecFn,
    ) -> Result<AggregatedResult, CoordinationError> {
        // Party is not driven via `run` — embedders call `run_party` directly
        // with their [`PartyMember`] implementations. Surface this as a
        // dedicated error before the fallback participant check so callers
        // get a helpful message rather than `NoParticipants`.
        if let CoordinationPolicy::Party { .. } = &self.policy {
            return Err(CoordinationError::PartyConfig(
                "use Coordinator::run_party for Party coordination".into(),
            ));
        }

        let participants = self.policy.participants(fallback_participants);
        if participants.is_empty() {
            return Err(CoordinationError::NoParticipants);
        }

        let scheduler = ConcurrencyScheduler::new(self.concurrency);

        // Pre-run termination check — e.g. external signal already raised.
        if self.should_terminate() {
            self.archive_session(CircleStopReason::Condition);
            return Err(CoordinationError::TerminatedByCondition);
        }

        let result = match &self.policy {
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
                self.aggregator.aggregate(&results)?
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
                let mut terminated_by_condition = false;
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
                    // Post-round termination check for long-running loops.
                    if self.should_terminate() {
                        terminated_by_condition = true;
                        break;
                    }
                }
                if terminated_by_condition {
                    self.archive_session(CircleStopReason::Condition);
                    return Err(CoordinationError::TerminatedByCondition);
                }
                last.ok_or(CoordinationError::ConvergenceExhausted)?
            }
            // Handled by the early-return `PartyConfig` branch above.
            CoordinationPolicy::Party { .. } => unreachable!("Party handled above"),
        };

        // Post-run termination check — e.g. MaxMessages reached during this
        // round via participant-side blackboard appends.
        if self.should_terminate() {
            self.archive_session(CircleStopReason::Condition);
            return Err(CoordinationError::TerminatedByCondition);
        }

        self.archive_session(CircleStopReason::Completed);
        Ok(result)
    }

    /// Emit a stop marker into the blackboard on termination.  The marker
    /// is append-only so external archivers (DB sinks, audit) can replay it.
    fn archive_session(&self, reason: CircleStopReason) {
        let mut bb = self.blackboard();
        let total = bb.total_appended();
        bb.append(BlackboardEntry {
            participant_id: "__coordinator__".into(),
            timestamp: Utc::now(),
            artifact_type: "circle_session_archived".into(),
            payload: serde_json::json!({
                "reason": match reason {
                    CircleStopReason::Condition => "condition",
                    CircleStopReason::Completed => "completed",
                },
                "total_entries": total,
            }),
        });
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
// Party mode (bead sera-8d1.2 / GH#145)
// =========================================================================

/// A member of a Party mode coordination.
///
/// The runtime seam is intentionally minimal: each member is prompted with
/// the round's broadcast prompt plus a snapshot of every blackboard entry
/// posted up to (but not including) this member's turn. Members return their
/// response text as a plain `String`.
///
/// Implementations live outside this crate; `sera-runtime` wires real LLM
/// members, while tests use the lightweight [`EchoPartyMember`] fixture.
/// This trait is synchronous for determinism in tests — real LLM-backed
/// implementations can block on a tokio runtime internally.
pub trait PartyMember: Send + Sync {
    /// Stable participant id — matches the id used in blackboard entries.
    fn id(&self) -> &str;

    /// Optional importance hint consulted when the Party config uses
    /// [`PartyOrdering::ImportanceBased`]. Higher values run earlier.
    ///
    /// Default is `None` — callers with no importance signal fall back to
    /// [`PartyOrdering::RoundRobin`] ordering.
    fn importance(&self) -> Option<f32> {
        None
    }

    /// Produce a response for `prompt` given the visible transcript.
    fn respond(&self, prompt: &str, transcript: &[BlackboardEntry]) -> String;
}

/// Party-mode blackboard artifact types. Kept here rather than in
/// `sera-types` because they are pure runtime coordination markers, not
/// data types that survive YAML round-trip.
pub const PARTY_PROMPT_ARTIFACT: &str = "party.prompt";
pub const PARTY_RESPONSE_ARTIFACT: &str = "party.response";
pub const PARTY_SYNTHESIS_ARTIFACT: &str = "party.synthesis";

impl Coordinator {
    /// Run a Party mode coordination against a slice of [`PartyMember`]s.
    ///
    /// Flow:
    /// 1. Append the broadcast `prompt` to the blackboard as `party.prompt`.
    /// 2. For each of `max_rounds` rounds, iterate members in the order
    ///    dictated by [`PartyOrdering`]; each member sees the full blackboard
    ///    transcript up to its turn and appends its response as
    ///    `party.response`.
    /// 3. Honor the configured [`TerminationCondition`] — stop early if
    ///    `should_terminate()` fires between rounds.
    /// 4. After the last round, feed the full transcript to the synthesizer
    ///    (matched from `members` by id) and archive its output as
    ///    `party.synthesis`.
    ///
    /// Returns a structured [`PartyOutcome`] on success; the blackboard
    /// receives one `circle_session_archived` entry at the end.
    ///
    /// # Errors
    /// - `PartyConfig` if the coordinator's policy isn't `Party`, members is
    ///   empty, or the configured synthesizer id is absent from `members`.
    /// - `TerminatedByCondition` if a configured termination condition fires
    ///   before synthesis runs.
    pub fn run_party(
        &self,
        prompt: &str,
        members: &[&dyn PartyMember],
    ) -> Result<PartyOutcome, CoordinationError> {
        let cfg = match &self.policy {
            CoordinationPolicy::Party { config } => config.clone(),
            _ => {
                return Err(CoordinationError::PartyConfig(
                    "run_party requires CoordinationPolicy::Party".into(),
                ));
            }
        };
        if members.is_empty() {
            return Err(CoordinationError::NoParticipants);
        }
        if !members.iter().any(|m| m.id() == cfg.synthesizer) {
            return Err(CoordinationError::PartyConfig(format!(
                "synthesizer '{}' not present in party members",
                cfg.synthesizer
            )));
        }

        // Pre-run termination check — e.g. external signal already raised.
        if self.should_terminate() {
            self.archive_session(CircleStopReason::Condition);
            return Err(CoordinationError::TerminatedByCondition);
        }

        let order = party_turn_order(&cfg.ordering, members);
        let mut rounds: Vec<PartyRound> = Vec::with_capacity(cfg.max_rounds as usize);

        for round_idx in 0..cfg.max_rounds {
            let round_no = round_idx + 1;
            // Broadcast the prompt for this round.
            let prompts_sent_at = Utc::now();
            {
                let mut bb = self.blackboard();
                bb.append(BlackboardEntry {
                    participant_id: "__coordinator__".into(),
                    timestamp: prompts_sent_at,
                    artifact_type: PARTY_PROMPT_ARTIFACT.into(),
                    payload: serde_json::json!({
                        "round": round_no,
                        "prompt": prompt,
                    }),
                });
            }

            let mut responses: Vec<PartyResponse> = Vec::with_capacity(order.len());
            for member_idx in order.iter().copied() {
                // Each member sees the full current blackboard snapshot.
                let transcript = self.blackboard().snapshot();
                let member = members[member_idx];
                let text = member.respond(prompt, &transcript);
                let posted_at = Utc::now();
                {
                    let mut bb = self.blackboard();
                    bb.append(BlackboardEntry {
                        participant_id: member.id().to_string(),
                        timestamp: posted_at,
                        artifact_type: PARTY_RESPONSE_ARTIFACT.into(),
                        payload: serde_json::json!({
                            "round": round_no,
                            "text": text,
                        }),
                    });
                }
                responses.push(PartyResponse {
                    participant_id: member.id().to_string(),
                    text,
                    posted_at,
                });

                // Fine-grained termination check — e.g. MaxMessages may fire
                // mid-round once enough responses have been posted.
                if self.should_terminate() {
                    rounds.push(PartyRound {
                        round_no,
                        prompts_sent_at,
                        responses,
                    });
                    self.archive_session(CircleStopReason::Condition);
                    return Err(CoordinationError::TerminatedByCondition);
                }
            }

            rounds.push(PartyRound {
                round_no,
                prompts_sent_at,
                responses,
            });

            // Post-round termination check.
            if self.should_terminate() {
                self.archive_session(CircleStopReason::Condition);
                return Err(CoordinationError::TerminatedByCondition);
            }
        }

        // Synthesis turn — the synthesizer sees the full transcript and
        // produces the final output.
        let transcript = self.blackboard().snapshot();
        let synthesizer = members
            .iter()
            .find(|m| m.id() == cfg.synthesizer)
            .copied()
            .expect("synthesizer presence checked above");
        let synthesis = synthesizer.respond(prompt, &transcript);
        {
            let mut bb = self.blackboard();
            bb.append(BlackboardEntry {
                participant_id: synthesizer.id().to_string(),
                timestamp: Utc::now(),
                artifact_type: PARTY_SYNTHESIS_ARTIFACT.into(),
                payload: serde_json::json!({
                    "text": synthesis,
                }),
            });
        }

        self.archive_session(CircleStopReason::Completed);

        Ok(PartyOutcome { rounds, synthesis })
    }
}

/// Compute a turn order (indices into `members`) for a single Party round.
///
/// For [`PartyOrdering::RoundRobin`], this is just `0..members.len()`.
/// For [`PartyOrdering::ImportanceBased`], members are sorted by descending
/// `importance()`. Members without an importance hint are stable-sorted
/// after those with hints; when *no* member supplies a hint, we fall back
/// to RoundRobin ordering.
///
/// TODO(sera-8d1.2-importance): Surface an explicit importance override
/// per-round via the Circle manifest so callers can tune priority without
/// modifying member implementations (separate follow-up bead).
fn party_turn_order(ordering: &PartyOrdering, members: &[&dyn PartyMember]) -> Vec<usize> {
    let default_order: Vec<usize> = (0..members.len()).collect();
    match ordering {
        PartyOrdering::RoundRobin => default_order,
        PartyOrdering::ImportanceBased => {
            let any_hint = members.iter().any(|m| m.importance().is_some());
            if !any_hint {
                return default_order;
            }
            let mut order = default_order;
            order.sort_by(|a, b| {
                let ia = members[*a].importance().unwrap_or(f32::NEG_INFINITY);
                let ib = members[*b].importance().unwrap_or(f32::NEG_INFINITY);
                ib.partial_cmp(&ia).unwrap_or(std::cmp::Ordering::Equal)
            });
            order
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

    // =====================================================================
    // Blackboard + TerminationCondition (bead sera-8d1.3 / GH#146)
    // =====================================================================

    fn bb_entry(participant: &str, artifact: &str, payload: Value) -> BlackboardEntry {
        BlackboardEntry {
            participant_id: participant.into(),
            timestamp: Utc::now(),
            artifact_type: artifact.into(),
            payload,
        }
    }

    #[test]
    fn termination_max_messages_fires_at_limit() {
        let mut bb = CircleBlackboard::new();
        let cond = TerminationCondition::MaxMessages(3);
        for i in 0..3 {
            assert!(!bb.evaluate(&cond), "should not terminate at i={i}");
            bb.append(bb_entry("p", "message", json!(i)));
        }
        assert!(bb.evaluate(&cond), "should terminate after 3 messages");
    }

    #[test]
    fn termination_text_mention_fires_on_payload_match() {
        let mut bb = CircleBlackboard::new();
        let cond = TerminationCondition::TextMention("ADJOURN".into());
        bb.append(bb_entry("p", "message", json!("hello")));
        assert!(!bb.evaluate(&cond));
        bb.append(bb_entry("p", "message", json!("please ADJOURN now")));
        assert!(bb.evaluate(&cond));
    }

    #[test]
    fn termination_timeout_fires_after_elapsed_duration() {
        let mut bb = CircleBlackboard::new();
        let cond = TerminationCondition::Timeout(std::time::Duration::from_millis(50));
        assert!(!bb.evaluate(&cond));
        // Rewind the "started_at" 60ms into the past.
        bb.set_started_at(Instant::now() - std::time::Duration::from_millis(60));
        assert!(bb.evaluate(&cond));
    }

    #[test]
    fn termination_agent_decision_fires_on_stop_artifact() {
        let mut bb = CircleBlackboard::new();
        let cond = TerminationCondition::AgentDecision;
        bb.append(bb_entry("p", "message", json!("normal")));
        assert!(!bb.evaluate(&cond));
        bb.append(bb_entry("p", AGENT_DECISION_ARTIFACT, json!({"reason": "done"})));
        assert!(bb.evaluate(&cond));
    }

    #[test]
    fn termination_external_signal_triggered_via_api() {
        let coord = Coordinator::new(
            CoordinationPolicy::Sequential,
            ConcurrencyPolicy::Sequential,
            Box::new(FirstSuccess),
        )
        .with_termination(TerminationCondition::ExternalSignal);
        assert!(!coord.should_terminate());
        coord.signal_external_stop();
        assert!(coord.should_terminate());
    }

    #[test]
    fn termination_and_requires_both_true() {
        let mut bb = CircleBlackboard::new();
        let cond = TerminationCondition::And(
            Box::new(TerminationCondition::MaxMessages(2)),
            Box::new(TerminationCondition::TextMention("go".into())),
        );
        bb.append(bb_entry("p", "message", json!("first")));
        bb.append(bb_entry("p", "message", json!("second")));
        // MaxMessages satisfied but TextMention not.
        assert!(!bb.evaluate(&cond));
        bb.append(bb_entry("p", "message", json!("time to go")));
        assert!(bb.evaluate(&cond));
    }

    #[test]
    fn termination_or_fires_on_either() {
        let mut bb = CircleBlackboard::new();
        let cond = TerminationCondition::Or(
            Box::new(TerminationCondition::TextMention("halt".into())),
            Box::new(TerminationCondition::MaxMessages(100)),
        );
        assert!(!bb.evaluate(&cond));
        bb.append(bb_entry("p", "message", json!("please halt")));
        assert!(bb.evaluate(&cond));
    }

    #[test]
    fn termination_nested_composition() {
        // And(MaxMessages(2), Or(TextMention("X"), Timeout(1s)))
        let cond = TerminationCondition::And(
            Box::new(TerminationCondition::MaxMessages(2)),
            Box::new(TerminationCondition::Or(
                Box::new(TerminationCondition::TextMention("X".into())),
                Box::new(TerminationCondition::Timeout(
                    std::time::Duration::from_secs(1),
                )),
            )),
        );

        let mut bb = CircleBlackboard::new();
        bb.append(bb_entry("p", "message", json!("a")));
        assert!(!bb.evaluate(&cond));
        bb.append(bb_entry("p", "message", json!("b")));
        // MaxMessages now satisfied, but inner Or has neither X nor timeout.
        assert!(!bb.evaluate(&cond));
        bb.append(bb_entry("p", "message", json!("X marks it")));
        assert!(bb.evaluate(&cond));
    }

    #[test]
    fn blackboard_compaction_by_max_entries() {
        let cap = std::num::NonZeroUsize::new(3).unwrap();
        let mut bb =
            CircleBlackboard::with_retention(BlackboardRetention::with_max_entries(cap));
        for i in 0..10 {
            bb.append(bb_entry("p", "message", json!(i)));
        }
        assert_eq!(bb.len(), 3);
        assert_eq!(bb.total_appended(), 10);
        let snap = bb.snapshot();
        // oldest retained should be the 8th (0-indexed: payload 7,8,9)
        assert_eq!(snap[0].payload, json!(7));
        assert_eq!(snap[2].payload, json!(9));
    }

    #[test]
    fn blackboard_compaction_by_max_age() {
        // Use very-small max_age so appends trim themselves by the time the
        // retention window passes via a sleep.
        let mut bb = CircleBlackboard::with_retention(BlackboardRetention::with_max_age(
            std::time::Duration::from_millis(40),
        ));
        bb.append(bb_entry("p", "message", json!("old")));
        std::thread::sleep(std::time::Duration::from_millis(70));
        bb.append(bb_entry("p", "message", json!("fresh")));
        // 'old' was older than max_age at the time 'fresh' was appended and
        // should have been compacted out.
        let snap = bb.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].payload, json!("fresh"));
    }

    #[test]
    fn blackboard_entries_since_returns_only_new() {
        let mut bb = CircleBlackboard::new();
        let c1 = bb.append(bb_entry("p", "message", json!(1)));
        let _c2 = bb.append(bb_entry("p", "message", json!(2)));
        let _c3 = bb.append(bb_entry("p", "message", json!(3)));

        let since_start = bb.entries_since(BLACKBOARD_START);
        assert_eq!(since_start.len(), 3);

        let since_first = bb.entries_since(c1);
        assert_eq!(since_first.len(), 2);
        assert_eq!(since_first[0].payload, json!(2));
        assert_eq!(since_first[1].payload, json!(3));
    }

    #[test]
    fn blackboard_entries_by_participant_filters_correctly() {
        let mut bb = CircleBlackboard::new();
        bb.append(bb_entry("alice", "message", json!(1)));
        bb.append(bb_entry("bob", "message", json!(2)));
        bb.append(bb_entry("alice", "message", json!(3)));
        let alice_entries = bb.entries_by_participant("alice");
        assert_eq!(alice_entries.len(), 2);
        assert_eq!(alice_entries[0].payload, json!(1));
        assert_eq!(alice_entries[1].payload, json!(3));
    }

    #[test]
    fn coordinator_termination_archives_and_errors() {
        // Pre-populate the blackboard so MaxMessages(1) fires before
        // Coordinator::run even dispatches.
        let coord = Coordinator::new(
            CoordinationPolicy::Sequential,
            ConcurrencyPolicy::Sequential,
            Box::new(FirstSuccess),
        )
        .with_termination(TerminationCondition::MaxMessages(1));
        coord
            .blackboard()
            .append(bb_entry("p", "message", json!("pre")));

        let task = CoordTask {
            task_id: "x".into(),
            payload: json!(null),
        };
        let exec: Box<ExecFn> = Box::new(|p, t| ok(p, &t.task_id, json!(p.clone())));
        let err = coord
            .run(task, &["a".into()], exec.as_ref())
            .expect_err("should terminate");
        assert!(matches!(err, CoordinationError::TerminatedByCondition));

        // Session must be archived in the blackboard.
        let archived = coord
            .blackboard()
            .snapshot()
            .iter()
            .any(|e| e.artifact_type == "circle_session_archived");
        assert!(archived, "expected archival marker");
    }

    #[test]
    fn coordinator_post_run_archives_completed() {
        let coord = Coordinator::new(
            CoordinationPolicy::Parallel,
            ConcurrencyPolicy::Sequential,
            Box::new(AllComplete),
        );
        let task = CoordTask {
            task_id: "done".into(),
            payload: json!(null),
        };
        let exec: Box<ExecFn> = Box::new(|p, t| ok(p, &t.task_id, json!(p.clone())));
        let _ = coord
            .run(task, &["a".into(), "b".into()], exec.as_ref())
            .unwrap();
        let archived = coord
            .blackboard()
            .snapshot()
            .iter()
            .any(|e| {
                e.artifact_type == "circle_session_archived"
                    && e.payload["reason"] == json!("completed")
            });
        assert!(archived);
    }

    // =====================================================================
    // Party mode (bead sera-8d1.2 / GH#145)
    // =====================================================================

    /// Deterministic party member used in tests. Responds with a canned
    /// template containing its id and the round count observed in the
    /// transcript so assertions can verify it ran N times.
    struct EchoMember {
        id: String,
        importance: Option<f32>,
    }

    impl EchoMember {
        fn new(id: &str) -> Self {
            Self {
                id: id.to_string(),
                importance: None,
            }
        }
        fn with_importance(id: &str, i: f32) -> Self {
            Self {
                id: id.to_string(),
                importance: Some(i),
            }
        }
    }

    impl PartyMember for EchoMember {
        fn id(&self) -> &str {
            &self.id
        }
        fn importance(&self) -> Option<f32> {
            self.importance
        }
        fn respond(&self, _prompt: &str, transcript: &[BlackboardEntry]) -> String {
            let prior = transcript
                .iter()
                .filter(|e| {
                    e.participant_id == self.id
                        && e.artifact_type == PARTY_RESPONSE_ARTIFACT
                })
                .count();
            format!("{}:round-{}", self.id, prior + 1)
        }
    }

    #[test]
    fn party_round_robin_three_members_two_rounds() {
        let cfg = sera_types::circle::PartyConfig {
            max_rounds: 2,
            ordering: PartyOrdering::RoundRobin,
            synthesizer: "carol".into(),
        };
        let coord = Coordinator::new(
            CoordinationPolicy::Party { config: cfg },
            ConcurrencyPolicy::Sequential,
            Box::new(FirstSuccess),
        );
        let alice = EchoMember::new("alice");
        let bob = EchoMember::new("bob");
        let carol = EchoMember::new("carol");
        let members: Vec<&dyn PartyMember> = vec![&alice, &bob, &carol];
        let outcome = coord
            .run_party("Discuss the plan", &members)
            .expect("party run succeeds");

        assert_eq!(outcome.rounds.len(), 2);
        assert_eq!(outcome.rounds[0].round_no, 1);
        assert_eq!(outcome.rounds[1].round_no, 2);
        // Each member posted exactly 2 responses (one per round).
        for id in &["alice", "bob", "carol"] {
            let count: usize = outcome
                .rounds
                .iter()
                .map(|r| r.responses.iter().filter(|x| x.participant_id == *id).count())
                .sum();
            assert_eq!(count, 2, "member {id} did not post twice");
        }
        // Synthesis text should reflect the synthesizer's id.
        assert!(
            outcome.synthesis.starts_with("carol:"),
            "synthesis did not come from synthesizer: {}",
            outcome.synthesis
        );

        // Blackboard has one prompt per round, one response per member per
        // round, one synthesis, plus one session_archived marker.
        let bb = coord.blackboard();
        let prompts = bb
            .snapshot()
            .iter()
            .filter(|e| e.artifact_type == PARTY_PROMPT_ARTIFACT)
            .count();
        let responses = bb
            .snapshot()
            .iter()
            .filter(|e| e.artifact_type == PARTY_RESPONSE_ARTIFACT)
            .count();
        let synthesis = bb
            .snapshot()
            .iter()
            .filter(|e| e.artifact_type == PARTY_SYNTHESIS_ARTIFACT)
            .count();
        assert_eq!(prompts, 2);
        assert_eq!(responses, 6); // 3 members × 2 rounds
        assert_eq!(synthesis, 1);
    }

    #[test]
    fn party_blackboard_cursor_advances_between_rounds() {
        let cfg = sera_types::circle::PartyConfig {
            max_rounds: 2,
            ordering: PartyOrdering::RoundRobin,
            synthesizer: "a".into(),
        };
        let coord = Coordinator::new(
            CoordinationPolicy::Party { config: cfg },
            ConcurrencyPolicy::Sequential,
            Box::new(FirstSuccess),
        );
        let a = EchoMember::new("a");
        let b = EchoMember::new("b");
        let members: Vec<&dyn PartyMember> = vec![&a, &b];
        coord
            .run_party("q", &members)
            .expect("party run succeeds");

        // Entries are strictly ordered — round 1 responses appear before
        // round 2 prompt, which appears before round 2 responses.
        let snap = coord.blackboard().snapshot();
        let first_round_prompt_pos = snap
            .iter()
            .position(|e| {
                e.artifact_type == PARTY_PROMPT_ARTIFACT && e.payload["round"] == 1
            })
            .unwrap();
        let second_round_prompt_pos = snap
            .iter()
            .position(|e| {
                e.artifact_type == PARTY_PROMPT_ARTIFACT && e.payload["round"] == 2
            })
            .unwrap();
        assert!(first_round_prompt_pos < second_round_prompt_pos);
    }

    #[test]
    fn party_synthesis_invoked_after_final_round() {
        let cfg = sera_types::circle::PartyConfig {
            max_rounds: 1,
            ordering: PartyOrdering::RoundRobin,
            synthesizer: "syn".into(),
        };
        let coord = Coordinator::new(
            CoordinationPolicy::Party { config: cfg },
            ConcurrencyPolicy::Sequential,
            Box::new(FirstSuccess),
        );
        let a = EchoMember::new("a");
        let syn = EchoMember::new("syn");
        let members: Vec<&dyn PartyMember> = vec![&a, &syn];
        let outcome = coord.run_party("q", &members).unwrap();
        // Synthesis comes after the responses it sees — syn should see its
        // own round-1 response already in the transcript → "syn:round-2".
        assert_eq!(outcome.synthesis, "syn:round-2");
    }

    #[test]
    fn party_early_termination_via_max_messages() {
        let cfg = sera_types::circle::PartyConfig {
            max_rounds: 5,
            ordering: PartyOrdering::RoundRobin,
            synthesizer: "a".into(),
        };
        // 3 members × 5 rounds = 15 responses, plus 5 prompts = 20 entries.
        // MaxMessages(3) should fire during round 1 (after the first prompt
        // and first few responses).
        let coord = Coordinator::new(
            CoordinationPolicy::Party { config: cfg },
            ConcurrencyPolicy::Sequential,
            Box::new(FirstSuccess),
        )
        .with_termination(TerminationCondition::MaxMessages(3));
        let a = EchoMember::new("a");
        let b = EchoMember::new("b");
        let c = EchoMember::new("c");
        let members: Vec<&dyn PartyMember> = vec![&a, &b, &c];
        let err = coord
            .run_party("q", &members)
            .expect_err("should terminate early");
        assert!(matches!(err, CoordinationError::TerminatedByCondition));
        // Session archival marker must be present.
        let archived = coord
            .blackboard()
            .snapshot()
            .iter()
            .any(|e| e.artifact_type == "circle_session_archived");
        assert!(archived);
    }

    #[test]
    fn party_importance_based_ordering_sorts_members() {
        let cfg = sera_types::circle::PartyConfig {
            max_rounds: 1,
            ordering: PartyOrdering::ImportanceBased,
            synthesizer: "z".into(),
        };
        let coord = Coordinator::new(
            CoordinationPolicy::Party { config: cfg },
            ConcurrencyPolicy::Sequential,
            Box::new(FirstSuccess),
        );
        let low = EchoMember::with_importance("low", 0.1);
        let high = EchoMember::with_importance("high", 9.0);
        let mid = EchoMember::with_importance("mid", 5.0);
        let z = EchoMember::with_importance("z", 0.0);
        let members: Vec<&dyn PartyMember> = vec![&low, &high, &mid, &z];
        let outcome = coord.run_party("q", &members).unwrap();
        let ids: Vec<_> = outcome.rounds[0]
            .responses
            .iter()
            .map(|r| r.participant_id.clone())
            .collect();
        assert_eq!(ids, vec!["high", "mid", "low", "z"]);
    }

    #[test]
    fn party_importance_based_with_no_hints_falls_back_to_round_robin() {
        let cfg = sera_types::circle::PartyConfig {
            max_rounds: 1,
            ordering: PartyOrdering::ImportanceBased,
            synthesizer: "c".into(),
        };
        let coord = Coordinator::new(
            CoordinationPolicy::Party { config: cfg },
            ConcurrencyPolicy::Sequential,
            Box::new(FirstSuccess),
        );
        let a = EchoMember::new("a");
        let b = EchoMember::new("b");
        let c = EchoMember::new("c");
        let members: Vec<&dyn PartyMember> = vec![&a, &b, &c];
        let outcome = coord.run_party("q", &members).unwrap();
        let ids: Vec<_> = outcome.rounds[0]
            .responses
            .iter()
            .map(|r| r.participant_id.clone())
            .collect();
        // Fallback is declaration order.
        assert_eq!(ids, vec!["a", "b", "c"]);
    }

    #[test]
    fn party_rejects_missing_synthesizer() {
        let cfg = sera_types::circle::PartyConfig {
            max_rounds: 1,
            ordering: PartyOrdering::RoundRobin,
            synthesizer: "ghost".into(),
        };
        let coord = Coordinator::new(
            CoordinationPolicy::Party { config: cfg },
            ConcurrencyPolicy::Sequential,
            Box::new(FirstSuccess),
        );
        let a = EchoMember::new("a");
        let members: Vec<&dyn PartyMember> = vec![&a];
        let err = coord.run_party("q", &members).expect_err("synth missing");
        assert!(matches!(err, CoordinationError::PartyConfig(_)));
    }

    #[test]
    fn party_run_via_generic_run_errors_with_party_config() {
        let cfg = sera_types::circle::PartyConfig::new("a");
        let coord = Coordinator::new(
            CoordinationPolicy::Party { config: cfg },
            ConcurrencyPolicy::Sequential,
            Box::new(FirstSuccess),
        );
        let task = CoordTask {
            task_id: "x".into(),
            payload: json!(null),
        };
        let exec: Box<ExecFn> = Box::new(|p, t| ok(p, &t.task_id, json!(p.clone())));
        let err = coord
            .run(task, &["a".into()], exec.as_ref())
            .expect_err("party requires run_party");
        assert!(matches!(err, CoordinationError::PartyConfig(_)));
    }
}

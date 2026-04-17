//! Dynamic model routing primitives — Phase 1: in-memory `HealthStore`; Phase 2: weighted selection;
//! Phase 3: circuit breakers + catalog auto-discovery scaffold.
//!
//! See `docs/plan/DYNAMIC-MODEL-ROUTING.md` §5 (data model), §6 (selection), §7 (auto-discovery),
//! §8 (failover), §13 (phases).
//!
//! This module provides:
//!
//! - Phase 1 — a thread-safe, in-memory [`HealthStore`] that records
//!   per-`(provider, model)` latency, error rate (rolling 10-minute window),
//!   cost, and rate-limit events.
//! - Phase 2 — a [`RoutingPolicy`] trait plus a default [`WeightedRoutingPolicy`]
//!   that picks a `ModelRef` from a candidate pool using a configurable weighted
//!   score over latency / error-rate / cost / recency, with hard-filter
//!   preferences ([`AgentPreferences`]).
//! - Phase 3 — per-[`ModelRef`] [`CircuitState`] driven by error-rate thresholds,
//!   integrated into `HealthStore` record paths and respected by
//!   [`WeightedRoutingPolicy::select`]; plus a [`ProviderCatalog`] trait and
//!   [`ModelCatalogRegistry`] that provide scaffolding for periodic catalog
//!   polling (the scheduler itself lives in the gateway in a follow-up bead).
//!
//! Phase 2/3 are not yet wired into the gateway request path — that integration
//! is tracked separately.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::RwLock;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Length of the rolling latency window used to compute p95.
const LATENCY_WINDOW: usize = 100;

/// Lookback window for the error-rate metric.
const ERROR_RATE_WINDOW: Duration = Duration::from_secs(600); // 10 minutes

/// Stable identifier for a `(provider, model)` pair. Used as the `HealthStore` key.
///
/// `provider` is the operator-facing provider name (e.g. `"openai"`, `"anthropic"`).
/// `model` is the upstream model identifier (e.g. `"gpt-4o-mini"`, `"claude-3-5-sonnet"`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModelRef {
    pub provider: String,
    pub model: String,
}

impl ModelRef {
    pub fn new(provider: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
        }
    }
}

/// Circuit-breaker state for a single [`ModelRef`]. See §8 of the design doc.
///
/// Transitions are driven by [`HealthStore::record_success`] / [`HealthStore::record_error`]
/// against a [`CircuitConfig`]:
///
/// - `Closed` → `Open` once `err_rate_10m > err_rate_threshold` AND
///   `total_requests > min_samples_for_open`.
/// - `Open` → `HalfOpen` once `cooldown` has elapsed since `circuit_opened_at`.
///   A [`HealthStore::snapshot`] or a [`RoutingPolicy::select`] call is all it
///   takes to observe the transition — the store performs the clock check
///   lazily on read, so no scheduler is needed.
/// - `HalfOpen` → `Closed` on the first recorded success (probe succeeded).
/// - `HalfOpen` → `Open` on the first recorded error (probe failed, cooldown
///   restarts from the fresh `circuit_opened_at`).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CircuitState {
    /// Happy path — candidate is selectable normally.
    #[default]
    Closed,
    /// Cooldown has elapsed; a single probe is allowed through to learn the
    /// current upstream state.
    HalfOpen,
    /// Candidate is filtered out of selection until cooldown elapses.
    Open,
}

/// Tunables for the circuit-breaker state machine. See [`CircuitConfig::defaults`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CircuitConfig {
    /// Fraction in `(0.0, 1.0]`. `Closed` opens once observed `err_rate_10m`
    /// strictly exceeds this value.
    pub err_rate_threshold: f32,
    /// Minimum lifetime `total_requests` required before the breaker will open.
    /// Prevents tripping on a first-call error.
    pub min_samples_for_open: u64,
    /// How long the breaker stays `Open` before transitioning to `HalfOpen`.
    pub cooldown: Duration,
    /// Advisory — fraction of traffic a future scheduler may route to `HalfOpen`
    /// candidates as probes. This module does not enforce a QPS cap; callers
    /// that want a strict limit should layer their own rate limiter on top.
    pub half_open_probe_ratio: f64,
}

impl CircuitConfig {
    /// Spec defaults (`DYNAMIC-MODEL-ROUTING.md` §10 + phase-3 notes):
    /// err-rate 0.5, 10 samples, 60 s cooldown, 10 % probe ratio.
    pub const fn defaults() -> Self {
        Self {
            err_rate_threshold: 0.5,
            min_samples_for_open: 10,
            cooldown: Duration::from_secs(60),
            half_open_probe_ratio: 0.10,
        }
    }
}

impl Default for CircuitConfig {
    fn default() -> Self {
        Self::defaults()
    }
}

/// Public snapshot of a model's observed health. Returned by `HealthStore::snapshot`.
///
/// All fields are point-in-time computed values, safe to clone and ship to metrics.
#[derive(Debug, Clone)]
pub struct ModelHealth {
    /// Rolling p95 latency over the last `LATENCY_WINDOW` observations, in ms.
    /// `0` when no samples have been recorded.
    pub p95_latency_ms: u32,
    /// Error rate over the last 10 minutes, in `[0.0, 1.0]`.
    pub err_rate_10m: f32,
    /// Seeded from config on first successful record; refined on subsequent records.
    pub cost_per_1k_tokens: f64,
    /// Timestamp of the most recent successful call, if any.
    pub last_ok_at: Option<Instant>,
    /// Timestamp of the most recent 429 (rate-limit) error, if any.
    pub last_429_at: Option<Instant>,
    /// Total terminal outcomes recorded (success + error).
    pub total_requests: u64,
    /// Total errors recorded (subset of `total_requests`).
    pub total_errors: u64,
    /// Total input tokens observed across all successful calls.
    pub total_tokens_in: u64,
    /// Total output tokens observed across all successful calls.
    pub total_tokens_out: u64,
    /// Accumulated cost in USD, computed as `tokens × cost_per_1k / 1000.0` per call.
    pub total_cost_usd: f64,
    /// Current circuit-breaker state. Computed lazily on read — the cooldown
    /// `Open → HalfOpen` transition is evaluated before the snapshot is built.
    pub circuit: CircuitState,
    /// When the breaker most recently transitioned into `Open`. `None` while
    /// `Closed`. Used to compute the cooldown check.
    pub circuit_opened_at: Option<Instant>,
}

/// Interior mutable state for one model. Not exposed publicly — callers see `ModelHealth`.
#[derive(Debug)]
struct HealthEntry {
    latencies: VecDeque<Duration>,
    /// `(timestamp, is_error)` events used for the 10-minute error-rate window.
    events: VecDeque<(Instant, bool)>,
    cost_per_1k_tokens: f64,
    last_ok_at: Option<Instant>,
    last_429_at: Option<Instant>,
    total_requests: u64,
    total_errors: u64,
    total_tokens_in: u64,
    total_tokens_out: u64,
    total_cost_usd: f64,
    circuit: CircuitState,
    circuit_opened_at: Option<Instant>,
}

impl HealthEntry {
    fn new() -> Self {
        Self {
            latencies: VecDeque::with_capacity(LATENCY_WINDOW),
            events: VecDeque::new(),
            cost_per_1k_tokens: 0.0,
            last_ok_at: None,
            last_429_at: None,
            total_requests: 0,
            total_errors: 0,
            total_tokens_in: 0,
            total_tokens_out: 0,
            total_cost_usd: 0.0,
            circuit: CircuitState::Closed,
            circuit_opened_at: None,
        }
    }

    /// Drop events older than the 10-minute error-rate window.
    fn evict_old_events(&mut self, now: Instant) {
        while let Some(&(t, _)) = self.events.front() {
            if now.duration_since(t) > ERROR_RATE_WINDOW {
                self.events.pop_front();
            } else {
                break;
            }
        }
    }

    fn push_latency(&mut self, latency: Duration) {
        if self.latencies.len() == LATENCY_WINDOW {
            self.latencies.pop_front();
        }
        self.latencies.push_back(latency);
    }

    fn p95_latency_ms(&self) -> u32 {
        if self.latencies.is_empty() {
            return 0;
        }
        let mut samples: Vec<u128> = self.latencies.iter().map(|d| d.as_millis()).collect();
        samples.sort_unstable();
        // p95 index: ceil(0.95 * n) - 1, clamped to [0, n-1].
        let n = samples.len();
        let idx = ((n as f64 * 0.95).ceil() as usize).saturating_sub(1).min(n - 1);
        samples[idx].min(u32::MAX as u128) as u32
    }

    fn err_rate_10m(&self) -> f32 {
        if self.events.is_empty() {
            return 0.0;
        }
        let errors = self.events.iter().filter(|(_, is_err)| *is_err).count();
        errors as f32 / self.events.len() as f32
    }

    fn to_health(&self) -> ModelHealth {
        ModelHealth {
            p95_latency_ms: self.p95_latency_ms(),
            err_rate_10m: self.err_rate_10m(),
            cost_per_1k_tokens: self.cost_per_1k_tokens,
            last_ok_at: self.last_ok_at,
            last_429_at: self.last_429_at,
            total_requests: self.total_requests,
            total_errors: self.total_errors,
            total_tokens_in: self.total_tokens_in,
            total_tokens_out: self.total_tokens_out,
            total_cost_usd: self.total_cost_usd,
            circuit: self.circuit,
            circuit_opened_at: self.circuit_opened_at,
        }
    }

    /// Lazy `Open → HalfOpen` transition when `cooldown` has elapsed since
    /// `circuit_opened_at`. Called at the top of every record/snapshot path so
    /// callers always observe a consistent view without a background ticker.
    fn maybe_half_open(&mut self, cfg: &CircuitConfig, now: Instant) {
        if self.circuit == CircuitState::Open
            && let Some(opened_at) = self.circuit_opened_at
            && now.saturating_duration_since(opened_at) >= cfg.cooldown
        {
            self.circuit = CircuitState::HalfOpen;
        }
    }

    /// Apply a terminal outcome to the circuit state machine. Called after the
    /// event has been appended and counters have been updated.
    fn apply_circuit_transition(&mut self, is_error: bool, cfg: &CircuitConfig, now: Instant) {
        match self.circuit {
            CircuitState::Closed => {
                if is_error
                    && self.total_requests > cfg.min_samples_for_open
                    && self.err_rate_10m() > cfg.err_rate_threshold
                {
                    self.circuit = CircuitState::Open;
                    self.circuit_opened_at = Some(now);
                }
            }
            CircuitState::HalfOpen => {
                if is_error {
                    // Probe failed — reopen, reset cooldown.
                    self.circuit = CircuitState::Open;
                    self.circuit_opened_at = Some(now);
                } else {
                    // Probe succeeded — close.
                    self.circuit = CircuitState::Closed;
                    self.circuit_opened_at = None;
                }
            }
            CircuitState::Open => {
                // No transition triggered by a record; cooldown is handled
                // by `maybe_half_open`. In practice `Open` candidates shouldn't
                // be receiving traffic, but if they do we stay `Open` for
                // errors and flip to `HalfOpen → Closed` for successes.
                if !is_error {
                    self.circuit = CircuitState::Closed;
                    self.circuit_opened_at = None;
                }
            }
        }
    }
}

/// In-memory, thread-safe health store for model routing observations.
///
/// All mutating methods take `&self` — interior mutability is provided by a
/// single `RwLock<HashMap<ModelRef, HealthEntry>>`. Contention is expected to
/// be low (one write per LLM call; reads are background metrics scrapes), so a
/// plain `RwLock` is preferred over `DashMap` at this stage.
pub struct HealthStore {
    inner: RwLock<HashMap<ModelRef, HealthEntry>>,
    circuit_cfg: CircuitConfig,
}

impl HealthStore {
    pub fn new() -> Self {
        Self::with_circuit_config(CircuitConfig::defaults())
    }

    /// Construct a `HealthStore` with non-default circuit-breaker tunables.
    /// Useful for tests that want a tiny `min_samples_for_open` or a short
    /// cooldown.
    pub fn with_circuit_config(circuit_cfg: CircuitConfig) -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
            circuit_cfg,
        }
    }

    /// Inspect the active circuit-breaker configuration.
    pub fn circuit_config(&self) -> &CircuitConfig {
        &self.circuit_cfg
    }

    /// Record a successful LLM call.
    ///
    /// - `latency` is the end-to-end call duration.
    /// - `tokens_in` / `tokens_out` are the reported usage values.
    /// - `cost_per_1k` seeds or refreshes the per-model cost-per-1k-tokens figure;
    ///   the contribution to `total_cost_usd` is `(tokens_in + tokens_out) × cost_per_1k / 1000`.
    pub fn record_success(
        &self,
        model: &ModelRef,
        latency: Duration,
        tokens_in: u32,
        tokens_out: u32,
        cost_per_1k: f64,
    ) {
        self.record_success_at(model, latency, tokens_in, tokens_out, cost_per_1k, Instant::now());
    }

    /// Record a failed LLM call. Pass `is_429 = true` for rate-limit errors so
    /// the rate-limit cooldown logic (phase 2) can pick them up.
    pub fn record_error(&self, model: &ModelRef, is_429: bool) {
        self.record_error_at(model, is_429, Instant::now());
    }

    /// Internal variant with explicit `now` — used by tests to inject timestamps.
    pub(crate) fn record_success_at(
        &self,
        model: &ModelRef,
        latency: Duration,
        tokens_in: u32,
        tokens_out: u32,
        cost_per_1k: f64,
        now: Instant,
    ) {
        let mut guard = self.inner.write().expect("HealthStore lock poisoned");
        let entry = guard
            .entry(model.clone())
            .or_insert_with(HealthEntry::new);
        entry.evict_old_events(now);
        entry.maybe_half_open(&self.circuit_cfg, now);
        entry.push_latency(latency);
        entry.events.push_back((now, false));
        entry.cost_per_1k_tokens = cost_per_1k;
        entry.last_ok_at = Some(now);
        entry.total_requests += 1;
        entry.total_tokens_in += u64::from(tokens_in);
        entry.total_tokens_out += u64::from(tokens_out);
        let tokens = f64::from(tokens_in) + f64::from(tokens_out);
        entry.total_cost_usd += tokens * cost_per_1k / 1000.0;
        entry.apply_circuit_transition(false, &self.circuit_cfg, now);
    }

    pub(crate) fn record_error_at(&self, model: &ModelRef, is_429: bool, now: Instant) {
        let mut guard = self.inner.write().expect("HealthStore lock poisoned");
        let entry = guard
            .entry(model.clone())
            .or_insert_with(HealthEntry::new);
        entry.evict_old_events(now);
        entry.maybe_half_open(&self.circuit_cfg, now);
        entry.events.push_back((now, true));
        entry.total_requests += 1;
        entry.total_errors += 1;
        if is_429 {
            entry.last_429_at = Some(now);
        }
        entry.apply_circuit_transition(true, &self.circuit_cfg, now);
    }

    /// Return a point-in-time snapshot of the given model's health, if observed.
    pub fn snapshot(&self, model: &ModelRef) -> Option<ModelHealth> {
        self.snapshot_at(model, Instant::now())
    }

    /// Test-visible variant of [`HealthStore::snapshot`] that accepts an
    /// explicit clock. The cooldown `Open → HalfOpen` transition is evaluated
    /// against `now` so tests can drive the state machine deterministically.
    pub(crate) fn snapshot_at(&self, model: &ModelRef, now: Instant) -> Option<ModelHealth> {
        let mut guard = self.inner.write().expect("HealthStore lock poisoned");
        if let Some(entry) = guard.get_mut(model) {
            entry.evict_old_events(now);
            entry.maybe_half_open(&self.circuit_cfg, now);
            Some(entry.to_health())
        } else {
            None
        }
    }

    /// Return snapshots for every observed model. Intended for metrics exposure.
    pub fn all(&self) -> Vec<(ModelRef, ModelHealth)> {
        let mut guard = self.inner.write().expect("HealthStore lock poisoned");
        let now = Instant::now();
        let circuit_cfg = self.circuit_cfg;
        guard
            .iter_mut()
            .map(|(k, v)| {
                v.evict_old_events(now);
                v.maybe_half_open(&circuit_cfg, now);
                (k.clone(), v.to_health())
            })
            .collect()
    }
}

impl Default for HealthStore {
    fn default() -> Self {
        Self::new()
    }
}

// Quick compile-time assertion that HealthStore is Send + Sync.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<HealthStore>();
};

// ---------------------------------------------------------------------------
// Phase 2 — RoutingPolicy trait + WeightedRoutingPolicy
// ---------------------------------------------------------------------------

/// Errors produced by policy construction and selection.
#[derive(Debug, Error, PartialEq)]
pub enum RoutingError {
    /// Weighted-score weights did not sum to 1.0 within the allowed epsilon.
    #[error("weighted-score weights must sum to 1.0 (got {sum}, epsilon {epsilon})")]
    InvalidWeights { sum: f64, epsilon: f64 },
    /// A weight was outside the `[0.0, 1.0]` range or non-finite.
    #[error("weight {name} out of range or non-finite: {value}")]
    WeightOutOfRange { name: &'static str, value: f64 },
}

/// Agent-level routing preferences. Hard filters applied before scoring.
///
/// Marked `#[non_exhaustive]` so we can grow the preference set in future
/// phases (e.g. `require_tool_calling`, `tags`) without a breaking change.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AgentPreferences {
    /// Drop candidates whose `cost_per_1k_tokens` exceeds this cap.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cost_per_1k: Option<f64>,
    /// Drop candidates whose `p95_latency_ms` exceeds this cap.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_latency_ms_p95: Option<u64>,
    /// Ordered list of provider names to prefer when scores tie. Earlier wins.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prefer_providers: Vec<String>,
}

impl AgentPreferences {
    /// Convenience constructor for empty preferences.
    pub fn none() -> Self {
        Self::default()
    }
}

/// Configurable weights for [`WeightedRoutingPolicy`].
///
/// Weights must be non-negative, finite, and sum to 1.0 within [`WeightedScoreConfig::EPSILON`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WeightedScoreConfig {
    pub w_latency: f64,
    pub w_err: f64,
    pub w_cost: f64,
    pub w_recency: f64,
}

impl WeightedScoreConfig {
    /// Tolerance used when validating that the four weights sum to 1.0.
    pub const EPSILON: f64 = 1e-6;

    /// Default weights from `DYNAMIC-MODEL-ROUTING.md` §10
    /// (latency 0.5, err 0.35, cost 0.10, recency 0.05).
    pub const fn defaults() -> Self {
        Self {
            w_latency: 0.5,
            w_err: 0.35,
            w_cost: 0.10,
            w_recency: 0.05,
        }
    }

    /// Validate that weights are finite, non-negative, and sum to 1.0 within
    /// [`WeightedScoreConfig::EPSILON`].
    pub fn validate(&self) -> Result<(), RoutingError> {
        let weights = [
            ("w_latency", self.w_latency),
            ("w_err", self.w_err),
            ("w_cost", self.w_cost),
            ("w_recency", self.w_recency),
        ];
        for (name, value) in weights {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(RoutingError::WeightOutOfRange { name, value });
            }
        }
        let sum = self.w_latency + self.w_err + self.w_cost + self.w_recency;
        if (sum - 1.0).abs() > Self::EPSILON {
            return Err(RoutingError::InvalidWeights {
                sum,
                epsilon: Self::EPSILON,
            });
        }
        Ok(())
    }
}

impl Default for WeightedScoreConfig {
    fn default() -> Self {
        Self::defaults()
    }
}

/// Pluggable selection strategy. Object-safe (no generic methods).
///
/// The caller supplies the candidate pool (derived from provider config), the
/// current [`HealthStore`] to look up observed metrics, and [`AgentPreferences`]
/// for hard filters. Implementations return a single `ModelRef` (the winning
/// pick) or `None` if no candidate satisfies the filters.
pub trait RoutingPolicy: Send + Sync {
    fn select(
        &self,
        candidates: &[ModelRef],
        store: &HealthStore,
        prefs: &AgentPreferences,
    ) -> Option<ModelRef>;

    /// Stable identifier for metrics / logs.
    fn name(&self) -> &'static str;
}

/// Weighted-score selection policy. Default implementation of [`RoutingPolicy`].
///
/// Algorithm (see `DYNAMIC-MODEL-ROUTING.md` §6):
///
/// 1. Apply hard filters from [`AgentPreferences`] (`max_cost_per_1k`,
///    `max_latency_ms_p95`). Candidates without observed health in the store
///    are kept (baseline assumption, see below).
/// 2. For each surviving candidate compute four sub-scores in `[0.0, 1.0]`:
///    - `latency_score`: min-max normalise `p95_latency_ms` across the surviving
///      set, then invert (`1.0 - normalised`) so lower latency → higher score.
///    - `err_score`: min-max normalise `err_rate_10m`, invert.
///    - `cost_score`: min-max normalise `cost_per_1k_tokens`, invert.
///    - `recency_score`: `exp(-age_seconds / 3600.0)` based on `last_ok_at`;
///      `0.0` if never recorded.
/// 3. Combine: `w_latency*latency + w_err*err + w_cost*cost + w_recency*recency`.
///    Higher combined score wins.
/// 4. Tie-break order:
///    (a) earlier entry in `prefer_providers`,
///    (b) lexicographic `(provider, model)`.
///
/// **Missing in store.** Candidates that have never been observed are assigned
/// baseline neutral health (`p95=0`, `err_rate=0`, `cost=0`, `last_ok_at=None`).
/// They therefore score well on latency/err/cost but get `recency_score = 0`.
/// This is the explicit "unknown, low confidence" treatment: they remain
/// selectable — important for cold-start and newly-added models — but the
/// recency term (if weighted) discounts them relative to confirmed-warm peers.
#[derive(Debug, Clone)]
pub struct WeightedRoutingPolicy {
    config: WeightedScoreConfig,
}

impl WeightedRoutingPolicy {
    /// Construct and validate the policy's weights.
    pub fn new(config: WeightedScoreConfig) -> Result<Self, RoutingError> {
        config.validate()?;
        Ok(Self { config })
    }

    /// Construct with [`WeightedScoreConfig::defaults`].
    pub fn with_defaults() -> Self {
        // Safe: defaults are static and known-valid.
        Self::new(WeightedScoreConfig::defaults()).expect("default weights are valid")
    }

    /// The configured weights.
    pub fn config(&self) -> &WeightedScoreConfig {
        &self.config
    }

    fn recency_score(last_ok_at: Option<Instant>, now: Instant) -> f64 {
        match last_ok_at {
            Some(t) => {
                let age_seconds = now.saturating_duration_since(t).as_secs_f64();
                (-age_seconds / 3600.0).exp()
            }
            None => 0.0,
        }
    }
}

/// Internal per-candidate record used during scoring. Raw metrics from the
/// store plus the candidate reference.
struct CandidateMetrics {
    model: ModelRef,
    p95_latency_ms: f64,
    err_rate_10m: f64,
    cost_per_1k_tokens: f64,
    last_ok_at: Option<Instant>,
    /// Phase-3 circuit state at selection time. `HalfOpen` is allowed through
    /// as a probe; `Open` candidates are filtered before this record is built.
    /// Not currently read during scoring — retained so future phases can bias
    /// scores for `HalfOpen` candidates (e.g. probe-rate dampening).
    #[allow(dead_code)]
    circuit: CircuitState,
}

/// Min-max normalise `v` into `[0.0, 1.0]` given the set's `(min, max)`.
///
/// If `max == min` all members are identical — return `0.0` so the inverted
/// score becomes `1.0` (everyone is equally good on this axis and the term
/// drops out of the ranking).
fn min_max(v: f64, min: f64, max: f64) -> f64 {
    if (max - min).abs() < f64::EPSILON {
        0.0
    } else {
        ((v - min) / (max - min)).clamp(0.0, 1.0)
    }
}

impl RoutingPolicy for WeightedRoutingPolicy {
    fn select(
        &self,
        candidates: &[ModelRef],
        store: &HealthStore,
        prefs: &AgentPreferences,
    ) -> Option<ModelRef> {
        if candidates.is_empty() {
            return None;
        }

        // 1. Build metric records, applying hard filters. Unknown models use
        //    baseline neutral metrics and are not filtered by cost/latency
        //    caps (we have no evidence against them). `Open` circuits are
        //    dropped; `HalfOpen` candidates are retained — selection is how
        //    the probe gets sent.
        let now = Instant::now();
        let mut pool: Vec<CandidateMetrics> = Vec::with_capacity(candidates.len());
        for c in candidates {
            let (p95, err_rate, cost, last_ok, circuit) = match store.snapshot_at(c, now) {
                Some(h) => (
                    f64::from(h.p95_latency_ms),
                    f64::from(h.err_rate_10m),
                    h.cost_per_1k_tokens,
                    h.last_ok_at,
                    h.circuit,
                ),
                None => (0.0, 0.0, 0.0, None, CircuitState::Closed),
            };

            if circuit == CircuitState::Open {
                continue;
            }
            if let Some(cap) = prefs.max_cost_per_1k
                && cost > cap
            {
                continue;
            }
            if let Some(cap) = prefs.max_latency_ms_p95
                && p95 > cap as f64
            {
                continue;
            }

            pool.push(CandidateMetrics {
                model: c.clone(),
                p95_latency_ms: p95,
                err_rate_10m: err_rate,
                cost_per_1k_tokens: cost,
                last_ok_at: last_ok,
                circuit,
            });
        }

        if pool.is_empty() {
            return None;
        }

        // 2. Compute min/max across the surviving pool for each axis.
        let (mut lat_min, mut lat_max) = (f64::INFINITY, f64::NEG_INFINITY);
        let (mut err_min, mut err_max) = (f64::INFINITY, f64::NEG_INFINITY);
        let (mut cost_min, mut cost_max) = (f64::INFINITY, f64::NEG_INFINITY);
        for m in &pool {
            lat_min = lat_min.min(m.p95_latency_ms);
            lat_max = lat_max.max(m.p95_latency_ms);
            err_min = err_min.min(m.err_rate_10m);
            err_max = err_max.max(m.err_rate_10m);
            cost_min = cost_min.min(m.cost_per_1k_tokens);
            cost_max = cost_max.max(m.cost_per_1k_tokens);
        }

        // 3. Score each candidate. Higher = better.
        let cfg = &self.config;
        let mut scored: Vec<(f64, &CandidateMetrics)> = pool
            .iter()
            .map(|m| {
                let latency_score = 1.0 - min_max(m.p95_latency_ms, lat_min, lat_max);
                let err_score = 1.0 - min_max(m.err_rate_10m, err_min, err_max);
                let cost_score = 1.0 - min_max(m.cost_per_1k_tokens, cost_min, cost_max);
                let recency_score = Self::recency_score(m.last_ok_at, now);
                let combined = cfg.w_latency * latency_score
                    + cfg.w_err * err_score
                    + cfg.w_cost * cost_score
                    + cfg.w_recency * recency_score;
                (combined, m)
            })
            .collect();

        // 4. Rank: higher score wins; tie-break by prefer_providers then lex.
        let prefer_rank = |provider: &str| -> usize {
            prefs
                .prefer_providers
                .iter()
                .position(|p| p == provider)
                .unwrap_or(usize::MAX)
        };

        scored.sort_by(|(sa, ca), (sb, cb)| {
            sb.partial_cmp(sa)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| prefer_rank(&ca.model.provider).cmp(&prefer_rank(&cb.model.provider)))
                .then_with(|| ca.model.provider.cmp(&cb.model.provider))
                .then_with(|| ca.model.model.cmp(&cb.model.model))
        });

        scored.first().map(|(_, m)| m.model.clone())
    }

    fn name(&self) -> &'static str {
        "weighted"
    }
}

// Compile-time check: RoutingPolicy must be object-safe.
const _: fn() = || {
    fn assert_object_safe(_: &dyn RoutingPolicy) {}
    let p = WeightedRoutingPolicy::with_defaults();
    assert_object_safe(&p);
};

// ---------------------------------------------------------------------------
// Phase 3 — ProviderCatalog + ModelCatalogRegistry (auto-discovery scaffold)
// ---------------------------------------------------------------------------

/// Lightweight metadata describing a model offered by a provider. This is the
/// output of an auto-discovery poll (`/v1/models` or equivalent) before the
/// router has any runtime health observations.
///
/// See `DYNAMIC-MODEL-ROUTING.md` §7.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModelInfo {
    /// Operator-facing provider name (e.g. `"openai"`). Must agree with
    /// [`ModelRef::provider`] so catalog entries can be joined against
    /// [`HealthStore`] records.
    pub provider: String,
    /// Upstream model identifier (e.g. `"gpt-4o-mini"`).
    pub model: String,
    /// Optional human-readable display label. Not used for routing decisions —
    /// present so UI surfaces can avoid a second lookup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

impl ModelInfo {
    pub fn new(provider: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
            display_name: None,
        }
    }

    /// Project to a [`ModelRef`] for joining with [`HealthStore`] keys.
    pub fn to_model_ref(&self) -> ModelRef {
        ModelRef::new(self.provider.clone(), self.model.clone())
    }
}

/// Errors produced by catalog polling.
#[derive(Debug, Error, PartialEq)]
pub enum CatalogError {
    /// Upstream returned an unexpected response. Kept as `String` — callers
    /// that need richer context should wrap this with their own enum.
    #[error("upstream catalog poll failed: {0}")]
    Upstream(String),
    /// Provider returned a response we could not parse.
    #[error("invalid catalog payload: {0}")]
    Invalid(String),
}

/// A provider that can enumerate its currently-offered models.
///
/// Object-safe (no generic methods). Implementations are typically HTTP-backed
/// (`/v1/models`), but the trait is deliberately transport-agnostic so tests
/// can substitute [`StaticProviderCatalog`].
#[async_trait]
pub trait ProviderCatalog: Send + Sync {
    /// Human-readable provider name. Should match the `provider` field of the
    /// returned [`ModelInfo`]s.
    fn provider_name(&self) -> &str;

    /// Enumerate the currently-offered models. Called by
    /// [`ModelCatalogRegistry::refresh`].
    async fn list_models(&self) -> Result<Vec<ModelInfo>, CatalogError>;
}

/// In-memory [`ProviderCatalog`] stub with a fixed `Vec<ModelInfo>`. Used by
/// tests and by operators who want to pin the catalog via config without a
/// live upstream poll.
pub struct StaticProviderCatalog {
    provider_name: String,
    models: Vec<ModelInfo>,
}

impl StaticProviderCatalog {
    pub fn new(provider_name: impl Into<String>, models: Vec<ModelInfo>) -> Self {
        Self {
            provider_name: provider_name.into(),
            models,
        }
    }
}

#[async_trait]
impl ProviderCatalog for StaticProviderCatalog {
    fn provider_name(&self) -> &str {
        &self.provider_name
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, CatalogError> {
        Ok(self.models.clone())
    }
}

/// Tunables for the (future) catalog refresh scheduler.
///
/// The scheduler itself (a `tokio::spawn` loop that calls
/// [`ModelCatalogRegistry::refresh`] every `refresh_interval`) is **out of
/// scope for phase 3**. It lives in the gateway and will be wired in a
/// follow-up bead — see `DYNAMIC-MODEL-ROUTING.md` §7 and the scheduler hook
/// note at the bottom of this module. `refresh_interval` is captured here so
/// callers can plumb config through without yet binding to a runtime.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CatalogRefreshConfig {
    pub enabled: bool,
    #[serde(with = "duration_secs")]
    pub refresh_interval: Duration,
    #[serde(with = "duration_secs")]
    pub stale_retention: Duration,
}

impl CatalogRefreshConfig {
    /// Spec defaults (`DYNAMIC-MODEL-ROUTING.md` §10 — `discovery.interval_secs`
    /// / `stale_retention_secs`): 600 s refresh, 3600 s stale retention.
    pub const fn defaults() -> Self {
        Self {
            enabled: true,
            refresh_interval: Duration::from_secs(600),
            stale_retention: Duration::from_secs(3600),
        }
    }
}

impl Default for CatalogRefreshConfig {
    fn default() -> Self {
        Self::defaults()
    }
}

mod duration_secs {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(d.as_secs())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        let secs = u64::deserialize(d)?;
        Ok(Duration::from_secs(secs))
    }
}

/// Aggregates multiple [`ProviderCatalog`] implementations and caches their
/// union. The registry owns its providers and its cache; `refresh()` is the
/// only entry point that reaches upstream.
///
/// The cache is populated on [`ModelCatalogRegistry::refresh`] — before that
/// call, [`ModelCatalogRegistry::all`] returns an empty slice. This is
/// deliberate: the registry has no opinion about when to refresh, leaving the
/// scheduling choice to the caller (gateway `tokio::spawn` task).
pub struct ModelCatalogRegistry {
    providers: Vec<Box<dyn ProviderCatalog>>,
    cache: RwLock<Vec<ModelInfo>>,
    refresh_cfg: CatalogRefreshConfig,
}

impl ModelCatalogRegistry {
    pub fn new(providers: Vec<Box<dyn ProviderCatalog>>) -> Self {
        Self::with_config(providers, CatalogRefreshConfig::defaults())
    }

    pub fn with_config(
        providers: Vec<Box<dyn ProviderCatalog>>,
        refresh_cfg: CatalogRefreshConfig,
    ) -> Self {
        Self {
            providers,
            cache: RwLock::new(Vec::new()),
            refresh_cfg,
        }
    }

    /// Tunables a future scheduler will consult to decide how often to call
    /// [`ModelCatalogRegistry::refresh`].
    pub fn refresh_config(&self) -> &CatalogRefreshConfig {
        &self.refresh_cfg
    }

    /// Poll every registered provider and merge the results into the cache,
    /// de-duplicating by `(provider, model)`. On a per-provider error the
    /// registry keeps the previously-cached entries for that provider —
    /// transient catalog failures must not purge routable models.
    ///
    /// Returns the list of errors encountered (empty on full success). The
    /// cache is updated regardless of partial failures.
    pub async fn refresh(&self) -> Vec<(String, CatalogError)> {
        let mut errors = Vec::new();
        // Snapshot the current cache so we can preserve entries for providers
        // whose poll fails. Keyed by provider name.
        let prior: Vec<ModelInfo> = {
            let guard = self.cache.read().expect("catalog cache poisoned");
            guard.clone()
        };

        let mut fresh: HashMap<String, Vec<ModelInfo>> = HashMap::new();
        for provider in &self.providers {
            let name = provider.provider_name().to_string();
            match provider.list_models().await {
                Ok(models) => {
                    fresh.insert(name, models);
                }
                Err(e) => {
                    errors.push((name.clone(), e));
                    // Fall through — we'll re-use prior entries for this provider.
                }
            }
        }

        // Rebuild cache: fresh provider data wins; providers that failed keep
        // their previous entries. De-duplicate across all providers.
        let mut seen: HashSet<(String, String)> = HashSet::new();
        let mut merged: Vec<ModelInfo> = Vec::new();
        for provider in &self.providers {
            let name = provider.provider_name();
            let entries: &[ModelInfo] = if let Some(f) = fresh.get(name) {
                f.as_slice()
            } else {
                // Provider failed — keep previously-cached entries for it.
                &prior[..]
            };
            for info in entries {
                if !fresh.contains_key(name) && info.provider != name {
                    // Prior-entry filter: only retain prior entries whose
                    // provider field matches the failing provider.
                    continue;
                }
                let key = (info.provider.clone(), info.model.clone());
                if seen.insert(key) {
                    merged.push(info.clone());
                }
            }
        }

        let mut guard = self.cache.write().expect("catalog cache poisoned");
        *guard = merged;
        errors
    }

    /// Return the cached union of all provider catalogs as [`ModelRef`]s,
    /// suitable for feeding into [`RoutingPolicy::select`]. Empty before the
    /// first successful [`ModelCatalogRegistry::refresh`].
    pub fn all(&self) -> Vec<ModelRef> {
        let guard = self.cache.read().expect("catalog cache poisoned");
        guard.iter().map(ModelInfo::to_model_ref).collect()
    }

    /// Cached [`ModelInfo`]s. Same freshness semantics as
    /// [`ModelCatalogRegistry::all`].
    pub fn cached(&self) -> Vec<ModelInfo> {
        let guard = self.cache.read().expect("catalog cache poisoned");
        guard.clone()
    }
}

// Compile-time check: ProviderCatalog must be object-safe so the registry can
// own a heterogenous Vec<Box<dyn ProviderCatalog>>.
const _: fn() = || {
    fn assert_object_safe(_: &dyn ProviderCatalog) {}
    let c = StaticProviderCatalog {
        provider_name: String::new(),
        models: Vec::new(),
    };
    assert_object_safe(&c);
};

#[cfg(test)]
mod tests {
    use super::*;

    fn mref(p: &str, m: &str) -> ModelRef {
        ModelRef::new(p, m)
    }

    #[test]
    fn empty_store_returns_none() {
        let store = HealthStore::new();
        assert!(store.snapshot(&mref("openai", "gpt-4o")).is_none());
        assert!(store.all().is_empty());
    }

    #[test]
    fn single_success_populates_all_fields() {
        let store = HealthStore::new();
        let key = mref("openai", "gpt-4o");
        store.record_success(&key, Duration::from_millis(123), 10, 20, 0.5);

        let snap = store.snapshot(&key).expect("should have snapshot");
        assert_eq!(snap.total_requests, 1);
        assert_eq!(snap.total_errors, 0);
        assert!((snap.err_rate_10m - 0.0).abs() < f32::EPSILON);
        assert_eq!(snap.p95_latency_ms, 123);
        assert_eq!(snap.total_tokens_in, 10);
        assert_eq!(snap.total_tokens_out, 20);
        assert!((snap.cost_per_1k_tokens - 0.5).abs() < f64::EPSILON);
        // 30 tokens * 0.5 / 1000 = 0.015
        assert!((snap.total_cost_usd - 0.015).abs() < 1e-9);
        assert!(snap.last_ok_at.is_some());
        assert!(snap.last_429_at.is_none());
    }

    #[test]
    fn p95_matches_naive_computation_over_100_samples() {
        let store = HealthStore::new();
        let key = mref("anthropic", "claude-3-5-sonnet");

        // Record 100 samples with latencies 1..=100 ms.
        for i in 1..=100u64 {
            store.record_success(&key, Duration::from_millis(i), 0, 0, 0.0);
        }

        // Naive p95 of sorted [1..=100] at idx=ceil(0.95*100)-1 = 94, which is the 95th value = 95.
        let snap = store.snapshot(&key).unwrap();
        assert_eq!(snap.p95_latency_ms, 95);
        assert_eq!(snap.total_requests, 100);
    }

    #[test]
    fn latency_window_caps_at_100_samples() {
        let store = HealthStore::new();
        let key = mref("openai", "gpt-4o-mini");

        // First 100 samples are 1000ms each; next 100 are 1ms. p95 should reflect only the recent 100.
        for _ in 0..100 {
            store.record_success(&key, Duration::from_millis(1000), 0, 0, 0.0);
        }
        for _ in 0..100 {
            store.record_success(&key, Duration::from_millis(1), 0, 0, 0.0);
        }
        let snap = store.snapshot(&key).unwrap();
        assert_eq!(snap.p95_latency_ms, 1);
        assert_eq!(snap.total_requests, 200);
    }

    #[test]
    fn error_window_evicts_old_entries() {
        let store = HealthStore::new();
        let key = mref("openai", "gpt-4o");

        // Inject an "old" error from 11 minutes ago and a fresh success now.
        let eleven_min_ago = Instant::now()
            .checked_sub(Duration::from_secs(660))
            .expect("instant arithmetic");
        store.record_error_at(&key, false, eleven_min_ago);

        // Before eviction-on-read: we still have one event.
        // After a fresh success, the old error should be evicted and err_rate should be 0.
        store.record_success(&key, Duration::from_millis(10), 1, 1, 0.0);

        let snap = store.snapshot(&key).unwrap();
        // err_rate_10m only counts the one success within the window.
        assert!((snap.err_rate_10m - 0.0).abs() < f32::EPSILON);
        // total_requests is lifetime-cumulative and keeps the old error.
        assert_eq!(snap.total_requests, 2);
        assert_eq!(snap.total_errors, 1);
    }

    #[test]
    fn record_error_429_sets_last_429_at() {
        let store = HealthStore::new();
        let key = mref("openai", "gpt-4o");
        store.record_error(&key, true);

        let snap = store.snapshot(&key).unwrap();
        assert!(snap.last_429_at.is_some());
        assert_eq!(snap.total_errors, 1);
        assert!((snap.err_rate_10m - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn record_error_non_429_leaves_last_429_at_unset() {
        let store = HealthStore::new();
        let key = mref("openai", "gpt-4o");
        store.record_error(&key, false);

        let snap = store.snapshot(&key).unwrap();
        assert!(snap.last_429_at.is_none());
        assert_eq!(snap.total_errors, 1);
    }

    #[test]
    fn cost_accumulates_across_multiple_calls() {
        let store = HealthStore::new();
        let key = mref("openai", "gpt-4o");

        // Call 1: 1000 tokens @ $0.50/1k  = $0.50
        store.record_success(&key, Duration::from_millis(10), 500, 500, 0.5);
        // Call 2: 2000 tokens @ $1.00/1k  = $2.00
        store.record_success(&key, Duration::from_millis(10), 1000, 1000, 1.0);
        // Call 3: 4000 tokens @ $0.25/1k  = $1.00
        store.record_success(&key, Duration::from_millis(10), 2000, 2000, 0.25);

        let snap = store.snapshot(&key).unwrap();
        assert_eq!(snap.total_tokens_in, 3500);
        assert_eq!(snap.total_tokens_out, 3500);
        assert!((snap.total_cost_usd - 3.5).abs() < 1e-9);
        // Last-write-wins for cost_per_1k_tokens.
        assert!((snap.cost_per_1k_tokens - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn all_returns_every_observed_model() {
        let store = HealthStore::new();
        let k1 = mref("openai", "gpt-4o");
        let k2 = mref("anthropic", "claude-3-5-sonnet");
        store.record_success(&k1, Duration::from_millis(50), 10, 10, 0.1);
        store.record_error(&k2, true);

        let mut all = store.all();
        all.sort_by(|a, b| a.0.provider.cmp(&b.0.provider));
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].0.provider, "anthropic");
        assert_eq!(all[1].0.provider, "openai");
        assert!(all[0].1.last_429_at.is_some());
        assert!(all[1].1.last_ok_at.is_some());
    }

    // -----------------------------------------------------------------------
    // Phase 2 — WeightedRoutingPolicy tests
    // -----------------------------------------------------------------------

    /// Helper: build a store pre-loaded with one observation per model.
    fn store_with(observations: &[(ModelRef, u64, f32, f64)]) -> HealthStore {
        let store = HealthStore::new();
        for (key, latency_ms, err_rate, cost_per_1k) in observations {
            // Record a success so last_ok_at is set and cost_per_1k is seeded.
            store.record_success(key, Duration::from_millis(*latency_ms), 0, 0, *cost_per_1k);
            // Inject an error to fake a non-zero error rate: record_error/success
            // alternation. For the tests we just need the resulting err_rate
            // to reflect the intent — we do this by padding with extra errors.
            if *err_rate > 0.0 {
                // Record enough errors that err_rate approx matches.
                // For a single success, err_rate = errs / (errs + 1). Solve:
                //   target = e / (e + 1)  =>  e = target / (1 - target).
                let e = (*err_rate / (1.0 - *err_rate)).round() as u64;
                for _ in 0..e {
                    store.record_error(key, false);
                }
            }
        }
        store
    }

    fn weights(lat: f64, err: f64, cost: f64, recency: f64) -> WeightedScoreConfig {
        WeightedScoreConfig {
            w_latency: lat,
            w_err: err,
            w_cost: cost,
            w_recency: recency,
        }
    }

    #[test]
    fn policy_empty_candidate_set_returns_none() {
        let policy = WeightedRoutingPolicy::with_defaults();
        let store = HealthStore::new();
        let pick = policy.select(&[], &store, &AgentPreferences::none());
        assert!(pick.is_none());
    }

    #[test]
    fn policy_all_healthy_equal_weights_ranks_obviously() {
        // Three candidates. Equal weights (0.25 each).
        // `good` has low latency + low err + low cost + recent success;
        // `mid` has medium everything; `bad` has high everything.
        // Expect `good` to win.
        let good = mref("openai", "good");
        let mid = mref("openai", "mid");
        let bad = mref("openai", "bad");
        let store = store_with(&[
            (good.clone(), 100, 0.0, 0.001),
            (mid.clone(), 500, 0.1, 0.005),
            (bad.clone(), 2000, 0.5, 0.050),
        ]);
        let policy = WeightedRoutingPolicy::new(weights(0.25, 0.25, 0.25, 0.25)).unwrap();
        let pick = policy.select(
            &[bad.clone(), mid.clone(), good.clone()],
            &store,
            &AgentPreferences::none(),
        );
        assert_eq!(pick.as_ref(), Some(&good));
    }

    #[test]
    fn policy_cost_threshold_filters_candidate() {
        let cheap = mref("openai", "cheap");
        let dear = mref("openai", "dear");
        // `dear` is much faster but over the cost cap — must be filtered.
        let store = store_with(&[
            (cheap.clone(), 1000, 0.0, 0.001),
            (dear.clone(), 50, 0.0, 0.100),
        ]);
        let policy = WeightedRoutingPolicy::with_defaults();
        let prefs = AgentPreferences {
            max_cost_per_1k: Some(0.010),
            ..Default::default()
        };
        let pick = policy.select(&[cheap.clone(), dear.clone()], &store, &prefs);
        assert_eq!(pick.as_ref(), Some(&cheap));
    }

    #[test]
    fn policy_all_violate_thresholds_returns_none() {
        let a = mref("openai", "a");
        let b = mref("anthropic", "b");
        let store = store_with(&[
            (a.clone(), 5_000, 0.0, 0.100),
            (b.clone(), 6_000, 0.0, 0.200),
        ]);
        let policy = WeightedRoutingPolicy::with_defaults();
        let prefs = AgentPreferences {
            max_cost_per_1k: Some(0.010),
            max_latency_ms_p95: Some(1_000),
            ..Default::default()
        };
        let pick = policy.select(&[a, b], &store, &prefs);
        assert!(pick.is_none());
    }

    #[test]
    fn policy_recency_dominates_when_only_weight_nonzero() {
        // `fresh` just got a success. `stale` never has — store has no
        // entry, so last_ok_at = None and recency_score = 0.
        // With w_recency = 1.0 (all other weights zero) `fresh` must win.
        let fresh = mref("openai", "fresh");
        let stale = mref("openai", "stale");
        let store = HealthStore::new();
        store.record_success(&fresh, Duration::from_millis(500), 0, 0, 0.002);
        // Do NOT record anything for `stale`.

        let policy = WeightedRoutingPolicy::new(weights(0.0, 0.0, 0.0, 1.0)).unwrap();
        let pick = policy.select(
            &[stale.clone(), fresh.clone()],
            &store,
            &AgentPreferences::none(),
        );
        assert_eq!(pick.as_ref(), Some(&fresh));
    }

    #[test]
    fn policy_latency_dominates_when_only_weight_nonzero() {
        let fast = mref("openai", "fast");
        let slow = mref("openai", "slow");
        let store = store_with(&[
            (fast.clone(), 100, 0.5, 0.100), // bad err + cost, great latency
            (slow.clone(), 5_000, 0.0, 0.001), // good err + cost, awful latency
        ]);

        let policy = WeightedRoutingPolicy::new(weights(1.0, 0.0, 0.0, 0.0)).unwrap();
        let pick = policy.select(
            &[slow.clone(), fast.clone()],
            &store,
            &AgentPreferences::none(),
        );
        assert_eq!(pick.as_ref(), Some(&fast));
    }

    #[test]
    fn policy_missing_in_store_is_selectable_with_baseline() {
        // Unobserved candidates use baseline neutral metrics. When paired with
        // an observed-but-worse candidate under equal weights, the unobserved
        // one should still score well on the normalised axes. The only hit is
        // `recency_score = 0` vs. observed peers, which at equal weights is
        // not enough to lose.
        let observed = mref("openai", "observed");
        let unknown = mref("anthropic", "unknown");
        let store = HealthStore::new();
        // Give observed awful metrics so baseline wins on the normalised axes.
        store.record_success(&observed, Duration::from_millis(5_000), 0, 0, 0.100);
        for _ in 0..10 {
            store.record_error(&observed, false);
        }

        // Equal weights.
        let policy = WeightedRoutingPolicy::new(weights(0.25, 0.25, 0.25, 0.25)).unwrap();
        let pick = policy.select(
            &[observed.clone(), unknown.clone()],
            &store,
            &AgentPreferences::none(),
        );
        assert_eq!(
            pick.as_ref(),
            Some(&unknown),
            "baseline metrics should beat a demonstrably-bad observed peer"
        );

        // But unknown must still be *selectable* on its own — not filtered
        // out just because it has no health record.
        let solo_pick = policy.select(
            std::slice::from_ref(&unknown),
            &store,
            &AgentPreferences::none(),
        );
        assert_eq!(solo_pick.as_ref(), Some(&unknown));
    }

    #[test]
    fn policy_tie_break_prefers_prefer_providers_order() {
        // Two candidates with identical baseline health (never observed).
        // Under defaults every score term resolves to the same value, so
        // the tie-break logic (prefer_providers then lex) is all that picks.
        let a = mref("openai", "m");
        let b = mref("anthropic", "m");
        let store = HealthStore::new();
        let policy = WeightedRoutingPolicy::with_defaults();

        let prefs = AgentPreferences {
            prefer_providers: vec!["anthropic".to_string(), "openai".to_string()],
            ..Default::default()
        };
        let pick = policy.select(&[a.clone(), b.clone()], &store, &prefs);
        assert_eq!(pick.as_ref(), Some(&b));

        // Swap preference: openai wins.
        let prefs2 = AgentPreferences {
            prefer_providers: vec!["openai".to_string(), "anthropic".to_string()],
            ..Default::default()
        };
        let pick2 = policy.select(&[a.clone(), b.clone()], &store, &prefs2);
        assert_eq!(pick2.as_ref(), Some(&a));

        // No preference: falls through to lexicographic (anthropic < openai).
        let pick3 = policy.select(
            &[a.clone(), b.clone()],
            &store,
            &AgentPreferences::none(),
        );
        assert_eq!(pick3.as_ref(), Some(&b));
    }

    #[test]
    fn weighted_config_validate_rejects_bad_sums() {
        // Too low.
        let low = weights(0.1, 0.1, 0.1, 0.1);
        match low.validate() {
            Err(RoutingError::InvalidWeights { .. }) => {}
            other => panic!("expected InvalidWeights, got {other:?}"),
        }

        // Too high.
        let high = weights(0.5, 0.5, 0.5, 0.5);
        match high.validate() {
            Err(RoutingError::InvalidWeights { .. }) => {}
            other => panic!("expected InvalidWeights, got {other:?}"),
        }

        // Out-of-range negative.
        let neg = weights(-0.1, 0.4, 0.4, 0.3);
        match neg.validate() {
            Err(RoutingError::WeightOutOfRange { name, .. }) => assert_eq!(name, "w_latency"),
            other => panic!("expected WeightOutOfRange, got {other:?}"),
        }

        // NaN.
        let nan = weights(f64::NAN, 0.4, 0.4, 0.2);
        assert!(matches!(
            nan.validate(),
            Err(RoutingError::WeightOutOfRange { .. })
        ));

        // Valid sum within epsilon.
        let ok = WeightedScoreConfig::defaults();
        ok.validate().expect("defaults must validate");

        // `new()` mirrors `validate()`.
        assert!(WeightedRoutingPolicy::new(low).is_err());
        assert!(WeightedRoutingPolicy::new(ok).is_ok());
    }

    #[test]
    fn policy_name_is_stable() {
        let p = WeightedRoutingPolicy::with_defaults();
        assert_eq!(p.name(), "weighted");
    }

    // -----------------------------------------------------------------------
    // Phase 3 — circuit breaker tests
    // -----------------------------------------------------------------------

    /// Tiny min-samples threshold so tests trip the breaker in a handful of
    /// calls, with a short cooldown for deterministic advancement.
    fn test_circuit_cfg() -> CircuitConfig {
        CircuitConfig {
            err_rate_threshold: 0.5,
            min_samples_for_open: 2,
            cooldown: Duration::from_secs(60),
            half_open_probe_ratio: 1.0,
        }
    }

    #[test]
    fn circuit_closed_to_open_on_err_rate_threshold() {
        let store = HealthStore::with_circuit_config(test_circuit_cfg());
        let key = mref("openai", "flaky");
        let now = Instant::now();

        // Starts Closed. First error alone must NOT open (min_samples_for_open=2).
        store.record_error_at(&key, false, now);
        assert_eq!(
            store.snapshot_at(&key, now).unwrap().circuit,
            CircuitState::Closed,
            "single error must not open the breaker below min_samples"
        );

        // Third error crosses the sample threshold AND keeps err_rate at 1.0 (> 0.5).
        store.record_error_at(&key, false, now);
        store.record_error_at(&key, false, now);
        let snap = store.snapshot_at(&key, now).unwrap();
        assert_eq!(snap.circuit, CircuitState::Open);
        assert!(snap.circuit_opened_at.is_some());
    }

    #[test]
    fn circuit_open_to_half_open_after_cooldown() {
        let cfg = test_circuit_cfg();
        let store = HealthStore::with_circuit_config(cfg);
        let key = mref("openai", "flaky");
        let t0 = Instant::now();

        for _ in 0..3 {
            store.record_error_at(&key, false, t0);
        }
        assert_eq!(
            store.snapshot_at(&key, t0).unwrap().circuit,
            CircuitState::Open
        );

        // Before cooldown elapses — still Open.
        let t_mid = t0 + Duration::from_secs(30);
        assert_eq!(
            store.snapshot_at(&key, t_mid).unwrap().circuit,
            CircuitState::Open
        );

        // After cooldown elapses — HalfOpen on read.
        let t_after = t0 + Duration::from_secs(61);
        assert_eq!(
            store.snapshot_at(&key, t_after).unwrap().circuit,
            CircuitState::HalfOpen
        );
    }

    #[test]
    fn circuit_half_open_success_closes() {
        let store = HealthStore::with_circuit_config(test_circuit_cfg());
        let key = mref("openai", "flaky");
        let t0 = Instant::now();

        for _ in 0..3 {
            store.record_error_at(&key, false, t0);
        }
        let t_probe = t0 + Duration::from_secs(61);
        // Reading at t_probe transitions Open→HalfOpen.
        assert_eq!(
            store.snapshot_at(&key, t_probe).unwrap().circuit,
            CircuitState::HalfOpen
        );

        // Probe succeeds → Closed.
        store.record_success_at(&key, Duration::from_millis(50), 1, 1, 0.001, t_probe);
        let snap = store.snapshot_at(&key, t_probe).unwrap();
        assert_eq!(snap.circuit, CircuitState::Closed);
        assert!(snap.circuit_opened_at.is_none());
    }

    #[test]
    fn circuit_half_open_failure_reopens_and_resets_cooldown() {
        let store = HealthStore::with_circuit_config(test_circuit_cfg());
        let key = mref("openai", "flaky");
        let t0 = Instant::now();

        for _ in 0..3 {
            store.record_error_at(&key, false, t0);
        }
        let first_opened_at = store
            .snapshot_at(&key, t0)
            .unwrap()
            .circuit_opened_at
            .unwrap();

        // Advance past cooldown, then reopen via a failing probe at a fresh timestamp.
        let t_probe = t0 + Duration::from_secs(61);
        store.record_error_at(&key, false, t_probe);

        let snap = store.snapshot_at(&key, t_probe).unwrap();
        assert_eq!(snap.circuit, CircuitState::Open);
        let reopened_at = snap.circuit_opened_at.unwrap();
        assert!(
            reopened_at > first_opened_at,
            "circuit_opened_at must be refreshed on HalfOpen → Open"
        );
    }

    #[test]
    fn policy_filters_open_circuit_candidates() {
        let store = HealthStore::with_circuit_config(test_circuit_cfg());
        let broken = mref("openai", "broken");
        let healthy = mref("openai", "healthy");
        let now = Instant::now();

        // Trip the broken circuit.
        for _ in 0..3 {
            store.record_error_at(&broken, false, now);
        }
        // Seed healthy with a success so it has metrics.
        store.record_success_at(&healthy, Duration::from_millis(100), 1, 1, 0.001, now);

        let policy = WeightedRoutingPolicy::with_defaults();
        let pick = policy.select(
            &[broken.clone(), healthy.clone()],
            &store,
            &AgentPreferences::none(),
        );
        assert_eq!(pick.as_ref(), Some(&healthy));

        // If the open candidate is the only option, select returns None.
        let pick_solo = policy.select(std::slice::from_ref(&broken), &store, &AgentPreferences::none());
        assert!(
            pick_solo.is_none(),
            "Open-only pool must not produce a selection"
        );
    }

    #[test]
    fn policy_allows_half_open_candidate_as_probe() {
        let store = HealthStore::with_circuit_config(test_circuit_cfg());
        let probing = mref("openai", "probing");
        let t0 = Instant::now();

        for _ in 0..3 {
            store.record_error_at(&probing, false, t0);
        }

        let policy = WeightedRoutingPolicy::with_defaults();
        // With the circuit Open the candidate is filtered.
        let pick_open = policy.select(std::slice::from_ref(&probing), &store, &AgentPreferences::none());
        assert!(pick_open.is_none());

        // After cooldown, snapshot_at (used internally by select via real-clock
        // snapshot) would surface HalfOpen. To drive deterministically we
        // fake-advance by mutating via a success_at past cooldown first — but
        // that would close the circuit. Instead, we directly assert HalfOpen
        // behaviour using snapshot_at + select's real-clock path: tick the
        // cooldown by letting snapshot_at flip to HalfOpen, then confirm the
        // policy *would* select it by checking the snapshot mirrors the
        // expected state for a subsequent real-clock select() call.
        let t_probe = t0 + Duration::from_secs(61);
        let snap = store.snapshot_at(&probing, t_probe).unwrap();
        assert_eq!(snap.circuit, CircuitState::HalfOpen);
        // Now a real-clock select() will see HalfOpen (because the lazy
        // transition persists in the entry) and must produce the candidate.
        let pick_half = policy.select(std::slice::from_ref(&probing), &store, &AgentPreferences::none());
        assert_eq!(
            pick_half.as_ref(),
            Some(&probing),
            "HalfOpen candidates must remain selectable as probes"
        );
    }

    // -----------------------------------------------------------------------
    // Phase 3 — catalog scaffold tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn static_catalog_roundtrips_models() {
        let models = vec![
            ModelInfo::new("openai", "gpt-4o"),
            ModelInfo::new("openai", "gpt-4o-mini"),
        ];
        let cat = StaticProviderCatalog::new("openai", models.clone());
        assert_eq!(cat.provider_name(), "openai");
        let out = cat.list_models().await.unwrap();
        assert_eq!(out, models);
    }

    #[tokio::test]
    async fn catalog_registry_merges_two_providers() {
        let openai = Box::new(StaticProviderCatalog::new(
            "openai",
            vec![
                ModelInfo::new("openai", "gpt-4o"),
                ModelInfo::new("openai", "gpt-4o-mini"),
            ],
        ));
        let anthropic = Box::new(StaticProviderCatalog::new(
            "anthropic",
            vec![ModelInfo::new("anthropic", "claude-3-5-sonnet")],
        ));

        let registry = ModelCatalogRegistry::new(vec![openai, anthropic]);
        // Before refresh, cache is empty.
        assert!(registry.all().is_empty());

        let errors = registry.refresh().await;
        assert!(errors.is_empty());

        let mut all = registry.all();
        all.sort_by(|a, b| a.provider.cmp(&b.provider).then(a.model.cmp(&b.model)));
        assert_eq!(
            all,
            vec![
                ModelRef::new("anthropic", "claude-3-5-sonnet"),
                ModelRef::new("openai", "gpt-4o"),
                ModelRef::new("openai", "gpt-4o-mini"),
            ]
        );
    }

    #[tokio::test]
    async fn catalog_registry_dedups_duplicate_model_info() {
        // Two providers that both report the same ModelInfo. Registry must
        // collapse the union by (provider, model).
        let a = Box::new(StaticProviderCatalog::new(
            "openai",
            vec![
                ModelInfo::new("openai", "gpt-4o"),
                ModelInfo::new("openai", "gpt-4o"), // intra-provider dup
            ],
        ));
        let b = Box::new(StaticProviderCatalog::new(
            "mirror",
            // `mirror` provider mistakenly also claims it — deduped on (provider, model).
            vec![ModelInfo::new("openai", "gpt-4o")],
        ));

        let registry = ModelCatalogRegistry::new(vec![a, b]);
        let errors = registry.refresh().await;
        assert!(errors.is_empty());
        let cached = registry.cached();
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0], ModelInfo::new("openai", "gpt-4o"));
    }

    /// Catalog whose `list_models` can be flipped between success and error at
    /// runtime via a shared `Mutex<Vec<ModelInfo>>` or a failure flag.
    #[allow(dead_code)]
    struct TogglingCatalog {
        name: String,
        // `None` → return an error; `Some(v)` → return v.
        payload: std::sync::Mutex<Option<Vec<ModelInfo>>>,
    }

    #[async_trait]
    impl ProviderCatalog for TogglingCatalog {
        fn provider_name(&self) -> &str {
            &self.name
        }
        async fn list_models(&self) -> Result<Vec<ModelInfo>, CatalogError> {
            let guard = self.payload.lock().unwrap();
            match &*guard {
                Some(v) => Ok(v.clone()),
                None => Err(CatalogError::Upstream("boom".into())),
            }
        }
    }

    #[tokio::test]
    async fn catalog_refresh_updates_cache_on_subsequent_calls() {
        // A single provider whose payload mutates between refresh() calls via
        // a shared Arc<Mutex>. The cache must reflect whichever payload the
        // most recent refresh saw.
        struct SharedCatalog {
            name: String,
            payload: std::sync::Arc<std::sync::Mutex<Vec<ModelInfo>>>,
        }

        #[async_trait]
        impl ProviderCatalog for SharedCatalog {
            fn provider_name(&self) -> &str {
                &self.name
            }
            async fn list_models(&self) -> Result<Vec<ModelInfo>, CatalogError> {
                Ok(self.payload.lock().unwrap().clone())
            }
        }

        let shared = std::sync::Arc::new(std::sync::Mutex::new(vec![ModelInfo::new(
            "openai", "gpt-4o",
        )]));
        let cat = SharedCatalog {
            name: "openai".into(),
            payload: shared.clone(),
        };
        let registry = ModelCatalogRegistry::new(vec![Box::new(cat)]);

        registry.refresh().await;
        assert_eq!(registry.cached().len(), 1);

        // Mutate upstream payload and refresh again — cache grows.
        shared.lock().unwrap().push(ModelInfo::new("openai", "gpt-4o-mini"));
        registry.refresh().await;
        assert_eq!(registry.cached().len(), 2);

        // Mutate upstream to shrink — cache shrinks too.
        shared.lock().unwrap().clear();
        shared.lock().unwrap().push(ModelInfo::new("openai", "o3-mini"));
        registry.refresh().await;
        let cached = registry.cached();
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].model, "o3-mini");
    }

    #[tokio::test]
    async fn catalog_refresh_reports_errors_but_preserves_cache() {
        // First pass: success populates cache. Second pass: upstream fails —
        // cache must NOT be purged.
        struct FlipCatalog {
            name: String,
            payload: std::sync::Arc<std::sync::Mutex<Option<Vec<ModelInfo>>>>,
        }

        #[async_trait]
        impl ProviderCatalog for FlipCatalog {
            fn provider_name(&self) -> &str {
                &self.name
            }
            async fn list_models(&self) -> Result<Vec<ModelInfo>, CatalogError> {
                match &*self.payload.lock().unwrap() {
                    Some(v) => Ok(v.clone()),
                    None => Err(CatalogError::Upstream("boom".into())),
                }
            }
        }

        let shared = std::sync::Arc::new(std::sync::Mutex::new(Some(vec![ModelInfo::new(
            "openai", "gpt-4o",
        )])));
        let cat = FlipCatalog {
            name: "openai".into(),
            payload: shared.clone(),
        };
        let registry = ModelCatalogRegistry::new(vec![Box::new(cat)]);

        let errors = registry.refresh().await;
        assert!(errors.is_empty());
        assert_eq!(registry.cached().len(), 1);

        // Flip upstream to failure; refresh must report the error but keep
        // the last-known catalog entries in place.
        *shared.lock().unwrap() = None;
        let errors = registry.refresh().await;
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].0, "openai");
        assert_eq!(
            registry.cached(),
            vec![ModelInfo::new("openai", "gpt-4o")],
            "cache must not be purged on transient upstream failure"
        );
    }

    #[test]
    fn catalog_refresh_config_defaults_are_sensible() {
        let cfg = CatalogRefreshConfig::defaults();
        assert!(cfg.enabled);
        assert_eq!(cfg.refresh_interval, Duration::from_secs(600));
        assert_eq!(cfg.stale_retention, Duration::from_secs(3600));
    }
}

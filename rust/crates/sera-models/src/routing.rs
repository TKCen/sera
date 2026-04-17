//! Dynamic model routing primitives — Phase 1: in-memory `HealthStore`.
//!
//! See `docs/plan/DYNAMIC-MODEL-ROUTING.md` §5 (data model) and §13 (phase 1).
//!
//! This module provides the observation-side of the dynamic routing design:
//! a thread-safe, in-memory `HealthStore` that records per-`(provider, model)`
//! latency, error rate (rolling 10-minute window), cost, and rate-limit events.
//!
//! Phase 1 scope is intentionally narrow — this data structure is not yet wired
//! into the request path. It is populated by gateway observation hooks and read
//! by metrics exporters. Selection (phase 2) and circuit breakers (phase 3)
//! build on top of these signals.

use std::collections::{HashMap, VecDeque};
use std::sync::RwLock;
use std::time::{Duration, Instant};

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
}

impl HealthStore {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
        }
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
    fn record_success_at(
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
        entry.push_latency(latency);
        entry.events.push_back((now, false));
        entry.cost_per_1k_tokens = cost_per_1k;
        entry.last_ok_at = Some(now);
        entry.total_requests += 1;
        entry.total_tokens_in += u64::from(tokens_in);
        entry.total_tokens_out += u64::from(tokens_out);
        let tokens = f64::from(tokens_in) + f64::from(tokens_out);
        entry.total_cost_usd += tokens * cost_per_1k / 1000.0;
    }

    fn record_error_at(&self, model: &ModelRef, is_429: bool, now: Instant) {
        let mut guard = self.inner.write().expect("HealthStore lock poisoned");
        let entry = guard
            .entry(model.clone())
            .or_insert_with(HealthEntry::new);
        entry.evict_old_events(now);
        entry.events.push_back((now, true));
        entry.total_requests += 1;
        entry.total_errors += 1;
        if is_429 {
            entry.last_429_at = Some(now);
        }
    }

    /// Return a point-in-time snapshot of the given model's health, if observed.
    pub fn snapshot(&self, model: &ModelRef) -> Option<ModelHealth> {
        let mut guard = self.inner.write().expect("HealthStore lock poisoned");
        // Evict stale events so err_rate reflects only the last 10 minutes. This is
        // a write under the hood even on a read path, but callers tolerate it.
        if let Some(entry) = guard.get_mut(model) {
            entry.evict_old_events(Instant::now());
            Some(entry.to_health())
        } else {
            None
        }
    }

    /// Return snapshots for every observed model. Intended for metrics exposure.
    pub fn all(&self) -> Vec<(ModelRef, ModelHealth)> {
        let mut guard = self.inner.write().expect("HealthStore lock poisoned");
        let now = Instant::now();
        guard
            .iter_mut()
            .map(|(k, v)| {
                v.evict_old_events(now);
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
}

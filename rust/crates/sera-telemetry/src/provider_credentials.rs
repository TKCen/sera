//! Per-credential outcome counters for the multi-account LLM auth pool
//! (sera-hjem).
//!
//! Each LLM request acquires a credential from a
//! `sera_models::AccountPool`; the call site reports the outcome
//! (`success` / `rate_limited` / `error_4xx` / `error_5xx`) and this module
//! aggregates the totals in process-local atomic counters keyed by
//! `(provider, credential_id, outcome)`.
//!
//! The structure is intentionally minimal — it lets dashboards / health
//! probes ask "which credential is failing?" without pulling in a full
//! metrics SDK.  When OpenTelemetry metric counters are wired up later, this
//! module's [`record`] entrypoint becomes a single integration point.
//!
//! # Example
//!
//! ```rust
//! use sera_telemetry::provider_credentials::{record, snapshot, CredentialOutcome};
//!
//! record("openai", "primary", CredentialOutcome::Success);
//! record("openai", "primary", CredentialOutcome::RateLimited);
//! record("openai", "backup",  CredentialOutcome::Success);
//!
//! let snap = snapshot();
//! assert!(snap.iter().any(|s| s.credential_id == "primary"));
//! ```

use std::collections::HashMap;
use std::sync::Mutex;

use once_cell::sync::Lazy;

/// Outcome bucket for a single credential request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CredentialOutcome {
    /// HTTP 2xx — credential is healthy.
    Success,
    /// HTTP 429 — credential hit a rate limit.
    RateLimited,
    /// HTTP 4xx (except 429) — non-retryable; credential is likely
    /// invalid / revoked.
    Error4xx,
    /// HTTP 5xx — provider-side fault, credential is fine but transiently
    /// unavailable.
    Error5xx,
}

impl CredentialOutcome {
    /// Stable label string used as the `outcome` metric tag.
    pub fn as_str(self) -> &'static str {
        match self {
            CredentialOutcome::Success => "success",
            CredentialOutcome::RateLimited => "rate_limited",
            CredentialOutcome::Error4xx => "error_4xx",
            CredentialOutcome::Error5xx => "error_5xx",
        }
    }
}

/// Composite key for the in-memory counter table.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CounterKey {
    provider: String,
    credential_id: String,
    outcome: CredentialOutcome,
}

/// Snapshot row returned by [`snapshot`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterSnapshot {
    pub provider: String,
    pub credential_id: String,
    pub outcome: CredentialOutcome,
    pub count: u64,
}

/// Process-wide counter table.  Single mutex keeps the API lock-light at the
/// per-credential granularity that matters; callers may invoke [`record`]
/// from any thread.
static COUNTERS: Lazy<Mutex<HashMap<CounterKey, u64>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Increment the counter for `(provider, credential_id, outcome)` by one.
///
/// `tracing::debug!` is also emitted with the same labels so the event
/// shows up in standard log scrapers.
pub fn record(provider: &str, credential_id: &str, outcome: CredentialOutcome) {
    let key = CounterKey {
        provider: provider.to_string(),
        credential_id: credential_id.to_string(),
        outcome,
    };
    let mut map = COUNTERS.lock().unwrap_or_else(|e| e.into_inner());
    let entry = map.entry(key).or_insert(0);
    *entry = entry.saturating_add(1);
    drop(map);
    tracing::debug!(
        target: "sera.provider_credentials",
        provider = provider,
        credential_id = credential_id,
        outcome = outcome.as_str(),
        "credential outcome recorded"
    );
}

/// Snapshot the entire counter table.  Useful for `/metrics`-style probes
/// and tests.  Order is unspecified.
pub fn snapshot() -> Vec<CounterSnapshot> {
    let map = COUNTERS.lock().unwrap_or_else(|e| e.into_inner());
    map.iter()
        .map(|(k, v)| CounterSnapshot {
            provider: k.provider.clone(),
            credential_id: k.credential_id.clone(),
            outcome: k.outcome,
            count: *v,
        })
        .collect()
}

/// Reset all counters.  Test-only helper to keep state hermetic.
#[cfg(test)]
pub fn reset_for_tests() {
    let mut map = COUNTERS.lock().unwrap_or_else(|e| e.into_inner());
    map.clear();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn count_for(
        provider: &str,
        credential_id: &str,
        outcome: CredentialOutcome,
    ) -> u64 {
        snapshot()
            .into_iter()
            .find(|s| {
                s.provider == provider
                    && s.credential_id == credential_id
                    && s.outcome == outcome
            })
            .map(|s| s.count)
            .unwrap_or(0)
    }

    #[test]
    fn outcome_label_strings_are_stable() {
        assert_eq!(CredentialOutcome::Success.as_str(), "success");
        assert_eq!(CredentialOutcome::RateLimited.as_str(), "rate_limited");
        assert_eq!(CredentialOutcome::Error4xx.as_str(), "error_4xx");
        assert_eq!(CredentialOutcome::Error5xx.as_str(), "error_5xx");
    }

    #[test]
    fn record_increments_per_label_set() {
        // Hermetic: namespace per-test by provider/key prefix.
        let p = "test_provider_increments";
        record(p, "k1", CredentialOutcome::Success);
        record(p, "k1", CredentialOutcome::Success);
        record(p, "k1", CredentialOutcome::RateLimited);
        record(p, "k2", CredentialOutcome::Success);

        assert_eq!(count_for(p, "k1", CredentialOutcome::Success), 2);
        assert_eq!(count_for(p, "k1", CredentialOutcome::RateLimited), 1);
        assert_eq!(count_for(p, "k2", CredentialOutcome::Success), 1);
        assert_eq!(count_for(p, "k2", CredentialOutcome::Error4xx), 0);
    }

    #[test]
    fn snapshot_returns_all_counters_for_provider() {
        let p = "test_provider_snapshot";
        record(p, "alpha", CredentialOutcome::Error5xx);
        record(p, "beta", CredentialOutcome::Error4xx);

        let entries: Vec<_> = snapshot()
            .into_iter()
            .filter(|s| s.provider == p)
            .collect();
        assert!(entries.len() >= 2);
        let credential_ids: std::collections::HashSet<_> =
            entries.iter().map(|s| s.credential_id.clone()).collect();
        assert!(credential_ids.contains("alpha"));
        assert!(credential_ids.contains("beta"));
    }
}

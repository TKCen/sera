//! Account pool — multi-account LLM auth with failover + cooldown.
//!
//! Holds N [`ProviderAccount`]s for a single provider id and round-robins
//! across them. When an account hits a rate limit, network error, or
//! repeated failure, the pool puts it into a [`AccountState::CoolingDown`]
//! state with a configurable TTL. Exhausted pools return
//! [`PoolError::NoAccountsAvailable`].
//!
//! # Scope
//!
//! - Single-provider pool (OpenAI, Anthropic, etc.).  No cross-provider
//!   failover — that's a larger follow-up.
//! - In-memory cooldown state only — does not persist across restarts.
//! - Round-robin selection with "skip cooling down" semantics.
//!
//! # Example
//!
//! ```rust,ignore
//! use sera_models::account_pool::{AccountPool, CooldownConfig, ProviderAccount};
//! use std::sync::Arc;
//!
//! let pool = AccountPool::new(
//!     "openai",
//!     vec![
//!         ProviderAccount::new("k1", "sk-one", None),
//!         ProviderAccount::new("k2", "sk-two", None),
//!     ],
//!     CooldownConfig::default(),
//! );
//!
//! let guard = pool.acquire()?;
//! // use guard.account().api_key ...
//! // on 429: guard.mark_rate_limited();
//! // on success: guard.mark_success(); // optional, drop also does this
//! ```

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use thiserror::Error;

// ---------------------------------------------------------------------------
// Cooldown reasons
// ---------------------------------------------------------------------------

/// Why an account was placed into cooldown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CooldownReason {
    /// HTTP 429 / explicit rate limit.
    RateLimited,
    /// HTTP 5xx or network-level failure.
    ProviderUnavailable,
    /// Repeated failures triggered exponential backoff.
    RepeatedFailure,
    /// Non-retryable 4xx (e.g. 401/403/404) — disables the credential
    /// until an operator clears it.
    NonRetryable,
}

impl CooldownReason {
    pub fn as_str(self) -> &'static str {
        match self {
            CooldownReason::RateLimited => "rate_limited",
            CooldownReason::ProviderUnavailable => "provider_unavailable",
            CooldownReason::RepeatedFailure => "repeated_failure",
            CooldownReason::NonRetryable => "non_retryable",
        }
    }
}

// ---------------------------------------------------------------------------
// Account state
// ---------------------------------------------------------------------------

/// Current runtime state of an account.
#[derive(Debug, Clone)]
pub enum AccountState {
    /// Ready to be acquired.
    Available,
    /// Rate-limited / erroring. Skipped by `acquire()` until `until`.
    CoolingDown {
        until: Instant,
        reason: CooldownReason,
    },
    /// Persistently disabled (e.g. invalid credential — non-retryable 4xx).
    /// `acquire()` always skips. Cleared only by an explicit operator action
    /// via [`ProviderAccount::force_available`].
    Disabled { reason: CooldownReason },
}

impl AccountState {
    /// True if this state is still cooling down at `now`.
    pub fn is_cooling_down(&self, now: Instant) -> bool {
        matches!(self, AccountState::CoolingDown { until, .. } if *until > now)
    }

    /// True if this state is `Disabled` (skipped by round-robin until cleared).
    pub fn is_disabled(&self) -> bool {
        matches!(self, AccountState::Disabled { .. })
    }
}

// ---------------------------------------------------------------------------
// Cooldown config
// ---------------------------------------------------------------------------

/// Tuning knobs for cooldown durations.
#[derive(Debug, Clone)]
pub struct CooldownConfig {
    /// Base TTL for a 429 rate-limit cooldown when no `Retry-After` is supplied.
    pub rate_limit_duration: Duration,
    /// Base TTL for a provider-unavailable / 5xx cooldown.
    pub provider_unavailable_duration: Duration,
    /// Base TTL for a repeated-failure cooldown (doubles each step until max).
    pub failure_base_duration: Duration,
    /// Hard upper bound on any cooldown duration. Sera-hjem default: 5 min.
    pub max_duration: Duration,
    /// Failure count (per account) after which repeated-failure backoff kicks in.
    pub failure_threshold: u32,
    /// "Sticky fallback" window: after a successful call on a fallback
    /// credential, the pool stays on that credential for this duration before
    /// round-robin resumes. `Duration::ZERO` disables sticky behaviour.
    pub sticky_fallback_window: Duration,
}

impl Default for CooldownConfig {
    fn default() -> Self {
        Self {
            rate_limit_duration: Duration::from_secs(60),
            provider_unavailable_duration: Duration::from_secs(30),
            failure_base_duration: Duration::from_secs(60), // 1m base for 429 backoff
            max_duration: Duration::from_secs(300),         // 5m cap (sera-hjem)
            failure_threshold: 1, // exponential backoff doubles from the first 429
            sticky_fallback_window: Duration::from_secs(300), // 5m sticky window
        }
    }
}

impl CooldownConfig {
    fn exp_backoff(&self, failure_count: u32) -> Duration {
        // failure_count starts at 1 when threshold is first crossed.
        // duration = base * 2^(count - threshold), clamped.
        let over = failure_count.saturating_sub(self.failure_threshold);
        let shift = over.min(16); // prevent overflow
        let multiplier: u64 = 1u64 << shift;
        let base_ms = self.failure_base_duration.as_millis() as u64;
        let total_ms = base_ms.saturating_mul(multiplier);
        let capped_ms = total_ms.min(self.max_duration.as_millis() as u64);
        Duration::from_millis(capped_ms)
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors returned by the pool.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum PoolError {
    #[error(
        "no accounts available for provider '{provider_id}' \
         (all {total} rate-limited, disabled or unavailable; \
         soonest cooldown expiry in {min_cooldown_remaining:?})"
    )]
    NoAccountsAvailable {
        provider_id: String,
        total: usize,
        /// Smallest remaining cooldown across all accounts. `None` when every
        /// non-disabled account is permanently disabled (no cooldown to wait
        /// for) or when the pool is empty.
        min_cooldown_remaining: Option<Duration>,
    },

    #[error("pool for provider '{0}' is empty — no accounts configured")]
    EmptyPool(String),
}

// ---------------------------------------------------------------------------
// ProviderAccount
// ---------------------------------------------------------------------------

/// A single credential entry in an account pool.
///
/// Interior mutability is used for the state + failure counter so that
/// `AccountPool` itself can be shared as `Arc<AccountPool>` without a top-level
/// lock.
#[derive(Debug)]
pub struct ProviderAccount {
    /// Human-readable account id (e.g. `"primary"`, `"backup"`, `"k1"`).
    pub id: String,
    /// API key used for authentication.
    pub api_key: String,
    /// Optional per-account base URL override. When `None`, the pool's
    /// `default_base_url` is used.
    pub base_url: Option<String>,
    /// Optional tier marker (e.g. `"free"`, `"tier1"`, `"tier2"`) for
    /// diagnostics / future cost-aware routing.
    pub tier: Option<String>,

    state: Mutex<AccountState>,
    failure_count: Mutex<u32>,
}

impl ProviderAccount {
    /// Build a new available account.
    pub fn new(id: impl Into<String>, api_key: impl Into<String>, base_url: Option<String>) -> Self {
        Self {
            id: id.into(),
            api_key: api_key.into(),
            base_url,
            tier: None,
            state: Mutex::new(AccountState::Available),
            failure_count: Mutex::new(0),
        }
    }

    /// Attach a tier marker.
    pub fn with_tier(mut self, tier: impl Into<String>) -> Self {
        self.tier = Some(tier.into());
        self
    }

    /// Snapshot the current state.
    pub fn state_snapshot(&self) -> AccountState {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Current failure counter.
    pub fn failure_count(&self) -> u32 {
        *self.failure_count.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Force-clear any cooldown and return the account to `Available`.
    /// Primarily intended for operator overrides and tests.
    pub fn force_available(&self) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        *s = AccountState::Available;
    }

    fn reset_failures(&self) {
        let mut f = self.failure_count.lock().unwrap_or_else(|e| e.into_inner());
        *f = 0;
    }

    fn bump_failures(&self) -> u32 {
        let mut f = self.failure_count.lock().unwrap_or_else(|e| e.into_inner());
        *f = f.saturating_add(1);
        *f
    }

    fn set_cooldown(&self, until: Instant, reason: CooldownReason) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        *s = AccountState::CoolingDown { until, reason };
    }

    fn set_disabled(&self, reason: CooldownReason) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        *s = AccountState::Disabled { reason };
    }

    /// Lazily flip back to Available if the cooldown expired. Returns `true`
    /// if the account is now `Available` (either freshly expired or already).
    /// `Disabled` accounts always return `false`.
    fn refresh_state(&self, now: Instant) -> bool {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        match &*s {
            AccountState::Available => true,
            AccountState::CoolingDown { until, .. } => {
                if *until <= now {
                    *s = AccountState::Available;
                    true
                } else {
                    false
                }
            }
            AccountState::Disabled { .. } => false,
        }
    }

    /// Remaining cooldown duration at `now`. Returns `None` for `Available`
    /// or `Disabled` accounts.
    fn remaining_cooldown(&self, now: Instant) -> Option<Duration> {
        let s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        match &*s {
            AccountState::CoolingDown { until, .. } if *until > now => Some(*until - now),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// AccountPool
// ---------------------------------------------------------------------------

/// "Sticky fallback" hint — when a fallback credential succeeds, the pool
/// pins acquisitions to that credential index until `until` expires, so the
/// caller is not bounced back to a still-recovering primary mid-burst.
#[derive(Debug, Clone, Copy)]
struct StickyHint {
    idx: usize,
    until: Instant,
}

/// Pool of provider accounts for a single provider id.
#[derive(Debug)]
pub struct AccountPool {
    provider_id: String,
    accounts: Vec<Arc<ProviderAccount>>,
    next_idx: AtomicUsize,
    cooldown: CooldownConfig,
    default_base_url: Option<String>,
    /// Active sticky fallback (interior mutability so `acquire` can pin
    /// without taking `&mut self`).
    sticky: Mutex<Option<StickyHint>>,
}

impl AccountPool {
    /// Build a new pool.  `default_base_url` is used when an individual
    /// account's `base_url` is `None`.
    pub fn new(
        provider_id: impl Into<String>,
        accounts: Vec<ProviderAccount>,
        cooldown: CooldownConfig,
    ) -> Self {
        Self {
            provider_id: provider_id.into(),
            accounts: accounts.into_iter().map(Arc::new).collect(),
            next_idx: AtomicUsize::new(0),
            cooldown,
            default_base_url: None,
            sticky: Mutex::new(None),
        }
    }

    /// Set a pool-wide default base URL (used when account has no override).
    pub fn with_default_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.default_base_url = Some(base_url.into());
        self
    }

    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }

    pub fn len(&self) -> usize {
        self.accounts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.accounts.is_empty()
    }

    pub fn default_base_url(&self) -> Option<&str> {
        self.default_base_url.as_deref()
    }

    /// Acquire the next available account.
    ///
    /// Selection order:
    /// 1. If a sticky-fallback hint is active and the pinned credential is
    ///    still available at `now`, return it (sera-hjem stickiness).
    /// 2. Otherwise round-robin from `next_idx`, skipping cooling-down or
    ///    `Disabled` entries.
    ///
    /// Returns [`PoolError::NoAccountsAvailable`] (with the soonest cooldown
    /// expiry, if any) when no usable account exists.
    pub fn acquire(self: &Arc<Self>) -> Result<AccountGuard, PoolError> {
        if self.accounts.is_empty() {
            return Err(PoolError::EmptyPool(self.provider_id.clone()));
        }

        let now = Instant::now();
        let len = self.accounts.len();

        // 1) Sticky-fallback: prefer the pinned credential while it's hot.
        let sticky_idx = {
            let mut s = self.sticky.lock().unwrap_or_else(|e| e.into_inner());
            match *s {
                Some(StickyHint { until, idx }) if until > now && idx < len => Some(idx),
                Some(_) => {
                    *s = None;
                    None
                }
                None => None,
            }
        };
        if let Some(idx) = sticky_idx {
            let account = &self.accounts[idx];
            if account.refresh_state(now) {
                return Ok(AccountGuard {
                    pool: Arc::clone(self),
                    account: Arc::clone(account),
                    sticky_idx: Some(idx),
                    completed: false,
                });
            }
            // Sticky credential lapsed — drop the hint and fall through.
            let mut s = self.sticky.lock().unwrap_or_else(|e| e.into_inner());
            *s = None;
        }

        // 2) Round-robin scan.
        let start = self.next_idx.fetch_add(1, Ordering::Relaxed);
        for offset in 0..len {
            let idx = (start.wrapping_add(offset)) % len;
            let account = &self.accounts[idx];
            if account.refresh_state(now) {
                return Ok(AccountGuard {
                    pool: Arc::clone(self),
                    account: Arc::clone(account),
                    sticky_idx: Some(idx),
                    completed: false,
                });
            }
        }

        // 3) Exhaustion — compute soonest expiry across the cooling accounts.
        let min_cooldown_remaining = self
            .accounts
            .iter()
            .filter_map(|a| a.remaining_cooldown(now))
            .min();

        Err(PoolError::NoAccountsAvailable {
            provider_id: self.provider_id.clone(),
            total: len,
            min_cooldown_remaining,
        })
    }

    /// Set / clear the sticky-fallback pin. Public for tests and operator
    /// tooling; normal use goes via [`AccountGuard::mark_success`].
    pub fn set_sticky(&self, idx: usize, until: Instant) {
        if idx >= self.accounts.len() {
            return;
        }
        let mut s = self.sticky.lock().unwrap_or_else(|e| e.into_inner());
        *s = Some(StickyHint { idx, until });
    }

    /// Clear any active sticky-fallback pin.
    pub fn clear_sticky(&self) {
        let mut s = self.sticky.lock().unwrap_or_else(|e| e.into_inner());
        *s = None;
    }

    /// Inspect the active sticky pin (for tests/diagnostics).
    pub fn sticky_idx(&self) -> Option<usize> {
        self.sticky
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|h| h.idx)
    }

    /// Configured sticky-fallback window (mirrors `CooldownConfig`).
    pub fn sticky_window(&self) -> Duration {
        self.cooldown.sticky_fallback_window
    }

    /// True when at least one account is currently available.
    pub fn any_available(&self) -> bool {
        let now = Instant::now();
        self.accounts.iter().any(|a| a.refresh_state(now))
    }

    /// Snapshot (for tests/diagnostics): (account_id, state) pairs.
    pub fn state_snapshot(&self) -> Vec<(String, AccountState)> {
        self.accounts
            .iter()
            .map(|a| (a.id.clone(), a.state_snapshot()))
            .collect()
    }

    fn apply_cooldown(&self, account: &ProviderAccount, reason: CooldownReason) {
        let now = Instant::now();
        match reason {
            CooldownReason::RateLimited => {
                let failures = account.bump_failures();
                let base = if failures >= self.cooldown.failure_threshold {
                    self.cooldown.exp_backoff(failures)
                } else {
                    self.cooldown.rate_limit_duration
                };
                account.set_cooldown(now + base, reason);
            }
            CooldownReason::ProviderUnavailable => {
                let failures = account.bump_failures();
                let base = if failures >= self.cooldown.failure_threshold {
                    self.cooldown.exp_backoff(failures)
                } else {
                    self.cooldown.provider_unavailable_duration
                };
                account.set_cooldown(now + base, reason);
            }
            CooldownReason::RepeatedFailure => {
                let failures = account.bump_failures();
                let dur = self.cooldown.exp_backoff(failures.max(self.cooldown.failure_threshold));
                account.set_cooldown(now + dur, reason);
            }
            CooldownReason::NonRetryable => {
                // No backoff math — disable persistently.
                account.bump_failures();
                account.set_disabled(reason);
            }
        }
    }

    /// Apply a 429 cooldown that honours an explicit `Retry-After` from the
    /// provider when it is at least as long as the exponentially-backed-off
    /// default.  Bumps the failure counter so subsequent 429s extend the
    /// backoff (capped at [`CooldownConfig::max_duration`]).
    fn apply_rate_limited(&self, account: &ProviderAccount, retry_after: Option<Duration>) {
        let now = Instant::now();
        let failures = account.bump_failures();
        // Base = max(server-supplied retry_after, exponential backoff).
        let backoff = if failures >= self.cooldown.failure_threshold {
            self.cooldown.exp_backoff(failures)
        } else {
            self.cooldown.rate_limit_duration
        };
        let base = match retry_after {
            Some(server) => server.max(backoff),
            None => backoff,
        };
        let capped = base.min(self.cooldown.max_duration);
        account.set_cooldown(now + capped, CooldownReason::RateLimited);
    }
}

// ---------------------------------------------------------------------------
// AccountGuard
// ---------------------------------------------------------------------------

/// RAII handle returned by [`AccountPool::acquire`].  On drop, an un-completed
/// guard is treated as a silent success (no cooldown applied).  Callers should
/// invoke [`AccountGuard::mark_rate_limited`] / [`AccountGuard::mark_unavailable`]
/// / [`AccountGuard::mark_failure`] when the provider call failed.
#[derive(Debug)]
pub struct AccountGuard {
    pool: Arc<AccountPool>,
    account: Arc<ProviderAccount>,
    /// Index of the acquired credential — needed to set the sticky-fallback
    /// pin on success.
    sticky_idx: Option<usize>,
    completed: bool,
}

impl AccountGuard {
    /// The acquired account.
    pub fn account(&self) -> &ProviderAccount {
        &self.account
    }

    /// User-facing credential id for telemetry / logs.
    pub fn credential_id(&self) -> &str {
        &self.account.id
    }

    /// Effective base URL (account override or pool default, if any).
    pub fn effective_base_url(&self) -> Option<&str> {
        self.account
            .base_url
            .as_deref()
            .or(self.pool.default_base_url())
    }

    /// Record a successful call.  Clears the failure counter and pins the
    /// sticky-fallback hint to this credential for
    /// [`CooldownConfig::sticky_fallback_window`].
    pub fn mark_success(mut self) {
        self.account.reset_failures();
        let window = self.pool.cooldown.sticky_fallback_window;
        if !window.is_zero()
            && let Some(idx) = self.sticky_idx
        {
            let until = Instant::now() + window;
            self.pool.set_sticky(idx, until);
        }
        self.completed = true;
    }

    /// Sera-hjem alias for [`AccountGuard::mark_success`].
    pub fn record_success(self) {
        self.mark_success();
    }

    /// Mark the account as rate-limited (HTTP 429).  Starts / extends the
    /// rate-limit cooldown using the default exponential backoff.
    pub fn mark_rate_limited(mut self) {
        self.pool.apply_rate_limited(&self.account, None);
        self.pool.clear_sticky();
        self.completed = true;
    }

    /// Sera-hjem: 429 with optional server-supplied `Retry-After`.  When
    /// supplied, the longer of (server retry_after, exponential backoff) is
    /// used, capped at [`CooldownConfig::max_duration`].
    pub fn record_429(mut self, retry_after: Option<Duration>) {
        self.pool.apply_rate_limited(&self.account, retry_after);
        self.pool.clear_sticky();
        self.completed = true;
    }

    /// Mark the account as unavailable (HTTP 5xx / network error).
    pub fn mark_unavailable(mut self) {
        self.pool.apply_cooldown(&self.account, CooldownReason::ProviderUnavailable);
        self.pool.clear_sticky();
        self.completed = true;
    }

    /// Generic failure — bumps counter and applies the general failure
    /// backoff if the threshold was crossed.
    pub fn mark_failure(mut self) {
        self.pool.apply_cooldown(&self.account, CooldownReason::RepeatedFailure);
        self.pool.clear_sticky();
        self.completed = true;
    }

    /// Sera-hjem: persistently disable this credential — used for non-retryable
    /// 4xx responses (401, 403, 404 etc.).  The credential is permanently
    /// skipped by `acquire()` until [`ProviderAccount::force_available`].
    pub fn record_non_retryable_error(mut self) {
        self.pool.apply_cooldown(&self.account, CooldownReason::NonRetryable);
        self.pool.clear_sticky();
        self.completed = true;
    }
}

impl Drop for AccountGuard {
    fn drop(&mut self) {
        if !self.completed {
            // Treat un-completed drops as silent success so callers that
            // propagate via `?` after a successful acquire don't accidentally
            // mark the account as failing.
            self.account.reset_failures();
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Test helper: round-robin-friendly config with sticky-fallback disabled
    /// and the legacy 60s rate-limit baseline (failure_threshold=3).  Tests
    /// that exercise sticky behaviour build their own config explicitly.
    fn round_robin_config() -> CooldownConfig {
        CooldownConfig {
            sticky_fallback_window: Duration::ZERO,
            failure_threshold: 3,
            ..CooldownConfig::default()
        }
    }

    fn make_pool(n: usize) -> Arc<AccountPool> {
        let accounts: Vec<ProviderAccount> = (0..n)
            .map(|i| ProviderAccount::new(format!("k{i}"), format!("sk-{i}"), None))
            .collect();
        Arc::new(AccountPool::new("openai", accounts, round_robin_config()))
    }

    // ── Round-robin distribution ──────────────────────────────────────────────

    #[test]
    fn round_robin_cycles_through_accounts() {
        let pool = make_pool(3);
        let mut seen = Vec::new();
        for _ in 0..6 {
            let g = pool.acquire().expect("acquire");
            seen.push(g.account().id.clone());
            g.mark_success();
        }
        // Expect each key to appear twice across 6 acquisitions.
        let k0 = seen.iter().filter(|id| *id == "k0").count();
        let k1 = seen.iter().filter(|id| *id == "k1").count();
        let k2 = seen.iter().filter(|id| *id == "k2").count();
        assert_eq!(k0, 2);
        assert_eq!(k1, 2);
        assert_eq!(k2, 2);
    }

    #[test]
    fn single_account_pool_always_returns_same_account() {
        let pool = make_pool(1);
        for _ in 0..3 {
            let g = pool.acquire().expect("acquire");
            assert_eq!(g.account().id, "k0");
            g.mark_success();
        }
    }

    // ── Cooldown on 429 ───────────────────────────────────────────────────────

    #[test]
    fn rate_limit_cools_down_account() {
        let pool = make_pool(2);
        let g = pool.acquire().expect("acquire");
        let id1 = g.account().id.clone();
        g.mark_rate_limited();

        // Next acquire should skip the cooling-down account.
        let g2 = pool.acquire().expect("acquire second");
        assert_ne!(g2.account().id, id1);
        g2.mark_success();
    }

    #[test]
    fn rate_limit_duration_matches_config() {
        let short = CooldownConfig {
            rate_limit_duration: Duration::from_millis(1),
            ..round_robin_config()
        };
        let pool = Arc::new(AccountPool::new(
            "openai",
            vec![ProviderAccount::new("k0", "sk-0", None)],
            short,
        ));

        let g = pool.acquire().expect("acquire");
        g.mark_rate_limited();

        // Immediately after, still cooling down.
        assert!(!pool.any_available());

        // After the configured TTL, available again.
        std::thread::sleep(Duration::from_millis(5));
        assert!(pool.any_available());
    }

    // ── Cooldown expiry restores Available ────────────────────────────────────

    #[test]
    fn cooldown_expiry_restores_account_on_acquire() {
        let short = CooldownConfig {
            rate_limit_duration: Duration::from_millis(1),
            ..round_robin_config()
        };
        let pool = Arc::new(AccountPool::new(
            "openai",
            vec![ProviderAccount::new("k0", "sk-0", None)],
            short,
        ));

        let g = pool.acquire().expect("acquire first");
        g.mark_rate_limited();

        std::thread::sleep(Duration::from_millis(10));

        let g2 = pool.acquire().expect("should be available after expiry");
        assert_eq!(g2.account().id, "k0");
        assert!(matches!(g2.account().state_snapshot(), AccountState::Available));
    }

    // ── All-exhausted returns NoAccountsAvailable ─────────────────────────────

    #[test]
    fn all_exhausted_returns_error() {
        let pool = make_pool(2);

        let g1 = pool.acquire().expect("first");
        g1.mark_rate_limited();
        let g2 = pool.acquire().expect("second");
        g2.mark_rate_limited();

        let err = pool.acquire().expect_err("pool should be exhausted");
        match err {
            PoolError::NoAccountsAvailable {
                provider_id,
                total,
                min_cooldown_remaining,
            } => {
                assert_eq!(provider_id, "openai");
                assert_eq!(total, 2);
                assert!(
                    min_cooldown_remaining.is_some(),
                    "exhausted pool should report a soonest expiry"
                );
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn empty_pool_returns_empty_pool_error() {
        let pool: Arc<AccountPool> = Arc::new(AccountPool::new(
            "openai",
            vec![],
            CooldownConfig::default(),
        ));
        let err = pool.acquire().expect_err("empty pool");
        assert!(matches!(err, PoolError::EmptyPool(_)));
    }

    // ── Mark success resets failure counter ───────────────────────────────────

    #[test]
    fn mark_success_resets_failure_count() {
        let pool = make_pool(1);
        let g = pool.acquire().expect("acquire");
        g.mark_rate_limited();
        std::thread::sleep(Duration::from_millis(2));

        // Make rate_limit_duration long first so we need to manually reset for test clarity:
        // here the default is 60s, so we use a fresh short-cooldown pool.
        let short = CooldownConfig {
            rate_limit_duration: Duration::from_millis(1),
            ..round_robin_config()
        };
        let pool = Arc::new(AccountPool::new(
            "openai",
            vec![ProviderAccount::new("k0", "sk-0", None)],
            short,
        ));
        let g = pool.acquire().expect("acquire");
        g.mark_rate_limited();
        assert_eq!(pool.accounts[0].failure_count(), 1);

        std::thread::sleep(Duration::from_millis(3));
        let g2 = pool.acquire().expect("acquire");
        g2.mark_success();
        assert_eq!(pool.accounts[0].failure_count(), 0);
    }

    // ── Exponential backoff ───────────────────────────────────────────────────

    #[test]
    fn exp_backoff_doubles_over_threshold() {
        let cfg = CooldownConfig {
            failure_threshold: 3,
            failure_base_duration: Duration::from_secs(10),
            max_duration: Duration::from_secs(3600),
            ..CooldownConfig::default()
        };
        // At threshold → base (over = 0 → 1x).
        let d3 = cfg.exp_backoff(3);
        assert_eq!(d3, Duration::from_secs(10));
        // One over → 2x.
        let d4 = cfg.exp_backoff(4);
        assert_eq!(d4, Duration::from_secs(20));
        // Two over → 4x.
        let d5 = cfg.exp_backoff(5);
        assert_eq!(d5, Duration::from_secs(40));
    }

    #[test]
    fn exp_backoff_respects_max_duration() {
        let cfg = CooldownConfig {
            failure_threshold: 1,
            failure_base_duration: Duration::from_secs(10),
            max_duration: Duration::from_secs(60),
            ..CooldownConfig::default()
        };
        // Would be 10 * 2^20 seconds but must clamp.
        let d = cfg.exp_backoff(100);
        assert_eq!(d, Duration::from_secs(60));
    }

    // ── Guard drop behaviour ──────────────────────────────────────────────────

    #[test]
    fn guard_drop_without_completion_does_not_cool_down() {
        let pool = make_pool(1);
        {
            let _g = pool.acquire().expect("acquire");
            // Drop without marking anything.
        }
        // Still available.
        let g2 = pool.acquire().expect("should still be acquirable");
        assert!(matches!(g2.account().state_snapshot(), AccountState::Available));
    }

    // ── Effective base URL ────────────────────────────────────────────────────

    #[test]
    fn account_override_wins_over_pool_default() {
        let pool = Arc::new(
            AccountPool::new(
                "openai",
                vec![ProviderAccount::new(
                    "k0",
                    "sk-0",
                    Some("https://account.example".into()),
                )],
                CooldownConfig::default(),
            )
            .with_default_base_url("https://pool.default"),
        );
        let g = pool.acquire().expect("acquire");
        assert_eq!(g.effective_base_url(), Some("https://account.example"));
    }

    #[test]
    fn pool_default_used_when_account_has_none() {
        let pool = Arc::new(
            AccountPool::new(
                "openai",
                vec![ProviderAccount::new("k0", "sk-0", None)],
                CooldownConfig::default(),
            )
            .with_default_base_url("https://pool.default"),
        );
        let g = pool.acquire().expect("acquire");
        assert_eq!(g.effective_base_url(), Some("https://pool.default"));
    }

    // ── Concurrent acquire ────────────────────────────────────────────────────

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_acquire_distributes_across_accounts() {
        let pool = make_pool(4);
        let handles: Vec<_> = (0..12)
            .map(|_| {
                let pool = Arc::clone(&pool);
                tokio::spawn(async move {
                    let g = pool.acquire().expect("acquire");
                    let id = g.account().id.clone();
                    g.mark_success();
                    id
                })
            })
            .collect();

        let mut counts = std::collections::HashMap::<String, usize>::new();
        for h in handles {
            let id = h.await.expect("task");
            *counts.entry(id).or_insert(0) += 1;
        }
        // Every account should have been used at least once under multi-threaded
        // round-robin acquisition.
        assert_eq!(counts.len(), 4, "each account should have been used: {counts:?}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_acquire_with_cooldown_falls_back() {
        let pool = make_pool(3);

        // Cool down one account.
        let g = pool.acquire().expect("acquire");
        let cooled_id = g.account().id.clone();
        g.mark_rate_limited();

        // Concurrent acquisitions should avoid the cooled account.
        let handles: Vec<_> = (0..6)
            .map(|_| {
                let pool = Arc::clone(&pool);
                tokio::spawn(async move {
                    let g = pool.acquire().expect("acquire");
                    let id = g.account().id.clone();
                    g.mark_success();
                    id
                })
            })
            .collect();

        for h in handles {
            let id = h.await.expect("task");
            assert_ne!(id, cooled_id, "cooled account should not have been used");
        }
    }

    // ── CooldownReason::as_str ────────────────────────────────────────────────

    #[test]
    fn cooldown_reason_string() {
        assert_eq!(CooldownReason::RateLimited.as_str(), "rate_limited");
        assert_eq!(CooldownReason::ProviderUnavailable.as_str(), "provider_unavailable");
        assert_eq!(CooldownReason::RepeatedFailure.as_str(), "repeated_failure");
    }

    // ── state_snapshot ────────────────────────────────────────────────────────

    #[test]
    fn state_snapshot_reports_all_accounts() {
        let pool = make_pool(3);
        let snapshot = pool.state_snapshot();
        assert_eq!(snapshot.len(), 3);
        for (_, state) in snapshot {
            assert!(matches!(state, AccountState::Available));
        }
    }

    #[test]
    fn state_snapshot_reflects_cooldown() {
        let pool = make_pool(2);
        let g = pool.acquire().expect("acquire");
        g.mark_rate_limited();
        let snapshot = pool.state_snapshot();
        let cooling = snapshot
            .iter()
            .filter(|(_, s)| matches!(s, AccountState::CoolingDown { .. }))
            .count();
        assert_eq!(cooling, 1);
    }

    // ── force_available helper (used by internal cooldown expiry) ─────────────

    #[test]
    fn force_available_clears_cooldown() {
        let pool = make_pool(1);
        let g = pool.acquire().expect("acquire");
        g.mark_rate_limited();
        assert!(matches!(
            pool.accounts[0].state_snapshot(),
            AccountState::CoolingDown { .. }
        ));
        pool.accounts[0].force_available();
        assert!(matches!(
            pool.accounts[0].state_snapshot(),
            AccountState::Available
        ));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // sera-hjem additions
    // ─────────────────────────────────────────────────────────────────────────

    /// Pool builder for sera-hjem tests — exposes sticky window + non-default
    /// failure threshold knobs without inheriting the round-robin shim.
    fn make_hjem_pool(n: usize, cfg: CooldownConfig) -> Arc<AccountPool> {
        let accounts: Vec<ProviderAccount> = (0..n)
            .map(|i| ProviderAccount::new(format!("k{i}"), format!("sk-{i}"), None))
            .collect();
        Arc::new(AccountPool::new("openai", accounts, cfg))
    }

    fn no_sticky_cfg() -> CooldownConfig {
        CooldownConfig {
            sticky_fallback_window: Duration::ZERO,
            failure_threshold: 1,
            failure_base_duration: Duration::from_millis(10),
            rate_limit_duration: Duration::from_millis(10),
            max_duration: Duration::from_secs(300),
            ..CooldownConfig::default()
        }
    }

    // Round-robin across multiple credentials (no sticky bias).
    #[test]
    fn hjem_round_robin_selects_each_credential_at_least_once() {
        let pool = make_hjem_pool(3, no_sticky_cfg());
        let mut seen = std::collections::HashSet::new();
        for _ in 0..3 {
            let g = pool.acquire().expect("acquire");
            seen.insert(g.account().id.clone());
            g.mark_success();
        }
        assert_eq!(seen.len(), 3, "every credential should be picked: {seen:?}");
    }

    // 429 cooldown skips the offending key.
    #[test]
    fn hjem_429_cools_down_and_round_robin_skips() {
        let pool = make_hjem_pool(2, no_sticky_cfg());
        let g = pool.acquire().expect("acquire");
        let bad = g.account().id.clone();
        g.record_429(None);

        let g2 = pool.acquire().expect("acquire fallback");
        assert_ne!(g2.account().id, bad);
    }

    // Exponential backoff doubles each consecutive 429.
    #[test]
    fn hjem_exponential_backoff_doubles_each_consecutive_429() {
        let cfg = CooldownConfig {
            failure_threshold: 1,
            rate_limit_duration: Duration::from_secs(10),
            failure_base_duration: Duration::from_secs(10),
            max_duration: Duration::from_secs(3600),
            sticky_fallback_window: Duration::ZERO,
            ..CooldownConfig::default()
        };
        // First 429: failures=1 → exp_backoff(1) = 10s
        assert_eq!(cfg.exp_backoff(1), Duration::from_secs(10));
        // Second 429: failures=2 → 20s
        assert_eq!(cfg.exp_backoff(2), Duration::from_secs(20));
        // Third 429: failures=3 → 40s
        assert_eq!(cfg.exp_backoff(3), Duration::from_secs(40));
    }

    // Sticky fallback: after a successful call on a fallback credential, the
    // pool stays on it for the configured window.
    #[test]
    fn hjem_sticky_fallback_pins_after_success() {
        let cfg = CooldownConfig {
            sticky_fallback_window: Duration::from_secs(60),
            failure_threshold: 1,
            rate_limit_duration: Duration::from_millis(10),
            ..CooldownConfig::default()
        };
        let pool = make_hjem_pool(3, cfg);

        // Burn the first credential (k0) → 429.
        let g0 = pool.acquire().expect("acquire k0");
        let primary = g0.account().id.clone();
        g0.record_429(None);

        // Acquire fallback (whichever round-robin lands on next).
        let g1 = pool.acquire().expect("acquire fallback");
        let fallback = g1.account().id.clone();
        assert_ne!(fallback, primary);
        g1.mark_success();

        // Subsequent acquires within the sticky window must keep returning
        // the same fallback credential.
        for _ in 0..5 {
            let g = pool.acquire().expect("acquire sticky");
            assert_eq!(g.account().id, fallback, "sticky fallback should pin");
            g.mark_success();
        }
    }

    // Sticky window expires → round-robin resumes.
    #[test]
    fn hjem_sticky_window_expires_then_round_robin_resumes() {
        let cfg = CooldownConfig {
            sticky_fallback_window: Duration::from_millis(20),
            failure_threshold: 3,
            rate_limit_duration: Duration::from_millis(5),
            ..CooldownConfig::default()
        };
        let pool = make_hjem_pool(3, cfg);

        // Burn k0 briefly so a fallback is chosen.
        let g0 = pool.acquire().expect("acquire k0");
        let primary = g0.account().id.clone();
        g0.record_429(None);

        let g1 = pool.acquire().expect("fallback");
        let fallback = g1.account().id.clone();
        assert_ne!(fallback, primary);
        g1.mark_success();

        // Wait for sticky window to expire AND k0 cooldown to expire.
        std::thread::sleep(Duration::from_millis(50));

        // After expiry, round-robin should yield variety across the 3 keys.
        let mut seen = std::collections::HashSet::new();
        for _ in 0..6 {
            let g = pool.acquire().expect("acquire");
            seen.insert(g.account().id.clone());
            g.mark_success();
            // Each success re-pins sticky → clear it so we keep rotating.
            pool.clear_sticky();
        }
        assert!(
            seen.len() >= 2,
            "post-sticky round-robin should hit multiple keys: {seen:?}"
        );
    }

    // Non-retryable error disables the credential persistently.
    #[test]
    fn hjem_non_retryable_disables_credential_permanently() {
        let pool = make_hjem_pool(2, no_sticky_cfg());
        let g = pool.acquire().expect("acquire");
        let bad = g.account().id.clone();
        g.record_non_retryable_error();

        // Even after waiting, disabled credential is never selected.
        std::thread::sleep(Duration::from_millis(50));
        for _ in 0..6 {
            let g = pool.acquire().expect("acquire");
            assert_ne!(
                g.account().id,
                bad,
                "disabled credential must never be returned"
            );
            g.mark_success();
        }
        // State snapshot reports Disabled.
        let bad_snapshot = pool
            .accounts
            .iter()
            .find(|a| a.id == bad)
            .expect("find bad")
            .state_snapshot();
        assert!(matches!(bad_snapshot, AccountState::Disabled { .. }));
    }

    // All-in-cooldown returns NoAccountsAvailable with min-expiry duration.
    #[test]
    fn hjem_all_in_cooldown_returns_min_cooldown_remaining() {
        // Long cooldown so we can read the soonest-expiry value reliably.
        let cfg = CooldownConfig {
            rate_limit_duration: Duration::from_secs(60),
            failure_threshold: 5,
            sticky_fallback_window: Duration::ZERO,
            ..CooldownConfig::default()
        };
        let pool = make_hjem_pool(2, cfg);

        let g0 = pool.acquire().expect("k0");
        g0.record_429(None);
        let g1 = pool.acquire().expect("k1");
        g1.record_429(None);

        let err = pool.acquire().expect_err("exhausted");
        match err {
            PoolError::NoAccountsAvailable {
                total,
                min_cooldown_remaining,
                ..
            } => {
                assert_eq!(total, 2);
                let remaining = min_cooldown_remaining.expect("should report min expiry");
                // Remaining must be positive and ≤ rate_limit_duration.
                assert!(remaining > Duration::ZERO);
                assert!(remaining <= Duration::from_secs(60));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    // record_429 honours server-supplied retry_after when longer than backoff.
    #[test]
    fn hjem_record_429_honours_long_retry_after() {
        let cfg = CooldownConfig {
            rate_limit_duration: Duration::from_secs(1),
            failure_threshold: 5,
            max_duration: Duration::from_secs(300),
            sticky_fallback_window: Duration::ZERO,
            ..CooldownConfig::default()
        };
        let pool = make_hjem_pool(1, cfg);
        let g = pool.acquire().expect("acquire");
        g.record_429(Some(Duration::from_secs(120)));

        let snap = pool.accounts[0].state_snapshot();
        match snap {
            AccountState::CoolingDown { until, .. } => {
                let remaining = until.saturating_duration_since(Instant::now());
                // Should be close to 120s (server retry_after wins over 1s default).
                assert!(remaining > Duration::from_secs(100), "remaining: {remaining:?}");
            }
            other => panic!("expected CoolingDown, got {other:?}"),
        }
    }

    // record_429 caps cooldown at max_duration even when retry_after is huge.
    #[test]
    fn hjem_record_429_caps_at_max_duration() {
        let cfg = CooldownConfig {
            rate_limit_duration: Duration::from_secs(1),
            failure_threshold: 5,
            max_duration: Duration::from_secs(300),
            sticky_fallback_window: Duration::ZERO,
            ..CooldownConfig::default()
        };
        let pool = make_hjem_pool(1, cfg);
        let g = pool.acquire().expect("acquire");
        g.record_429(Some(Duration::from_secs(10_000)));

        let snap = pool.accounts[0].state_snapshot();
        match snap {
            AccountState::CoolingDown { until, .. } => {
                let remaining = until.saturating_duration_since(Instant::now());
                assert!(
                    remaining <= Duration::from_secs(301),
                    "max_duration cap not honoured: {remaining:?}"
                );
            }
            other => panic!("expected CoolingDown, got {other:?}"),
        }
    }

    // CooldownReason::NonRetryable surfaces in the as_str() label set used by
    // telemetry counters.
    #[test]
    fn hjem_non_retryable_reason_string() {
        assert_eq!(CooldownReason::NonRetryable.as_str(), "non_retryable");
    }

    // force_available clears Disabled state too (operator override).
    #[test]
    fn hjem_force_available_clears_disabled() {
        let pool = make_hjem_pool(1, no_sticky_cfg());
        let g = pool.acquire().expect("acquire");
        g.record_non_retryable_error();
        assert!(matches!(
            pool.accounts[0].state_snapshot(),
            AccountState::Disabled { .. }
        ));
        pool.accounts[0].force_available();
        assert!(matches!(
            pool.accounts[0].state_snapshot(),
            AccountState::Available
        ));
    }

    // CredentialHandle.credential_id passes through to the account id.
    #[test]
    fn hjem_credential_id_accessor() {
        let pool = make_hjem_pool(1, no_sticky_cfg());
        let g = pool.acquire().expect("acquire");
        assert_eq!(g.credential_id(), "k0");
        g.mark_success();
    }
}

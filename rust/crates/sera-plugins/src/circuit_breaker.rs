//! Simple three-state circuit breaker for plugin failure handling.
//!
//! State transitions:
//! - `Closed` → `Open` after N consecutive failures
//! - `Open` → `HalfOpen` after a configured timeout
//! - `HalfOpen` → `Closed` on success, `Open` on failure

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::error::PluginError;

pub use sera_types::CircuitState;

/// Internal mutable state of the breaker.
#[derive(Debug)]
struct BreakerState {
    state: CircuitState,
    consecutive_failures: u32,
    opened_at: Option<Instant>,
}

/// A circuit breaker for a single plugin.
///
/// Thread-safe via an internal `Mutex`. Cheap to clone (wraps `Arc` internally).
/// Intended to be stored alongside the plugin registration in the gateway.
#[derive(Debug)]
pub struct CircuitBreaker {
    plugin_name: String,
    /// How many consecutive failures before the circuit opens.
    failure_threshold: u32,
    /// How long to wait in the Open state before moving to HalfOpen.
    reset_timeout: Duration,
    inner: Mutex<BreakerState>,
}

impl CircuitBreaker {
    /// Create a new circuit breaker with the given thresholds.
    pub fn new(
        plugin_name: impl Into<String>,
        failure_threshold: u32,
        reset_timeout: Duration,
    ) -> Self {
        Self {
            plugin_name: plugin_name.into(),
            failure_threshold,
            reset_timeout,
            inner: Mutex::new(BreakerState {
                state: CircuitState::Closed,
                consecutive_failures: 0,
                opened_at: None,
            }),
        }
    }

    /// Return the current state, transitioning Open → HalfOpen if the reset
    /// timeout has elapsed.
    pub fn state(&self) -> CircuitState {
        let mut inner = self.inner.lock().expect("circuit breaker lock poisoned");
        if inner.state == CircuitState::Open
            && let Some(opened_at) = inner.opened_at
            && opened_at.elapsed() >= self.reset_timeout
        {
            inner.state = CircuitState::HalfOpen;
        }
        inner.state
    }

    /// Check whether a call is allowed. Returns [`PluginError::CircuitOpen`] if
    /// the circuit is in the Open state.
    pub fn allow(&self) -> Result<(), PluginError> {
        match self.state() {
            CircuitState::Open => Err(PluginError::CircuitOpen {
                name: self.plugin_name.clone(),
            }),
            CircuitState::Closed | CircuitState::HalfOpen => Ok(()),
        }
    }

    /// Record a successful call.
    ///
    /// In HalfOpen, success closes the circuit. In Closed, it resets the
    /// failure counter.
    pub fn record_success(&self) {
        let mut inner = self.inner.lock().expect("circuit breaker lock poisoned");
        inner.consecutive_failures = 0;
        inner.state = CircuitState::Closed;
        inner.opened_at = None;
    }

    /// Record a failed call.
    ///
    /// In HalfOpen, any failure re-opens the circuit. In Closed, failures
    /// accumulate; once the threshold is reached the circuit opens.
    pub fn record_failure(&self) {
        let mut inner = self.inner.lock().expect("circuit breaker lock poisoned");
        inner.consecutive_failures += 1;
        if inner.state == CircuitState::HalfOpen
            || inner.consecutive_failures >= self.failure_threshold
        {
            inner.state = CircuitState::Open;
            inner.opened_at = Some(Instant::now());
        }
    }

    /// Current consecutive failure count (for observability).
    pub fn consecutive_failures(&self) -> u32 {
        self.inner
            .lock()
            .expect("circuit breaker lock poisoned")
            .consecutive_failures
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn breaker() -> CircuitBreaker {
        CircuitBreaker::new("test-plugin", 3, Duration::from_millis(50))
    }

    #[test]
    fn starts_closed() {
        let cb = breaker();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.allow().is_ok());
    }

    #[test]
    fn opens_after_threshold() {
        let cb = breaker();
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed); // not yet
        cb.record_failure(); // 3rd failure — threshold reached
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(cb.allow().is_err());
    }

    #[test]
    fn success_resets_failure_counter() {
        let cb = breaker();
        cb.record_failure();
        cb.record_failure();
        cb.record_success();
        assert_eq!(cb.consecutive_failures(), 0);
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn half_open_after_reset_timeout() {
        let cb = CircuitBreaker::new("p", 1, Duration::from_millis(10));
        cb.record_failure(); // opens circuit
        assert_eq!(cb.state(), CircuitState::Open);
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(cb.state(), CircuitState::HalfOpen);
        assert!(cb.allow().is_ok());
    }

    #[test]
    fn half_open_success_closes_circuit() {
        let cb = CircuitBreaker::new("p", 1, Duration::from_millis(10));
        cb.record_failure();
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(cb.state(), CircuitState::HalfOpen);
        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn half_open_failure_reopens_circuit() {
        let cb = CircuitBreaker::new("p", 1, Duration::from_millis(10));
        cb.record_failure();
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(cb.state(), CircuitState::HalfOpen);
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[test]
    fn circuit_open_error_contains_plugin_name() {
        let cb = CircuitBreaker::new("named-plugin", 1, Duration::from_secs(60));
        cb.record_failure();
        let err = cb.allow().unwrap_err();
        assert!(matches!(err, PluginError::CircuitOpen { name } if name == "named-plugin"));
    }
}

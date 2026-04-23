//! Circuit Breaker Service
//!
//! Generic, reusable circuit breaker for protecting against cascading failures.
//! Implements the standard three-state pattern: Closed → Open → HalfOpen → Closed

use std::collections::VecDeque;
use std::future::Future;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Circuit breaker error type, parameterized over the inner error type.
#[derive(Debug, Clone)]
pub enum CircuitBreakerError<E> {
    /// Circuit is open; operation rejected.
    Open { name: String },
    /// Inner operation failed.
    Inner(E),
}

impl<E> CircuitBreakerError<E> {
    /// Extract the inner error if this is an `Inner` variant.
    pub fn inner(self) -> Option<E> {
        match self {
            CircuitBreakerError::Inner(e) => Some(e),
            CircuitBreakerError::Open { .. } => None,
        }
    }

    /// Map the inner error type.
    pub fn map<F, U>(self, f: F) -> CircuitBreakerError<U>
    where
        F: FnOnce(E) -> U,
    {
        match self {
            CircuitBreakerError::Inner(e) => CircuitBreakerError::Inner(f(e)),
            CircuitBreakerError::Open { name } => CircuitBreakerError::Open { name },
        }
    }
}

impl<E: std::fmt::Display> std::fmt::Display for CircuitBreakerError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CircuitBreakerError::Open { name } => write!(f, "circuit breaker {} is open", name),
            CircuitBreakerError::Inner(e) => write!(f, "{}", e),
        }
    }
}

impl<E: std::fmt::Display + std::fmt::Debug> std::error::Error for CircuitBreakerError<E> {}

/// Internal circuit breaker state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Closed: normal operation, recording failures.
    Closed,
    /// Open: rejecting requests, waiting for cooldown.
    Open,
    /// HalfOpen: allowing one request to test recovery.
    HalfOpen,
}

/// Failure record: timestamp (milliseconds since start) for windowing.
#[derive(Debug, Clone)]
struct FailureRecord {
    timestamp: u64,
}

/// Inner circuit breaker state.
struct CircuitBreakerState {
    name: String,
    state: State,
    failures: VecDeque<FailureRecord>,
    failure_threshold: u32,
    window_secs: u64,
    cooldown_secs: u64,
    opened_at: Option<u64>,
    half_open_attempt: bool,
}

impl CircuitBreakerState {
    /// Create a new circuit breaker state.
    fn new(name: String, failure_threshold: u32, window_secs: u64, cooldown_secs: u64) -> Self {
        Self {
            name,
            state: State::Closed,
            failures: VecDeque::new(),
            failure_threshold,
            window_secs,
            cooldown_secs,
            opened_at: None,
            half_open_attempt: false,
        }
    }

    /// Get the current time in milliseconds since the start of the process.
    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// Record a failure and return true if threshold is breached.
    fn record_failure(&mut self) -> bool {
        let now = Self::now_ms();
        let window_ms = self.window_secs * 1000;

        // Remove failures outside the window
        while let Some(front) = self.failures.front() {
            if now.saturating_sub(front.timestamp) > window_ms {
                self.failures.pop_front();
            } else {
                break;
            }
        }

        // Record the new failure
        self.failures.push_back(FailureRecord { timestamp: now });

        // Check if threshold is breached
        self.failures.len() >= self.failure_threshold as usize
    }

    /// Try to transition from Open to HalfOpen if cooldown has passed.
    fn try_half_open(&mut self) {
        if self.state != State::Open {
            return;
        }

        if let Some(opened_at) = self.opened_at {
            let now = Self::now_ms();
            let cooldown_ms = self.cooldown_secs * 1000;

            if now.saturating_sub(opened_at) >= cooldown_ms {
                self.state = State::HalfOpen;
                self.half_open_attempt = false;
            }
        }
    }

    /// Handle successful call: transition to Closed if in HalfOpen.
    fn on_success(&mut self) {
        if self.state == State::HalfOpen {
            self.state = State::Closed;
            self.failures.clear();
            self.opened_at = None;
        }
    }

    /// Handle failed call: open circuit if threshold breached, or reopen if in HalfOpen.
    fn on_failure(&mut self) {
        match self.state {
            State::Closed => {
                if self.record_failure() {
                    self.state = State::Open;
                    self.opened_at = Some(Self::now_ms());
                }
            }
            State::HalfOpen => {
                self.state = State::Open;
                self.opened_at = Some(Self::now_ms());
            }
            State::Open => {
                // Already open, don't need to do anything
            }
        }
    }
}

/// Generic, reusable circuit breaker service.
pub struct CircuitBreaker {
    state: Arc<Mutex<CircuitBreakerState>>,
}

impl CircuitBreaker {
    /// Create a new circuit breaker.
    ///
    /// # Arguments
    /// * `name` - Identifier for this circuit breaker
    /// * `failure_threshold` - Number of failures before opening
    /// * `window_secs` - Time window for counting failures
    /// * `cooldown_secs` - Time to wait before attempting recovery
    pub fn new(
        name: impl Into<String>,
        failure_threshold: u32,
        window_secs: u64,
        cooldown_secs: u64,
    ) -> Self {
        let state =
            CircuitBreakerState::new(name.into(), failure_threshold, window_secs, cooldown_secs);
        Self {
            state: Arc::new(Mutex::new(state)),
        }
    }

    /// Call a protected function with circuit breaker semantics.
    ///
    /// # Arguments
    /// * `f` - Async closure that returns `Result<T, E>`
    ///
    /// # Returns
    /// * `Ok(T)` if the call succeeded
    /// * `Err(CircuitBreakerError::Open { .. })` if the circuit is open
    /// * `Err(CircuitBreakerError::Inner(e))` if the call failed
    pub async fn call_protected<F, Fut, T, E>(&self, f: F) -> Result<T, CircuitBreakerError<E>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        // Check and potentially transition state
        {
            let mut state = self.state.lock().await;
            state.try_half_open();

            // Reject if circuit is open
            if state.state == State::Open {
                return Err(CircuitBreakerError::Open {
                    name: state.name.clone(),
                });
            }
        }

        // Execute the protected call
        match f().await {
            Ok(result) => {
                let mut state = self.state.lock().await;
                state.on_success();
                Ok(result)
            }
            Err(e) => {
                let mut state = self.state.lock().await;
                state.on_failure();
                Err(CircuitBreakerError::Inner(e))
            }
        }
    }

    /// Get the current state (for testing/monitoring).
    pub async fn current_state(&self) -> &'static str {
        let state = self.state.lock().await;
        match state.state {
            State::Closed => "Closed",
            State::Open => "Open",
            State::HalfOpen => "HalfOpen",
        }
    }
}

impl Clone for CircuitBreaker {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_circuit_breaker_closed_state() {
        let cb = CircuitBreaker::new("test", 3, 1, 1);
        assert_eq!(cb.current_state().await, "Closed");

        let result: Result<i32, CircuitBreakerError<String>> =
            cb.call_protected(|| async { Ok::<i32, String>(42) }).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_circuit_breaker_opens_on_threshold() {
        let cb = CircuitBreaker::new("test", 2, 1, 1);

        // First failure
        let result: Result<i32, CircuitBreakerError<String>> = cb
            .call_protected(|| async { Err::<i32, String>("error1".to_string()) })
            .await;
        assert!(matches!(result, Err(CircuitBreakerError::Inner(_))));

        // Second failure — opens the circuit
        let result: Result<i32, CircuitBreakerError<String>> = cb
            .call_protected(|| async { Err::<i32, String>("error2".to_string()) })
            .await;
        assert!(matches!(result, Err(CircuitBreakerError::Inner(_))));

        // Third call should be rejected (circuit open)
        let result: Result<i32, CircuitBreakerError<String>> =
            cb.call_protected(|| async { Ok::<i32, String>(42) }).await;
        assert!(matches!(result, Err(CircuitBreakerError::Open { .. })));
    }

    #[tokio::test]
    async fn test_circuit_breaker_half_open_recovery() {
        let cb = CircuitBreaker::new("test", 1, 1, 1);

        // Trigger failure to open
        let _: Result<i32, CircuitBreakerError<String>> = cb
            .call_protected(|| async { Err::<i32, String>("error".to_string()) })
            .await;

        // Should be open now
        let result: Result<i32, CircuitBreakerError<String>> =
            cb.call_protected(|| async { Ok::<i32, String>(42) }).await;
        assert!(matches!(result, Err(CircuitBreakerError::Open { .. })));

        // Wait for cooldown
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        // Should transition to HalfOpen and allow the call
        let result: Result<i32, CircuitBreakerError<String>> =
            cb.call_protected(|| async { Ok::<i32, String>(42) }).await;
        assert!(result.is_ok());

        // Should be back to closed
        assert_eq!(cb.current_state().await, "Closed");
    }

    #[tokio::test]
    async fn test_circuit_breaker_half_open_reopens_on_failure() {
        let cb = CircuitBreaker::new("test", 1, 1, 1);

        // Trigger failure to open
        let _: Result<i32, CircuitBreakerError<String>> = cb
            .call_protected(|| async { Err::<i32, String>("error".to_string()) })
            .await;

        // Wait for cooldown
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        // Transition to HalfOpen, but fail the recovery attempt
        let result: Result<i32, CircuitBreakerError<String>> = cb
            .call_protected(|| async { Err::<i32, String>("still broken".to_string()) })
            .await;
        assert!(matches!(result, Err(CircuitBreakerError::Inner(_))));

        // Should be open again
        let result: Result<i32, CircuitBreakerError<String>> =
            cb.call_protected(|| async { Ok::<i32, String>(42) }).await;
        assert!(matches!(result, Err(CircuitBreakerError::Open { .. })));
    }

    #[tokio::test]
    async fn test_circuit_breaker_window_expiry() {
        let cb = CircuitBreaker::new("test", 2, 1, 1);

        // Record a failure
        let _: Result<i32, CircuitBreakerError<String>> = cb
            .call_protected(|| async { Err::<i32, String>("error1".to_string()) })
            .await;

        // Wait for window to expire
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        // Failure count should reset, so this won't open the circuit
        let _: Result<i32, CircuitBreakerError<String>> = cb
            .call_protected(|| async { Err::<i32, String>("error2".to_string()) })
            .await;

        // Circuit should still be closed (only one failure within window)
        let result: Result<i32, CircuitBreakerError<String>> =
            cb.call_protected(|| async { Ok::<i32, String>(42) }).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_circuit_breaker_cloneable() {
        let cb = CircuitBreaker::new("test", 1, 1, 1);
        let cb_clone = cb.clone();

        // Trigger failure on original
        let _: Result<i32, CircuitBreakerError<String>> = cb
            .call_protected(|| async { Err::<i32, String>("error".to_string()) })
            .await;

        // Clone should see the same state
        let result: Result<i32, CircuitBreakerError<String>> = cb_clone
            .call_protected(|| async { Ok::<i32, String>(42) })
            .await;
        assert!(matches!(result, Err(CircuitBreakerError::Open { .. })));
    }
}

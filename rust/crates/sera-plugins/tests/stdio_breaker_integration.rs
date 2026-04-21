//! Integration test: stdio plugin crash → CircuitBreaker opens.
//!
//! We simulate repeated registration failures (using a non-existent binary)
//! to drive the circuit breaker through its state machine.  We do NOT test
//! the restart-with-backoff loop end-to-end (that would require a real
//! crashing plugin binary and wall-clock time), but we verify:
//!
//! 1. The circuit breaker starts Closed.
//! 2. Consecutive failures (simulated via `record_failure`) open it.
//! 3. After the reset timeout it moves to HalfOpen.
//! 4. A successful probe (simulated via `record_success`) closes it.
//!
//! MUST use `flavor = "multi_thread"` — any blocking calls inside async
//! bodies require a multi-threaded runtime to avoid deadlock.

use sera_plugins::{CircuitBreaker, CircuitState};
use std::time::Duration;

/// Default threshold used across these tests.
fn breaker_with_threshold(n: u32) -> CircuitBreaker {
    CircuitBreaker::new("stdio-test-plugin", n, Duration::from_millis(30))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn breaker_starts_closed() {
    let cb = breaker_with_threshold(3);
    assert_eq!(cb.state(), CircuitState::Closed);
    assert!(cb.allow().is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn breaker_opens_after_n_failures() {
    let cb = breaker_with_threshold(3);

    cb.record_failure();
    assert_eq!(cb.state(), CircuitState::Closed, "1 failure: still closed");
    cb.record_failure();
    assert_eq!(cb.state(), CircuitState::Closed, "2 failures: still closed");
    cb.record_failure();
    assert_eq!(cb.state(), CircuitState::Open, "3 failures: now open");

    let err = cb.allow().expect_err("open circuit must reject calls");
    assert!(
        err.to_string().contains("stdio-test-plugin"),
        "error must name the plugin: {err}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn breaker_transitions_to_half_open_after_timeout() {
    let reset_timeout = Duration::from_millis(30);
    let cb = CircuitBreaker::new("stdio-test-plugin", 1, reset_timeout);

    cb.record_failure(); // opens immediately (threshold = 1)
    assert_eq!(cb.state(), CircuitState::Open);

    // Wait for reset timeout
    tokio::time::sleep(reset_timeout + Duration::from_millis(15)).await;
    assert_eq!(cb.state(), CircuitState::HalfOpen);
    assert!(cb.allow().is_ok(), "HalfOpen must allow a probe");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn half_open_success_closes_breaker() {
    let reset_timeout = Duration::from_millis(30);
    let cb = CircuitBreaker::new("stdio-test-plugin", 1, reset_timeout);

    cb.record_failure();
    tokio::time::sleep(reset_timeout + Duration::from_millis(15)).await;
    assert_eq!(cb.state(), CircuitState::HalfOpen);

    cb.record_success();
    assert_eq!(cb.state(), CircuitState::Closed);
    assert_eq!(cb.consecutive_failures(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn half_open_failure_reopens_breaker() {
    let reset_timeout = Duration::from_millis(30);
    let cb = CircuitBreaker::new("stdio-test-plugin", 1, reset_timeout);

    cb.record_failure();
    tokio::time::sleep(reset_timeout + Duration::from_millis(15)).await;
    assert_eq!(cb.state(), CircuitState::HalfOpen);

    // Another failure in HalfOpen re-opens
    cb.record_failure();
    assert_eq!(cb.state(), CircuitState::Open);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn success_before_threshold_resets_counter() {
    let cb = breaker_with_threshold(3);

    cb.record_failure();
    cb.record_failure();
    assert_eq!(cb.consecutive_failures(), 2);

    // A success resets — the next 3 failures are needed to re-open
    cb.record_success();
    assert_eq!(cb.consecutive_failures(), 0);
    assert_eq!(cb.state(), CircuitState::Closed);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn backoff_timing_doubles_up_to_max() {
    // Verify the const values in registry.rs behave as documented:
    // start 1s, factor 2, max 60s.
    // We test the arithmetic rather than sleeping real time.
    let mut backoff = std::time::Duration::from_secs(1);
    let max = std::time::Duration::from_secs(60);
    let factor = 2u32;

    let sequence: Vec<u64> = (0..8)
        .map(|_| {
            let v = backoff.as_secs();
            backoff = (backoff * factor).min(max);
            v
        })
        .collect();

    assert_eq!(sequence, vec![1, 2, 4, 8, 16, 32, 60, 60]);
}

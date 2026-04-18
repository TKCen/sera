//! Integration tests for `CronSchedule` — `next_fire_after`, `is_valid`, `validate`.
//!
//! These run in a separate process (cargo integration test binary) so they exercise
//! the public API end-to-end without needing a database or network.

use chrono::{TimeZone, Utc};
use sera_workflow::{CronSchedule, WorkflowError};

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn schedule(expr: &str) -> CronSchedule {
    CronSchedule {
        expression: expr.to_string(),
    }
}

// ---------------------------------------------------------------------------
// 1. Valid expression fires strictly in the future
// ---------------------------------------------------------------------------

#[test]
fn valid_hourly_expression_fires_in_future() {
    // cron crate uses 6-field format: sec min hour dom mon dow
    let s = schedule("0 0 * * * *"); // top of every hour
    let reference = Utc.with_ymd_and_hms(2025, 6, 1, 12, 30, 0).unwrap();
    let next = s.next_fire_after(reference).expect("should produce a next fire time");
    assert!(
        next > reference,
        "next fire ({next}) must be strictly after reference ({reference})"
    );
}

// ---------------------------------------------------------------------------
// 2. Invalid expression returns WorkflowError::InvalidCronExpression
// ---------------------------------------------------------------------------

#[test]
fn invalid_expression_returns_error() {
    let s = schedule("not a cron expression");
    let result = s.validate();
    assert!(
        matches!(result, Err(WorkflowError::InvalidCronExpression { .. })),
        "expected InvalidCronExpression, got {result:?}"
    );
}

// ---------------------------------------------------------------------------
// 3. is_valid agrees with validate
// ---------------------------------------------------------------------------

#[test]
fn is_valid_agrees_with_validate() {
    // 6-field: sec min hour dom mon dow
    let valid = schedule("0 */5 * * * *"); // every 5 minutes
    assert!(valid.is_valid());
    assert!(valid.validate().is_ok());

    let invalid = schedule("not a cron expression");
    assert!(!invalid.is_valid());
    assert!(invalid.validate().is_err());
}

// ---------------------------------------------------------------------------
// 4. Minutely expression fires within 60 s of reference
// ---------------------------------------------------------------------------

#[test]
fn minutely_expression_fires_within_one_minute() {
    let s = schedule("0 * * * * *"); // every minute (6-field: sec min hour dom mon dow)
    let reference = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 30).unwrap(); // :30 s
    let next = s.next_fire_after(reference).expect("minutely must fire");
    let delta = (next - reference).num_seconds();
    assert!(
        delta > 0 && delta <= 60,
        "minutely schedule should fire within 60 s, got {delta} s"
    );
}

//! Persistent pending-count backend for [`LaneQueue`].
//!
//! [`LaneQueue`] tracks `pending_count` in-process (via `active_run_count` +
//! per-lane queue depths). This works fine for a single gateway pod but breaks
//! multi-instance deployments: each pod has an independent counter, so the
//! global admission-control invariant is violated.
//!
//! This module adds a [`LaneCounterStore`] trait and two implementations:
//!
//! * [`InMemoryLaneCounter`] — the original in-process behaviour, useful in
//!   tests and single-node deployments.
//! * [`PostgresLaneCounter`] — reads/writes to
//!   `lane_pending_counts(lane_id TEXT PRIMARY KEY, pending BIGINT)` so all
//!   pods share a consistent view of per-lane pending counts.
//!
//! The gateway wiring (replacing the in-memory counter with the Postgres one)
//! is a separate follow-up; this crate-only change is purely additive.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use sqlx::PgPool;

use crate::error::DbError;

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Atomic increment / decrement / snapshot of per-lane pending counts.
///
/// The `lane_id` parameter is an opaque string key that callers choose — in
/// practice it will be the session key used by [`crate::lane_queue::LaneQueue`].
///
/// All methods are `async` so that Postgres-backed implementations can issue
/// network I/O without blocking. In-memory implementations simply return
/// `async { Ok(...) }`.
pub trait LaneCounterStore: Send + Sync + 'static {
    /// Add `delta` to the pending count for `lane_id`.
    ///
    /// The count must never go below zero; implementations should use
    /// saturating semantics (i.e. GREATEST(pending + delta, 0) in SQL).
    fn increment<'a>(
        &'a self,
        lane_id: &'a str,
        delta: i64,
    ) -> impl std::future::Future<Output = Result<(), DbError>> + Send + 'a;

    /// Subtract `delta` from the pending count for `lane_id`.
    ///
    /// Saturates at zero — the count is never allowed to go negative.
    fn decrement<'a>(
        &'a self,
        lane_id: &'a str,
        delta: i64,
    ) -> impl std::future::Future<Output = Result<(), DbError>> + Send + 'a;

    /// Return the current pending count for `lane_id`.
    ///
    /// Returns `0` for unknown lane IDs (no row has ever been written for them).
    fn snapshot<'a>(
        &'a self,
        lane_id: &'a str,
    ) -> impl std::future::Future<Output = Result<i64, DbError>> + Send + 'a;
}

// ---------------------------------------------------------------------------
// In-memory implementation
// ---------------------------------------------------------------------------

/// In-process [`LaneCounterStore`].
///
/// Thread-safe via a `Mutex`-protected `HashMap`.  Suitable for single-node
/// deployments and unit tests.
#[derive(Debug, Default, Clone)]
pub struct InMemoryLaneCounter {
    counts: Arc<Mutex<HashMap<String, i64>>>,
}

impl InMemoryLaneCounter {
    /// Create a new, empty counter.
    pub fn new() -> Self {
        Self::default()
    }
}

impl LaneCounterStore for InMemoryLaneCounter {
    async fn increment(&self, lane_id: &str, delta: i64) -> Result<(), DbError> {
        let mut map = self.counts.lock().expect("InMemoryLaneCounter mutex poisoned");
        let entry = map.entry(lane_id.to_string()).or_insert(0);
        *entry = entry.saturating_add(delta);
        Ok(())
    }

    async fn decrement(&self, lane_id: &str, delta: i64) -> Result<(), DbError> {
        let mut map = self.counts.lock().expect("InMemoryLaneCounter mutex poisoned");
        let entry = map.entry(lane_id.to_string()).or_insert(0);
        *entry = (*entry - delta).max(0);
        Ok(())
    }

    async fn snapshot(&self, lane_id: &str) -> Result<i64, DbError> {
        let map = self.counts.lock().expect("InMemoryLaneCounter mutex poisoned");
        Ok(map.get(lane_id).copied().unwrap_or(0))
    }
}

// ---------------------------------------------------------------------------
// Postgres implementation
// ---------------------------------------------------------------------------

/// Postgres-backed [`LaneCounterStore`].
///
/// Uses the `lane_pending_counts` table:
///
/// ```sql
/// CREATE TABLE lane_pending_counts (
///     lane_id TEXT    PRIMARY KEY,
///     pending BIGINT  NOT NULL DEFAULT 0
/// );
/// ```
///
/// All mutations use `INSERT … ON CONFLICT … DO UPDATE` (UPSERT) semantics so
/// the row is created on first touch without requiring an explicit seed step.
/// Decrements clamp at zero with `GREATEST(pending - $2, 0)`.
#[derive(Clone, Debug)]
pub struct PostgresLaneCounter {
    pool: PgPool,
}

impl PostgresLaneCounter {
    /// Wrap an existing [`PgPool`].
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl LaneCounterStore for PostgresLaneCounter {
    async fn increment(&self, lane_id: &str, delta: i64) -> Result<(), DbError> {
        sqlx::query(
            r#"
            INSERT INTO lane_pending_counts (lane_id, pending)
            VALUES ($1, $2)
            ON CONFLICT (lane_id) DO UPDATE
              SET pending = lane_pending_counts.pending + EXCLUDED.pending
            "#,
        )
        .bind(lane_id)
        .bind(delta)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn decrement(&self, lane_id: &str, delta: i64) -> Result<(), DbError> {
        sqlx::query(
            r#"
            INSERT INTO lane_pending_counts (lane_id, pending)
            VALUES ($1, 0)
            ON CONFLICT (lane_id) DO UPDATE
              SET pending = GREATEST(lane_pending_counts.pending - $2, 0)
            "#,
        )
        .bind(lane_id)
        .bind(delta)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn snapshot(&self, lane_id: &str) -> Result<i64, DbError> {
        let row: Option<(i64,)> =
            sqlx::query_as("SELECT pending FROM lane_pending_counts WHERE lane_id = $1")
                .bind(lane_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(v,)| v).unwrap_or(0))
    }
}

// ---------------------------------------------------------------------------
// Unit tests (in-memory only — no DB required)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- basic increment / decrement / snapshot ---------------------------

    #[tokio::test]
    async fn increment_creates_entry_from_zero() {
        let c = InMemoryLaneCounter::new();
        c.increment("lane-a", 1).await.unwrap();
        assert_eq!(c.snapshot("lane-a").await.unwrap(), 1);
    }

    #[tokio::test]
    async fn snapshot_unknown_lane_returns_zero() {
        let c = InMemoryLaneCounter::new();
        assert_eq!(c.snapshot("no-such-lane").await.unwrap(), 0);
    }

    #[tokio::test]
    async fn increment_accumulates() {
        let c = InMemoryLaneCounter::new();
        c.increment("lane-a", 3).await.unwrap();
        c.increment("lane-a", 2).await.unwrap();
        assert_eq!(c.snapshot("lane-a").await.unwrap(), 5);
    }

    #[tokio::test]
    async fn decrement_reduces_count() {
        let c = InMemoryLaneCounter::new();
        c.increment("lane-a", 5).await.unwrap();
        c.decrement("lane-a", 2).await.unwrap();
        assert_eq!(c.snapshot("lane-a").await.unwrap(), 3);
    }

    #[tokio::test]
    async fn decrement_saturates_at_zero() {
        let c = InMemoryLaneCounter::new();
        c.increment("lane-a", 2).await.unwrap();
        c.decrement("lane-a", 10).await.unwrap(); // would underflow → clamp at 0
        assert_eq!(c.snapshot("lane-a").await.unwrap(), 0);
    }

    #[tokio::test]
    async fn decrement_on_unknown_lane_is_zero() {
        let c = InMemoryLaneCounter::new();
        c.decrement("lane-a", 1).await.unwrap(); // no prior entry
        assert_eq!(c.snapshot("lane-a").await.unwrap(), 0);
    }

    #[tokio::test]
    async fn lanes_are_independent() {
        let c = InMemoryLaneCounter::new();
        c.increment("lane-a", 3).await.unwrap();
        c.increment("lane-b", 7).await.unwrap();
        assert_eq!(c.snapshot("lane-a").await.unwrap(), 3);
        assert_eq!(c.snapshot("lane-b").await.unwrap(), 7);
        c.decrement("lane-a", 1).await.unwrap();
        assert_eq!(c.snapshot("lane-a").await.unwrap(), 2);
        assert_eq!(c.snapshot("lane-b").await.unwrap(), 7);
    }

    // ---- concurrent correctness -------------------------------------------

    #[tokio::test]
    async fn concurrent_increments_produce_correct_total() {
        use std::sync::Arc;
        use tokio::task::JoinSet;

        const N: i64 = 50;
        let c = Arc::new(InMemoryLaneCounter::new());

        let mut set = JoinSet::new();
        for _ in 0..N {
            let c = Arc::clone(&c);
            set.spawn(async move { c.increment("lane-concurrent", 1).await.unwrap() });
        }
        while let Some(r) = set.join_next().await {
            r.unwrap();
        }

        assert_eq!(c.snapshot("lane-concurrent").await.unwrap(), N);
    }

    #[tokio::test]
    async fn concurrent_decrements_never_go_negative() {
        use std::sync::Arc;
        use tokio::task::JoinSet;

        const N: i64 = 20;
        let c = Arc::new(InMemoryLaneCounter::new());
        // Seed with 10 — decrements will saturate at 0 for the remainder.
        c.increment("lane-neg", 10).await.unwrap();

        let mut set = JoinSet::new();
        for _ in 0..N {
            let c = Arc::clone(&c);
            set.spawn(async move { c.decrement("lane-neg", 1).await.unwrap() });
        }
        while let Some(r) = set.join_next().await {
            r.unwrap();
        }

        // Must be exactly 0 — never negative.
        assert_eq!(c.snapshot("lane-neg").await.unwrap(), 0);
    }
}

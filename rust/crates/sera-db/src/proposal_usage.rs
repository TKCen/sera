//! Persistent proposal-usage counters — backs the `/api/evolve/propose` quota.
//!
//! The `ProposalUsageStore` trait mirrors the `ProposalUsageTracker` shape but
//! is async so both an in-memory backend (for tests) and a Postgres backend
//! (for production) can satisfy it.
//!
//! # Table shape
//!
//! ```sql
//! CREATE TABLE proposal_usage (
//!     token_id    TEXT    NOT NULL,
//!     used        BIGINT  NOT NULL DEFAULT 0,
//!     updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
//!     PRIMARY KEY (token_id)
//! );
//! ```
//!
//! A single row per `token_id` is maintained with an atomic
//! `INSERT … ON CONFLICT DO UPDATE` so `check_and_increment` is safe under
//! concurrent gateway instances.

use std::collections::HashMap;
use std::sync::Mutex;

use sqlx::PgPool;

use crate::error::DbError;

// ── Trait ──────────────────────────────────────────────────────────────────

/// Async proposal-usage store.
///
/// Both backends satisfy this trait so callers (route layer, AppState) can
/// depend on the trait object rather than a concrete type.
#[async_trait::async_trait]
pub trait ProposalUsageStore: Send + Sync + std::fmt::Debug {
    /// Atomically check whether `token_id` has consumed fewer than
    /// `max_proposals` proposals and, if so, increment the counter.
    ///
    /// Returns `Ok(new_count)` — the post-increment value — when budget
    /// remains.  Returns `Err(DbError::QuotaExceeded { … })` when
    /// `used >= max_proposals`; the counter is **not** incremented in that
    /// case.
    async fn check_and_increment(
        &self,
        token_id: &str,
        max_proposals: u32,
    ) -> Result<u64, DbError>;

    /// Return the current usage count for `token_id`, or `0` if unseen.
    async fn current_count(&self, token_id: &str) -> Result<u64, DbError>;

    /// Reset the counter for `token_id` to zero (removes the row).
    ///
    /// Primarily for test isolation and token-reissuance flows.
    async fn reset(&self, token_id: &str) -> Result<(), DbError>;
}

// ── In-memory backend ──────────────────────────────────────────────────────

/// In-memory proposal-usage store.
///
/// Uses `std::sync::Mutex` — all critical sections are short (HashMap ops,
/// no `.await`).  Suitable for unit tests and for running without Postgres.
#[derive(Debug, Default)]
pub struct InMemoryProposalUsageStore {
    counts: Mutex<HashMap<String, u64>>,
}

impl InMemoryProposalUsageStore {
    /// Create a new, empty store.
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait::async_trait]
impl ProposalUsageStore for InMemoryProposalUsageStore {
    async fn check_and_increment(
        &self,
        token_id: &str,
        max_proposals: u32,
    ) -> Result<u64, DbError> {
        let mut counts = self.counts.lock().expect("proposal_usage mutex poisoned");
        let used = counts.entry(token_id.to_string()).or_insert(0);
        if *used >= u64::from(max_proposals) {
            return Err(DbError::QuotaExceeded {
                token_id: token_id.to_string(),
                limit: max_proposals,
            });
        }
        *used += 1;
        Ok(*used)
    }

    async fn current_count(&self, token_id: &str) -> Result<u64, DbError> {
        let counts = self.counts.lock().expect("proposal_usage mutex poisoned");
        Ok(counts.get(token_id).copied().unwrap_or(0))
    }

    async fn reset(&self, token_id: &str) -> Result<(), DbError> {
        let mut counts = self.counts.lock().expect("proposal_usage mutex poisoned");
        counts.remove(token_id);
        Ok(())
    }
}

// ── Postgres backend ───────────────────────────────────────────────────────

/// Postgres-backed proposal-usage store.
///
/// Persists counters in the `proposal_usage` table so they survive gateway
/// restarts, closing the in-memory quota bypass documented in the original
/// `ProposalUsageTracker`.
///
/// The DDL required:
/// ```sql
/// CREATE TABLE IF NOT EXISTS proposal_usage (
///     token_id   TEXT        NOT NULL PRIMARY KEY,
///     used       BIGINT      NOT NULL DEFAULT 0,
///     updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
/// );
/// ```
#[derive(Debug, Clone)]
pub struct PostgresProposalUsageStore {
    pool: PgPool,
}

impl PostgresProposalUsageStore {
    /// Create a store backed by `pool`.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Create from a [`crate::DbPool`] wrapper.
    pub fn from_db_pool(db: &crate::DbPool) -> Self {
        Self::new(db.inner().clone())
    }
}

#[async_trait::async_trait]
impl ProposalUsageStore for PostgresProposalUsageStore {
    async fn check_and_increment(
        &self,
        token_id: &str,
        max_proposals: u32,
    ) -> Result<u64, DbError> {
        // Atomic upsert: insert row with used=1 or increment existing row,
        // but only when the current value is less than max_proposals.
        // The WHERE clause on DO UPDATE means no row is returned when the
        // quota is already reached — we detect that as QuotaExceeded.
        let row: Option<(i64,)> = sqlx::query_as(
            r#"
            INSERT INTO proposal_usage (token_id, used, updated_at)
            VALUES ($1, 1, NOW())
            ON CONFLICT (token_id) DO UPDATE
                SET used       = proposal_usage.used + 1,
                    updated_at = NOW()
            WHERE proposal_usage.used < $2
            RETURNING used
            "#,
        )
        .bind(token_id)
        .bind(i64::from(max_proposals))
        .fetch_optional(&self.pool)
        .await
        .map_err(DbError::Sqlx)?;

        match row {
            Some((used,)) => Ok(used as u64),
            None => {
                // The WHERE clause blocked the update — quota already reached.
                Err(DbError::QuotaExceeded {
                    token_id: token_id.to_string(),
                    limit: max_proposals,
                })
            }
        }
    }

    async fn current_count(&self, token_id: &str) -> Result<u64, DbError> {
        let row: Option<(i64,)> = sqlx::query_as(
            "SELECT used FROM proposal_usage WHERE token_id = $1",
        )
        .bind(token_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(DbError::Sqlx)?;

        Ok(row.map(|(used,)| used as u64).unwrap_or(0))
    }

    async fn reset(&self, token_id: &str) -> Result<(), DbError> {
        sqlx::query("DELETE FROM proposal_usage WHERE token_id = $1")
            .bind(token_id)
            .execute(&self.pool)
            .await
            .map_err(DbError::Sqlx)?;
        Ok(())
    }
}

// ── Unit tests (in-memory backend) ────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn increment_first_use_returns_one() {
        let store = InMemoryProposalUsageStore::new();
        let result = store.check_and_increment("tok-a", 5).await;
        assert_eq!(result.unwrap(), 1);
    }

    #[tokio::test]
    async fn increment_up_to_limit_accepted() {
        let store = InMemoryProposalUsageStore::new();
        for expected in 1u64..=3 {
            let count = store.check_and_increment("tok-b", 3).await.unwrap();
            assert_eq!(count, expected);
        }
    }

    #[tokio::test]
    async fn increment_beyond_limit_returns_quota_error() {
        let store = InMemoryProposalUsageStore::new();
        store.check_and_increment("tok-c", 2).await.unwrap();
        store.check_and_increment("tok-c", 2).await.unwrap();
        let err = store.check_and_increment("tok-c", 2).await.unwrap_err();
        match err {
            DbError::QuotaExceeded { token_id, limit } => {
                assert_eq!(token_id, "tok-c");
                assert_eq!(limit, 2);
            }
            other => panic!("expected QuotaExceeded, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn current_count_returns_zero_for_unseen_token() {
        let store = InMemoryProposalUsageStore::new();
        assert_eq!(store.current_count("tok-never").await.unwrap(), 0);
    }

    #[tokio::test]
    async fn current_count_matches_increments() {
        let store = InMemoryProposalUsageStore::new();
        store.check_and_increment("tok-d", 10).await.unwrap();
        store.check_and_increment("tok-d", 10).await.unwrap();
        assert_eq!(store.current_count("tok-d").await.unwrap(), 2);
    }

    #[tokio::test]
    async fn reset_clears_counter() {
        let store = InMemoryProposalUsageStore::new();
        store.check_and_increment("tok-e", 1).await.unwrap();
        // Exhausted
        store.check_and_increment("tok-e", 1).await.unwrap_err();
        // Reset
        store.reset("tok-e").await.unwrap();
        // Should succeed again
        let count = store.check_and_increment("tok-e", 1).await.unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn different_token_ids_have_independent_counters() {
        let store = InMemoryProposalUsageStore::new();
        store.check_and_increment("tok-x", 1).await.unwrap();
        store.check_and_increment("tok-x", 1).await.unwrap_err();
        // tok-y is still fresh
        assert!(store.check_and_increment("tok-y", 1).await.is_ok());
    }

    #[tokio::test]
    async fn counter_survives_new_handle_to_same_store() {
        // Simulates restart-safety at the in-memory level (shared Arc).
        use std::sync::Arc;
        let store = Arc::new(InMemoryProposalUsageStore::new());
        store.check_and_increment("tok-f", 3).await.unwrap();
        store.check_and_increment("tok-f", 3).await.unwrap();

        // Clone the Arc (same underlying store, not a new one).
        let store2 = Arc::clone(&store);
        assert_eq!(store2.current_count("tok-f").await.unwrap(), 2);
    }
}

//! Integration tests for PostgresProposalUsageStore.
//!
//! Requires a running Postgres instance:
//!   DATABASE_URL=postgres://... cargo test -p sera-db --features integration

#![cfg(feature = "integration")]

use crate::TestDb;
use sera_db::proposal_usage::{PostgresProposalUsageStore, ProposalUsageStore};

/// Run the proposal_usage DDL in the test schema.
async fn create_table(pool: &sqlx::PgPool, schema: &str) {
    let ddl = format!(
        r#"
        CREATE TABLE IF NOT EXISTS "{schema}".proposal_usage (
            token_id   TEXT        NOT NULL PRIMARY KEY,
            used       BIGINT      NOT NULL DEFAULT 0,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )
        "#
    );
    sqlx::query(&ddl)
        .execute(pool)
        .await
        .expect("Failed to create proposal_usage table");
}

// ---------------------------------------------------------------------------

#[tokio::test]
async fn pg_increment_first_use_returns_one() {
    let Some(db) = TestDb::new().await else { return };
    create_table(&db.pool, &db.schema).await;
    let store = PostgresProposalUsageStore::new(db.pool.clone());

    let count = store.check_and_increment("tok-pg-a", 5).await.unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn pg_increment_up_to_limit_accepted() {
    let Some(db) = TestDb::new().await else { return };
    create_table(&db.pool, &db.schema).await;
    let store = PostgresProposalUsageStore::new(db.pool.clone());

    for expected in 1u64..=3 {
        let count = store.check_and_increment("tok-pg-b", 3).await.unwrap();
        assert_eq!(count, expected);
    }
}

#[tokio::test]
async fn pg_increment_beyond_limit_returns_quota_error() {
    let Some(db) = TestDb::new().await else { return };
    create_table(&db.pool, &db.schema).await;
    let store = PostgresProposalUsageStore::new(db.pool.clone());

    store.check_and_increment("tok-pg-c", 2).await.unwrap();
    store.check_and_increment("tok-pg-c", 2).await.unwrap();

    let err = store.check_and_increment("tok-pg-c", 2).await.unwrap_err();
    match err {
        sera_db::DbError::QuotaExceeded { token_id, limit } => {
            assert_eq!(token_id, "tok-pg-c");
            assert_eq!(limit, 2);
        }
        other => panic!("expected QuotaExceeded, got {other:?}"),
    }
}

#[tokio::test]
async fn pg_current_count_zero_for_unseen_token() {
    let Some(db) = TestDb::new().await else { return };
    create_table(&db.pool, &db.schema).await;
    let store = PostgresProposalUsageStore::new(db.pool.clone());

    assert_eq!(store.current_count("tok-pg-never").await.unwrap(), 0);
}

#[tokio::test]
async fn pg_current_count_matches_increments() {
    let Some(db) = TestDb::new().await else { return };
    create_table(&db.pool, &db.schema).await;
    let store = PostgresProposalUsageStore::new(db.pool.clone());

    store.check_and_increment("tok-pg-d", 10).await.unwrap();
    store.check_and_increment("tok-pg-d", 10).await.unwrap();
    assert_eq!(store.current_count("tok-pg-d").await.unwrap(), 2);
}

#[tokio::test]
async fn pg_reset_clears_counter() {
    let Some(db) = TestDb::new().await else { return };
    create_table(&db.pool, &db.schema).await;
    let store = PostgresProposalUsageStore::new(db.pool.clone());

    store.check_and_increment("tok-pg-e", 1).await.unwrap();
    // Exhausted
    store.check_and_increment("tok-pg-e", 1).await.unwrap_err();
    // Reset
    store.reset("tok-pg-e").await.unwrap();
    // Should succeed again
    let count = store.check_and_increment("tok-pg-e", 1).await.unwrap();
    assert_eq!(count, 1);
}

/// Restart-safety test: create one store instance, increment, drop it, create
/// a fresh instance on the same pool and confirm the count persisted.
#[tokio::test]
async fn pg_counter_survives_fresh_store_instance() {
    let Some(db) = TestDb::new().await else { return };
    create_table(&db.pool, &db.schema).await;

    // First "gateway instance"
    {
        let store = PostgresProposalUsageStore::new(db.pool.clone());
        store.check_and_increment("tok-pg-restart", 10).await.unwrap();
        store.check_and_increment("tok-pg-restart", 10).await.unwrap();
    }

    // Second "gateway instance" — same pool, brand-new struct
    let store2 = PostgresProposalUsageStore::new(db.pool.clone());
    assert_eq!(
        store2.current_count("tok-pg-restart").await.unwrap(),
        2,
        "count should survive across store instances (simulates gateway restart)"
    );
    // And quota enforcement carries over
    for _ in 0..8 {
        store2.check_and_increment("tok-pg-restart", 10).await.unwrap();
    }
    let err = store2.check_and_increment("tok-pg-restart", 10).await.unwrap_err();
    assert!(matches!(err, sera_db::DbError::QuotaExceeded { .. }));
}

//! Integration tests for [`PostgresLaneCounter`].
//!
//! These tests require a live Postgres instance:
//!
//!   DATABASE_URL=postgres://sera:sera@localhost/sera \
//!     cargo test -p sera-db --features integration

#![cfg(feature = "integration")]

use crate::TestDb;
use sera_db::lane_queue_counter::{LaneCounterStore, PostgresLaneCounter};

// ---- basic CRUD -----------------------------------------------------------

#[tokio::test]
async fn increment_creates_row_from_zero() {
    let Some(db) = TestDb::new().await else { return };
    let c = PostgresLaneCounter::new(db.pool.clone());

    c.increment("lane-a", 1).await.unwrap();
    assert_eq!(c.snapshot("lane-a").await.unwrap(), 1);
}

#[tokio::test]
async fn snapshot_unknown_lane_returns_zero() {
    let Some(db) = TestDb::new().await else { return };
    let c = PostgresLaneCounter::new(db.pool.clone());

    assert_eq!(c.snapshot("no-such-lane").await.unwrap(), 0);
}

#[tokio::test]
async fn increment_accumulates_across_calls() {
    let Some(db) = TestDb::new().await else { return };
    let c = PostgresLaneCounter::new(db.pool.clone());

    c.increment("lane-acc", 3).await.unwrap();
    c.increment("lane-acc", 4).await.unwrap();
    assert_eq!(c.snapshot("lane-acc").await.unwrap(), 7);
}

#[tokio::test]
async fn decrement_reduces_count() {
    let Some(db) = TestDb::new().await else { return };
    let c = PostgresLaneCounter::new(db.pool.clone());

    c.increment("lane-dec", 5).await.unwrap();
    c.decrement("lane-dec", 2).await.unwrap();
    assert_eq!(c.snapshot("lane-dec").await.unwrap(), 3);
}

#[tokio::test]
async fn decrement_saturates_at_zero() {
    let Some(db) = TestDb::new().await else { return };
    let c = PostgresLaneCounter::new(db.pool.clone());

    c.increment("lane-sat", 2).await.unwrap();
    c.decrement("lane-sat", 99).await.unwrap(); // would underflow → clamped by GREATEST
    assert_eq!(c.snapshot("lane-sat").await.unwrap(), 0);
}

#[tokio::test]
async fn decrement_on_unknown_lane_produces_zero() {
    let Some(db) = TestDb::new().await else { return };
    let c = PostgresLaneCounter::new(db.pool.clone());

    c.decrement("lane-unknown", 5).await.unwrap();
    assert_eq!(c.snapshot("lane-unknown").await.unwrap(), 0);
}

#[tokio::test]
async fn lanes_are_independent() {
    let Some(db) = TestDb::new().await else { return };
    let c = PostgresLaneCounter::new(db.pool.clone());

    c.increment("lane-x", 10).await.unwrap();
    c.increment("lane-y", 3).await.unwrap();
    assert_eq!(c.snapshot("lane-x").await.unwrap(), 10);
    assert_eq!(c.snapshot("lane-y").await.unwrap(), 3);

    c.decrement("lane-x", 4).await.unwrap();
    assert_eq!(c.snapshot("lane-x").await.unwrap(), 6);
    assert_eq!(c.snapshot("lane-y").await.unwrap(), 3);
}

// ---- concurrent correctness -----------------------------------------------

#[tokio::test]
async fn concurrent_increments_produce_correct_total() {
    let Some(db) = TestDb::new().await else { return };
    let c = std::sync::Arc::new(PostgresLaneCounter::new(db.pool.clone()));

    const N: i64 = 20;
    let mut handles = Vec::new();
    for _ in 0..N {
        let c = std::sync::Arc::clone(&c);
        handles.push(tokio::spawn(async move {
            c.increment("lane-concurrent", 1).await.unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }

    assert_eq!(c.snapshot("lane-concurrent").await.unwrap(), N);
}

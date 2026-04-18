//! Integration tests for AuditRepository.

#![cfg(feature = "integration")]

use crate::TestDb;
use sera_db::audit::AuditRepository;

fn payload() -> serde_json::Value {
    serde_json::json!({"key": "value"})
}

async fn append_event(db: &TestDb, actor_id: &str, event_type: &str, seq: u32) -> i64 {
    // Build a trivially unique hash so the NOT NULL constraint is satisfied.
    let hash = format!("hash-{actor_id}-{event_type}-{seq}");
    AuditRepository::append(
        &db.pool,
        "user",
        actor_id,
        None,
        event_type,
        &payload(),
        &hash,
        None,
    )
    .await
    .expect("append failed")
}

// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_append_and_get_latest() {
    let Some(db) = TestDb::new().await else { return };

    let seq1 = append_event(&db, "actor-1", "session.created", 1).await;
    let seq2 = append_event(&db, "actor-1", "session.closed", 2).await;
    assert!(seq2 > seq1, "sequence should increment");

    let latest = AuditRepository::get_latest(&db.pool)
        .await
        .expect("get_latest failed")
        .expect("expected a row");
    assert_eq!(latest.sequence, seq2);
    assert_eq!(latest.event_type, "session.closed");
}

#[tokio::test]
async fn test_get_latest_empty_returns_none() {
    let Some(db) = TestDb::new().await else { return };

    // Fresh schema — no rows yet.
    let result = AuditRepository::get_latest(&db.pool)
        .await
        .expect("get_latest failed");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_entries_with_actor_filter() {
    let Some(db) = TestDb::new().await else { return };

    append_event(&db, "actor-a", "ev.one", 1).await;
    append_event(&db, "actor-a", "ev.two", 2).await;
    append_event(&db, "actor-b", "ev.one", 3).await;

    let rows = AuditRepository::get_entries(&db.pool, Some("actor-a"), None, 100, 0)
        .await
        .expect("get_entries failed");

    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r.actor_id == "actor-a"));
}

#[tokio::test]
async fn test_get_entries_with_event_type_filter() {
    let Some(db) = TestDb::new().await else { return };

    append_event(&db, "actor-x", "type.alpha", 1).await;
    append_event(&db, "actor-x", "type.beta", 2).await;
    append_event(&db, "actor-y", "type.alpha", 3).await;

    let rows = AuditRepository::get_entries(&db.pool, None, Some("type.alpha"), 100, 0)
        .await
        .expect("get_entries filtered by event_type failed");

    assert!(rows.len() >= 2);
    assert!(rows.iter().all(|r| r.event_type == "type.alpha"));
}

#[tokio::test]
async fn test_count_entries() {
    let Some(db) = TestDb::new().await else { return };

    let actor = format!("counter-actor-{}", uuid::Uuid::new_v4().simple());
    for i in 0..4u32 {
        append_event(&db, &actor, "counted.event", i).await;
    }

    let count = AuditRepository::count_entries(&db.pool, Some(&actor), None)
        .await
        .expect("count_entries failed");
    assert_eq!(count, 4);
}

#[tokio::test]
async fn test_get_chain_for_verification() {
    let Some(db) = TestDb::new().await else { return };

    for i in 0..5u32 {
        append_event(&db, "chain-actor", "chain.event", i).await;
    }

    let chain = AuditRepository::get_chain_for_verification(&db.pool, 3)
        .await
        .expect("get_chain_for_verification failed");

    assert_eq!(chain.len(), 3);
    // Returned in ASC order — earlier sequence first.
    assert!(chain[0].sequence <= chain[1].sequence);
    assert!(chain[1].sequence <= chain[2].sequence);
}

//! Integration tests for SessionRepository.

#![cfg(feature = "integration")]

use crate::TestDb;
use sera_db::sessions::SessionRepository;
use uuid::Uuid;

/// Helper: create a unique session and return its id string.
async fn create_session(db: &TestDb, agent: &str, title: Option<&str>) -> String {
    let id = Uuid::new_v4().to_string();
    SessionRepository::create(&db.pool, &id, agent, title)
        .await
        .expect("create session failed");
    id
}

// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_and_get_by_id() {
    let Some(db) = TestDb::new().await else { return };

    let id = create_session(&db, "test-agent", Some("My Chat")).await;
    let row = SessionRepository::get_by_id(&db.pool, &id)
        .await
        .expect("get_by_id failed");

    assert_eq!(row.agent_name, "test-agent");
    assert_eq!(row.title, "My Chat");
}

#[tokio::test]
async fn test_create_uses_default_title() {
    let Some(db) = TestDb::new().await else { return };

    let id = create_session(&db, "agent-x", None).await;
    let row = SessionRepository::get_by_id(&db.pool, &id)
        .await
        .expect("get_by_id failed");

    assert_eq!(row.title, "New Chat");
}

#[tokio::test]
async fn test_list_sessions_all_and_filtered() {
    let Some(db) = TestDb::new().await else { return };

    create_session(&db, "alpha", Some("Alpha session")).await;
    create_session(&db, "beta", Some("Beta session")).await;

    let all = SessionRepository::list_sessions(&db.pool, None)
        .await
        .expect("list all failed");
    assert!(all.len() >= 2);

    let alpha_only = SessionRepository::list_sessions(&db.pool, Some("alpha"))
        .await
        .expect("list filtered failed");
    assert!(alpha_only.iter().all(|r| r.agent_name == "alpha"));
    assert!(!alpha_only.is_empty());
}

#[tokio::test]
async fn test_update_title() {
    let Some(db) = TestDb::new().await else { return };

    let id = create_session(&db, "agent-u", Some("Old Title")).await;
    let updated = SessionRepository::update_title(&db.pool, &id, "New Title")
        .await
        .expect("update_title failed");

    assert_eq!(updated.title, "New Title");
}

#[tokio::test]
async fn test_update_title_not_found() {
    let Some(db) = TestDb::new().await else { return };

    let missing = Uuid::new_v4().to_string();
    let err = SessionRepository::update_title(&db.pool, &missing, "X")
        .await
        .unwrap_err();

    let msg = err.to_string();
    assert!(msg.contains("not found"), "expected not-found, got: {msg}");
}

#[tokio::test]
async fn test_delete_session() {
    let Some(db) = TestDb::new().await else { return };

    let id = create_session(&db, "agent-d", Some("To delete")).await;
    let deleted = SessionRepository::delete(&db.pool, &id)
        .await
        .expect("delete failed");
    assert!(deleted);

    let err = SessionRepository::get_by_id(&db.pool, &id)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("not found"));
}

#[tokio::test]
async fn test_get_by_id_not_found() {
    let Some(db) = TestDb::new().await else { return };

    let missing = Uuid::new_v4().to_string();
    let err = SessionRepository::get_by_id(&db.pool, &missing)
        .await
        .unwrap_err();

    assert!(err.to_string().contains("not found"));
}

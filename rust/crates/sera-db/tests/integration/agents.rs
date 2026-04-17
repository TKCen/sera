//! Integration tests for AgentRepository.

#![cfg(feature = "integration")]

use crate::TestDb;
use sera_db::agents::AgentRepository;
use uuid::Uuid;

async fn insert_template(db: &TestDb, name: &str) {
    sqlx::query(
        "INSERT INTO agent_templates (name, display_name, builtin, spec) \
         VALUES ($1, $2, false, '{}'::jsonb)",
    )
    .bind(name)
    .bind(format!("{name} display"))
    .execute(&db.pool)
    .await
    .expect("insert template failed");
}

async fn insert_instance(db: &TestDb, id: &str, name: &str, template: &str) {
    AgentRepository::create_instance(
        &db.pool,
        sera_db::agents::CreateInstanceInput {
            id,
            name,
            template_name: template,
            template_ref: template,
            workspace_path: "/workspace",
            display_name: None,
            circle: None,
            lifecycle_mode: None,
        },
    )
    .await
    .expect("create_instance failed");
}

// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_list_and_get_template() {
    let Some(db) = TestDb::new().await else { return };

    insert_template(&db, "tpl-list-test").await;
    let templates = AgentRepository::list_templates(&db.pool)
        .await
        .expect("list_templates failed");
    assert!(templates.iter().any(|t| t.name == "tpl-list-test"));

    let tpl = AgentRepository::get_template(&db.pool, "tpl-list-test")
        .await
        .expect("get_template failed");
    assert_eq!(tpl.name, "tpl-list-test");
}

#[tokio::test]
async fn test_get_template_not_found() {
    let Some(db) = TestDb::new().await else { return };

    let err = AgentRepository::get_template(&db.pool, "no-such-template")
        .await
        .unwrap_err();
    assert!(err.to_string().contains("not found"));
}

#[tokio::test]
async fn test_create_and_get_instance() {
    let Some(db) = TestDb::new().await else { return };

    let id = Uuid::new_v4().to_string();
    insert_instance(&db, &id, "inst-create-test", "some-tpl").await;

    let inst = AgentRepository::get_instance(&db.pool, &id)
        .await
        .expect("get_instance failed");
    assert_eq!(inst.name, "inst-create-test");
    assert_eq!(inst.status.as_deref(), Some("created"));
}

#[tokio::test]
async fn test_list_instances_with_status_filter() {
    let Some(db) = TestDb::new().await else { return };

    let id1 = Uuid::new_v4().to_string();
    let id2 = Uuid::new_v4().to_string();
    insert_instance(&db, &id1, "inst-running", "tpl-a").await;
    insert_instance(&db, &id2, "inst-stopped", "tpl-b").await;

    AgentRepository::update_status(&db.pool, &id1, "running")
        .await
        .expect("update_status failed");

    let running = AgentRepository::list_instances(&db.pool, Some("running"))
        .await
        .expect("list_instances failed");
    assert!(running.iter().any(|i| i.name == "inst-running"));
    assert!(running.iter().all(|i| i.status.as_deref() == Some("running")));
}

#[tokio::test]
async fn test_update_status_not_found() {
    let Some(db) = TestDb::new().await else { return };

    let missing = Uuid::new_v4().to_string();
    let err = AgentRepository::update_status(&db.pool, &missing, "running")
        .await
        .unwrap_err();
    assert!(err.to_string().contains("not found"));
}

#[tokio::test]
async fn test_duplicate_instance_name_constraint() {
    let Some(db) = TestDb::new().await else { return };

    let id1 = Uuid::new_v4().to_string();
    let id2 = Uuid::new_v4().to_string();
    insert_instance(&db, &id1, "unique-name", "tpl-dup").await;

    // Second insert with same name must fail (UNIQUE constraint).
    let err = AgentRepository::create_instance(
        &db.pool,
        &id2,
        "unique-name", // duplicate
        "tpl-dup",
        "tpl-dup",
        "/workspace",
        None,
        None,
        None,
    )
    .await
    .unwrap_err();

    let msg = err.to_string();
    assert!(
        msg.contains("duplicate") || msg.contains("unique") || msg.contains("already exists"),
        "expected constraint violation, got: {msg}"
    );
}

#[tokio::test]
async fn test_delete_instance() {
    let Some(db) = TestDb::new().await else { return };

    let id = Uuid::new_v4().to_string();
    insert_instance(&db, &id, "inst-to-delete", "tpl-del").await;

    let name = AgentRepository::delete_instance(&db.pool, &id)
        .await
        .expect("delete_instance failed");
    assert_eq!(name, "inst-to-delete");

    let err = AgentRepository::get_instance(&db.pool, &id)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("not found"));
}

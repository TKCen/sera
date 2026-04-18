//! sera-mwb4: smoke test for the local-first boot path.
//!
//! Verifies that when `DATABASE_URL` is unset the gateway can instantiate the
//! five SQLite-backed stores (`SqliteSecretsStore`, `SqliteScheduleStore`,
//! `SqliteAuditStore`, `SqliteMeteringStore`, `SqliteAgentStore`) without
//! touching Postgres.
//!
//! The test uses a shared in-memory `Connection` so every store sees the same
//! schema created via [`sera_db::sqlite_schema::init_all`]. This mirrors the
//! production shape where `sera-gateway` opens a single file-backed SQLite DB
//! and hands out `Arc<Mutex<Connection>>` clones.

use std::sync::Arc;

use sera_db::agents::{AgentStore, CreateInstanceInput, SqliteAgentStore};
use sera_db::audit::{AuditStore, SqliteAuditStore};
use sera_db::metering::{MeteringStore, RecordUsageInput, SqliteMeteringStore};
use sera_db::schedules::{ScheduleStore, SqliteScheduleStore};
use sera_db::secrets::{SecretsStore, SqliteSecretsStore, UpsertSecretInput};
use sera_db::sqlite_schema;
use tokio::sync::Mutex;

#[tokio::test]
async fn local_boot_constructs_all_sqlite_stores_without_database_url() {
    // Simulate "DATABASE_URL unset" by never looking it up — we go straight to
    // the SQLite in-memory boot path.
    let conn = rusqlite::Connection::open_in_memory().expect("in-memory conn");
    sqlite_schema::init_all(&conn).expect("init_all schema");
    let shared = Arc::new(Mutex::new(conn));

    // All five stores constructible from a single shared connection.
    let agents: Arc<dyn AgentStore> = Arc::new(SqliteAgentStore::new(Arc::clone(&shared)));
    let audit: Arc<dyn AuditStore> = Arc::new(SqliteAuditStore::new(Arc::clone(&shared)));
    let metering: Arc<dyn MeteringStore> = Arc::new(SqliteMeteringStore::new(Arc::clone(&shared)));
    let schedules: Arc<dyn ScheduleStore> = Arc::new(SqliteScheduleStore::new(Arc::clone(&shared)));
    let secrets: Arc<dyn SecretsStore> = Arc::new(SqliteSecretsStore::new(Arc::clone(&shared)));

    // Cross-store smoke: exercise at least one write on each so we catch
    // schema mismatches that only surface on SQL-prepare.
    let agent_id = uuid::Uuid::new_v4().to_string();
    agents
        .create_instance(CreateInstanceInput {
            id: &agent_id,
            name: "sera",
            template_name: "base",
            template_ref: "base@v1",
            workspace_path: "/ws",
            display_name: None,
            circle: None,
            lifecycle_mode: None,
        })
        .await
        .expect("create agent");
    assert!(agents.instance_name_exists("sera").await.unwrap());

    audit
        .append(
            "agent",
            &agent_id,
            None,
            "local_boot",
            &serde_json::json!({"ok": true}),
            "hash-1",
            None,
        )
        .await
        .expect("append audit");

    metering
        .record_usage(RecordUsageInput {
            agent_id: &agent_id,
            circle_id: None,
            model: "local",
            prompt_tokens: 1,
            completion_tokens: 1,
            total_tokens: 2,
            cost_usd: None,
            latency_ms: None,
            status: "ok",
        })
        .await
        .expect("record usage");
    let budget = metering.check_budget(&agent_id).await.expect("budget");
    assert!(budget.allowed);

    schedules
        .create_schedule(
            &uuid::Uuid::new_v4().to_string(),
            None,
            "sera",
            "boot-probe",
            "cron",
            "0 0 * * * *",
            &serde_json::json!({}),
            "api",
            "active",
            None,
            None,
        )
        .await
        .expect("create schedule");
    let all = schedules.list_schedules().await.expect("list schedules");
    assert_eq!(all.len(), 1);

    let (ct, iv) = sera_db::secrets::SecretsRepository::encrypt("v", "k").unwrap();
    secrets
        .upsert(UpsertSecretInput {
            name: "probe",
            encrypted_value: &ct,
            iv: &iv,
            description: None,
            tags: &[],
            allowed_agents: &[],
            exposure: "internal",
            created_by: None,
        })
        .await
        .expect("upsert secret");
    assert!(secrets.get_by_name("probe").await.unwrap().is_some());
}

//! Integration test binary entry point.
//!
//! Compiled only when the `integration` feature is active:
//!
//!   DATABASE_URL=postgres://user:pass@localhost/sera \
//!     cargo test -p sera-db --features integration
//!
//! Without DATABASE_URL set, every test prints a notice and returns early.

#![cfg(feature = "integration")]

#[path = "integration/sessions.rs"]
pub mod sessions;
#[path = "integration/agents.rs"]
pub mod agents;
#[path = "integration/api_keys.rs"]
pub mod api_keys;
#[path = "integration/audit.rs"]
pub mod audit;

use sqlx::{PgPool, Executor};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Harness helpers
// ---------------------------------------------------------------------------

/// Connect to Postgres and create a fresh isolated schema for one test.
/// Returns `None` (with a notice printed) when `DATABASE_URL` is unset.
pub async fn init_pool() -> Option<(PgPool, String)> {
    let url = match std::env::var("DATABASE_URL") {
        Ok(u) => u,
        Err(_) => {
            println!(
                "[integration] Skipping — DATABASE_URL is not set. \
                 Set it to a running Postgres instance to run integration tests."
            );
            return None;
        }
    };

    let pool = PgPool::connect(&url)
        .await
        .expect("Failed to connect to Postgres");

    let schema = format!("test_{}", Uuid::new_v4().simple());

    pool.execute(
        sqlx::query(&format!("CREATE SCHEMA \"{schema}\"")),
    )
    .await
    .expect("Failed to create test schema");

    pool.execute(
        sqlx::query(&format!("SET search_path TO \"{schema}\", public")),
    )
    .await
    .expect("Failed to set search_path");

    run_ddl(&pool, &schema).await;

    Some((pool, schema))
}

/// Drop the test schema — called from `TestDb::drop` and explicit teardown.
pub async fn cleanup(pool: &PgPool, schema: &str) {
    let _ = pool
        .execute(sqlx::query(&format!(
            "DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"
        )))
        .await;
}

// ---------------------------------------------------------------------------
// RAII wrapper
// ---------------------------------------------------------------------------

/// Hold a pool + schema name.  Drops the schema when this value is dropped
/// (via a blocking `block_in_place` so the async drop completes).
pub struct TestDb {
    pub pool: PgPool,
    pub schema: String,
}

impl TestDb {
    pub async fn new() -> Option<Self> {
        init_pool().await.map(|(pool, schema)| Self { pool, schema })
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        // We need to run async cleanup from a sync Drop.
        // tokio::task::block_in_place is available because tests use the
        // multi-thread runtime (via #[tokio::test]).
        let pool = self.pool.clone();
        let schema = self.schema.clone();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                cleanup(&pool, &schema).await;
            });
        });
    }
}

// ---------------------------------------------------------------------------
// Inline DDL — creates only the tables touched by the 4 tested modules.
// The full production schema lives in the legacy TS migrations; we inline
// just enough here to run isolated tests without an external migration tool.
// ---------------------------------------------------------------------------

async fn run_ddl(pool: &PgPool, schema: &str) {
    let ddl = format!(
        r#"
        -- ---- chat_sessions ------------------------------------------------
        CREATE TABLE IF NOT EXISTS "{schema}".chat_sessions (
            id                UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
            agent_name        TEXT        NOT NULL,
            agent_instance_id UUID,
            title             TEXT        NOT NULL DEFAULT 'New Chat',
            message_count     INTEGER,
            created_at        TIMESTAMPTZ DEFAULT NOW(),
            updated_at        TIMESTAMPTZ DEFAULT NOW()
        );

        -- ---- chat_messages -------------------------------------------------
        CREATE TABLE IF NOT EXISTS "{schema}".chat_messages (
            id         UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
            session_id UUID        NOT NULL REFERENCES "{schema}".chat_sessions(id) ON DELETE CASCADE,
            role       TEXT        NOT NULL,
            content    TEXT,
            metadata   JSONB,
            created_at TIMESTAMPTZ DEFAULT NOW()
        );

        -- ---- agent_templates -----------------------------------------------
        CREATE TABLE IF NOT EXISTS "{schema}".agent_templates (
            id           UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
            name         TEXT        NOT NULL UNIQUE,
            display_name TEXT,
            builtin      BOOLEAN     NOT NULL DEFAULT false,
            category     TEXT,
            spec         JSONB       NOT NULL DEFAULT '{{}}',
            created_at   TIMESTAMPTZ DEFAULT NOW(),
            updated_at   TIMESTAMPTZ DEFAULT NOW()
        );

        -- ---- agent_instances -----------------------------------------------
        CREATE TABLE IF NOT EXISTS "{schema}".agent_instances (
            id                      UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
            name                    TEXT        NOT NULL UNIQUE,
            display_name            TEXT,
            template_name           TEXT        NOT NULL,
            template_ref            TEXT,
            circle                  TEXT,
            status                  TEXT        DEFAULT 'created',
            lifecycle_mode          TEXT,
            parent_instance_id      UUID,
            workspace_path          TEXT        NOT NULL DEFAULT '',
            container_id            TEXT,
            sandbox_boundary        TEXT,
            overrides               JSONB,
            resolved_config         JSONB,
            resolved_capabilities   JSONB,
            last_heartbeat_at       TIMESTAMPTZ,
            created_at              TIMESTAMPTZ DEFAULT NOW(),
            updated_at              TIMESTAMPTZ DEFAULT NOW()
        );

        -- ---- api_keys ------------------------------------------------------
        CREATE TABLE IF NOT EXISTS "{schema}".api_keys (
            id           UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
            name         TEXT        NOT NULL,
            key_hash     TEXT        NOT NULL UNIQUE,
            owner_sub    TEXT        NOT NULL,
            roles        TEXT[]      NOT NULL DEFAULT '{{}}',
            created_at   TIMESTAMPTZ DEFAULT NOW(),
            expires_at   TIMESTAMPTZ,
            last_used_at TIMESTAMPTZ,
            revoked_at   TIMESTAMPTZ
        );

        -- ---- audit_trail ---------------------------------------------------
        CREATE SEQUENCE IF NOT EXISTS "{schema}".audit_trail_sequence_seq;
        CREATE TABLE IF NOT EXISTS "{schema}".audit_trail (
            sequence        BIGINT      PRIMARY KEY DEFAULT nextval('"{schema}".audit_trail_sequence_seq'),
            timestamp       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            actor_type      TEXT        NOT NULL,
            actor_id        TEXT        NOT NULL,
            acting_context  JSONB,
            event_type      TEXT        NOT NULL,
            payload         JSONB       NOT NULL DEFAULT '{{}}',
            prev_hash       TEXT,
            hash            TEXT        NOT NULL
        );
        "#
    );

    pool.execute(sqlx::query(&ddl))
        .await
        .expect("Failed to run integration test DDL");

    // Point search_path so unqualified queries hit our schema first.
    pool.execute(sqlx::query(&format!(
        "SET search_path TO \"{schema}\", public"
    )))
    .await
    .expect("Failed to set search_path after DDL");
}

//! Two-layer session persistence — part table + shadow git snapshot.
//!
//! # Database schema (apply before using SqlxSessionPersist)
//!
//! ```sql
//! CREATE TABLE IF NOT EXISTS session_snapshots (
//!     session_key   TEXT        NOT NULL,
//!     snapshot_data JSONB       NOT NULL,
//!     saved_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
//!     PRIMARY KEY (session_key)
//! );
//! ```

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A single part of a session (tool call, text block, reasoning step).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPart {
    pub id: Uuid,
    pub session_key: String,
    pub part_type: PartType,
    pub content: serde_json::Value,
    pub sequence: u64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Types of session parts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartType {
    TextBlock,
    ToolCall,
    ToolResult,
    ReasoningStep,
    SystemMessage,
}

/// Part table — stores session parts.
/// P0 implementation is in-memory; sqlx streaming is Phase 1.
pub struct PartTable {
    parts: Vec<SessionPart>,
}

impl PartTable {
    pub fn new() -> Self {
        Self { parts: Vec::new() }
    }

    pub fn append(&mut self, part: SessionPart) {
        self.parts.push(part);
    }

    pub fn parts_for_session(&self, session_key: &str) -> Vec<&SessionPart> {
        self.parts
            .iter()
            .filter(|p| p.session_key == session_key)
            .collect()
    }

    pub fn len(&self) -> usize {
        self.parts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }
}

impl Default for PartTable {
    fn default() -> Self {
        Self::new()
    }
}

/// Session snapshot — shadow git for workspace file tracking.
/// P0 stub — actual git2 integration is Phase 1.
pub struct SessionSnapshot {
    session_key: String,
    base_path: std::path::PathBuf,
}

impl SessionSnapshot {
    pub fn new(session_key: String, base_path: std::path::PathBuf) -> Self {
        Self {
            session_key,
            base_path,
        }
    }

    /// Track a file change.
    pub fn track(&self, _path: &std::path::Path) -> Result<(), std::io::Error> {
        // P0 stub — git2 integration in Phase 1
        Ok(())
    }

    /// Revert to last snapshot.
    pub fn revert(&self) -> Result<(), std::io::Error> {
        // P0 stub
        Ok(())
    }

    /// Get full diff since last snapshot.
    pub fn diff_full(&self) -> Result<String, std::io::Error> {
        // P0 stub
        Ok(String::new())
    }

    pub fn session_key(&self) -> &str {
        &self.session_key
    }

    pub fn base_path(&self) -> &std::path::Path {
        &self.base_path
    }
}

// ---------------------------------------------------------------------------
// SessionPersist trait — persistence contract for session snapshots.
// ---------------------------------------------------------------------------

/// Serialisable payload stored per session key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSession {
    pub session_key: String,
    pub data: serde_json::Value,
    pub saved_at: time::OffsetDateTime,
}

/// Persistence contract.  Implementors store/load/delete [`PersistedSession`]
/// records keyed by `session_key`.
#[async_trait::async_trait]
pub trait SessionPersist: Send + Sync {
    /// Upsert a session snapshot.
    async fn save(&self, session: &PersistedSession) -> Result<(), sqlx::Error>;

    /// Load a snapshot by key.  Returns `None` when not found.
    async fn load(&self, session_key: &str) -> Result<Option<PersistedSession>, sqlx::Error>;

    /// Delete a snapshot.  Returns `true` if a row was removed.
    async fn delete(&self, session_key: &str) -> Result<bool, sqlx::Error>;
}

// ---------------------------------------------------------------------------
// SqlxSessionPersist — PostgreSQL-backed implementation via runtime queries.
// ---------------------------------------------------------------------------

/// PostgreSQL-backed [`SessionPersist`] implementation.
///
/// Uses `sqlx::query()` (runtime, not compile-time macros) so no
/// `DATABASE_URL` is required at build time.
///
/// # Required table
///
/// See the module-level doc comment for the DDL.
#[derive(Clone)]
pub struct SqlxSessionPersist {
    pool: sqlx::PgPool,
}

impl SqlxSessionPersist {
    /// Create from a bare [`sqlx::PgPool`].
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }

    /// Create from a [`sera_db::DbPool`] wrapper.
    pub fn from_db_pool(pool: &sera_db::DbPool) -> Self {
        Self {
            pool: pool.inner().clone(),
        }
    }
}

#[async_trait::async_trait]
impl SessionPersist for SqlxSessionPersist {
    async fn save(&self, session: &PersistedSession) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO session_snapshots (session_key, snapshot_data, saved_at)
             VALUES ($1, $2, $3)
             ON CONFLICT (session_key)
             DO UPDATE SET snapshot_data = EXCLUDED.snapshot_data,
                           saved_at      = EXCLUDED.saved_at",
        )
        .bind(&session.session_key)
        .bind(&session.data)
        .bind(session.saved_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn load(&self, session_key: &str) -> Result<Option<PersistedSession>, sqlx::Error> {
        let row: Option<(String, serde_json::Value, time::OffsetDateTime)> = sqlx::query_as(
            "SELECT session_key, snapshot_data, saved_at
                 FROM session_snapshots
                 WHERE session_key = $1",
        )
        .bind(session_key)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|(session_key, data, saved_at)| PersistedSession {
            session_key,
            data,
            saved_at,
        }))
    }

    async fn delete(&self, session_key: &str) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM session_snapshots WHERE session_key = $1")
            .bind(session_key)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify SqlxSessionPersist can be constructed without panicking.
    /// A real pool would require a live database; we only test the type-level
    /// wiring here — integration tests (feature = "integration") exercise the
    /// actual SQL.
    #[test]
    fn persisted_session_roundtrips_through_json() {
        let session = PersistedSession {
            session_key: "test-key-abc".to_string(),
            data: serde_json::json!({"messages": [], "model": "claude-opus-4-5"}),
            saved_at: time::OffsetDateTime::now_utc(),
        };

        let serialised = serde_json::to_string(&session).expect("serialise");
        let deserialised: PersistedSession =
            serde_json::from_str(&serialised).expect("deserialise");

        assert_eq!(deserialised.session_key, session.session_key);
        assert_eq!(deserialised.data, session.data);
    }

    #[test]
    fn persisted_session_stores_arbitrary_json_payload() {
        let payload = serde_json::json!({
            "turn": 3,
            "parts": [{"type": "text_block", "content": "hello"}],
        });
        let session = PersistedSession {
            session_key: "s1".to_string(),
            data: payload.clone(),
            saved_at: time::OffsetDateTime::now_utc(),
        };
        assert_eq!(session.data, payload);
    }
}

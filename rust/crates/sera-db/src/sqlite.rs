//! SQLite backend for SERA's MVS (Minimum Viable SERA).
//!
//! Provides a lightweight, single-file (or in-memory) database for local
//! development and single-node deployments. Does NOT replace the PostgreSQL
//! backend — both coexist.

use rusqlite::{Connection, params};
use std::path::Path;

// ---------------------------------------------------------------------------
// Row types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SessionRow {
    pub id: String,
    pub agent_id: String,
    pub session_key: String,
    pub state: String,
    pub principal_id: Option<String>,
    pub created_at: String,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TranscriptRow {
    pub id: i64,
    pub session_id: String,
    pub role: String,
    pub content: Option<String>,
    pub tool_calls: Option<String>,
    pub tool_call_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct AuditRow {
    pub id: i64,
    pub event_type: String,
    pub actor_id: String,
    pub actor_kind: String,
    pub details: Option<String>,
    pub created_at: String,
}

// ---------------------------------------------------------------------------
// SqliteDb
// ---------------------------------------------------------------------------

pub struct SqliteDb {
    conn: Connection,
}

impl SqliteDb {
    /// Open (or create) a database file at the given path.
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        Self::initialize(&conn)?;
        Ok(Self { conn })
    }

    /// Open an in-memory database — useful for tests.
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::initialize(&conn)?;
        Ok(Self { conn })
    }

    fn initialize(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS sessions (
                id          TEXT PRIMARY KEY,
                agent_id    TEXT NOT NULL,
                session_key TEXT NOT NULL UNIQUE,
                state       TEXT NOT NULL DEFAULT 'active',
                principal_id TEXT,
                metadata    TEXT,
                created_at  TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at  TEXT
            );

            CREATE TABLE IF NOT EXISTS transcript (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id   TEXT NOT NULL REFERENCES sessions(id),
                role         TEXT NOT NULL,
                content      TEXT,
                tool_calls   TEXT,
                tool_call_id TEXT,
                created_at   TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_transcript_session ON transcript(session_id);

            CREATE TABLE IF NOT EXISTS queue (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                session_key  TEXT NOT NULL,
                event_data   TEXT NOT NULL,
                status       TEXT NOT NULL DEFAULT 'pending',
                created_at   TEXT NOT NULL DEFAULT (datetime('now')),
                processed_at TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_queue_status ON queue(status, created_at);

            CREATE TABLE IF NOT EXISTS audit_log (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                event_type  TEXT NOT NULL,
                actor_id    TEXT NOT NULL,
                actor_kind  TEXT NOT NULL,
                details     TEXT,
                created_at  TEXT NOT NULL DEFAULT (datetime('now'))
            );
            ",
        )?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Sessions
    // -----------------------------------------------------------------------

    pub fn create_session(
        &self,
        id: &str,
        agent_id: &str,
        session_key: &str,
        principal_id: Option<&str>,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO sessions (id, agent_id, session_key, principal_id) VALUES (?1, ?2, ?3, ?4)",
            params![id, agent_id, session_key, principal_id],
        )?;
        Ok(())
    }

    pub fn get_session_by_key(&self, session_key: &str) -> rusqlite::Result<Option<SessionRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, agent_id, session_key, state, principal_id, created_at, updated_at
             FROM sessions WHERE session_key = ?1",
        )?;
        let mut rows = stmt.query_map(params![session_key], |row| {
            Ok(SessionRow {
                id: row.get(0)?,
                agent_id: row.get(1)?,
                session_key: row.get(2)?,
                state: row.get(3)?,
                principal_id: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn list_sessions(&self) -> rusqlite::Result<Vec<SessionRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, agent_id, session_key, state, principal_id, created_at, updated_at
             FROM sessions ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(SessionRow {
                id: row.get(0)?,
                agent_id: row.get(1)?,
                session_key: row.get(2)?,
                state: row.get(3)?,
                principal_id: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;
        rows.collect()
    }

    pub fn update_session_state(&self, id: &str, state: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE sessions SET state = ?1, updated_at = datetime('now') WHERE id = ?2",
            params![state, id],
        )?;
        Ok(())
    }

    /// Return an existing active session for the given agent, or create a new one.
    pub fn get_or_create_session(&self, agent_id: &str) -> rusqlite::Result<SessionRow> {
        // Try to find an existing active session for this agent.
        let mut stmt = self.conn.prepare(
            "SELECT id, agent_id, session_key, state, principal_id, created_at, updated_at
             FROM sessions WHERE agent_id = ?1 AND state = 'active' LIMIT 1",
        )?;
        let mut rows = stmt.query_map(params![agent_id], |row| {
            Ok(SessionRow {
                id: row.get(0)?,
                agent_id: row.get(1)?,
                session_key: row.get(2)?,
                state: row.get(3)?,
                principal_id: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;

        if let Some(row) = rows.next() {
            return row;
        }
        // Drop the borrow on `stmt` / `rows` before mutating via insert.
        drop(rows);
        drop(stmt);

        // No active session — create one.
        let id = format!("ses_{}", uuid_v4_hex());
        let session_key = format!("sk_{}", uuid_v4_hex());
        self.conn.execute(
            "INSERT INTO sessions (id, agent_id, session_key) VALUES (?1, ?2, ?3)",
            params![id, agent_id, session_key],
        )?;

        // Re-fetch to get the server-generated created_at.
        // We know the key is unique so unwrap is safe.
        self.get_session_by_key(&session_key)
            .map(|opt| opt.expect("just-inserted session must exist"))
    }

    // -----------------------------------------------------------------------
    // Transcript
    // -----------------------------------------------------------------------

    pub fn append_transcript(
        &self,
        session_id: &str,
        role: &str,
        content: Option<&str>,
        tool_calls: Option<&str>,
        tool_call_id: Option<&str>,
    ) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT INTO transcript (session_id, role, content, tool_calls, tool_call_id)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![session_id, role, content, tool_calls, tool_call_id],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_transcript(&self, session_id: &str) -> rusqlite::Result<Vec<TranscriptRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, role, content, tool_calls, tool_call_id, created_at
             FROM transcript WHERE session_id = ?1 ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok(TranscriptRow {
                id: row.get(0)?,
                session_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                tool_calls: row.get(4)?,
                tool_call_id: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        rows.collect()
    }

    pub fn get_transcript_recent(
        &self,
        session_id: &str,
        limit: usize,
    ) -> rusqlite::Result<Vec<TranscriptRow>> {
        // Sub-select the N most recent rows (DESC), then re-sort ASC so the
        // caller gets chronological order.
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, role, content, tool_calls, tool_call_id, created_at
             FROM (
                 SELECT id, session_id, role, content, tool_calls, tool_call_id, created_at
                 FROM transcript WHERE session_id = ?1
                 ORDER BY id DESC LIMIT ?2
             ) ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![session_id, limit as i64], |row| {
            Ok(TranscriptRow {
                id: row.get(0)?,
                session_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                tool_calls: row.get(4)?,
                tool_call_id: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        rows.collect()
    }

    pub fn clear_transcript(&self, session_id: &str) -> rusqlite::Result<usize> {
        self.conn.execute(
            "DELETE FROM transcript WHERE session_id = ?1",
            params![session_id],
        )
    }

    // -----------------------------------------------------------------------
    // Queue
    // -----------------------------------------------------------------------

    pub fn enqueue(&self, session_key: &str, event_data: &str) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT INTO queue (session_key, event_data) VALUES (?1, ?2)",
            params![session_key, event_data],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Dequeue the oldest pending item for the given session key.
    /// Atomically marks it as `processing`.
    pub fn dequeue(&self, session_key: &str) -> rusqlite::Result<Option<(i64, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, event_data FROM queue
             WHERE session_key = ?1 AND status = 'pending'
             ORDER BY created_at ASC, id ASC
             LIMIT 1",
        )?;
        let mut rows = stmt.query_map(params![session_key], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;

        match rows.next() {
            Some(result) => {
                let (id, data) = result?;
                drop(rows);
                drop(stmt);
                self.conn.execute(
                    "UPDATE queue SET status = 'processing', processed_at = datetime('now') WHERE id = ?1",
                    params![id],
                )?;
                Ok(Some((id, data)))
            }
            None => Ok(None),
        }
    }

    pub fn mark_done(&self, id: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE queue SET status = 'done', processed_at = datetime('now') WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn mark_failed(&self, id: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE queue SET status = 'failed', processed_at = datetime('now') WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Audit
    // -----------------------------------------------------------------------

    pub fn append_audit(
        &self,
        event_type: &str,
        actor_id: &str,
        actor_kind: &str,
        details: Option<&str>,
    ) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT INTO audit_log (event_type, actor_id, actor_kind, details) VALUES (?1, ?2, ?3, ?4)",
            params![event_type, actor_id, actor_kind, details],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn query_audit(&self, limit: usize) -> rusqlite::Result<Vec<AuditRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, event_type, actor_id, actor_kind, details, created_at
             FROM audit_log ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(AuditRow {
                id: row.get(0)?,
                event_type: row.get(1)?,
                actor_id: row.get(2)?,
                actor_kind: row.get(3)?,
                details: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;
        rows.collect()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Generate a short hex string suitable for IDs (not cryptographically random,
/// but good enough for local/dev session identifiers).
fn uuid_v4_hex() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    // Mix nanos + a counter-ish value for uniqueness within a process.
    format!("{:016x}{:08x}", d.as_nanos(), std::process::id())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn new_db() -> SqliteDb {
        SqliteDb::open_in_memory().expect("in-memory db")
    }

    // -- Sessions -----------------------------------------------------------

    #[test]
    fn test_create_and_get_session() {
        let db = new_db();
        db.create_session("s1", "agent-a", "sk_1", Some("principal-x"))
            .unwrap();
        let row = db.get_session_by_key("sk_1").unwrap().expect("should exist");
        assert_eq!(row.id, "s1");
        assert_eq!(row.agent_id, "agent-a");
        assert_eq!(row.session_key, "sk_1");
        assert_eq!(row.state, "active");
        assert_eq!(row.principal_id.as_deref(), Some("principal-x"));
        assert!(!row.created_at.is_empty());
    }

    #[test]
    fn test_get_session_missing() {
        let db = new_db();
        assert!(db.get_session_by_key("nonexistent").unwrap().is_none());
    }

    #[test]
    fn test_update_session_state() {
        let db = new_db();
        db.create_session("s2", "agent-b", "sk_2", None).unwrap();
        db.update_session_state("s2", "completed").unwrap();
        let row = db.get_session_by_key("sk_2").unwrap().unwrap();
        assert_eq!(row.state, "completed");
        assert!(row.updated_at.is_some());
    }

    #[test]
    fn test_get_or_create_session_creates_new() {
        let db = new_db();
        let row = db.get_or_create_session("agent-new").unwrap();
        assert_eq!(row.agent_id, "agent-new");
        assert_eq!(row.state, "active");
        assert!(!row.session_key.is_empty());
    }

    #[test]
    fn test_get_or_create_session_returns_existing() {
        let db = new_db();
        db.create_session("s3", "agent-c", "sk_3", None).unwrap();
        let row = db.get_or_create_session("agent-c").unwrap();
        assert_eq!(row.id, "s3");
        assert_eq!(row.session_key, "sk_3");
    }

    #[test]
    fn test_get_or_create_ignores_non_active() {
        let db = new_db();
        db.create_session("s4", "agent-d", "sk_4", None).unwrap();
        db.update_session_state("s4", "completed").unwrap();
        // Should create a new session since the existing one is not active.
        let row = db.get_or_create_session("agent-d").unwrap();
        assert_ne!(row.id, "s4");
        assert_eq!(row.state, "active");
    }

    // -- Transcript ---------------------------------------------------------

    #[test]
    fn test_append_and_get_transcript() {
        let db = new_db();
        db.create_session("s10", "a", "sk_10", None).unwrap();

        let id1 = db
            .append_transcript("s10", "user", Some("hello"), None, None)
            .unwrap();
        let id2 = db
            .append_transcript("s10", "assistant", Some("hi"), None, None)
            .unwrap();
        assert!(id2 > id1);

        let rows = db.get_transcript("s10").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].role, "user");
        assert_eq!(rows[0].content.as_deref(), Some("hello"));
        assert_eq!(rows[1].role, "assistant");
    }

    #[test]
    fn test_transcript_with_tool_calls() {
        let db = new_db();
        db.create_session("s11", "a", "sk_11", None).unwrap();
        db.append_transcript(
            "s11",
            "assistant",
            None,
            Some(r#"[{"name":"search"}]"#),
            None,
        )
        .unwrap();
        db.append_transcript("s11", "tool", Some("result"), None, Some("tc_1"))
            .unwrap();

        let rows = db.get_transcript("s11").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0].tool_calls.as_deref(),
            Some(r#"[{"name":"search"}]"#)
        );
        assert_eq!(rows[1].tool_call_id.as_deref(), Some("tc_1"));
    }

    #[test]
    fn test_get_transcript_recent() {
        let db = new_db();
        db.create_session("s12", "a", "sk_12", None).unwrap();
        for i in 0..10 {
            db.append_transcript("s12", "user", Some(&format!("msg{i}")), None, None)
                .unwrap();
        }

        let recent = db.get_transcript_recent("s12", 3).unwrap();
        assert_eq!(recent.len(), 3);
        // Should be the last 3 in chronological order.
        assert_eq!(recent[0].content.as_deref(), Some("msg7"));
        assert_eq!(recent[1].content.as_deref(), Some("msg8"));
        assert_eq!(recent[2].content.as_deref(), Some("msg9"));
    }

    #[test]
    fn test_clear_transcript() {
        let db = new_db();
        db.create_session("s13", "a", "sk_13", None).unwrap();
        db.append_transcript("s13", "user", Some("x"), None, None)
            .unwrap();
        db.append_transcript("s13", "user", Some("y"), None, None)
            .unwrap();

        let deleted = db.clear_transcript("s13").unwrap();
        assert_eq!(deleted, 2);
        assert!(db.get_transcript("s13").unwrap().is_empty());
    }

    #[test]
    fn test_clear_transcript_empty() {
        let db = new_db();
        db.create_session("s14", "a", "sk_14", None).unwrap();
        let deleted = db.clear_transcript("s14").unwrap();
        assert_eq!(deleted, 0);
    }

    // -- Queue --------------------------------------------------------------

    #[test]
    fn test_enqueue_dequeue_fifo() {
        let db = new_db();
        db.enqueue("sk_q1", r#"{"event":"first"}"#).unwrap();
        db.enqueue("sk_q1", r#"{"event":"second"}"#).unwrap();
        db.enqueue("sk_q1", r#"{"event":"third"}"#).unwrap();

        let (id1, data1) = db.dequeue("sk_q1").unwrap().expect("first");
        assert_eq!(data1, r#"{"event":"first"}"#);

        let (id2, data2) = db.dequeue("sk_q1").unwrap().expect("second");
        assert_eq!(data2, r#"{"event":"second"}"#);
        assert!(id2 > id1);

        let (_id3, data3) = db.dequeue("sk_q1").unwrap().expect("third");
        assert_eq!(data3, r#"{"event":"third"}"#);

        // Queue is now empty for this key.
        assert!(db.dequeue("sk_q1").unwrap().is_none());
    }

    #[test]
    fn test_dequeue_empty() {
        let db = new_db();
        assert!(db.dequeue("nonexistent").unwrap().is_none());
    }

    #[test]
    fn test_mark_done() {
        let db = new_db();
        let id = db.enqueue("sk_q2", "data").unwrap();
        let (dequeued_id, _) = db.dequeue("sk_q2").unwrap().unwrap();
        assert_eq!(dequeued_id, id);
        db.mark_done(id).unwrap();

        // Should not be dequeued again.
        assert!(db.dequeue("sk_q2").unwrap().is_none());
    }

    #[test]
    fn test_mark_failed() {
        let db = new_db();
        let id = db.enqueue("sk_q3", "data").unwrap();
        db.dequeue("sk_q3").unwrap(); // move to processing
        db.mark_failed(id).unwrap();

        // Failed items are not re-dequeued.
        assert!(db.dequeue("sk_q3").unwrap().is_none());
    }

    #[test]
    fn test_dequeue_only_pending() {
        let db = new_db();
        let id1 = db.enqueue("sk_q4", "a").unwrap();
        db.enqueue("sk_q4", "b").unwrap();

        // Dequeue first -> moves to processing.
        db.dequeue("sk_q4").unwrap();
        db.mark_done(id1).unwrap();

        // Second item should still be pending.
        let (_, data) = db.dequeue("sk_q4").unwrap().expect("second pending");
        assert_eq!(data, "b");
    }

    #[test]
    fn test_queue_isolation_by_session_key() {
        let db = new_db();
        db.enqueue("sk_a", "for_a").unwrap();
        db.enqueue("sk_b", "for_b").unwrap();

        let (_, data) = db.dequeue("sk_a").unwrap().unwrap();
        assert_eq!(data, "for_a");

        let (_, data) = db.dequeue("sk_b").unwrap().unwrap();
        assert_eq!(data, "for_b");
    }

    // -- Audit --------------------------------------------------------------

    #[test]
    fn test_append_and_query_audit() {
        let db = new_db();
        db.append_audit("session.start", "agent-1", "agent", Some(r#"{"foo":"bar"}"#))
            .unwrap();
        db.append_audit("session.end", "agent-1", "agent", None)
            .unwrap();
        db.append_audit("api.call", "user-1", "user", Some("details"))
            .unwrap();

        let rows = db.query_audit(10).unwrap();
        assert_eq!(rows.len(), 3);
        // Most recent first.
        assert_eq!(rows[0].event_type, "api.call");
        assert_eq!(rows[1].event_type, "session.end");
        assert_eq!(rows[2].event_type, "session.start");
    }

    #[test]
    fn test_audit_with_limit() {
        let db = new_db();
        for i in 0..5 {
            db.append_audit(&format!("evt{i}"), "a", "agent", None)
                .unwrap();
        }

        let rows = db.query_audit(2).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].event_type, "evt4");
        assert_eq!(rows[1].event_type, "evt3");
    }

    #[test]
    fn test_audit_empty() {
        let db = new_db();
        let rows = db.query_audit(10).unwrap();
        assert!(rows.is_empty());
    }
}

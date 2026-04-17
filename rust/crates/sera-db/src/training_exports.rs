//! Repository for `training_exports` table (SQLite-backed, MVS path).
//!
//! Uses rusqlite directly — matching the SQLite pattern established in
//! `sera-db::sqlite`. The PostgreSQL path can be added later via sqlx.

use rusqlite::{Connection, params};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use sera_types::training_export::{
    TrainingExportFilter, TrainingExportFormat, TrainingExportRecord, TrainingExportRequest,
    TrainingExportStatus,
};

use crate::error::DbError;

// ---------------------------------------------------------------------------
// Helpers — convert between Rust types and SQLite TEXT/INTEGER values
// ---------------------------------------------------------------------------

fn format_to_str(f: TrainingExportFormat) -> &'static str {
    match f {
        TrainingExportFormat::OpenaiJsonl => "openai_jsonl",
        TrainingExportFormat::Alpaca => "alpaca",
        TrainingExportFormat::ShareGpt => "share_gpt",
    }
}

fn format_from_str(s: &str) -> TrainingExportFormat {
    match s {
        "alpaca" => TrainingExportFormat::Alpaca,
        "share_gpt" => TrainingExportFormat::ShareGpt,
        _ => TrainingExportFormat::OpenaiJsonl,
    }
}

fn status_from_str(s: &str) -> TrainingExportStatus {
    match s {
        "running" => TrainingExportStatus::Running,
        "complete" => TrainingExportStatus::Complete,
        "failed" => TrainingExportStatus::Failed,
        _ => TrainingExportStatus::Queued,
    }
}

fn dt_from_str(s: Option<String>) -> Option<chrono::DateTime<chrono::Utc>> {
    s.and_then(|v| {
        // Try RFC3339 first, then SQLite's `datetime('now')` format (no T, no Z).
        v.parse::<chrono::DateTime<chrono::Utc>>().ok().or_else(|| {
            chrono::NaiveDateTime::parse_from_str(&v, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|ndt| ndt.and_utc())
        })
    })
}

fn dt_to_string(dt: chrono::DateTime<chrono::Utc>) -> String {
    dt.to_rfc3339()
}

// ---------------------------------------------------------------------------
// TrainingExportRepo
// ---------------------------------------------------------------------------

/// Repository for the `training_exports` table.
///
/// Wraps a `rusqlite::Connection` behind an `Arc<Mutex<_>>` so it can be
/// shared across async handlers (the lock is held only for the duration of
/// each synchronous SQLite call).
#[derive(Clone)]
pub struct TrainingExportRepo {
    conn: Arc<Mutex<Connection>>,
}

impl TrainingExportRepo {
    /// Create a repo backed by the given connection.
    /// The caller is responsible for ensuring the `training_exports` table
    /// already exists (e.g. via `SqliteDb::open_in_memory()`).
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Insert a new export record in `queued` status; returns the new id.
    pub fn create(&self, req: &TrainingExportRequest) -> Result<Uuid, DbError> {
        let id = Uuid::new_v4();
        let conn = self.conn.lock().map_err(|_| {
            DbError::Integrity("mutex poisoned".to_string())
        })?;

        let filter = &req.filter;
        conn.execute(
            "INSERT INTO training_exports
               (id, format, filter_min_score, filter_date_from, filter_date_to,
                filter_trigger_type, pii_redaction, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'queued')",
            params![
                id.to_string(),
                format_to_str(req.format),
                filter.min_score,
                filter.date_from.map(dt_to_string),
                filter.date_to.map(dt_to_string),
                filter.trigger_type.as_deref(),
                req.pii_redaction as i32,
            ],
        )
        .map_err(|e| DbError::Integrity(e.to_string()))?;

        Ok(id)
    }

    /// Fetch a single export record by id.
    pub fn get(&self, id: Uuid) -> Result<Option<TrainingExportRecord>, DbError> {
        let conn = self.conn.lock().map_err(|_| {
            DbError::Integrity("mutex poisoned".to_string())
        })?;

        let mut stmt = conn
            .prepare(
                "SELECT id, format, filter_min_score, filter_date_from, filter_date_to,
                        filter_trigger_type, pii_redaction, status, total_records,
                        output_path, error, created_at, started_at, finished_at
                 FROM training_exports WHERE id = ?1",
            )
            .map_err(|e| DbError::Integrity(e.to_string()))?;

        let mut rows = stmt
            .query_map(params![id.to_string()], |row| {
                Ok(RawRow {
                    id: row.get(0)?,
                    format: row.get(1)?,
                    filter_min_score: row.get(2)?,
                    filter_date_from: row.get(3)?,
                    filter_date_to: row.get(4)?,
                    filter_trigger_type: row.get(5)?,
                    pii_redaction: row.get::<_, i32>(6)? != 0,
                    status: row.get(7)?,
                    total_records: row.get(8)?,
                    output_path: row.get(9)?,
                    error: row.get(10)?,
                    created_at: row.get(11)?,
                    started_at: row.get(12)?,
                    finished_at: row.get(13)?,
                })
            })
            .map_err(|e| DbError::Integrity(e.to_string()))?;

        match rows.next() {
            Some(Ok(raw)) => Ok(Some(raw_to_record(raw))),
            Some(Err(e)) => Err(DbError::Integrity(e.to_string())),
            None => Ok(None),
        }
    }

    /// Transition a record to `running` and set `started_at`.
    pub fn mark_running(&self, id: Uuid) -> Result<(), DbError> {
        let conn = self.conn.lock().map_err(|_| {
            DbError::Integrity("mutex poisoned".to_string())
        })?;
        conn.execute(
            "UPDATE training_exports
             SET status = 'running', started_at = datetime('now')
             WHERE id = ?1",
            params![id.to_string()],
        )
        .map_err(|e| DbError::Integrity(e.to_string()))?;
        Ok(())
    }

    /// Transition a record to `complete`, recording total rows and output path.
    pub fn mark_complete(&self, id: Uuid, total: i32, path: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().map_err(|_| {
            DbError::Integrity("mutex poisoned".to_string())
        })?;
        conn.execute(
            "UPDATE training_exports
             SET status = 'complete', total_records = ?2, output_path = ?3,
                 finished_at = datetime('now')
             WHERE id = ?1",
            params![id.to_string(), total, path],
        )
        .map_err(|e| DbError::Integrity(e.to_string()))?;
        Ok(())
    }

    /// Transition a record to `failed`, recording the error message.
    pub fn mark_failed(&self, id: Uuid, error: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().map_err(|_| {
            DbError::Integrity("mutex poisoned".to_string())
        })?;
        conn.execute(
            "UPDATE training_exports
             SET status = 'failed', error = ?2, finished_at = datetime('now')
             WHERE id = ?1",
            params![id.to_string(), error],
        )
        .map_err(|e| DbError::Integrity(e.to_string()))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Internal raw row + conversion
// ---------------------------------------------------------------------------

struct RawRow {
    id: String,
    format: String,
    filter_min_score: Option<f64>,
    filter_date_from: Option<String>,
    filter_date_to: Option<String>,
    filter_trigger_type: Option<String>,
    pii_redaction: bool,
    status: String,
    total_records: Option<i32>,
    output_path: Option<String>,
    error: Option<String>,
    created_at: String,
    started_at: Option<String>,
    finished_at: Option<String>,
}

fn raw_to_record(raw: RawRow) -> TrainingExportRecord {
    TrainingExportRecord {
        id: raw.id.parse().unwrap_or_else(|_| Uuid::nil()),
        format: format_from_str(&raw.format),
        filter: TrainingExportFilter {
            min_score: raw.filter_min_score,
            date_from: dt_from_str(raw.filter_date_from),
            date_to: dt_from_str(raw.filter_date_to),
            trigger_type: raw.filter_trigger_type,
        },
        pii_redaction: raw.pii_redaction,
        status: status_from_str(&raw.status),
        total_records: raw.total_records,
        output_path: raw.output_path,
        error: raw.error,
        created_at: raw.created_at.parse().unwrap_or_else(|_| chrono::Utc::now()),
        started_at: dt_from_str(raw.started_at),
        finished_at: dt_from_str(raw.finished_at),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use sera_types::training_export::{TrainingExportFilter, TrainingExportFormat};

    fn make_repo() -> TrainingExportRepo {
        let conn = Connection::open_in_memory().expect("in-memory db");
        // Run the schema so training_exports table exists.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS training_exports (
                id                  TEXT PRIMARY KEY,
                format              TEXT NOT NULL,
                filter_min_score    REAL,
                filter_date_from    TEXT,
                filter_date_to      TEXT,
                filter_trigger_type TEXT,
                pii_redaction       INTEGER NOT NULL DEFAULT 1,
                status              TEXT NOT NULL DEFAULT 'queued',
                total_records       INTEGER,
                output_path         TEXT,
                error               TEXT,
                created_at          TEXT NOT NULL DEFAULT (datetime('now')),
                started_at          TEXT,
                finished_at         TEXT
            );",
        )
        .expect("schema");
        TrainingExportRepo::new(Arc::new(Mutex::new(conn)))
    }

    fn sample_request() -> TrainingExportRequest {
        TrainingExportRequest {
            format: TrainingExportFormat::OpenaiJsonl,
            filter: TrainingExportFilter {
                min_score: Some(0.7),
                date_from: None,
                date_to: None,
                trigger_type: Some("user".to_string()),
            },
            pii_redaction: true,
        }
    }

    #[test]
    fn create_returns_uuid_and_get_returns_record() {
        let repo = make_repo();
        let req = sample_request();
        let id = repo.create(&req).expect("create");
        let record = repo.get(id).expect("get").expect("should exist");

        assert_eq!(record.id, id);
        assert_eq!(record.format, TrainingExportFormat::OpenaiJsonl);
        assert_eq!(record.status, TrainingExportStatus::Queued);
        assert!(record.pii_redaction);
        assert_eq!(record.filter.min_score, Some(0.7));
        assert_eq!(record.filter.trigger_type.as_deref(), Some("user"));
        assert!(record.total_records.is_none());
        assert!(record.output_path.is_none());
        assert!(record.error.is_none());
    }

    #[test]
    fn get_missing_returns_none() {
        let repo = make_repo();
        let result = repo.get(Uuid::new_v4()).expect("get");
        assert!(result.is_none());
    }

    #[test]
    fn mark_running_transitions_status() {
        let repo = make_repo();
        let id = repo.create(&sample_request()).expect("create");
        repo.mark_running(id).expect("mark_running");
        let record = repo.get(id).expect("get").expect("should exist");
        assert_eq!(record.status, TrainingExportStatus::Running);
        assert!(record.started_at.is_some());
    }

    #[test]
    fn mark_complete_records_path_and_total() {
        let repo = make_repo();
        let id = repo.create(&sample_request()).expect("create");
        repo.mark_running(id).expect("mark_running");
        repo.mark_complete(id, 42, "/tmp/export.jsonl")
            .expect("mark_complete");
        let record = repo.get(id).expect("get").expect("should exist");
        assert_eq!(record.status, TrainingExportStatus::Complete);
        assert_eq!(record.total_records, Some(42));
        assert_eq!(record.output_path.as_deref(), Some("/tmp/export.jsonl"));
        assert!(record.finished_at.is_some());
    }

    #[test]
    fn mark_failed_records_error_message() {
        let repo = make_repo();
        let id = repo.create(&sample_request()).expect("create");
        repo.mark_running(id).expect("mark_running");
        repo.mark_failed(id, "io error").expect("mark_failed");
        let record = repo.get(id).expect("get").expect("should exist");
        assert_eq!(record.status, TrainingExportStatus::Failed);
        assert_eq!(record.error.as_deref(), Some("io error"));
        assert!(record.finished_at.is_some());
    }
}

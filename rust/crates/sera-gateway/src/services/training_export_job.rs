//! Training-data export job — synchronous Phase 2 implementation.
//!
//! Generates a JSONL file from agent transcripts matching the request filter.
//! Alpaca and ShareGPT formats are stubs (NotImplemented) — only
//! `openai_jsonl` is landed in this phase.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use regex::Regex;
use sera_db::sqlite::TranscriptRow;
use sera_db::training_exports::TrainingExportRepo;
use sera_types::training_export::{TrainingExportFormat, TrainingExportRequest};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// TranscriptRepo trait — stub until the real repo lands
// ---------------------------------------------------------------------------

/// Minimal transcript query interface needed by the export job.
///
/// The full `TranscriptRepo` over PostgreSQL is tracked in a follow-up bead
/// (sera-dca-phase3). For Phase 2, callers inject an impl — tests use a
/// synthetic in-memory impl; production wires in the SQLite-backed impl.
pub trait TranscriptRepo: Send + Sync {
    /// Return all transcript rows for sessions that match the export filter.
    /// `min_score` and `trigger_type` are advisory — implementations may
    /// apply them as DB predicates or post-filter.
    fn query_for_export(&self, req: &TrainingExportRequest) -> Vec<TranscriptRow>;
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum TrainingExportError {
    #[error("format not implemented: {0:?}")]
    NotImplemented(TrainingExportFormat),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("repository error: {0}")]
    Repo(String),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

// ---------------------------------------------------------------------------
// PII redaction
// ---------------------------------------------------------------------------

/// Redact common PII patterns in-place on the given string.
///
/// Patterns covered (simple regex, no NLP):
/// - Email addresses: `\b[\w.+-]+@[\w-]+\.[\w.-]+\b`
/// - US SSNs: `\b\d{3}-\d{2}-\d{4}\b`
fn redact_pii(text: &str) -> String {
    // Compile once per call — acceptable for Phase 2 (no hot path).
    let email_re = Regex::new(r"\b[\w.+\-]+@[\w\-]+\.[\w.\-]+\b").expect("valid regex");
    let ssn_re = Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").expect("valid regex");

    let s = email_re.replace_all(text, "[REDACTED]");
    let s = ssn_re.replace_all(&s, "[REDACTED]");
    s.into_owned()
}

// ---------------------------------------------------------------------------
// TrainingExportJob
// ---------------------------------------------------------------------------

pub struct TrainingExportJob;

impl TrainingExportJob {
    /// Run the export job synchronously.
    ///
    /// Steps:
    /// 1. Mark running
    /// 2. Query transcripts
    /// 3. Write output file (openai_jsonl only; others → NotImplemented)
    /// 4. Mark complete
    ///
    /// On any failure, marks the record as failed and propagates the error.
    pub fn run(
        request: TrainingExportRequest,
        repo: Arc<TrainingExportRepo>,
        transcript_repo: Arc<dyn TranscriptRepo>,
        output_dir: &Path,
        id: Uuid,
    ) -> Result<PathBuf, TrainingExportError> {
        // 1. Mark running.
        repo.mark_running(id)
            .map_err(|e| TrainingExportError::Repo(e.to_string()))?;

        let result = Self::run_inner(&request, &transcript_repo, output_dir, id);

        match &result {
            Ok(path) => {
                // Count lines to get total_records — each JSONL line = 1 record.
                let contents = std::fs::read_to_string(path)?;
                let total = contents.lines().filter(|l| !l.trim().is_empty()).count() as i32;
                repo.mark_complete(id, total, &path.to_string_lossy())
                    .map_err(|e| TrainingExportError::Repo(e.to_string()))?;
            }
            Err(e) => {
                let _ = repo.mark_failed(id, &e.to_string());
            }
        }

        result
    }

    fn run_inner(
        request: &TrainingExportRequest,
        transcript_repo: &Arc<dyn TranscriptRepo>,
        output_dir: &Path,
        id: Uuid,
    ) -> Result<PathBuf, TrainingExportError> {
        // 2. Only openai_jsonl is implemented in Phase 2.
        if request.format != TrainingExportFormat::OpenaiJsonl {
            tracing::warn!(
                format = ?request.format,
                "Training export format not implemented in Phase 2 — only openai_jsonl is supported"
            );
            return Err(TrainingExportError::NotImplemented(request.format));
        }

        // 3. Query transcripts.
        let rows = transcript_repo.query_for_export(request);

        // 4. Group rows by session_id to build conversation turns.
        // Each session becomes one JSONL line: {"messages": [...]}.
        let mut sessions: std::collections::BTreeMap<String, Vec<&TranscriptRow>> =
            std::collections::BTreeMap::new();
        for row in &rows {
            sessions
                .entry(row.session_id.clone())
                .or_default()
                .push(row);
        }

        // 5. Write output file.
        let filename = format!("{}.jsonl", id);
        let path = output_dir.join(&filename);
        let mut file = std::fs::File::create(&path)?;

        for (_session_id, turn_rows) in &sessions {
            let messages: Vec<serde_json::Value> = turn_rows
                .iter()
                .filter_map(|row| {
                    let content = row.content.as_deref().unwrap_or("").to_string();
                    let content = if request.pii_redaction {
                        redact_pii(&content)
                    } else {
                        content
                    };
                    // Skip empty tool-only rows (no content and no tool_calls displayed).
                    if content.is_empty() && row.tool_calls.is_none() {
                        return None;
                    }
                    Some(serde_json::json!({
                        "role": row.role,
                        "content": content,
                    }))
                })
                .collect();

            if messages.is_empty() {
                continue;
            }

            let line = serde_json::to_string(&serde_json::json!({ "messages": messages }))?;
            writeln!(file, "{}", line)?;
        }

        Ok(path)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use sera_db::sqlite::TranscriptRow;
    use sera_db::training_exports::TrainingExportRepo;
    use sera_types::training_export::{TrainingExportFilter, TrainingExportFormat};
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    // ---- Synthetic TranscriptRepo ----

    struct FakeTranscriptRepo {
        rows: Vec<TranscriptRow>,
    }

    impl TranscriptRepo for FakeTranscriptRepo {
        fn query_for_export(&self, _req: &TrainingExportRequest) -> Vec<TranscriptRow> {
            self.rows.clone()
        }
    }

    fn make_row(session_id: &str, role: &str, content: &str, id: i64) -> TranscriptRow {
        TranscriptRow {
            id,
            session_id: session_id.to_string(),
            role: role.to_string(),
            content: Some(content.to_string()),
            tool_calls: None,
            tool_call_id: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
        }
    }

    fn make_repo_for_test() -> Arc<TrainingExportRepo> {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS training_exports (
                id TEXT PRIMARY KEY,
                format TEXT NOT NULL,
                filter_min_score REAL,
                filter_date_from TEXT,
                filter_date_to TEXT,
                filter_trigger_type TEXT,
                pii_redaction INTEGER NOT NULL DEFAULT 1,
                status TEXT NOT NULL DEFAULT 'queued',
                total_records INTEGER,
                output_path TEXT,
                error TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                started_at TEXT,
                finished_at TEXT
            );",
        )
        .expect("schema");
        Arc::new(TrainingExportRepo::new(Arc::new(Mutex::new(conn))))
    }

    fn openai_request() -> TrainingExportRequest {
        TrainingExportRequest {
            format: TrainingExportFormat::OpenaiJsonl,
            filter: TrainingExportFilter::default(),
            pii_redaction: false,
        }
    }

    #[test]
    fn openai_jsonl_writes_file_and_marks_complete() {
        let tmp = TempDir::new().unwrap();
        let repo = make_repo_for_test();
        let id = repo.create(&openai_request()).expect("create");

        let transcript_repo: Arc<dyn TranscriptRepo> = Arc::new(FakeTranscriptRepo {
            rows: vec![
                make_row("sess-1", "user", "hello", 1),
                make_row("sess-1", "assistant", "hi there", 2),
                make_row("sess-2", "user", "what is 2+2?", 3),
                make_row("sess-2", "assistant", "4", 4),
            ],
        });

        let path = TrainingExportJob::run(
            openai_request(),
            repo.clone(),
            transcript_repo,
            tmp.path(),
            id,
        )
        .unwrap();

        assert!(path.exists());
        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<_> = contents.lines().collect();
        // 2 sessions → 2 lines
        assert_eq!(lines.len(), 2);
        // Each line is valid JSON with "messages" key
        for line in &lines {
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            assert!(v["messages"].is_array());
        }

        // Repo state should be complete
        let record = repo.get(id).unwrap().unwrap();
        assert_eq!(record.status, sera_types::training_export::TrainingExportStatus::Complete);
        assert_eq!(record.total_records, Some(2));
    }

    #[test]
    fn pii_redaction_replaces_email_and_ssn() {
        let text = "Contact me at user@example.com or SSN 123-45-6789 please";
        let redacted = redact_pii(text);
        assert!(!redacted.contains("user@example.com"));
        assert!(!redacted.contains("123-45-6789"));
        assert!(redacted.contains("[REDACTED]"));
    }

    #[test]
    fn unsupported_format_returns_not_implemented_and_marks_failed() {
        let tmp = TempDir::new().unwrap();
        let repo = make_repo_for_test();
        let alpaca_req = TrainingExportRequest {
            format: TrainingExportFormat::Alpaca,
            filter: TrainingExportFilter::default(),
            pii_redaction: false,
        };
        let id = repo.create(&alpaca_req).expect("create");
        let transcript_repo: Arc<dyn TranscriptRepo> =
            Arc::new(FakeTranscriptRepo { rows: vec![] });

        let err = TrainingExportJob::run(alpaca_req, repo.clone(), transcript_repo, tmp.path(), id)
            .unwrap_err();

        assert!(matches!(err, TrainingExportError::NotImplemented(_)));
        let record = repo.get(id).unwrap().unwrap();
        assert_eq!(
            record.status,
            sera_types::training_export::TrainingExportStatus::Failed
        );
    }
}

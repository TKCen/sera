//! SQLite results store — persists `eval_runs` + `eval_task_results`.
//!
//! Mirrors the pattern in `sera-db::sqlite` (rusqlite, TEXT ids, JSON blobs
//! for bag-of-fields) so an operator familiar with `sera.db` can read
//! `sera-eval.db` without learning a new convention.
//!
//! This stub ships the schema, a thin open/init helper, and `insert_run` +
//! `insert_task_result`. Query helpers (reporting, cross-run diff) land in
//! the follow-up PR that adds the `sera eval report` command.

use rusqlite::{Connection, params};
use std::path::Path;

use crate::error::EvalError;
use crate::runner::HarnessConfig;
use crate::task_def::{MetricSet, TaskResult, Verdict};

/// The full schema, embedded so it is a single source of truth. Applied by
/// [`EvalStore::open`]. Idempotent via `IF NOT EXISTS`.
pub const EVAL_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS eval_runs (
  id              TEXT PRIMARY KEY,
  suite           TEXT NOT NULL,
  model           TEXT NOT NULL,
  harness         TEXT NOT NULL,
  harness_config  TEXT NOT NULL,
  started_at      TEXT NOT NULL,
  finished_at     TEXT,
  git_sha         TEXT NOT NULL,
  host            TEXT NOT NULL,
  notes           TEXT
);
CREATE INDEX IF NOT EXISTS idx_eval_runs_suite ON eval_runs(suite);
CREATE INDEX IF NOT EXISTS idx_eval_runs_model ON eval_runs(model);

CREATE TABLE IF NOT EXISTS eval_tasks (
  id              TEXT NOT NULL,
  suite           TEXT NOT NULL,
  version         INTEGER NOT NULL,
  title           TEXT NOT NULL,
  definition_json TEXT NOT NULL,
  PRIMARY KEY (suite, id, version)
);

CREATE TABLE IF NOT EXISTS eval_task_results (
  id                TEXT PRIMARY KEY,
  run_id            TEXT NOT NULL REFERENCES eval_runs(id) ON DELETE CASCADE,
  task_id           TEXT NOT NULL,
  verdict           TEXT NOT NULL,
  turns             INTEGER NOT NULL,
  prompt_tokens     INTEGER NOT NULL,
  completion_tokens INTEGER NOT NULL,
  latency_ms        INTEGER NOT NULL,
  tool_calls_total  INTEGER NOT NULL,
  tool_calls_valid  INTEGER NOT NULL,
  memory_hits       INTEGER,
  memory_k          INTEGER,
  memory_gold       INTEGER,
  metrics_json      TEXT NOT NULL,
  transcript_json   TEXT NOT NULL,
  error_message     TEXT,
  created_at        TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_eval_task_results_run ON eval_task_results(run_id);
CREATE INDEX IF NOT EXISTS idx_eval_task_results_task ON eval_task_results(task_id);
"#;

pub struct EvalStore {
    conn: Connection,
}

/// Row describing a persisted run, used by reporting queries (the full
/// listing / compare surface lands in the follow-up PR that wires
/// `sera eval report`).
#[derive(Debug, Clone)]
pub struct RunRow {
    pub id: String,
    pub suite: String,
    pub model: String,
    pub harness: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub git_sha: String,
}

impl EvalStore {
    /// Open (or create) a store at `path` and apply the schema.
    ///
    /// Pass `:memory:` for an in-process store — used by tests.
    pub fn open(path: &Path) -> Result<Self, EvalError> {
        let conn = if path.as_os_str() == ":memory:" {
            Connection::open_in_memory()?
        } else {
            Connection::open(path)?
        };
        conn.execute_batch(EVAL_SCHEMA_SQL)?;
        Ok(Self { conn })
    }

    /// Insert an eval run. The caller is responsible for choosing a stable
    /// `run_id` (ULID) and capturing `git_sha` + `host`.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_run(
        &self,
        run_id: &str,
        suite: &str,
        model: &str,
        harness: HarnessConfig,
        harness_config_json: &str,
        started_at: &str,
        git_sha: &str,
        host: &str,
        notes: Option<&str>,
    ) -> Result<(), EvalError> {
        self.conn.execute(
            "INSERT INTO eval_runs (id, suite, model, harness, harness_config,
                started_at, finished_at, git_sha, host, notes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, ?8, ?9)",
            params![
                run_id,
                suite,
                model,
                harness.as_str(),
                harness_config_json,
                started_at,
                git_sha,
                host,
                notes,
            ],
        )?;
        Ok(())
    }

    /// Mark a run finished.
    pub fn finish_run(&self, run_id: &str, finished_at: &str) -> Result<(), EvalError> {
        self.conn.execute(
            "UPDATE eval_runs SET finished_at = ?1 WHERE id = ?2",
            params![finished_at, run_id],
        )?;
        Ok(())
    }

    /// Insert a task result. `result_id` must be unique; callers typically
    /// pass a ULID.
    pub fn insert_task_result(
        &self,
        result_id: &str,
        run_id: &str,
        result: &TaskResult,
        created_at: &str,
    ) -> Result<(), EvalError> {
        let metrics_json = serde_json::to_string(&result.metrics)?;
        let transcript_json = serde_json::to_string(&result.transcript)?;
        let (memory_hits, memory_k, memory_gold) = match &result.metrics.memory_precision {
            Some(mp) => (
                Some(mp.hit_count as i64),
                Some(mp.k as i64),
                Some(mp.gold_count as i64),
            ),
            None => (None, None, None),
        };
        self.conn.execute(
            "INSERT INTO eval_task_results (
                id, run_id, task_id, verdict, turns, prompt_tokens,
                completion_tokens, latency_ms, tool_calls_total,
                tool_calls_valid, memory_hits, memory_k, memory_gold,
                metrics_json, transcript_json, error_message, created_at
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                ?14, ?15, ?16, ?17
             )",
            params![
                result_id,
                run_id,
                result.task_id,
                result.verdict.as_str(),
                result.metrics.turns as i64,
                result.metrics.prompt_tokens as i64,
                result.metrics.completion_tokens as i64,
                result.metrics.latency_ms as i64,
                result.metrics.tool_calls_total as i64,
                result.metrics.tool_calls_valid as i64,
                memory_hits,
                memory_k,
                memory_gold,
                metrics_json,
                transcript_json,
                result.error_message,
                created_at,
            ],
        )?;
        Ok(())
    }

    /// Count successful results for a run — basic reporting helper exercised
    /// by the stub tests.
    pub fn count_passes(&self, run_id: &str) -> Result<u32, EvalError> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM eval_task_results
             WHERE run_id = ?1 AND verdict = ?2",
            params![run_id, Verdict::Pass.as_str()],
            |row| row.get(0),
        )?;
        Ok(n as u32)
    }

    /// Return a summary row for one run.
    pub fn get_run(&self, run_id: &str) -> Result<Option<RunRow>, EvalError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, suite, model, harness, started_at, finished_at, git_sha
             FROM eval_runs WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![run_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(RunRow {
                id: row.get(0)?,
                suite: row.get(1)?,
                model: row.get(2)?,
                harness: row.get(3)?,
                started_at: row.get(4)?,
                finished_at: row.get(5)?,
                git_sha: row.get(6)?,
            }))
        } else {
            Ok(None)
        }
    }
}

/// Helper used by both the runner and the tests to shape a skeleton
/// [`MetricSet`] from scratch without repeating the default boilerplate.
pub fn empty_metrics() -> MetricSet {
    MetricSet::default()
}

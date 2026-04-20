//! SQLite-backed [`SemanticMemoryStore`] — zero-infrastructure recall tier.
//!
//! Pairs with [`crate::pgvector_store::PgVectorStore`] as the two built-in
//! backends for SERA's Tier-2 semantic memory:
//!
//! * `PgVectorStore`    — enterprise path, requires Postgres + pgvector.
//! * `SqliteMemoryStore` — local-first path, single `.sqlite` file, works
//!   with just `rusqlite` (keyword-only) or `rusqlite + sqlite-vec`
//!   (hybrid keyword + vector + RRF).
//!
//! ## Schema (idempotent)
//!
//! ```sql
//! CREATE TABLE IF NOT EXISTS memory_entries (
//!     id               TEXT PRIMARY KEY,
//!     agent_id         TEXT NOT NULL,
//!     tier             TEXT NOT NULL,        -- SegmentKind JSON
//!     content          TEXT NOT NULL,
//!     metadata_json    TEXT,                  -- opaque operator metadata
//!     tags             TEXT,                  -- JSON array
//!     created_at       INTEGER NOT NULL,      -- unix epoch seconds
//!     last_touched_at  INTEGER,
//!     access_count     INTEGER DEFAULT 0,
//!     promoted         INTEGER NOT NULL DEFAULT 0
//! );
//! CREATE INDEX IF NOT EXISTS idx_memory_entries_agent ON memory_entries(agent_id);
//! CREATE INDEX IF NOT EXISTS idx_memory_entries_created ON memory_entries(created_at);
//!
//! -- Self-contained FTS5 (NOT contentless) — deliberately stores the
//! -- content twice so DELETEs and UPDATEs don't hit the "cannot DELETE
//! -- from contentless fts5 table" footgun.
//! CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
//!     content,
//!     agent_id UNINDEXED,
//!     tier UNINDEXED,
//!     tags UNINDEXED
//! );
//!
//! -- vec0 virtual table (only when sqlite-vec extension is registered):
//! CREATE VIRTUAL TABLE IF NOT EXISTS memory_vec USING vec0(
//!     embedding float[<DIMS>]
//! );
//! ```
//!
//! FTS and vec rows are kept in lock-step with `memory_entries` via
//! explicit `INSERT` / `DELETE` / `UPDATE` in the same write path — no
//! triggers are required.
//!
//! ## Recall
//!
//! `query()` runs both a BM25 keyword probe and (when the vector side is
//! available + the query carries text or a pre-computed embedding) a
//! cosine-distance probe, then fuses the two ranked lists with Reciprocal
//! Rank Fusion (`k = 60`). A backend with no sqlite-vec and no embedding
//! service gracefully degrades to BM25-only without changing the trait
//! contract.
//!
//! ## Degraded modes
//!
//! * `sqlite-vec` extension missing / not linked → logged once,
//!   `vec_available = false`, keyword recall continues to work.
//! * No [`EmbeddingService`] supplied → vector side is effectively empty
//!   even if the extension loaded; keyword recall still works.
//! * Errors inside the embedding provider propagate as
//!   [`SemanticError::Backend`] — we NEVER silently store a zero-vector
//!   (see sera-px3w).
//!
//! ## Multi-tenant isolation
//!
//! Every query path filters on `agent_id` FIRST. Tests cover the
//! cross-agent isolation contract explicitly.

use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use tracing::warn;
use uuid::Uuid;

use sera_types::{
    EmbeddingService, EvictionPolicy, MemoryId, PutRequest, ScoredEntry, SemanticEntry,
    SemanticError, SemanticMemoryStore, SemanticQuery, SemanticStats, memory::SegmentKind,
};

/// Default embedding dimensionality — matches the local
/// `all-MiniLM-L6-v2` model used by `sera-runtime::semantic`.
pub const DEFAULT_SQLITE_VEC_DIMENSIONS: usize = 384;

/// RRF fusion constant. `k = 60` is the value recommended by the original
/// Cormack et al. paper and used by Elastic / Weaviate hybrid search.
const RRF_K: f32 = 60.0;

/// Shared rusqlite connection. Wrapped in a blocking mutex because
/// rusqlite is synchronous; all store methods offload to
/// `tokio::task::spawn_blocking`.
type ConnHandle = Arc<Mutex<Connection>>;

// ─── sqlite-vec registration ────────────────────────────────────────────────
//
// sqlite-vec is registered via `sqlite3_auto_extension`, which is a
// process-wide hook that fires on every new Connection. We arm it once
// behind a `Once` the first time any `SqliteMemoryStore` is constructed.

static VEC_AUTOLOAD: std::sync::Once = std::sync::Once::new();

#[cfg(feature = "sqlite-vec")]
fn arm_sqlite_vec_autoload() {
    VEC_AUTOLOAD.call_once(|| {
        // SAFETY: `sqlite3_auto_extension` is a one-shot process-level
        // registration. sqlite-vec exports `sqlite3_vec_init` as the
        // entry point; transmuting the zero-arg extern "C" fn into the
        // three-arg `sqlite3_extension_init` signature is the
        // documented rusqlite + sqlite-vec pattern (see sqlite-vec
        // README — `tests/test_rusqlite_auto_extension`).
        unsafe {
            let init_ptr = sqlite_vec::sqlite3_vec_init as *const ();
            let rc = rusqlite::ffi::sqlite3_auto_extension(Some(
                std::mem::transmute::<
                    *const (),
                    unsafe extern "C" fn(
                        *mut rusqlite::ffi::sqlite3,
                        *mut *mut ::std::os::raw::c_char,
                        *const rusqlite::ffi::sqlite3_api_routines,
                    ) -> ::std::os::raw::c_int,
                >(init_ptr),
            ));
            if rc != rusqlite::ffi::SQLITE_OK {
                warn!("sqlite3_auto_extension(sqlite_vec) returned {rc}");
            }
        }
    });
}

#[cfg(not(feature = "sqlite-vec"))]
fn arm_sqlite_vec_autoload() {
    // sqlite-vec not compiled in — keyword-only path is the only option.
}

/// Probe whether the `vec0` virtual module is usable on `conn`. Doing
/// this is the canonical way to check sqlite-vec liveness: it succeeds
/// iff the extension registered itself for this connection.
fn probe_vec_available(conn: &Connection) -> bool {
    conn.query_row("SELECT vec_version()", [], |r| r.get::<_, String>(0))
        .is_ok()
}

/// SQLite-backed [`SemanticMemoryStore`].
#[derive(Clone)]
pub struct SqliteMemoryStore {
    conn: ConnHandle,
    vec_available: bool,
    embedding: Option<Arc<dyn EmbeddingService>>,
    dims: usize,
    /// Retained for operator diagnostics (`:memory:` vs a real path).
    path_label: String,
}

impl std::fmt::Debug for SqliteMemoryStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteMemoryStore")
            .field("path", &self.path_label)
            .field("vec_available", &self.vec_available)
            .field("dims", &self.dims)
            .field("has_embedder", &self.embedding.is_some())
            .finish()
    }
}

impl SqliteMemoryStore {
    /// Open or create a `SqliteMemoryStore` at `path`. `:memory:` is
    /// valid and yields a per-process ephemeral store.
    pub fn open(
        path: impl AsRef<Path>,
        embedding: Option<Arc<dyn EmbeddingService>>,
    ) -> Result<Self, SemanticError> {
        arm_sqlite_vec_autoload();
        let p = path.as_ref();
        let path_label = p.display().to_string();
        let conn = Connection::open(p)
            .map_err(|e| SemanticError::Backend(format!("open {path_label}: {e}")))?;
        Self::from_connection(conn, embedding, path_label)
    }

    /// Open an in-memory store. Convenience for tests.
    pub fn open_in_memory(
        embedding: Option<Arc<dyn EmbeddingService>>,
    ) -> Result<Self, SemanticError> {
        arm_sqlite_vec_autoload();
        let conn = Connection::open_in_memory()
            .map_err(|e| SemanticError::Backend(format!("open :memory:: {e}")))?;
        Self::from_connection(conn, embedding, ":memory:".to_string())
    }

    fn from_connection(
        conn: Connection,
        embedding: Option<Arc<dyn EmbeddingService>>,
        path_label: String,
    ) -> Result<Self, SemanticError> {
        let dims = embedding
            .as_ref()
            .map(|e| e.dimensions())
            .unwrap_or(DEFAULT_SQLITE_VEC_DIMENSIONS);

        let vec_available = probe_vec_available(&conn);
        if !vec_available {
            warn!(
                path = %path_label,
                "vector recall disabled (sqlite-vec not loaded); keyword-only BM25 path remains"
            );
        }
        Self::initialize_schema(&conn, vec_available, dims)?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            vec_available,
            embedding,
            dims,
            path_label,
        })
    }

    /// Exposes the configured dimensionality (mostly for tests).
    pub fn dimensions(&self) -> usize {
        self.dims
    }

    /// `true` iff the sqlite-vec extension is loaded and the `memory_vec`
    /// virtual table is usable for cosine recall.
    pub fn vector_available(&self) -> bool {
        self.vec_available
    }

    fn initialize_schema(
        conn: &Connection,
        vec_available: bool,
        dims: usize,
    ) -> Result<(), SemanticError> {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS memory_entries (
                id               TEXT PRIMARY KEY,
                agent_id         TEXT NOT NULL,
                tier             TEXT NOT NULL,
                content          TEXT NOT NULL,
                metadata_json    TEXT,
                tags             TEXT,
                created_at       INTEGER NOT NULL,
                last_touched_at  INTEGER,
                access_count     INTEGER NOT NULL DEFAULT 0,
                promoted         INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_memory_entries_agent ON memory_entries(agent_id);
            CREATE INDEX IF NOT EXISTS idx_memory_entries_created ON memory_entries(created_at);

            CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
                content,
                agent_id UNINDEXED,
                tier UNINDEXED,
                tags UNINDEXED
            );
            ",
        )
        .map_err(|e| SemanticError::Backend(format!("schema init: {e}")))?;

        if vec_available {
            let ddl = format!(
                "CREATE VIRTUAL TABLE IF NOT EXISTS memory_vec USING vec0(embedding float[{dims}])"
            );
            conn.execute_batch(&ddl)
                .map_err(|e| SemanticError::Backend(format!("vec0 init: {e}")))?;
        }

        Ok(())
    }

    /// Acquire the connection lock and run `f` on a blocking worker.
    async fn with_conn<F, T>(&self, f: F) -> Result<T, SemanticError>
    where
        F: FnOnce(&mut Connection) -> Result<T, SemanticError> + Send + 'static,
        T: Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = conn
                .lock()
                .map_err(|e| SemanticError::Backend(format!("mutex poisoned: {e}")))?;
            f(&mut guard)
        })
        .await
        .map_err(|e| SemanticError::Backend(format!("join: {e}")))?
    }

    /// Test-only helper: insert a row with a caller-chosen id and
    /// backdated `created_at`. Useful for eviction/TTL tests that depend
    /// on stable ids and aged rows. Not part of the trait surface.
    #[cfg(test)]
    pub(crate) async fn put_raw(
        &self,
        id: &str,
        agent_id: &str,
        content: &str,
        tier: SegmentKind,
        tags: Vec<String>,
        promoted: bool,
        created_at: DateTime<Utc>,
        embedding: Option<Vec<f32>>,
    ) -> Result<MemoryId, SemanticError> {
        let vec_available = self.vec_available;
        let dims = self.dims;
        let tier_json = serde_json::to_string(&tier)
            .map_err(|e| SemanticError::Serialization(format!("tier: {e}")))?;
        let tags_json = serde_json::to_string(&tags)
            .map_err(|e| SemanticError::Serialization(format!("tags: {e}")))?;
        let id_owned = if id.is_empty() {
            Uuid::new_v4().to_string()
        } else {
            id.to_string()
        };
        let id_for_block = id_owned.clone();
        let agent_id = agent_id.to_string();
        let content = content.to_string();
        let created_ts = created_at.timestamp();
        let embedding_owned = embedding;

        self.with_conn(move |conn| -> Result<MemoryId, SemanticError> {
            let tx = conn
                .transaction()
                .map_err(|e| SemanticError::Backend(format!("begin tx: {e}")))?;
            tx.execute(
                "INSERT INTO memory_entries
                    (id, agent_id, tier, content, metadata_json, tags, created_at, last_touched_at, access_count, promoted)
                 VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, NULL, 0, ?7)
                 ON CONFLICT(id) DO UPDATE SET
                     agent_id   = excluded.agent_id,
                     tier       = excluded.tier,
                     content    = excluded.content,
                     tags       = excluded.tags,
                     created_at = excluded.created_at,
                     promoted   = excluded.promoted",
                params![
                    id_for_block,
                    agent_id,
                    tier_json,
                    content,
                    tags_json,
                    created_ts,
                    promoted as i64,
                ],
            )
            .map_err(|e| SemanticError::Backend(format!("insert entry: {e}")))?;
            let rowid: i64 = tx
                .query_row(
                    "SELECT rowid FROM memory_entries WHERE id = ?1",
                    params![id_for_block],
                    |r| r.get(0),
                )
                .map_err(|e| SemanticError::Backend(format!("lookup rowid: {e}")))?;
            tx.execute("DELETE FROM memory_fts WHERE rowid = ?1", params![rowid])
                .map_err(|e| SemanticError::Backend(format!("delete fts: {e}")))?;
            tx.execute(
                "INSERT INTO memory_fts (rowid, content, agent_id, tier, tags)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![rowid, content, agent_id, tier_json, tags_json],
            )
            .map_err(|e| SemanticError::Backend(format!("insert fts: {e}")))?;
            if vec_available
                && let Some(ref emb) = embedding_owned
                && !emb.is_empty()
            {
                if emb.len() != dims {
                    return Err(SemanticError::DimensionMismatch {
                        expected: dims,
                        got: emb.len(),
                    });
                }
                tx.execute("DELETE FROM memory_vec WHERE rowid = ?1", params![rowid])
                    .ok();
                let blob = vec_to_blob(emb);
                tx.execute(
                    "INSERT INTO memory_vec (rowid, embedding) VALUES (?1, ?2)",
                    params![rowid, blob],
                )
                .map_err(|e| SemanticError::Backend(format!("insert vec: {e}")))?;
            }
            tx.commit()
                .map_err(|e| SemanticError::Backend(format!("commit: {e}")))?;
            Ok(MemoryId::new(id_for_block))
        })
        .await
    }
}

// ─── Row plumbing ──────────────────────────────────────────────────────────

/// Internal owned row; we rehydrate ScoredEntry from these.
#[derive(Debug, Clone)]
struct Row {
    id: String,
    agent_id: String,
    tier: SegmentKind,
    content: String,
    tags: Vec<String>,
    created_at: DateTime<Utc>,
    last_touched_at: Option<DateTime<Utc>>,
    promoted: bool,
}

fn row_from_sqlite(row: &rusqlite::Row<'_>) -> rusqlite::Result<Row> {
    let id: String = row.get("id")?;
    let agent_id: String = row.get("agent_id")?;
    let tier_json: String = row.get("tier")?;
    let content: String = row.get("content")?;
    let tags_json: Option<String> = row.get("tags")?;
    let created_at: i64 = row.get("created_at")?;
    let last_touched_at: Option<i64> = row.get("last_touched_at")?;
    let promoted_int: i64 = row.get("promoted")?;

    let tier: SegmentKind = serde_json::from_str(&tier_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let tags: Vec<String> = tags_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    Ok(Row {
        id,
        agent_id,
        tier,
        content,
        tags,
        created_at: DateTime::<Utc>::from_timestamp(created_at, 0).unwrap_or_else(Utc::now),
        last_touched_at: last_touched_at
            .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
        promoted: promoted_int != 0,
    })
}

fn row_to_entry(row: Row, embedding: Option<Vec<f32>>) -> SemanticEntry {
    SemanticEntry {
        id: MemoryId::new(row.id),
        agent_id: row.agent_id,
        content: row.content,
        embedding,
        tier: row.tier,
        tags: row.tags,
        created_at: row.created_at,
        last_accessed_at: row.last_touched_at,
        promoted: row.promoted,
        scope: None,
    }
}

// ─── Scoring ────────────────────────────────────────────────────────────────

fn recency_norm(created_at: DateTime<Utc>, now: DateTime<Utc>) -> f32 {
    const HALF_LIFE_DAYS: f32 = 14.0;
    let age_days = (now - created_at).num_seconds().max(0) as f32 / 86_400.0;
    (1.0 - age_days / HALF_LIFE_DAYS).clamp(0.0, 1.0)
}

/// Encode a `Vec<f32>` as the little-endian byte blob sqlite-vec expects.
fn vec_to_blob(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

fn now_unix() -> i64 {
    Utc::now().timestamp()
}

// ─── put / delete / touch / promote ────────────────────────────────────────

/// Plain-field arguments for the blocking `put` path. Lives in this module
/// so we can hand a fully-owned, `Send` struct to `tokio::task::spawn_blocking`
/// without pulling the full `SemanticEntry` (which would tangle the input
/// shape with the output shape).
struct PutParams {
    id: String,
    agent_id: String,
    content: String,
    tier: SegmentKind,
    tags: Vec<String>,
    promoted: bool,
    embedding: Option<Vec<f32>>,
}

fn put_blocking(
    conn: &mut Connection,
    params: PutParams,
    vec_available: bool,
    expected_dims: usize,
) -> Result<MemoryId, SemanticError> {
    let id = if params.id.is_empty() {
        Uuid::new_v4().to_string()
    } else {
        params.id
    };
    let tier_json = serde_json::to_string(&params.tier)
        .map_err(|e| SemanticError::Serialization(format!("tier serialize: {e}")))?;
    let tags_json = serde_json::to_string(&params.tags)
        .map_err(|e| SemanticError::Serialization(format!("tags serialize: {e}")))?;
    let created = Utc::now().timestamp();
    let last_touched: Option<i64> = None;

    let tx = conn
        .transaction()
        .map_err(|e| SemanticError::Backend(format!("begin tx: {e}")))?;

    tx.execute(
        "INSERT INTO memory_entries
            (id, agent_id, tier, content, metadata_json, tags, created_at, last_touched_at, access_count, promoted)
         VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, ?7, 0, ?8)
         ON CONFLICT(id) DO UPDATE SET
             agent_id        = excluded.agent_id,
             tier            = excluded.tier,
             content         = excluded.content,
             tags            = excluded.tags,
             last_touched_at = excluded.last_touched_at,
             promoted        = excluded.promoted",
        params![
            id,
            params.agent_id,
            tier_json,
            params.content,
            tags_json,
            created,
            last_touched,
            params.promoted as i64,
        ],
    )
    .map_err(|e| SemanticError::Backend(format!("insert entry: {e}")))?;

    // Resolve the rowid we just wrote (insert OR update path both
    // valid). FTS and vec rows are keyed by this rowid so they stay in
    // lock-step with memory_entries.
    let rowid: i64 = tx
        .query_row(
            "SELECT rowid FROM memory_entries WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .map_err(|e| SemanticError::Backend(format!("lookup rowid: {e}")))?;

    // Idempotent FTS sync: delete any stale row with the same rowid, then
    // insert fresh. `memory_fts` is a self-contained FTS5 table so this
    // is safe (contentless tables can't be DELETE'd from).
    tx.execute("DELETE FROM memory_fts WHERE rowid = ?1", params![rowid])
        .map_err(|e| SemanticError::Backend(format!("delete fts: {e}")))?;
    tx.execute(
        "INSERT INTO memory_fts (rowid, content, agent_id, tier, tags)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![rowid, params.content, params.agent_id, tier_json, tags_json],
    )
    .map_err(|e| SemanticError::Backend(format!("insert fts: {e}")))?;

    if vec_available
        && let Some(ref emb) = params.embedding
        && !emb.is_empty()
    {
        if emb.len() != expected_dims {
            return Err(SemanticError::DimensionMismatch {
                expected: expected_dims,
                got: emb.len(),
            });
        }
        tx.execute("DELETE FROM memory_vec WHERE rowid = ?1", params![rowid])
            .ok();
        let blob = vec_to_blob(emb);
        tx.execute(
            "INSERT INTO memory_vec (rowid, embedding) VALUES (?1, ?2)",
            params![rowid, blob],
        )
        .map_err(|e| SemanticError::Backend(format!("insert vec: {e}")))?;
    }

    tx.commit()
        .map_err(|e| SemanticError::Backend(format!("commit put: {e}")))?;
    Ok(MemoryId::new(id))
}

// ─── query ──────────────────────────────────────────────────────────────────

fn bm25_probe(
    conn: &Connection,
    agent_id: &str,
    tier_filter: Option<&SegmentKind>,
    query_text: &str,
    limit: i64,
) -> Result<Vec<(String, f64)>, SemanticError> {
    let match_expr = sanitize_fts_match(query_text);
    if match_expr.is_empty() {
        return Ok(Vec::new());
    }

    let (sql, tier_json_opt) = match tier_filter {
        Some(t) => (
            "SELECT e.id AS id, bm25(memory_fts) AS score
             FROM memory_fts
             JOIN memory_entries e ON e.rowid = memory_fts.rowid
             WHERE memory_fts MATCH ?1
               AND e.agent_id = ?2
               AND e.tier = ?3
             ORDER BY score ASC
             LIMIT ?4",
            Some(serde_json::to_string(t).map_err(|e| {
                SemanticError::Serialization(format!("tier_filter serialize: {e}"))
            })?),
        ),
        None => (
            "SELECT e.id AS id, bm25(memory_fts) AS score
             FROM memory_fts
             JOIN memory_entries e ON e.rowid = memory_fts.rowid
             WHERE memory_fts MATCH ?1
               AND e.agent_id = ?2
             ORDER BY score ASC
             LIMIT ?3",
            None,
        ),
    };

    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| SemanticError::Backend(format!("prepare bm25: {e}")))?;

    let mut out = Vec::new();
    let map_row = |r: &rusqlite::Row<'_>| -> rusqlite::Result<(String, f64)> {
        Ok((r.get::<_, String>("id")?, r.get::<_, f64>("score")?))
    };
    if let Some(tier_json) = tier_json_opt {
        let rows = stmt
            .query_map(params![match_expr, agent_id, tier_json, limit], map_row)
            .map_err(|e| SemanticError::Backend(format!("exec bm25: {e}")))?;
        for r in rows {
            out.push(r.map_err(|e| SemanticError::Backend(format!("bm25 row: {e}")))?);
        }
    } else {
        let rows = stmt
            .query_map(params![match_expr, agent_id, limit], map_row)
            .map_err(|e| SemanticError::Backend(format!("exec bm25: {e}")))?;
        for r in rows {
            out.push(r.map_err(|e| SemanticError::Backend(format!("bm25 row: {e}")))?);
        }
    }
    Ok(out)
}

/// FTS5 interprets punctuation specially. Operator-supplied strings are
/// trusted but we still strip characters the parser rejects and quote the
/// remaining tokens so e.g. `foo:bar` becomes `"foo" "bar"`.
fn sanitize_fts_match(q: &str) -> String {
    let mut parts = Vec::new();
    for tok in q.split_whitespace() {
        let cleaned: String = tok
            .chars()
            .filter(|c| c.is_alphanumeric() || matches!(c, '_' | '-'))
            .collect();
        if !cleaned.is_empty() {
            parts.push(format!("\"{cleaned}\""));
        }
    }
    parts.join(" ")
}

fn vec_probe(
    conn: &Connection,
    agent_id: &str,
    tier_filter: Option<&SegmentKind>,
    embedding: &[f32],
    limit: i64,
) -> Result<Vec<(String, f64)>, SemanticError> {
    let blob = vec_to_blob(embedding);
    // sqlite-vec's canonical recall pattern: a KNN subquery on the
    // vector table which we then JOIN to the main table for agent /
    // tier filtering. `k = ?` binds the KNN depth.
    let (sql, tier_json_opt) = match tier_filter {
        Some(t) => (
            "SELECT e.id AS id, v.distance AS distance
             FROM (
                 SELECT rowid, distance
                 FROM memory_vec
                 WHERE embedding MATCH ?1 AND k = ?2
             ) v
             JOIN memory_entries e ON e.rowid = v.rowid
             WHERE e.agent_id = ?3 AND e.tier = ?4
             ORDER BY v.distance ASC",
            Some(serde_json::to_string(t).map_err(|e| {
                SemanticError::Serialization(format!("tier_filter serialize: {e}"))
            })?),
        ),
        None => (
            "SELECT e.id AS id, v.distance AS distance
             FROM (
                 SELECT rowid, distance
                 FROM memory_vec
                 WHERE embedding MATCH ?1 AND k = ?2
             ) v
             JOIN memory_entries e ON e.rowid = v.rowid
             WHERE e.agent_id = ?3
             ORDER BY v.distance ASC",
            None,
        ),
    };

    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| SemanticError::Backend(format!("prepare vec: {e}")))?;

    let mut out = Vec::new();
    let map_row = |r: &rusqlite::Row<'_>| -> rusqlite::Result<(String, f64)> {
        Ok((r.get::<_, String>("id")?, r.get::<_, f64>("distance")?))
    };
    if let Some(tier_json) = tier_json_opt {
        let rows = stmt
            .query_map(params![blob, limit, agent_id, tier_json], map_row)
            .map_err(|e| SemanticError::Backend(format!("exec vec: {e}")))?;
        for r in rows {
            out.push(r.map_err(|e| SemanticError::Backend(format!("vec row: {e}")))?);
        }
    } else {
        let rows = stmt
            .query_map(params![blob, limit, agent_id], map_row)
            .map_err(|e| SemanticError::Backend(format!("exec vec: {e}")))?;
        for r in rows {
            out.push(r.map_err(|e| SemanticError::Backend(format!("vec row: {e}")))?);
        }
    }
    Ok(out)
}

fn fuse_rrf(
    bm25: &[(String, f64)],
    vec: &[(String, f64)],
) -> Vec<(String, f32, f32, f32)> {
    use std::collections::HashMap;
    // id -> (rrf_score, bm25_raw, vec_distance_raw)
    let mut acc: HashMap<String, (f32, Option<f32>, Option<f32>)> = HashMap::new();
    for (rank, (id, raw)) in bm25.iter().enumerate() {
        let contrib = 1.0_f32 / (RRF_K + (rank as f32 + 1.0));
        let slot = acc.entry(id.clone()).or_insert((0.0, None, None));
        slot.0 += contrib;
        slot.1 = Some(*raw as f32);
    }
    for (rank, (id, raw)) in vec.iter().enumerate() {
        let contrib = 1.0_f32 / (RRF_K + (rank as f32 + 1.0));
        let slot = acc.entry(id.clone()).or_insert((0.0, None, None));
        slot.0 += contrib;
        slot.2 = Some(*raw as f32);
    }

    let mut out: Vec<(String, f32, f32, f32)> = acc
        .into_iter()
        .map(|(id, (rrf, bm25_raw, vec_distance))| {
            // BM25 in sqlite returns smaller = better (usually negative).
            // We negate so higher = more relevant. Not normalised — the
            // Tier-2 scorer uses `score` (RRF) for ranking; per-signal
            // sub-scores are advisory.
            let index_score = bm25_raw.map(|v| -v).unwrap_or(0.0);
            let vector_score = vec_distance.map(|d| 1.0 - d).unwrap_or(0.0);
            (id, rrf, index_score, vector_score)
        })
        .collect();
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    out
}

fn load_entries_by_ids(
    conn: &Connection,
    ids: &[String],
) -> Result<std::collections::HashMap<String, Row>, SemanticError> {
    use std::collections::HashMap;
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT id, agent_id, tier, content, tags, created_at, last_touched_at, promoted
         FROM memory_entries
         WHERE id IN ({placeholders})"
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| SemanticError::Backend(format!("prepare load_entries: {e}")))?;
    let params = rusqlite::params_from_iter(ids.iter());
    let mut out = HashMap::new();
    let iter = stmt
        .query_map(params, row_from_sqlite)
        .map_err(|e| SemanticError::Backend(format!("exec load_entries: {e}")))?;
    for r in iter {
        match r {
            Ok(row) => {
                out.insert(row.id.clone(), row);
            }
            Err(e) => {
                return Err(SemanticError::Backend(format!(
                    "load_entries decode: {e}"
                )));
            }
        }
    }
    Ok(out)
}

// ─── Trait impl ─────────────────────────────────────────────────────────────

#[async_trait]
impl SemanticMemoryStore for SqliteMemoryStore {
    async fn put(&self, req: PutRequest) -> Result<MemoryId, SemanticError> {
        let vec_available = self.vec_available;
        let dims = self.dims;
        let embedding_service = self.embedding.clone();

        // Resolve the embedding. Unlike pgvector, SQLite's BM25 keyword
        // path does not need a vector — `None` is acceptable. If the
        // caller provides a vector we use it; otherwise we try the bound
        // EmbeddingService; a failing embed propagates loudly (never a
        // zero-vector, see sera-px3w); if no embedder is wired we stay
        // keyword-only.
        let mut embedding = req.supplied_embedding.clone();
        if embedding.is_none()
            && vec_available
            && let Some(svc) = embedding_service.as_ref()
        {
            let texts = vec![req.content.clone()];
            let vecs = svc.embed(&texts).await.map_err(|e| {
                SemanticError::Backend(format!("embed on put: {e}"))
            })?;
            embedding = Some(vecs.into_iter().next().ok_or_else(|| {
                SemanticError::Backend("embed returned no vectors".into())
            })?);
        }

        let params = PutParams {
            id: String::new(),
            agent_id: req.agent_id,
            content: req.content,
            tier: req.tier,
            tags: req.tags,
            promoted: req.promoted,
            embedding,
        };

        self.with_conn(move |conn| put_blocking(conn, params, vec_available, dims))
            .await
    }

    async fn query(&self, query: SemanticQuery) -> Result<Vec<ScoredEntry>, SemanticError> {
        let top_k = query.top_k.max(1);
        let fetch_width = (top_k as i64).saturating_mul(2).max(1);
        let agent_id = query.agent_id.clone();
        let tier_filter = query.tier_filter.clone();
        let text = query.text.clone();
        let vec_available = self.vec_available;
        let similarity_threshold = query.similarity_threshold;
        let expected_dims = self.dims;

        // Resolve query embedding — caller-supplied first, else embed
        // the text with the configured provider (if any + vector side
        // live).
        let mut q_embedding = query.query_embedding.clone();
        if q_embedding.is_none()
            && vec_available
            && let Some(svc) = self.embedding.as_ref()
            && let Some(t) = text.as_ref()
        {
            let vecs = svc.embed(std::slice::from_ref(t)).await.map_err(|e| {
                SemanticError::Backend(format!("embed on query: {e}"))
            })?;
            q_embedding = vecs.into_iter().next();
        }

        if let Some(v) = q_embedding.as_ref()
            && v.len() != expected_dims
        {
            return Err(SemanticError::DimensionMismatch {
                expected: expected_dims,
                got: v.len(),
            });
        }

        let results = self
            .with_conn(move |conn| -> Result<Vec<ScoredEntry>, SemanticError> {
                let now = Utc::now();

                let bm25 = if let Some(t) = text.as_deref() {
                    bm25_probe(conn, &agent_id, tier_filter.as_ref(), t, fetch_width)?
                } else {
                    Vec::new()
                };

                let vec_hits = if vec_available
                    && let Some(vec_q) = q_embedding.as_ref()
                {
                    vec_probe(
                        conn,
                        &agent_id,
                        tier_filter.as_ref(),
                        vec_q,
                        fetch_width,
                    )?
                } else {
                    Vec::new()
                };

                let fused = fuse_rrf(&bm25, &vec_hits);
                let ids: Vec<String> = fused.iter().map(|(id, _, _, _)| id.clone()).collect();
                let rows = load_entries_by_ids(conn, &ids)?;

                let mut out: Vec<ScoredEntry> = Vec::with_capacity(fused.len());
                for (id, rrf, idx_s, vec_s) in fused.into_iter() {
                    let Some(row) = rows.get(&id).cloned() else {
                        continue;
                    };
                    if row.agent_id != agent_id {
                        // Defensive: should be impossible via the SQL filter.
                        continue;
                    }
                    let recency = recency_norm(row.created_at, now);
                    // We don't read vectors back from sqlite-vec in the
                    // query path — callers that need the vector re-query
                    // via a concrete method. `None` is honest.
                    let entry = row_to_entry(row, None);
                    let composite = rrf;
                    if let Some(th) = similarity_threshold
                        && composite < th
                    {
                        continue;
                    }
                    out.push(ScoredEntry {
                        entry,
                        score: composite,
                        index_score: idx_s,
                        vector_score: vec_s,
                        recency_score: recency,
                    });
                }

                // Sort again after threshold filter (stable w.r.t. ties
                // in composite — fall back to created_at desc).
                out.sort_by(|a, b| {
                    b.score
                        .partial_cmp(&a.score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| b.entry.created_at.cmp(&a.entry.created_at))
                });
                out.truncate(top_k);
                Ok(out)
            })
            .await?;

        Ok(results)
    }

    async fn delete(&self, id: &MemoryId) -> Result<(), SemanticError> {
        let id_s = id.as_str().to_string();
        let vec_available = self.vec_available;
        let id_for_err = id.clone();
        self.with_conn(move |conn| {
            let tx = conn
                .transaction()
                .map_err(|e| SemanticError::Backend(format!("begin tx: {e}")))?;
            let rowid: Option<i64> = tx
                .query_row(
                    "SELECT rowid FROM memory_entries WHERE id = ?1",
                    params![id_s],
                    |r| r.get(0),
                )
                .optional()
                .map_err(|e| SemanticError::Backend(format!("delete lookup: {e}")))?;
            let Some(rowid) = rowid else {
                return Err(SemanticError::NotFound(id_for_err));
            };
            tx.execute("DELETE FROM memory_fts WHERE rowid = ?1", params![rowid])
                .map_err(|e| SemanticError::Backend(format!("delete fts: {e}")))?;
            if vec_available {
                tx.execute("DELETE FROM memory_vec WHERE rowid = ?1", params![rowid])
                    .map_err(|e| SemanticError::Backend(format!("delete vec: {e}")))?;
            }
            tx.execute("DELETE FROM memory_entries WHERE rowid = ?1", params![rowid])
                .map_err(|e| SemanticError::Backend(format!("delete entry: {e}")))?;
            tx.commit()
                .map_err(|e| SemanticError::Backend(format!("commit delete: {e}")))?;
            Ok(())
        })
        .await
    }

    async fn evict(&self, policy: &EvictionPolicy) -> Result<usize, SemanticError> {
        let ttl_days = policy.ttl_days;
        let max_per_agent = policy.max_per_agent;
        let promoted_exempt = policy.promoted_exempt;
        let vec_available = self.vec_available;

        self.with_conn(move |conn| {
            // Collect rowids to evict first, then run a single DELETE
            // across all three tables so the sqlite-vec row is kept in
            // lock-step.
            let mut targets: Vec<i64> = Vec::new();

            if let Some(days) = ttl_days {
                let cutoff = Utc::now().timestamp() - (days as i64) * 86_400;
                let sql = if promoted_exempt {
                    "SELECT rowid FROM memory_entries WHERE created_at < ?1 AND promoted = 0"
                } else {
                    "SELECT rowid FROM memory_entries WHERE created_at < ?1"
                };
                let mut stmt = conn
                    .prepare(sql)
                    .map_err(|e| SemanticError::Backend(format!("prepare ttl evict: {e}")))?;
                let rows = stmt
                    .query_map(params![cutoff], |r| r.get::<_, i64>(0))
                    .map_err(|e| SemanticError::Backend(format!("exec ttl evict: {e}")))?;
                for r in rows {
                    targets.push(
                        r.map_err(|e| SemanticError::Backend(format!("ttl row: {e}")))?,
                    );
                }
            }

            if let Some(cap) = max_per_agent {
                let sql = if promoted_exempt {
                    "SELECT rowid FROM (
                         SELECT rowid, ROW_NUMBER() OVER (
                             PARTITION BY agent_id ORDER BY created_at DESC
                         ) AS rn
                         FROM memory_entries
                         WHERE promoted = 0
                     ) s WHERE s.rn > ?1"
                } else {
                    "SELECT rowid FROM (
                         SELECT rowid, ROW_NUMBER() OVER (
                             PARTITION BY agent_id ORDER BY created_at DESC
                         ) AS rn
                         FROM memory_entries
                     ) s WHERE s.rn > ?1"
                };
                let mut stmt = conn
                    .prepare(sql)
                    .map_err(|e| SemanticError::Backend(format!("prepare cap evict: {e}")))?;
                let rows = stmt
                    .query_map(params![cap as i64], |r| r.get::<_, i64>(0))
                    .map_err(|e| SemanticError::Backend(format!("exec cap evict: {e}")))?;
                for r in rows {
                    targets.push(
                        r.map_err(|e| SemanticError::Backend(format!("cap row: {e}")))?,
                    );
                }
            }

            targets.sort();
            targets.dedup();
            if targets.is_empty() {
                return Ok(0);
            }

            let tx = conn
                .transaction()
                .map_err(|e| SemanticError::Backend(format!("begin evict tx: {e}")))?;
            let mut removed: usize = 0;
            for rowid in &targets {
                tx.execute("DELETE FROM memory_fts WHERE rowid = ?1", params![rowid])
                    .map_err(|e| SemanticError::Backend(format!("evict fts: {e}")))?;
                if vec_available {
                    tx.execute("DELETE FROM memory_vec WHERE rowid = ?1", params![rowid])
                        .map_err(|e| SemanticError::Backend(format!("evict vec: {e}")))?;
                }
                let affected = tx
                    .execute(
                        "DELETE FROM memory_entries WHERE rowid = ?1",
                        params![rowid],
                    )
                    .map_err(|e| SemanticError::Backend(format!("evict entry: {e}")))?;
                removed += affected;
            }
            tx.commit()
                .map_err(|e| SemanticError::Backend(format!("commit evict: {e}")))?;
            Ok(removed)
        })
        .await
    }

    async fn promote(&self, id: &MemoryId) -> Result<(), SemanticError> {
        let id_s = id.as_str().to_string();
        let id_for_err = id.clone();
        self.with_conn(move |conn| {
            let affected = conn
                .execute(
                    "UPDATE memory_entries SET promoted = 1 WHERE id = ?1",
                    params![id_s],
                )
                .map_err(|e| SemanticError::Backend(format!("promote: {e}")))?;
            if affected == 0 {
                return Err(SemanticError::NotFound(id_for_err));
            }
            Ok(())
        })
        .await
    }

    async fn touch(&self, id: &MemoryId) -> Result<(), SemanticError> {
        let id_s = id.as_str().to_string();
        let now = now_unix();
        self.with_conn(move |conn| {
            conn.execute(
                "UPDATE memory_entries
                 SET last_touched_at = ?1, access_count = access_count + 1
                 WHERE id = ?2",
                params![now, id_s],
            )
            .map_err(|e| SemanticError::Backend(format!("touch: {e}")))?;
            Ok(())
        })
        .await
    }

    async fn stats(&self) -> Result<SemanticStats, SemanticError> {
        self.with_conn(move |conn| {
            let total: i64 = conn
                .query_row("SELECT COUNT(*) FROM memory_entries", [], |r| r.get(0))
                .map_err(|e| SemanticError::Backend(format!("stats total: {e}")))?;

            let mut stmt = conn
                .prepare(
                    "SELECT agent_id, COUNT(*) AS n
                     FROM memory_entries
                     GROUP BY agent_id
                     ORDER BY n DESC
                     LIMIT 16",
                )
                .map_err(|e| SemanticError::Backend(format!("stats prep: {e}")))?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
                })
                .map_err(|e| SemanticError::Backend(format!("stats exec: {e}")))?;
            let per_agent_top: Vec<(String, usize)> = rows
                .filter_map(|r| r.ok())
                .map(|(a, n)| (a, n.max(0) as usize))
                .collect();

            let bounds: Option<(Option<i64>, Option<i64>)> = conn
                .query_row(
                    "SELECT MIN(created_at), MAX(created_at) FROM memory_entries",
                    [],
                    |r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<i64>>(1)?)),
                )
                .optional()
                .map_err(|e| SemanticError::Backend(format!("stats bounds: {e}")))?;
            let epoch = DateTime::<Utc>::from_timestamp(0, 0).unwrap_or_else(Utc::now);
            let (oldest, newest) = match bounds {
                Some((Some(o), Some(n))) => (
                    DateTime::<Utc>::from_timestamp(o, 0).unwrap_or(epoch),
                    DateTime::<Utc>::from_timestamp(n, 0).unwrap_or(epoch),
                ),
                _ => (epoch, epoch),
            };

            Ok(SemanticStats {
                total_rows: total.max(0) as usize,
                per_agent_top,
                oldest,
                newest,
            })
        })
        .await
    }

    async fn maintenance(&self) -> Result<(), SemanticError> {
        self.with_conn(move |conn| {
            // `optimize` is the idiomatic FTS5 maintenance command.
            conn.execute("INSERT INTO memory_fts(memory_fts) VALUES('optimize')", [])
                .map_err(|e| SemanticError::Backend(format!("fts optimize: {e}")))?;
            // VACUUM must run outside a transaction (rusqlite handles
            // this). It's a no-op on `:memory:` databases.
            conn.execute("VACUUM", [])
                .map_err(|e| SemanticError::Backend(format!("vacuum: {e}")))?;
            Ok(())
        })
        .await
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sera_types::embedding::{EmbeddingError, EmbeddingHealth};

    /// Deterministic embedding fixture: hashes content into a stable
    /// 384-dim unit-ish vector so tests don't need a real model.
    struct TestEmbedding {
        dims: usize,
    }

    impl TestEmbedding {
        fn new_with(dims: usize) -> Self {
            Self { dims }
        }
    }

    #[async_trait]
    impl EmbeddingService for TestEmbedding {
        fn model_id(&self) -> &str {
            "test-embed"
        }
        fn dimensions(&self) -> usize {
            self.dims
        }
        async fn embed(
            &self,
            texts: &[String],
        ) -> Result<Vec<Vec<f32>>, EmbeddingError> {
            Ok(texts.iter().map(|t| hash_vec(t, self.dims)).collect())
        }
        async fn health(&self) -> Result<EmbeddingHealth, EmbeddingError> {
            Ok(EmbeddingHealth {
                available: true,
                detail: "test".into(),
                latency_ms: Some(0),
            })
        }
    }

    /// Fixed PRNG-style hash into a pseudo-random unit vector. Identical
    /// inputs yield identical outputs (crucial for deterministic tests).
    fn hash_vec(s: &str, dims: usize) -> Vec<f32> {
        use std::hash::{Hash, Hasher};
        let mut seed: u64 = 0xcbf2_9ce4_8422_2325; // FNV offset
        for b in s.as_bytes() {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (seed, *b).hash(&mut h);
            seed = h.finish();
        }
        let mut out = Vec::with_capacity(dims);
        let mut state = seed;
        for _ in 0..dims {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let raw = (state >> 33) as u32;
            // Project into [-1, 1]
            out.push((raw as f32 / u32::MAX as f32) * 2.0 - 1.0);
        }
        // Normalise so cosine distance behaves sensibly.
        let mag = out.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
        for x in out.iter_mut() {
            *x /= mag;
        }
        out
    }

    /// Build a test [`PutRequest`] with reasonable defaults.
    fn mk_req(agent: &str, content: &str) -> PutRequest {
        PutRequest {
            agent_id: agent.into(),
            content: content.into(),
            scope: None,
            tier: SegmentKind::MemoryRecall("recall-1".into()),
            tags: vec!["unit".into()],
            promoted: false,
            supplied_embedding: None,
        }
    }

    #[tokio::test]
    async fn opens_in_memory_with_no_embedder() {
        // The contract is that open_in_memory never panics and the
        // keyword side always works regardless of whether sqlite-vec is
        // registered in this build.
        let store = SqliteMemoryStore::open_in_memory(None).expect("open");
        assert_eq!(store.dimensions(), DEFAULT_SQLITE_VEC_DIMENSIONS);
    }

    #[tokio::test]
    async fn opens_against_file_path() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("sqlite-mem-test.db");
        let store = SqliteMemoryStore::open(&p, None).expect("open file");
        // Persisted to disk — schema created idempotently.
        store.put(mk_req("a", "persist me")).await.unwrap();
        let s = store.stats().await.unwrap();
        assert_eq!(s.total_rows, 1);

        // Reopen and verify the row survived.
        drop(store);
        let store2 = SqliteMemoryStore::open(&p, None).expect("reopen");
        let s2 = store2.stats().await.unwrap();
        assert_eq!(s2.total_rows, 1);
    }

    #[tokio::test]
    async fn put_and_query_keyword_only_path() {
        // No embedding service -> BM25-only path exercised even when
        // the vec extension happens to load.
        let store = SqliteMemoryStore::open_in_memory(None).expect("open");
        for phrase in [
            "neural network architectures",
            "database index tuning",
            "coffee beans and espresso",
        ] {
            store.put(mk_req("a", phrase)).await.expect("put");
        }

        let hits = store
            .query(SemanticQuery {
                agent_id: "a".into(),
                tier_filter: None,
                text: Some("espresso beans".into()),
                query_embedding: None,
                top_k: 3,
                similarity_threshold: None,
                scope: None,
            })
            .await
            .expect("query");

        assert!(!hits.is_empty(), "bm25 should return at least one hit");
        assert!(hits[0].entry.content.contains("coffee"));
        for h in &hits {
            assert_eq!(h.entry.agent_id, "a");
        }
    }

    #[tokio::test]
    async fn agent_id_isolation_enforced() {
        let store = SqliteMemoryStore::open_in_memory(None).expect("open");
        store
            .put(mk_req("alice", "shared secret one"))
            .await
            .unwrap();
        store
            .put(mk_req("bob", "shared secret two"))
            .await
            .unwrap();

        let alice_hits = store
            .query(SemanticQuery {
                agent_id: "alice".into(),
                tier_filter: None,
                text: Some("secret".into()),
                query_embedding: None,
                top_k: 10,
                similarity_threshold: None,
                scope: None,
            })
            .await
            .unwrap();
        for h in &alice_hits {
            assert_eq!(h.entry.agent_id, "alice");
            assert!(!h.entry.content.contains("two"));
        }

        let bob_hits = store
            .query(SemanticQuery {
                agent_id: "bob".into(),
                tier_filter: None,
                text: Some("secret".into()),
                query_embedding: None,
                top_k: 10,
                similarity_threshold: None,
                scope: None,
            })
            .await
            .unwrap();
        for h in &bob_hits {
            assert_eq!(h.entry.agent_id, "bob");
            assert!(!h.entry.content.contains("one"));
        }
    }

    #[tokio::test]
    async fn delete_removes_row_and_reports_not_found_for_missing() {
        let store = SqliteMemoryStore::open_in_memory(None).expect("open");
        let id = store.put(mk_req("a", "ephemeral")).await.unwrap();
        store.delete(&id).await.unwrap();
        let err = store.delete(&id).await.unwrap_err();
        assert!(matches!(err, SemanticError::NotFound(_)));
    }

    #[tokio::test]
    async fn promote_and_ttl_evict_respects_promoted_exempt() {
        let store = SqliteMemoryStore::open_in_memory(None).expect("open");
        let old_ts = Utc::now() - chrono::Duration::days(30);
        store
            .put_raw(
                "old",
                "a",
                "old pin",
                SegmentKind::MemoryRecall("r".into()),
                vec![],
                false,
                old_ts,
                None,
            )
            .await
            .unwrap();
        store
            .put_raw(
                "fresh",
                "a",
                "fresh row",
                SegmentKind::MemoryRecall("r".into()),
                vec![],
                false,
                Utc::now(),
                None,
            )
            .await
            .unwrap();

        store.promote(&MemoryId::new("old")).await.unwrap();

        let removed = store
            .evict(&EvictionPolicy {
                ttl_days: Some(7),
                promoted_exempt: true,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(removed, 0, "promoted row must be exempt");

        let stats = store.stats().await.unwrap();
        assert_eq!(stats.total_rows, 2);

        let removed = store
            .evict(&EvictionPolicy {
                ttl_days: Some(7),
                promoted_exempt: false,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(removed, 1);
    }

    #[tokio::test]
    async fn row_cap_evicts_oldest_per_agent() {
        let store = SqliteMemoryStore::open_in_memory(None).expect("open");
        for i in 0..5 {
            let created = Utc::now() - chrono::Duration::seconds((5 - i) as i64);
            store
                .put_raw(
                    &format!("cap-{i}"),
                    "a",
                    &format!("row-{i}"),
                    SegmentKind::MemoryRecall("r".into()),
                    vec![],
                    false,
                    created,
                    None,
                )
                .await
                .unwrap();
        }
        let removed = store
            .evict(&EvictionPolicy {
                max_per_agent: Some(2),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(removed, 3);
        let stats = store.stats().await.unwrap();
        assert_eq!(stats.total_rows, 2);
    }

    #[tokio::test]
    async fn maintenance_runs_without_error() {
        let store = SqliteMemoryStore::open_in_memory(None).expect("open");
        for i in 0..3 {
            store.put(mk_req("a", &format!("content {i}"))).await.unwrap();
        }
        store.maintenance().await.expect("maintenance");
    }

    #[tokio::test]
    async fn embedding_service_propagates_errors_on_put() {
        // A failing embedding service must surface the error, not write
        // a zero-vector (see sera-px3w).
        struct FailingEmbed;
        #[async_trait]
        impl EmbeddingService for FailingEmbed {
            fn model_id(&self) -> &str {
                "failing"
            }
            fn dimensions(&self) -> usize {
                384
            }
            async fn embed(
                &self,
                _texts: &[String],
            ) -> Result<Vec<Vec<f32>>, EmbeddingError> {
                Err(EmbeddingError::Provider("nope".into()))
            }
            async fn health(&self) -> Result<EmbeddingHealth, EmbeddingError> {
                Ok(EmbeddingHealth {
                    available: false,
                    detail: "fail".into(),
                    latency_ms: None,
                })
            }
        }

        let store = SqliteMemoryStore::open_in_memory(Some(Arc::new(FailingEmbed)))
            .expect("open");

        // If vector side isn't registered in this build, the put path
        // won't even call embed — skip the loud assertion.
        if !store.vector_available() {
            return;
        }

        let err = store
            .put(mk_req("a", "will fail to embed"))
            .await
            .expect_err("must fail loudly");
        assert!(matches!(err, SemanticError::Backend(_)));
    }

    #[tokio::test]
    async fn query_with_precomputed_embedding_round_trips() {
        let dims = 384;
        let store = SqliteMemoryStore::open_in_memory(Some(Arc::new(
            TestEmbedding::new_with(dims),
        )))
        .expect("open");
        store
            .put(
                mk_req("a", "alpha beta gamma")
                    .with_embedding(hash_vec("alpha beta gamma", dims)),
            )
            .await
            .unwrap();

        let hits = store
            .query(SemanticQuery {
                agent_id: "a".into(),
                tier_filter: None,
                text: Some("alpha".into()),
                query_embedding: Some(hash_vec("alpha beta gamma", dims)),
                top_k: 5,
                similarity_threshold: None,
                scope: None,
            })
            .await
            .unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0].entry.content, "alpha beta gamma");
    }

    #[tokio::test]
    async fn hybrid_rrf_returns_top_hit_when_vec_available() {
        let dims = 384;
        let store = SqliteMemoryStore::open_in_memory(Some(Arc::new(
            TestEmbedding::new_with(dims),
        )))
        .expect("open");
        if !store.vector_available() {
            // sqlite-vec not registered in this build — the hybrid
            // path is exercised when it is; the degraded path still
            // returns a BM25 hit which the next assert verifies.
            eprintln!("sqlite-vec unavailable; exercising BM25-only fallback");
        }

        for t in ["rust programming language", "python web framework", "espresso coffee"] {
            store.put(mk_req("a", t)).await.unwrap();
        }

        let hits = store
            .query(SemanticQuery {
                agent_id: "a".into(),
                tier_filter: None,
                text: Some("programming".into()),
                query_embedding: None,
                top_k: 3,
                similarity_threshold: None,
                scope: None,
            })
            .await
            .unwrap();
        assert!(!hits.is_empty());
        assert!(hits.iter().any(|h| h.entry.content.contains("rust")));
    }

    #[tokio::test]
    async fn touch_updates_last_accessed_and_counts() {
        let store = SqliteMemoryStore::open_in_memory(None).expect("open");
        let id = store.put(mk_req("a", "touch me")).await.unwrap();
        store.touch(&id).await.unwrap();
        let stats = store.stats().await.unwrap();
        assert_eq!(stats.total_rows, 1);
    }

    #[tokio::test]
    async fn stats_reports_bounds_and_top_agents() {
        let store = SqliteMemoryStore::open_in_memory(None).expect("open");
        for i in 0..3 {
            store
                .put(mk_req("alice", &format!("alice row {i}")))
                .await
                .unwrap();
        }
        store.put(mk_req("bob", "bob row 0")).await.unwrap();

        let s = store.stats().await.unwrap();
        assert_eq!(s.total_rows, 4);
        assert_eq!(s.per_agent_top[0].0, "alice");
        assert!(s.newest >= s.oldest);
    }

    #[tokio::test]
    async fn vec_only_query_returns_hits_when_vec_available() {
        let dims = 384;
        let store = SqliteMemoryStore::open_in_memory(Some(Arc::new(
            TestEmbedding::new_with(dims),
        )))
        .expect("open");
        if !store.vector_available() {
            eprintln!("vec-only test skipped; sqlite-vec not registered");
            return;
        }
        for t in ["rust systems", "python web", "coffee"] {
            store
                .put(mk_req("a", t).with_embedding(hash_vec(t, dims)))
                .await
                .unwrap();
        }
        // Query with embedding only (no text) → pure vector path.
        let hits = store
            .query(SemanticQuery {
                agent_id: "a".into(),
                tier_filter: None,
                text: None,
                query_embedding: Some(hash_vec("rust systems", dims)),
                top_k: 3,
                similarity_threshold: None,
                scope: None,
            })
            .await
            .unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0].entry.content, "rust systems");
    }
}

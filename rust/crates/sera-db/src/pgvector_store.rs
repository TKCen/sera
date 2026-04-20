//! Postgres + pgvector implementation of [`SemanticMemoryStore`].
//!
//! Schema is issued inline at store construction via
//! [`PgVectorStore::initialize`] rather than a separate migration file —
//! matching the pattern used by [`crate::lane_queue_counter::PostgresLaneCounter`]
//! and [`crate::proposal_usage::PostgresProposalUsageStore`].
//!
//! ## Schema
//!
//! ```sql
//! CREATE EXTENSION IF NOT EXISTS vector;
//!
//! CREATE TABLE IF NOT EXISTS semantic_memory_entries (
//!     id                UUID        PRIMARY KEY,
//!     agent_id          TEXT        NOT NULL,
//!     content           TEXT        NOT NULL,
//!     embedding         vector(1536) NOT NULL,
//!     tier              JSONB       NOT NULL,
//!     tags              TEXT[]      NOT NULL DEFAULT '{}',
//!     created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
//!     last_accessed_at  TIMESTAMPTZ,
//!     promoted          BOOLEAN     NOT NULL DEFAULT false,
//!     scope_kind        TEXT        NOT NULL DEFAULT 'agent',
//!     scope_key         TEXT        NOT NULL DEFAULT ''
//! );
//!
//! CREATE INDEX IF NOT EXISTS idx_semantic_memory_agent_id
//!     ON semantic_memory_entries (agent_id);
//! CREATE INDEX IF NOT EXISTS idx_semantic_memory_created_at
//!     ON semantic_memory_entries (created_at);
//! CREATE INDEX IF NOT EXISTS idx_semantic_memory_scope
//!     ON semantic_memory_entries (scope_kind, scope_key);
//! CREATE INDEX IF NOT EXISTS idx_semantic_memory_embedding
//!     ON semantic_memory_entries USING ivfflat (embedding vector_cosine_ops)
//!     WITH (lists = 100);
//! ```
//!
//! ## Hierarchical scopes (GH#140)
//!
//! Rows persist the [`Scope`] they belong to via the `scope_kind` / `scope_key`
//! columns. Pre-migration rows self-backfill via the column defaults
//! (`'agent'` / `''`) so callers see them under `Scope::Agent(agent_id)`
//! semantics with an empty key — no backfill query is required.
//!
//! ## Dimensions
//!
//! Dimensions are fixed at table-creation time — pgvector validates them
//! per-row. The default 1536 matches OpenAI's `text-embedding-3-small`
//! (see `sera-runtime::semantic::providers::openai::DEFAULT_OPENAI_DIMENSIONS`).
//! Changing dimensions requires dropping and recreating the table; this
//! store flags mismatches up-front by rejecting rows whose embedding
//! length differs from its configured value.
//!
//! ## Graceful fallback
//!
//! If the `vector` extension is not installed, [`PgVectorStore::initialize`]
//! returns a [`SemanticError::Backend`] with a clear message so the caller
//! can wire in the in-memory backend instead. It does NOT auto-install
//! the extension — that requires superuser privileges we shouldn't assume.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use pgvector::Vector as PgVector;
use sera_types::{
    EmbeddingService, EvictionPolicy, MemoryId, PutRequest, ScoredEntry, Scope, SemanticEntry,
    SemanticError, SemanticMemoryStore, SemanticQuery, SemanticStats, memory::SegmentKind,
};
use sqlx::PgPool;
use sqlx::Row;
use sqlx::types::Json;
use time::OffsetDateTime;
use uuid::Uuid;

/// Convert a `chrono::DateTime<Utc>` to `time::OffsetDateTime`. `sqlx` is
/// compiled with the `time` feature (see workspace Cargo.toml); the
/// public [`SemanticEntry`] uses `chrono` to stay consistent with the
/// rest of `sera-types`, so we convert at the DB boundary.
fn chrono_to_time(dt: DateTime<Utc>) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(dt.timestamp())
        .unwrap_or(OffsetDateTime::UNIX_EPOCH)
        + time::Duration::nanoseconds(dt.timestamp_subsec_nanos() as i64)
}

fn time_to_chrono(dt: OffsetDateTime) -> DateTime<Utc> {
    let secs = dt.unix_timestamp();
    let nsec = dt.nanosecond();
    DateTime::<Utc>::from_timestamp(secs, nsec).unwrap_or_else(Utc::now)
}

/// Default embedding dimensionality — matches OpenAI `text-embedding-3-small`.
pub const DEFAULT_SEMANTIC_DIMENSIONS: usize = 1536;

/// Postgres + pgvector-backed [`SemanticMemoryStore`].
///
/// `embedding_service` is optional. When set, `put` with
/// `supplied_embedding = None` embeds the content server-side before
/// writing. When unset, `put` requires the caller to pass
/// `supplied_embedding = Some(...)` and returns
/// [`SemanticError::Backend`] otherwise — the pgvector backend does not
/// own embeddings (see SPEC-memory-pluggability §3).
#[derive(Clone)]
pub struct PgVectorStore {
    pool: PgPool,
    dimensions: usize,
    embedding_service: Option<Arc<dyn EmbeddingService>>,
}

impl std::fmt::Debug for PgVectorStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PgVectorStore")
            .field("dimensions", &self.dimensions)
            .field("has_embedder", &self.embedding_service.is_some())
            .finish()
    }
}

impl PgVectorStore {
    /// Wrap an existing [`PgPool`] using the default 1536-dimension schema.
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            dimensions: DEFAULT_SEMANTIC_DIMENSIONS,
            embedding_service: None,
        }
    }

    /// Wrap an existing [`PgPool`] with a custom embedding dimensionality.
    ///
    /// The value is baked into the `embedding vector(N)` column at
    /// [`Self::initialize`] time; subsequent writes whose vector length
    /// disagrees are rejected with
    /// [`SemanticError::DimensionMismatch`].
    pub fn with_dimensions(pool: PgPool, dimensions: usize) -> Self {
        Self {
            pool,
            dimensions,
            embedding_service: None,
        }
    }

    /// Attach an [`EmbeddingService`] so `put` can accept
    /// `PutRequest::supplied_embedding = None` and embed server-side.
    pub fn with_embedding_service(
        mut self,
        embedding: Arc<dyn EmbeddingService>,
    ) -> Self {
        self.embedding_service = Some(embedding);
        self
    }

    /// Borrow the underlying pool (integration tests / admin tooling).
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Configured embedding dimensionality.
    pub fn dimensions(&self) -> usize {
        self.dimensions
    }

    /// Probe the `vector` extension. Returns `Ok(true)` when available,
    /// `Ok(false)` when missing. Network / auth errors surface as
    /// [`SemanticError::Backend`].
    ///
    /// Callers that want a non-fatal fallback (dev environments without
    /// pgvector) should branch on this before calling [`Self::initialize`].
    pub async fn extension_available(&self) -> Result<bool, SemanticError> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT extname FROM pg_extension WHERE extname = 'vector'",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SemanticError::Backend(format!("extension probe failed: {e}")))?;
        Ok(row.is_some())
    }

    /// Issue the DDL idempotently. If the `vector` extension cannot be
    /// created (e.g. not installed at the cluster level) this returns
    /// [`SemanticError::Backend`] so callers can fall back to the
    /// in-memory store.
    pub async fn initialize(&self) -> Result<(), SemanticError> {
        sqlx::query("CREATE EXTENSION IF NOT EXISTS vector")
            .execute(&self.pool)
            .await
            .map_err(|e| {
                SemanticError::Backend(format!(
                    "failed to create pgvector extension (install it or fall back to InMemorySemanticStore): {e}"
                ))
            })?;

        let table_ddl = format!(
            "CREATE TABLE IF NOT EXISTS semantic_memory_entries (
                id                UUID        PRIMARY KEY,
                agent_id          TEXT        NOT NULL,
                content           TEXT        NOT NULL,
                embedding         vector({dims}) NOT NULL,
                tier              JSONB       NOT NULL,
                tags              TEXT[]      NOT NULL DEFAULT '{{}}',
                created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
                last_accessed_at  TIMESTAMPTZ,
                promoted          BOOLEAN     NOT NULL DEFAULT false,
                scope_kind        TEXT        NOT NULL DEFAULT 'agent',
                scope_key         TEXT        NOT NULL DEFAULT ''
            )",
            dims = self.dimensions
        );
        sqlx::query(&table_ddl)
            .execute(&self.pool)
            .await
            .map_err(|e| SemanticError::Backend(format!("create semantic_memory_entries: {e}")))?;

        // Additive migration for pre-GH#140 deployments: these are no-ops on
        // fresh tables thanks to `IF NOT EXISTS`. Pre-migration rows
        // self-backfill via the column defaults — no UPDATE required.
        sqlx::query(
            "ALTER TABLE semantic_memory_entries
                ADD COLUMN IF NOT EXISTS scope_kind TEXT NOT NULL DEFAULT 'agent'",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| SemanticError::Backend(format!("add scope_kind column: {e}")))?;

        sqlx::query(
            "ALTER TABLE semantic_memory_entries
                ADD COLUMN IF NOT EXISTS scope_key TEXT NOT NULL DEFAULT ''",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| SemanticError::Backend(format!("add scope_key column: {e}")))?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_semantic_memory_agent_id
             ON semantic_memory_entries (agent_id)",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| SemanticError::Backend(format!("create agent_id index: {e}")))?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_semantic_memory_created_at
             ON semantic_memory_entries (created_at)",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| SemanticError::Backend(format!("create created_at index: {e}")))?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_semantic_memory_scope
             ON semantic_memory_entries (scope_kind, scope_key)",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| SemanticError::Backend(format!("create scope index: {e}")))?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_semantic_memory_embedding
             ON semantic_memory_entries USING ivfflat (embedding vector_cosine_ops)
             WITH (lists = 100)",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| SemanticError::Backend(format!("create ivfflat index: {e}")))?;

        Ok(())
    }

    /// Translate a [`Scope`] to its SQL `(scope_kind, scope_key)` tuple.
    ///
    /// Thin wrapper around [`Scope::kind_str`] + [`Scope::key_str`] kept
    /// local for test-coverage reporting (GH#140). Public so integration
    /// harnesses can drive the same mapping without re-implementing it.
    pub fn scope_to_sql(scope: &Scope) -> (&'static str, &str) {
        (scope.kind_str(), scope.key_str())
    }

    fn validate_dims(&self, v: &[f32]) -> Result<(), SemanticError> {
        if v.len() != self.dimensions {
            return Err(SemanticError::DimensionMismatch {
                expected: self.dimensions,
                got: v.len(),
            });
        }
        Ok(())
    }

    fn parse_id(id: &MemoryId) -> Result<Uuid, SemanticError> {
        Uuid::parse_str(id.as_str())
            .map_err(|e| SemanticError::Serialization(format!("invalid UUID '{id}': {e}")))
    }
}

/// Internal helper — reconstruct a [`SemanticEntry`] from a query row.
fn row_to_entry(row: &sqlx::postgres::PgRow) -> Result<SemanticEntry, SemanticError> {
    let id: Uuid = row
        .try_get("id")
        .map_err(|e| SemanticError::Backend(format!("row.id: {e}")))?;
    let agent_id: String = row
        .try_get("agent_id")
        .map_err(|e| SemanticError::Backend(format!("row.agent_id: {e}")))?;
    let content: String = row
        .try_get("content")
        .map_err(|e| SemanticError::Backend(format!("row.content: {e}")))?;
    let embedding: PgVector = row
        .try_get("embedding")
        .map_err(|e| SemanticError::Backend(format!("row.embedding: {e}")))?;
    let tier: Json<SegmentKind> = row
        .try_get("tier")
        .map_err(|e| SemanticError::Backend(format!("row.tier: {e}")))?;
    let tags: Vec<String> = row
        .try_get("tags")
        .map_err(|e| SemanticError::Backend(format!("row.tags: {e}")))?;
    let created_at: OffsetDateTime = row
        .try_get("created_at")
        .map_err(|e| SemanticError::Backend(format!("row.created_at: {e}")))?;
    let last_accessed_at: Option<OffsetDateTime> = row
        .try_get("last_accessed_at")
        .map_err(|e| SemanticError::Backend(format!("row.last_accessed_at: {e}")))?;
    let promoted: bool = row
        .try_get("promoted")
        .map_err(|e| SemanticError::Backend(format!("row.promoted: {e}")))?;

    let scope_kind: String = row
        .try_get("scope_kind")
        .map_err(|e| SemanticError::Backend(format!("row.scope_kind: {e}")))?;
    let scope_key: String = row
        .try_get("scope_key")
        .map_err(|e| SemanticError::Backend(format!("row.scope_key: {e}")))?;
    let scope = Scope::from_parts(&scope_kind, &scope_key).ok();

    Ok(SemanticEntry {
        id: MemoryId::new(id.to_string()),
        agent_id,
        content,
        embedding: Some(embedding.to_vec()),
        tier: tier.0,
        tags,
        created_at: time_to_chrono(created_at),
        last_accessed_at: last_accessed_at.map(time_to_chrono),
        promoted,
        scope,
    })
}

#[async_trait::async_trait]
impl SemanticMemoryStore for PgVectorStore {
    async fn put(&self, req: PutRequest) -> Result<MemoryId, SemanticError> {
        // Resolve the embedding. Precedence: caller-supplied → bound
        // EmbeddingService → error. pgvector does not silently store a
        // zero vector (see sera-px3w).
        let embedding_vec: Vec<f32> = match req.supplied_embedding {
            Some(v) => v,
            None => match self.embedding_service.as_ref() {
                Some(svc) => {
                    let vecs = svc
                        .embed(std::slice::from_ref(&req.content))
                        .await
                        .map_err(|e| {
                            SemanticError::Backend(format!("embed on put: {e}"))
                        })?;
                    vecs.into_iter().next().ok_or_else(|| {
                        SemanticError::Backend(
                            "embed on put returned no vectors".into(),
                        )
                    })?
                }
                None => {
                    return Err(SemanticError::Backend(
                        "no embedding service configured for pgvector put with supplied_embedding=None".into(),
                    ));
                }
            },
        };
        self.validate_dims(&embedding_vec)?;

        let id = Uuid::new_v4();

        let embedding = PgVector::from(embedding_vec);
        let tier = Json(req.tier.clone());

        // GH#140 scope persistence. `None` back-compat maps to
        // `Scope::Agent(agent_id)` so pre-migration callers stay visible
        // under agent-only queries.
        let effective_scope = req
            .scope
            .clone()
            .unwrap_or_else(|| Scope::Agent(req.agent_id.clone()));
        let (scope_kind, scope_key) = (
            effective_scope.kind_str().to_string(),
            effective_scope.key_str().to_string(),
        );

        let now = Utc::now();

        sqlx::query(
            r#"
            INSERT INTO semantic_memory_entries
                (id, agent_id, content, embedding, tier, tags, created_at, last_accessed_at, promoted, scope_kind, scope_key)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            ON CONFLICT (id) DO UPDATE SET
                agent_id         = EXCLUDED.agent_id,
                content          = EXCLUDED.content,
                embedding        = EXCLUDED.embedding,
                tier             = EXCLUDED.tier,
                tags             = EXCLUDED.tags,
                last_accessed_at = EXCLUDED.last_accessed_at,
                promoted         = EXCLUDED.promoted,
                scope_kind       = EXCLUDED.scope_kind,
                scope_key        = EXCLUDED.scope_key
            "#,
        )
        .bind(id)
        .bind(&req.agent_id)
        .bind(&req.content)
        .bind(embedding)
        .bind(tier)
        .bind(&req.tags)
        .bind(chrono_to_time(now))
        .bind::<Option<OffsetDateTime>>(None)
        .bind(req.promoted)
        .bind(scope_kind)
        .bind(scope_key)
        .execute(&self.pool)
        .await
        .map_err(|e| SemanticError::Backend(format!("put insert: {e}")))?;

        Ok(MemoryId::new(id.to_string()))
    }

    async fn query(&self, query: SemanticQuery) -> Result<Vec<ScoredEntry>, SemanticError> {
        let embedding = query
            .query_embedding
            .as_ref()
            .ok_or_else(|| {
                SemanticError::Backend(
                    "PgVectorStore::query requires query.query_embedding (no embed-on-read)".into(),
                )
            })?;
        self.validate_dims(embedding)?;
        let pg_vec = PgVector::from(embedding.clone());

        let top_k = query.top_k.max(1) as i64;

        // GH#140: when the caller pins a `Scope`, filter on
        // `(scope_kind, scope_key)`. Otherwise preserve the pre-hierarchy
        // `agent_id`-only path so back-compat callers stay unchanged.
        let scope_filter: Option<(&'static str, String)> = query
            .scope
            .as_ref()
            .map(|s| (s.kind_str(), s.key_str().to_string()));

        // `embedding <=> $v` returns cosine distance in [0, 2]. We convert
        // to similarity in [-1, 1] via `1 - distance`.
        let where_primary = if scope_filter.is_some() {
            "scope_kind = $2 AND scope_key = $3"
        } else {
            "agent_id = $2"
        };
        // Number of params consumed by the primary filter (for $N offsets).
        let primary_count = if scope_filter.is_some() { 3 } else { 2 };

        let mut sql = format!(
            "SELECT id, agent_id, content, embedding, tier, tags, created_at, last_accessed_at, promoted, scope_kind, scope_key,
                1 - (embedding <=> $1) AS vector_score
             FROM semantic_memory_entries
             WHERE {where_primary}"
        );

        let mut next_param = primary_count + 1;
        if query.tier_filter.is_some() {
            sql.push_str(&format!(" AND tier = ${next_param}"));
            next_param += 1;
        }
        if query.similarity_threshold.is_some() {
            sql.push_str(&format!(" AND (1 - (embedding <=> $1)) >= ${next_param}"));
            next_param += 1;
        }
        sql.push_str(&format!(
            " ORDER BY embedding <=> $1 ASC, created_at DESC LIMIT ${next_param}"
        ));

        let mut builder = sqlx::query(&sql).bind(pg_vec);
        if let Some((kind, key)) = scope_filter.as_ref() {
            builder = builder.bind(*kind).bind(key);
        } else {
            builder = builder.bind(&query.agent_id);
        }
        if let Some(tier) = &query.tier_filter {
            builder = builder.bind(Json(tier.clone()));
        }
        if let Some(thr) = query.similarity_threshold {
            builder = builder.bind(thr);
        }
        builder = builder.bind(top_k);

        let rows = builder
            .fetch_all(&self.pool)
            .await
            .map_err(|e| SemanticError::Backend(format!("query fetch: {e}")))?;

        let now = Utc::now();
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let vector_score: f64 = row
                .try_get("vector_score")
                .map_err(|e| SemanticError::Backend(format!("row.vector_score: {e}")))?;
            let entry = row_to_entry(&row)?;
            let recency_score = recency_norm(entry.created_at, now);
            let vs = vector_score as f32;
            out.push(ScoredEntry {
                entry,
                score: vs,
                index_score: 0.0,
                vector_score: vs,
                recency_score,
            });
        }
        Ok(out)
    }

    async fn delete(&self, id: &MemoryId) -> Result<(), SemanticError> {
        let uuid = Self::parse_id(id)?;
        let result = sqlx::query("DELETE FROM semantic_memory_entries WHERE id = $1")
            .bind(uuid)
            .execute(&self.pool)
            .await
            .map_err(|e| SemanticError::Backend(format!("delete: {e}")))?;
        if result.rows_affected() == 0 {
            return Err(SemanticError::NotFound(id.clone()));
        }
        Ok(())
    }

    async fn evict(&self, policy: &EvictionPolicy) -> Result<usize, SemanticError> {
        let mut removed: u64 = 0;

        if let Some(ttl) = policy.ttl_days {
            let cutoff =
                OffsetDateTime::now_utc() - time::Duration::days(ttl as i64);
            let sql = if policy.promoted_exempt {
                "DELETE FROM semantic_memory_entries WHERE created_at < $1 AND promoted = false"
            } else {
                "DELETE FROM semantic_memory_entries WHERE created_at < $1"
            };
            let r = sqlx::query(sql)
                .bind(cutoff)
                .execute(&self.pool)
                .await
                .map_err(|e| SemanticError::Backend(format!("evict ttl: {e}")))?;
            removed = removed.saturating_add(r.rows_affected());
        }

        if let Some(cap) = policy.max_per_agent {
            let sql = if policy.promoted_exempt {
                "DELETE FROM semantic_memory_entries e
                 WHERE e.promoted = false AND e.id IN (
                   SELECT id FROM (
                     SELECT id, ROW_NUMBER() OVER (
                       PARTITION BY agent_id ORDER BY created_at DESC
                     ) AS rn
                     FROM semantic_memory_entries
                     WHERE promoted = false
                   ) s
                   WHERE s.rn > $1
                 )"
            } else {
                "DELETE FROM semantic_memory_entries e
                 WHERE e.id IN (
                   SELECT id FROM (
                     SELECT id, ROW_NUMBER() OVER (
                       PARTITION BY agent_id ORDER BY created_at DESC
                     ) AS rn
                     FROM semantic_memory_entries
                   ) s
                   WHERE s.rn > $1
                 )"
            };
            let r = sqlx::query(sql)
                .bind(cap as i64)
                .execute(&self.pool)
                .await
                .map_err(|e| SemanticError::Backend(format!("evict cap: {e}")))?;
            removed = removed.saturating_add(r.rows_affected());
        }

        Ok(removed as usize)
    }

    async fn promote(&self, id: &MemoryId) -> Result<(), SemanticError> {
        let uuid = Self::parse_id(id)?;
        let result = sqlx::query(
            "UPDATE semantic_memory_entries SET promoted = true WHERE id = $1",
        )
        .bind(uuid)
        .execute(&self.pool)
        .await
        .map_err(|e| SemanticError::Backend(format!("promote: {e}")))?;
        if result.rows_affected() == 0 {
            return Err(SemanticError::NotFound(id.clone()));
        }
        Ok(())
    }

    async fn touch(&self, id: &MemoryId) -> Result<(), SemanticError> {
        let uuid = Self::parse_id(id)?;
        // NotFound is tolerated at this seam (row may have been evicted
        // between query and touch). Surface only hard backend errors.
        sqlx::query(
            "UPDATE semantic_memory_entries SET last_accessed_at = now() WHERE id = $1",
        )
        .bind(uuid)
        .execute(&self.pool)
        .await
        .map_err(|e| SemanticError::Backend(format!("touch: {e}")))?;
        Ok(())
    }

    async fn maintenance(&self) -> Result<(), SemanticError> {
        // REINDEX INDEX CONCURRENTLY cannot run inside a transaction.
        // sqlx::execute with the default pool handles this correctly.
        sqlx::query("REINDEX INDEX CONCURRENTLY idx_semantic_memory_embedding")
            .execute(&self.pool)
            .await
            .map_err(|e| SemanticError::Backend(format!("reindex: {e}")))?;
        Ok(())
    }

    async fn stats(&self) -> Result<SemanticStats, SemanticError> {
        let (total,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM semantic_memory_entries")
                .fetch_one(&self.pool)
                .await
                .map_err(|e| SemanticError::Backend(format!("stats count: {e}")))?;

        let per_agent: Vec<(String, i64)> = sqlx::query_as(
            "SELECT agent_id, COUNT(*)::BIGINT
             FROM semantic_memory_entries
             GROUP BY agent_id
             ORDER BY COUNT(*) DESC
             LIMIT 16",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| SemanticError::Backend(format!("stats per_agent: {e}")))?;

        let bounds: Option<(Option<OffsetDateTime>, Option<OffsetDateTime>)> = sqlx::query_as(
            "SELECT MIN(created_at), MAX(created_at) FROM semantic_memory_entries",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SemanticError::Backend(format!("stats bounds: {e}")))?;

        let (oldest, newest) = match bounds {
            Some((Some(o), Some(n))) => (time_to_chrono(o), time_to_chrono(n)),
            _ => {
                let epoch = DateTime::<Utc>::from_timestamp(0, 0).unwrap_or_else(Utc::now);
                (epoch, epoch)
            }
        };

        Ok(SemanticStats {
            total_rows: total.max(0) as usize,
            per_agent_top: per_agent
                .into_iter()
                .map(|(a, c)| (a, c.max(0) as usize))
                .collect(),
            oldest,
            newest,
        })
    }
}

/// Normalise `created_at` into a `[0, 1]` recency score. A row created
/// right now scores `1.0`; a row created ≥14 days ago scores `0.0`.
fn recency_norm(created_at: DateTime<Utc>, now: DateTime<Utc>) -> f32 {
    const HALF_LIFE_DAYS: f32 = 14.0;
    let age_days = (now - created_at).num_seconds().max(0) as f32 / 86_400.0;
    (1.0 - age_days / HALF_LIFE_DAYS).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recency_norm_monotonic() {
        let now = Utc::now();
        let fresh = recency_norm(now, now);
        let a_bit_old = recency_norm(now - chrono::Duration::days(1), now);
        let very_old = recency_norm(now - chrono::Duration::days(30), now);
        assert!(fresh > a_bit_old);
        assert!(a_bit_old > very_old);
        assert_eq!(very_old, 0.0);
    }

    #[test]
    fn parse_id_rejects_non_uuid() {
        let err = PgVectorStore::parse_id(&MemoryId::new("not-a-uuid")).unwrap_err();
        assert!(matches!(err, SemanticError::Serialization(_)));
    }

    #[test]
    fn parse_id_accepts_uuid() {
        let uuid = Uuid::new_v4();
        let id = MemoryId::new(uuid.to_string());
        assert_eq!(PgVectorStore::parse_id(&id).unwrap(), uuid);
    }

    // GH#140 — Scope → SQL mapping unit tests. DB-backed tests for the
    // round-trip live under `#[cfg(feature = "integration")]` in the sqlx
    // fixtures; the mapping helper itself needs no Postgres so it stays in
    // unit-test land.

    #[test]
    fn scope_to_sql_agent_carries_key() {
        let s = Scope::Agent("agent-42".into());
        let (kind, key) = PgVectorStore::scope_to_sql(&s);
        assert_eq!(kind, "agent");
        assert_eq!(key, "agent-42");
    }

    #[test]
    fn scope_to_sql_circle_carries_key() {
        let s = Scope::Circle("ring-1".into());
        let (kind, key) = PgVectorStore::scope_to_sql(&s);
        assert_eq!(kind, "circle");
        assert_eq!(key, "ring-1");
    }

    #[test]
    fn scope_to_sql_org_carries_key() {
        let s = Scope::Org("acme".into());
        let (kind, key) = PgVectorStore::scope_to_sql(&s);
        assert_eq!(kind, "org");
        assert_eq!(key, "acme");
    }

    #[test]
    fn scope_to_sql_global_has_empty_key() {
        let s = Scope::Global;
        let (kind, key) = PgVectorStore::scope_to_sql(&s);
        assert_eq!(kind, "global");
        assert_eq!(key, "");
    }

    #[test]
    fn scope_roundtrip_via_from_parts() {
        for s in [
            Scope::Agent("a".into()),
            Scope::Circle("c".into()),
            Scope::Org("o".into()),
            Scope::Global,
        ] {
            let (kind, key) = PgVectorStore::scope_to_sql(&s);
            let back = Scope::from_parts(kind, key).unwrap();
            assert_eq!(back, s);
        }
    }
}

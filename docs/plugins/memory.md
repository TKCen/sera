# SemanticMemoryStore Plugin Contract

> **Purpose:** User-facing plugin contract for implementing custom semantic memory backends.
> **Reference:** [`ARCHITECTURE-2.0.md §5`](../plan/ARCHITECTURE-2.0.md) (memory tiers overview) · [`SPEC-memory.md`](../plan/specs/SPEC-memory.md) (full memory system spec).

---

## 1. Overview

SERA's memory is pluggable at the **Tier-1 semantic search layer**. The trait `sera_types::semantic_memory::SemanticMemoryStore` is the extensibility seam.

Users can implement custom backends for:

- **mem0 HTTP** — cloud-based memory service
- **Hindsight local** — lightweight local vector search
- **HTTP RAG** — external retrieval-augmented generation
- **Vector DB alternatives** — Milvus, Weaviate, Pinecone, or any VectorStore
- **Knowledge graphs** — Neo4j, RDF triple stores, or custom graph backends

SERA provides two built-in implementations:

- **`SqliteFtsMemoryStore`** — SQLite FTS5 + sqlite-vec. Single-file, zero external deps. Landing in bead sera-vzce.
- **`PgVectorStore`** — PostgreSQL + pgvector extension. Multi-node, HNSW/IVFFlat indexes. Shipped in sera-db.

---

## 2. Trait Signature

```rust
use async_trait::async_trait;
use sera_types::semantic_memory::{
    MemoryId, SemanticEntry, SemanticQuery, ScoredEntry, EvictionPolicy, SemanticStats, SemanticError,
};

/// Tier-1 semantic-memory backend.
///
/// Implementations live in user crates or as dynamically-loaded plugins.
/// The trait is the contract — all backends must implement these methods identically.
///
/// Callers should hold a `Box<dyn SemanticMemoryStore>` or `Arc<dyn SemanticMemoryStore>`
/// and depend only on this trait.
#[async_trait]
pub trait SemanticMemoryStore: Send + Sync + 'static {
    /// Persist `entry` and return its canonical [`MemoryId`]. If
    /// `entry.id` is already populated, backends SHOULD use that value
    /// (useful for idempotent writes from replays).
    ///
    /// **Contract:**
    /// - Idempotent: calling `put()` twice with the same `entry.id` returns the same id.
    /// - Embedding: if the backend has an [`EmbeddingService`] bound, automatically embed
    ///   the `entry.content` and store the resulting vector alongside the text.
    /// - Error policy: MUST fail loudly. Do not emit `vec![0.0; dims]` on embed failure.
    ///   Return `SemanticError::Backend` with the root cause.
    async fn put(&self, entry: SemanticEntry) -> Result<MemoryId, SemanticError>;

    /// Scoped similarity search. Results are ordered by descending [`ScoredEntry::score`]
    /// with ties broken by `created_at` desc.
    ///
    /// **Contract:**
    /// - Multi-tenant isolation: MUST filter on `query.agent_id` first.
    ///   No cross-agent leakage under any circumstances.
    /// - Hybrid scoring: combine lexical (text) and vector (embedding) signals when both are available.
    ///   Each `ScoredEntry` MUST populate at least one of `index_score` or `vector_score`.
    /// - Threshold: if `query.similarity_threshold` is set, drop rows below that score before `top_k` truncation.
    /// - Tier filtering: if `query.tier_filter` is set, only match rows whose `tier` equals that value.
    /// - Embedding: if `query.query_embedding` is None and `query.text` is provided, the backend
    ///   MAY call its bound [`EmbeddingService`] to embed the query text. Naked backends (no embedding bound)
    ///   return `SemanticError::Backend` if neither field is supplied.
    /// - Top-K: return at most `query.top_k` rows, ordered by score descending.
    async fn query(&self, query: SemanticQuery) -> Result<Vec<ScoredEntry>, SemanticError>;

    /// Remove the row identified by `id`. Returns [`SemanticError::NotFound`] if the id
    /// is not in the store. All other errors return `SemanticError::Backend`.
    async fn delete(&self, id: &MemoryId) -> Result<(), SemanticError>;

    /// Apply `policy` to the store and return the number of rows removed.
    ///
    /// **Contract:**
    /// - Multi-agent: each agent's row count is capped independently by `policy.max_per_agent`.
    /// - TTL: rows older than `policy.ttl_days` are removed.
    /// - Promotion: if `policy.promoted_exempt` is true, rows with `entry.promoted = true`
    ///   are skipped by BOTH the row-cap and TTL passes.
    /// - Composability: all fields compose (AND semantics). If `max_per_agent` is set but
    ///   `ttl_days` is None, only row-cap eviction runs.
    async fn evict(&self, policy: &EvictionPolicy) -> Result<usize, SemanticError>;

    /// Return a fresh aggregate snapshot for operator dashboards.
    ///
    /// **Contract:**
    /// - Cheap: backend MAY use approximations (e.g., table statistics) if exact counts are expensive.
    /// - Per-agent top-k: `stats.per_agent_top` is the top N agents by row count (descending).
    ///   N is implementation-defined (e.g., top 10). If the store is empty, all counts are 0.
    /// - Timestamps: `oldest` and `newest` are the `created_at` range across all rows. If empty,
    ///   both default to epoch (1970-01-01T00:00:00Z).
    async fn stats(&self) -> Result<SemanticStats, SemanticError>;

    /// Mark the row identified by `id` as promoted. Promoted rows are
    /// exempt from eviction policies with `promoted_exempt = true` and
    /// serve as persistent recall candidates surfaced by the
    /// dreaming-workflow consolidation pass.
    ///
    /// Returns [`SemanticError::NotFound`] if the id is not in the store.
    ///
    /// **Contract:**
    /// - Atomic: the default implementation is a load-modify-put (not atomic).
    ///   Backends with better primitives (e.g., SQL `UPDATE`) SHOULD override
    ///   to provide atomic semantics.
    /// - Idempotent: calling `promote()` on an already-promoted row is a no-op.
    /// - Visibility: once promoted, the row's `last_accessed_at` is updated to reflect promotion.
    async fn promote(&self, id: &MemoryId) -> Result<(), SemanticError> {
        let _ = id;
        Err(SemanticError::Backend(
            "promote() not implemented for this backend".into(),
        ))
    }

    /// Update `last_accessed_at` for the given row. Called by the
    /// memory-search tool on every hit.
    ///
    /// Default impl returns `Ok(())` — backends that persist access
    /// timestamps SHOULD override. NotFound is tolerated here (the row
    /// may have been evicted between query and touch) to keep the tool
    /// pure-read from the caller's perspective.
    ///
    /// **Contract:**
    /// - Opportunistic: a failed touch (e.g., row evicted) is not an error.
    ///   Silently succeed. The `memory-search` tool must not fail on stale row IDs.
    /// - Recency: backends that track `last_accessed_at` can use it to weight
    ///   `recency_score` in hybrid scoring.
    async fn touch(&self, id: &MemoryId) -> Result<(), SemanticError> {
        let _ = id;
        Ok(())
    }

    /// Perform opportunistic maintenance — e.g., `REINDEX INDEX CONCURRENTLY`
    /// for pgvector backends. Callers are expected to invoke this on a cron schedule
    /// (weekly by default).
    ///
    /// Default impl is a no-op so in-memory / stub backends don't have to override.
    ///
    /// **Contract:**
    /// - Best effort: maintenance is NOT transactional. It may run concurrently with queries.
    /// - Non-blocking: avoid long-running operations that lock the table.
    /// - Idempotent: calling `maintenance()` multiple times is safe.
    async fn maintenance(&self) -> Result<(), SemanticError> {
        Ok(())
    }
}
```

---

## 3. Isolation & Safety

### Agent ID Isolation (Multi-Tenant)

**MANDATORY:** The store MUST scope all queries by `agent_id`. No cross-agent leakage.

```rust
// In your query() implementation:
// 1. Filter rows by agent_id FIRST
// 2. Then apply vector similarity or text search
// 3. Return results for this agent_id only

// WRONG (returns rows from all agents):
let results = self.search_by_vector(&query_embedding)?;

// RIGHT (filters by agent first):
let results = self.search_by_agent_and_vector(query.agent_id, &query_embedding)?;
```

### MemoryId Deduplication

`MemoryId` is the stable identifier. Backends SHOULD treat it as a unique key:

- If `entry.id` is pre-populated, use that value (supports idempotent replays).
- If `entry.id` is empty, generate a fresh id (UUIDv4, content hash, or backend-specific).
- Calling `put()` with the same id twice returns the same id (idempotency).

### Error Policy — Fail Loudly

Never silently degrade. Examples:

| Scenario | Right | Wrong |
|---|---|---|
| Embedding service is down | Return `SemanticError::Backend("embedding service unreachable")` | Return `vec![0.0; dims]` |
| Query embedding has wrong dimensions | Return `SemanticError::DimensionMismatch { expected: 1536, got: 768 }` | Pad with zeros |
| Database connection lost | Return `SemanticError::Backend("postgres connection lost")` | Return empty result set |

### Concurrency

The trait is `Send + Sync + 'static`. Backends are wrapped in `Arc<dyn SemanticMemoryStore>` and may be called concurrently from multiple threads:

- Use `tokio::sync::RwLock` for shared state (prefer readers over writers)
- Use `Arc<Mutex<>>` for exclusive access to external resources (e.g., HTTP clients)
- Do NOT use `std::sync::Mutex` in async code — use `tokio::sync::Mutex` instead

---

## 4. Example Skeleton

Create a new Rust crate `sera-memory-{name}`:

```
sera-memory-mem0/
  Cargo.toml
  src/
    lib.rs
    mem0_store.rs
```

**Cargo.toml:**

```toml
[package]
name = "sera-memory-mem0"
version = "0.1.0"
edition = "2021"

[dependencies]
async-trait = "0.1"
tokio = { version = "1.35", features = ["full"] }
sera-types = { path = "../../rust/crates/sera-types" }
reqwest = { version = "0.11", features = ["json"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tracing = "0.1"

[features]
default = []
```

**src/lib.rs:**

```rust
mod mem0_store;

pub use mem0_store::Mem0MemoryStore;
```

**src/mem0_store.rs:**

```rust
use async_trait::async_trait;
use sera_types::semantic_memory::{
    MemoryId, SemanticEntry, SemanticQuery, ScoredEntry, EvictionPolicy, 
    SemanticStats, SemanticError,
};
use std::sync::Arc;

/// Memory backend powered by mem0.com HTTP API.
pub struct Mem0MemoryStore {
    api_key: String,
    api_url: String,
    client: reqwest::Client,
}

impl Mem0MemoryStore {
    pub fn new(api_key: String, api_url: String) -> Self {
        Self {
            api_key,
            api_url,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl SemanticMemoryStore for Mem0MemoryStore {
    async fn put(&self, entry: SemanticEntry) -> Result<MemoryId, SemanticError> {
        todo!("Call mem0 /add endpoint and return the assigned MemoryId")
    }

    async fn query(&self, query: SemanticQuery) -> Result<Vec<ScoredEntry>, SemanticError> {
        todo!("Call mem0 /search endpoint; filter by agent_id; return scored results")
    }

    async fn delete(&self, id: &MemoryId) -> Result<(), SemanticError> {
        todo!("Call mem0 /delete endpoint")
    }

    async fn evict(&self, policy: &EvictionPolicy) -> Result<usize, SemanticError> {
        todo!("Implement TTL and row-cap eviction per policy")
    }

    async fn stats(&self) -> Result<SemanticStats, SemanticError> {
        todo!("Return aggregate stats from mem0")
    }

    // promote(), touch(), maintenance() use defaults or override
}
```

---

## 5. Gateway Wiring

### Compile-Time Selection (Current)

Currently, plugin loading is **compile-time feature selection**. You enable your backend via a Cargo feature:

**Cargo.toml** (sera-gateway):

```toml
[features]
default = ["pgvector-memory"]
pgvector-memory = ["sera-db/pgvector-memory"]
sqlite-fts-memory = ["sera-db/sqlite-fts-memory"]
mem0-memory = ["sera-memory-mem0"]
```

**src/bin/sera.rs** (gateway entrypoint):

```rust
// At startup, read sera.yaml and instantiate the active backend
let store: Arc<dyn SemanticMemoryStore> = match config.memory.backend {
    MemoryBackend::Pgvector => Arc::new(PgVectorStore::new(db_url, config).await?),
    MemoryBackend::SqliteFts => Arc::new(SqliteFtsMemoryStore::new(db_path).await?),
    MemoryBackend::Mem0 => Arc::new(Mem0MemoryStore::new(api_key, api_url)),
};

app_state.semantic_memory = store;
```

### Dynamic Plugin Loading (Future)

Planned improvements (sera-czpa, sera-dmpl phases):

- **Dylib loading** — `plugin: {crate: "sera-memory-mem0", path: "/opt/sera/plugins/libsera_memory_mem0.so"}`
- **gRPC plugin API** — out-of-process backends with service discovery

For now, recompile with your feature enabled:

```bash
cargo build --release --features mem0-memory
```

**sera.yaml:**

```yaml
memory:
  backend: mem0
  mem0:
    api_key: "${MEM0_API_KEY}"
    api_url: "https://api.mem0.com"
```

---

## 6. Reference Adapters (Planned)

| Adapter | Status | Landing Bead | Purpose |
|---|---|---|---|
| **mem0 HTTP** | Planned | TBD | Cloud-based memory service |
| **Hindsight local** | Planned | TBD | Lightweight local vector DB |
| **HTTP RAG** | Planned | TBD | External retrieval endpoint |
| **Qdrant** | Planned | TBD | Vector database alternative to pgvector |
| **Milvus** | Future | — | Distributed vector DB |

---

## 7. Testing Your Implementation

### Test Scaffold

Use `sera-testing::MockSemanticMemoryStore` as the test harness:

```rust
#[tokio::test]
async fn test_put_and_query_round_trip() {
    let store = Arc::new(YourCustomMemoryStore::new(/* config */));
    
    // Put 100 entries
    let mut ids = Vec::new();
    for i in 0..100 {
        let entry = SemanticEntry {
            id: MemoryId::new(format!("entry-{}", i)),
            agent_id: "test-agent".into(),
            content: format!("This is entry number {}", i),
            embedding: vec![0.1; 1536],  // mock embedding
            tier: SegmentKind::MemoryRecall("test".into()),
            tags: vec![],
            created_at: Utc::now(),
            last_accessed_at: None,
            promoted: false,
        };
        ids.push(store.put(entry).await.unwrap());
    }
    
    // Query: top-5 similar to entry-50
    let query = SemanticQuery {
        agent_id: "test-agent".into(),
        text: Some("entry number 50".into()),
        query_embedding: None,
        top_k: 5,
        similarity_threshold: Some(0.5),
        tier_filter: None,
    };
    
    let results = store.query(&query).await.unwrap();
    assert_eq!(results.len(), 5);
    assert!(results.iter().all(|r| r.entry.agent_id == "test-agent"));
    
    // Verify multi-tenant isolation
    let other_query = SemanticQuery {
        agent_id: "other-agent".into(),
        text: Some("entry number 50".into()),
        query_embedding: None,
        top_k: 5,
        similarity_threshold: None,
        tier_filter: None,
    };
    
    let other_results = store.query(&other_query).await.unwrap();
    assert_eq!(other_results.len(), 0);  // No cross-agent leakage
}
```

### Smoke Test Bar

Minimum acceptance criteria:

1. **Round-trip:** Put 100 entries, query top-5, all results belong to requesting agent
2. **Multi-tenant isolation:** Two agents writing entries; queries return only their own
3. **Scoring:** Each `ScoredEntry` has at least one of `index_score` or `vector_score` populated
4. **Eviction:** Apply `EvictionPolicy` with `max_per_agent: 50` and `promoted_exempt: true`; verify promoted rows survive
5. **Error handling:** Attempt to delete a non-existent row; verify `SemanticError::NotFound` is returned

---

## 8. Cross-References

- **Architecture overview:** [`docs/plan/ARCHITECTURE-2.0.md §5`](../plan/ARCHITECTURE-2.0.md)
- **Full memory spec:** [`docs/plan/specs/SPEC-memory.md`](../plan/specs/SPEC-memory.md) (four-tier working memory, two-tier injection)
- **Built-in implementations:** `sera-db/src/pgvector_store.rs` (PostgreSQL + pgvector), `sera-db/src/sqlite_fts_store.rs` (landing in bead sera-vzce)
- **Integration point:** `sera-gateway/src/services/memory.rs` (wiring, context injection)
- **Tools using the store:** `sera-runtime` memory_search, memory_write tools

# SPEC-memory-pluggability — Semantic memory contract for SERA plugins

**Status:** accepted (bead sera-50y1 / sera-lg2i)
**Scope:** `sera-memory::SemanticMemoryStore` trait and the pluggability rules it enforces.

## 1. Why this spec

Memory is SERA's biggest pluggability test. Two real-world backends put the
trait under stress in opposite ways:

- **hindsight** is a local Docker service that owns its own embeddings end to
  end (content in, score out). It has no per-memory delete; it retains,
  recalls, and bulk-deletes.
- **LCM** (OpenClaw context engine) is a turn-log + DAG over discrete
  segments with typed links, depth queries, and FTS search. It is not a
  vector store and cannot be forced through a "put/query/delete" shape.

If the SERA trait can host hindsight (a real, external backend) and its
**shape is provably compatible** with LCM (a context engine, not a store),
the seam is durable enough to expose as a plugin contract. This document is
the signed-off design for that seam.

## 2. Trait surface

The canonical definitions live in `rust/crates/sera-memory/src/semantic_memory.rs`.
The essentials are reproduced here so external reviewers have a single stop.

```rust
/// Write input. Caller provides content + optional pre-computed vector.
/// Backends that own embeddings ignore `supplied_embedding` and embed
/// server-side.
pub struct PutRequest {
    pub agent_id: String,
    pub content: String,
    pub scope: Option<Scope>,
    pub tier: SegmentKind,
    pub tags: Vec<String>,
    pub promoted: bool,
    pub supplied_embedding: Option<Vec<f32>>,
}

pub struct SemanticEntry {
    pub id: MemoryId,
    pub agent_id: String,
    pub content: String,
    pub embedding: Option<Vec<f32>>, // previously Vec<f32>
    pub tier: SegmentKind,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub last_accessed_at: Option<DateTime<Utc>>,
    pub promoted: bool,
    pub scope: Option<Scope>,
}

#[async_trait]
pub trait SemanticMemoryStore: Send + Sync + 'static {
    async fn put(&self, req: PutRequest) -> Result<MemoryId, SemanticError>;
    async fn query(&self, query: SemanticQuery) -> Result<Vec<ScoredEntry>, SemanticError>;
    async fn query_hierarchical(
        &self,
        hierarchy: &ScopeHierarchy,
        query_embedding: Vec<f32>,
        k: usize,
    ) -> Result<Vec<MemoryHit>, SemanticError> { /* default walks levels() */ }
    async fn delete(&self, id: &MemoryId) -> Result<(), SemanticError>;
    async fn evict(&self, policy: &EvictionPolicy) -> Result<usize, SemanticError>;
    async fn stats(&self) -> Result<SemanticStats, SemanticError>;
    async fn promote(&self, id: &MemoryId) -> Result<(), SemanticError> { /* default errors */ }
    async fn touch(&self, id: &MemoryId) -> Result<(), SemanticError> { Ok(()) }
    async fn maintenance(&self) -> Result<(), SemanticError> { Ok(()) }
}
```

The trait bound `Send + Sync + 'static` is load-bearing — every production
call site stores it as `Arc<dyn SemanticMemoryStore>`.

## 3. Backend freedoms

A conforming backend **MAY**:

1. **Own embeddings.** Callers pass content, not a vector. A backend that
   prefers server-side embedding ignores `PutRequest::supplied_embedding`
   entirely.
2. **Choose its own async latency.** There is no synchronous fast-path
   assumption; backends may serialise through `spawn_blocking`, a gRPC
   channel, or a remote plugin RPC — the trait is `async fn`.
3. **Reject in-place update.** `put` is logically insert-only; backends that
   do not support upsert surface repeat writes as append-only rows.
4. **Reject per-id delete.** `delete` is trait-level but backends that only
   support bulk delete (hindsight) return
   `SemanticError::Backend("<name> supports bulk delete only")`.

A conforming backend **MUST**:

5. **Filter by `agent_id` and optional `Scope`.** Multi-tenant isolation is
   not optional. Queries that forget this leak across agents.
6. **Return `SemanticError` on network or storage faults.** No silent
   fallbacks, no zero-vector placeholders. If the backend cannot produce a
   real answer, it fails loudly. (This is the same policy already enforced
   on the embedding provider side — see `sera-px3w`.)

Returned `SemanticEntry::embedding` reflects what the backend actually has.
`None` is honest: hindsight, for example, never returns vectors to callers —
it returns scored content. Backends that can return vectors (pgvector,
in-memory) return `Some(_)`.

## 4. Hindsight worked example

Hindsight is a local Docker service that exposes:

- `retain(content)` — returns `operation_id`, embeds server-side
- `recall(content, top_k)` — returns `[{memory_id, score}]`
- `list()` — lists all memories
- `bulk_delete(agent_id)` — wipes an agent

A `HindsightStore` mapping:

| Trait method | Hindsight call |
| --- | --- |
| `put(req)` | `retain(req.content)` with `supplied_embedding` ignored. Hindsight's async `operation_id` polling is hidden behind the `async fn` seam; the trait caller only sees the final `MemoryId`. |
| `query(q)` | `recall(q.text, q.top_k)`. `SemanticQuery::query_embedding` is ignored — hindsight owns embeddings. `q.agent_id` gates the tenant scope. |
| `delete(id)` | `SemanticError::Backend("hindsight supports bulk delete only")`. |
| `evict(policy)` | `bulk_delete(agent_id)` when policy matches; else no-op. |
| `stats()` | Derived from `list()` + `len()`. |
| `promote(id)` | `SemanticError::Backend("hindsight does not support promotion")`. |
| `touch(id)` | `Ok(())` — no-op (hindsight tracks recall itself). |
| `maintenance()` | `Ok(())` — hindsight is self-maintaining. |
| `query_hierarchical` | Default impl is sufficient — it fans out to `query` per-scope. |

A `list()`-style enumeration is **not** in the trait. Backends that want to
expose it do so through their concrete type (`HindsightStore::list`), not
through the trait. This is deliberate: LCM has no flat list, so forcing it
into the trait would exclude LCM.

## 5. LCM worked example (shape proof by separation)

LCM is the context engine from `hermes-agent::OpenClaw`: a turn log + typed
DAG (citations, rebuttals, follow-ups) over discrete segments with:

- `append_batch(turn_id, segments)` — ingest one turn's worth of segments
- `get_range(depth_from, depth_to)` — depth-windowed segment retrieval
- `dag.add_node`, `dag.add_edge`, `dag.count_at_depth(label)` — DAG ops
- `fts.search(query, k)` — FTS5 keyword search
- `externalize_refs(threshold)` — move cold segments to blob storage

This is **not** a `SemanticMemoryStore` and must not be forced to pretend it
is. `put(content)` loses the turn-id grouping; `query(embedding)` loses the
depth/label filters; `delete(id)` is nonsensical in an append-only DAG.

Design decision: **LCM is a separate `ContextEngine` trait**, landing in
bead `sera-ze27`. The two traits coexist:

- `SemanticMemoryStore` — semantic recall: embedding-similarity,
  hierarchical scopes, eviction + promotion. hindsight, pgvector, sqlite,
  in-memory all conform.
- `ContextEngine` — structured turn log: append-only, DAG-addressable,
  depth-windowed, FTS-searchable. LCM and future discrete-segment stores
  conform.

Proving LCM fits by building a **different** trait for it is the shape
proof. The claim is not "our one trait is so general it hosts everything" —
the claim is "our trait is honest about what it models, and the things it
doesn't model get their own trait, and neither hides the other." That is
what a durable pluggability seam looks like.

## 6. Migration

The type move landed in a single PR with the trait-revision commit (a
breaking change) and the crate-extract commit kept separate for reviewer
ergonomics. No re-export shims were kept in `sera-types` — that would
have created a circular dependency (`sera-memory` depends on
`sera-types` for `SegmentKind`). Instead:

1. **`sera-types`** — the old `semantic_memory` module is deleted
   outright. `sera-types` now owns only the primitives that all other
   crates consume (`memory::SegmentKind`, `EmbeddingService` in
   `embedding.rs`, etc.).
2. **`sera-memory`** — new crate. Owns `SemanticMemoryStore` + the
   accompanying types (`PutRequest`, `SemanticEntry`, `SemanticQuery`,
   `ScoredEntry`, `EvictionPolicy`, `SemanticStats`, `SemanticError`,
   `Scope`, `Damping`, `ScopeHierarchy`, `MemoryHit`, `MemoryId`) plus
   the three in-process backends (`PgVectorStore`, `SqliteMemoryStore`,
   `InMemorySemanticStore`) behind the `pgvector`, `sqlite`, and
   `testing` features.
3. **`sera-db` and `sera-testing`** — kept thin re-export stubs at the
   original file locations (`sera_db::pgvector_store::PgVectorStore`,
   `sera_db::sqlite_memory_store::SqliteMemoryStore`,
   `sera_testing::semantic_memory::InMemorySemanticStore`). Downstream
   call sites that already imported from these paths keep working
   verbatim. A follow-up bead removes the stubs once downstream has
   migrated to direct `sera_memory::…` imports.
4. **Downstream call sites** — every `use sera_types::{PutRequest, …}`
   was rewritten to `use sera_memory::{PutRequest, …}` in the same PR.
   `SegmentKind` remains at `sera_types::memory::SegmentKind`.

The `embedding: Option<Vec<f32>>` and `put(PutRequest)` changes are
breaking — no shim can smooth over a type-signature break. Every caller
in this workspace adapted at once. The stub re-exports above preserve
**import paths in sera-db and sera-testing**, not type signatures.

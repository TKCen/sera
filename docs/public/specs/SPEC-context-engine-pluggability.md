# SPEC-context-engine-pluggability — Context assembly + drill-tool contract for SERA plugins

**Status:** accepted (bead sera-ze27)
**Scope:** `sera_runtime::context_engine::{ContextEngine, ContextQuery, ContextDiagnostics}` and the pluggability rules they enforce.

## 1. Why this spec

Memory ([SPEC-memory-pluggability](SPEC-memory-pluggability.md)) and context
assembly are separate concerns. A semantic memory store is a
recall-by-similarity surface over discrete facts; a context engine is the
component that decides **what ends up in the prompt this turn**.

Two real-world context engines put a pluggable contract under stress in
opposite ways:

- **The default `ContextPipeline`** (KV cache + summarising condenser) is
  ephemeral: ingest messages, assemble a prompt under a budget, emit
  compaction checkpoints. No drill tools, no durable message store, no
  diagnostics worth exposing to an agent.
- **LCM** (OpenClaw's lossless-context-management engine from
  `hermes-agent/plugins/context_engine/lcm`) is durable: every message is
  persisted to SQLite, summaries form a depth-stratified DAG, and the
  **agent** gets drill tools (`lcm_grep`, `lcm_describe`, `lcm_expand`,
  `lcm_expand_query`, `lcm_status`, `lcm_doctor`) that let the LLM reach
  back into compacted history mid-turn.

If the SERA contract can host the default pipeline *and* drop LCM in as a
plugin without forcing either into the other's shape, the seam is
durable enough to expose as a plugin contract. This document is the
signed-off design for that seam.

This spec is the sibling of SPEC-memory-pluggability. The two shapes
together cover SERA's two biggest pluggability tests.

## 2. Trait surface

Three traits, tiered by opt-in level. Canonical definitions live in
`rust/crates/sera-runtime/src/context_engine/mod.rs`.

### 2.1 Core — `ContextEngine`

Every context engine MUST implement:

```rust
#[async_trait]
pub trait ContextEngine: Send + Sync {
    async fn ingest(&mut self, msg: serde_json::Value) -> Result<(), ContextError>;
    async fn assemble(&self, budget: TokenBudget) -> Result<ContextWindow, ContextError>;
    async fn compact(&mut self, trigger: CheckpointReason) -> Result<CompactionCheckpoint, ContextError>;
    async fn maintain(&mut self) -> Result<(), ContextError>;
    fn describe(&self) -> ContextEngineDescriptor;
}
```

This is the minimum surface needed to replace the default pipeline. It is
the trait that existed before bead sera-ze27 and stays unchanged — the
bead is additive, not a break.

### 2.2 Optional — `ContextQuery`

Engines that expose **agent-facing drill tools** also implement:

```rust
#[async_trait]
pub trait ContextQuery: Send + Sync {
    async fn search(
        &self,
        req: ContextSearchRequest,
    ) -> Result<Vec<ContextSearchHit>, ContextError>;

    async fn describe_node(
        &self,
        node_id: &ContextNodeId,
    ) -> Result<ContextSubtreeDescription, ContextError>;

    async fn expand_node(
        &self,
        node_id: &ContextNodeId,
        max_tokens: u32,
    ) -> Result<ContextExpansion, ContextError>;

    async fn describe_ref(
        &self,
        ref_name: &str,
    ) -> Result<ContextSubtreeDescription, ContextError> { /* default errors */ }

    async fn expand_ref(
        &self,
        ref_name: &str,
        max_tokens: u32,
    ) -> Result<ContextExpansion, ContextError> { /* default errors */ }
}
```

Opaque types by design:

- `ContextNodeId(String)` — backends encode whatever they like. LCM uses
  stringified `INTEGER PRIMARY KEY`. Callers never parse the inner string.
- `depth_label: String` — LCM returns `"D0"`, `"D1"`, `"D2"`. A flat
  engine returns `""`. Consumers render it verbatim — there is no shared
  depth enum, because "depth" is not a universal concept.
- `rank: Option<f64>` — lower is stronger (mirroring SQLite FTS5's rank
  convention). `None` means the backend did not rank the hit.
- `metadata: serde_json::Value` — forward-compatible slot for
  backend-specific fields (LCM's externalize paths, recency buckets, etc.)
  without trait churn.

### 2.3 Optional — `ContextDiagnostics`

Engines that expose health introspection (status / doctor) also
implement:

```rust
#[async_trait]
pub trait ContextDiagnostics: Send + Sync {
    async fn status(&self, session_id: Option<&str>) -> Result<ContextStatus, ContextError>;
    async fn doctor(&self) -> Result<Vec<DoctorCheck>, ContextError>;
}
```

`ContextStatus::fields` is `serde_json::Value` so backends report their
own metrics (compression count, depth distribution, externalization
stats, DB size) without requiring a schema revision for every new
backend.

## 3. Why three traits and not one

The shape-proof from SPEC-memory-pluggability — *"our trait is honest
about what it models, and the things it doesn't model get their own
trait"* — applies here too.

A simple ring-buffer context engine SHOULD NOT be forced to implement
`expand_node` or `doctor`. LCM DOES implement both, natively. Packing all
five methods into one trait would either force stub impls (dishonest,
a `Err("not supported")` per call) or exclude the simple case (wrong
scope).

Splitting the optional methods across two traits — not one "extended"
trait — lets a future engine opt into drill tools **without** also
committing to diagnostics, and vice versa.

The split at `ContextQuery` vs `ContextDiagnostics` is not arbitrary: any engine that implements `ContextQuery` at all has semantic reason to make `search`, `describe_node`, and `expand_node` work (they are the read side of whatever the engine persists), whereas `ContextDiagnostics` methods (`status`, `doctor`) have no semantic home for an engine that persists nothing — defaulting them would be the same dishonest stub problem under a different name.

Capability matrix:

| Engine | `ContextEngine` | `ContextQuery` | `ContextDiagnostics` |
| --- | --- | --- | --- |
| Default `ContextPipeline` (KV cache + condenser) | ✅ | — | — |
| LCM (hermes-agent OpenClaw) | ✅ | ✅ | ✅ |
| Future: plain ring-buffer engine | ✅ | — | — |
| Future: vector-indexed turn log | ✅ | ✅ | optional |

## 4. LCM worked example

LCM has three internal surfaces:

- `MessageStore` — SQLite FTS5-indexed immutable append-only log with
  monotonic integer `store_id`.
- `SummaryDAG` — append-only typed DAG over summaries with depth
  stratification (`D0` = leaf summaries of raw messages, `D1` =
  condensation of D0, etc.) and FTS5-indexed summary text.
- `externalize_refs` — cold-blob overflow for large tool outputs moved
  out of the main context (recovered by filename).

Its agent-facing tools — `lcm_grep / lcm_describe / lcm_expand /
lcm_expand_query / lcm_status / lcm_doctor` — are the public drill
surface exposed to the LLM.

Mapping:

| Trait method | LCM call |
| --- | --- |
| `ContextEngine::ingest(msg)` | `store.append(session_id, msg)` — the monotonic `store_id` is hidden behind the seam. |
| `ContextEngine::assemble(budget)` | `engine.build_prompt(session_id, budget)` — walks the DAG picking summaries / raw messages to fit. |
| `ContextEngine::compact(reason)` | `engine.maybe_compact(session_id, reason)` — runs the condenser and emits a new DAG node at depth N+1. |
| `ContextEngine::maintain` | `engine.housekeep()` — externalize cold blobs, GC tombstones, run pending migrations. |
| `ContextEngine::describe` | `{name: "lcm", version: "0.4.0"}`. |
| `ContextQuery::search(req)` | Fans out to `store.search(query)` (raw messages) and `dag.search(query)` (summaries), merges and ranks. `req.sort` and `req.scope` map 1:1 to LCM's existing `sort` / `session_scope`. |
| `ContextQuery::describe_node(id)` | `dag.describe_subtree(int(id))` — token counts, child manifest, expand hints. |
| `ContextQuery::expand_node(id, max_tokens)` | `engine.expand(int(id), max_tokens)` — walks DAG back to source messages, honouring the token ceiling. |
| `ContextQuery::describe_ref(ref)` | `externalize.describe(ref)` — externalized-payload metadata + preview. |
| `ContextQuery::expand_ref(ref, max_tokens)` | `externalize.load(ref, max_tokens)` — full externalized payload. |
| `ContextDiagnostics::status(session)` | `engine.status(session)` — compression count, DAG depth distribution, store size, context-usage %. |
| `ContextDiagnostics::doctor()` | `engine.doctor()` — DB integrity, orphan DAG nodes, config validation. |

LCM's internal types (`SummaryNode`, integer `store_id`, integer `depth`)
do NOT leak across the seam. They are encoded as `ContextNodeId(String)`
+ `depth_label: String`. A sera tool that presents LCM output
stringifies `SummaryNode.node_id` as `"42"` and renders `depth = 1` as
`"D1"`.

`lcm_expand_query` (LCM's sixth agent-facing tool) has no row in this
table because it is a composition — search + `expand_node` + LLM
synthesis — not a 1:1 mapping to a trait method; it lives at the sera
tool authorship layer described in §7.

## 5. Default pipeline worked example

The existing `ContextPipeline` (`sera-runtime/src/context_engine/pipeline.rs`)
implements only `ContextEngine`. It does NOT implement `ContextQuery` or
`ContextDiagnostics` — there is no DAG to search and no operational state
worth exposing to an agent beyond the compaction-checkpoint stream it
already emits.

Consumers detect the absence by holding the ContextQuery handle
optionally (see §6). A sera tool registry that wants to expose
`context_search` simply declines to register when the optional handle is
`None`.

This is the point: the simple case stays simple. The complex case gets
the surface it needs. Neither forces the other.

## 6. Wiring pattern

Because Rust trait objects do not support safe downcasting, the
constructor for a `ContextEngine`-backed runtime takes the optional
traits as separate arcs:

```rust
pub struct RuntimeContext {
    pub engine: Arc<tokio::sync::Mutex<dyn ContextEngine>>,
    pub query: Option<Arc<dyn ContextQuery>>,
    pub diagnostics: Option<Arc<dyn ContextDiagnostics>>,
}
```

A backend that implements all three (LCM) provides three pointers to the
same underlying struct. A backend that only implements `ContextEngine`
(default pipeline) provides `None` for the other two. Tool plumbing
checks for `Some` and registers the drill tools conditionally.

This is the same pattern `sera-memory` uses for `EmbeddingService`:
optional handles composed in the runtime, never baked into the core
trait bound.

## 7. Agent drill tools as sera tools

Agent-facing drill tools (the sera equivalents of LCM's
`lcm_grep / lcm_describe / lcm_expand / lcm_status / lcm_doctor`) are
regular sera tools, registered separately from the engine. A tool
implementation takes `Arc<dyn ContextQuery>` (or `Arc<dyn ContextDiagnostics>`)
at construction and dispatches to the trait methods.

Consequences:

- A circle operator chooses which drill tools to expose per agent via
  normal tool-registry config — they are not automatic.
- Multiple tools can share one `ContextQuery` handle (`context_search`,
  `context_expand`, `context_describe`) or one tool can multiplex them.
- Tool authorship is engine-independent — the tool talks to the trait,
  not to LCM.

**Out of scope for sera-ze27:** building those tools. Bead sera-ze27
ships the trait. A follow-up bead wires the agent-facing tools once an
LCM-backed implementation lands as a backend.

## 8. Crate placement

The new traits ship in `sera-runtime::context_engine` for now — same
location as the existing `ContextEngine` trait. Extracting to a dedicated
`sera-context-engine` crate (mirroring the sera-memory extraction in
sera-50y1 / PR #982) is deferred until a second `ContextEngine`
implementation justifies the move — specifically, an implementation that
needs to depend on the trait without pulling in sera-runtime's LLM
client, tool registry, and turn loop.

This is the same staging order sera-memory used: land the trait first,
extract to a crate when a second implementation justifies it.

## 9. Migration

Additive. Zero call sites change. The existing `ContextEngine` trait is
unchanged; `ContextPipeline` continues to implement it; `ContextError`
variants are unchanged. New types (`ContextSearchRequest`, etc.) are
siblings of the existing ones.

Nothing downstream breaks. A follow-up bead builds an LCM-backed
implementation of these traits; that bead is where the shape-proof
becomes a working shape.

## 10. Non-goals

- **Not a message-store trait.** LCM's `MessageStore` is an
  implementation detail of the LCM `ContextEngine`. A different engine
  might use Postgres, a ring buffer, or no storage at all. The trait
  seam is at the context-engine level, not the storage level.
- **Not a DAG trait.** Same reason — different engines, different
  internals. Depth semantics are expressed as an opaque `depth_label`
  string, not a typed enum.
- **Not a summariser trait.** The condenser that turns raw messages
  into DAG nodes lives inside each engine's impl. `compact()` is the
  only seam.
- **Not a prompt-renderer trait.** `assemble()` returns a
  `ContextWindow` of JSON messages; prompt template rendering remains
  the caller's concern.

If any of those become plugin seams later, they get their own trait and
their own bead — same pattern as context engine vs memory store.

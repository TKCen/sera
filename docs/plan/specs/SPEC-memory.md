# SPEC: Memory System (`sera-memory`)

> **Status:** DRAFT
> **Source:** PRD §6.1, §6.2, §6.4, §14 (invariants 3, 7), plus deltas from [SPEC-dependencies](SPEC-dependencies.md) §10.4 (beads content-hash IDs solve multi-writer workspace merge conflicts + `Wisp`/ephemeral lifecycle), §10.10 (OpenHands composable `PipelineCondenser` — moved to SPEC-runtime §6a, referenced from here), §10.12 (ChatDev `blackboard` as Circle-level concept — see SPEC-circles §3f, distinct from per-agent memory), §10.14 (CrewAI unified Memory model with `RecallFlow` adaptive-depth recall — alternative to four-tier split), §10.15 (MetaGPT `Memory.index[cause_by]` O(1) filtered retrieval + `RoleZeroLongTermMemory` experience pool), §10.16 (**BeeAI four-tier memory ABC** — directly validated: `Unconstrained / Token / SlidingWindow / Summarize + ReadOnly` wrapper), §10.17 (CAMEL three memory tiers + `WorkflowMemoryManager` coordinator-scoped cross-task summary)
> **Crate:** `sera-memory`
> **Priority:** Phase 1

---

## 1. Overview

The memory system is SERA's **durable knowledge store**. It is a **pluggable system** — the memory backend can be different per agent. The default is file-based (inspired by Karpathy's llm-wiki pattern), but the architecture supports LCM-style DAG compaction, knowledge graphs, and database-backed stores as switchable backends.

Memory is **not a monolith** — it is a workflow. Memory operations (especially write and compact) involve hook chains, tier decisions, and can trigger downstream effects (compaction, indexing, notification).

---

## 2. Memory Trait

```rust
#[async_trait]
pub trait MemoryBackend: Send + Sync {
    /// Store a memory entry
    async fn write(&self, entry: MemoryEntry, ctx: &MemoryContext) -> Result<MemoryId, MemoryError>;

    /// Search memories (may trigger a workflow: embed → search → rank → expand)
    async fn search(&self, query: &MemoryQuery, ctx: &MemoryContext) -> Result<Vec<MemoryResult>, MemoryError>;

    /// Get a specific memory by ID
    async fn get(&self, id: &MemoryId) -> Result<MemoryEntry, MemoryError>;

    /// Compact/summarize older memories (implementation-specific)
    async fn compact(&self, scope: &CompactionScope) -> Result<CompactionResult, MemoryError>;

    /// Health and stats
    async fn stats(&self) -> MemoryStats;
}

/// Content-hash ID for memory entries (beads pattern, SPEC-dependencies §10.4).
/// SHA-256 of canonical serialization. Two agents writing the same entry on separate branches
/// produce identical IDs — merge-safe across multi-writer workspaces. This closes the
/// open question about git conflict resolution in multi-agent workspaces (§5.3).
pub struct MemoryId(pub [u8; 32]);

impl MemoryId {
    pub fn from_entry(entry: &MemoryEntry) -> Self {
        Self(sha256_canonical(entry))
    }
}
```

## 2.0 Four-Tier Memory ABC

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.16 BeeAI — the four tiers below are directly validated by BeeAI's production implementation. SERA ships these exact four variants plus the read-only wrapper.

Beyond the pluggable `MemoryBackend` trait, SERA provides four standard **working-memory tier** implementations that wrap any backend:

```rust
#[async_trait]
pub trait MemoryTier: Send + Sync {
    async fn messages(&self) -> Vec<Message>;
    async fn add(&self, message: Message) -> Result<(), MemoryError>;
    async fn delete(&self, id: MessageId) -> Result<(), MemoryError>;
    async fn reset(&self) -> Result<(), MemoryError>;
}

/// Tier 1: No limit — keeps full history. Use for short interactive sessions.
pub struct UnconstrainedMemory { storage: Vec<Message> }

/// Tier 2: Evicts oldest when token budget exceeded.
pub struct TokenMemory { storage: VecDeque<Message>, budget: usize, counter: TokenCounter }

/// Tier 3: Fixed message-count sliding window.
pub struct SlidingWindowMemory { storage: VecDeque<Message>, max_messages: usize }

/// Tier 4: LLM-driven compaction when the budget is hit. The most sophisticated variant.
/// Uses sera-runtime §6a PipelineCondenser internally.
pub struct SummarizeMemory { storage: Vec<Message>, condenser: Arc<dyn Condenser>, budget: usize }

/// Wrapper: prevents mutation. Use to pass memory views into sub-agents without risking writes.
pub struct ReadOnlyMemory<T: MemoryTier> { inner: T }
```

These are orthogonal to `MemoryBackend` — the backend is about durable long-term storage, the tier is about the ephemeral working-memory window fed into each LLM turn. A single agent uses **both**: one backend (file / LCM / database) and one tier (typically `SlidingWindowMemory` or `SummarizeMemory` for long-running sessions).

## 2.0a Ephemeral / Wisp Lifecycle

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.4 beads.

Some memory entries are **ephemeral** — transient scratch state that must not pollute the durable log or be synced across multi-agent workspaces. SERA mirrors beads' `Wisp` pattern:

```rust
pub struct MemoryEntry {
    // ... existing fields ...
    pub ephemeral: bool,              // If true, not synced via git, TTL-compacted
    pub wisp_type: Option<WispType>,  // TTL-based compaction class
}

pub enum WispType {
    ScratchPad,        // Inline agent notes during turn execution
    TempCalc,          // Intermediate calculation state
    DebugTrace,        // Diagnostic output that should not persist beyond the session
    SessionNote,       // Session-scoped context that survives within a session but not across restarts
}
```

Ephemeral entries are stored in a separate `.sera-wisp/` directory, excluded from git (if git management is enabled), and TTL-compacted by the dreaming workflow.

## 2.0b Coordinator-Scoped `WorkflowMemoryManager`

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.17 CAMEL.

In a multi-agent Circle run, a coordinator agent often needs a summary of completed sub-task results to inject into subsequent task assignments. This is distinct from per-agent memory — it belongs to the Circle coordinator's session, not to any individual member.

```rust
pub struct WorkflowMemoryManager {
    circle_id: CircleId,
    completed_tasks: HashMap<TaskId, TaskSummary>,
    active_context: RollingSummary,
}

pub struct TaskSummary {
    pub task_id: TaskId,
    pub assignee: PrincipalRef,
    pub result_digest: String,        // Structured summary, not full transcript
    pub cause_by: Option<ActionId>,   // MetaGPT routing key (SPEC-dependencies §10.15)
    pub completed_at: DateTime<Utc>,
}
```

The Circle coordinator reads `completed_tasks` when assigning new sub-tasks, giving it cross-task context without requiring the full per-member transcripts. Distinct from the `CircleBlackboard` (SPEC-circles §3f) which is a write-many/read-all artifact bus — `WorkflowMemoryManager` is coordinator-scoped and read-only to workers.

---

## 2a. Embedding-Based Memory Search

> **Enhancement: OpenSwarm v3 §3 (Smart Search / Wiki TOC Router)**

To prevent context window bloat, the memory injection step does **not** load the entire wiki. Instead, the search system uses a local embedding model to retrieve only the top-K most relevant memory files/sections for the current task.

### Search Strategy

```rust
pub struct MemoryQuery {
    pub text: String,                          // Natural language query
    pub strategy: SearchStrategy,
    pub top_k: u32,                            // Maximum results to return
    pub similarity_threshold: Option<f64>,     // Minimum similarity score (0.0–1.0)
    pub tier_filter: Option<MemoryTier>,        // Search within specific tier
}

pub enum SearchStrategy {
    /// Embedding-based semantic search (default when embedding model is configured)
    Semantic,
    /// Keyword/heading-based search (fallback when no embedding model)
    Keyword,
    /// Hybrid: embedding search + keyword boost
    Hybrid,
    /// Exact file/path lookup
    Exact(String),
}

pub struct MemoryResult {
    pub id: MemoryId,
    pub content: String,
    pub score: f64,                            // Relevance score (0.0–1.0)
    pub source: MemorySource,                  // File path, section heading, etc.
    pub tier: MemoryTier,
}
```

### Embedding Model

SERA uses a **local embedding model** (e.g., `text-embedding-gemma-300m-qat`) for semantic search. The embedding model is lightweight enough to run alongside the primary model without VRAM contention.

```yaml
sera:
  memory:
    embedding:
      model: "text-embedding-gemma-300m-qat"    # Local embedding model
      provider: "local"                          # local | api
      dimensions: 256                            # Embedding dimensions
      batch_size: 32                             # Batch size for indexing
```

### Indexing

The file-based backend maintains a **pre-computed embedding index** of all memory files:
- Index is rebuilt on write/compact operations (incremental update, not full rebuild)
- Index is stored alongside memory files (e.g., `.sera-index/embeddings.bin`)
- On search, the query is embedded and compared against the index using cosine similarity
- Top-K results are returned, filtered by similarity threshold

### Fallback

When no embedding model is configured (e.g., minimal Tier 1 deployments), the search falls back to:
1. **Heading-based matching** — searches file headings and `index.md` structure
2. **Keyword search** — simple text search across memory files

### Context Injection Integration

The memory injection step (SPEC-runtime §4.2, step 4) uses the search system to load only the most relevant memory:

```
Memory Injection Step:
  → Extract key terms from current turn + session context
  → MemoryBackend.search(query, top_k=3, strategy=Semantic)
  → Inject top results into context window
  → Total injected tokens bounded by configurable budget
```

This ensures the context window receives precisely the knowledge needed for the current task rather than a dump of the entire wiki.

### Hybrid Search

The `Hybrid` strategy merges results from both embedding-based and keyword-based search:

1. Pull candidate pools from both semantic and keyword search
2. Convert keyword rank into a normalized score
3. Compute a weighted final score: `final = (vector_weight × semantic_score) + (text_weight × keyword_score)`
4. Apply optional post-processing: temporal decay (boost recency) and MMR re-ranking (increase diversity)

```yaml
sera:
  memory:
    search:
      hybrid:
        vector_weight: 0.7           # Semantic similarity weight
        text_weight: 0.3             # Keyword relevance weight
        candidate_multiplier: 4      # Fetch 4× top_k candidates from each source
        temporal_decay:
          enabled: true
          half_life_days: 14         # Recency boost half-life
        mmr_diversity:
          enabled: false             # MMR re-ranking for result diversity
          lambda: 0.7                # Diversity vs relevance tradeoff
```

Hybrid matters because real agent workspaces contain both fuzzy intent ("speed up the pipeline") and exact strings (`ERR_CONNECTION_RESET`, `ticket-1427`).

---

## 2b. Recall Signal Tracking (Dreaming Support)

> **Enhancement: OpenClaw Dreaming Guide**

The dreaming workflow (SPEC-workflow-engine §5) requires **tracking search recall signals during normal agent operation.** Every time the agent searches memory and a candidate is returned, its recall count and query diversity are incremented in a short-term recall store.

### Signal Collection

When `search()` returns results, the memory backend emits side-channel signals:

```rust
pub struct RecallSignal {
    pub memory_id: MemoryId,
    pub query_text: String,                // The query that triggered this recall
    pub query_hash: u64,                   // Hash for unique-query counting
    pub score: f64,                        // Relevance score at recall time
    pub timestamp: DateTime<Utc>,
}

pub struct ShortTermRecallEntry {
    pub memory_id: MemoryId,
    pub recall_count: u32,                 // How many times recalled
    pub unique_queries: HashSet<u64>,      // Diverse query hashes
    pub total_relevance: f64,              // Accumulated relevance scores
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub light_phase_hits: u32,             // Boosted during dreaming light phase
    pub rem_phase_hits: u32,               // Boosted during dreaming REM phase
}
```

### Short-Term Recall Store

The recall store is an ephemeral signal accumulation layer:
- Stored alongside memory files (e.g., `.sera-index/recall-signals.json`)
- Entries accumulate signals over time as the agent naturally uses memory search
- Consumed by the dreaming workflow during its scoring phase
- Entries older than `max_age_days` (default: 30) are expired

### Integration with Dreaming

The dreaming workflow's deep phase scores candidates using signals from this store:
- **Frequency** (weight 0.24) — `recall_count`
- **Query diversity** (weight 0.15) — `unique_queries.len()`
- **Recency** (weight 0.15) — decay from `last_seen`
- **Relevance** (weight 0.30) — `total_relevance / recall_count`

Without this tracking, the dreaming workflow has no evidence-based scoring and degrades to heuristic promotion.
```

### Memory Context

```rust
pub struct MemoryContext {
    pub agent: AgentRef,
    pub session: Option<SessionRef>,
    pub principal: PrincipalRef,
    pub tier: MemoryTier,
}

pub enum MemoryTier {
    ShortTerm,    // Session-scoped, volatile
    LongTerm,     // Agent workspace, durable
    Shared,       // Cross-agent, durable
}
```

---

## 3. Built-in Backends

### 3.1 File-Based (Default)

Markdown files in the agent's workspace directory, organized by a heading hierarchy. The LLM-maintained wiki pattern uses:
- **`index.md`** — structured knowledge index, maintained by the agent
- **`log.md`** — chronological log of interactions and decisions
- Additional markdown files organized by topic

The file-based backend supports **optional automatic git management** (see §5).

> [!NOTE]  
> **Beads task graph:** Originally considered for the memory backend, Beads has been reclassified as a **workflow/tool integration** (see [SPEC-workflow-engine](SPEC-workflow-engine.md) §6.1). Beads provides deterministic task DAGs for structured multi-step work tracking, which is a task decomposition concern rather than a memory storage concern.

### 3.2 LCM / DAG Backend

DAG-based lossless context management (inspired by [lossless-claw](https://github.com/martian-engineering/lossless-claw)). Persists every message, builds hierarchical summaries, provides tools for drill-down (`search`, `describe`, `expand`).

- **Tier:** 2, 3 (not default)
- **Priority:** Phase 4

### 3.3 Database Backend

PostgreSQL-backed structured store for enterprise deployments requiring SQL queries, audit trails, and relational integrity.

- **Tier:** 3 (not default)
- **Priority:** Phase 4

### 3.4 Custom Backend

Implement the `MemoryBackend` trait for domain-specific storage.

---

## 4. Write Workflow

Memory writes are **not simple CRUD** — they are workflows with hook chains:

```
write_request
  → pre_memory_write hook chain (PII filter, classification, dedup)
  → tier decision (short-term session vs. long-term workspace)
  → backend.write()
  → post_memory_write hook chain (index update, cross-reference, notification)
  → optional: trigger compaction if threshold exceeded
```

### Tier Decision

The write workflow decides whether a memory entry goes to short-term (session-scoped) or long-term (workspace-scoped) storage based on:
- The entry's content type and importance signal
- The agent's memory configuration
- Hook chain output (a hook can reclassify the tier)

---

## 5. File-Based Memory with Optional Git Management

The file-based backend supports **automatic git management** — every write, compaction, and dreaming promotion can be auto-committed with structured commit messages.

### 5.1 Configuration

```yaml
agents:
  - name: "sera"
    memory:
      backend: "file"
      workspace: "./agents/sera/memory"    # Agent's durable home directory
      git:
        enabled: true                       # Auto-commit memory changes
        auto_commit: true
        commit_template: "[sera-memory] {operation}: {description}"
        branch: "memory/sera"               # Dedicated branch for memory changes
        push_remote: null                    # Optional: push to remote for backup
```

### 5.2 Implementation

Git management is implemented **at the memory backend level** — no hooks required. The file-based backend wraps git operations around write and compact calls. This is transparent to the rest of the system.

```
write() → write file → git add → git commit (template)
compact() → modify files → git add → git commit (template)
```

### 5.3 Conflict Resolution — RESOLVED via content-hash IDs + Dolt

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.4 beads + Dolt (git-for-SQL).

Content-hash `MemoryId`s (§2) plus Dolt storage solve the multi-writer conflict problem cleanly:

1. **Content-addressed IDs** — two agents creating an entry with identical content on separate branches produce the same `MemoryId`. No collision, no reconciliation needed.
2. **Dolt cell-level merge** — for structured memory (database backend in Tier-3), Dolt provides cell-level 3-way merge. Conflicts are resolved per-field, not per-row.
3. **Git for file-based backend** — file-based memory uses content-hash filenames, so independent writes of the same content produce the same file. Merge conflicts only arise when two agents modify the **same key with different content** — a genuine semantic conflict that requires human review.

**Strategy by tier:**

| Tier | Multi-writer model |
|---|---|
| Tier 1 (file-based, local) | Single-writer enforced by workspace ownership — one agent per workspace |
| Tier 2 (file-based, multi-agent) | Content-hash filenames + git, semantic conflicts surface as merge conflicts for human review |
| Tier 3 (database, multi-agent) | Dolt SQL server mode with cell-level 3-way merge, content-hash IDs prevent accidental collisions |
| Tier 3+ (federated) | Wasteland DoltHub fork/sync pattern (SPEC-dependencies §10.4) — out of scope for SERA 1.0 |

---

## 6. Compaction

The `compact()` operation summarizes older memories to manage storage growth and context window pressure. Implementation is backend-specific:

- **File-based:** Consolidate log entries into summary sections, archive old logs
- **LCM/DAG:** Build hierarchical summaries from leaf nodes
- **Database:** Aggregate and summarize, archive raw entries

### Flush-Before-Discard Invariant

> [!IMPORTANT]  
> Before any memory is discarded or compacted, it **must be flushed** to durable storage first. No memory is lost without being persisted. This is enforced at the backend level.

---

## 7. Memory as Tool

Memory is accessed by agents via **tools** (memory_read, memory_write, memory_search). These tools are registered in the tool registry and subject to all standard tool policies, hook chains, and authorization checks.

```rust
// Conceptual — actual implementation in sera-tools
pub struct MemoryReadTool;
pub struct MemoryWriteTool;
pub struct MemorySearchTool;
```

---

## 8. Invariants

| # | Invariant | Enforcement |
|---|---|---|
| 3 | Flush before discard | `sera-memory` — pre-compaction checkpoint |
| 7 | Memory writes are privileged | `sera-memory` + hooks — pre_memory_write chain |

---

## 9. Hook Points

| Hook Point | Fires When |
|---|---|
| `context_memory` | During memory injection step in context assembly |
| `pre_memory_write` | Before durable memory write |
| `post_memory_search` | After memory search returns results — enables recall signal collection and result filtering |

---

## 10. Configuration

```yaml
sera:
  memory:
    embedding:
      model: "text-embedding-gemma-300m-qat"
      provider: "local"                # local | api
      dimensions: 256

agents:
  - name: "sera"
    memory:
      backend: "file"                  # file | lcm | postgres | custom
      workspace: "./agents/sera/memory"
      search:
        strategy: "semantic"           # semantic | keyword | hybrid
        top_k: 3                       # Default results per search
        similarity_threshold: 0.4      # Minimum relevance score
        context_token_budget: 2000     # Max tokens injected from memory per turn
      compaction:
        threshold_entries: 1000        # Trigger compaction after N entries
        auto_compact: true
      git:
        enabled: true
        auto_commit: true
        commit_template: "[sera-memory] {operation}: {description}"
        branch: "memory/sera"
```

---

## 11. Cross-References

| Dependency | Spec | Relationship |
|---|---|---|
| `sera-hooks` | [SPEC-hooks](SPEC-hooks.md) | Pre/post memory write hook chains |
| `sera-tools` | [SPEC-tools](SPEC-tools.md) | Memory access via tool interface |
| `sera-runtime` | [SPEC-runtime](SPEC-runtime.md) | Memory injection in context assembly; **`PipelineCondenser` lives in SPEC-runtime §6a** — this spec references it for `SummarizeMemory` implementation |
| `sera-workflow` | [SPEC-workflow-engine](SPEC-workflow-engine.md) | Dreaming workflow reads/writes memory; `WorkflowMemoryManager` (§2.0b) backs coordinator-scoped summaries |
| `sera-auth` | [SPEC-identity-authz](SPEC-identity-authz.md) | AuthZ for memory access |
| `sera-circles` | [SPEC-circles](SPEC-circles.md) | `CircleBlackboard` is distinct from per-agent memory; `WorkflowMemoryManager` is coordinator-scoped |
| `sera-db` | [SPEC-crate-decomposition](SPEC-crate-decomposition.md) | Database backends use sera-db; Dolt server mode for Tier-3 multi-writer |
| Dependencies | [SPEC-dependencies](SPEC-dependencies.md) | §10.4 beads content-hash IDs + `Wisp` ephemeral lifecycle + Dolt conflict resolution; §10.10 OpenHands `PipelineCondenser` (the most complete compaction architecture); §10.14 CrewAI unified Memory with `RecallFlow`; §10.15 MetaGPT `Memory.index[cause_by]` + `RoleZeroLongTermMemory` experience pool; **§10.16 BeeAI four-tier memory ABC (directly validated)**; §10.17 CAMEL three memory tiers + `WorkflowMemoryManager` |

---

## 12. Open Questions

1. ~~**Beads task graph**~~ — Resolved: Reclassified as workflow/tool integration. See [SPEC-workflow-engine](SPEC-workflow-engine.md) §6.1.
2. **Git conflict resolution** — Strategy for multi-agent workspaces with git (see §5.3)
3. ~~**Memory search implementation**~~ — Resolved: See §2a. Embedding-based semantic search with configurable fallback to keyword search.
4. **Cross-agent memory sharing** — How does the `Shared` memory tier work? Separate workspace? Shared files?
5. **Memory retention policies** — Are there configurable TTLs or retention policies per tier?
6. **Memory export/import** — Can an agent's memory be exported and imported to another instance?
7. **Embedding model deployment** — How is the local embedding model deployed alongside the primary model? Separate process? In-process?

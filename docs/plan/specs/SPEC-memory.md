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

## 1a. Two Backends — File and Gateway

> **Design decision — 2026-04-13.** Memory looks different depending on deployment mode. From the LLM's perspective, both modes deliver the same thing: injected context and callable tools.

### From the LLM's perspective, memory is two things only

1. **Injected context** — files/records that appear in the system prompt before the first turn, automatically, without any LLM action. The LLM doesn't retrieve them; they just arrive.
2. **Tools** — `knowledge_query`, `knowledge_store`, `memory_write`. Explicit LLM calls into the memory system.

Hooks that intercept writes, trigger sync, or augment injections are **transparent middleware** — invisible to the LLM. The LLM never sees the `MemoryBackend` trait or knows which backend is active.

### File backend (pet / standalone mode)

Used when `memory.backend: file` is configured. The runtime (or its `ContextEngine`) reads workspace files directly:

- `soul.md` — immutable anchor / persona definition
- `memory.md` — agent's durable working memory
- `knowledge/*.md` — topic-organized knowledge base

These files are **auto-injected** at session start via the `ContextEngine.assemble()` step. Tools write back to these files. Git management is optional (see §5).

**Context injection responsibility in file mode:** the `ContextEngine` running in the runtime reads and assembles workspace files. The gateway provides the workspace path; the runtime reads it.

### Gateway backend (enterprise / cattle mode)

Used when `memory.backend: postgres` (or equivalent) is configured. The gateway owns all memory:

- **PostgreSQL** for structured, queryable memory entries
- **Qdrant** (or compatible) for semantic embedding index
- Memory is session-scoped, agent-scoped, or circle-scoped
- Semantic retrieval via embeddings — the gateway selects top-K relevant entries

**Context injection responsibility in gateway mode:** the **gateway** assembles and injects memory context. The runtime receives an already-assembled context window. It does NOT read `soul.md` directly — the soul definition is injected BY THE GATEWAY as part of context assembly. The runtime doesn't know the source.

```
Gateway backend flow:
  Session start → Gateway queries PostgreSQL + Qdrant
               → Selects top-K memory entries for current task
               → Assembles system prompt prefix with injected context
               → Sends assembled context window to runtime
               → Runtime receives opaque context — doesn't know it came from Qdrant
```

### Switching backends

Backend selection is a **configuration change, not a recompilation**. Both backends are compiled into the binary. See SPEC-config §1a and SPEC-deployment §1a for the single-binary / feature-activation model.

```yaml
memory:
  backend: postgres          # switch to file: change this line, restart
  postgres:
    url: "${DATABASE_URL}"
  qdrant:
    url: "http://qdrant:6334"
```

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

## 2.1 Two-Tier Injection Model (Hermes-Aligned)

> **Design decision — 2026-04-16.** Hermes uses a proven 2-tier memory model: a compact injected block (always present in context) + semantic search (on-demand retrieval). SERA adopts this as the default before adding complexity. The four-tier working-memory ABC (§2.0) governs *eviction strategy*; this section governs *what the LLM sees each turn*.

### Tier A — Compact Injected Block (`MemoryBlock`)

Every turn, the gateway assembles a single `MemoryBlock` that is unconditionally injected into the context window. This block is budget-constrained and priority-ordered — the most important context always fits.

```rust
/// A prioritized block of memory content injected into every turn's context window.
/// Analogous to Hermes's `prefetch_all` / SERA's `pre_agent_turn` hook output.
///
/// The gateway assembles this block; the runtime receives it as opaque context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryBlock {
    /// Ordered memory segments — highest priority first.
    pub segments: Vec<MemorySegment>,
    /// Total character budget for the injected block.
    /// Segments are included top-down until the budget is exhausted.
    pub char_budget: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySegment {
    /// Source identifier (e.g., "soul", "memory.md", "knowledge/rust-patterns").
    pub source: String,
    /// The content to inject.
    pub content: String,
    /// Priority for inclusion (0 = highest). Soul is always 0.
    pub priority: u32,
    /// Recency boost — added to base priority score for recently-accessed segments.
    /// Decays over turns. Prevents important but old context from being permanently evicted.
    pub recency_boost: f64,
    /// Character count of this segment.
    pub char_count: usize,
}

impl MemoryBlock {
    /// Assemble the final injection string, respecting the character budget.
    /// Segments are sorted by effective priority (priority - recency_boost, lower = better).
    pub fn assemble(&self) -> String {
        let mut sorted = self.segments.clone();
        sorted.sort_by(|a, b| {
            let eff_a = a.priority as f64 - a.recency_boost;
            let eff_b = b.priority as f64 - b.recency_boost;
            eff_a.partial_cmp(&eff_b).unwrap_or(std::cmp::Ordering::Equal)
        });
        
        let mut result = String::new();
        let mut remaining = self.char_budget;
        for seg in &sorted {
            if seg.char_count <= remaining {
                result.push_str(&seg.content);
                result.push('\n');
                remaining -= seg.char_count + 1;
            }
        }
        result
    }
}
```

### Tier B — Semantic Search (On-Demand)

Tier B is the existing embedding-based search system (§2a). It fires when:
- The LLM explicitly calls `knowledge_query` / `memory_search` tools
- The `context_memory` hook triggers retrieval based on turn content
- The dreaming workflow promotes entries that cross the promotion gates

### Design Rationale

> Start with Hermes's 2-tier model and only add tiers when it breaks down.

The four-tier working-memory ABC (§2.0) and the two-tier injection model serve different purposes:
- **Working-memory tiers** = eviction/compaction strategy (how history is managed within a session)
- **Injection tiers** = what context the LLM sees each turn (compact block + on-demand search)

This separation avoids over-architecting. Most agents need only: (1) a soul + key facts always present, and (2) semantic search for everything else.

### `flush_min_turns` — Auto-Prompt Skill Creation

When the `MemoryBlock` consistently exceeds its `char_budget` for `flush_min_turns` consecutive turns (default: 6), the system emits a `memory_pressure` event. This event can trigger:
- A dreaming workflow to compact and consolidate memory
- A skill creation prompt — the agent is asked to extract recurring patterns into a reusable skill (Hermes's `skill_manage patch` pattern)
- A notification to the operator that the agent's memory needs attention

```yaml
agents:
  - name: "sera"
    memory:
      injection:
        char_budget: 4000          # Characters for the compact block
        flush_min_turns: 6         # Turns before memory_pressure fires
        recency_half_life: 10      # Turns until recency_boost decays to 50%
```

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
| `context_memory` (alias: `pre_agent_turn`) | During memory injection step in context assembly — assembles the `MemoryBlock` (§2.1) |
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
7. ~~**Embedding model deployment**~~ — Resolved: See §13.3. Local provider uses `fastembed` (in-process ONNX); remote provider uses `async-openai` or `genai`. No separate sidecar process required for local embeddings.
8. **Skill auto-creation from memory pressure** — When `flush_min_turns` fires, should the system auto-create skill drafts or only notify? Hermes auto-patches via `skill_manage`; SERA should at minimum prompt the agent.
9. **MemoryBlock segment priority tuning** — Should priority values be operator-configurable per agent, or auto-tuned based on recall signals from the dreaming system?

---

## 13. Embedding Service & RAG

> **Status:** DRAFT (fills gap `sera-ifue`)

### 13.1 Motivation

The hybrid scorer in `rust/crates/sera-runtime/src/context_engine/hybrid.rs` already computes a weighted sum of BM25, cosine similarity, and recency decay (see §2a). However, the `ContextPipeline` (`pipeline.rs`, line ~207) currently passes an empty `Vec<f32>` as the query embedding and relies on whatever embedding vectors happen to be serialised onto messages — when none are present the vector component silently collapses to `0.0` for every candidate, making the system purely lexical. This means semantically similar but lexically distant history (e.g., "speed up the ingestion loop" vs. "optimise throughput") is never retrieved, regardless of how relevant it actually is. A real embedding service is needed so that `HybridScorer` can distinguish between genuinely related history and unrelated noise, and so that the Tier B semantic search described in §2.1 can fire with non-trivial results. Without it, the gateway backend's Qdrant index (§1a) is also effectively inert — it would be seeded only with zero vectors and return arbitrary results. This section specifies the `EmbeddingService` trait, concrete provider implementations, the wire-up point in `ContextPipeline`, and the failure/degradation contract that keeps the system usable when the embedding service is unavailable.

### 13.2 `EmbeddingService` Trait

```rust
use async_trait::async_trait;

/// A batch embedding service. Implementations must be `Send + Sync + 'static`
/// so they can be held behind `Arc` and shared across async tasks.
#[async_trait]
pub trait EmbeddingService: Send + Sync + 'static {
    /// Provider-assigned model identifier (e.g. `"text-embedding-3-small"`).
    /// Used as the first component of the embedding cache key.
    fn model_id(&self) -> &str;

    /// Fixed output dimension for this model/provider combination.
    /// All vectors returned by `embed` have exactly this length.
    fn dimensions(&self) -> usize;

    /// Embed a batch of texts. Returns one `Embedding` per input, in the same
    /// order. Empty input returns `Ok(vec![])` without making an API call.
    async fn embed(&self, texts: &[String]) -> Result<Vec<Embedding>, EmbeddingError>;

    /// Liveness check — resolves `Ok(())` if the provider is reachable and
    /// the configured model is available. Called at pipeline startup and by
    /// the SERA health check subsystem.
    async fn health(&self) -> Result<(), EmbeddingError>;
}

/// A single embedding vector. Newtype keeps the inner `Vec<f32>` from being
/// confused with other float vecs in scope.
#[derive(Debug, Clone)]
pub struct Embedding(pub Vec<f32>);

/// Typed errors from an embedding provider.
#[derive(Debug, thiserror::Error)]
pub enum EmbeddingError {
    /// The remote provider returned a non-retryable error.
    #[error("embedding provider error: {0}")]
    Provider(String),

    /// The request timed out before a response was received.
    #[error("embedding request timed out")]
    Timeout,

    /// The provider rate-limited the request.
    /// `retry_after` is the minimum wait in seconds, when the provider
    /// supplies a `Retry-After` header; `None` means unknown.
    #[error("embedding rate limited (retry_after={retry_after:?}s)")]
    RateLimited { retry_after: Option<u64> },

    /// The returned vector length does not match `dimensions()`.
    /// Should never fire against a well-behaved provider but is surfaced
    /// explicitly so callers can distinguish it from `Provider`.
    #[error("embedding dimension mismatch: expected {expected}, got {actual}")]
    DimensionMismatch { expected: usize, actual: usize },

    /// The input text was rejected before it was sent (e.g., empty after
    /// redaction, or exceeded the model's token limit).
    #[error("invalid embedding input: {0}")]
    InvalidInput(String),
}
```

**Trait contracts:**

- `embed` is **batch-first**: the caller passes a `&[String]` slice; the implementation is expected to make a single API call (or one call per provider batch limit) rather than one call per text.
- The returned `Vec<Embedding>` is **1:1 ordered** with the input slice — `result[i]` is always the embedding for `texts[i]`.
- Every returned `Embedding` vector has length exactly equal to `dimensions()`. A provider that violates this must return `EmbeddingError::DimensionMismatch`.
- An empty input slice (`texts.is_empty()`) returns `Ok(vec![])` **without any network call**.

> **Tradeoff:** Requiring a fixed `dimensions()` at construction time (rather than inferring it from the first response) enables dimension-compatibility checks at pipeline startup (§13.5) and avoids lazy errors during scoring.

### 13.3 Provider Implementations

Three concrete providers are required. Each is a struct with an associated configuration struct; full implementation is deferred to the implementor bead.

#### `OpenAIEmbeddingService`

| Field | Value |
|---|---|
| Config struct | `OpenAIEmbeddingConfig` |
| Default `model_id` | `"text-embedding-3-small"` |
| Default dimensions | `1536` (must match the model; configurable for `text-embedding-3-large` at 3072) |
| Auth | API key read from `sera-secrets` via the existing `SecretsProvider` trait (key name: `OPENAI_API_KEY`) |
| HTTP client | `async-openai ^0.28` (already listed in [SPEC-dependencies](SPEC-dependencies.md) §9.1) |
| Rate limiting | Reads `Retry-After` header on HTTP 429; returns `EmbeddingError::RateLimited { retry_after }` rather than blocking the caller — callers decide whether to wait or degrade |
| Batch size | Configurable via `OpenAIEmbeddingConfig::batch_size` (default: `100`); texts exceeding this are split into multiple sequential calls |

```rust
pub struct OpenAIEmbeddingConfig {
    pub model_id: String,          // default: "text-embedding-3-small"
    pub dimensions: usize,         // default: 1536
    pub batch_size: usize,         // default: 100
    pub timeout_secs: u64,         // default: 30
    /// Secret key name resolved via SecretsProvider. Default: "OPENAI_API_KEY".
    pub api_key_secret: String,
}
```

#### `OllamaEmbeddingService`

| Field | Value |
|---|---|
| Config struct | `OllamaEmbeddingConfig` |
| Default `model_id` | `"nomic-embed-text"` |
| Default dimensions | `768` |
| Base URL | `OLLAMA_BASE_URL` env var (default: `http://localhost:11434`) |
| Auth | None — local service, no API key |
| HTTP client | `async-openai` with a custom `base_url` (Ollama exposes an OpenAI-compatible `/api/embeddings` endpoint), or direct `reqwest` call |

```rust
pub struct OllamaEmbeddingConfig {
    pub model_id: String,          // default: "nomic-embed-text"
    pub dimensions: usize,         // default: 768
    pub base_url: String,          // default: "http://localhost:11434"
    pub timeout_secs: u64,         // default: 30
}
```

> **Tradeoff:** Ollama is zero-cost for local development and CI but unavailable in cloud deployments without a sidecar. The tier policy (§13.7) gates OpenAI use to agents where `allow_external_embedding: true`; Ollama is always permitted regardless of tier.

#### `StubEmbeddingService`

The existing zero-vector behavior is extracted into an explicit stub, available **only** behind the `cfg(test)` attribute or the `testing` Cargo feature. It must be removed from the default build path — no production `ContextPipeline` should silently use it.

```rust
#[cfg(any(test, feature = "testing"))]
pub struct StubEmbeddingService {
    pub dimensions: usize,
}

#[cfg(any(test, feature = "testing"))]
#[async_trait]
impl EmbeddingService for StubEmbeddingService {
    fn model_id(&self) -> &str { "stub" }
    fn dimensions(&self) -> usize { self.dimensions }
    async fn embed(&self, texts: &[String]) -> Result<Vec<Embedding>, EmbeddingError> {
        Ok(texts.iter().map(|_| Embedding(vec![0.0; self.dimensions])).collect())
    }
    async fn health(&self) -> Result<(), EmbeddingError> { Ok(()) }
}
```

### 13.4 `ContextPipeline` Integration

**Wire-up point:** `pipeline.rs` around line 207, where `let query_embedding: Vec<f32> = Vec::new()` is the current placeholder (documented in the source as "no embedding service wired in yet").

The fix replaces the hardcoded empty vector with a call to a `Box<dyn EmbeddingService>` held on `ContextPipeline`:

```rust
pub struct ContextPipeline {
    messages: Vec<serde_json::Value>,
    condensers: Vec<Box<dyn Condenser>>,
    session_key: String,
    model_id: String,
    hybrid_config: Option<HybridRetrievalConfig>,
    /// Optional embedding service. When `None`, the pipeline degrades to
    /// lexical + recency scoring (vector component is zero). When `Some`,
    /// the query is embedded and all uncached candidates are embedded in
    /// one batch call before scoring.
    embedding_service: Option<Arc<dyn EmbeddingService>>,
}
```

**Builder method:**

```rust
impl ContextPipeline {
    pub fn with_embedding_service(mut self, svc: Arc<dyn EmbeddingService>) -> Self {
        self.embedding_service = Some(svc);
        self
    }
}
```

**Injection at pipeline construction** is the responsibility of the caller (typically the agent harness or `DefaultRuntime`) — the pipeline does not construct a provider itself.

**Embedding call in `assemble`:** When `embedding_service` is `Some` and hybrid retrieval is enabled, the pipeline:

1. Derives the query text from the last user turn (as `derive_query_tokens` already does, but as raw text, not tokenised).
2. Calls `embed(&[query_text])` to get the query embedding.
3. For each candidate message that lacks a cached embedding, collects its `content` field.
4. Calls `embed(&uncached_texts)` in one batch.
5. Stores results back onto the candidate's `embedding` field (in-memory only, not persisted to the JSON message).

**Embedding cache** — per message, keyed by `(model_id, content_hash)`:

- `content_hash` is the SHA-256 of the message content string (the `MemoryId::from_entry` pattern already established in §2 applies here).
- The cache store reuses whatever in-process store is present on the `ContextPipeline` session — specifically the existing `KvCache` abstraction in `context_engine/kvcache.rs`. The implementor should check that module for the available API before introducing a new cache type.
- Cache entries are session-scoped and dropped when the pipeline is dropped. Cross-session persistence of embeddings belongs to the gateway backend's Qdrant index, not to this cache.

> **Tradeoff:** Session-scoped cache avoids stale embeddings when the model changes mid-session while keeping the common case (repeated assembly calls within a session) allocation-free after the first pass.

### 13.5 Dimension Handling

The `HybridScorer` currently assumes both the query embedding and candidate embeddings have a consistent length (it returns `0.0` silently when lengths differ — see `hybrid.rs::cosine`). With a real embedding service this silent fallback is not sufficient because a dimension mismatch indicates a misconfiguration, not a missing embedding.

**Rule:** At pipeline construction time, if `embedding_service` is `Some` and `hybrid_config` is `Some`, the pipeline calls `embedding_service.dimensions()` and stores the value. Any candidate embedding on an ingested message whose length does not match this stored dimension is treated as a cache miss — the candidate is re-embedded rather than used as-is. Mixing two different-dimension services in one pipeline is rejected at startup with `EmbeddingError::DimensionMismatch { expected, actual }` logged as an error and causing pipeline construction to fail.

**Re-embedding policy when switching models (e.g., operator changes `model_id` in config):**

- **Phase 1 (this spec):** Lazy. Existing cached embeddings are considered stale whenever `model_id` changes. On the next `assemble` call the stale entries are detected (model_id component of cache key does not match) and re-embedded on demand. No background work.
- **Phase 2 (future):** Eager background reindex — a maintenance task triggered on model change that re-embeds all messages in parallel before the next user turn. Noted here for future implementors; not part of this bead's scope.

> **Tradeoff:** Lazy re-embedding adds latency to the first `assemble` call after a model switch but avoids background task complexity and race conditions in Phase 1. Most model switches happen at restart time when the message history is empty anyway.

### 13.6 Failure & Degradation

The pipeline must remain usable when the embedding service fails. Degradation is always logged and metriced (§13.8).

| Failure mode | Behaviour |
|---|---|
| `EmbeddingError::Provider` | Fall back to **lexical-only scoring**: set `vector_weight = 0.0`, redistribute to `index_weight` and `recency_weight` proportionally so they still sum to `1.0`. Log `warn!` with provider name and error. Increment `embedding_service_failures_total`. |
| `EmbeddingError::Timeout` | Return **partial scoring**: candidates that were embedded before the timeout get their real vector score; the slow candidate gets `vector_score = 0.0`. Emit `debug!` per skipped candidate. Do not increment failure counter (timeouts are expected under load). |
| `EmbeddingError::RateLimited { retry_after }` | Honor `retry_after` if the caller can tolerate the wait (e.g., a background indexing job). If the caller is on the hot path (`assemble`), degrade the same as `Provider` error — do not block the turn. Increment `embedding_service_failures_total`. |
| `EmbeddingError::DimensionMismatch` | Hard error at pipeline construction (§13.5). At runtime (unexpected provider response) treat as `Provider` error and degrade to lexical-only. |
| `EmbeddingError::InvalidInput` | Drop the affected candidate from the embedding batch; assign `vector_score = 0.0` for that candidate. Log `debug!`. |

**Proportional rescaling formula for lexical-only fallback:**

```
total_remaining = index_weight + recency_weight
index_weight'   = index_weight   / total_remaining
recency_weight' = recency_weight / total_remaining
vector_weight'  = 0.0
```

This preserves the relative balance between BM25 and recency that the operator configured.

### 13.7 Security & Data Handling

**External embedding flag:**

```yaml
agents:
  - name: "my-agent"
    tier: 3                    # Tier-3 is the most restrictive
    memory:
      embedding:
        allow_external_embedding: false   # default for Tier-3; must be explicit true to use OpenAI
```

- `allow_external_embedding: false` (default for Tier-3) means the pipeline resolver rejects any provider whose `model_id` is served by a remote API at startup. Only providers whose HTTP client points to a local address (loopback or internal network) are permitted. `OllamaEmbeddingService` always passes; `OpenAIEmbeddingService` is rejected.
- `allow_external_embedding: true` must be explicit in the agent manifest. It is not inheritable from parent config — it must be set per agent.
- Tier-1 and Tier-2 agents default `allow_external_embedding: true` (these tiers are presumed to be less sensitive workloads).

**Redaction before embedding:** `sera-secrets` does not currently expose a `Redactor` type (confirmed by inspection of `rust/crates/sera-secrets/src/lib.rs` — only `SecretsProvider` implementations are exported). Until a `Redactor` is added to that crate, the embedding pipeline applies a **best-effort pattern redaction** inline: before passing content to `embed`, it replaces strings matching the patterns configured under `sera.secrets.redact_patterns[]` (a new config key to be added in the config spec) with the placeholder `[REDACTED]`. This is not cryptographically guaranteed — full redaction support is a follow-on task.

> **Tradeoff:** Inline pattern redaction is imperfect but provides a safety net for the common cases (API keys, tokens) without blocking this bead on a `Redactor` trait that does not yet exist.

### 13.8 Metrics

The following counters and histograms are **named but not implemented** here — they are registered during the implementation bead and emitted by the embedding service wrapper:

| Metric | Type | Labels | Description |
|---|---|---|---|
| `embedding_requests_total` | Counter | `provider`, `model`, `result` (`ok`\|`error`\|`timeout`\|`rate_limited`) | Total embedding API calls, by outcome |
| `embedding_latency_seconds` | Histogram | `provider`, `model` | End-to-end latency per batch call (not per text) |
| `embedding_cache_hit_ratio` | Gauge | `provider`, `model` | Ratio of cache hits to total embedding lookups in the current session |
| `embedding_batch_size` | Histogram | `provider`, `model` | Number of texts per `embed` call — tracks batching efficiency |

All metrics follow the existing `sera-telemetry` OTel conventions. The `provider` label is derived from the `EmbeddingService` implementation's type name (e.g., `"openai"`, `"ollama"`, `"stub"`).

### 13.9 Test Strategy

**Unit tests** (no network, no Docker):

- Trait mock: a `MockEmbeddingService` in `sera-testing` (behind `feature = "testing"`) that returns deterministic embeddings derived from a hash of the input text (e.g., hash the first 4 bytes of SHA-256 into a normalised `f32` vector of the configured `dimensions`). Non-zero and deterministic, unlike the stub.
- Dimension-mismatch path: construct a pipeline where the service reports `dimensions() = 768` but inject a message with a 1536-dim embedding; assert that the pipeline re-embeds rather than using the stale vector.
- Rate-limit retry accounting: `MockEmbeddingService` configured to return `EmbeddingError::RateLimited { retry_after: Some(1) }` on the first call and `Ok` on the second; assert that hot-path `assemble` degrades rather than blocking, and that the background path retries.
- Lexical fallback weight rescaling: configure `index_weight = 0.6, vector_weight = 0.3, recency_weight = 0.1`; trigger a `Provider` error; assert `index_weight' ≈ 0.857`, `recency_weight' ≈ 0.143`, `vector_weight' = 0.0`.

**Integration tests** (gated behind `--features integration`, require a running Ollama instance):

- `OllamaEmbeddingService::embed(&["hello world"])` returns a vector of length 768 with at least one non-zero element.
- `health()` resolves `Ok(())` when Ollama is reachable.

**Property-based tests** (using `proptest` or equivalent):

- `embed(empty) == Ok(vec![])` — for any provider configuration.
- `len(result) == len(input)` — returned slice length always equals input slice length.
- `len(result[i]) == dimensions()` — every returned embedding has the declared length.

### 13.10 Open Questions

1. **`text-embedding-3-large` as a tier upgrade:** Should `OpenAIEmbeddingService` support `text-embedding-3-large` (3072 dims) as a configurable alternative to the 1536-dim default? The cost is approximately 2× per token. This is a human decision about cost vs. retrieval quality — if it should be supported, the `OpenAIEmbeddingConfig::dimensions` field already allows it, but the default and any guardrails need operator sign-off.
2. **No embedding service configured at all:** When neither an OpenAI key nor an Ollama URL is provided, should `ContextPipeline` refuse to start (forcing the operator to acknowledge the degraded mode) or silently fall back to lexical scoring with a startup warning? The current stub behavior is silent fallback, but making it explicit opt-in (`hybrid_config.require_embedding: false`) would prevent surprise degradation in production.
3. **Cross-session embedding persistence:** Session-scoped cache means every new session re-embeds the entire history on first `assemble`. For long-lived agents with large histories this is expensive. Should the implementor wire `EmbeddingService` into the gateway backend's Qdrant index so that embeddings computed during one session are reused in the next? This would require the `(model_id, content_hash)` cache key to be stored in Qdrant alongside the vector, and is likely a Phase 2 concern — but the implementor should confirm scope with the architect.

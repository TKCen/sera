# SERA 2.0 — Rust Workspace Architecture Guide

> **Companion to:** [architecture.md](architecture.md) (visual diagrams) · [ARCHITECTURE-ADDENDUM-2026-04-13.md](ARCHITECTURE-ADDENDUM-2026-04-13.md) (10 canonical design decisions) · [ARCHITECTURE-ADDENDUM-2026-04-16.md](ARCHITECTURE-ADDENDUM-2026-04-16.md) (Hermes comparison refinements: MemoryBlock, native hooks, sera-commands, flush_min_turns) · [Spec Index](specs/README.md)
> **Scope:** Rust workspace (`rust/`) — crate graph, data flow, extensibility model, deployment tiers
> **Read first:** [ARCHITECTURE-ADDENDUM-2026-04-13.md](ARCHITECTURE-ADDENDUM-2026-04-13.md) for the ten canonical design decisions, then [ARCHITECTURE-ADDENDUM-2026-04-16.md](ARCHITECTURE-ADDENDUM-2026-04-16.md) for Hermes-informed refinements.

---

## 1. Mental Model

SERA's gateway is a **Manufacturing Execution System (MES) for AI agents**. Workers (runtimes) are ephemeral cattle. All durable state — sessions, memory, audit records, credentials — lives at the gateway. A worker crash loses nothing.

Three invariants govern every design decision in this workspace:

1. **Workers are cattle; state is gateway-owned.** Any state that needs to survive a worker restart must live at the gateway.
2. **Runtime is a tool consumer, not a tool executor.** The runtime forwards tool calls; the gateway dispatches and executes them.
3. **Recompilation ≠ configuration.** Switching backends is a config change. Adding new backends is a code change. Never conflate them.

---

## 2. Crate Dependency Graph

The workspace has one true leaf crate (`sera-types`) from which all others grow. The gateway aggregates everything.

```
sera-types  ←──────────────────────────────────────────── leaf (no internal deps)
  │
  ├── sera-errors          error types scaffold
  ├── sera-commands        shared command registry (CLI + gateway dispatch) — Hermes pattern
  ├── sera-config          env/file config loading; manifest_loader (K8s-style YAML)
  ├── sera-session         6-state SessionStateMachine, ContentBlock transcript
  ├── sera-cache           cache layer scaffold
  ├── sera-secrets         secrets management scaffold
  │
  └── MEMORY BLOCK (struct)   priority: f32, recency_boost: f32, char_budget: usize
                              injected into every ContextWindow; self-assembly via
                              1-sentence summary header for semantic lookups
  │
  ├── sera-db              PostgreSQL (sqlx) + SQLite (rusqlite), migrations, repos
  │     └── sera-auth      API keys, JWT, OIDC, axum middleware
  │
  ├── sera-events          audit trail, Centrifugo pub/sub, lifecycle events
  ├── sera-hooks           in-process Hook registry + ChainExecutor
  ├── sera-hitl            HITL approval routing, escalation chains
  ├── sera-workflow        workflow engine, dreaming config, cron scheduling
  ├── sera-telemetry       OTel tracing, AuditBackend, LaneFailureClass
  ├── sera-queue           QueueBackend trait, LocalQueueBackend, GlobalThrottle
  ├── sera-tools           SandboxProvider, SsrfValidator, BashAstChecker
  │
  ├── sera-runtime  (lib + bin)
  │     └── ContextEngine, CompactionStrategy, condensers, turn loop
  │           depends on: sera-types, sera-config, sera-session, sera-events,
  │                       sera-hooks, sera-telemetry, sera-tools
  │
  ├── sera-testing  (lib)   MockQueueBackend, MockSandboxProvider, contract tests
  ├── sera-tui      (bin)   ratatui terminal UI; depends on sera-types + reqwest only
  ├── sera-byoh-agent (bin) BYOH reference implementation; connects via gRPC
  │
  └── sera-gateway  (bin)   ← aggregates all of the above
        axum HTTP/WS server, gRPC server, channel connectors,
        lane-aware queue, session state machine, hook orchestration,
        tool dispatch, auth, config surface, observability
```

**Key constraint:** `sera-tui` intentionally depends only on `sera-types` and `reqwest` — it talks to the gateway over HTTP/WS and must not import gateway internals.

For crate-level documentation see [`docs/plan/crate-docs/`](crate-docs/) (coverage grows per session). For the full dependency rationale see [`specs/SPEC-crate-decomposition.md`](specs/SPEC-crate-decomposition.md) and [`specs/SPEC-dependencies.md`](specs/SPEC-dependencies.md).

---

## 3. Data Flow — Event Ingress to Response

Every event follows a single path through the system. Hook injection points are marked `[⛓]`.

```
External world
  │  WebSocket / HTTP / gRPC / Discord / Slack / Webhook
  ▼
┌─────────────────────────────────────────────────────────────┐
│  sera-gateway: Event Ingress                                │
│                                                             │
│  [⛓ edge_ingress]  ← webhooks, callbacks                  │
│       │                                                     │
│  Event Router                                               │
│  [⛓ pre_route]     ← authz, dedupe, rate-limit, PII filter │
│       │                                                     │
│  Lane-Aware FIFO Queue  (sera-queue::QueueBackend)          │
│  ┌──────────────────────────────────────────────────────┐   │
│  │  Lane modes: collect · followup · steer · interrupt  │   │
│  │  One writer per session — no races                   │   │
│  │  Global concurrency cap (GlobalThrottle)             │   │
│  └──────────────────────────────────────────────────────┘   │
│       │                                                     │
│  Session State Machine  (sera-session)                     │
│  Created → Active → Compacting → Archived                  │
│  [⛓ post_route]    ← audit, HITL hooks, async tasks      │
│       │                                                     │
│  Dequeue turn → inject context window                       │
│  (soul, memory, tool schemas — all assembled gateway-side)  │
│  Hook naming (Hermes ↔ SERA):                              │
│    Hermes: prefetch_all() + sync_all() + queue_prefetch_all() │
│    SERA:   pre_agent_turn + post_human_turn                 │
└─────────────────────────┬───────────────────────────────────┘
                          │ TurnContext
                          ▼
┌─────────────────────────────────────────────────────────────┐
│  sera-runtime: Turn Loop                                    │
│                                                             │
│  [⛓ pre_turn]                                              │
│       │                                                     │
│  ContextEngine::assemble(&TurnContext) → ContextWindow
│  (pluggable — default pipeline, RAG-heavy, LCM/DAG,
│   minimal; CompactionStrategy fires when budget nears)
│  flush_min_turns: 6  ← auto-prompt skill creation after N tool calls
│  (Hermes self-improving skills loop — agent proposes saving approach as skill)
│       │
│  LLM call (provider-agnostic; gateway acts as LLM proxy)
│       │                                                     │
│  ┌── tool_call loop ────────────────────────────────────┐   │
│  │  runtime emits tool_call event                       │   │
│  │  gateway receives → [⛓ pre_tool] → dispatch         │   │
│  │  → CapabilityPolicy check → executor                 │   │
│  │  → [⛓ post_tool] → tool_result back to runtime      │   │
│  │  (runtime never holds credentials or executor refs)  │   │
│  └───────────────────────────────────────────────────────┘   │
│       │                                                     │
│  [⛓ post_turn]                                             │
│       │                                                     │
│  TurnOutcome → memory write (gateway-side) → deliver        │
│  [⛓ pre_deliver / post_deliver]                            │
└─────────────────────────────────────────────────────────────┘
                          │
                          ▼
              Response to client / channel

Scheduler (sera-workflow) injects synthetic events into the queue
at any point — cron jobs, dreaming workflows, heartbeats follow
the same path as user-originated events.
```

Spec references: [`SPEC-gateway.md §1b`](specs/SPEC-gateway.md) (tool dispatch), [`SPEC-runtime.md §1a`](specs/SPEC-runtime.md) (runtime responsibilities), [`SPEC-hooks.md §1a`](specs/SPEC-hooks.md) (layer assignment — which hooks are gateway-side vs harness-side).

---

## 4. Trait-Based Extensibility Model

Behaviour is swapped at the trait boundary, not by recompiling. All traits are `Send + Sync + 'static` and used behind `Arc<dyn Trait>` or `Box<dyn Trait>`.

### 4.1 Core Traits

| Trait | Crate | Purpose |
|---|---|---|
| `QueueBackend` | `sera-queue` | Enqueue/dequeue turns. Implementations: `LocalQueueBackend` (SQLite), PostgreSQL (`apalis` 0.7 in Tier 3). Config selects active backend. |
| `ContextEngine` | `sera-runtime` | Assemble a `ContextWindow` from a `TurnContext`. Pluggable strategy: default pipeline, RAG-heavy, LCM/DAG, minimal. Harness does not care which engine is active. |
| `CompactionStrategy` | `sera-runtime` | Compact session history when the token budget nears threshold. Built-in: `AgentAsSummarizer` (default), `AlgorithmicCleanup`, `Hybrid`. |
| `Hook` | `sera-hooks` | In-process hook interface. **Built-in hooks are native Rust trait methods.** `WasmHookAdapter` implements the same trait for third-party opt-in isolation only — WASM chain executor is over-engineering for built-in hooks; plain method calls at known points work better (Hermes lesson). |
| `SandboxProvider` | `sera-tools` | Execute sandboxed tool calls. Implementations: native (bollard + wasmtime), `OpenShellSandboxProvider` (gRPC, Tier 3), `MockSandboxProvider` (tests). |
| `MemoryBackend` | `sera-gateway` | Read/write agent memory. File backend (Tier 1/2), PostgreSQL + Qdrant backend (Tier 2/3). Runtime never calls this directly — gateway assembles a priority-ordered `MemoryBlock` (SPEC-memory §2.1) and injects it as opaque context. **2-tier injection model (Hermes pattern): compact always-present MemoryBlock + semantic search on demand. Add tiers only when 2-tier proves insufficient.** Each MemoryBlock has: `priority: f32`, `recency_boost: f32`, `char_budget: usize`, and a 1-sentence summary header enabling the agent to self-look up relevant memories. |
| `AgentRuntimeService` | gRPC (proto) | External harness registration. BYOH runtimes (Claude Code, Codex, Hermes) implement this service and connect via gRPC. The embedded `sera-runtime` is called as a library — no gRPC hop. |

### 4.2 Trait Signatures (abbreviated)

```rust
// sera-queue
#[async_trait]
pub trait QueueBackend: Send + Sync {
    async fn enqueue(&self, event: QueuedEvent) -> Result<EventId, QueueError>;
    async fn dequeue(&self, session_id: SessionId) -> Result<Option<QueuedEvent>, QueueError>;
    async fn ack(&self, event_id: EventId) -> Result<(), QueueError>;
}

// sera-runtime
#[async_trait]
pub trait ContextEngine: Send + Sync {
    async fn assemble(&self, ctx: &TurnContext) -> Result<ContextWindow, ContextError>;
    fn describe(&self) -> EngineDescription;
}

#[async_trait]
pub trait CompactionStrategy: Send + Sync {
    async fn compact(&self, history: &[Turn], config: &CompactionConfig)
        -> Result<CompactedContext, CompactionError>;
    fn name(&self) -> &str;
}

// sera-hooks
#[async_trait]
pub trait Hook: Send + Sync {
    fn metadata(&self) -> &HookMetadata;
    async fn execute(&self, ctx: HookContext) -> Result<HookResult, HookError>;
}

// sera-tools
#[async_trait]
pub trait SandboxProvider: Send + Sync {
    async fn execute(&self, call: SandboxedCall) -> Result<SandboxOutput, SandboxError>;
    fn supports(&self, tool: &ToolRef) -> bool;
}
```

See individual crate docs and specs for the full signatures:
- `Hook` trait: [`docs/plan/crate-docs/sera-hooks.md`](crate-docs/sera-hooks.md)
- `ContextEngine`: [`specs/SPEC-runtime.md §2`](specs/SPEC-runtime.md)
- `QueueBackend`: [`specs/SPEC-gateway.md`](specs/SPEC-gateway.md) · [`specs/SPEC-dependencies.md §8.3`](specs/SPEC-dependencies.md) (apalis)

---

## 4.3 SemanticMemoryStore Trait (Tier-2 Pluggable Memory)

The `SemanticMemoryStore` trait (in `sera-types::semantic_memory`) is the user-extensible seam for custom memory backends. See [`docs/plugins/memory.md`](../plugins/memory.md) for the full plugin contract.

```rust
#[async_trait]
pub trait SemanticMemoryStore: Send + Sync + 'static {
    async fn put(&self, entry: SemanticEntry) -> Result<MemoryId, SemanticError>;
    async fn query(&self, query: SemanticQuery) -> Result<Vec<ScoredEntry>, SemanticError>;
    async fn delete(&self, id: &MemoryId) -> Result<(), SemanticError>;
    async fn evict(&self, policy: &EvictionPolicy) -> Result<usize, SemanticError>;
    async fn stats(&self) -> Result<SemanticStats, SemanticError>;
    async fn promote(&self, id: &MemoryId) -> Result<(), SemanticError>;
    async fn touch(&self, id: &MemoryId) -> Result<(), SemanticError>;
    async fn maintenance(&self) -> Result<(), SemanticError>;
}
```

---

## 5. Memory Tiers

Memory is a **two-tier injection model** (SPEC-memory §2.1, ARCHITECTURE-ADDENDUM-2026-04-16 §1): a compact injected block (always present) + pluggable semantic search (on-demand retrieval).

### Tier 0 — MemoryBlock (In-Process)

Always active. Built into `sera-runtime`, injected into every turn at the gateway. A priority-ordered buffer of context segments with character-budget constraints.

**Structure** (from SPEC-memory §2.1):
- `segments: Vec<MemorySegment>` — ordered by priority
- `char_budget: usize` — total token budget
- Each segment: `source`, `content`, `priority` (0 = highest), `recency_boost`, `char_count`

**Key invariants** (from HANDOFF §4.37):
- `SegmentKind::Soul` is priority 0 — never trimmed
- `record_turn()` increments `overflow_turns` and returns true when `flush_min_turns` is reached
- Default `flush_min_turns: 6` — auto-prompt skill creation after N tool calls
- Memory pressure can trigger dreaming, skill creation, or operator notification

**Responsibility:** Gateway or runtime assembles the block; runtime receives it as opaque injected context.

### Tier 1 — SemanticMemoryStore (Pluggable)

The **trait-based extensibility seam** at `sera-types::semantic_memory::SemanticMemoryStore`. Users plug in custom backends here.

**Built-in implementations:**

| Backend | Crate | Scope | Dependencies |
|---|---|---|---|
| `SqliteFtsMemoryStore` | sera-db (landing bead sera-vzce) | Single-node dev | SQLite FTS5 + sqlite-vec, zero external deps |
| `PgVectorStore` | sera-db | Multi-node production | PostgreSQL + pgvector extension, HNSW/IVFFlat indexes |
| User-built (plugin) | any user crate or dylib | Any | Implementing the trait |

**Contract** (full spec in [`docs/plugins/memory.md`](../../plugins/memory.md)):

- `put(entry)` — idempotent by `MemoryId`; embed-and-index if embedding provider wired; fail loudly on error (no silent `vec![0.0; dims]`)
- `query(SemanticQuery)` — hybrid when possible; return `ScoredEntry` with at least one of `index_score`/`vector_score` populated
- `delete(id)` — remove by id; return `NotFound` if absent
- `evict(policy)` — prune by `max_per_agent` and `ttl_days`; respect `promoted_exempt` flag
- `promote(id)` — mark row as exempt from eviction; used by dreaming-workflow consolidation
- `touch(id)` — update `last_accessed_at` for recency tracking
- `stats()` — return aggregate snapshot: total rows, per-agent top-k, oldest/newest timestamps
- `maintenance()` — opportunistic operations (e.g., `REINDEX CONCURRENTLY`) on weekly cron

**Multi-tenant isolation:** All operations scoped by `agent_id`. Queries MUST filter by `agent_id` before vector operations.

**Error policy:** Fail loudly. No silent fallbacks, no degenerate embeddings. Any backend-level issue (database down, embedding failure, dimension mismatch) surfaces as `SemanticError`.

### Tier 2 — ContextEnricher (Auto-Promotion)

Wired in bead sera-0yqq (closed). Auto-promotes top-k `SemanticMemoryStore` hits into the `MemoryBlock` on each turn without a second LLM prompt. Transparent to the runtime.

**Decision matrix** — which tier for your deployment:

| Scenario | Tier 0 only | Tier 0+1 | Tier 0+1+2 |
|---|---|---|---|
| Single-node dev (no embeddings) | X | — | — |
| Single-node with semantic recall | X | X | — |
| Small team (SQLite FTS) | X | X | — |
| Multi-pod production | X | X | X |
| GPU-accelerated RAG | X | X | X |
| Thin-client harness (BYOH) | — | X | X |

---

## 6. Four Extension Points

SERA has exactly four extension mechanisms. They must not be conflated.

| Extension point | Mechanism | When to use |
|---|---|---|
| **Compiled-in backends** | Ships in binary; config selects which is active | Switching between officially supported implementations (e.g., SQLite → PostgreSQL queue, file → Qdrant memory) |
| **Native Rust hooks** (default) | Built-in hooks use plain Rust trait methods at known lifecycle points (`pre_agent_turn`, `post_human_turn`). No WASM overhead, no chain executor ceremony. | Core functionality: authz, PII filtering, audit, HITL routing, memory assembly |
| **WASM hooks** (opt-in) | Runtime-loaded, sandboxed middleware — **for third-party isolation only**. Built-in hooks are NOT WASM. | Third-party custom authz, content policy, PII filtering — small, fast, stateless, inline. No host FS/net access; external calls go through the gateway's approved proxy. |
| **gRPC plugins** | Out-of-process services registered with the gateway | Custom backends, enterprise connectors (Siemens PLC, SCADA, ERP), custom runtimes (BYOH), any language — stateful, independently scaled, own lifecycle |

**Example decisions:**
- "We need JWT auth" → change `auth.backend: oidc` in config. No recompilation.
- "We need a content filter for a regulated industry" → write a WASM hook, deploy without recompiling.
- "We need a Siemens S7 PLC connector" → implement the gRPC `ToolExecutor` service, register it with the gateway. No binary change.
- "We need Claude Code as a BYOH harness" → implement `AgentRuntimeService` gRPC, point it at the gateway endpoint.

Spec: [`SPEC-plugins.md`](specs/SPEC-plugins.md) (gRPC plugin interface, mTLS, ToolExecutor/MemoryBackend gRPC contracts) · [`SPEC-hooks.md §1a`](specs/SPEC-hooks.md) (WASM hook layer assignment).

---

## 6a. Agent Delegation (bead sera-a1u)

SERA exposes two complementary delegation paths to agents:

1. **Fire-and-forget spawn** via the `spawn-ephemeral` tool — the caller
   hands off a task and does not wait for the result. Best when the parent
   has nothing useful to do after dispatch (e.g. "log and move on"
   triggers, notification dispatch). Flows through sera-core's sandboxed
   subagent runner.
2. **Richer session primitives** — `session_spawn`, `session_yield`, and
   `session_send`. These let a parent stay in conversation with a named
   child session across multiple turns without blocking its own loop.

| Tool | Risk | Use it when… |
|---|---|---|
| `session_spawn` | `Execute` | You need a named child session you can refer to later (e.g. "start the researcher and keep working"). Returns a stable `session_id`. |
| `session_yield` | `Read` | You want to pause the current turn and wait for the child's next event. Returns the event as a tool result (bounded by `timeout_secs`, default 120 s). |
| `session_send` | `Write` | You want to push a message into a named child session. Fire-and-forget unless a peer is yielding on the same session. |

Under the hood, the three tools share a `DelegationBus` (see
`sera-runtime/src/delegation_bus.rs`) — a lightweight in-process pub/sub
over child-session events. The bus exposes four event types:

- `MessageEmitted { content }` — intermediate child output (e.g. streaming
  delta coalesced for the parent).
- `TurnCompleted { output }` — child finished one turn with a final answer.
- `SessionClosed { reason }` — child session terminated.
- `Error { message }` — child produced an error.

Each `subscribe_next(session_id)` call returns a fresh
`tokio::sync::oneshot::Receiver`. Multiple concurrent yields on the same
session queue as independent subscribers; a `publish` call fires each
pending oneshot with a clone of the event in FIFO order, so two sibling
agents may both yield on the same child and each receive the response.

**Rule of thumb:** default to `spawn-ephemeral` when you don't need the
reply; reach for `session_spawn` + `session_yield` whenever the parent
needs to interleave its own reasoning with the child's answers.

---

## 7. Deployment Tiers

All tiers run the **same binary**. The tier table describes common groupings of config activations — not separate architectures or code paths.

> **Invariant:** Switching tiers is a config-and-restart operation. Not a deployment or recompilation event.

### Tier 1 — Local / Development

| Aspect | Choice |
|---|---|
| Entrypoint | Single process: `sera start` |
| Database | SQLite (runtime state: sessions, queue, audit) |
| Memory | File-based (soul.md, memory.md, knowledge/*.md) |
| Queue | `LocalQueueBackend` (SQLite-backed) |
| Cache | In-memory |
| Auth | Autonomous (no auth, or auto-generated admin key) |
| Secrets | Environment variables |
| Sandbox | None (by default) |
| Connectors | Built-in in-process (Discord, Slack) |
| Model providers | gRPC to local provider (LM Studio, Ollama) |

```
┌─────────────────────────────┐
│  sera start                  │
│  ┌───────────────────────┐  │
│  │  sera-gateway         │  │
│  │  ├─ HTTP/WS server    │  │
│  │  ├─ gRPC server       │  │
│  │  ├─ sera-runtime (lib)│  │
│  │  ├─ Discord connector │  │
│  │  ├─ SQLite (state)    │  │
│  │  ├─ File memory       │  │
│  │  └─ In-memory cache   │  │
│  └───────────────────────┘  │
│          ↕ gRPC              │
│  External adapters (opt.)   │
└─────────────────────────────┘
```

### Tier 2 — Team / Private

| Aspect | Choice |
|---|---|
| Deployment | Docker Compose or single server |
| Database | PostgreSQL |
| Memory | File + git, or PostgreSQL |
| Queue | PostgreSQL-backed |
| Cache | Redis |
| Auth | JWT auth, basic RBAC |
| Secrets | File-based (encrypted) |
| Connectors | Built-in + external gRPC |

### Tier 3 — Enterprise

| Aspect | Choice |
|---|---|
| Deployment | Kubernetes / Nomad; OpenShell K3s-in-Docker as sandbox backend option |
| Database | PostgreSQL HA (or Dolt SQL server for multi-writer task graphs) |
| Memory | File + git, PostgreSQL, or LCM |
| Queue | `apalis` 0.7 with Postgres/Redis backend |
| Cache | Redis Cluster or Dragonfly |
| Auth | OIDC + AuthZen + SSF CAEP/RISC |
| Secrets | Vault / AWS SM / Azure KV / GCP SM |
| Connectors | External gRPC, independently scaled |
| HA | Multi-node with leader election; two-generation boot for zero-downtime self-evolution |
| Constitutional signer | Operator HSM or air-gapped key store |

Spec: [`SPEC-deployment.md`](specs/SPEC-deployment.md).

---

## 8. Key Design Principles

These principles (established 2026-04-13) inform every crate boundary and trait decision:

1. **Workers are cattle; state is gateway-owned.** Any state that needs to survive a worker restart must live at the gateway.
2. **Runtime is a tool consumer, not a tool executor.** The runtime forwards tool calls; the gateway dispatches and executes them.
3. **Policy hooks belong to the gateway.** Any hook that enforces a security or compliance decision must run in the gateway process. Harness-side hooks govern formatting and context assembly — they cannot be bypassed but do not enforce security policy.
4. **Recompilation ≠ configuration.** Switching backends is a config change; adding new backends is a code change.
5. **Three extension points, never conflated.** Compiled-in (config selects), WASM hooks (inline middleware), gRPC plugins (out-of-process).
6. **One binary, all modes.** `sera start` and an enterprise Kubernetes deployment run the same binary. Config activates features.
7. **Gateway as LLM proxy.** All LLM calls from any connected harness (including BYOH) route through the gateway for budget enforcement, cost attribution, provider routing, audit, and content filtering.

Full rationale: [`ARCHITECTURE-ADDENDUM-2026-04-13.md`](ARCHITECTURE-ADDENDUM-2026-04-13.md).

---

## 9. Crate Documentation Index

| Crate | Doc | Status |
|---|---|---|
| `sera-hooks` | [`crate-docs/sera-hooks.md`](crate-docs/sera-hooks.md) | Complete |
| `sera-auth` | `crate-docs/sera-auth.md` | Pending |
| `sera-byoh-agent` | `crate-docs/sera-byoh-agent.md` | Pending |
| `sera-cache` | `crate-docs/sera-cache.md` | Pending |
| `sera-config` | `crate-docs/sera-config.md` | Pending |
| `sera-db` | `crate-docs/sera-db.md` | Pending |
| `sera-errors` | `crate-docs/sera-errors.md` | Pending |
| `sera-events` | `crate-docs/sera-events.md` | Pending |
| `sera-gateway` | `crate-docs/sera-gateway.md` | Pending |
| `sera-hitl` | `crate-docs/sera-hitl.md` | Pending |
| `sera-queue` | `crate-docs/sera-queue.md` | Pending |
| `sera-runtime` | `crate-docs/sera-runtime.md` | Pending |
| `sera-secrets` | `crate-docs/sera-secrets.md` | Pending |
| `sera-session` | `crate-docs/sera-session.md` | Pending |
| `sera-telemetry` | `crate-docs/sera-telemetry.md` | Pending |
| `sera-testing` | `crate-docs/sera-testing.md` | Pending |
| `sera-tools` | `crate-docs/sera-tools.md` | Pending |
| `sera-tui` | `crate-docs/sera-tui.md` | Pending |
| `sera-types` | `crate-docs/sera-types.md` | Pending |
| `sera-workflow` | `crate-docs/sera-workflow.md` | Pending |

---

## 10. Navigation

| Question | Go to |
|---|---|
| What does the system look like end-to-end? | [`architecture.md`](architecture.md) — mermaid diagrams |
| Why was X designed this way? | [`ARCHITECTURE-ADDENDUM-2026-04-13.md`](ARCHITECTURE-ADDENDUM-2026-04-13.md) |
| What are the acceptance criteria for a spec? | [`specs/SPEC-{name}.md`](specs/README.md) |
| What does crate X export? | [`crate-docs/sera-{name}.md`](crate-docs/) |
| How do I build / test the workspace? | [`rust/CLAUDE.md`](../../rust/CLAUDE.md) |
| What is the migration plan from TypeScript? | [`rust/MIGRATION-PLAN.md`](../../rust/MIGRATION-PLAN.md) |
| What is the implementation order? | [`docs/IMPLEMENTATION-ORDER.md`](../IMPLEMENTATION-ORDER.md) |

---

## 11. Local profile: boot matrix

SERA 2.0 ships with a dual-backend data layer so `sera-gateway start` boots
against either a pure-SQLite local profile (zero infra) or a Postgres-backed
enterprise profile (`DATABASE_URL` set). Each repository trait has both
implementations; the gateway chooses at boot.

| Store | Local (SQLite) | Enterprise (Postgres) | Trait |
| ----- | -------------- | --------------------- | ----- |
| Secrets | `SqliteSecretsStore` | `PgSecretsStore` | `sera_db::secrets::SecretsStore` |
| Schedules | `SqliteScheduleStore` | `PgScheduleStore` | `sera_db::schedules::ScheduleStore` |
| Audit | `SqliteAuditStore` | `PgAuditStore` | `sera_db::audit::AuditStore` |
| Metering | `SqliteMeteringStore` | `PgMeteringStore` | `sera_db::metering::MeteringStore` |
| Agents | `SqliteAgentStore` | `PgAgentStore` | `sera_db::agents::AgentStore` |
| Lane counters | `InMemoryLaneCounter` | `PostgresLaneCounter` | `sera_db::lane_queue_counter::LaneCounterStore` |
| Proposal usage | `InMemoryProposalUsageStore` | `PostgresProposalUsageStore` | `sera_db::proposal_usage::ProposalUsageStore` |
| Semantic memory | `SqliteMemoryStore` (FTS5 + sqlite-vec + RRF) | `PgVectorStore` | `sera_types::semantic_memory::SemanticMemoryStore` |

### Schema provisioning

All SQLite-backed tables are created idempotently via
[`sera_db::sqlite_schema::init_all`](../../rust/crates/sera-db/src/sqlite_schema.rs),
which the gateway calls once at boot before any store is constructed. Each
module also exposes its own `init_schema(conn)` so ad-hoc deployments can
opt into a subset.

Tables created on the local profile: `agent_instances`, `agent_templates`,
`audit_trail`, `token_usage`, `usage_events`, `token_quotas`, `schedules`,
`secrets`. The existing `sessions`, `transcript`, `queue`, `audit_log`, and
`training_exports` tables (owned by `SqliteDb`) are created by
[`sera_db::sqlite::SqliteDb::initialize`](../../rust/crates/sera-db/src/sqlite.rs)
and remain independent of `sqlite_schema::init_all`.

### Selection rule

```
if DATABASE_URL is set and Postgres reachable → Pg<Store>
else                                           → Sqlite<Store>
```

The enterprise path keeps the full sqlx-backed surface (PostgreSQL arrays,
`NOW()`, `make_interval`, JSONB). The local path substitutes `datetime('now')`,
`datetime('now', '-N hours')`, and JSON text serialisation for array columns
(`tags`, `allowed_agents`). Multi-tenant isolation is preserved in both
backends — every `agent_id` / `actor_id` / `agent_instance_id` filter applies
identically.

### Testing the local boot

An integration smoke test lives at
[`rust/crates/sera-gateway/tests/local_boot_test.rs`](../../rust/crates/sera-gateway/tests/local_boot_test.rs)
and verifies all five SQLite stores can be constructed from a shared in-memory
connection without `DATABASE_URL`. Run with:

```bash
cargo test -p sera-gateway --test local_boot_test
```

## Session Transcript Indexing (sera-4nj)

On session close, the runtime extracts a compact summary of the conversation
and persists it into the `SemanticMemoryStore` so future sessions can recall
it via the standard `ContextEnricher` recall path.

**Pipeline:**

1. Session transitions to `Archived` / `Closed`.
2. `SemanticTranscriptIndexer::index_transcript(agent_id, session_id, started_at, transcript)` runs.
3. The indexer strips tool-result payloads, summarises tool-use calls as
   `[tool:<name>] args={k1,k2}`, drops internal thinking blocks, and joins
   the remaining user + assistant text.
4. The blob is stored as a `SemanticEntry` with
   `tier = SegmentKind::Custom("session_transcript")` and tag
   `kind:session_transcript` plus `session_id:…` / `started_at:…`.
5. Subsequent sessions surface past transcripts through the existing
   semantic-recall path — no new query code needed.

**Size policy:** raw tool I/O is never persisted; per-entry text is capped at
`MAX_ENTRY_CHARS` (2 000) and the composite blob at `MAX_BLOB_CHARS` (32 000).

**Failure policy:** indexing is best-effort. Store errors log at `warn` and
the session close path continues unimpeded.

## SKILL.md Format (sera-4nj)

Skills can be authored as a single `.md` file with YAML frontmatter:

```markdown
---
name: lookup-invoice
description: Find an invoice by its external id via the finance API
inputs:
  invoice_id: string
tier: 1
---

# Behaviour
When asked about invoices, ...
```

The loader (`sera_skills::md_loader::load_skill_md`) validates `name` and
`description` as required fields, defaults `tier` to `1`, and logs at `warn`
(without failing) for unknown frontmatter keys so user-authored files stay
resilient to drift.

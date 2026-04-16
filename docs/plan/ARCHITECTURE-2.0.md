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

## 5. Four Extension Points

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

## 6. Deployment Tiers

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

## 7. Key Design Principles

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

## 8. Crate Documentation Index

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

## 9. Navigation

| Question | Go to |
|---|---|
| What does the system look like end-to-end? | [`architecture.md`](architecture.md) — mermaid diagrams |
| Why was X designed this way? | [`ARCHITECTURE-ADDENDUM-2026-04-13.md`](ARCHITECTURE-ADDENDUM-2026-04-13.md) |
| What are the acceptance criteria for a spec? | [`specs/SPEC-{name}.md`](specs/README.md) |
| What does crate X export? | [`crate-docs/sera-{name}.md`](crate-docs/) |
| How do I build / test the workspace? | [`rust/CLAUDE.md`](../../rust/CLAUDE.md) |
| What is the migration plan from TypeScript? | [`rust/MIGRATION-PLAN.md`](../../rust/MIGRATION-PLAN.md) |
| What is the implementation order? | [`docs/IMPLEMENTATION-ORDER.md`](../IMPLEMENTATION-ORDER.md) |

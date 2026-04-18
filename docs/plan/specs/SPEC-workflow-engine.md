# SPEC: Workflow Engine (`sera-workflow`)

> **Status:** DRAFT
> **Source:** PRD §6.3 (triggered workflows, dreaming), plus deltas from [SPEC-dependencies](SPEC-dependencies.md) §10.3 (Paperclip 4-trigger wakeup taxonomy, three-layer failure model), §10.4 (**`gastownhall/beads` data model — promoted from Phase-3 deferral to Phase-1 design input**, atomic `--claim` protocol, `bd ready` algorithm, `Issue` schema, `DependencyType` with `conditional_blocks`, content-hash IDs, `Wisp`/ephemeral lifecycle, `bd prime` context injection, Dolt conflict resolution, `wasteland` federation), §10.15 (MetaGPT termination triad: `n_round` + `is_idle` + `NoMoneyException`), §10.16 (BeeAI `Workflow` step sentinels), §10.17 (CAMEL pause/resume via `WorkforceSnapshot`), [SPEC-self-evolution](SPEC-self-evolution.md) §5.8 (`meta_scope` field for self-evolution task routing; atomic claim on Change Artifact lane)
> **Crate:** `sera-workflow`
> **Priority:** Phase 1

---

## 1. Overview

The workflow engine is a **general-purpose triggered task system** for agent-driven background work. It is not limited to memory operations — it can power any autonomous agent task that runs on a schedule, in response to events, or when thresholds are exceeded.

Dreaming (background memory consolidation) is a **built-in workflow** — not special-cased. The same engine powers knowledge audits, inbox triage, workspace cleanup, or any domain-specific background task.

Workflows are **agent-executed** — they produce turns, use tools, and write memory, subject to all the same hooks and policies as interactive turns.

---

## 2. Workflow Definition

```rust
pub struct WorkflowDef {
    pub name: String,                     // e.g., "dreaming", "knowledge-audit", "inbox-triage"
    pub trigger: WorkflowTrigger,
    pub agent: AgentRef,                  // Which agent executes this workflow
    pub config: serde_json::Value,        // Workflow-specific configuration
    pub enabled: bool,
    pub hook_chain: Option<HookChainRef>, // Optional hook chain on trigger
    pub step_sentinels: bool,              // BeeAI Workflow step sentinel semantics (§2d)
}

pub enum WorkflowTrigger {
    Cron(CronSchedule),                   // "0 3 * * *" — daily at 3 AM
    Event(EventPattern),                  // On specific event types
    Threshold(ThresholdCondition),        // When memory size exceeds X, session count > N, etc.
    Manual,                               // Triggered by principal via API/CLI
}
```

### 2a. WorkflowTask Data Model (new — beads Issue shape)

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.4 gastownhall/beads `Issue`. **Beads integration has been promoted from Phase-3 deferral to Phase-1 design input.** The `bd ready` algorithm and `--claim` atomic protocol are foundational to SERA's workflow engine, not optional enhancements.

```rust
pub struct WorkflowTask {
    /// Content-hash ID (SHA-256 of canonical fields) — merge-safe across branches, beads-style.
    pub id: WorkflowTaskId,

    pub title: String,
    pub description: String,
    pub acceptance_criteria: String,
    pub status: WorkflowTaskStatus,
    pub priority: u8,                            // 0 (critical) to 4
    pub task_type: WorkflowTaskType,             // Task | Epic | Bug | Feature | Chore | Decision | Message | Spike | Story | Milestone
    pub assignee: Option<PrincipalRef>,
    pub due_at: Option<DateTime<Utc>>,
    pub defer_until: Option<DateTime<Utc>>,      // Hide from ready queue until this time
    pub metadata: serde_json::Value,             // Arbitrary JSON for extensions

    /// Gate fields (async coordination) — beads AwaitType enum.
    pub await_type: Option<AwaitType>,
    pub await_id: Option<String>,
    pub timeout: Option<Duration>,

    /// Molecule/workflow composition fields.
    pub mol_type: Option<MolType>,
    pub work_type: Option<WorkType>,

    /// Messaging + ephemeral lifecycle (beads Wisp pattern).
    pub ephemeral: bool,                          // Not synced via git, TTL-compacted
    pub wisp_type: Option<WispType>,

    /// Provenance.
    pub source_formula: Option<String>,
    pub source_location: Option<String>,

    /// Self-evolution routing — when Some, this task flows through the Tier-2/3 pipeline
    /// instead of normal execution. SPEC-self-evolution §5.8.
    pub meta_scope: Option<BlastRadius>,

    /// Change Artifact provenance for self-evolution tasks.
    pub change_artifact_id: Option<ChangeArtifactId>,
}

/// Status lifecycle — beads adds `hooked` and `pinned` beyond typical issue trackers.
pub enum WorkflowTaskStatus {
    Open,                    // Ready for claim (subject to dependencies + defer_until)
    InProgress,               // Assignee is working on it
    /// Atomically claimed by a worker via claim_task() — prevents double-assignment races.
    /// Distinct from InProgress (which is a looser "actively worked on" signal).
    Hooked,
    Blocked,                  // Waiting on a dependency
    Deferred,                  // Hidden from ready queue until defer_until
    Closed,                   // Terminal: successful completion
    Pinned,                   // Persistent context anchor; never auto-closed
}

/// Await/gate type — the thing this task is waiting on before it can proceed.
pub enum AwaitType {
    GhRun,                    // GitHub Actions run
    GhPr,                     // GitHub pull request
    Timer,                    // Time-based gate
    Human,                    // Human approval gate
    Mail,                     // External mail/notification gate
    Change,                   // Change Artifact approval gate (self-evolution)
}
```

### 2b. Dependency Types (incl. `conditional_blocks`)

```rust
pub enum DependencyType {
    /// Standard dependency: A blocks B (B cannot start until A is closed).
    Blocks,

    /// Non-blocking related link for navigation / cross-reference.
    Related,

    /// Parent → child hierarchy (epic → task → sub-task). Children block parent from closing.
    ParentChild,

    /// B was discovered while working on A. Maintains provenance chain for audit.
    DiscoveredFrom,

    /// **Conditional blocking: B runs only if A fails.** A branching primitive not present
    /// in most task-graph systems; enables elegant failure-handler tasks without ad-hoc plumbing.
    /// Source: SPEC-dependencies §10.4 beads.
    ConditionalBlocks,
}

pub struct WorkflowTaskDependency {
    pub from: WorkflowTaskId,
    pub to: WorkflowTaskId,
    pub kind: DependencyType,
}
```

### 2c. Content-Addressed IDs

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.4 beads hash-based IDs.

Every `WorkflowTaskId` is derived as `SHA-256(canonical_serialization(title + description + acceptance_criteria + source_formula + source_location + created_at))`. Two agents creating the same task on separate branches produce identical IDs — their tasks merge cleanly at sync time without collision. This also solves the open question in SPEC-memory §5.3 about multi-agent workspace merge conflicts.

### 2d. BeeAI-Style Step Sentinels (for in-workflow state machines)

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.16 BeeAI `Workflow[T, K]`.

A workflow step handler can return one of five sentinels instead of requiring an explicit transition table:

```rust
pub enum WorkflowSentinel {
    Start,           // "__start__" — jump to the workflow's start node
    SelfLoop,        // "__self__" — re-run this step
    Prev,            // "__prev__" — go back one step
    Next,            // "__next__" — advance to the declared next step
    End,             // "__end__" — terminate the workflow
    Named(String),   // Jump to a named step
}
```

Sentinels are optional — workflows can declare their own transition graph instead. But for simple state-machine workflows, sentinels collapse the boilerplate significantly.

---

## 3. Trigger Types

### 3.1 Cron

Standard cron expression scheduling. The gateway's scheduler manages cron triggers and fires workflow events at the configured times.

```yaml
workflows:
  dreaming:
    trigger:
      type: "cron"
      schedule: "0 3 * * *"          # Daily at 3 AM
```

### 3.2 Event

Fires when a specific event pattern matches. Patterns can filter on event kind, source, agent, or custom metadata.

```yaml
workflows:
  inbox-triage:
    trigger:
      type: "event"
      pattern:
        kind: "Message"
        source: "Channel"
        metadata:
          priority: "low"
```

### 3.3 Threshold

Fires when a monitored metric exceeds a threshold. The workflow engine periodically checks thresholds.

```yaml
workflows:
  memory-compaction:
    trigger:
      type: "threshold"
      condition:
        metric: "memory_entry_count"
        agent: "sera"
        operator: ">"
        value: 1000
```

### 3.4 Manual

Triggered by a principal via API, CLI, or agent tool call.

---

## 4. Workflow Execution

When a workflow triggers:

1. `on_workflow_trigger` hook chain fires (gating, context injection)
2. A `WorkflowTask` is created and registered in the task lane (§4a)
3. Workers claim tasks via the atomic claim protocol (§4b)
4. The designated agent executes the workflow as a series of turns
5. Workflow turns use the same runtime, context pipeline, tools, hooks, and policies as interactive turns
6. Workflow results are validated via `validate_task_content` (failure-pattern blacklist) before being marked `Closed`
7. Workflow results are logged and auditable

### 4a. `bd ready` — Ready-Task Detection Algorithm

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.4 beads. This is the **default ready-detection algorithm** for SERA's workflow queue.

A `WorkflowTask` is *claimable* when all of:

1. `status == Open` (not `InProgress`, `Hooked`, `Blocked`, `Deferred`, `Closed`, or `Pinned`)
2. No open / in-progress task has a `Blocks` (or `ParentChild` from a non-closed parent) dependency pointing to it
3. `defer_until` is `None` or in the past
4. Any `AwaitType` gate has been satisfied (timer elapsed, GH check green, human approved, etc.)

```rust
pub async fn ready_tasks(
    engine: &WorkflowEngine,
    principal: &PrincipalRef,
) -> Result<Vec<WorkflowTask>, EngineError>;
```

Workers call `ready_tasks()` periodically or on wakeup, pick a task matching their capabilities, and invoke `claim_task()` (§4b). Losers on a race retry `ready_tasks()` with fresh state.

### 4b. Atomic Claim Protocol

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.4 beads `bd update --claim` + §10.3 Paperclip `checkout()`.

The gateway exposes a single-transaction `claim_task(task_id, agent_id)` operation that sets `assignee + status=Hooked` atomically. If the task is no longer in `Open` state, the claim fails with `ClaimError::StatusMismatch`. This is the core multi-agent safety model and is shared with the Circle task channel (SPEC-circles §3b).

```rust
pub async fn claim_task(
    engine: &WorkflowEngine,
    task_id: WorkflowTaskId,
    agent_id: PrincipalRef,
) -> Result<ClaimToken, ClaimError>;
```

**Rules:**

- Claims are atomic under the transaction boundary of the underlying store (`sqlx` for Tier-1/2, Dolt SQL for Tier-3 multi-agent workspaces)
- Stale claims are reaped by a background `StaleClaimReaper` whose reference implementation is openclaw's `StaleLockReaper` pattern (process-liveness check + release on orphan detection)
- Claims carry an idempotency key so a worker crash + retry does not double-execute tool side-effects

### 4c. Termination Triad

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.15 MetaGPT `Team.run()`.

A workflow run terminates on **any of three orthogonal conditions**:

1. **`n_round` countdown** — a maximum number of execution rounds, configured per workflow
2. **`is_idle` convergence** — all workers have empty inboxes and no `todo` tasks for a configurable grace period
3. **Cost budget exhaustion** — the workflow's accumulated cost exceeds `CostBudget` and raises `NoMoneyException`

The workflow engine emits `WorkflowTermination { reason: NRoundExceeded | Idle | BudgetExhausted | ExplicitStop }` as a first-class event for observability.

### 4d. Three-Layer Failure Model

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.3 Paperclip. Mirrors SPEC-circles §5b.

| Layer | Mechanism |
|---|---|
| **Orphan reaping** | `StaleClaimReaper` scans `Hooked` tasks with dead assignees; releases claim; emits `LaneFailureClass::OrphanReaped` |
| **Process-loss retry** | Crashed workers' tasks retry up to `max_task_retries` (default 3) with idempotency-key preservation |
| **Budget cancellation** | `NoMoneyException` scoped-cancellation kills all tasks in the exhausted budget scope |
| **Output enforcement** | Missing expected outputs queue a follow-up wake with `RevisionRequested` feedback |

### 4e. `meta_scope` Field — Self-Evolution Task Routing

> **Source:** [SPEC-self-evolution](SPEC-self-evolution.md) §5.8.

When a `WorkflowTask` carries `meta_scope: Some(BlastRadius::*)`, the workflow engine routes it through the **self-evolution pipeline** instead of the normal execution path. Routing differences:

- `claim_task()` is gated through the `MetaChange` capability (§SPEC-auth)
- The `constitutional_gate` hook fires before any other processing
- Approval matrix (SPEC-self-evolution §9.1) determines review window and required approvers
- A `ShadowSession` replay is required before the task is marked `Closed`
- Completion emits a Change Artifact `Applied` event, not a normal `TaskClosed`

Tasks without `meta_scope` are normal agent work and flow through the standard path. The engine itself stays unified — self-evolution is just a scoped code path, not a separate engine.

### Workflow Session Isolation

Workflow runs execute in **dedicated workflow sessions** with the session key `workflow:{agent_id}:{workflow_name}`. This provides:

1. **Isolation:** Workflow turns do not pollute interactive session transcripts
2. **Concurrency safety:** The single-writer invariant is per-session, so a workflow session does not block interactive sessions
3. **Auditability:** Workflow runs have their own audit trail and transcript
4. **Queue mode independence:** Workflow sessions can use different queue modes than interactive sessions

Workflow sessions are **hidden from the normal session list** by default — they do not appear in client session listings unless the client explicitly requests workflow sessions.

```yaml
sera:
  workflows:
    session_prefix: "workflow"         # Session key: workflow:{agent_id}:{name}
    session_visible: false             # Hidden from interactive session list
    session_retention: "last_run"      # Keep only the last workflow run's transcript
```

---

## 5. Built-in Workflow: Dreaming

Dreaming is SERA's built-in memory consolidation workflow, inspired by [OpenClaw's dreaming system](https://dev.to/czmilo/openclaw-dreaming-guide-2026-background-memory-consolidation-for-ai-agents-585e). It consolidates short-term memory signals into durable long-term knowledge via a three-phase background sweep.

### 5.1 Phases

```
Phase 1: Light Sleep
  → Ingest daily notes + session transcripts
  → Deduplicate, stage candidates
  → Record signal hits

Phase 2: REM Sleep
  → Extract recurring themes
  → Identify candidate truths
  → Record reinforcement signals

Phase 3: Deep Sleep
  → Score candidates (6 weighted signals)
  → Apply threshold gates
  → Promote survivors to long-term memory (MEMORY.md)
  → Generate Dream Diary (human-readable narrative)
```

### 5.2 Scoring Signals

| Signal | Weight | Meaning |
|---|---|---|
| Relevance | 0.30 | How relevant to the agent's domain |
| Frequency | 0.24 | How often recalled |
| Query diversity | 0.15 | Recalled by diverse queries (not just one) |
| Recency | 0.15 | Recency-weighted (half-life decay) |
| Consolidation | 0.10 | Already part of consolidated knowledge |
| Conceptual richness | 0.06 | Connects multiple concepts |

### 5.3 Promotion Gates

All must pass for a memory candidate to be promoted:
- `minScore ≥ 0.8`
- `minRecallCount ≥ 3`
- `minUniqueQueries ≥ 3`

### 5.4 Configuration

```yaml
agents:
  - name: "sera"
    workflows:
      dreaming:
        enabled: true                 # Opt-in, disabled by default
        frequency: "0 3 * * *"        # Cron schedule
        phases:
          light:
            lookback_days: 2
            limit: 100
          rem:
            lookback_days: 7
            min_pattern_strength: 0.75
          deep:
            min_score: 0.8
            min_recall_count: 3
            min_unique_queries: 3
            max_age_days: 30
            limit: 10
```

---

## 6. Other Example Workflows

The workflow engine is not limited to dreaming. Other built-in or custom workflows include:

| Workflow | Description |
|---|---|
| **Knowledge audit** | Agent reviews its own memory for staleness, contradictions, or gaps |
| **Inbox triage** | Agent processes accumulated low-priority messages in batch |
| **Workspace cleanup** | Agent organizes its files, removes stale artifacts |
| **Health check** | Agent runs self-diagnostics on its tools, memory, and connections |
| **Report generation** | Agent synthesizes periodic reports from accumulated data |
| **Task graph management** | Agent manages structured task decompositions via a Beads-style DAG (see §6.1) |

These are all configured the same way — as `WorkflowDef` entries in the agent's config.

### 6.1 Beads Task Graph Integration — PROMOTED TO PHASE 1 DESIGN INPUT

> [!IMPORTANT]
> **This section was previously deferred to Phase 3+. It is now Phase-1 design input.** Beads is not a simple task list — it is a full DAG execution substrate with atomic claim, dependency-aware ready detection, content-hash IDs, and first-class LLM agent integration. SERA's workflow engine is designed **around** the beads data model, not as an afterthought integration. See [SPEC-dependencies](SPEC-dependencies.md) §10.4 for the full rationale.

The `gastownhall/beads` deterministic task DAG pattern integrates with SERA's workflow engine at **three layers**:

1. **Data model (Phase 0):** `WorkflowTask` is modeled directly on beads' `Issue` struct — see §2a. Hash-based IDs, `Hooked` status, `ConditionalBlocks` dependency type, `AwaitType` gates, `DeferUntil`, `Wisp`/ephemeral lifecycle all come from beads.

2. **Algorithm (Phase 1):** The `bd ready` ready-detection algorithm (§4a) and the atomic `--claim` protocol (§4b) are the default task-lane semantics in SERA's workflow engine. Not optional.

3. **Runtime tool integration (Phase 1):** The `beads` CLI binary is embeddable as a SERA tool. Agents call `bd_create`, `bd_ready`, `bd_update --claim`, `bd_close`, `bd_dep_add`, and `bd_prime` as normal tool invocations against a Dolt-backed task database. The [`beads-mcp`](https://pypi.org/project/beads-mcp/) Python package wraps the same commands as MCP tools for MCP-only environments.

#### Dolt-backed storage for multi-writer scenarios

Beads uses [Dolt](https://www.dolthub.com/) (git-for-data) for storage. Two modes:

- **Embedded (Tier-1/2):** Dolt runs in-process; data lives in `.beads/embeddeddolt/`. Single-writer, file-locked.
- **Server mode (Tier-3):** External `dolt sql-server` supports concurrent writers. Cell-level 3-way merge via Dolt's built-in conflict resolution. Content-hash IDs ensure that two agents creating the same task on separate branches merge without collision.

For SERA's multi-agent workspaces (Circles with independent worker branches), Dolt + content-hash IDs together solve the cross-agent merge conflict problem that `sera-memory` §5.3 flagged as an open question.

#### `bd prime` context injection

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.4.

The `bd prime` command injects a dedicated `PRIME.md` document into an agent's session start. SERA wires this into the `on_workflow_trigger` hook: when a workflow fires, the hook calls `bd prime` for the workflow's task scope and passes the result as additional system prompt context. This gives the workflow agent a scoped view of the relevant task graph without injecting the full database.

#### `wasteland` federation — cross-organization coordination

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.4.

`gastownhall/wasteland` is a federation protocol on top of beads + DoltHub: each participating organization forks a shared "commons" Dolt database, claims work via `wl claim`, and submits evidence-linked completions. This pattern is relevant to cross-organizational Circle coordination (SPEC-circles §8 Q11) but is **out of scope for SERA 1.0** — documented here for future reference.

---

## 7. Hook Points

| Hook Point | Fires When |
|---|---|
| `on_workflow_trigger` | When a scheduled/triggered workflow fires |

Additionally, all standard turn hooks (`pre_turn`, `post_turn`, `pre_tool`, `post_tool`, etc.) fire during workflow turns, since workflows execute as normal agent turns.

---

## 8. Configuration

```yaml
agents:
  - name: "sera"
    workflows:
      dreaming:
        enabled: false                # Opt-in
        trigger:
          type: "cron"
          schedule: "0 3 * * *"
        config:
          phases:
            light:
              lookback_days: 2
              limit: 100
            # ...

      knowledge-audit:
        enabled: false
        trigger:
          type: "cron"
          schedule: "0 6 * * 0"       # Weekly Sunday 6 AM
        config:
          max_staleness_days: 30

      inbox-triage:
        enabled: false
        trigger:
          type: "threshold"
          condition:
            metric: "unprocessed_messages"
            operator: ">"
            value: 50
```

---

## 9. Cross-References

| Dependency | Spec | Relationship |
|---|---|---|
| `sera-gateway` | [SPEC-gateway](SPEC-gateway.md) | Gateway scheduler manages cron triggers; workflows create events; `PAUSED` state + `WorkforceSnapshot` |
| `sera-runtime` | [SPEC-runtime](SPEC-runtime.md) | Workflow turns execute via the runtime |
| `sera-memory` | [SPEC-memory](SPEC-memory.md) | Dreaming reads/writes memory; content-hash IDs solve multi-writer workspace merge conflicts |
| `sera-hooks` | [SPEC-hooks](SPEC-hooks.md) | `on_workflow_trigger` hook + standard turn hooks; `constitutional_gate` fires on `meta_scope` tasks |
| `sera-session` | [SPEC-gateway](SPEC-gateway.md) | Workflow session scoping |
| `sera-circles` | [SPEC-circles](SPEC-circles.md) | Atomic claim protocol is shared with Circle task channel; WorkflowTask ↔ Circle Packet mapping |
| `sera-meta` | [SPEC-self-evolution](SPEC-self-evolution.md) | `meta_scope` field routes self-evolution Change Artifacts through the workflow engine |
| Dependencies | [SPEC-dependencies](SPEC-dependencies.md) | §10.3 Paperclip 4-trigger wakeup + 3-layer failure; §10.4 **beads data model + `bd ready` + atomic claim + content-hash IDs + `bd prime` + Dolt multi-writer** (promoted Phase-1); §10.15 MetaGPT termination triad; §10.16 BeeAI step sentinels; §10.17 CAMEL pause/resume |

---

## 10. Open Questions

1. ~~**Workflow session isolation**~~ — Resolved: Dedicated `workflow:{agent_id}:{name}` sessions (see §4)
2. **Workflow auditability** — Are workflow turns auditable the same way as interactive turns? Same telemetry? Same transcript? **Tentative answer:** yes, identical pipeline. Workflow turns emit the same OCSF events (SPEC-observability) and flow through the same audit log append-only path.
3. ~~**Concurrent workflow execution**~~ — Resolved: atomic claim protocol (§4b) + beads `Hooked` status + `StaleClaimReaper`. Maps to Circle `ConcurrencyPolicy` (SPEC-circles §3.1).
4. **Workflow state persistence** — Do workflows maintain state across runs (e.g., "last processed up to timestamp X")? **Tentative answer:** yes, in the workflow's private state store via `sera-db`. BeeAI step sentinels (§2d) model this as shared typed state per workflow instance.
5. ~~**Workflow failure handling**~~ — Resolved: three-layer failure model (§4d) — orphan reaping + process-loss retry + budget cancellation + output enforcement.
6. **Dreaming scoring model** — Are the weights and thresholds fixed, or can they be tuned per-agent?
7. **Beads CLI vs embedded library** — Should SERA embed beads as a Rust library dependency (if one is available) or shell out to the `bd` CLI binary? **Tentative answer:** shell out to the CLI for MVS simplicity; evaluate library embedding as a Phase-2 optimization if dispatch latency becomes measurable.
8. **WorkflowTask ↔ Circle Packet mapping** — SPEC-circles §3a defines a `Packet` type that is structurally similar to `WorkflowTask`. Should they be the same type? **Tentative answer:** `Packet` wraps a `WorkflowTask` — the wrapper adds Circle-specific fields (`claim_token`, `publisher_id`) without re-inventing the task type.
9. **Meta-scope routing conflicts** — If a workflow task has `meta_scope` but is also scheduled via cron, who wins — the cron trigger or the constitutional gate? **Tentative answer:** constitutional gate wins; it runs before any trigger-based dispatch.
10. **Phase-1 beads promotion completeness** — Which specific beads features are Phase 1 vs Phase 2? Confirming: data model (§2a) + `bd ready` (§4a) + atomic claim (§4b) are Phase 1; `wasteland` federation + Dolt server mode are Phase 3+.

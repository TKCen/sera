# SPEC: Workflow Engine (`sera-workflow`)

> **Status:** DRAFT  
> **Source:** PRD §6.3 (triggered workflows, dreaming)  
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
}

pub enum WorkflowTrigger {
    Cron(CronSchedule),                   // "0 3 * * *" — daily at 3 AM
    Event(EventPattern),                  // On specific event types
    Threshold(ThresholdCondition),        // When memory size exceeds X, session count > N, etc.
    Manual,                               // Triggered by principal via API/CLI
}
```

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
2. A workflow event is created and enqueued via the gateway
3. The designated agent executes the workflow as a series of turns
4. Workflow turns use the same runtime, context pipeline, tools, hooks, and policies as interactive turns
5. Workflow results are logged and auditable

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

### 6.1 Beads Task Graph Integration

The [Beads](https://github.com/gastownhall/beads) deterministic task DAG pattern can integrate with the workflow engine as a **tool + workflow combination**:

- **As a tool:** A `task_graph` tool allows agents to create, query, and update content-addressed task DAGs during interactive turns. Each node in the graph represents a sub-task with tracked state (pending, active, completed, failed).
- **As a workflow:** A scheduled workflow can check task graph progress, identify blocked tasks, and trigger follow-up actions.

Beads is a **task decomposition and provenance** concern, not a memory storage concern. Task graphs are stored in the agent workspace alongside memory files but serve a distinct purpose: tracking structured multi-step work with deterministic completion evidence.

> [!NOTE]
> Beads integration is a Phase 3+ enhancement. The workflow engine and tool system are designed to accommodate it without core changes.

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
| `sera-gateway` | [SPEC-gateway](SPEC-gateway.md) | Gateway scheduler manages cron triggers; workflows create events |
| `sera-runtime` | [SPEC-runtime](SPEC-runtime.md) | Workflow turns execute via the runtime |
| `sera-memory` | [SPEC-memory](SPEC-memory.md) | Dreaming reads/writes memory |
| `sera-hooks` | [SPEC-hooks](SPEC-hooks.md) | `on_workflow_trigger` hook + standard turn hooks |
| `sera-session` | [SPEC-gateway](SPEC-gateway.md) | Workflow session scoping |

---

## 10. Open Questions

1. ~~**Workflow session isolation**~~ — Resolved: Dedicated `workflow:{agent_id}:{name}` sessions (see §4)
2. **Workflow auditability** — Are workflow turns auditable the same way as interactive turns? Same telemetry? Same transcript?
3. **Concurrent workflow execution** — Can the same workflow run concurrently? What happens if a cron trigger fires while the previous run is still in progress?
4. **Workflow state persistence** — Do workflows maintain state across runs (e.g., "last processed up to timestamp X")? Where is this state stored?
5. **Workflow failure handling** — What happens when a workflow turn fails? Retry? Alert? Skip?
6. **Dreaming scoring model** — Are the weights and thresholds fixed, or can they be tuned per-agent?

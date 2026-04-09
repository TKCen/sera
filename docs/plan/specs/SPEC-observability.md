# SPEC: Observability (`sera-telemetry`)

> **Status:** DRAFT  
> **Source:** PRD §4.1 (diagnostics), §14 (invariant 10)  
> **Crate:** `sera-telemetry`  
> **Priority:** Phase 0  

---

## 1. Overview

Observability in SERA provides **evidence-based insight** into system behavior. Every significant action produces structured traces, metrics, and logs. The observability stack is built on **OpenTelemetry** for vendor-neutral, standards-based instrumentation.

The core principle: **evidence survives the run.** Every turn, tool call, hook execution, and system decision is recorded in a way that can be audited after the fact.

---

## 2. Three Pillars

### 2.1 Structured Tracing

Distributed traces cover the full lifecycle of events through the system:

```
Event ingress → Gateway routing → Queue → Runtime turn
  → Context assembly → Model call → Tool calls → Memory writes
  → Response delivery
```

Each span carries:
- Principal identity (who initiated)
- Agent identity (who executed)
- Session key
- Event ID
- Duration and status

#### Trace Propagation

Traces propagate across:
- Hook chain invocations (each hook is a child span)
- Tool executions (including gRPC external tools)
- Memory operations
- Approval flows (including HITL wait time)
- Workflow triggers and execution

### 2.2 Metrics

Key metrics exported via OpenTelemetry:

| Metric | Type | Description |
|---|---|---|
| `sera.gateway.events.total` | Counter | Total events processed |
| `sera.gateway.routing.latency` | Histogram | Event routing latency |
| `sera.queue.depth` | Gauge | Per-lane and global queue depth |
| `sera.queue.active` | Gauge | Currently active sessions |
| `sera.runtime.turns.total` | Counter | Total turns executed |
| `sera.runtime.turns.latency` | Histogram | Turn execution latency |
| `sera.tools.calls.total` | Counter | Total tool invocations (by tool, risk level) |
| `sera.hooks.invocations.total` | Counter | Hook invocations (by hook point, hook name) |
| `sera.hooks.latency` | Histogram | Per-hook execution time |
| `sera.memory.writes.total` | Counter | Memory write operations |
| `sera.hitl.approvals.total` | Counter | Approval requests (by outcome) |
| `sera.hitl.latency` | Histogram | Approval roundtrip time |
| `sera.model.calls.total` | Counter | Model API calls |
| `sera.model.tokens.total` | Counter | Token usage (prompt + completion) |

### 2.3 Structured Logging

All log output is structured (JSON) with consistent fields:
- Timestamp
- Level
- Component (crate name)
- Principal
- Session key
- Event ID
- Message

```rust
tracing::info!(
    principal = %ctx.principal,
    session = %ctx.session_key,
    event = %ctx.event_id,
    "Turn completed in {:?}",
    duration
);
```

---

## 3. Audit Log

The audit log is a **separate, append-only, tamper-evident** record of all security-relevant actions:

| Event Type | What's Recorded |
|---|---|
| Authentication | Principal login, token refresh, failed auth |
| Authorization | AuthZ check results (allow, deny, needs_approval) |
| Tool execution | Tool name, arguments (sanitized), result status, principal, risk level |
| Config changes | What changed, who proposed, who approved |
| Secret access | Secret path accessed (never the value), by whom |
| Approval decisions | Approval request, decision (approved/rejected/escalated), by whom |
| Session lifecycle | Created, transitioned, archived, destroyed |
| Memory writes | What was written, to which tier, by whom |

### Audit Storage

Audit logs are stored in the database (`sera-db`) with:
- Immutable rows (append-only)
- Timestamps
- Principal identity
- Action details

---

## 3.1 Run Evidence / Proof Bundle

> **Enhancement: OpenClaw Part 6, Agent Stack Part 1**

A production run should leave behind more than isolated traces. It should leave a **proof bundle** — the triggering intent, the capability surface exposed, any approval events, and the durable record of side effects. This enables operators to fully explain any run after the fact.

```rust
pub struct RunEvidence {
    pub run_id: RunId,
    pub event: EventSummary,               // What triggered the run
    pub session_key: SessionKey,           // Isolation boundary
    pub agent: AgentRef,                   // Which agent executed
    pub principal: PrincipalRef,           // Acting principal
    pub tools_exposed: Vec<ToolRef>,       // Capability surface snapshot
    pub tools_called: Vec<ToolCallRecord>, // What was actually called
    pub approvals: Vec<ApprovalRecord>,    // HITL decisions during the run
    pub memory_writes: Vec<MemoryWriteRecord>, // Durable state changes
    pub model_calls: Vec<ModelCallRecord>, // LLM invocations (model, tokens, latency)
    pub cost: CostRecord,                 // Token + API costs
    pub duration: Duration,
    pub outcome: RunOutcome,               // Success, failure, timeout, budget_exceeded
}

pub struct CostRecord {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub estimated_cost_usd: Option<f64>,   // Based on model pricing config
}

pub enum RunOutcome {
    Success,
    Failure(String),
    Timeout,
    BudgetExceeded,
    Interrupted,
    ApprovalRejected,
}
```

Run evidence is:
- Generated automatically by the runtime at turn completion
- Stored in `sera-db` alongside traces
- Queryable via the observability API
- Exportable for offline analysis and eval frameworks

### Cost Attribution

Token usage metrics are attributed at multiple levels:
- **Per-agent:** Total tokens consumed by each agent
- **Per-session:** Cost per conversation session
- **Per-principal:** Cost attributed to the initiating principal
- **Per-model:** Breakdown by model provider for multi-model routing

---

## 3.2 Evaluation Framework Hooks

> **Enhancement: OpenClaw Part 6 — Can-Defer (Phase 3+)**

Observability tells you what happened. Evaluation tells you whether it was good enough. SERA does not build an eval framework — it provides hooks for external eval systems.

**Two feedback loops:**

1. **Offline eval:** Export run evidence → external eval framework runs regression tests → results feed back into config/skill refinement
2. **Online eval:** Sample live runs → review → failures become offline regression tests

**Extension points:**
- `post_turn` hook can apply custom eval criteria (e.g., response quality scoring)
- Run evidence export API enables external tool integration
- Workflow engine can schedule periodic eval runs against historical evidence

> [!NOTE]
> The run evidence structure (§3.1) is the primary data contract for eval frameworks. As long as evidence is complete and exportable, eval tooling can be fully external.

---

## 4. Health & Diagnostics

The gateway exposes health and diagnostics endpoints:

| Endpoint | Purpose |
|---|---|
| `/health` | Liveness check (is the process running?) |
| `/ready` | Readiness check (is the system fully initialized?) |
| `/status` | System status — active sessions, queue depth, connected connectors |
| `/diagnostics` | Detailed diagnostics — runtime info, memory stats, hook chain status |
| `/metrics` | Prometheus-compatible metrics endpoint |

---

## 5. OpenTelemetry Configuration

```yaml
sera:
  telemetry:
    # Tracing
    tracing:
      enabled: true
      exporter: "otlp"                # otlp | jaeger | zipkin | stdout
      endpoint: "http://localhost:4317"
      sampling_rate: 1.0               # 1.0 = trace everything

    # Metrics
    metrics:
      enabled: true
      exporter: "prometheus"           # prometheus | otlp | stdout
      prometheus_port: 9090

    # Logging
    logging:
      level: "info"                    # trace | debug | info | warn | error
      format: "json"                   # json | pretty (for local dev)
      output: "stdout"                 # stdout | file

    # Audit
    audit:
      enabled: true
      retention_days: 90               # How long to keep audit records
```

---

## 6. Invariants

| # | Invariant | Enforcement |
|---|---|---|
| 10 | Evidence survives the run | `sera-telemetry` + `sera-session` — all significant actions produce traces and audit records |

---

## 7. Cross-References

| Dependency | Spec | Relationship |
|---|---|---|
| `sera-gateway` | [SPEC-gateway](SPEC-gateway.md) | Health endpoints, event tracing |
| `sera-runtime` | [SPEC-runtime](SPEC-runtime.md) | Turn tracing |
| `sera-tools` | [SPEC-tools](SPEC-tools.md) | Tool call audit |
| `sera-hooks` | [SPEC-hooks](SPEC-hooks.md) | Hook execution tracing |
| `sera-auth` | [SPEC-identity-authz](SPEC-identity-authz.md) | AuthN/AuthZ audit |
| `sera-hitl` | [SPEC-hitl-approval](SPEC-hitl-approval.md) | Approval audit |
| `sera-secrets` | [SPEC-secrets](SPEC-secrets.md) | Secret access audit |
| `sera-db` | [SPEC-crate-decomposition](SPEC-crate-decomposition.md) | Audit log storage |

---

## 8. Open Questions

1. **Audit log integrity** — Is cryptographic tamper-evidence required (hash chaining)? Or is append-only database storage sufficient?
2. **Trace sampling** — In high-throughput enterprise deployments, what sampling strategy? Head-based? Tail-based?
3. **Log aggregation** — Does SERA provide log aggregation, or does it rely on external tools (ELK, Loki)?
4. **Alert rules** — Are alert rules configurable within SERA, or externalized to prometheus/alertmanager?
5. ~~**Cost attribution**~~ — Resolved: See §3.1. Token usage attributed per-agent, per-session, per-principal, per-model.
6. **Evidence retention policy** — How long are run evidence bundles retained? Size-based or time-based?
7. **Eval framework recommendations** — Should SERA recommend or bundle a specific eval framework, or remain agnostic?

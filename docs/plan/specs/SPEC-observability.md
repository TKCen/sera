# SPEC: Observability (`sera-telemetry`)

> **Status:** DRAFT
> **Source:** PRD §4.1 (diagnostics), §14 (invariant 10), plus deltas from [SPEC-dependencies](SPEC-dependencies.md) §8.4 (locked OTel triad: `opentelemetry` 0.27 + `opentelemetry-otlp` 0.27 + `tracing-opentelemetry` 0.28 — must be pinned together), §10.1 (claw-code `LaneFailureClass` typed failure taxonomy + `LaneCommitProvenance`), §10.2 (Codex W3C trace context non-optional on every Submission), §10.16 (BeeAI hierarchical `Emitter` namespace tree + `EventTrace` correlation with per-entity child emitters), §10.18 (**NVIDIA OpenShell OCSF v1.7.0 structured audit events** with canonical `class_uid` taxonomy), [SPEC-self-evolution](SPEC-self-evolution.md) §5.7 (separate audit log write path, cryptographically chained, unreachable from Change Artifacts)
> **Crate:** `sera-telemetry`
> **Priority:** Phase 0

---

## 1. Overview

Observability in SERA provides **evidence-based insight** into system behavior. Every significant action produces structured traces, metrics, and logs. The observability stack is built on **OpenTelemetry** for vendor-neutral, standards-based instrumentation.

The core principle: **evidence survives the run.** Every turn, tool call, hook execution, and system decision is recorded in a way that can be audited after the fact.

---

## 2. Three Pillars

### 2.1 Structured Tracing

> **Implementation:** [SPEC-dependencies](SPEC-dependencies.md) §8.4 — the OpenTelemetry triad MUST be pinned exactly together: `opentelemetry = "=0.27"`, `opentelemetry-otlp = "=0.27"`, `tracing-opentelemetry = "=0.28"`. Version drift produces compile-time trait bound errors. Pin in workspace-level `Cargo.toml`.

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
- **W3C trace context** (non-optional — every gateway Submission carries `trace: W3cTraceContext`; see SPEC-gateway §3.1)

#### Trace Propagation

Traces propagate across:
- Hook chain invocations (each hook is a child span)
- Tool executions (including gRPC external tools)
- Memory operations
- Approval flows (including HITL wait time)
- Workflow triggers and execution

### 2.1a Hierarchical `Emitter` Namespace Tree

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.16 BeeAI Framework.

Beyond OpenTelemetry spans, SERA maintains a **hierarchical `Emitter` namespace tree** — every runtime entity (agent, tool, workflow, circle, session) owns a child emitter forked from a root singleton. Listeners match by string name, `re.Pattern`, or predicate. Pattern-matched subscriptions work without regex on the hot path via namespace path prefix comparison.

```rust
pub struct Emitter {
    pub namespace: Vec<String>,        // Hierarchical path, e.g. ["gateway", "session", "tool"]
    pub context: HashMap<String, serde_json::Value>, // Propagated to all child emitters
    pub trace: Option<EventTrace>,     // Span/trace correlation
    parent: Option<Weak<Emitter>>,
}

pub struct EventMeta {
    pub id: EventId,
    pub name: String,
    pub path: Vec<String>,             // Hierarchical namespace path
    pub created_at: DateTime<Utc>,
    pub source: String,                // The emitter that produced this event
    pub creator: String,                // The object that owns the emitter
    pub context: HashMap<String, serde_json::Value>,
    pub group_id: Option<String>,      // For batch correlation
    pub trace: Option<EventTrace>,
    pub data_type: String,             // serde type tag
}

impl Emitter {
    pub fn child(&self, namespace: &str, creator: &str) -> Arc<Emitter> { /* ... */ }
    pub fn root() -> Arc<Emitter> { /* singleton */ }
}
```

**Why both OTel AND Emitter?** They serve different use cases:

- **OpenTelemetry** is for distributed-systems tracing — request-level causality across services, sampling, exporter fan-out
- **Emitter** is for programmatic in-process event subscription — plugins, hooks, internal subsystems wanting to react to events without registering an OTel span processor

A plugin subscribing to `["gateway", "session", "tool", "*"]` via the Emitter tree gets hot-path-friendly callbacks; the same events also generate OTel spans for distributed trace export. Both layers are populated from the same source, but consumers pick whichever one fits their use case.

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

The audit log is a **separate, append-only, cryptographically chained** record of all security-relevant actions. SERA adopts the **OCSF v1.7.0** (Open Cybersecurity Schema Framework) taxonomy from [SPEC-dependencies](SPEC-dependencies.md) §10.18 (NVIDIA OpenShell) to make the audit log SIEM-compatible without custom schema work.

### 3.0 OCSF v1.7.0 Event Classes

| class_uid | Class | Use in SERA |
|---|---|---|
| `1007` | Process Activity | Tool execution, sandbox process lifecycle |
| `2004` | Detection Finding | Policy violations, denied actions, doom-loop detection |
| `3002` | Authentication | Principal login, token refresh, failed auth |
| `3005` | Authorization | AuthZ check results (allow, deny, needs_approval) |
| `4001` | Network Activity | Outbound egress (CONNECT allowed/denied) |
| `4002` | HTTP Activity | L7 method+path decisions on the egress proxy |
| `4007` | SSH Activity | Interactive shell session events (if enabled) |
| `5019` | Device Config State Change | Config hot-reload, Change Artifact promotion, policy updates |
| `6002` | Application Lifecycle | Gateway boot, shutdown, crash, two-generation transition |
| `6003` | Application Activity | Tool invocation lifecycle (start/end), approval decisions |

Every OCSF event carries canonical fields: `actor.process.name` (binary path + PID), `dst_endpoint`, `firewall_rule.name` (for network events), `action`/`disposition` (Allowed/Denied), `raw_data` (for forensic replay), and a custom `sera.change_artifact_id` extension field when the event is part of a self-evolution flow.

### 3.1 Event Type → OCSF Mapping

| Event Type | OCSF class_uid |
|---|---|
| Authentication | 3002 |
| Authorization | 3005 |
| Tool execution | 1007 + 6003 |
| Config changes | 5019 |
| Secret access | 3005 + custom `sera.secret_access` extension |
| Approval decisions | 6003 |
| Session lifecycle | 6002 |
| Memory writes | 6003 + custom `sera.memory_write` extension |
| Policy violation / doom-loop | 2004 |
| Outbound network | 4001 + 4002 |
| Self-evolution (Change Artifact) | 5019 + custom `sera.meta_change` extension |

### 3.2 Audit Storage — Separate Write Path

> **Source:** [SPEC-self-evolution](SPEC-self-evolution.md) §5.7 — critical architectural invariant.

The audit log is on a **separate write path** from the normal event pipeline. It is cryptographically chained (beads-style content-hash chain per SPEC-dependencies §10.4), append-only, and **unreachable from any Change Artifact code path**:

```rust
pub struct AuditLog {
    /// The audit log storage backend is bound at gateway boot and cannot be rebound at runtime.
    /// No Change Artifact can modify this binding (enforced by CON-02 + CON-03 + §5.7).
    backend: &'static dyn AuditBackend,
}

pub struct AuditEntry {
    pub id: AuditId,
    pub ocsf_class_uid: u32,
    pub payload: serde_json::Value,      // OCSF v1.7.0 schema
    pub timestamp: DateTime<Utc>,
    pub prev_hash: [u8; 32],             // Cryptographic chain link
    pub this_hash: [u8; 32],             // Hash of (payload + prev_hash)
    pub signature: Option<AuditSignature>, // Optional: HSM-signed for enterprise
}

#[async_trait]
pub trait AuditBackend: Send + Sync {
    /// Append-only write. There is no update or delete API at any language level.
    async fn append(&self, entry: AuditEntry) -> Result<(), AuditError>;

    /// Verify the chain is intact from entry_id back to the last known-good boot.
    async fn verify_chain(&self, from: AuditId) -> Result<VerifyResult, AuditError>;
}
```

**Invariants enforced in Rust:**

1. **No delete/update API at any layer.** The `AuditBackend` trait has only `append` and `verify_chain`. A Change Artifact cannot compile code that mutates audit entries.
2. **Static binding.** The backend is `&'static dyn AuditBackend`, bound at gateway boot via an `OnceCell`. Runtime rebinding fails at the type level.
3. **Chain verification at boot.** If the chain is broken, the gateway refuses to start. Tampering produces a hard failure, not silent drift.
4. **Separate credentials.** The audit log's underlying storage (file, S3, blockchain, etc.) uses credentials never exposed to the normal event pipeline. A compromised event pipeline cannot write forged audit entries.

SIEM compatibility is out-of-the-box via OCSF — SERA audit logs can be ingested directly by Splunk OCSF Add-on, Amazon Security Lake, and Elastic Filebeat without schema mapping.

### 3.3 `LaneFailureClass` Typed Failure Taxonomy

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.1 claw-code.

Failure events use a typed enum rather than error strings to enable routing and aggregation without parsing log text:

```rust
pub enum LaneFailureClass {
    PromptDelivery,
    TrustGate,
    BranchDivergence,
    Compile,
    Test,
    PluginStartup,
    McpStartup,
    McpHandshake,
    GatewayRouting,
    ToolRuntime,
    WorkspaceMismatch,
    Infra,
    OrphanReaped,
    ConstitutionalViolation, // Fired when the constitutional_gate rejects a change
    KillSwitchActivated,      // Fired when the kill switch is triggered
}
```

The coordinator agent (or human operator) can decide whether to retry, escalate, or reroute a failure based on its class without parsing log text. Every OCSF `2004 Detection Finding` event carries a `sera.lane_failure_class` extension.

### 3.4 `LaneCommitProvenance` for Subagent Result Reporting

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.1 claw-code.

When a subagent finishes work, it emits a `LaneCommitProvenance` struct describing what it actually did at the filesystem/git level:

```rust
pub struct LaneCommitProvenance {
    pub commit: Option<GitSha>,
    pub branch: Option<String>,
    pub worktree: Option<PathBuf>,
    pub canonical_commit: Option<GitSha>,
    pub superseded_by: Option<GitSha>,
    pub lineage: Vec<GitSha>,
}
```

This lets the parent session verify what actually happened rather than trusting a free-text summary. Matches opencode's two-layer persistence pattern (SPEC-gateway §6.1b) and becomes part of the run evidence bundle (§3.1).

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
| `sera-gateway` | [SPEC-gateway](SPEC-gateway.md) | Health endpoints, event tracing; W3C trace non-optional on every Submission |
| `sera-runtime` | [SPEC-runtime](SPEC-runtime.md) | Turn tracing; `TurnOutcome` events |
| `sera-tools` | [SPEC-tools](SPEC-tools.md) | Tool call audit; OCSF 1007 + 6003 + per-sandbox denial events (class 4001/4002) |
| `sera-hooks` | [SPEC-hooks](SPEC-hooks.md) | Hook execution tracing; constitutional_gate emits 2004 + `sera.meta_change` |
| `sera-auth` | [SPEC-identity-authz](SPEC-identity-authz.md) | AuthN (OCSF 3002) + AuthZ (OCSF 3005) audit |
| `sera-hitl` | [SPEC-hitl-approval](SPEC-hitl-approval.md) | Approval audit (OCSF 6003) |
| `sera-secrets` | [SPEC-secrets](SPEC-secrets.md) | Secret access audit (OCSF 3005 + `sera.secret_access` extension) |
| `sera-db` | [SPEC-crate-decomposition](SPEC-crate-decomposition.md) | Audit log storage — via a **separate write path** not reachable from Change Artifacts |
| `sera-meta` | [SPEC-self-evolution](SPEC-self-evolution.md) | Audit log's append-only guarantee + cryptographic chain; `sera.meta_change` OCSF extension; constitutional gate violations route here |
| Dependencies | [SPEC-dependencies](SPEC-dependencies.md) | §8.4 OTel triad pinning requirement; §10.1 claw-code `LaneFailureClass` + `LaneCommitProvenance`; §10.2 Codex W3C trace context; §10.16 BeeAI `Emitter` namespace tree; **§10.18 NVIDIA OpenShell OCSF v1.7.0 adoption** |

---

## 8. Open Questions

1. **Audit log integrity** — Is cryptographic tamper-evidence required (hash chaining)? Or is append-only database storage sufficient?
2. **Trace sampling** — In high-throughput enterprise deployments, what sampling strategy? Head-based? Tail-based?
3. **Log aggregation** — Does SERA provide log aggregation, or does it rely on external tools (ELK, Loki)?
4. **Alert rules** — Are alert rules configurable within SERA, or externalized to prometheus/alertmanager?
5. ~~**Cost attribution**~~ — Resolved: See §3.1. Token usage attributed per-agent, per-session, per-principal, per-model.
6. **Evidence retention policy** — How long are run evidence bundles retained? Size-based or time-based?
7. **Eval framework recommendations** — Should SERA recommend or bundle a specific eval framework, or remain agnostic?

# SPEC: Gateway (`sera-gateway`)

> **Status:** DRAFT
> **Source:** PRD §4.1, §13 (ChannelConnector proto), §14 (invariants 1–4, 6), plus deltas from [SPEC-dependencies](SPEC-dependencies.md) §10.1 (claw-code `WorkerStatus`, `LaneFailureClass`), §10.2 (Codex SQ/EQ envelope, `AppServerTransport`, `Op::UserTurn`), §10.5 (openclaw `AgentHarness.supports()`, `parent_session_key`), §10.7 (opencode two-layer persistence), §10.10 (OpenHands `EventStream`, webhook-back), §10.15 (MetaGPT `cause_by` routing, Environment push-to-inbox), §10.17 (CAMEL `WorkforceState` PAUSED + `WorkforceSnapshot`), and [SPEC-self-evolution](SPEC-self-evolution.md) §5.5 (ShadowSession mode, generation marker, kill-switch socket)
> **Crate:** `sera-gateway`
> **Priority:** Phase 2

---

## 1. Overview

The gateway is the **central control plane and single entry point** for all traffic in and out of SERA. Every event — whether from a client, a channel connector, an external agent protocol, a scheduled workflow, or an internal subsystem — flows through the gateway. It is the source of truth for routing, session ownership, policy enforcement, and system coordination.

In Tier 1 (local), the gateway is the **single entrypoint process**. Built-in adapters (connectors, providers) run in-process; external adapters connect via gRPC. In Tier 2/3, the gateway remains the coordination hub but may be scaled horizontally behind a load balancer.

---

## 1a. Gateway as Manufacturing Execution System (MES)

> **Architectural framing — 2026-04-13.** This analogy is not cosmetic — it drives concrete design decisions about where state lives, which components are ephemeral, and why.

The SERA gateway is a **Manufacturing Execution System for AI agents**. The MES analogy maps directly onto the architecture:

| MES concept | SERA equivalent |
|---|---|
| **Manufacturing order** | Session — persisted at the gateway, survives worker failures |
| **Work record** | Memory — owned and injected by the gateway, not held by the runtime |
| **Production line / work cell** | Circle — agent groups with shared constitutions |
| **Quality gate** | HITL — nothing destructive without human sign-off |
| **Industrial compliance log** | Merkle audit chain — immutable, for SOC 2 / ISO auditors |
| **Industrial middleware / event bus** | Hook system |
| **Machine operators (cattle)** | Runtimes — ephemeral, replaceable, cloneable |

**Workers are cattle.** Runtimes have no durable local state by design. If a worker crashes mid-turn, the gateway re-queues the turn to a fresh worker. The work record (session, memory, transcript) is always safe at the gateway. This is how industrial systems achieve resilience — not by making individual machines reliable, but by making machine loss inconsequential.

**Sessions are manufacturing orders.** A session persists at the gateway layer across the entire lifecycle of the agent's engagement with a task — across worker restarts, model provider failures, and HITL interruptions. The gateway owns session state; runtimes are stateless.

---

## 1b. Tool Dispatch Ownership

> **Design decision — 2026-04-13.** Tool dispatch belongs entirely to the gateway. This is not a performance consideration — it is a security and architectural boundary.

The tool execution pipeline lives at the gateway, not the harness:

```
Harness              Gateway                   Tool Executor
  │                     │                            │
  │── tool_call ────────▶│                            │
  │                     │── CapabilityPolicy check ──▶│
  │                     │── pre_tool hooks ───────────▶│
  │                     │── dispatch ─────────────────▶│ (local/sandboxed/remote)
  │                     │◀─ result ────────────────────│
  │◀─ tool_result ───────│                            │
```

**Why this matters for remote access:** Industrial systems (PLCs, SCADA, sensors, ERP) are safely exposed to agents because the gateway holds the connections and enforces policy. The harness never holds credentials, never knows the network topology, and never has a direct path to sensitive infrastructure. The harness just sees tool results.

The gateway is responsible for:
- **Resolving** which executor handles a given tool call
- **AuthZ** via CapabilityPolicy before dispatch
- **`pre_tool` and `post_tool` hook chains**
- **Credential injection** — harnesses never receive raw credentials
- **Audit** — every tool call is logged with full provenance

See SPEC-tools §6 for the complete dispatch flow.

---

## 1c. Context Injection Responsibility

In **enterprise/cattle mode**, the gateway is responsible for assembling and injecting all session context into the runtime before the turn begins. The runtime does not know the source of the context it receives:

- Soul/persona definition → injected as system prompt prefix
- Memory → selected by the gateway (semantic retrieval, scope filtering) and injected
- Tool schemas → selected by the gateway (CapabilityPolicy, progressive disclosure) and injected
- Circle constitution → injected as part of context assembly

The runtime treats all of this as an opaque context window. It does not read files, query databases, or talk to the memory backend directly.

In **standalone/pet mode**, the runtime itself reads workspace files (`soul.md`, `memory.md`) during context assembly — but this is an implementation detail of the file-based `ContextEngine`, not a general architecture principle. See SPEC-memory §1a for the two-backend model.

---

## 2. Responsibilities

1. **Event ingress and egress routing** with hook pipeline support (`pre_route` / `post_route` chains)
2. **Session lifecycle management** via configurable state machine with hook transitions
3. **Lane-aware FIFO queue** with configurable modes and global concurrency throttle
4. **Authentication and authorization enforcement** for all principals — delegates to `sera-auth`
5. **Inbound message deduplication and debouncing**
6. **Scheduler** for cron jobs, heartbeats, and triggered workflows (delegates to `sera-workflow`)
7. **Hook trigger orchestration** — invokes hook chains at defined hook points (delegates to `sera-hooks`)
8. **Webhook ingress** and **webhook trigger dispatch**
9. **Channel connector registry** with identity mapping and lifecycle management
10. **Plugin registry** with dynamic registration and hot-reloading
11. **Health, status, and diagnostics endpoints** with OpenTelemetry support
12. **HITL approval routing** — delegates to `sera-hitl`
13. **Configuration management surface** accessible to principals for self-bootstrapping — delegates to `sera-config`
14. **Bundled documentation** accessible to agents running on the instance

---

## 3. Event Model (SQ/EQ Envelope)

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.2 — Codex `codex-rs/protocol/src/protocol.rs` pattern. SERA's event envelope is a **Submission Queue / Event Queue (SQ/EQ)** model.

Clients, channels, and internal subsystems push `Submission` values onto the gateway's submission queue. The gateway emits `Event` values onto the event queue back to the submitter. This is the canonical envelope for every gateway interaction — no parallel RPC surface is allowed to bypass it.

### 3.1 Submission (inbound)

```rust
/// The canonical inbound envelope. Every action into the gateway is a Submission.
pub struct Submission {
    pub id: SubmissionId,
    pub op: Op,
    pub trace: W3cTraceContext,              // NON-OPTIONAL per SPEC-observability §2
    pub change_artifact: Option<ChangeArtifactId>, // Populated when part of a self-evolution flow (SPEC-self-evolution §5.3)
}

/// The operation the Submission requests. Per-turn policy overrides are FIRST-CLASS fields on UserTurn.
pub enum Op {
    /// Start or continue a session turn. Per-turn policy overrides live on this variant.
    UserTurn {
        items: Vec<UserInput>,
        cwd: Option<PathBuf>,
        approval_policy: AskForApproval,       // see SPEC-hitl-approval §4
        sandbox_policy: SandboxPolicy,          // see SPEC-tools §6a
        model_override: Option<String>,
        effort: Option<Effort>,
        final_output_schema: Option<serde_json::Value>,
    },

    /// Inject a mid-turn user message (steer queue mode, §5.2).
    Steer { session_key: SessionKey, items: Vec<UserInput> },

    /// Cancel an active turn.
    Interrupt { session_key: SessionKey },

    /// Administrative ops (session lifecycle, config, self-evolution).
    System(SystemOp),

    /// Approval response (amendment pattern — approvals flow back through the SQ, not through a separate RPC).
    ApprovalResponse { approval_id: ApprovalId, decision: ApprovalDecision },

    /// Register an external channel connector or harness.
    Register(RegisterOp),
}
```

### 3.2 Event (outbound)

```rust
/// The canonical outbound envelope. Lifecycle and streaming share the same channel.
pub struct Event {
    pub id: EventId,
    pub submission_id: Option<SubmissionId>, // Correlates back to a Submission when applicable
    pub msg: EventMsg,
    pub trace: W3cTraceContext,
    pub timestamp: DateTime<Utc>,
}

pub enum EventMsg {
    // Turn lifecycle
    TurnStarted { turn_id, started_at, model_context_window, collaboration_mode },
    TurnComplete { turn_id, last_agent_message, completed_at, duration_ms },

    // Streaming deltas (single channel mixes lifecycle + streaming — per SPEC-dependencies §10.2)
    AgentMessageDelta { turn_id, content: String },
    AgentReasoningDelta { turn_id, content: String },
    AgentReasoningSectionBreak { turn_id },
    ExecCommandOutputDelta { turn_id, call_id: ToolCallId, stream: StdStream, content: String },
    TerminalInteraction { turn_id, stdin_sent: String, stdout_seen: String },

    // Tool lifecycle
    ToolCallBegin { turn_id, call_id, tool: ToolRef, arguments: serde_json::Value },
    ToolCallEnd { turn_id, call_id, result: ToolResult },
    McpToolCallBegin { turn_id, call_id, server: String, tool: String, arguments: serde_json::Value },
    McpToolCallEnd { turn_id, call_id, result: ToolResult },

    // HITL
    ExecApprovalRequest { approval_id, turn_id, risk: ActionSecurityRisk, proposed_action: ProposedAction },
    ApplyPatchApprovalRequest { approval_id, turn_id, patch: PatchSummary },
    ElicitationRequest { approval_id, turn_id, prompt: String, input_schema: serde_json::Value },
    GuardianAssessment { turn_id, risk_level: GuardianRiskLevel, rationale: String },

    // Compaction (first-class event per SPEC-dependencies §10.10 OpenHands pattern)
    CondensationRequest { session_key, target_tokens: u64 },
    Condensation { session_key, summary: String, forgotten_event_ids: Vec<EventId> },

    // Session lifecycle
    SessionTransition { session_key, from: SessionState, to: SessionState, trigger: TransitionTrigger },
    SubagentSpawned { parent: SessionKey, child: SessionKey, cause_by: ActionId },
    SubagentEnded { parent: SessionKey, child: SessionKey, result: AgentDelegateObservation },

    // System
    Error { kind: GatewayError, message: String },
    Heartbeat { session_key, status: TurnStatus, tokens_used: u64, elapsed: Duration },
}

pub enum EventSource {
    Channel,
    Scheduler,
    API,
    Internal,
    A2A,
    // Acp — removed. See SPEC-interop §5 (merged into A2A 2025-08-25).
}

pub struct EventContext {
    pub agent_id: AgentId,
    pub session_key: SessionKey,
    pub sender: PrincipalRef,
    pub recipient: Option<AgentRef>,
    pub principal: PrincipalRef,
    pub cause_by: Option<ActionId>,         // Typed routing discriminant per SPEC-dependencies §10.15 MetaGPT
    pub parent_session_key: Option<SessionKey>, // Subagent lineage per SPEC-dependencies §10.5 openclaw
    pub generation: GenerationMarker,       // N or N+1 per SPEC-self-evolution §10
    pub metadata: HashMap<String, serde_json::Value>,
}
```

### 3.3 Event Stream persistence

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.10 OpenHands `EventStream` + `FileStore`.

The gateway maintains a **persistent `EventStream`** per session with durable append-only JSON-page storage. Subscribers register per-component callbacks (`AGENT_CONTROLLER`, `SERVER`, `RUNTIME`, `MEMORY`, `MAIN`, `AUDIT`); events are dispatched asynchronously via per-subscriber queues. Every event is persisted to the store **before** any subscriber is notified — this is the foundation for the shadow-session dry-run (SPEC-self-evolution §11) and session replay.

### 3.4 Delta event naming convention

All streaming token-level events use the `*Delta` suffix (`AgentMessageDelta`, `ExecCommandOutputDelta`). All lifecycle events use a verb-noun form (`TurnStarted`, `ToolCallEnd`). No separate streaming sub-protocol exists — the EQ mixes lifecycle and streaming deltas on one channel.

---

## 4. Event Routing Pipeline

```
Event Ingress
  → Deduplication (idempotency_key check)
  → Authentication (resolve principal)
  → pre_route hook chain
  → Routing decision (agent + session resolution)
  → post_route hook chain
  → Authorization check (can this principal interact with this agent?)
  → Enqueue to lane-aware queue
```

### 4.1 Inbound Deduplication

Real channels redeliver messages after reconnects. The gateway maintains a short-lived dedupe cache keyed by `(channel, account, peer, session_key, message_id)`. Duplicate deliveries do not trigger additional runs.

```rust
pub struct DedupeKey {
    pub channel: ChannelRef,
    pub account: Option<AccountRef>,
    pub peer: PrincipalRef,
    pub session_key: SessionKey,
    pub message_id: String,
}
```

Cache entries expire after a configurable TTL (default: 5 minutes).

### 4.2 Inbound Debouncing

Humans type in bursts. The gateway can batch rapid consecutive text messages into a single agent turn via configurable debounce.

**Rules:**
- Debounce applies to **text-only messages** — attachments and media flush immediately
- **Control commands** (e.g., `/pause`, `/cancel`) bypass debouncing and are processed standalone
- Debounce window is configurable globally and per-channel

```yaml
sera:
  gateway:
    messages:
      inbound:
        dedupe_ttl_ms: 300000          # 5 minutes
        debounce_ms: 500               # Default: batch rapid messages
        debounce_override:             # Per-channel overrides
          discord: 300
          telegram: 500
```

---

## 5. Lane-Aware FIFO Queue

The queue enforces **single-writer-per-session** and **global concurrency throttle**.

### 5.1 Queue Semantics

- Each session has its own **lane** — a FIFO queue guaranteeing ordered processing
- A **global concurrency cap** (`max_concurrent_runs`) limits how many sessions can be actively processing at once
- When a session's lane is being processed, subsequent messages for that session queue behind it

### 5.2 Queue Modes

Queue modes control what happens when a new message arrives for a session that is currently mid-turn. The mode is configurable per-agent and can be overridden per-session.

| Mode | Behavior |
|---|---|
| **collect** (default) | Coalesce queued messages into one follow-up turn. All messages that arrived during the current run are batched and delivered together after the run completes. |
| **followup** | Always wait until the current run ends, then process queued messages as sequential follow-up turns (one per message). |
| **steer** | Inject the incoming message into the current run at the next **tool boundary**. Remaining pending tool calls from the current assistant message are skipped, and the queued user message is injected before the next assistant response. |
| **steer-backlog** | Steer now (inject at next tool boundary) AND also preserve the message for a follow-up turn after the current run completes. |
| **interrupt** (legacy) | Abort the active run immediately, then start a new run with the newest message. Not recommended for production — risks orphaned side effects. |

**Steer Contract:** The queue is checked **after each tool call** in the runtime's tool call loop. If a queued steer message exists:
1. Remaining tool calls from the current assistant message are skipped
2. The queued user message is appended to the session transcript
3. The model is called again with the updated context

This is safe because tool boundaries are natural interrupt points — each tool call is atomic.

> [!NOTE]
> The `steer` mode's behavior may vary slightly depending on whether the runtime supports streaming delivery. On some surfaces, steer behaves closer to a follow-up. The runtime documents the exact steer semantics in [SPEC-runtime](SPEC-runtime.md) §6.

### 5.3 Queue Persistence

| Tier | Backend | Durability |
|---|---|---|
| Tier 1 (local) | SQLite-backed | Survives crash |
| Tier 2 (team) | PostgreSQL-backed | Durable |
| Tier 3 (enterprise) | TBD (Redis Streams / NATS — deferred to Phase 4) | Durable + distributed |

The queue trait abstracts the backend:

```rust
#[async_trait]
pub trait QueueBackend: Send + Sync {
    async fn enqueue(&self, event: Event, lane: SessionKey) -> Result<(), QueueError>;
    async fn dequeue(&self, lane: &SessionKey) -> Result<Option<Event>, QueueError>;
    async fn peek(&self, lane: &SessionKey) -> Result<Option<&Event>, QueueError>;
    async fn lane_depth(&self, lane: &SessionKey) -> Result<usize, QueueError>;
    async fn global_active_count(&self) -> Result<usize, QueueError>;
}
```

### 5.4 Configuration

```yaml
sera:
  queue:
    max_concurrent_runs: 10
    default_mode: "followup"       # collect | followup | steer | interrupt
    backend: "sqlite"              # sqlite | postgres | (future: redis-streams, nats)
```

---

## 6. Session State Machine

> [!IMPORTANT]  
> **Requires further research:** The PRD states the session state machine is "extensible via config" with custom states and hook-driven transitions. The exact configuration format for defining custom states and transitions needs to be designed.

### 6.1 Default States

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.1 (claw-code `WorkerStatus` 6-state lifecycle) + §10.17 (CAMEL `WorkforceState` `PAUSED` + `WorkforceSnapshot`) + [SPEC-self-evolution](SPEC-self-evolution.md) (`ShadowSession` mode).

```rust
pub enum SessionState {
    // Boot sequence (mirrors claw-code WorkerStatus)
    Created,
    Spawning,             // Harness process starting
    TrustRequired,        // Auth/permission gate at harness boot
    ReadyForPrompt,       // Harness ready for first message from gateway
    Active,

    // HITL + compaction
    WaitingForApproval,   // HITL gate
    Compacting,
    Paused,               // CAMEL WorkforceState::PAUSED — human-intervention pause, resumable via WorkforceSnapshot

    // Terminal states
    Suspended,
    Archived,
    Destroyed,

    // Shadow mode (SPEC-self-evolution §5.5) — session is captured, replayable, but no new events are accepted
    Shadow,
}

pub enum WorkerFailureKind {
    TrustGate,
    PromptDelivery,
    Protocol,
    Provider,
}
```

### 6.1a Prompt-Misdelivery Replay

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.1 claw-code `PromptMisdelivery` + `PromptReplayArmed`.

When the gateway dispatches a turn submission to a harness and the harness does not acknowledge receipt within a configurable window, the gateway:

1. Marks the prompt as `PromptMisdelivery` on the session
2. Increments `prompt_delivery_attempts`
3. If `auto_recover_prompt_misdelivery` is enabled and attempts < `max_delivery_attempts`, arms a `PromptReplayArmed` state and re-sends the original submission
4. Emits a `LaneEvent::PromptDelivery` with `LaneFailureClass::PromptDelivery` for observability (§12)
5. If attempts exceed `max_delivery_attempts`, fails the turn with a typed `GatewayError::PromptDeliveryExhausted`

This is a concrete anti-flake mechanism for harnesses that crash or hang during prompt ingestion — it prevents lost user messages in the common case without requiring session-level recovery.

### 6.1b Session Persistence — Two-Layer

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.7 opencode two-layer persistence.

Session state is persisted in **two complementary layers**:

| Layer | Storage | Content |
|---|---|---|
| **Conversation state** | `sqlx` (SQLite Tier-1, PostgreSQL Tier-2/3) via `sera-db` | Tables: `SessionTable`, `MessageTable`, `PartTable` (tool calls, text, reasoning, file attachments stored separately for efficient streaming updates) with `parent_id` foreign key for subagent lineage |
| **Filesystem state** | Shadow git repo at `~/.local/share/sera/snapshot/<project-id>/<worktree-hash>/` | `Snapshot.track()` before each tool execution captures a git hash; `revert(patches)` enables per-message undo; `diffFull(from, to) -> FileDiff[]` computes display diffs |

The `pre_tool` hook calls `Snapshot.track()`; the `post_tool` hook settles it. On session recovery, both layers are rehydrated — conversation state from sqlx, filesystem state from the shadow git repo. This covers the "runtime crash during tool execution" failure mode.

### 6.2 Transitions

```rust
pub struct SessionTransition {
    pub from: SessionState,
    pub to: SessionState,
    pub hook_chain: Vec<HookRef>,
    pub condition: Option<TransitionCondition>,
}
```

Transitions fire hook chains, enabling lifecycle actions (cleanup, notification, audit) to be attached declaratively.

### 6.3 Session Key and Scoping

The session key determines which transcript and state bucket a turn belongs to. It does **not** determine authorization — that is handled separately (Invariant #6).

#### Scoping Strategies

| Strategy | Session Key Format | Use Case |
|---|---|---|
| **main** (default) | `agent:{agent_id}:main` | Single-user DM with one agent. All DMs collapse into one session for continuity. |
| **per-channel** | `agent:{agent_id}:channel:{channel_id}` | Each channel/group gets its own session. DMs still share the main session. |
| **per-channel-peer** | `agent:{agent_id}:channel:{channel_id}:peer:{peer_id}` | Each person in each channel gets their own session. Use when multiple people can DM the agent. |
| **per-account-channel-peer** | `agent:{agent_id}:account:{account_id}:channel:{channel_id}:peer:{peer_id}` | Full isolation when the agent has multiple platform accounts. |
| **per-thread** | `agent:{agent_id}:channel:{channel_id}:thread:{thread_id}` | Thread-level isolation in platforms that support threads (Discord, Slack). |

#### Secure DM Mode

When an agent can receive DMs from multiple people, the default **main** scoping leaks context between senders. Operators **MUST** configure a per-peer scoping strategy in multi-user setups:

```yaml
sera:
  sessions:
    dm_scope: "per-channel-peer"       # Isolate DMs per sender
```

> [!WARNING]
> Using `main` scoping with multiple DM senders is a **confidentiality risk**. One sender's context becomes visible to subsequent senders' turns. The gateway should warn during startup if `main` scoping is used with multi-user connectors.

---

## 7. Channel Connector Registry

Each connector maps **one bot token / identity to one agent identity** — providing traceable agent identity across channels.

### 7.0 Harness Selection by Capability

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.5 openclaw `AgentHarness.supports()`.

The gateway does **not** hardcode which harness handles which provider/model. Instead, every registered harness implements a `supports()` query and the gateway ranks results by priority at dispatch time:

```rust
#[async_trait]
pub trait AgentHarness: Send + Sync {
    fn id(&self) -> HarnessId;
    fn label(&self) -> &str;

    /// Capability negotiation — returns whether this harness can handle the given context and at what priority.
    async fn supports(&self, ctx: &HarnessSupportContext) -> HarnessSupport;

    /// Execute a turn. The gateway routes via the transport selected in §7a.
    async fn run_attempt(&self, params: AgentHarnessAttemptParams) -> Result<AgentHarnessAttemptResult, HarnessError>;

    /// Optional lifecycle hooks.
    async fn compact(&self, params: AgentHarnessCompactParams) -> Result<Option<AgentHarnessCompactResult>, HarnessError> { Ok(None) }
    async fn reset(&self, params: AgentHarnessResetParams) -> Result<(), HarnessError> { Ok(()) }
    async fn dispose(self: Box<Self>) -> Result<(), HarnessError> { Ok(()) }
}

pub struct HarnessSupportContext {
    pub provider: String,
    pub model_id: String,
    pub requested_runtime: Option<RuntimeKind>,
    pub capabilities_required: HashSet<HarnessCapability>,
}

pub enum HarnessSupport {
    Supported { priority: u32 },
    Unsupported { reason: String },
}
```

At dispatch time the gateway queries every registered harness and picks the one returning the highest `priority` among `Supported` results. This eliminates static dispatch by ID and lets operators plug in specialised harnesses (coding, vision, long-context) without patching the gateway.

---

## 7a. Gateway ↔ Harness Transport (NEW)

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.2 — openai/codex `codex-rs/app-server/src/transport/mod.rs`. **This is the architectural spine that makes SERA's harness pluggable.**

The gateway and harness talk over a **pluggable transport** negotiated at harness registration time. All three variants carry identical JSON-RPC framing over the SQ/EQ envelope (§3) — the transport only determines the byte-level medium.

```rust
pub enum AppServerTransport {
    /// In-process: harness is a Rust object loaded into the gateway binary. Zero-copy envelope.
    /// Used for: test harnesses, Tier-1 single-binary local deployments.
    InProcess,

    /// Standard input/output: harness is a subprocess; JSON-RPC messages framed over stdin/stdout.
    /// Used for: language-agnostic harnesses, claw-code-style monolithic harnesses wrapped as subprocesses.
    Stdio {
        command: PathBuf,
        args: Vec<String>,
        env: HashMap<String, String>,
    },

    /// WebSocket (or gRPC streaming): harness runs on a remote host, connects inbound to the gateway
    /// or accepts inbound connections from the gateway. JSON-RPC framing over WS; gRPC variant uses tonic streaming.
    WebSocket {
        bind_address: SocketAddr,
        tls: TlsConfig,
    },
    Grpc {
        endpoint: String,
        tls: TlsConfig,
    },

    /// Webhook-back: harness runs inside a sandbox that cannot accept inbound connections;
    /// the SDK calls back to the gateway via OH_WEBHOOKS_0_BASE_URL + OH_SESSION_API_KEY env vars.
    /// Used for: NAT'd or cloud-isolated sandboxes. See SPEC-dependencies §10.10 OpenHands V1 pattern.
    WebhookBack {
        callback_base_url: String,
        session_api_key_generator: Box<dyn Fn() -> SessionApiKey + Send + Sync>,
    },

    /// Disabled (used for gateway health-check configurations).
    Off,
}
```

### 7a.1 Invariants

1. **Same envelope on every transport.** Every transport serialises the SQ/EQ envelope (§3) identically. A harness written against `InProcess` can be repackaged as `Stdio` or `WebSocket` without touching the agent loop.
2. **Transport negotiation at registration.** Harness registration includes the supported transport list; the gateway picks the lowest-latency variant available to both sides. `InProcess` > `Stdio` > `WebSocket`/`Grpc` > `WebhookBack`.
3. **Protocol versioning via serde alias.** JSON-RPC envelope fields use serde `alias` attributes for forward/backward compatibility — `task_started` ↔ `turn_started`, etc. Never bump the proto version solely for a rename. See [SPEC-versioning](SPEC-versioning.md) §4.5.
4. **`InProcess` is required for tests.** Every harness implementation must provide an `InProcess` variant so the gateway test suite can exercise it without spinning up a subprocess.

### 7a.2 Two-Generation Boot

> **Source:** [SPEC-self-evolution](SPEC-self-evolution.md) §10.

For Tier-3 self-evolution, the gateway supports running **two generations of a transport in parallel**. The generation is tracked on `EventContext.generation: GenerationMarker` (§3.2) and on the session key. New sessions route to generation `N+1` after promotion; in-flight sessions on generation `N` continue until they drain. Session-level routing is enforced at the gateway — a session does not split across generations mid-turn (closes "live-migration replay corruption", SPEC-self-evolution §14.8).

```rust
pub struct GenerationMarker {
    pub label: GenerationLabel,        // "n" | "n_plus_1"
    pub binary_identity: BuildIdentity, // see SPEC-versioning §4.6
    pub started_at: DateTime<Utc>,
}
```

### 7a.3 Shadow Session Mode

> **Source:** [SPEC-self-evolution](SPEC-self-evolution.md) §5.5, §11.

When a session is marked `SessionState::Shadow`, the gateway:

- Accepts **no new inbound submissions** on the SQ
- Replays the captured event stream against a *proposed* config or binary
- Emits replay events onto a separate EQ channel for dry-run validation
- Does not mutate any durable state except the replay result record

This is the shadow-session dry-run gate required for every Tier-2 and Tier-3 Change Artifact.

### 7a.4 Kill-Switch Admin Socket

> **Source:** [SPEC-self-evolution](SPEC-self-evolution.md) §13.

The gateway MUST listen on a dedicated Unix socket (default `/var/lib/sera/admin.sock`) that bypasses the normal auth stack. Authentication is by OS-level file ownership — only the process user running the gateway can connect. The socket accepts a single command: `ROLLBACK`. Activation forces a rollback to the last known-good state regardless of agent/session state, kills `N+1` if running, and emits a `KILL_SWITCH_ACTIVATED` audit event.

Additional kill-switch paths (file-based, env var, `SIGUSR2`) are documented in SPEC-self-evolution §13 and must also be implemented by the gateway process.

---

## 7.1 Connector Registration (renumbered)

Connectors register with the gateway via gRPC. The gateway manages their lifecycle (start, stop, health check, reconnect).

### 7.2 Identity Mapping

```yaml
connectors:
  - name: "discord-main"
    kind: "discord"
    token: { secret: "connectors/discord-main/token" }
    agent: "sera"                # 1:1 mapping
```

### 7.3 gRPC Service (for external connectors)

```protobuf
service ChannelConnector {
    rpc SendMessage(SendMessageRequest) returns (SendMessageResponse);
    rpc StreamEvents(ConnectorAuth) returns (stream InboundEvent);
    rpc GetStatus(Empty) returns (ConnectorStatus);
    rpc Shutdown(Empty) returns (Empty);
}
```

Built-in connectors (e.g., Discord in Tier 1) run in-process and implement the same trait, but skip gRPC serialization.

---

## 8. Plugin Registry

Dynamic registration and hot-reloading of plugins (connectors, tools, model providers, hook modules).

> [!NOTE]  
> Hot-reloading scope: ideally all registrations are hot-reloadable without gateway restart. If specific registration types require restart, this should be documented as an engineering constraint.

---

## 9. Webhook Ingress / Egress

> [!IMPORTANT]  
> **Requires further specification.** The PRD mentions webhook ingress and webhook trigger dispatch but does not detail:
> - How inbound webhooks are authenticated (shared secret? HMAC? JWT?)
> - How agents trigger outbound webhooks (as a tool? as a hook result?)
> - Webhook registration and lifecycle management
> - Retry and delivery guarantee semantics

---

## 10. Protocol Support

The gateway serves both **WebSocket** and **gRPC streaming** on the same process:

| Protocol | Use Case | Transport |
|---|---|---|
| WebSocket | Web clients, AG-UI streaming, simple clients | `axum` |
| gRPC | Inter-service, connectors, external runtimes, tools | `tonic` |
| HTTP/REST | Health endpoints, webhook ingress, simple queries | `axum` |

---

## 11. Invariants (from PRD §14)

| # | Invariant | Enforcement |
|---|---|---|
| 1 | Single-writer per session | Queue lane isolation |
| 2 | Global concurrency cap | Queue global throttle |
| 4 | Inbound dedupe | Dedupe cache (idempotency_key) |
| 6 | Session key = routing ≠ authorization | Session key used for routing; authz checked separately via `sera-auth` |

---

## 12. Hook Points

| Hook Point | Fires When |
|---|---|
| `pre_route` | After event ingress, before queue |
| `post_route` | After routing decision, before enqueue |
| `on_session_transition` | On session state machine transition |

---

## 13. Configuration

```yaml
sera:
  instance:
    name: "my-sera"
    tier: "local"               # local | team | enterprise
    docs_dir: "./docs"

  gateway:
    host: "0.0.0.0"
    grpc_port: 50051
    http_port: 8080
    ws_port: 8081
    max_concurrent_runs: 10

  connectors:
    - name: "discord-main"
      kind: "discord"
      token: { secret: "connectors/discord-main/token" }
      agent: "sera"
```

---

## 13a. LLM Proxy Surface

> **Design decision — 2026-04-13.** The gateway CAN act as a universal LLM proxy for any connected harness.

When a BYOH harness (Claude Code, Codex, Hermes, external agent) connects to the gateway, all LLM calls from that harness can be routed through the gateway's LLM proxy surface. This is optional but recommended for regulated environments.

The gateway LLM proxy provides:

| Feature | Description |
|---|---|
| **Budget enforcement** | Per-agent, per-circle, per-project token budget tracking and hard caps |
| **Cost attribution** | All LLM spend attributed to the originating agent/circle/project |
| **Provider routing** | Opaque rerouting to different models/providers without harness changes |
| **Provider failover** | Automatic failover when a provider returns errors or exceeds latency threshold |
| **Audit log** | Every LLM request and response logged with full provenance |
| **Content filtering** | Pre-prompt and post-response filtering before content leaves the compliance boundary |

For regulated environments (industrial, healthcare, finance) this is not optional — it is a **compliance requirement**. All LLM calls MUST go through the control plane so they can be audited, filtered, and attributed. The gateway is the compliance boundary.

```yaml
sera:
  llm_proxy:
    enabled: true                    # false = harnesses call providers directly (not recommended)
    audit_all_calls: true
    content_filter:
      pre_prompt: true               # Filter prompts before they leave the network
      post_response: true            # Filter responses before harness sees them
    budget:
      default_per_agent_tokens: 1000000
      hard_cap_on_exceed: true       # Reject calls that would exceed budget
```

**Inference virtual host.** When `enabled: true`, harnesses inside sandboxes direct all inference requests to `inference.local:443`. The gateway intercepts, rewrites the `model` field, injects auth headers, and forwards to the resolved provider. This is the same `inference.local` pattern as OpenShell (SPEC-tools §6a.6) — one egress rule covers all providers.

---

## 14. Cross-References

| Dependency | Spec | Relationship |
|---|---|---|
| `sera-auth` | [SPEC-identity-authz](SPEC-identity-authz.md) | AuthN + AuthZ for all principals; `MetaChange` / `CodeChange` / `MetaApprover` capabilities consumed here |
| `sera-hooks` | [SPEC-hooks](SPEC-hooks.md) | Hook chain execution at routing points; `constitutional_gate` is the first hook fired on any Submission carrying a Change Artifact |
| `sera-queue` | This spec (§5) | Queue is architecturally part of the gateway; `apalis`-backed (SPEC-crate-decomposition §3, SPEC-dependencies §8.3) |
| `sera-session` | This spec (§6) | Session state machine is gateway-owned; 6-state boot lifecycle + `Paused` + `Shadow` per §6.1 |
| `sera-hitl` | [SPEC-hitl-approval](SPEC-hitl-approval.md) | Approval routing delegation; approval responses flow through SQ `Op::ApprovalResponse` — no parallel RPC surface |
| `sera-workflow` | [SPEC-workflow-engine](SPEC-workflow-engine.md) | Scheduling trigger delegation; beads `Issue` model for `WorkflowTask` |
| `sera-config` | [SPEC-config](SPEC-config.md) | Config management surface; shadow config store for dry-run replay |
| `sera-secrets` | [SPEC-secrets](SPEC-secrets.md) | Secret resolution for connector tokens; gRPC injection at sandbox startup per SPEC-dependencies §10.18 |
| `sera-telemetry` | [SPEC-observability](SPEC-observability.md) | Health, diagnostics, OpenTelemetry, **OCSF v1.7.0 audit events** + `LaneFailureClass` taxonomy |
| `sera-meta` | [SPEC-self-evolution](SPEC-self-evolution.md) | Two-generation transport (§7a.2), shadow session mode (§7a.3), kill-switch socket (§7a.4), `ChangeArtifact` field on Submission (§3.1) |
| Dependencies | [SPEC-dependencies](SPEC-dependencies.md) | §10.1 claw-code `WorkerStatus`/`PromptMisdelivery`/`LaneFailureClass`; §10.2 Codex SQ/EQ + `AppServerTransport`; §10.5 openclaw `supports()` + `parent_session_key`; §10.7 opencode two-layer persistence; §10.10 OpenHands `EventStream` + webhook-back; §10.15 MetaGPT `cause_by`; §10.17 CAMEL `Paused` state |

---

## 15. Open Questions

1. ~~**Queue modes**~~ — Resolved: see §5.2
2. ~~**Session key construction**~~ — Resolved: see §6.3
3. **Session state machine extensibility** — configuration format for custom states/transitions (see §6)
4. **Webhook authentication** — method and lifecycle (see §9)
5. **Multi-node gateway** — leader election, session affinity, queue partitioning (Phase 4)
6. **Rate limiting** — built-in gateway feature or purely hook-driven?

---

## 16. Success Criteria (from PRD §20)

| Metric | Target |
|---|---|
| Gateway routing latency | < 50ms |
| Single-node throughput | ≥ 100 concurrent sessions |
| gRPC adapter latency | < 10ms roundtrip for local connectors |
| Local startup time | < 2 seconds (Tier 1) |

---

## 17. LSP Routing

> **Status:** DRAFT (fills gap `sera-w3np`)

### 17.1 Motivation

The in-process LSP supervisor in `sera-tools` (`rust/crates/sera-tools/src/lsp/supervisor.rs`) serves agents that run **inside** the gateway process — a tool call arrives at `ToolDispatcher`, which calls `LspToolsState::get_or_spawn`, which manages a child `rust-analyzer` process whose lifetime is tied to the agent turn. This path is intentionally short-lived and per-turn.

The gateway needs a separate, persistent LSP routing layer for three reasons that the in-process path cannot satisfy:

1. **Multi-tenant / multi-workspace isolation.** Different agents and different sera-web users may work on different `workspace_root` directories simultaneously. Each needs an isolated language-server process (separate `rootUri`, separate type-check context). The in-process path spawns per-language, not per-workspace.

2. **External callers.** sera-web, IDE plugins connecting via MCP-over-SSE, and remote agents via A2A all need to issue LSP queries without running inside the gateway process. They require a REST surface that the gateway exposes, not an in-process function call.

3. **Process pool amortisation.** `rust-analyzer` peaks at ~1 GB RSS and takes 10–30 s to fully index a large workspace (see `docs/plan/LSP-TOOLS-DESIGN.md §1`). If every new browser session spawned a fresh process, costs would be prohibitive. A gateway-owned pool shares one warmed process across all sessions attached to the same `(language_id, workspace_root)` pair.

**Tradeoff:** This layer adds a REST round-trip vs. the in-process path. That overhead (~1–5 ms for a local gateway) is irrelevant compared to LSP request latency (typically 50–500 ms). External callers have no alternative.

---

### 17.2 Routing Surface

Five REST endpoints. All require a valid sera-auth session (§17.5). Stubs at `rust/crates/sera-gateway/src/routes/lsp.rs` must be replaced by implementations that delegate to `ProcessManager` (§18).

| Method | Path | Purpose |
|---|---|---|
| `POST` | `/api/lsp/sessions` | Create a scoped LSP session |
| `POST` | `/api/lsp/sessions/{id}/request` | JSON-RPC pass-through |
| `GET` | `/api/lsp/sessions/{id}` | Session status + health |
| `GET` | `/api/lsp/sessions/{id}/events` | SSE stream for notifications |
| `DELETE` | `/api/lsp/sessions/{id}` | Tear down session |
| `GET` | `/api/lsp/sessions` | List sessions for authenticated caller |

**Request / response shapes:**

```
POST /api/lsp/sessions
Body:  { "language_id": "rust", "workspace_root": "/workspaces/myproject",
         "initialization_options": { ... } }
200:   { "session_id": "<uuid>" }
400:   language_id unknown in LspServerRegistry
403:   workspace_root outside allowed patterns
```

```
POST /api/lsp/sessions/{id}/request
Body:  <LSP JSON-RPC request object, e.g. textDocument/documentSymbol>
200:   <LSP JSON-RPC response object, id preserved>
413:   body > 10 MB
429:   rate limit exceeded
```

```
GET /api/lsp/sessions/{id}
200:  { "session_id": "...", "language_id": "rust",
       "status": "running|degraded|exited|restarting",
       "last_request_at": "<rfc3339>", "restart_count": 0 }
```

```
GET /api/lsp/sessions/{id}/events
200:  text/event-stream — one JSON object per notification
     ($/progress, textDocument/publishDiagnostics, etc.)
```

```
DELETE /api/lsp/sessions/{id}
204:  session torn down
404:  session not found or not owned by caller
```

```
GET /api/lsp/sessions
200:  [ { "session_id": "...", "language_id": "...", "status": "..." }, ... ]
     Filtered to sessions owned by the authenticated principal.
```

**Stub locations in `rust/crates/sera-gateway/src/routes/lsp.rs`:**
- Line 42: `Err(AppError::Internal(anyhow::anyhow!("LSP server routing not yet implemented...")))` in `definition` — replace with session-based routing once `ProcessManager` exists.
- Line 66: same pattern in `references`.
- Line 100: same pattern in `symbols`.

These three handlers are a legacy shape (per-request language dispatch). Phase 1 implementation replaces them with the session-oriented surface above; the old `POST /api/lsp/definition`, `/references`, `/symbols` routes are deprecated in favour of `POST /api/lsp/sessions/{id}/request` with the equivalent LSP method in the body.

---

### 17.3 Session Lifecycle

1. **Create** — `POST /api/lsp/sessions` arrives. Gateway resolves `language_id` against `LspServerRegistry` (the shared type from `sera-tools::lsp::registry::LspServerRegistry`; see §17.7 for the sharing model). If no entry, return 400. If `workspace_root` fails the ACL check (§17.5), return 403.

2. **Spawn or adopt** — Gateway calls `ProcessManager::spawn` (§18.2) or, if a pooled process already covers `(language_id, workspace_root)`, increments its reference count and returns the existing `ProcessId`. The `ProcessManager` spawns via `SandboxProvider::spawn_restricted` (§17.6).

3. **Initialise** — Gateway sends LSP `initialize` with the caller-supplied `initialization_options` merged over the defaults from `LspServerConfig`. Waits for `initialized` notification. Marks the managed process `Running`.

4. **Store** — Session record written to `ProcessManager`'s persistence store (§18.3): `{ session_id, principal_id, language_id, workspace_root, process_id }`.

5. **Ready** — `session_id` returned to caller.

6. **Tear down** — `DELETE /api/lsp/sessions/{id}` triggers `ProcessManager::shutdown(process_id)` (§18.6 graceful sequence). Registry entry removed.

Session ownership: the `principal_id` from the sera-auth token is stored at creation time. All subsequent requests for that `session_id` must present a token matching the same principal, or the admin role (§17.5).

---

### 17.4 JSON-RPC Proxy Semantics

The gateway is a **thin proxy** on the `/request` path:

- The request body is forwarded verbatim to the language server's stdin via the existing `LspClient` (`sera-tools::lsp::client`). The gateway does not parse or validate the LSP method name or params beyond body-size enforcement.
- The response is read from stdout and returned verbatim with the LSP `id` field intact.
- **Request body size limit: 10 MB.** Requests exceeding this return HTTP 413 before the body is forwarded.
- **Large responses (Phase 2).** `workspace/symbol` and similar queries can return hundreds of KB. Phase 1 buffers the full response. Phase 2 adds chunked streaming via `Transfer-Encoding: chunked`.

**Notifications are not returned from `/request`.** LSP servers emit notifications on stdout interleaved with responses (`$/progress`, `textDocument/publishDiagnostics`, `window/logMessage`, etc.). The gateway's stdout reader classifies each message:

- If `"id"` is present → it is a response to a pending request; match by id and return from `/request`.
- If `"id"` is absent → it is a notification; fan it out to the SSE stream at `GET /api/lsp/sessions/{id}/events`.

The SSE stream carries a JSON object per event line, prefixed `data:`, with `event:` set to the LSP method name (e.g. `event: textDocument/publishDiagnostics`).

**Per-session request serialisation.** LSP servers are single-threaded by design; concurrent requests from multiple callers sharing one process must be serialised. The `ProcessManager` holds a per-process `Arc<Semaphore>` (capacity 1) that `/request` acquires before forwarding and releases after reading the response.

---

### 17.5 Authentication and Authorisation

- All six endpoints require a valid sera-auth bearer token. Unauthenticated requests return 401.
- **Session ownership check:** `GET`, `POST .../request`, and `DELETE` on `{id}` verify that the token's `principal_id` matches the session's stored `principal_id`. Admin role bypasses this check.
- **Workspace ACL:** Operators configure an allowlist of `workspace_root` path patterns in gateway config:
  ```yaml
  sera:
    lsp:
      allowed_workspace_roots:
        - "/workspaces/**"
        - "/home/*/projects/**"
  ```
  A `POST /api/lsp/sessions` whose `workspace_root` does not match any pattern returns 403. An empty allowlist means all paths are permitted (dev-mode default).
- **Tier-based capability:**
  - Tier-1 agents (lowest trust): read-only LSP sessions — `textDocument/documentSymbol`, `textDocument/hover`, `workspace/symbol`. Write-capable methods (`textDocument/rangeFormatting`, `workspace/applyEdit`) are rejected with 403.
  - Tier-3 agents (highest trust): full method set; sandbox config relaxed to allow write access within `workspace_root` (§17.6).

---

### 17.6 Sandbox Model

Gateway-spawned LSP processes run in Tier-2 sandbox by default. The gateway calls `SandboxProvider::spawn_restricted` from `sera-tools::sandbox` with:

```
memory_limit:  2 GiB
cpu_limit:     2 cores
network:       isolated (no external egress)
filesystem:    read-only except workspace_root (rw) and /tmp (rw)
```

For Tier-3 callers, the sandbox config allows write access to `workspace_root` (enabling code-action apply and formatting).

> **Tradeoff:** Sandboxing adds ~200 ms cold-start per LSP process (one-time per session lifetime). For sessions lasting minutes to hours, this is negligible. See `docs/plan/LSP-TOOLS-DESIGN.md §9.1` for the base sandbox rationale.

Operator opt-out: set `sera.lsp.sandbox: false` in gateway config for local dev. A startup warning is emitted.

SPEC-security does not yet define a named "Tier 2 sandbox" concept; when it does, this section should cross-reference it by section number.

---

### 17.7 Differences from the In-Process Path

This section explicitly disambiguates the two LSP paths to prevent confusion during implementation.

| Dimension | In-process path (sera-tools) | Gateway routing (this section) |
|---|---|---|
| **Owner** | `LspToolsState` in `sera-tools::lsp::state` | `ProcessManager` in `sera-gateway::process_manager` |
| **Callers** | Agent ToolDispatcher (in-process tool calls) | sera-web, IDE plugins, remote A2A agents |
| **Lifetime** | Per-agent-turn; supervisor cached across turns but not across gateway restarts | Explicit session; persisted across gateway restarts (§18) |
| **Session granularity** | Per language_id | Per (principal_id, language_id, workspace_root) |
| **Restart policy** | None (Phase 1); crash = LspError::SpawnFailed | Configurable `RestartPolicy` via ProcessManager (§18.2) |
| **Exposed over REST** | No | Yes |

**Shared artefact:** Both paths use the same `LspServerRegistry` type (`sera-tools::lsp::registry::LspServerRegistry`) as the single source of truth for `language_id → command` mappings. Phase 1 keeps two separate registry instances (one in `LspToolsState`, one in the gateway's `AppState`). Phase 2 (shared-pool mode, §18.5) unifies them behind a single gateway-owned registry consulted by both paths.

**They do NOT share a `ProcessManager` in Phase 1.** The in-process supervisor map (`LspToolsState.supervisors`) and the gateway `ProcessManager.children` are separate. Process pooling across both paths is deferred to Phase 2 (§18.5).

---

### 17.8 Metrics

| Metric | Type | Labels |
|---|---|---|
| `lsp_gateway_sessions_active` | Gauge | `language` |
| `lsp_gateway_request_duration_seconds` | Histogram | `language`, `method` |
| `lsp_gateway_restarts_total` | Counter | `language`, `reason` |
| `lsp_gateway_session_creates_total` | Counter | `language`, `result` (`ok`\|`err`) |
| `lsp_gateway_notification_fan_out_total` | Counter | `language`, `lsp_method` |

All metrics exposed on the gateway's existing `/metrics` endpoint (Prometheus scrape format).

---

### 17.9 Test Strategy

**Unit tests** (no real language server):
- Route handlers wired against a stub `ProcessManager` (trait object; see §18 for the `ProcessRegistryStore` trait pattern). Assert correct HTTP status codes for ownership violation, missing session, oversized body, and workspace ACL rejection.
- JSON-RPC proxy: mock stdin/stdout pair; send a `textDocument/documentSymbol` request body, verify the response body is returned verbatim with `id` intact; verify a notification emitted mid-stream is routed to the SSE channel, not the HTTP response.

**Integration tests** (gated on `#[cfg(feature = "integration")]`, require `rust-analyzer` on PATH and Docker):
- Spin up a real gateway with a real rust-analyzer inside a Docker sandbox.
- Create a session against `rust/crates/sera-tools/` as the workspace root.
- Issue three `textDocument/documentSymbol` requests; assert non-empty symbol lists.
- Tear down; assert process is no longer running.

**Negative tests:**
- 403 on `POST /api/lsp/sessions` with a `workspace_root` outside the allowlist.
- 413 on a `/request` body > 10 MB.
- 403 on a Tier-1 session issuing `workspace/applyEdit`.
- 404 on `GET /api/lsp/sessions/{id}` with a session_id owned by a different principal.

---

### 17.10 Open Questions

1. **SSE backpressure.** If a caller disconnects from `GET /api/lsp/sessions/{id}/events` while the language server is emitting a burst of `$/progress` notifications, should the gateway buffer them (bounded channel?) or drop them? Bounded channel risks blocking the stdout reader for other in-flight requests.

2. **Session TTL / idle expiry.** Should sessions with no `/request` activity for N minutes be automatically torn down? If so, what is the default TTL and where is it configured? A stuck rust-analyzer at 1 GB for an idle session is expensive.

3. **Multi-instance gateway.** In Tier 2/3 horizontal deployment (§15 open question 5), sessions are pinned to a gateway instance that owns the child process. How should session routing work when the owning instance restarts? Options: sticky sessions via load balancer; migrate session to new instance (requires process hand-off); accept session loss and require client reconnect.

4. **Backwards compatibility of legacy stub routes.** The existing `POST /api/lsp/definition`, `/references`, `/symbols` handlers (lines 42, 66, 100 in `routes/lsp.rs`) have an incompatible shape from the new session model. Should they be removed immediately, shimmed to create ephemeral sessions internally, or kept as a permanent convenience API?

---

## 18. Process Persistence

> **Status:** DRAFT (fills gap `sera-w3np`)

### 18.1 Motivation

Gateway-managed child processes — LSP servers today, with indexers, language-specific runtimes, and custom plugins to follow — accumulate state that is expensive to recreate. Without a registry:

- **Orphan accumulation.** Gateway crash or restart leaves child processes running with no owner. On the next boot, new processes are spawned alongside the orphans. A single `rust-analyzer` peaks at ~1 GB RSS; ten orphans exhaust a developer workstation.
- **Cold-start tax.** rust-analyzer takes 10–30 s to fully index a large workspace. A registry that allows re-adoption of live processes across gateway restarts eliminates this cost entirely for the common case of a clean gateway restart (e.g. config reload, binary upgrade).
- **Operator visibility.** Without a registry, there is no answer to "which processes is the gateway managing right now?" The registry is the source of truth for this question.

**Tradeoff:** Persistence adds complexity and a storage dependency. The `InMemoryProcessRegistryStore` (§18.3) trades durability for simplicity and is the default; operators that need restart-survival enable `SqliteProcessRegistryStore`.

---

### 18.2 `ProcessManager` Type

New file: `rust/crates/sera-gateway/src/process_manager.rs`.

```rust
pub struct ProcessManager {
    children: Arc<RwLock<HashMap<ProcessId, ManagedProcess>>>,
    persistence: Arc<dyn ProcessRegistryStore>,
    restart_policy: RestartPolicy,
}

pub struct ManagedProcess {
    pub id: ProcessId,
    pub kind: ProcessKind,          // LspServer | CustomPlugin | Indexer
    pub command: String,
    pub args: Vec<String>,
    pub workspace_root: PathBuf,
    pub pid: i32,
    pub started_at: DateTime<Utc>,
    pub status: ProcessStatus,      // Running | Degraded | Exited | Restarting
    pub last_heartbeat: Option<DateTime<Utc>>,
    pub restart_count: u32,
}

pub enum ProcessStatus {
    Running,
    Degraded,
    Exited,
    Restarting,
}

pub enum ProcessKind {
    LspServer,
    Indexer,
    CustomPlugin,
}

pub enum RestartPolicy {
    Never,
    OnCrash { max_attempts: u32, backoff_secs: u64 },
    Always  { backoff_secs: u64 },
}

pub type ProcessId = uuid::Uuid;
```

**Public API:**

| Method | Signature (sketch) | Notes |
|---|---|---|
| `spawn` | `async fn spawn(&self, req: SpawnRequest) -> Result<ProcessId, ProcessError>` | Spawn child, write to store, return id |
| `adopt_existing` | `async fn adopt_existing(&self, pid: i32, meta: ProcessMeta) -> Result<ProcessId, ProcessError>` | Re-adopt a live PID found during reconcile |
| `shutdown` | `async fn shutdown(&self, id: ProcessId) -> Result<(), ProcessError>` | Graceful sequence (§18.6) |
| `list` | `async fn list(&self) -> Vec<ManagedProcess>` | Snapshot of all children |
| `get` | `async fn get(&self, id: ProcessId) -> Option<ManagedProcess>` | Single entry lookup |
| `set_restart_policy` | `async fn set_restart_policy(&self, id: ProcessId, policy: RestartPolicy) -> Result<(), ProcessError>` | Override per-process policy |

`ProcessManager` is held in `AppState` as `Arc<ProcessManager>`. Route handlers for §17 receive it via `State<AppState>` in the standard axum pattern — consistent with the existing `ConnectorRegistry` shape in `rust/crates/sera-gateway/src/connector/mod.rs`.

---

### 18.3 Persistence Store

```rust
#[async_trait]
pub trait ProcessRegistryStore: Send + Sync + 'static {
    async fn upsert(&self, p: &ManagedProcess) -> Result<(), ProcessError>;
    async fn remove(&self, id: ProcessId)     -> Result<(), ProcessError>;
    async fn list(&self)                      -> Result<Vec<ManagedProcess>, ProcessError>;
}
```

Two implementations are specified for Phase 1:

**`InMemoryProcessRegistryStore`** — default, non-persistent. Backed by a `Mutex<HashMap<ProcessId, ManagedProcess>>`. Fast, no I/O, survives nothing across gateway restarts. Used in tests and local dev by default.

**`SqliteProcessRegistryStore`** — persists across gateway restarts. Storage path: `$SERA_DATA_ROOT/process_registry.sqlite`. Uses `rusqlite` (matching the workspace convention for SQLite — see `rust/CLAUDE.md`: "SQLite via rusqlite"). Schema:

```sql
CREATE TABLE IF NOT EXISTS managed_processes (
    id            TEXT PRIMARY KEY,   -- ProcessId (UUID)
    kind          TEXT NOT NULL,
    command       TEXT NOT NULL,
    args_json     TEXT NOT NULL,      -- JSON array, never logged raw
    workspace_root TEXT NOT NULL,
    pid           INTEGER NOT NULL,
    started_at    TEXT NOT NULL,      -- RFC 3339
    status        TEXT NOT NULL,
    restart_count INTEGER NOT NULL DEFAULT 0,
    last_heartbeat TEXT              -- RFC 3339, nullable
);
```

Storage file permissions: `0600` (owner read-write only). The `SqliteProcessRegistryStore` sets this on creation via `std::fs::set_permissions`. On platforms where `0600` is not enforceable (Windows), a startup warning is emitted.

Selection via gateway config:

```yaml
sera:
  lsp:
    process_registry:
      backend: sqlite          # "memory" | "sqlite" (default: "memory")
      sqlite_path: ""          # defaults to $SERA_DATA_ROOT/process_registry.sqlite
```

---

### 18.4 Reconciliation on Gateway Startup

On gateway boot, `ProcessManager::reconcile()` runs before the HTTP listener opens. Steps:

1. **Load stored entries** from `ProcessRegistryStore::list()`. If the store is empty or unavailable, skip reconcile (log a warning).

2. **Probe each PID.** For each stored entry whose `status` was `Running` or `Restarting`:
   - **Linux:** `kill(pid, 0)` returns `Ok(())` if the process is alive and the gateway has permission, `ESRCH` if dead, `EPERM` if alive but owned by another UID (treat as alive, skip re-adoption).
   - **macOS:** same — `kill -0` is POSIX.
   - **Windows:** `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid)` — success means alive.

3. **Verify identity (Linux only).** Read `/proc/{pid}/exe` and compare the resolved path against `ManagedProcess.command`. If they differ, the PID has been recycled by the OS — treat as dead and do not re-adopt.

   > **Tradeoff:** On macOS, `/proc` is unavailable; on Windows the equivalent (`QueryFullProcessImageName`) requires additional privilege. Phase 1 falls back to trusting the PID on non-Linux platforms and logs a warning. Phase 2 can implement platform-specific identity verification.

4. **Alive and identity verified → re-adopt.** Call `ProcessManager::adopt_existing(pid, ...)`. For `LspServer` kind, send a minimal `$/ping` probe (if the server supports it) or a `workspace/symbol` with an empty query and a 2 s timeout. If the probe succeeds, mark `Running`. If the probe fails, mark `Degraded`.

5. **Dead → evict.** Remove from in-memory map and from persistence store. If `restart_policy` is `OnCrash` or `Always`, immediately re-spawn.

6. **Reconcile complete.** Log a summary: N re-adopted, M evicted, K re-spawned.

---

### 18.5 Shared-Pool Mode (Phase 2)

> **This subsection describes Phase 2 only. Phase 1 keeps the in-process and gateway process pools completely separate.**

In Phase 2, multiple consumers — the LSP routing layer (§17) and the agent ToolDispatcher via `LspToolsState` in `sera-tools` — can share a single `ManagedProcess` per `(language_id, workspace_root)` pair to eliminate duplicate processes and avoid duplicate warmup cost.

Sharing requires:

- `ManagedProcess.ref_count: u32` — incremented on each `adopt_existing` call from a new consumer, decremented on each `shutdown` call. Process is actually killed only when `ref_count` reaches zero.
- A negotiation protocol between `sera-tools` and the gateway: when `LspToolsState::get_or_spawn` is called, it checks whether the gateway's `ProcessManager` already holds a live process for the requested `(language_id, workspace_root)`. If so, it attaches to the existing process rather than spawning a new one. The channel for this check is an in-process function call (both live in the same binary); no IPC is needed.
- The `LspServerRegistry` is unified: a single instance, owned by `AppState`, shared (as `Arc`) by both `ProcessManager` and `LspToolsState`.

Phase 1 does not implement any of this. The two pools are separate and may hold duplicate processes for the same workspace. This is a known Phase 1 limitation.

---

### 18.6 Cleanup and Shutdown

**Graceful shutdown sequence** (triggered by `ProcessManager::shutdown(id)` or gateway SIGTERM):

1. For `LspServer` kind: send LSP `shutdown` request, wait up to 5 s for response.
2. Send LSP `exit` notification.
3. Wait up to 5 s for process to exit voluntarily.
4. If still alive: send `SIGTERM` (Unix) or `TerminateProcess` (Windows).
5. Wait up to 5 s.
6. If still alive: send `SIGKILL` (Unix) or forceful `TerminateProcess` with exit code 1 (Windows).
7. Remove from `children` map and call `ProcessRegistryStore::remove(id)`.

For `Indexer` and `CustomPlugin` kinds, steps 1–2 are skipped (no LSP protocol); the sequence begins at step 3.

**Orphan detection on startup** (part of reconcile, §18.4):

On Linux, scan `/proc/*/status` for processes whose `PPid` is 1 (reparented to init, i.e. the gateway that spawned them has exited) and whose command matches an entry in the persistence store with `owner: gateway`. Re-adopt or kill per restart_policy.

**`DELETE /api/lsp/sessions/{id}`** calls `ProcessManager::shutdown(process_id)` and then removes the session record from the LSP session store. If the process is shared (`ref_count > 1` in Phase 2), shutdown decrements `ref_count` instead of killing.

---

### 18.7 Error Surface

```rust
#[derive(Debug, thiserror::Error)]
pub enum ProcessError {
    #[error("spawn failed: {0}")]
    SpawnFailed(#[source] std::io::Error),

    #[error("process not found: {0}")]
    NotFound(ProcessId),

    #[error("process already exists: {0}")]
    AlreadyExists(ProcessId),

    #[error("store failure: {reason}")]
    StoreFailure { reason: String },

    #[error("reconciliation failed for pid {pid}: {reason}")]
    ReconciliationFailed { pid: i32, reason: String },

    #[error("shutdown timed out for process {0}")]
    ShutdownTimeout(ProcessId),
}
```

> **Note:** The `reason` field in `StoreFailure` and `ReconciliationFailed` uses `reason` (not `source`) to avoid the `thiserror` v2 auto-source behaviour for `String` fields (see `rust/CLAUDE.md`: "thiserror v2 auto-detects `source` fields").

---

### 18.8 Metrics

| Metric | Type | Labels |
|---|---|---|
| `gateway_managed_processes_total` | Gauge | `kind`, `status` |
| `gateway_process_spawns_total` | Counter | `kind`, `result` (`ok`\|`err`) |
| `gateway_process_restarts_total` | Counter | `kind`, `reason` |
| `gateway_process_reconcile_duration_seconds` | Histogram | — |
| `gateway_process_shutdown_duration_seconds` | Histogram | `kind`, `outcome` (`clean`\|`sigterm`\|`sigkill`) |

---

### 18.9 Security Considerations

- **Command path logging.** `ManagedProcess.command` is safe to log. `ManagedProcess.args` MUST NOT be logged in full — args may contain secrets (API keys, tokens passed via argv). Log only `command` and `args.len()`.
- **Reconcile side-effect constraint.** During reconcile (§18.4), the gateway probes PIDs and verifies identity. It MUST NOT re-execute any command as a side effect of reading the persistence store. Reconcile either re-adopts an existing live process or re-spawns using the stored metadata — it does not run arbitrary stored commands without operator-configured restart policy authorising it.
- **Storage file permissions.** `SqliteProcessRegistryStore` creates `process_registry.sqlite` with mode `0600`. On creation and after each open, the implementation calls `std::fs::set_permissions` to enforce this. A file found with wider permissions at startup emits a warning and optionally aborts (operator-configurable).
- **Workspace root validation.** `ProcessManager::spawn` validates `workspace_root` against the same allowlist used in §17.5 before spawning any child process. Storing an arbitrary path in the registry is not sufficient to bypass the ACL — the ACL is re-checked at spawn time.

---

### 18.10 Test Strategy

**Unit tests** (no real processes):

- Mock `ProcessRegistryStore` (in-memory); call `spawn → list → shutdown` round-trip; assert store reflects each state transition.
- `RestartPolicy::OnCrash { max_attempts: 2, backoff_secs: 1 }` — simulate two crashes, assert process re-spawned twice, then marked `Exited` with no further spawns.
- Reconcile with a known-dead PID: mock `kill -0` returning `ESRCH`; assert entry evicted from store; assert `ProcessStatus::Exited` emitted.
- Reconcile with a live PID whose `/proc/{pid}/exe` returns a different path: assert re-adoption refused.

**Integration tests** (gated on `#[cfg(feature = "integration")]`):

- Spawn a real `echo` process (harmless); persist via `SqliteProcessRegistryStore`; simulate gateway restart by dropping and recreating `ProcessManager`; call `reconcile()`; assert `echo` is re-adopted (probe: `kill -0` succeeds).
- Kill the `echo` process externally; call `reconcile()`; assert entry transitions to `Exited`; assert `OnCrash` policy fires and a new `echo` is spawned.

---

### 18.11 Open Questions

1. **Windows process identity verification.** `QueryFullProcessImageName` requires `PROCESS_QUERY_LIMITED_INFORMATION`. If the gateway runs as a non-admin user, this may not be available for processes owned by other users. Should Phase 1 skip identity verification on Windows unconditionally, or require the gateway to run with elevated privileges?

2. **Restart backoff implementation.** `backoff_secs` in `RestartPolicy` is a flat delay. Should Phase 2 use exponential backoff with jitter (standard practice for crash loops) and if so, what are the cap and jitter parameters? The flat value is simpler to reason about but does not protect against tight crash-restart loops.

3. **Multi-instance process ownership.** In Tier 2/3 horizontal gateway deployment, a process is owned by one gateway instance. If that instance dies, orphans accumulate on its host. Should a peer gateway be able to take ownership via a distributed lock (e.g. a PostgreSQL advisory lock), or should processes always be re-spawned from scratch on a new instance?

4. **`$SERA_DATA_ROOT` definition.** The SQLite path `$SERA_DATA_ROOT/process_registry.sqlite` assumes this env var is defined. The gateway config spec (§13) does not yet define a canonical data-root convention. Should this be `sera.gateway.data_dir` in the YAML config, or a dedicated environment variable, and should it default to `$XDG_DATA_HOME/sera` on Linux?

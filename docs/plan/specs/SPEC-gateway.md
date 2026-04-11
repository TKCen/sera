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

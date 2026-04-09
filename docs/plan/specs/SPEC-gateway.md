# SPEC: Gateway (`sera-gateway`)

> **Status:** DRAFT  
> **Source:** PRD §4.1, §13 (ChannelConnector proto), §14 (invariants 1–4, 6)  
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

## 3. Event Model

```rust
pub struct Event {
    pub id: EventId,
    pub kind: EventKind,
    pub source: EventSource,
    pub context: EventContext,
    pub payload: EventPayload,
    pub timestamp: DateTime<Utc>,
    pub idempotency_key: Option<String>,
    pub requires_approval: Option<ApprovalSpec>,
    pub principal: PrincipalRef,
}

pub enum EventKind {
    Message,
    Heartbeat,
    Cron,
    Webhook,
    Hook,
    System,
    Approval,
    Workflow,
}

pub enum EventSource {
    Channel,
    Scheduler,
    API,
    Internal,
    A2A,
    ACP,
}

pub struct EventContext {
    pub agent_id: AgentId,
    pub session_key: SessionKey,
    pub sender: PrincipalRef,
    pub recipient: Option<AgentRef>,
    pub principal: PrincipalRef,
    pub metadata: HashMap<String, serde_json::Value>,
}
```

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

```rust
pub enum SessionState {
    Created,
    Active,
    WaitingForApproval,  // HITL gate
    Compacting,
    Suspended,
    Archived,
    Destroyed,
}
```

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

### 7.1 Registration

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
| `sera-auth` | [SPEC-identity-authz](SPEC-identity-authz.md) | AuthN + AuthZ for all principals |
| `sera-hooks` | [SPEC-hooks](SPEC-hooks.md) | Hook chain execution at routing points |
| `sera-queue` | This spec (§5) | Queue is architecturally part of the gateway |
| `sera-session` | This spec (§6) | Session state machine is gateway-owned |
| `sera-hitl` | [SPEC-hitl-approval](SPEC-hitl-approval.md) | Approval routing delegation |
| `sera-workflow` | [SPEC-workflow-engine](SPEC-workflow-engine.md) | Scheduling trigger delegation |
| `sera-config` | [SPEC-config](SPEC-config.md) | Config management surface |
| `sera-secrets` | [SPEC-secrets](SPEC-secrets.md) | Secret resolution for connector tokens |
| `sera-telemetry` | [SPEC-observability](SPEC-observability.md) | Health, diagnostics, OpenTelemetry |

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

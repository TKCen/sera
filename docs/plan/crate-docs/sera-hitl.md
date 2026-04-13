# `sera-hitl` — Human-in-the-Loop Approval Crate

## Overview

`sera-hitl` is the planned Rust crate that encapsulates SERA's human-in-the-loop (HitL) approval
layer. It provides the types and service logic for requesting, routing, escalating, and resolving
operator approval decisions at runtime.

HitL appears in three SERA subsystems today (TypeScript):

| Subsystem | Where | Stories |
|---|---|---|
| Capability permission requests | `PermissionRequestService` in sera-core | Story 3.9 |
| Delegation requests | `DelegationRequestService` in sera-core | Story 17.4 |
| Knowledge merge approvals | `KnowledgeGitService` in sera-core | Epic 8 |

The `sera-hitl` crate unifies these into a single approval engine with consistent routing,
escalation, state management, and enforcement semantics. It is a library crate with no binary
entrypoint — it is embedded in `sera-core`.

**Source location (planned):** `rust/crates/sera-hitl/src/`

**Dependencies:**
- `sera-domain` — shared agent types, acting contexts, capability dimensions
- `sera-db` — grant persistence, operator request rows
- `sera-events` — Centrifugo publication and audit trail emission

---

## ApprovalSpec

`ApprovalSpec` is the type-safe description of a single HitL approval request. It answers: *what
is the agent asking for, from whom, and why?*

```rust
/// A complete description of one human-in-the-loop approval request.
pub struct ApprovalSpec {
    /// Unique request ID — used as the deduplication key across all channels.
    pub id: Uuid,

    /// The agent instance submitting the request.
    pub agent_id: String,
    pub agent_name: String,
    pub instance_id: String,

    /// What the agent is requesting.
    pub request: ApprovalRequest,

    /// The grant type the agent is hinting at (operator may choose differently).
    pub preferred_grant_type: Option<GrantType>,

    /// When the request was created.
    pub created_at: OffsetDateTime,

    /// Hard deadline — request is auto-denied after this time regardless of escalation state.
    pub hard_deadline: OffsetDateTime,
}

/// The three categories of HitL request in SERA.
pub enum ApprovalRequest {
    /// Agent requests access to a resource outside its resolved capability set (Story 3.9).
    CapabilityGrant {
        dimension: CapabilityDimension,
        /// The specific resource being requested (path, host, command pattern).
        value: String,
        reason: Option<String>,
    },

    /// Agent requests delegated authority to act on behalf of an operator for an
    /// external service (Story 17.4).
    Delegation {
        scope: DelegationScope,
        reason: Option<String>,
    },

    /// Agent requests that a circle knowledge branch be merged into `main` (Epic 8).
    KnowledgeMerge {
        circle_id: Uuid,
        branch: String,
        /// Short diff summary for the operator approval UI.
        summary: Option<String>,
    },
}

/// The capability dimension being requested in a CapabilityGrant.
pub enum CapabilityDimension {
    Filesystem,
    Network,
    ExecCommands,
    SeraManagement,
}

/// The scope of authority being delegated.
pub struct DelegationScope {
    /// External service identifier, e.g. "github", "google-calendar", "*".
    pub service: String,
    /// Permission names on that service, e.g. ["repo:read", "issues:write"] or ["*"].
    pub permissions: Vec<String>,
    /// Optional resource constraints, e.g. `{ repos: ["org/repo"] }`.
    pub resource_constraints: Option<HashMap<String, Vec<String>>>,
}

/// How long the granted access persists.
pub enum GrantType {
    /// Consumed by the single operation that triggered the request; nothing stored.
    OneTime,
    /// Valid until the agent container stops; held in memory, lost on restart.
    Session,
    /// Stored in `capability_grants` or `delegation_tokens` table; survives restarts.
    Persistent {
        expires_at: Option<OffsetDateTime>,
    },
}
```

### Grant lifecycle by type

| Type | Storage | When effective | Lost on |
|---|---|---|---|
| `OneTime` | None | Immediately, single use | Consumption |
| `Session` | In-memory session grant map | Immediately | Container stop |
| `Persistent` | `capability_grants` / `delegation_tokens` table | Immediately (capability) or next spawn (filesystem bind mount) | `revoked_at` set or `expires_at` reached |

---

## ApprovalRouting

`ApprovalRouting` controls *how* an `ApprovalSpec` reaches an operator. There are three routing
variants, designed to be composed (Autonomous wraps a fallback router for the unmatched case).

```rust
pub enum ApprovalRouting {
    /// Always send to a fixed set of channels — no runtime evaluation.
    Static {
        channel_ids: Vec<String>,
        escalation_chain: EscalationChain,
    },

    /// EgressRouter evaluates `notification_routing_rules` at request time and selects
    /// the matching channels. Falls back to `fallback_channel_ids` if no rule matches.
    Dynamic {
        fallback_channel_ids: Vec<String>,
        escalation_chain: EscalationChain,
    },

    /// Auto-resolve without human involvement when conditions match. Falls back to
    /// the inner router when no condition matches.
    Autonomous {
        conditions: Vec<AutoGrantCondition>,
        /// Routing to use when no condition matches (typically Static or Dynamic).
        fallback: Box<ApprovalRouting>,
    },
}
```

### Static routing

The operator pre-configures which channels receive a given class of approval request. For example,
all `permission.requested` events go to `discord-ops-approvals`. The channel list is fixed at
configuration time; no dynamic evaluation occurs.

**When to use:** High-priority dimensions (network, exec) where the approval audience is always
the same team.

```rust
ApprovalRouting::Static {
    channel_ids: vec!["discord-ops-approvals".into()],
    escalation_chain: EscalationChain {
        levels: vec![
            EscalationLevel {
                channel_ids: vec!["email-on-call".into()],
                timeout: Duration::from_secs(300),
            },
        ],
        on_exhausted: EscalationFinalAction::AutoDeny,
    },
}
```

### Dynamic routing

`EgressRouter` evaluates `notification_routing_rules` rows against the incoming `ChannelEvent`.
Rules specify `event_type`, `min_severity`, and an optional JSONB `filter`. The union of all
matching rules' `channel_ids` is the delivery set for this request.

**When to use:** Multi-team instances where routing varies by agent, dimension, or circle.

```rust
ApprovalRouting::Dynamic {
    fallback_channel_ids: vec!["sera-web-ui".into()],
    escalation_chain: EscalationChain { /* ... */ },
}
```

Dynamic routing rules (from `notification_routing_rules` table):

```yaml
event_type: 'permission.requested'
filter: { "dimension": "filesystem", "agentName": "developer-*" }
channel_ids: ["discord-dev-team"]
min_severity: 'info'
```

### Autonomous routing

The `Autonomous` variant evaluates a list of `AutoGrantCondition`s before touching any channel.
If a condition matches, the request is granted immediately — no operator notification, no blocking
wait. This models pre-approved access patterns (e.g. an agent is always allowed to read `/tmp/**`
without prompting).

```rust
pub struct AutoGrantCondition {
    /// Match on specific request type; None matches any.
    pub request_type: Option<ApprovalRequestType>,
    /// Glob pattern applied to the `value` field of CapabilityGrant requests.
    pub value_pattern: Option<String>,
    /// Capability dimension filter; None matches any.
    pub dimension: Option<CapabilityDimension>,
    /// Grant type to issue when this condition triggers.
    pub grant_type: GrantType,
    /// Optional ceiling: condition stops matching after this many auto-grants per session.
    pub max_per_session: Option<u32>,
}
```

**When to use:** Low-risk patterns the operator has explicitly pre-approved, e.g. allowing
filesystem reads in `/workspace/**` for all development agents without a prompt.

**Security note:** Autonomous conditions must be configured by an operator, not derived from agent
capabilities. The `sera-hitl` service rejects `ApprovalRouting::Autonomous` configurations
sourced from agent manifests — only operator-level CapabilityPolicy and SandboxBoundary files
may set autonomous grant conditions.

---

## Escalation Chains

An escalation chain defines what happens when an approval request is not resolved within the
primary routing timeout. Each level in the chain is tried in order before the final action fires.

```rust
pub struct EscalationChain {
    /// Ordered list of escalation levels. Level 0 is tried first on primary timeout.
    pub levels: Vec<EscalationLevel>,
    /// What to do when all levels are exhausted without a decision.
    pub on_exhausted: EscalationFinalAction,
}

pub struct EscalationLevel {
    /// Channels to notify at this escalation level.
    pub channel_ids: Vec<String>,
    /// How long to wait at this level before moving to the next.
    pub timeout: Duration,
    /// Optional: notify a specific operator by sub/ID at this level.
    pub notify_operator: Option<String>,
}

pub enum EscalationFinalAction {
    /// Auto-deny the request; audit trail entry written; agent receives permission_denied.
    AutoDeny,
    /// Auto-grant with the specified grant type; audit trail records escalation_auto_grant.
    AutoGrant { grant_type: GrantType },
    /// Publish a `hitl.escalation_exhausted` event and leave the request in `Expired` state.
    /// The blocking agent tool call receives an `escalation_exhausted` error.
    PublishAndExpire,
}
```

### Escalation semantics

- **Clock starts** when the `ApprovalSpec` enters `Pending` state.
- **Level 0** uses the primary channel set and the level's `timeout`.
- On timeout: if another level exists, the request transitions to `Escalating`, the next level's
  channels are notified, and the clock resets to that level's `timeout`.
- A decision (`grant` or `deny`) at any level terminates the chain immediately — later levels are
  never notified.
- If the primary channel delivery fails (channel adapter error), escalation begins immediately
  without waiting for the timeout.
- `hard_deadline` in `ApprovalSpec` is an absolute ceiling — even mid-escalation, a request that
  has reached its `hard_deadline` is treated as `on_exhausted: AutoDeny`.

### Default escalation configuration (no operator override)

```
Level 0: SERA web UI                timeout: 5 min
  → Level 1: (none configured)
  → on_exhausted: AutoDeny
```

The 5-minute default matches the `PERMISSION_REQUEST_TIMEOUT_MS` environment variable that was
established in Story 3.9.

---

## Approval State Machine

Each `ApprovalSpec` instance moves through the following states. The state is held in memory
by `HitlService` and mirrored to the `operator_requests` DB table for durability.

### States

```
┌────────────┐
│  Pending   │  Initial state. Request created, routing being evaluated.
└────────────┘
      │
      ▼
┌────────────┐
│  Notified  │  At least one channel has successfully delivered the request to an
│            │  operator. The blocking agent call is now waiting for a decision.
└────────────┘
      │
      ├──────────────────────────────────────────────────────────────┐
      │ operator decides                                             │ timeout elapsed
      │                                                             ▼
      │                                                    ┌──────────────┐
      │                                                    │  Escalating  │  Next escalation
      │                                                    │              │  level notified.
      │                                                    └──────────────┘
      │                                                          │
      │                                                          │ (may return to Notified
      │                                                          │  for each additional level)
      │                                             ┌────────────┘
      │                                             │ all levels exhausted
      │                                             ▼
      │                                       ┌──────────┐
      │                                       │  Expired │  Terminal. on_exhausted action fires.
      │                                       └──────────┘
      │
      ├──── Grant ──────────────────┐
      │                             ▼
      │                    ┌──────────────────┐
      │                    │ Approved(type)   │  Terminal. Grant stored per GrantType.
      │                    └──────────────────┘
      │
      └──── Deny ───────────────────┐
                                    ▼
                           ┌──────────────────┐
                           │     Denied       │  Terminal. Agent receives permission_denied.
                           └──────────────────┘
```

### State transition table

| From | Event | To | Side effects |
|---|---|---|---|
| `Pending` | Routing resolves to Autonomous match | `Approved(type)` | Auto-grant stored; audit written |
| `Pending` | Channel notification sent | `Notified` | `operator_requests` row inserted |
| `Pending` | No channels reachable | `Denied` | Audit written; agent unblocked |
| `Notified` | Operator grants | `Approved(type)` | Grant stored; agent unblocked; stale notifications marked |
| `Notified` | Operator denies | `Denied` | Audit written; agent unblocked |
| `Notified` | Timeout elapsed, next level exists | `Escalating` | Next level channels notified |
| `Notified` | Timeout elapsed, no next level | `Expired` | `on_exhausted` action fires |
| `Escalating` | Escalation notification sent | `Notified` | Escalation level counter incremented |
| `Escalating` | `hard_deadline` reached | `Expired` | Auto-deny regardless of `on_exhausted` |
| `Escalating` | All levels exhausted | `Expired` | `on_exhausted` action fires |
| `Expired` | — | — | Terminal |
| `Approved` | — | — | Terminal |
| `Denied` | — | — | Terminal |

### DB representation

States map to the `status` column on the `operator_requests` table:

| State | `status` value |
|---|---|
| `Pending` / `Notified` | `pending` |
| `Escalating` | `pending` (escalation level tracked in `payload`) |
| `Approved` | `approved` |
| `Denied` | `denied` |
| `Expired` | `expired` |

---

## Enforcement Modes

The enforcement mode controls what happens when an agent tool call encounters a resource outside
its resolved capability set, before any approval request is submitted.

```rust
pub enum EnforcementMode {
    /// The tool call blocks until an approval decision is received (grant, deny, or timeout).
    /// This is the default for all capability dimensions.
    Enforcing,

    /// The tool call proceeds immediately. The out-of-scope access is logged and a
    /// `permission.violation` event is published to all configured notification channels.
    /// No approval request is created; the agent is not blocked.
    Permissive,

    /// The out-of-scope access is recorded in the audit trail only. No notification,
    /// no blocking, no channel event. Used for monitoring and policy development.
    AuditOnly,
}
```

### Mode selection

Enforcement mode is configured per capability dimension in the `SandboxBoundary` or
`CapabilityPolicy` — not in the agent manifest (agents cannot loosen their own enforcement).

```yaml
# In a SandboxBoundary or CapabilityPolicy:
enforcement:
  filesystem: enforcing       # default
  network: enforcing          # default
  exec.commands: enforcing    # default
  llm.budget: permissive      # log over-budget calls, don't block
  memory: audit-only          # observing memory access patterns only
```

### Enforcement mode matrix

| Mode | Agent blocked | Channel notified | Audit written | Grant required |
|---|---|---|---|---|
| `Enforcing` | Yes | Yes (on request) | Yes | Yes |
| `Permissive` | No | Yes (violation event) | Yes | No |
| `AuditOnly` | No | No | Yes | No |

---

## Escalation Flow Diagram

The full end-to-end flow from a tool call violation to a resolved decision:

```
Agent tool call hits out-of-scope resource
              │
              ▼
    ┌─────────────────────┐
    │   EnforcementMode   │
    └──────────┬──────────┘
               │
     ┌─────────┼──────────────┐
     │         │              │
 AuditOnly  Permissive    Enforcing
     │         │              │
   write      write      build ApprovalSpec
   audit      audit      │
   record   + publish    │
     │       violation   ▼
     │       event  ApprovalRouting::resolve(spec)
     │         │         │
     │         │    ┌────┴──────────────────┐
     │         │    │                       │
     │         │  Autonomous           Static / Dynamic
     │         │    │                       │
     │         │  eval conditions      EgressRouter selects
     │         │    │                  channels from rules
     │         │  match?                    │
     │         │  yes │  no                 │
     │         │      └──► fallback ───────►│
     │         │                           │
     │         │              ┌────────────▼────────────┐
     │         │              │          Pending         │
     │         │              │   notification sent to   │
     │         │   auto-grant │   operator channel(s)    │
     │         │      │       └────────────┬────────────┘
     │         │      │                    │ wait (default: 5 min)
     │         │      │          ┌─────────┴─────────┐
     │         │      │          │                   │
     │         │      │     operator            timeout
     │         │      │     decides              │
     │         │      │   grant/deny    next escalation level?
     │         │      │       │          yes │       no │
     │         │      │       │           notify    on_exhausted
     │         │      │       │           next       action:
     │         │      │       │           level     AutoDeny /
     │         │      │       │           (loop)    AutoGrant /
     │         │      │       │                     Expire
     │         │      │       │
     │         └──────┤       │
     │                │       │
     └────────────────┼───────┘
                      │
           ┌──────────┴──────────┐
           │                     │
         Grant                  Deny
       (one-time /            agent tool call
        session /             returns:
        persistent)           permission_denied
           │                  agent handles
       store grant            gracefully
       unblock agent
       write audit
       mark stale
       notifications
```

---

## Integration points

### `sera-core` integration

`HitlService` is instantiated in `sera-core`'s `AppState` and called from:

- `RuntimeToolExecutor` — checks enforcement mode, submits `CapabilityGrant` requests
- `PermissionRequestHandler` — REST route `POST /api/agents/:id/permission-request`
- `DelegationRequestHandler` — REST route `POST /api/agents/:id/delegation-request`
- `KnowledgeGitService` — submits `KnowledgeMerge` requests before merging branches

### Decision endpoints

Operator decisions arrive via:

- `POST /api/permission-requests/:id/decision` — UI submission
- `POST /api/actions/approve` / `POST /api/actions/deny` — channel action tokens (Story 18.2)

Both paths call `HitlService::resolve(request_id, decision)`, which advances the state machine
and unblocks the waiting agent.

### Audit trail

Every state transition emits an audit event:

| Transition | Audit action |
|---|---|
| Pending → Notified | `hitl.requested` |
| Notified → Escalating | `hitl.escalated` |
| Any → Approved | `hitl.granted` |
| Any → Denied | `hitl.denied` |
| Any → Expired | `hitl.expired` |
| Autonomous auto-grant | `hitl.auto_granted` |
| Permissive violation | `hitl.violation_permissive` |
| AuditOnly violation | `hitl.violation_audit` |

All audit records include the `ActingContext` (from `sera-domain`) carrying the full
`delegationChain` for the requesting agent.

---

## See also

- `docs/ARCHITECTURE.md` → Dynamic permission grants, Agent Identity & Delegation
- `docs/epics/03-docker-sandbox-and-lifecycle.md` → Story 3.9 (PermissionRequestService)
- `docs/epics/17-agent-identity-and-delegation.md` → Stories 17.1–17.4 (delegation HitL)
- `docs/epics/18-integration-channels.md` → Story 18.2 (actionable HitL notifications)
- `rust/crates/sera-domain/src/` — `ActingContext`, `DelegationLink`, `DelegationScope` types (planned; not yet implemented)
- `rust/crates/sera-db/src/operator_requests.rs` — `OperatorRequestRepository`

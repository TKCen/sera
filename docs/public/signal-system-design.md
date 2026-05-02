# SERA 2.0 Signal System Design

**Status:** Final — Sera 2.0 Lead  
**Date:** 2026-04-19

---

## Guiding Principles

1. **Agents are alive, not tasks** — signals are how they communicate alive-state, not just completion
2. **Tiered delivery** — completion signals can be routed, but BLOCKED/REVIEW always reach a human
3. **Artifact-first** — full results stored as artifacts; signals carry metadata + summary
4. **Configurable per dispatch** — `deliver_to` is a first-class field on every dispatch

---

## Signal Types

```rust
// sera-types/src/signal.rs  (new file)
enum Signal {
    // Terminal states
    Done { artifact_id: String, summary: String, duration_ms: u64 },
    Failed { artifact_id: String, error: String, retries: u8 },

    // Attention states — cannot be silenced (see SignalTarget invariant)
    Blocked { reason: String, requires: Vec<Capability> },
    Review { artifact_id: String, prompt: String },

    // Lifecycle states
    Started { task_id: String, description: String },
    Progress { task_id: String, pct: u8, note: String },
    Handoff { from_agent: String, to_agent: String, artifact_id: String },
}
```

---

## SignalTarget — Delivery Routing

```rust
enum SignalTarget {
    MainSession,   // Push into the dispatching agent's active context (inbox)
    ArtifactOnly,  // Store result; agent pulls on demand
    Silent,        // Fire-and-forget; result stored only
}
```

**Invariant:** `Blocked` and `Review` signals ALWAYS route to `sera-hitl` regardless of `SignalTarget`. They cannot be silenced.

---

## Dispatch Call Shape

```rust
struct Dispatch {
    task: Task,
    deliver_to: SignalTarget,
    signal_on: Vec<SignalType>,   // which signals to actually transmit
    timeout: Option<Duration>,
    retry_policy: RetryPolicy,
}
```

---

## Cron Integration

Crons carry the same `deliver_to` flag at creation time:

```rust
struct CronSpec {
    prompt: String,
    schedule: Schedule,
    deliver_to: SignalTarget,
    always_alert: Vec<SignalType>,  // BLOCKED/REVIEW — always, ignores deliver_to
}
```

The existing `deliver="discord:1492957434690670713"` pattern maps to `SignalTarget::MainSession` with Discord as the outbound transport via `notification_service.rs` (`NotificationChannel::Webhook`).

---

## Message Routing — Cross-Agent Results

When Agent A dispatches Agent B:

1. A calls `dispatch(B, task, deliver_to=MainSession)`
2. B works, writes artifact to shared store
3. B sends `Signal::Done` → A's inbox (push via NDJSON event stream)
4. A's session receives signal + artifact summary in its turn stream
5. A pulls full artifact on demand

**Handoff pattern** (Gastown-style convoy):
- `Signal::Handoff { from: A, to: B, artifact }` published on the event bus
- B acknowledges with `Signal::Started`
- On completion B → A: `Signal::Done`

---

## Resolved Design Decisions

### 1. Inbox model — Push, with durable fallback

Use push over the existing NDJSON event stream (runtime → gateway, `sera-runtime/src/main.rs`). The `DelegationOrchestrator` (`sera-runtime/src/delegation.rs`) already polls a `status_rx: watch::Receiver<SubagentStatus>` channel — signal delivery is layered on top of the same channel. If the dispatching agent is offline (no active NDJSON connection), signals are written to the inbox table in SQLite and delivered on next session resume. No polling loop required on the hot path.

### 2. Signal persistence — SQLite inbox table, 30-day retention

Signals are stored in `sera-db` alongside the existing schema (`sqlite_schema.rs` / `init_all`). A new `SqliteSignalStore::init_schema` call is added to `init_all`. Schema:

```sql
CREATE TABLE IF NOT EXISTS agent_signals (
    id          TEXT PRIMARY KEY,
    to_agent_id TEXT NOT NULL,
    signal_type TEXT NOT NULL,       -- "Done", "Failed", "Blocked", etc.
    payload     TEXT NOT NULL,       -- JSON
    delivered   INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL,    -- Unix seconds
    expires_at  INTEGER NOT NULL     -- created_at + 30 days
);
CREATE INDEX IF NOT EXISTS idx_signals_to_agent ON agent_signals(to_agent_id, delivered);
```

Signals for `ArtifactOnly` / `Silent` targets skip the inbox row entirely — only the artifact is stored.

### 3. Rate limiting — Token bucket per dispatching agent

If an agent dispatches N sub-agents concurrently, signals are delivered individually as they arrive (no mandatory batching). A token-bucket rate limiter in `sera-gateway/src/services/notification_service.rs` caps inbound signal delivery at **60 signals/minute per agent** (configurable via `SERA_SIGNAL_RATE_LIMIT`). Signals that exceed the bucket are queued in the SQLite inbox, not dropped.

---

## Wire-Up Map

| Concern | Crate | File |
|---|---|---|
| `Signal` enum + `SignalTarget` | `sera-types` | `src/signal.rs` (new) |
| `Dispatch` struct | `sera-types` | `src/signal.rs` (new) |
| Agent inbox + session delivery | `sera-runtime` | `src/session_manager.rs` |
| Delegation status channel | `sera-runtime` | `src/delegation.rs` |
| Dispatch HTTP endpoint | `sera-gateway` | `src/bin/sera.rs` + `src/services/orchestrator.rs` |
| Rate limiting + fan-out | `sera-gateway` | `src/services/notification_service.rs` |
| Cron `deliver_to` field | `sera-gateway` | `src/services/schedule_service.rs` |
| `Blocked` / `Review` routing | `sera-hitl` | `src/router.rs` + `src/ticket.rs` |
| Signal persistence | `sera-db` | `src/signals.rs` (new) + `src/sqlite_schema.rs` |
| Real-time push | `sera-events` | `src/centrifugo.rs` (optional — Centrifugo is non-required infrastructure) |

**Note on `sera-hitl`:** The crate is thinner than its name implies — `src/router.rs` and `src/ticket.rs` exist but implement approval routing via `ApprovalSpec` / `ApprovalRouting`, not generic signal routing. `Blocked` and `Review` signals map to `ApprovalScope::SessionAction` and feed into the existing `ApprovalSpec` path. No new HITL types are needed.

**Note on `sera-events`:** Centrifugo is optional infrastructure. The primary push path is the NDJSON stream already in `sera-runtime`. Centrifugo is the upgrade for multi-pod fan-out.

---

## Failure Modes

| Scenario | Behavior |
|---|---|
| **Agent offline when signal arrives** | Signal written to `agent_signals` inbox (SQLite). Delivered on next session resume. TTL = 30 days. |
| **Gateway crash mid-dispatch** | `DelegationOrchestrator` timeout fires; caller gets `DelegationResponse::Timeout`. Signal for the timed-out task is not written (dispatcher died). Calling agent must detect timeout and retry or escalate. |
| **Dispatching agent dies mid-task** | Sub-agent runs to completion; `Signal::Done` written to inbox. Inbox row persists until TTL expiry. Next agent session that owns the same `to_agent_id` drains the inbox on resume. |
| **`Blocked` / `Review` with no HITL reviewer** | `sera-hitl` escalates to `ApprovalRouting::Static` fallback chain (configured at deploy time). If chain is empty, ticket is parked and the agent session is suspended until manual intervention. |
| **Rate limit exceeded** | Signals queued in SQLite inbox, not dropped. Delivery resumes when the token bucket refills. Bucket size and refill rate are operator-configurable. |

---

## Implementation Order

1. Add `sera-types/src/signal.rs` — `Signal`, `SignalTarget`, `Dispatch`, `CronSpec.deliver_to`
2. Add `sera-db/src/signals.rs` — `SqliteSignalStore`, schema, CRUD
3. Wire `SqliteSignalStore::init_schema` into `sqlite_schema.rs::init_all`
4. Add inbox drain on session resume in `sera-runtime/src/session_manager.rs`
5. Add dispatch endpoint + rate limiter in `sera-gateway`
6. Map `Blocked`/`Review` → `ApprovalSpec` in `sera-hitl/src/router.rs`
7. Add `deliver_to` to `CronSpec` in `sera-gateway/src/services/schedule_service.rs`

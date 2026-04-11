# Discord-Like Internal Introspection UI — Design Assessment

**Date:** April 2026  
**Audience:** Operators monitoring agent activity, circles, and platform health  
**Scope:** Web-only implementation using existing Centrifugo channels and core APIs; no core changes required

---

## 1. Current State Summary: Real-Time Data Already Flowing

The SERA platform publishes real-time events to Centrifugo (WebSocket broker at `ws://hostname:10001/connection/websocket`) across six distinct channel families. All are accessible from the web client with a valid Centrifugo connection token. No core modifications are needed to tap into this data.

### Real-Time Channels Currently Active

| Channel Family        | Pattern                   | Source                               | Frequency             | Payload Type    |
| --------------------- | ------------------------- | ------------------------------------ | --------------------- | --------------- |
| **Agent Thoughts**    | `thoughts:{agentId}`      | IntercomService.publishThought()     | Per reasoning step    | ThoughtPayload  |
| **LLM Tokens**        | `tokens:{agentId}`        | IntercomService.publishToken()       | Per token (~50-100ms) | TokenPayload    |
| **Agent Status**      | `agent:{agentId}:status`  | IntercomService.publishAgentStatus() | Lifecycle events      | StatusPayload   |
| **Direct Messages**   | `private:{fromId}:{toId}` | IntercomService.sendDirectMessage()  | On-demand             | IntercomMessage |
| **Circle Broadcasts** | `circle:{circleId}`       | IntercomService.broadcastToCircle()  | On-demand             | IntercomMessage |
| **System Events**     | `system:events`           | IntercomService.publishSystem\*()    | Platform-wide         | SystemEvent     |

### Message Envelope Format (All Channels)

All Centrifugo messages use the **IntercomMessage** standardized envelope:

```typescript
interface IntercomMessage {
  id: string; // UUID, unique per message
  timestamp: number; // Unix ms
  sourceAgent: string; // agentId or "system"
  targetAgent?: string; // For direct messages
  type: string; // "thought", "token", "status", etc.
  payload: unknown; // Type-specific data
  metadata?: {
    circleId?: string; // For circle-scoped messages
    conversationId?: string; // For multi-turn threads
    delegationId?: string; // For delegation tracking
  };
}
```

---

## 2. Channel Inventory: Subscription Map & Message Shapes

### 2.1 Agent Observability Channels

#### `thoughts:{agentId}`

**What it shows:** Real-time agent reasoning steps, tool calls, decision points.  
**Who publishes:** Core (IntercomService.publishThought)  
**Persistence:** Thoughts with UUID agent IDs are stored in DB; web can also fetch historical with `/api/intercom/channels/{channel}/history`.

**ThoughtPayload:**

```typescript
{
  timestamp: number;            // When the thought occurred
  stepType: "reasoning" | "planning" | "tool_call" | "observation";
  content: string;              // Human-readable description
  agentId: string;              // Source agent UUID
  agentDisplayName: string;     // Agent's human name
  toolName?: string;            // If stepType === "tool_call"
  toolArgs?: Record<string, any>; // Tool arguments if available
  toolCallId?: string;          // Trace ID for tool execution
}
```

**Usage in Web:** Subscribe in `useChatPage.ts` pattern; displayed in agent activity sidebar or detail view. Correlate multiple agents to build composite timeline.

---

#### `tokens:{agentId}`

**What it shows:** Streaming LLM token delivery for real-time text generation visibility.  
**Who publishes:** Core (IntercomService.publishToken)  
**Latency:** ~50-100ms per token (LLM streaming speed).

**TokenPayload:**

```typescript
{
  token: string;               // Single token or chunk
  done: boolean;               // True when LLM response complete
  messageId?: string;          // Linked to specific message
  error?: string;              // If generation failed
}
```

**Usage in Web:** Accumulate tokens into live text display. When `done: true`, lock the message and move to next. Gracefully handle `error` by highlighting failed generation.

---

#### `agent:{agentId}:status`

**What it shows:** Agent lifecycle events (startup, shutdown, delegation grant/revoke, error).  
**Who publishes:** Core (IntercomService.publishAgentStatus)  
**Frequency:** Low (lifecycle events only).

**StatusPayload:**

```typescript
{
  status: "online" | "offline" | "error" | "paused";
  reason?: string;             // Human explanation
  timestamp: number;
  context?: {
    errorType?: string;
    delegationCount?: number;  // Active delegations
    circleMembers?: number;    // If in circle
  };
}
```

**Usage in Web:** Status indicator in agent list; "online now" / "offline 5 min ago" badges.

---

### 2.2 Direct Communication Channels

#### `private:{fromAgentId}:{toAgentId}`

**What it shows:** Agent-to-agent direct messages (conversation-like).  
**Who publishes:** Core (IntercomService.sendDirectMessage) after permission validation.  
**Visibility:** Private to the two agents + operator (if operator has read permission).

**Message Format:** Full IntercomMessage envelope with:

```typescript
{
  sourceAgent: fromAgentId;
  targetAgent: toAgentId;
  type: "direct_message" | "delegation_request" | "task_delegation";
  payload: {
    content: string;
    conversationId?: string;   // Thread ID for multi-turn
    requestId?: string;        // For tracked responses
  };
}
```

**Usage in Web:** DM thread view between any two agents. Operator can monitor specific agent-pair conversations or watch all DMs in a circle.

---

### 2.3 Circle Channels

#### `circle:{circleId}`

**What it shows:** Circle-wide broadcasts and multi-agent discussions (party mode).  
**Who publishes:** Core (IntercomService.broadcastToCircle or PartyMode orchestrator).  
**Membership:** Validated at publish time; only circle members receive tokens.

**Message Format:** IntercomMessage with metadata.circleId set.

**Sub-patterns within circle broadcasts:**

| Event Type           | Example                                                             | When Sent                                  |
| -------------------- | ------------------------------------------------------------------- | ------------------------------------------ |
| **Party Mode Start** | `{ type: "party_start", payload: { sessionId, agents, strategy } }` | Circle operator initiates group discussion |
| **Party Mode Round** | `{ type: "party_round", payload: { round, agentId, message } }`     | Agent speaks in turn                       |
| **Party Mode End**   | `{ type: "party_end", payload: { sessionId, summary } }`            | Max rounds or exit keyword reached         |
| **Circle Broadcast** | `{ type: "broadcast", payload: { content, sender } }`               | Ad-hoc message to all members              |

**Usage in Web:** Thread view for circle conversations; "party mode in progress" indicator; transcripts of multi-agent discussions.

---

### 2.4 Platform-Wide Channels

#### `system:events`

**What it shows:** Platform-level events (migrations, config changes, operator actions, delegation audits).  
**Who publishes:** Core (IntercomService.publishSystem, publishSystemEvent).  
**Frequency:** Medium (application-dependent).

**SystemEventPayload:**

```typescript
{
  eventType: string;           // e.g., "delegation_granted", "circle_created", "agent_crash"
  timestamp: number;
  actor?: string;              // Operator or system
  resourceId?: string;         // agentId, circleId, etc.
  details: Record<string, any>; // Event-specific fields
  severity: "info" | "warning" | "error";
}
```

**Common Event Types:**

- `delegation_granted` — Operator granted scope to agent
- `delegation_revoked` — Operator revoked scope (cascade included)
- `circle_created` / `circle_deleted`
- `agent_registered` / `agent_removed`
- `party_session_started` / `party_session_ended`
- `config_updated`
- `error:agent_crash` — Unhandled exception in agent runtime
- `error:auth_denied` — Permission denied on message send

**Usage in Web:** Audit log / activity feed; filter by severity and resource type.

---

### 2.5 Advanced: Cross-Circle Federation (Future)

#### `bridge:dm:{circleA}:{circleB}:{agentA}:{agentB}`

**What it shows:** DM traffic between agents in different circles (cross-circle delegation).  
**Current Status:** Pattern defined in ChannelNamespace; publish implementation planned.  
**Note:** Do not subscribe yet — not actively publishing in v1.

---

## 3. Gap Analysis: Core Changes vs. Available Infrastructure

### What's Already Available (No Core Changes Needed)

| Feature                                     | Status       | Evidence                                                                   |
| ------------------------------------------- | ------------ | -------------------------------------------------------------------------- |
| Real-time agent thought streaming           | ✅ Live      | `thoughts:{agentId}` channel active; `useChatPage.ts` already subscribes   |
| Real-time LLM token delivery                | ✅ Live      | `tokens:{agentId}` channel active; token accumulation logic in chat UI     |
| Agent status lifecycle events               | ✅ Live      | `agent:{agentId}:status` published by IntercomService                      |
| Circle broadcasts & multi-agent discussions | ✅ Live      | `circle:{circleId}` channel; PartyMode orchestration in core               |
| Direct agent-to-agent messaging             | ✅ Live      | `private:{fromId}:{toId}` with permission validation                       |
| Platform system events                      | ✅ Live      | `system:events` channel with audit event types                             |
| Channel history retrieval                   | ✅ Available | `/api/intercom/channels/{channel}/history` endpoint exists                 |
| Channel listing                             | ✅ Available | `/api/intercom/channels` enumerates all active channels                    |
| Delegation audit trail                      | ✅ Live      | Delegation events published to `system:events`; cascade revocation tracked |
| Circle context & membership                 | ✅ Live      | `/circles` and `/circles/{name}` API endpoints provide full metadata       |

### What Requires Core Changes (To Avoid)

| Feature                                                      | Reason                                             | Alternative                                            |
| ------------------------------------------------------------ | -------------------------------------------------- | ------------------------------------------------------ |
| Custom channel creation from web                             | Would require web→core API + security model        | Subscribe only to ChannelNamespace-defined patterns    |
| Selective message filtering/sampling                         | Would add computation to core                      | Implement client-side filtering in web UI              |
| Message encryption/signing                                   | Would require cryptographic infrastructure         | Rely on WebSocket TLS + JWT token validation           |
| Real-time metric aggregation (e.g., "agents processing now") | Would require stateful collection in core          | Derive from subscription lifecycle + thought frequency |
| Operator impersonation of agents                             | Would require dangerous delegation scope expansion | Use explicit delegation grants only                    |

---

## 4. Recommended Discord-Like Structure

### Mapping SERA Concepts to Discord Metaphor

| Discord Concept         | SARA Entity                            | Implementation                                                                                              |
| ----------------------- | -------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| **Server**              | Circle                                 | Each circle is a "server" with its own namespace, members, and shared context                               |
| **Text Channel**        | Circle broadcast channel               | `circle:{circleId}` subscription shows all messages to that circle                                          |
| **Direct Message (DM)** | Private agent DM                       | `private:{fromId}:{toId}` subscriptions; operator can view any agent's DMs                                  |
| **Thread**              | Party mode session or delegation chain | Party mode creates ephemeral thread-like structure; delegation grants create visible request/approval chain |
| **Global Feed**         | System events                          | `system:events` channel shows platform activity (user actions, config changes, errors)                      |
| **Activity Sidebar**    | Agent timeline                         | Correlate `thoughts:{agentId}`, `tokens:{agentId}`, and status updates across all subscribed agents         |
| **User Profile**        | Agent Detail Page                      | Agent capabilities, active delegations, circle membership, recent activity                                  |
| **Message Reactions**   | (Not mapped)                           | N/A — focus on monitoring, not collaboration                                                                |
| **Voice**               | (Not mapped)                           | Out of scope for v1                                                                                         |

### Proposed UI Structure

```
┌─────────────────────────────────────────────────────────┐
│  SERA Introspection Dashboard                           │
├──────────────────┬──────────────────────────────────────┤
│ Left Sidebar     │ Main Content Area                    │
│                  │                                      │
│ Circles (expand) │ Selected Chat or Feed               │
│ ├─ Research      │                                      │
│ ├─ Ops           │  [Real-time messages/thoughts]      │
│ └─ Admin         │                                      │
│                  │  [Live token generation]            │
│ Agents (grid)    │                                      │
│ ├─ Alice (●)     │  [Message input if operator role]   │
│ ├─ Bob (●)       │                                      │
│ └─ Charlie (○)   │                                      │
│                  │                                      │
│ System Events    │                                      │
│ (activity feed)  │                                      │
│                  │                                      │
│ DM Conversations │                                      │
│ ├─ Alice → Bob   │                                      │
│ └─ Bob → Charlie │                                      │
│                  │                                      │
└──────────────────┴──────────────────────────────────────┘
```

### Channel Subscription Strategy (Web-Only)

**On dashboard load:**

1. Fetch all circles: `GET /circles`
2. For each active circle, subscribe to `circle:{circleId}`
3. For each agent in circles, subscribe to `thoughts:{agentId}` and `agent:{agentId}:status`
4. Subscribe to `system:events` for global audit log
5. For operator, subscribe to all `private:{*}:{*}` patterns they have permission to view

**On user click (agent detail view):** 6. Subscribe to `tokens:{agentId}` and `agent:{agentId}:status` (if not already) 7. Fetch `/api/intercom/channels/thoughts:{agentId}/history` for scrollback

**On circle detail view:** 8. Fetch circle metadata: `GET /circles/{circleId}` 9. Fetch `circle:{circleId}` channel history: `GET /api/intercom/channels/circle:{circleId}/history`

---

## 5. Web-Only Implementation Plan: No Core Changes

### Phase 1: Foundation (Weeks 1-2)

**Goal:** Multi-agent timeline with real-time thought streaming.

**Components to Build:**

1. **AgentActivityTimeline** — Render `ThoughtPayload` items from all subscribed agents in chronological order
   - Input: array of subscribed agent IDs
   - Subscriptions: `thoughts:{agentId}` for each
   - Display: timestamp, agentId, stepType, content, tool calls

2. **AgentStatusIndicator** — Show agent online/offline state
   - Subscriptions: `agent:{agentId}:status`
   - Display: green/red dot; "online", "offline 5 min ago", error state

3. **LiveTokenDisplay** — Accumulate and display LLM token stream for selected agent
   - Subscriptions: `tokens:{agentId}` (on-demand when viewing detail)
   - Display: live text generation; "thinking...", "done" states

**Entry Point:** New dashboard route `/dashboard/introspection` (or `/circles?view=introspection`)

**No core changes:** All subscriptions to existing Centrifugo channels.

---

### Phase 2: Circle & System Context (Weeks 3-4)

**Goal:** Organize timeline by circle; add system event feed.

**Components to Build:**

1. **CircleChatView** — Thread-like display of circle broadcasts and party mode sessions
   - Subscriptions: `circle:{circleId}`
   - Display: who said what, in order; detect party mode and show "round X of Y"

2. **SystemEventFeed** — Audit log of platform activity
   - Subscriptions: `system:events`
   - Display: operator actions, delegations granted/revoked, circle created/deleted
   - Filters: by severity, by resource type, by time range

3. **CircleSelector** — Left sidebar list of circles; click to view
   - Data source: `GET /circles`
   - Display: circle name, member count, status

**No core changes:** All data from existing channels and APIs.

---

### Phase 3: Agent Conversations & DM Monitoring (Weeks 5-6)

**Goal:** Show all direct messages between agents; let operator monitor specific pairs.

**Components to Build:**

1. **AgentDMThread** — Conversation view between two agents
   - Subscriptions: `private:{agent1}:{agent2}`
   - Display: conversation timeline; operator can see requests and responses

2. **DMConversationList** — Left sidebar showing all DM pairs with unread badges
   - Logic: track all `private:{*}:{*}` subscriptions
   - Display: "Alice ↔ Bob", "Bob ↔ Charlie", etc.

3. **ConversationSearcher** — Filter/search conversations by agent name or keyword
   - Data source: In-memory map of subscribed DM channels
   - Display: quick-jump to conversation

**No core changes:** Reading existing `private:{*}:{*}` channels.

---

### Phase 4: Delegation & Authority Tracking (Weeks 7-8)

**Goal:** Show delegation grant chains; visualize permission hierarchy.

**Components to Build:**

1. **DelegationTimeline** — Show each delegation grant/revoke event and cascade impact
   - Data source: `system:events` filtered to `delegation_*` types
   - Display: "Operator Alice granted Agent Bob scope X at 2:34pm", "Revoked (cascaded to 3 sub-tokens)"

2. **AgentCapabilities** — Per-agent view of active delegations and scope
   - Data source: Aggregate `system:events` in real-time
   - Display: list of scopes, grant time, expiry if applicable

3. **DelegationAuditLog** — Full audit trail with filters
   - Data source: `system:events` for `delegation_*` events
   - Display: sortable, filterable table; export to CSV

**No core changes:** Reading delegation events from `system:events`.

---

### Implementation Checklist

- [ ] Add CentrifugoContext + useCentrifugo hook to web package (already exists; extend if needed)
- [ ] Build AgentActivityTimeline component (parallel render from multiple `thoughts:*` subscriptions)
- [ ] Build AgentStatusIndicator component
- [ ] Build LiveTokenDisplay component (reuse logic from current useChatPage.ts)
- [ ] Create `/dashboard/introspection` route
- [ ] Build CircleChatView and CircleSelector components
- [ ] Build SystemEventFeed component
- [ ] Build AgentDMThread and DM conversation list
- [ ] Build DelegationTimeline and DelegationAuditLog
- [ ] Test with 5+ agents and 3+ circles in dev environment
- [ ] Document new components in `web/CLAUDE.md`

### Tech Stack (Existing)

- **React 19** (web framework)
- **TanStack Query** (data fetching, state)
- **Centrifugo** (WebSocket subscription library)
- **Tailwind CSS** (styling)
- **TypeScript** (type safety)

### Performance Considerations

1. **Thought & Token Accumulation:** Use circular buffer (max 1000 items) per agent to cap memory.
2. **Subscription Cleanup:** Unsubscribe from `tokens:{agentId}` when detail view closes.
3. **System Events Volume:** Paginate system feed; show last 100 events; lazy-load older on scroll.
4. **DM Channel Explosion:** For many agents, avoid subscribing to all `private:*:*` at once; only subscribe on explicit request or when operator opens DM view.

---

## 6. Dependencies & Blockers

### None

All required channels are actively publishing in the current core codebase. No feature flags, config changes, or core API modifications are needed.

### Assumptions

1. **Centrifugo is running and accessible** at configured WebSocket endpoint.
2. **Operator has valid JWT token** from `/api/delegation/token` or similar auth endpoint (already implemented in core).
3. **Agent IDs are predictable** (UUID format or registered name).
4. **Circle IDs are predictable** (URL-friendly slugs or UUIDs).

---

## 7. Risk Assessment

| Risk                                                 | Probability | Mitigation                                                                                                                     |
| ---------------------------------------------------- | ----------- | ------------------------------------------------------------------------------------------------------------------------------ |
| Centrifugo connection dropout during monitoring      | Medium      | Implement auto-reconnect with exponential backoff; show "disconnected" state; buffer missed events if within reasonable window |
| Too many simultaneous subscriptions (10+ agents)     | Low         | Use selective agent subscription (only agents in viewed circle); lazy-load detail views                                        |
| Message ordering issues in high-throughput scenarios | Low         | Use `timestamp` field in IntercomMessage for client-side sorting; prefer chronological ordering over arrival order             |
| Operator sees stale delegation state                 | Low         | Refresh delegation audit log on page focus; subscribe to real-time revocation events                                           |
| Browser memory explosion from long sessions          | Medium      | Implement automatic cleanup: drop messages older than 1 hour; paginate feeds; use virtual scrolling for long lists             |

---

## 8. Success Criteria

- [ ] Operator can see all agents in all circles on a single dashboard
- [ ] Real-time thought streaming displays with <2s latency
- [ ] Agent status (online/offline) updates within 5s of status change
- [ ] Circle broadcasts appear in dedicated thread views
- [ ] System event feed shows all delegation and configuration changes
- [ ] DM conversations between agents are visible and filterable
- [ ] Introspection UI works for 10+ agents and 5+ circles without performance degradation
- [ ] No core changes required; all data sourced from existing channels and APIs

---

## 9. Next Steps

1. **Validate this assessment** with platform team
2. **Create detailed Figma wireframes** for each phase
3. **Schedule Phase 1 implementation** (AgentActivityTimeline + CircleSelector + SystemEventFeed)
4. **Define component API contracts** in TypeScript interfaces
5. **Plan test scenarios** (agent startup/shutdown, circle creation, delegation revocation)

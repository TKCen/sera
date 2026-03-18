# Epic 09: Real-Time Messaging

## Overview

SERA's real-time layer is built on Centrifugo — a purpose-built WebSocket pub/sub server. This powers three distinct communication patterns: agent thought streaming to the operator UI, agent-to-agent intercom, and system events. The messaging architecture is also the foundation for future federation between SERA instances. Getting the channel namespace design right now avoids breaking changes later.

## Context

- See `docs/ARCHITECTURE.md` → Real-Time Messaging, Circles & Multi-Agent Coordination
- Centrifugo runs as a standalone container; sera-core publishes via Centrifugo's HTTP API
- Agent runtimes also publish directly to Centrifugo (thought streams) using the API URL + key injected at spawn
- The browser subscribes to channels via the Centrifugo WebSocket endpoint (not via sera-core)
- Channel naming is a stable public contract — changes break client subscriptions

## Dependencies

- Epic 01 (Infrastructure) — Centrifugo running
- Epic 03 (Docker Sandbox) — `CENTRIFUGO_API_URL` and `CENTRIFUGO_API_KEY` injected into containers
- Epic 05 (Agent Runtime) — runtime publishes thoughts and tokens

---

## Stories

### Story 9.1: Centrifugo channel namespace design

**As** the SERA system
**I want** a well-defined, versioned channel namespace
**So that** all publishers and subscribers use consistent channel names and the design supports future federation

**Acceptance Criteria:**
- [ ] Channel namespaces documented in `docs/messaging/CHANNELS.md`
- [ ] Namespaces defined in Centrifugo config with appropriate settings per namespace
- [ ] Defined channels:
  - `thoughts:{agentId}:{agentName}` — thought steps (observe/plan/act/reflect)
  - `tokens:{agentId}` — LLM token stream
  - `agent:{agentId}:status` — lifecycle events (started/stopped/error)
  - `private:{sourceAgentId}:{targetAgentId}` — agent-to-agent direct message
  - `circle:{circleId}` — circle-wide broadcast
  - `system.agents` — agent registry events (created/updated/deleted)
  - `system.providers` — LLM provider events (added/removed/circuit-open)
  - `system.tools` — tool registry events
- [ ] `thoughts` namespace: history enabled (last 100 messages), TTL 1 hour
- [ ] `tokens` namespace: no history (stream only), presence disabled
- [ ] `private` namespace: requires subscription token (not open to all UI clients)
- [ ] Channel naming documented as a stable v1 contract

**Technical Notes:**
- Channel names containing agent IDs use the UUID, not the display name — stable across renames
- `agentName` included in thought channels for human readability in logs only

---

### Story 9.2: IntercomService

**As** sera-core
**I want** a typed service for publishing to Centrifugo channels
**So that** all publishing is consistent, authenticated, and observable

**Acceptance Criteria:**
- [ ] `IntercomService` wraps Centrifugo HTTP API with typed methods
- [ ] `publishThought(agentId, thought: Thought)` publishes to `thoughts:{agentId}:{name}` channel
- [ ] `publishToken(agentId, token: string, done: boolean)` publishes to `tokens:{agentId}` channel
- [ ] `publishAgentStatus(agentId, status: AgentStatus)` publishes to `agent:{agentId}:status`
- [ ] `publishSystemEvent(channel: SystemChannel, payload)` publishes to `system.*` channels
- [ ] `sendDirectMessage(sourceId, targetId, message: IntercomMessage)` publishes to `private:{sourceId}:{targetId}`
- [ ] All publishes authenticated with Centrifugo API key
- [ ] Publish failures logged as warnings but never throw — messaging is best-effort
- [ ] Message envelope includes: `timestamp`, `version: '1'`, `source: 'sera-core'`

---

### Story 9.3: Agent-to-agent direct messaging

**As an** agent
**I want** to send a message directly to another specific agent
**So that** agents can collaborate and delegate without going through a central orchestrator

**Acceptance Criteria:**
- [ ] `POST /api/intercom/message` accepts: `{ targetAgentName, message, correlationId? }` from an authenticated agent (JWT)
- [ ] Target agent resolved to `agentId` by name lookup in registry
- [ ] Caller's `agentId` from JWT — agents cannot spoof sender identity
- [ ] Permission check: caller's manifest `intercom.canMessage` list must include target name (or `*` wildcard)
- [ ] Unauthorised message attempt returns 403 with reason
- [ ] Message published to `private:{sourceAgentId}:{targetAgentId}` channel
- [ ] Agent runtime subscribes to its own `private:{agentId}:*` channels at startup
- [ ] Message payload: `{ sourceAgentId, sourceAgentName, targetAgentId, message, correlationId, timestamp }`

---

### Story 9.4: Circle broadcast channels

**As an** agent
**I want** to publish a message to all agents in my circle
**So that** I can share information with the entire team without addressing each agent individually

**Acceptance Criteria:**
- [ ] `POST /api/intercom/broadcast` accepts: `{ circleId, message }` from an authenticated agent
- [ ] Agent must be a member of the target circle (validated against manifest `metadata.circle` and `metadata.additionalCircles`)
- [ ] Message published to `circle:{circleId}` channel
- [ ] All agents subscribed to their circle channel at runtime startup
- [ ] Circle channel history: last 50 messages, TTL 4 hours

---

### Story 9.5: Subscription token issuance (secure channels)

**As** the sera-web UI
**I want** to receive a Centrifugo subscription token for agent-specific channels
**So that** only authorised UI sessions can subscribe to private or sensitive channels

**Acceptance Criteria:**
- [ ] `POST /api/centrifugo/token` issues a Centrifugo subscription token for a given channel
- [ ] Token grants access to: public channels (thoughts, tokens, system.*) for any authenticated UI session
- [ ] `private:` channels require the UI session to have explicit access to both the source and target agent
- [ ] Tokens expire after 1 hour; UI must refresh
- [ ] `GET /api/centrifugo/config` returns: `{ url: string, token: string }` — the WebSocket URL and a connection token for the browser

**Technical Notes:**
- The browser connects directly to Centrifugo for WebSocket — sera-core only issues tokens, it does not proxy WebSocket traffic
- This is the correct architecture for scalability; Centrifugo handles WebSocket fan-out

---

### Story 9.6: Federation bridge stub

**As** a SERA instance
**I want** a stub implementation of the federation bridge
**So that** the architecture supports future cross-instance messaging without a re-architecture

**Acceptance Criteria:**
- [ ] `BridgeService` class exists with `connect(remoteUrl, token)`, `disconnect()`, `route(message)` methods
- [ ] Implementation is a no-op stub that logs the intent — no actual cross-instance connection in v1
- [ ] `federation:{remoteInstance}` channel namespace reserved in Centrifugo config
- [ ] `GET /api/federation/peers` returns an empty list in v1 (structure established, feature disabled)
- [ ] Federation design documented in `docs/messaging/FEDERATION.md` as a future spec

**Technical Notes:**
- The stub exists to ensure the channel namespace and routing architecture is correct before federating instances
- Real federation implementation is a future epic; this story just ensures we don't accidentally block it

---

### Story 9.7: Thought stream persistence

**As an** operator
**I want** agent thought streams persisted to the database
**So that** I can review an agent's full reasoning history after it completes — not just in real time

**Acceptance Criteria:**
- [ ] `thought_events` table: `id` (UUID), `agent_instance_id`, `task_id` (nullable), `step` (observe/plan/act/reflect), `content` (TEXT), `iteration` (integer), `published_at` (TIMESTAMPTZ)
- [ ] sera-core intercepts all `publishThought()` calls in `IntercomService` and writes to `thought_events` before forwarding to Centrifugo
- [ ] Write is non-blocking (async, fire-and-forget) — Centrifugo publish is not delayed
- [ ] `GET /api/agents/:id/thoughts` returns paginated thought history — filterable by `taskId`, `step`, and time range; ordered by `published_at ASC`
- [ ] `GET /api/agents/:id/thoughts?taskId={id}` returns the complete reasoning trace for one task execution
- [ ] Thoughts retained for `THOUGHT_RETENTION_DAYS` (default: 14 days); older records pruned by background job
- [ ] `act` thoughts with tool arguments stored with arguments sanitised: values for keys matching `/secret|token|key|password|credential/i` replaced with `[REDACTED]`
- [ ] Thought history available in sera-web's agent detail view (Epic 13) via this endpoint

**Technical Notes:**
- Centrifugo's built-in channel history (last 100 messages, 1h TTL, Story 9.1) covers the live streaming case; this story covers the durable historical record needed for post-hoc analysis and debugging
- The `thought_events` table is the source of truth; Centrifugo history is ephemeral

---

### Story 9.8: Webhook and external event triggers

**As an** operator
**I want** to trigger agent tasks from external systems via webhooks
**So that** SERA can react to events from GitHub, monitoring systems, or other tooling without polling

**Acceptance Criteria:**
- [ ] `webhooks` table: `id` (UUID), `name`, `secret_hash` (bcrypt), `target_agent_id`, `event_filter` (JSONB — optional JSONPath or field match conditions), `task_template` (TEXT — handlebars template for the task string), `created_at`, `last_triggered_at`, `trigger_count`
- [ ] `POST /api/webhooks` creates a webhook — returns a one-time-visible webhook secret; admin/operator role required
- [ ] `GET /api/webhooks` lists webhooks (secret not returned)
- [ ] `DELETE /api/webhooks/:id` deletes a webhook
- [ ] Webhook delivery endpoint: `POST /webhooks/:id` — public endpoint (no auth header required); validates HMAC-SHA256 signature (`X-Sera-Signature` header, or `X-Hub-Signature-256` for GitHub compatibility)
- [ ] On valid delivery: evaluate `event_filter` against payload; if filter matches (or no filter set), render `task_template` with payload as context and enqueue a task for `target_agent_id`
- [ ] Replay attack prevention: `X-Sera-Timestamp` header required on all deliveries; requests with timestamp older than 5 minutes rejected with 401; short-lived nonce cache (in-memory set of `{webhookId}:{nonce}` tuples keyed on `X-Sera-Nonce` header) prevents identical requests being processed twice within the timestamp window
- [ ] On invalid signature: return 401, do not process
- [ ] Webhook delivery logged: `webhook_deliveries` table with `id`, `webhook_id`, `payload` (JSONB), `signature_valid`, `filter_matched`, `task_id` (nullable), `received_at`
- [ ] `GET /api/webhooks/:id/deliveries` lists recent deliveries with status
- [ ] `POST /api/webhooks/:id/test` sends a synthetic test payload to verify filter and template rendering — does not enqueue a real task; returns the rendered task string

**Technical Notes:**
- The public webhook endpoint is intentionally on a separate path prefix (`/webhooks/`) from the API (`/api/`) to make routing and rate limiting clearer
- GitHub webhook compatibility (`X-Hub-Signature-256`) means SERA can receive GitHub events without a translation layer

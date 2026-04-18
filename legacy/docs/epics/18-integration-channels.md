# Epic 18: Integration Channels

## Overview

SERA uses a unified **Channel** model for all ingress and egress communication. A channel is any surface through which messages enter or leave the system — Discord bots, Slack apps, webhooks, email, schedules, agent-to-agent intercom, and even the REST API itself. Every channel has a **binding mode** that determines where messages are routed: to a specific agent, to a circle (any/all agents), or outbound-only for notifications.

This unification means:
- An operator approving a permission request via Discord reply uses the same primitives as a schedule firing a task
- A circle channel on Discord and an agent-to-agent intercom message both flow through the same routing model
- Adding a new platform (Telegram, MQTT, GitHub webhooks) requires implementing one interface, not wiring into multiple subsystems

**Built-in channels** (always present, not stored in DB):
- `api` — REST API chat/task endpoints (binding per request)
- `intercom` — agent-to-agent direct messages and circle broadcasts via Centrifugo
- `schedule` — cron/one-shot schedule fires (agent-bound)
- `webhook` — inbound HTTP POST to `/api/webhooks/incoming/:slug`

**External channels** (operator-configured, stored in DB):
- `discord` — bot with gateway connection, slash commands, threaded chat
- `slack` — app with interactive messages and slash commands
- `email` — SMTP outbound (notification mode only in v1)
- `webhook-outbound` — generic HTTP POST with HMAC signatures

## Context

- The `Channel` interface is the universal ingress/egress primitive — subsumes Story 9.8 (webhooks), schedule triggers, intercom, and external platform adapters
- All HitL flows (Story 3.9 permission requests, Story 17.4 delegation requests) generate egress events routed through channels
- All work triggers (chat, tasks, schedule fires, external messages) are ingress events routed through channels
- The `Channel` interface is a plugin surface (Epic 15) — community adapters for new platforms can be published without core changes
- External channel adapters run in sera-core's process (not in containers) — they are trusted integration code
- Built-in channels (api, intercom, schedule, webhook) implement the same interface but are not stored in the DB

## Dependencies

- Epic 01 (Infrastructure) — sera-core running
- Epic 03 (Docker Sandbox) — permission request events (Story 3.9)
- Epic 09 (Real-Time Messaging) — Centrifugo events that trigger outbound notifications
- Epic 15 (Plugin SDK) — channel adapters as plugins
- Epic 17 (Agent Identity & Delegation) — delegation request events (Story 17.4)

---

## Stories

### Story 18.1: Channel interface and event routing

**As** sera-core
**I want** a unified `Channel` interface for all ingress and egress communication
**So that** every message entering or leaving SERA flows through the same routing model — whether it's a Discord chat, a schedule firing, an agent-to-agent message, or a REST API call

**Acceptance Criteria:**
- [ ] `Channel` TypeScript interface — unified ingress/egress primitive:
  ```typescript
  interface Channel {
    id: string
    type: string           // 'api' | 'intercom' | 'schedule' | 'webhook' | 'discord' | 'slack' | 'email'
    name: string           // human label e.g. 'ops-discord', 'eng-circle-bot', 'builtin:api'
    bindingMode: BindingMode
    builtin: boolean       // true for api, intercom, schedule, webhook — not stored in DB

    // Egress: push events out (notifications, responses)
    send(event: ChannelEvent): Promise<void>
    canHandle(eventType: string): boolean

    // Lifecycle
    healthCheck(): Promise<boolean>
    start?(): Promise<void>   // for channels with persistent connections (bots, gateways)
    stop?(): Promise<void>
  }

  // How the channel binds to SERA's agent/circle model
  type BindingMode =
    | { type: 'agent'; agentId: string }       // messages route to a specific agent
    | { type: 'circle'; circleId: string }     // messages route to a circle (responder determined by circle config)
    | { type: 'notification' }                 // egress only — alerts, approvals, no inbound chat
    | { type: 'dynamic' }                      // binding resolved per-message (used by builtin:api, builtin:webhook)

  // Ingress: a message arriving from any channel
  interface IngressEvent {
    channelId: string        // which channel produced this event
    channelType: string
    senderId: string         // operator sub, agent ID, platform user ID, or 'system'
    senderName: string
    message: string
    threadId?: string        // platform-native thread ID (Discord thread, Slack thread)
    sessionId?: string       // resolved SERA session ID (set by IngressRouter after channel_sessions lookup)
    metadata: Record<string, unknown>  // platform-specific context
    timestamp: string
  }

  // Egress: a SERA event routed outbound
  interface ChannelEvent {
    id: string               // deduplication key
    eventType: string        // e.g. 'permission.requested', 'agent.crashed', 'chat.response'
    sessionId?: string       // SERA session ID — enables the channel to post responses in the right thread
    agentId?: string         // responding agent — enables channel to attribute the message
    title: string
    body: string
    severity: 'info' | 'warning' | 'critical'
    actionable?: ActionPayload
    metadata: Record<string, unknown>
    timestamp: string
  }

  interface ActionPayload {
    requestId: string
    requestType: 'permission' | 'delegation' | 'knowledge-merge'
    approveToken: string
    denyToken: string
    expiresAt: string
  }
  ```

**Built-in channels (registered at startup, not in DB):**
- [ ] `builtin:api` — wraps REST API chat/task endpoints. `bindingMode: 'dynamic'` (agentId specified per request). Ingress: `POST /api/chat`, `POST /api/agents/:id/tasks`. Egress: HTTP response.
- [ ] `builtin:intercom` — wraps `IntercomService`. Ingress: agent DMs and circle broadcasts via Centrifugo. Egress: publish to Centrifugo channel. `bindingMode: 'dynamic'` (target agent/circle specified per message).
- [ ] `builtin:schedule` — wraps `ScheduleService`. Ingress: schedule fires (cron/one-shot). `bindingMode: { type: 'agent', agentId }` per schedule. No egress.
- [ ] `builtin:webhook` — wraps inbound webhook endpoint. Ingress: `POST /api/webhooks/incoming/:slug`. `bindingMode: 'dynamic'` (target resolved from webhook route table).

**External channels (stored in DB):**
- [ ] `notification_channels` table: `id` (UUID), `name`, `type`, `binding_mode` (JSONB), `config` (JSONB — encrypted), `enabled` (boolean), `created_at`
- [ ] **Multiple channels of the same type supported** — e.g. three Discord bots with different tokens, each bound to a different agent or circle

**Routing:**
- [ ] `IngressRouter.route(event: IngressEvent)` performs:
  1. Resolve target agent/circle from the channel's `bindingMode`
  2. Resolve or create SERA session: look up `channel_sessions` by `{ channelId, threadId }`; if not found, create a new chat session and insert a `channel_sessions` row. Set `event.sessionId`.
  3. Dispatch to chat (with session context for history) or task queue
- [ ] `EgressRouter.route(event: ChannelEvent)` evaluates `notification_routing_rules` and calls `channel.send()` for each matching channel. If `event.sessionId` is set, the channel adapter uses `channel_sessions` to resolve the platform thread and post the response in context.
- [ ] `notification_routing_rules` table: `id`, `eventType` (or `*` wildcard), `channelIds` (array), `filter` (JSONB), `minSeverity`
- [ ] Both routers are async and non-blocking — failures logged, never block the originating flow

**Lifecycle:**
- [ ] `ChannelManager` at startup: instantiates all built-in channels + loads enabled external channels from DB; calls `start()` on channels with persistent connections
- [ ] On shutdown: calls `stop()` on all active channels

**REST API (operator):**
- [ ] `POST /api/channels` creates an external channel — admin role required; `bindingMode` required
- [ ] `GET /api/channels` lists all channels (built-in + external; config values redacted on external)
- [ ] `DELETE /api/channels/:id` removes an external channel; calls `stop()` if active
- [ ] `POST /api/channels/:id/test` sends a test event through the channel

**sera-core MCP server (agent-accessible — see `docs/ARCHITECTURE.md` → sera-core as MCP server):**
- [ ] `channels.list` — list all channels with binding mode and health status
- [ ] `channels.get(id)` — get channel detail (config redacted for non-owned channels)
- [ ] `channels.create(type, bindingMode, config)` — create external channel; requires `seraManagement.channels.create`
- [ ] `channels.modify(id, updates)` — update channel config or binding; scope-checked (`own` or `own-circle`)
- [ ] `channels.delete(id)` — remove channel; scope-checked
- [ ] `channels.test(id)` — send test event through channel
- [ ] `routingRules.list` — list all routing rules
- [ ] `routingRules.create(eventType, channelIds, filter?, minSeverity?)` — create routing rule
- [ ] `routingRules.delete(id)` — remove routing rule
- [ ] `alertRules.list` — list alert rules with `lastFiredAt`
- [ ] `alertRules.create(name, condition, channelIds, severity, cooldownMinutes)` — create alert rule
- [ ] `alertRules.modify(id, updates)` — update alert rule
- [ ] `alertRules.delete(id)` — remove alert rule
- [ ] `alertRules.test(id)` — evaluate rule immediately and route test notification
- [ ] All MCP tools gated by `seraManagement.channels.*` capability dimension
- [ ] Sera's built-in template includes `channels.*`, `routingRules.*`, `alertRules.*` in `tools.allowed`

**Technical Notes:**
- The `bindingMode` determines ingress routing:
  - `agent` mode: inbound messages become chat/tasks for that specific agent
  - `circle` mode: inbound messages are routed to the circle; the circle's orchestration pattern or a `@agent-name` mention determines the responder
  - `notification` mode: egress only — inbound messages ignored
  - `dynamic` mode: binding resolved per-message from the message payload (used by built-in channels where the target varies per request)
- Built-in channels are thin wrappers around existing services — they don't replace `IntercomService` or `ScheduleService`; they expose them through the channel interface so routing rules and audit work uniformly
- The key insight: **a schedule firing, an agent DM, a Discord message, and a REST API call are all the same thing** — an ingress event with a sender, a message, and a target. The channel model makes this explicit.
- **Session continuity across all channels:** Every ingress/egress pair is linked by a `sessionId`. The `channel_sessions` table maps platform-native thread identifiers to SERA chat sessions. This means:
  - A Discord thread, a Slack thread, an API session, and an intercom DM conversation all produce the same session history that feeds into context assembly (Epic 8 Story 8.4)
  - Agent responses include the `sessionId` in the `ChannelEvent`, so the egress router posts the reply in the correct platform thread
  - Built-in channels bridge to their native session model: `builtin:api` uses the existing `sessions` table directly; `builtin:intercom` creates `channel_sessions` rows keyed by the Centrifugo channel name as `platformThreadId`
- Multiple Discord bots: one for ops notifications (notification mode), one for direct chat with a developer agent (agent mode), one for an engineering circle channel (circle mode). Each has its own `botToken` and `guildId`.

---

### Story 18.2: Actionable HitL notifications

**As an** operator receiving a notification on Discord or Slack
**I want** to approve or deny a permission request or delegation request by replying to the notification
**So that** I don't need to open the SERA UI for routine approvals

**Acceptance Criteria:**
- [ ] For every HitL event (`permission.requested`, `delegation.requested`, `knowledge-merge.requested`): `ChannelRouter` generates an `ActionPayload` with `approveToken` and `denyToken`
- [ ] Tokens are short-lived JWTs signed by sera-core: `{ requestId, requestType, decision: 'grant'|'deny', operatorSub, exp }`
- [ ] Token expiry matches the HitL request timeout (default: 5 min)
- [ ] `POST /api/actions/approve` and `POST /api/actions/deny` — **public endpoints** (no session auth required); validate token signature and expiry only
- [ ] On valid token: executes the decision as if the operator had submitted via the normal UI endpoint; records `actorAuthMethod: 'channel-action-token'` in audit trail
- [ ] On expired or invalid token: returns 401; operator must use the UI
- [ ] Channels embed the approve/deny URLs (or platform-native buttons) in the notification message
- [ ] After a decision is made via one channel: all other channel notifications for the same request are marked stale (a follow-up message or edit indicates the request was already resolved)
- [ ] Audit trail records which channel the decision came from: `{ actor, channel: 'discord-ops', channelType: 'discord' }`

**Technical Notes:**
- The action token model means no OAuth or session is needed for channel-based approvals — the token is the authorisation. This is intentionally limited: tokens encode only one specific decision for one specific request, not general API access.
- Token URLs should use SERA's public URL (`SERA_PUBLIC_URL` env var) — must be reachable from the operator's device

---

### Story 18.3: Webhook outbound channel

**As an** operator
**I want** to receive SERA events as HTTP POST requests to a URL I control
**So that** I can integrate SERA notifications into any system (Home Assistant, n8n, custom scripts)

**Acceptance Criteria:**
- [ ] `WebhookChannel` adapter sends `POST {url}` with JSON body: `{ event, timestamp, instanceId, signature }`
- [ ] Signature: HMAC-SHA256 of the body with a per-channel secret — same format as inbound webhooks (Story 9.8)
- [ ] `config`: `{ url, secret, timeout?: number (default: 10s), retryOnFailure?: boolean (default: true) }`
- [ ] On HTTP 4xx: log, do not retry
- [ ] On HTTP 5xx or timeout: retry once after 30s; log if second attempt also fails
- [ ] `actionable` events include `approveUrl` and `denyUrl` fields in the payload pointing to `POST /api/actions/approve|deny` with the action token embedded

---

### Story 18.4: Discord channel adapter

**As an** operator
**I want** SERA to provide full interactive chat in Discord — conversations with agents, slash commands, rich embeds, approval buttons, and streamed responses
**So that** Discord is a first-class SERA interaction surface, not just a notification sink

**Acceptance Criteria:**

**Prerequisites (manual, outside SERA):**
- The operator must create a Discord Application and Bot in the [Discord Developer Portal](https://discord.com/developers/applications) — SERA cannot do this
- The operator copies the bot token and adds the bot to their guild with the required intents (`MESSAGE_CONTENT`, `GUILD_MESSAGES`)
- The bot token is stored in SERA via `POST /api/secrets` (or Sera can store it via `sera-core/secrets.*` if delegated) — it is never exposed in plaintext after storage

**Bot setup and identity:**
- [ ] `DiscordChannel` adapter uses `discord.js` with a bot token (`config.botToken` — references a secret name, not a raw token)
- [ ] `config`: `{ botTokenSecret: string, guildId, chatChannelIds?: string[], notificationChannelId?, approvalChannelId? }`
  - `botTokenSecret` is a key in the `SecretsProvider` (e.g. `"discord-ops-bot-token"`) — resolved at `start()` time, never stored in `notification_channels.config` as plaintext
- [ ] **Multiple Discord channel instances supported** — each with its own bot token (secret ref), guild, and `bindingMode`. Common setups:
  - Bot A (agent mode) → DM or dedicated channel for chatting with a specific agent
  - Bot B (circle mode) → engineering channel where any circle member can respond
  - Bot C (notification mode) → ops channel for alerts and approvals only
- [ ] Bot registers slash commands on startup via Discord Application Commands API (scoped to `guildId`)
- [ ] Bot sets activity/status to reflect its binding (e.g. "Chatting as developer-prime" or "Engineering circle")

**Interactive chat — agent mode (`bindingMode.type: 'agent'`):**
- [ ] Any message in a bound channel or DM to the bot → routed to the bound agent as chat
- [ ] Agent response posted as a Discord embed in a thread off the original message (keeps the channel clean)
- [ ] Long responses chunked into multiple messages (Discord 2000-char limit per message)
- [ ] Thought steps posted as collapsed embed fields in the thread (type icon + summary, not full content)
- [ ] Streaming: bot edits its initial "thinking..." message as tokens arrive, updating every ~1s (avoids rate limits)
- [ ] Thread persists as a conversation — subsequent messages in the thread continue the same chat session
- [ ] No `/sera chat <agent>` needed — the agent is implicit from the binding

**Interactive chat — circle mode (`bindingMode.type: 'circle'`):**
- [ ] Messages in a bound channel → routed to the circle
- [ ] Default responder: the circle's primary agent (first member); operator can override via `@agent-name` mention or `/sera chat <agent-name> <message>`
- [ ] If the circle has an orchestration pattern configured (e.g. party mode), the message triggers that pattern — all agents contribute, responses posted sequentially in a thread
- [ ] `/sera party <prompt>` — starts a multi-agent party session in the circle; each agent's response posted as a separate embed in the thread

**Slash commands (all binding modes):**
- [ ] `/sera chat <agent-name> <message>` — explicit agent targeting (useful in circle mode or notification mode)
- [ ] `/sera task <agent-name> <task>` — enqueues a background task; bot replies with task ID and posts result when complete
- [ ] `/sera agents` — lists running agents with status badges (embed with fields)
- [ ] `/sera status <agent-name>` — agent detail: status, current task, queue depth, budget usage
- [ ] `/sera approve <requestId>` — approve a pending permission request
- [ ] `/sera deny <requestId>` — deny a pending permission request
- [ ] `/sera stop <agent-name>` — stop an agent (operator-mapped users only)
- [ ] `/sera start <agent-name>` — start an agent (operator-mapped users only)

**Rich notifications (outbound):**
- [ ] Informational events: Discord embed with colour-coded severity (green/yellow/red), title, body, metadata fields
- [ ] Actionable events (permission requests, delegation requests): embed includes Discord buttons ("Approve" / "Deny") that call the action token endpoint via interaction callback
- [ ] Button callbacks handled via Discord Interactions endpoint — sera-core validates the interaction signature and executes the decision

**Identity and authorization:**
- [ ] Discord user → SERA operator mapping via `POST /api/channels/discord/user-mapping` (discordUserId → operator sub)
- [ ] Slash commands that mutate state (approve/deny/stop/start) require a mapped operator identity — unmapped users receive an ephemeral "not authorised" response
- [ ] Chat commands (chat/task) from unmapped users: task is still routed but audit shows `actorId: 'discord:{discordUserId}'`

**Resilience:**
- [ ] Message delivery failures logged; exponential backoff on Discord API 429s (rate limit)
- [ ] Bot reconnects automatically on WebSocket disconnect (discord.js handles this)
- [ ] Graceful shutdown: bot goes offline on SIGTERM

**Technical Notes:**
- `discord.js` v14 is the recommended library — handles gateway, interactions, and REST API
- Bot requires `MESSAGE_CONTENT` intent (privileged) for reading messages in threads; slash commands don't need it
- Interaction callbacks (button clicks) arrive as HTTP POST to sera-core — register via Discord's Interactions Endpoint URL or use the gateway for simpler setup
- Thread-based conversations map naturally to SERA chat sessions — `sessionId` stored in thread metadata or a lookup table keyed by Discord thread ID

---

### Story 18.5: Slack channel adapter

**As an** operator
**I want** SERA to send notifications to Slack and accept approvals via Slack interactive messages
**So that** Slack is a first-class SERA management surface

**Prerequisites (manual, outside SERA):**
- The operator creates a Slack App at [api.slack.com/apps](https://api.slack.com/apps) with the required scopes and installs it to their workspace
- For interactive features: enable Interactivity and configure the Request URL to point to sera-core's callback endpoint
- App token and signing secret stored in SERA via `POST /api/secrets`

**Acceptance Criteria:**
- [ ] `SlackChannel` adapter uses Slack's Web API for outbound messages
- [ ] `config`: `{ webhookUrl?, appTokenSecret?, signingSecretSecret? }` — secret refs, not raw values
- [ ] Actionable events rendered with Slack Block Kit including "Approve" and "Deny" action buttons
- [ ] If `appTokenSecret` and `signingSecretSecret` provided: receives Slack interactive component callbacks; validates Slack signature on callback; executes decision via action token
- [ ] `/sera task @agent-name {task text}` slash command routes a task to the named agent (requires app token and Slack–operator mapping)
- [ ] Slack user → SERA operator mapping: `POST /api/channels/slack/user-mapping`

---

### Story 18.6: Email channel adapter (SMTP)

**As an** operator
**I want** to receive SERA notifications by email
**So that** critical alerts reach me even when I'm not in Discord or Slack

**Acceptance Criteria:**
- [ ] `EmailChannel` adapter sends via SMTP
- [ ] `config`: `{ smtpHost, smtpPort, smtpUser, smtpPasswordSecret, from, to (list) }` — `smtpPasswordSecret` is a key in `SecretsProvider`
- [ ] Email subject: `[SERA] [{severity}] {event title}`
- [ ] Email body: HTML and plain-text parts; includes approve/deny URLs for actionable events
- [ ] TLS required (`STARTTLS` or implicit TLS) — plain SMTP disallowed
- [ ] Delivery failures logged; no retry (SMTP servers handle queuing)
- [ ] `minSeverity: 'warning'` default for email routes — prevents email flooding from info-level events

---

### Story 18.7: Inbound message routing (external platform → agent/circle)

**As an** operator or external user
**I want** to send a message on Discord or Slack and have it routed to an agent or circle based on the channel's binding mode
**So that** those platforms become natural, conversational interfaces for interacting with agents

**Acceptance Criteria:**

**Routing via binding mode (primary mechanism):**
- [ ] Inbound messages are routed based on the channel's `bindingMode` (defined in Story 18.1):
  - `agent` mode: message → `POST /api/chat` with the bound `agentId`, session resolved from platform thread ID
  - `circle` mode: message → broadcast to circle; responding agent determined by circle config or `@agent-name` mention
  - `notification` mode: inbound messages ignored (outbound only)
- [ ] Chat session tracked per platform thread — `channel_sessions` table maps `{ channelId, platformThreadId }` → `{ seraSessionId, agentInstanceId }`
- [ ] Agent/circle response posted back to the originating platform thread as a reply

**Legacy route table (override mechanism):**
- [ ] `inbound_channel_routes` table retained for explicit overrides: `id`, `channelId`, `platformChannelId`, `targetAgentId`, `prefix?`, `created_at`
- [ ] If an explicit route exists for a platform channel ID, it takes precedence over the channel's default binding mode
- [ ] `POST /api/channels/routes` creates an explicit override — admin/operator role
- [ ] `GET /api/channels/routes` lists explicit routes

**Context and audit:**
- [ ] Chat/task `context` includes: `{ source: 'channel', channelType, channelId, platformUserId, platformUsername, platformThreadId }`
- [ ] Platform user mapping (Story 18.4/18.5) used to associate messages with operator identity in the audit trail
- [ ] Unmapped platform users: chat is still routed but audit shows `actorId: 'discord:{platformUserId}'` or `'slack:{platformUserId}'`

**Technical Notes:**
- The binding mode is the simple, declarative path: "this Discord bot talks to this agent/circle". No routing table needed for the common case.
- The explicit route table exists for advanced setups: "messages in #frontend-help go to the frontend agent, messages in #backend-help go to the backend agent" — even if both are on the same bot.
- `channel_sessions` table enables conversation continuity: a Discord thread started with one message maintains context across the full conversation.

---

### Story 18.8: Alert rule engine

**As an** operator
**I want** to define threshold-based alert rules that fire notifications automatically
**So that** I'm notified when budgets approach limits, agents are unresponsive, or queues are backing up

**Acceptance Criteria:**
- [ ] `alert_rules` table: `id`, `name`, `condition` (JSONB — rule definition), `channelIds` (array), `severity`, `cooldown_minutes` (minimum re-fire interval), `enabled`, `last_fired_at`
- [ ] Supported condition types (v1):
  - `{ type: 'budget_threshold', agentId?, threshold: 0.8 }` — fires when token usage exceeds N% of daily budget
  - `{ type: 'agent_unresponsive', agentId? }` — fires when agent transitions to `unresponsive` status
  - `{ type: 'task_queue_depth', agentId, threshold: N }` — fires when queue depth exceeds N
  - `{ type: 'agent_dead_lettered', agentId? }` — fires when a task enters dead-letter state (Story 5.8)
  - `{ type: 'permission_request_pending', ageSeconds: 300 }` — fires when a HitL request has been waiting longer than N seconds
- [ ] Background job evaluates rules every 60s; respects `cooldown_minutes` to prevent alert storms
- [ ] Rule fires → `ChannelRouter.route()` with appropriate severity and actionable payload if applicable
- [ ] `POST /api/alert-rules` creates a rule — operator role required
- [ ] `GET /api/alert-rules` lists rules with `lastFiredAt`
- [ ] `POST /api/alert-rules/:id/test` evaluates the rule immediately and routes a test notification (marked as test)

---

### Story 18.9: Channel topology UI — visual graph and configuration

**As an** operator
**I want** a visual graph showing all channels, their bindings to agents/circles, and how events flow through routing rules
**So that** I can understand, configure, and debug the messaging topology without reading JSON configs

**Acceptance Criteria:**

**Topology graph (read-only visualisation):**
- [ ] `/channels` page renders an interactive node-link graph:
  - **Agent nodes** — coloured by status (running/stopped/error)
  - **Circle nodes** — group node showing member count
  - **Channel nodes** — icon by type (Discord, Slack, email, webhook, schedule, intercom), badge showing binding mode
  - **Routing rule edges** — directed arrows from event sources to channel targets, labelled with event type filter
  - **Binding edges** — lines connecting channels to their bound agent/circle
- [ ] Graph uses `react-flow` (or `elkjs` for auto-layout) — draggable nodes, zoomable, minimap
- [ ] Node click → detail panel slides in from the right showing full config (redacted secrets), status, last event, active sessions
- [ ] Edge click → shows routing rule detail with filter conditions
- [ ] Live status: channel health badges update via Centrifugo subscription (green/amber/red)
- [ ] Built-in channels shown as a distinct group (muted styling — always present, not editable)

**Channel CRUD (from the graph):**
- [ ] "Add channel" button opens a wizard:
  1. Select type (Discord / Slack / Email / Webhook)
  2. **Prerequisites panel** — type-specific guidance for manual setup steps the operator must complete outside SERA:
     - Discord: "Create a bot in the Discord Developer Portal → copy the bot token → add bot to your server with MESSAGE_CONTENT intent → store the token as a SERA secret"
     - Slack: "Create a Slack App → enable Interactivity → copy app token and signing secret → store as SERA secrets"
     - Email: "Have your SMTP credentials ready → store the password as a SERA secret"
     - Webhook: no external prerequisites
  3. Select binding mode (agent → pick agent / circle → pick circle / notification)
  4. Configure adapter-specific fields (secret references, guild/channel IDs, etc.) — secrets selected from a dropdown of existing SERA secrets, with a "create new secret" inline option
  5. Test connection → green checkmark or error detail
  6. Save → node appears on graph
- [ ] Right-click channel node → edit config, disable/enable, delete, test connection
- [ ] Drag from a channel node to an agent/circle node → creates or changes the binding (visual shortcut for updating `bindingMode`)

**Routing rule builder:**
- [ ] "Add routing rule" button (or drag from an event source to a channel):
  1. Select event type (dropdown: `permission.requested`, `task.completed`, `agent.crashed`, `budget.exceeded`, `*` wildcard)
  2. Select target channels (multi-select from existing channels)
  3. Optional: set filter conditions (agent name, circle, severity threshold)
  4. Save → edge appears on graph connecting source to targets
- [ ] Routing rules editable inline from the edge detail panel
- [ ] Visual indication of catch-all (`*`) rules vs specific rules

**Session explorer (secondary view):**
- [ ] Tab or toggle on `/channels` page showing active sessions across all channels
- [ ] Table: session ID, channel name, platform thread ID, bound agent/circle, message count, last activity
- [ ] Click session → opens session detail with full message history (same view as `/chat` but read-only, showing the channel context)
- [ ] Filter by channel, agent, circle, activity recency

**Technical Notes:**
- `react-flow` is recommended over `react-force-graph-2d` here — the topology is relatively small (10-50 nodes) and benefits from manual layout persistence rather than physics simulation
- Node positions should be persisted in `localStorage` or a `ui_preferences` table so the graph layout is stable across page loads
- The graph is a configuration tool, not a monitoring dashboard — real-time message flow visualisation (message counts, throughput sparklines) can be added later as an overlay

---

### Story 18.10: Channel activity dashboard

**As an** operator
**I want** to see message volume, latency, and error rates across all channels
**So that** I can monitor channel health and debug delivery issues

**Acceptance Criteria:**
- [ ] `/channels/activity` page (or tab on `/channels`)
- [ ] Per-channel stats: messages sent (last 1h/24h/7d), delivery success rate, average latency, last error
- [ ] Time-series chart of message volume across all channels (stacked by channel, using recharts)
- [ ] Error log: recent delivery failures with channel name, event type, error message, timestamp
- [ ] Session stats: active sessions, total sessions, average session length (messages)
- [ ] Alert rule fire history: which rules fired, when, to which channels, outcome

**Technical Notes:**
- Data source: aggregate from `audit_trail` (channel events are audited) + `channel_sessions` activity
- Follow Epic 14 chart patterns (recharts, TanStack Query hooks)

---

## DB Schema

```sql
-- Story 18.1: Channel configurations (supports multiple instances per type)
CREATE TABLE notification_channels (
  id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  name            TEXT NOT NULL,
  type            TEXT NOT NULL,                -- 'discord' | 'slack' | 'email' | 'webhook'
  binding_mode    JSONB NOT NULL,               -- { type: 'agent', agentId } | { type: 'circle', circleId } | { type: 'notification' }
  config          JSONB NOT NULL,               -- encrypted; bot tokens, webhook URLs, etc.
  enabled         BOOLEAN NOT NULL DEFAULT true,
  created_at      TIMESTAMPTZ DEFAULT now()
);

-- Story 18.1: Event routing rules
CREATE TABLE notification_routing_rules (
  id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  event_type      TEXT NOT NULL,                -- e.g. 'permission.requested', '*'
  channel_ids     UUID[] NOT NULL,
  filter          JSONB,                        -- optional field match conditions
  min_severity    TEXT NOT NULL DEFAULT 'info',
  created_at      TIMESTAMPTZ DEFAULT now()
);

-- Story 18.7: Explicit inbound route overrides
CREATE TABLE inbound_channel_routes (
  id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  channel_id          UUID NOT NULL REFERENCES notification_channels ON DELETE CASCADE,
  platform_channel_id TEXT NOT NULL,            -- Discord channel ID, Slack channel ID
  target_agent_id     TEXT NOT NULL,
  prefix              TEXT,                     -- message prefix filter (optional)
  created_at          TIMESTAMPTZ DEFAULT now()
);

-- Story 18.7: Platform thread → SERA session mapping (conversation continuity)
-- Every conversation across any channel (Discord thread, Slack thread, API session, intercom DM)
-- maps to a SERA chat session. This enables context building across ingress/egress.
CREATE TABLE channel_sessions (
  id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  channel_id          UUID NOT NULL REFERENCES notification_channels ON DELETE CASCADE,
  platform_thread_id  TEXT NOT NULL,            -- Discord thread ID, Slack thread_ts, webhook slug, etc.
  sera_session_id     UUID NOT NULL,            -- FK to chat sessions table
  agent_instance_id   UUID,                     -- bound agent (null for circle-mode, resolved per message)
  circle_id           UUID,                     -- bound circle (null for agent-mode)
  last_activity_at    TIMESTAMPTZ DEFAULT now(), -- updated on every ingress/egress event
  created_at          TIMESTAMPTZ DEFAULT now(),
  UNIQUE (channel_id, platform_thread_id)
);
CREATE INDEX channel_sessions_session_idx ON channel_sessions (sera_session_id);

-- Story 18.8: Alert rules
CREATE TABLE alert_rules (
  id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  name            TEXT NOT NULL,
  condition       JSONB NOT NULL,               -- rule definition
  channel_ids     UUID[] NOT NULL,
  severity        TEXT NOT NULL DEFAULT 'warning',
  cooldown_minutes INT NOT NULL DEFAULT 30,
  enabled         BOOLEAN NOT NULL DEFAULT true,
  last_fired_at   TIMESTAMPTZ,
  created_at      TIMESTAMPTZ DEFAULT now()
);
```

# Epic 18: Integration Channels

## Overview

SERA's human-in-the-loop flows — permission requests, delegation requests, agent task assignments, alerts — currently require the operator to be watching the Centrifugo-connected UI in real time. This epic introduces an outbound integration channel system: a configurable layer that routes SERA events to external platforms (Discord, Slack, WhatsApp, email, generic webhooks) and, critically, makes HitL decisions *actionable* from those platforms. An operator should be able to approve a permission request via a Discord reply without opening the SERA UI.

The same channel abstraction also enables SERA agents to receive tasks from external platforms — a Discord message or Slack command becomes a task for a named agent, making those platforms first-class interaction surfaces.

## Context

- Complements Story 9.8 (inbound webhooks) — this epic covers outbound notification and bidirectional channel integration
- All HitL flows (Story 3.9 permission requests, Story 17.4 delegation requests) generate events that need to reach operators wherever they are
- The `Channel` interface is a plugin surface (Epic 15) — community adapters for new platforms can be published without core changes
- Channel adapters run in sera-core's process (not in containers) — they are trusted integration code

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
**I want** a pluggable `Channel` interface and an event routing table
**So that** any SERA event can be routed to any registered channel without core code changes

**Acceptance Criteria:**
- [ ] `Channel` TypeScript interface:
  ```typescript
  interface Channel {
    id: string
    type: string        // 'webhook' | 'discord' | 'slack' | 'email' | ...
    name: string        // human label e.g. 'ops-discord'
    send(event: ChannelEvent): Promise<void>
    canHandle(eventType: string): boolean
    healthCheck(): Promise<boolean>
  }

  interface ChannelEvent {
    id: string          // deduplication key
    eventType: string   // e.g. 'permission.requested', 'agent.crashed', 'budget.exceeded'
    title: string
    body: string
    severity: 'info' | 'warning' | 'critical'
    actionable?: ActionPayload  // present if HitL decision is possible from the channel
    metadata: Record<string, unknown>
    timestamp: string
  }

  interface ActionPayload {
    requestId: string
    requestType: 'permission' | 'delegation' | 'knowledge-merge'
    approveToken: string  // short-lived signed token encoding the pre-approved decision
    denyToken: string
    expiresAt: string
  }
  ```
- [ ] `notification_channels` table: `id` (UUID), `name`, `type`, `config` (JSONB — encrypted), `enabled` (boolean), `created_at`
- [ ] `notification_routing_rules` table: `id`, `eventType` (or `*` wildcard), `channelIds` (array), `filter` (JSONB — optional field match conditions), `minSeverity`
- [ ] `ChannelRouter.route(event)` evaluates routing rules and calls `channel.send()` for each matching channel
- [ ] Routing is async and non-blocking — send failures logged as warnings; never block the originating flow
- [ ] `POST /api/channels` creates a channel — admin role required
- [ ] `GET /api/channels` lists channels (config values redacted)
- [ ] `DELETE /api/channels/:id` removes a channel and its routing rules
- [ ] `POST /api/channels/:id/test` sends a test event
- [ ] `POST /api/routing-rules` creates an event routing rule
- [ ] `GET /api/routing-rules` lists rules
- [ ] `DELETE /api/routing-rules/:id` removes a rule

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
**I want** SERA to send notifications to a Discord channel and accept approvals via Discord message replies
**So that** my Discord ops server becomes a fully functional SERA management surface

**Acceptance Criteria:**
- [ ] `DiscordChannel` adapter sends rich embed messages to a configured webhook URL
- [ ] `config`: `{ webhookUrl, botToken?, approvalChannelId? }`
- [ ] Informational events: Discord embed with colour-coded severity (green/yellow/red), title, body, metadata fields
- [ ] Actionable events: embed includes two Discord buttons ("✅ Approve" / "❌ Deny") linking to `POST /api/actions/approve|deny` action token URLs
- [ ] If `botToken` and `approvalChannelId` provided: bot listens for `/sera approve {requestId}` and `/sera deny {requestId}` slash commands as an alternative approval path; these call the same action token endpoint
- [ ] Bot slash commands require the Discord user to have their `discordUserId` mapped to a SERA operator `sub` via `POST /api/channels/discord/user-mapping` — unmapped users receive "not authorised" response
- [ ] Message delivery failures (Discord API errors) logged; no retry storm

---

### Story 18.5: Slack channel adapter

**As an** operator
**I want** SERA to send notifications to Slack and accept approvals via Slack interactive messages
**So that** Slack is a first-class SERA management surface

**Acceptance Criteria:**
- [ ] `SlackChannel` adapter uses Slack's Incoming Webhooks API for outbound messages
- [ ] `config`: `{ webhookUrl, appToken?, signingSecret? }`
- [ ] Actionable events rendered with Slack Block Kit including "Approve" and "Deny" action buttons
- [ ] If `appToken` and `signingSecret` provided: receives Slack interactive component callbacks; validates Slack signature on callback; executes decision via action token
- [ ] `/sera task @agent-name {task text}` slash command routes a task to the named agent (requires `appToken` and Slack–operator mapping)
- [ ] Slack user → SERA operator mapping: `POST /api/channels/slack/user-mapping`

---

### Story 18.6: Email channel adapter (SMTP)

**As an** operator
**I want** to receive SERA notifications by email
**So that** critical alerts reach me even when I'm not in Discord or Slack

**Acceptance Criteria:**
- [ ] `EmailChannel` adapter sends via SMTP
- [ ] `config`: `{ smtpHost, smtpPort, smtpUser, smtpPassword (stored in SecretsProvider), from, to (list) }`
- [ ] Email subject: `[SERA] [{severity}] {event title}`
- [ ] Email body: HTML and plain-text parts; includes approve/deny URLs for actionable events
- [ ] TLS required (`STARTTLS` or implicit TLS) — plain SMTP disallowed
- [ ] Delivery failures logged; no retry (SMTP servers handle queuing)
- [ ] `minSeverity: 'warning'` default for email routes — prevents email flooding from info-level events

---

### Story 18.7: Inbound channel routing (external platform → agent task)

**As an** operator or external user
**I want** to send a message on Discord, Slack, or WhatsApp and have it routed as a task to a specific SERA agent
**So that** those platforms become natural interfaces for interacting with agents

**Acceptance Criteria:**
- [ ] `inbound_channel_routes` table: `id`, `channelId`, `channelType`, `platformChannelId` (e.g. Discord channel ID or Slack channel ID), `targetAgentId`, `prefix?` (message must start with this to be routed), `created_at`
- [ ] `POST /api/channels/routes` creates an inbound route — admin/operator role
- [ ] When a message arrives on a platform channel (Discord, Slack) matching a route: extract message text → strip prefix if configured → enqueue as task for `targetAgentId` via `POST /api/agents/:id/tasks`
- [ ] Task `context` includes: `{ source: 'channel', channelType, platformUserId, platformUsername }`
- [ ] Task result posted back to the originating platform thread/channel as a reply
- [ ] Platform user mapping (Story 18.4/18.5) used to associate platform messages with operator identity in the audit trail
- [ ] Unmapped platform users: task is still routed but audit record shows `actorId: 'unmapped:{platformUserId}'`
- [ ] `GET /api/channels/routes` lists inbound routes

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

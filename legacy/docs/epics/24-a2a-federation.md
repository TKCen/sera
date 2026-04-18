# Epic 24: A2A Federation Protocol

## Overview

SERA uses a **dual-tier federation model** for multi-agent communication. Internal agents (same SERA instance) communicate via Centrifugo intercom — sub-millisecond, fully audited, budget-enforced. External agents (other SERA instances, third-party A2A agents) communicate via the **A2A (Agent-to-Agent) protocol** — a Linux Foundation standard for cross-platform agent interoperability.

The A2A protocol enables SERA agents to collaborate with agents running on different platforms (Google ADK, LangGraph, CrewAI, etc.) through a standardized JSON-RPC interface with Agent Cards for capability discovery.

## Context

- **Internal** (Centrifugo): Core sees everything, enforces budgets, sub-ms latency, supports delegation/capability model
- **External** (A2A): opaque agents, HTTP round-trips, OAuth2/mTLS security, no built-in metering
- A2A endpoint in sera-core acts as the bridge — internal agents never speak A2A directly
- Agent Cards are auto-generated from agent templates at `/.well-known/agent.json`
- A2A is a Linux Foundation project with 50+ partners — broad ecosystem compatibility
- Key decision: A2A for **external federation only**; internal communication stays on Centrifugo because Core needs visibility for governance, auditing, and budget enforcement

| Aspect | Internal (Centrifugo) | External (A2A) |
|---|---|---|
| Latency | Sub-ms (pub/sub) | HTTP round-trips |
| Visibility | Core sees all messages | Opaque |
| Budget | Enforced by Core | Not built-in |
| Auth | JWT (internal) | OAuth2 / mTLS |
| Agent types | SERA-managed only | Any A2A-compatible |

## Dependencies

- Epic 09 (Real-Time Messaging) — Centrifugo intercom for internal tier
- Epic 17 (Agent Identity & Delegation) — service identity tokens for A2A auth
- Epic 18 (Integration Channels) — A2A as an inbound/outbound channel type

---

## Stories

### Story 24.1: A2A inbound server

**As** sera-core
**I want** an A2A-compliant JSON-RPC endpoint
**So that** external agents can discover and communicate with SERA agents

**Acceptance Criteria:**
- [ ] `A2AServer` in `core/src/a2a/server.ts` — Express router mounted at `/api/a2a`
- [ ] Implements A2A JSON-RPC methods:
  - `tasks/send` — receive a task from an external agent
  - `tasks/get` — query task status
  - `tasks/cancel` — cancel a running task
  - `tasks/sendSubscribe` — SSE streaming for task updates
- [ ] Request validation against A2A JSON Schema
- [ ] Authentication: OAuth2 bearer token or mTLS client certificate
- [ ] Rate limiting per external agent identity

### Story 24.2: Agent Card generation

**As** an operator
**I want** SERA agents to be discoverable via standard Agent Cards
**So that** external agents can find and understand what my agents can do

**Acceptance Criteria:**
- [ ] `GET /.well-known/agent.json` — serves the instance's root Agent Card
- [ ] `GET /api/a2a/agents/:id/card` — serves per-agent Agent Cards
- [ ] Agent Card auto-generated from agent template manifest:
  ```json
  {
    "name": "sera-architect",
    "description": "Architecture and design agent",
    "url": "https://sera.example.com/api/a2a",
    "version": "1.0.0",
    "capabilities": {
      "streaming": true,
      "pushNotifications": true
    },
    "skills": [
      { "id": "architecture-review", "name": "Architecture Review" }
    ],
    "authentication": {
      "schemes": ["oauth2", "mtls"]
    }
  }
  ```
- [ ] Cards update automatically when templates change (no manual sync)
- [ ] Capability filtering: only publicly-exposed skills appear in cards

### Story 24.3: A2A outbound client

**As** a SERA agent
**I want** to send tasks to external A2A agents
**So that** I can delegate work to agents outside my SERA instance

**Acceptance Criteria:**
- [ ] `A2AClient` in `core/src/a2a/client.ts` — HTTP client for A2A JSON-RPC
- [ ] Discovers external agents via Agent Card URL
- [ ] Sends tasks via `tasks/send` with message parts (text, file, data)
- [ ] Supports streaming responses via `tasks/sendSubscribe` (SSE)
- [ ] Agent tool: `a2a.delegate` — available to agents for delegating to external agents
- [ ] Client handles retries, timeouts, and circuit breaking

### Story 24.4: Instance pairing and trust

**As** an operator
**I want** to pair my SERA instance with other A2A-compatible platforms
**So that** agents can federate across trusted instances

**Acceptance Criteria:**
- [ ] `PairingService` — manages trusted external agent registrations
- [ ] Pairing flow: operator enters Agent Card URL → SERA fetches card → operator confirms trust
- [ ] Paired agents stored in `a2a_paired_agents` table
- [ ] Trust levels: `full` (all skills), `restricted` (selected skills only), `read-only` (status queries only)
- [ ] Operator can revoke trust at any time from Settings page
- [ ] Paired agents visible in Agents page with "external" badge

### Story 24.5: Capability gate for federation

**As** sera-core
**I want** to control which agent capabilities are exposed via A2A
**So that** internal agents don't accidentally expose sensitive operations externally

**Acceptance Criteria:**
- [ ] `FederationPolicy` in capability resolution — specifies which skills are A2A-visible
- [ ] Default: no skills exposed (opt-in model)
- [ ] Operator configures per-agent federation exposure in agent template
- [ ] Inbound A2A tasks validated against federation policy before routing
- [ ] Blocked requests return A2A-compliant error response

### Story 24.6: A2A streaming and push notifications

**As** an external agent
**I want** to receive streaming updates for long-running tasks
**So that** I can show progress to my user without polling

**Acceptance Criteria:**
- [ ] `tasks/sendSubscribe` returns SSE stream with task state updates
- [ ] Stream events: `status-update`, `artifact`, `message`
- [ ] Push notification support: external agent registers webhook for async updates
- [ ] Push notifications stored in `a2a_push_subscriptions` table
- [ ] Webhook delivery with HMAC signatures and retry logic

### Story 24.7: Cross-instance circle membership

**As** an operator
**I want** to add external agents to my circles
**So that** federated agents can participate in circle discussions and coordinated tasks

**Acceptance Criteria:**
- [ ] External agents representable as `CircleMember` with `source: 'a2a'`
- [ ] Circle broadcasts to external members delivered via A2A `tasks/send`
- [ ] External member responses collected and merged into circle context
- [ ] Party mode (Epic 10) supports mixed internal + external participants
- [ ] External member health status queryable via Agent Card endpoint

---

## DB Schema

```sql
-- Story 24.4: Paired external agents
CREATE TABLE a2a_paired_agents (
  id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  agent_card_url  text NOT NULL UNIQUE,
  name            text NOT NULL,
  description     text,
  trust_level     text NOT NULL DEFAULT 'restricted',  -- 'full' | 'restricted' | 'read-only'
  allowed_skills  text[],                               -- null = all (when trust_level='full')
  oauth_config    jsonb,                                -- client_id, token_endpoint, etc.
  status          text NOT NULL DEFAULT 'active',       -- 'active' | 'revoked'
  paired_at       timestamptz NOT NULL DEFAULT now(),
  paired_by       uuid REFERENCES operators(id),
  last_seen_at    timestamptz
);

CREATE INDEX idx_a2a_paired_status ON a2a_paired_agents(status);

-- Story 24.6: Push notification subscriptions
CREATE TABLE a2a_push_subscriptions (
  id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  paired_agent_id uuid NOT NULL REFERENCES a2a_paired_agents(id) ON DELETE CASCADE,
  webhook_url     text NOT NULL,
  hmac_secret     text NOT NULL,
  task_id         uuid,                                 -- null = all tasks from this agent
  created_at      timestamptz NOT NULL DEFAULT now(),
  expires_at      timestamptz
);

CREATE INDEX idx_a2a_push_agent ON a2a_push_subscriptions(paired_agent_id);

-- Story 24.1: Inbound A2A task tracking
CREATE TABLE a2a_inbound_tasks (
  id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  external_task_id text NOT NULL,
  paired_agent_id uuid NOT NULL REFERENCES a2a_paired_agents(id),
  target_agent_id uuid REFERENCES agent_instances(id),
  status          text NOT NULL DEFAULT 'submitted',    -- 'submitted' | 'working' | 'completed' | 'failed' | 'canceled'
  messages        jsonb NOT NULL DEFAULT '[]',
  artifacts       jsonb NOT NULL DEFAULT '[]',
  created_at      timestamptz NOT NULL DEFAULT now(),
  updated_at      timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX idx_a2a_inbound_status ON a2a_inbound_tasks(status);
CREATE INDEX idx_a2a_inbound_external ON a2a_inbound_tasks(external_task_id);
```

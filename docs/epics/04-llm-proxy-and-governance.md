# Epic 04: LLM Proxy & Governance

## Overview

sera-core is the authoritative governance layer for all LLM usage. Every LLM call — regardless of which agent or component makes it — passes through the sera-core proxy. This enables per-agent metering, budget enforcement, key vaulting, circuit breaking, and audit recording in one place. LiteLLM sits downstream as a dumb routing socket; SERA owns policy.

## Context

- See `docs/ARCHITECTURE.md` → LLM Routing, Provider Gateway: LiteLLM
- The proxy endpoint is `POST /v1/llm/chat/completions` (OpenAI-compatible)
- All agents authenticate with JWT; the proxy validates identity and enforces budget before forwarding
- LiteLLM features used: routing, load balancing, fallbacks, model management API
- LiteLLM features explicitly NOT used: virtual keys, team budgets, usage dashboards — SERA owns these

## Dependencies

- Epic 01 (Infrastructure Foundation) — LiteLLM container running
- Epic 02 (Agent Manifest & Registry) — agent identity, model configuration in manifest
- Epic 03 (Docker Sandbox) — JWT issued at container spawn

---

## Stories

### Story 4.1: LLM proxy endpoint

**As an** agent container
**I want** to make LLM calls via `POST /v1/llm/chat/completions` on sera-core
**So that** my LLM calls are metered, governed, and routed without me knowing the upstream provider

**Acceptance Criteria:**
- [ ] `POST /v1/llm/chat/completions` accepts OpenAI-compatible request body
- [ ] Request body: `{ model, messages, tools?, temperature?, stream? }`
- [ ] Response: OpenAI-compatible response with `choices`, `usage`
- [ ] `Authorization: Bearer {JWT}` header required — 401 if absent or invalid
- [ ] On success: response forwarded unchanged from LiteLLM to caller
- [ ] `GET /v1/llm/models` returns available models from LiteLLM `GET /model/info`
- [ ] Proxy adds `X-SERA-Agent-Id` header on forwarded requests for LiteLLM log correlation (but LiteLLM budget enforcement is disabled)
- [ ] Request/response latency logged at DEBUG level

**Technical Notes:**
- The proxy is OpenAI-compatible so the same `LLMClient` works whether calling sera-core directly or (in testing) LiteLLM directly
- Streaming (`stream: true`) must be proxied correctly — server-sent events forwarded chunk by chunk

---

### Story 4.2: JWT authentication at the proxy

**As** sera-core
**I want** to validate the caller's JWT on every LLM proxy request
**So that** only authorised agents and components can make LLM calls

**Acceptance Criteria:**
- [ ] JWT validated on every request: signature, expiry, required claims (`agentId`, `scope`)
- [ ] `scope: 'agent'` required for agent containers; `scope: 'internal'` for sera-core's own calls
- [ ] Invalid/expired JWT returns 401 with a descriptive error message
- [ ] `agentId` from JWT used as the identity for metering and audit — not a caller-supplied field
- [ ] JWT validation is synchronous and adds < 1ms to request handling
- [ ] Revocation list (optional, future): `jti` claim checked against a deny-list in Redis/DB

---

### Story 4.3: Per-agent token budget enforcement

**As an** operator
**I want** per-agent hourly and daily token budgets enforced at the proxy
**So that** a runaway agent cannot consume unlimited LLM tokens

**Acceptance Criteria:**
- [ ] Budget check runs before forwarding to LiteLLM — no upstream call if budget exceeded
- [ ] Hourly budget (`maxLlmTokensPerHour`) and daily budget (`maxLlmTokensPerDay`) from manifest `resources`
- [ ] Budget exceeded returns HTTP 429 with body: `{ error: 'budget_exceeded', period: 'hourly'|'daily', limit: N, used: N }`
- [ ] Budgets checked against `token_usage` table aggregated by agent + time window
- [ ] Budget check query is indexed and executes in < 10ms under normal load
- [ ] `GET /api/agents/:id/budget` returns current usage vs limits for both windows
- [ ] Agents without `maxLlmTokensPerHour`/`maxLlmTokensPerDay` in manifest have no limit (but usage still recorded)

---

### Story 4.4: Token usage metering

**As an** operator
**I want** every LLM call's token usage recorded against the responsible agent
**So that** I can audit costs, track trends, and enforce budgets accurately

**Acceptance Criteria:**
- [ ] After each successful LLM response, record to `token_usage`: `agent_id`, `model`, `prompt_tokens`, `completion_tokens`, `total_tokens`, `latency_ms`, `timestamp`
- [ ] Usage recording is async (non-blocking) — does not add latency to the LLM response path
- [ ] Failed LLM calls (4xx/5xx from LiteLLM) recorded with `status: error`, `total_tokens: 0`
- [ ] `GET /api/metering/usage?agentId=&from=&to=&groupBy=hour|day` returns aggregated usage data
- [ ] `GET /api/metering/summary` returns total usage across all agents for the current day
- [ ] Usage data retained for 90 days (configurable via `METERING_RETENTION_DAYS`)

---

### Story 4.5: Provider management API

**As an** operator
**I want** to add, remove, and list LLM providers through SERA's API
**So that** LiteLLM remains an implementation detail I never interact with directly

**Acceptance Criteria:**
- [ ] `GET /api/providers` lists all configured models/providers (proxies LiteLLM `GET /model/info`)
- [ ] `POST /api/providers` adds a new model/provider (proxies LiteLLM `POST /model/new`) — requires `operator` scope
- [ ] `DELETE /api/providers/:modelName` removes a model (proxies LiteLLM `DELETE /model/delete`) — requires `operator` scope
- [ ] Request/response bodies use SERA's own schema — LiteLLM schema is not exposed to callers
- [ ] SERA validates the provider config before forwarding to LiteLLM
- [ ] Provider add/remove events recorded in audit trail
- [ ] `POST /api/providers/:modelName/test` sends a minimal test completion to verify the provider is reachable

**Technical Notes:**
- These endpoints call LiteLLM's model management API which supports live updates (no restart needed for adding models)
- Routing strategy and fallback chain changes still require LiteLLM restart — document this clearly

---

### Story 4.6: Circuit breaker for LLM calls

**As** sera-core
**I want** a circuit breaker on the LLM proxy
**So that** a failing upstream LLM provider doesn't cascade into agent failures or hang the system

**Acceptance Criteria:**
- [ ] Circuit breaker per provider (identified by model name prefix or provider tag)
- [ ] Opens after N consecutive failures (default: 5) within a time window (default: 60s)
- [ ] Open circuit returns 503 immediately with `{ error: 'provider_unavailable', provider: '...' }`
- [ ] Half-open state: one test request allowed after cool-down period (default: 30s)
- [ ] Circuit state visible at `GET /api/providers/:modelName/health`
- [ ] Circuit events (open/close/half-open) logged and published to Centrifugo `system.providers` channel

**Technical Notes:**
- LiteLLM handles retries and fallbacks at the routing level; the circuit breaker in sera-core is the higher-level policy layer that can halt all calls to a provider regardless of LiteLLM's retry logic

---

### Story 4.7: API rate limiting (P2 — deferred)

**As** sera-core
**I want** per-caller rate limiting on the LLM proxy and management API endpoints
**So that** a single agent or operator cannot monopolise the system during peak load

> **Status:** Deferred. Stub story to prevent architectural foreclosure.

**Acceptance Criteria (minimum viable, when implemented):**
- [ ] Token-bucket rate limiter per `agentId` on `POST /v1/llm/chat/completions` — configurable RPM ceiling
- [ ] Per-operator rate limit on management API endpoints (create/delete/modify)
- [ ] Rate limit headers returned: `X-RateLimit-Limit`, `X-RateLimit-Remaining`, `X-RateLimit-Reset`
- [ ] 429 response on limit breach — distinct from budget-exceeded 429 (include `reason: 'rate_limit'`)
- [ ] Rate limit state stored in-process (single node) in v1 — Redis-backed for multi-node future

**Technical Notes:**
- In-process state means rate limits reset on sera-core restart; acceptable for v1 homelab deployment
- Distinguish from token budget (Epic 4.3): rate limiting is requests-per-minute; budgets are tokens-per-period

# Epic 14: sera-web Observability & Provider Management

## Overview

Operators need visibility into system health, resource consumption, and cost. This epic covers metering dashboards, budget management, the audit log viewer, and provider configuration — the "control room" of a running SERA deployment. These views are critical for trust: an operator who can see exactly what their agents are doing and what it costs will trust the system to operate autonomously.

## Context

- See `docs/ARCHITECTURE.md` → Provider Gateway: LiteLLM (provider management API abstraction)
- Metering data comes from `GET /api/metering/*` endpoints
- Audit data comes from `GET /api/audit` endpoint
- Provider management calls `POST|DELETE /api/providers` on sera-core (which proxies to LiteLLM)
- Operators never interact with LiteLLM directly — SERA's UI is the only interface

## Dependencies

- Epic 12 (sera-web Foundation) — API client, component library
- Epic 04 (LLM Proxy & Governance) — metering and provider API endpoints
- Epic 11 (Scheduling & Audit) — audit trail API

---

## Stories

### Story 14.1: Token usage dashboard

**As an** operator
**I want** a dashboard showing LLM token usage across all agents over time
**So that** I can understand system load, identify expensive agents, and plan capacity

**Acceptance Criteria:**
- [ ] `/settings` or dedicated `/usage` page has a usage dashboard section
- [ ] Summary cards: total tokens today, total tokens this month, most active agent (by tokens), estimated cost (if provider pricing is configured)
- [ ] Time-series chart: tokens per hour/day for the selected time range (7d default, 30d option)
- [ ] Per-agent breakdown table: agent name, prompt tokens, completion tokens, total tokens, % of total — sortable
- [ ] Model breakdown: usage split by model name
- [ ] Time range selector: today, 7 days, 30 days, custom range
- [ ] Data refreshes automatically every 60s (or on-demand via refresh button)
- [ ] Export: "Download CSV" for the current view

---

### Story 14.2: Budget management UI

**As an** operator
**I want** to view and manage per-agent token budgets in the UI
**So that** I don't have to edit YAML files to adjust spending limits

**Acceptance Criteria:**
- [ ] Agent detail page shows budget panel: hourly limit, daily limit, current hourly usage, current daily usage — with progress bars
- [ ] Budget warnings: visual indicator when usage reaches 80% of a limit
- [ ] Budget exceeded: prominent warning banner on agent detail page; agent shows as `budget_exceeded` status
- [ ] Inline edit: click hourly/daily limit to edit in place and save to `PUT /api/agents/:id` (updates manifest snapshot)
- [ ] "Reset budget" (operator action): clears current period usage counters (useful for testing)
- [ ] Budget events visible in the agent's activity timeline: `budget_warning`, `budget_exceeded`, `budget_reset`

---

### Story 14.3: Audit log viewer

**As an** operator
**I want** to browse and search the audit log in the UI
**So that** I can investigate what happened during an agent run without querying the database directly

**Acceptance Criteria:**
- [ ] `/audit` page shows paginated audit events, newest first
- [ ] Each event row: timestamp, actor (agent name or "operator"), action, resource type + ID, status
- [ ] Expand row: shows full event payload (formatted JSON)
- [ ] Filters: actor, action type, resource type, date range
- [ ] Search: full-text search on action and resource ID fields
- [ ] "Verify chain integrity" button: calls `GET /api/audit/verify` and shows result (valid ✓ or broken at sequence N)
- [ ] Export button: downloads filtered results as JSON or CSV
- [ ] Deep link: `/audit?agentId={id}` pre-filters to a specific agent's events

---

### Story 14.4: Provider management UI

**As an** operator
**I want** to manage LLM providers through the SERA dashboard
**So that** I can add, test, and remove providers without touching config files or the LiteLLM API directly

**Acceptance Criteria:**
- [ ] `/settings` page has a "Providers" section listing all configured models
- [ ] Each provider card shows: model name, provider type (local/cloud), status (reachable / unreachable / circuit-open), last checked timestamp
- [ ] "Add Provider" button opens a form: model name (display), provider type selector, API base URL, API key (masked input), model identifier
- [ ] On add: `POST /api/providers` — success shows new card, failure shows error from sera-core
- [ ] "Test" button per provider: `POST /api/providers/:name/test` — shows latency or error inline
- [ ] "Remove" button: `DELETE /api/providers/:name` with confirmation dialog
- [ ] Provider status auto-refreshes every 30s via polling (providers don't have a Centrifugo channel in v1)
- [ ] Circuit breaker status visible: amber badge "Circuit Open" with reset button

---

### Story 14.5: System health overview

**As an** operator
**I want** a system health overview page
**So that** I can quickly confirm all SERA components are running correctly

**Acceptance Criteria:**
- [ ] `/settings` or dedicated `/health` page shows status of: sera-core, Centrifugo, PostgreSQL, Qdrant, LiteLLM, configured LLM providers
- [ ] Each component: green (healthy), amber (degraded), red (unreachable)
- [ ] Status fetched from `GET /api/health` (existing endpoint) — extend to include component-level status
- [ ] Agent stats: total agents, running, stopped, errored — as summary numbers
- [ ] Centrifugo connection status shown in the UI header (persistent indicator)
- [ ] Page auto-refreshes every 30s

---

### Story 14.6: Schedule management UI

**As an** operator
**I want** to view and manage all agent schedules in the UI
**So that** I can see what's scheduled, when it last ran, and pause or trigger schedules manually

**Acceptance Criteria:**
- [ ] `/schedules` page lists all schedules across all agents
- [ ] Each row: agent name, schedule name, type (cron/once), expression, next run (human-readable: "in 2 hours"), last run status (success/error/missed), status (active/paused)
- [ ] Status indicator: colour-coded active (green), paused (grey), error (red)
- [ ] Toggle active/paused per schedule inline
- [ ] "Run Now" button triggers `POST /api/schedules/:id/trigger` — with confirmation
- [ ] "Edit" opens an inline edit form for expression and task prompt
- [ ] "Delete" with confirmation
- [ ] Filter by agent, status
- [ ] Last run result expandable: shows agent output excerpt or error message

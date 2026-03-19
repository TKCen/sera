# SERA Implementation Order

This document defines the recommended implementation sequence for the 18 epics. The phases reflect both technical dependency (you cannot spawn agents without infrastructure) and risk sequencing (foundational decisions should be validated before building on top of them).

Each phase produces a meaningful, runnable milestone — not just a collection of stories.

---

## Phase 1 — MVP: A governed, sandboxed agent you can talk to

**Target state:** A single agent (Sera) running in a Docker container, receiving tasks via the API, reasoning with a local LLM, publishing thoughts in real time, with operator auth and secrets working.

| Epic | Stories to implement | Notes |
|---|---|---|
| **01 Infrastructure** | All (1.1–1.6) | Foundation everything else runs on |
| **16 Auth & Secrets** | 16.3 (API key), 16.4 (RBAC), 16.8 (PostgreSQL secrets) | API-key-only mode first; OIDC comes in Phase 3. Bootstrap key gives first-start access. |
| **02 Manifest & Registry** | 2.1, 2.1b, 2.1c, 2.1d, 2.2, 2.2b, 2.2c, 2.3 | Full template + instance model; import-on-load for policies |
| **03 Docker Sandbox** | 3.1, 3.2, 3.2b, 3.3, 3.5, 3.6, 3.7, 3.8 | Spawn, capability resolution, workspace, lifecycle. Permission requests (3.9/3.10) in Phase 2. |
| **04 LLM Proxy** | 4.1, 4.2, 4.3, 4.4, 4.6 | Proxy, auth, budgets, metering, circuit breaker. Provider management API (4.5) usable but not blocking. |
| **05 Agent Runtime** | 5.1, 5.2, 5.3, 5.4, 5.5, 5.6, 5.7, 5.10 | Container image, reasoning loop, tool executor, thoughts, graceful shutdown, context window, prompt injection defence |
| **12 sera-web Foundation** | 12.1–12.4 | API client, Centrifugo hooks, routing, design system foundation. Enough to see thoughts stream. |

**MVP milestone:** `docker compose up -d` → Sera agent starts → operator logs in with bootstrap API key → sends a task via API or basic UI → sees thought stream → gets a result.

**Not in MVP:** OIDC, skills, MCP tools, memory, scheduling, circles, delegation, channels.

---

## Phase 2 — Usable: Skills, tools, memory, scheduling, chat UI

**Target state:** A configured agent that uses skills and MCP tools, remembers things across sessions, can be scheduled, has a full chat interface, and a meaningful audit trail.

| Epic | Stories to implement | Notes |
|---|---|---|
| **06 Skill Library** | All (6.1–6.6) | Skills injected into agent context |
| **07 MCP Tool Registry** | 7.1–7.6, 7.8 | Containerised MCP servers; SERA MCP Extension Protocol. Story 7.7 (sera-core as MCP server) in Phase 3. |
| **08 Memory & RAG** | 8.1–8.7, 8.8 | Personal memory + git-backed circle knowledge + scoped tools |
| **09 Real-Time Messaging** | 9.1–9.5, 9.7 | Channels, IntercomService, thought persistence. Webhooks (9.8) and federation (9.6) lower priority. |
| **11 Scheduling & Audit** | 11.1–11.5 | Schedule engine + audit trail. Export (11.6) is convenience. |
| **03 Docker Sandbox** | 3.9, 3.10, 3.11, 3.12 | Permission requests, dynamic mounts, recursion guard, disk quotas |
| **05 Agent Runtime** | 5.8, 5.9 | Task queue + task result storage |
| **13 sera-web Agent UX** | All (13.1–13.6) | Agent list/detail/create, chat, thought visualisation, memory graph |
| **14 sera-web Observability** | 14.1–14.4 | Token usage, budget UI, audit log viewer, provider management |

**Phase 2 milestone:** Multiple configured agents with skills and MCP tools, talking to each other through circles, with memory that persists across sessions, scheduled tasks running overnight, and a complete operator UI.

---

## Phase 3 — Ecosystem: Auth, delegation, channels, plugins

**Target state:** Multi-operator ready (OIDC, RBAC enforced), full delegation model, external notification channels, and a plugin SDK for community contributions.

| Epic | Stories to implement | Notes |
|---|---|---|
| **16 Auth & Secrets** | 16.1, 16.2, 16.5, 16.6, 16.7, 16.9–16.11 | Full OIDC, Authentik, web UI auth flow, CLI device flow, secrets interface, injection, rotation |
| **17 Agent Identity & Delegation** | All (17.1–17.9) | ActingContext, service identities, operator/agent delegation, credential resolver, audit chain |
| **07 MCP Tool Registry** | 7.7 | sera-core as MCP server — Sera can now orchestrate the full instance |
| **10 Circles & Coordination** | All (10.1–10.6) | Circle management, constitutions, orchestration patterns, party mode |
| **18 Integration Channels** | All (18.1–18.8) | Discord/Slack/email/webhook, actionable HitL, alert rules |
| **15 Plugin SDK** | All (15.1–15.8) | Plugin interface, `@sera/mcp-sdk`, contributor docs, `sera` CLI |
| **14 sera-web Observability** | 14.5–14.6 | System health, schedule management UI |
| **09 Real-Time Messaging** | 9.6, 9.8 | Federation stub, webhooks |
| **04 LLM Proxy** | 4.7 | Rate limiting |
| **01 Infrastructure** | 1.7–1.9 | Backup/restore, instance identity, upgrade path |

**Phase 3 milestone:** Multiple operators with distinct identities and roles, agents delegating credentials, community-published MCP tools working in SERA, Discord-based HitL approvals, and a plugin SDK for the ecosystem.

---

## Phase 4 — Consolidation: Technical debt, migrations, clean architecture

**Target state:** All legacy shims removed, one coherent memory model, fully tested internals. Safe to hand off to community contributors.

| Epic | Stories to implement | Notes |
|---|---|---|
| **19 Memory System Consolidation** | All (19.1–19.5) | Retire Letta-style memory; migrate BaseAgent/WorkerAgent to Epic 8 scoped model; remove MemoryManager, Reflector; on-disk migration for legacy files |

**Phase 4 milestone:** Zero references to the old Letta memory system; all agents read the same scoped memory; memory graph UI reflects real data.

---

## Dependency constraints

These are hard prerequisites — do not start a story before its upstream is complete:

```
Epic 01 → all other epics
Epic 02 → Epic 03
Epic 03 → Epic 05
Epic 04 → Epic 05
Epic 05 → Epic 08 (context assembly), Epic 10 (circles)
Epic 16 (API key, Story 16.3) → all authenticated API work
Epic 16 (OIDC, Story 16.1) → Epic 17
Epic 07 (MCP containers) → Epic 07 (sera-core as MCP server, Story 7.7)
Epic 08 (git knowledge) → Epic 10 (circle knowledge sharing)
Epic 09 (channels) → Epic 18
Epic 15 (plugin SDK) → @sera/mcp-sdk (Story 15.8)
Epic 08 (Memory & RAG) → Epic 19 (Memory Consolidation)
Epic 05 (Agent Runtime) → Epic 19 (BaseAgent migration)
Epic 13 (sera-web Agent UX) → Epic 19 (memory graph UI must be updated before old routes are removed)
```

---

## Story ordering within an epic

Within each epic, implement stories in the order they are written — the numbering reflects the natural dependency chain. Exception: "deferred" stubs (P2/P3 stories) can always be skipped until their phase.

---

## Parallel work

Once Phase 1 is complete, Phase 2 work can be parallelised across tracks. Suggested agent assignments if multiple agents implement in parallel:

| Track | Epics |
|---|---|
| **Core runtime** | 05 (remaining), 06, 07 |
| **Data & memory** | 08, 11 |
| **UI** | 13, 14 |
| **Messaging** | 09 |

Tracks are independent after the Phase 1 foundation is in place. The seams between tracks (context assembly calling memory, UI consuming audit API) are well-defined API contracts — teams can build against mocks until the implementation is ready.

# SERA Implementation Order

This document defines the recommended implementation sequence. The phases reflect both technical dependency (you cannot spawn agents without infrastructure) and risk sequencing (foundational decisions should be validated before building on top of them).

Each phase produces a meaningful, runnable milestone — not just a collection of stories.

**Tracking:** [v1 Prototype Tracking Issue (#565)](https://github.com/TKCen/sera/issues/565) | [v1 Gate: E2E Smoke Test (#564)](https://github.com/TKCen/sera/issues/564)

---

## Phase 0 — v1 Prototype: Three Pillars

**Target state:** A single agent (Sera) that you can talk to via web UI and Discord, who reasons with tools, remembers everything across sessions, develops personality over time, and makes proactive proposals. Powered by LM Studio (Qwen 3.5 for inference, EmbeddingGemma for embeddings).

**Principle:** Working software over broad infrastructure. Fix the broken paths before building new features.

### Track A: Container Reliability + Chat Loop (week 1–2)

Stories drawn from Epics 04, 05, 12, 13.

| Step | What | Issue | Notes |
|------|------|-------|-------|
| A.1 | Fix Sera agent model name resolution | #552 | Config fix — model name must match providers.json |
| A.2 | Verify apiKey propagation for local LLM | #497 | Use static providers.json entry for LM Studio |
| A.3 | Verify streaming error surfacing | #553 | PR #561 merged — verify fix works e2e |
| A.4 | E2E smoke test (Tests 1–3) | #564 | Basic chat, tool use, error handling |
| A.5 | Display reasoning steps in chat UI | #455 | Thought stream data arrives, rendering needs work |
| A.6 | Web UI CRUD for agents | #317 | Delegate sub-tasks to Jules |

### Track B: Discord Integration (week 2–3, after Track A.4 passes)

Stories drawn from Epic 18.

| Step | What | Notes |
|------|------|-------|
| B.1 | Deprecate legacy DiscordAdapter | Remove instantiation from `index.ts`, wire `DiscordChatAdapter` as default |
| B.2 | Verify container routing e2e via Discord | `DiscordChatAdapter` already routes through containers with session management |
| B.3 | Implement slash command registration | `/ask`, `/status`, `/history`, `/reset` — genuinely missing |
| B.4 | Setup guide for Discord bot token | Document env var configuration |

**Note:** `DiscordChatAdapter` already implements session management, typing indicators, chunked messages, and DM support. Track B is days, not weeks.

### Track C: Memory & Context (week 1–3, parallel with A after C.1)

Stories drawn from Epics 08, 13.

| Step | What | Issue | Notes |
|------|------|-------|-------|
| C.0 | **PREREQUISITE:** Embedding model in LM Studio | — | Load `text-embedding-embeddinggemma-300m-qat` (or `nomic-embed-text`). Configure `core/config/embedding.json` with `provider: "lm-studio"`, matching baseUrl and dimension. |
| C.1 | Verify Qdrant collections auto-create on startup | — | Fresh Qdrant may not have namespace collections |
| C.2 | Verify knowledge-store → Qdrant → ContextAssembler cycle | — | Full RAG retrieval path |
| C.3 | Add startup warning when embedding service unavailable | — | Currently silently skips RAG |
| C.4 | DELETE memory blocks endpoint | #465 | PR #556 exists but has CI failure — fix and merge |
| C.5 | Memory exploration UI | #352 | Operators need to see what the agent remembers |

### v1 Gate

All three tracks must pass the [5-scenario smoke test (#564)](https://github.com/TKCen/sera/issues/564):
1. **Basic Chat** — send message, receive streamed response with thoughts
2. **Tool Use** — agent creates a file via tool, confirms success
3. **Error Handling** — meaningful error on LLM failure (not infinite spinner)
4. **Discord** — DM the bot, get response with session persistence
5. **Memory** — store knowledge, retrieve in new session

**Not in v1:** Multi-agent delegation, scheduling, additional channels (Slack/Email/Webhook), Plugin SDK, ACP/IDE Bridge, Voice, Canvas/A2UI, A2A Federation.

---

## Phase 1 — Usable: Skills, tools, memory enhancements, scheduling, chat UI

**Target state:** A configured agent that uses skills and MCP tools, has enhanced memory with hybrid search, can be scheduled, has a full chat interface, and a meaningful audit trail.

| Epic | Stories to implement | Notes |
|---|---|---|
| **06 Skill Library** | All (6.1–6.6) | Skills injected into agent context |
| **07 MCP Tool Registry** | 7.1–7.6, 7.8 | Containerised MCP servers; SERA MCP Extension Protocol. Story 7.7 (sera-core as MCP server) in Phase 2. |
| **08 Memory & RAG** | Remaining: 8.1–8.7, 8.8 | Enhanced memory — hybrid search, categorization, hierarchical scopes |
| **09 Real-Time Messaging** | 9.1–9.5, 9.7 | Channels, IntercomService, thought persistence. Webhooks (9.8) and federation (9.6) lower priority. |
| **11 Scheduling & Audit** | 11.1–11.5 | Schedule engine + audit trail. Export (11.6) is convenience. |
| **03 Docker Sandbox** | 3.9, 3.10, 3.11, 3.12 | Permission requests, dynamic mounts, recursion guard, disk quotas |
| **05 Agent Runtime** | 5.8, 5.9 + enhancements | Task queue + result storage, compaction strategy (#501), system prompt builder (#500) |
| **13 sera-web Agent UX** | 13.1–13.6 (Phase 1), 13.7–13.12 (Phase 2) | Core: agent list/detail/create, chat, thoughts, memory graph |
| **14 sera-web Observability** | 14.1–14.4 | Token usage, budget UI, audit log viewer, provider management |
| **20 Egress Proxy** | 20.1–20.7 | Squid forward proxy on agent_net, per-agent ACLs, audit integration, egress metering |

**Phase 1 milestone:** Multiple configured agents with skills and MCP tools, with memory that persists across sessions, scheduled tasks running overnight, network egress audited and metered, and a complete operator UI.

---

## Phase 2 — Ecosystem: Auth, delegation, channels, plugins

**Target state:** Multi-operator ready (OIDC, RBAC enforced), full delegation model, external notification channels, and a plugin SDK for community contributions.

| Epic | Stories to implement | Notes |
|---|---|---|
| **16 Auth & Secrets** | 16.1, 16.2, 16.5–16.12 | Full OIDC, Authentik, web UI auth flow, CLI device flow, secrets interface, injection, rotation, out-of-band secret entry (16.12) |
| **13 sera-web Agent UX** | 13.7–13.12 | Permission approval UI, grants viewer, circle mgmt, secret entry modal, delegation UI, Centrifugo indicator |
| **17 Agent Identity & Delegation** | All (17.1–17.9) | ActingContext, service identities, operator/agent delegation, credential resolver, audit chain |
| **07 MCP Tool Registry** | 7.7 | sera-core as MCP server — Sera can now orchestrate the full instance |
| **10 Circles & Coordination** | All (10.1–10.6) | Circle management, constitutions, orchestration patterns, party mode |
| **18 Integration Channels** | All (18.1–18.10) | Unified channel model (ingress+egress), Slack/email/webhook adapters, actionable HitL, alert rules, topology UI (18.9), activity dashboard (18.10) |
| **15 Plugin SDK** | All (15.1–15.8) | Plugin interface, `@sera/mcp-sdk`, contributor docs, `sera` CLI |
| **14 sera-web Observability** | 14.5–14.6 | System health, schedule management UI |
| **09 Real-Time Messaging** | 9.6, 9.8 | Federation stub, webhooks |
| **04 LLM Proxy** | 4.7 | Rate limiting |
| **01 Infrastructure** | 1.7–1.9 | Backup/restore, instance identity, upgrade path |

**Phase 2 milestone:** Multiple operators with distinct identities and roles, agents delegating credentials, community-published MCP tools working in SERA, Discord-based HitL approvals, and a plugin SDK for the ecosystem.

---

## Phase 3 — Consolidation & Expansion: Clean architecture, IDE bridge, voice

**Target state:** All legacy shims removed, one coherent memory model, fully tested internals. IDE integration operational. Safe to hand off to community contributors.

| Epic | Stories to implement | Notes |
|---|---|---|
| **19 Memory System Consolidation** | All (19.1–19.5) | Retire Letta-style memory; migrate BaseAgent/WorkerAgent to Epic 8 scoped model; remove MemoryManager, Reflector; on-disk migration for legacy files |
| **21 ACP / IDE Bridge** | All | ACP stdio server, session mapping, multi-agent routing from IDE, sub-agent spawning, CWD injection, thinking level control |
| **23 Voice Interface** | Initial stories | Voice input via Web Speech API, TTS output, voice-to-chat routing, push-to-talk in sera-web |
| **24 A2A Federation Protocol** | All | Google A2A protocol (Linux Foundation standard) for external federation. Inbound A2A server, outbound client, Agent Card generation, instance pairing, capability gate. Internal comms stay on Centrifugo. |

**Phase 3 milestone:** Zero references to the old Letta memory system; developers can work with SERA agent teams from their IDE via ACP; basic voice interaction in sera-web; SERA instances federate via industry-standard A2A protocol.

---

## Phase 4 — Agent-Driven UI: Canvas and advanced interaction

**Target state:** Agents can push dynamic, interactive UI to the dashboard. Companion apps on the horizon.

| Epic | Stories to implement | Notes |
|---|---|---|
| **22 Canvas / A2UI** | All | A2UI message format, canvas panel in sera-web, agent canvas tools, Centrifugo streaming, component catalog |
| **23 Voice Interface** | Advanced stories | Wake words, continuous listening, companion app voice (deferred until mobile apps) |

**Phase 4 milestone:** Agents render rich visual output (dashboards, forms, visualisations) in the sera-web canvas panel alongside chat.

---

## Dependency constraints

These are hard prerequisites — do not start a story before its upstream is complete:

```
Phase 0 (v1 Prototype) → all later phases
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
Epic 18 (Integration Channels) → Epic 21 (ACP is a channel type)
Epic 09 (Real-Time Messaging) → Epic 21 (ACP uses WebSocket to sera-core)
Epic 12/13 (sera-web) → Epic 22 (Canvas renders in sera-web)
Epic 09 (Centrifugo) → Epic 22 (Canvas updates streamed via Centrifugo)
Epic 18 (chat sessions) → Epic 23 (Voice routes to chat sessions)
Epic 12/13 (sera-web) → Epic 23 (Voice UI in sera-web)
Epic 09 (Real-Time Messaging) → Epic 24 (Federation uses intercom)
Epic 16 (Auth) → Epic 24 (Instance authentication)
Epic 17 (Agent Identity) → Epic 24 (Cross-instance agent identity)
```

---

## Story ordering within an epic

Within each epic, implement stories in the order they are written — the numbering reflects the natural dependency chain. Exception: "deferred" stubs (P2/P3 stories) can always be skipped until their phase.

---

## Parallel work

Once Phase 0 (v1 Prototype) gate passes, Phase 1 work can be parallelised across tracks. Suggested agent assignments if multiple agents implement in parallel:

| Track | Epics | Best agent |
|---|---|---|
| **Core runtime** | 05 (remaining), 06, 07 | Claude Code |
| **Data & memory** | 08, 11 | Claude Code or Jules |
| **UI** | 13, 14 | Jules or Antigravity |
| **Messaging** | 09 | Claude Code |

Tracks are independent after Phase 0 is complete. The seams between tracks (context assembly calling memory, UI consuming audit API) are well-defined API contracts — teams can build against mocks until the implementation is ready.

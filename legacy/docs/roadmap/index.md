# Roadmap

SERA is organised as 29 epics across five development phases. Each phase produces a meaningful, runnable milestone.

## Phase 0 — v1 Prototype (Current Focus)

**Target:** A single agent (Sera) you can talk to via web UI and Discord, who reasons with tools, remembers across sessions, and makes proactive proposals.

| Track                   | What                                                         | Status          |
| ----------------------- | ------------------------------------------------------------ | --------------- |
| **A: Container + Chat** | Fix model routing, verify streaming, e2e smoke test, chat UI | Mostly complete |
| **B: Discord**          | Wire DiscordChatAdapter, slash commands, setup guide         | In progress     |
| **C: Memory**           | Qdrant auto-create, RAG cycle, embedding warnings, memory UI | In progress     |

See the [V1 Execution Plan](../SERA-V1-EXECUTION-PLAN.md) for detailed tracking.

## Phase 1 — Usable

Skills, MCP tools, enhanced memory, scheduling, egress proxy, full operator dashboard, interactive setup.

| Epic                       | Scope                                                  |
| -------------------------- | ------------------------------------------------------ |
| 06: Skill Library          | Skills injected into agent context                     |
| 07: MCP Tool Registry      | Containerised MCP servers                              |
| 08: Memory & RAG           | Hybrid search, categorisation, hierarchical scopes     |
| 09: Real-Time Messaging    | Channels, IntercomService, thought persistence         |
| 11: Scheduling & Audit     | Schedule engine + audit trail                          |
| 13: sera-web Agent UX      | Agent list/detail/create, chat, thoughts, memory graph |
| 14: sera-web Observability | Token usage, budget UI, audit viewer                   |
| 20: Egress Proxy           | Per-agent ACLs, audit integration                      |
| 27: Setup & Diagnostics    | `sera doctor`, setup wizard, onboarding                |

## Phase 2 — Ecosystem

Multi-operator auth, delegation, channels, plugins.

| Epic                            | Scope                                                  |
| ------------------------------- | ------------------------------------------------------ |
| 16: Auth & Secrets              | Full OIDC, Authentik, secrets interface                |
| 17: Agent Identity & Delegation | ActingContext, service identities, credential resolver |
| 10: Circles & Coordination      | Circle management, constitutions, party mode           |
| 15: Plugin SDK                  | `@sera/mcp-sdk`, contributor docs, `sera` CLI          |
| 18: Integration Channels        | Slack, email, webhook adapters, HitL                   |

## Phase 3 — Consolidation

Legacy cleanup, IDE integration, federation, voice.

| Epic                     | Scope                                              |
| ------------------------ | -------------------------------------------------- |
| 19: Memory Consolidation | Retire Letta-style memory, migrate to scoped model |
| 21: ACP/IDE Bridge       | IDE integration via ACP protocol                   |
| 23: Voice Interface      | Web Speech API, TTS, push-to-talk                  |
| 24: A2A Federation       | Google A2A protocol for instance pairing           |

## Phase 4 — Agent-Driven UI

| Epic            | Scope                                   |
| --------------- | --------------------------------------- |
| 22: Canvas/A2UI | Agents push dynamic UI to the dashboard |

## Phase 5 — Multimodal

| Epic                  | Scope                                         |
| --------------------- | --------------------------------------------- |
| 25: Media Processing  | Audio, image, video, PDF processing           |
| 26: Extended Channels | Telegram, WhatsApp, Signal, Matrix, iMessage  |
| 28: Image Generation  | DALL-E, Stability, ComfyUI integration        |
| 29: Web Intelligence  | Multi-provider search, readability extraction |

See [Implementation Order](../IMPLEMENTATION-ORDER.md) for the full dependency graph.

# SERA ↔ OpenClaw Gap Analysis

**Date:** 2026-04-02
**Reference:** `D:\projects\homelab\references\openclaw`

## Summary

This document catalogs feature gaps where OpenClaw has capabilities that SERA lacks or has only partially implemented. Gaps are organized into **new epics** (major feature areas not covered by SERA's existing 24 epics) and **parity enhancements** (improvements to existing epics inspired by OpenClaw's implementation).

---

## New Epics

### Epic 25: Media Processing Pipeline

**Priority:** P2 (Phase 2) | **Spec:** `docs/epics/25-media-processing-pipeline.md`

OpenClaw has a rich media understanding pipeline: audio transcription (Deepgram, OpenAI Whisper), image analysis via vision models, video frame extraction + summarization, and PDF text extraction with OCR fallback. SERA agents currently process text only.

**Key stories:** Media service with provider registry, audio transcription, image analysis, PDF extraction, video understanding, channel media attachment handling, media observability.

**OpenClaw reference:** `src/media-understanding/`, `src/agents/tools/pdf-tool.ts`, `src/agents/tools/image-tool.ts`

---

### Epic 26: Extended Channel Adapters

**Priority:** P3 (Phase 3) | **Spec:** `docs/epics/26-extended-channel-adapters.md`

OpenClaw supports 25+ messaging channels via a plugin architecture. SERA's Epic 18 covers Discord, Slack, Email, and Webhook. This epic adds Telegram, WhatsApp, Signal, Matrix, and iMessage adapters plus a channel plugin architecture for community-built adapters.

**Key stories:** Telegram adapter (grammy), WhatsApp adapter (Business API), Signal adapter (signal-cli), Matrix adapter (matrix-bot-sdk), iMessage adapter (BlueBubbles), channel plugin architecture.

**OpenClaw reference:** `src/channels/plugins/`, `src/channels/plugins/catalog.ts`, `src/channels/plugins/outbound/`

---

### Epic 27: Interactive Setup & Diagnostics

**Priority:** P1 (Phase 1) | **Spec:** `docs/epics/27-interactive-setup-and-diagnostics.md`

OpenClaw has a `doctor` diagnostic command, interactive setup wizard, guided provider configuration, and channel setup flows. SERA requires manual YAML editing and Docker debugging. This epic adds `sera doctor`, a first-run setup wizard, provider/channel setup flows, a health dashboard, and an onboarding checklist.

**Key stories:** `sera doctor` command, first-run wizard, provider setup flow, channel setup flow, web health dashboard, onboarding checklist.

**OpenClaw reference:** `src/commands/doctor/`, `src/wizard/`, `src/flows/`, `src/commands/setup/`

---

### Epic 28: Image Generation

**Priority:** P3 (Phase 3) | **Spec:** `docs/epics/28-image-generation.md`

OpenClaw has a built-in image generation tool with a provider registry. SERA agents cannot create images. This epic adds multi-provider image generation (DALL-E, Stability AI, local ComfyUI/A1111) as an agent tool with budget enforcement and chat/canvas display.

**Key stories:** Image generation service, cloud providers (DALL-E, Stability, Imagen), local providers (ComfyUI, A1111), `generate-image` agent tool, image display in sera-web.

**OpenClaw reference:** `src/image-generation/`, `src/agents/tools/image-generate-tool.ts`

---

### Epic 29: Enhanced Web Intelligence

**Priority:** P2 (Phase 2) | **Spec:** `docs/epics/29-enhanced-web-intelligence.md`

OpenClaw has multi-provider web search (Google, Brave, Tavily, SearXNG), readability-based content extraction (Mozilla Readability), citation tracking, and SSRF-safe fetching. SERA has basic web-fetch and web-search with raw content return. This epic upgrades web tools comprehensively.

**Key stories:** Multi-provider search, readability extraction, citation tracking, SSRF-safe HTTP client, JS-rendered page fetching, search/fetch observability.

**OpenClaw reference:** `src/agents/tools/web-search.ts`, `src/agents/tools/web-fetch.ts`, `src/link-understanding/`, `src/web-search/`

---

## Parity Enhancements to Existing Epics

### Epic 04: LLM Proxy & Governance

| Gap                                                                                                                                       | OpenClaw Reference           | SERA Status                          | Priority |
| ----------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------- | ------------------------------------ | -------- |
| **Thinking/reasoning level abstraction** — unified `low/medium/high/x-high` across all providers, auto-mapped to provider-specific params | `src/auto-reply/thinking.ts` | Issue #127 exists, no implementation | P1       |
| **Multi-account LLM auth with failover** — API key rotation, cooldown on rate limit, seamless failover between accounts                   | `src/agents/auth-profiles/`  | Issue #126 exists, no implementation | P2       |

### Epic 05: Agent Runtime

| Gap                                                                                                         | OpenClaw Reference                  | SERA Status | Priority |
| ----------------------------------------------------------------------------------------------------------- | ----------------------------------- | ----------- | -------- |
| **Memory flush before context compaction** — silent agent turn to persist memories before window compaction | `src/hooks/bundled/session-memory/` | Not tracked | P1       |
| **Boot-time markdown context injection** — inject custom markdown files into agent context at startup       | `src/hooks/bundled/boot-md/`        | Not tracked | P2       |
| **Command logging hook** — log all agent tool/command invocations for debugging                             | `src/hooks/bundled/command-logger/` | Not tracked | P2       |

### Epic 10: Circles & Coordination

| Gap                                                                                                                                                          | OpenClaw Reference                                                                           | SERA Status | Priority |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------- | ----------- | -------- |
| **Session spawn/yield/send tools** — inter-agent conversation delegation: spawn a new session, yield control to another agent, send messages across sessions | `src/agents/tools/sessions-spawn-tool.ts`, `sessions-yield-tool.ts`, `sessions-send-tool.ts` | Not tracked | P2       |

### Epic 15: Plugin SDK & Ecosystem

| Gap                                                                                                                             | OpenClaw Reference                                                | SERA Status                      | Priority |
| ------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------- | -------------------------------- | -------- |
| **Channel plugin manifest pattern** — declarative `plugin.json` with capabilities, config schema, required secrets, setup guide | `src/channels/plugins/types.plugin.ts`, `src/plugins/manifest.ts` | Issue #149 partially covers this | P2       |

### Epic 18: Integration Channels

| Gap                                                                                                          | OpenClaw Reference                                    | SERA Status                               | Priority |
| ------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------- | ----------------------------------------- | -------- |
| **Interactive polls** — agents create native polls on Discord/Telegram, collect results                      | `src/polls.ts`, `src/channels/plugins/actions/`       | Not tracked                               | P3       |
| **DM pairing / inbound access control** — challenge-response pairing for unknown senders across all channels | `src/pairing/`                                        | Issue #151 exists, needs full spec        | P2       |
| **Message reactions** — agents receive and respond to message reactions                                      | `src/channels/plugins/actions/reaction-message-id.ts` | Not tracked                               | P3       |
| **Telegram adapter**                                                                                         | `src/channels/plugins/` (via plugin)                  | Issue #152 exists, now covered by Epic 26 | P3       |

---

## Features Where SERA Is Already Ahead

These are areas where SERA has a stronger architecture than OpenClaw:

| Area                    | SERA Advantage                                               | OpenClaw Approach                        |
| ----------------------- | ------------------------------------------------------------ | ---------------------------------------- |
| **Process isolation**   | Per-agent Docker containers                                  | In-process (single Node.js process)      |
| **Audit trail**         | Merkle hash-chain, tamper-evident                            | JSONL files, no integrity verification   |
| **Network security**    | Per-agent Squid ACLs, SNI filtering, egress metering         | SSRF guards for browser only             |
| **LLM governance**      | Centralized proxy with token budgets, per-agent metering     | In-process direct calls, no metering     |
| **Agent collaboration** | Circles with shared memory, intercom, orchestration patterns | Isolated agents, basic session routing   |
| **Agent config**        | Template → Instance (reusable, publishable blueprints)       | Flat per-agent config                    |
| **Scheduling**          | pg-boss with cron, dead-letter, dedup                        | Basic cron with isolated agent spawn     |
| **Database**            | PostgreSQL + Qdrant (queryable, ACID, vector search)         | Filesystem-only (JSON5, JSONL, Markdown) |
| **Secret management**   | Encrypted PostgreSQL store, per-call injection, rotation     | Environment variables, config files      |

---

## Implementation Priority

### Phase 1 (immediate value)

- Epic 27 (Setup & Diagnostics) — lowers barrier to entry
- Thinking/reasoning abstraction (Epic 04 enhancement)
- Memory flush before compaction (Epic 05 enhancement)

### Phase 2 (ecosystem growth)

- Epic 25 (Media Processing) — multimodal capabilities
- Epic 29 (Web Intelligence) — better research tools
- Multi-account LLM auth (Epic 04 enhancement)
- DM pairing full spec (Epic 18 enhancement)
- Session delegation tools (Epic 10 enhancement)

### Phase 3 (channel breadth)

- Epic 26 (Extended Channels) — Telegram, WhatsApp, Signal, Matrix
- Epic 28 (Image Generation) — creative output
- Polls and reactions (Epic 18 enhancements)

---

## Relationship to Existing Epics

```
Phase 0 (v1 Prototype) → all later phases
Epic 18 (Channels) → Epic 26 (Extended Channels)
Epic 05 (Runtime) → Epic 25 (Media Processing)
Epic 15 (Plugin SDK) → Epic 26 Story 26.6 (Channel plugin arch)
Epic 15 (Plugin SDK) → Epic 27 (CLI commands)
Epic 04 (LLM Proxy) → Epic 28 (Image Generation budget)
Epic 20 (Egress Proxy) → Epic 29 (SSRF-safe fetch)
Epic 25 (Media) → Epic 29 Story 29.2 (PDF fallback to media pipeline)
```

# OpenClaw Analysis — What to Adopt, What Differentiates SERA

**Date:** 2026-03-22
**Source:** `D:\projects\homelab\references\openclaw`

OpenClaw is a personal AI assistant platform focused on running across 25+ messaging channels with companion apps for macOS/iOS/Android. This document captures what SERA should adopt, what we already do better, and where the platforms diverge architecturally.

---

## Table of Contents

1. [Architecture Comparison](#architecture-comparison)
2. [Features to Adopt](#features-to-adopt)
3. [SERA Differentiators](#sera-differentiators)
4. [New Epic Candidates](#new-epic-candidates)
5. [Enhancements to Existing Epics](#enhancements-to-existing-epics)

---

## Architecture Comparison

| Aspect | SERA | OpenClaw |
|---|---|---|
| **Core model** | Docker-native multi-agent orchestration | Gateway-centric personal assistant hub |
| **Agent execution** | Separate Docker container per agent | In-process (same Node.js process) |
| **Storage** | PostgreSQL + Qdrant | Filesystem (JSON5 config, JSONL sessions, Markdown memory) |
| **Real-time** | Centrifugo pub/sub (channels, presence, history recovery) | Raw WebSocket from gateway |
| **LLM access** | Proxied through Core (metered, budgeted, circuit-broken) | In-process direct calls, no centralised budget |
| **Audit** | Merkle hash-chain in PostgreSQL | JSONL logs, no integrity verification |
| **Agent config** | Template → Instance (class/instance, like Helm charts) | Flat agent config with workspace isolation |
| **Network control** | Per-agent Squid ACLs, SNI filtering, bandwidth limiting | SSRF policy for browser sandbox only |
| **Security model** | Capability intersection (Boundary ∩ Policy ∩ Overrides ∩ RuntimeGrants) | Three-way sandbox toggle (off/non-main/all) |
| **Memory** | Structured block store + Qdrant vector search | Plain Markdown files + optional vector search |
| **Channels** | Unified Channel interface (Epic 18) — ingress/egress with binding modes | 25+ messaging channel adapters, gateway-centric routing |
| **Companion apps** | Web dashboard + TUI (mobile deferred) | macOS, iOS, Android native apps |
| **Voice** | Not yet | Wake words, Talk Mode, PTT, TTS |
| **IDE integration** | Not yet | ACP (Agent Client Protocol) stdio bridge for Zed, VS Code |
| **Plugin ecosystem** | Plugin SDK (Epic 15), skill ecosystem in progress | ClawHub marketplace, `openclaw plugins install` |
| **Multi-agent** | Circles with shared memory, intercom, orchestration patterns | Isolated agents bound to channels via bindings |

---

## Features to Adopt

### 1. Memory Flush Before Context Compaction

**What:** Before compacting the context window, a silent agent turn reminds the model to persist important memories to disk.

**Why:** Prevents information loss during long sessions — the model gets a chance to save anything worth remembering before old context is evicted.

**Where in SERA:** Epic 08 (Memory & RAG) or Epic 05 (Agent Runtime, context window management). Add as a story or enhancement to context compaction logic.

**OpenClaw reference:** `agents.defaults.compaction.memoryFlush` config, works only if workspace is writable.

### 2. Multi-Account LLM Auth with Failover

**What:** Multiple API key accounts per LLM provider with cooldown/retry on rate limits, auto-rotation on 429s.

**Why:** Heavy workloads (multiple agents hitting the same provider) can exhaust a single key's rate limit. Rotation improves throughput without upgrading plans.

**Where in SERA:** Epic 04 (LLM Proxy) — enhance `ProviderRegistry` to support multiple credentials per provider with round-robin and backoff.

### 3. Thinking/Reasoning Level Abstraction

**What:** Unified `low`/`medium`/`high`/`x-high` thinking levels normalised across providers (Anthropic extended thinking, OpenAI o1/o3 reasoning, Qwen3 streaming thinking).

**Why:** Agent templates shouldn't need to know provider-specific reasoning knobs. A single `thinking: high` field abstracts across all backends.

**Where in SERA:** Epic 04 / `LlmRouter` — add a `thinking` field to the LLM proxy request that maps to provider-specific params.

### 4. Diagnostic CLI (`doctor` command)

**What:** `openclaw doctor` checks config validity, credential presence, connectivity to providers, database access, container runtime health.

**Why:** SERA's multi-container Docker setup has many failure modes (missing env vars, Centrifugo misconfigured, database unreachable, Squid not proxying). A diagnostic command catches these fast.

**Where in SERA:** `sera` CLI (Epic 15, Story 15.3) — add `sera doctor` subcommand.

### 5. Hybrid Memory Search (BM25 + Semantic)

**What:** Combine keyword search (BM25) with vector search, MMR diversity re-ranking, and temporal decay.

**Why:** Vector search alone misses exact keyword matches (config names, error codes, IDs). Hybrid improves recall.

**Where in SERA:** Epic 08 (Memory & RAG) — add BM25 path to `MemoryManager.search()` alongside Qdrant. PostgreSQL's `tsvector` could serve as the BM25 backend.

### 6. DM Pairing / Inbound Access Control

**What:** Challenge-response approval for unknown senders on messaging channels. Bot sends 8-char code, owner approves via CLI/UI, sender permanently added to allowlist. Four policies: `open`, `pairing` (default), `allowlist`, `disabled`.

**Why:** Essential for inbound channels — SERA needs to decide who gets through when someone messages an agent on Discord/Telegram/etc. Prevents unauthorised access to agents with tool capabilities.

**Where in SERA:** Epic 18 (Integration Channels) — add as a new story or enhancement to Story 18.7 (Inbound message routing). Adds a `dmPolicy` field to channel config and a `channel_allowlists` table.

**Federation extension:** The pairing model can extend to cross-instance trust — instance A "pairs" with instance B via challenge exchange, establishing a trusted channel for agent-to-agent communication across homelab boundaries. Feeds into Epic 09 Story 9.6 (Federation stub).

### 7. Plugin Manifest Pattern

**What:** Declarative `openclaw.plugin.json` with declared channels, providers, auth env vars, config schemas — machine-readable plugin metadata.

**Why:** SERA's current plugin/skill registration is code-driven. A declarative manifest enables tooling (validation, discovery, marketplace listing, dependency checking) without loading the plugin.

**Where in SERA:** Epic 15 (Plugin SDK) — add a manifest spec (e.g. `sera-plugin.json`) to Story 15.1 or as a new story.

---

## SERA Differentiators

These are areas where SERA's architecture is fundamentally stronger. They should be emphasised in documentation and positioning.

### 1. True Process Isolation
Agents run in separate Docker containers. One agent crashing, hanging, or being compromised cannot affect others. OpenClaw runs all agents in-process — a single misbehaving agent can take down the entire gateway.

### 2. Tiered Sandbox Boundaries with Capability Intersection
SERA's `tier-1/2/3` boundaries with fine-grained capabilities (`Boundary ∩ Policy ∩ Overrides ∩ RuntimeGrants`) are significantly more sophisticated than OpenClaw's `off/non-main/all` toggle. You can precisely control each agent's filesystem scope, network access, resource limits, and tool allowlist — all enforced at the infrastructure level.

### 3. Per-Agent Network ACLs via Egress Proxy
All agent outbound traffic routes through Squid with per-agent ACL files, SNI-based HTTPS filtering, and bandwidth rate limiting. OpenClaw has SSRF policy for the browser sandbox but no general network-level isolation.

### 4. Merkle Hash-Chain Audit Trail
Every agent action produces a tamper-evident, hash-chained audit record in PostgreSQL. OpenClaw logs to JSONL files with no integrity verification. Critical for running agents with real-world side effects.

### 5. Template → Instance Separation
Reusable, community-publishable agent templates (like Helm charts) with instance-level overrides. OpenClaw's agents are flat workspace configurations. SERA's model enables an ecosystem of shared blueprints.

### 6. Centralised LLM Proxy with Budget Enforcement
Agents never call LLMs directly. All calls go through sera-core's proxy with JWT auth, per-agent metering, and configurable token budgets (`maxLlmTokensPerHour`, `maxLlmTokensPerDay`). OpenClaw has no centralised budget control.

### 7. Circles (Agent Teams)
Named groups of agents with shared memory namespaces, intercom channels, constitutions, and orchestration patterns (including party mode). OpenClaw's multi-agent is "isolated agents bound to channels" — no concept of collaborative agent teams.

### 8. Real Databases
PostgreSQL for relational data + Qdrant for vectors vs OpenClaw's file-based everything. Proper databases enable complex queries, ACID transactions, concurrent access, and scale beyond what flat files can handle.

---

## New Epic Candidates

### Epic 21: ACP / IDE Bridge

**What:** Implement Agent Client Protocol (ACP) support so IDEs (Zed, VS Code, others) can route prompts to SERA agents through a stdio bridge.

**Why:** SERA already has isolated agent containers, capability policies, and intercom. Adding ACP lets developers work with a full agent team from their IDE — architect reviews design, developer writes code in a sandbox, QA runs tests — all with SERA's trust guarantees (audit, budget, isolation).

**OpenClaw reference:** `src/acp/server.ts`, `src/acp/translator.ts`, `src/acp/session-mapper.ts`

**Key stories:**
- ACP stdio server that connects to sera-core via WebSocket
- Session mapping: ACP client sessions → SERA chat sessions
- Multi-agent routing: IDE prompts routed to specific agents or circles
- Sub-agent spawning from IDE context
- Working directory context injection (prefix prompts with CWD)
- Thinking level control per prompt

**Dependencies:** Epic 18 (channel model — ACP is effectively another channel type), Epic 09 (real-time messaging)

**Phase:** 3 or 4

---

### Epic 22: Canvas / Agent-Driven UI (A2UI)

**What:** Allow agents to push dynamic, interactive UI to the sera-web dashboard (and eventually companion apps). Uses a declarative component format where agents describe UI intent and the client renders it natively.

**Why:** Agents communicating only via chat text is limiting. An agent should be able to show a live infrastructure dashboard, a deployment approval form, a debugging visualization, or an interactive workflow — pushed dynamically as part of a conversation.

**OpenClaw reference:** A2UI v0.8 format — declarative JSONL with `surfaceUpdate`, `beginRendering`, `dataModelUpdate`, `deleteSurface` messages. Component types: `Column`, `Row`, `Card`, `Text`, `Image`, `TextField`. Designed to be LLM-friendly (flat component list, incremental updates) and security-first (declarative data, not executable code).

**Key stories:**
- A2UI message format spec (adapted for SERA — may use a simpler v1)
- Canvas panel component in sera-web (per-agent or per-session)
- Agent tool: `canvas.push`, `canvas.reset`, `canvas.snapshot`
- Centrifugo channel for canvas updates (real-time rendering)
- Component catalog (approved component types the agent can request)
- Canvas eval (optional — run JS in the panel for inspection)

**Dependencies:** Epic 12/13 (sera-web), Epic 09 (Centrifugo)

**Phase:** 4+

---

### Epic 23: Voice Interface

**What:** Voice input/output for agent interactions — push-to-talk, continuous listening, wake words, text-to-speech responses.

**Why:** Critical for non-desktop contexts: talking to office agents, family agents, private agents. Full control over the interaction (unlike cloud assistants).

**Key stories:**
- Voice input via Web Speech API in sera-web (browser-based, no companion app needed initially)
- TTS output (browser SpeechSynthesis API or external like ElevenLabs)
- Voice-to-text routing to chat sessions
- Wake word detection (deferred — requires always-listening, better suited to companion apps)
- Push-to-talk mode in sera-web

**Dependencies:** Epic 18 (chat sessions), Epic 12/13 (sera-web)

**Phase:** 4+

---

## Enhancements to Existing Epics

### Epic 18 — Integration Channels

**Add: DM pairing / inbound access control (Story 18.11)**

When external platform channels accept inbound messages, SERA needs to decide who gets through. Add `dmPolicy` to channel config with four modes: `open`, `pairing` (challenge-response default), `allowlist`, `disabled`.

- `channel_allowlists` table: `{ channel_id, sender_id, approved_at, approved_by }`
- Pairing flow: unknown sender → 8-char code sent back → operator approves via UI/CLI/slash command → sender added to allowlist
- Group vs DM separation: groups require explicit pre-config, DMs can use dynamic pairing
- Integrates with operator identity mapping (Stories 18.4, 18.5)

**Add: Telegram channel adapter (Story 18.12)**

OpenClaw supports Telegram as a first-class channel. Given SERA's move to full bidirectional chat, Telegram should be added alongside Discord and Slack. Similar structure to Story 18.4 but using Telegram Bot API (`node-telegram-bot-api` or `telegraf`).

**Add: Federation pairing (enhancement to Epic 09 Story 9.6)**

Extend the DM pairing model for cross-instance trust. SERA instance A can "pair" with instance B by exchanging a challenge code, establishing a trusted channel for agent-to-agent communication across homelab boundaries.

---

### Epic 04 — LLM Proxy & Governance

**Add: Multi-account auth with failover**

Enhance `ProviderRegistry` to support multiple API keys per provider. Round-robin with backoff on 429s. Configurable in `providers.json` as an array of credentials per provider.

**Add: Thinking level abstraction**

Add `thinking: 'low' | 'medium' | 'high' | 'x-high'` to the LLM proxy request. `LlmRouter` maps to provider-specific params (Anthropic extended thinking budget, OpenAI reasoning effort, Qwen3 thinking mode). Agent templates declare a default thinking level.

---

### Epic 05 — Agent Runtime

**Add: Memory flush before context compaction**

Before compacting the context window, inject a silent system turn asking the model to persist important information to memory. Skip if workspace/memory is read-only.

---

### Epic 08 — Memory & RAG

**Add: BM25 hybrid search**

Add keyword search (BM25 via PostgreSQL `tsvector`) alongside Qdrant vector search. Merge results with configurable weighting. Optionally add MMR diversity re-ranking and temporal decay.

---

### Epic 15 — Plugin SDK & Ecosystem

**Add: Plugin manifest spec**

Define `sera-plugin.json` format with: `id`, `name`, `version`, `pluginTypes` (skill/storage/audit/auth/channel), `requiredEnvVars`, `configSchema`, `dependencies`. Enables validation, discovery, and marketplace listing without loading code.

**Add: `sera doctor` command**

Add to `sera` CLI (Story 15.3): checks config validity, credential presence, provider connectivity, database access, Centrifugo health, Squid proxy health, Docker daemon reachability.

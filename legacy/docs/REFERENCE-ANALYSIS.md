# Reference Project Analysis — What to Adopt for SERA

**Date:** 2026-03-22 (updated with additional projects)
**Source:** `D:\projects\homelab\references\*` (14 projects) + web research (7 additional projects/protocols)

This document captures the competitive landscape, what SERA should adopt from each project, what we already do better, and new epic/story candidates that emerge from the analysis.

---

## Table of Contents

1. [Landscape Overview](#landscape-overview)
2. [Per-Project Summaries](#per-project-summaries)
3. [Additional Projects (Web Research)](#additional-projects-web-research)
4. [A2A Protocol — Federation Standard](#a2a-protocol--federation-standard)
5. [Cross-Cutting Features to Adopt](#cross-cutting-features-to-adopt)
6. [SERA Differentiators](#sera-differentiators)
7. [New Epic Candidates](#new-epic-candidates)
8. [Enhancements to Existing Epics](#enhancements-to-existing-epics)

---

## Landscape Overview

| Project | Language | Category | Key Innovation | SERA Relevance |
|---|---|---|---|---|
| **OpenClaw** | TypeScript | Personal AI assistant | 25+ messaging channels, A2UI canvas, DM pairing | High — channel arch, IDE bridge, voice |
| **OpenFang** | Rust | Agent OS | 16 security layers, WASM sandbox, Merkle audit, 40 channels | High — closest competitor in philosophy |
| **AutoGen** | Python/.NET | Multi-agent framework | CloudEvents pub/sub, topic routing, cross-language gRPC | Medium — orchestration patterns |
| **CrewAI** | Python | Agent teams | Unified memory with composite scoring, Flow system | Medium — memory, orchestration |
| **Letta** | Python | Stateful agents | Three-tier memory (core/archival/recall), sleeptime agent | Medium — memory architecture |
| **OpenHands** | Python/TS | AI dev platform | SWE-Bench 77.6%, microagents, multi-runtime (Docker/K8s) | Medium — runtime flexibility |
| **Goose** | Rust | Agent framework | MCP-first, toolshim, error-as-information, recipe system | Medium — tool patterns |
| **Docker Agent** | Go | Docker CLI plugin | OCI distribution, declarative YAML, A2A federation | Medium — distribution, federation |
| **HERM** | Go | Coding agent | Container-first safety, self-building devenv, token-efficient tools | High — container philosophy |
| **Serena** | Python | Coding toolkit | LSP symbol-level ops via MCP, name-path navigation | Medium — agent coding tools |
| **OpenCode** | TypeScript | AI coding CLI | Provider-agnostic, Effect library, Claude Code skill compat | Low — TUI patterns |
| **Devika** | Python | AI SWE | Browser-first research, internal monologue, state visualization | Low — transparency patterns |
| **BMAD-METHOD** | JavaScript | Dev methodology | Party mode, project-context constitution, scope escalation | Low — workflow patterns |
| **pi-mono** | TypeScript | LLM abstraction | Unified provider API, TypeBox schemas, lazy loading | Already integrated |
| | | | | |
| **A2A Protocol** | Spec (Google/LF) | Federation standard | Agent Cards, task lifecycle, JSON-RPC+SSE+gRPC | **High — adopt for Epic 24** |
| **Agent Zero** | Python | Multi-agent framework | Docker sub-agents, MCP, A2A, SearXNG | Medium — confirms direction |
| **OpenSandbox** | Python | Sandbox platform | Multi-lang SDK, K8s runtime, VNC desktops | Medium — K8s scaling |
| **DeerFlow 2.0** | Python | SuperAgent harness | Parallel sub-agents, sandbox, LangGraph | Low-Medium |
| **Dify** | Python/TS | Workflow builder | Visual builder, RAG, 1.4M installs, $30M funding | Low arch / High market ref |
| **AIO Sandbox** | TypeScript | All-in-one sandbox | Browser+Shell+MCP+VSCode in one container | Low — opposite philosophy |
| **Moltworker** | JS | Edge agent | Cloudflare Workers, always-on, R2 storage | Low — edge pattern |

---

## Per-Project Summaries

### OpenClaw
**What it is:** Personal AI assistant running across 25+ messaging channels (WhatsApp, Telegram, Discord, etc.) with companion apps.

**Architecture:** Gateway-centric hub. All agents run in-process (same Node.js). File-based storage (JSON5/JSONL/Markdown). Native WebSocket.

**Adopt:** Full bidirectional chat channels, DM pairing for inbound access control, ACP/IDE bridge, Canvas/A2UI agent-pushed UI, memory flush before context compaction, multi-account LLM auth with failover, thinking level abstraction, `doctor` CLI command, hybrid BM25+vector memory search, plugin manifest pattern.

**Detailed analysis in prior conversation — see `memory/reference_openclaw.md` and `memory/project_openclaw_adoption.md`.**

---

### OpenFang
**What it is:** Rust-based "Agent Operating System" — single ~32MB binary, 137K LOC, 1,767 tests.

**Architecture:** 14-crate workspace. Kernel with capability-based RBAC, WASM sandboxing (Wasmtime), Merkle hash-chain audit, 40 channel adapters, SQLite storage, OFP P2P wire protocol.

**Key concepts SERA should study:**
- **Information flow taint tracking** — labels propagate through execution, secrets tracked source→sink. SERA could add taint labels to capability resolution.
- **WASM dual metering** — fuel (instruction count) + epoch interruption (wall-clock timeout). Complements SERA's Docker isolation for lightweight workloads.
- **Prompt injection scanner** — detects override attempts, exfiltration patterns, shell references in skill content. SERA's agent-runtime (Epic 5, Story 5.10) covers prompt injection defence but could adopt pattern-based scanning.
- **Loop guard** — SHA256-based tool call dedup (3x warn, 5x block, 30x circuit break). Simple, effective for SERA's reasoning loop.
- **"Hands" concept** — pre-built autonomous capability packages (video processing, lead gen, research, etc.) with multi-phase playbooks. Maps to SERA's agent templates but with richer built-in automation.
- **Session repair** — 7-phase message history validation and auto-recovery. SERA's session management could adopt similar integrity checking.
- **40 channel adapters** — confirms the direction of full bidirectional chat for SERA.

**SERA already does better:** Docker container isolation (vs WASM), PostgreSQL+Qdrant (vs SQLite), Centrifugo pub/sub (vs raw WS), template→instance separation, per-agent network ACLs via Squid.

---

### AutoGen (Microsoft)
**What it is:** Event-driven, cross-language multi-agent framework (Python + .NET).

**Architecture:** Three-layer API (Core → AgentChat → Extensions). CloudEvents messaging, gRPC distributed runtime, topic-based pub/sub.

**Key concepts SERA should study:**
- **Topic-based pub/sub with matcher/mapper functions** — pure functions that route messages by topic pattern. SERA's Centrifugo channels are simpler but could benefit from declarative routing rules.
- **AgentTool pattern** — wrap an agent as a callable tool for another agent. SERA's subagent spawning is more isolated (containers) but the "agent-as-tool" interface is cleaner for lightweight delegation.
- **SocietyOfMindAgent** — manages multiple internal sub-agents, orchestrates their communication, aggregates responses. Maps to SERA's circle orchestration patterns.
- **Workbench pattern** — tools organized into workbenches with shared state, lifecycle management, dynamic availability. Cleaner than flat tool lists.
- **Cross-language via Protocol Buffers** — SERA is TypeScript-only, but if Go TUI needs to call core services, protobuf contracts would be cleaner than REST.
- **Termination conditions as composable objects** — `MaxMessage | TextMention & ExternalSignal`. SERA's task completion is ad-hoc; composable conditions would help.

---

### CrewAI
**What it is:** Python framework for orchestrating autonomous AI agent teams.

**Architecture:** Dual paradigm — Crews (autonomous teams) + Flows (event-driven workflows). Pydantic throughout. LiteLLM for routing.

**Key concepts SERA should study:**
- **Unified memory with composite scoring** — `semantic(0.5) + recency(0.3) + importance(0.2)`. SERA's Qdrant search is vector-only. Adding recency decay and importance weighting would improve recall quality.
- **Hierarchical memory scopes** — path-based (`/project/decisions`, `/agent/researcher`). SERA has per-agent and per-circle namespaces but no hierarchical scoping within those.
- **Flow system** — `@start`, `@listen`, `@router` decorators for production workflows with state persistence. More structured than SERA's current task queue.
- **Tool-based collaboration** — agents delegate via `DelegateWork` and `AskQuestion` tools rather than hard-coded routing. More flexible than direct agent-to-agent messaging.
- **Memory flush on save** — LLM analyses importance/scope/categories when persisting memories. SERA saves raw blocks; LLM-guided categorisation would improve retrieval.

---

### Letta (formerly MemGPT)
**What it is:** Platform for building stateful agents with advanced memory systems.

**Architecture:** FastAPI server, three-tier memory (core blocks + archival passages + recall messages), PostgreSQL+pgvector, tool sandbox.

**Key concepts SERA should study:**
- **Editable memory blocks** — user-readable text blocks (persona, human, custom) rendered in system prompt with character limits. More transparent than opaque vector embeddings.
- **Sleeptime agent** — background thread for memory consolidation separate from response generation. Prevents user-facing latency during memory management.
- **Tool rules** — constraints on which tools can call which others, preventing infinite loops and enforcing valid workflows. `TerminalToolRule` marks tools that end the loop.
- **Core memory as system prompt injection** — memory blocks rendered as XML tags in the system prompt with metadata (chars_current, chars_limit). Agent self-edits via `core_memory_append`/`core_memory_replace`.
- **Agent types** — different agent behaviors (memgpt, react, workflow, sleeptime, voice). SERA's agent templates define behavior via skills/tools but don't have first-class behavioral modes.

---

### OpenHands
**What it is:** AI-driven development platform (SWE-Bench 77.6%).

**Architecture:** FastAPI + React/Remix. EventStream pub/sub. Multiple runtimes (Docker, Local, Remote, Kubernetes).

**Key concepts SERA should study:**
- **Microagents/skills as trigger-based knowledge** — markdown files with YAML frontmatter, activated by keyword matching in messages. SERA's skills are explicitly assigned; trigger-based activation could complement this.
- **Multi-runtime abstraction** — same agent code runs on Docker, local, remote, or Kubernetes. SERA is Docker-only; a runtime interface could enable K8s deployment for scaling.
- **CodeAct paradigm** — unifies all agent actions into code/tool calls. No custom parsing per action type.
- **Event condensing** — LLM-based summarization of old events to prevent unbounded history growth. Similar to OpenClaw's compaction flush.

---

### Goose (Block)
**What it is:** Rust-based on-machine AI agent framework with MCP-first extension model.

**Architecture:** Axum HTTP server, 50+ LLM providers, MCP for all extensions, recipe system for repeatable tasks.

**Key concepts SERA should study:**
- **Error-as-information** — errors sent back to LLM as prompts for recovery, not treated as fatal. Two error types: traditional (network, crashes) vs agent (bad tool calls). SERA's agent-runtime could adopt this pattern.
- **Toolshim** — specialized small model selected to route tool calls, separate from main reasoning model. Optimises cost/performance for tool selection.
- **Recipe system** — YAML-based task specifications with parameters, sub-recipes, response schemas, retry policies. More structured than SERA's current task queue.
- **Context compaction via summarizer model** — uses smaller/faster LLM to summarize conversation history when context exceeds 80%. Cheaper than using the main model.

---

### Docker Agent
**What it is:** Go-based Docker CLI plugin for building, running, and sharing AI agents.

**Architecture:** Single Go binary. Declarative YAML config. MCP-first tooling. OCI distribution.

**Key concepts SERA should study:**
- **OCI distribution** — agents packaged as OCI artifacts, pushed/pulled from registries like container images. SERA's agent templates are YAML files; OCI distribution could enable `sera pull template/developer:v2`.
- **A2A (Agent-to-Agent) protocol** — HTTP-based federation where agents expose `/a2a` endpoints. More structured than SERA's federation stub (Epic 9, Story 9.6).
- **Tool-per-model override** — each tool can specify a different LLM for its result processing. SERA routes all tools through the same model.
- **Hooks system** — `pre_tool_use`, `post_tool_use`, `post_completion` hooks configurable in YAML. SERA could adopt for skill/tool lifecycle.
- **RAG built-in** — BM25 + embedding hybrid search with reranking and fusion. Confirms the direction for SERA's memory system.

---

### HERM
**What it is:** Go-based containerized coding agent where Docker isolation is the default.

**Architecture:** Single Go binary. Docker container per session. langdag for LLM routing + conversation DAG. SQLite persistence.

**Key concepts SERA should study:**
- **No approval prompts** — because the container can't break the host, permission gates are unnecessary. SERA already uses containers but still has permission requests (Epic 3, Story 3.9). For tier-1 agents, SERA could adopt the "container = safety" model and skip approval.
- **Self-building dev environments** — agent writes `.herm/Dockerfile`, builds it, hot-swaps the running container. Image persists across sessions. SERA's agent containers use static images; dynamic Dockerfile evolution per agent would be powerful.
- **Token-efficient file tools** — dedicated `glob`, `grep`, `read_file` tools with structured JSON I/O instead of bash wrappers. 3x more token-efficient than raw shell commands.
- **Exploration model routing** — separate cheap model (Haiku) for sub-agents and context compaction. SERA's LlmRouter could support per-purpose model selection.
- **Context clearing at 80%** — old tool results replaced with `[cleared — re-read if needed]` placeholder. Simple, effective context management.

---

### Serena
**What it is:** LSP-powered coding toolkit exposing symbol-level operations via MCP.

**Architecture:** Python agent wrapping 30+ language servers. MCP server for tool exposure. Symbol-level code operations.

**Key concepts SERA should study:**
- **Symbol-level code operations** — `FindSymbol`, `ReplaceSymbolBody`, `InsertAfterSymbol` instead of file/line-based edits. More precise, fewer tokens, fewer errors.
- **Name-path navigation** — `MyClass/my_method[0]` to identify symbols hierarchically. Language-agnostic abstraction over LSP.
- **MCP exposure of coding tools** — SERA agents that need to code could connect to a Serena MCP server for symbol-aware editing. No need to build our own LSP integration.

---

### OpenCode
**What it is:** Open-source Claude Code alternative with provider-agnostic LLM routing.

**Architecture:** Bun + TypeScript monorepo. Solid.js TUI. Effect library for typed errors. Drizzle ORM + SQLite.

**Key concepts:** Skill compatibility with Claude Code (`.claude/skills/`), permission-first design, session compaction. Less novel for SERA but confirms patterns.

---

### Devika
**What it is:** AI software engineer with browser-first research capability.

**Architecture:** Flask + Svelte. Modular agent chain (Planner→Researcher→Coder→Runner). SQLite.

**Key concepts:** Internal monologue for transparency (agent's thinking surfaced to user), contextual keyword accumulation for focused research, browser-as-first-class-tool. Less architecturally relevant but the monologue pattern could enhance SERA's thought streaming.

---

### BMAD-METHOD
**What it is:** AI-driven agile development methodology framework (not a runtime).

**Architecture:** npm package. Skill-based invocation. YAML agent configs. Jinja2-like workflow templates.

**Key concepts:** "Party mode" multi-agent discussions, project-context.md as agent "constitution" (loaded by all agents for consistency), scope escalation detection (light→heavy), adversarial review tools. The constitution pattern maps to SERA's circle-level shared context.

---

### pi-mono
**What it is:** Unified LLM abstraction layer. **Already integrated into SERA.**

Validates our architecture. Key capabilities: unified reasoning API across providers, TypeBox schemas for tool definitions, lazy provider loading, streaming partial JSON for tool arguments, per-message cost tracking.

---

## Additional Projects (Web Research)

Projects not in the local references folder but worth tracking.

### Agent Zero
**What it is:** Open-source multi-agent framework where agents spawn subordinates in isolated Docker containers. Python-based, supports MCP, A2A, and multiple LLM providers.

**Key concepts:** Subordinate agent spawning with dedicated prompts/tools/sandbox per child, SearXNG integration for privacy-respecting search, extensions system for behaviour modification, cross-platform skills.

**SERA relevance:** Medium. Similar container-first approach but less sophisticated capability model. Confirms SERA's direction.

**Source:** [github.com/agent0ai/agent-zero](https://github.com/agent0ai/agent-zero)

### OpenSandbox (Alibaba)
**What it is:** General-purpose sandbox platform for AI agents. Released March 2026, Apache 2.0. Four-layer architecture: SDKs → Specs → Runtime → Sandbox Instances.

**Key concepts:** Multi-language SDKs (Python, Java, JS, C#), unified OpenAPI spec for sandbox lifecycle, Docker + Kubernetes runtimes, full VNC desktops for GUI agents, browser automation. FastAPI-based server manages sandbox lifecycles.

**SERA relevance:** Medium. SERA's SandboxManager fills a similar role but is TypeScript-only. OpenSandbox's multi-language SDK approach and K8s runtime support are worth studying if SERA needs to scale beyond single-node Docker.

**Source:** [github.com/alibaba/OpenSandbox](https://github.com/alibaba/OpenSandbox)

### DeerFlow 2.0 (ByteDance)
**What it is:** Open-source SuperAgent harness built on LangGraph. Ground-up rewrite (v2 shares no code with v1). Topped GitHub Trending on release (Feb 2026).

**Key concepts:** Isolated sandbox with full filesystem/shell, sub-agent orchestration (parallel, up to 3 concurrent), memory system, extensible skills. Handles tasks from minutes to hours.

**SERA relevance:** Low-Medium. Confirms parallel sub-agent spawning and sandbox isolation as mainstream patterns. LangGraph dependency makes it less portable.

**Source:** [github.com/bytedance/deer-flow](https://github.com/bytedance/deer-flow)

### Dify
**What it is:** Most-starred agentic workflow platform on GitHub. Visual workflow builder with RAG, agent capabilities, model management. $30M funding at $180M valuation (March 2026). 1.4M+ installs.

**Key concepts:** Visual drag-and-drop workflow builder (Beehive architecture), built-in RAG pipeline (pgvector default, Milvus/Chroma supported), prompt IDE, 200+ LLM integrations, self-hosted with Docker Compose.

**SERA relevance:** Low architecturally (visual builder, not container-orchestrated agents) but high as market reference. Dify is what non-technical users reach for. SERA targets a different audience (infra-aware homelabbers) but should understand Dify's UX patterns.

**Source:** [github.com/langgenius/dify](https://github.com/langgenius/dify)

### AIO Sandbox (agent-infra)
**What it is:** All-in-one sandbox combining Browser, Shell, File, MCP, and VSCode Server in a single Docker container.

**Key concepts:** Unified filesystem (browser downloads available in shell), VNC + CDP browser access, aggregated MCP servers through single `/mcp` endpoint with namespacing, Jupyter integration. Opposite of SERA's per-agent isolation — everything in one container.

**SERA relevance:** Low as architecture reference (opposite design philosophy). But the MCP aggregation pattern (single endpoint, namespaced tools) is interesting for SERA's MCPRegistry.

**Source:** [github.com/agent-infra/sandbox](https://github.com/agent-infra/sandbox)

### Moltworker (Cloudflare)
**What it is:** Middleware for running MoltBot (personal AI agent) on Cloudflare Workers instead of dedicated hardware. Proof of concept, open-sourced Jan 2026.

**Key concepts:** Edge-deployed always-on agent, Cloudflare Sandbox containers at edge, R2 for persistent storage, administration UI, proxy between APIs and isolated environment.

**SERA relevance:** Low. Interesting as proof that agents can run at the edge, but SERA is homelab-focused (local-first). The "always-on background agent" pattern is already covered by SERA's persistent agent lifecycle.

**Source:** [blog.cloudflare.com/moltworker-self-hosted-ai-agent](https://blog.cloudflare.com/moltworker-self-hosted-ai-agent/)

---

## A2A Protocol — Federation Standard

**Google's Agent2Agent (A2A) protocol** is the emerging industry standard for agent-to-agent communication. Now a [Linux Foundation project](https://www.linuxfoundation.org/press/linux-foundation-launches-the-agent2agent-protocol-project-to-enable-secure-intelligent-communication-between-ai-agents) with 50+ partners. SERA should adopt A2A for **external federation** while keeping Centrifugo intercom for **internal** agent communication.

**Source:** [github.com/a2aproject/A2A](https://github.com/a2aproject/A2A) | [Specification](https://a2a-protocol.org/latest/specification/)

### Why A2A for External, Centrifugo for Internal

| Concern | Internal (Centrifugo) | External (A2A) |
|---|---|---|
| Latency | Sub-ms pub/sub | HTTP round-trips |
| Observability | Core sees everything | Agents are opaque by design |
| Capability enforcement | Core resolves before dispatch | Agent self-declares via Agent Card |
| State sharing | Shared memory via circles | No shared state (by design) |
| Budget enforcement | LLM proxy meters every call | No built-in metering |
| Security | JWT within trusted network | OAuth2/mTLS/API key per Agent Card |

**Architecture:**
```
Internal (same SERA instance):
  Agent ←→ Centrifugo intercom ←→ Agent
  (governed by Core: capabilities, audit, budgets)

External (cross-instance or cross-platform):
  SERA Agent ←→ A2A endpoint ←→ Remote Agent
  (A2A protocol, Agent Cards, task lifecycle)
```

The A2A endpoint in sera-core acts as a **bridge** — translates A2A tasks into internal intercom messages and vice versa. Core still enforces capabilities on the SERA side.

### A2A Protocol Key Concepts

**Agent Card** (discovery at `/.well-known/agent.json`):
- Identity: `name`, `description`, `provider`
- Capabilities: `streaming`, `pushNotifications`
- Skills: array of `AgentSkill` with name/description/input/output
- Security: `securitySchemes` (API Key, OAuth2, OpenID Connect, mTLS)
- Interfaces: JSON-RPC, gRPC, HTTP/REST
- Signature: ed25519 for card authenticity

**Task Lifecycle States:**
`submitted` → `working` → `completed` | `failed` | `canceled`
Branch: `working` → `input-required` → `working` (multi-turn)
Special: `auth-required`, `rejected`

**Key JSON-RPC Methods:**
- `SendMessage` — create or continue a task (returns Task or Message)
- `SendStreamingMessage` — same but returns SSE stream of events
- `GetTask` — retrieve task state and history
- `ListTasks` — paginated task listing with filters
- `CancelTask` — request cancellation
- `SubscribeToTask` — SSE stream for task updates
- `CreateTaskPushNotificationConfig` — register webhook for async updates

**Message Parts:** `TextPart`, `FilePart` (URI + MIME), `DataPart` (structured JSON)

**How A2A complements MCP:**
- **MCP** = agent ↔ tools (agent calls tools on a server)
- **A2A** = agent ↔ agent (agents collaborate as peers, multi-turn, opaque)

### SERA A2A Implementation Plan

1. **Inbound A2A server** in sera-core — receives tasks from external agents, routes to internal agents via intercom
2. **Outbound A2A client** — SERA agents can send tasks to external A2A-compatible agents
3. **Agent Card generation** — sera-core auto-generates Agent Cards from agent templates/instances
4. **Instance pairing** — SERA-to-SERA federation uses A2A with additional trust metadata (challenge-response for initial pairing, then OAuth2 for ongoing comms)
5. **Capability gate** — `seraManagement.federation.*` capabilities control which agents can receive/send A2A tasks

---

## Cross-Cutting Features to Adopt

Features that appear in 3+ reference projects, confirming their importance:

### 1. Hybrid Memory Search (BM25 + Vector)
**Seen in:** CrewAI, OpenFang, Docker Agent, OpenClaw, Goose
**SERA action:** Add BM25 via PostgreSQL `tsvector` alongside Qdrant. Merge with configurable weighting.

### 2. Memory Composite Scoring (Semantic + Recency + Importance)
**Seen in:** CrewAI, Letta, OpenFang
**SERA action:** Enhance `MemoryManager.search()` with recency decay and importance weighting beyond pure vector similarity.

### 3. Context Compaction with Cheap Model
**Seen in:** HERM, Goose, OpenHands, OpenClaw, OpenCode, Letta
**SERA action:** Add compaction to agent-runtime using exploration/cheap model. Memory flush before compaction.

### 4. Token-Efficient File Tools
**Seen in:** HERM, Goose, OpenCode, OpenHands
**SERA action:** Agent-runtime should use structured `glob`/`grep`/`read` tools with JSON I/O instead of raw shell commands. 3x token savings.

### 5. Loop Guard / Tool Call Dedup
**Seen in:** OpenFang, Goose, HERM, OpenHands
**SERA action:** Add SHA256-based tool call dedup to reasoning loop (warn at 3x, block at 5x, circuit-break at 30x).

### 6. Error-as-Information Pattern
**Seen in:** Goose, HERM, OpenHands
**SERA action:** Tool errors fed back to LLM for recovery instead of failing the task. Already partially implemented in agent-runtime but should be explicit.

### 7. Hook System (Pre/Post Tool Execution)
**Seen in:** Docker Agent, Goose, OpenCode, CrewAI
**SERA action:** Add `beforeToolCall`/`afterToolCall` hooks to agent-runtime for audit logging, capability enforcement, result transformation.

### 8. Agent-as-Tool Pattern
**Seen in:** AutoGen (AgentTool), CrewAI (DelegateWork), Docker Agent (transfer_task)
**SERA action:** Expose subagent spawning as a tool call with structured result, not just intercom messaging.

---

## SERA Differentiators

Things SERA does that **no reference project matches**:

1. **True Docker container isolation per agent** — OpenFang uses WASM, HERM uses one container per session, others run in-process. SERA gives each agent its own container with distinct capabilities.

2. **Capability intersection model** — `Boundary ∩ Policy ∩ Overrides ∩ RuntimeGrants`. No other project has this level of fine-grained, layered capability resolution.

3. **Per-agent network ACLs via egress proxy** — Squid with SNI filtering, bandwidth limiting, per-agent ACL files. OpenFang has SSRF protection but no per-agent network segmentation.

4. **Merkle hash-chain audit trail** — OpenFang also has this (confirming it's the right approach). No other project does.

5. **Template → Instance separation** — Like Helm charts → releases. Docker Agent has OCI distribution but no template/instance split. OpenFang has "Hands" but they're monolithic.

6. **Circles with shared memory and intercom** — No other project has the concept of named agent teams with shared memory namespaces and orchestration patterns.

7. **Centralised LLM proxy with per-agent budget enforcement** — Token budgets per hour/day, metering, circuit breaking. OpenFang has metering but not per-agent budgets.

---

## New Epic Candidates

### Epic 21: ACP / IDE Bridge
*(Already documented in prior analysis — see OpenClaw section)*

### Epic 22: Canvas / Agent-Driven UI (A2UI)
*(Already documented in prior analysis — see OpenClaw section)*

### Epic 23: Voice Interface
*(Already documented in prior analysis — see OpenClaw section)*

### Epic 24: A2A Federation Protocol

**What:** Implement Google's [Agent2Agent (A2A) protocol](https://github.com/a2aproject/A2A) for cross-instance and cross-platform agent communication. Use A2A for **external** federation while keeping Centrifugo for **internal** agent comms.

**Why:** A2A is now a Linux Foundation project with 50+ partners (Atlassian, Salesforce, SAP). Docker Agent and OpenFang already support it. Building a proprietary federation protocol would be a dead end. A2A gives SERA interoperability with the entire ecosystem.

**Key stories:**
- A2A inbound server — sera-core receives A2A tasks, bridges to internal intercom
- A2A outbound client — SERA agents send tasks to external A2A agents
- Agent Card generation — auto-generated from agent templates/instances at `/.well-known/agent.json`
- Instance pairing — challenge-response for initial SERA-to-SERA trust, then OAuth2 for ongoing comms
- Capability gate — `seraManagement.federation.*` controls A2A access
- Push notification config — webhook registration for async task updates
- Streaming support — SSE stream for real-time task updates
- Cross-instance circle membership — remote agents join local circles via A2A

**Dependencies:** Epic 09 (Real-Time Messaging), Epic 16 (Auth), Epic 17 (Agent Identity)

**Phase:** 4+

---

## Enhancements to Existing Epics

### Epic 04 — LLM Proxy & Governance

| Enhancement | Source | Description |
|---|---|---|
| Multi-account auth with failover | OpenClaw | Multiple API keys per provider, rotation on 429s |
| Thinking level abstraction | OpenClaw, pi-mono | Unified `low/medium/high/x-high` mapped to provider-specific params |
| Toolshim / exploration model | Goose, HERM | Separate cheap model for tool routing and context compaction |
| Tool-per-model override | Docker Agent | Each tool/skill can specify a different model for its execution |

### Epic 05 — Agent Runtime

| Enhancement | Source | Description |
|---|---|---|
| Memory flush before compaction | OpenClaw, Letta | Silent agent turn to persist memories before context eviction |
| Loop guard | OpenFang, Goose | SHA256 tool call dedup (3x warn, 5x block, 30x circuit-break) |
| Error-as-information | Goose, HERM | Tool errors fed back to LLM for recovery |
| Token-efficient file tools | HERM, Goose | Structured glob/grep/read with JSON I/O instead of bash |
| Context clearing at 80% | HERM | Old tool results replaced with placeholder |
| Pre/post tool hooks | Docker Agent, Goose | `beforeToolCall`/`afterToolCall` for audit, enforcement |
| Self-building dev environments | HERM | Agent can extend its container image via Dockerfile |

### Epic 06 — Skill Library

| Enhancement | Source | Description |
|---|---|---|
| Trigger-based skill activation | OpenHands | Skills activated by keyword matching in messages |
| Serena MCP integration | Serena | Connect to Serena MCP server for symbol-level code editing |

### Epic 08 — Memory & RAG

| Enhancement | Source | Description |
|---|---|---|
| BM25 hybrid search | CrewAI, Docker Agent, OpenClaw | Keyword search via PostgreSQL tsvector alongside Qdrant |
| Composite scoring | CrewAI | Semantic(0.5) + recency(0.3) + importance(0.2) weighting |
| Hierarchical memory scopes | CrewAI | Path-based scoping (`/project/decisions`, `/agent/name`) |
| Sleeptime memory consolidation | Letta | Background thread for memory management |
| Editable memory blocks | Letta | Transparent, user-readable text blocks in system prompt |
| LLM-guided memory categorisation | CrewAI | Importance scoring and scope inference on save |

### Epic 10 — Circles & Coordination

| Enhancement | Source | Description |
|---|---|---|
| Agent-as-tool pattern | AutoGen, CrewAI | Expose subagent as callable tool, not just intercom message |
| Party mode discussions | BMAD-METHOD | Multi-agent open discussion with cross-talk |
| Composable termination conditions | AutoGen | `MaxMessage | TextMention & ExternalSignal` |
| Project-context constitution | BMAD-METHOD | Shared context doc loaded by all circle members |

### Epic 15 — Plugin SDK & Ecosystem

| Enhancement | Source | Description |
|---|---|---|
| OCI distribution for templates | Docker Agent | `sera pull template/developer:v2` from OCI registries |
| Plugin manifest spec | OpenClaw | Declarative `sera-plugin.json` with metadata |
| `sera doctor` command | OpenClaw, Goose | Diagnose config, credentials, connectivity |

### Epic 18 — Integration Channels

| Enhancement | Source | Description |
|---|---|---|
| DM pairing / inbound access control | OpenClaw | Challenge-response approval for unknown senders |
| Telegram adapter | OpenClaw, OpenFang | Telegram Bot API channel (alongside Discord/Slack) |
| Federation pairing | OpenClaw + Docker Agent | Cross-instance trust via challenge exchange |

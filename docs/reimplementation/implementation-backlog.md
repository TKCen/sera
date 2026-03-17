# 📋 SERA Implementation Backlog

> Comprehensive task list with user stories for implementing the SERA agent workspace architecture.
> Updated 2026-03-17 — reprioritized around core chat experience + OpenFang gap analysis.
>
> **References**: [agent-workspace-architecture.md](./agent-workspace-architecture.md) · [OpenFang Analysis](../../.gemini/antigravity/brain/4e4a23d8-c437-4703-8cda-6e057cc348ae/openfang_analysis.md)

> [!IMPORTANT]
> **Current Priority**: Get a fully working chat experience — persistent sessions, thinking mode,
> tool usage, agent-to-agent comms, general assistant agent, and agent template creation from the UI.
> SERA is primarily a **personal assistant platform**, not only a code-building tool.

---

## 📊 Current State Summary *(updated 2026-03-17)*

### ✅ Fully Implemented (Backend)

| Component | Files | Notes |
|---|---|---|
| **Express API Server** | `index.ts` (839 LOC) | 20+ endpoints, all subsystems wired, CORS, error handling |
| **LLM Provider** | `OpenAIProvider.ts`, `ProviderFactory.ts` | Multi-provider catalog with config persistence, test endpoint |
| **Agent Manifest System** | `AgentManifestLoader.ts` (7.2KB) + types + tests | Loads/validates `AGENT.yaml`, 3 templates ship (architect, developer, researcher) |
| **Identity Service** | `IdentityService.ts` (5.5KB) | System prompt generation from manifest, streaming prompt variant |
| **Agent Factory + File Watcher** | `AgentFactory.ts` (2.6KB + tests) | Dynamic agent creation from manifests, hot-reload on file changes |
| **Session Store** | `SessionStore.ts` (7.3KB + tests) | PostgreSQL + JSONL hybrid, CRUD + messages, integrated into chat API |
| **Chat API (session-aware)** | `index.ts` lines 479-632 | `POST /api/chat` and `POST /api/chat/stream`, auto-creates/resumes sessions, auto-title |
| **Intercom Service** | `IntercomService.ts` (9.9KB + tests) | Centrifugo integration, thought streaming, direct messages, stream tokens |
| **Channel Namespaces** | `ChannelNamespace.ts` (4.6KB + tests) | `internal:`, `intercom:`, `stream:` namespace validation |
| **Sandbox Manager** | `SandboxManager.ts` (10.2KB + tests) | Container lifecycle via dockerode, resource limits, network modes |
| **Tier Policy** | `TierPolicy.ts` (6.9KB + tests) | Security tier enforcement (CPU, memory, network) |
| **Sandbox Tool Runner** | `ToolRunner.ts` (5KB) | Execute tools in sandbox containers with timeout and output capture |
| **Circle Registry** | `CircleRegistry.ts` (10.7KB + tests) | Load/validate CIRCLE.yaml, agent roster, project context injection |
| **Party Mode** | `PartyMode.ts` (10.8KB + tests) | Multi-agent group discussion sessions, full API (create/message/end/list) |
| **Skill Registry** | `SkillRegistry.ts` (7.8KB + tests) | Register/invoke skills, MCP tool bridge, dependency validation |
| **Built-in Skills (6)** | `skills/builtins/` | `file-read`, `file-write`, `web-search`, `knowledge-store`, `knowledge-query`, index |
| **Memory Block Store** | `MemoryBlockStore.ts` (13KB + tests) | Markdown+YAML frontmatter files, 4 block types, refs, wikilinks, graph API |
| **Reflector** | `Reflector.ts` (4.2KB + tests) | Auto-compaction of old memory entries into archival summaries |
| **Storage Providers** | `StorageProvider.ts` + Local + DockerVolume (all with tests) | Pluggable workspace storage abstraction |
| **Subagent Runner** | `SubagentRunner.ts` (6.4KB) | Spawn child agents, workspace sharing, result collection |
| **Process Manager** | `ProcessManager.ts` + Sequential + Parallel + Hierarchical (all + tests) | Three execution strategies with typed process definitions |
| **Docker Compose** | `docker-compose.yaml` | sera-core, sera-web, centrifugo, postgres, qdrant — all healthy |

### 🟡 Partially Implemented

| Component | Status | What's Missing |
|---|---|---|
| **BaseAgent** | Streaming + thoughts work | ❌ No tool call loop — agent can't call tools during reasoning, `observe/plan/act/reflect` are stubs |
| **Orchestrator** | Process patterns work | ❌ Uses `ProcessManager` but agents themselves lack tool execution |
| **MCP Registry** | Client registration works | ❌ No MCP server mode (exposing agents as MCP tools) |
| **Vector Services** | Embedding + Qdrant search | ❌ Not connected to agent reasoning loop |
| **Web UI** | Pages exist: chat, agents, circles, insights, memory, schedules, settings, tools | ❌ No session sidebar, no thinking mode panel, no agent template creator, no streaming display |
| **Centrifugo** | Wired into IntercomService | ❌ Web UI doesn't subscribe to channels yet — no live streaming in browser |
| **LSP Router** | Basic route exists | ❌ No symbol-level integration (Serena-style) |

### 🔴 Not Yet Started

| Component | Notes |
|---|---|
| **Tool Execution in Agent Loop** | Agent can't call `web_search`, `file_read` etc. during reasoning — this is THE critical gap |
| **Thinking Mode UI** | No collapsible thinking panel, no tool call display in browser |
| **Session Sidebar (Web UI)** | Sessions exist in backend but UI has no sidebar to browse/resume them |
| **General Assistant Agent** | Only code-focused agents exist (architect, developer, researcher) — no personal assistant |
| **Agent Template Creator** | No UI to create new agent types from the dashboard |
| **Agent Loop Stability** | No loop guard, session repair, compaction, tool timeout |
| **Metering / Budget** | No token quotas, cost tracking, per-agent spend |
| **Channel Adapters** | No Telegram, Discord, WhatsApp adapters |
| **OpenAI-Compatible API** | No `/v1/chat/completions` endpoint |
| **Audit Trail** | No Merkle hash-chain logging |

---

## 🔍 Gap Analysis vs OpenFang

The following table maps OpenFang systems to existing SERA coverage and identifies gaps:

| OpenFang System | SERA Coverage | Gap | Priority |
|---|---|---|---|
| Agent Manifests (`agent.toml`) | ✅ **Done** — `AgentManifestLoader` + 3 YAML templates | Implemented | — |
| Capability-Based Security | ✅ **Done** — `TierPolicy` + `SandboxManager` RBAC | MCP server mode missing | Low |
| Memory Substrate (6 layers) | ✅ **Done** — `MemoryBlockStore` + `Reflector` + graph API | Missing KG, task board, canonical sessions | Medium |
| Skills Framework | ✅ **Done** — `SkillRegistry` + 6 builtins + MCP bridge | — | — |
| Orchestrator (process patterns) | ✅ **Done** — `ProcessManager` (Sequential/Parallel/Hierarchical) | — | — |
| MCP Client | ✅ **Done** — `MCPRegistry` + skill bridge | No server mode | Medium |
| **Tool Execution in Agent Loop** | ❌ Critical gap | 🔴 Agent can't call tools during reasoning — **Story 0.3** | **Critical** |
| **Agent Loop Stability** | ❌ Not started | 🔴 **Epic 13** — loop guard, session repair, compaction | **High** |
| **Metering & Budget** | ❌ Not started | 🔴 **Epic 14** — token quotas, cost tracking | **High** |
| **Channel Adapters** | ❌ Not started | 🔴 **Epic 15** — Telegram, Discord, WhatsApp | **Medium** |
| **OpenAI-Compatible API** | ❌ Not started | 🔴 **Epic 17** — `/v1/chat/completions` endpoint | **Medium** |
| **Audit Trail** | ❌ Not started | 🔴 **Epic 18** — Merkle hash-chain logging | **Low** |
| Workflow Engine | ❌ Deferred | Deferred — not needed for personal assistant UX | Future |
| Autonomous Hands | ❌ Deferred | Deferred — requires workflows + triggers | Future |

---

## 💬 Epic 0: Core Chat Experience *(TOP PRIORITY)*

*Get the fundamental chat loop working end-to-end: persistent sessions, streaming thoughts, tool usage, and a general-purpose assistant.*

### Story 0.1: Session Management (Persistent Chat Sessions) — ✅ BACKEND DONE

> **As a** user,
> **I want** my conversations with agents to persist across page reloads and restarts,
> **so that** I can resume conversations, browse history, and maintain context over time.

**Acceptance Criteria:**
- [x] Create `Session` type: id, agentId, title (auto-generated from first message), messages array, createdAt, updatedAt
- [x] PostgreSQL `sessions` table stores session metadata; JSONL files on disk for human-readable message logs
- [x] `SessionManager` creates, loads, lists, and deletes sessions per agent
- [x] Chat endpoint accepts optional `sessionId` — resumes existing session or creates a new one
- [x] API: `GET /api/sessions` (list), `GET /api/sessions/:id` (load), `POST /api/sessions` (create), `DELETE /api/sessions/:id`
- [ ] Web UI sidebar shows session history grouped by agent; clicking a session loads the conversation
- [ ] New chat button creates a fresh session; agent retains memory context from previous sessions

**Files:**
- `[NEW]` `core/src/sessions/SessionManager.ts`
- `[NEW]` `core/src/sessions/types.ts`
- `[NEW]` `core/src/routes/sessions.ts`
- `[MODIFY]` `core/src/routes/chat.ts` — session-aware chat endpoint
- `[MODIFY]` `web/src/app/chat/page.tsx` — session sidebar + resume

---

### Story 0.2: Thinking Mode & Streaming — 🟡 BACKEND PARTIAL

> **As a** user,
> **I want** to see the agent's reasoning process in real-time as it thinks,
> **so that** I understand what it's doing and can intervene if needed.

**Acceptance Criteria:**
- [/] Agent reasoning loop publishes "thinking" chunks via SSE or Centrifugo before the final response — *backend emits thoughts via `publishThought()` but reasoning steps are stubs*
- [x] Thinking stream includes: step type (reasoning/planning/tool-call/tool-result), content, timestamp
- [ ] Web UI renders thinking steps in a collapsible panel above the response (like Claude/ChatGPT thinking)
- [ ] Tool calls show the tool name, parameters, and result inline in the thinking stream
- [x] Final response is displayed separately from thinking content
- [ ] Toggle to show/hide thinking in the UI; default on

**Files:**
- `[MODIFY]` `core/src/agents/BaseAgent.ts` — emit thinking events during reasoning loop
- `[MODIFY]` `core/src/routes/chat.ts` — SSE streaming of thinking + response
- `[MODIFY]` `web/src/app/chat/page.tsx` — thinking panel UI

---

### Story 0.3: Tool Execution in Agent Loop

> **As an** agent,
> **I want** to call tools (web search, file read/write, knowledge queries) during my reasoning loop,
> **so that** I can gather information and take actions to answer the user's question.

**Acceptance Criteria:**
- [ ] Agent loop supports LLM tool-call responses — parse tool calls, execute them, feed results back
- [ ] Built-in tools available immediately: `web_search`, `web_fetch`, `file_read`, `file_write`, `file_list`, `memory_store`, `memory_recall`
- [ ] Tool definitions sent to the LLM in the standard tool/function format for the provider
- [ ] Tool results truncated at 50K chars with marker (prevent context overflow)
- [ ] Tool execution timeout (default 60s) prevents hanging
- [ ] Tool calls and results appear in the thinking stream (Story 0.2)
- [ ] Agents only have access to tools listed in their manifest's `tools.allowed`

**Files:**
- `[NEW]` `core/src/tools/ToolRunner.ts`
- `[NEW]` `core/src/tools/builtins/` — web_search, web_fetch, file_read, file_write, file_list, memory_store, memory_recall
- `[MODIFY]` `core/src/agents/BaseAgent.ts` — tool call loop

---

### Story 0.4: General Assistant Agent & Default Templates

> **As a** user,
> **I want** a pre-configured general-purpose assistant agent available out of the box,
> **so that** I can use SERA as a personal assistant immediately without configuring agents.

**Acceptance Criteria:**
- [ ] Ship with a `general-assistant.agent.yaml` — friendly, helpful, broad knowledge, all safe tools enabled
- [ ] General assistant is the default agent selected on first visit to the dashboard
- [ ] Ship with 3-4 additional starter templates: `researcher`, `coder`, `writer`
- [ ] Each template has a distinct persona, communication style, and tool set
- [ ] Templates are stored in `sera/agents/` and loaded automatically on startup
- [ ] Dashboard agent list shows all loaded agents with their persona avatars

**Files:**
- `[NEW]` `sera/agents/general-assistant.agent.yaml`
- `[NEW]` `sera/agents/researcher.agent.yaml`
- `[NEW]` `sera/agents/coder.agent.yaml`
- `[NEW]` `sera/agents/writer.agent.yaml`
- `[MODIFY]` `web/src/app/agents/page.tsx` — show loaded agents with persona info

---

## 🏗️ Epic 1: Agent Manifest System — ✅ DONE

*Establish the declarative agent definition system that everything else builds on.*

> [!TIP]
> **Status**: Fully implemented. `AgentManifestLoader` (7.2KB + tests), `IdentityService` (5.5KB),
> `ProviderFactory` (1.9KB), 3 agent YAML templates, `Orchestrator` creates agents from manifests.

### Story 1.1: AGENT.yaml Parser & Validator

> **As a** platform developer,
> **I want** agents to be defined in declarative YAML files,
> **so that** agent configuration is version-controlled, auditable, and separate from code.

**Acceptance Criteria:**
- [ ] Create `AgentManifest` TypeScript interface matching the `AGENT.yaml` schema from the architecture doc
- [ ] Implement `AgentManifestLoader` in `core/src/agents/manifest/AgentManifestLoader.ts` that reads and validates YAML files
- [ ] Validation rejects manifests with unknown fields, invalid security tiers, or referencing nonexistent tools/skills
- [ ] Create 2-3 example `AGENT.yaml` files in `sera/agents/` (e.g., `architect.agent.yaml`, `developer.agent.yaml`, `researcher.agent.yaml`)
- [ ] Unit tests for validation logic (valid manifest, invalid tier, missing required fields)

**Files:**
- `[NEW]` `core/src/agents/manifest/AgentManifestLoader.ts`
- `[NEW]` `core/src/agents/manifest/types.ts`
- `[NEW]` `sera/agents/architect.agent.yaml`
- `[NEW]` `sera/agents/developer.agent.yaml`
- `[NEW]` `sera/agents/researcher.agent.yaml`

---

### Story 1.2: Agent Identity & System Prompt Generation

> **As an** agent,
> **I want** my persona, communication style, and principles loaded from my manifest,
> **so that** I behave consistently according to my defined identity across all sessions.

**Acceptance Criteria:**
- [ ] Create `IdentityService` that reads the `identity` block from `AGENT.yaml`
- [ ] Generate system prompts from identity fields (role, description, communicationStyle, principles)
- [ ] Refactor `PrimaryAgent` and `WorkerAgent` to use identity-driven system prompts instead of hardcoded strings
- [ ] The system prompt should include the agent's tools, skills, and allowed subagents as context

**Files:**
- `[NEW]` `core/src/agents/identity/IdentityService.ts`
- `[MODIFY]` `core/src/agents/BaseAgent.ts` — accept manifest instead of raw strings
- `[MODIFY]` `core/src/agents/PrimaryAgent.ts` — use IdentityService
- `[MODIFY]` `core/src/agents/WorkerAgent.ts` — use IdentityService

---

### Story 1.3: Model Configuration from Manifest

> **As a** platform operator,
> **I want** each agent's LLM provider and model to be configured in its manifest,
> **so that** different agents can use different models (e.g., cheap model for simple tasks, frontier model for reasoning).

**Acceptance Criteria:**
- [ ] `AgentManifestLoader` resolves the `model` block to an `LLMProvider` instance
- [ ] Support `fallback` model configuration with complexity-based routing
- [ ] Refactor `Orchestrator` to create agents from manifests rather than hardcoded constructors
- [ ] Existing provider config system remains available as a default for manifests that don't specify a provider

**Files:**
- `[MODIFY]` `core/src/agents/manifest/AgentManifestLoader.ts`
- `[NEW]` `core/src/lib/llm/ProviderFactory.ts`
- `[MODIFY]` `core/src/agents/Orchestrator.ts`

---

## 🔄 Epic 2: Circle System — ✅ DONE

*Implement the organizational layer that groups agents and scopes knowledge.*

> [!TIP]
> **Status**: Fully implemented. `CircleRegistry` (10.7KB + tests), `PartyMode` (10.8KB + tests), circle routes, party mode API.

### Story 2.1: Circle Registry & CIRCLE.yaml

> **As a** platform operator,
> **I want** to define circles as YAML files that group agents together,
> **so that** agents within a circle share a knowledge scope and communication mesh.

**Acceptance Criteria:**
- [ ] Create `CircleManifest` TypeScript interface matching the `CIRCLE.yaml` schema
- [ ] Implement `CircleRegistry` that loads and validates circle definitions
- [ ] Validate that all agents referenced in a circle have corresponding `AGENT.yaml` files
- [ ] Create example circle definitions (e.g., `development.circle.yaml`, `operations.circle.yaml`)
- [ ] API endpoint `GET /api/circles` returns all loaded circles and their agent rosters

**Files:**
- `[NEW]` `core/src/circles/CircleRegistry.ts`
- `[NEW]` `core/src/circles/types.ts`
- `[NEW]` `sera/circles/development.circle.yaml`
- `[NEW]` `sera/circles/operations.circle.yaml`
- `[MODIFY]` `core/src/index.ts` — register circle routes

---

### Story 2.2: Project Context (Circle Constitution)

> **As an** agent in a circle,
> **I want** a shared `project-context.md` loaded into my system prompt,
> **so that** all agents in my circle follow the same conventions and architectural decisions.

**Acceptance Criteria:**
- [ ] `CircleRegistry` loads `project-context.md` for each circle from the configured path
- [ ] Project context content is injected into agent system prompts on activation
- [ ] Agents can propose amendments to the project context (logged to intercom channel)
- [ ] If no project-context.md exists, agents operate without it (graceful fallback)

**Files:**
- `[MODIFY]` `core/src/circles/CircleRegistry.ts`
- `[MODIFY]` `core/src/agents/identity/IdentityService.ts` — inject circle context
- `[NEW]` `sera/circles/development/project-context.md`

---

### Story 2.3: Circle Knowledge Scoping

> **As a** platform developer,
> **I want** each circle's knowledge stored in a separate Qdrant collection and PostgreSQL schema,
> **so that** knowledge is isolated between circles and doesn't pollute unrelated agent contexts.

**Acceptance Criteria:**
- [ ] `KnowledgeManager` creates and manages per-circle Qdrant collections
- [ ] Vector ingestion and search are scoped to the active circle's collection
- [ ] Memory archival is namespaced per circle in PostgreSQL
- [ ] API endpoints for knowledge operations accept a `circleId` parameter

**Files:**
- `[NEW]` `core/src/circles/KnowledgeManager.ts`
- `[MODIFY]` `core/src/services/vector.service.ts` — circle-scoped collections
- `[MODIFY]` `core/src/memory/manager.ts` — circle-scoped archival

---

## 🐳 Epic 3: Sandbox Manager — ✅ DONE

*Implement the secure container management layer that agents use instead of direct Docker access.*

> [!TIP]
> **Status**: Fully implemented. `SandboxManager` (10.2KB + tests), `TierPolicy` (6.9KB + tests), `ToolRunner` (5KB), `SubagentRunner` (6.4KB), sandbox routes.

### Story 3.1: Sandbox Manager Core

> **As an** agent,
> **I want** to request container spawning through a managed API,
> **so that** I never need direct Docker socket access and all my actions are gated by RBAC.

**Acceptance Criteria:**
- [ ] Implement `SandboxManager` using `dockerode` for container lifecycle management
- [ ] API endpoints: `POST /sandbox/spawn`, `POST /sandbox/exec`, `DELETE /sandbox/:id`, `GET /sandbox/:id/logs`
- [ ] All operations validate the requesting agent's `AGENT.yaml` permissions before execution
- [ ] Resource limits (CPU, memory) are applied based on the agent's security tier
- [ ] Network mode is applied based on security tier (none / sera_net / bridge)
- [ ] All operations are logged to the audit trail

**Files:**
- `[NEW]` `core/src/sandbox/SandboxManager.ts`
- `[NEW]` `core/src/sandbox/types.ts`
- `[NEW]` `core/src/sandbox/TierPolicy.ts`
- `[NEW]` `core/src/routes/sandbox.ts`
- `[MODIFY]` `core/src/index.ts` — register sandbox routes

---

### Story 3.2: Subagent Spawning

> **As an** agent,
> **I want** to spawn subagents as ephemeral containers that share my workspace,
> **so that** I can delegate specialized tasks while maintaining filesystem continuity.

**Acceptance Criteria:**
- [ ] `SandboxManager.spawnSubagent(parentManifest, childRole, task)` creates a new container
- [ ] Child container mounts the parent's workspace volume (read-write)
- [ ] Child container gets its own `AGENT.yaml`-derived configuration
- [ ] Parent receives subagent results via the event bus when the subagent completes
- [ ] Subagent containers are automatically cleaned up after task completion
- [ ] `maxInstances` from parent's manifest is enforced

**Files:**
- `[MODIFY]` `core/src/sandbox/SandboxManager.ts`
- `[NEW]` `core/src/agents/SubagentRunner.ts`

---

### Story 3.3: Tool Container Execution

> **As an** agent,
> **I want** to execute tools (terminal, browser, file operations) in isolated containers,
> **so that** tool execution is sandboxed and doesn't affect the agent brain's stability.

**Acceptance Criteria:**
- [ ] `SandboxManager.runTool(agentManifest, toolCommand, tier)` spawns a tool container
- [ ] Tool container mounts the agent's workspace (read-write for tier 2+, read-only for tier 1)
- [ ] Tool output is captured and returned to the agent
- [ ] Tool containers have configurable timeout (default 60s, max 300s)
- [ ] Failed tool executions return error details without crashing the agent

**Files:**
- `[MODIFY]` `core/src/sandbox/SandboxManager.ts`
- `[NEW]` `core/src/sandbox/ToolRunner.ts`

---

## 📡 Epic 4: Intercom System — ✅ DONE

*Connect agents within and across circles using Centrifugo.*

> [!TIP]
> **Status**: Fully implemented. `IntercomService` (9.9KB + tests), `ChannelNamespace` (4.6KB + tests), intercom routes, thought streaming, direct messaging, all integrated into `BaseAgent`.

### Story 4.1: Centrifugo Integration

> **As a** platform developer,
> **I want** agents to publish and subscribe to Centrifugo channels,
> **so that** they can communicate in real-time without polling or shared databases.

**Acceptance Criteria:**
- [ ] Implement `IntercomService` that wraps the Centrifugo HTTP API
- [ ] Support publishing messages to channels with the standard envelope schema
- [ ] Support subscribing agent brain containers to their allowed channels
- [ ] Channel namespace validation enforces the naming scheme (`internal:`, `intercom:`, `channel:`, etc.)
- [ ] Integrate with existing Centrifugo deployment (already in `docker-compose.yaml`)

**Files:**
- `[NEW]` `core/src/intercom/IntercomService.ts`
- `[NEW]` `core/src/intercom/types.ts`
- `[NEW]` `core/src/intercom/ChannelNamespace.ts`
- `[MODIFY]` `sera/centrifugo/config.json` — add namespace configuration

---

### Story 4.2: Agent-to-Agent Messaging

> **As an** agent,
> **I want** to send direct messages to permitted peer agents,
> **so that** I can request help, share findings, or coordinate work without human intervention.

**Acceptance Criteria:**
- [ ] Agents can send messages to peers listed in their `intercom.canMessage` manifest entry
- [ ] Messages are delivered via dedicated `intercom:{circle}:{from}:{to}` channels
- [ ] Receiving agents can access messages via their reasoning loop
- [ ] Messages that cannot be delivered (agent offline / not permitted) return an error
- [ ] Message history is stored in PostgreSQL for replay

**Files:**
- `[MODIFY]` `core/src/intercom/IntercomService.ts`
- `[MODIFY]` `core/src/agents/BaseAgent.ts` — add intercom message handling

---

### Story 4.3: Thought Streaming to UI

> **As a** user viewing the SERA dashboard,
> **I want** to see an agent's reasoning process in real-time,
> **so that** I can understand what the agent is doing and intervene if needed.

**Acceptance Criteria:**
- [ ] Agent reasoning steps are published to `internal:agent:{id}:thoughts` channel
- [ ] Tool container stdout/stderr is streamed to `internal:agent:{id}:terminal` channel
- [ ] Web UI subscribes to these channels and renders real-time thought and terminal panels
- [ ] Thought stream includes: timestamp, step type (observe/plan/act/reflect), content

**Files:**
- `[MODIFY]` `core/src/agents/BaseAgent.ts` — publish thoughts to Centrifugo
- `[MODIFY]` `core/src/sandbox/ToolRunner.ts` — stream tool output
- `[MODIFY]` `web/src/app/chat/page.tsx` — render thought stream

---

## 🧠 Epic 5: Memory Blocks (Letta-style × Obsidian-style) — ✅ DONE

*Upgrade the POC memory system to structured, graph-linked memory blocks stored as human-readable markdown files.*

> [!TIP]
> **Status**: Fully implemented. `MemoryBlockStore` (13KB + tests), `Reflector` (4.2KB + tests),
> full API (blocks, entries, refs, graph, search). Memory entries are markdown files with YAML frontmatter.

### Storage Format

Each memory entry is a markdown file with YAML frontmatter:

```markdown
---
id: a1b2c3d4-...
title: Project Testing Stack
type: core
tags: [tooling, testing]
refs: [e5f6g7h8-...]
source: agent
createdAt: 2026-03-16T22:00:00Z
updatedAt: 2026-03-16T22:00:00Z
---

The project uses **Vitest** for unit testing with `supertest` for HTTP assertions.
See also [[CI Pipeline Config]] for the pipeline configuration.
```

Entries are organized in block-type folders (`memory/blocks/human/`, `persona/`, `core/`, `archive/`).
Graph edges come from: explicit `refs` in frontmatter + `[[Title]]` wikilinks in content.

---

### Story 5.1: Structured Memory Blocks

> **As an** agent,
> **I want** my memory organized into distinct blocks (Human, Persona, Core, Archive) as readable markdown files,
> **so that** I can selectively edit and improve different aspects of my knowledge over time, with traceable links between entries.

**Acceptance Criteria:**
- [ ] Create `MemoryBlockType` (`human`, `persona`, `core`, `archive`) and `MemoryEntry` types (id, title, type, content, refs, tags, source, timestamps)
- [ ] Each entry is a `.md` file with YAML frontmatter, named by slugified title (e.g. `project-testing-stack.md`)
- [ ] Agents can read, create, update, and delete memory entries within their reasoning loop
- [ ] Entries link via `refs` (explicit) and `[[Title]]` wikilinks (implicit) for graph visualization
- [ ] Working memory context is assembled from `human` + `persona` + `core` block entries
- [ ] API endpoint `GET /api/memory/graph` returns all entries + edges for graph visualization
- [ ] Refactor existing `MemoryManager` to use `MemoryBlockStore`; remove old flat-array and markdown archival system

**Files:**
- `[NEW]` `core/src/memory/blocks/types.ts`
- `[NEW]` `core/src/memory/blocks/MemoryBlockStore.ts`
- `[MODIFY]` `core/src/memory/manager.ts` — block-based memory
- `[MODIFY]` `core/src/index.ts` — new block/entry/graph API routes
- `[DELETE]` `core/src/memory/archival.ts`
- `[DELETE]` `core/src/memory/test-memory.ts`

---

### Story 5.2: Auto-Compaction (Reflector)

> **As an** agent,
> **I want** old interactions automatically summarized and moved to archival memory,
> **so that** my working memory stays focused while retaining important context long-term.

**Acceptance Criteria:**
- [ ] Background process monitors `core` block entry count per agent
- [ ] When `core` exceeds threshold (20 entries), oldest entries are summarized by the LLM
- [ ] Summary is stored as a new `archive` entry that refs back to the original entries (preserving trace chain)
- [ ] Original entries are moved to `archive` type (preserving their IDs and existing refs)
- [ ] Compaction events are logged to the audit trail with `source: 'reflector'`

**Files:**
- `[NEW]` `core/src/memory/Reflector.ts`
- `[MODIFY]` `core/src/memory/manager.ts`

---

## ⚡ Epic 6: Skills Framework — ✅ DONE

*Make agents composable through a skills registry.*

> [!TIP]
> **Status**: Fully implemented. `SkillRegistry` (7.8KB + tests), 6 builtins
> (`file-read`, `file-write`, `web-search`, `knowledge-store`, `knowledge-query`), MCP tool bridge.

### Story 6.1: Skill Registry

> **As a** platform developer,
> **I want** to define reusable skills that agents can invoke,
> **so that** agent capabilities are modular and composable.

**Acceptance Criteria:**
- [ ] Create `SkillDefinition` interface with: id, description, parameters, handler
- [ ] Implement `SkillRegistry` that registers and looks up skills
- [ ] Agent manifests reference skills by ID; registry validates they exist
- [ ] Skills can invoke other skills (composition)
- [ ] Built-in skills: `web-search`, `file-read`, `file-write`, `knowledge-store`, `knowledge-query`

**Files:**
- `[NEW]` `core/src/skills/SkillRegistry.ts`
- `[NEW]` `core/src/skills/types.ts`
- `[NEW]` `core/src/skills/builtins/` — individual skill implementations

---

### Story 6.2: MCP Tools as Skills

> **As an** agent,
> **I want** MCP server tools automatically registered as skills,
> **so that** I can use any MCP tool through the same skills interface.

**Acceptance Criteria:**
- [ ] `MCPRegistry` tools are automatically wrapped as `SkillDefinition` entries
- [ ] Skill invocation calls the underlying MCP tool via the existing `MCPClient`
- [ ] Agent manifests can reference MCP tools in their `tools.allowed` list
- [ ] Tool results are returned in the standard skill result format

**Files:**
- `[MODIFY]` `core/src/mcp/registry.ts`
- `[MODIFY]` `core/src/skills/SkillRegistry.ts`

---

## 🔐 Epic 7: Orchestrator V2 — ✅ DONE

*Upgrade the orchestrator from POC to production-grade process management.*

> [!TIP]
> **Status**: Fully implemented. `ProcessManager` + `SequentialProcess` + `ParallelProcess` +
> `HierarchicalProcess` (all with tests), `AgentFactory` (2.6KB + tests), file watcher for hot-reload.

### Story 7.1: Process Patterns (Sequential, Parallel, Hierarchical)

> **As an** orchestrator,
> **I want** to execute tasks using different process patterns,
> **so that** I can run agents sequentially, in parallel, or in a manager-worker hierarchy.

**Acceptance Criteria:**
- [ ] Implement `ProcessManager` with three execution strategies
- [ ] **Sequential**: Tasks run one after another, output of one feeds into the next
- [ ] **Parallel**: Independent tasks run concurrently, results aggregated
- [ ] **Hierarchical**: Manager agent validates and can reject/retry worker results
- [ ] Process pattern is selectable per task or defaulted from circle configuration
- [ ] Refactor existing `Orchestrator.executeTask` to use `ProcessManager`

**Files:**
- `[NEW]` `core/src/agents/process/ProcessManager.ts`
- `[NEW]` `core/src/agents/process/SequentialProcess.ts`
- `[NEW]` `core/src/agents/process/ParallelProcess.ts`
- `[NEW]` `core/src/agents/process/HierarchicalProcess.ts`
- `[MODIFY]` `core/src/agents/Orchestrator.ts`

---

### Story 7.2: Dynamic Agent Creation

> **As an** orchestrator,
> **I want** to dynamically instantiate agents from YAML manifests at runtime,
> **so that** new agents can be added without restarting the system.

**Acceptance Criteria:**
- [ ] `AgentFactory` creates agent instances from loaded manifests
- [ ] New `AGENT.yaml` files placed in the agents directory are detected and loaded
- [ ] File watcher monitors the agents directory for changes
- [ ] API endpoint `POST /api/agents/reload` forces a manifest rescan
- [ ] Dashboard shows currently active agents and their status

**Files:**
- `[NEW]` `core/src/agents/AgentFactory.ts`
- `[MODIFY]` `core/src/agents/Orchestrator.ts`
- `[MODIFY]` `core/src/index.ts`

---

### Story 7.3: Party Mode (Multi-Agent Discussion)

> **As a** user,
> **I want** to start a group discussion with multiple agents from a circle,
> **so that** I can get diverse perspectives on a problem from agents with different expertise.

**Acceptance Criteria:**
- [ ] API endpoint `POST /api/circles/:circleId/party` starts a party mode session
- [ ] The circle's orchestrator selects 2-3 relevant agents per user message
- [ ] Each agent responds in character according to its identity/persona
- [ ] Agents can reference and build on each other's responses
- [ ] Party mode supports exit triggers and graceful conclusion
- [ ] Web UI renders party mode as a multi-avatar conversation

**Files:**
- `[NEW]` `core/src/circles/PartyMode.ts`
- `[MODIFY]` `core/src/routes/` — party mode route
- `[MODIFY]` `web/src/app/chat/page.tsx` — party mode UI

---

## 💾 Epic 8: Storage Abstraction — ✅ DONE

*Make the workspace storage layer pluggable for future multi-host scenarios.*

> [!TIP]
> **Status**: Fully implemented. `StorageProvider` interface + `LocalStorageProvider` + `DockerVolumeProvider` (all with tests).

### Story 8.1: Storage Provider Interface

> **As a** platform developer,
> **I want** workspace storage abstracted behind a provider interface,
> **so that** I can switch between local, NFS, and S3 storage without changing agent code.

**Acceptance Criteria:**
- [ ] Define `StorageProvider` interface with: `mount(agentId, config)`, `unmount(agentId)`, `getPath(agentId)`
- [ ] Implement `LocalStorageProvider` (bind mounts — current behavior)
- [ ] Implement `DockerVolumeProvider` (named Docker volumes)
- [ ] `AGENT.yaml` `workspace.provider` field selects the provider
- [ ] `SandboxManager` uses the provider to create volume mounts

**Files:**
- `[NEW]` `core/src/storage/StorageProvider.ts`
- `[NEW]` `core/src/storage/LocalStorageProvider.ts`
- `[NEW]` `core/src/storage/DockerVolumeProvider.ts`
- `[MODIFY]` `core/src/sandbox/SandboxManager.ts`

---

## 🖥️ Epic 9: Dashboard Integration

*Update the web UI to reflect the new agent architecture.*

### Story 9.1: Agent Management Page

> **As a** user,
> **I want** to see all registered agents, their status, and their circle membership,
> **so that** I can monitor and manage the agent system from the dashboard.

**Acceptance Criteria:**
- [ ] Agents page shows all loaded agents with: name, persona, circle, model, tier, status (running/stopped)
- [ ] Each agent links to its detail view showing full manifest, memory blocks, and recent activity
- [ ] User can start/stop agent brain containers from the UI
- [ ] API endpoints: `GET /api/agents`, `GET /api/agents/:id`, `POST /api/agents/:id/start`, `POST /api/agents/:id/stop`

**Files:**
- `[MODIFY]` `web/src/app/agents/page.tsx`
- `[NEW]` `core/src/routes/agents.ts`

---

### Story 9.2: Circle Overview Page

> **As a** user,
> **I want** a dashboard view of all circles showing their agents, knowledge stats, and intercom activity,
> **so that** I can understand the organizational structure of my agent system.

**Acceptance Criteria:**
- [ ] Circle page shows all circles with their agent rosters
- [ ] Each circle shows: agent count, knowledge entries, recent intercom messages
- [ ] Circle detail view shows the project-context.md content and allows editing
- [ ] API endpoints: `GET /api/circles`, `GET /api/circles/:id`

**Files:**
- `[NEW]` `web/src/app/circles/page.tsx`
- `[MODIFY]` `core/src/routes/` — circle routes

---

### Story 9.3: Agent Configuration Editor

> **As a** user,
> **I want** to edit agent manifests (model, tools, permissions, identity) from the web UI,
> **so that** I can tune agent behavior without manually editing YAML files.

**Acceptance Criteria:**
- [ ] Agent detail page includes a form-based editor for all `AGENT.yaml` fields
- [ ] Editable sections: identity (persona, communication style, principles), model (provider, name, fallback), tools (allowed/denied), subagents (allowed roles, max instances), intercom (peers, channels), resources (memory, CPU), and security tier
- [ ] Changes are validated against the `AgentManifest` schema before saving
- [ ] Saving writes the updated YAML back to disk and triggers a live reload (no restart needed)
- [ ] A "raw YAML" toggle allows advanced users to edit the manifest directly
- [ ] Change history is logged to the audit trail
- [ ] API endpoints: `PUT /api/agents/:id/manifest`, `GET /api/agents/:id/manifest/raw`

**Files:**
- `[NEW]` `web/src/app/agents/[id]/edit/page.tsx`
- `[NEW]` `core/src/routes/agents.ts` — manifest CRUD endpoints
- `[MODIFY]` `core/src/agents/manifest/AgentManifestLoader.ts` — write-back support

---

### Story 9.4: Skills & Tools Marketplace

> **As a** user,
> **I want** to browse available skills and MCP tools, install them, and assign them to agents from the dashboard,
> **so that** I can extend agent capabilities without touching configuration files.

**Acceptance Criteria:**
- [ ] Skills page lists all registered skills (built-in + MCP-bridged) with: id, description, source (builtin/mcp/custom), and which agents currently use them
- [ ] User can install new MCP tools by providing a server name and connection command
- [ ] User can assign/revoke skills and tools to/from agents via a drag-and-drop or checkbox UI
- [ ] Assigning a tool updates the agent's `AGENT.yaml` `tools.allowed` list and triggers live reload
- [ ] User can create custom skills by providing: id, description, and a handler template
- [ ] API endpoints: `GET /api/skills`, `POST /api/skills/install`, `PUT /api/agents/:id/tools`, `POST /api/mcp/register`

**Files:**
- `[NEW]` `web/src/app/tools/page.tsx`
- `[NEW]` `core/src/routes/skills.ts`
- `[MODIFY]` `core/src/skills/SkillRegistry.ts` — install/uninstall support
- `[MODIFY]` `core/src/mcp/registry.ts` — runtime registration API

---

### Story 9.5: Circle Management UI

> **As a** user,
> **I want** to create, edit, and manage circles from the dashboard,
> **so that** I can organize agents into teams and configure their shared context.

**Acceptance Criteria:**
- [ ] Circle page supports creating new circles (name, description, initial agent roster)
- [ ] Circle editor allows adding/removing agents from the roster
- [ ] Project context (`project-context.md`) is viewable and editable in a rich markdown editor
- [ ] Circle connections (to other circles) can be configured from the UI
- [ ] Party mode can be enabled/disabled and orchestrator agent selected per circle
- [ ] Changes write to `CIRCLE.yaml` and trigger live reload
- [ ] API endpoints: `POST /api/circles`, `PUT /api/circles/:id`, `DELETE /api/circles/:id`, `PUT /api/circles/:id/context`

**Files:**
- `[NEW]` `web/src/app/circles/[id]/edit/page.tsx`
- `[MODIFY]` `core/src/circles/CircleRegistry.ts` — write-back support
- `[NEW]` `core/src/routes/circles.ts` — circle CRUD endpoints

---

### Story 9.6: Agent Template Creator *(HIGH PRIORITY)*

> **As a** user,
> **I want** to create new agent templates from the dashboard,
> **so that** I can define custom assistants (personal finance advisor, meal planner, health coach, etc.) without editing YAML files.

**Acceptance Criteria:**
- [ ] "New Agent" button on the agents page opens a guided creation form
- [ ] Form fields: name, description/persona, communication style, system prompt (with a rich text/markdown editor), model selection (from available providers), tool selection (checkbox list), avatar/icon picker
- [ ] Advanced section: temperature, max tokens, principles, allowed sub-agents
- [ ] Preview mode: test-chat with the agent before saving
- [ ] Saving creates a new `AGENT.yaml` file and live-reloads the agent into the system
- [ ] Template gallery: user can duplicate and customize existing agent templates as a starting point
- [ ] API: `POST /api/agents/templates`, `GET /api/agents/templates`, `POST /api/agents/templates/:id/clone`

**Files:**
- `[NEW]` `web/src/app/agents/create/page.tsx`
- `[NEW]` `core/src/routes/agent-templates.ts`
- `[MODIFY]` `core/src/agents/manifest/AgentManifestLoader.ts` — write-back + template generation

---

## 🌐 Epic 10: Expansion Preparation (Future)

*These stories are documented for future work. They require no code now but influence design decisions above.*

### Story 10.1: Multi-Host via Docker Swarm

> **As a** homelab operator with multiple nodes,
> **I want** to distribute agent brain containers across hosts,
> **so that** I can scale beyond a single machine.

**Steps when ready:**
- [ ] Add Redis broker to Centrifugo config
- [ ] Add `NFSStorageProvider` implementation
- [ ] Switch `SandboxManager` from `docker.createContainer` to `docker.createService`
- [ ] Deploy via `docker stack deploy` instead of `docker-compose up`

---

### Story 10.2: External Subscribers

> **As an** external client (mobile app, CLI, webhook),
> **I want** to subscribe to agent thought streams and status channels,
> **so that** I can monitor agents from outside the SERA web UI.

**Steps when ready:**
- [ ] Configure Centrifugo JWT auth with channel-scoped claims
- [ ] Add `POST /auth/token` endpoint to issue scoped JWTs
- [ ] Enable `public:` channel namespace in Centrifugo config
- [ ] TLS termination via NPM for WebSocket connections

---

### Story 10.3: SERA-to-SERA Federation

> **As a** homelab community member,
> **I want** to connect my SERA instance with a friend's instance,
> **so that** our agents can collaborate across homelabs.

**Steps when ready:**
- [ ] Implement `BridgeService` for mTLS authentication between instances
- [ ] Support remote agent addressing (`agent@circle@instance`)
- [ ] Event-sourced knowledge sync via bridge channels
- [ ] `CIRCLE.yaml` `connections` block with remote circle references

---

### Story 10.4: Agent & Skills Sharing (Registry)

> **As a** SERA community member,
> **I want** to share my agent configs, skills, and tool definitions with others,
> **so that** the community can benefit from proven agent setups without building from scratch.

**Steps when ready:**
- [ ] Define an export format for agent configs (AGENT.yaml + identity + skills bundle)
- [ ] OCI-compliant packaging for agent manifests (push to container registries)
- [ ] Community registry API for publishing and discovering shared agents/skills
- [ ] Import workflow: browse registry → preview config → install → customize
- [ ] Versioned agent definitions with upgrade/rollback support

---

## 🔀 Epic 11: Workflow Engine *(from OpenFang — DEFERRED)*

> [!NOTE]
> **Deferred to future work.** The workflow engine extends orchestration patterns but is not needed
> for the core personal assistant experience. Revisit after the core chat UX is solid.

*Multi-step agent pipelines with variable substitution, parallel fan-out, and iterative loops.*

### Story 11.1: Workflow Definition & Engine Core

> **As a** platform developer,
> **I want** to define multi-step workflows that chain agents together,
> **so that** complex tasks can be broken into orchestrated pipelines.

**Acceptance Criteria:**
- [ ] Create `Workflow` type with: id, name, description, steps array, created_at
- [ ] Create `WorkflowStep` type with: name, agent reference (by name or ID), prompt template, mode, timeout, error mode, output variable
- [ ] Implement `WorkflowEngine` that stores workflow definitions and manages runs
- [ ] Support `{{input}}` (previous step output) and `{{variable_name}}` (named variables) in prompt templates
- [ ] Workflow runs track state: `Pending` → `Running` → `Completed` / `Failed`
- [ ] Each step records: agent info, output text, token counts, duration
- [ ] API: `POST /api/workflows`, `GET /api/workflows`, `POST /api/workflows/:id/run`, `GET /api/workflows/:id/runs`

**Files:**
- `[NEW]` `core/src/workflows/WorkflowEngine.ts`
- `[NEW]` `core/src/workflows/types.ts`
- `[NEW]` `core/src/routes/workflows.ts`

---

### Story 11.2: Step Modes (Sequential, Fan-Out, Collect, Conditional, Loop)

> **As a** workflow author,
> **I want** multiple execution modes for steps,
> **so that** I can model parallel work, conditional branching, and iterative refinement.

**Acceptance Criteria:**
- [ ] **Sequential** (default): Step runs after previous, chaining output as `{{input}}`
- [ ] **Fan-Out**: Consecutive fan-out steps run in parallel via `Promise.all()`, all receiving the same input
- [ ] **Collect**: Joins all fan-out outputs with `---` separator (data-only step, no agent execution)
- [ ] **Conditional**: Step executes only if previous output contains a specified substring (case-insensitive)
- [ ] **Loop**: Step repeats up to `maxIterations` times until output contains `until` substring
- [ ] Step timeout (default 120s) enforced per-step; fan-out steps get independent timeouts

**Files:**
- `[MODIFY]` `core/src/workflows/WorkflowEngine.ts`

---

### Story 11.3: Workflow Error Handling

> **As a** workflow author,
> **I want** per-step error handling policies,
> **so that** workflows can gracefully handle failures without always aborting.

**Acceptance Criteria:**
- [ ] **Fail** (default): Workflow aborts immediately on step failure
- [ ] **Skip**: Step failure is logged, workflow continues, `{{input}}` unchanged
- [ ] **Retry**: Step retries up to `maxRetries` times; each attempt gets its own timeout
- [ ] Run eviction cap (200 retained runs, LRU eviction of completed/failed runs)

**Files:**
- `[MODIFY]` `core/src/workflows/WorkflowEngine.ts`
- `[MODIFY]` `core/src/workflows/types.ts`

---

## ⚡ Epic 12: Trigger Engine *(from OpenFang)*

*Event-driven automation — triggers watch the event bus and auto-dispatch messages to agents.*

### Story 12.1: Trigger Registration & Matching

> **As a** platform operator,
> **I want** to register event triggers that auto-send messages to agents when patterns match,
> **so that** agents can react automatically to system events without polling.

**Acceptance Criteria:**
- [ ] Create `Trigger` type with: id, agentId, pattern, promptTemplate, enabled, fireCount, maxFires
- [ ] Implement `TriggerEngine` with pattern matching against Centrifugo/system events
- [ ] Support patterns: `All`, `AgentSpawned(namePattern)`, `AgentTerminated`, `System`, `MemoryUpdate`, `ContentMatch(substring)`
- [ ] `{{event}}` placeholder in prompt templates is replaced with human-readable event description
- [ ] Triggers auto-disable when `fireCount >= maxFires` (0 = unlimited)
- [ ] API: `POST /api/triggers`, `GET /api/triggers`, `PUT /api/triggers/:id`, `DELETE /api/triggers/:id`

**Files:**
- `[NEW]` `core/src/triggers/TriggerEngine.ts`
- `[NEW]` `core/src/triggers/types.ts`
- `[NEW]` `core/src/routes/triggers.ts`

---

## 🛡️ Epic 13: Agent Loop Stability *(from OpenFang)*

*Hardening the agent reasoning loop to prevent runaway behavior.*

### Story 13.1: Loop Guard & Tool Safety

> **As a** platform developer,
> **I want** the agent loop to detect and prevent degenerate patterns,
> **so that** agents don't get stuck in infinite loops or exhaust resources.

**Acceptance Criteria:**
- [ ] **Loop Guard**: Hash `(toolName, params)` with SHA256; warn at 3 repeats, block at 5, circuit-break at 30
- [ ] **Tool Timeout**: All tool executions wrapped in configurable timeout (default 60s)
- [ ] **Tool Result Truncation**: Hard cap at 50K characters with truncation marker
- [ ] **Max Continuations**: Cap "please continue" loops at 3 iterations
- [ ] **Inter-Agent Depth Limit**: Max recursive agent-to-agent call depth of 5
- [ ] **Stability Guidelines**: Anti-loop behavioral rules injected into system prompts

**Files:**
- `[NEW]` `core/src/agents/stability/LoopGuard.ts`
- `[NEW]` `core/src/agents/stability/ToolTimeout.ts`
- `[MODIFY]` `core/src/agents/BaseAgent.ts` — integrate stability systems

---

### Story 13.2: Session Repair & Auto-Compaction

> **As an** agent,
> **I want** my message history validated and auto-compacted,
> **so that** corrupted sessions are self-healing and my context window stays within limits.

**Acceptance Criteria:**
- [ ] **Session Repair**: Before each agent loop, validate message history — drop orphaned tool results, remove empty messages, merge consecutive same-role messages
- [ ] **Block-Aware Compaction**: Auto-compact when session exceeds 80% of context window, keeping the most recent N messages
- [ ] Compaction produces a summary that preserves key context
- [ ] Compaction events logged with before/after token counts

**Files:**
- `[NEW]` `core/src/agents/stability/SessionRepair.ts`
- `[NEW]` `core/src/agents/stability/SessionCompactor.ts`
- `[MODIFY]` `core/src/agents/BaseAgent.ts`

---

## 💰 Epic 14: Metering & Budget *(from OpenFang)*

*Track token usage, costs, and enforce per-agent quotas.*

### Story 14.1: Usage Tracking & Cost Estimation

> **As a** platform operator,
> **I want** to track token usage and estimated costs per agent,
> **so that** I can monitor spending and identify expensive agents.

**Acceptance Criteria:**
- [ ] Create `UsageEvent` type: agentId, model, inputTokens, outputTokens, costUsd, timestamp
- [ ] `MeteringEngine` records usage after every LLM call
- [ ] Cost estimation based on a model pricing catalog (per-model input/output rates)
- [ ] PostgreSQL `usage_events` table for persistence
- [ ] API: `GET /api/budget` (global), `GET /api/budget/agents` (per-agent ranking), `GET /api/budget/agents/:id`
- [ ] Dashboard widget showing daily/weekly/monthly spend

**Files:**
- `[NEW]` `core/src/metering/MeteringEngine.ts`
- `[NEW]` `core/src/metering/ModelPricingCatalog.ts`
- `[NEW]` `core/src/metering/types.ts`
- `[NEW]` `core/src/routes/budget.ts`

---

### Story 14.2: Per-Agent Token Quotas

> **As a** platform operator,
> **I want** to set hourly token limits per agent,
> **so that** a runaway agent can't burn through my entire API budget.

**Acceptance Criteria:**
- [ ] `AGENT.yaml` `resources.maxLlmTokensPerHour` field sets the quota
- [ ] `AgentScheduler` tracks per-agent usage with rolling 1-hour window (auto-reset)
- [ ] Quota exceeded → return `QuotaExceeded` error to the agent, do not send the LLM request
- [ ] Quota warnings published to event bus at configurable thresholds (e.g., 80%, 90%)
- [ ] API: `GET /api/agents/:id/quota` shows current usage vs limit

**Files:**
- `[NEW]` `core/src/metering/AgentScheduler.ts`
- `[MODIFY]` `core/src/agents/BaseAgent.ts` — check quota before LLM calls
- `[MODIFY]` `core/src/agents/manifest/types.ts` — add resources.maxLlmTokensPerHour

---

## 📡 Epic 15: Channel Adapters *(from OpenFang)*

*Connect agents to external messaging platforms.*

### Story 15.1: Channel Adapter Framework

> **As a** platform developer,
> **I want** a standard adapter interface for messaging platforms,
> **so that** new channels can be added with minimal boilerplate.

**Acceptance Criteria:**
- [ ] Create `ChannelAdapter` interface with: `start()`, `stop()`, `sendMessage()`, `onMessage(callback)`
- [ ] Create `ChannelRouter` that routes incoming messages to the correct agent
- [ ] Per-channel config: default agent, allowed users, rate limiting, model override
- [ ] Output formatter: Markdown → platform-specific format (HTML for Telegram, mrkdwn for Slack, plain text fallback)
- [ ] Per-user rate limiting via in-memory tracking (prevent message flooding)
- [ ] Channel config in `config.yaml` under `channels.*` sections

**Files:**
- `[NEW]` `core/src/channels/ChannelAdapter.ts`
- `[NEW]` `core/src/channels/ChannelRouter.ts`
- `[NEW]` `core/src/channels/Formatter.ts`
- `[NEW]` `core/src/channels/RateLimiter.ts`
- `[NEW]` `core/src/channels/types.ts`

---

### Story 15.2: Telegram, Discord & WhatsApp Adapters

> **As a** user,
> **I want** to interact with SERA agents via Telegram, Discord, and WhatsApp,
> **so that** I can use agents from my preferred messaging platform.

**Acceptance Criteria:**
- [ ] **Telegram**: Long-polling adapter using Bot API, allowed user filtering, thread support
- [ ] **Discord**: Gateway adapter via discord.js, guild/channel filtering, slash commands
- [ ] **WhatsApp Web**: QR-code-based connection (like OpenFang's whatsapp-gateway), Node.js sidecar
- [ ] Each adapter supports per-channel model and system prompt overrides
- [ ] Incoming messages routed to the configured default agent; responses sent back formatted for the platform
- [ ] Chat commands: `/models`, `/new` (new session), `/stop`, `/usage`

**Files:**
- `[NEW]` `core/src/channels/adapters/TelegramAdapter.ts`
- `[NEW]` `core/src/channels/adapters/DiscordAdapter.ts`
- `[NEW]` `packages/whatsapp-gateway/` — Node.js sidecar (port from OpenFang)

---

## 🤖 Epic 16: Autonomous Hands *(from OpenFang — DEFERRED)*

> [!NOTE]
> **Deferred to future work.** Hands require workflows, triggers, and metering to be in place first.
> Focus on getting the core chat experience and agent creation working first.

*Pre-built autonomous capability packages that run independently on schedules.*

### Story 16.1: HAND.yaml Manifest & Lifecycle

> **As a** platform operator,
> **I want** to define autonomous Hands as declarative YAML manifests,
> **so that** they can be activated, paused, and configured from the dashboard.

**Acceptance Criteria:**
- [ ] Create `HandManifest` type: id, name, description, category, icon, tools, settings, agent config, dashboard metrics
- [ ] `HandRegistry` loads HAND.yaml files and manages lifecycle (activate/pause/resume/deactivate)
- [ ] Hands run on configurable schedules (cron expressions or intervals) via `BackgroundExecutor`
- [ ] Configurable settings (select, toggle, text) exposed in dashboard UI
- [ ] Dashboard metrics pulled from agent memory keys (e.g., `researcher_hand_queries_solved`)
- [ ] API: `GET /api/hands`, `POST /api/hands/:id/activate`, `POST /api/hands/:id/pause`, `GET /api/hands/:id/status`

**Files:**
- `[NEW]` `core/src/hands/HandRegistry.ts`
- `[NEW]` `core/src/hands/BackgroundExecutor.ts`
- `[NEW]` `core/src/hands/types.ts`
- `[NEW]` `core/src/routes/hands.ts`

---

### Story 16.2: Researcher & Collector Hands

> **As a** user,
> **I want** pre-built Researcher and Collector Hands,
> **so that** I can run autonomous deep research and OSINT monitoring out of the box.

**Acceptance Criteria:**
- [ ] **Researcher Hand**: Multi-phase playbook (question analysis → search strategy → information gathering → cross-reference → fact-check → report generation → stats), CRAAP evaluation, configurable depth/style/citation
- [ ] **Collector Hand**: Continuous target monitoring, change detection, knowledge graph construction, critical alert publishing
- [ ] Both Hands store their state in agent memory for persistence across restarts
- [ ] Both Hands publish completion events to the intercom for UI notification
- [ ] Example `HAND.yaml` files in `sera/hands/`

**Files:**
- `[NEW]` `sera/hands/researcher.hand.yaml`
- `[NEW]` `sera/hands/collector.hand.yaml`

---

## 🔌 Epic 17: OpenAI-Compatible API *(from OpenFang)*

*Drop-in replacement endpoint for OpenAI client libraries.*

### Story 17.1: Chat Completions Endpoint

> **As a** developer,
> **I want** SERA to expose an OpenAI-compatible `/v1/chat/completions` endpoint,
> **so that** I can use SERA agents from any tool that supports the OpenAI API format.

**Acceptance Criteria:**
- [ ] `POST /v1/chat/completions` accepts the standard OpenAI request format (model, messages, stream, tools)
- [ ] The `model` field maps to a SERA agent name (e.g., `model: "researcher"` routes to the researcher agent)
- [ ] Streaming support via SSE (server-sent events) with OpenAI-compatible delta format
- [ ] Non-streaming returns a complete response object with usage stats
- [ ] `GET /v1/models` returns all available agents in OpenAI model list format
- [ ] Bearer token auth matches SERA's API key config

**Files:**
- `[NEW]` `core/src/routes/openai-compat.ts`
- `[MODIFY]` `core/src/index.ts` — mount `/v1/*` routes

---

## 🔏 Epic 18: Audit Trail *(from OpenFang)*

*Cryptographically linked action logging for tamper-evident audit.*

### Story 18.1: Merkle Hash-Chain Audit Log

> **As a** platform operator,
> **I want** every agent action cryptographically linked in a hash chain,
> **so that** the audit trail is tamper-evident and I can verify integrity at any time.

**Acceptance Criteria:**
- [ ] Create `AuditEntry` type: id, agentId, action, details, timestamp, previousHash, hash
- [ ] Each entry's hash = SHA256(previousHash + action + details + timestamp)
- [ ] All agent actions (tool calls, LLM requests, memory writes, intercom messages) generate audit entries
- [ ] PostgreSQL `audit_trail` table with index on agentId and timestamp
- [ ] `AuditService.verify()` validates the entire chain for a given agent
- [ ] API: `GET /api/audit/:agentId` (paginated), `GET /api/audit/:agentId/verify`

**Files:**
- `[NEW]` `core/src/audit/AuditService.ts`
- `[NEW]` `core/src/audit/types.ts`
- `[NEW]` `core/src/routes/audit.ts`

---

## 📅 Recommended Execution Order

### Priority: Get the Chat Working First

The new execution order puts the **core chat experience** front and center. Everything else builds on top of a working, polished conversational interface.

```
┌──────────────────────────────────────────────────────┐
│  PHASE 1: MAKE IT WORK (Core Chat UX)           │
│                                                  │
│  Epic 0 (Chat + Sessions + Thinking + Tools)     │
│  Epic 1 (Agent Manifests + AGENT.yaml)            │
│  Epic 13 (Loop Stability)                        │
│  Story 9.6 (Agent Template Creator UI)           │
└──────────────────────────────────────────────────────┘
                        │
                        ▼
┌──────────────────────────────────────────────────────┐
│  PHASE 2: MAKE IT SMART (Intelligence Layer)      │
│                                                  │
│  Epic 4 (Intercom / Agent-to-Agent)              │
│  Epic 5 (Memory Blocks)                          │
│  Epic 6 (Skills Framework)                       │
│  Epic 14 (Metering & Budget)                     │
└──────────────────────────────────────────────────────┘
                        │
                        ▼
┌──────────────────────────────────────────────────────┐
│  PHASE 3: MAKE IT SECURE (Isolation & Infra)      │
│                                                  │
│  Epic 2 (Circles) + Epic 3 (Sandbox)             │
│  Epic 7 (Orchestrator V2)                        │
│  Epic 8 (Storage Abstraction)                    │
│  Epic 18 (Audit Trail)                           │
└──────────────────────────────────────────────────────┘
                        │
                        ▼
┌──────────────────────────────────────────────────────┐
│  PHASE 4: MAKE IT CONNECTED (Expansion)           │
│                                                  │
│  Epic 9 (Dashboard polish)                       │
│  Epic 12 (Triggers) + Epic 15 (Channels)         │
│  Epic 17 (OpenAI-Compatible API)                 │
└──────────────────────────────────────────────────────┘
                        │
                        ▼
┌──────────────────────────────────────────────────────┐
│  FUTURE: Automation & Autonomy                   │
│                                                  │
│  Epic 10 (Multi-Host / Federation)               │
│  Epic 11 (Workflows) — deferred                  │
│  Epic 16 (Autonomous Hands) — deferred            │
└──────────────────────────────────────────────────────┘
```

**Suggested sprint sequence:**

1. **Sprint 1**: **Epic 0** (Chat + Sessions + Thinking + Tools) + Epic 1 (Manifests) — the core experience
2. **Sprint 2**: **Epic 13 (Loop Stability)** + **Story 9.6 (Agent Template Creator)** — quality + creation UX
3. **Sprint 3**: Epic 4 (Intercom) + Epic 6 (Skills) — agent-to-agent comms & capabilities
4. **Sprint 4**: Epic 5 (Memory Blocks) + **Epic 14 (Metering)** — intelligence & budget
5. **Sprint 5**: Epic 2 (Circles) + Epic 3 (Sandbox) + Epic 7 (Orchestrator V2) — isolation & process
6. **Sprint 6**: Epic 12 (Triggers) + Epic 15 (Channels) + Epic 17 (OpenAI API) — connectivity
7. **Sprint 7**: Epic 8 (Storage) + Epic 9 (Dashboard polish) + Epic 18 (Audit) — infrastructure
8. **Future**: Epic 10 (Expansion) + Epic 11 (Workflows) + Epic 16 (Hands)

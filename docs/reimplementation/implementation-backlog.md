# 📋 SERA Implementation Backlog

> Comprehensive task list with user stories for implementing the SERA agent workspace architecture.
> Based on the current project state as of 2026-03-16.
>
> **Reference**: [agent-workspace-architecture.md](./agent-workspace-architecture.md)

---

## 📊 Current State Summary

| Component | Status | Notes |
|---|---|---|
| Express API Server | ✅ Working | Health, chat, config, memory, vector endpoints |
| LLM Provider (OpenAI-compatible) | ✅ Working | Multi-provider config with persistence |
| BaseAgent / PrimaryAgent | 🟡 POC | LLM-backed, JSON response parsing, no tools |
| WorkerAgent | 🔴 Stub | Returns hardcoded response, no real processing |
| Orchestrator | 🟡 POC | Simple delegation, no process patterns |
| MCP Registry | 🟡 POC | Client registration works, no agent integration |
| Memory Manager | 🟡 POC | Working memory (array) + archival (markdown files) |
| Vector Services | 🟡 POC | Embedding + ingestion + Qdrant search |
| LSP Router | 🟡 POC | Basic route, no symbol-level integration |
| Web UI | 🟡 POC | Sidebar, chat page, settings, agents/insights/schedules pages |
| Docker Compose | ✅ Working | sera-core, sera-web, centrifugo, postgres, qdrant |
| Centrifugo | 🔴 Unused | Deployed but not integrated into agent system |
| AGENT.yaml / Circle system | 🔴 Missing | Not yet implemented |
| Sandbox Manager | 🔴 Missing | Not yet implemented |
| Intercom | 🔴 Missing | Not yet implemented |

---

## 🏗️ Epic 1: Agent Manifest System

*Establish the declarative agent definition system that everything else builds on.*

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

## 🔄 Epic 2: Circle System

*Implement the organizational layer that groups agents and scopes knowledge.*

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

## 🐳 Epic 3: Sandbox Manager

*Implement the secure container management layer that agents use instead of direct Docker access.*

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

## 📡 Epic 4: Intercom System

*Connect agents within and across circles using Centrifugo.*

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

## 🧠 Epic 5: Memory Blocks (Letta-style × Obsidian-style)

*Upgrade the POC memory system to structured, graph-linked memory blocks stored as human-readable markdown files.*

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

## ⚡ Epic 6: Skills Framework

*Make agents composable through a skills registry.*

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

## 🔐 Epic 7: Orchestrator V2

*Upgrade the orchestrator from POC to production-grade process management.*

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

## 💾 Epic 8: Storage Abstraction

*Make the workspace storage layer pluggable for future multi-host scenarios.*

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

## 📅 Recommended Execution Order

The epics are designed with clear dependencies:

```
Epic 1 (Agent Manifests) ──────┐
                               ├──► Epic 3 (Sandbox Manager)
Epic 2 (Circles) ─────────────┘        │
                                        ├──► Epic 7 (Orchestrator V2)
Epic 4 (Intercom) ─────────────────────┘        │
                                                 ├──► Epic 9 (Dashboard)
Epic 5 (Memory Blocks) ────────────────────────┘
                                                
Epic 6 (Skills) ─── can be done in parallel with Epics 3-5
Epic 8 (Storage) ── can be done in parallel with Epics 3-5
Epic 10 (Expansion) ── future work, no code required now
```

**Suggested sprint sequence:**

1. **Sprint 1**: Epic 1 (Manifests) + Epic 2 (Circles) — the schema foundation
2. **Sprint 2**: Epic 3 (Sandbox Manager) + Epic 8 (Storage) — the runtime layer
3. **Sprint 3**: Epic 4 (Intercom) + Epic 6 (Skills) — the communication & capability layer
4. **Sprint 4**: Epic 5 (Memory Blocks) + Epic 7 (Orchestrator V2) — the intelligence layer
5. **Sprint 5**: Epic 9 (Dashboard) — the user-facing integration
6. **Backlog**: Epic 10 (Expansion) — when the single-host system is proven

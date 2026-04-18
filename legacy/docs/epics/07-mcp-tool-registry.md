# Epic 07: MCP Tool Registry

## Overview

MCP (Model Context Protocol) tools are the executable layer — they run code, produce side effects, and return structured results to the agent. SERA's MCP registry bridges external tool providers into the agent tool-calling system. Critically, untrusted or external MCP servers must run in their own sandboxed containers — not on the host — with the same isolation guarantees as agent containers. This is SERA's key differentiator over agent frameworks that allow MCP servers to run with host-level access.

## Context

- See `docs/ARCHITECTURE.md` → Extensibility Model (MCP servers as containers), Skills vs MCP Tools
- MCP tools are separate from skills: tools execute code, skills are guidance text
- MCPServerManifest is a public spec (like AGENT.yaml) — community-publishable
- Internal built-in tools (file-read, shell-exec, etc.) are registered directly in SkillRegistry, not via MCP
- The `agent_net` Docker network is shared between agent containers and MCP server containers

## Dependencies

- Epic 01 (Infrastructure) — `agent_net` network
- Epic 02 (Agent Manifest) — `tools.allowed` field
- Epic 03 (Docker Sandbox) — `SandboxManager` for MCP container lifecycle

---

## Stories

### Story 7.1: MCPServerManifest specification

**As a** developer or contributor
**I want** a documented MCPServerManifest format for defining sandboxed MCP tool providers
**So that** I can publish reusable, sandboxed tool providers for the community

**Acceptance Criteria:**
- [ ] `MCPServerManifest` format documented in `docs/mcp/FORMAT.md`
- [ ] JSON Schema at `schemas/mcp-server-manifest.v1.json`
- [ ] Required fields: `apiVersion: sera/v1`, `kind: SkillProvider`, `metadata.name`, `image`, `transport` (`stdio | http`)
- [ ] Optional fields: `network.allowlist` (egress hostnames), `mounts` (read-only file mounts into MCP container), `secrets` (env var names sourced from sera-core secrets store), `healthCheck`
- [ ] Example manifests: a GitHub MCP server, a local filesystem tool server, a web search server
- [ ] Manifests live in `mcp-servers/{name}.mcp.yaml`

---

### Story 7.2: MCPRegistry — host-side MCP connections (current state)

**As** sera-core
**I want** to manage connections to MCP servers and expose their tools in SkillRegistry
**So that** agents can call MCP tools via the standard tool-calling interface

**Acceptance Criteria:**
- [ ] `MCPRegistry` maintains a map of MCP server name → active connection
- [ ] Supports `stdio` transport: spawns MCP server as a child process with stdin/stdout JSON-RPC
- [ ] Supports `http` transport: connects to an MCP server via HTTP
- [ ] On connect: calls `tools/list` to discover available tools
- [ ] Each discovered tool bridged into `SkillRegistry` with `source: 'mcp'` and `serverId: serverName`
- [ ] Tool ID format: `{serverName}/{toolName}` (e.g. `github-mcp/create_pull_request`)
- [ ] On disconnect: tools from that server removed from SkillRegistry
- [ ] `GET /api/mcp-servers` lists connected servers and their tool counts

---

### Story 7.3: Containerised MCP servers (sandboxed model)

**As** sera-core
**I want** to spawn MCP servers in isolated Docker containers rather than as host processes
**So that** untrusted external MCP servers cannot access host resources

**Acceptance Criteria:**
- [ ] `MCPServerManager` (extending `SandboxManager` patterns) spawns MCP server containers from `MCPServerManifest`
- [ ] MCP server containers: connected to `agent_net` only, not `sera_net`
- [ ] Network egress controlled by `manifest.network.allowlist` — default: no outbound
- [ ] Secrets injected as env vars into the MCP server container (not the agent container)
- [ ] `transport: stdio` — sera-core connects to the MCP container via Docker exec/attach, not network
- [ ] `transport: http` — MCP server listens on a port inside `agent_net`; sera-core connects by container name
- [ ] MCP server containers labelled with `sera.type=mcp-server`, `sera.mcp-server={name}`
- [ ] Container lifecycle tied to registry lifecycle: container started when server registered, stopped when unregistered
- [ ] Agent containers do NOT connect directly to MCP server containers — all tool calls go through sera-core

**Technical Notes:**
- The agent → sera-core → MCP container topology is intentional: sera-core is always in the call path for governance
- This means tool execution via MCP has a round-trip: agent calls Core proxy, Core calls MCP container, result returned

---

### Story 7.4: Agent tool access control

**As** sera-core
**I want** to enforce an agent's `tools.allowed` and `tools.denied` lists at tool execution time
**So that** an agent cannot call tools outside its declared capability scope

**Acceptance Criteria:**
- [ ] `ToolExecutor.execute(toolId, args, agentContext)` checks `toolId` against agent's allowed/denied lists
- [ ] `tools.denied` takes precedence — a tool in both lists is denied
- [ ] Wildcard support: `tools.allowed: [github-mcp/*]` allows all tools from the `github-mcp` server
- [ ] Denied tool call returns a tool result with `error: 'tool_not_permitted'` — does not throw, allows LLM to handle gracefully
- [ ] Tool access decisions logged to audit trail
- [ ] `GET /api/agents/:id/tools` returns the agent's resolved tool list (allowed tools with their descriptions)

---

### Story 7.5: MCP server hot-registration

**As an** operator
**I want** to add and remove MCP servers at runtime without restarting sera-core
**So that** I can expand agent capabilities dynamically

**Acceptance Criteria:**
- [ ] `POST /api/mcp-servers` registers a new MCP server from a manifest body or manifest file path
- [ ] `DELETE /api/mcp-servers/:name` unregisters a server and removes its tools from SkillRegistry
- [ ] Adding an MCP server that is already registered: returns 409 Conflict
- [ ] Tool additions/removals broadcast to connected agents via Centrifugo `system.tools` channel
- [ ] `POST /api/mcp-servers/:name/reload` reconnects to a server and refreshes its tool list
- [ ] MCP server manifests in `mcp-servers/` directory automatically loaded at sera-core startup

---

### Story 7.6: Built-in tool registry

**As** sera-core
**I want** built-in tools (file I/O, shell, web, knowledge, scheduling) registered in SkillRegistry
**So that** agents can use core capabilities without an MCP server

**Acceptance Criteria:**
- [ ] All built-in tools registered at startup with `source: 'builtin'`
- [ ] Built-in tool IDs: `file-read`, `file-write`, `file-list`, `file-delete`, `shell-exec`, `web-search`, `web-fetch`, `knowledge-store`, `knowledge-query`, `schedule-task`
- [ ] Each tool has: `id`, `name`, `description`, `inputSchema` (JSON Schema for arguments), `handler`
- [ ] Tool descriptions are LLM-optimised — clear, concise, with parameter descriptions that help the LLM use them correctly
- [ ] Built-in tool definitions exposed via `GET /v1/llm/tools` so agent runtime can fetch the tool schema list
- [ ] `web-search` uses DuckDuckGo HTML scraping (no API key required) — compatible with homelab/offline-first goal

---

### Story 7.7: sera-core as MCP server

**As an** agent with `seraManagement` capabilities
**I want** to call sera-core management operations as MCP tools
**So that** I can orchestrate the SERA instance — creating agents, managing circles, scheduling tasks — from within my own reasoning loop

**Acceptance Criteria:**
- [ ] sera-core runs an embedded MCP server on a dedicated internal endpoint (e.g. `sera-core:3001/mcp`)
- [ ] The MCP server is registered in `MCPRegistry` under the name `sera-core` — same infrastructure as external MCP servers
- [ ] Agent manifests grant access via `tools.allowed: [sera-core/agents.*, sera-core/circles.*]` etc. — wildcard and explicit patterns both supported
- [ ] Each tool call to `sera-core/*` is authenticated: agent's JWT validated, then `seraManagement` capability checked for the specific operation before execution
- [ ] Capability check is fine-grained: `agents.create` checked for `agents.create`, `agents.modify` + scope check for `agents.modify`
- [ ] Scope checks enforced at the MCP handler level, not just the capability model — e.g. `modify: allow: [own-circle]` means the handler verifies the target agent is in the acting agent's circle before proceeding
- [ ] Tools exposed (initial set):
  - `agents.list(filters?)`, `agents.get(id)`, `agents.create(templateRef, name, circle, overrides?)`, `agents.modify(id, overrides)`, `agents.start(id)`, `agents.stop(id)`
  - `templates.list(filters?)`, `templates.get(name)`
  - `circles.list()`, `circles.get(id)`, `circles.create(name, constitution?)`, `circles.addMember(circleId, agentName)`
  - `schedules.create(agentId, name, type, expression, task)`, `schedules.list(agentId?)`
  - `skills.list(filters?)`
  - `providers.list()`
- [ ] All management tool calls recorded in audit trail: actor agent, tool name, arguments (sanitised), result
- [ ] `agents.create` called by an agent always sets `parent_instance_id` to the calling agent — lineage preserved
- [ ] Tool input schemas are LLM-optimised with clear parameter descriptions
- [ ] `GET /api/mcp-servers/sera-core/tools` lists the available management tools (honoring the requesting agent's capabilities — tools it cannot use are omitted from the list)

**Technical Notes:**
- The sera-core MCP server is intentionally internal — it is not accessible from outside the `sera_net` Docker network
- Authentication uses the same JWT mechanism as all other sera-core endpoints — the MCP server is not a privileged backdoor, it is a governed interface
- This is the mechanism through which Sera (the primary agent) orchestrates the instance; it is also how future multi-agent workflows can self-modify their own composition

---

### Story 7.8: SERA MCP Extension Protocol implementation

**As** sera-core
**I want** to implement the SERA MCP Extension Protocol when calling MCP server containers
**So that** credentials, acting context, and standard error codes are consistently delivered to all MCP servers on every tool call

**Acceptance Criteria:**
- [ ] For HTTP transport MCP servers: sera-core adds `X-Sera-Acting-Context`, `X-Sera-Credential-{NAME}`, and `X-Sera-Instance-Id` headers on every `tools/call` request
- [ ] For stdio transport MCP servers: sera-core wraps the JSON-RPC `tools/call` message with a `_sera` envelope field containing `{ actingContext, credentials, instanceId }` before sending; strips the envelope from the `arguments` field passed to the handler
- [ ] Credentials injected via the protocol are resolved by `CredentialResolver` (Epic 17 Story 17.5) at call time — not from the container's startup environment
- [ ] `ActingContext` serialised to base64 JSON for HTTP header transport; raw JSON object in the `_sera` stdio envelope
- [ ] If a tool's `x-sera.requiresCredentials` declaration lists a credential that `CredentialResolver` cannot resolve: sera-core returns `{ error: 'credential_unavailable', requiredCredential: '...' }` to the agent without calling the MCP server
- [ ] Standard SERA error codes (`credential_unavailable`, `tool_not_permitted`, `acting_context_invalid`, `scope_exceeded`) handled by the MCP proxy: mapped to structured tool result errors returned to the agent
- [ ] `GET /api/mcp-servers/:name/tools` response includes `x-sera` extension metadata per tool when present
- [ ] Protocol version negotiated at registration: `POST /api/mcp-servers` response includes `{ seraExtensions: true | false }` indicating whether the server advertised SERA extension support
- [ ] MCP servers without SERA extension support still work — they receive the base MCP protocol only; credential injection is skipped with a warning log

**Technical Notes:**
- The `_sera` envelope is a SERA-defined extension to the base MCP JSON-RPC format; it is outside the `params.arguments` object so it does not affect tool input schema validation
- The `@sera/mcp-sdk` (Epic 15 Story 15.8) handles envelope parsing for SDK-using servers; raw MCP servers must parse it themselves if they want acting context or credentials

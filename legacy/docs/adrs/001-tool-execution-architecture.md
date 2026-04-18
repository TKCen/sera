# ADR-001: Tool Execution Architecture

**Status:** Proposed
**Date:** 2026-03-30
**Related:** Epic 7 (MCP Tool Registry), Epic 5 (Agent Runtime), Epic 3 (Sandbox), Issue #462, #482

## Context

SERA agents execute tasks through an LLM reasoning loop that can invoke tools. The tool system spans three execution layers (core, agent containers, MCP containers) with different security boundaries. The canonical design (documented in Epics 3, 5, 7 and ARCHITECTURE.md) defines a specific topology, but the implementation has deviated in ways that limit agent capabilities.

## Canonical Design (from Epics)

### Tool Execution Topology (Story 7.3)

```
Agent Container ──→ sera-core ──→ MCP Server Container
(local tools)       (governance)   (external tools)
```

> "Agent containers do NOT connect directly to MCP server containers — all tool calls go through sera-core for governance."

### Core as Single Source of Truth (Story 7.6)

> "Built-in tool definitions exposed via `GET /v1/llm/tools` so agent-runtime can fetch the tool schema list."

The agent-runtime fetches its tool catalog dynamically from core at startup, not from a hardcoded list.

### Tool Proxy for Delegated Execution (Story 3.10)

> "sera-core acts as a host-side file proxy — RuntimeToolExecutor detects that the target path is outside /workspace but covered by a grant → forwards the tool call to sera-core via `POST /v1/tools/proxy`"

This proxy pattern generalizes: any tool the agent cannot execute locally gets proxied to core.

### Tool Filtering (Story 7.4)

> "`ToolExecutor.execute(toolId, args, agentContext)` checks toolId against agent's allowed/denied lists"

Capability checking happens at the core level, not in the agent container.

## Current Implementation State

### What's Implemented

| Component | Status |
|---|---|
| SkillRegistry (core) | ✅ 12+ skills registered (file-*, knowledge-*, web-*, etc.) |
| RuntimeToolExecutor (agent-runtime) | ⚠️ Only 7 tools hardcoded |
| `POST /v1/tools/proxy` | ⚠️ Filesystem operations only |
| `GET /v1/llm/tools` | ❌ Not implemented |
| Agent manifest `tools.allowed` | ✅ Declared but partially enforced |
| MCP tool bridge | ✅ Core-side only (agents can't call MCP tools) |

### Deviations from Canonical Design

#### 1. Hardcoded Tool Catalog (vs. Dynamic)

**Canonical:** Agent-runtime fetches `GET /v1/llm/tools` at startup.
**Actual:** Agent-runtime has hardcoded `BUILTIN_TOOLS` array with 7 entries.

**Impact:** Adding a new tool requires an agent-runtime image rebuild. Agents cannot access knowledge-store, web-search, web-fetch, knowledge-query, or schedule-task — even though these are registered in core's SkillRegistry and declared in agent manifests.

**Reasoning for deviation:** The agent-runtime was built in Epic 5 before the tool framework was designed in Epic 7. The local-first approach was pragmatic for the initial file/shell tools.

#### 2. No Remote Tool Invocation (vs. Proxy)

**Canonical:** Agent detects it can't execute locally → proxies to `POST /v1/tools/proxy`.
**Actual:** Proxy exists only for filesystem grants (Story 3.10). No generalized skill invocation.

**Impact:** The 5 API-backed tools (knowledge-store, knowledge-query, web-search, web-fetch, schedule-task) are core-side only and unreachable from agent containers.

**Reasoning:** The SkillHandler interface requires a full `AgentContext` (manifest, sandboxManager, containerId, tier) which is not available at a remote proxy endpoint. The handler contract needs to be split into local-context and remote-context variants.

#### 3. Tool Definitions Duplicated

**Canonical:** Single source of truth in SkillRegistry.
**Actual:** Tool schemas defined twice — once in SkillRegistry (core) and once in BUILTIN_TOOLS (agent-runtime).

**Impact:** Schema drift, maintenance burden, and a class of bugs where the LLM sees different tool parameters than what the handler expects.

## Decision

### Recommended Path

1. **Split SkillHandler into two context types:**
   - `LocalSkillContext` — for tools running in agent containers (has filesystem, shell)
   - `RemoteSkillContext` — for tools invoked via proxy (has agentId, agentName, capabilities only)

2. **Implement `GET /v1/llm/tools?agentId={id}`** — returns filtered tool catalog in OpenAI function-calling format. Each tool includes `executionMode: 'local' | 'remote'`.

3. **Generalize `POST /v1/tools/proxy`** — accept any skill ID, construct `RemoteSkillContext` from the JWT identity, delegate to SkillRegistry.

4. **Update agent-runtime** — fetch catalog at startup with BUILTIN_TOOLS fallback. Route `remote` tools to the proxy. Keep `local` tools executing natively.

5. **Phase out BUILTIN_TOOLS** — once the catalog endpoint is reliable, remove hardcoded definitions entirely.

### What Stays Local

- `file-read`, `file-write`, `file-list`, `file-delete` — must access container filesystem
- `shell-exec` — must run in sandboxed container
- `spawn-subagent` — needs container context for the curl call

### What Becomes Remote

- `knowledge-store`, `knowledge-query` — needs DB + vector access on core
- `web-search`, `web-fetch` — could be local (via egress proxy) but safer remote for governance
- `schedule-task` — needs DB access on core
- `delegate-task` — already proxies to core
- All MCP tools — always routed through core (Story 7.3)

## Consequences

### Positive
- Agents gain access to all declared tools
- New tools added to SkillRegistry are immediately available without image rebuilds
- Core maintains governance (metering, audit, capability checks) over all tool execution
- Self-modifying SERA agents can add tools that are immediately usable

### Negative
- Every remote tool call adds an HTTP round-trip (~1ms on sera_net)
- Core becomes a bottleneck for tool execution
- SkillHandler refactor touches many skill implementations

### Risks
- Backward compatibility: existing agents with hardcoded tool assumptions may break
- Mitigation: BUILTIN_TOOLS kept as fallback until catalog is reliable

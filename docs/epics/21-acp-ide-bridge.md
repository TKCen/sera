# Epic 21: ACP / IDE Bridge

## Overview

A bi-directional bridge between SERA's agent team and developer IDEs (VS Code, JetBrains, Neovim) using the **Agent Communication Protocol (ACP)**. Developers can interact with SERA agents directly from their editor — routing tasks to the architect, developer, or QA agent, seeing agent thoughts in real time, and receiving code edits as workspace changes. ACP extends SERA's reach from dashboard-only into the developer's primary workflow surface.

## Context

- ACP is a stdio-based protocol (similar to LSP/MCP) that connects an IDE extension to sera-core
- Each IDE session maps to a SERA operator identity and can target specific agents or circles
- Agent thoughts, tool calls, and code edits stream back through the bridge in real time
- The bridge is an authenticated channel (Epic 18) — it reuses the same ingress/egress model as Discord or Slack
- Reference implementation: OpenClaw ACP (`src/acp/server.ts`, `src/acp/translator.ts`, `src/acp/session-mapper.ts`)

## Dependencies

- Epic 09 (Real-Time Messaging) — Centrifugo for streaming agent responses
- Epic 18 (Integration Channels) — ACP as a channel type with ingress/egress routing

---

## Stories

### Story 21.1: ACP stdio server

**As** sera-core
**I want** an ACP stdio server that IDE extensions can connect to
**So that** agents are accessible from any IDE with an ACP-compatible extension

**Acceptance Criteria:**
- [ ] `AcpServer` class in `core/src/acp/` — listens on stdio (stdin/stdout JSON-RPC)
- [ ] Implements ACP handshake: `initialize` → `initialized` with capability negotiation
- [ ] Server advertises capabilities: `agentRouting`, `streaming`, `codeEdits`, `thinking`
- [ ] Graceful shutdown on `shutdown` request or broken pipe
- [ ] Health check via `ping` → `pong` round-trip

### Story 21.2: Session mapping and authentication

**As** an operator using an IDE
**I want** my ACP session to authenticate with SERA and map to my identity
**So that** agent interactions respect my permissions and audit trail

**Acceptance Criteria:**
- [ ] `AcpSessionMapper` — maps each stdio connection to an operator identity
- [ ] Authentication via API key passed in `initialize` params or environment variable
- [ ] Session tracks: operator ID, active agent, working directory, IDE metadata (type, version)
- [ ] Session stored in `acp_sessions` table for audit and reconnection
- [ ] Multiple concurrent sessions supported (one per IDE window)

### Story 21.3: Agent routing from IDE

**As** a developer in VS Code
**I want** to route my message to a specific SERA agent (architect, developer, QA)
**So that** I get the right expertise for my current task

**Acceptance Criteria:**
- [ ] `acp/route` request with `agentId` or `agentName` parameter
- [ ] Falls back to circle routing if no agent specified (circle picks best responder)
- [ ] Agent list queryable via `acp/agents` request — returns available agents with status
- [ ] Routing creates a chat session (reuses Epic 09 chat flow internally)

### Story 21.4: Streaming thoughts and responses

**As** a developer
**I want** to see agent thinking, tool calls, and responses streaming in my IDE
**So that** I can follow the agent's reasoning without switching to the dashboard

**Acceptance Criteria:**
- [ ] ACP notifications: `agent/thinking` (reasoning tokens), `agent/toolCall` (tool name + args), `agent/response` (content chunks)
- [ ] Mapped from Centrifugo stream events to ACP notification format
- [ ] Thinking level configurable per session: `full`, `summary`, `none`
- [ ] Streaming cancellation via `acp/cancel` request

### Story 21.5: Code edit integration

**As** a developer
**I want** agent-produced code changes to appear as workspace edits in my IDE
**So that** I can review, accept, or reject them inline

**Acceptance Criteria:**
- [ ] `agent/codeEdit` notification with: file path, range, new content, description
- [ ] Edits delivered as workspace edit operations (not raw file writes)
- [ ] Agent can request file content via `workspace/readFile` request
- [ ] Agent can query project structure via `workspace/listFiles` request
- [ ] Working directory context sent with each session

### Story 21.6: Sub-agent spawning from IDE

**As** a developer
**I want** to spawn a sub-agent for a specific task from my IDE
**So that** I can delegate focused work (e.g., "write tests for this file") without leaving my editor

**Acceptance Criteria:**
- [ ] `acp/spawnAgent` request — creates a task-scoped agent instance
- [ ] Sub-agent inherits parent session's working directory and file context
- [ ] Sub-agent results stream back through the same ACP session
- [ ] Sub-agent lifecycle visible in IDE (running, completed, failed)

---

## DB Schema

```sql
-- Story 21.2: ACP session tracking
CREATE TABLE acp_sessions (
  id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  operator_id   uuid NOT NULL REFERENCES operators(id),
  agent_id      uuid REFERENCES agent_instances(id),
  ide_type      text NOT NULL,               -- 'vscode' | 'jetbrains' | 'neovim'
  ide_version   text,
  working_dir   text,
  status        text NOT NULL DEFAULT 'active',  -- 'active' | 'disconnected'
  connected_at  timestamptz NOT NULL DEFAULT now(),
  last_ping_at  timestamptz NOT NULL DEFAULT now(),
  disconnected_at timestamptz
);

CREATE INDEX idx_acp_sessions_operator ON acp_sessions(operator_id, status);
```

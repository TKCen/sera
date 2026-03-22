# Epic 03: Docker Sandbox & Agent Lifecycle

## Overview

Agents run in isolated Docker containers. sera-core holds the Docker socket exclusively — agents cannot spawn containers unless explicitly permitted by their tier. This epic covers container spawning from manifests, tier-based resource and network enforcement, workspace bind-mounts, git worktree isolation for coding agents, and the full container lifecycle from start to teardown.

## Context

- See `docs/ARCHITECTURE.md` → Docker Sandbox Model, Capability & Permission Model
- The Docker socket is a privileged resource; only sera-core's `SandboxManager` interacts with it
- Capabilities are resolved from three layers: SandboxBoundary (ceiling) → CapabilityPolicy → manifest inline overrides. Deny always wins.
- `sandboxBoundary: tier-1|tier-2|tier-3` are built-in boundary profiles; operators can define custom ones
- Git worktrees are the isolation model for concurrent coding agents
- `agent_net` Docker network is used for agent containers and MCP server containers

## Dependencies

- Epic 01 (Infrastructure Foundation) — `agent_net` network, Docker Compose base
- Epic 02 (Agent Manifest & Registry) — manifest schema, agent instance DB

---

## Stories

### Story 3.1: Container spawn from manifest

**As** sera-core
**I want** to spawn a Docker container for an agent from its manifest definition
**So that** agent code runs in an isolated environment with the correct configuration injected

**Acceptance Criteria:**
- [ ] `SandboxManager.spawn(manifest, instanceId)` creates and starts a Docker container
- [ ] Container uses the agent-runtime image (`sera-agent-worker:latest`)
- [ ] Container environment includes: `AGENT_NAME`, `AGENT_INSTANCE_ID`, `SERA_IDENTITY_TOKEN` (JWT), `SERA_CORE_URL`, `CENTRIFUGO_API_URL`, `CENTRIFUGO_API_KEY`
- [ ] Container labelled with: `sera.agent={name}`, `sera.instance={instanceId}`, `sera.tier={tier}`, `sera.circle={circle}`
- [ ] Container connected to `agent_net`
- [ ] Container name format: `sera-agent-{name}-{instanceId-short}`
- [ ] Container ID stored in `agent_instances.container_id` after spawn
- [ ] Spawn failures update agent status to `error` with error message

**Technical Notes:**
- Use `dockerode` for all Docker API interactions
- JWT issued by `IdentityService` with claims: `{ agentId, agentName, circleId, tier, scope: 'agent' }`
- JWT expiry configurable via `AGENT_JWT_EXPIRY_HOURS` env var (default: `24`). Heartbeat validates liveness, not the token — generous expiry avoids unnecessary token rotation during long-running tasks

---

### Story 3.2: Capability resolution engine

**As** sera-core
**I want** a capability resolution engine that computes the effective permission set for an agent before spawning its container
**So that** every spawned container has a precisely computed, auditable capability set regardless of how policies compose

**Acceptance Criteria:**
- [ ] `CapabilityResolver.resolve(manifest)` returns a fully materialised `ResolvedCapabilities` object
- [ ] Resolution steps, in order:
  1. Load `SandboxBoundary` referenced in `manifest.metadata.sandboxBoundary` — this is the ceiling
  2. Load `CapabilityPolicy` referenced in `manifest.policyRef` (if present) — intersect with boundary
  3. Apply manifest inline `capabilities` overrides — may only narrow, never broaden
  4. Apply all `always-denied` entries from NamedLists of type `command-denylist` or `network-denylist` marked `alwaysEnforced: true` — unconditional, final
- [ ] All `$ref` entries in allow/deny lists resolved recursively before intersection
- [ ] **Deny always beats Allow** at every step and every layer — no exceptions
- [ ] Inline manifest override that attempts to broaden beyond the resolved policy raises a `CapabilityEscalationError` — agent does not start
- [ ] `resolved_capabilities` stored as JSONB on `agent_instances` at spawn time — immutable record of what the container was permitted
- [ ] `POST /api/agents/:id/resolve-capabilities` dry-runs resolution without spawning — used by UI and CLI for inspection
- [ ] Unit tests covering:
  - Boundary alone (no policy, no inline)
  - Policy intersection with boundary — boundary wins on conflict
  - Inline narrowing accepted
  - Inline broadening rejected
  - `$ref` resolution in network allowlist
  - `$ref` composition (list referencing a list)
  - Circular `$ref` detected and rejected
  - `alwaysEnforced` deny list overrides an explicit allow at policy level
  - Unknown boundary name → startup error

**Technical Notes:**
- `SandboxManager.buildContainerConfig(resolvedCapabilities)` translates the resolved set into Docker API parameters
- Network resolution → Docker network configuration: empty allow list = `--network none`; specific hosts = custom bridge + iptables egress rules; `allow: ["*"]` = standard bridge with no egress restriction
- Linux capabilities: start from `cap-drop ALL`, add only what `linux.capabilities` in the resolved set specifies
- Command allow/deny enforcement is runtime (RuntimeToolExecutor checks patterns before exec), not container-level — the container config enforces the structural boundary (network, filesystem mount mode, Linux caps), the runtime enforces the behavioural boundary (which commands)

---

### Story 3.2b: Runtime command enforcement

**As** the agent runtime
**I want** shell command requests checked against the resolved allow/deny pattern lists before execution
**So that** an agent cannot run commands outside its declared capability set even if shell access is granted

**Acceptance Criteria:**
- [ ] `RuntimeToolExecutor` receives the resolved `exec.commands` allow/deny lists at startup (injected via env or mounted config)
- [ ] Before executing any `shell-exec` call: match the command string against deny patterns first, then allow patterns
- [ ] Pattern matching: glob-style (`git *` matches `git status`, `git commit -m "foo"`)
- [ ] Deny match → return tool error `{ error: 'command_denied', command: '...', matchedPattern: '...' }` — no execution
- [ ] No allow match → return tool error `{ error: 'command_not_permitted', command: '...' }` — no execution
- [ ] Allow match with no deny match → execute
- [ ] `shell: false` in resolved capabilities → `shell-exec` tool not registered at all (not just rejected at runtime)
- [ ] Every allow/deny decision logged: command, matched pattern, decision, timestamp
- [ ] Deny decisions recorded in audit trail via heartbeat or direct API call

---

### Story 3.3: Workspace bind-mount management

**As** sera-core
**I want** each agent container to receive a bind-mounted workspace directory
**So that** agents can read and write files within their authorised scope

**Acceptance Criteria:**
- [ ] Workspace path resolved from manifest `workspace.path` (default: `/workspaces/{agent-name}`)
- [ ] Workspace directory created on host if it does not exist before container spawn
- [ ] Resolved `filesystem.write: false` → workspace mounted read-only (`:ro`)
- [ ] Resolved `filesystem.write: true` → workspace mounted read-write (`:rw`)
- [ ] Container internal path: `/workspace` (consistent across all agents)
- [ ] Memory directory mounted at `/memory` from manifest `memory.personalMemory` path
- [ ] Shared knowledge directory mounted read-only at `/knowledge` if `memory.sharedKnowledge` is set
- [ ] `SandboxManager.teardown(instanceId)` does NOT delete workspace contents — data persists after container removal

---

### Story 3.4: Git worktree isolation for coding agents

**As** sera-core
**I want** to create a git worktree for each coding agent task and bind-mount it as the agent's workspace
**So that** multiple coding agents can work on the same repository concurrently without interfering with each other

**Acceptance Criteria:**
- [ ] `WorktreeManager.create(repoPath, agentName, taskId)` runs `git worktree add .worktrees/{agentName}-{taskId} -b agent/{agentName}/{taskId}`
- [ ] Created worktree path bind-mounted into agent container instead of the base workspace
- [ ] `WorktreeManager.remove(agentName, taskId)` runs `git worktree remove .worktrees/{agentName}-{taskId}` after agent completes
- [ ] `WorktreeManager.diff(agentName, taskId)` returns the diff between worktree branch and base branch
- [ ] Worktrees listed and associated with their agent instance in `GET /api/agents/:id`
- [ ] Worktree creation failure (e.g. path not a git repo) falls back to plain workspace bind-mount with a warning
- [ ] API: `POST /api/agents/:id/worktree/merge` triggers merge of agent branch into target branch
- [ ] API: `DELETE /api/agents/:id/worktree` discards worktree without merging

**Technical Notes:**
- Worktree is a git concept; the workspace directory passed to `SandboxManager` is the worktree path, not the repo root
- The agent container only sees its worktree — it has no visibility into other agents' worktrees or the repo root
- Worktrees share the git object store; no data duplication
- `taskId` is the UUID from the `task_queue` record (Story 5.8). For ad-hoc tasks without a queue entry (e.g. direct chat), `taskId` is generated by sera-core at dispatch time (`crypto.randomUUID()`) and passed to the agent via stdin

---

### Story 3.5: Container lifecycle events and status tracking

**As an** operator
**I want** real-time visibility into container state changes
**So that** I know when agents start, stop, crash, or get stuck

**Acceptance Criteria:**
- [ ] sera-core listens to Docker events stream (`dockerode.getEvents`) filtered to `sera.agent` labelled containers
- [ ] Docker events (`start`, `stop`, `die`, `oom`) update `agent_instances.status` accordingly
- [ ] OOM kill detected and status set to `error` with reason `oom-killed`
- [ ] Container exit code recorded on stop/die
- [ ] Status change events published to Centrifugo `system.agents` channel for real-time UI updates
- [ ] Dangling containers (labelled `sera.agent` but no matching DB record) logged as warnings on startup
- [ ] `GET /api/agents/:id/logs` proxies Docker container logs with `tail` and `follow` query params

---

### Story 3.6: Agent heartbeat and liveness

**As** sera-core
**I want** agent containers to send periodic heartbeats
**So that** I can detect stuck or crashed agents that Docker events alone may not catch

**Acceptance Criteria:**
- [ ] `POST /api/agents/:id/heartbeat` endpoint accepts heartbeat from agent runtime
- [ ] Endpoint validates JWT — `agentId` in token must match URL `:id`
- [ ] Heartbeat updates `agent_instances.last_heartbeat_at` timestamp
- [ ] sera-core background job (pg-boss, every 30s) checks for agents with `status: running` and `last_heartbeat_at` older than configurable threshold (`AGENT_HEARTBEAT_TIMEOUT_MS`, default: 120s)
- [ ] Agents exceeding threshold: status transitions to `unresponsive`; event published to `agent:{agentId}:status` and `system.agents` channels
- [ ] Agents `unresponsive` for a second threshold (`AGENT_HEARTBEAT_KILL_MS`, default: 300s): sera-core force-stops the container via `dockerode`; status transitions to `error` with reason `heartbeat_timeout`
- [ ] Heartbeat resume (agent sends heartbeat while `unresponsive`): status transitions back to `running`; recovery event published
- [ ] Heartbeat interval configurable via `AGENT_HEARTBEAT_INTERVAL_MS` (default: 30s) — injected into container env

---

### Story 3.7: Container cleanup and resource reclamation

**As an** operator
**I want** stopped and errored agent containers cleaned up automatically
**So that** Docker resources don't accumulate indefinitely on the host

**Acceptance Criteria:**
- [ ] Ephemeral agents (`lifecycle.mode: ephemeral`): `AutoRemove: true` set on container; DB record deleted after removal
- [ ] Background job periodically removes containers with `status: stopped | error` older than configurable retention period (default: 1h)
- [ ] `POST /api/agents/:id/cleanup` manually triggers cleanup for a specific agent
- [ ] Cleanup removes the container but preserves workspace files and DB record
- [ ] Cleanup events logged and published to Centrifugo

---

### Story 3.8: Ephemeral vs persistent agent lifecycle enforcement

**As** sera-core
**I want** lifecycle mode enforced as a hard constraint at spawn and capability resolution time
**So that** ephemeral agents cannot escalate to persistent scope even if misconfigured

**Acceptance Criteria:**
- [ ] `lifecycle.mode` resolved from template `spec.lifecycle.mode`; instance `overrides.lifecycle` can set `ephemeral` but cannot change `ephemeral` to `persistent` (only operators can instantiate persistent agents via `POST /api/templates/:name/instantiate`)
- [ ] Ephemeral agent spawned by a parent: resolved capabilities are the **intersection** of parent's resolved capabilities and the subagent template's spec — child can never exceed parent
- [ ] Hard guard: ephemeral agent with `seraManagement.agents.create` can only create other ephemeral agents; `persistent` creation requires `lifecycle.mode: persistent` on the acting agent — enforced in sera-core MCP server handler, not just in the capability model
- [ ] `parent_instance_id` set on all subagent instances — lineage is always traceable
- [ ] `GET /api/agents/:id/subagents` returns active subagent tree with full lineage
- [ ] Subagent spawn attempt that would exceed parent capabilities returns `CapabilityEscalationError` — logged to audit trail

---

### Story 3.9: Permission request service (human-in-the-loop grants)

**As an** agent
**I want** to request runtime access to resources outside my current capability set
**So that** I can ask the operator for permission to access a new path or host rather than silently failing

**Acceptance Criteria:**
- [ ] `PermissionRequestService` in sera-core handles inbound requests from agents via `POST /api/agents/:id/permission-request`
- [ ] Request payload: `{ dimension: 'filesystem'|'network'|'exec.commands', value: string, reason?: string }`
- [ ] Request authenticated by agent JWT — agent cannot request on behalf of another agent
- [ ] Request published to Centrifugo `system.permission-requests` channel: `{ requestId, agentId, agentName, dimension, value, reason, requestedAt }`
- [ ] Request held in memory pending operator decision — agent call blocks with configurable timeout (default: 5 min)
- [ ] `POST /api/permission-requests/:requestId/decision` accepts: `{ decision: 'grant'|'deny', grantType?: 'one-time'|'session'|'persistent', expiresAt?: ISO8601 }`
- [ ] On **grant, one-time**: decision returned to waiting agent; nothing stored
- [ ] On **grant, session**: added to in-memory session grant map keyed by `agent_instance_id`; lost on container stop
- [ ] On **grant, persistent**: inserted to `capability_grants` table; decision returned; applied at next spawn
- [ ] On **deny**: `{ granted: false, reason? }` returned; agent handles gracefully (does not crash)
- [ ] On timeout: auto-deny; operator notified via Centrifugo that request expired
- [ ] All decisions (grant and deny) recorded in audit trail: dimension, value, grant type, operator identity, timestamp
- [ ] `GET /api/permission-requests` lists pending requests — filterable by agent
- [ ] `GET /api/agents/:id/grants` lists active grants (session + persistent) for an agent

---

### Story 3.10: Dynamic filesystem access (proxy + bind mount grants)

**As an** operator
**I want** to grant an agent access to a filesystem path at runtime — with or without a container restart
**So that** I can say "go work on this folder" in a conversation without pre-configuring bind mounts

**Acceptance Criteria:**
- [ ] For `one-time` and `session` filesystem grants: sera-core acts as a **host-side file proxy**
  - Agent's `RuntimeToolExecutor` detects that the target path is outside `/workspace` but covered by a grant → forwards the tool call to sera-core via `POST /v1/tools/proxy` with `{ tool, args, grantId }`
  - sera-core validates the grant, reads/writes the path on the host filesystem, and returns the result
  - The container never needs a bind mount for these operations — no restart required
  - `LLMClient`-style HTTP call from agent to core — reuses the existing JWT auth and `SERA_CORE_URL`
- [ ] For `persistent` filesystem grants:
  - Path added to agent instance's `overrides.capabilities.filesystem.scope` in DB
  - Also inserted to `capability_grants` table
  - Applied as a bind mount on next container start
  - sera-core offers the operator an inline option: "Persist grant and restart container now?" — `POST /api/agents/:id/restart?applyPendingGrants=true`
- [ ] `POST /api/agents/:id/restart` stops the container and starts it again with the current resolved capabilities (including any new persistent grants) — only valid for persistent lifecycle agents
- [ ] Shell access to a dynamically granted path (for `shell-exec` commands referencing that path) requires a persistent grant + restart — the tool executor returns `{ error: 'path_requires_restart', hint: 'grant is session-scoped; restart required for shell access' }` if shell access is attempted against a session-only grant
- [ ] Path canonicalised before grant: symlinks resolved, `..` collapsed — grants cannot escape via path traversal
- [ ] Revoke grant: `DELETE /api/agents/:id/grants/:grantId` — session grants removed from memory immediately, persistent grants set `revoked_at` and excluded from next spawn

---

### Story 3.11: Subagent recursion depth guard

**As** sera-core
**I want** a hard limit on subagent spawning depth
**So that** a misbehaving or looping agent cannot exhaust system resources by recursively spawning sub-subagents

**Acceptance Criteria:**
- [ ] `SUBAGENT_MAX_DEPTH` env var (default: `5`) defines the maximum allowed agent lineage depth
- [ ] Depth calculated at spawn time by traversing `parent_instance_id` chain in `agent_instances` — depth 0 = directly spawned by an operator or schedule, depth N = spawned by an agent at depth N-1
- [ ] Spawn attempt that would exceed `SUBAGENT_MAX_DEPTH` returns a `RecursionLimitError` to the calling agent's MCP tool call — agent does not crash, but receives `{ error: 'recursion_limit_exceeded', currentDepth: N, maxDepth: M }`
- [ ] `RecursionLimitError` recorded in audit trail: `{ action: 'agent.spawn.denied', reason: 'recursion_limit', callerAgentId, depth: N }`
- [ ] Depth visible in `GET /api/agents/:id` response as `lineageDepth: N`
- [ ] `GET /api/agents/:id/subagents` includes depth annotation for each node in the tree
- [ ] Limit is a hard system-level ceiling — individual agents cannot override it via `seraManagement` capabilities
- [ ] Operator can increase limit by changing `SUBAGENT_MAX_DEPTH` — not agent-configurable at runtime

---

### Story 3.12: Workspace disk quotas

**As an** operator
**I want** per-agent workspace disk usage limits enforced
**So that** a runaway agent or a large build artifact cannot fill the host disk and destabilise the system

**Acceptance Criteria:**
- [ ] `maxWorkspaceSizeGB` added to the capability model (`filesystem` dimension); resolvable via CapabilityPolicy or manifest inline override
- [ ] Background job (pg-boss, cron `*/15 * * * *`): checks actual disk usage of each active agent's workspace directory via `du -s`
- [ ] Agent exceeding `maxWorkspaceSizeGB`: status transitions to `throttled`; `file-write` and `shell-exec` tool calls that would write files return `{ error: 'workspace_quota_exceeded', usedGB, limitGB }` — reads still permitted
- [ ] `throttled` state published to `agent:{agentId}:status` Centrifugo channel and to alert rules (Epic 18)
- [ ] Agent transitions back from `throttled` to `running` automatically when usage drops below the limit (e.g. agent cleans up temp files)
- [ ] `GET /api/agents/:id` includes `workspaceUsageGB` and `workspaceLimitGB` fields (null if no limit set)
- [ ] Quota check is advisory for `ephemeral` agents — they clean up on exit; enforce strictly for `persistent` agents
- [ ] No default limit — `maxWorkspaceSizeGB` must be explicitly set in a CapabilityPolicy or manifest; agents with no limit configured log a startup warning if they have `filesystem.write: true`

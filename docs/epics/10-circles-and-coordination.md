# Epic 10: Circles & Multi-Agent Coordination

## Overview

Circles are named groups of agents that share a context, a communication channel, and optionally a pooled resource budget. They are the primary organisational unit above the individual agent. Beyond grouping, SERA supports multi-agent coordination patterns: sequential pipelines, parallel fan-out, hierarchical delegation, and party mode (structured all-agent discussion). These patterns make SERA a genuine multi-agent platform, not just a collection of isolated agents.

## Context

- See `docs/ARCHITECTURE.md` → Real-Time Messaging (circle channels), Open Source Ecosystem
- Circle membership is declared in agent manifests (`metadata.circle`, `metadata.additionalCircles`)
- Circle channels (`circle:{circleId}`) are the broadcast bus for intra-circle communication
- Orchestrator patterns (sequential/parallel/hierarchical/flow) are how multi-step agent workflows are encoded
- Party mode: a facilitated discussion where multiple agents respond to a shared prompt

## Dependencies

- Epic 02 (Agent Manifest) — circle membership in manifest
- Epic 03 (Docker Sandbox) — agent lifecycle
- Epic 09 (Real-Time Messaging) — circle broadcast channels

---

## Stories

### Story 10.1: Circle management API

**As an** operator
**I want** to create, update, and delete circles via the API
**So that** I can organise agents into logical teams

**Acceptance Criteria:**
- [ ] `circles` table: `id` (UUID), `name` (slug), `display_name`, `description`, `constitution` (text), `created_at`
- [ ] `GET /api/circles` lists all circles
- [ ] `POST /api/circles` creates a circle: `{ name, displayName, description?, constitution? }`
- [ ] `GET /api/circles/:id` returns circle details including member agent list
- [ ] `PUT /api/circles/:id` updates display name, description, or constitution
- [ ] `DELETE /api/circles/:id` deletes a circle — requires no active agent members (returns 409 if members exist)
- [ ] `GET /api/circles/:id/members` returns agents whose manifest declares this circle

**Technical Notes:**
- Circle membership is derived from agent manifests — it is not stored separately in the DB
- The `constitution` field is a free-text governance document describing the circle's purpose, norms, and decision rules — injected into member agents' context

---

### Story 10.2: Circle constitution injection

**As** an agent in a circle
**I want** my circle's constitution injected into my context at startup
**So that** I understand my circle's shared norms, goals, and communication expectations

**Acceptance Criteria:**
- [ ] If agent manifest declares a circle with a constitution, the constitution text is appended to the system prompt as a `<circle-constitution>` block
- [ ] Constitution injected after individual skills but before task context
- [ ] If circle has no constitution, nothing injected — no placeholder text
- [ ] Constitution changes (via `PUT /api/circles/:id`) take effect for agents started after the update
- [ ] Constitution token size logged; warn if > 2000 tokens

---

### Story 10.3: Sequential orchestration pattern

**As an** operator or orchestrator agent
**I want** to run a pipeline of agents sequentially, passing output from one to the next
**So that** complex multi-step tasks are decomposed and delegated automatically

**Acceptance Criteria:**
- [ ] `ProcessManager.sequential(steps: AgentTask[])` runs agents one after another
- [ ] Each step's output passed as input context to the next step
- [ ] If a step fails, the pipeline halts with the error and partial results
- [ ] Pipeline state persisted to DB: `{ pipelineId, steps: [{agentId, status, result}], createdAt, completedAt }`
- [ ] `POST /api/pipelines` creates and starts a sequential pipeline
- [ ] `GET /api/pipelines/:id` returns current pipeline state
- [ ] Pipeline progress published to `system.agents` channel as each step completes

---

### Story 10.4: Parallel fan-out pattern

**As an** operator or orchestrator agent
**I want** to fan a task out to multiple agents simultaneously and collect their results
**So that** independent subtasks complete faster via parallelism

**Acceptance Criteria:**
- [ ] `ProcessManager.parallel(tasks: AgentTask[])` spawns all agents simultaneously
- [ ] Waits for all agents to complete (or timeout)
- [ ] Returns array of results, matched to input tasks
- [ ] Partial failures: individual agent failures included as error results, not pipeline abort
- [ ] Configurable timeout per parallel run (default: 10min)
- [ ] `POST /api/pipelines` with `type: 'parallel'` supports fan-out
- [ ] Results aggregated and returned when all complete (or timeout reached)

---

### Story 10.5: Hierarchical delegation (subagent spawning)

**As an** orchestrator agent
**I want** to spawn subagents to handle delegated subtasks during my own reasoning
**So that** I can decompose complex problems without a human in the loop

**Acceptance Criteria:**
- [ ] `spawn-subagent` tool available to agents with `permissions.canSpawnSubagents: true`
- [ ] Tool args: `{ role, task, maxInstances? }` — `role` matched against manifest `subagents.allowed` list
- [ ] Spawn count validated against `subagents.allowed[role].maxInstances`
- [ ] Subagent container spawned, task injected, result returned to parent as tool result
- [ ] `requiresApproval: true` in manifest: spawn request held, operator notified, waits for `POST /api/agents/:id/approve-spawn`
- [ ] Subagent hierarchy tracked in DB: parent `instanceId` stored on child instance
- [ ] `GET /api/agents/:id/subagents` returns active subagents spawned by this agent

---

### Story 10.6: Party mode (structured multi-agent discussion)

**As an** operator
**I want** to start a facilitated discussion where multiple agents respond to a shared prompt in turn
**So that** I get diverse perspectives from a team of specialised agents on a complex question

**Acceptance Criteria:**
- [ ] `POST /api/circles/:id/party` starts a party mode session: `{ prompt, participantAgentIds, rounds? }`
- [ ] Each participant agent receives the prompt + all previous responses in sequence
- [ ] Agents respond in declared order (deterministic, not race-based)
- [ ] Each response published to `circle:{circleId}` channel in real time as it's generated
- [ ] Default: 1 round (each agent responds once); configurable up to 3 rounds
- [ ] Session record stored: `{ sessionId, circleId, prompt, rounds: [{ agentId, response, timestamp }] }`
- [ ] `GET /api/circles/:id/party/:sessionId` returns full session transcript
- [ ] Session concludes with an optional synthesis step (designated agent summarises the discussion)

---

## DB Schema

```sql
-- Story 10.1: Circle definitions
CREATE TABLE circles (
  id              UUID PRIMARY KEY,
  name            TEXT NOT NULL UNIQUE,
  display_name    TEXT NOT NULL,
  description     TEXT,
  constitution    TEXT,
  created_at      TIMESTAMPTZ DEFAULT now(),
  updated_at      TIMESTAMPTZ DEFAULT now()
);
CREATE INDEX circles_name_idx ON circles (name);

-- agent_instances gains a circle_id FK:
ALTER TABLE agent_instances ADD COLUMN circle_id UUID REFERENCES circles ON DELETE SET NULL;

-- Stories 10.3/10.4: Pipeline state for sequential and parallel orchestration
CREATE TABLE pipelines (
  id              UUID PRIMARY KEY,
  type            TEXT NOT NULL,           -- 'sequential' | 'parallel'
  status          TEXT NOT NULL DEFAULT 'pending',
  steps           JSONB NOT NULL DEFAULT '[]',
  created_at      TIMESTAMPTZ DEFAULT now(),
  completed_at    TIMESTAMPTZ
);

-- Story 10.6: Party mode sessions
CREATE TABLE party_sessions (
  id              UUID PRIMARY KEY,
  circle_id       UUID NOT NULL REFERENCES circles ON DELETE CASCADE,
  prompt          TEXT NOT NULL,
  rounds          JSONB NOT NULL DEFAULT '[]',
  created_at      TIMESTAMPTZ DEFAULT now(),
  completed_at    TIMESTAMPTZ
);
CREATE INDEX party_sessions_circle_idx ON party_sessions (circle_id);
```

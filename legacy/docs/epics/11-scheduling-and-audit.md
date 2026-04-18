# Epic 11: Scheduling & Audit Trail

## Overview

Two cross-cutting concerns that every production agentic system needs: scheduled task execution and a tamper-evident audit trail. Scheduling enables autonomous agents to act on time-based triggers without human initiation. The audit trail provides the full, immutable record of every agent action — essential for debugging, compliance, and building operator trust in autonomous systems.

## Context

- See `docs/ARCHITECTURE.md` → Key Architectural Decisions (Audit trail: Merkle hash-chain)
- Schedules are agent-centric: a schedule always belongs to an agent and results in that agent executing a task
- The audit trail uses a Merkle hash-chain in PostgreSQL — each record chains to the previous, making tampering detectable
- Both scheduling and auditing are core governance concerns: they belong in sera-core, not in individual agents

## Dependencies

- Epic 02 (Agent Manifest) — agent identity
- Epic 03 (Docker Sandbox) — agent container spawn (triggered by schedules)
- Epic 04 (LLM Proxy) — audit records LLM proxy calls

---

## Stories

### Story 11.1: Schedule data model and CRUD

**As an** operator
**I want** to create, update, and delete schedules for agents
**So that** I can configure autonomous time-based agent tasks

**Acceptance Criteria:**
- [ ] `schedules` table: `id` (UUID), `agent_id`, `agent_name`, `name`, `description`, `type` (`cron | once`), `expression` (cron string or ISO8601 datetime), `task` (text prompt), `status` (`active | paused | completed | error`), `last_run_at`, `next_run_at`, `last_run_status`, `created_at`, `updated_at`
- [ ] `POST /api/schedules` creates a schedule
- [ ] `GET /api/schedules` lists all schedules with optional `?agentId=` filter
- [ ] `GET /api/agents/:id/schedules` returns schedules for a specific agent
- [ ] `PUT /api/schedules/:id` updates schedule (name, expression, task, status)
- [ ] `DELETE /api/schedules/:id` deletes a schedule
- [ ] Cron expressions validated on create/update — invalid expression returns 400 with human-readable error
- [ ] `next_run_at` computed and stored on create/update

---

### Story 11.2: Schedule execution engine

**As** sera-core
**I want** a reliable schedule execution engine that fires agents at the right time
**So that** scheduled tasks run even when no human is watching

**Acceptance Criteria:**
- [ ] Background scheduler polls `schedules` table every 30s for due schedules (`next_run_at <= now AND status = 'active'`)
- [ ] On trigger: spawns agent container with `task` as the initial prompt
- [ ] `last_run_at` and `last_run_status` updated after execution completes
- [ ] `next_run_at` recomputed after each run for cron schedules
- [ ] One-shot schedules (`type: once`): set `status: completed` after successful run
- [ ] Missed schedules (sera-core was down when run was due): logged as missed, not retroactively executed — next scheduled time used
- [ ] Concurrent execution guard: if an agent is already running from a previous schedule trigger, skip this fire and log a warning
- [ ] `POST /api/schedules/:id/trigger` manually triggers a schedule immediately (for testing)

---

### Story 11.3: Schedule management in agent manifest

**As an** agent developer
**I want** to define default schedules in an agent's manifest
**So that** deploying an agent also sets up its recurring tasks without manual configuration

**Acceptance Criteria:**
- [ ] `schedules` block in AGENT.yaml:
  ```yaml
  schedules:
    - name: daily-summary
      type: cron
      expression: "0 8 * * *"
      task: "Generate a daily summary of all completed tasks from yesterday"
    - name: health-check
      type: cron
      expression: "*/15 * * * *"
      task: "Run a quick system health check and report any anomalies"
  ```
- [ ] On agent registration, manifest schedules created in DB if no schedule with the same name exists for that agent
- [ ] Manifest schedules do not overwrite operator-modified schedules (idempotent create, not overwrite)
- [ ] Manifest schedule removal does NOT auto-delete existing DB schedules (operator must delete explicitly)

---

### Story 11.4: Audit trail — Merkle hash-chain

**As an** operator
**I want** every significant system action recorded in a tamper-evident audit log
**So that** I can verify the integrity of the audit record and trust it for debugging and compliance

**Acceptance Criteria:**
- [ ] `audit_trail` table: `id` (UUID), `sequence` (monotonic integer), `timestamp`, `actor_type` (`agent | operator | system`), `actor_id`, `action` (string), `resource_type`, `resource_id`, `payload` (JSONB), `prev_hash` (SHA-256 of previous record), `hash` (SHA-256 of this record's content + prev_hash)
- [ ] Every insert computes `hash` from: `sequence + timestamp + actorId + action + resourceId + JSON.stringify(payload) + prevHash`
- [ ] First record: `prev_hash = '0000...0000'` (genesis)
- [ ] `AuditService.record(entry)` is the only write path — no direct table inserts elsewhere
- [ ] `AuditService.verify(fromSequence?, toSequence?)` validates the hash chain — returns `{ valid: boolean, brokenAt?: sequence }`
- [ ] `GET /api/audit?actorId=&action=&from=&to=&limit=` queries audit records
- [ ] `GET /api/audit/verify` runs chain verification and returns result

---

### Story 11.5: Audit event coverage

**As an** operator
**I want** all significant actions to generate audit events automatically
**So that** the audit log is comprehensive without requiring individual callsites to remember

**Acceptance Criteria:**
- [ ] Audit events generated for:
  - Agent created, started, stopped, errored
  - LLM proxy call (actor: agent, action: `llm.call`, payload: model + token counts, not content)
  - Tool execution (actor: agent, action: `tool.execute`, payload: tool name + sanitised args)
  - Memory write (actor: agent, action: `memory.write`, payload: block ID + type)
  - Schedule created, modified, triggered, completed
  - Provider added, removed
  - MCP server registered, unregistered
  - Operator API calls that modify state (all POST/PUT/DELETE)
- [ ] LLM call content (actual messages) NOT stored in audit — only metadata
- [ ] Tool arguments sanitised before audit storage — values matching common secret patterns (API keys, passwords) replaced with `[REDACTED]`

---

### Story 11.6: Audit log export

**As an** operator
**I want** to export the audit log in standard formats
**So that** I can archive it, analyse it externally, or feed it into compliance tooling

**Acceptance Criteria:**
- [ ] `GET /api/audit/export?format=json&from=&to=` exports filtered audit records as JSON array
- [ ] `GET /api/audit/export?format=csv` exports as CSV with headers
- [ ] Export includes the `hash` and `prev_hash` fields so chain integrity can be verified offline
- [ ] Exports streamed (not buffered in memory) for large time ranges
- [ ] Export action itself recorded in audit trail

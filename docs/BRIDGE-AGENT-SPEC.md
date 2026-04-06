# Bridge Agent Specification

Poll-based integration contract for host-native CLI tool bridges.

## 1. Overview

SERA uses poll-based bridges (Option D) to integrate host-native CLI tools — claude/OMC, opencode/OMO, codex/OMX, and gemini — without requiring them to run inside Docker containers. Each bridge is a long-running host process that registers as a persistent agent instance and polls sera-core's task queue for work.

This is the same protocol used by BYOH (Bring Your Own Host) containers: register once, poll `/tasks/next`, report results via `/tasks/:taskId/complete`. No inbound connections are required from sera-core to the bridge.

## 2. Bridge Registration

Bridges register as agent instances on startup via:

```
POST /api/agents/instances
Authorization: Bearer <api-key>
Content-Type: application/json

{
  "templateRef": "tool-bridge",
  "name": "omc-bridge",
  "displayName": "OMC Bridge (host)",
  "lifecycleMode": "persistent",
  "start": false
}
```

**Critical constraints:**

- `start: false` — omitting this or setting it to `true` causes sera to attempt a Docker container spawn, which will fail for host-native bridges.
- `lifecycleMode: "persistent"` — the task queue rejects ephemeral agents with HTTP 405. All bridge instances must be persistent.
- `name` must be unique. On restart, bridges should detect the existing instance by name (HTTP 409 on duplicate) and reuse the returned `id` rather than creating a new registration.

**Response (201):**

```json
{
  "id": "<agent_instance_id>",
  "name": "omc-bridge",
  "lifecycle_mode": "persistent",
  "status": "stopped"
}
```

Store `id` — all subsequent API calls use it as `:id`.

**Idempotent startup pattern:**

```
POST /api/agents/instances  →  201 (new) or 409 (exists)
If 409: GET /api/agents/instances?name=omc-bridge  →  extract id
```

## 3. Poll-Based Task Contract

Bridges implement a simple three-operation loop.

### 3.1 Poll for next task

```
GET /api/agents/:id/tasks/next
Authorization: Bearer <api-key>
```

- **204 No Content** — queue is empty; wait and retry.
- **200 OK** — task dispatched; body contains the task payload (see §4).
- **409 Conflict** — a task is already running for this agent (`runningTaskId` in body); wait for it to complete before polling again.
- **405 Method Not Allowed** — agent is ephemeral; registration was wrong (fix `lifecycleMode`).

The server atomically transitions the task from `queued` → `running` using `SELECT ... FOR UPDATE SKIP LOCKED`, preventing duplicate dispatch.

### 3.2 Complete a task

```
POST /api/agents/:id/tasks/:taskId/complete
Authorization: Bearer <api-key>
Content-Type: application/json

{
  "result": "<output string>",
  "exitReason": "success",
  "usage": {
    "promptTokens": 1234,
    "completionTokens": 567,
    "totalTokens": 1801
  },
  "thoughtStream": []
}
```

### 3.3 Fail a task

Report failure through the same complete endpoint with an `error` field and no `result`:

```json
{
  "error": "CLI tool exited with code 1: <message>",
  "exitReason": "error"
}
```

If `retry_count < max_retries`, sera-core automatically re-queues the task with exponential backoff (`2^n` seconds). After exhausting retries the task is dead-lettered and a `system.task-dead-lettered` event is published to the Centrifugo `system` channel.

### 3.4 Poll interval and backoff

- Default poll interval: **2–5 seconds** (configurable per bridge).
- On 5+ consecutive non-task responses (errors or 500s): double the interval, cap at 30 seconds.
- Reset to default interval on any successful 200 or 204.
- Do not apply backoff on 204 — empty queue is normal.

## 4. Task Assignment Payload

The `GET /tasks/next` 200 response:

```json
{
  "taskId": "550e8400-e29b-41d4-a716-446655440000",
  "task": "<prompt string>",
  "context": {
    "tool": "omc",
    "repo": "/home/user/projects/myrepo",
    "branch": "feat/my-feature",
    "files": ["src/foo.ts", "src/bar.ts"],
    "delegation": {
      "fromInstanceId": "<parent-agent-id>"
    }
  },
  "priority": 100,
  "retryCount": 0,
  "maxRetries": 3
}
```

Field notes:

- `task` — the prompt string to pass to the CLI tool.
- `context` — arbitrary JSON set by the enqueuing caller; bridges should treat unknown fields as opaque pass-through.
- `priority` — lower number = higher priority (default 100).
- `context.delegation.fromInstanceId` — present when a SERA agent delegated the task; sera-core notifies the parent agent on completion automatically.

## 5. Authentication

| Environment | Header value |
|-------------|-------------|
| Development | `Authorization: Bearer sera_bootstrap_dev_123` |
| Production  | `Authorization: Bearer <per-bridge-api-key>` |

Generate per-bridge API keys via `POST /api/auth/api-keys` (requires admin scope). Store keys in the host environment — never hard-code them in bridge source.

## 6. Idempotency

Before invoking the CLI tool, bridges must check whether `taskId` has already been processed:

1. Maintain a local processed-task log (file or in-memory ring buffer, last 1000 entries).
2. If `taskId` is in the log, call complete immediately with the cached result (or a duplicate-detected error if result was not stored).
3. This guards against the poll racing with a slow network ack — `/tasks/next` uses `FOR UPDATE SKIP LOCKED` but network retries can re-deliver a 200.

## 7. Workspace Isolation

Each task runs in an isolated workspace to avoid cross-task git contamination:

1. Resolve `context.repo` to a working directory.
2. Create a task-scoped branch or worktree:
   ```bash
   git worktree add /tmp/bridge-tasks/<taskId> -b bridge/<taskId>
   ```
3. Invoke the CLI tool inside that worktree path.
4. On completion or failure, remove the worktree:
   ```bash
   git worktree remove /tmp/bridge-tasks/<taskId> --force
   git branch -D bridge/<taskId>
   ```

If `context.repo` is absent, run the CLI tool in a scratch directory under `/tmp/bridge-tasks/<taskId>/`.

## 8. Concurrency Model

Bridges execute **one task at a time**:

1. Poll `/tasks/next`.
2. On 200: execute task synchronously (blocking the poll loop).
3. Call `/tasks/:taskId/complete` (success or failure).
4. Return to step 1.

Do not poll for the next task while one is running. sera-core enforces this server-side (returns 409 if a running task exists), but bridges should self-enforce to avoid unnecessary HTTP round-trips.

## 9. Health and Lifecycle

**Stale detection:** sera-core's `/api/agents/:id/tasks/clear-stale` endpoint marks running tasks as failed if `started_at` is older than a configurable timeout (default 30 minutes). Operators or monitoring jobs call this endpoint — bridges do not need to implement their own timeout enforcement, but should complete tasks within the deadline.

**Graceful shutdown:**

1. On SIGTERM/SIGINT, set a shutdown flag.
2. If a task is in progress, allow it to complete and call `/tasks/:taskId/complete`.
3. Exit after completion. Do not deregister the instance — the registration persists so the bridge can reconnect without creating a duplicate.

**Reconnection:** If the bridge process restarts, it re-registers by name (detecting the 409 conflict), reuses the existing `id`, and resumes polling. Running tasks from the previous process that never received a complete call will eventually be cleared by the stale-task cleanup.

## 10. Error Reference

| Status | Meaning |
|--------|---------|
| 204 | Queue empty — poll again after interval |
| 400 | Malformed request body |
| 404 | Agent instance not found — re-register |
| 405 | Agent is ephemeral — fix `lifecycleMode` in registration |
| 409 (poll) | Task already running for this agent |
| 409 (complete) | Task not in `running` state (already completed or cancelled) |
| 500 | Server error — apply exponential backoff |

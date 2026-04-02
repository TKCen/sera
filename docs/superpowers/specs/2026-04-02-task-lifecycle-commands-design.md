# Spec: Task Lifecycle Commands — Story 16.7

**Date:** 2026-04-02
**Status:** Approved
**Owner:** Gemini CLI

## 1. Overview

This specification defines the expansion of the `sera` CLI (Go) to support agent lifecycle management and task queue interaction. It builds on the existing OIDC authentication (Story 16.6) and leverages backend routes in `sera-core` (Stories 3.x, 5.x).

## 2. Requirements

### 2.1 Agent Instance Lifecycle

- **List Agents:** Display all agent instances with their current status, circle, and lifecycle mode.
- **Start/Stop/Restart:** Control the Docker-backed lifecycle of a specific agent.
- **Logs:** Stream container logs from a running agent.

### 2.2 Task Queue Management

- **List Tasks:** View the per-agent task queue (queued, running, completed, failed).
- **Create Task:** Enqueue a new prompt for an agent to process.
- **Get Task:** Retrieve full details, including the final result or error.
- **Cancel Task:** Remove a task from the queue before it starts.

### 2.3 Non-Functional Requirements

- **Authentication:** Use existing `~/.sera/credentials` (Access Token) or `SERA_API_KEY`.
- **Resilience:** Clear error messages for network failures, missing agents, or invalid task states.
- **UX:** Tabular output for lists; consistent argument parsing (ID or Name).

## 3. Design

### 3.1 Command Structure

The CLI will expose two new top-level subcommands: `agents` and `tasks`.

#### `sera agents`

| Subcommand | Arguments    | API Endpoint                           | Description                                 |
| ---------- | ------------ | -------------------------------------- | ------------------------------------------- |
| `list`     | None         | `GET /api/agents/instances`            | Table: ID, NAME, STATUS, CIRCLE, MODE.      |
| `start`    | `<id\|name>` | `POST /api/agents/instances/:id/start` | Spawns the agent container.                 |
| `stop`     | `<id\|name>` | `POST /api/agents/instances/:id/stop`  | Stops the agent container.                  |
| `restart`  | `<id\|name>` | `POST /api/agents/:id/restart`         | Restarts the agent.                         |
| `logs`     | `<id\|name>` | `GET /api/agents/:id/logs`             | Fetches container logs (default 100 lines). |

#### `sera tasks`

| Subcommand | Arguments                     | API Endpoint                           | Description                                 |
| ---------- | ----------------------------- | -------------------------------------- | ------------------------------------------- |
| `list`     | `<agent-id\|name>`            | `GET /api/agents/:id/tasks`            | Table: ID, TASK (summary), STATUS, CREATED. |
| `create`   | `<agent-id\|name> ["prompt"]` | `POST /api/agents/:id/tasks`           | Enqueues a new task.                        |
| `get`      | `<agent-id\|name> <task-id>`  | `GET /api/agents/:id/tasks/:taskId`    | Detailed view of task.                      |
| `cancel`   | `<agent-id\|name> <task-id>`  | `DELETE /api/agents/:id/tasks/:taskId` | Cancels a `queued` task.                    |

### 3.2 Implementation Strategy (Go)

#### Shared Client (`cli/client.go`)

Refactor request logic into a reusable `Client` struct:

- **`DoRequest(method, path string, body interface{}) (*http.Response, error)`**: Handles URL resolution, auth headers, and JSON encoding/decoding.
- **`ResolveAgentID(nameOrID string) (string, error)`**: Fetches the agent list and finds the UUID for a given name if it's not already a UUID.

#### Name Resolution

For `agents` commands, the CLI will use `ResolveAgentID` to ensure the UUID is passed to the backend. For `tasks` commands, the backend already handles resolution, but the CLI will still use it for consistency or just pass it through.

#### Output Formatting

- **Tables:** Use `tabwriter` for aligned columns.
- **Colors:** Use ANSI escape codes for status coloring (e.g., Green for `running`/`completed`, Yellow for `queued`, Red for `failed`/`error`).

### 3.3 Error Handling

- **401 Unauthorized:** "Not logged in. Run: sera auth login"
- **404 Not Found:** "Agent/Task not found: <id>"
- **409 Conflict:** "Action invalid for current state (e.g., agent already running)."

## 4. Testing Plan

### 4.1 Integration Tests

- Verify `sera agents list` returns expected instances.
- Verify `sera tasks create` results in a new entry in `sera tasks list`.
- Verify `sera agents stop` transitions status in `sera agents list`.

### 4.2 Manual Verification

- Attempt to start an already running agent.
- Attempt to cancel a running task.
- Test with invalid credentials.

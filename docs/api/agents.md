# Agent API

## List Agents

```
GET /api/agents
```

Returns all agent instances with their current status.

**Response:** `AgentInstance[]`

## Get Agent

```
GET /api/agents/:id
```

Returns a single agent instance with full configuration.

## Create Agent

```
POST /api/agents
```

```json
{
  "name": "my-agent",
  "displayName": "My Agent",
  "templateRef": "developer",
  "circle": "development",
  "overrides": {
    "model": { "name": "qwen3.5-35b-a3b" },
    "skills": { "$append": ["typescript-best-practices"] }
  }
}
```

## Update Agent

```
PATCH /api/agents/:id
```

Updates the agent's override configuration. Changes take effect on next start.

## Delete Agent

```
DELETE /api/agents/:id
```

Stops the agent (if running) and removes the instance.

## Start Agent

```
POST /api/agents/:id/start
```

Spawns the agent's Docker container with resolved capabilities.

## Stop Agent

```
POST /api/agents/:id/stop
```

Stops the agent's container. The DB record is preserved.

## Agent Heartbeat

```
POST /api/agents/:id/heartbeat
```

Called by agent-runtime to report liveness. Includes current status and resource usage.

## Agent Grants

```
GET /api/agents/:id/grants
```

Lists all active permission grants (session + persistent).

```
DELETE /api/agents/:id/grants/:grantId
```

Revokes a specific grant.

## Agent Tasks

```
GET /api/agents/:id/tasks
POST /api/agents/:id/tasks
```

Manage the agent's task queue. Ephemeral agents cannot use the task queue (405).

## Agent Status via WebSocket

Subscribe to `agent:{agentId}:status` via Centrifugo for real-time status updates.

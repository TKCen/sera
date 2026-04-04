# Your First Agent

SERA ships with four built-in agent templates. This guide shows you how to create a custom agent instance from a template.

## Built-in Templates

| Template       | Role                                              | Sandbox | Lifecycle  |
| -------------- | ------------------------------------------------- | ------- | ---------- |
| **sera**       | Primary orchestrator and conversational interface | tier-2  | Persistent |
| **developer**  | Full-stack developer for coding tasks             | tier-2  | Persistent |
| **architect**  | System architect for design and review            | tier-2  | Persistent |
| **researcher** | Investigative agent for research and synthesis    | tier-3  | Ephemeral  |

## Creating an Agent via the Dashboard

1. Open the dashboard at `http://localhost:3000`
2. Navigate to **Templates** in the sidebar
3. Select a template (e.g., "Developer")
4. Click **Create Agent** and configure:
   - **Name**: A unique identifier (e.g., `backend-dev`)
   - **Display Name**: Human-readable name
   - **Model**: Select from configured providers
   - **Circle**: Optionally assign to a circle
5. Click **Create**

The agent appears in the **Agents** list. Click **Start** to spawn its container.

## Creating an Agent via YAML

Create a file in `agents/`:

```yaml title="agents/backend-dev.agent.yaml"
apiVersion: sera/v1
kind: Agent

metadata:
  name: backend-dev
  displayName: Backend Developer
  templateRef: developer
  circle: development

overrides:
  model:
    name: qwen3.5-35b-a3b # Must match a model in providers.json
  resources:
    maxLlmTokensPerHour: 200000
  skills:
    $append:
      - typescript-best-practices
```

Restart sera-core to pick up the new manifest. The agent will be created from the `developer` template with your overrides applied.

## Creating an Agent via API

```bash
curl -X POST http://localhost:3001/api/agents \
  -H "Authorization: Bearer $SERA_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "backend-dev",
    "displayName": "Backend Developer",
    "templateRef": "developer",
    "overrides": {
      "model": { "name": "qwen3.5-35b-a3b" }
    }
  }'
```

## Starting and Chatting

Once created, start the agent:

```bash
# Via API
curl -X POST http://localhost:3001/api/agents/{id}/start \
  -H "Authorization: Bearer $SERA_API_KEY"
```

Or click **Start** in the dashboard. Then navigate to **Chat** and select your agent to begin a conversation.

## What Happens at Startup

When an agent starts, sera-core:

1. **Resolves capabilities** — intersects SandboxBoundary, CapabilityPolicy, and manifest overrides
2. **Spawns a Docker container** — with bind mounts, network config, and injected JWT
3. **Injects skills** — loads referenced skill documents into the system prompt
4. **Starts the reasoning loop** — the agent-runtime process begins its observe-plan-act-reflect cycle
5. **Publishes status** — agent status streams to the dashboard via Centrifugo

## Next Steps

- [Capability Model](../concepts/agents.md) — understand how permissions work
- [Writing Skills](../guides/writing-skills.md) — create custom guidance documents
- [MCP Servers](../guides/mcp-servers.md) — give agents executable tools

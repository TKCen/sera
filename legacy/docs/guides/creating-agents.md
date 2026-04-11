# Creating Custom Agents

This guide covers the three ways to create agents in SERA: via the dashboard, YAML manifests, and the API.

## From the Dashboard

1. Navigate to **Templates** and select a base template
2. Click **Create Agent**
3. Fill in the required fields:
   - **Name** — unique identifier (kebab-case, e.g., `my-coder`)
   - **Display Name** — human-readable label
   - **Model** — select from configured providers
4. Optionally configure overrides (skills, resources, circle membership)
5. Click **Create**, then **Start** to spawn the container

## From a YAML Manifest

Create a file in `agents/`:

```yaml title="agents/my-coder.agent.yaml"
apiVersion: sera/v1
kind: Agent

metadata:
  name: my-coder
  displayName: My Coder
  templateRef: developer # inherits from developer template
  circle: development # optional circle membership

overrides:
  model:
    name: qwen3.5-35b-a3b # must match providers.json entry
  resources:
    cpu: '1.0'
    memory: 1Gi
    maxLlmTokensPerHour: 150000
    maxLlmTokensPerDay: 800000
  skills:
    $append:
      - security-best-practices # add to template's skill list
  tools:
    allowed:
      - file-read
      - file-write
      - file-list
      - shell-exec
      - knowledge-store
      - knowledge-query
```

Restart sera-core to pick up the manifest. The agent is created from the template with your overrides applied.

## From the API

```bash
curl -X POST http://localhost:3001/api/agents \
  -H "Authorization: Bearer $SERA_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "my-coder",
    "displayName": "My Coder",
    "templateRef": "developer",
    "overrides": {
      "model": { "name": "qwen3.5-35b-a3b" },
      "skills": { "$append": ["security-best-practices"] }
    }
  }'
```

## Creating Custom Templates

For reusable agent blueprints, create a template:

```yaml title="templates/custom/data-analyst.template.yaml"
apiVersion: sera/v1
kind: AgentTemplate

metadata:
  name: data-analyst
  displayName: Data Analyst
  category: analytics
  description: 'Agent specialised in data analysis and visualisation'

spec:
  identity:
    role: 'Senior data analyst'
    principles:
      - 'Always validate data before drawing conclusions'
      - 'Present findings with supporting evidence'
      - 'Flag uncertainty and data quality issues'

  model:
    provider: lmstudio
    name: qwen3.5-35b-a3b
    temperature: 0.2

  sandboxBoundary: tier-2

  lifecycle:
    mode: persistent

  skills:
    - typescript-best-practices

  tools:
    allowed:
      - file-read
      - file-write
      - shell-exec
      - knowledge-store
      - knowledge-query

  resources:
    cpu: '0.5'
    memory: 512Mi
    maxLlmTokensPerHour: 100000
    maxLlmTokensPerDay: 500000
```

Place it in `templates/custom/` and restart sera-core.

## Managing Running Agents

```bash
# Start an agent
curl -X POST http://localhost:3001/api/agents/{id}/start \
  -H "Authorization: Bearer $SERA_API_KEY"

# Stop an agent
curl -X POST http://localhost:3001/api/agents/{id}/stop \
  -H "Authorization: Bearer $SERA_API_KEY"

# Update overrides (takes effect on next start)
curl -X PATCH http://localhost:3001/api/agents/{id} \
  -H "Authorization: Bearer $SERA_API_KEY" \
  -d '{"overrides": {"model": {"name": "new-model"}}}'
```

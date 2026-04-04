# Managing Budgets

SERA enforces per-agent token budgets to control LLM spending. Every LLM call is metered and checked against hourly and daily limits.

## Default Budgets

| Budget | Default          | Enforcement    |
| ------ | ---------------- | -------------- |
| Hourly | 100,000 tokens   | Pre-call check |
| Daily  | 1,000,000 tokens | Pre-call check |

A budget of `0` means **unlimited**.

## Setting Budgets

### In Agent Templates

```yaml title="templates/builtin/developer.template.yaml"
spec:
  resources:
    maxLlmTokensPerHour: 100000
    maxLlmTokensPerDay: 500000
```

### Via API

```bash
curl -X PATCH http://localhost:3001/api/budget/agents/{id}/budget \
  -H "Authorization: Bearer $SERA_API_KEY" \
  -d '{
    "hourlyQuota": 200000,
    "dailyQuota": 1000000
  }'
```

### In Agent Overrides

```yaml title="agents/my-agent.agent.yaml"
overrides:
  resources:
    maxLlmTokensPerHour: 200000
    maxLlmTokensPerDay: 800000
```

## How Enforcement Works

1. Agent calls LLM via sera-core's proxy
2. `MeteringService.checkBudget()` checks current usage against quotas
3. If budget exceeded: **429 Too Many Requests** returned
4. If within budget: request proceeds, usage recorded after completion

Usage is tracked in the `token_usage` table with hourly rollups.

## Monitoring Usage

### Dashboard

The **Dashboard** page shows:

- Per-agent token consumption (hourly/daily)
- Budget utilisation bars
- Trend charts

The **Agent Detail** page has a **Budget** tab with:

- Current period usage
- Historical usage graphs
- Budget configuration controls

### API

```bash
# Get usage for an agent
GET /api/metering/usage?agentId={id}&groupBy=hour

# Get all agent budgets
GET /api/budget/agents
```

## Circuit Breakers

When an LLM provider is failing, `CircuitBreakerService` prevents cascading failures:

- **Closed** — requests flow normally
- **Open** — requests immediately rejected (provider down)
- **Half-open** — probe requests test if provider has recovered

Circuit breaker state is visible on the dashboard **Health** page.

## Cost Estimation

Token counts include both input and output tokens. For cost estimation:

| Provider                 | Approximate Cost                   |
| ------------------------ | ---------------------------------- |
| Local (Ollama/LM Studio) | Free (your hardware)               |
| OpenAI GPT-4o            | ~$2.50 / 1M input, $10 / 1M output |
| Anthropic Claude 3.5     | ~$3 / 1M input, $15 / 1M output    |
| Google Gemini 1.5        | ~$1.25 / 1M input, $5 / 1M output  |

Set budgets based on your cost tolerance and the agent's expected workload.

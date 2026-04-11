# Scheduling

SERA provides per-agent task scheduling using pg-boss (PostgreSQL-backed job queue). Agents can have cron-based recurring tasks and one-shot scheduled tasks.

## Schedule Types

| Type         | Description                       | Example                          |
| ------------ | --------------------------------- | -------------------------------- |
| **Cron**     | Recurring on a cron expression    | `0 */4 * * *` (every 4 hours)    |
| **One-shot** | Single execution at a future time | Run once at 2026-04-15T09:00:00Z |
| **Manifest** | Defined in agent template YAML    | Auto-created on agent startup    |

## Defining Schedules in Templates

```yaml title="templates/builtin/sera.template.yaml"
spec:
  schedules:
    - name: reflection
      displayName: Self-Reflection
      cron: '0 */6 * * *'
      task:
        prompt: 'Review your recent interactions and update your understanding...'
      enabled: false # paused by default
    - name: knowledge-consolidation
      displayName: Knowledge Consolidation
      cron: '0 2 * * *'
      task:
        prompt: 'Review and consolidate your knowledge base...'
      enabled: false
```

Manifest-defined schedules are synced on every startup — changed schedules are updated in-place, removed ones are deleted, and operator-created API schedules are never touched.

## Creating Schedules via API

```bash
curl -X POST http://localhost:3001/api/schedules \
  -H "Authorization: Bearer $API_KEY" \
  -d '{
    "agentId": "abc-123",
    "name": "daily-report",
    "cron": "0 9 * * *",
    "task": {
      "prompt": "Generate a summary of yesterday activities..."
    }
  }'
```

## Schedule Execution

When a schedule triggers:

1. pg-boss enqueues the task in the agent's queue
2. `AgentScheduler` checks the agent's token budget
3. If within budget, the task prompt is sent to the agent's reasoning loop
4. The agent processes the task and stores results
5. Execution is recorded in the audit trail

!!! note "Budget enforcement"
`AgentScheduler.isWithinQuota()` checks hourly and daily token budgets before triggering. A budget of `0` means unlimited.

## Managing Schedules

The web dashboard **Schedules** page provides:

- List of all schedules across agents
- Enable/disable toggle
- Manual trigger button
- Last run status and output
- Cron expression editor

Agents can also manage their own schedules using the `schedule-task` tool (if they have `seraManagement.schedules` capability).

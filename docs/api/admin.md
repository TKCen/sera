# Admin API

## Providers

### List Providers

```
GET /api/providers
```

Returns all configured LLM providers and their models.

### Add Provider

```
POST /api/providers
```

```json
{
  "modelName": "qwen3.5-35b-a3b",
  "provider": "lmstudio",
  "baseUrl": "http://host.docker.internal:1234/v1"
}
```

Hot-reloads — no restart required.

### Remove Provider

```
DELETE /api/providers/:name
```

## Budgets

### Get Agent Budget

```
GET /api/budget/agents/:id/budget
```

### Update Agent Budget

```
PATCH /api/budget/agents/:id/budget
```

```json
{
  "hourlyQuota": 200000,
  "dailyQuota": 1000000
}
```

Set to `0` for unlimited.

## Usage / Metering

### Get Usage

```
GET /api/metering/usage?agentId={id}&groupBy=hour
```

Returns token usage grouped by hour or day.

## Schedules

### List Schedules

```
GET /api/schedules
```

### Create Schedule

```
POST /api/schedules
```

```json
{
  "agentId": "abc-123",
  "name": "daily-report",
  "cron": "0 9 * * *",
  "task": { "prompt": "Generate daily summary..." }
}
```

### Trigger Schedule

```
POST /api/schedules/:id/trigger
```

Manually triggers a scheduled task.

### Delete Schedule

```
DELETE /api/schedules/:id
```

## Audit

### List Audit Events

```
GET /api/audit/events?agentId={id}&eventType={type}&from={date}&to={date}
```

### Verify Chain Integrity

```
GET /api/audit/verify?from={eventId}&to={eventId}
```

### Export Audit Events

```
GET /api/audit/export?format=csv
```

## Health

### System Health

```
GET /api/health
```

Returns health status of all services (no auth required).

### Detailed Health

```
GET /api/health/detailed
```

Returns per-service health with connection details.

## Secrets

### List Secrets (Metadata Only)

```
GET /api/secrets
```

Returns secret names and descriptions. **Never returns values.**

### Store Secret

```
POST /api/secrets
```

```json
{
  "name": "discord-bot-token",
  "value": "...",
  "description": "Discord bot token for ops channel",
  "allowedAgents": ["sera"]
}
```

### Delete Secret

```
DELETE /api/secrets/:name
```

## Permission Requests

### List Pending Requests

```
GET /api/permission-requests
```

### Submit Decision

```
POST /api/permission-requests/:id/decision
```

```json
{
  "decision": "grant",
  "grantType": "session",
  "expiresAt": null
}
```

## Templates

### List Templates

```
GET /api/templates
```

### Get Template

```
GET /api/templates/:name
```

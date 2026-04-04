# API Reference

sera-core exposes a comprehensive REST API for managing all aspects of the SERA platform. All endpoints (except health checks) require authentication via `Authorization: Bearer <key>`.

## Base URL

| Environment | URL                                               |
| ----------- | ------------------------------------------------- |
| Development | `http://localhost:3001`                           |
| Production  | `http://localhost:3001` (or your configured host) |

## Authentication

```bash
curl -H "Authorization: Bearer $SERA_API_KEY" \
  http://localhost:3001/api/agents
```

## API Sections

| Section                     | Prefix                                   | Description                          |
| --------------------------- | ---------------------------------------- | ------------------------------------ |
| [Agents](agents.md)         | `/api/agents`                            | Agent CRUD, lifecycle, configuration |
| [Chat](chat.md)             | `/api/chat`                              | Chat sessions, message history       |
| [Memory](memory.md)         | `/api/memory`, `/api/knowledge`          | Memory blocks, knowledge store/query |
| [MCP & Tools](mcp-tools.md) | `/api/mcp`, `/api/tools`                 | MCP server management, tool registry |
| [Admin](admin.md)           | `/api/providers`, `/api/schedules`, etc. | Providers, schedules, audit, budgets |

## OpenAPI Specification

The full API specification is available in [OpenAPI 3.0 format](../openapi.yaml). This covers all ~190 endpoints with request/response schemas.

## Common Patterns

### Pagination

List endpoints support pagination:

```
GET /api/agents?limit=20&offset=0
```

### Error Responses

All errors follow a consistent format:

```json
{
  "error": "Not Found",
  "message": "Agent with id abc-123 not found",
  "statusCode": 404
}
```

### Streaming

Chat and LLM proxy endpoints support Server-Sent Events (SSE) for streaming responses:

```
GET /api/chat/{sessionId}/stream
Accept: text/event-stream
```

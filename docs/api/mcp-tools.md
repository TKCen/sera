# MCP & Tools API

## MCP Servers

### List MCP Servers

```
GET /api/mcp/servers
```

Returns all registered MCP servers with their status and available tools.

### Register MCP Server

```
POST /api/mcp/servers
```

```json
{
  "name": "github",
  "displayName": "GitHub Tools",
  "transport": "stdio",
  "command": "npx",
  "args": ["-y", "@modelcontextprotocol/server-github"],
  "secrets": [{ "name": "GITHUB_TOKEN", "exposure": "per-call" }]
}
```

### Unregister MCP Server

```
DELETE /api/mcp/servers/:name
```

### Test MCP Server Connection

```
POST /api/mcp/servers/:name/test
```

Tests the connection and returns the server's tool list.

## Tool Registry

### List All Tools

```
GET /api/tools
```

Returns all available tools (built-in + MCP) with their schemas and capability requirements.

### Tool Proxy

```
POST /api/tools/proxy
```

```json
{
  "agentId": "abc-123",
  "tool": "github/create_pull_request",
  "arguments": {
    "repo": "owner/repo",
    "title": "Fix bug",
    "body": "..."
  }
}
```

Executes a tool call on behalf of an agent, with full capability checking, credential resolution, and audit logging.

## Skills

### List Skills

```
GET /api/skills
```

Returns all registered skills with their metadata.

### Get Skill Content

```
GET /api/skills/:id
```

Returns the full skill document content.

### Create/Update Skill

```
POST /api/skills
PUT /api/skills/:id
```

Create or update a skill document.

# Setting Up MCP Servers

MCP (Model Context Protocol) servers provide executable tools that agents can call during their reasoning loop. This guide covers registering and managing MCP servers in SERA.

## What MCP Servers Do

MCP servers expose tools via a standardised protocol. When an agent needs to perform an action (create a GitHub PR, query a database, search the web), it calls an MCP tool. The MCP server executes the action and returns a structured result.

## Registering an MCP Server

### Via the Dashboard

1. Navigate to **MCP Servers** in the sidebar
2. Click **Add Server**
3. Configure the server connection (stdio or HTTP transport)
4. Test the connection
5. The server's tools appear in the tool registry

### Via YAML Manifest

Create a manifest in `mcp-servers/`:

```yaml title="mcp-servers/github.mcp.yaml"
apiVersion: sera/v1
kind: MCPServer

metadata:
  name: github
  displayName: GitHub Tools
  description: 'GitHub API operations — PRs, issues, repos'

spec:
  transport: stdio
  command: npx
  args: ['-y', '@modelcontextprotocol/server-github']

  secrets:
    - name: GITHUB_TOKEN
      exposure: per-call
      required: true

  capabilities:
    requiredPolicy: network-github
```

### Via API

```bash
curl -X POST http://localhost:3001/api/mcp/servers \
  -H "Authorization: Bearer $SERA_API_KEY" \
  -d '{
    "name": "github",
    "transport": "stdio",
    "command": "npx",
    "args": ["-y", "@modelcontextprotocol/server-github"]
  }'
```

## SERA MCP Extension Protocol

SERA extends the base MCP protocol with credential injection and acting context:

```
X-Sera-Acting-Context: <base64-encoded ActingContext JSON>
X-Sera-Credential-GITHUB_TOKEN: ghp_...
X-Sera-Instance-Id: <instance UUID>
```

For stdio transport, these arrive in a `_sera` envelope field on each `tools/call` message.

## Agent Access to MCP Tools

Agents declare which MCP tools they can use in their manifest:

```yaml
tools:
  allowed:
    - github/* # all tools from the github server
    - filesystem/read # specific tool from filesystem server
```

Tools are capability-gated — the agent's resolved capability set determines which tools are available.

## sera-core as MCP Server

sera-core itself exposes an MCP server with management tools. Agents with `seraManagement` capabilities can use these to orchestrate the SERA instance:

| Tool                                 | Capability                            |
| ------------------------------------ | ------------------------------------- |
| `agents.list`, `agents.create`       | `seraManagement.agents.*`             |
| `circles.create`, `circles.list`     | `seraManagement.circles.*`            |
| `schedules.create`                   | `seraManagement.schedules.*`          |
| `channels.create`, `channels.modify` | `seraManagement.channels.*`           |
| `secrets.requestEntry`               | `seraManagement.secrets.requestEntry` |

This is how Sera (the primary agent) manages the instance autonomously.

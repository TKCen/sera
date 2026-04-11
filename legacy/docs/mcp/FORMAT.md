# MCPServerManifest Format (sera/v1)

`MCPServerManifest` defines how an external MCP tool provider is containerized and exposed within the SERA platform.

## Specification

A manifest must be defined in YAML and follow the `sera/v1` schema.

```yaml
apiVersion: sera/v1
kind: SkillProvider
metadata:
  name: github-mcp
  description: "GitHub API tool server"

image: mcp-server-github:latest
transport: stdio # stdio | http

# Network egress — default is none
network:
  allowlist:
    - api.github.com
    - raw.githubusercontent.com

# Read-only file mounts into the MCP container
mounts:
  - hostPath: /etc/github/config
    containerPath: /app/config
    mode: ro

# Secrets to be injected (Story 7.8)
# These are handled per-call and not directly injected as env vars at startup
secrets:
  - GITHUB_PERSONAL_ACCESS_TOKEN

# Optional health check
healthCheck:
  command: ["ls", "/app"]
  interval: 30s
  timeout: 10s
  retries: 3
```

## Fields

| Field | Type | Description |
|---|---|---|
| `apiVersion` | `string` | Must be `sera/v1` |
| `kind` | `string` | Must be `SkillProvider` |
| `metadata.name` | `string` | Unique name for the MCP server |
| `image` | `string` | Docker image to run |
| `transport` | `string` | `stdio` or `http` |
| `network.allowlist` | `string[]` | List of hostnames the container can egress to |
| `mounts` | `object[]` | List of read-only bind mounts |
| `secrets` | `string[]` | List of secret names to be made available |
| `healthCheck` | `object` | Docker healthcheck configuration |

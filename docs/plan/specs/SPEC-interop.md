# SPEC: Interoperability Protocols (`sera-mcp`, `sera-a2a`, `sera-acp`, `sera-agui`)

> **Status:** DRAFT  
> **Source:** PRD §7  
> **Crates:** `sera-mcp`, `sera-a2a`, `sera-acp`, `sera-agui`  
> **Priority:** Phase 3  

---

## 1. Overview

SERA is **protocol-native** — it speaks the emerging agent ecosystem standards so agents can participate in multi-agent networks beyond the SERA boundary. The interop layer provides adapters for four protocols, each enabling a different interaction pattern.

---

## 2. Protocol Summary

| Protocol | What | SERA Role | Crate |
|---|---|---|---|
| **MCP** (Model Context Protocol) | Tool & resource exposure to LLMs | Server + Client | `sera-mcp` |
| **ACP** (Agent Communication Protocol) | Structured agent-to-agent messaging | Adapter | `sera-acp` |
| **A2A** (Agent-to-Agent, Google) | Federated agent discovery and task delegation | Adapter | `sera-a2a` |
| **AG-UI** (Agent-User Interaction, CopilotKit) | Frontend streaming protocol for agent UIs | Server | `sera-agui` |

---

## 3. MCP — Model Context Protocol (`sera-mcp`)

### 3.1 Dual Role

SERA acts as both **MCP Server** and **MCP Client**:

- **Server:** Exposes SERA tools and resources to external agents/LLMs that speak MCP. External systems can discover and invoke SERA agent tools via the MCP protocol.
- **Client (Bridge):** Consumes external MCP servers as tool sources. Tools from connected MCP servers appear in SERA's tool registry and are available to agents.

### 3.2 MCP Server

The MCP server exposes SERA tools through the standard MCP protocol. Each SERA agent can optionally expose an MCP server endpoint.

**Identity:** When SERA agents are **acted upon** (external MCP client invokes SERA tools), the external caller's identity is the acting principal. Authorization is checked against this external identity.

### 3.3 MCP Client Bridge

External MCP servers are **manually configured** per agent:

```yaml
agents:
  - name: "sera"
    mcp_servers:
      - name: "github"
        url: "http://localhost:3000"
        transport: "stdio"              # stdio | sse | streamable-http
      - name: "filesystem"
        command: "npx"
        args: ["-y", "@modelcontextprotocol/server-filesystem", "/path"]
        transport: "stdio"
```

**Identity:** When SERA agents are **acting** (calling external MCP servers), they must do so with **their own agent identity**. The agent's credentials are used for authentication with the external MCP server.

**Discovery:** Manual configuration only. No auto-discovery of MCP servers.

**Namespacing:** MCP server tools are namespaced by the configured server name to avoid collisions with built-in tools (e.g., `github.create_issue`, `filesystem.read_file`).

### 3.4 Authorization

In both directions, authorization plays a role:
- **Inbound (server):** `sera-auth` checks whether the external caller is authorized to invoke the requested tool
- **Outbound (client):** The agent's tool policy determines which MCP tools it can call; `sera-auth` enforces authorization

---

## 4. A2A — Agent-to-Agent (`sera-a2a`)

### 4.1 Role

Adapter for Google's A2A protocol. SERA agents can:
- **Discover** external A2A agents via federated discovery
- **Delegate** tasks to external A2A agents
- **Receive** task delegations from external A2A agents

### 4.2 Identity

External A2A agents are registered as `ExternalAgentPrincipal` in SERA's principal registry (see [SPEC-identity-authz](SPEC-identity-authz.md)). They have:
- A trust level
- Authorization boundaries
- Audit trail entries

### 4.3 Integration

A2A messages are translated to SERA events at the gateway boundary. The A2A adapter converts between A2A task format and SERA's internal event model.

```
External A2A Agent → A2A Adapter → SERA Event → Gateway → Queue → Runtime
```

---

## 5. ACP — Agent Communication Protocol (`sera-acp`)

### 5.1 Role

Adapter for structured agent-to-agent messaging. SERA agents can send and receive ACP messages via the gateway.

### 5.2 Integration

Similar to A2A — ACP messages are translated to SERA events at the gateway boundary.

---

## 6. AG-UI — Agent-User Interaction (`sera-agui`)

### 6.1 Role

Server-side streaming protocol for agent UIs. The gateway streams AG-UI events for:
- **`sera-web`** — full AG-UI event stream for the SPA
- **Compatible frontends** — any AG-UI compatible client

### 6.2 Full Event Stream

The full AG-UI stream includes all events needed for a rich agent UI:
- Streaming text (token-by-token)
- Tool call notifications with arguments and results
- Approval prompts and responses
- Session metadata updates
- Agent status changes
- Error events

### 6.3 Thin Client Stream

> [!NOTE]  
> See [SPEC-thin-clients](SPEC-thin-clients.md) for the minimal AG-UI event stream designed for HMIs and embedded clients. The `sera-agui` crate exposes both full and minimal streams.

---

## 7. Protocol Integration Architecture

```
External MCP Servers  →  MCP Client Bridge  →  Gateway (tools)
External A2A Agents   →  A2A Adapter       →  Gateway (events)
External ACP Agents   →  ACP Adapter       →  Gateway (events)
Gateway               →  AG-UI Stream      →  sera-web / thin clients
Gateway               →  MCP Server        →  External MCP Clients
```

All protocol adapters connect to the gateway. External protocol traffic enters the same event pipeline as client and connector traffic — subject to the same hooks, authorization, and session management.

---

## 8. Configuration

```yaml
sera:
  interop:
    mcp:
      server:
        enabled: true
        port: 50052
      # Client bridges are configured per-agent (see §3.3)

    a2a:
      enabled: false
      discovery_endpoint: null         # A2A agent registry URL

    acp:
      enabled: false

    agui:
      enabled: true
      # Served on the gateway's HTTP port
```

---

## 9. Cross-References

| Dependency | Spec | Relationship |
|---|---|---|
| `sera-gateway` | [SPEC-gateway](SPEC-gateway.md) | All protocol adapters connect through the gateway |
| `sera-tools` | [SPEC-tools](SPEC-tools.md) | MCP bridge injects tools into the registry |
| `sera-auth` | [SPEC-identity-authz](SPEC-identity-authz.md) | External agent identity registration and authorization |
| `sera-hooks` | [SPEC-hooks](SPEC-hooks.md) | Protocol traffic subject to hook chains |
| Thin clients | [SPEC-thin-clients](SPEC-thin-clients.md) | Minimal AG-UI stream for HMIs |

---

## 10. Open Questions

1. **MCP server per-agent vs. global** — Does each agent get its own MCP server endpoint, or is there one global MCP server that routes to the appropriate agent?
2. **A2A trust bootstrapping** — How is initial trust established with external A2A agents? Manual registration only?
3. **ACP scope** — What specific ACP message types does SERA support? Is this a full ACP implementation or a subset?
4. **AG-UI version** — Which version of the AG-UI protocol spec does SERA target?
5. ~~**Protocol versioning**~~ — Resolved: See [SPEC-versioning](SPEC-versioning.md) §7. Protocol compatibility registry with version negotiation.

---

## 11. Success Criteria

| Metric | Target |
|---|---|
| Extension authoring | < 4 hours for a gRPC connector |
| gRPC adapter latency | < 10ms roundtrip for local adapters |

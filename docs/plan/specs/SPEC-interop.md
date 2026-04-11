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

> **ACP dropped.** The IBM/BeeAI Agent Communication Protocol was donated to the Linux Foundation AI & Data foundation and **merged into A2A on 2025-08-25**. The canonical `i-am-bee/acp` repository is archived. SERA no longer ships a separate `sera-acp` crate — ACP interoperability is provided through the A2A adapter. See [SPEC-dependencies](SPEC-dependencies.md) §5 (Interop Protocol Adapters) and §10.16 (BeeAI Framework ACP→A2A migration playbook) for the full rationale and migration strategy.

| Protocol | What | SERA Role | Crate | Dependency Model |
|---|---|---|---|---|
| **MCP** (Model Context Protocol) | Tool & resource exposure to LLMs | Server + Client | `sera-mcp` | **DEPEND ON** [`rmcp` ^1.3](https://crates.io/crates/rmcp) Apache-2.0, Anthropic official SDK |
| **A2A** (Agent-to-Agent, Linux Foundation, originally Google) | Federated agent discovery and task delegation; **now also carries former ACP semantics** | Adapter | `sera-a2a` | **VENDOR** from canonical [`a2aproject/A2A`](https://github.com/a2aproject/A2A) `specification/a2a.proto` at a pinned commit (no mature Rust SDK yet) |
| ~~**ACP**~~ (Agent Communication Protocol) | ~~Structured agent-to-agent messaging~~ | **DROPPED** — merged into A2A | ~~`sera-acp`~~ | — |
| **AG-UI** (Agent-User Interaction, CopilotKit) | Frontend streaming protocol for agent UIs | Server | `sera-agui` | **VENDOR (hand-roll)** — ~200-line `serde` enum set for the 17 canonical event types from `ag-ui-protocol/ag-ui` at a pinned commit (community Rust crate too immature) |

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

## 5. ACP — Dropped (superseded by A2A)

> [!IMPORTANT]
> **`sera-acp` has been removed from the SERA crate decomposition.**

### 5.1 Timeline

On **2025-08-25**, the IBM/BeeAI Agent Communication Protocol was donated to the Linux Foundation AI & Data foundation and formally merged into A2A. The canonical `i-am-bee/acp` GitHub repository is now marked `archived: true`; its README points to the merger announcement. BeeAI's own framework (see [SPEC-dependencies](SPEC-dependencies.md) §10.16) left its `adapters/acp/` module in-tree but deprecated, and the new `adapters/a2a/` module is the structural twin that replaces it.

### 5.2 SERA's stance

SERA follows the BeeAI playbook:

1. **No new `sera-acp` crate.** The crate from the original plan is deleted from the workspace. See [SPEC-crate-decomposition](SPEC-crate-decomposition.md) §3 for the updated crate catalog.
2. **ACP interoperability via the A2A adapter.** Any existing ACP clients should migrate to A2A. For the brief transition window during which legacy ACP traffic may still appear, the A2A adapter accepts the ACP message shape via a compatibility translator — this translator is optional and feature-gated behind a Cargo feature `acp-compat` on `sera-a2a`.
3. **Dedicated migration ADR.** The transition guidance lives in a standalone doc (`docs/adr/ACP-A2A-migration.md`) rather than inline in any spec, following the BeeAI pattern where the migration guide lives in `beeai-platform/docs/community-and-support/acp-a2a-migration-guide.mdx` rather than inside `beeai-framework`.
4. **Feature-gated compatibility.** The `acp-compat` feature exists so operators with legacy ACP deployments can opt-in without forcing the retired protocol SDK as a mandatory dependency on everyone else.

### 5.3 Evidence

- [`i-am-bee/acp`](https://github.com/i-am-bee/acp) — archived, `archived: true`, last push 2025-08-25
- [LF AI & Data announcement](https://lfaidata.foundation/communityblog/2025/08/29/acp-joins-forces-with-a2a-under-the-linux-foundations-lf-ai-data/) — "ACP joins forces with A2A under the Linux Foundation"
- [i-am-bee/acp discussion #5](https://github.com/orgs/i-am-bee/discussions/5) — migration decision
- [SPEC-dependencies](SPEC-dependencies.md) §10.16 — full playbook applicable to SERA

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
External MCP Servers           →  MCP Client Bridge       →  Gateway (tools)
External A2A Agents            →  A2A Adapter             →  Gateway (events)
Legacy ACP Agents (compat)     →  A2A Adapter (acp-compat) →  Gateway (events)
Gateway                        →  AG-UI Stream             →  sera-web / thin clients
Gateway                        →  MCP Server               →  External MCP Clients
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
      acp_compat: false                # Opt-in legacy ACP shape translator
                                       # (requires `acp-compat` Cargo feature)

    # NOTE: `acp:` block removed. ACP was merged into A2A on 2025-08-25.
    # Use `a2a.acp_compat: true` for legacy ACP client support during transition.
    # See §5 and SPEC-dependencies §10.16.

    agui:
      enabled: true
      # Served on the gateway's HTTP port
      events:                          # MVS minimum event subset
        - RUN_STARTED
        - RUN_FINISHED
        - RUN_ERROR
        - TEXT_MESSAGE_START
        - TEXT_MESSAGE_CONTENT
        - TEXT_MESSAGE_END
        - TOOL_CALL_START
        - TOOL_CALL_ARGS
        - TOOL_CALL_END
        - STATE_SNAPSHOT
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
3. ~~**ACP scope**~~ — Resolved: ACP merged into A2A on 2025-08-25. See §5 and [SPEC-dependencies](SPEC-dependencies.md) §10.16.
4. **AG-UI version** — Which specific commit of `ag-ui-protocol/ag-ui` does SERA's vendored enum set pin against? Needs a pinned commit SHA in `sera-agui` at Phase 3 implementation.
5. ~~**Protocol versioning**~~ — Resolved: See [SPEC-versioning](SPEC-versioning.md) §7. Protocol compatibility registry with version negotiation.
6. **`acp-compat` feature lifetime** — How long does the A2A adapter carry the ACP compatibility translator? Tentative: 12 months from first SERA release, then removed in a major version bump with migration notice.

---

## 11. Success Criteria

| Metric | Target |
|---|---|
| Extension authoring | < 4 hours for a gRPC connector |
| gRPC adapter latency | < 10ms roundtrip for local adapters |

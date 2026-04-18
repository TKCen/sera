# SPEC: Clients (`sera-cli`, `sera-tui`, `sera-web`, `sera-sdk`)

> **Status:** DRAFT  
> **Source:** PRD §4.4  
> **Crates:** `sera-cli`, `sera-tui`, `sera-sdk`; `sera-web` is a separate frontend project  
> **Priority:** Phase 3  

---

## 1. Overview

SERA provides multiple client interfaces for different use cases and user preferences. All clients communicate with the gateway via standardized protocols (gRPC, WebSocket, AG-UI). No client has direct access to internal subsystems — everything flows through the gateway.

---

## 2. Client Catalog

| Client | Type | Protocol | Primary Use | Crate/Project |
|---|---|---|---|---|
| `sera-cli` | Terminal | gRPC + WS | Developer / operator interaction | `sera-cli` |
| `sera-tui` | Rich TUI | gRPC + WS | Interactive local operation | `sera-tui` |
| `sera-web` | SPA | WS + AG-UI stream | Browser-based multi-agent management | Separate frontend |
| Client SDKs | Library | gRPC + WS | Programmatic integration | `sera-sdk` |

> [!NOTE]  
> **Thin Clients / HMIs** are covered in a separate spec: [SPEC-thin-clients](SPEC-thin-clients.md).

---

## 3. `sera-cli`

### 3.1 Purpose

Command-line interface for developers and operators. Provides both interactive conversation mode and administrative commands.

### 3.2 Capabilities

| Category | Commands |
|---|---|
| **Init & Bootstrap** | `sera init`, `sera start`, `sera stop` |
| **Agent Management** | `sera agent create/list/delete/config` |
| **Conversation** | `sera chat [agent]` — interactive conversation mode |
| **Session Management** | `sera session list/view/archive/destroy` |
| **Config** | `sera config get/set/validate` |
| **Secrets** | `sera secret set/list/delete` |
| **Connectors** | `sera connector add/list/status/remove` |
| **Approval** | `sera approval list/approve/reject` |
| **Workflow** | `sera workflow list/trigger/status` |
| **Diagnostics** | `sera status`, `sera health`, `sera logs` |

### 3.3 Bootstrap Flow

```
sera init                     # Interactive: pick LLM provider, set API key
sera agent create "sera"      # Create a default agent with basic tools
sera start                    # Agent is ready — it can help configure the rest
```

### 3.4 Dependencies

- `clap` — command parsing
- `sera-sdk` — client library for gateway communication

---

## 4. `sera-tui`

### 4.1 Purpose

Rich terminal UI built with `ratatui` for interactive local operation. Provides a visual dashboard with conversation panes, session management, and system monitoring.

### 4.2 Capabilities

- Multi-pane layout (conversation, session list, agent status)
- Streaming response display
- Approval prompts inline
- System status monitoring
- Log viewing

### 4.3 Dependencies

- `ratatui` — terminal UI framework
- `sera-sdk` — client library

---

## 5. `sera-web`

### 5.1 Purpose

Browser-based SPA for multi-agent management. Provides a rich visual interface for managing agents, sessions, conversations, approvals, and system configuration.

### 5.2 Design

> [!IMPORTANT]  
> **Requires further research.** The PRD specifies a "clean rebuild" of the web client, AG-UI compatibility, and no backward compatibility constraint. The UI framework choice (React/Next.js, Svelte, etc.) is not yet decided.

### 5.3 Protocol

The web client consumes the **full AG-UI event stream** from `sera-agui` for real-time agent interaction. Administrative operations use REST or gRPC-Web.

### 5.4 Key Feature Areas

- Agent conversation interface (streaming, tool call display)
- Session management (list, view transcript, archive)
- Approval queue (pending approvals, approve/reject)
- Agent configuration (edit config with live validation)
- System dashboard (health, metrics, active sessions)
- Secret management (enter secrets, view references)
- Connector status
- Workflow management

---

## 6. `sera-sdk`

### 6.1 Purpose

Client SDK library for programmatic integration. Enables developers to build custom clients, scripts, and integrations that interact with SERA programmatically.

### 6.2 API Surface

```rust
pub struct SeraClient {
    // Connection
    pub async fn connect(config: ClientConfig) -> Result<Self, ConnectionError>;

    // Conversation
    pub async fn send_message(&self, agent: &str, message: &str) -> Result<ResponseStream, ClientError>;

    // Sessions
    pub async fn list_sessions(&self) -> Result<Vec<SessionInfo>, ClientError>;
    pub async fn get_session(&self, id: &SessionId) -> Result<SessionDetails, ClientError>;

    // Agents
    pub async fn list_agents(&self) -> Result<Vec<AgentInfo>, ClientError>;

    // Approvals
    pub async fn list_pending_approvals(&self) -> Result<Vec<ApprovalTicket>, ClientError>;
    pub async fn approve(&self, ticket: &ApprovalTicket) -> Result<(), ClientError>;
    pub async fn reject(&self, ticket: &ApprovalTicket, reason: &str) -> Result<(), ClientError>;

    // Config
    pub async fn config_read(&self, path: &str) -> Result<ConfigValue, ClientError>;
    pub async fn config_propose(&self, change: ConfigChange) -> Result<ConfigChangeResult, ClientError>;

    // Health
    pub async fn health(&self) -> Result<HealthStatus, ClientError>;
}
```

### 6.3 Dependencies

- `tonic` — gRPC client
- `tokio-tungstenite` — WebSocket client

---

## 7. Protocol Support

All clients communicate with the gateway via standardized protocols:

| Protocol | Transport | Use Case |
|---|---|---|
| **gRPC** | HTTP/2 | Structured RPC calls (admin, config, tools) |
| **WebSocket** | HTTP upgrade | Streaming conversation, real-time updates |
| **AG-UI** | SSE / HTTP stream | Web client streaming (served by `sera-agui`) |

Both WebSocket and gRPC streaming coexist on the same gateway.

---

## 8. Cross-References

| Dependency | Spec | Relationship |
|---|---|---|
| `sera-gateway` | [SPEC-gateway](SPEC-gateway.md) | All clients connect to the gateway |
| `sera-agui` | [SPEC-interop](SPEC-interop.md) | AG-UI stream for web client |
| `sera-auth` | [SPEC-identity-authz](SPEC-identity-authz.md) | Client authentication |
| `sera-hitl` | [SPEC-hitl-approval](SPEC-hitl-approval.md) | Approval UI in clients |
| `sera-config` | [SPEC-config](SPEC-config.md) | Config management surface |
| Thin clients | [SPEC-thin-clients](SPEC-thin-clients.md) | Minimal client variant |

---

## 9. Open Questions

1. **Web framework choice** — React/Next.js? Svelte? SolidJS? (Requires research)
2. **CLI packaging** — Single binary? How is `sera-cli` distributed apart from the main `sera` binary?
3. **SDK language support** — Is the SDK Rust-only initially? Plans for Python/TypeScript SDK wrappers?
4. **Offline mode** — Can CLI/TUI work with cached data when the gateway is unreachable?
5. **Client authentication flow** — How does initial client auth work? Interactive login? API key setup?

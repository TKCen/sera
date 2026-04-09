# SPEC: Tools (`sera-tools`)

> **Status:** DRAFT  
> **Source:** PRD §4.3, §13 (ToolService proto), §14 (invariant 5)  
> **Crate:** `sera-tools`  
> **Priority:** Phase 1  

---

## 1. Overview

The tool system is where **chat becomes action**. Tools are the capabilities that agents can invoke to interact with the world — read files, run commands, search memory, send messages, make API calls, etc.

A critical design principle: **capability exposure is separated from execution authority**. Exposing a tool schema to the model (so it knows the tool exists) does NOT grant permission to execute it. Execution authority is checked at call time via the authorization system.

---

## 2. Design Principles

1. Tools are **capability proposals**, not execution grants
2. Execution targets are explicit (sandbox, local, remote node)
3. Tool results re-enter the runtime loop before final response
4. Tools support **credential injection** via hooks and the secret manager
5. **Pre-tool hooks** for risk checks, approval gates, argument validation
6. **Post-tool hooks** for result sanitization, audit, compliance

---

## 3. Tool Trait

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn metadata(&self) -> ToolMetadata;
    fn schema(&self) -> ToolSchema;
    async fn execute(&self, input: ToolInput, ctx: ToolContext) -> Result<ToolOutput, ToolError>;
    fn risk_level(&self) -> RiskLevel;
}

pub enum RiskLevel {
    Read,       // Read-only observation
    Write,      // Modifies state
    Execute,    // Runs arbitrary code
    Admin,      // System-level operations
}
```

### Tool Context

```rust
pub struct ToolContext {
    pub session: SessionRef,
    pub principal: PrincipalRef,       // The acting principal (may be agent or human)
    pub credentials: CredentialBag,     // Populated by Secret Manager + pre_tool hooks
    pub policy: ToolPolicy,
    pub audit_handle: AuditHandle,
}
```

### Tool Metadata

```rust
pub struct ToolMetadata {
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: Option<String>,
    pub risk_level: RiskLevel,
    pub execution_target: ExecutionTarget,
    pub tags: Vec<String>,
}

pub enum ExecutionTarget {
    InProcess,             // Runs inside the gateway process
    Sandbox(SandboxRef),   // Runs in a sandboxed environment (see §6a)
    Local,                 // Runs on the local machine (non-sandboxed)
    Remote(String),        // Runs on a specific remote node
    External,              // gRPC external tool service
}
```

---

## 4. Tool Registry

The tool registry is the **catalog of all available tools**. Tools can be:
- **Built-in** — compiled into the binary (memory tools, session tools, config tools, shell, etc.)
- **Plugin** — dynamically registered via gRPC (ToolService)
- **MCP-bridged** — tools discovered from connected MCP servers (see [SPEC-interop](SPEC-interop.md))

### Registry Operations

```rust
#[async_trait]
pub trait ToolRegistry: Send + Sync {
    async fn register(&self, tool: Box<dyn Tool>) -> Result<(), RegistryError>;
    async fn unregister(&self, name: &str) -> Result<(), RegistryError>;
    async fn get(&self, name: &str) -> Option<Arc<dyn Tool>>;
    async fn list(&self, filter: &ToolFilter) -> Vec<ToolMetadata>;
    async fn list_for_agent(&self, agent: &AgentRef, policy: &ToolPolicy) -> Vec<ToolSchema>;
}
```

### Hot-Reload

Tool registrations should support hot-reload — adding or removing tools without restarting the gateway.

### 4.1 Progressive Tool Disclosure

> **Enhancement: Anthropic Code Execution with MCP**

When an agent has access to many tools (50+), loading all tool definitions into the context window upfront causes significant token overhead. The tool injection step supports **progressive disclosure** — only the most relevant tools are injected.

**Strategy:**

1. **Core tools always injected:** A small set of essential tools (memory, session, config, yield_to_supervisor) are always present in context. These are the agent's "baseline capabilities."
2. **On-demand discovery:** A `search_tools` meta-tool allows the agent to discover additional tools by keyword, category, or tag:

```rust
pub struct SearchToolsInput {
    pub query: String,                     // Natural language or keyword
    pub tags: Option<Vec<String>>,        // Filter by tags
    pub risk_level: Option<RiskLevel>,    // Filter by risk level
    pub limit: u32,                        // Max results (default: 10)
}

pub struct SearchToolsOutput {
    pub tools: Vec<ToolMetadata>,         // Matching tool metadata (not full schemas)
}
```

3. **Activation:** When the agent decides to use a discovered tool, its full schema is injected into subsequent turns via the `context_tool` step.
4. **Skill-bound tools:** Tools listed in the active skill's `tool_bindings` are automatically injected (see [SPEC-runtime](SPEC-runtime.md) §13).

**Configuration:**

```yaml
agents:
  - name: "sera"
    tools:
      disclosure:
        strategy: "progressive"          # all | progressive
        core_tools:                      # Always available
          - "memory_*"
          - "session_*" 
          - "config_*"
          - "yield_to_supervisor"
          - "search_tools"
        max_injected: 15                 # Maximum tool schemas per turn
```

> [!NOTE]
> Progressive disclosure is most valuable in enterprise deployments with large MCP tool surfaces. Tier 1 deployments with <15 tools can use `strategy: "all"` without penalty.

---

## 5. Tool Profiles

> [!IMPORTANT]  
> **Design uncertain.** The PRD mentions tool profiles (`minimal | basic | coding | full | custom`) but does not define what tools are in each profile. The concept of pre-defined profiles may be replaced with a more flexible allow/deny list mechanism.
>
> **Current design:** Profiles are syntactic sugar over allow/deny lists. The spec defines the profile **mechanism** but the concrete profile contents are TBD.

### Profile Mechanism

```yaml
agents:
  - name: "sera"
    tools:
      profile: "basic"               # Pre-defined profile (TBD what's in each)
      allow: ["memory_*", "session_*", "shell"]  # Additional allows (glob patterns)
      deny: ["admin_*"]              # Explicit denials (glob patterns, override allows)
```

```rust
pub struct ToolPolicy {
    pub profile: Option<ToolProfile>,
    pub allow_patterns: Vec<String>,
    pub deny_patterns: Vec<String>,
}

pub enum ToolProfile {
    Minimal,
    Basic,
    Coding,
    Full,
    Custom,
}
```

Deny always wins over allow. The authorization system (`sera-auth`) is checked **in addition to** the tool policy — both must permit execution.

---

## 6. Tool Execution Flow

```
Agent requests tool call
  → Resolve tool from registry
  → Check ToolPolicy (profile + allow/deny)
  → pre_tool hook chain
    → Secret injection (credential resolution via sera-secrets)
    → Risk assessment
    → Argument validation
    → Approval gate check (may route to sera-hitl)
  → AuthZ check via sera-auth (can this principal execute this tool?)
  → Execute tool
  → post_tool hook chain
    → Result sanitization
    → Audit logging
    → Risk assessment of result
  → Return result to runtime (re-enters model)
```

---

## 6a. Sandbox Lifecycle Management

> **Enhancement: OpenSwarm §3 (Isolated Execution), Strategic Rearchitecture §Bunnyshell/Firecracker**

When a tool's `ExecutionTarget` is `Sandbox`, the tool system provisions an ephemeral execution environment via the `SandboxProvider` trait. The sandbox provides hardware or process-level isolation for untrusted code execution.

### Sandbox Provider Trait

```rust
#[async_trait]
pub trait SandboxProvider: Send + Sync {
    /// Provision a new sandbox instance from a profile.
    async fn create(&self, config: &SandboxConfig) -> Result<SandboxId, SandboxError>;

    /// Execute a command inside the sandbox. Captures stdout, stderr, exit code.
    async fn execute(
        &self,
        id: &SandboxId,
        command: &SandboxCommand,
    ) -> Result<SandboxResult, SandboxError>;

    /// Read a file from the sandbox filesystem.
    async fn read_file(&self, id: &SandboxId, path: &str) -> Result<Vec<u8>, SandboxError>;

    /// Write a file to the sandbox filesystem.
    async fn write_file(&self, id: &SandboxId, path: &str, content: &[u8]) -> Result<(), SandboxError>;

    /// Destroy the sandbox. All state is lost.
    async fn destroy(&self, id: &SandboxId) -> Result<(), SandboxError>;

    /// Health check.
    async fn health(&self) -> HealthStatus;
}

pub struct SandboxConfig {
    pub profile: SandboxProfile,
    pub timeout: Duration,              // Kill sandbox after this duration
    pub memory_limit: Option<u64>,      // Bytes
    pub cpu_limit: Option<f64>,         // CPU fraction
    pub network_access: bool,           // Allow external network (default: false)
    pub filesystem_mounts: Vec<Mount>,  // Read-only or read-write mounts from host
}

pub enum SandboxProfile {
    Docker(DockerProfile),              // Docker container
    Wasm,                               // WASM sandbox (lightweight)
    MicroVm(MicroVmProfile),            // Firecracker / similar microVM
    External(String),                   // External provider via gRPC/MCP
}

pub struct SandboxResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration: Duration,
}
```

### Lifecycle

```
Tool execution with Sandbox target:
  → SandboxProvider.create(config) — provision ephemeral environment
  → SandboxProvider.write_file() — inject any necessary input files
  → SandboxProvider.execute(command) — run the tool command
  → Capture stdout, stderr, exit_code
  → SandboxProvider.read_file() — extract output artifacts if needed
  → SandboxProvider.destroy() — tear down sandbox (mandatory, even on error)
  → Return SandboxResult as ToolOutput to runtime
```

**Sandboxes are ephemeral by default** — created per tool execution and destroyed immediately after. For tools that need a persistent environment across multiple calls within a turn (e.g., iterative code editing), a sandbox can be kept alive for the turn's duration and destroyed at turn completion.

### Built-in Providers

| Provider | Use Case | Isolation Level | Startup Time |
|---|---|---|---|
| **Docker** | General code execution, testing | Process + namespace | ~1–5s |
| **WASM** | Lightweight, safe computations | Memory sandbox | ~10ms |
| **MicroVM** | High-security execution (Firecracker) | Hardware-level kernel isolation | ~100ms |
| **External** | Remote sandbox via gRPC or MCP (e.g., Bunnyshell) | Provider-dependent | Provider-dependent |

### Configuration

```yaml
sera:
  sandbox:
    default_provider: "docker"         # docker | wasm | microvm | external
    docker:
      image: "sera-sandbox:latest"     # Default sandbox image
      network: false                   # No network by default
      memory_limit: "512m"
      timeout: "60s"
    external:
      url: "http://bunnyshell:50053"   # External sandbox provider endpoint
      transport: "grpc"                # grpc | mcp

agents:
  - name: "sera"
    tools:
      sandbox_overrides:
        shell:
          provider: "docker"
          network: false
          timeout: "30s"
        python_execute:
          provider: "docker"
          image: "sera-sandbox-python:latest"
```

### Security Invariants

- Sandboxes have **no ambient host access** — no filesystem, no network unless explicitly configured
- Sandboxes have **resource limits** (memory, CPU, timeout) — mandatory, no unlimited sandboxes
- Sandbox destruction is **mandatory** — the system guarantees cleanup even on tool failure or crash
- Sandbox outputs go through `post_tool` hooks — results are sanitized before reaching the model

---

## 7. Credential Injection

Tools that need external credentials (API keys, OAuth tokens, etc.) receive them via the `CredentialBag` in `ToolContext`. Credentials are:
1. Resolved by the Secret Manager (`sera-secrets`) based on the agent's credential mappings
2. Optionally enriched/overridden by `pre_tool` hooks (e.g., a secret-injector hook)
3. Scoped to the acting principal — agent credentials are distinct from human credentials

See [SPEC-secrets](SPEC-secrets.md) for the full secret management and injection flow.

---

## 8. External Tool Service (gRPC)

```protobuf
service ToolService {
    rpc GetMetadata(Empty) returns (ToolMetadata);
    rpc GetSchema(Empty) returns (ToolSchema);
    rpc Execute(ToolInput) returns (ToolOutput);
}
```

External tools register with the gateway's plugin registry and are available to agents just like built-in tools.

---

## 9. MCP Bridge

The MCP bridge allows tools from external MCP servers to appear in the SERA tool registry. MCP tools are manually configured per agent:

```yaml
agents:
  - name: "sera"
    mcp_servers:
      - name: "github"
        url: "http://localhost:3000"
        # Each MCP server's tools appear with a namespace prefix
```

See [SPEC-interop](SPEC-interop.md) for full MCP integration details.

---

## 10. Invariants

| # | Invariant | Enforcement |
|---|---|---|
| 5 | Capability ≠ execution | Tool schemas exposed ≠ execution authorized; checked at call time |

---

## 11. Hook Points

| Hook Point | Fires When |
|---|---|
| `context_tool` | During tool injection step in context assembly — selects which tools to inject |
| `pre_tool` | Before tool execution — risk checks, approval gates, secret injection |
| `post_tool` | After tool execution — result sanitization, audit, PII tokenization |

---

## 12. Cross-References

| Dependency | Spec | Relationship |
|---|---|---|
| `sera-auth` | [SPEC-identity-authz](SPEC-identity-authz.md) | AuthZ for tool execution |
| `sera-secrets` | [SPEC-secrets](SPEC-secrets.md) | Credential resolution and injection |
| `sera-hooks` | [SPEC-hooks](SPEC-hooks.md) | Pre/post tool hook chains |
| `sera-hitl` | [SPEC-hitl-approval](SPEC-hitl-approval.md) | Approval gating for risky tools |
| `sera-mcp` | [SPEC-interop](SPEC-interop.md) | MCP bridge for external tools |
| `sera-runtime` | [SPEC-runtime](SPEC-runtime.md) | Tool execution within turn loop |

---

## 13. Open Questions

1. **Tool profiles** — Should pre-defined profiles exist at all, or just allow/deny lists? If profiles exist, what tools are in each? (Design uncertain)
2. **MCP tool namespacing** — How are MCP server tools namespaced to avoid collisions with built-in tools?
3. **Tool versioning** — How does the registry handle multiple versions of the same tool?
4. ~~**Sandbox execution**~~ — Resolved: See §6a. Pluggable `SandboxProvider` trait with Docker, WASM, MicroVM, and External providers.
5. **Tool result size limits** — Are there limits on tool output size? How are large results handled? See also [SPEC-runtime](SPEC-runtime.md) §5.5 tool result filtering.
6. **Built-in tool catalog** — What is the concrete list of built-in tools shipped with SERA? (memory_read, memory_write, memory_search, search_tools, session_*, config_read, config_propose, docs_read, shell, yield_to_supervisor, secret_request, ...)
7. **Sandbox image management** — How are Docker sandbox images built, versioned, and distributed?
8. **Progressive disclosure cache invalidation** — When tools are hot-reloaded, how are previously-disclosed tool schemas updated in active sessions?

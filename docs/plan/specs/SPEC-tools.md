# SPEC: Tools (`sera-tools`)

> **Status:** DRAFT
> **Source:** PRD §4.3, §13 (ToolService proto), §14 (invariant 5), plus deltas from [SPEC-dependencies](SPEC-dependencies.md) §9.3 (Docker sandbox via `bollard`, WASM via `wasmtime`, MicroVM via `firecracker` binary wrap, `microsandbox` watch-only), §10.2 (Codex `DynamicToolSpec` schema-driven registration + `defer_loading` + three-layer sandbox model), §10.7 (opencode `Tool.Context::ask()` inline approval + `FileTime.withLock` conflict detection + tree-sitter bash analysis + `assertExternalDirectoryEffect` path policy), §10.8 (NemoClaw per-binary egress allowlist + method+path REST rules + enforce/audit modes + TLS terminate/passthrough + Landlock rule-union gotcha + pinned-image dual-field lockstep + opt-in preset system + Sentry-class exfiltration defense + SSRF validation), §10.13 (openai-agents-python `is_enabled` / `needs_approval` / `tool_use_behavior`), §10.16 (BeeAI `Tool` Pydantic-schema interface + `BaseCache` SHA-512 content-addressed caching), §10.18 (**`NVIDIA/OpenShell`** — published mTLS gRPC protocol, `SandboxPolicy` proto schema, in-process OPA via `regorus`, binary SHA-256 TOFU identity, `allowed_ips: [CIDR]` SSRF mitigation, `access` preset shorthand, AI-assisted policy advisor, hot-reload with version tracking, `inference.local` virtual host)
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

SERA exposes tools via **two parallel registration paths** — both are first-class and interoperable:

1. **Rust trait path** — the `Tool` trait, for in-process tools compiled into the gateway binary
2. **Schema-driven path** — `DynamicToolSpec` values, for MCP-bridged tools, external gRPC tools, and runtime-registered tools whose implementation lives outside the gateway

Both paths produce the same `ToolRegistryEntry` and flow through the same tool-call pipeline.

### 3.1 Rust `Tool` trait

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn metadata(&self) -> ToolMetadata;
    fn schema(&self) -> ToolSchema;              // Derived from schemars on the input struct
    async fn execute(&self, input: ToolInput, ctx: ToolContext) -> Result<ToolOutput, ToolError>;
    fn risk_level(&self) -> RiskLevel;

    /// Dynamic tool visibility callback (openai-agents-python pattern, SPEC-dependencies §10.13).
    /// Tool is hidden from the LLM at registration time if this returns false.
    fn is_enabled(&self, ctx: &ToolEnableContext) -> bool { true }

    /// Per-tool HITL gate (openai-agents-python needs_approval pattern).
    /// Returns Some(ApprovalSpec) if this specific call requires approval based on its arguments.
    fn needs_approval(&self, input: &ToolInput, ctx: &ToolContext) -> Option<ApprovalSpec> { None }
}

pub enum RiskLevel {
    Read,       // Read-only observation
    Write,      // Modifies state
    Execute,    // Runs arbitrary code
    Admin,      // System-level operations
}
```

### 3.2 Schema-Driven `DynamicToolSpec`

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.2 Codex. Schema-driven registration, no Rust trait required.

```rust
pub struct DynamicToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,   // JSON Schema — derived externally (e.g. from MCP server)
    pub output_schema: Option<serde_json::Value>,
    pub defer_loading: bool,                // If true, not injected into context until activated (progressive disclosure)
    pub risk_level: RiskLevel,
    pub execution_target: ExecutionTarget,
}

pub struct DynamicToolCallRequest {
    pub call_id: ToolCallId,
    pub turn_id: TurnId,                    // Scoping every call with turn_id is essential for routing multi-call results
    pub tool: String,
    pub arguments: serde_json::Value,
}

pub struct DynamicToolResponse {
    pub call_id: ToolCallId,
    pub turn_id: TurnId,
    pub content_items: Vec<DynamicToolCallOutputContentItem>,
    pub success: bool,
}

pub enum DynamicToolCallOutputContentItem {
    InputText { text: String },
    InputImage { image_url: String },
}
```

**Why both paths?** MCP tools arrive at runtime from external servers with their own JSON Schema — wrapping them in the `Tool` trait requires ad-hoc code. The `DynamicToolSpec` path lets them register as first-class citizens without imposing a Rust type on their schema.

### 3.3 `tool_use_behavior` Discriminated Union

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.13 openai-agents-python.

```rust
pub enum ToolUseBehavior {
    /// Re-invoke the LLM after each tool result (default — standard agent loop)
    RunLlmAgain,

    /// Stop the loop after the first tool call and return its result as the final output
    StopOnFirstTool,

    /// Stop when the LLM calls any tool in this set
    StopAtTools(HashSet<String>),

    /// Custom function decides whether to continue the loop after each tool call
    ToolsToFinalOutputFunction(Box<dyn Fn(&ToolResult) -> ToolLoopDecision + Send + Sync>),
}

pub enum ToolLoopDecision {
    Continue,
    Stop,
    StopWith(ToolResult),
}
```

Per-agent `tool_use_behavior` lets the harness compose short-circuit logic without subclassing the runner.

### 3.4 Tool Context (opencode-enhanced)

```rust
pub struct ToolContext {
    pub session: SessionRef,
    pub principal: PrincipalRef,
    pub credentials: CredentialBag,
    pub policy: ToolPolicy,
    pub audit_handle: AuditHandle,
    pub turn_id: TurnId,
    pub call_id: ToolCallId,
    pub agent: AgentRef,
    pub abort: AbortSignal,
    pub messages: Arc<SessionTranscript>,

    /// Inline approval callback — the tool itself can request HITL approval mid-execution.
    /// Source: SPEC-dependencies §10.7 opencode Tool.Context::ask().
    pub ask: Box<dyn Fn(ApprovalRequest) -> BoxFuture<'static, ApprovalResponse> + Send + Sync>,

    /// Metadata updater — tool can update its own lifecycle metadata (progress, intermediate state).
    pub metadata: Box<dyn Fn(MetadataUpdate) + Send + Sync>,
}
```

Inline `ask()` is the key insight: the tool itself is responsible for requesting approval during execution, not a separate interceptor layer. This keeps approval logic close to the tool's own state machine and avoids having to re-design the tool-call pipeline every time a new approval pattern is added.

### 3.5 `FileTime.withLock` Conflict Detection

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.7 opencode.

File-write tools MUST check file modification time before writing. If the file has been modified externally since the harness last read it, raise a conflict error rather than silently overwrite.

```rust
pub struct FileTime {
    known_times: Mutex<HashMap<PathBuf, SystemTime>>,
}

impl FileTime {
    pub async fn with_lock<F, R>(&self, path: &Path, f: F) -> Result<R, FileTimeError>
    where F: FnOnce() -> BoxFuture<'static, R>;

    pub async fn record_read(&self, path: &Path) -> Result<(), FileTimeError>;
    pub async fn check_unchanged_since_read(&self, path: &Path) -> Result<(), FileTimeError>;
}
```

This is a file-system-level companion to the atomic-claim protocol — concurrent-edit safety for multi-agent worktrees without requiring OS-level locking.

### 3.6 `SsrfValidator` Trait

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.8 NemoClaw + §10.18 OpenShell.

Tools that accept URL arguments run the URL through a pluggable `SsrfValidator` before any network operation:

```rust
#[async_trait]
pub trait SsrfValidator: Send + Sync {
    async fn validate(&self, url: &Url, policy: &NetworkPolicy) -> Result<(), SsrfError>;
}
```

The validator checks for private IPs, link-local addresses, DNS rebinding vectors, and policy-specific allow/deny lists. This runs **in addition to** network egress policy — SSRF is a request-side concern (what URL the tool was given) while network policy is a connection-side concern (what the egress proxy permits).

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

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §9.3 (crate choices), §10.2 (Codex three-layer sandbox model), §10.8 (NemoClaw policy schema), §10.18 (NVIDIA OpenShell Rust gRPC protocol + `regorus` in-process OPA + OCSF events + binary SHA-256 TOFU identity).

When a tool's `ExecutionTarget` is `Sandbox`, the tool system provisions an execution environment via the `SandboxProvider` trait. The sandbox provides hardware or process-level isolation for untrusted code execution.

### 6a.0 Three-Layer Sandbox Policy Model

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.2 Codex + §10.18 OpenShell `SandboxPolicy` proto.

Sandbox policy has **three orthogonal layers**, not a single flat allowlist:

1. **Coarse `SandboxPolicy`** — per-session/per-agent default (read-only vs workspace-write vs full-access vs external-contained)
2. **Fine `FileSystemSandboxPolicy`** — per-exec filesystem scope (specific mount points, read-only/read-write per path, Landlock compatibility mode)
3. **Fine `NetworkSandboxPolicy`** — per-exec network egress allowlist (method+path REST rules, TLS terminate/passthrough, `allowed_ips: [CIDR]`, enforce/audit mode, per-binary SHA-256 TOFU)

All three layers apply simultaneously; the intersection is the effective policy for that execution.

```rust
pub enum SandboxPolicy {
    /// Default — no writes, no network, read-only workspace
    ReadOnly { access: ReadOnlyAccess, network_access: bool },
    /// Writes to workspace only, no network
    WorkspaceWrite { allowed_paths: Vec<PathBuf>, network_access: bool },
    /// Full access — used for trusted contexts
    DangerFullAccess,
    /// Harness itself is already containerized; policy enforcement delegates up
    ExternalSandbox { network_access: NetworkAccess },
}

pub struct NetworkPolicyRule {
    pub name: String,
    pub endpoints: Vec<NetworkEndpoint>,
    pub binaries: Vec<NetworkBinary>,  // Per-binary scoping — only this rule applies if the caller binary matches
}

pub struct NetworkEndpoint {
    pub host: String,                       // Glob: "*.example.com"
    pub ports: Vec<u16>,
    pub protocol: Protocol,                 // Rest | Sql | L4
    pub tls: TlsHandling,                   // Terminate (MITM for L7 rules) | Passthrough (trusted endpoint)
    pub enforcement: EnforcementMode,        // Enforce | Audit
    pub access: AccessPreset,                // ReadOnly | ReadWrite | Full — expands to L7 rules
    pub rules: Vec<L7Rule>,                 // Explicit method+path allow rules
    pub allowed_ips: Vec<IpNet>,             // CIDR allowlist (SSRF mitigation beyond host-pattern filtering)
}

pub struct L7Rule {
    pub method: HttpMethod,                  // GET | POST | PUT | PATCH | DELETE | HEAD | OPTIONS
    pub path: String,                        // Wildcard path: "/v1/messages/batches/**"
}

pub struct NetworkBinary {
    pub path: PathBuf,                       // /usr/local/bin/claude
    pub tofu_sha256: Option<[u8; 32]>,       // Trust-on-first-use content hash (§6a.2)
}

pub enum EnforcementMode {
    Enforce,                                 // Rule decisions are binding
    Audit,                                   // Rule decisions are logged but not enforced — incremental rollout
}

pub enum TlsHandling {
    Terminate,                                // Proxy terminates TLS, inspects payload against L7 rules
    Passthrough,                              // Trusted endpoint; TLS is preserved end-to-end
}
```

**Deny-by-default filesystem policy.** The filesystem policy MUST declare `read_only` and `read_write` paths explicitly. The agent's home directory (`/sandbox`) is read-only; writable state is restricted to `/sandbox/.agent-data/` or an analogous subdirectory via symlinks. This prevents agents from tampering with their own runtime environment.

**Landlock rule-union gotcha** (must be in invariants): Landlock grants the union of all matching rules, not the intersection. The `include_workdir` setting must always be `false` — when `true`, OpenShell would auto-add WORKDIR to `read_write`, silently overriding any explicit `read_only` entry. All writable paths must be declared explicitly. This closes NemoClaw issue #804.

**Opt-in preset system.** Base sandbox policy ships with a **minimum** set of egress rules (inference provider only). Common developer tooling (GitHub, npm, brew, huggingface, slack, etc.) lives in **opt-in presets** under `policies/presets/*.yaml`. Even GitHub is not in the base — operators explicitly opt in during onboarding. This is deny-by-default taken seriously.

### 6a.1 Hot-Reload with Version Tracking

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.18 OpenShell.

Static fields (`filesystem`, `landlock`, `process`) are **locked at sandbox creation** and cannot be modified. Dynamic fields (`network_policies`, `inference`) can be hot-reloaded via a version-tracked poll loop:

```rust
pub enum PolicyStatus {
    Pending,                                 // Applied by gateway, not yet loaded by sandbox
    Loaded { version: u64, hash: [u8; 32] },  // Sandbox has loaded this version
    Failed { reason: String },
    Superseded { by_version: u64 },
}
```

Version numbers are monotonic. Policy changes are content-hashed (SHA-256) for integrity. Sandboxes poll `GetSandboxConfig` for updates; the gateway pushes via `UpdateConfig`.

### 6a.2 Binary SHA-256 Trust-On-First-Use

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.18 OpenShell.

Network policy rules can bind to specific binaries by content hash. The first time a binary makes an outbound call matching a rule, its SHA-256 content hash is recorded. Subsequent calls from the same path but a different hash are rejected — preventing agent process substitution attacks (e.g., an attacker replacing `/usr/local/bin/claude` with a malicious binary).

### 6a.3 AI-Assisted Policy Advisor

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.18 OpenShell. **Unique contribution — not present in any other research source.**

When a sandbox denies outbound traffic in `Audit` mode (§6a.0), the gateway aggregates denial events and proposes policy rules. A `PolicyDraftAdvisor` (optionally LLM-backed) generates candidate rules and surfaces them to operators in chunks:

```rust
pub async fn submit_policy_analysis(&self, denials: Vec<DenialEvent>) -> Result<DraftPolicy, AdvisorError>;
pub async fn approve_draft_chunk(&self, chunk_id: ChunkId, decision: ChunkDecision) -> Result<(), AdvisorError>;
```

Operators approve, reject, or edit individual chunks. Approved chunks are promoted to `Enforce` mode and added to the base policy. This turns audit-mode rollout into an iterative, explainable policy design loop.

### 6a.4 Sandbox Provider Trait

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

    /// Hot-reload the dynamic portion of the policy (network_policies, inference).
    async fn set_policy(&self, id: &SandboxId, policy: &SandboxPolicy) -> Result<PolicyStatus, SandboxError>;

    /// Destroy the sandbox. All state is lost.
    async fn destroy(&self, id: &SandboxId) -> Result<(), SandboxError>;

    /// Health check.
    async fn health(&self) -> HealthStatus;
}

pub struct SandboxConfig {
    pub profile: SandboxProfile,
    pub policy: SandboxPolicy,            // Three-layer policy per §6a.0
    pub timeout: Duration,
    pub memory_limit: Option<u64>,
    pub cpu_limit: Option<f64>,
    pub filesystem_mounts: Vec<Mount>,
    pub pinned_image_digest: Option<[u8; 32]>, // SHA-256 of the sandbox image — prevents registry compromise
    pub blueprint_digest: Option<[u8; 32]>,    // Mirrors the top-level blueprint digest for dual-field lockstep
}

pub enum SandboxProfile {
    Docker(DockerProfile),      // bollard — already in sera-docker
    Wasm,                       // wasmtime — shared with hook runtime
    MicroVm(MicroVmProfile),    // firecracker binary wrapped via tokio::process::Command
    External(String),           // External provider via gRPC/MCP
    OpenShell(OpenShellConfig), // Tier-3 enterprise backend (§6a.5)
}
```

**Pinned image digest dual-field lockstep.** The blueprint declares a top-level `digest: sha256:...` that MUST match `components.sandbox.image` digest. Release tooling rewrites both together; CI enforces lockstep. This blocks `:latest` force-push attacks and registry compromise per [SPEC-dependencies](SPEC-dependencies.md) §10.8 NemoClaw issue #1438.

### 6a.5 `OpenShellSandboxProvider` — Tier-3 Enterprise Backend

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.18 — published mTLS gRPC protocol at `github.com/NVIDIA/OpenShell`.

SERA ships an `OpenShellSandboxProvider` implementation that speaks the OpenShell gRPC protocol directly. Tier-3 deployments can use this as their sandbox backend without running SERA's native implementation.

```rust
pub struct OpenShellConfig {
    pub endpoint: String,                    // gRPC endpoint (typically port 8080 in-cluster, 30051 NodePort)
    pub ca_cert: PathBuf,
    pub client_cert: PathBuf,
    pub client_key: PathBuf,
}

pub struct OpenShellSandboxProvider {
    client: OpenShellClient,                 // Generated by tonic from proto/openshell.proto
}
```

The proto files (`proto/openshell.proto`, `proto/sandbox.proto`, `proto/datamodel.proto`) are Apache-2.0 and are vendored into `crates/sera-tools/openshell-proto/` at a pinned commit. `tonic-build` generates the Rust client stubs during `build.rs`.

**Native implementation mirrors OpenShell.** Even without the OpenShell backend, SERA's native sandbox implementation mirrors OpenShell's enforcement primitives: `regorus` in-process OPA for Rego policy evaluation (no external OPA sidecar), named `NetworkPolicyRule` hot-reload with version tracking, per-binary SHA-256 TOFU, `allowed_ips: [CIDR]` SSRF mitigation, `inference.local` virtual host routing, OCSF v1.7.0 audit events (see [SPEC-observability](SPEC-observability.md)). Operators running SERA without the OpenShell backend get equivalent security guarantees.

### 6a.6 `inference.local` Virtual Host Pattern

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.18 OpenShell.

All inference requests from inside a sandbox go to a **single virtual hostname** (`inference.local:443`). The sandbox's egress proxy intercepts, rewrites the `model` field in the JSON body, injects auth headers, and forwards to the resolved provider. Provider profiles (`openai`, `anthropic`, `nvidia`, `vllm`, `nim-local`) are configured via `GetInferenceBundle`:

```rust
pub struct ResolvedRoute {
    pub base_url: String,
    pub api_key: SecretRef,
    pub protocols: Vec<InferenceProtocol>,   // openai | anthropic | nim | ...
    pub model_id: String,
    pub provider_type: ProviderType,
    pub credential_env: Option<String>,      // Env var name the sandbox injects
    pub dynamic_endpoint: bool,              // If true, base_url is resolved per-call
    pub timeout_secs: u64,
}
```

This is cleaner than per-provider egress rules — one rule allows `inference.local`, and provider routing happens behind the proxy. It also means credential injection never flows through the sandbox filesystem.

### 6a.7 Pre-Execute Shell AST Analysis

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.7 opencode tree-sitter bash analysis.

Before executing any `bash` tool invocation, SERA parses the shell command via `tree-sitter-bash` and extracts the set of file paths the command will touch (via `BashArity` analysis of shell verbs: `rm`, `cp`, `mv`, `mkdir`, `touch`, `chmod`, `chown`, `cat`, etc.). Each path is checked against the filesystem policy **before** the shell process is spawned. This avoids needing OS-level sandboxing for most cases — untrusted paths are rejected at parse time, not at execution time.

The analysis runs as a `pre_execute` hook emitting a `ShellAudit` event to the gateway for policy evaluation.

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
| `sera-auth` | [SPEC-identity-authz](SPEC-identity-authz.md) | AuthZ for tool execution; `needs_approval` callback integration |
| `sera-secrets` | [SPEC-secrets](SPEC-secrets.md) | Credential resolution and injection; `GetInferenceBundle` credential flow never touches filesystem |
| `sera-hooks` | [SPEC-hooks](SPEC-hooks.md) | Pre/post tool hook chains; `pre_execute` shell AST analysis hook |
| `sera-hitl` | [SPEC-hitl-approval](SPEC-hitl-approval.md) | Approval gating for risky tools; inline `Tool.Context::ask()`; `CorrectedError` feedback |
| `sera-mcp` | [SPEC-interop](SPEC-interop.md) | MCP bridge for external tools; MCP tools register via `DynamicToolSpec` path |
| `sera-runtime` | [SPEC-runtime](SPEC-runtime.md) | Tool execution within turn loop; `tool_use_behavior` policy; `DynamicToolCallRequest { call_id, turn_id }` |
| `sera-docker` | existing Rust crate | Docker sandbox backend via `bollard` — already in the workspace |
| `sera-observability` | [SPEC-observability](SPEC-observability.md) | OCSF v1.7.0 audit events for all tool executions and sandbox denials |
| `sera-meta` | [SPEC-self-evolution](SPEC-self-evolution.md) | Tier 2 scope includes tool policy changes; constitutional gate checks policy modifications |
| Dependencies | [SPEC-dependencies](SPEC-dependencies.md) | §9.3 sandbox crate choices (`bollard`, `wasmtime`, `firecracker` binary wrap, `microsandbox` watch-only); §10.2 Codex three-layer sandbox + `DynamicToolSpec`; §10.7 opencode `Tool.Context::ask()` + `FileTime.withLock` + tree-sitter bash; §10.8 NemoClaw policy schema + presets + Sentry defense; §10.13 openai-agents-python `is_enabled`/`needs_approval`/`tool_use_behavior`; §10.16 BeeAI content-addressed cache; **§10.18 NVIDIA OpenShell full enforcement stack** |

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

# SPEC: Configuration Layer (`sera-config`)

> **Status:** DRAFT  
> **Source:** PRD §10.1, §10.2  
> **Crate:** `sera-config`  
> **Priority:** Phase 0  

---

## 1. Overview

The configuration layer is SERA's **declarative system definition**. It governs how every component behaves — from gateway settings to agent personas to hook chains. Configuration is:

- **Composable** — Kubernetes-style multi-file manifests, not one monolithic config file
- **Typed and versioned** — every config object has an `apiVersion` and `kind`
- **Layered** — defaults → file-discovered manifests → environment overrides → runtime changes
- **Agent-accessible** — principals (including agents) can read and propose config changes
- **Hot-reloadable** — ideally all config changes take effect without restart
- **Self-documenting** — bundled documentation is part of the config surface

---

## 2. Composable Config Model

Inspired by Kubernetes, SERA configuration uses **typed manifests** instead of a single monolithic YAML file. Each manifest describes one logical resource (an agent, a connector, a hook chain, a provider, etc.).

### 2.1 Manifest Structure

Every config manifest follows a uniform envelope:

```yaml
apiVersion: sera.dev/v1               # Schema version for this resource kind
kind: Agent                           # Resource type
metadata:
  name: "sera"                        # Unique name within kind
  labels:                             # Optional: arbitrary key-value labels
    tier: "local"
    team: "platform"
  annotations:                        # Optional: non-identifying metadata
    description: "Primary assistant"
spec:                                 # Kind-specific configuration
  provider: "lm-studio"
  model: "gemma-4-12b"
  persona:
    immutable_anchor: |
      You are Sera, an autonomous assistant.
    mutable_persona: |
      You prefer concise technical answers.
    mutable_token_budget: 300
  # ... (kind-specific fields)
```

```rust
pub struct ConfigManifest {
    pub api_version: ApiVersion,       // e.g., "sera.dev/v1"
    pub kind: ResourceKind,            // e.g., Agent, Connector, HookChain, Provider
    pub metadata: ResourceMetadata,
    pub spec: serde_json::Value,       // Validated against kind-specific schema
}

pub struct ResourceMetadata {
    pub name: String,
    pub labels: HashMap<String, String>,
    pub annotations: HashMap<String, String>,
}

pub struct ApiVersion {
    pub group: String,                 // "sera.dev"
    pub version: String,              // "v1", "v1beta1", etc.
}
```

### 2.2 Resource Kinds

| Kind | Description | Example File |
|---|---|---|
| `Instance` | Top-level instance config (tier, docs, ports) | `instance.yaml` |
| `Agent` | Agent definition (persona, model, tools, skills, memory, workflows) | `agents/sera.yaml` |
| `Provider` | Model provider config (endpoint, auth, default model) | `providers/lm-studio.yaml` |
| `Connector` | Channel connector (Discord, Slack, Telegram, etc.) | `connectors/discord-main.yaml` |
| `HookChain` | Named hook chain definition (pre_route, pre_tool, etc.) | `hooks/content-filter.yaml` |
| `ToolProfile` | Named tool allow/deny profile | `tools/profiles/coding.yaml` |
| `WorkflowDef` | Workflow definition (dreaming, knowledge-audit, etc.) | `workflows/dreaming.yaml` |
| `ApprovalPolicy` | Approval mode and escalation configuration | `policies/approval.yaml` |
| `SecretProvider` | Secret provider configuration | `secrets/vault.yaml` |
| `InteropConfig` | Protocol adapter config (MCP, A2A, ACP, AG-UI) | `interop/mcp.yaml` |

### 2.3 Directory-Based Discovery

SERA discovers config manifests by scanning a **config directory tree**. All `.yaml` and `.yml` files are loaded, parsed, and merged into the runtime config:

```
sera.d/                                # Config root (configurable via SERA_CONFIG_DIR)
├── instance.yaml                      # Kind: Instance
├── agents/
│   ├── sera.yaml                      # Kind: Agent
│   └── reviewer.yaml                  # Kind: Agent
├── providers/
│   ├── lm-studio.yaml                 # Kind: Provider
│   └── gemini-api.yaml                # Kind: Provider
├── connectors/
│   ├── discord-main.yaml              # Kind: Connector
│   └── telegram-alerts.yaml           # Kind: Connector
├── hooks/
│   ├── content-filter.yaml            # Kind: HookChain
│   └── pii-tokenizer.yaml            # Kind: HookChain
├── workflows/
│   └── dreaming.yaml                  # Kind: WorkflowDef
├── policies/
│   └── approval.yaml                  # Kind: ApprovalPolicy
└── overrides/                         # Runtime-applied overrides (auto-managed)
    └── runtime.yaml
```

**Rules:**
- The config root is set via `SERA_CONFIG_DIR` environment variable (default: `./sera.d`)
- All `.yaml`/`.yml` files in the tree are loaded recursively
- Directory structure is **advisory, not enforced** — manifests are identified by `kind`, not by directory
- Multiple manifests of the same kind share a namespace — names must be unique within kind
- Files can contain **one or multiple manifests** (separated by `---` YAML document separators)

### 2.4 Single-File Mode

For simple setups, SERA also supports a **single-file mode** where all manifests are in one file:

```yaml
# sera.yaml — all-in-one for simple local setups
---
apiVersion: sera.dev/v1
kind: Instance
metadata:
  name: "my-sera"
spec:
  tier: "local"
  docs_dir: "./docs"
---
apiVersion: sera.dev/v1
kind: Provider
metadata:
  name: "lm-studio"
spec:
  kind: "openai-compatible"
  base_url: "http://localhost:1234/v1"
  default_model: "gemma-4-12b"
---
apiVersion: sera.dev/v1
kind: Agent
metadata:
  name: "sera"
spec:
  provider: "lm-studio"
  model: "gemma-4-12b"
  persona:
    immutable_anchor: |
      You are Sera, an autonomous assistant.
  tools:
    profile: "basic"
    allow: ["memory_*", "session_*", "shell"]
---
apiVersion: sera.dev/v1
kind: Connector
metadata:
  name: "discord-main"
spec:
  kind: "discord"
  token: { secret: "connectors/discord-main/token" }
  agent: "sera"
```

A single file with `---` separators is equivalent to the directory tree. The system is agnostic to how manifests are laid out on disk.

---

## 3. Config Layering

Configuration is resolved in layers, with later layers overriding earlier ones:

| Layer | Source | Priority |
|---|---|---|
| 1. Defaults | Compiled-in defaults per resource kind | Lowest |
| 2. Manifests | Files discovered in `SERA_CONFIG_DIR` | |
| 3. Environment | `SERA_*` env vars | |
| 4. Runtime changes | Applied via config tools (agent or operator) | Highest |

### 3.1 Environment Variable Mapping

Environment variables override specific fields within manifests:

```
SERA_INSTANCE_TIER=team                     → Instance.spec.tier
SERA_AGENT_SERA_MODEL=gemma-4-26b          → Agent(name=sera).spec.model
SERA_PROVIDER_LM_STUDIO_BASE_URL=http://... → Provider(name=lm-studio).spec.base_url
```

Pattern: `SERA_{KIND}_{NAME}_{FIELD_PATH}` with nested fields separated by `__`.

### 3.2 Runtime Changes

Runtime config changes (via config tools or API) are:
- Persisted to `overrides/` directory as manifest files
- Applied on top of the base manifests
- Subject to approval policy (see §5)
- Each override records the proposing principal and timestamp

```yaml
# overrides/runtime.yaml (auto-generated)
---
apiVersion: sera.dev/v1
kind: Agent
metadata:
  name: "sera"
  annotations:
    sera.dev/proposed-by: "agent:sera"
    sera.dev/proposed-at: "2026-04-09T08:00:00Z"
    sera.dev/approved-by: "operator:admin"
spec:
  persona:
    mutable_persona: |
      You prefer concise technical answers. You favor Rust over Python.
```

### 3.3 Merge Semantics

When multiple manifests target the same resource (same `kind` + `metadata.name`):

1. **Scalar fields:** Last-writer wins (higher layer overrides lower)
2. **Lists:** Replace by default; use `sera.dev/merge-strategy: append` annotation for additive merge
3. **Maps:** Deep merge by default; keys in higher layers override keys in lower layers
4. **Null/omission:** A field absent in a higher layer does NOT override a lower layer's value

---

## 4. Schema Registry and Validation

Each resource kind has a registered JSON Schema. Config is validated at load time.

```rust
pub struct SchemaRegistry {
    schemas: HashMap<(ResourceKind, ApiVersion), schemars::Schema>,
}

impl SchemaRegistry {
    /// Validate a manifest against its registered schema.
    pub fn validate(&self, manifest: &ConfigManifest) -> Result<(), Vec<ValidationError>>;
    
    /// Get the schema for a resource kind at a specific API version.
    pub fn get_schema(&self, kind: &ResourceKind, version: &ApiVersion) -> Option<&schemars::Schema>;
    
    /// List all registered resource kinds with their supported versions.
    pub fn list_kinds(&self) -> Vec<(ResourceKind, Vec<ApiVersion>)>;
}
```

**Validation behavior:**
- Invalid manifests prevent startup with clear, actionable error messages
- Runtime config changes are validated before application
- Unknown `apiVersion` values are rejected with a message indicating supported versions
- Unknown `kind` values are rejected (forward-compatibility is handled via version negotiation, not silent acceptance)

---

## 5. Agent-Accessible Config

Principals (including agents) can **read their own configuration and propose changes**. This enables self-bootstrapping — an agent can help configure the system.

### 5.1 Config Tools

```rust
pub trait ConfigTools {
    /// Read current config (filtered to what this principal can see)
    async fn config_read(&self, path: &str) -> Result<ConfigValue, ConfigError>;

    /// Propose a config change (approval requirements depend on policy)
    async fn config_propose(&self, change: ConfigChange) -> Result<ConfigChangeResult, ConfigError>;

    /// Read bundled documentation (for agent self-help)
    async fn docs_read(&self, topic: &str) -> Result<DocContent, ConfigError>;
    
    /// List all resource kinds and their schemas (for agent config authoring)
    async fn config_schema(&self, kind: &ResourceKind) -> Result<SchemaInfo, ConfigError>;
}

pub struct ConfigChange {
    pub manifest: ConfigManifest,          // The proposed manifest (full or partial)
    pub reason: String,                    // Why this change is proposed
    pub merge_strategy: MergeStrategy,     // Replace | DeepMerge | Patch
}

pub enum ConfigChangeResult {
    Applied(ConfigVersion),               // Autonomous mode or auto-approved
    PendingApproval(ApprovalTicket),       // Requires approval
    Rejected(DenyReason),                  // Not authorized
}
```

### 5.2 Config Visibility

Principals see config **filtered by their authorization scope**:
- Agents see their own agent manifest, not other agents' manifests
- Operators see all agent manifests
- Admins see everything
- Secret values are **never** exposed — only secret references (`{ secret: "..." }`)

### 5.3 Self-Bootstrapping Flow

```
User: "add a Discord connector"
  → Agent reads bundled docs (docs_read("connectors/discord"))
  → Agent reads the Connector kind schema (config_schema(Connector))
  → Agent composes a Connector manifest
  → Agent proposes config change (config_propose)
  → Approval policy determines: auto-apply or require human approval
  → If approved: manifest written to config dir, connector starts
```

### 5.4 Pipeline Modification

An agent can modify its own context pipeline at runtime (add custom context steps, reorder steps, etc.) via `config_propose`, subject to authorization policy. This means an agent can self-optimize given sufficient permissions.

---

## 6. Hot-Reload

> [!IMPORTANT]  
> **Goal:** Ideally all config changes are hot-reloadable without process restart. If specific changes require restart, this should be documented as an engineering constraint during implementation.

### 6.1 Hot-Reload Mechanism

The config system watches the config directory tree (via `notify` / `inotify`) and:
1. Detects new, modified, or deleted manifest files
2. Re-parses and validates affected manifests
3. Computes a diff against the current runtime config
4. Notifies interested subsystems of changed resources via an internal event

### 6.2 Hot-Reload Candidates

| Resource Kind | Hot-Reloadable | Notes |
|---|---|---|
| `Agent` | ✅ Target | Next turn uses new config |
| `HookChain` | ✅ Target | WASM modules loaded/unloaded |
| `Connector` | ✅ Target | Start/stop connectors |
| `ToolProfile` | ✅ Target | Next tool resolution uses new config |
| `ApprovalPolicy` | ✅ Target | Immediate effect |
| `WorkflowDef` | ✅ Target | Next trigger uses new config |
| `Provider` | ⚠️ Partial | New connections use new config; existing in-flight calls complete with old |
| `Instance` (ports/bind) | ❌ Likely restart | Requires socket rebind |
| `Instance` (database) | ❌ Likely restart | Requires connection pool rebuild |

---

## 7. Bundled Documentation

SERA ships with **locally-accessible documentation** (bundled markdown in the workspace) so that agents can consume the docs to help users configure the system.

```yaml
apiVersion: sera.dev/v1
kind: Instance
metadata:
  name: "my-sera"
spec:
  docs_dir: "./docs"
```

The docs are accessible via the `docs_read` tool. An agent can read documentation about connectors, tools, hooks, etc., and use that knowledge to compose correct config manifests.

---

## 8. Config Version History

Every config change produces a versioned snapshot. This enables rollback, diff, and audit.

```rust
pub struct ConfigVersion {
    pub version: u64,                      // Monotonically increasing
    pub timestamp: DateTime<Utc>,
    pub changes: Vec<ConfigDelta>,        // What changed
    pub proposed_by: PrincipalRef,        // Who proposed
    pub approved_by: Option<PrincipalRef>, // Who approved (if HITL)
}

pub struct ConfigDelta {
    pub kind: ResourceKind,
    pub name: String,
    pub operation: DeltaOp,               // Created | Modified | Deleted
    pub diff: Option<String>,             // Human-readable diff
}

pub enum DeltaOp {
    Created,
    Modified,
    Deleted,
}
```

**Operations:**
- `config_history()` — list version history
- `config_diff(v1, v2)` — diff between two versions
- `config_rollback(version)` — propose a rollback to a previous version (subject to approval)

---

## 9. Cross-References

| Dependency | Spec | Relationship |
|---|---|---|
| `sera-secrets` | [SPEC-secrets](SPEC-secrets.md) | Secret references in config resolved by secret provider |
| `sera-auth` | [SPEC-identity-authz](SPEC-identity-authz.md) | Config visibility scoped by authorization |
| `sera-hitl` | [SPEC-hitl-approval](SPEC-hitl-approval.md) | Config changes may require approval |
| `sera-hooks` | [SPEC-hooks](SPEC-hooks.md) | Hook chain config as HookChain manifests |
| `sera-gateway` | [SPEC-gateway](SPEC-gateway.md) | Instance config (ports, connectors) |
| `sera-types` | [SPEC-crate-decomposition](SPEC-crate-decomposition.md) | ApiVersion, ResourceKind types defined in sera-types |

---

## 10. Open Questions

1. ~~**Config file format**~~ — Resolved: YAML with `---` document separators. TOML/JSON not needed.
2. ~~**Config versioning**~~ — Resolved: See §8. Version history with rollback support.
3. ~~**Multi-file config**~~ — Resolved: See §2.3. Directory-based discovery with recursive scanning.
4. **Config diff tool** — Should `config_diff` be a built-in CLI command or agent tool?
5. **Config migration tool** — How are manifest schemas migrated across SERA version upgrades? Auto-migration? Manual?
6. **Config validation strictness** — Should unknown fields in manifests be rejected (strict) or warned (lenient)?
7. **Config encryption at rest** — Should sensitive config values (beyond secrets) support encryption?
8. **Manifest ordering** — When multiple manifests have interdependencies (e.g., Agent references Provider), is load order validated?

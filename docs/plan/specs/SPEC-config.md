# SPEC: Configuration Layer (`sera-config`)

> **Status:** DRAFT
> **Source:** PRD §10.1, §10.2, plus deltas from [SPEC-dependencies](SPEC-dependencies.md) §8.5 (`figment` 0.10 for composable K8s-style YAML, `schemars` 0.8 + `jsonschema` 0.38, `notify` 8.2 for hot-reload), §10.9 (spec-kit frontmatter discipline), §10.18 (NVIDIA OpenShell 5 published JSON schemas for editor validation, AI-assisted policy advisor chunk approval workflow), [SPEC-self-evolution](SPEC-self-evolution.md) §5.4 (`change_artifact` provenance on every manifest, append-only `config_version_log`, shadow config store)
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
    /// Provenance: which Change Artifact produced this manifest revision.
    /// Source: SPEC-self-evolution §5.4.
    pub change_artifact: Option<ChangeArtifactId>,
    /// Whether the manifest is live or a shadow (dry-run replay target).
    pub shadow: bool,
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
| `InteropConfig` | Protocol adapter config (MCP, A2A, AG-UI — ACP merged into A2A, see SPEC-interop §5) | `interop/mcp.yaml` |
| `SandboxPolicy` | Sandbox filesystem / network policy (see SPEC-tools §6a.0) | `sandbox-policies/openclaw-sandbox.yaml` |
| `Circle` | Multi-agent Circle definition (see SPEC-circles §2) | `circles/engineering.yaml` |
| `ChangeArtifact` | Self-evolution proposal envelope (Tier-2/3, see SPEC-self-evolution §8) | `changes/2026-04-11-add-slack-connector.yaml` |

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

**Implementation** ([SPEC-dependencies](SPEC-dependencies.md) §8.5): schema **generation** via `schemars` 0.8 (derived from Rust types), schema **validation** via `jsonschema` 0.38. Manifest loading uses [`figment` 0.10](https://crates.io/crates/figment) for composable layer resolution (YAML files → environment → runtime overrides). Hot-reload file watching uses [`notify` 8.2](https://crates.io/crates/notify) — **not** 9.x (RC only as of this spec).

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

### 4.1 Published JSON Schemas Per Kind

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.18 NVIDIA OpenShell (which ships 5 canonical JSON schemas: `blueprint.schema.json`, `onboard-config.schema.json`, `openclaw-plugin.schema.json`, `policy-preset.schema.json`, `sandbox-policy.schema.json`).

SERA **must** ship a published JSON Schema per resource kind alongside the compiled binary. These enable:

1. **Editor integration** — VS Code, IntelliJ, and other YAML editors auto-complete, validate, and surface inline errors against the schemas
2. **Agent-authored config** — agents reading `config_schema(Connector)` get the canonical schema structure, enabling them to compose correct manifests without trial-and-error
3. **CI-time validation** — config repositories can lint manifests against the schemas before committing

Schemas are published to:

```
docs/schemas/
├── instance.schema.json
├── agent.schema.json
├── provider.schema.json
├── connector.schema.json
├── hook-chain.schema.json
├── tool-profile.schema.json
├── workflow-def.schema.json
├── approval-policy.schema.json
├── secret-provider.schema.json
├── interop-config.schema.json
├── sandbox-policy.schema.json
├── circle.schema.json
└── change-artifact.schema.json
```

Each schema is derived from the corresponding Rust type via `schemars::schema_for!(T)` and checked into git. CI enforces that generated schemas match the Rust types (breaks build on drift).

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

## 7a. Shadow Config Store (new)

> **Source:** [SPEC-self-evolution](SPEC-self-evolution.md) §5.4, §11.

The config system supports **shadow manifest loading** — apply a proposed config to an isolated shadow store without mutating the live store. This is the foundation for the shadow-session dry-run gate required on every Tier-2 Change Artifact.

```rust
#[async_trait]
pub trait ConfigStore: Send + Sync {
    async fn load(&self, manifest: ConfigManifest) -> Result<(), ConfigError>;
    async fn get(&self, kind: &ResourceKind, name: &str) -> Option<ConfigManifest>;
    async fn list(&self, kind: &ResourceKind) -> Vec<ConfigManifest>;
    /// Version tracking: monotonically increasing, signed on promotion to live.
    async fn version(&self) -> ConfigStoreVersion;
}

pub struct ShadowConfigStore {
    base: Arc<LiveConfigStore>,           // Points at current live state
    overlay: RwLock<HashMap<ManifestKey, ConfigManifest>>, // Shadow overrides
}
```

A `ShadowConfigStore` reads through to the live store by default but returns shadow overrides when present. Writes go only to the shadow overlay — the live store is never touched. When the dry-run completes successfully and the Change Artifact is promoted, the shadow overlay is **atomically merged** into the live store in one transaction.

**Usage flow:**

1. Change Artifact proposed with a `ConfigDelta`
2. Gateway creates a new `ShadowConfigStore` containing the delta
3. Gateway replays selected shadow sessions (see SPEC-runtime §3.3) against the shadow store
4. On successful replay, promote shadow → live via atomic merge
5. Emit `ConfigChangeApplied` event with the originating `ChangeArtifactId`
6. Rollback pointer retained for the change's rollback window

### 7b. Append-Only Config Version Log

> **Source:** [SPEC-self-evolution](SPEC-self-evolution.md) §5.4.

The config version history is maintained in a **separate append-only log** from the config store itself:

```rust
pub struct ConfigVersionLog {
    entries: Vec<ConfigVersionEntry>,     // Append-only
}

pub struct ConfigVersionEntry {
    pub version: u64,                      // Monotonic
    pub timestamp: DateTime<Utc>,
    pub change_artifact: ChangeArtifactId, // Back-reference to the Change Artifact
    pub signature: ConfigVersionSignature, // Signed by the gateway's config signing key
    pub prev_hash: [u8; 32],               // Cryptographic chain link
    pub this_hash: [u8; 32],               // Hash of entry content + prev_hash
}
```

The log is cryptographically chained (each entry's `prev_hash` is the previous entry's `this_hash`), append-only at the storage layer, and verified at boot. An attacker cannot silently rewrite history without breaking the chain.

---

## 8. Config Version History

Every config change produces a versioned snapshot. This enables rollback, diff, and audit. The canonical source of truth is the append-only `ConfigVersionLog` in §7b.

```rust
pub struct ConfigVersion {
    pub version: u64,                      // Monotonically increasing
    pub timestamp: DateTime<Utc>,
    pub changes: Vec<ConfigDelta>,        // What changed
    pub proposed_by: PrincipalRef,        // Who proposed
    pub approved_by: Option<PrincipalRef>, // Who approved (if HITL)
    pub change_artifact: ChangeArtifactId, // Back-reference to the SPEC-self-evolution §8 envelope
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
| `sera-auth` | [SPEC-identity-authz](SPEC-identity-authz.md) | Config visibility scoped by authorization; `MetaChange` capability gates `config_propose` |
| `sera-hitl` | [SPEC-hitl-approval](SPEC-hitl-approval.md) | Config changes may require approval; `ChangeArtifact` scope routing |
| `sera-hooks` | [SPEC-hooks](SPEC-hooks.md) | Hook chain config as HookChain manifests; `constitutional_gate` fires on `ChangeArtifact` proposals |
| `sera-gateway` | [SPEC-gateway](SPEC-gateway.md) | Instance config (ports, connectors); emits `ConfigChangeApplied` events on promotion |
| `sera-types` | [SPEC-crate-decomposition](SPEC-crate-decomposition.md) | ApiVersion, ResourceKind types defined in sera-types |
| `sera-meta` | [SPEC-self-evolution](SPEC-self-evolution.md) | `change_artifact` provenance (§2.1), shadow config store (§7a), append-only version log (§7b); SPEC-self-evolution §5.4 design obligations land in this spec |
| Dependencies | [SPEC-dependencies](SPEC-dependencies.md) | §8.5 `figment`/`schemars`/`jsonschema`/`notify` crate choices; §10.9 spec-kit frontmatter discipline; §10.18 OpenShell per-kind published JSON schemas |

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

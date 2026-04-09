# SPEC: Interface Versioning (`sera-types`)

> **Status:** DRAFT  
> **Source:** Research synthesis — cross-cutting concern  
> **Crate:** `sera-types` (version types), all crates (trait versioning)  
> **Priority:** Phase 0 (foundational)  

---

## 1. Overview

SERA is a platform with **multiple published interface surfaces** — traits, gRPC contracts, config schemas, hook APIs, and interop protocols. These surfaces evolve over time. A disciplined versioning strategy ensures:

1. **Backward compatibility** — existing integrations don't break on upgrade
2. **Capability detection** — components can negotiate what's supported
3. **Migration paths** — clear upgrade procedures when breaking changes are necessary
4. **Interoperability** — external adapters, hooks, and clients know what version they're talking to

---

## 2. Versioned Surfaces

SERA has six distinct versioned interface surfaces:

| Surface | What It Covers | Consumers | Versioning Mechanism |
|---|---|---|---|
| **Config Schemas** | Manifest `apiVersion` per resource kind | Operators, agents, tooling | `sera.dev/v1`, `sera.dev/v1beta1` |
| **gRPC Contracts** | Protobuf service definitions | External adapters, connectors, tools, runtimes, model providers | Proto package versioning |
| **Rust Traits** | Public trait interfaces (`MemoryBackend`, `Tool`, `SandboxProvider`, etc.) | Internal crates, custom backends | Crate semver |
| **Hook SDK** | WASM component model interface for hooks | Hook authors (Rust, Python, TS) | WIT package versioning |
| **Interop Protocols** | MCP, A2A, ACP, AG-UI protocol versions | External agents, clients | Protocol version negotiation |
| **CLI/API** | CLI commands, REST/WebSocket API | Client applications, scripts | API version header |

---

## 3. Config Schema Versioning

Config manifests carry an `apiVersion` that determines how the `spec` section is parsed and validated.

### 3.1 Version Format

```
apiVersion: sera.dev/{stability}{major}
```

| Stability | Meaning | Compatibility Guarantee |
|---|---|---|
| `v1`, `v2`, ... | **Stable** — production-ready, backward-compatible within major | Breaking changes only on major version bump |
| `v1beta1`, `v1beta2` | **Beta** — feature-complete but schema may change | May break between betas; upgrade path provided |
| `v1alpha1` | **Alpha** — experimental, may be removed entirely | No compatibility guarantee |

### 3.2 Version Negotiation

When loading a manifest:

1. Parser reads `apiVersion` from the manifest envelope
2. Schema registry looks up the registered schema for `(kind, apiVersion)`
3. If the version is unknown → **reject with error** listing supported versions
4. If the version is deprecated → **warn** but still load (with migration guidance)
5. If the version is removed → **reject** with migration instructions

### 3.3 Multi-Version Support

The config system can support **multiple versions of the same kind simultaneously**:

```rust
// Schema registry supports multiple versions per kind
pub fn register_schema(
    &mut self,
    kind: ResourceKind,
    version: ApiVersion,
    schema: schemars::Schema,
    converter: Option<Box<dyn VersionConverter>>,  // Converts from this version to latest
);
```

This enables gradual migration — operators can upgrade manifests one at a time. The system internally converts all versions to the latest representation for runtime use.

### 3.4 Version Converter

```rust
#[async_trait]
pub trait VersionConverter: Send + Sync {
    /// Convert a manifest spec from this version to the target (latest) version.
    fn convert(&self, spec: serde_json::Value) -> Result<serde_json::Value, ConversionError>;
    
    /// The target version this converter produces.
    fn target_version(&self) -> ApiVersion;
}
```

---

## 4. gRPC Contract Versioning

All gRPC contracts use **proto package versioning** with stability tiers.

### 4.1 Package Naming

```protobuf
// Stable
package sera.gateway.v1;
package sera.runtime.v1;
package sera.tools.v1;
package sera.models.v1;
package sera.secrets.v1;

// Beta
package sera.gateway.v1beta1;
```

### 4.2 Versioning Rules

| Rule | Description |
|---|---|
| **Additive-only in stable** | New fields, new RPCs, new enum values can be added. Existing fields/RPCs cannot be removed or have their types changed. |
| **Field numbering** | Proto field numbers are never reused. Deprecated fields are marked `reserved`. |
| **New major version** | When breaking changes are required, a new package version is created (`v2`). The old version continues to be served for a deprecation period. |
| **Deprecation** | Deprecated RPCs/fields are annotated with `[deprecated = true]` and a comment indicating the replacement. |
| **Removal** | Deprecated versions are removed only after a minimum deprecation period (2 minor SERA releases or 6 months, whichever is longer). |

### 4.3 Proto Directory Layout

```
proto/
├── sera/
│   ├── types/
│   │   └── v1/
│   │       └── types.proto              # Shared types (EventId, PrincipalRef, etc.)
│   ├── gateway/
│   │   └── v1/
│   │       └── channel_connector.proto
│   ├── runtime/
│   │   └── v1/
│   │       └── agent_runtime.proto
│   ├── tools/
│   │   └── v1/
│   │       └── tool_service.proto
│   ├── models/
│   │   └── v1/
│   │       └── model_provider.proto
│   └── secrets/
│       └── v1/
│           └── secret_provider.proto
```

### 4.4 Version Reporting

Every gRPC service includes a `GetVersion` RPC:

```protobuf
message VersionInfo {
    string sera_version = 1;           // e.g., "0.3.0"
    string api_version = 2;            // e.g., "v1"
    repeated string supported_versions = 3;  // e.g., ["v1", "v1beta1"]
    map<string, string> capabilities = 4;    // Feature flags
}

service ChannelConnector {
    rpc GetVersion(google.protobuf.Empty) returns (VersionInfo);
    // ... existing RPCs
}
```

---

## 5. Rust Trait Versioning

Internal Rust traits are versioned via **crate semver** following standard Rust conventions.

### 5.1 Semver Rules for SERA Crates

| Change Type | Version Bump | Example |
|---|---|---|
| New method with default impl | **Minor** | Adding `fn health(&self) -> HealthStatus { HealthStatus::Ok }` |
| New method without default | **Major** | Adding `fn required_method(&self)` |
| Method signature change | **Major** | Changing `fn search(&self, query: &str)` → `fn search(&self, query: &MemoryQuery)` |
| New type/enum variant | **Minor** (if non-exhaustive) | Adding `EventKind::Custom(String)` |
| Removing a public type | **Major** | Removing `ToolProfile::Minimal` |
| New optional field in struct | **Minor** | Adding `pub timeout: Option<Duration>` to a config struct |

### 5.2 Exhaustiveness Strategy

All public enums that are expected to grow should be marked `#[non_exhaustive]`:

```rust
#[non_exhaustive]
pub enum EventKind {
    Message,
    Heartbeat,
    Cron,
    Webhook,
    Hook,
    System,
    Approval,
    Workflow,
}

#[non_exhaustive]
pub enum RiskLevel {
    Read,
    Write,
    Execute,
    Admin,
}
```

This allows new variants to be added without breaking downstream match statements.

### 5.3 Trait Evolution Pattern

When a trait must change in a breaking way, use the **adapter pattern** to maintain backward compatibility:

```rust
// v1 trait (deprecated but still supported)
pub trait MemoryBackendV1: Send + Sync {
    async fn search(&self, query: &str) -> Result<Vec<MemoryResult>, MemoryError>;
}

// v2 trait (current)
pub trait MemoryBackend: Send + Sync {
    async fn search(&self, query: &MemoryQuery) -> Result<Vec<MemoryResult>, MemoryError>;
}

// Adapter: wraps V1 impl to satisfy V2 interface
pub struct V1Adapter<T: MemoryBackendV1>(T);

impl<T: MemoryBackendV1> MemoryBackend for V1Adapter<T> {
    async fn search(&self, query: &MemoryQuery) -> Result<Vec<MemoryResult>, MemoryError> {
        self.0.search(&query.text).await
    }
}
```

---

## 6. Hook SDK Versioning

Hooks communicate via the WASM Component Model. The interface is defined using **WIT (WebAssembly Interface Types)**.

### 6.1 WIT Package Versioning

```wit
// sera:hooks/hook@1.0.0
package sera:hooks@1.0.0;

interface hook-v1 {
    record hook-context { ... }
    record hook-result { ... }
    execute: func(ctx: hook-context) -> hook-result;
}
```

### 6.2 SDK Compatibility Matrix

| Hook SDK Version | Supported WIT Versions | SERA Versions |
|---|---|---|
| 0.1.x | `sera:hooks@1.0.0` | 0.1.x – 0.3.x |
| 0.2.x | `sera:hooks@1.0.0`, `sera:hooks@1.1.0` | 0.2.x – 0.5.x |

The hook runtime in SERA supports loading hooks compiled against any supported WIT version. Version negotiation happens at hook load time:

```rust
pub struct HookModule {
    pub wit_version: WitVersion,       // Declared by the hook module
    pub capabilities: HookCapabilities, // What the hook can do
}

// At load time:
// 1. Read the hook module's declared WIT version
// 2. Check if it's in the supported set
// 3. If yes → load and run
// 4. If no → reject with error listing supported versions
```

---

## 7. Interop Protocol Versioning

External protocols (MCP, A2A, ACP, AG-UI) have their own versioning schemes. SERA tracks compatibility.

### 7.1 Protocol Compatibility Registry

```rust
pub struct ProtocolSupport {
    pub protocol: ProtocolKind,
    pub supported_versions: Vec<String>,
    pub default_version: String,
    pub deprecated_versions: Vec<String>,
}

pub enum ProtocolKind {
    Mcp,
    A2A,
    Acp,
    AgUi,
}
```

### 7.2 Version Negotiation

For protocols that support version negotiation (e.g., MCP), SERA advertises its supported versions and negotiates the highest mutually supported version:

```yaml
apiVersion: sera.dev/v1
kind: InteropConfig
metadata:
  name: "mcp-server"
spec:
  protocol: "mcp"
  versions:
    supported: ["2025-11-25", "2024-10-07"]
    preferred: "2025-11-25"
```

---

## 8. CLI / REST API Versioning

### 8.1 API Version Header

All REST API requests include a version header:

```
GET /api/v1/agents HTTP/1.1
X-Sera-Api-Version: v1
```

The API version is embedded in the URL path (`/api/v1/...`). Multiple API versions can be served simultaneously.

### 8.2 CLI Version Reporting

```bash
$ sera version
sera 0.3.0
  api:       v1
  config:    sera.dev/v1
  protos:    sera.gateway.v1, sera.runtime.v1, sera.tools.v1
  hooks:     sera:hooks@1.0.0
  protocols: mcp/2025-11-25, a2a/v1, agui/v1
```

---

## 9. Capability Reporting

Every running SERA instance exposes a **capability manifest** that reports all supported interface versions:

```rust
pub struct CapabilityManifest {
    pub sera_version: String,              // SERA release version
    pub config_versions: HashMap<ResourceKind, Vec<ApiVersion>>,
    pub proto_versions: HashMap<String, Vec<String>>,
    pub hook_wit_versions: Vec<String>,
    pub protocol_support: Vec<ProtocolSupport>,
    pub api_version: String,
    pub features: Vec<String>,            // Enabled feature flags
}
```

This manifest is:
- Queryable via the `/status` health endpoint
- Available to agents via a `capabilities` tool
- Returned in the gRPC `GetVersion` response
- Used by clients for capability-based feature detection

---

## 10. Deprecation Policy

| Phase | Duration | Behavior |
|---|---|---|
| **Active** | Current | Fully supported, documented |
| **Deprecated** | Minimum 2 minor releases or 6 months | Functional but emits warnings. Migration guide published. |
| **Removed** | After deprecation period | Rejected with error pointing to migration guide |

### Deprecation Notification

When a deprecated version is used:

```
WARN [sera-config] Manifest "agents/sera.yaml" uses deprecated apiVersion "sera.dev/v1beta1". 
     Migrate to "sera.dev/v1". See: https://docs.sera.dev/migration/v1beta1-to-v1
```

---

## 11. Cross-References

| Dependency | Spec | Relationship |
|---|---|---|
| `sera-config` | [SPEC-config](SPEC-config.md) | Config schema versioning (apiVersion) |
| `sera-types` | [SPEC-crate-decomposition](SPEC-crate-decomposition.md) | Version types, ApiVersion, capability types defined here |
| `sera-hooks` | [SPEC-hooks](SPEC-hooks.md) | Hook WIT versioning |
| Protobuf contracts | [SPEC-crate-decomposition](SPEC-crate-decomposition.md) §5 | gRPC contract versioning |
| Interop | [SPEC-interop](SPEC-interop.md) | Protocol version negotiation |
| Gateway | [SPEC-gateway](SPEC-gateway.md) | Capability manifest served via health endpoint |

---

## 12. Open Questions

1. **Version lock file** — Should SERA generate a lock file recording the exact versions of all interface surfaces for reproducibility?
2. **Auto-migration CLI** — Should `sera migrate` be a built-in command that auto-converts deprecated config manifests?
3. **Proto backward compatibility testing** — Should CI enforce proto backward compatibility (e.g., `buf breaking`)?
4. **Minimum supported version** — How far back should SERA support deprecated versions? Fixed window or release-based?
5. **Hook binary compatibility** — Can hooks compiled against `sera:hooks@1.0.0` run on a SERA instance that only ships `sera:hooks@1.1.0`? (Likely yes with WIT subtyping, but needs validation.)

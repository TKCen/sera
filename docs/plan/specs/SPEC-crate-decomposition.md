# SPEC: Crate Decomposition

> **Status:** DRAFT  
> **Source:** PRD §12  
> **Priority:** Phase 0 (workspace setup)  

---

## 1. Overview

SERA is built as a **Rust workspace** — a monorepo of interconnected crates with clear dependency boundaries. The decomposition follows domain-driven design: foundation crates at the bottom, domain crates in the middle, runtime/gateway at the top, and clients on the periphery.

---

## 2. Workspace Layers

```
┌─────────────────────────────────────────────┐
│  Clients                                     │
│  sera-cli, sera-tui, sera-sdk                │
├─────────────────────────────────────────────┤
│  Gateway                                     │
│  sera-gateway                                │
├─────────────────────────────────────────────┤
│  Runtime                                     │
│  sera-runtime                                │
├─────────────────────────────────────────────┤
│  Interop                                     │
│  sera-mcp, sera-a2a, sera-acp, sera-agui     │
├─────────────────────────────────────────────┤
│  Core Domain                                 │
│  sera-session, sera-memory, sera-tools,      │
│  sera-hooks, sera-auth, sera-models,         │
│  sera-skills, sera-hitl, sera-workflow       │
├─────────────────────────────────────────────┤
│  Infrastructure                              │
│  sera-db, sera-queue, sera-cache,            │
│  sera-telemetry, sera-secrets                │
├─────────────────────────────────────────────┤
│  Foundation                                  │
│  sera-types, sera-config, sera-errors        │
├─────────────────────────────────────────────┤
│  Hook SDKs (separate publishable crates)     │
│  sera-hook-sdk, sera-hook-sdk-python,        │
│  sera-hook-sdk-ts                            │
└─────────────────────────────────────────────┘
```

---

## 3. Crate Catalog

### Foundation

| Crate | Purpose | Key Dependencies |
|---|---|---|
| `sera-types` | Shared domain types, IDs, Principal model, event model, protobuf definitions, `ApiVersion`, `ResourceKind`, `CapabilityManifest` | `prost`, `serde`, `uuid` |
| `sera-config` | Composable manifest loading, directory-based discovery, schema registry, validation, environment layering, hot-reload, agent-accessible config, bundled docs | `config`, `serde`, `schemars`, `notify` |
| `sera-errors` | Unified error types with error codes | `thiserror` |

### Infrastructure

| Crate | Purpose | Key Dependencies |
|---|---|---|
| `sera-db` | Database abstraction (PostgreSQL + SQLite), migrations | `sqlx`, `sea-query` |
| `sera-queue` | Lane-aware FIFO queue, global throttle, queue modes | `tokio` |
| `sera-cache` | Caching layer (Redis + in-memory) | `redis`, `moka` |
| `sera-telemetry` | OpenTelemetry tracing, metrics, structured logging | `tracing`, `opentelemetry` |
| `sera-secrets` | Secret provider trait + built-in providers (env, file, Vault, AWS SM, etc.) | `reqwest`, `tokio` |

### Core Domain

| Crate | Purpose | Key Dependencies |
|---|---|---|
| `sera-session` | Session state machine, transcript, compaction | `sera-db`, `sera-queue` |
| `sera-memory` | Memory trait + file-based default backend (with optional git) + LCM option | `sera-db`, `git2` |
| `sera-tools` | Tool registry, schema, profiles, execution, credential injection | `sera-types`, `sera-secrets` |
| `sera-hooks` | WASM runtime, chainable hook pipelines, fuel metering, per-instance config | `wasmtime` |
| `sera-auth` | AuthN (JWT, OIDC, SCIM), Principal registry, AuthZ trait, built-in RBAC, AuthZen client, SSF/CAEP/RISC | `jsonwebtoken`, `openidconnect` |
| `sera-models` | Model adapter trait + provider implementations | `reqwest` |
| `sera-skills` | Skill pack loading, AgentSkills compat, mode transitions | `sera-types` |
| `sera-hitl` | Approval routing, escalation chains, dynamic risk-based routing, approval state machine | `sera-types` |
| `sera-workflow` | Triggered workflow engine, cron scheduler, dreaming built-in workflow | `sera-types`, `cron` |

### Interop

| Crate | Purpose | Key Dependencies |
|---|---|---|
| `sera-mcp` | MCP server + client bridge | `mcp-sdk` |
| `sera-a2a` | A2A protocol adapter | `tonic` |
| `sera-acp` | ACP protocol adapter | `tonic` |
| `sera-agui` | AG-UI streaming protocol — full stream for SPAs + minimal stream for thin clients/HMIs | `axum`, `tokio` |

### Runtime

| Crate | Purpose | Key Dependencies |
|---|---|---|
| `sera-runtime` | Agent turn loop, KV-cache-optimized context pipeline, subagent management | All core domain |

### Gateway

| Crate | Purpose | Key Dependencies |
|---|---|---|
| `sera-gateway` | HTTP/WS/gRPC server, event routing, connector registry, plugin registry, secret management | `tonic`, `axum`, `tokio` |

### Clients

| Crate | Purpose | Key Dependencies |
|---|---|---|
| `sera-cli` | CLI client | `clap`, `sera-sdk` |
| `sera-tui` | Terminal UI | `ratatui`, `sera-sdk` |
| `sera-sdk` | Client SDK library | `tonic`, `tokio-tungstenite` |

### Hook SDKs (Separate Publishable)

| Crate | Purpose | Distribution |
|---|---|---|
| `sera-hook-sdk` | Rust hook authoring SDK | crates.io |
| `sera-hook-sdk-python` | Python hook authoring SDK | PyPI |
| `sera-hook-sdk-ts` | TypeScript hook authoring SDK | npm |

These are in the **same monorepo** but are **separately publishable** to their respective package registries.

---

## 4. Dependency Graph

```
sera-types ← sera-config, sera-errors
  ↑
sera-db, sera-queue, sera-cache, sera-telemetry, sera-secrets
  ↑
sera-session, sera-memory, sera-tools, sera-hooks, sera-auth,
sera-models, sera-skills, sera-hitl, sera-workflow
  ↑
sera-runtime
  ↑
sera-mcp, sera-a2a, sera-acp, sera-agui → sera-gateway
  ↑
sera-cli, sera-tui, sera-sdk
```

Key dependency rules:
- **Foundation crates** have no internal dependencies (only external)
- **Infrastructure crates** depend on foundation only
- **Core domain crates** depend on foundation + infrastructure
- **Runtime** depends on all core domain crates
- **Gateway** depends on runtime + interop
- **Clients** depend on SDK only (which depends on gateway protos)

---

## 5. Protobuf Contracts

All gRPC interfaces are defined as protobuf contracts using **package versioning** (see [SPEC-versioning](SPEC-versioning.md) §4).

| Proto Package | Service |
|---|---|
| `sera.gateway.v1` | `ChannelConnector` |
| `sera.runtime.v1` | `AgentRuntimeService` |
| `sera.tools.v1` | `ToolService` |
| `sera.models.v1` | `ModelProviderService` |
| `sera.secrets.v1` | `SecretProviderService` |
| `sera.types.v1` | Shared types (EventId, PrincipalRef, etc.) |

Proto files are organized by domain and version:

```
proto/
├── sera/
│   ├── types/v1/types.proto
│   ├── gateway/v1/channel_connector.proto
│   ├── runtime/v1/agent_runtime.proto
│   ├── tools/v1/tool_service.proto
│   ├── models/v1/model_provider.proto
│   └── secrets/v1/secret_provider.proto
```

Every gRPC service includes a `GetVersion` RPC returning `VersionInfo` (see [SPEC-versioning](SPEC-versioning.md) §4.4).

---

## 6. Build & Release

### 6.1 Workspace Build

```toml
# Cargo.toml (workspace root)
[workspace]
members = [
    "crates/sera-types",
    "crates/sera-config",
    "crates/sera-errors",
    "crates/sera-db",
    "crates/sera-queue",
    "crates/sera-cache",
    "crates/sera-telemetry",
    "crates/sera-secrets",
    "crates/sera-session",
    "crates/sera-memory",
    "crates/sera-tools",
    "crates/sera-hooks",
    "crates/sera-auth",
    "crates/sera-models",
    "crates/sera-skills",
    "crates/sera-hitl",
    "crates/sera-workflow",
    "crates/sera-mcp",
    "crates/sera-a2a",
    "crates/sera-acp",
    "crates/sera-agui",
    "crates/sera-runtime",
    "crates/sera-gateway",
    "crates/sera-cli",
    "crates/sera-tui",
    "crates/sera-sdk",
    "sdk/hooks/sera-hook-sdk",
]
```

### 6.2 Feature Flags

Optional enterprise features are behind cargo feature flags:

```toml
# sera-auth/Cargo.toml
[features]
default = ["jwt", "basic-auth"]
enterprise = ["oidc", "scim", "authzen", "ssf"]
```

```toml
# sera-secrets/Cargo.toml
[features]
default = ["env", "file"]
enterprise = ["vault", "aws-sm", "azure-kv", "gcp-sm"]
```

---

## 7. Cross-References

Every spec maps to one or more crates in this decomposition. See individual specs for crate-level details.

---

## 8. Open Questions

1. ~~**Protobuf versioning**~~ — Resolved: See [SPEC-versioning](SPEC-versioning.md) §4. Package-based versioning with stability tiers.
2. **Workspace layout** — `crates/` directory for all crates, or domain-grouped (e.g., `core/`, `infra/`, `interop/`)?
3. **Binary outputs** — How many binaries? Just `sera` (gateway + cli combined)? Or separate `sera-gateway` and `sera-cli` binaries?
4. **Hook SDK repo strategy** — Keep hook SDKs in-repo long-term, or split to separate repos once stable?
5. **Shared test utilities** — Should there be a `sera-test-utils` crate for shared test helpers, mock implementations?

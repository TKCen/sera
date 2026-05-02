# SPEC: Crate Decomposition

> **Status:** DRAFT
> **Source:** PRD §12, plus deltas from [SPEC-dependencies](SPEC-dependencies.md) §5, §8.3, §12, §16 (drop `sera-acp`, collapse `sera-queue` to `apalis`, add `sera-meta`, harden plugin SDK boundary)
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
│  sera-mcp, sera-a2a, sera-agui               │
├─────────────────────────────────────────────┤
│  Core Domain                                 │
│  sera-session, sera-memory, sera-tools,      │
│  sera-hooks, sera-auth, sera-models,         │
│  sera-skills, sera-hitl, sera-workflow,      │
│  sera-meta (Phase 4 impl; Phase 0–3 types)   │
├─────────────────────────────────────────────┤
│  Infrastructure                              │
│  sera-db, sera-queue (apalis adapter),       │
│  sera-cache, sera-telemetry, sera-secrets    │
├─────────────────────────────────────────────┤
│  Foundation                                  │
│  sera-types, sera-config, sera-errors,       │
│  sera-commands                               │
├─────────────────────────────────────────────┤
│  Plugin / Hook SDKs (separate publishable)   │
│  sera-plugin-sdk (hard boundary — plugins    │
│   may ONLY import from here, not from core)  │
│  sera-hook-sdk, sera-hook-sdk-python,        │
│  sera-hook-sdk-ts                            │
└─────────────────────────────────────────────┘
```

> **ACP dropped.** The `sera-acp` crate has been removed. The IBM/BeeAI Agent Communication Protocol was donated to the Linux Foundation and merged into A2A on 2025-08-25. See [SPEC-dependencies](SPEC-dependencies.md) §5 (Interop Protocol Adapters) and §10.16 (BeeAI Framework ACP→A2A migration playbook). SERA's ACP support is provided by the A2A adapter.

---

## 3. Crate Catalog

### Foundation

| Crate | Purpose | Key Dependencies |
|---|---|---|
| `sera-types` | Shared domain types, IDs, Principal model, event model, protobuf definitions, `ApiVersion`, `ResourceKind`, `CapabilityManifest` | `prost`, `serde`, `uuid` |
| `sera-config` | Composable manifest loading, directory-based discovery, schema registry, validation, environment layering, hot-reload, agent-accessible config, bundled docs | `config`, `serde`, `schemars`, `notify` |
| `sera-errors` | Unified error types with error codes | `thiserror` |
| `sera-commands` | **Shared command registry** — unified command definitions used by both CLI and gateway. Commands are registered once and dispatched from either entrypoint. Inspired by Hermes's `COMMAND_REGISTRY` pattern. | `sera-types`, `clap` |

### Infrastructure

| Crate | Purpose | Key Dependencies |
|---|---|---|
| `sera-db` | Database abstraction (PostgreSQL + SQLite), migrations | `sqlx` 0.8 (compile-time checked queries); **avoid `sea-orm`** — see [SPEC-dependencies](SPEC-dependencies.md) §8.2 |
| `sera-queue` | **Thin trait over `apalis` 0.7** + session-lane FIFO layer (lane-aware FIFO, global throttle, cron scheduling, orphan recovery) — replaces the hand-rolled queue in the original plan | `apalis`, `apalis-sql`, `tokio` — see [SPEC-dependencies](SPEC-dependencies.md) §8.3 |
| `sera-cache` | Caching layer (Redis + in-memory) | `moka` 0.12 (in-process), `redis` 0.27 or `fred` 9 (distributed) |
| `sera-telemetry` | OpenTelemetry tracing, metrics, structured logging, **OCSF v1.7.0 audit events** | `tracing`, `opentelemetry` 0.27 (pinned triad), `opentelemetry-otlp` 0.27, `tracing-opentelemetry` 0.28 — **must be pinned together** |
| `sera-secrets` | Secret provider trait + built-in providers (env, file, Vault, AWS SM, etc.) | `reqwest`, `tokio`, `vaultrs`, `aws-sdk-secretsmanager`, `azure_security_keyvault_secrets` |

### Core Domain

| Crate | Purpose | Key Dependencies |
|---|---|---|
| `sera-session` | Session state machine (6-state lifecycle per [SPEC-dependencies](SPEC-dependencies.md) §10.1 claw-code `WorkerStatus`), transcript (`ContentBlock` enum per §10.1), two-layer persistence (sqlx + shadow git per §10.7 opencode), compaction | `sera-db`, `sera-queue`, `git2` |
| `sera-memory` | Memory trait + **four-tier ABC** (`Unconstrained / Token / SlidingWindow / Summarize + ReadOnly` per [SPEC-dependencies](SPEC-dependencies.md) §10.16 BeeAI) + file-based default backend with optional git + LCM option + experience pool | `sera-db`, `git2`, `fastembed` |
| `sera-tools` | Tool registry, schema, profiles, execution, credential injection, **`SandboxProvider` trait** (Docker/WASM/MicroVM/External/**OpenShell**), `SsrfValidator`, per-binary SHA-256 TOFU identity | `sera-types`, `sera-secrets`, `bollard`, `wasmtime`, `regorus` (in-process OPA) |
| `sera-hooks` | WASM runtime, chainable hook pipelines, fuel metering, per-instance config, **`constitutional_gate` hook point (no `fail_open`)**, `updated_input` on `HookResult` per [SPEC-dependencies](SPEC-dependencies.md) §10.1 | `wasmtime` 43, `wasmtime-wasi`, `wasmtime-wasi-http` (allow-list via `WasiHttpView::send_request`) |
| `sera-auth` | AuthN (JWT, OIDC, SCIM), Principal registry, AuthZ trait, built-in RBAC, AuthZen client, SSF/CAEP/RISC, **capability tokens with narrowing**, **`MetaChange` / `CodeChange` / `MetaApprover` capabilities** per [SPEC-self-evolution](SPEC-self-evolution.md) §5.2 | `jsonwebtoken` 10, `openidconnect` 3.5, `oauth2` 5, `casbin` 2.19 |
| `sera-models` | Model adapter trait + provider implementations, **parser registry** (hermes/mistral/llama/qwen/deepseek/etc. per [SPEC-dependencies](SPEC-dependencies.md) §10.6), structured output via `llguidance` | `genai` (primary), `async-openai`, `llguidance`, `outlines-core`, `tiktoken`, `tokenizers` |
| `sera-skills` | Skill pack loading, **`AGENTS.md` + `SKILL.md` cross-tool standards** per [SPEC-dependencies](SPEC-dependencies.md) §10.11, three-tier microagent classification per §10.10, mode transitions, **self-patching skill loop** (agent can propose skill edits via `skill_manage patch`, validated and applied in a closed loop — Hermes pattern) | `sera-types` |
| `sera-hitl` | Approval routing, escalation chains, dynamic risk-based routing, approval state machine with `revision_requested` state, `CorrectedError { feedback }` tool-result variant per [SPEC-dependencies](SPEC-dependencies.md) §10.7, `SecurityAnalyzer` trait per §10.10 | `sera-types` |
| `sera-workflow` | Triggered workflow engine, cron scheduler (via `apalis`), **`WorkflowTask` modeled on beads `Issue` with atomic claim** per [SPEC-dependencies](SPEC-dependencies.md) §10.4 (promoted from Phase-3 to Phase-1 design input), dreaming built-in workflow, `meta_scope` field for self-evolution routing | `sera-types`, `apalis` |
| **`sera-meta`** (new) | **Self-evolution machinery** per [SPEC-self-evolution](SPEC-self-evolution.md). Change Artifact data model, blast-radius scope enum, constitutional anchor, shadow-session dry-run, two-generation live pattern, kill switch. **Phase 4 implementation; Phase 0–3 design-forward types** | `sera-types`, `sera-auth`, `sera-hooks`, `sera-config` |

### Interop

| Crate | Purpose | Key Dependencies |
|---|---|---|
| `sera-mcp` | MCP server + client bridge | **`rmcp` 1.3** with `server`/`client`/`macros` features + `schemars` — see [SPEC-dependencies](SPEC-dependencies.md) §5 |
| `sera-a2a` | A2A protocol adapter — generated from canonical `a2aproject/A2A` `specification/a2a.proto` at a pinned commit | `tonic`, `prost` — see [SPEC-dependencies](SPEC-dependencies.md) §5 |
| ~~`sera-acp`~~ | **DROPPED** — ACP merged into A2A under LF (2025-08-25). See [SPEC-dependencies](SPEC-dependencies.md) §5 + §10.16 for the migration playbook. Legacy clients use the A2A adapter. | — |
| `sera-agui` | AG-UI streaming protocol — full stream for SPAs + minimal stream for thin clients/HMIs. **Hand-rolled ~200-line serde enum set** over the 17 canonical event types from `ag-ui-protocol/ag-ui` at pinned commit | `axum::response::sse`, `async-stream`, `tokio-stream` — see [SPEC-dependencies](SPEC-dependencies.md) §5 |

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
sera-types ← sera-config, sera-errors, sera-commands
  ↑
sera-db, sera-queue, sera-cache, sera-telemetry, sera-secrets
  ↑
sera-session, sera-memory, sera-tools, sera-hooks, sera-auth,
sera-models, sera-skills, sera-hitl, sera-workflow, sera-meta
  ↑
sera-runtime
  ↑
sera-mcp, sera-a2a, sera-agui → sera-gateway
  ↑
sera-cli, sera-tui, sera-sdk
  ↑
sera-plugin-sdk (hard import boundary — plugins may only import
                 from sera-plugin-sdk, not from any core crate)
```

Key dependency rules:
- **Foundation crates** have no internal dependencies (only external)
- **Infrastructure crates** depend on foundation only
- **Core domain crates** depend on foundation + infrastructure
- **`sera-meta`** depends on `sera-types`, `sera-auth`, `sera-hooks`, `sera-config` (self-evolution machinery)
- **Runtime** depends on all core domain crates including `sera-meta`
- **Gateway** depends on runtime + interop
- **Clients** depend on SDK only (which depends on gateway protos)
- **Plugin SDK boundary:** external plugins may **only** import from `sera-plugin-sdk`, never from any `crates/sera-*` core crate. Enforced in CI via a static check per [SPEC-dependencies](SPEC-dependencies.md) §10.5 openclaw `AGENTS.md` import rule.

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
    "crates/sera-commands",
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
    "crates/sera-meta",
    "crates/sera-mcp",
    "crates/sera-a2a",
    "crates/sera-agui",
    "crates/sera-runtime",
    "crates/sera-gateway",
    "crates/sera-cli",
    "crates/sera-tui",
    "crates/sera-sdk",
    "sdk/plugin/sera-plugin-sdk",
    "sdk/hooks/sera-hook-sdk",
]
# NOTE: sera-acp was removed; ACP merged into A2A (LF, 2025-08-25).
# See SPEC-dependencies §5 and §10.16.
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

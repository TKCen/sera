# SPEC: External Dependencies & Reference Implementations

> **Status:** DRAFT
> **Source:** Ecosystem survey Apr 2026 (8 parallel research agents covering crates + 4 covering reference harnesses/coordination)
> **Priority:** Phase 0 (feeds all other specs)
> **Companion:** [SPEC-crate-decomposition](SPEC-crate-decomposition.md), [SPEC-versioning](SPEC-versioning.md)

---

## 1. Purpose

This spec is the canonical **buy-vs-build map** for the SERA Rust workspace. Every external crate, protocol schema, and reference implementation SERA takes a dependency on — or deliberately models its interfaces against — is listed here with:

- The exact version or source revision SERA pins against
- The license and its compatibility with SERA
- A classification: **DEPEND ON** (direct dependency), **VENDOR** (code generated or copied from a schema at a pinned revision), **MIRROR** (trait/interface shape replicated in SERA), **LEARN FROM** (architectural pattern only), **IGNORE** (wrong fit, explicitly rejected)
- A per-entry rationale and the risk notes operators / future maintainers must know

When an ecosystem choice in any other spec conflicts with this document, **this document wins** until it is explicitly updated. When this document changes, the affected specs must be updated in the same PR.

---

## 2. Scope & Non-Goals

**In scope:** Rust crates, protocol schemas, reference-implementation repositories, and architectural patterns SERA adopts from the ecosystem.

**Out of scope:**
- Cargo workspace layout — see [SPEC-crate-decomposition](SPEC-crate-decomposition.md)
- Versioning of SERA's own interfaces (Rust traits, protos, WIT) — see [SPEC-versioning](SPEC-versioning.md)
- Deployment topology and binary packaging — see [SPEC-deployment](SPEC-deployment.md)

---

## 3. Classification Keys

| Key | Meaning |
|---|---|
| **DEPEND ON** | Add to `[workspace.dependencies]` in the Cargo workspace at the pinned version. Updates require a SERA workspace bump. |
| **VENDOR** | Generate Rust code from a pinned upstream schema (proto / OpenAPI / JSON Schema) and check the generated file into the workspace, or copy a narrow set of types at a pinned commit. Avoids runtime dependency churn. |
| **MIRROR** | Replicate the interface shape (trait, struct, enum, message envelope) in SERA's own code. No dependency, no copied code — just shape alignment for future interoperability. |
| **LEARN FROM** | Read the upstream source as a design reference. No code, interfaces, or schemas taken. |
| **IGNORE** | Explicitly rejected. Document the reason so future contributors don't re-litigate. |

---

## 4. Version Pinning Policy

1. All entries in §5–§10 **must** include a concrete version range.
2. Crates whose public API has shipped ≥ 1.0 use tilde or caret ranges (e.g. `"^1.3"`).
3. Crates in active pre-1.0 development use exact-minor pinning (e.g. `"=0.8.0"` or `">=0.8, <0.9"`).
4. `wasmtime` is a special case: monthly major bumps, pinned as `">=43, <50"` and revisited quarterly.
5. The OpenTelemetry triad (`opentelemetry`, `opentelemetry-otlp`, `tracing-opentelemetry`) **must** be pinned together; version drift between the three produces compile-time trait bound errors.
6. Generated code from upstream schemas (VENDOR entries) pins to a commit SHA, not a tag.

---

## 5. Interop Protocol Adapters

Each row maps to a SERA crate under `crates/interop/`.

| Protocol | SERA crate | Classification | Upstream | License | Notes |
|---|---|---|---|---|---|
| **MCP** | `sera-mcp` | DEPEND ON | [`rmcp` ^1.3](https://crates.io/crates/rmcp) with features `["server", "client", "macros"]` + [`rmcp-macros`](https://crates.io/crates/rmcp-macros) | Apache-2.0 | Official Anthropic/MCP SDK. `#[tool]` macro + `schemars` feature derives JSON Schema from Rust types. Tracks spec 2025-11-25. Both `rig` and `goose` are migrating to it — de-facto standard. |
| **A2A** | `sera-a2a` | VENDOR (proto) + MIRROR (transport) | [`a2aproject/A2A`](https://github.com/a2aproject/A2A) — `specification/a2a.proto` at pinned commit | Apache-2.0 | No official Rust SDK. Generate types with `prost` + `tonic` from the canonical `.proto`; reference [`a2a-rs`](https://crates.io/crates/a2a-rs) v0.2 for transport wiring patterns only. |
| **ACP** | ~~`sera-acp`~~ | IGNORE — **DROP THE CRATE** | [`i-am-bee/acp`](https://github.com/i-am-bee/acp) | Apache-2.0 | **ACP merged into A2A under the Linux Foundation on 2025-08-25.** Do not build a separate ACP adapter. Existing ACP deployments use the A2A-ACP bridge. Remove `sera-acp` from the crate decomposition and update `SPEC-interop.md`. |
| **AG-UI** | `sera-agui` | VENDOR (hand-write from schema) | [`ag-ui-protocol/ag-ui`](https://github.com/ag-ui-protocol/ag-ui) at pinned commit | MIT | No production-grade Rust SDK. Hand-roll the 17 event types as `serde` enums (~200 LoC) against the canonical TypeScript/proto definitions. Community crate [`ag-ui-client`](https://crates.io/crates/ag-ui-client) is too immature to depend on. |

**SSE transport for AG-UI**: built-in `axum::response::sse` + [`async-stream`](https://crates.io/crates/async-stream) + [`tokio-stream`](https://crates.io/crates/tokio-stream). No separate SSE crate.

**Events (initial 10 of 17 for the MVS subset):** `RUN_STARTED`, `RUN_FINISHED`, `RUN_ERROR`, `TEXT_MESSAGE_START`, `TEXT_MESSAGE_CONTENT`, `TEXT_MESSAGE_END`, `TOOL_CALL_START`, `TOOL_CALL_ARGS`, `TOOL_CALL_END`, `STATE_SNAPSHOT`. The remaining seven (`STEP_*`, `TOOL_CALL_RESULT`, `STATE_DELTA`, `MESSAGES_SNAPSHOT`, `RAW`, `CUSTOM`) are post-MVS.

---

## 6. WASM Hook Runtime

| Component | Classification | Pin | License | Notes |
|---|---|---|---|---|
| [`wasmtime`](https://crates.io/crates/wasmtime) | DEPEND ON | `">=43, <50"` | Apache-2.0 | Monthly major cadence — loose range, reviewed quarterly. |
| [`wasmtime-wasi`](https://crates.io/crates/wasmtime-wasi) | DEPEND ON | Matches `wasmtime` | Apache-2.0 | WASI Preview 2 host. |
| [`wasmtime-wasi-http`](https://crates.io/crates/wasmtime-wasi-http) | DEPEND ON | Matches `wasmtime` | Apache-2.0 | **This is the canonical outbound-network mechanism for hooks.** Allow-list is enforced in `WasiHttpView::send_request`; hooks cannot open raw sockets. |
| [`wit-bindgen`](https://crates.io/crates/wit-bindgen) | DEPEND ON | `^0.x` (latest stable) | Apache-2.0 | Host-side WIT bindings. |
| [`cargo-component`](https://github.com/bytecodealliance/cargo-component) | DEV TOOL | Latest | Apache-2.0 | Rust guest build toolchain — production-ready. |
| [`componentize-py`](https://github.com/bytecodealliance/componentize-py) | DEV TOOL | Latest | Apache-2.0 | Python guest toolchain — functional with rough edges. |
| [`jco` / `componentize-js`](https://github.com/bytecodealliance/jco) | DEV TOOL | Latest | Apache-2.0 | TypeScript guest toolchain — **explicitly experimental**. Breaking changes expected. Plan for stabilization work if TS hooks are an MVP requirement. |
| [`notify`](https://crates.io/crates/notify) | DEPEND ON | `^8.2` | CC0-1.0 / MIT / Apache-2.0 | Hook hot-reload. **Do not adopt 9.x RC until it ships stable.** |
| [`extism`](https://github.com/extism/extism) | **IGNORE** | — | BSD-3 | Rejected. Extism is pinned to `wasmtime < 31`, uses a non-WIT proprietary PDK ABI, and does not expose `WasiHttpView::send_request` allow-listing. Useful only as a reference for plugin UX patterns. |

### 6.1 Host API proxying (SERA's security contract)

Hooks call `wasi:http/outgoing-handler.handle(...)`. The host implements `WasiHttpView::send_request`, inspects the target URL against SERA's per-chain allow-list, and either forwards the request via SERA's egress proxy or returns `HttpError::Forbidden`. This is the only outbound-network path available to hooks. Raw socket access is never granted.

### 6.2 Known runtime caveats

- **Fuel metering is bytecode-only.** Hooks blocked in a host call are not subject to fuel; the host must wrap host functions with `tokio::time::timeout`.
- **`Store<T>` is `!Send`.** Thread-pool dispatch requires either a per-thread store pool or `Arc<Mutex<Store>>`.
- **Fuel is counted per basic block**, not per instruction ([wasmtime#4109](https://github.com/bytecodealliance/wasmtime/issues/4109)). Slight overshoot is possible. Not a security hole; affects precision only.

---

## 7. Identity & Authorization Stack

Each row maps to a feature or module inside `sera-auth`.

| Layer | Crate | Pin | License | Classification | Write-yourself gap |
|---|---|---|---|---|---|
| JWT | [`jsonwebtoken`](https://crates.io/crates/jsonwebtoken) | `^10.3` | MIT | DEPEND ON | API key lookup is a separate bearer-token middleware against a KV store. |
| OAuth2 | [`oauth2`](https://crates.io/crates/oauth2) | `^5.0` (RC, stable API) | MIT / Apache-2.0 | DEPEND ON | None for client flows. |
| OIDC | [`openidconnect`](https://crates.io/crates/openidconnect) | `^3.5` | MIT / Apache-2.0 | DEPEND ON | **Background JWKS refresh is not built-in.** SERA must spawn a `tokio::task` that re-fetches discovery + JWKS on a TTL matching the IdP cache-control header, and rebuild the verifier atomically. |
| SCIM | [`scim-server`](https://crates.io/crates/scim-server) + [`scim_v2`](https://crates.io/crates/scim_v2) | `^0.5` / `^0.x` | Apache-2.0 | DEPEND ON (scaffolding) | Complex PATCH filter expressions and Principal↔SCIM schema mapping require custom code on top of these crates. Treat as acceleration, not a complete solution. |
| AuthZen PDP | **none exists** | — | — | MIRROR | Write a ~60-line `reqwest` wrapper over `POST /access/v1/evaluation`. Track the [OpenID AuthZen final spec](https://openid.github.io/authzen/). |
| RBAC fallback | [`casbin`](https://crates.io/crates/casbin) | `^2.19` | Apache-2.0 | DEPEND ON | Domain-tenanted RBAC via PERM model; avoids hand-rolling ABAC. Avoid `oso` (pivoted to SaaS). |
| SSF/CAEP/RISC | **none exists** | — | — | MIRROR | The three specs reached final status Sept 2025 and have zero Rust implementations. Write the SET ingester (~300 LoC) on top of `jsonwebtoken` for JWS verification. |
| Passwords | [`argon2`](https://crates.io/crates/argon2) (RustCrypto) | `^0.5` | MIT / Apache-2.0 | DEPEND ON | Prefer over the older `rust-argon2`. Local admin accounts only. |
| Session cookies | [`axum-login`](https://crates.io/crates/axum-login) + [`tower-sessions`](https://crates.io/crates/tower-sessions) | `^0.16` / `^0.13` | MIT | DEPEND ON | Backed by a Redis or Postgres store via the respective session store adapters. |
| Shortcut: Ory Kratos sidecar | [`ory-kratos-client`](https://crates.io/crates/ory-kratos-client) | Latest | Apache-2.0 | LEARN FROM | Running Kratos as an out-of-process sidecar is the fastest path to complete OIDC + MFA + social-login if SERA wants to defer writing the enterprise auth layer. Verify sessions via the Kratos Admin API. |

---

## 8. Core Infrastructure

### 8.1 Runtime, HTTP, gRPC, Protobuf

| Crate | Pin | License | Notes |
|---|---|---|---|
| [`tokio`](https://crates.io/crates/tokio) | `^1.49` | MIT | Uncontested. `smol`/`async-std` do not wire to the rest of this stack. |
| [`axum`](https://crates.io/crates/axum) | `^0.8` | MIT | Requires `tower ^0.5`; incompatible with axum 0.7 middleware. |
| [`tower`](https://crates.io/crates/tower) | `^0.5` | MIT | |
| [`tower-http`](https://crates.io/crates/tower-http) | `^0.6` | MIT | `TraceLayer`, `CorsLayer`, compression. |
| [`tonic`](https://crates.io/crates/tonic) | `^0.13` | MIT | Requires `prost ^0.13` — do not mix majors. |
| [`tonic-build`](https://crates.io/crates/tonic-build) | `^0.13` | MIT | `build.rs` codegen. |
| [`tonic-reflection`](https://crates.io/crates/tonic-reflection) | `^0.13` | MIT | gRPC reflection for `grpcurl`/`evans`. |
| [`tonic-health`](https://crates.io/crates/tonic-health) | `^0.13` | MIT | Standard gRPC health protocol. |
| [`prost`](https://crates.io/crates/prost) | `^0.13` | Apache-2.0 | |

### 8.2 Database

| Crate | Pin | License | Classification | Notes |
|---|---|---|---|---|
| [`sqlx`](https://crates.io/crates/sqlx) | `^0.8` | MIT / Apache-2.0 | DEPEND ON | Compile-time checked queries, SQLite + Postgres, async-native. The primary DB layer. |
| [`sqlx-cli`](https://crates.io/crates/sqlx-cli) | `^0.8` | MIT / Apache-2.0 | DEV TOOL | Migrations. |
| [`sea-orm`](https://crates.io/crates/sea-orm) | — | MIT | **IGNORE** | ORM abstraction adds overhead SERA doesn't need. Wraps sqlx internally. |
| [`sea-query`](https://crates.io/crates/sea-query) | `^0.32` | MIT | LEARN FROM | Useful as reference for dynamic query building; not a default dependency. |

### 8.3 Queue / Scheduler

| Crate | Pin | License | Classification | Notes |
|---|---|---|---|---|
| [`apalis`](https://crates.io/crates/apalis) | `^0.7` | MIT | **DEPEND ON** | **Replaces the hand-rolled `sera-queue` crate in the original plan.** Tower-based worker pipeline, SQLite/Postgres/Redis backends, cron scheduling, retries, orphan recovery, heartbeat — all in one. SERA's `sera-queue` becomes a thin trait over apalis plus a session-lane FIFO layer. |
| [`apalis-sql`](https://crates.io/crates/apalis-sql) | `^0.7` | MIT | DEPEND ON | SQLite + Postgres storage, polling and LISTEN/NOTIFY modes. |
| [`tokio-cron-scheduler`](https://crates.io/crates/tokio-cron-scheduler) | `^0.15` | MIT / Apache-2.0 | DEPEND ON (fallback) | Use only if apalis is not chosen for a given SERA component. |
| [`sqlxmq`](https://crates.io/crates/sqlxmq) | — | — | IGNORE | Unmaintained. |
| [`pgmq`](https://github.com/tembo-io/pgmq) | — | PostgreSQL | LEARN FROM | Requires a pgrx extension installed in the DB — operational overhead. Revisit if SERA adopts a Postgres-only tier. |

### 8.4 Cache, Secrets, Observability

| Crate | Pin | License | Classification | Notes |
|---|---|---|---|---|
| [`moka`](https://crates.io/crates/moka) | `^0.12` | MIT / Apache-2.0 | DEPEND ON | In-process, async-aware cache. |
| [`redis`](https://crates.io/crates/redis) | `^0.27` | BSD-3 | DEPEND ON | Distributed cache. `fred ^9` is a viable alternative if connection-pool ergonomics bite. |
| [`vaultrs`](https://crates.io/crates/vaultrs) | `^0.7` | MIT | DEPEND ON | Community-maintained Vault client. Not official HashiCorp. |
| [`aws-sdk-secretsmanager`](https://crates.io/crates/aws-sdk-secretsmanager) | `^1` | Apache-2.0 | DEPEND ON | Official AWS SDK. |
| [`azure_security_keyvault_secrets`](https://crates.io/crates/azure_security_keyvault_secrets) | `^0.21` | MIT | DEPEND ON | Official azure-sdk-for-rust workspace. |
| `google-secretmanager1` | — | MIT | IGNORE | `gcloud-sdk` or raw REST via `reqwest` is preferred; the generated client has poor ergonomics. |
| [`tracing`](https://crates.io/crates/tracing) | `^0.1` | MIT | DEPEND ON | |
| [`tracing-subscriber`](https://crates.io/crates/tracing-subscriber) | `^0.3` | MIT | DEPEND ON | `EnvFilter` + `fmt`. |
| [`opentelemetry`](https://crates.io/crates/opentelemetry) | `=0.27` | Apache-2.0 | DEPEND ON (locked) | **Pin exactly with `-otlp` and `tracing-opentelemetry`.** Global tracer provider API changed in 0.26/0.27. |
| [`opentelemetry-otlp`](https://crates.io/crates/opentelemetry-otlp) | `=0.27` | Apache-2.0 | DEPEND ON (locked) | |
| [`tracing-opentelemetry`](https://crates.io/crates/tracing-opentelemetry) | `=0.28` | MIT | DEPEND ON (locked) | Lags `opentelemetry` by one minor — check compatibility before bumping any of the three. |

### 8.5 Config, Schema, File Watching, Validation

| Crate | Pin | License | Classification | Notes |
|---|---|---|---|---|
| [`figment`](https://crates.io/crates/figment) | `^0.10` | MIT / Apache-2.0 | DEPEND ON | Best fit for K8s-style composable YAML manifests. |
| [`schemars`](https://crates.io/crates/schemars) | `^0.8` | MIT / Apache-2.0 | DEPEND ON | JSON Schema generation from Rust types. 1.0 alpha exists; 0.8 is the stable line. |
| [`jsonschema`](https://crates.io/crates/jsonschema) | `^0.38` | MIT | DEPEND ON | High-performance JSON Schema validator. Complements `schemars`. |
| [`config`](https://crates.io/crates/config) | — | MIT / Apache-2.0 | IGNORE | Bumpy migration history; YAML behind a feature flag. |

---

## 9. LLM Adapters, Tools, Sandboxes

### 9.1 LLM client layer (`sera-models`)

| Crate | Pin | License | Classification | Notes |
|---|---|---|---|---|
| [`genai`](https://crates.io/crates/genai) | Latest | MIT | **DEPEND ON (primary)** | Unified native API across OpenAI, Anthropic, Gemini, Ollama, DeepSeek, xAI, Groq, Cohere. Not shims — native per-provider protocols. Streaming, reasoning/thinking controls, image/PDF/embedding. Actively maintained. |
| [`async-openai`](https://crates.io/crates/async-openai) | `^0.28` | MIT | DEPEND ON (secondary) | Use when deep OpenAI-specific features are needed (assistants, fine-tuning), or to front LM Studio / llama.cpp / Ollama via custom base URL. |
| [`rig`](https://crates.io/crates/rig-core) | — | MIT | IGNORE as dependency, MIRROR trait bounds | `CompletionModel` + `WasmCompatSend + WasmCompatSync` trait bound pattern is worth replicating in SERA's traits from day one. Do not depend on the crate. |
| `langchain-rust`, `llm-chain` | — | — | IGNORE | Stale or redundant. |
| Anthropic official Rust SDK | — | — | IGNORE | Does not exist. `genai` covers the Anthropic path natively. |

### 9.2 Structured output, embeddings, tokens

| Crate | Pin | License | Classification | Notes |
|---|---|---|---|---|
| [`llguidance`](https://crates.io/crates/llguidance) | Latest | MIT | DEPEND ON | Microsoft; CFG + JSON schema → token bitmasks; ~50 µs/token; lazy automata. Primary constrained-generation path for local inference servers. |
| [`outlines-core`](https://github.com/dottxt-ai/outlines-core) | Latest | Apache-2.0 | DEPEND ON (complement) | Schema → FSM index. Avoid for recursive schemas or large enums (compile-time explosions reported). |
| [`fastembed`](https://crates.io/crates/fastembed) | `^5.12` | Apache-2.0 | DEPEND ON | Local ONNX-based embeddings. Actively releasing. |
| [`tiktoken`](https://github.com/anysphere/tiktoken-rs) (pure-Rust anysphere port) | Latest | MIT | DEPEND ON | Multi-provider token counting: OpenAI cl100k/o200k, Llama 3, DeepSeek v3, Qwen2, Mistral. |
| [`tokenizers`](https://crates.io/crates/tokenizers) | Latest | Apache-2.0 | DEPEND ON (local models) | HuggingFace tokenizers for arbitrary local models loaded via `fastembed`. |

### 9.3 Sandboxes (`sera-tools` SandboxProvider implementations)

| Backend | Crate / Tool | Classification | Notes |
|---|---|---|---|
| Docker | [`bollard`](https://crates.io/crates/bollard) `^0.19` | DEPEND ON | **Already in `sera-docker`.** Full async Docker API, Windows named pipes, rustls. |
| WASM | [`wasmtime`](https://crates.io/crates/wasmtime) (shared with hooks) | DEPEND ON | |
| MicroVM | `firecracker` binary via `tokio::process::Command` | MIRROR | Firecracker's own Rust SDK is thin; prefer wrapping the binary. |
| Linux process isolation | `bwrap` / `nsjail` / `landlock` via `Command` | MIRROR | No mature Rust wrappers; `Command` invocation is the right path. |
| MicroVM (async, OCI) | [`microsandbox`](https://crates.io/crates/microsandbox) | **WATCH** | YC-backed libkrun wrapper; currently 0.3 with breaking changes expected. Revisit at 1.0. |

SERA's own `SandboxProvider` trait is the right abstraction layer — no mature "sandbox abstraction" crate exists in the Rust ecosystem.

---

## 10. Harness & Coordination Reference Implementations

These four projects are **not dependencies** — they are reference implementations whose interfaces and architectural decisions SERA adopts directly. Each entry specifies what to MIRROR, LEARN FROM, or IGNORE, with a file-level citation so future maintainers can re-verify.

### 10.1 `ultraworkers/claw-code` — Rust harness

**Source:** [github.com/ultraworkers/claw-code](https://github.com/ultraworkers/claw-code) — Rust workspace, 9 crates, ~48K LoC.
**Relationship:** Rust reimplementation of the `claw` CLI agent harness. Not affiliated with `openclaw` / `zeroclaw` / `lossless-claw`.
**License:** MIT. SERA's use is clean-room reimplementation from observed interfaces (MIRROR only, no code copy), so license terms are not a blocker even if they were restrictive.

| What | Classification | Evidence | Spec to update |
|---|---|---|---|
| `ContentBlock` enum — `Text`, `ToolUse { id, name, input }`, `ToolResult { tool_use_id, tool_name, output, error }` | **MIRROR** | `session.rs` | SPEC-runtime §4 (context assembly) |
| Atomic JSONL append with rollback on I/O failure | MIRROR | `session.rs::push_message` | SPEC-gateway §6 (session persistence) |
| `WorkerStatus` 6-state lifecycle: `Spawning → TrustRequired → ReadyForPrompt → Running → Finished \| Failed` | **MIRROR** | `worker_boot.rs` | SPEC-gateway §6 (session state machine) |
| `PromptMisdelivery` + `PromptReplayArmed` replay mechanism | MIRROR | `worker_boot.rs` | SPEC-gateway §4 (event routing) — **new:** gateway-side prompt replay on harness miss |
| Hook `updated_input` — hooks can rewrite tool input before execution, not just observe | **MIRROR** | `hooks.rs::HookRunResult` | SPEC-hooks §2.2 (`HookResult` extension) |
| `HookAbortSignal` thread-safe cancellation primitive | MIRROR | `hooks.rs` | SPEC-hooks §5 |
| `LaneFailureClass` typed failure taxonomy (`PromptDelivery`, `TrustGate`, `BranchDivergence`, `Compile`, `Test`, `PluginStartup`, `McpStartup`, `McpHandshake`, `GatewayRouting`, `ToolRuntime`, `WorkspaceMismatch`, `Infra`) | **MIRROR** | `lane_events.rs` | SPEC-observability §3 + SPEC-gateway §12 |
| `LaneCommitProvenance { commit, branch, worktree, canonical_commit, superseded_by, lineage }` — subagent result provenance chain | MIRROR | `lane_events.rs` | SPEC-circles §4 (result aggregation) |
| Compaction ToolUse/ToolResult pairing invariant — never split a `ToolUse` from its matching `ToolResult` | **MIRROR (invariant)** | `compact.rs` | SPEC-runtime §5 (compaction) |
| Synthetic continuation message after compaction — inform the LLM about the summarization | LEARN FROM | `compact.rs` | SPEC-runtime §5 |
| `ValidatedPacket` newtype — a subagent does not receive a raw `TaskPacket`, only a `ValidatedPacket` constructed after `validate()` passes | **MIRROR** | `task_packet.rs` | SPEC-circles §3 (task delegation) |
| `TaskPacket` 8 fields: `objective`, `scope`, `repo`, `branch_policy`, `acceptance_tests`, `commit_policy`, `reporting_contract`, `escalation_policy` | MIRROR | `task_packet.rs` | SPEC-circles §3 |
| 11-phase `McpLifecyclePhase` enum with validated transitions | MIRROR | `mcp_lifecycle_hardened.rs` | SPEC-interop §3 (MCP bridge lifecycle) |
| `McpDegradedReport { working_servers, failed_servers, available_tools, missing_tools }` | MIRROR | `mcp_lifecycle_hardened.rs` | SPEC-interop §3 |
| `EnforcementResult::Denied { active_mode, required_mode, reason }` — denied errors carry why | **MIRROR** | `permission_enforcer.rs` | SPEC-hitl-approval §4 (approval escalation) |
| Monolithic harness (no gateway/harness split) | **IGNORE** | overall architecture | SERA's split is deliberate; claw-code's single-process design is not a model for the transport layer |

### 10.2 `openai/codex` — Rust harness + app-server

**Source:** [github.com/openai/codex](https://github.com/openai/codex) — `codex-rs/` Rust workspace, ~70 crates.
**License:** Apache-2.0.
**Why it's the most important reference:** Codex already has the exact **stateless app-server with swappable transport** shape SERA is targeting. The `codex-rs/app-server` + `codex-rs/app-server-protocol` + `codex-rs/core` split maps almost 1:1 onto SERA's `sera-gateway` + `sera-runtime`/`sera-harness`.

| What | Classification | Evidence | Spec to update |
|---|---|---|---|
| **SQ/EQ pattern** — `Submission { id, op, trace: W3cTraceContext }` → stream of `EventMsg { id, msg }` | **MIRROR (core envelope)** | `codex-rs/protocol/src/protocol.rs` | SPEC-gateway §3 (event model) + SPEC-runtime §2 (runtime trait) — this replaces SERA's current `Event` envelope |
| `Op::UserTurn { items, cwd, approval_policy, sandbox_policy, model, effort, ... }` — per-turn policy overrides as first-class fields | **MIRROR** | `codex-rs/protocol/src/protocol.rs` | SPEC-runtime §3 (turn loop) — policy scoping is per-turn, not per-session |
| W3C trace context carrier field on every submission | MIRROR | `protocol.rs::Submission` | SPEC-observability §2 (trace propagation) — make non-optional |
| **`AppServerTransport::{Stdio, WebSocket, InProcess}`** — three-variant transport enum | **MIRROR (architectural core)** | `codex-rs/app-server/src/transport/mod.rs` | **New section in SPEC-gateway:** "Gateway↔Harness Transport" — SERA adopts this enum verbatim. `InProcess` variant is critical for testing and embedded deployments. |
| JSON-RPC framing on all transports with serde alias-based protocol versioning (e.g. `task_started` aliased to `turn_started`) | **MIRROR** | `codex-app-server-protocol` | SPEC-versioning §4 (proto versioning) — forward-compat pattern for JSON envelopes |
| **`AskForApproval { UnlessTrusted, OnRequest, Granular(GranularApprovalConfig), Never }`** — five-level approval enum (omit deprecated `OnFailure`) | **MIRROR** | `protocol.rs::approvals` | SPEC-hitl-approval §4 |
| `GranularApprovalConfig` — per-category (exec / patch / network) approval control | MIRROR | `protocol.rs::approvals` | SPEC-hitl-approval §5 |
| **Guardian pre-approval LLM risk assessor** — `GuardianAssessmentEvent { risk_level }` fires before HITL surfacing | LEARN FROM | `protocol.rs::guardian` | SPEC-hitl-approval §6 — consider adding a pre-HITL LLM risk gate |
| **Three-layer sandbox model**: coarse `SandboxPolicy` + fine `FileSystemSandboxPolicy` + `NetworkSandboxPolicy` per-exec, plus `ExternalSandbox` variant for already-containerized harnesses | **MIRROR** | `codex-rs/sandboxing/src/lib.rs` | SPEC-tools §6a (SandboxProvider tiers) + SPEC-deployment §3 |
| Approval requests as EQ events, amendments (`ExecPolicyAmendment`, `NetworkPolicyAmendment`) as SQ submissions — no separate RPC surface | **MIRROR** | `protocol.rs` | SPEC-hitl-approval §4 |
| `DynamicToolSpec { name, description, input_schema: JsonValue, defer_loading }` — schema-driven tool registration, no Rust trait | **MIRROR** | `codex-rs/protocol/src/dynamic_tools.rs` | SPEC-tools §3 (tool trait) — add a schema-driven registration path parallel to the Rust `Tool` trait |
| `DynamicToolCallRequest { call_id, turn_id, tool, arguments }` — `turn_id` scoping on every call | **MIRROR** | `dynamic_tools.rs` | SPEC-runtime §6 (tool call loop) — essential for routing multi-call results |
| `defer_loading: bool` — progressive tool disclosure | MIRROR | `dynamic_tools.rs` | SPEC-tools §4.1 |
| `InitialContextInjection::{ DoNotInject, BeforeLastUserMessage }` — two compaction injection modes for KV-cache positioning | **MIRROR** | `codex-rs/core/src/compact.rs` | SPEC-runtime §5.1 |
| `CompactedItem { message, replacement_history }` compaction wire shape | MIRROR | `compact.rs` | SPEC-runtime §5 |
| **Five hook points**: `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `Stop` — each with typed `*Request` / `*Outcome` pairs; outcomes can block or modify | **MIRROR** | `codex-rs/hooks/src/lib.rs` | SPEC-hooks §3 — align hook point names with codex for interop |
| `HookToolInput { kind: HookToolKind, ... }` — hooks discriminate tool kinds (shell / patch / MCP / web-search) | MIRROR | `hooks/src/lib.rs` | SPEC-hooks §2.3 (HookContext) |
| Single EQ channel mixes lifecycle events and streaming deltas (`AgentMessageDelta`, `ExecCommandOutputDelta`, etc.) — no separate streaming sub-protocol | **MIRROR** | `protocol.rs::EventMsg` | SPEC-gateway §3 + SPEC-clients §2 |
| Delta event naming convention: `*Delta` suffix | MIRROR | `protocol.rs` | cross-cutting |
| Rollout-based persistence with explicit `flush_rollout()` / `ensure_rollout_materialized()` checkpoints (not event-sourced replay) | LEARN FROM | `codex-rs/rollout/src/state_db.rs` | SPEC-gateway §6 (session persistence) — SERA can use sqlx-backed rollouts instead of reinventing a rollout format |
| `TurnStartedEvent { turn_id, started_at, model_context_window, ... }` — context window size on every turn start so the client knows when compaction is imminent | MIRROR | `protocol.rs` | SPEC-runtime §3 |
| `deprecated: OnFailure` approval mode | **IGNORE** | `approvals.rs` | Do not mirror. |

### 10.3 `paperclipai/paperclip` — Multi-agent coordination (TypeScript)

**Source:** [github.com/paperclipai/paperclip](https://github.com/paperclipai/paperclip) — TypeScript monorepo.
**License:** MIT.
**Why it's useful:** Mature production coordination patterns across hundreds of agent runs. SERA's Circle model is more declarative than Paperclip's emergent coordination, but several primitives translate directly.

| What | Classification | Evidence | Spec to update |
|---|---|---|---|
| **Atomic optimistic-lock checkout**: `checkout(id, agentId, expectedStatuses, runId)` for task dispatch | **MIRROR** | `server/src/services/issues.ts` | SPEC-circles §4 — every Circle task dispatch must use this |
| **Four wakeup trigger taxonomy**: `timer`, `assignment`, `on_demand`, `automation` | **MIRROR** | `server/src/services/heartbeat.ts` | SPEC-workflow-engine §3 (WorkflowTrigger) — add the four named categories |
| `automation` wakeup type for process-loss retry + missing-output follow-up | **MIRROR** | `heartbeat.ts` | SPEC-workflow-engine §3 — operationally important and often omitted from simpler models |
| **`PluginEvent` envelope**: `{ eventId, eventType, circleId, occurredAt, entityId, entityType, payload, actorType, actorId }` — typed pub/sub with namespace isolation (`plugin.acme.*` wildcards) | **MIRROR** | `server/src/services/plugin-event-bus.ts` | SPEC-hooks §6 + SPEC-circles §5 — add `correlationId` field (Paperclip gap) |
| Anti-spoofing: plugins cannot emit `plugin.`-prefixed events | MIRROR | `plugin-event-bus.ts` | SPEC-hooks §6 |
| **Three-layer failure model**: orphan reaping via `isProcessAlive(pid)` + single process-loss retry + budget hard-stop + output-enforcement follow-up wakeup | **MIRROR** | `heartbeat.ts` | SPEC-workflow-engine §5 + SPEC-circles §6 |
| **`revision_requested` approval state** — two-step `pending → revision_requested → pending` cycle | **MIRROR** | `server/src/services/approvals.ts` | SPEC-hitl-approval §3 (approval state machine) |
| **`ConcurrencyPolicy = SkipIfActive \| Coalesce \| AlwaysEnqueue`** — orthogonal to Sequential/Parallel | **MIRROR** | `routines.ts` | SPEC-circles §3 — add as sub-field of `CoordinationPolicy` |
| `reportsTo` hierarchy with `assertNoCycle()` | MIRROR | `agents.ts::orgForCompany` | SPEC-circles §2 |
| **No result-aggregation primitive exists** — Paperclip aggregates implicitly via supervisor agents reading child issue outputs on next heartbeat | **REJECT as pattern** | `agents.ts`, `issues.ts` | SPEC-circles §4 — **SERA must build a `ResultAggregator` trait**; this is the largest gap between Paperclip's emergent model and SERA's declared `Consensus` / `Supervised` policies |
| Implicit DAG via issue-graph (cannot inspect planned execution before runtime) | **REJECT** | `issues.ts` | SPEC-circles §2 — SERA's Circle DAG stays explicit and pre-validated |
| In-process event bus (single Node server) | **IGNORE** (transport only) | `plugin-event-bus.ts` | Message shape mirrors; transport does not |
| Per-agent heartbeat with `maxConcurrentRuns` cap (1–10) and `withAgentStartLock()` serial slot claiming | LEARN FROM | `heartbeat.ts` | SPEC-gateway §5 — maps onto SERA's single-writer-per-session invariant |
| `hire_agent` approval gate as onboarding-time (not per-action) control | LEARN FROM | `approvals.ts` | SPEC-hitl-approval §4 — SERA should distinguish onboarding gates from per-action gates |

### 10.4 `gastownhall/beads` — Task DAG execution substrate

**Source:** [github.com/gastownhall/beads](https://github.com/gastownhall/beads) — Go, MIT, backed by Dolt (SQL git).
**Why it's more than inspiration:** Beads is production-grade infrastructure for multi-agent task DAGs with atomic claim, dependency-aware ready detection, content-hash IDs for merge-safety, and first-class LLM agent integration (CLI + MCP + Claude plugin). SERA's `sera-workflow` should be designed around beads' data model, not merely "integrate with" it.

> **Action:** Upgrade beads from SPEC-workflow-engine's Phase-3 deferral to **Phase-1 design input**. The `bd ready` algorithm and `--claim` atomic protocol are foundational, not optional enhancements.

| What | Classification | Evidence | Spec to update |
|---|---|---|---|
| **`Issue` struct**: content-hash ID (`bd-a1b2`), `Status` (`open / in_progress / hooked / blocked / deferred / closed / pinned`), `Priority 0-4`, `IssueType` (`task / epic / bug / feature / chore / decision / message / spike / story / milestone`), `Metadata json.RawMessage` extension point | **MIRROR** | `internal/types/types.go` | SPEC-workflow-engine §2 (WorkflowTask type) |
| **`DependencyType`** enum: `blocks`, `related`, `parent_child`, `discovered_from`, **`conditional_blocks`** (B runs only if A fails) | **MIRROR** | `types.go` | SPEC-workflow-engine §3 — `conditional_blocks` adds a branching primitive SERA doesn't currently model |
| **`StatusHooked`** = atomically claimed by worker, separate from `in_progress` | **MIRROR** | `types.go` | SPEC-workflow-engine §4 (concurrent execution) |
| **`bd update --claim` atomic protocol** — sets `assignee + status=hooked` in one transaction | **MIRROR as gateway operation** | `beads.go` | SPEC-gateway §5 + SPEC-workflow-engine §4 — SERA's gateway exposes `claim_workflow_task(task_id, agent_id)` with the same atomicity guarantees |
| **`bd ready` algorithm** — claimable ⟺ `status == open` AND no open/in_progress issue has a `blocks` edge to it AND `DeferUntil` is past | **MIRROR** | `beads.go` | SPEC-workflow-engine §4 — this is the default ready-detection algorithm for SERA's workflow queue |
| **Content-addressed IDs** — SHA256 of canonical fields, stable across branches, merge-safe | **MIRROR** | `types.go` ID derivation | SPEC-workflow-engine §2 + SPEC-memory §5.3 (git conflict resolution) — content hashes also solve the open question about multi-agent workspace merges |
| `DeferUntil` field — separates scheduling from blocking | MIRROR | `types.go` | SPEC-workflow-engine §3 |
| `AwaitType` gate enum — `gh:run`, `gh:pr`, `timer`, `human`, `mail` | MIRROR | `types.go` | SPEC-workflow-engine §3 — directly maps onto `WorkflowTrigger` |
| Formula / molecule DAG template system (TOML, `[[steps]]` with `needs = [...]` and `[steps.gate]`) | MIRROR | beads templates | SPEC-workflow-engine §2 (WorkflowDef) — adopt `needs` / `waits_for` fan-in declaration |
| `Wisp` / `Ephemeral` lifecycle — transient scratch state not synced via git, TTL-compacted | LEARN FROM | `types.go` | SPEC-memory §6 — model for SERA's scratch-memory tier |
| `bd prime` context-injection mechanism — single command injects `PRIME.md` into agent session start | LEARN FROM | beads CLI + SERA's existing `CLAUDE.md` | SPEC-hooks §3 (`on_workflow_trigger`) |
| Dolt storage (SQL git) — embedded or `dolt sql-server`, cell-level 3-way merge | LEARN FROM | `.beads/embeddeddolt/` | SPEC-memory §5.3 — alternative storage option for multi-writer workspaces |
| `wasteland` federation protocol (fork + claim + evidence-linked completion over DoltHub) | LEARN FROM | [gastownhall/wasteland](https://github.com/gastownhall/wasteland) | SPEC-circles §7 — cross-organization task coordination |
| **Beads CLI as a SERA tool** | **DEPEND ON (runtime tool)** | [gastownhall/beads](https://github.com/gastownhall/beads) + [`beads-mcp` PyPI](https://pypi.org/project/beads-mcp/) | SPEC-tools §6 + SPEC-workflow-engine §6.1 — agents call `bd_ready`, `bd_create`, `bd_update --claim`, `bd_close`, `bd_dep_add` as tool invocations |
| `gastown` workspace manager (Mayor/Deacon/Refinery/Convoy) | **IGNORE** | gastown repo | Superset of SERA's needs; tightly coupled to tmux worktree management |
| `gascity` SDK | LEARN FROM | [gastownhall/gascity](https://github.com/gastownhall/gascity) | Architecture reference only; Go + tmux, not embeddable |
| `gascity-otel` compose stack | LEARN FROM | [gastownhall/gascity-otel](https://github.com/gastownhall/gascity-otel) | Ready-made VictoriaMetrics + Grafana reference for SERA's deployment guide |

### 10.5 `openclaw/openclaw` — Personal-assistant platform (TypeScript)

**Source:** [github.com/openclaw/openclaw](https://github.com/openclaw/openclaw) — TypeScript monorepo with Swift, Kotlin, Go mobile clients; homepage openclaw.ai. Distinct from `claw-code`, `zeroclaw`, `lossless-claw`.
**License:** MIT.
**Why it's useful:** openclaw has a clean **gateway / pluggable harness separation** with explicitly published contracts — the closest TypeScript analog to SERA's architecture. File shapes verified via direct GitHub API fetches; surface-level repo metrics are suspect and not cited.

| What | Classification | Evidence | Spec to update |
|---|---|---|---|
| **`AgentHarness` trait**: `id`, `label`, `supports(ctx) -> HarnessSupport`, `runAttempt(params) -> AttemptResult`, optional `compact`, `reset`, `dispose`. `supports()` returns `{ supported: true; priority? }` or `{ supported: false; reason? }` | **MIRROR** | `src/agents/harness/types.ts` | SPEC-runtime §2 (AgentRuntime trait) — add `supports()` method; gateway ranks harnesses by priority at dispatch rather than static ID lookup |
| **Capability-negotiated harness selection** — gateway queries every registered harness with `{ provider, modelId, requestedRuntime }`, ranks results | **MIRROR** | `src/agents/harness/registry.ts` | SPEC-gateway — new subsection: "Harness Selection by Capability" |
| **`ContextEngine` as a separately pluggable slot**: `bootstrap`, `ingest`, `assemble(params) -> { messages, estimatedTokens, systemPromptAddition }`, `compact`, `maintain?`, `afterTurn?` | **MIRROR** | `src/context-engine/types.ts`, `src/context-engine/registry.ts` | SPEC-runtime §4 — make `ContextEngine` a distinct extension axis, orthogonal to `AgentRuntime`. A plugin can replace memory/compaction without touching the turn loop |
| **Compaction checkpoint with dual transcript refs**: `{ reason: "manual" \| "auto-threshold" \| "overflow-retry" \| "timeout-retry", preCompaction: { sessionId, sessionFile, leafId, entryId }, postCompaction: {...}, tokensBefore, tokensAfter, summary }` | **MIRROR** | `src/gateway/protocol/schema/sessions.ts`, `src/gateway/session-compaction-checkpoints.ts` | SPEC-runtime §5 (compaction) + SPEC-gateway §6 (session persistence) — checkpoint reasons as discriminant; reversible snapshots |
| `MAX_COMPACTION_CHECKPOINTS_PER_SESSION = 25` rolling window | MIRROR | `session-compaction-checkpoints.ts` | SPEC-runtime §5 — cap checkpoint accumulation |
| **`subagent_delivery_target` hook** — fires between subagent completion and parent session delivery; middle hook for result transformation and fan-in aggregation (most frameworks only expose spawn/end) | **MIRROR** | `src/plugins/hook-types.ts` | SPEC-circles §4 (`ResultAggregator` integration point) + SPEC-hooks §3 (add to hook point table) |
| **`parentSessionKey` on `SessionsCreateParams` + `spawnedBy` on `SessionsListParams`** — subagent lineage as first-class session fields, queryable | **MIRROR** | `src/gateway/protocol/schema/sessions.ts` | SPEC-gateway §6.3 (session scoping) — add to session key/metadata model |
| `ExecApprovalsFileSchema` — per-agent allowlist `{ pattern, argPattern }` plus `autoAllowSkills`, addressable per-node per-agent | MIRROR | `src/gateway/protocol/schema/exec-approvals.ts` | SPEC-hitl-approval §4 — argument-pattern separate from command-pattern |
| **Two-tier hook bus**: `InternalHookEvent` (gateway-internal) separate from `PluginHookName` (plugin-facing SDK) — internal signaling decoupled from external contract | LEARN FROM | `src/hooks/internal-hook-types.ts`, `src/plugins/hook-types.ts` | SPEC-hooks §6 — document the two-tier split as a versioning strategy |
| Full hook point set: `before_model_resolve`, `before_prompt_build`, `before_agent_start`, `before_agent_reply`, `llm_input`, `llm_output`, `agent_end`, `before_compaction`, `after_compaction`, `before_reset`, `inbound_claim`, `message_received`, `message_sending`, `message_sent`, `before_tool_call`, `after_tool_call`, `tool_result_persist`, `before_message_write`, `session_start`, `session_end`, `subagent_spawning`, `subagent_delivery_target`, `subagent_spawned`, `subagent_ended`, `gateway_start`, `gateway_stop`, `before_dispatch`, `reply_dispatch`, `before_install` | LEARN FROM | `src/plugins/hook-types.ts` | SPEC-hooks §3 — reference hook point set for cross-check against SERA's 16 points |
| Plugin SDK import boundary — plugins may only import from `src/plugin-sdk/*`, direct imports from `src/**` forbidden by `AGENTS.md` | LEARN FROM | `AGENTS.md` | SPEC-crate-decomposition — consider a similar hard boundary between `sera-plugin-sdk` and core crates |
| `ContextEngine` factory pattern `() => ContextEngine \| Promise<ContextEngine>` for async init (DB connections) | LEARN FROM | `src/context-engine/registry.ts` | SPEC-runtime §4 — in Rust: `async fn create() -> Box<dyn ContextEngine>` |
| Channel system (Telegram, Discord, iMessage, Signal, WhatsApp...) | **IGNORE** | `src/gateway/channels/*` | SERA's transport and channel layer is separate |
| Mobile clients (Swift, Kotlin) | **IGNORE** | `apps/` | Not relevant |
| Product UI, onboarding wizard, i18n | **IGNORE** | — | No architectural value for SERA's gateway/harness design |

### 10.6 `NousResearch/hermes-agent` — Python harness + RL pipeline + ACP server

**Source:** [github.com/NousResearch/hermes-agent](https://github.com/NousResearch/hermes-agent) — Python, MIT, actively maintained. Full production harness, not a fine-tuning pipeline alone; ships Atropos RL environments for trajectory-based model training.
**Why it's useful:** hermes-agent solves **per-model tool-call format variance** cleanly. It is the reference design for SERA's `sera-models` adapter layer.

| What | Classification | Evidence | Spec to update |
|---|---|---|---|
| **Parser registry pattern** — `@register_parser("name")` decorator + `HashMap<name, Parser>` populated at import time; the turn loop calls a named parser only on the fallback path | **MIRROR** | `environments/tool_call_parsers/__init__.py` | SPEC-runtime §5 + SPEC-dependencies — introduce a `trait ToolCallParser { fn parse(&self, text: &str) -> Result<(Option<String>, Vec<ToolCall>), ParseError>; }` with an inventory-based registry |
| **Two-tier tool-call normalization**: primary path is always OpenAI-spec `tool_calls` on the response; fallback is named parser on raw `content` for models whose serving layer cannot parse | **MIRROR** | `environments/agent_loop.py::HermesAgentLoop.run()` | SPEC-runtime §5.1 — turn loop never branches on model name; adapter returns a canonical `Vec<ToolCall>` |
| **11 named parsers** at startup: `hermes`, `mistral` (pre-v11 and v11+), `llama`, `qwen`, `qwen3_coder`, `deepseek_v3`, `deepseek_v3_1`, `kimi_k2`, `glm45`, `glm47`, `longcat` — each a single file with a single interface | MIRROR | `environments/tool_call_parsers/*.py` | SPEC-runtime §5.1 — adding a new model family is a single file + `register!` macro, zero loop changes |
| **Reasoning-content extraction** as a standalone normalization step — handles three provider field variants (`reasoning_content`, `reasoning`, `reasoning_details[].text`) before the tool-call check | **MIRROR** | `agent_loop.py::_extract_reasoning_from_message()` | SPEC-runtime §5 — reasoning extraction is its own step; returns `Option<String>` alongside tool calls |
| **`extra_body` passthrough** on `chat_kwargs` for provider-specific params (OpenRouter preferences/transforms, DeepSeek `thinking_budget`, vLLM sampling) | **MIRROR** | `agent_loop.py::chat_kwargs["extra_body"]` | SPEC-runtime §5.3 + SPEC-dependencies §9.1 — `ModelInfo { provider_params: serde_json::Value }` merged into every request |
| `&lt;tool_call&gt;{JSON}&lt;/tool_call&gt;` Hermes XML-JSON hybrid with regex that handles truncated/unclosed tags at EOS | LEARN FROM | `environments/tool_call_parsers/hermes_parser.py` | SPEC-runtime §5.1 — reference impl for the `hermes` parser in SERA |
| **ACP adapter wrapping the agent as a protocol server** — `HermesACPAgent` exposes `NewSession`, `ResumeSession`, `ForkSession`, `ListSessions`, `Prompt`, model switching, tool-approval callbacks, streaming events (message, step, thinking, tool_progress) | LEARN FROM | `acp_adapter/server.py`, `acp_adapter/permissions.py`, `acp_registry/agent.json` | SPEC-interop — note: "ACP" here is Zed's Agent Client Protocol, not the IBM/BeeAI ACP merged into A2A. Different protocol. Evaluate whether SERA's harness should expose an ACP-compat surface for Zed / IDE clients |
| `ForkSession` + memory-manager cross-session persistence | LEARN FROM | `acp_adapter/server.py`, `memory_manager.py` | SPEC-gateway §6 — session fork semantics |
| Cron + scheduled automations + multi-channel messaging gateway (Telegram, Discord, Slack, Signal, WhatsApp) | **IGNORE** | `cron/`, `channels/` | SERA owns its own gateway and scheduler |
| Atropos RL trajectory recording + reward-tuning pipeline | **IGNORE** | `tinker-atropos/`, `trajectory.py` | Out of scope for SERA's gateway |
| Skill system compatible with `agentskills.io` standard | LEARN FROM | `~/.hermes/skills/` loader | SPEC-runtime §13 (skills) — AgentSkills format is a real ecosystem standard worth tracking |
| Python sync-in-`ThreadPoolExecutor` for async ACP wrapping | **IGNORE** | `acp_adapter/` | Python-specific, not applicable |

### 10.7 `anomalyco/opencode` — Coding agent with Effect-TS + Vercel AI SDK (TypeScript)

**Source:** [github.com/anomalyco/opencode](https://github.com/anomalyco/opencode) — MIT, TypeScript monorepo. Verified not a fork (`fork: false`, `parent: null`). Default branch `dev`; built on Vercel AI SDK + Effect-TS. Ships packages `opencode`, `app`, `sdk`, `desktop`, `plugin`, `ui`, `util`.
**Why it's useful:** opencode has unique patterns around **in-turn user-correction feedback**, **subagent session resumption**, and **permission-based shell analysis** that are not present in codex or claw-code.

| What | Classification | Evidence | Spec to update |
|---|---|---|---|
| **`CorrectedError { feedback: String }` as a tool-result variant** — when the user rejects a tool call with a written reason, the reason is passed back as the tool output so the LLM can self-correct in the same turn without a restart | **MIRROR (high leverage)** | `packages/opencode/src/permission/index.ts::CorrectedError` | SPEC-runtime §6 + SPEC-hitl-approval — add `ToolResult::Rejected { feedback: String }` variant |
| **`task_id` subagent session resumption** — child sessions addressable across parent turns; `task` tool looks up `Agent.Info` by `subagent_type`, creates a child `Session` with `parentID`, inherits model unless overridden | **MIRROR** | `packages/opencode/src/tool/task.ts` | SPEC-gateway §6.3 + SPEC-circles §3 — child session IDs are stable and continuable |
| **Turn result signal enum**: `"compact" \| "stop" \| "continue"` — harness emits a typed completion signal to the gateway; compaction is a first-class turn outcome, not implicit | **MIRROR** | `packages/opencode/src/session/processor.ts::Result` | SPEC-runtime §3 (turn loop) — explicit `TurnOutcome` enum so the gateway can act on compaction without introspecting session state |
| **Doom-loop detector**: `DOOM_LOOP_THRESHOLD = 3` — repeated identical tool calls escalate to a `doom_loop` permission check instead of hard-failing | **MIRROR** | `packages/opencode/src/session/processor.ts` | SPEC-hitl-approval — doom-loop escalation as a distinct approval category |
| **`Permission.Ruleset` wildcard evaluator** with three actions (`allow` / `deny` / `ask`) + cascading `always` / `once` / `reject+feedback` replies; rejecting one call cancels other pending requests in the same session | MIRROR | `packages/opencode/src/permission/index.ts`, `permission/evaluate.ts` | SPEC-hitl-approval §3 — complement static tier policies with per-session runtime overrides |
| **`FileTime.withLock` conflict detection** — before any file write, check whether the file has been modified externally since the harness last read it; raise conflict rather than overwrite | **MIRROR** | `packages/opencode/src/file/time.ts` + `src/tool/edit.ts` | SPEC-tools §6 — file-write tools check mtime; concurrent-edit safety for multi-agent worktrees |
| **Tree-sitter static analysis of bash commands before execution** — extract file paths touched by shell command via AST (`BashArity` module), check each against permission ruleset; no OS-level sandbox needed for most cases | **MIRROR** | `packages/opencode/src/tool/bash.ts` | SPEC-tools §6a — `pre_execute` hook on `bash` tool runs AST analysis and emits `ShellAudit` event before dispatch |
| **`assertExternalDirectoryEffect`** — single reusable check that any file path is inside the project worktree root | **MIRROR** | `packages/opencode/src/tool/external-directory.ts` | SPEC-gateway §4 — path policy lives in the gateway (not the harness) and runs before dispatching any file tool |
| **Two-layer persistence**: SQLite via Drizzle ORM (`SessionTable`, `MessageTable`, `PartTable` with `parent_id` FK) + shadow git repo via `Snapshot` service (`track()` before tool, `revert(patches)` for undo, `diffFull(from, to) -> FileDiff[]`) | **MIRROR** | `packages/opencode/src/session/session.sql.ts`, `src/snapshot/index.ts` | SPEC-gateway §6 (session persistence) — conversation state in sqlx, filesystem state in a shadow git workspace; `pre_tool` → `track`, `post_tool` → `settle` |
| **`Tool.Def<Parameters, Metadata>`** interface: `{ id, description, parameters: ZodSchema, execute(args, ctx: Tool.Context) -> Effect<ExecuteResult<M>>, formatValidationError? }`. `Tool.Context` carries `sessionID`, `messageID`, `agent`, `abort: AbortSignal`, `messages`, `metadata()` updater, and **inline `ask()` hook** so the tool itself requests approval | MIRROR | `packages/opencode/src/tool/tool.ts` | SPEC-tools §3 — a Rust analog has the approval `ask()` callable on `ToolContext`, keeping the gateway clean of tool-specific approval logic |
| Shipped tools: `read`, `edit`, `multiedit`, `apply_patch`, `bash`, `glob`, `grep`, `ls`, `codesearch`, `lsp`, `webfetch`, `plan`, `question`, `task` (subagent), `todo`, `skill`, `truncate` | LEARN FROM | `packages/opencode/src/tool/*` | SPEC-tools — minimum-viable tool catalog reference |
| Three edit strategies: `edit` (string-replace with diff display; cites Cline and Gemini CLI), `apply_patch` (structured patch: `*** Begin Patch` / `*** End Patch` with add/update/delete/move in one batch), `multiedit` (batch multiple edits) | LEARN FROM | `packages/opencode/src/tool/edit.ts`, `apply_patch.ts`, `patch/` | SPEC-tools — edit primitive choice; SERA should ship at least `edit` + `apply_patch` parity |
| Full MCP client via `@modelcontextprotocol/sdk` with stdio, SSE, StreamableHTTP transports and OAuth (`oauth-provider.ts`, `oauth-callback.ts`); MCP tools bridged into the same `Tool.Def` via `dynamicTool` + `jsonSchema()` | LEARN FROM | `packages/opencode/src/mcp/index.ts` | SPEC-interop §3 — SERA's MCP client (rmcp) bridges external tools into the same `DynamicToolSpec` surface |
| Built-in agents: `build`, `plan` (read-only), `general` (subagent), `explore` (read-only search), `compaction` (hidden), `title` (hidden), `summary` (hidden) | LEARN FROM | `packages/opencode/src/agent/agent.ts` | SPEC-runtime §13 (skills) — hidden system agents for compaction/title/summary are a useful pattern |
| Effect-TS `Layer`/`Context` architecture | **IGNORE** | cross-cutting | Language-specific; no Rust applicability |
| Monolithic server (no gateway/harness split) | **IGNORE** | package structure | SERA's split is strictly superior |

### 10.8 `NVIDIA/NemoClaw` — Enterprise sandbox-hardening reference stack

**Source:** [github.com/NVIDIA/NemoClaw](https://github.com/NVIDIA/NemoClaw) — Apache-2.0. Created 2026-03-15, alpha. Languages: TypeScript (~66%), Shell (~24%), Python (~2%), Dockerfile. Verified directly via GitHub API — file contents quoted below are from upstream, not synthesized.
**What it is:** A **deployment and hardening reference** that installs [NVIDIA OpenShell](https://github.com/NVIDIA/OpenShell) (part of the NVIDIA Agent Toolkit, provides the sandbox runtime), creates blueprint-defined sandboxes, applies layered policies, and runs OpenClaw assistants inside. It is not an agent framework, not a harness, and not a coordination layer — it is a **Tier-3 deployment pattern** SERA should mirror for its enterprise sandbox story.
**Scope reality check:** Single-agent containment. No multi-agent coordination, no hooks, no MCP/A2A/ACP. Its value is **explicitly in the sandbox policy model and blueprint/image-pinning discipline**, nothing else.

| What | Classification | Evidence | Spec to update |
|---|---|---|---|
| **Per-binary process-scoped egress allowlist** — each `network_policies[].binaries: [{path}]` entry ties an outbound allowlist to a specific binary path (e.g. `/usr/local/bin/claude` only). A policy only applies to outbound connections *from that binary* — not the whole sandbox | **MIRROR (high leverage)** | `nemoclaw-blueprint/policies/openclaw-sandbox.yaml` (`claude_code` policy + `binaries: [{path: /usr/local/bin/claude}]`) | SPEC-tools §6a + SPEC-deployment — SERA's `NetworkSandboxPolicy` should support per-tool-binary scoping, not just per-session |
| **Method + path REST allowlist** with wildcards — each endpoint has `rules: [{ allow: { method: POST, path: "/v1/messages/batches/**" } }]`. Fine-grained API-level enforcement, not just host:port | **MIRROR** | `schemas/sandbox-policy.schema.json` (`$defs.rule.allow.{method, path}`) | SPEC-tools §6a — tier policies get a `rest_rules: Vec<{method, path}>` field per endpoint; escalates from L4 (host:port) to L7 (method+path) |
| **`enforcement: "enforce" \| "audit"` mode per endpoint** — audit mode lets operators observe policy violations before promoting to enforce. Essential for incremental rollout | **MIRROR** | `schemas/sandbox-policy.schema.json` (`$defs.endpoint.enforcement`) | SPEC-hooks §5 — policy hooks and sandbox policies both need an audit mode |
| **`tls: "terminate" \| "passthrough"`** — the egress proxy can terminate TLS to inspect payloads against the rule set, or passthrough encrypted traffic for trusted endpoints | **MIRROR** | `schemas/sandbox-policy.schema.json` (`$defs.endpoint.tls`) | SPEC-deployment + SPEC-security §4 — document TLS terminate as the default inspection model for Tier-3 egress |
| **`access: "full"` endpoint escape hatch** — per-endpoint bypass for trusted upstreams, coexists with `rules` (must have one or the other) | MIRROR | `schemas/sandbox-policy.schema.json` | SPEC-tools §6a |
| **Pinned image digest with dual-field lockstep** — `digest: "sha256:..."` at the blueprint top level mirrors the `components.sandbox.image` digest. Release tooling rewrites both fields together; prevents bumping the image without the top-level declaration and blocks registry-compromise or `:latest` force-push attacks (issue #1438) | **MIRROR** | `nemoclaw-blueprint/blueprint.yaml` header comment | SPEC-deployment — SERA's blueprint schema adopts a top-level `digest:` that mirrors the sandbox image digest; CI enforces lockstep |
| **Deny-by-default filesystem policy** — `read_only: [/usr, /lib, /proc, /dev/urandom, /app, /etc, /var/log, /sandbox, /sandbox/.openclaw]` + narrow `read_write: [/tmp, /dev/null, /sandbox/.openclaw-data, /sandbox/.nemoclaw]`. **Home directory itself is read-only**; writable state lives in a specific subdir via symlinks — prevents agents from tampering with their own runtime environment | **MIRROR** | `nemoclaw-blueprint/policies/openclaw-sandbox.yaml` | SPEC-tools §6a — deny-by-default with narrow writable state under a `.<agent>-data/` subdir |
| **`include_workdir: false` with documented rationale** — when true, OpenShell auto-adds `WORKDIR` to `read_write`, which **overrides** the explicit `read_only` entry because **Landlock grants the union of all matching rules**, not intersection. Explicit comment in source: *"must be false"* (issue #804) | **MIRROR (invariant)** | `openclaw-sandbox.yaml` lines documenting `include_workdir` | SPEC-tools §6a — document the Landlock rule-union gotcha and require explicit writable paths; never rely on implicit WORKDIR inclusion |
| **Landlock compatibility mode enum**: `"strict" \| "best_effort"` — strict fails closed if Landlock is unavailable, best_effort degrades gracefully | MIRROR | `schemas/sandbox-policy.schema.json` (`landlock.compatibility`) | SPEC-deployment §3 — SERA's sandbox tiers declare their Landlock requirement explicitly |
| **`run_as_user: sandbox`, `run_as_group: sandbox`** — non-root execution enforced in policy | MIRROR | `openclaw-sandbox.yaml` | SPEC-tools §6a — process identity in the policy schema |
| **Opt-in preset system** — `policies/presets/{brave, brew, discord, github, huggingface, jira, npm, outlook, pypi, slack, telegram}.yaml`. **GitHub access used to be in the base policy granted to every sandbox; moved to an opt-in preset (#1583)** so a sandbox only gets GitHub access when the user explicitly selects it during onboard | **MIRROR (discipline)** | `nemoclaw-blueprint/policies/presets/` | SPEC-deployment + SPEC-tools §5 — SERA's tier policies ship a minimal base + discoverable opt-in presets; common developer tooling (GitHub, npm, brew) is never in the base |
| **Sentry-class exfiltration defense** — documented in the policy file: initially allowed `POST /**` to sentry.io for crash telemetry, then realized Sentry is multi-tenant (any authenticated client can POST to any project, so the host became a generic exfil channel). Fix: **block POST entirely, allow only GET**. Comment: *"that is the right tradeoff for a sandbox whose stated goal is preventing data egress."* (#1437) | **MIRROR (threat model)** | `openclaw-sandbox.yaml` sentry.io block | SPEC-security — add "multi-tenant SaaS exfiltration channels" as a documented threat class; tier policies default to **GET-only** for any endpoint whose tenant boundary is not per-user path-isolated |
| **SSRF validation as a separate module** from network policy — runtime validation on user-supplied URLs going into requests, complementing the L4/L7 egress filter | MIRROR | `nemoclaw/src/blueprint/ssrf.ts` + `ssrf.test.ts` | SPEC-tools §6 — add a `SsrfValidator` trait invoked on any tool that takes a URL argument, even when network egress is already policy-filtered |
| Blueprint lifecycle: `runner.ts` + `snapshot.ts` + `state.ts` + `ssrf.ts` — production-quality with companion `.test.ts` | LEARN FROM | `nemoclaw/src/blueprint/` | SPEC-deployment — blueprint as a first-class versioned unit with runner, snapshot, and state files |
| **Inference provider profiles** — blueprint `components.inference.profiles: { default, ncp, nim-local, vllm }` each with `{ provider_type: "nvidia" \| "openai", endpoint, model, credential_env, credential_default, timeout_secs, dynamic_endpoint }` | MIRROR | `nemoclaw-blueprint/blueprint.yaml` | SPEC-dependencies §9.1 + SPEC-runtime §5.2 — SERA's model-routing config needs `credential_env` + `dynamic_endpoint` + `timeout_secs` fields; `genai` should plumb these through |
| **5 published JSON schemas**: `blueprint.schema.json`, `onboard-config.schema.json`, `openclaw-plugin.schema.json`, `policy-preset.schema.json`, `sandbox-policy.schema.json` — users author config against these | LEARN FROM | `schemas/*.schema.json` | SPEC-config — SERA's K8s-style manifests should each ship a published JSON Schema for editor validation |
| Platform matrix documented: Linux Docker (primary), macOS Apple Silicon (Colima / Docker Desktop), DGX Spark, Windows WSL2 | LEARN FROM | `README.md` | SPEC-deployment — document tested vs limited platforms |
| OpenShell gateway + sandbox lifecycle managed via `nemoclaw onboard`; explicit warning not to call `openshell self-update`, `openshell gateway start --recreate`, or `openshell sandbox create` directly | LEARN FROM | `README.md` "OpenShell Lifecycle" | SPEC-deployment — lifecycle ownership: the deployment tool owns the gateway process; operators do not hand-tune it |
| Pre-commit config 10.8 KB + commitlint + CodeRabbit YAML + pre-commit hooks | LEARN FROM | `.pre-commit-config.yaml`, `commitlint.config.js`, `.coderabbit.yaml` | Cross-cutting: NVIDIA-grade CI hygiene is worth mirroring in SERA's dev workflow |
| Multi-agent coordination | **IGNORE (not present)** | — | NemoClaw is single-agent containment; no coordination patterns here |
| MCP / A2A / ACP integration | **IGNORE (not present)** | — | — |
| Hook system | **IGNORE (not present)** | — | — |
| Python-specific blueprint runner packaging | **IGNORE** | `pyproject.toml`, `uv.lock` | SERA is Rust |

### 10.9 `github/spec-kit` — Spec-driven development methodology

**Source:** [github.com/github/spec-kit](https://github.com/github/spec-kit) — MIT, v0.6.1, Python CLI + Markdown templates. GitHub's own "spec-driven development" toolkit.
**Why it matters:** SERA already maintains `docs/plan/specs/` in a semi-structured way. spec-kit supplies the conventions we're missing for machine-consumable specs and "spec drift" control.
**Scope reality:** No JSON Schema for specs, no proto, no MCP, no built-in CI drift check (community extensions only). It's **methodology + CLI + Markdown templates**, not a schema registry.

| What | Classification | Evidence | Spec to update |
|---|---|---|---|
| **`[NEEDS CLARIFICATION: topic]` inline blocking markers** — agents treat as halt signal on plan generation | **MIRROR** | `spec-driven.md` gates plan generation | All SPEC-*.md — replace prose "unresolved" notes with inline markers |
| **YAML frontmatter on every SPEC**: `id / status / phase / crate / prd_sections / depends_on / last_updated` — makes `README.md` index machine-derivable | **MIRROR** | spec-kit command files | All SPEC-*.md — new frontmatter convention |
| **`## Acceptance Criteria` section** per SPEC with checkbox testable criteria | **MIRROR** | spec-kit self-test template | All SPEC-*.md — replace prose requirement lists |
| **`## Constitutional Constraints`** per SPEC (spec-kit "Phase -1 Gates" pattern) — lists invariants from `docs/ARCHITECTURE.md` this spec must satisfy | **MIRROR** | `plan.md` "Phase -1 Gates" | All SPEC-*.md — bind specs to architectural invariants |
| **`contracts/` subdirectory** for proto snippets / HTTP path excerpts too detailed for spec body | **MIRROR** | spec-kit `specs/NNN-name/contracts/` | New `docs/plan/specs/contracts/` directory |
| **Separation of WHAT (spec) from HOW (plan)** — spec.md describes requirements, plan.md describes implementation rationale | LEARN FROM | spec-kit file taxonomy | SERA SPECs currently mix both — move Rust type definitions out of requirement body |
| **Contract-first artifact ordering** (`contracts/ → tests → source`) as a documented rule | LEARN FROM | spec-kit workflow | SPEC-interop, SPEC-gateway — add explicit "contract-first" note |
| Python `specify` CLI, agent slash-command scaffolding, preset system | **IGNORE** | — | SERA agents read docs directly; no bootstrap CLI needed |
| Community extensions (`spec-kit-ci-guard`, `spec-kit-canon`, Azure DevOps sync) | **IGNORE** | — | SERA uses `bd` (beads) and its own CI |

### 10.10 `OpenHands/OpenHands` — Event stream agent platform (Python)

**Source:** [github.com/OpenHands/OpenHands](https://github.com/OpenHands/OpenHands) — **License: `NOASSERTION` — custom license, review before any code copy.** Python, actively maintained. V0 (controller/runtime) deprecated since 1.0.0; V1 agentic core lives in a sibling repo: [OpenHands/software-agent-sdk](https://github.com/OpenHands/software-agent-sdk).
**Why it matters:** Fills four concrete gaps not covered by prior research — composable compaction pipeline, sub-agent event isolation, typed HITL input collection, per-action security classification.

| What | Classification | Evidence (V0 or V1) | Spec to update |
|---|---|---|---|
| **`Action`/`Observation` discriminated union over persistent `EventStream`** with durable JSON-page `FileStore` and subscriber-per-component (`AGENT_CONTROLLER / SERVER / RUNTIME / MEMORY / MAIN`). Most complete replayable event-stream model in the research set. | **MIRROR** | V0 `openhands/events/stream.py` | SPEC-gateway §3 — event bus with per-subscriber dispatch + append-only `FileStore` |
| **`CondensationAction` / `CondensationRequestAction` split** — agent emits a request; controller schedules compaction as a first-class replayable event rather than silent side effect | **MIRROR** | V0 `openhands/events/action/agent.py` | SPEC-runtime §5 — compaction as event, not mutation |
| **Composable `PipelineCondenser`** with 9 implementations: `NoOp`, `RecentEvents`, `ConversationWindow`, `AmortizedForgetting`, `ObservationMasking`, `BrowserOutput`, `LLMSummarizing`, `LLMAttention`, `StructuredSummary`. Config-driven registry. `Condenser::condense(view) -> View \| Condensation` — returns either transparent view or event | **MIRROR (high leverage)** | V0 `openhands/memory/condenser/` | **New section in SPEC-runtime**: "Compaction Pipeline" — `Condenser` trait + registry; chain via `PipelineCondenser` |
| `LLMSummarizingCondenser` prompt tracks `USER_CONTEXT` and `TASK_TRACKING` sections explicitly across compactions | MIRROR | V0 `openhands/memory/condenser/impl/llm_summarizing_condenser.py` | SPEC-runtime — structured summary sections |
| **`CondensationAction.forgotten_event_ids` OR `forgotten_events_start_id..end_id`** (exclusive choice) + optional `summary` + `summary_offset` insertion point | MIRROR | V0 `openhands/events/action/agent.py` | SPEC-runtime §5 — compaction wire format |
| **`NestedEventStore`** — child agent writes to a sub-store, parent sees a single `AgentDelegateObservation`. Low-level isolation primitive not in prior research | **MIRROR** | V0 `openhands/events/nested_event_store.py` | SPEC-circles §4 — sub-agent event isolation below `subagent_delivery_target` hook |
| **Three-tier Microagents**: `RepoMicroagent` (always-on, `.openhands/microagents/repo.md`) + `KnowledgeMicroagent` (keyword-triggered via `trigger.lower() in message.lower()`) + **`TaskMicroagent`** (slash-command `/{name}` with typed `${variable}` input collection). Auto-ingests `.cursorrules`, `AGENTS.md`, `agent.md` as `RepoMicroagent` compat | **MIRROR** | V0 `openhands/microagent/microagent.py` | SPEC-runtime §13 (skills) — three-tier classification + typed `${variable}` input protocol. `TaskMicroagent` is a novel HITL input-collection primitive |
| Microagent frontmatter: `name`, `version`, `triggers`, `inputs`, `mcp_tools.stdio_servers` (auto-start MCP servers when microagent fires) | MIRROR | V0 `openhands/microagent/microagent.py` | SPEC-runtime §13 + SPEC-interop §3 — MCP servers per-skill |
| **`SecurityAnalyzer` trait** with `async security_risk(action) -> ActionSecurityRisk { LOW \| MEDIUM \| HIGH }`, pluggable backends (Invariant Labs, GraySwan). Paired with `confirmation_mode: bool` + `_pending_action_info` hold-pending pattern | **MIRROR** | V0 `openhands/security/analyzer.py`, `controller/agent_controller.py` | SPEC-hitl-approval §5 — per-action risk classification as a separate pluggable analyzer trait, independent of static tier policy |
| **V1 `SandboxService` ABC**: `search_sandboxes`, `get_sandbox`, `get_sandbox_by_session_api_key`, `start_sandbox`, `resume_sandbox`, `wait_for_sandbox_running`. Implementations: `docker_sandbox_service.py`, `process_sandbox_service.py`, `remote_sandbox_service.py`, `preset_sandbox_spec_service.py` | **MIRROR** | V1 `openhands/app_server/sandbox/sandbox_service.py` | SPEC-tools §6a — SERA's `SandboxProvider` trait lifecycle methods should mirror this shape |
| **Webhook-back sandbox communication** — V1 SDK agent inside sandbox calls *back* to app_server via `OH_WEBHOOKS_0_BASE_URL` + `OH_SESSION_API_KEY` env vars. Inversion of SERA's gateway-push model | LEARN FROM | V1 `openhands/app_server/` | SPEC-gateway — document both gateway-push and webhook-back as valid transport patterns for NAT'd / remote harnesses |
| Tool dispatch: MCP tools and native tools share the same `Action`/`Observation` pipeline via `MCPClientTool` wrapper; no special routing | MIRROR | V0 `openhands/mcp/client.py` | SPEC-interop §3 — unified tool dispatch |
| Custom license (`NOASSERTION`) | **WARNING** | GitHub license metadata | Legal review required before copying any code — MIRROR is clean-room and license-free |

### 10.11 `Kilo-Org/kilocode` — Mode-based coding agent (TypeScript)

**Source:** [github.com/Kilo-Org/kilocode](https://github.com/Kilo-Org/kilocode) — MIT, TypeScript. Clean-room evolution of Roo Code / Cline / opencode; uses `packages/opencode/` as its CLI core with Kilo-specific changes marked `// kilocode_change`.
**Why it matters:** Converges on the **`AGENTS.md` cross-tool open standard** and supplies a concrete mode-manifest format, rules layering, and skills progressive-disclosure pattern.

| What | Classification | Evidence | Spec to update |
|---|---|---|---|
| **Agent-as-Markdown-with-YAML-frontmatter** — manifest fields: `description`, `mode: primary\|subagent\|all`, `color`, `permission { edit: { glob: "allow"\|"ask"\|"deny" }, bash, skill }`, `model`, `steps`, `temperature`, `hidden`, `disable`. Body is the system prompt | **MIRROR** | `packages/kilo-docs/pages/customize/custom-modes.md` | SERA `templates/` + `agents/` — replace YAML-only manifests; body-is-system-prompt convention |
| **Discovery scan order**: `~/.config/kilo/agent/*.md` → `.kilo/agents/*.md` → `.opencode/agents/*.md` → `agent` key in `kilo.jsonc` | MIRROR | `packages/opencode/src/agent/agent.ts` | SERA skill/agent loader — priority chain |
| **Four-layer rules priority stack** (highest to lowest): agent prompt → project `kilo.jsonc` `instructions[]` glob refs → **`AGENTS.md` (write-protected, root-level)** → global `~/.config/kilo/kilo.jsonc` | **MIRROR** | `packages/kilo-docs/pages/customize/agents-md.md` | SPEC-runtime §4 — context engine rules layering |
| **`AGENTS.md` as the cross-tool open standard** — write-protected, subdirectory merging with child-wins. Loaded at task start. kilocode, openhands (as `RepoMicroagent` compat), spec-kit, opencode all read it. **SERA should emit and consume `AGENTS.md`, not invent a new name** | **MIRROR (cross-tool interop)** | kilocode + openhands + spec-kit convergence | SPEC-runtime §13 — first-class `AGENTS.md` support |
| **`SKILL.md` skill progressive disclosure** — only `name` + `description` metadata in system prompt; full body loads on demand via `read skill` tool call. Solves context-window bloat directly | **MIRROR (high leverage)** | `packages/opencode/src/skill/index.ts` | SPEC-runtime §13 (skills) — two-level disclosure: metadata always, body on-demand |
| **Per-agent permission ACL with glob values**: `permission.edit["*.md"] = "allow"`, `permission.edit["*"] = "deny"`, `permission.bash = "deny"`. Layers on top of sandbox tier | **MIRROR** | `packages/opencode/src/agent/agent.ts` frontmatter schema | SPEC-tools §5 — per-agent ACL independent of tier policy; stricter-wins composition |
| **Wave-based orchestrator prompt** (`orchestrator.txt`) — parallel tool calls per wave, explicit `explore` (read-only) vs `general` (full-access) subagent taxonomy, orchestrator never direct-edits files | MIRROR | `packages/opencode/src/agent/prompt/orchestrator.txt` | SPEC-circles — `Parallel` policy wave scheduling; built-in orchestrator persona |
| **Two built-in subagents**: `explore` (read-only, search-focused) + `general` (full-access) — minimal viable delegation taxonomy | MIRROR | `packages/opencode/src/agent/` | SPEC-runtime §13 — ship `explore` + `general` as default subagent types |
| **Snapshot at step boundaries, revert at message granularity** — `git write-tree` per step in detached snapshot repo at `~/.local/share/kilo/snapshot/<project-id>/<worktree-hash>/`, UI revert at turn boundaries, hourly `git gc --prune=7.days` | **MIRROR** | `packages/kilo-docs/pages/code-with-ai/features/checkpoints.md` | SPEC-gateway §6 — session persistence with shadow git repo |
| **Two-layer approval**: `exec-approvals.json` (what commands tools may run) separate from tool-policy (what tools exist); stricter wins | MIRROR | `packages/kilo-docs/pages/kiloclaw/control-ui/exec-approvals.md` | SPEC-hitl-approval — complement static tier policies |
| **Org-mode library backed by PostgreSQL `config jsonb` column** — enterprise multi-tenant mode storage with marketplace sync | LEARN FROM | `packages/kilo-docs/pages/contributing/architecture/organization-modes-library.md` | SPEC-deployment §3 — `jsonb` for non-critical config fields avoids schema migrations |
| **Memory bank pattern (`.kilocode/rules/memory-bank/`)** deprecated upstream in favor of `AGENTS.md` | **IGNORE** | `agents-md.md` deprecation note | Don't implement memory bank in SERA |
| VS Code extension internals, webview-based UI | **IGNORE** | `packages/kilo-vscode/` | SERA is headless |

### 10.12 `OpenBMB/ChatDev` — DAG workflow engine with explicit loop terminators (Python)

**Source:** [github.com/OpenBMB/ChatDev](https://github.com/OpenBMB/ChatDev) — Apache-2.0. **Two branches: 1.0 (academic waterfall virtual-software-company on `chatdev1.0` branch) and 2.0 "DevAll" on `main` (production DAG workflow engine).** The 2.0 branch is the relevant one for SERA.
**Why it matters:** Fills concrete gaps in SERA's `Supervised`/`Consensus` coordination policies — explicit loop terminators, edge-level verdicts, fan-out/fan-in primitives.

| What | Classification | Evidence | Spec to update |
|---|---|---|---|
| **Tarjan SCC super-node cycle detection** — cycles in the DAG are promoted to super-nodes; layers within the DAG run in true parallel; cycles execute recursively via SCC abstraction | **MIRROR** | `workflow/cycle_manager.py`, `workflow/executor/cycle_executor.py`, `docs/user_guide/en/execution_logic.md` | SPEC-circles §2 — DAG execution algorithm for Circles |
| **Three orthogonal loop terminators** — all must be separately configurable: (a) **keyword/verdict condition on exit edge** (`none: [ACCEPT]` keeps looping, `any: [ACCEPT]` exits), (b) **`loop_counter` node** with `max_iterations` (count-based circuit breaker), (c) **`loop_timer` node** with `max_duration` (time-based circuit breaker) | **MIRROR (high leverage)** | `docs/user_guide/en/execution_logic.md` §3.3, `nodes/loop_counter.md`, `nodes/loop_timer.md` | SPEC-circles §3 — `Supervised` policy specifies `convergence_signal`, `max_review_cycles`, `review_timeout` as independent fields |
| **Keyword/verdict condition on edges, not node prompts** — review verdict is an edge condition, not parsed from free-text Reviewer output. Structured `CircleVerdict { Approved \| RevisionRequired(String) \| Escalate }` enum | **MIRROR** | `docs/user_guide/en/execution_logic.md` §5.1 | SPEC-circles §4 — Lead review emits structured verdict; Circle DAG wiring determines phase exit |
| **Map (fan-out) + Tree (fan-out + reduce) dynamic execution** on edges — concrete `Parallel` merge strategies | **MIRROR** | `docs/user_guide/en/dynamic_execution.md` | SPEC-circles §3 — `Parallel` policy with `Collect` (flat list to `ResultAggregator`) + `Reduce { group_size: u32 }` (hierarchical recursive reduction) |
| **`subgraph` node** — inline or file-referenced nested Circle with its own `start` / `end` / variable inheritance; entry = `start` member, exit = `end` member output | **MIRROR** | `docs/user_guide/en/nodes/subgraph.md` | SPEC-circles §2 — `sub_circles` semantics: start is entry, end's output is the single return value to parent |
| **`blackboard` memory** — append-only, recency-ordered, no embeddings, shared by name across nodes. Distinct from per-agent recall memory | **MIRROR** | `docs/user_guide/en/modules/memory.md` | SPEC-memory + SPEC-circles — add `CircleBlackboard` as write-many/read-all intermediate artifact log scoped to Circle session, for `Parallel`/`Consensus` result sharing |
| **Edge `process` block** (regex_extract, function) for payload transformation between nodes | LEARN FROM | `docs/user_guide/en/workflow_authoring.md` §5.1 | SPEC-circles §5 — optional edge transform hook between task handoffs |
| 1.0 Instructor/Assistant role-play prompt pattern + `<INFO> Finished.` sentinel token | LEARN FROM | chatdev1.0 branch, arxiv.org/abs/2307.07924 | SPEC-circles §6 — reference academic pattern for two-agent dialogue |
| **Consensus gap confirmed** — ChatDev does not implement quorum/voting; `# TODO: consensual` is commented out in `process.py` | **SERA must build** | `process.py` | SPEC-circles §4 — `Consensus` policy needs `quorum: f32`, `voting_fn: VotingStrategy { Majority \| Unanimous \| Weighted }`, `consensus_timeout` with fallback to `Collect` |
| 1.0 waterfall phase pipeline (Designing → Coding → Testing) | **IGNORE** | chatdev1.0 branch | SERA already has `Sequential` policy; waterfall is a special case |

### 10.13 `openai/openai-agents-python` — Canonical agent SDK (OpenAI)

**Source:** [github.com/openai/openai-agents-python](https://github.com/openai/openai-agents-python) — MIT, Python, production successor to the deprecated `openai-swarm` educational project. Provider-agnostic; supports Responses API, Chat Completions, and 100+ other LLMs.
**Why it matters:** This is the canonical reference shape by which reviewers will compare SERA's runtime and Circle specs. Adopt the field inventory.

| What | Classification | Evidence | Spec to update |
|---|---|---|---|
| **`Agent` dataclass field inventory**: `name`, `instructions: str \| Fn`, `prompt`, `handoffs`, `model`, `model_settings`, `input_guardrails`, `output_guardrails`, `output_type`, `hooks`, `tools`, `mcp_servers`, `mcp_config`, `tool_use_behavior`, `reset_tool_choice`, `handoff_description` | **MIRROR** | `src/agents/agent.py::Agent` | SPEC-runtime §2 — SERA's `Agent` Rust struct should mirror this field inventory directly |
| **`instructions: str \| Callable[(ctx, agent), Awaitable[str]]`** — static string OR dynamic callable with access to run context | **MIRROR** | `src/agents/agent.py` | SPEC-runtime §2 — persona supports dynamic prompt generation |
| **Handoff-as-tool-call, first-class** — each `Agent` in `handoffs[]` is wrapped into a `Handoff` object with `tool_name` (e.g. `transfer_to_billing`), `tool_description`, `input_json_schema`, `on_invoke_handoff` callback. LLM emits a function call; runner intercepts by name prefix and switches the active agent | **MIRROR (high leverage)** | `src/agents/handoffs/__init__.py::Handoff` | SPEC-circles §4 — delegation is an LLM-visible tool call; runner intercepts by tool name convention |
| **`HandoffInputFilter(input_history, pre_handoff_items, new_items) -> HandoffInputData`** — context passed to receiving agent is fully programmable (strip, summarize, rewrite) | **MIRROR** | `src/agents/handoffs/__init__.py::HandoffInputData`, `HandoffInputFilter` | SPEC-circles §4 — the `subagent_delivery_target` hook signature should match this filter shape |
| **`NextStep` discriminated enum**: `Handoff \| FinalOutput \| RunAgain \| Interruption` — canonical turn-evaluation return type | **MIRROR** | `src/agents/run_internal/run_steps.py` | SPEC-runtime §3 — Rust enum variants for each turn outcome |
| **Two-level hook lifecycle**: `RunHooks` (cross-agent: `on_agent_start/end`, `on_handoff`, `on_tool_start/end`, **`on_llm_start/end`**) + `AgentHooks` (per-agent, receiver side on handoff). Distinct LLM-call boundary events not in codex's SQ/EQ model | **MIRROR** | `src/agents/lifecycle.py::RunHooks`, `AgentHooks` | SPEC-runtime + SPEC-hooks — add `on_llm_start/end` as distinct events from `on_tool_start/end`; two-level RunHooks vs AgentHooks split |
| **Input guardrails run concurrently with the LLM call** (`run_in_parallel=True` default); `GuardrailFunctionOutput { tripwire_triggered: bool, output_info: Any }`. `InputGuardrailTripwireTriggered` exception halts | **MIRROR** | `src/agents/guardrail.py` | SPEC-hooks + SPEC-hitl-approval — `InputGuardrail` trait; run concurrently via `tokio::join!` by default |
| **`tool_use_behavior`** discriminated union: `"run_llm_again"` \| `"stop_on_first_tool"` \| `StopAtTools` \| `ToolsToFinalOutputFunction` — composable short-circuit without subclassing | **MIRROR** | `src/agents/agent.py` | SPEC-runtime §6 — tool loop termination policy |
| **`is_enabled: bool \| Callable[(ctx, agent), bool]`** on tools — dynamic tool visibility per-turn, not static registration | **MIRROR** | `src/agents/tool.py::function_tool` | SPEC-tools §3 — runtime tool gating callback |
| **`needs_approval: bool \| Callable`** on tools — triggers `NextStep::Interruption` for HITL gate | MIRROR | `src/agents/tool.py` | SPEC-hitl-approval — per-tool approval callback |
| **`Session` as Protocol** (not ABC): `async get_items(limit) / add_items(items) / pop_item()`. Backends: `SQLiteSession`, `OpenAIConversationsSession`, `OpenAIResponsesCompactionSession` | **MIRROR** | `src/agents/memory/session.py` | SPEC-gateway §6 — Rust `Session` trait with three methods; no inheritance; swappable backends |
| **MCP re-fetched per turn** via `AgentBase.get_mcp_tools(run_context)` (not cached at agent construction) | **MIRROR** | `src/agents/agent.py`, `src/agents/mcp/` | SPEC-interop §3 — freshness guarantee on MCP tool list |
| **`@function_tool` auto-schema from type hints + docstrings** (Google/NumPy/Sphinx auto-detected), `strict_mode=True` default, tool-level `tool_input_guardrails` / `tool_output_guardrails` | LEARN FROM | `src/agents/tool.py` | SPEC-tools — Rust equivalent via `schemars` + doc comments |
| **`output_type`** accepts dataclass / Pydantic / TypedDict; runner wraps in JSON schema, passes to model as response-format constraint, deserializes via `TypeAdapter` | MIRROR | `src/agents/agent.py` | SPEC-runtime §5.1 — structured output via serde + schemars |
| **`StreamEvent` discriminant**: `RawResponsesStreamEvent \| RunItemStreamEvent \| AgentUpdatedStreamEvent` — three-way union for streaming consumers | MIRROR | `src/agents/stream_events.py` | SPEC-interop §6 (AG-UI) — align stream discriminant |
| **`DEFAULT_MAX_TURNS = 10`** (env-overridable) with clear per-turn loop | LEARN FROM | `src/agents/run_config.py` | SPEC-runtime §5.5 — default cost bound |
| Proprietary `TracingProcessor` shipping to OpenAI's backend | **IGNORE** | `src/agents/tracing/` | SERA uses its own OTel stack |
| Responses API `Prompt` / `DynamicPromptFunction` | **IGNORE** | `src/agents/agent.py` | OpenAI-specific |

### 10.14 `crewAIInc/crewAI` — Crew + Flow multi-agent orchestration (Python)

**Source:** [github.com/crewAIInc/crewAI](https://github.com/crewAIInc/crewAI) — canonical casing `crewAIInc/crewAI`, MIT, v1.14.1, actively maintained. Production multi-agent framework.
**Why it matters:** Delivers two orthogonal coordination primitives: **Crew** (role-based DAG with hierarchical manager LLM) and **Flow** (event-driven state machine). Both are reference-quality and compose.

| What | Classification | Evidence | Spec to update |
|---|---|---|---|
| **`Task.context: list[Task]`** — explicit DAG wiring; framework auto-resolves and **materializes** serialized `TaskOutput` into downstream prompts. Agents never see raw message IDs | **MIRROR** | `lib/crewai/src/crewai/task.py` | SPEC-circles §3 — `input_refs: Vec<TaskId>` first-class on task descriptors; gateway materializes before dispatch |
| **`Process.hierarchical` with manager LLM** — manager is an LLM session with system prompt + worker roster + `DelegateWorkTool`; workers have `allow_delegation=False`. Manager is auto-created from `manager_llm` if not provided | **MIRROR (high leverage)** | `lib/crewai/src/crewai/crew.py`, `process.py` | SPEC-circles §3 — `Supervised` policy is a ManagerSession holding `parent_session_key` dispatching via `subagent_delivery_target`. No special coordinator plumbing — just a regular LLM session with delegation tool |
| **`DelegateWorkTool(task, context, coworker)`** + **`AskQuestionTool`** — pre-registered when `allow_delegation=True`; LLM-visible schema; coworker resolved by `role` string match | **MIRROR** | `lib/crewai/src/crewai/tools/agent_tools/delegate_work_tool.py` | SPEC-circles §4 — delegation surfaced as tool call, not framework routing. Auditable in tool-call trace |
| **`Flow` event-driven state machine** — decorators: `@start(condition?)`, `@listen(method \| or_(...) \| and_(...))`, `@router(method) -> string`. Compound triggers via `or_()` / `and_()`. Nested `FlowCondition` AND/OR trees. Typed `FlowState` Pydantic model. `FlowPersistence` pluggable (SQLite default) | **MIRROR** | `lib/crewai/src/crewai/flow/flow.py`, `flow_wrappers.py` | SPEC-circles — **`CoordinationPolicy::Custom` is a Flow**: each handler is a stage, state is the typed envelope, `@router` is a conditional edge. A Flow can embed Crew kickoffs as `@listen` handlers — compose, don't compete |
| **`@human_feedback` decorator** on Flow methods — pauses the flow, serializes state, resumes asynchronously when feedback arrives | **MIRROR** | `lib/crewai/src/crewai/flow/human_feedback.py` | SPEC-hitl-approval §6 — async HITL with state serialization |
| **`Task.guardrail` + retry loop** — callable (or LLM-evaluated string) validates output before next task; retries up to `max_retry_limit`. `response_model` uses native provider structured-output APIs | **MIRROR** | `lib/crewai/src/crewai/utilities/guardrail.py` | SPEC-circles §4 — `ResultAggregator` runs `validate(output) -> GuardrailResult` before marking task complete; on failure requeues to same agent |
| **Task-level fields**: `description`, `expected_output`, `agent`, `context`, `async_execution`, `output_json`, `output_pydantic`, `response_model`, `output_file`, `human_input`, `guardrail`, `callback`, `markdown`, `input_files`, `security_config` | MIRROR | `lib/crewai/src/crewai/task.py` | SPEC-circles §3 — canonical task descriptor field inventory |
| **Unified Memory model** (v1.x): single Pydantic `Memory` with LLM-inferred scope/category/importance, `recency + semantic + importance` composite score, `RecallFlow` adaptive-depth recall, `MemoryScope` / `MemorySlice` views, consolidation-on-save at similarity threshold 0.85 | LEARN FROM | `lib/crewai/src/crewai/memory/unified_memory.py`, `memory/recall_flow.py` | SPEC-memory — unified memory as a simpler alternative to four-tier split. **Pre-1.0 four-layer docs (ShortTerm/LongTerm/Entity/Contextual) are superseded** |
| **`CrewPlanner` crew-level planning pass** (pre-kickoff) — annotates tasks with plan injected into prompts. Plans **mutate prompts, not the task graph** | LEARN FROM | `lib/crewai/src/crewai/utilities/planning_handler.py` | SPEC-circles — optional `PlanningPhase` hook in Circle lifecycle; patches task descriptors before distribution |
| **Training mode** (`crew.train(n_iterations, filename)`) — captures human feedback per agent per iteration, persists to disk, re-injects into system prompts on subsequent runs | LEARN FROM | `lib/crewai/src/crewai/utilities/training_handler.py` | SPEC-runtime §13 — per-agent experiential learning from human feedback |
| **`allow_delegation: bool` as capability flag** that injects delegation tools | MIRROR | `lib/crewai/src/crewai/agents/agent_builder/base_agent.py` | SPEC-runtime — `capabilities: HashSet<AgentCapability>` where `Delegation` injects tools |
| `share_crew`, telemetry fields | **IGNORE** | `crew.py` | SERA owns its own observability |
| `consensual` process (commented out, `# TODO`) | **IGNORE (confirms gap)** | `process.py` | SERA still must build `Consensus` from scratch |

### 10.15 `FoundationAgents/MetaGPT` — SOP-as-watch-graph (Python)

**Source:** [github.com/FoundationAgents/MetaGPT](https://github.com/FoundationAgents/MetaGPT) (canonical org — `geekan/MetaGPT` redirects here), MIT, actively maintained.
**Why it matters:** Provides the **`Action` vs `Tool`** separation and the **`cause_by` typed routing key** that make SOPs declarative and inspectable without a coordinator.

| What | Classification | Evidence | Spec to update |
|---|---|---|---|
| **`Action` as typed BaseModel distinct from `Tool`** — carries its own LLM binding (`llm_name_or_type`), system-prompt prefix, structured output schema via `ActionNode`, description (`desc`). Reused across Roles via `Role.set_actions([WriteCode, WriteTest])` | **MIRROR (high leverage)** | `metagpt/actions/action.py::Action` | SPEC-runtime — introduce `Action` trait distinct from `Tool`. `Action` owns model binding + structured output schema + `cause_by` identifier. `Tool` remains low-level callable. An Action may invoke Tools internally |
| **`cause_by: str` on Message — typed routing discriminant** — string name of the Action class that produced the message. Enables declarative subscription via `_watch([ActionClass])` without a coordinator | **MIRROR (high leverage)** | `metagpt/schema.py::Message`, `roles/role.py::Role._watch` | SPEC-circles + SPEC-hooks — `Message.cause_by: ActionId` field. `subagent_delivery_target` hook dispatch keyed on `cause_by` match |
| **Role lifecycle**: `_observe` (content-addressed filter: `n.cause_by in rc.watch OR self.name in n.send_to`) → `_think` (LLM picks next state or deterministic by `react_mode`) → `_act` (runs `rc.todo.run(rc.history)`) → `_react` loop with `max_react_loop` | **MIRROR** | `metagpt/roles/role.py::Role` | SPEC-runtime §3 — four-method harness lifecycle; observe does content-addressed filtering at delivery time |
| **`react_mode` per-role enum**: `REACT` (LLM-selected action each turn), `BY_ORDER` (sequential through `actions[]`), **`PLAN_AND_ACT`** (LLM builds plan first, then executes via `Planner`). Different roles in the same Team can use different react_modes — not a per-framework choice | **MIRROR** | `metagpt/roles/role.py::RoleReactMode` | SPEC-circles §3 — `CoordinationPolicy` maps: `Sequential → BY_ORDER`, `Supervised → PLAN_AND_ACT`, `Parallel` stays parallel |
| **Environment push-to-inbox filtered pub/sub (NOT blackboard)** — `publish_message` iterates `member_addrs` and pushes directly into each role's private `msg_buffer` at delivery time. Routing is delivery-time, not read-time | **MIRROR** | `metagpt/environment/base_env.py::Environment` | SPEC-gateway §3 — push-to-per-agent-queues filtered by `send_to` / `cause_by` match; no global shared buffer that agents poll |
| **`ActionNode` per-field structured output schema** — each Action defines `expected_type`, `instruction`, `example` per field; LLM fills a Pydantic model, not a freeform string | **MIRROR** | `metagpt/actions/action_node.py` | SPEC-runtime §5.1 — schema-enforced Action output, not downstream parsing |
| **Team termination triad**: `n_round` countdown **OR** `env.is_idle` (all roles have empty buffers and no `todo`) **OR** cost budget exhaustion (`NoMoneyException`). All three orthogonal | **MIRROR** | `metagpt/team.py::Team.run` | SPEC-workflow-engine §5 — three-way termination with `CostBudget` guard |
| **`Team.hire(roles)` + `Team.invest(cost)`** — simple composition API | LEARN FROM | `metagpt/team.py` | SPEC-circles — `Circle.add_agent(role)` + cost policy |
| **`Memory.index[cause_by]`** — append-log with `cause_by` index for O(1) filtered retrieval | MIRROR | `metagpt/memory/memory.py` | SPEC-memory — recall index by `ActionId` |
| **`RoleZero` experience pool** — retrieval-augmented action selection from prior successful runs via `exp_cache` + `RoleZeroLongTermMemory`. `tool_execution_map` dispatches structured JSON command outputs | LEARN FROM | `metagpt/roles/di/role_zero.py`, `memory/role_zero_memory.py` | SPEC-memory §6 — experiential memory tier |
| **`DataInterpreter`** — `react_mode = "plan_and_act"`, Jupyter kernel via `ExecuteNbCode`, BM25 tool recommendation from tool registry | LEARN FROM | `metagpt/roles/di/data_interpreter.py` | SPEC-tools — BM25 tool-selection pattern for large tool registries |
| **SOP as implicit watch-graph, not a DSL** — no `sop.yaml`, no SOP class. The SOP *is* the `_watch` graph. **Key design lesson: SERA shouldn't build a DSL for Circle workflow; workflow emerges from each agent's `watch_signals` declaration** | **LEARN FROM (architectural)** | `metagpt/team.py` docstring: "Possesses one or more roles, SOP, and a env" | SPEC-circles — Circle YAML declares only `watch_signals` per agent + `CoordinationPolicy` enum; rest is runtime behavior |
| `LongTermMemory` base class (commented out of `RoleContext`, replaced by role-specific `RoleZeroLongTermMemory`) | **IGNORE** | `metagpt/memory/` | Unstable; not active on main |
| `MGXEnv` proprietary environment (`use_mgx: bool = True` default) | **IGNORE** | `team.py` | Not open; use `Environment` explicitly |

### 10.16 `i-am-bee/beeai-framework` — LF AI framework with ACP→A2A migration playbook (Python + TypeScript)

**Source:** [github.com/i-am-bee/beeai-framework](https://github.com/i-am-bee/beeai-framework) — Apache-2.0, Linux Foundation AI & Data project (originated at IBM, donated). Dual Python + TypeScript SDKs maintained in parallel. Actively maintained.
**Why it matters:** **BeeAI is the canonical real-world example of exactly the ACP→A2A migration SERA needs to do.** The transition playbook is directly copyable.

| What | Classification | Evidence | Spec to update |
|---|---|---|---|
| **ACP→A2A migration playbook** (SERA's exact problem): (1) keep old adapter in-tree but deprecated — `adapters/acp/` still compiles; (2) new adapter is a structural twin with identical `BaseAgent` interface — callers change one import line; (3) optional install extras — `beeai-framework[acp]` vs `[a2a]` prevents mandatory retired-protocol SDK dependency; (4) migration guide lives in a dedicated ADR/docs page, not inline comments; (5) framework-level dated announcement in README "Latest Updates" table linking to LF decision | **MIRROR (directly applicable)** | `python/beeai_framework/adapters/acp/`, `python/beeai_framework/adapters/a2a/`, `docs/integrations/a2a.mdx`, ACP archived at [i-am-bee/acp](https://github.com/i-am-bee/acp) with `archived: true` | SPEC-interop §4, §5 — adopt the playbook exactly: gate ACP behind a Cargo feature, A2A adapter is the default, migration guide is a dedicated ADR. The archival of `i-am-bee/acp` is the canonical evidence for dropping ACP from SERA |
| **Four-tier memory ABC** directly validated: `UnconstrainedMemory` + `TokenMemory` + `SlidingWindowMemory` + **`SummarizeMemory` (LLM-driven compaction)** + `ReadOnlyMemory` wrapper | **MIRROR** | `python/beeai_framework/memory/base_memory.py`, `memory/unconstrained_memory.py`, `token_memory.py`, `sliding_memory.py`, `summarize_memory.py`, `readonly_memory.py` | SPEC-memory — SERA's `MemoryBackend` trait ships these exact four variants + the read-only wrapper |
| **Hierarchical `Emitter` namespace tree** — every runtime entity (agent, tool, workflow) owns a child emitter forked from a root singleton. `EventMeta { id, name, path: Vec<String>, created_at, source, creator, context, group_id, trace: Option<EventTrace>, data_type }`. Listeners match by string name, `re.Pattern`, or predicate. **Pattern-matched subscriptions without regex on the hot path** via namespace path prefix | **MIRROR (high leverage)** | `python/beeai_framework/emitter/emitter.py` | SPEC-observability §2 — SERA's `ObservabilityEvent` adopts namespace-path + trace correlation; hierarchical child emitters per runtime entity |
| **`Workflow[T, K]` step sentinels**: `START` / `SELF` / `PREV` / `NEXT` / `END` — handlers return a sentinel instead of explicit transition tables. State is a single shared Pydantic model | **MIRROR** | `python/beeai_framework/workflows/workflow.py` | SPEC-workflow-engine + SPEC-circles — step sentinels as cleaner alternative to explicit transition maps within Circle phases |
| **`HandoffTool(Runnable, propagate_inputs: bool)`** — sub-agent as synchronous tool. Matches CrewAI's `DelegateWorkTool` pattern — confirms cross-framework convergence on delegation-as-tool | MIRROR | `python/beeai_framework/tools/handoff.py` | SPEC-circles §4 |
| **`BaseCache[T]` ABC** with `set / get / has / delete / clear / size / generate_key`. `generate_key(*args)` SHA-512 hashes Pydantic models/dicts — content-addressed cache keys. Implementations: `NullCache`, `UnconstrainedCache`, `SlidingCache`, `DecoratorCache`. **Cache is tool-level, pluggable — no built-in LLM-call cache at provider layer** | **MIRROR** | `python/beeai_framework/cache/base.py` | SPEC-tools §6 — add `Cache` trait with SHA-512 content-addressed keys; tool-level cache; no provider-layer cache |
| **`Tool` interface** with Pydantic `input_schema` property — no decorator magic; declare a BaseModel subclass. Schema derived via `.to_json_safe() -> { name, description, input_schema }`. Events (`ToolStartEvent`, `ToolSuccessEvent`, `ToolErrorEvent`, `ToolRetryEvent`) emitted via tool's own `Emitter` child | MIRROR | `python/beeai_framework/tools/tool.py` | SPEC-tools §3 — schema-first tool definition |
| **`Run[R]`** lazy handle returned by `BaseAgent.run()` — supports `.on("event", callback)` chaining before awaiting. `AgentOptions { expected_output, total_max_retries, max_retries_per_step, max_iterations, backstory }` | LEARN FROM | `python/beeai_framework/agents/base.py::BaseAgent` | SPEC-runtime §3 — lazy Run handle with event subscription before execution |
| **Dual Python + TypeScript SDK parity** with intentionally independent tooling (Poetry + tsup) — parity in structure, not implementation | LEARN FROM | `python/` vs `typescript/src/` | SERA's future client SDKs (sera-sdk-ts, sera-sdk-py) — maintain structural parity, independent toolchains |
| **`PromptTemplate` uses Python string formatting, not Jinja2**; a recent bugfix ("deep copy PromptTemplate config to prevent shared mutable state across agents") confirms templates are mutable config objects that must be cloned per-agent | LEARN FROM | `python/beeai_framework/template.py` | SPEC-runtime — persona/prompt templates cloned per agent instance, not shared |
| ACP adapter (as protocol) | **IGNORE (archived upstream)** | [i-am-bee/acp](https://github.com/i-am-bee/acp) `archived: true`, last push 2025-08-25 | SPEC-interop §5 — canonical evidence for dropping ACP |

### 10.17 `camel-ai/camel` + `camel-ai/owl` — Role-playing + Workforce (Python)

**Source:** [github.com/camel-ai/camel](https://github.com/camel-ai/camel) (framework, Apache-2.0) + [github.com/camel-ai/owl](https://github.com/camel-ai/owl) (thin app wrapper, NeurIPS 2025, topped GAIA at 69.09%, arxiv.org/abs/2505.23885). OWL imports CAMEL directly — no re-implementation.
**Why it matters:** Supplies a production **`TaskChannel` lifecycle state machine**, a **runtime-switchable decomposition mode** missing from every other framework, and a **pause/resume pattern** for human intervention.

| What | Classification | Evidence | Spec to update |
|---|---|---|---|
| **`TaskChannel` + `Packet` lifecycle state machine**: `PacketStatus = SENT → PROCESSING → RETURNED → ARCHIVED`. Backed by `asyncio.Condition` + `Dict[str, Packet]` (O(1) lookup) + `Dict[PacketStatus, Set[str]]` (status index). Coordinator routes `Packet`-wrapped tasks to registered workers | **MIRROR (high leverage)** | `camel/societies/workforce/task_channel.py` | SPEC-gateway §5 + SPEC-circles §4 — SERA's atomic task checkout adopts `Packet { task, publisher_id, assignee_id, status }` + four-state lifecycle with explicit `ARCHIVED` terminal |
| **`WorkforceMode = AUTO_DECOMPOSE \| PIPELINE`** — runtime-switchable decomposition strategy within the same Workforce. `AUTO_DECOMPOSE` = LLM-driven dynamic DAG decomposition; `PIPELINE` = predefined sequential. **Genuinely unique: CrewAI has fixed flows, MetaGPT has fixed SOPs, neither is runtime-switchable** | **MIRROR** | `camel/societies/workforce/workforce.py::WorkforceMode` | SPEC-circles §3 — `CoordinationPolicy` adds a `Pipeline` variant alongside `Sequential`/`Parallel`/`Supervised`/`Consensus`. Pipeline = predefined ordered steps, no dynamic decomposition |
| **`WorkforceState = IDLE \| RUNNING \| PAUSED \| STOPPED`** + **`WorkforceSnapshot`** for serialized pause/resume — human intervention primitive | **MIRROR** | `camel/societies/workforce/workforce.py` | SPEC-gateway §6 — session state machine adds `PAUSED` state and snapshot/resume serialization; missing from SERA's current state model |
| **Production operational defaults**: `MAX_TASK_RETRIES = 3`, `MAX_PENDING_TASKS_LIMIT = 20`, `TASK_TIMEOUT_SECONDS = 600.0`, `DEFAULT_WORKER_POOL_SIZE = 10` | LEARN FROM | `camel/societies/workforce/workforce.py` | SPEC-circles §6 — ship similar defaults in the reference Circle config |
| **`validate_task_content()` with failure-pattern blacklist** — checks output for `"I cannot complete"`, `"task failed"`, etc. before accepting a result. **Prevents silent hallucinated completions from propagating**. Not in CrewAI, BeeAI, or openai-agents-python | **MIRROR (high leverage)** | `camel/societies/workforce/utils.py::validate_task_content`, `RecoveryStrategy` enum | SPEC-circles §4 — `ResultAggregator::validate()` includes a failure-pattern blacklist step before marking task complete |
| **`TaskSpecifyAgent` pre-pass** — dedicated agent class that sharpens the vague task into a specific one via LLM call **before** any execution. `RolePlaying.with_task_specify=True` | **MIRROR** | `camel/societies/role_playing.py::RolePlaying`, `agents/task_agents.py` | SPEC-circles — new `TaskSpecifier` lifecycle stage runs before Circle dispatch; sharpens prompt via dedicated planner agent. Fills the "no pre-execution task sharpening" gap |
| **`SystemMessageGenerator`** keyed on `TaskType` enum — role-conditioned system prompt templates (`AI_SOCIETY`, `CODE`, `SCIENCE`) as a formal registry. Not ad-hoc prompt engineering | MIRROR | `camel/generators.py::SystemMessageGenerator`, `TaskType` | SPEC-runtime §4.3 — persona templates keyed by task type; formal template registry |
| **`RolePlaying` two-agent protocol** with optional `CriticAgent` or `Human` interception at each turn, `stop_event: threading.Event` termination, `TaskPlannerAgent` optional step-planning pass | MIRROR | `camel/societies/role_playing.py::RolePlaying` | SPEC-circles §6 — `RolePlayingCircle` primitive: two agents + optional critic + stop signal; wrapped as `RolePlayingWorker` inside a Workforce |
| **`RolePlayingWorker` composition** — Workforce worker that wraps a RolePlaying session. Primitives compose: a 2-agent dialogue becomes one Workforce worker | **MIRROR** | `camel/societies/workforce/role_playing_worker.py` | SPEC-circles — Circle workers can themselves be sub-Circles |
| **GAIA canonical 3-agent workforce**: Web Agent + Document Processing Agent + Reasoning/Coding Agent. L1 = single-hop retrieval, L2 = multi-step tool chaining, L3 = cross-modal reasoning. **Minimum viable general-purpose workforce** | LEARN FROM | OWL README; `examples/run.py`; paper | SPEC-circles — ship a canonical 3-agent eval Circle; assign GAIA-style difficulty tiers to Circle tasks; measure pass@1 per tier |
| **Evaluation branches vendor a pinned CAMEL fork** in `owl/camel/` rather than using pip — reveals benchmark stability requires pinned tool versions | LEARN FROM | OWL `gaia58.18` / `gaia69` branches | SPEC-dependencies + SPEC-circles — evaluation Circles need deterministic tool versions, not floating dependencies |
| **Three memory tiers**: `ChatHistoryBlock` (sliding window, token-bounded via `BaseContextCreator` score-based trimming) + `VectorDBBlock` (semantic retrieval) + `WorkflowMemoryManager` (coordinator-scoped cross-task summary within a Workforce run) | MIRROR | `camel/memories/base.py`, `memories/blocks/`, `societies/workforce/workflow_memory.py` | SPEC-memory — three-tier memory model confirmed; `WorkflowMemoryManager` is the new primitive (coordinator-scoped, not per-agent) |
| **MCP as first-class toolkit adapter** — `MCPToolkit` converts MCP server tool schemas to strict OpenAI JSON Schema via `ensure_strict_json_schema()`, exposes as `FunctionTool` — MCP servers indistinguishable from native tools in the agent loop | MIRROR | `camel/toolkits/mcp_toolkit.py` | SPEC-interop §3 — MCP tools register as first-class into the same tool registry; no special dispatch path |
| OWL Gradio webapp, vendored CAMEL in GAIA branch | **IGNORE** | `owl/webapp.py`, `owl/camel/` | Python-specific / benchmark-specific |

### 10.18 `NVIDIA/OpenShell` — Rust-based sandbox runtime with published mTLS gRPC protocol

**Source:** [github.com/NVIDIA/OpenShell](https://github.com/NVIDIA/OpenShell) — **Apache-2.0, Rust (edition 2024, rust-version 1.88), 12 crates, self-described alpha ("proof-of-life: one developer, one environment, one gateway"), actively developed.**
**Critical correction:** NemoClaw framed OpenShell as "part of NVIDIA Agent Toolkit." The OpenShell repo itself makes no such claim. **OpenShell is a standalone Apache-2.0 Rust project that publishes its full enforcement stack and mTLS gRPC protocol.** This is the most directly actionable finding of the entire research program for SERA's sandbox story.
**Architecture:** K3s cluster runs inside a single Docker container. Gateway is a K8s StatefulSet (gRPC+HTTP on 8080, mTLS). Each sandbox is a K8s Pod with a privileged Supervisor process (SSH server + HTTP CONNECT proxy + in-process OPA + inference router + TLS MITM cache) and a restricted Agent process.

| What | Classification | Evidence | Spec to update |
|---|---|---|---|
| **Published versioned mTLS-authenticated gRPC protocol** — `proto/openshell.proto` + `proto/sandbox.proto` + `proto/datamodel.proto`, Apache-2.0. Full RPC surface: `CreateSandbox`, `DeleteSandbox`, `ListSandboxes`, `GetSandboxConfig`, `GetSandboxProviderEnvironment`, `GetInferenceBundle`, `PushSandboxLogs` (client-streaming), `WatchSandbox` (server-streaming), `UpdateConfig`, `SubmitPolicyAnalysis`, `ApproveDraftChunk`, `ExecSandbox` (server-streaming). Single port (8080 in-cluster, 30051 NodePort). **No REST API — gRPC only.** Python SDK (`openshell` on PyPI) wraps the client | **DEPEND ON (Tier-3 sandbox backend)** | `proto/openshell.proto`, `proto/sandbox.proto`, `proto/datamodel.proto` | SPEC-tools §6a — SERA adds `OpenShellSandboxProvider` as a `SandboxProvider` trait implementation. Generate Rust stubs with `tonic-build` from the published protos; connect with `tonic` + mTLS from an `OpenShellConfig { endpoint, ca_cert, client_cert, client_key }` struct |
| **`SandboxPolicy` proto schema** — `version`, `filesystem`, `landlock`, `process`, `map<string, NetworkPolicyRule>`. `NetworkPolicyRule { name, endpoints, binaries }`. `NetworkEndpoint { host (glob), port, ports, protocol (rest\|sql\|""), tls (terminate\|passthrough), enforcement (enforce\|audit), access (read-only\|read-write\|full), rules: [L7Rule], allowed_ips: [CIDR] }`. **This is the canonical network policy schema.** | **MIRROR** | `proto/sandbox.proto::SandboxPolicy` | SPEC-tools §6a — SERA's own sandbox policy model mirrors this shape. `map<string, NetworkPolicyRule>` keyed by rule name enables named hot-reload. Supersedes NemoClaw's policy shape since NemoClaw's YAML is a 1:1 view of this proto |
| **Per-endpoint `allowed_ips: [CIDR]` field** — SSRF mitigation at policy level, not just host-pattern filtering. Not in NemoClaw, claw-code, codex, opencode | **MIRROR** | `proto/sandbox.proto::NetworkEndpoint` | SPEC-tools §6a + SPEC-security — add CIDR allowlist to SERA's network policy schema |
| **`access: "read-only" \| "read-write" \| "full"` preset shorthand** — expands to explicit L7 rules; reduces operator burden for common cases | MIRROR | `proto/sandbox.proto::NetworkEndpoint` | SPEC-tools §6a |
| **In-process OPA via `regorus` crate** (Rust-native Rego evaluator) — policy evaluation is in-process, not a forked OPA binary or sidecar | **DEPEND ON (crate)** | `crates/openshell-sandbox/src/proxy.rs` uses `regorus` | SPEC-tools §6a + SPEC-hooks — SERA adds `regorus` as a Rust dependency for policy evaluation; no external OPA process |
| **Custom Rust HTTP CONNECT proxy** (not Envoy) — intercepts all outbound traffic via netns veth pair, evaluates against OPA/Rego, performs TLS MITM with sandbox CA for `terminate` endpoints, enforces L7 method+path rules. Single in-process component of the supervisor binary | LEARN FROM | `crates/openshell-sandbox/src/proxy.rs` | SPEC-tools §6a — SERA's native sandbox implementation should use a custom Rust CONNECT proxy for full control, not sidecar Envoy |
| **Hot-reload policy with version tracking** — `PolicyStatus { PENDING, LOADED, FAILED, SUPERSEDED }` enum, monotonic version numbers, SHA-256 integrity hashes. Sandboxes poll `GetSandboxConfig` for updates. Static fields (filesystem, landlock, process) are locked at creation; dynamic fields (network_policies, inference) are hot-reloadable | **MIRROR** | `proto/sandbox.proto`, supervisor poll loop | SPEC-tools §6a — SERA's policy update path distinguishes static-at-creation from dynamic-hot-reloadable; versioned with SHA-256 integrity |
| **Binary identity via SHA-256 trust-on-first-use** — `NetworkPolicy.binaries: [{ path }]` matched against process binary at connection time; identity bound to content hash on first observation. Prevents agent process substitution attacks | **MIRROR** | `crates/openshell-sandbox/src/identity.rs` | SPEC-tools §6a + SPEC-security — per-binary network authorization with TOFU content-hash binding, beyond NemoClaw's path-only binding |
| **OCSF v1.7.0 structured audit events** — Open Cybersecurity Schema Framework, JSONL to `/var/log/openshell-ocsf.YYYY-MM-DD.log`. Event classes: `4001` Network Activity, `4002` HTTP Activity, `4007` SSH Activity, `1007` Process Activity, `2004` Detection Finding, `5019` Device Config State Change, `6002` Application Lifecycle. Each event carries `actor.process.name`, `dst_endpoint`, `firewall_rule.name`, `action`/`disposition`. SIEM-ready (Splunk OCSF Add-on, Amazon Security Lake, Elastic Filebeat) | **MIRROR** | `crates/openshell-ocsf/` | SPEC-observability — SERA adopts OCSF v1.7.0 class UIDs for network/process/HTTP audit events. Makes SERA's audit log SIEM-compatible without custom schema work. **No OpenTelemetry dependency at this layer** — OCSF is complementary |
| **AI-assisted policy advisor** — sandbox aggregates denial events, proposes policy rules (mechanistically or via LLM), `SubmitPolicyAnalysis` / `GetDraftPolicy` / `ApproveDraftChunk` RPC chain lets operators approve/reject/edit individual chunks. **Not in any prior research — unique contribution** | **MIRROR** | `proto/openshell.proto`, `crates/openshell-server/` | SPEC-tools §6a + SPEC-config — SERA's policy workflow adopts a denial-aggregation → draft-proposal → chunk-approval loop |
| **Credential injection via `GetSandboxProviderEnvironment` gRPC** — supervisor calls this at sandbox startup, receives `map<string, string>` of env vars, injects into agent process. **Credentials never touch the sandbox filesystem**. Auto-discovery: CLI reads known env vars (`ANTHROPIC_API_KEY`, etc.) from host shell | **MIRROR** | `proto/openshell.proto`, supervisor startup | SPEC-secrets — SERA's secret injection via gateway gRPC at sandbox startup; no filesystem transit |
| **`inference.local` virtual host pattern** — all inference requests go to `inference.local:443`; proxy intercepts and rewrites `model` field + injects auth headers. Provider profiles (openai, anthropic, nvidia NIM) via `GetInferenceBundle → ResolvedRoute { base_url, api_key, protocols, model_id, provider_type }`. **Cleaner than per-provider egress rules** | **MIRROR** | `crates/openshell-router/`, `proto/openshell.proto::ResolvedRoute` | SPEC-dependencies §9.1 + SPEC-tools §6a — SERA's harness routes all inference through a single virtual hostname; proxy rewrites at L7. Provider metadata carries `credential_env`, `dynamic_endpoint`, `timeout_secs` (matching NemoClaw/`genai`) |
| **`ResolvedRoute` provider profile schema** — `{ base_url, api_key, protocols, model_id, provider_type }` | MIRROR | `proto/openshell.proto::ResolvedRoute` | SPEC-runtime §5.2 — model routing config shape |
| **K3s-in-Docker runtime substrate** — one Docker container wraps full K3s cluster; sandboxes are K8s Pods managed by `AgentSandbox` CRD controller in `agent-sandbox-system` namespace; Helm chart in `deploy/helm/` | **LEARN FROM** | `deploy/helm/`, `agent-sandbox-system` namespace | SPEC-deployment §3 — evaluate for SERA's Tier-3 enterprise mode; heavy for Tier-1/2 |
| Agent process has **no special ABI** — ordinary Unix process with outbound TCP forced through HTTP CONNECT proxy at 10.200.0.1:3128 via netns + iptables; credentials as env vars; SSH access via embedded `russh` server; inference only via `inference.local:443` | LEARN FROM | `architecture/sandbox.md` | SPEC-tools §6a — SERA's SandboxProvider contract should be this minimal: no ABI, only transport redirection |
| **Single-client-cert mTLS PKI** (all sandbox pods share one client cert; identity via `x-sandbox-id` header) | **IGNORE** | OpenShell current PKI | SERA's multi-tenant model requires per-sandbox certificates |
| K3s/Kubernetes CRD controller + Helm chart | **IGNORE for core, LEARN FROM for Tier-3** | `deploy/helm/`, CRD | SERA is not Kubernetes-native at the core; a Tier-3 K8s backend is optional |

---

## 11. License Audit

SERA's workspace is intended to be dual-licensed MIT/Apache-2.0 (to be confirmed in SPEC-versioning). All **DEPEND ON** entries in this document use one of: MIT, Apache-2.0, dual MIT/Apache-2.0, BSD-3, CC0-1.0. No GPL or AGPL dependencies are accepted.

**Items requiring license verification before adoption:**

1. `a2a-rs` — verify if used as a code reference (MIRROR is license-free).

**Clean-room reimplementation note.** The `claw-code` and `openai/codex` entries in §10 are classified as MIRROR, not code-copy. SERA re-derives the interface shapes, type names, and state machines from observed public behavior and documentation. No source files are copied. Under this policy, the upstream licenses (MIT for `claw-code`, Apache-2.0 for `openai/codex`) are compatible with SERA's intended dual MIT/Apache-2.0 licensing, and even if an upstream were restrictively licensed, clean-room MIRROR use would remain safe.

All other entries are license-confirmed via crates.io metadata or upstream LICENSE files.

---

## 12. Buy-vs-Build Summary

| Area | Verdict | Primary dependency |
|---|---|---|
| MCP wire protocol | **BUY** | `rmcp` 1.x |
| A2A wire protocol | **BUILD from proto** | `prost` + `tonic` against `a2aproject/A2A` |
| ACP wire protocol | **DROP** | Merged into A2A |
| AG-UI streaming | **BUILD (hand-roll)** | Serde enums + `axum::response::sse` |
| WASM hook runtime | **BUY** | `wasmtime` 43 + `wasmtime-wasi-http` |
| Hook guest toolchain | **BUY** | `cargo-component` (Rust), `componentize-py`, `jco` (experimental TS) |
| LLM provider adapters | **BUY** | `genai` (primary) + `async-openai` |
| Structured output | **BUY** | `llguidance` + `outlines-core` |
| Local embeddings | **BUY** | `fastembed` |
| Token counting | **BUY** | `tiktoken` (anysphere) + `tokenizers` |
| Docker sandbox | **BUY** | `bollard` (already in `sera-docker`) |
| WASM sandbox | **BUY (shared with hooks)** | `wasmtime` |
| MicroVM sandbox | **BUILD (wrap binary)** | `firecracker` via `Command` |
| Auth — JWT / OAuth2 / OIDC / RBAC | **BUY** | `jsonwebtoken` + `oauth2` + `openidconnect` + `casbin` |
| Auth — AuthZen PDP client | **BUILD** | ~60 LoC `reqwest` wrapper |
| Auth — SSF/CAEP/RISC | **BUILD** | ~300 LoC from spec |
| SCIM | **BUY scaffolding, BUILD PATCH + mapping** | `scim-server` + `scim_v2` |
| Queue + cron | **BUY** | `apalis` 0.7 (replaces hand-rolled `sera-queue`) |
| HTTP/WS | **BUY** | `axum` 0.8 |
| gRPC | **BUY** | `tonic` 0.13 + `prost` 0.13 |
| Database | **BUY** | `sqlx` 0.8 |
| Observability | **BUY (locked triad)** | `tracing` + `opentelemetry` 0.27 + `tracing-opentelemetry` 0.28 |
| Config (K8s manifests) | **BUY** | `figment` |
| Schema validation | **BUY** | `schemars` + `jsonschema` |
| Secrets | **BUY per-provider** | `vaultrs`, `aws-sdk-secretsmanager`, `azure_security_keyvault_secrets` |
| Harness turn loop | **MIRROR** | `codex-rs/core` + `claw-code` |
| Gateway↔Harness transport | **MIRROR** | `codex-rs/app-server` `AppServerTransport` enum |
| Hook points | **MIRROR** | Codex 5-hook-point model + `claw-code` `updated_input` semantics |
| Coordination primitives | **MIRROR** | Paperclip atomic checkout + 4-trigger taxonomy + `PluginEvent` envelope |
| Workflow DAG substrate | **MIRROR + DEPEND ON beads CLI** | `gastownhall/beads` data model + CLI |
| Result aggregation (Consensus / Supervised) | **BUILD** | No prior art — SERA must define `ResultAggregator` trait |

---

## 13. Risk Register

| Risk | Severity | Mitigation |
|---|---|---|
| `wasmtime` monthly major bumps churn SERA Cargo.lock | Medium | Loose range `">=43, <50"`, quarterly audit, CI matrix build against latest |
| OpenTelemetry triad version drift produces trait bound errors | High | Lock all three at exact versions; add a workspace-level doc comment pointing to this spec |
| `jco componentize` (TS WASM guest) is experimental | Medium | Treat TS hook support as post-MVS; Rust + Python guests are production paths |
| `rmcp` 0.x → 1.x breaking changes (history of churn) | Medium | Pin `^1.3`; re-audit on any 2.0 announcement |
| `microsandbox` pre-1.0 churn if adopted | Medium | WATCH-only classification; do not depend until 1.0 |
| `a2a-rs` single maintainer, alpha | High (if depended on) | VENDOR from upstream proto instead of depending on the crate |
| `ag-ui-client` community crate, unverified maturity | High (if depended on) | Hand-roll the enum set; never add as a dependency |
| `extism` pinned to old wasmtime, non-WIT ABI | N/A — rejected | Explicitly IGNORED; documented so the decision isn't re-litigated |
| SSF/CAEP/RISC: zero Rust prior art | Medium | Defer to post-MVS; SET verification leverages `jsonwebtoken` |
| Gastown org (beads, gastown, gascity, wasteland) could change direction | Medium | Pin beads CLI at a specific release tag; track `gastownhall` org health quarterly |

---

## 14. Maintenance Protocol

1. This spec is reviewed **quarterly** or when any spec it feeds (`SPEC-crate-decomposition`, `SPEC-interop`, `SPEC-hooks`, `SPEC-runtime`, `SPEC-gateway`, `SPEC-identity-authz`, `SPEC-workflow-engine`, `SPEC-circles`) needs an ecosystem update.
2. Every version bump of a **DEPEND ON** entry must be accompanied by a changelog line in this file with the PR link.
3. Every new entry must cite a verified source URL. Fabricated crates or unverified claims are rejected at review.
4. When a **MIRROR** entry's upstream file path changes, update the evidence column — stale citations defeat the spec's purpose.
5. When an upstream project SERA mirrors changes direction (e.g. codex pivots away from its current architecture), this spec issues a **delta PR** that either (a) re-pins to a specific commit and stops tracking, or (b) adopts the new direction and updates every downstream spec.

---

## 15. Open Questions

1. **Codex rollout format** — should SERA's gateway adopt the `StateDbHandle` shape, or build its own on `sqlx` and reference the rollout pattern only? (Preference: build on `sqlx`; reference the flush-checkpoint discipline.)
2. **beads-as-tool vs beads-as-engine** — integrate the beads CLI as an agent tool (current plan), adopt beads' data model inside `sera-workflow` (new recommendation), or do both? Both is the tentative answer; needs validation against multi-writer scenarios.
3. **Paperclip's `ResultAggregator` gap** — what interface does SERA define? Candidate shape: `async fn aggregate(&self, results: Vec<TaskResult>) -> Result<AggregatedResult, AggregationError>` with pluggable implementations (`FirstNonError`, `Majority`, `WeightedConsensus`, `LeadReviewer`, `Custom`).
4. **`genai` long-term viability** — single-maintainer project; what's the fallback if it stalls? Likely: fork-friendly because of its provider-per-module structure; or fall back to `async-openai` + bespoke Anthropic/Gemini clients.
5. **Hook point naming alignment** — should SERA adopt Codex's hook-point names verbatim (`SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `Stop`) to ease future interop? Current SPEC-hooks has 16 hook points — a superset. Alignment question only applies to the shared five.

---

## 16. Cross-References

| Source spec | Sections affected by this document |
|---|---|
| [SPEC-crate-decomposition](SPEC-crate-decomposition.md) | §3 (drop `sera-acp`), §3 (collapse `sera-queue` into an `apalis` adapter), §3 (hard boundary between `sera-plugin-sdk` and core per openclaw AGENTS.md) |
| [SPEC-interop](SPEC-interop.md) | §3 (MCP via `rmcp`), §4 (A2A vendored from proto), §5 (**DROP ACP**), §6 (AG-UI hand-rolled); evaluate Zed-ACP (distinct from IBM/BeeAI ACP) as an optional harness surface per hermes-agent |
| [SPEC-hooks](SPEC-hooks.md) | §2.2 (add `updated_input` to `HookResult`), §3 (Codex 5-hook alignment, openclaw full 29-hook reference set), §3 (add `subagent_delivery_target`), §5 (`wasmtime-wasi-http` allow-list), §5 (add `audit`/`enforce` modes), §6 (PluginEvent envelope, two-tier internal vs plugin hook bus) |
| [SPEC-runtime](SPEC-runtime.md) | §2 (SQ/EQ envelope + `supports()` on AgentRuntime), §3 (per-turn policy on `Op::UserTurn`, `TurnOutcome { Compact, Stop, Continue }` signal, doom-loop threshold), §4 (`ContentBlock`, **ContextEngine as distinct axis** per openclaw), §5 (two-mode compaction injection, checkpoint `reason` discriminant + dual pre/post refs, 25-entry rolling cap), §5.1 (parser-registry pattern, two-tier normalization, reasoning extraction), §5.2 (`credential_env` / `dynamic_endpoint` / `timeout_secs` on model routing), §5.3 (`provider_params: JsonValue` passthrough), §6 (`DynamicToolCallRequest { call_id, turn_id }`) |
| [SPEC-gateway](SPEC-gateway.md) | §3 (Submission/Event envelope), §4 (path policy `assertExternalDirectoryEffect` enforced gateway-side), §5 (harness selection by `supports()` priority), §6 (6-state session lifecycle, `PromptMisdelivery` replay, two-layer persistence: sqlx + shadow git), §6.3 (session scoping: `parent_session_key` + `spawned_by` first-class), **new section**: Gateway↔Harness Transport (`AppServerTransport`) |
| [SPEC-identity-authz](SPEC-identity-authz.md) | §2 (JWT / OIDC / OAuth2 crate choices), §3 (`casbin` RBAC), §4 (AuthZen write-yourself), §5 (SSF/CAEP/RISC write-yourself) |
| [SPEC-hitl-approval](SPEC-hitl-approval.md) | §3 (`revision_requested` state, **`CorrectedError { feedback }` tool-result variant**), §4 (five-level `AskForApproval`, Guardian pre-gate, doom-loop escalation category, `ExecApprovals` per-agent `argPattern`), §5 (`GranularApprovalConfig`) |
| [SPEC-tools](SPEC-tools.md) | §3 (schema-driven registration via `DynamicToolSpec`; `Tool.Context::ask()` inline approval callback), §4.1 (`defer_loading`), §5 (minimal base policy + opt-in preset system; no common dev tools in base), §6 (`FileTime.withLock` conflict detection, `SsrfValidator` trait), §6a (three-layer sandbox model + **per-binary process-scoped egress**, method+path REST rules, enforce/audit modes, TLS terminate/passthrough, Landlock rule-union gotcha, pinned-image dual-field lockstep) |
| [SPEC-deployment](SPEC-deployment.md) | §3 (sandbox tier declaration, Landlock `strict`/`best_effort` mode, pinned image digest with blueprint-level `digest:` field mirroring, blueprint runner/snapshot/state lifecycle, platform matrix), enterprise egress-proxy TLS inspection |
| [SPEC-config](SPEC-config.md) | Ship a published JSON Schema per K8s manifest kind (NemoClaw ships 5 canonical schemas) |
| [SPEC-security](SPEC-security.md) | Add "multi-tenant SaaS exfiltration channels" as a documented threat class (Sentry case); default to GET-only for any endpoint whose tenant boundary is not per-user path-isolated |
| [SPEC-workflow-engine](SPEC-workflow-engine.md) | §2 (`WorkflowTask` modeled on beads `Issue`), §3 (`DependencyType` incl. `conditional_blocks`, 4-trigger taxonomy), §4 (atomic `claim` protocol, `bd ready` algorithm), §5 (three-layer failure model), §6.1 (promote beads from Phase-3) |
| [SPEC-circles](SPEC-circles.md) | §3 (`ConcurrencyPolicy` sub-field), §4 (atomic checkout, **new `ResultAggregator` trait** — integration point = openclaw's `subagent_delivery_target` hook), §5 (`PluginEvent` envelope + `correlationId`), §7 (cross-circle federation via wasteland pattern) |
| [SPEC-observability](SPEC-observability.md) | §2 (W3C trace on every submission), §3 (`LaneFailureClass` taxonomy) |
| [SPEC-versioning](SPEC-versioning.md) | §4 (serde alias-based JSON protocol versioning pattern) |

---

## 17. Sources

### Crate & protocol research (8 agents, Apr 2026)

- MCP: https://crates.io/crates/rmcp · https://github.com/modelcontextprotocol/rust-sdk
- A2A / ACP: https://github.com/a2aproject/A2A · https://a2a-protocol.org/latest/specification/ · https://lfaidata.foundation/communityblog/2025/08/29/acp-joins-forces-with-a2a-under-the-linux-foundations-lf-ai-data/ · https://crates.io/crates/a2a-rs
- AG-UI: https://docs.ag-ui.com/introduction · https://github.com/ag-ui-protocol/ag-ui · https://crates.io/crates/ag-ui-client
- wasmtime: https://docs.wasmtime.dev/ · https://docs.wasmtime.dev/api/wasmtime_wasi_http/index.html · https://github.com/bytecodealliance/wasmtime/issues/4109
- Auth: https://crates.io/crates/jsonwebtoken · https://docs.rs/openidconnect · https://crates.io/crates/oauth2 · https://crates.io/crates/casbin · https://openid.github.io/authzen/
- Infra: https://crates.io/crates/axum · https://crates.io/crates/tonic · https://crates.io/crates/sqlx · https://crates.io/crates/apalis · https://github.com/open-telemetry/opentelemetry-rust · https://crates.io/crates/figment
- LLM/tools: https://crates.io/crates/genai · https://crates.io/crates/async-openai · https://crates.io/crates/llguidance · https://crates.io/crates/fastembed · https://github.com/anysphere/tiktoken-rs · https://github.com/fussybeaver/bollard

### Harness & coordination reference research (8 agents across two rounds, Apr 2026)

**Round 1 (initial four):**

- `ultraworkers/claw-code`: https://github.com/ultraworkers/claw-code — MIT; key modules: `session.rs`, `worker_boot.rs`, `hooks.rs`, `lane_events.rs`, `compact.rs`, `task_packet.rs`, `mcp_lifecycle_hardened.rs`, `permission_enforcer.rs`
- `openai/codex`: https://github.com/openai/codex — Apache-2.0; key files: `codex-rs/protocol/src/protocol.rs`, `codex-rs/core/src/compact.rs`, `codex-rs/sandboxing/src/lib.rs`, `codex-rs/hooks/src/lib.rs`, `codex-rs/app-server/src/transport/mod.rs`, `codex-rs/protocol/src/dynamic_tools.rs`
- `paperclipai/paperclip`: https://github.com/paperclipai/paperclip — MIT; key files: `server/src/services/heartbeat.ts`, `issues.ts`, `agents.ts`, `approvals.ts`, `routines.ts`, `plugin-event-bus.ts`, `docs/agents-runtime.md`
- `gastownhall/beads`: https://github.com/gastownhall/beads — MIT; key files: `internal/types/types.go`, `beads.go` · ecosystem: https://github.com/gastownhall/gastown · https://github.com/gastownhall/gascity · https://github.com/gastownhall/wasteland · https://github.com/gastownhall/gascity-otel

**Round 2 (deeper dive):**

- `openclaw/openclaw`: https://github.com/openclaw/openclaw — MIT; key files: `src/agents/harness/types.ts`, `src/agents/harness/registry.ts`, `src/context-engine/types.ts`, `src/context-engine/registry.ts`, `src/gateway/protocol/schema/sessions.ts`, `src/gateway/session-compaction-checkpoints.ts`, `src/plugins/hook-types.ts`, `src/hooks/internal-hook-types.ts`, `src/gateway/protocol/schema/exec-approvals.ts`, `AGENTS.md`
- `NousResearch/hermes-agent`: https://github.com/NousResearch/hermes-agent — MIT; key files: `environments/agent_loop.py`, `environments/tool_call_parsers/__init__.py`, `environments/tool_call_parsers/hermes_parser.py`, `acp_adapter/server.py`, `acp_adapter/permissions.py`, `memory_manager.py`
- `anomalyco/opencode`: https://github.com/anomalyco/opencode — MIT (verified `fork: false`, `parent: null`); key files: `packages/opencode/src/session/processor.ts`, `session/llm.ts`, `session/session.sql.ts`, `tool/tool.ts`, `tool/task.ts`, `tool/edit.ts`, `tool/apply_patch.ts`, `tool/bash.ts`, `tool/external-directory.ts`, `permission/index.ts`, `permission/evaluate.ts`, `snapshot/index.ts`, `mcp/index.ts`, `agent/agent.ts`, `file/time.ts`
- `NVIDIA/NemoClaw`: https://github.com/NVIDIA/NemoClaw — Apache-2.0; verified directly via `gh api repos/NVIDIA/NemoClaw/contents/*`. Key files: `schemas/sandbox-policy.schema.json` (full JSON Schema quoted above), `nemoclaw-blueprint/blueprint.yaml` (top-level digest + components.sandbox.image pinning + 4 inference profiles), `nemoclaw-blueprint/policies/openclaw-sandbox.yaml` (full deny-by-default filesystem policy + per-binary egress rules + Sentry exfil comment), `nemoclaw-blueprint/policies/presets/{brave,brew,discord,github,huggingface,jira,npm,outlook,pypi,slack,telegram}.yaml`, `nemoclaw/src/blueprint/{runner,snapshot,state,ssrf}.ts`, `README.md`. Related: [NVIDIA/OpenShell](https://github.com/NVIDIA/OpenShell) (sandbox runtime, part of NVIDIA Agent Toolkit).

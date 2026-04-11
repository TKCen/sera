# SERA Architecture

## Workspace Structure

SERA is a 11-crate workspace with Rust edition 2024. Crates are organized in dependency layers:

```
rust/crates/
├── sera-domain/          # Tier 0: types only (Principal, Event, Session, ConfigManifest, Tool, Memory)
├── sera-config/          # Tier 1: manifest loading + secret resolution (K8s YAML + env)
├── sera-db/              # Tier 2: SQLite + PostgreSQL abstraction (rusqlite for MVS, sqlx for enterprise)
├── sera-auth/            # Tier 2: JWT + OIDC + session tokens
├── sera-events/          # Tier 2: audit trail + event dispatch
├── sera-runtime/         # Tier 3: LLM client + tools + context assembly + reasoning loop (lib + bin)
├── sera-core/            # Tier 4: HTTP server + Discord connector + `sera` MVS binary
├── sera-docker/          # Tier 2: container lifecycle for BYOH agents
├── sera-testing/         # Test utilities
├── sera-tui/             # Terminal UI (post-MVS)
└── sera-byoh-agent/      # BYOH agent reference implementation
```

### MVS vs Post-MVS

**MVS (Minimum Viable System)** delivers:
- `sera-domain` through `sera-core` (all layers)
- Single `sera` binary at `rust/crates/sera-core/src/bin/sera.rs`
- SQLite only (no PostgreSQL)
- Discord connector only (no Slack, email, etc.)
- Inline tool stub definitions (full tool schemas in post-MVS)

**Post-MVS** includes:
- Full `sera-runtime` as separate daemon
- PostgreSQL integration via `sera-db/sqlx`
- Multi-connector framework
- TUI (`sera-tui`)
- Distributed agent deployment

See `mvs-review-plan §6.6` for detailed scope boundaries.

## Crate Responsibilities

### sera-domain
**Responsibility**: Type definitions matching BYOH contract and full domain model.

**Key Types**:
- `Principal` — user/service identity
- `Event` — audit events (create session, execute tool, etc.)
- `Session` — conversation state (id, agent_id, created_at, state)
- `ConfigManifest` — K8s-style document (apiVersion, kind, metadata, spec)
- `Tool` — function definition (name, description, parameters)
- `Memory` — searchable knowledge (content, vectors, timestamps)
- `LifecycleMode` — Persistent vs Ephemeral agent mode
- `TaskInput` / `TaskOutput` — container protocol messages

**Module layout**:
```rust
pub mod agent;              // AgentSpec
pub mod audit;              // AuditEvent
pub mod capability;         // Capability definitions
pub mod chat;               // ChatMessage, ToolCall
pub mod config_manifest;    // ConfigManifest trait + 4 kinds
pub mod event;              // Event enum
pub mod intercom;           // gRPC proto stubs
pub mod manifest;           // Manifest + ManifestKind enums
pub mod memory;             // Memory + MemoryIndex
pub mod metering;           // Token/cost tracking
pub mod policy;             // PII policy, capability policies
pub mod principal;          // Principal types
pub mod sandbox;            // Tier definitions
pub mod secrets;            // SecretRef
pub mod session;            // Session state
pub mod skill;              // Skill definition
pub mod tool;               // ToolDefinition, FunctionDefinition
```

### sera-config
**Responsibility**: Load manifest files and environment variables; resolve secret references.

**Key Components**:
- `manifest_loader` — Parse single/multi-document YAML; return `ManifestSet`
- `providers.json` — Provider registry (base_url, model defaults)
- `SeraConfig` struct — BYOH contract env vars (SERA_CORE_URL, SERA_IDENTITY_TOKEN, etc.)
- Secret resolution — Convert `{ secret: "path" }` to `SERA_SECRET_PATH` env var

**Single-file mode**: --- YAML document separators split manifests into individual documents.

**Config layers** (POST-MVS full layering):
1. Defaults (hardcoded in spec types)
2. File (sera.yaml)
3. Environment (SERA_* and SERA_SECRET_* vars)
4. Runtime (HTTP API patches, not in MVS)

### sera-db
**Responsibility**: SQLite abstraction for MVS; PostgreSQL (sqlx) for enterprise.

**Key Modules**:
- `sqlite` — Sync interface (rusqlite) for embedded use
- `pool` — Connection pooling (sqlx for enterprise)
- Domain-specific query modules: `sessions`, `audit`, `tasks`, `memory`, `job_queue`, `webhooks`

**MVS uses SQLite only**; full DB layer supports both via feature flags.

**Design rule**: SQL types and row structs live here; domain objects live in `sera-domain`. No leaking `sqlx::Row`.

### sera-runtime
**Responsibility**: Reusable agent runtime — LLM client, tools, context assembly, reasoning loop.

**Key modules**:
- `llm_client` — OpenAI-compatible API client with retry/timeout handling
- `tools` — Tool registry (file_ops, shell_exec, http_request, knowledge, etc.) + `mvs_tools` for path-safe execution
- `context_assembler` — Assemble system prompt, tools, memory, history, current prompt into LLM message list
- `session_manager` — Get/create sessions; append/load transcripts
- `reasoning_loop` — State machine (Init → Think → Act → Observe → Done) with error recovery

**Both lib and bin**:
- `lib.rs` — Used by `sera-core` to import reasoning loop logic
- `main.rs` — Standalone container agent for POST-MVS

**Context assembly order** (KV-cache optimized):
1. System prompt (persona)
2. Tool definitions
3. Memory context (optional)
4. History (full transcript)
5. Current prompt

**Max 10 iterations per turn** (configurable).

### sera-core
**Responsibility**: HTTP server, Discord connector, MVS `sera` binary integration.

**Key Components**:
- `src/lib.rs` — Axum routes, middleware, health/API handlers
- `src/bin/sera.rs` — **MVS binary**: CLI (start, init, agent create/list), HTTP + Discord event loop
- `discord.rs` — WebSocket gateway (wss://gateway.discord.gg/?v=10)
- Manifest loading + provider/agent resolution

**MVS Entry Points**:
- `sera start [-c sera.yaml] [-p 3001]` — Start HTTP + Discord
- `sera init` — Create template sera.yaml
- `sera agent list` — Show configured agents
- `sera agent create <name>` — Add agent to manifest

**HTTP API** (MVS):
- `GET /health` — Status
- `POST /api/chat` — Turn execution (message in, reply + session_id + usage out)

## Dependency Graph

```
Domain
  ↑
Config
  ↑
  ├─→ DB (sqlite for MVS, sqlx for enterprise)
  ├─→ Auth (JWT, OIDC)
  ├─→ Events (audit log)
  ↑
Runtime (llm_client + tools + context_assembler + reasoning_loop)
  ↑
Core (HTTP + Discord + MVP binary)
```

## Key Design Patterns

### 1. Manifests as Code
K8s-style YAML with typed Rust structs. Single file supports multiple documents via `---` separators. Four kinds in MVS: Instance, Provider, Agent, Connector.

### 2. Secret References
`{ secret: "connectors/discord-main/token" }` → `SERA_SECRET_CONNECTORS_DISCORD_MAIN_TOKEN` env var. Slashes and hyphens become underscores.

### 3. Tool Matching
Glob patterns (e.g., `memory_*`, `file_read`) in AgentSpec.tools.allow. At runtime, validate tool name against patterns before execution.

### 4. Session Persistence
Every message (user, assistant, tool) appended to SQLite transcript. Session key format: `agent:{agent_id}:main`. Archives created on reset.

### 5. Error Recovery
Three recovery layers:
- Timeout retries (2)
- Context overflow retries (3) with aggressive compaction
- Provider unavailable (1 retry)

### 6. Streaming (Post-MVS)
Centrifugo for real-time message dispatch. MVS uses simple HTTP JSON responses.

---

Last updated: 2026-04-09

# SERA Rust Migration — Detailed Execution Plan

This document is the actionable companion to `docs/RUST-MIGRATION-PLAN.md`. It maps every TypeScript module to its Rust target and defines phase-by-phase acceptance criteria.

## Workspace Layout

```
rust/
  Cargo.toml                  # Workspace root
  crates/
    sera-domain/              # Shared domain types, enums, IDs
    sera-config/              # Environment/file config loading
    sera-db/                  # PostgreSQL via sqlx, migrations, row types
    sera-auth/                # API keys, JWT, OIDC middleware
    sera-events/              # Audit trail, lifecycle events, Centrifugo payloads
    sera-docker/              # Container lifecycle via bollard
    sera-core/                # Main API server binary (axum)
    sera-runtime/             # Agent worker binary (runs in containers)
    sera-tui/                 # Terminal UI binary (ratatui)
    sera-testing/             # Test utilities, fixtures, golden tests
    sera-byoh-agent/          # BYOH agent reference implementation (existing)
```

## Crate Dependency Graph

```
sera-domain (leaf — no internal deps)
    ↑
sera-config (reads sera-domain types)
    ↑
sera-db (sera-domain for row↔domain mapping)
    ↑
sera-auth (sera-domain, sera-db for operator/key lookups)
    ↑
sera-events (sera-domain for event payloads)
    ↑
sera-docker (sera-domain, sera-events for lifecycle events)
    ↑
sera-core (all crates above + axum/tower/reqwest)
    ↑
sera-runtime (sera-domain, sera-config, reqwest — runs standalone)
    ↑
sera-tui (sera-domain, reqwest — API client only)

sera-testing (sera-domain, sera-db — dev-dependency of sera-core)
sera-byoh-agent (sera-domain, sera-config — standalone binary)
```

---

## Rosetta Stone: TypeScript → Rust Module Mapping

### sera-domain (from core/src/agents/types.ts, manifest/types.ts, schemas.ts)

| TypeScript Source                                   | Rust Target                                     | Notes                      |
| --------------------------------------------------- | ----------------------------------------------- | -------------------------- |
| `agents/types.ts` → `ChatMessage`                   | `sera-domain::chat::ChatMessage`                | enum role, Option content  |
| `agents/types.ts` → `CapturedThought`               | `sera-domain::chat::CapturedThought`            |                            |
| `agents/types.ts` → `AgentResponse`                 | `sera-domain::chat::AgentResponse`              | enum with variants         |
| `agents/types.ts` → `AgentInstance`                 | `sera-domain::agent::AgentInstance`             | status as enum, not string |
| `agents/manifest/types.ts` → `AgentManifest`        | `sera-domain::manifest::AgentManifest`          | serde_yaml, nested structs |
| `agents/manifest/types.ts` → `ResolvedCapabilities` | `sera-domain::capability::ResolvedCapabilities` | typed fields, not Record   |
| `agents/schemas.ts` → `AgentTemplateSchema`         | `sera-domain::manifest::AgentTemplate`          | serde + validation fns     |
| `agents/schemas.ts` → `NamedListSchema`             | `sera-domain::policy::NamedList`                | enum for list type         |
| `agents/schemas.ts` → `SandboxBoundarySchema`       | `sera-domain::policy::SandboxBoundary`          |                            |
| `metering/MeteringService.ts` → `UsageRecord`       | `sera-domain::metering::UsageRecord`            |                            |
| `metering/MeteringService.ts` → `BudgetStatus`      | `sera-domain::metering::BudgetStatus`           |                            |
| `audit/AuditService.ts` → `AuditEntry`              | `sera-domain::audit::AuditEntry`                | actor_type as enum         |
| `audit/AuditService.ts` → `AuditRecord`             | `sera-domain::audit::AuditRecord`               |                            |
| `sandbox/SandboxManager.ts` → `SandboxInfo`         | `sera-domain::sandbox::SandboxInfo`             |                            |
| `intercom/types.ts` → `IntercomMessage`             | `sera-domain::intercom::IntercomMessage`        |                            |
| `skills/types.ts` → `SkillDefinition`               | `sera-domain::skill::SkillDefinition`           |                            |
| `sessions/types.ts` → `Session`                     | `sera-domain::session::Session`                 |                            |
| `lib/llm/types.ts` → `ToolCall`                     | `sera-domain::chat::ToolCall`                   |                            |
| `secrets/interfaces.ts` → `SecretsProvider`         | `sera-domain::secrets::SecretEntry`             | trait in sera-db           |

### sera-config (from core/src/lib/config.ts)

| TypeScript Source                   | Rust Target                  | Notes                    |
| ----------------------------------- | ---------------------------- | ------------------------ |
| `lib/config.ts` → `config`          | `sera-config::SeraConfig`    | from_env() + from_file() |
| `lib/config.ts` → `config.llm`      | `sera-config::LlmConfig`     |                          |
| `lib/config.ts` → `config.channels` | `sera-config::ChannelConfig` |                          |

### sera-db (from core/src/lib/database.ts + service queries)

| TypeScript Source                         | Rust Target                                      | Notes                |
| ----------------------------------------- | ------------------------------------------------ | -------------------- |
| `lib/database.ts` → `pool`                | `sera-db::pool::DbPool`                          | sqlx::PgPool wrapper |
| `lib/database.ts` → `initDb()`            | `sera-db::migrate::run_migrations()`             | sqlx migrate         |
| `agents/registry.service.ts` queries      | `sera-db::agents::AgentRepository`               |                      |
| `metering/MeteringService.ts` queries     | `sera-db::metering::MeteringRepository`          |                      |
| `audit/AuditService.ts` queries           | `sera-db::audit::AuditRepository`                |                      |
| `secrets/postgres-secrets-provider.ts`    | `sera-db::secrets::SecretsRepository`            |                      |
| `memory/CoreMemoryService.ts` queries     | `sera-db::memory::MemoryRepository`              |                      |
| `services/ScheduleService.ts` queries     | `sera-db::schedules::ScheduleRepository`         |                      |
| `channels/NotificationService.ts` queries | `sera-db::notifications::NotificationRepository` |                      |
| `skills/SkillLibrary.ts` queries          | `sera-db::skills::SkillRepository`               |                      |

### sera-auth (from core/src/auth/)

| TypeScript Source          | Rust Target                            | Notes               |
| -------------------------- | -------------------------------------- | ------------------- |
| `auth/authMiddleware.ts`   | `sera-auth::middleware::auth_layer()`  | axum middleware     |
| `auth/api-key-provider.ts` | `sera-auth::api_key::ApiKeyValidator`  |                     |
| `auth/oidc-provider.ts`    | `sera-auth::oidc::OidcValidator`       | openidconnect crate |
| `auth/auth-service.ts`     | `sera-auth::AuthService`               | pluggable trait     |
| `auth/IdentityService.ts`  | `sera-auth::identity::IdentityService` |                     |

### sera-events (from core/src/audit/, intercom/, channels/)

| TypeScript Source                 | Rust Target                                       | Notes                  |
| --------------------------------- | ------------------------------------------------- | ---------------------- |
| `audit/AuditService.ts`           | `sera-events::audit::AuditService`                | SHA-256 hash chain     |
| `intercom/IntercomService.ts`     | `sera-events::intercom::IntercomService`          | Centrifugo HTTP client |
| `intercom/ChannelNamespace.ts`    | `sera-events::intercom::ChannelNamespace`         |                        |
| `channels/NotificationService.ts` | `sera-events::notifications::NotificationService` |                        |
| `channels/adapters/*`             | `sera-events::notifications::adapters::*`         | trait-based            |

### sera-docker (from core/src/sandbox/)

| TypeScript Source                     | Rust Target                                          | Notes   |
| ------------------------------------- | ---------------------------------------------------- | ------- |
| `sandbox/SandboxManager.ts`           | `sera-docker::sandbox::SandboxManager`               | bollard |
| `sandbox/TierPolicy.ts`               | `sera-docker::policy::TierPolicy`                    |         |
| `sandbox/EgressAclManager.ts`         | `sera-docker::egress::EgressAclManager`              |         |
| `sandbox/ContainerSecurityMapper.ts`  | `sera-docker::security::ContainerSecurityMapper`     |         |
| `sandbox/BindMountBuilder.ts`         | `sera-docker::mounts::BindMountBuilder`              |         |
| `sandbox/WorktreeManager.ts`          | `sera-docker::workspace::WorktreeManager`            |         |
| `sandbox/PermissionRequestService.ts` | `sera-docker::permissions::PermissionRequestService` |         |

### sera-core (from core/src/routes/, agents/, llm/, skills/, mcp/, services/)

| TypeScript Source                 | Rust Target                                 | Notes                            |
| --------------------------------- | ------------------------------------------- | -------------------------------- |
| `index.ts` startup                | `sera-core::main()`                         | tokio::main, axum Server         |
| `routes/agents.ts`                | `sera-core::api::agents`                    | Agent CRUD, templates, instances |
| `routes/auth.ts`                  | `sera-core::api::auth`                      | Login, session, token refresh    |
| `routes/audit.ts`                 | `sera-core::api::audit`                     | Audit trail query, export        |
| `routes/budget.ts`                | `sera-core::api::budget`                    | Token quota queries/updates      |
| `routes/channels.ts`              | `sera-core::api::channels`                  | Notification channel CRUD        |
| `routes/chat.ts`                  | `sera-core::api::chat`                      | Stream agent reasoning           |
| `routes/circles.ts`               | `sera-core::api::circles`                   | YAML-based circle listing        |
| `routes/circles-db.ts`            | `sera-core::api::circles_db`                | Circle database CRUD             |
| `routes/config.ts`                | `sera-core::api::config`                    | Configuration endpoints          |
| `routes/delegation.ts`            | `sera-core::api::delegation`                | Cross-agent authorization        |
| `routes/embedding.ts`             | `sera-core::api::embedding`                 | Text embeddings                  |
| `routes/federation.ts`            | `sera-core::api::federation`                | A2A federation                   |
| `routes/heartbeat.ts`             | `sera-core::api::heartbeat`                 | Agent liveness                   |
| `routes/intercom.ts`              | `sera-core::api::intercom`                  | Centrifugo pub/sub               |
| `routes/knowledge.ts`             | `sera-core::api::knowledge`                 | Qdrant query/store               |
| `routes/lifecycle.ts`             | `sera-core::api::lifecycle`                 | Agent lifecycle + permissions    |
| `routes/llmProxy.ts`              | `sera-core::api::llm_proxy`                 | LLM proxy with metering          |
| `routes/lsp.ts`                   | `sera-core::api::lsp`                       | Language server endpoints        |
| `routes/mcp.ts`                   | `sera-core::api::mcp`                       | MCP server management            |
| `routes/memory.ts`                | `sera-core::api::memory`                    | Agent core memory blocks         |
| `routes/metering.ts`              | `sera-core::api::metering`                  | Usage stats                      |
| `routes/notifications.ts`         | `sera-core::api::notifications`             | Notification dispatch            |
| `routes/openai-compat.ts`         | `sera-core::api::openai_compat`             | OpenAI compatibility layer       |
| `routes/operator-requests.ts`     | `sera-core::api::operator_requests`         | Permission requests              |
| `routes/pipelines.ts`             | `sera-core::api::pipelines`                 | Multi-step workflows             |
| `routes/providers.ts`             | `sera-core::api::providers`                 | Provider management              |
| `routes/registry.ts`              | `sera-core::api::registry`                  | Template import/export           |
| `routes/sandbox.ts`               | `sera-core::api::sandbox`                   | Container management             |
| `routes/schedules.ts`             | `sera-core::api::schedules`                 | Task scheduling                  |
| `routes/secrets.ts`               | `sera-core::api::secrets`                   | Encrypted vault                  |
| `routes/sessions.ts`              | `sera-core::api::sessions`                  | Session management               |
| `routes/skills.ts`                | `sera-core::api::skills`                    | Skill registry                   |
| `routes/tasks.ts`                 | `sera-core::api::tasks`                     | Task management                  |
| `routes/toolProxy.ts`             | `sera-core::api::tool_proxy`                | Skill invocation                 |
| `routes/webhooks.ts`              | `sera-core::api::webhooks`                  | Webhook management               |
| `db/migrations/*`                 | `sera-db::migrate`                          | sqlx migrations                  |
| `identity/CredentialResolver.ts`  | `sera-auth::credentials`                    | Agent credential resolution      |
| `identity/acting-context.ts`      | `sera-auth::acting_context`                 | Acting context type              |
| `lsp/LspManager.ts`               | `sera-core::lsp::LspManager`                | LSP process management           |
| `storage/StorageProvider.ts`      | `sera-docker::storage::StorageProvider`     | Workspace storage trait          |
| `storage/LocalStorageProvider.ts` | `sera-docker::storage::LocalStorage`        | Filesystem storage               |
| `storage/DockerVolumeProvider.ts` | `sera-docker::storage::DockerVolume`        | Docker volume storage            |
| `agents/Orchestrator.ts`          | `sera-core::orchestrator::Orchestrator`     |                                  |
| `agents/AgentFactory.ts`          | `sera-core::orchestrator::AgentFactory`     |                                  |
| `agents/BaseAgent.ts`             | `sera-core::agent::BaseAgent`               | async trait                      |
| `agents/WorkerAgent.ts`           | `sera-core::agent::WorkerAgent`             |                                  |
| `agents/process/*.ts`             | `sera-core::process::*`                     |                                  |
| `llm/LlmRouter.ts`                | `sera-core::llm::LlmRouter`                 | reqwest + streaming              |
| `llm/ProviderRegistry.ts`         | `sera-core::llm::ProviderRegistry`          |                                  |
| `llm/CircuitBreakerService.ts`    | `sera-core::llm::CircuitBreaker`            |                                  |
| `llm/ContextAssembler.ts`         | `sera-core::llm::ContextAssembler`          |                                  |
| `skills/SkillRegistry.ts`         | `sera-core::skills::SkillRegistry`          |                                  |
| `skills/SkillLibrary.ts`          | `sera-core::skills::SkillLibrary`           |                                  |
| `skills/builtin/*.ts` (15)        | `sera-core::skills::builtin::*`             |                                  |
| `mcp/registry.ts`                 | `sera-core::mcp::McpRegistry`               |                                  |
| `mcp/SeraMCPServer.ts`            | `sera-core::mcp::SeraMcpServer`             |                                  |
| `memory/*.ts`                     | `sera-core::memory::*`                      |                                  |
| `services/EmbeddingService.ts`    | `sera-core::embeddings::EmbeddingService`   |                                  |
| `services/VectorService.ts`       | `sera-core::vector::VectorService`          |                                  |
| `services/ScheduleService.ts`     | `sera-core::schedules::ScheduleService`     |                                  |
| `circles/*.ts`                    | `sera-core::circles::*`                     |                                  |
| `capability/resolver.ts`          | `sera-core::capability::CapabilityResolver` |                                  |
| `lib/PgBossService.ts`            | `sera-core::jobs::JobQueue`                 | custom SKIP LOCKED               |
| `middleware/*.ts`                 | `sera-core::middleware::*`                  | tower layers                     |

### sera-runtime (from core/agent-runtime/src/)

| TypeScript Source                 | Rust Target                             | Notes               |
| --------------------------------- | --------------------------------------- | ------------------- |
| `agent-runtime/src/loop.ts`       | `sera-runtime::reasoning_loop`          | async state machine |
| `agent-runtime/src/llm-client.ts` | `sera-runtime::llm_client`              | reqwest streaming   |
| `agent-runtime/src/tools/*.ts`    | `sera-runtime::tools::*`                | trait ToolExecutor  |
| `agent-runtime/src/context/*.ts`  | `sera-runtime::context::ContextManager` |                     |

### sera-tui (from tui/)

| Go Source  | Rust Target   | Notes               |
| ---------- | ------------- | ------------------- |
| `tui/*.go` | `sera-tui::*` | ratatui + crossterm |

---

## Phase Execution Plan

### Phase 0: Contract Stabilization (Week 1)

**Goal:** Freeze and inventory all public contracts before writing Rust code.

**Deliverables:**

1. **Route inventory** — Extract from `core/src/routes/*.ts`:
   - Every endpoint path, method, auth requirement
   - Request/response body shapes
   - Classify: `stable` / `changing` / `legacy`
   - Output: `rust/contracts/routes.json`

2. **Manifest compatibility matrix** — From `schemas/*.json`:
   - All YAML manifest kinds: AgentTemplate, Agent, NamedList, SandboxBoundary, CapabilityPolicy
   - Field-by-field schema with required/optional/default
   - Output: golden YAML files in `rust/contracts/manifests/`

3. **Queue/topic inventory** — From `lib/PgBossService.ts`, `intercom/`:
   - pg-boss queue names: `agent-schedule`, `notification.dispatch`, `memory.compaction`, per-schedule UUIDs
   - Centrifugo channel prefixes: `internal:*`, `agent:*`, `broadcast:*`
   - Output: `rust/contracts/queues.md`

4. **Database ownership map** — From migration files + service queries:
   - Every table → owning service
   - Read/write access by service
   - Output: `rust/contracts/db-ownership.md`

5. **Black-box integration tests** — Against live TS system:
   - Health endpoints, provider listing, template listing
   - Manifest parse round-trip tests
   - Audit chain verification
   - Output: `rust/crates/sera-testing/tests/`

**Acceptance criteria:**

- [ ] `rust/contracts/` directory exists with route, manifest, queue, and DB inventories
- [ ] At least 10 golden YAML manifest files captured
- [ ] At least 5 black-box HTTP tests against the TS API

---

### Phase 1: Shared Rust Foundations (Weeks 2-3)

**Goal:** sera-domain, sera-db, sera-auth, sera-config, sera-events can parse manifests, connect to PostgreSQL, and validate JWTs.

**sera-domain tasks:**

1. Port `AgentInstance` with status as Rust enum (7 variants)
2. Port `AgentManifest` with nested structs (spec, identity, model, resources, etc.)
3. Port `AgentTemplate` with Zod → serde validation
4. Port `ChatMessage` with role enum
5. Port `ResolvedCapabilities` with typed fields
6. Port `AuditEntry`/`AuditRecord` with hash chain types
7. Port `UsageRecord`/`BudgetStatus`
8. Port `SandboxInfo` with tier enum
9. Port `NamedList`, `SandboxBoundary`, `CapabilityPolicy`
10. Port `SkillDefinition`, `ToolCall`
11. Add `serde_yaml` tests: parse every golden YAML from Phase 0
12. Add JSON Schema conformance tests against `schemas/*.json`

**sera-config tasks:**

1. Extend existing `SeraConfig` for full core config (LLM, channels, database)
2. Add file-based config loading (`providers.json`)
3. Add config validation

**sera-db tasks:**

1. `DbPool` connection + health check (exists as stub)
2. `run_migrations()` using sqlx migrate (baseline from existing schema)
3. `AgentRepository` — CRUD for agent_templates, agent_instances
4. `MeteringRepository` — token_usage, usage_events, token_quotas
5. `AuditRepository` — audit_trail append with EXCLUSIVE lock + SHA-256
6. `SecretsRepository` — AES-256-GCM encrypted secrets
7. `MemoryRepository` — core_memory_blocks, scoped_memory_blocks
8. `ScheduleRepository` — schedules table
9. `SkillRepository` — skill_library table
10. `NotificationRepository` — notification_channels table

**sera-auth tasks:**

1. API key validation from operators table
2. JWT issuance (HS256) for internal tokens
3. JWT verification middleware (axum layer)
4. Acting context extraction from request

**sera-events tasks:**

1. Audit event types with serde serialization
2. Centrifugo publish client (reqwest to Centrifugo HTTP API)
3. Lifecycle event enum and publisher

**Acceptance criteria:**

- [ ] `cargo test` passes with >50 unit tests across domain/db/auth
- [ ] Rust parses all golden YAML manifests identically to TypeScript
- [ ] Rust connects to existing PostgreSQL and reads agent_templates
- [ ] Rust validates/rejects the same JWTs as TypeScript
- [ ] sqlx compile-time query checking works (DATABASE_URL set)

---

### Phase 2: sera-core-rs Shadow Mode (Weeks 4-6)

**Goal:** Rust API server runs beside TypeScript, serving read-only endpoints.

**API routes to implement first (read-only, safe):**

| Priority | Route                       | TS Source             | Rust Module      |
| -------- | --------------------------- | --------------------- | ---------------- |
| 1        | `GET /api/health`           | `index.ts`            | `api::health`    |
| 2        | `GET /api/health/detail`    | `index.ts`            | `api::health`    |
| 3        | `GET /api/providers/list`   | `routes/providers.ts` | `api::providers` |
| 4        | `GET /api/agents/templates` | `routes/agents.ts`    | `api::agents`    |
| 5        | `GET /api/agents`           | `routes/agents.ts`    | `api::agents`    |
| 6        | `GET /api/agents/:id`       | `routes/agents.ts`    | `api::agents`    |
| 7        | `GET /api/audit/log`        | `routes/audit.ts`     | `api::audit`     |
| 8        | `GET /api/metering/usage`   | `routes/metering.ts`  | `api::metering`  |
| 9        | `GET /api/budget`           | `routes/budget.ts`    | `api::budget`    |
| 10       | `GET /api/skills`           | `routes/skills.ts`    | `api::skills`    |
| 11       | `GET /api/schedules`        | `routes/schedules.ts` | `api::schedules` |
| 12       | `GET /api/circles`          | `routes/circles.ts`   | `api::circles`   |
| 13       | `GET /api/mcp-servers`      | `routes/mcp.ts`       | `api::mcp`       |
| 14       | `GET /api/sessions`         | `routes/sessions.ts`  | `api::sessions`  |

**Infrastructure:**

1. axum app with tower middleware stack (CORS, tracing, auth)
2. Shared `AppState` with DbPool, config, service handles
3. JSON error responses matching TS format
4. Request logging via tracing spans
5. Graceful shutdown with tokio signal handling

**Shadow mode deployment:**

- Run on port 3002 (TS stays on 3001)
- Docker Compose service: `sera-core-rs`
- Same `DATABASE_URL`, read-only access
- Compare response shapes via contract tests

**Acceptance criteria:**

- [ ] `sera-core-rs` binary starts and serves `/api/health` returning `{ status: "ok" }`
- [ ] All 14 read-only GET endpoints return byte-compatible JSON with TS
- [ ] Auth middleware rejects requests without valid API key
- [ ] Contract tests pass: same request → same response shape from both servers
- [ ] Docker image builds and runs in `docker-compose.dev.yaml`

---

### Phase 3: Agent Runtime with Dual-Image Support (Weeks 7-9)

**Goal:** Rust agent worker binary can execute tasks inside containers.

**sera-runtime implementation:**

1. Reasoning loop as async state machine (not recursive)
2. LLM client: reqwest streaming to `/v1/llm/chat/completions`
3. Tool executor trait with built-in implementations:
   - `file-read`, `file-write`, `file-list`
   - `shell-exec` (tokio::process)
   - `http-request`
4. Context manager with compaction strategy
5. Health server on `AGENT_CHAT_PORT`
6. Heartbeat for persistent mode
7. stdin/stdout task protocol (preserve existing JSON format)
8. Graceful shutdown and cancellation propagation

**Orchestrator changes (in TS first):**

- Template field: `spec.sandbox.runtime: "ts" | "rust"`
- Image resolution: `sera-agent-worker:latest` vs `sera-agent-worker-rs:latest`
- Both images support same stdin/stdout protocol

**Acceptance criteria:**

- [ ] `sera-runtime` binary reads TaskInput from stdin, calls LLM proxy, writes TaskOutput
- [ ] Health endpoint responds on configured port
- [ ] Heartbeat runs for persistent lifecycle mode
- [ ] Container image < 20MB (static musl binary)
- [ ] Same task produces equivalent output from TS and Rust runtimes
- [ ] At least one template running on Rust runtime in dev environment

---

### Phase 4: Write Path Migration (Weeks 10-14)

**Goal:** Rust owns write operations, subsystem by subsystem.

**Migration order (from migration plan):**

| Order | Subsystem             | Key Write Operations             | Risk   |
| ----- | --------------------- | -------------------------------- | ------ |
| 1     | Auth & API keys       | Key creation, session management | Low    |
| 2     | Provider registry     | Provider CRUD, secret ingestion  | Low    |
| 3     | Metering              | Usage recording, quota updates   | Medium |
| 4     | Audit trail           | Event append with hash chain     | Medium |
| 5     | Template & agent CRUD | Create/update/delete instances   | Medium |
| 6     | Sandbox lifecycle     | Container spawn/stop/teardown    | High   |
| 7     | Schedules & jobs      | Schedule CRUD, job dispatch      | High   |
| 8     | MCP & advanced        | MCP registration, skill bridging | Medium |

**For each subsystem:**

1. Implement write handlers in sera-core-rs
2. Add to route table (new port or path-based routing)
3. Run dual-write comparison tests
4. Switch ownership flag in DB ownership map
5. Remove TS write path for that subsystem

**Acceptance criteria per subsystem:**

- [ ] Write operation produces identical DB state to TS version
- [ ] No double-writes or ownership conflicts
- [ ] Rollback path documented and tested

---

### Phase 5: API Front Door Cutover (Weeks 15-16)

**Goal:** sera-core-rs becomes the primary API server on port 3001.

**Strategy:**

- Reverse proxy in front (nginx or axum itself)
- Route migrated paths to Rust
- Route remaining paths to TS (fallback)
- One canonical auth surface

**Acceptance criteria:**

- [ ] All clients (web, TUI, agents) work against Rust server
- [ ] Response latency same or better than TS
- [ ] Zero-downtime switchover with rollback path
- [ ] TS server can be re-enabled as fallback

---

### Phase 6: TUI Migration (Weeks 17-18)

**Goal:** Replace Go TUI with Rust ratatui implementation.

**sera-tui implementation:**

1. ratatui + crossterm terminal rendering
2. Typed API client (shared with sera-testing)
3. Agent list, detail, log views
4. Real-time event stream (SSE or WebSocket)
5. Keyboard-driven navigation

**Acceptance criteria:**

- [ ] Feature parity with Go TUI
- [ ] Single static binary distribution
- [ ] Shared API client crate with integration tests

---

### Phase 7: TypeScript Decommission (Week 19+)

**Goal:** Remove TypeScript backend services.

**Prerequisites:**

- All write paths owned by Rust
- Dual-run comparison clean for 2+ weeks
- Rollback practiced at least once

**Steps:**

1. Remove `sera-core-ts` from docker-compose
2. Remove `core/` source directory (archive to branch)
3. Remove `core/agent-runtime/` source
4. Update CI to build only Rust
5. Update `web/` API client if any TS-specific endpoints remain

---

## Type System Mapping Reference

### Key Enum Conversions

```rust
// AgentInstance.status — replace string union
pub enum AgentStatus {
    Created,
    Running,
    Stopped,
    Error,
    Unresponsive,
    Throttled,
    Active,
    Inactive,
}

// ChatMessage.role — replace string literal union
pub enum ChatRole {
    User,
    Assistant,
    System,
    Tool,
}

// AuditEntry.actor_type
pub enum ActorType {
    Operator,
    Agent,
    System,
}

// NamedList.metadata.type
pub enum NamedListType {
    NetworkAllowlist,
    NetworkDenylist,
    CommandAllowlist,
    CommandDenylist,
    SecretList,
}

// Lifecycle mode
pub enum LifecycleMode {
    Persistent,
    Ephemeral,
}

// Sandbox tier
pub enum SandboxTier {
    Tier1,
    Tier2,
    Tier3,
}
```

### Key State Machine Conversions

```rust
// AgentResponse — replace "bag of optionals" with enum
pub enum AgentAction {
    Thinking { thought: String },
    ToolCall { tool: String, args: serde_json::Value },
    Delegation { agent_role: String, task: String },
    FinalAnswer { answer: String, thoughts: Vec<CapturedThought> },
}

// Job lifecycle — replace stringly-typed status
pub enum JobState {
    Available { available_at: OffsetDateTime },
    Active { lease_owner: String, lease_expires_at: OffsetDateTime },
    Completed { result: serde_json::Value },
    Failed { error: String, attempts: u32 },
    Dead { error: String, attempts: u32 },
}
```

---

## Validation Strategy

### Primary: `cargo check` Loop

Every code change should be validated immediately:

```bash
# Fast incremental check (~1-3s after first build)
cargo check --workspace 2>&1

# Full test suite
cargo test --workspace

# Type checking + clippy lints
cargo clippy --workspace -- -D warnings
```

### LSP: rust-analyzer

rust-analyzer is installed (`~/.cargo/bin/rust-analyzer`) and detects the workspace. It provides:

- Real-time type error highlighting
- Go-to-definition across crates
- Inline type hints
- Auto-import suggestions

Use `mcp__plugin_oh-my-claudecode_t__lsp_diagnostics` to check errors on specific files.

### Contract Tests

For each migrated endpoint/parser:

1. Golden input files in `rust/contracts/`
2. Run same input through TS and Rust
3. Compare: status code, response shape, key field values
4. Use `insta` for snapshot testing Rust outputs

### Integration Tests

```bash
# Requires DATABASE_URL pointing to test PostgreSQL
cargo test --workspace --features integration

# Docker-based tests (sandbox, container lifecycle)
cargo test --workspace --features docker-integration
```

### CI Pipeline

```yaml
# Added alongside existing bun CI
rust-check:
  - cargo check --workspace
  - cargo clippy --workspace -- -D warnings
  - cargo test --workspace
  - cargo build --release (sera-core, sera-runtime)
```

---

## Build Performance Tips (Windows)

1. **Linker:** Install `lld` for faster link times:

   ```bash
   # In .cargo/config.toml
   [target.x86_64-pc-windows-msvc]
   linker = "lld-link"
   ```

2. **Incremental builds:** Enabled by default in dev profile

3. **cargo check vs cargo build:** Always use `check` during development — it skips codegen

4. **sccache:** Consider for CI to cache compiled dependencies

5. **Parallel compilation:** Default `codegen-units = 256` in dev is fine; release uses `1` for optimization

---

## External Dependency Inventory

| External Service     | TS Client                | Rust Client               | Crate       |
| -------------------- | ------------------------ | ------------------------- | ----------- |
| PostgreSQL           | `pg` (node-postgres)     | `sqlx`                    | sera-db     |
| Docker               | `dockerode`              | `bollard`                 | sera-docker |
| Centrifugo           | `reqwest` (HTTP API)     | `reqwest`                 | sera-events |
| Qdrant               | `@qdrant/js-client-rest` | `qdrant-client`           | sera-core   |
| Ollama (embeddings)  | `fetch`                  | `reqwest`                 | sera-core   |
| Squid (egress proxy) | File-based ACL           | File-based ACL            | sera-docker |
| LLM providers        | `@mariozechner/pi-ai`    | `reqwest` (OpenAI-compat) | sera-core   |
| pg-boss (job queue)  | `pg-boss` npm            | Custom `SKIP LOCKED`      | sera-core   |

---

## Risk Mitigation Checklist

- [ ] **No dual writers:** Ownership map maintained per table per phase
- [ ] **Contract tests:** Run before every phase transition
- [ ] **Rollback:** TS server can be re-enabled within 5 minutes
- [ ] **Idempotency:** All write operations have idempotency keys
- [ ] **Schema compat:** Only additive migrations during transition
- [ ] **Monitoring:** Both servers emit comparable metrics for comparison

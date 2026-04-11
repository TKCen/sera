# SERA 2.0 — Per-Crate Implementation Audit

> **Purpose:** Delta report of the current `rust/crates/*` workspace against the patched SERA 2.0 specs (post-spec round of 2026-04-11). Drives the Phase 0 implementation plan.
> **Date:** 2026-04-12
> **Basis:** SPEC-crate-decomposition §3, SPEC-dependencies, SPEC-self-evolution §5, SPEC-config, SPEC-gateway, SPEC-runtime, SPEC-hooks, SPEC-workflow-engine, SPEC-circles, SPEC-hitl-approval, SPEC-identity-authz, SPEC-tools, SPEC-observability, SPEC-versioning, HANDOFF.md.
> **Scope:** 14 existing crates + identification of new/missing crates per the target decomposition.

---

## 0. How to read this document

Each crate entry carries:

- **Classification**: `aligned` / `needs-extension` / `needs-rewrite` / `delete` / `missing`
- **Current shape** — what exists today
- **Target shape** — what the spec requires, with section references
- **Deltas** — specific missing types, structural conflicts, and design-forward obligations
- **Priority** — P0 (Phase 0 blocker) / P1 (Phase 1) / P2 (Phase 2+) / P3 (deferred)

The cross-cutting sections at the end collect workspace-level issues (rename `sera-core` → `sera-gateway`, crate extractions, sequencing).

---

## 1. Target workspace reference

Per SPEC-crate-decomposition §3, the target workspace has these layers and members:

### Foundation (no internal deps)

| Crate | Responsibility |
|---|---|
| `sera-types` | Shared domain types, IDs, `Principal` model, event model, protobuf definitions, `ApiVersion`, `ResourceKind`, `CapabilityManifest`, **self-evolution primitives** (§5.1: `ChangeArtifactId`, `BlastRadius`, `CapabilityToken`, `ConstitutionalRule`, `EvolutionTier`, `AgentCapability`) |
| `sera-config` | Composable manifests (`figment` 0.10), schema registry (`schemars`/`jsonschema`), env layering, hot-reload (`notify` 8.2), `ShadowConfigStore`, `ConfigVersionLog` |
| `sera-errors` | Unified error types with error codes |

### Infrastructure (deps: foundation only)

| Crate | Responsibility |
|---|---|
| `sera-db` | `sqlx` 0.8 compile-time-checked PostgreSQL + SQLite; migrations with reversibility contract (§4.7); **`sea-orm` forbidden** |
| `sera-queue` | Thin trait over `apalis` 0.7 + session-lane FIFO, global throttle, cron, orphan recovery |
| `sera-cache` | In-process (`moka`) + distributed (`fred`/Redis) |
| `sera-telemetry` | OTel triad pinned (`opentelemetry = "=0.27"`, `opentelemetry-otlp = "=0.27"`, `tracing-opentelemetry = "=0.28"`), hierarchical `Emitter` tree, **OCSF v1.7.0** audit events, separate audit write path |
| `sera-secrets` | Secret provider trait + env/file/Vault/AWS SM/Azure KV |

### Core Domain (deps: foundation + infrastructure)

| Crate | Responsibility |
|---|---|
| `sera-session` | 6-state session lifecycle, `ContentBlock` transcript, two-layer persistence (sqlx + shadow git), compaction |
| `sera-memory` | Memory trait + four-tier ABC (`Unconstrained/Token/SlidingWindow/Summarize+ReadOnly`), experience pool |
| `sera-tools` | Tool registry, `SandboxProvider` trait (Docker/WASM/MicroVM/External/**OpenShell**), `SsrfValidator`, TOFU SHA-256 binary identity, three-layer sandbox policy, `inference.local` virtual host, bash AST pre-exec |
| `sera-hooks` | WASM hook runtime (`wasmtime` 43, **extism rejected**), two-tier hook bus, **`constitutional_gate` fail-closed**, `updated_input` on `HookResult`, `wasi:http` allow-list |
| `sera-auth` | AuthN (JWT, OIDC, SCIM), `Principal` registry, `AuthZTrait`, RBAC (`casbin` 2.19), AuthZen, SSF/CAEP/RISC, **capability tokens with narrowing**, `MetaChange`/`CodeChange`/`MetaApprover` |
| `sera-models` | Model adapter trait + parser registry (hermes/mistral/llama/qwen/deepseek), structured output via `llguidance` |
| `sera-skills` | Skill pack loading, **`AGENTS.md`** + `SKILL.md` cross-tool standards, three-tier microagent classification |
| `sera-hitl` | Approval routing, escalation, `SecurityAnalyzer` trait, `AskForApproval` dispatch, `revision_requested` state, `CorrectedError { feedback }` |
| `sera-workflow` | `WorkflowTask` modeled on beads `Issue` with atomic claim, `bd ready` algorithm, `meta_scope` routing, `apalis` cron |
| **`sera-meta` (NEW)** | Self-evolution machinery — Change Artifact, blast-radius, constitutional anchor, shadow dry-run, two-generation live, kill switch. **Phase 4 impl; Phase 0–3 design-forward types only** |

### Interop

| Crate | Responsibility |
|---|---|
| `sera-mcp` | MCP via `rmcp` 1.3 |
| `sera-a2a` | A2A from `a2aproject/A2A` proto at pinned commit; `acp-compat` feature |
| `sera-agui` | AG-UI 17-event hand-rolled serde enum (~200 LoC) |

### Runtime & Gateway

| Crate | Responsibility |
|---|---|
| `sera-runtime` | Agent turn loop, KV-cache context pipeline, subagent management, `AgentHarness` impl |
| `sera-gateway` | HTTP/WS/gRPC server, SQ/EQ envelope, `AppServerTransport` dispatch, connector/plugin registry |

### Clients & SDKs

| Crate | Responsibility |
|---|---|
| `sera-sdk` | Client SDK library |
| `sera-cli` | CLI client |
| `sera-tui` | Terminal UI |
| `sera-plugin-sdk` | **crates.io**, hard import boundary |
| `sera-hook-sdk` | **crates.io**, Rust hook authoring |

### Deletions / Rename / Absorption

| Action | Rationale |
|---|---|
| **Drop `sera-acp`** | ACP merged into A2A under LF on 2025-08-25; legacy clients via `acp-compat` feature on `sera-a2a` (SPEC-interop §5, HANDOFF §4.2) |
| **Rename `sera-core` → `sera-gateway`** | §3 is unambiguous |
| **Rename `sera-domain` → `sera-types`** | §3 uses `sera-types` |
| **Rename `sera-events` → `sera-telemetry` (or split)** | Current scope is Centrifugo sidecar; target is the full OCSF/OTel observability layer |
| **Absorb `sera-docker` into `sera-tools`** | `SandboxProvider` impls live inside `sera-tools`, not as peer crates |
| **Add `sera-meta`** | NEW — self-evolution |
| **Add `sera-a2a`, `sera-agui`, `sera-mcp`** | Interop crates missing from current workspace |
| **Add `sera-models`, `sera-skills`, `sera-memory`, `sera-session`** | Core domain crates missing from current workspace |
| **Add `sera-secrets`, `sera-cache`, `sera-queue`** | Infrastructure crates missing (queue logic currently lives in `sera-db`) |

### Hard boundaries (§4)

1. Plugin SDK: external plugins may **only** import from `sera-plugin-sdk` — CI-enforced.
2. Layer direction is one-way: foundation → infrastructure → core domain → runtime → gateway → clients. No reverse deps.
3. `sea-orm` forbidden in `sera-db`.
4. `constitutional_gate` chain compiled-in as fail-closed; no runtime override.
5. OTel triad version-locked together.

---

## 2. Per-crate audit

### 2.1 `sera-domain` (rename → `sera-types`)

**Classification:** needs-extension

**Current shape:** Broad shared-types crate (~24 modules) covering agents, sessions, hooks, tools, events, principals, capabilities, memory, skills, observability, connectors, queues, sandboxes, secrets, models, config manifests. No internal deps.

**Target shape:** `sera-types` foundation crate, MUST carry all self-evolution Phase 0–3 design-forward primitives per SPEC-self-evolution §5.1.

**Deltas — missing types:**

- `ChangeArtifactId { hash: [u8; 32] }` content-hash newtype (§5.1)
- `BlastRadius` 22-variant enum: `AgentMemory`, `AgentPersonaMutable`, `AgentSkill`, `AgentExperiencePool`, `SingleHookConfig`, `SingleToolPolicy`, …, `FullBinary`, `DbMigration` (§5.1, §9)
- `ChangeProposer { principal_id, capability_token }` (§5.1)
- `CapabilityToken { scopes, expires_at, max_proposals, signature }` — distinct from existing `ResolvedCapabilities`/`FilesystemCapability` container caps (§5.1)
- `ConstitutionalRule { id, description, enforcement_point, content_hash }` (§5.1)
- `EvolutionTier` enum (`AgentImprovement | ConfigEvolution | CodeEvolution`) (§5.1)
- `AgentCapability` enum with `MetaChange | CodeChange | MetaApprover | ConfigRead | ConfigPropose` (SPEC-crate-decomposition §3 / SPEC-self-evolution §5.2)
- `BuildIdentity { version, commit, build_time, signer_fingerprint, constitution_hash }` (SPEC-versioning §4.6)

**Deltas — structural conflicts:**

- `ResourceKind` has 10 variants labeled "POST-MVS" but is missing `SandboxPolicy`, `Circle`, `ChangeArtifact` entirely — SPEC-config §2.2 requires 13 variants.
- `ResourceMetadata` has only `name, labels, annotations` — missing required `change_artifact: Option<ChangeArtifactId>` and `shadow: bool` (SPEC-config §2.1, SPEC-self-evolution §5.4).
- `PersonaSpec` has only `immutable_anchor` — missing `mutable_persona: Option<String>` and `mutable_token_budget: Option<u32>` required for Tier-1 self-improvement (SPEC-self-evolution §2.1).
- `SessionState` has 7 variants but is missing `Spawning`, `TrustRequired`, `ReadyForPrompt`, `Paused`, and `Shadow` required by SPEC-gateway §6.1 and SPEC-self-evolution §5.5.
- `TurnResult` in `sera-domain::runtime` must be replaced by `TurnOutcome` discriminated enum (`RunAgain | Handoff | FinalOutput | Compact | Interruption | Stop`) per SPEC-runtime §2.3. **Breaking change to core trait.**
- Flat `ChatMessage` (OpenAI-style `role` + `content` + `tool_calls`) must be replaced by `ConversationMessage { role, content: Vec<ContentBlock>, usage, cause_by: Option<ActionId> }` so compaction never splits a `ToolUse`/`ToolResult` pair (SPEC-runtime §2.5).
- `ContentBlock` enum (`Text | ToolUse { id, name, input } | ToolResult { tool_use_id, tool_name, output, error }`) does not exist.
- Crate name mismatch: spec uses `sera-types`, current uses `sera-domain`. Known alias per CLAUDE.md but must be reconciled at workspace level.

**Deltas — design-forward obligations:**

- `#[non_exhaustive]` attribute on `ResourceKind`, `SessionState`, `BlastRadius`, `EvolutionTier` per SPEC-versioning §5.2.
- All self-evolution primitives above must exist as Phase 0 types even though they are unused until Phase 4. Retrofitting is not acceptable per SPEC-self-evolution §5.

**Priority:** **P0** — first-mover; every downstream crate depends on these types stabilizing.

---

### 2.2 `sera-config`

**Classification:** needs-extension

**Current shape:** Two config scopes: `SeraConfig` (BYOH env vars) and manifest loading via `manifest_loader.rs` (K8s-style YAML). Has `FileWatcher` using `notify`. No schema registry, no layered resolution, no shadow store, no version log.

**Target shape:** Full composable manifest system per SPEC-config §4, §4.1, §7a, §7b.

**Deltas — missing dependencies:**

- `figment = "0.10"` — layered resolution (defaults → files → env → runtime)
- `schemars = "0.8"` — schema generation from Rust types
- `jsonschema = "0.38"` — validation at load time
- Pin `notify = ">=8.2, <9"`

**Deltas — missing types:**

- `SchemaRegistry { schemas: HashMap<(ResourceKind, ApiVersion), Schema> }` with `validate()`, `get_schema()`, `list_kinds()` (§4)
- `ConfigStore` trait (`load`, `get`, `list`, `version`) + `LiveConfigStore` impl (§7a)
- `ShadowConfigStore` overlay for dry-run validation (§7a, SPEC-self-evolution §5.4)
- `ConfigVersionLog` + `ConfigVersionEntry { version: u64, change_artifact, signature, prev_hash: [u8;32], this_hash: [u8;32] }` append-only with cryptographic chain (§7b, SPEC-self-evolution §5.4)
- `ConfigChangeApplied` event type carrying `ChangeArtifactId`

**Deltas — structural conflicts:**

- `ManifestSet` is a flat accumulator; must be layer-ordered with merge semantics per §3.3.
- No env-var override pattern `SERA_{KIND}_{NAME}_{FIELD}` (§3.1).
- `ManifestSet` has no `SandboxPolicy`, `Circle`, `ChangeArtifact` arms.

**Deltas — design-forward obligations:**

- 13 published JSON Schemas in `docs/schemas/` generated via `schemars::schema_for!` (§4.1, HANDOFF P1 item 4).
- CI check enforcing schema/type sync.
- `ShadowConfigStore` stub must exist even as an empty-overlay for SPEC-gateway §5.5 to compile in Phase 2.
- `ConfigVersionLog` append-only skeleton required Phase 0 per SPEC-self-evolution §5.4.

**Priority:** **P0**

---

### 2.3 `sera-events` (rename → `sera-telemetry`, or split)

**Classification:** needs-rewrite

**Current shape:** Centrifugo client (`CentrifugoClient` with JWT token generation + HTTP publish), `AuditHashChain` verifier operating on `sera-domain::audit::AuditRecord` with string-field concatenation. Scoped to legacy Centrifugo sidecar integration.

**Target shape:** `sera-telemetry` owns the OTel triad, hierarchical `Emitter` namespace tree, OCSF v1.7.0 audit log with separate write path, `LaneFailureClass`, `LaneCommitProvenance`, `RunEvidence`, `CostRecord`. Centrifugo is an infrastructure concern belonging in `sera-gateway` or a thin adapter.

**Deltas — missing dependencies:**

- `opentelemetry = "=0.27"` (exact pin)
- `opentelemetry-otlp = "=0.27"` (exact pin)
- `tracing-opentelemetry = "=0.28"` (exact pin) — **three must move together** (HANDOFF §6.4)

**Deltas — missing types:**

- `Emitter { namespace, context, trace, parent }` with `child()` / `root()` singleton (§2.1a)
- `EventMeta { id, name, path, created_at, source, creator, context, group_id, trace, data_type }` (§2.1a)
- OCSF `AuditEntry { ocsf_class_uid: u32, payload, prev_hash: [u8;32], this_hash: [u8;32], signature }` (§3.2) — current `AuditEntry` in `sera-domain::observability` has string fields, not OCSF-shaped
- `AuditBackend` trait with **only** `append` and `verify_chain` methods; `OnceCell<&'static dyn AuditBackend>` static binding at boot (§3.2, SPEC-self-evolution §5.7)
- `LaneFailureClass` 15-variant enum including `ConstitutionalViolation` and `KillSwitchActivated` (§3.3)
- `LaneCommitProvenance { commit, branch, worktree, canonical_commit, superseded_by, lineage }` (§3.4)
- `RunEvidence { tools_exposed, tools_called, approvals, memory_writes, model_calls, cost, outcome }` — current `ProofBundle` partially overlaps but is missing most fields (§3.1)

**Deltas — structural conflicts:**

- Wrong crate identity: `sera-events` Centrifugo-sidecar ≠ `sera-telemetry` full observability.
- `jsonwebtoken` dependency (used for Centrifugo JWT) must not live here — belongs in `sera-auth`. Creates unauthorized dep path.
- Audit log writes must use credentials never exposed to the normal event pipeline — current code has no credential isolation.
- `AuditBackend` trait must have NO delete/update API at any layer.

**Deltas — design-forward obligations:**

- Static `OnceCell` binding + separate audit write path — required Phase 0 per SPEC-self-evolution §5.7 (closes trust-collapse attack class).

**Priority:** **P0**

---

### 2.4 `sera-db`

**Classification:** needs-extension

**Current shape:** PostgreSQL via `sqlx` + embedded SQLite via `rusqlite`. Rich repositories (agents, audit, circles, metering, schedules, sessions, skills, API keys, delegations, memory, notifications, tasks, webhooks, job queue). Hand-rolled in-memory `LaneQueue` (~597 LoC, 5 queue modes, global concurrency cap). AES-GCM for at-rest encryption.

**Target shape:** `sera-db` stays conceptually but loses the queue to a new `sera-queue` crate; migrations carry the reversibility contract.

**Deltas — structural conflicts (major):**

- **Queue location**: hand-rolled `LaneQueue` conflicts with SPEC-crate-decomposition §3 / SPEC-dependencies §8.3 which mandate a `sera-queue` crate built as a thin trait over `apalis` 0.7. Current workspace has no `apalis` dep.
- **`rusqlite` vs `sqlx` for SQLite**: CLAUDE.md records this as intentional MVS divergence but needs an ADR documenting the path forward.

**Deltas — missing:**

- `apalis = "0.7"` + `apalis-sql` (for the new `sera-queue` crate)
- Migration framework (`sqlx migrate` or equivalent) — no `migrations/` directory currently
- `MigrationKind` enum (`Reversible | ForwardOnlyWithPairedMigrationOut | Irreversible`) on each migration (SPEC-versioning §4.7)
- `change_artifact_id: Option<ChangeArtifactId>` on migration metadata (Tier-2/3 audit trail, SPEC-versioning §4.7, SPEC-self-evolution §8)
- Orphan recovery in queue (currently purely in-memory → lost on restart)
- Confirm `sqlx = "0.8"` in workspace `Cargo.toml` (not 0.7)

**Priority:** **P0** — queue extraction is a Phase 0 blocker because `apalis` must be wired before the runtime turn loop lands.

---

### 2.5 `sera-core` (rename → `sera-gateway`) — **highest-leverage rewrite**

**Classification:** needs-rewrite

**Current shape:** Axum HTTP REST API server with route handlers, `AppState`, and `orchestrator::run_turn()` direct in-process dispatch. Carries session mgmt, scheduling, orchestration, MCP, LLM routing, process management all in one binary. Direct descendant of TS `sera-core`.

**Target shape:** `sera-gateway` — HTTP/WS/gRPC server, SQ/EQ event routing, harness dispatch via `AppServerTransport`, connector/plugin registry (SPEC-crate-decomposition §3, SPEC-gateway §3, §7a).

**Deltas — missing core types:**

- `Submission { id, op, trace: W3cTraceContext, change_artifact: Option<ChangeArtifactId> }` — the canonical inbound envelope (§3.1)
- `Op` enum: `UserTurn { items, cwd, approval_policy, sandbox_policy, model_override, effort, final_output_schema } | Steer | Interrupt | System(SystemOp) | ApprovalResponse | Register(RegisterOp)`
- `Event { id, submission_id, msg: EventMsg, trace, timestamp }` — canonical outbound envelope (§3.2)
- `EventMsg` enum covering streaming, lifecycle, HITL, compaction, session transitions
- `EventContext { agent_id, session_key, sender, recipient, principal, cause_by: Option<ActionId>, parent_session_key, generation: GenerationMarker, metadata }` — `cause_by` and `generation` are design-forward Phase 0–3 obligations (§3.2, SPEC-self-evolution §5.5)
- **`AppServerTransport` enum**: `InProcess | Stdio { command, args, env } | WebSocket { bind, tls } | Grpc { endpoint, tls } | WebhookBack { callback_base_url, session_api_key_generator } | Off` — **the architectural spine** (§7a, HANDOFF §4.1). Every harness MUST provide an `InProcess` variant for testing.
- `AgentHarness` trait: `supports(ctx) -> HarnessSupport`, `run_attempt`, `compact`, `reset`, `dispose` (§7.0)
- `HarnessSupportContext`, `HarnessSupport` (§7.0)
- `QueueBackend` trait + `QueueMode` enum (`Collect | Followup | Steer | SteerBacklog | Interrupt`) (§5, §5.2)
- `DedupeKey { channel, account, peer, session_key, message_id }` (§4.1)
- `GenerationMarker { label: GenerationLabel, binary_identity: BuildIdentity, started_at }` (SPEC-self-evolution §5.5)
- `WorkerFailureKind` enum (`TrustGate | PromptDelivery | Protocol | Provider`) (§6.1)
- `PromptMisdelivery` / `PromptReplayArmed` tracking + auto-recovery (§6.1a)
- Two-layer session persistence: `PartTable` (tool calls, text, reasoning streamed separately) + shadow git snapshot at `~/.local/share/sera/snapshot/` with `Snapshot.track()` / `revert()` / `diffFull()` (§6.1b)
- Kill-switch admin socket at `/var/lib/sera/admin.sock`, bypasses auth stack, OS-level file-ownership auth, `ROLLBACK` command (§7a.4, SPEC-self-evolution §5.5)
- Shadow session replay mode (`SessionState::Shadow`, replay event streams without mutating durable state) (§7a.3)
- `PluginEvent { event_id, event_type, correlation_id, circle_id, session_key, occurred_at, entity_id, entity_type, payload, actor_type, actor_id }` (SPEC-hooks §2.5)

**Deltas — structural conflicts:**

- **Crate rename `sera-core` → `sera-gateway`** is load-bearing: other workspace crates importing `sera-core` today must update paths; the rename must precede new type additions to avoid a double migration.
- **REST surface**: all routes (`POST /api/agents/{id}/chat`, `/api/sessions`) are direct HTTP RPC. SPEC-gateway §3 says "no parallel RPC surface bypasses the SQ/EQ envelope." Handlers must be wrapped as `Submission` emitters, not removed.
- **`AppState` has no `HarnessRegistry` / `PluginRegistry` / `QueueBackend` / `GenerationMarker`** — structural extension needed.
- **`services/orchestrator.rs` does direct harness dispatch** — must be refactored into an `AgentHarness::run_attempt` + `AppServerTransport` dispatch path. This is the central structural conflict with §7a.
- **`services/job_queue.rs` is not lane-aware** — no steer-at-tool-boundary contract (§5.2).

**Deltas — design-forward obligations:**

- `ChangeArtifactId` on `Submission` + `EventContext` (Phase 0)
- `GenerationMarker` on `EventContext` (Phase 2)
- Shadow session mode (Phase 2)
- Kill-switch admin socket — gateway MUST refuse boot if health check fails per CON-04 (Phase 1)
- `config_version_log` integration into config-change events (Phase 0)

**Priority:** **P0**

---

### 2.6 `sera-runtime`

**Classification:** needs-rewrite

**Current shape:** Library crate with `DefaultRuntime` implementing `AgentRuntime` — placeholder: runs the `ContextPipeline` then returns a stub string. Has real tools (`shell_exec`, `file_ops`, `http_request`, `web_fetch`, `grep`, `glob`), a `reasoning_loop.rs` stub, and a binary entry point (`main.rs`) reading `TaskInput` from stdin and writing `TaskOutput` to stdout (old MVS container-mode pattern).

**Target shape:** SPEC-runtime §2 (Agent shape, `supports`, `TurnOutcome`, `ContextEngine`, `ContentBlock`), §3 (four-method lifecycle), §3.1 (doom-loop), §6a (Compaction Pipeline), §9a (Action vs Tool), §9b (Handoff-as-Tool-Call).

**Deltas — missing types (critical):**

- **`TurnOutcome`** enum: `RunAgain | Handoff { target, filtered_input } | FinalOutput { content, typed } | Compact { trigger, preserve_recent } | Interruption { approval_id, risk, reason } | Stop { reason: StopReason }` (§2.3) — **replaces flat `TurnResult`, breaking change to `AgentRuntime` trait**.
- `AgentRuntime::supports(ctx) -> HarnessSupport` method (§2.2, SPEC-gateway §7.0) — makes harness selection capability-negotiated rather than hardcoded.
- `AgentRuntime::reset(params)`, `AgentRuntime::dispose(self: Box<Self>)` optional lifecycle methods (§2.2).
- `Agent<TContext>` struct with full field inventory: `handoff_description`, `instructions: Instructions`, `handoffs: Vec<Handoff<TContext>>`, `input_guardrails`, `output_guardrails`, `output_type`, `hooks: Option<AgentHooks<TContext>>`, `tool_use_behavior: ToolUseBehavior`, `capabilities: HashSet<AgentCapability>`, `react_mode: ReactMode`, `watch_signals: HashSet<ActionId>` (§2.1).
- `ContextEngine` trait (`bootstrap`, `ingest`, `assemble`, `compact`, `maintain`, `after_turn`, `describe`) as a **separately pluggable axis** orthogonal to `AgentRuntime` (§2.4). Current `ContextPipeline` is a monolithic struct, not trait-based.
- `ContextWindow { messages, estimated_tokens, system_prompt_addition }` (§2.4).
- `ContentBlock` enum: `Text(String) | ToolUse { id, name, input } | ToolResult { tool_use_id, tool_name, output, error }` (§2.5). Compaction correctness requires this — flat OpenAI `ChatMessage` cannot guarantee ToolUse/ToolResult pairs stay together.
- `ConversationMessage { role, content: Vec<ContentBlock>, usage, cause_by: Option<ActionId> }` — `cause_by` is the MetaGPT routing key (§2.5, §9a).
- Four-method turn lifecycle: `_observe` (content-addressed filter by `cause_by ∈ watch_signals`) → `_think` (LLM or deterministic if `react_mode = ByOrder`) → `_act` (tool dispatch) → `_react` (returns `TurnOutcome`) (§3).
- `DOOM_LOOP_THRESHOLD = 3` doom-loop detector emitting `TurnOutcome::Interruption` with `ActionSecurityRisk::Medium` (§3.1).
- **Compaction Pipeline (§6a):**
  - `Condenser` trait
  - `PipelineCondenser` orchestrator
  - 9 concrete impls: `NoOpCondenser`, `RecentEventsCondenser`, `ConversationWindowCondenser`, `AmortizedForgettingCondenser`, `ObservationMaskingCondenser`, `BrowserOutputCondenser`, `LLMSummarizingCondenser`, `LLMAttentionCondenser`, `StructuredSummaryCondenser`
  - `CompactionCheckpoint { checkpoint_id, session_key, reason, pre_compaction, post_compaction, tokens_before, tokens_after, summary, created_at }` with `MAX_COMPACTION_CHECKPOINTS_PER_SESSION = 25`
  - `CheckpointReason` (`Manual | AutoThreshold | OverflowRetry | TimeoutRetry`)
- `Action<TInput, TOutput> { name, description, system_prompt_prefix, model_binding, output_schema, executor }` — Actions distinct from Tools (§9a).
- `Handoff<TContext> { tool_name, tool_description, input_json_schema, on_invoke_handoff, input_filter: Option<HandoffInputFilter> }` + `HandoffInputData { input_history, pre_handoff_items, new_items }` (§9b, HANDOFF §4.4).
- `ReactMode` enum: `React | ByOrder | PlanAndAct` (§2.1).
- `Instructions` enum: `Static(String) | Dynamic(Box<dyn Fn(...)>)` (§2.1).
- `ToolUseBehavior` discriminated union (§2.1).
- `PersonaConfig { immutable_anchor, mutable_persona, mutable_token_budget }` (§4.3).
- `ContextStep::stability_rank() -> u32` for KV-cache ordering (§4.1).
- `TurnHeartbeat { turn_id, status: TurnStatus, tool_calls_completed, current_tool, elapsed, tokens_used }` (§5.5).
- `ToolCallMetadata { idempotency_key, attempt, max_retries, completed }` (§6.1).
- Steer queue check after each tool call — cooperation with gateway queue for `Steer` mode (§6 step 9, SPEC-gateway §5.2).
- `ShadowSandboxProvider` for replay mode (§3.3).

**Deltas — structural conflicts:**

- `TurnResult` → `TurnOutcome` is a breaking change to `AgentRuntime::execute_turn` return type. All callers and tests must migrate. Must land in `sera-domain`/`sera-types` first.
- `ChatMessage` (`Vec<serde_json::Value>` on `TurnContext`, `Vec<ChatMessage>` on `types.rs`) uses flat OpenAI format. Replacing with `Vec<ConversationMessage>` with `Vec<ContentBlock>` content propagates through LLM client, context pipeline, tool execution.
- `ContextPipeline` is a concrete struct, not a `ContextEngine` trait implementor. Must be wrapped behind the trait.
- `DefaultRuntime::execute_turn` returns a placeholder string — reasoning loop, LLM client, tool loop are stubs. No part of the four-method lifecycle is wired.
- **Binary `main.rs` stdio loop is the old MVS pattern**: reads `TaskInput`/writes `TaskOutput`. Must be re-plumbed as `AppServerTransport::Stdio` variant: read `Submission` JSON from stdin, write `Event` JSON to stdout.

**Deltas — design-forward obligations:**

- `TurnContext` must carry `change_artifact: Option<ChangeArtifactId>` (SPEC-self-evolution §5.3, Phase 1).
- `ConversationMessage.cause_by: Option<ActionId>` (Phase 0 — required for `_observe` watch_signals filtering).
- Shadow session replay mode check + `ShadowSandboxProvider` path (Phase 2).
- `PersonaConfig::immutable_anchor` separated from mutable persona (Phase 1 — required before Tier 1 self-improvement).

**Priority:** **P0**

---

### 2.7 `sera-hooks`

**Classification:** needs-extension

**Current shape:** Coherent in-process hook skeleton — `Hook` async trait, `ChainExecutor` with timeout/fail-open/skip-disabled logic, `HookRegistry`, error types. Backing domain types in `sera-domain::hook` define `HookPoint` (16 variants), `HookChain`, `HookInstance`, `HookResult` (Continue/Reject/Redirect), `HookContext`, `HookMetadata`, `WasmConfig`, `ChainResult`. Executor correctly implements chain timeout, per-hook timeout, fail-open propagation. **This is the most complete of the three core-domain crates.**

**Target shape:** SPEC-hooks §2.4 (`constitutional_gate`), §2.5 (two-tier bus), §3 (hook points), §5.4 (`wasi:http` allow-list). `wasmtime` 43 as WASM runtime (extism explicitly rejected, HANDOFF §4.8).

**Deltas — missing hook points:**

- `HookPoint::ConstitutionalGate` — Phase 1, fail-closed compiled-in, may only `Continue` or `Reject` (not `Redirect`) (§2.4, SPEC-self-evolution §5.3). Current `ALL.len() == 16` asserts — target is 17+.
- `HookPoint::OnLlmStart` and `HookPoint::OnLlmEnd` (§3, SPEC-runtime §10).
- `HookPoint::SubagentDeliveryTarget` — fires between subagent completion and parent-session delivery; integration point for `ResultAggregator` (§3).
- `HookPoint::OnChangeArtifactProposed` — self-evolution observability (§3, SPEC-self-evolution §5.3).

**Deltas — missing types/fields:**

- `HookResult::updated_input: Option<serde_json::Value>` — hooks can TRANSFORM inputs, not just gate (§2.2, SPEC-dependencies §10.1 claw-code pattern). Key insight from the research round. Current `HookResult::Continue` carries only `context_updates` — `updated_input` has dedicated semantics.
- `HookResult` shape: spec uses a struct `{ outcome: HookOutcome, messages, denied, permission_overrides, updated_input }` wrapping an enum; current implementation uses an enum directly. Restructuring required.
- `HookAbortSignal { inner: Arc<AtomicBool> }` with `abort()` / `is_aborted()` on `HookContext` (§2.2, SPEC-dependencies §10.1).
- `HookContext::change_artifact: Option<ChangeArtifactId>` — Phase 1 design-forward obligation (§2.3, SPEC-self-evolution §5.3).
- `HookContext::tool_input: HookToolInput` with `HookToolKind` discriminant (`Shell | Patch | Mcp { server } | WebSearch | FileRead | FileWrite | Memory | Custom(String)`) — currently `tool_call: Option<serde_json::Value>` untyped (§2.3, Codex pattern).
- `HookInstance::enforcement: EnforcementMode` (`enforce | audit`) per-hook mode for incremental policy promotion (§3.1, NemoClaw pattern).
- **Two-tier hook bus:**
  - `InternalHookBus` type (§2.5)
  - `PluginHookBus` type (§2.5)
  - `PluginEvent { event_id, event_type, correlation_id, circle_id, session_key, occurred_at, entity_id, entity_type, payload, actor_type, actor_id }` (§2.5)
  - `validate_plugin_event_namespace(plugin, event) -> Result<(), SpoofError>` anti-spoofing (§2.5)
- `CommandHook { command, args, env, timeout }` subprocess hook type (§2.6).

**Deltas — missing WASM runtime:**

- `wasmtime = ">=43, <50"` dependency (loose range, revisit quarterly per HANDOFF §6.5)
- `wasmtime-wasi`, `wasmtime-wasi-http`
- `SeraHookState` implementing `WasiHttpView::send_request` with allow-list check and audit emission on denial (§5.4 — the sole outbound-network path for hooks)
- `HostAllowList` type on `HookChain` or `WasmConfig` (§5.4)
- Fuel metering, memory caps, per-instance config
- `extism` explicitly **MUST NOT** be added (HANDOFF §4.8)

**Deltas — structural conflicts:**

- `ChainExecutor::execute_chain` uses `chain.fail_open` uniformly. The `constitutional_gate` chain's `fail_open` is compiled-in and NOT runtime-configurable. Executor needs a branch: if `chain.point == HookPoint::ConstitutionalGate`, ignore `chain.fail_open` and always run fail-closed. Additionally reject `Redirect` outcomes.
- `HookRegistry` single `HashMap` — two-tier bus requires either two registries or a bus-level wrapper enforcing caller identity access control.

**Priority:** **P1** (executor logic is sound; gaps are mostly additive — but `ConstitutionalGate` enforcement + `HookResult` restructuring have Phase 1 dependencies from `sera-runtime` and `sera-gateway`).

---

### 2.8 `sera-workflow`

**Classification:** needs-rewrite

**Current shape:** Pre-research skeleton: `WorkflowDef { name, trigger, agent_id: String, config, enabled }`, `WorkflowTrigger` (Cron|Event|Threshold|Manual), `WorkflowRegistry` (in-memory name→def map), `DreamingConfig`/`DreamingPhases`/`DreamingWeights`/`DreamCandidate` (correctly shaped), `CronSchedule`. **No async executor, no task queue, no claim protocol, no persistence.**

**Target shape:** SPEC-workflow-engine §2a–§2d, §4a–§4e, §6.1 — full DAG execution substrate modeled on `gastownhall/beads` `Issue` schema. **Promoted from Phase-3 deferral to Phase-1 design input.**

**Deltas — missing types (entire execution substrate):**

- `WorkflowTask` (§2a): `id: WorkflowTaskId`, `title`, `description`, `acceptance_criteria`, `status: WorkflowTaskStatus`, `priority: u8`, `task_type: WorkflowTaskType`, `assignee: Option<PrincipalRef>`, `due_at`, `defer_until`, `metadata`, `await_type: Option<AwaitType>`, `await_id`, `timeout`, `mol_type`, `work_type`, `ephemeral: bool`, `wisp_type`, `source_formula`, `source_location`, **`meta_scope: Option<BlastRadius>`** (Phase 1 obligation per SPEC-self-evolution §5.8), `change_artifact_id: Option<ChangeArtifactId>`.
- `WorkflowTaskStatus`: `Open | InProgress | Hooked | Blocked | Deferred | Closed | Pinned` — `Hooked` is the beads atomic-claim state; conflating with `InProgress` is a semantic error.
- `AwaitType`: `GhRun | GhPr | Timer | Human | Mail | Change` (§2a).
- `DependencyType`: `Blocks | Related | ParentChild | DiscoveredFrom | ConditionalBlocks` — `ConditionalBlocks` is a first-class primitive ("B runs only if A fails"), not generic metadata (§2b).
- `WorkflowTaskDependency { from, to, kind }` (§2b).
- `WorkflowTaskId` — SHA-256 content hash over canonical `(title+description+acceptance_criteria+source_formula+source_location+created_at)`, newtype over `[u8;32]` (NOT `String`/`Uuid`). Merge-safe across branches (§2c).
- `WorkflowSentinel` enum: `Start | SelfLoop | Prev | Next | End | Named(String)` (§2d, BeeAI).
- `ClaimToken` (§4b) — idempotency key carried through claim; crash+retry must not double-execute.
- `ClaimError` with `StatusMismatch` variant (§4b).
- `WorkflowTermination { reason: TerminationReason }` with `NRoundExceeded | Idle | BudgetExhausted | ExplicitStop` (§4c).
- `WorkflowEngine` / `WorkflowExecutor` (§4):
  - `ready_tasks(principal) -> Vec<WorkflowTask>` implementing **`bd ready`** (§4a): `status==Open` + no open/in-progress blocker + `defer_until` past/None + all `AwaitType` gates satisfied
  - `claim_task(task_id, agent_id) -> Result<ClaimToken, ClaimError>` atomic (§4b)
  - Termination triad: `n_round` countdown + `is_idle` convergence + cost budget (§4c)
  - `StaleClaimReaper` background task (§4b, §4d)
- `WorkflowDef` missing: `agent: AgentRef` (not `String`), `hook_chain: Option<HookChainRef>`, `step_sentinels: bool`.
- `WorkflowError` missing: `ClaimFailed`, `TaskNotFound`, `BudgetExhausted`, `NRoundExceeded`.

**Deltas — structural conflicts:**

- `WorkflowDef.agent_id: String` not type-safe against `PrincipalRef`.
- `WorkflowRegistry` is sync, in-memory, no persistence, no transaction — cannot be the execution store. Needs async `WorkflowEngine` with transactional store (sqlx Tier-1/2, Dolt SQL Tier-3).
- `DreamingConfig.schedule: String` creates a parallel config surface divergent from "dreaming is a normal workflow" spec intent.
- No `apalis` dependency.

**Deltas — design-forward obligations:**

- `meta_scope: Option<BlastRadius>` on `WorkflowTask` — SPEC-self-evolution §5.8 requires Phase 1. When `Some`, engine routes through self-evolution pipeline (constitutional gate, `MetaChange` capability, `ShadowSession` replay, `Applied` event).
- `change_artifact_id: Option<ChangeArtifactId>` provenance link (Phase 1).
- Atomic claim protocol must exist before Circle coordination can be built (SPEC-circles §3b / §5.1 share the same primitive).

**Priority:** **P0**

---

### 2.9 `sera-hitl`

**Classification:** needs-extension

**Current shape:** Well-formed pre-expansion skeleton: `ApprovalScope` (ToolCall|SessionAction|MemoryWrite|ConfigChange), `ApprovalUrgency`, `ApprovalEvidence`, `ApprovalTarget`, `ApprovalRouting` (Static|Dynamic|Autonomous), `ApprovalPolicy`, `ApprovalSpec`, `ApprovalTicket` with full state machine, `TicketStatus`, `ApprovalRouter`, `EnforcementMode` (Autonomous|Standard|Strict). 17 tests covering lifecycle, multi-approval, rejection, escalation, router logic, serde roundtrips. **Core ticket lifecycle is correct.**

**Target shape:** The 293→531 line SPEC-hitl-approval expansion added 7 new subsystems, none represented.

**Deltas — missing subsystems:**

**1. SecurityAnalyzer (§2a):**
- `SecurityAnalyzer` async trait: `security_risk(action) -> ActionSecurityRisk`, `name() -> &str`
- `ActionSecurityRisk` enum: `Low | Medium | High`
- Three reference backends: `InvariantAnalyzer`, `GraySwanAnalyzer`, `HeuristicAnalyzer`
- `confirmation_mode: bool` on agent controller layer
- Must be **pluggable trait** for per-deployment-tier injection

**2. Guardian (§2b):**
- `GuardianAssessment { risk_level: GuardianRiskLevel (Low|Medium|High), rationale: String, recommended_action: GuardianRecommendation (AutoApprove|SurfaceToUser|Block) }`
- `ApprovalEvidence.guardian_assessment: Option<GuardianAssessment>` field

**3. AskForApproval dispatch (§3):**
- `AskForApproval` enum: `UnlessTrusted | OnRequest | Granular(GranularApprovalConfig) | Never | Policy(ApprovalRouting)` — top-level 5-level dispatch (Codex pattern). `EnforcementMode` may survive as a lower-level enforcement gate.

**4. ApprovalScope extensions (§2, SPEC-self-evolution §9):**
- `ApprovalScope::ChangeArtifact(ChangeArtifactId)` — Tier 2/3 self-evolution
- `ApprovalScope::MetaChange(MetaChangeContext)` — approval-path self-modification
- `ApprovalScope::ToolCall` should use `ToolRef` not `{ tool_name: String, risk_level }` (minor).
- `ApprovalTarget::PrincipalGroup(PrincipalGroupId)` and `ApprovalTarget::ExternalPDP`

**5. GranularApprovalConfig (§5a):**
- Per-category routing: `exec`, `patch`, `file_write`, `network`, `mcp_call`, `memory_write`, `config_change`, `meta_change: Option<CategoryRouting>`
- `CategoryRouting { default, allow_list: Vec<ExecAllowRule>, auto_allow_skills: bool }`
- `ExecAllowRule { agent_ref, pattern, arg_pattern, reason }` (openclaw)
- Wildcard evaluator: deny > ask > allow, with per-session temp overrides (opencode)

**6. CorrectedError / ToolResult::Rejected (§5b):**
- `ToolResult::Rejected { feedback: String }` — highest-leverage new pattern. User rejection reasons flow back as structured tool-result, enabling in-turn self-correction without a turn restart. May live in `sera-domain`/`sera-runtime`, but HITL must produce it.

**7. RevisionRequested state (§5c):**
- `ApprovalState` enum distinct from `TicketStatus`: `Pending | Approved | Rejected { reason } | RevisionRequested { feedback }` — two-step revision cycle (Paperclip).
- `ApprovalTicket::reject()` must split into `reject_final()` and `request_revision(approver, feedback) -> ApprovalState` — current collapses to terminal `Rejected`.

**8. Doom-loop escalation category (§5d):**
- Dedicated `doom_loop` approval category or `ApprovalScope` variant
- Triggered when `DoomLoopDetector` fires at threshold 3
- Surfaces `GuardianAssessment`; three responses: `Approve` (reset counter), `Reject` (stop), `Rewrite` (inject `CorrectedError { feedback }`)

**9. MetaChangeContext (§5e):**
- `MetaChangeContext { change_artifact, pinned_approvers: HashSet<PrincipalRef> (frozen at proposal time), required_signers: u32, offline_key_required: bool, observability_escalation: bool }`
- **Approver-pinning semantics enforced at ticket creation time**: live `MetaApprover` set snapshotted into ticket; subsequent mutations do not affect in-flight ticket. Closes "remove approver then push change" attack class.

**10. Guardrails (§6a):**
- `InputGuardrail` / `OutputGuardrail` traits with `GuardrailResult { tripwire_triggered, output_info }`
- `run_in_parallel() -> bool` — default `true` (concurrent with LLM call via `tokio::join!`)
- Registered per-agent, not per-ticket

**Deltas — dependency + error type gaps:**

- `async-trait` dependency (missing from `Cargo.toml`)
- `ApprovalSpec` missing: `security_risk: ActionSecurityRisk`, `meta_change: Option<MetaChangeContext>`
- `HitlError` missing variants: `DoomLoopEscalation`, `MetaChangeInFlight`, `OfflineKeyRequired { scope: BlastRadius }`, `GuardrailTripwire { guardrail: String }`

**Priority:** **P1** (existing core is sound; gaps are additive. `ApprovalScope::ChangeArtifact`/`MetaChange` + `MetaChangeContext` are P0-adjacent since Tier-2 approval routing blocks on them.)

---

### 2.10 `sera-auth`

**Classification:** needs-extension

**Current shape:** Six modules (`api_key`, `authz`, `error`, `jwt`, `middleware`, `types`). `JwtService` (HS256 only, no JWKS), `ApiKeyValidator` (**plaintext hash comparison — no argon2**), `ActingContext` flat struct, `AuthMethod` (ApiKey|Jwt|Oidc [stub]), `AuthorizationProvider` trait (placeholder `DefaultAuthzProvider` returns `Allow` for everything), `Action` (missing `ProposeChange`/`ApproveChange`), `Resource` (missing `ChangeArtifact`), `AuthzDecision::NeedsApproval(String)` (should be `ApprovalSpec`). Axum bearer middleware JWT-only (no API-key branch). No `argon2`, `casbin`, feature gating, `Principal` enum, `CapabilityToken`, or `AgentCapability`.

**Target shape:** SPEC-crate-decomposition §3 (Core Domain) — full AuthN/AuthZ stack with `jsonwebtoken` 10, `openidconnect` 3.5, `oauth2` 5, `casbin` 2.19, `argon2`, `scim-server` 0.5, `axum-login`, `tower-sessions`. Feature-gated enterprise tier.

**Deltas — missing dependencies:**

- `argon2` (Tier-1 password hashing, §4.1)
- `casbin = "2.19"` (RBAC, §5.3)
- `openidconnect = "3.5"` (behind `enterprise` feature)
- `oauth2 = "5"` (behind `enterprise` feature)
- `scim-server = "0.5"` + `scim_v2` (behind `enterprise` feature)
- `axum-login`, `tower-sessions` (behind `enterprise` feature)
- `[features]` block: `default = ["jwt", "basic-auth"]`, `enterprise = ["oidc", "scim", "authzen", "ssf"]` (SPEC-crate-decomposition §6.2)

**Deltas — missing types:**

- `Principal` enum: `Human | Agent | ExternalAgent | Service` with full sub-structs (§2)
- `PrincipalGroup` + registry (§3)
- **`AgentCapability` enum with `MetaChange`, `CodeChange`, `MetaApprover`** (§5.1a) — Phase 0–3 design-forward obligation
- **`CapabilityToken`** with narrowing rule enforcement, `proposals_consumed` counter, `revocation_check_required` flag (§5.1b) — Phase 0–3 design-forward obligation
- `Action::ProposeChange(BlastRadius)` + `Action::ApproveChange(ChangeArtifactId)` variants (§5.1)
- `Resource::ChangeArtifact(ChangeArtifactId)` variant (§5.1)
- `AuthzDecision::NeedsApproval` payload must be `ApprovalSpec` (structured) not `String`
- API-key branch in `auth_middleware` (currently JWT-only)
- Typed `PrincipalRef` insertion into request extensions instead of `ActingContext`
- `BasicAuthValidator` with argon2
- AuthZen PDP ~60-line `reqwest` wrapper (§5.4, behind `enterprise`)
- SSF/CAEP/RISC SET ingester (~300 LoC over `jsonwebtoken`, §6.1, behind `enterprise`)

**Deltas — structural conflicts:**

- `ActingContext` is a flat ad-hoc struct; spec canonicalizes `Principal`+`PrincipalRef`. `ActingContext` should be dropped or become a lightweight view over `PrincipalRef`.
- `StoredApiKey.key_hash` plaintext comparison (`k.key_hash == token`) must be replaced before `basic-auth` feature is declared stable.
- `JwtService` HS256-only — OIDC path requires RS256 with JWKS refresh; signing-algorithm abstraction needed.

**Priority:** **P0** (capability token + agent capability types are Phase 0–3 obligations; feature-gate scaffold must land in Phase 0 per §6.2)

---

### 2.11 `sera-docker` → **absorb into `sera-tools`**

**Classification:** needs-rewrite (or delete after extraction)

**Current shape:** Three modules (`container`, `events`, `error`). `ContainerManager` (bollard create/start/stop/remove/exec with tiered resource limits via `u32` tier number), `DockerEventListener` (Centrifugo-published start|stop|die|oom|health_status events with exponential backoff), `ExecOutput`. Labels: `sera.sandbox`, `sera.agent`, `sera.instance`, `sera.type`, `sera.template`, `sera.managed`. **Does not implement `SandboxProvider`.** No sandbox policy types, TOFU, hot-reload, `SsrfValidator`, Landlock awareness. Depends on `sera-events::CentrifugoClient`.

**Target shape:** SPEC-crate-decomposition §3 does **not** list `sera-docker` as a standalone target crate. Docker sandbox functionality is a `DockerSandboxProvider` impl residing **inside `sera-tools`**. `sera-docker` is a pre-decomposition artifact.

**Deltas — missing (to be implemented in `sera-tools`):**

- `SandboxProvider` trait impl: `create`, `execute`, `read_file`, `write_file`, `destroy`, `status` (§6a.4)
- Three-layer `SandboxPolicy` model: coarse enum + `FileSystemSandboxPolicy` + `NetworkSandboxPolicy` with `NetworkPolicyRule`/`NetworkEndpoint`/`L7Rule` (§6a.0)
- `PolicyStatus` with version tracking + SHA-256 content hash for hot-reload (§6a.1)
- Binary SHA-256 TOFU identity: `NetworkBinary { path, tofu_sha256 }` (§6a.2)
- `PolicyDraftAdvisor` AI-assisted policy advisor integration point (§6a.3)
- `inference.local` virtual host routing (§6a.6)
- Pre-execute tree-sitter bash AST analysis (§6a.7)
- `OpenShellSandboxProvider` as Tier-3 backend alternative (§6a.5) — vendor `openshell.proto` + `sandbox.proto` + `datamodel.proto` at pinned commit
- Landlock rule-union awareness (deny-by-default, `include_workdir = false`)
- `SsrfValidator` integration
- `regorus` dependency for in-process OPA
- Deny-by-default filesystem policy (`/sandbox` read-only, writable state in `/sandbox/.agent-data/`)

**Deltas — structural conflicts:**

- Exists as standalone peer crate but target absorbs into `sera-tools`. Resolution: migrate `ContainerManager` + `DockerEventListener` into `sera-tools/src/sandbox/docker.rs`; remove or retain `sera-docker` as an internal (non-published) integration-test helper. **Do NOT create a peer `sera-sandbox` crate.**
- `start_container(tier: Option<u32>)` conflates deployment tier with sandbox policy (orthogonal concepts). Target uses typed `SandboxConfig` + attached `SandboxPolicy`.
- `DockerEventListener` direct Centrifugo dependency — in target, sandbox provider emits to abstract `AuditHandle` from `ToolContext`, not directly to Centrifugo.
- No pre-execute policy check before `exec_in_container` — must pass through the full policy stack including bash AST analysis.

**Priority:** **P0** (SandboxProvider trait is Phase 1 work; three-layer policy types are Phase 0 design-forward obligations)

---

### 2.12 `sera-byoh-agent`

**Classification:** needs-extension (scope deferred)

**Current shape:** Binary crate. Four modules (`health`, `heartbeat`, `llm`, `main`). stdin→stdout `TaskInput`/`TaskOutput` pipeline, `/health` axum endpoint, heartbeat loop POSTing to `{core_url}/api/agents/{instance_id}/heartbeat`, plain `POST /chat/completions` LLM call (no streaming, no structured output, no tool calls). Ephemeral + persistent lifecycle modes.

**Target shape:** Not explicitly listed in SPEC-crate-decomposition §3. Target manifest lists `sera-cli`, `sera-tui`, `sera-sdk`, `sera-runtime`, `sera-gateway` as harness/client binaries. The BYOH **pattern** (external agent process implementing the SERA harness contract) remains relevant — it maps to `AppServerTransport::{Stdio, WebSocket, Grpc}` variants and `ExternalSandbox` execution target. But this **specific binary** predates `AppServerTransport` and `sera-sdk`.

**ACP relevance:** HANDOFF §4.2 drops ACP. BYOH itself is not ACP-specific. Whether this binary survives depends on whether `sera-runtime`'s canonical `Stdio` transport covers the same use case.

**Deltas — missing:**

- `AppServerTransport::Stdio` compliance — current stdin/stdout protocol doesn't implement SQ/EQ envelope (SPEC-gateway §7a)
- Tool call handling (no tool registry, `ToolContext`, hook invocation)
- Session state (no 6-state machine, no compaction)
- `sera-sdk` dependency — should be thin SDK consumer, not raw `reqwest` calls
- Streaming `ContentBlock` events
- `ActingContext` extraction and `AuthorizationProvider` check (currently just passes through an `identity_token` Bearer header)

**Deltas — structural conflicts:**

- Heartbeat endpoint (`/api/agents/{instance_id}/heartbeat`) belongs to old TS `sera-core` REST surface, not the Rust gateway event model.
- `TaskInput`/`TaskOutput` are V1 types; target evolves them into `Submission`/`Event` envelopes.

**Recommendation:** Do **not** delete. Retain as Tier-1 smoke-test harness and reference for BYOH deployment pattern. Do **not** promote as canonical harness. Rewrite as thin `sera-sdk` consumer once `sera-sdk` and `AppServerTransport::Stdio` land.

**Priority:** **P2** — not blocking Phase 0/1.

---

### 2.13 `sera-testing`

**Classification:** missing (stub only)

**Current shape:** Four-line doc comment in `src/lib.rs`. `Cargo.toml` declares deps (`sera-domain`, `sera-db`, `sqlx`, `serde`, `serde_json`, `tokio`, `insta`) but zero implementation.

**Target shape:** SPEC-crate-decomposition §8.5 lists this as an Open Question ("Should there be a `sera-test-utils` crate?"). §6.1 workspace `Cargo.toml` target member list **does not include** `sera-testing` — either oversight or deliberate exclusion. HANDOFF §5 items 1 and 4 implicitly confirm need.

**Deltas — to implement:**

- `TestDatabase` — ephemeral Postgres or SQLite test pool with automatic teardown
- Fixture builders for `Principal`, `Agent`, `AgentTemplate`, `SandboxPolicy`, `ChangeArtifact`, `CapabilityToken`
- `insta` snapshot helpers normalizing UUIDs, timestamps, container IDs
- Contract test runner: Rust vs TypeScript golden YAML manifest comparison (referenced in `rust/CLAUDE.md`)
- Mock `SandboxProvider` for unit-testing `sera-tools` without Docker
- Mock `AuthorizationProvider` for unit-testing auth flows without `casbin`
- Mock `AuditHandle` for unit-testing tool execution without a real audit backend
- `schemars::schema_for!` emission helpers for HANDOFF §5 item 4

**Deltas — structural:**

- Declare `publish = false` and add to workspace `Cargo.toml` explicitly, or accept as workspace-internal `[dev-dependencies]`-only crate.

**Priority:** **P1** — scaffold in parallel with `sera-tools`/`sera-auth` real implementations.

---

### 2.14 `sera-tui`

**Classification:** aligned (for current scope) / deferred (for Phase 0 target)

**Current shape:** Binary crate with four modules (`api`, `app`, `ui`, `views`). `App` with three views (Agents list, AgentDetail, Logs), `AgentsView` with `j/k` navigation, `ApiClient` making raw `reqwest` calls to `/api/agents/instances`, stub `get_agent_logs` returning empty. `r` refresh, `q` quit, `Enter` detail, `l` logs. 5-second auto-refresh. **Code is complete and functional for its declared scope**, correctly connects to TS-era API.

**Target shape:** SPEC-crate-decomposition §3 (Clients) — `sera-tui | Terminal UI | ratatui, sera-sdk`. Crate identity is correct; implementation gaps are expected.

**Deltas — missing:**

- `sera-sdk` dependency (replace raw `reqwest`)
- Session view
- Streaming event feed (WebSocket/SSE consumer for `Event` envelopes — current polling model won't scale to event-driven gateway)
- HITL approval UI (approval queue + respond to `NeedsApproval`)
- Capability token display/revoke UI
- `AgentDetail` and `Logs` views (currently render "not yet implemented")

**Deltas — structural conflicts:**

- **Hardcoded key bindings in `app.rs:handle_key()`** using `KeyCode::Char('r')` etc. — violates CLAUDE.md rule "Never hardcode keybinding checks". Must be extracted to `DEFAULT_APP_KEYBINDINGS` defaults object.

**Phase positioning:** Blocked on `sera-sdk` which is blocked on gateway protos. Current implementation continues to serve as developer convenience tool against the TS API.

**Priority:** **P2** for keybinding fix (independent of phase); **P3** for functional gaps.

---

## 3. Missing crates (per SPEC-crate-decomposition §3)

Crates that the target layout requires but that do not exist in the current workspace:

| Crate | Layer | Reason / obligation |
|---|---|---|
| `sera-errors` | Foundation | Unified error types — currently errors are scattered across crate-local modules |
| `sera-queue` | Infrastructure | Must extract lane-queue from `sera-db` into thin `apalis` 0.7 adapter |
| `sera-cache` | Infrastructure | `moka` + `fred`/Redis caching layer |
| `sera-telemetry` | Infrastructure | Rename/rebuild from `sera-events`; OTel triad + OCSF + Emitter tree |
| `sera-secrets` | Infrastructure | Secret provider trait + env/file/Vault/AWS SM/Azure KV |
| `sera-session` | Core Domain | 6-state lifecycle, `ContentBlock` transcript, two-layer persistence |
| `sera-memory` | Core Domain | 4-tier ABC memory, experience pool |
| `sera-tools` | Core Domain | Tool registry + `SandboxProvider` trait (absorbs `sera-docker`) |
| `sera-models` | Core Domain | Model adapter trait + parser registry |
| `sera-skills` | Core Domain | `AGENTS.md`/`SKILL.md` cross-tool standards |
| **`sera-meta`** | Core Domain | **NEW** — self-evolution machinery, Phase 4 impl + Phase 0–3 design-forward types |
| `sera-mcp` | Interop | `rmcp` 1.3 MCP server + client bridge |
| `sera-a2a` | Interop | A2A from pinned proto + `acp-compat` feature |
| `sera-agui` | Interop | Hand-rolled 17-event serde union |
| `sera-sdk` | Clients | Client SDK library (prerequisite for `sera-cli`, `sera-tui`) |
| `sera-cli` | Clients | CLI client |
| `sera-plugin-sdk` | Plugin SDK | Separately publishable crates.io crate; hard import boundary |
| `sera-hook-sdk` | Hook SDK | Separately publishable crates.io crate |

---

## 4. Cross-cutting issues

### 4.1 Sequencing: `sera-domain`/`sera-types` is the first mover

Many deltas in other crates require changes to `sera-domain` types first:

- `TurnOutcome` replacing `TurnResult` in `sera-domain::runtime`
- `ContentBlock` replacing flat `ChatMessage`
- `HookPoint` gaining 4 variants
- `HookResult` gaining `updated_input`
- `HookContext` gaining `change_artifact` and `abort_signal`
- `SessionState` gaining 5 variants
- `ResourceKind` gaining 3 variants
- `ResourceMetadata` gaining 2 fields
- **All self-evolution primitives** (`ChangeArtifactId`, `BlastRadius`, `CapabilityToken`, etc.)
- `ClaimToken` / `ClaimError` (shared between `sera-workflow` and future circle coordination)

`sera-domain` is a leaf crate — changes there compile cheaply, but every downstream crate must then update. **Correct order:** `sera-domain` → `sera-hooks` (additive) → `sera-runtime` (trait contract change) → `sera-core`/`sera-gateway` (structural).

### 4.2 Renames before new types

Do the crate rename before the type additions to avoid double migration:

1. `sera-core` → `sera-gateway`
2. `sera-domain` → `sera-types`
3. `sera-events` → `sera-telemetry` (or split)

### 4.3 Rename vs extract for `sera-docker`

Do not rename `sera-docker` in isolation. Migrate its contents into `sera-tools/src/sandbox/docker.rs` as a `DockerSandboxProvider` impl. Then either delete the `sera-docker` directory or keep it as an internal (non-published) test helper.

### 4.4 Queue extraction

Extract `lane_queue.rs` from `sera-db` into a new `rust/crates/sera-queue/` crate. Add `apalis 0.7` as primary backend. Keep in-memory queue as a `LocalQueueBackend` impl of a `QueueBackend` trait.

### 4.5 Workspace-wide dependency additions

Add (workspace-level `[workspace.dependencies]`, with exact-equals pins where required):

```toml
figment = "0.10"
schemars = "0.8"
jsonschema = "0.38"
apalis = "0.7"
apalis-sql = "0.7"
casbin = "2.19"
argon2 = "0.5"
wasmtime = ">=43, <50"           # loose range per HANDOFF §6.5
wasmtime-wasi = ">=43, <50"
wasmtime-wasi-http = ">=43, <50"
regorus = "0.3"                  # in-process OPA
opentelemetry = "=0.27"          # EXACT — load-bearing triad
opentelemetry-otlp = "=0.27"     # EXACT
tracing-opentelemetry = "=0.28"  # EXACT
async-trait = "0.1"
```

`extism` **must not** be added (HANDOFF §4.8).

### 4.6 Design-forward obligations checklist (Phase 0–3)

Per SPEC-self-evolution §5, these MUST exist in Phase 0 code even though they are unused until Phase 4:

- [ ] `ChangeArtifactId` in `sera-domain`
- [ ] `BlastRadius` enum (22 variants) in `sera-domain`
- [ ] `CapabilityToken` with narrowing rule in `sera-domain` + `sera-auth`
- [ ] `ConstitutionalRule` in `sera-domain`
- [ ] `EvolutionTier` in `sera-domain`
- [ ] `AgentCapability` enum with `MetaChange`/`CodeChange`/`MetaApprover` in `sera-domain` + `sera-auth`
- [ ] `BuildIdentity` in `sera-domain`
- [ ] `ResourceMetadata.change_artifact: Option<ChangeArtifactId>` + `.shadow: bool`
- [ ] `PersonaSpec.mutable_persona` + `.mutable_token_budget`
- [ ] `SessionState::Shadow` variant
- [ ] `HookPoint::ConstitutionalGate` + fail-closed enforcement in executor
- [ ] `HookContext::change_artifact: Option<ChangeArtifactId>`
- [ ] `HookResult::updated_input`
- [ ] `TurnContext::change_artifact: Option<ChangeArtifactId>`
- [ ] `ConversationMessage.cause_by: Option<ActionId>` (required for `_observe` watch_signals filtering)
- [ ] `WorkflowTask.meta_scope: Option<BlastRadius>` + `.change_artifact_id`
- [ ] `ApprovalScope::ChangeArtifact` + `::MetaChange` + `MetaChangeContext` with approver pinning
- [ ] `Action::ProposeChange(BlastRadius)` + `::ApproveChange(ChangeArtifactId)` + `Resource::ChangeArtifact`
- [ ] `ShadowConfigStore` overlay type (even as empty stub)
- [ ] `ConfigVersionLog` append-only skeleton
- [ ] Separate audit write path (`AuditBackend` trait + `OnceCell` static binding)
- [ ] Kill-switch admin socket + CON-04 boot-time health check
- [ ] `GenerationMarker` on `EventContext`

### 4.7 Code-quality defects called out independently

- `sera-tui` hardcoded keybindings (CLAUDE.md rule violation, P2 fix)
- `sera-auth` plaintext API-key comparison (must be replaced before declaring `basic-auth` feature stable)
- `sera-events` `jsonwebtoken` dependency creates unauthorized dep path (JWT belongs in `sera-auth`)
- `sera-db` in-memory `LaneQueue` loses state on restart (orphan recovery required per spec)

---

## 5. Prioritized action list

### P0 — Phase 0 blockers (must land before any new implementation work)

1. **`sera-domain` → `sera-types` rename + self-evolution primitives** (§2.1). Everything else depends on this.
2. **`sera-events` → `sera-telemetry` rewrite** (§2.3): OCSF `AuditEntry`, `AuditBackend` trait with `OnceCell` binding, OTel triad pinned, `Emitter` tree, `LaneFailureClass`. Unblocks constitutional-gate wiring.
3. **`sera-config` extensions** (§2.2): `figment`/`schemars`/`jsonschema` deps, `SchemaRegistry`, `ShadowConfigStore` stub, `ConfigVersionLog` skeleton.
4. **`sera-db` / `sera-queue` split** (§2.4): extract `LaneQueue` to new `sera-queue` crate, wire `apalis 0.7`, add migration reversibility enum.
5. **`sera-core` → `sera-gateway` rename + SQ/EQ + `AppServerTransport`** (§2.5): the architectural spine.
6. **`sera-runtime` core contract migration** (§2.6): `TurnOutcome`, `ContextEngine` trait, `ContentBlock`, four-method lifecycle skeleton, binary `main.rs` → `AppServerTransport::Stdio`.
7. **`sera-auth` design-forward types + feature gates** (§2.10): `AgentCapability`, `CapabilityToken`, `Action::ProposeChange`/`ApproveChange`, `Resource::ChangeArtifact`, argon2, `[features]` block.
8. **`sera-docker` absorption into `sera-tools`** (§2.11): stand up `sera-tools` crate with `SandboxProvider` trait + three-layer policy types, migrate container code.
9. **`sera-workflow` rewrite** (§2.8): `WorkflowTask`, `bd ready` algorithm, atomic claim protocol, `meta_scope` routing.
10. **Scaffold missing crates**: `sera-errors`, `sera-queue`, `sera-telemetry`, `sera-secrets`, `sera-tools`, `sera-meta`, `sera-mcp`, `sera-a2a`, `sera-agui`, `sera-session`, `sera-memory`, `sera-models`, `sera-skills`, `sera-sdk`, `sera-cli`, `sera-plugin-sdk`, `sera-hook-sdk`.

### P1 — Phase 1 follow-ups

11. **`sera-hooks` extensions** (§2.7): `HookPoint::ConstitutionalGate` + fail-closed enforcement, `HookResult::updated_input` restructuring, `HookToolInput`/`HookToolKind` discrimination, two-tier bus separation, `wasmtime` 43 integration with `wasi:http` allow-list.
12. **`sera-hitl` expansions** (§2.9): `SecurityAnalyzer`, `AskForApproval`, `GranularApprovalConfig`, `CorrectedError`/`ToolResult::Rejected`, `RevisionRequested`, `MetaChangeContext` with approver pinning, guardrails.
13. **`sera-testing` scaffolding** (§2.13): `TestDatabase`, fixture builders, mock `SandboxProvider`/`AuthorizationProvider`/`AuditHandle`, snapshot helpers.
14. **13 published JSON schemas** in `docs/schemas/` + CI check (SPEC-config §4.1).
15. **ADR: `acp-a2a-migration.md`** (HANDOFF §5 item 2).
16. **Pin OpenShell + A2A proto commits** in `docs/plan/VENDORED-PROTOS.md` (HANDOFF §5 item 3).
17. **`docs/plan/PHASE-0-PLAN.md`** — concrete code-level breakdown with Cargo features and milestones (HANDOFF §5 item 5).

### P2 — Phase 2+ / non-blocking

18. **`sera-byoh-agent`** — defer rewrite until `sera-sdk` + `AppServerTransport::Stdio` land (§2.12).
19. **`sera-tui` keybinding extraction** (`DEFAULT_APP_KEYBINDINGS`) — independent code-quality fix (§2.14).
20. Update `SPEC-clients.md` / `SPEC-thin-clients.md` (HANDOFF §5 item 6).
21. Update `SPEC-migration.md` (HANDOFF §5 item 7).
22. `PRIME.md` template for `bd prime` integration (HANDOFF §5 item 8).
23. `docs/plan/specs/README.md` spec index refresh (HANDOFF §5 item 9).

### P3 — Deferred

24. `sera-tui` functional gaps (session view, HITL UI, streaming feed) — blocked on `sera-sdk`.
25. Operator offline key distribution strategy (HANDOFF §5 item 10).
26. Multi-node cluster coordination (HANDOFF §5 item 11).
27. `wasteland` federation protocol (HANDOFF §5 item 12).

---

## 6. Summary table

| Crate | Classification | Priority | Blocking concern |
|---|---|---|---|
| `sera-domain` → `sera-types` | needs-extension | **P0** | Self-evolution Phase 0 primitives, `TurnOutcome`, `ContentBlock`, `SessionState` |
| `sera-config` | needs-extension | **P0** | `SchemaRegistry`, `ShadowConfigStore`, `ConfigVersionLog`, `figment`/`schemars`/`jsonschema` |
| `sera-events` → `sera-telemetry` | needs-rewrite | **P0** | OCSF `AuditBackend`, OTel triad, separate audit write path |
| `sera-db` | needs-extension | **P0** | Queue extraction to `sera-queue`, migration reversibility, `apalis` 0.7 |
| `sera-core` → `sera-gateway` | needs-rewrite | **P0** | SQ/EQ envelope, `AppServerTransport`, harness dispatch |
| `sera-runtime` | needs-rewrite | **P0** | `TurnOutcome`, `ContextEngine`, `ContentBlock`, four-method lifecycle, `Stdio` transport |
| `sera-hooks` | needs-extension | P1 | `ConstitutionalGate`, two-tier bus, `wasmtime` 43, `updated_input` |
| `sera-workflow` | needs-rewrite | **P0** | `WorkflowTask` (beads), `bd ready`, atomic claim, `meta_scope` |
| `sera-hitl` | needs-extension | P1 | `SecurityAnalyzer`, `AskForApproval`, `GranularApprovalConfig`, `MetaChangeContext` |
| `sera-auth` | needs-extension | **P0** | `AgentCapability`, `CapabilityToken`, `casbin`, argon2, feature gates |
| `sera-docker` | needs-rewrite | **P0** | Absorb into `sera-tools` as `DockerSandboxProvider` |
| `sera-byoh-agent` | needs-extension | P2 | Defer until `sera-sdk` + `AppServerTransport::Stdio` land |
| `sera-testing` | missing (stub) | P1 | Scaffold in parallel with `sera-tools`/`sera-auth` |
| `sera-tui` | aligned/deferred | P2/P3 | Keybinding fix P2; functional gaps blocked on `sera-sdk` |

---

**End of audit.** The next step is `docs/plan/PHASE-0-PLAN.md` — a code-level breakdown with Cargo features, milestone schedule, and acceptance tests per phase, using this audit as the input delta.
